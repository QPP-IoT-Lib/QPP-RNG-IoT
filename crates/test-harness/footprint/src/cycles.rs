//! Cycle-count-as-power-proxy interface, built on `entropy-timer`'s
//! platform timer -- the same abstraction that backs
//! `qpp-rng-reference`'s own jitter harvesting, so this measurement
//! automatically follows onto whatever backend (host `Instant`, x86
//! RDTSC, Cortex-M DWT, RISC-V `mcycle`, ...) each target actually
//! compiles in without this crate needing target-specific code.
//!
//! ## Naming note: `HighResTimer`, not `JitterClock`
//!
//! `rng-core::JitterClock` is declared but has no implementations
//! anywhere in this workspace; `entropy_timer::HighResTimer` (via
//! `entropy_timer::PlatformTimer`) is the trait every real timer
//! backend actually implements, and the one `qpp-rng-reference` itself
//! is built on. This module is written against `HighResTimer` for that
//! reason -- see this crate's root doc for the same note applied to the
//! rest of `footprint`.
//!
//! ## Ticks, not necessarily cycles
//!
//! Per [`entropy_timer::HighResTimer`]'s own docs, its native unit
//! varies by backend: nanoseconds on most host OSes, raw hardware
//! cycles on the RDTSC/DWT/`mcycle`/`CCOUNT` backends. On host, this
//! module's numbers are honestly "nanoseconds per output byte", not
//! cycles; the *cycles*-per-byte power proxy the testing architecture
//! doc asks for becomes accurate once this runs against an on-target
//! backend, with zero code changes here -- that's the point of building
//! on `HighResTimer` rather than a host-only timer.
//!
//! This also measures the whole `fill_bytes` call from the outside, not
//! `qpp-rng-reference`'s own internal per-convergence-cycle jitter
//! timer (which is private generator state, not something this crate
//! can reach through the boxed `dyn QppRngSource` [`candidates`]
//! hands out) -- see `candidates`' module doc for why that erasure is
//! deliberate. An outside-in measurement is also the more broadly
//! useful one here: it works uniformly for every candidate, including a
//! hypothetical future `qpp-rng-iot` variant whose entropy source isn't
//! timing-based at all.

use entropy_timer::HighResTimer;
use rng_core::QppRngSource;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CycleCountReport {
    pub n_bytes: usize,
    pub total_ticks: u64,
    pub ticks_per_output_byte: f64,
}

/// Times generating `n_bytes` from `rng` with `timer`, and returns the
/// total elapsed ticks plus the per-byte average.
///
/// `timer` is taken already-initialized (its [`HighResTimer::init`]
/// already called) so callers control calibration/warm-up themselves --
/// see `entropy_timer::calibrate_resolution` for the adaptive-resolution
/// story this mirrors on the generator side.
pub fn measure_ticks_per_byte<R, T>(rng: &mut R, timer: &mut T, n_bytes: usize) -> CycleCountReport
where
    R: QppRngSource + ?Sized,
    T: HighResTimer,
{
    let mut buf = vec![0u8; n_bytes];
    let t0 = timer.tick();
    rng.fill_bytes(&mut buf);
    let t1 = timer.tick();
    let total_ticks = t1.wrapping_sub(t0);

    CycleCountReport {
        n_bytes,
        total_ticks,
        ticks_per_output_byte: total_ticks as f64 / n_bytes as f64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scripted timer: each `tick()` call returns the next value from
    /// a fixed sequence, wrapping if exhausted. Mirrors the `MockTimer`
    /// pattern `qpp-rng-reference`'s own tests use.
    struct ScriptedTimer {
        values: Vec<u64>,
        idx: usize,
    }
    impl HighResTimer for ScriptedTimer {
        fn init(&mut self) -> u8 {
            1
        }
        fn tick(&mut self) -> u64 {
            let v = self.values[self.idx % self.values.len()];
            self.idx += 1;
            v
        }
    }

    struct CountingRng {
        counter: u8,
    }
    impl rand_core::TryRng for CountingRng {
        type Error = core::convert::Infallible;
        fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
            let mut b = [0u8; 4];
            self.try_fill_bytes(&mut b)?;
            Ok(u32::from_le_bytes(b))
        }
        fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
            let mut b = [0u8; 8];
            self.try_fill_bytes(&mut b)?;
            Ok(u64::from_le_bytes(b))
        }
        fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
            for b in dst.iter_mut() {
                *b = self.counter;
                self.counter = self.counter.wrapping_add(1);
            }
            Ok(())
        }
    }
    impl QppRngSource for CountingRng {
        fn diagnostics(&self) -> rng_core::RngDiagnostics {
            rng_core::RngDiagnostics {
                permutation_size_bits: 0,
                last_permutation_count: 0,
                last_jitter_ns: None,
            }
        }
    }

    #[test]
    fn measures_elapsed_ticks_and_divides_by_byte_count() {
        let mut rng = CountingRng { counter: 0 };
        let mut timer = ScriptedTimer {
            values: vec![1000, 1500],
            idx: 0,
        };
        let report = measure_ticks_per_byte(&mut rng, &mut timer, 100);
        assert_eq!(report.n_bytes, 100);
        assert_eq!(report.total_ticks, 500); // 1500 - 1000
        assert!((report.ticks_per_output_byte - 5.0).abs() < 1e-9);
    }

    #[test]
    fn handles_wraparound_in_the_underlying_counter() {
        let mut rng = CountingRng { counter: 0 };
        let mut timer = ScriptedTimer {
            values: vec![u64::MAX - 10, 40],
            idx: 0,
        };
        let report = measure_ticks_per_byte(&mut rng, &mut timer, 10);
        // wrapping_sub across the u64 wraparound: (40) - (MAX-10) mod 2^64 = 51
        assert_eq!(report.total_ticks, 51);
    }
}
