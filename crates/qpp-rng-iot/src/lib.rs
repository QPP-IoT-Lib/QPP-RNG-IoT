//! QPP-RNG for bare-metal IoT microcontrollers.
//!
//! ## Why this exists: the reference design has no entropy on MCUs
//!
//! `qpp-rng-reference` harvests entropy from the *execution-time
//! jitter* of its own permutation-sort convergence loop, timed with the
//! CPU's cycle counter. On a desktop CPU that time varies for reasons
//! outside the program's control (caches, branch predictors, interrupts,
//! other cores, frequency scaling). On an AVR or a Cortex-M4 running
//! from flash with no OS, none of those exist: every instruction takes
//! a fixed number of cycles, and the timer counts the same clock that
//! drives the CPU. Hardware-in-the-loop runs on an Arduino Mega 2560,
//! an ATmega328P Nano and an nRF52840 showed the reference generator's
//! `(n_p, Δt)` trace -- and so its output -- is **bit-for-bit identical
//! across resets** on all three (`hil-results/REPORT.md`). It degrades
//! to a PRNG keyed by its compile-time seed.
//!
//! ## What changes here
//!
//! 1. **Entropy source.** Jitter comes from a [`JitterSource`]: a
//!    measurement of an oscillator that is *physically independent* of
//!    the CPU clock, taken in CPU-clock ticks -- e.g. the AVR watchdog's
//!    128 kHz RC oscillator timestamped by crystal-driven Timer1. The
//!    RC oscillator's phase noise is physical, so these samples differ
//!    from run to run even from an identical reset state. Where a chip
//!    has a hardware noise source, that works too: on the nRF52840 the
//!    on-chip RNG peripheral assessed far higher (~6.9 bits/byte) than its
//!    LFRC-vs-cycle-counter jitter (~0.6 bits/sample).
//! 2. **Continuous health tests** (SP 800-90B §4.4 RCT + APT, see
//!    [`health`]) on every raw sample, so a dead or stuck source is an
//!    error instead of a silent loss of entropy.
//! 3. **Cheaper pads** ([`pad`]): `xorshift32` plus an exact 8-bit
//!    Lemire reduction instead of `xorshift128+` with 64-bit
//!    reductions, and a 64-bit seed register instead of 128-bit -- the
//!    PRNG only expands the seed into pad draws, it is not where the
//!    entropy comes from, so narrowing it costs nothing in entropy and
//!    removes most of the u64/u128 arithmetic an 8-bit core has to
//!    emulate.
//! 4. **Entropy accounting decoupled from compute.** The reference ties
//!    "one timing sample" to "one convergence cycle", so a low-entropy
//!    source forces many expensive walks per byte. Here the number of
//!    raw samples absorbed per output byte is derived from the
//!    min-entropy credited to the source (at least 8 credited bits per
//!    byte), while the number of walks per byte is fixed at
//!    [`WALKS_PER_BYTE`] -- just enough to flatten the `n_p mod 256`
//!    extraction bias. Samples are spread evenly across the walks.
//! 5. **`N = 6` by default** ([`DEFAULT_ARRAY_SIZE`]) to remove the
//!    reference's structural `n_p mod 256` MSB bias, with the walk made
//!    cheap enough (fused pad generation/composition, see
//!    `pad::compose_fresh_pad`) that this doesn't cost throughput on
//!    the measured boards.
//!
//! The QPP structure itself is unchanged: per cycle, fold fresh jitter
//! into the seed, reseed the pad PRNG, walk S_N with Fisher–Yates pads
//! until the identity recurs, and XOR `n_p mod 256` across cycles.
//!
//! Output is *raw* entropy-source output: condition it (e.g.
//! `conditioning::Sha256Conditioner`) before using it as key material,
//! exactly as for the reference generator.

#![no_std]

pub mod health;
pub mod pad;

use core::convert::Infallible;

use rand_core::TryRng;
use rng_core::RngDiagnostics;

pub use health::{HealthConfig, HealthError};
use health::HealthTests;
use pad::{Xorshift32, compose_fresh_pad, fmix32, is_identity};

/// Default permutation width. One more than the reference's `N = 5`:
/// `n_p` is geometric with mean `N!`, and with `5! = 120 < 256` the
/// extracted `n_p mod 256` is heavily skewed (the reference's
/// documented MSB bias -- bit 7 is set only ~26% of the time per cycle,
/// and still ~52.8% of the time after XOR-ing 4 cycles, enough to fail a chi-square
/// test on 40k bytes). With `6! = 720` the per-cycle skew drops to
/// ~46% and to ~50.003% after 4 cycles. The walk costs 6x more pad draws,
/// but on the measured boards it still completes well inside one
/// jitter-sample period, so throughput is set by the source, not the walk.
pub const DEFAULT_ARRAY_SIZE: usize = 6;

/// A raw physical timing measurement.
///
/// Implementations must measure something clocked independently of the
/// CPU -- a separate RC oscillator, a ring oscillator, an external
/// event -- in units of a CPU-synchronous counter. Timing CPU-only work
/// with a CPU-clocked counter (what `qpp-rng-reference` does) is *not*
/// a valid implementation on bare-metal targets: it is deterministic.
pub trait JitterSource {
    /// Blocks until the next measurement is available and returns it.
    /// Entropy is expected in the low-order bits.
    fn sample(&mut self) -> u32;
}

impl<J: JitterSource + ?Sized> JitterSource for &mut J {
    fn sample(&mut self) -> u32 {
        (**self).sample()
    }
}

/// Convergence walks XOR-ed into each output byte. A single
/// `n_p mod 256` draw is geometrically skewed (bit 7 is set ~45.6% of
/// the time for `N = 6`); XOR-ing 4 independent draws brings that to
/// ~50.003%, and every other bit closer still.
pub const WALKS_PER_BYTE: u8 = 4;

/// Raw samples absorbed per walk so that each output byte is backed by
/// at least 8 bits of credited min-entropy: `ceil(ceil(8 / bits) / 4)`,
/// and at least one.
pub const fn samples_per_walk_for_min_entropy_bits(bits: u8) -> u8 {
    let bits = if bits == 0 { 1 } else { bits };
    let per_byte = 8u8.div_ceil(bits);
    let per_walk = per_byte.div_ceil(WALKS_PER_BYTE);
    if per_walk == 0 { 1 } else { per_walk }
}

pub struct QppRngIot<J, const N: usize = DEFAULT_ARRAY_SIZE> {
    source: J,
    seed: u64,
    samples_per_walk: u8,
    health: HealthTests,
    last_permutation_count: u64,
    last_sample: u32,
}

impl<J: JitterSource, const N: usize> QppRngIot<J, N> {
    /// Builds a generator crediting `min_entropy_bits` of min-entropy to
    /// each raw sample of `source`. That one number sets both the
    /// health-test cutoffs and the samples absorbed per byte, so it should
    /// come from an SP 800-90B assessment of the actual source on the
    /// actual board (see `hil-results/REPORT.md` for the boards
    /// measured so far), rounded down.
    pub fn new(source: J, min_entropy_bits: u8) -> Self {
        assert!(N >= 2 && N <= 8, "permutation width must be 2..=8");
        Self {
            source,
            seed: 0,
            samples_per_walk: samples_per_walk_for_min_entropy_bits(min_entropy_bits),
            health: HealthTests::new(HealthConfig::for_min_entropy_bits(min_entropy_bits)),
            last_permutation_count: 0,
            last_sample: 0,
        }
    }

    /// Raw jitter samples absorbed per output byte.
    pub fn samples_per_byte(&self) -> u8 {
        self.samples_per_walk * WALKS_PER_BYTE
    }

    /// Fills the seed register with fresh samples before the first
    /// output, so the first bytes don't depend on the all-zero initial
    /// seed. Called automatically by the first `next_byte`.
    fn prime(&mut self) -> Result<(), HealthError> {
        for _ in 0..8 {
            self.absorb()?;
        }
        Ok(())
    }

    fn absorb(&mut self) -> Result<(), HealthError> {
        let s = self.source.sample();
        self.health.feed(s as u8)?;
        self.last_sample = s;
        self.seed = (self.seed << 8) ^ s as u64;
        Ok(())
    }

    /// One convergence cycle on the current seed; returns `n_p`.
    fn convergence_cycle(&mut self) -> u64 {
        let mut prng = Xorshift32::new(fmix32((self.seed ^ (self.seed >> 32)) as u32));
        let mut current: [u8; N] = core::array::from_fn(|i| i as u8);
        // u32, not u64: an 8-bit core pays per byte of counter width, and
        // a walk never comes close to 2^32 draws (mean N! <= 40320).
        let mut n_p: u32 = 0;
        loop {
            n_p += 1;
            compose_fresh_pad(&mut current, &mut prng);
            if is_identity(&current) {
                return n_p as u64;
            }
        }
    }

    pub fn next_byte(&mut self) -> Result<u8, HealthError> {
        if self.last_permutation_count == 0 {
            self.prime()?;
        }
        let mut out = 0u8;
        for _ in 0..WALKS_PER_BYTE {
            for _ in 0..self.samples_per_walk {
                self.absorb()?;
            }
            let n_p = self.convergence_cycle();
            self.last_permutation_count = n_p;
            out ^= n_p as u8;
        }
        Ok(out)
    }

    pub fn diagnostics(&self) -> RngDiagnostics {
        let factorial: u64 = (1..=N as u64).product();
        RngDiagnostics {
            permutation_size_bits: (63 - factorial.leading_zeros()) as u8,
            last_permutation_count: self.last_permutation_count,
            // Raw source sample, in the source's own CPU-clock ticks.
            last_jitter_ns: Some(self.last_sample as u64),
        }
    }

    pub fn into_source(self) -> J {
        self.source
    }
}

impl<J: JitterSource, const N: usize> TryRng for QppRngIot<J, N> {
    type Error = HealthError;

    fn try_next_u32(&mut self) -> Result<u32, HealthError> {
        let mut b = [0u8; 4];
        self.try_fill_bytes(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }

    fn try_next_u64(&mut self) -> Result<u64, HealthError> {
        let mut b = [0u8; 8];
        self.try_fill_bytes(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), HealthError> {
        for b in dst {
            *b = self.next_byte()?;
        }
        Ok(())
    }
}

/// A source that never fails, for wiring this generator into APIs that
/// want an infallible `Rng`: panics on a health-test failure instead of
/// returning it. Prefer handling [`HealthError`] directly on devices.
pub struct PanicOnHealthFailure<J, const N: usize>(pub QppRngIot<J, N>);

impl<J: JitterSource, const N: usize> TryRng for PanicOnHealthFailure<J, N> {
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        Ok(self.0.try_next_u32().unwrap_or_else(|e| panic!("{e}")))
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        Ok(self.0.try_next_u64().unwrap_or_else(|e| panic!("{e}")))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        self.0.try_fill_bytes(dst).unwrap_or_else(|e| panic!("{e}"));
        Ok(())
    }
}

impl<J: JitterSource, const N: usize> rng_core::QppRngSource for PanicOnHealthFailure<J, N> {
    fn diagnostics(&self) -> RngDiagnostics {
        self.0.diagnostics()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scripted source: replays a fixed sequence.
    struct Script<'a> {
        values: &'a [u32],
        i: usize,
    }

    impl JitterSource for Script<'_> {
        fn sample(&mut self) -> u32 {
            let v = self.values[self.i % self.values.len()];
            self.i += 1;
            v
        }
    }

    /// Stand-in for a healthy physical source: a Gaussian-ish jitter
    /// (sum of uniforms, sd ~ 18 ticks) around a fixed period, like the
    /// AVR watchdog-vs-Timer1 measurements.
    struct NoisyPeriod {
        x: u64,
    }

    impl JitterSource for NoisyPeriod {
        fn sample(&mut self) -> u32 {
            let mut acc = 0u32;
            for _ in 0..4 {
                // splitmix64
                self.x = self.x.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = self.x;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                z ^= z >> 31;
                acc += (z % 64) as u32;
            }
            271_400 + acc
        }
    }

    #[test]
    fn samples_per_byte_back_8_credited_bits() {
        for bits in 0..=8u8 {
            let per_byte = samples_per_walk_for_min_entropy_bits(bits) * WALKS_PER_BYTE;
            assert!(per_byte as u32 * bits.max(1) as u32 >= 8, "bits={bits}");
            assert!(per_byte >= WALKS_PER_BYTE);
        }
        assert_eq!(samples_per_walk_for_min_entropy_bits(1), 2);
        assert_eq!(samples_per_walk_for_min_entropy_bits(2), 1);
    }

    #[test]
    fn same_samples_same_output() {
        let v = [271_434u32, 271_441, 271_419, 271_441, 271_435, 271_457, 271_430, 271_447];
        let mut a = QppRngIot::<_>::new(Script { values: &v, i: 0 }, 2);
        let mut b = QppRngIot::<_>::new(Script { values: &v, i: 0 }, 2);
        for _ in 0..32 {
            assert_eq!(a.next_byte(), b.next_byte());
        }
    }

    #[test]
    fn one_tick_of_jitter_changes_the_output() {
        let v1 = [271_434u32, 271_441, 271_419, 271_441, 271_435, 271_457, 271_430, 271_447];
        let mut v2 = v1;
        v2[3] += 1;
        let mut a = QppRngIot::<_>::new(Script { values: &v1, i: 0 }, 2);
        let mut b = QppRngIot::<_>::new(Script { values: &v2, i: 0 }, 2);
        let oa: [u8; 16] = core::array::from_fn(|_| a.next_byte().unwrap());
        let ob: [u8; 16] = core::array::from_fn(|_| b.next_byte().unwrap());
        assert_ne!(oa, ob);
    }

    #[test]
    fn stuck_source_is_reported_not_hidden() {
        let v = [271_434u32];
        let mut rng = QppRngIot::<_>::new(Script { values: &v, i: 0 }, 2);
        let mut buf = [0u8; 64];
        assert_eq!(rng.try_fill_bytes(&mut buf), Err(HealthError::RepetitionCount));
    }

    #[test]
    fn healthy_source_output_is_roughly_uniform() {
        let mut rng = QppRngIot::<_>::new(NoisyPeriod { x: 1 }, 2);
        let n = 40_000u32;
        let mut counts = [0u32; 256];
        let mut ones = 0u32;
        for _ in 0..n {
            let b = rng.next_byte().unwrap();
            counts[b as usize] += 1;
            ones += b.count_ones();
        }
        let expected = n as f64 / 256.0;
        let chi2: f64 = counts
            .iter()
            .map(|&c| (c as f64 - expected) * (c as f64 - expected) / expected)
            .sum();
        // 255 dof: mean 255, sd ~22.6; 360 is ~4.6 sd out.
        assert!(chi2 < 360.0, "chi2 = {chi2}");
        let frac = ones as f64 / (8.0 * n as f64);
        assert!((frac - 0.5).abs() < 0.005, "ones fraction {frac}");
    }

    #[test]
    fn panic_wrapper_is_a_qpp_rng_source() {
        use rand_core::Rng;
        use rng_core::QppRngSource;
        let mut rng = PanicOnHealthFailure(QppRngIot::<_>::new(NoisyPeriod { x: 9 }, 2));
        let _ = rng.next_u64();
        assert!(rng.diagnostics().last_permutation_count > 0);
    }
}
