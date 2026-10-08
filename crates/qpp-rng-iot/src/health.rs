//! NIST SP 800-90B §4.4 continuous health tests on the raw jitter
//! samples: the Repetition Count Test and the Adaptive Proportion Test.
//!
//! These are what make it safe to deploy a jitter source on hardware
//! nobody characterized in a lab: if the source degrades to (near-)
//! constant output -- a stuck clock, a board where the "independent"
//! oscillator turns out to share the CPU's clock tree -- the generator
//! reports an error instead of silently streaming low-entropy bytes.
//!
//! What they *cannot* catch is a source that varies but is fully
//! deterministic, which is exactly how `qpp-rng-reference`'s same-clock
//! convergence timing behaves on bare-metal MCUs (see
//! `hil-results/REPORT.md`). That failure has to be designed out by
//! choosing a physically independent jitter source, not tested for.

/// Cutoffs for the two tests, derived from the min-entropy per sample
/// `H` credited to the source, at the false-positive rate
/// `alpha = 2^-20` SP 800-90B recommends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthConfig {
    /// RCT cutoff `C = 1 + ceil(20 / H)`.
    pub rct_cutoff: u16,
    /// APT cutoff for a 512-sample window,
    /// `C = 1 + CRITBINOM(512, 2^-H, 1 - 2^-20)`.
    pub apt_cutoff: u16,
}

impl HealthConfig {
    /// Window size of the Adaptive Proportion Test (SP 800-90B's value
    /// for non-binary sources).
    pub const APT_WINDOW: u16 = 512;

    /// Cutoffs for a credited min-entropy of `bits` per sample, for the
    /// values SP 800-90B tabulates. `bits` is rounded down to the
    /// nearest supported value (0 is treated as 1/2 bit), which only
    /// ever makes the tests stricter.
    pub const fn for_min_entropy_bits(bits: u8) -> Self {
        // Exact binomial critical values, computed offline (see
        // hil-results/REPORT.md, "Health-test cutoffs").
        match bits {
            0 => Self { rct_cutoff: 41, apt_cutoff: 410 },
            1 => Self { rct_cutoff: 21, apt_cutoff: 311 },
            2 => Self { rct_cutoff: 11, apt_cutoff: 177 },
            3 => Self { rct_cutoff: 8, apt_cutoff: 103 },
            _ => Self { rct_cutoff: 6, apt_cutoff: 62 },
        }
    }
}

/// Which health test tripped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthError {
    RepetitionCount,
    AdaptiveProportion,
}

impl core::fmt::Display for HealthError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            HealthError::RepetitionCount => "jitter source failed the SP 800-90B repetition count test",
            HealthError::AdaptiveProportion => {
                "jitter source failed the SP 800-90B adaptive proportion test"
            }
        })
    }
}

impl core::error::Error for HealthError {}

#[derive(Debug, Clone)]
pub(crate) struct HealthTests {
    cfg: HealthConfig,
    rct_last: u8,
    rct_count: u16,
    apt_ref: u8,
    apt_seen: u16,
    apt_count: u16,
}

impl HealthTests {
    pub(crate) fn new(cfg: HealthConfig) -> Self {
        Self {
            cfg,
            rct_last: 0,
            rct_count: 0,
            apt_ref: 0,
            apt_seen: 0,
            apt_count: 0,
        }
    }

    /// Feeds one raw sample symbol through both tests.
    pub(crate) fn feed(&mut self, s: u8) -> Result<(), HealthError> {
        if self.rct_count > 0 && s == self.rct_last {
            self.rct_count += 1;
            if self.rct_count >= self.cfg.rct_cutoff {
                return Err(HealthError::RepetitionCount);
            }
        } else {
            self.rct_last = s;
            self.rct_count = 1;
        }

        if self.apt_seen == 0 {
            self.apt_ref = s;
            self.apt_count = 1;
        } else if s == self.apt_ref {
            self.apt_count += 1;
            if self.apt_count >= self.cfg.apt_cutoff {
                return Err(HealthError::AdaptiveProportion);
            }
        }
        self.apt_seen += 1;
        if self.apt_seen == HealthConfig::APT_WINDOW {
            self.apt_seen = 0;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stuck_source_trips_rct() {
        let mut h = HealthTests::new(HealthConfig::for_min_entropy_bits(2));
        let mut err = None;
        for _ in 0..20 {
            if let Err(e) = h.feed(42) {
                err = Some(e);
                break;
            }
        }
        assert_eq!(err, Some(HealthError::RepetitionCount));
    }

    #[test]
    fn heavily_biased_source_trips_apt() {
        // Alternates so the RCT never fires, but one value is ~50% of
        // every window -- far above the 177/512 cutoff for H = 2.
        let mut h = HealthTests::new(HealthConfig::for_min_entropy_bits(2));
        let mut err = None;
        for i in 0..2048u32 {
            let s = if i % 2 == 0 { 7 } else { (i % 251) as u8 };
            if let Err(e) = h.feed(s) {
                err = Some(e);
                break;
            }
        }
        assert_eq!(err, Some(HealthError::AdaptiveProportion));
    }

    #[test]
    fn uniform_source_passes() {
        let mut h = HealthTests::new(HealthConfig::for_min_entropy_bits(4));
        let mut x: u32 = 1;
        for _ in 0..100_000 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            h.feed(x as u8).unwrap();
        }
    }
}
