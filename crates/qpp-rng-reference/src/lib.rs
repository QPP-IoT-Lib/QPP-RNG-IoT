//! Faithful reference port of **QPP-RNG**, the permutation-based true
//! random number generator described in:
//!
//! > Vrana, G., Lou, D. & Kuang, R. *Raw QPP-RNG randomness via system
//! > jitter across platforms: a NIST SP 800-90B evaluation.*
//! > Sci Rep 15, 27718 (2025). <https://doi.org/10.1038/s41598-025-13135-8>
//!
//! ## The mechanism, in one pass
//!
//! 1. Reseed the internal PRNG ([`prng::InternalPrng`]) from the current
//!    128-bit seed.
//! 2. Starting from the identity permutation of an `N`-element array,
//!    repeatedly draw a fresh permutation pad (Fisher–Yates shuffle,
//!    "Fisher–Yates permutation pad generation") and compose it onto the
//!    running state, counting draws `n_p`, until the state returns to
//!    identity ("Raw QPP-RNG design"'s convergence criterion).
//! 3. Time the whole convergence loop with a [`HighResTimer`], normalize
//!    the elapsed ticks to a byte via [`entropy_timer::normalize_tick`]
//!    ("Timer resolution and delta normalization"), and fold that byte
//!    into the seed: `seed = (seed << 8) | (Δt mod 256)` ("Seed
//!    evolution and output generation").
//! 4. Take `n_p mod 256` as the raw output for this cycle ("Random
//!    number extraction").
//!
//! For small `N` (the paper's default is `N = 5`, where `log2(5!) ≈
//! 6.9 < 8` bits), steps 1-4 repeat `oversample` times per output byte
//! and the per-cycle bytes are combined, so each output byte still
//! carries a full 8 bits of accounted entropy ("Seed evolution and
//! output generation": *"Implementing m ≥ 4 iterations for N = 5
//! achieves 8-bit entropy per output byte"*).
//!
//! ## Fidelity notes
//!
//! The paper describes this mechanism narratively, with a handful of
//! formulas, but publishes neither pseudocode nor reference source.
//! Where it leaves an implementation choice open, this port makes the
//! following choices, documented here so they can be checked against
//! the paper independently:
//!
//! - **Convergence walk.** "the cumulative effect of repeated random
//!   permutations restores the array to its original ordered state" is
//!   implemented as a random walk on the symmetric group $S_N$: start
//!   at the identity permutation and repeatedly right-multiply by an
//!   independently drawn permutation pad ([`permutation::apply_permutation`])
//!   until the identity is reached again. Right-multiplying a uniform
//!   state by an independent uniform permutation always yields a
//!   uniform result, so this walk is memoryless with per-draw success
//!   probability `1/N!` -- consistent with the paper's `e = log2(N!)`
//!   entropy accounting ("Fisher–Yates permutation pad generation").
//! - **Oversample combination.** The paper prescribes `m >= 4` repeated
//!   convergence cycles per byte for `N < 6` but does not specify how
//!   the `m` per-cycle values are combined. This port XORs the `m`
//!   `n_p mod 256` samples together: XOR of independent samples is at
//!   least as unpredictable as any one operand, so it preserves the
//!   accounted entropy without introducing correlation between bytes.
//!   `N >= 6` uses a single cycle per byte, exactly as the paper
//!   specifies ("single iterations suffice for N >= 6, at the cost of
//!   slower performance").
//! - **Seed evolution.** `seed = (seed << 8) + (Δt mod 256)` is applied
//!   to the full 128-bit seed as one rotating shift register (rather
//!   than independently to each 64-bit half the paper's prose splits it
//!   into), reseeding the internal PRNG before *every* convergence
//!   cycle -- matching the "Independence" property in "Random number
//!   extraction": *"Each permutation-sort cycle is initialized with a
//!   refreshed 128-bit internal seed."*
//! - **Internal PRNGs.** [`prng::Xorshift128Plus`] follows Vigna's
//!   reference `xorshift128+` construction; [`prng::NextX48`] follows
//!   the 48-bit LCG that `java.util.Random` implements (multiplier
//!   `0x5DEECE66D`, increment `0xB`), per the paper's own citation of
//!   the JDK docs for NEXT_X48 (ref. 22). Neither claims bit-for-bit
//!   compatibility with the original authors' unpublished
//!   implementation -- only fidelity to the algorithm each is named
//!   after.
//!
//! None of this affects the workspace's *statistical* validation: the
//! whole point of `test-harness/stats` is to check this port's raw
//! output against NIST SP 800-90B/SP 800-22/ENT independently of these
//! interpretive choices, the same way the paper validates its own
//! implementation.

//
#![cfg_attr(
    not(any(target_os = "linux", target_os = "macos", target_os = "windows")),
    no_std
)]

pub mod permutation;
pub mod prng;

use core::convert::Infallible;

use entropy_timer::{HighResTimer, PlatformTimer};
use rand_core::TryRng;
use rng_core::{QppRngSource, RngDiagnostics};

use permutation::{apply_permutation, generate_permutation};
use prng::{InternalPrng, NextX48, Xorshift128Plus};

/// Default permutation-array width from the paper's reference
/// configuration ("Configuration parameters": *"Array size: 5
/// elements"*).
pub const DEFAULT_ARRAY_SIZE: usize = 5;

/// Default oversampling factor for `N < 6` arrays -- how many
/// independent convergence cycles are combined into one output byte
/// ("Configuration parameters": *"Oversampling: 5 iterations/byte
/// (sys_osr=5)"*).
pub const DEFAULT_OVERSAMPLE: u8 = 5;

/// The QPP-RNG engine: a permutation-array width `N`, an internal PRNG
/// `P` used only to draw permutation pads, and a jitter clock `T`
/// supplying the timing entropy that actually drives the generator.
///
/// `N` and the choice of `P`/`T` are fixed at compile time via generics
/// so embedding this in a `no_std`/no-heap target costs nothing beyond
/// the chosen configuration -- there's no dynamic dispatch or
/// allocation anywhere in the hot path.
pub struct QppRng<P, T, const N: usize> {
    prng: P,
    timer: T,
    /// Native timer resolution for the current platform, as returned by
    /// [`HighResTimer::init`] (see `entropy_timer::normalize_tick`).
    k: u8,
    /// The evolving 128-bit internal seed (see the "Seed evolution"
    /// fidelity note above).
    seed: u128,
    /// Number of convergence cycles combined into each output byte.
    oversample: u8,
    last_permutation_count: u64,
    /// Elapsed ticks of the most recent convergence cycle, in the
    /// timer's native units. **Not necessarily nanoseconds**: per
    /// [`HighResTimer`]'s own docs, native units vary from nanoseconds
    /// (most host OSes) to raw CPU/timer cycles (RDTSC/PMU/CCOUNT/
    /// Timer1 paths). Surfaced via [`RngDiagnostics::last_jitter_ns`]
    /// for lack of a more precise field in that shared type.
    last_jitter_ticks: u64,
}

impl<P, T, const N: usize> QppRng<P, T, N>
where
    P: InternalPrng,
    T: HighResTimer,
{
    /// Builds a generator from an already-constructed PRNG and timer,
    /// with the paper's default oversample policy: `5` cycles/byte for
    /// `N < 6`, `1` cycle/byte otherwise ("single iterations suffice
    /// for N >= 6").
    pub fn new(prng: P, mut timer: T, seed: u128) -> Self {
        let k = timer.init();
        let oversample = if N < 6 { DEFAULT_OVERSAMPLE } else { 1 };
        Self {
            prng,
            timer,
            k,
            seed,
            oversample,
            last_permutation_count: 0,
            last_jitter_ticks: 0,
        }
    }

    /// Overrides the default oversample count. Must be at least `1`;
    /// values below the paper's recommended `m >= 4` for `N < 6` will
    /// under-run the accounted 8-bit-per-byte entropy target.
    pub fn with_oversample(mut self, oversample: u8) -> Self {
        assert!(oversample >= 1, "oversample must be at least 1");
        self.oversample = oversample;
        self
    }

    /// Number of convergence cycles combined into each output byte.
    pub fn oversample(&self) -> u8 {
        self.oversample
    }

    /// Runs one permutation-sort convergence cycle: reseed, then walk
    /// the symmetric group $S_N$ by repeatedly composing freshly-drawn
    /// permutation pads until the identity is reached again.
    ///
    /// Returns `(n_p, Δt)`: the number of pads drawn to converge, and
    /// the elapsed ticks of the whole cycle.
    #[inline(never)]
    fn run_convergence_cycle(&mut self) -> (u64, u64) {
        self.prng.seed(self.seed);
        let identity: [u8; N] = core::array::from_fn(|i| i as u8);
        let mut current = identity;
        let mut n_p: u64 = 0;

        let t0 = self.timer.tick();
        loop {
            n_p += 1;
            let pad = generate_permutation::<P, N>(&mut self.prng);
            current = apply_permutation(&current, &pad);
            // Force the optimizer to treat every iteration's array as
            // genuinely observed, real memory traffic. Without this, a
            // release build (LTO, single codegen unit) can prove the
            // loop's memory effects are locally dead until the final
            // `== identity` check, collapse the whole convergence walk
            // into something with almost no execution-time variance,
            // and starve the very timing jitter this generator depends
            // on -- empirically, that turns into a fixed point in
            // `evolve_seed`'s shift register (same Δt every cycle -> same
            // seed suffix forever -> same output byte forever). The
            // paper sidesteps this by mandating `-O0` builds ("Test
            // platforms and compilation"); `black_box` gets the same
            // property without forcing every consumer of this crate to
            // disable optimizations project-wide.
            core::hint::black_box(&current);
            if current == identity {
                break;
            }
        }
        let t1 = self.timer.tick();

        (n_p, t1.wrapping_sub(t0))
    }

    /// Folds this cycle's elapsed ticks into the 128-bit seed as one
    /// rotating shift register (see the "Seed evolution" fidelity
    /// note).
    fn evolve_seed(&mut self, delta_ticks: u64) {
        let normalized = entropy_timer::normalize_tick(delta_ticks, self.k);
        self.seed = (self.seed << 8) | (normalized as u128);
    }

    /// Generates the next raw output byte.
    ///
    /// Runs [`oversample`](Self::oversample) convergence cycles,
    /// evolving the seed after each one, and XORs their `n_p mod 256`
    /// results together (see the "Oversample combination" fidelity
    /// note above).
    pub fn next_byte(&mut self) -> u8 {
        let mut out = 0u8;
        for _ in 0..self.oversample {
            let (n_p, delta_ticks) = self.run_convergence_cycle();
            self.last_permutation_count = n_p;
            self.last_jitter_ticks = delta_ticks;
            self.evolve_seed(delta_ticks);
            out ^= (n_p % 256) as u8;
        }
        out
    }
}

impl<P, T, const N: usize> QppRng<P, T, N>
where
    P: InternalPrng,
    T: HighResTimer + Default,
{
    /// Convenience constructor for timers that are cheap to
    /// default-construct (true of every [`entropy_timer`] backend).
    pub fn from_seed(seed: u128) -> Self {
        Self::new(P::default(), T::default(), seed)
    }
}

impl<P, T, const N: usize> TryRng for QppRng<P, T, N>
where
    P: InternalPrng,
    T: HighResTimer,
{
    type Error = Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Infallible> {
        let mut buf = [0u8; 4];
        self.try_fill_bytes(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    fn try_next_u64(&mut self) -> Result<u64, Infallible> {
        let mut buf = [0u8; 8];
        self.try_fill_bytes(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        for b in dst.iter_mut() {
            *b = self.next_byte();
        }
        Ok(())
    }
}

impl<P, T, const N: usize> QppRngSource for QppRng<P, T, N>
where
    P: InternalPrng,
    T: HighResTimer,
{
    fn diagnostics(&self) -> RngDiagnostics {
        RngDiagnostics {
            permutation_size_bits: permutation_entropy_bits(N),
            last_permutation_count: self.last_permutation_count,
            last_jitter_ns: Some(self.last_jitter_ticks),
        }
    }
}

/// Floor of `log2(N!)`, computed with integer bit-length rather than
/// floating point (keeps this `no_std`-friendly without pulling in
/// `libm`). Used only for [`RngDiagnostics::permutation_size_bits`].
fn permutation_entropy_bits(n: usize) -> u8 {
    let factorial: u64 = (1..=n as u64).product::<u64>().max(1);
    if factorial <= 1 {
        0
    } else {
        (63 - factorial.leading_zeros()) as u8
    }
}

/// Vrana et al. (2025) reference configuration: `N = 5` with XORSHIFT128+ as
/// the permutation-pad generator, on whatever [`PlatformTimer`] the
/// current build target resolves to.
pub type QppRngXorshift = QppRng<Xorshift128Plus, PlatformTimer, DEFAULT_ARRAY_SIZE>;

/// Vrana et al. (2025) alternate configuration: `N = 5` with NEXT_X48 as the
/// permutation-pad generator, on whatever [`PlatformTimer`] the current
/// build target resolves to.
pub type QppRngNextX48 = QppRng<NextX48, PlatformTimer, DEFAULT_ARRAY_SIZE>;

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::Rng;

    /// A scripted [`HighResTimer`] for deterministic tests: `init()`
    /// reports a fixed resolution and `tick()` walks a fixed sequence
    /// of deltas (as cumulative ticks), so tests can pin down exactly
    /// what a convergence cycle's elapsed-time reading was without
    /// depending on real, non-reproducible system jitter.
    struct MockTimer {
        deltas: std::vec::Vec<u64>,
        idx: usize,
        cumulative: u64,
    }

    impl MockTimer {
        fn new(deltas: std::vec::Vec<u64>) -> Self {
            Self {
                deltas,
                idx: 0,
                cumulative: 0,
            }
        }
    }

    impl HighResTimer for MockTimer {
        fn init(&mut self) -> u8 { 1 }

        fn tick(&mut self) -> u64 {
            // Every *pair* of ticks (start/stop of one convergence
            // cycle) should differ by `deltas[i]`; emit 0, deltas[0],
            // deltas[0], deltas[0]+deltas[1], ... by only advancing on
            // odd calls, so the loop's t1 - t0 gives us exactly
            // `deltas[i]` per cycle regardless of how many pad draws
            // happened between the two `tick()` calls.
            if self.idx % 2 == 1 {
                let d = self.deltas[self.idx / 2 % self.deltas.len()];
                self.cumulative = self.cumulative.wrapping_add(d);
            }
            let value = self.cumulative;
            self.idx += 1;
            value
        }
    }

    #[test]
    fn oversample_defaults_match_the_paper() {
        let rng = QppRng::<Xorshift128Plus, MockTimer, 5>::new(
            Xorshift128Plus::default(),
            MockTimer::new(std::vec![7]),
            1,
        );
        assert_eq!(rng.oversample(), DEFAULT_OVERSAMPLE);

        let rng6 = QppRng::<Xorshift128Plus, MockTimer, 6>::new(
            Xorshift128Plus::default(),
            MockTimer::new(std::vec![7]),
            1,
        );
        assert_eq!(rng6.oversample(), 1);
    }

    #[test]
    fn same_seed_and_same_jitter_script_reproduce_the_same_bytes() {
        let deltas = std::vec![101u64, 43, 999, 7, 256, 12];
        let mut a = QppRng::<Xorshift128Plus, MockTimer, 5>::new(
            Xorshift128Plus::default(),
            MockTimer::new(deltas.clone()),
            0x1122_3344_5566_7788_99AA_BBCC_DDEE_FF00,
        );
        let mut b = QppRng::<Xorshift128Plus, MockTimer, 5>::new(
            Xorshift128Plus::default(),
            MockTimer::new(deltas),
            0x1122_3344_5566_7788_99AA_BBCC_DDEE_FF00,
        );

        for _ in 0..16 {
            assert_eq!(a.next_byte(), b.next_byte());
        }
    }

    #[test]
    fn different_jitter_scripts_diverge() {
        let mut a = QppRng::<Xorshift128Plus, MockTimer, 5>::new(
            Xorshift128Plus::default(),
            MockTimer::new(std::vec![101, 43, 999, 7, 256]),
            42,
        );
        let mut b = QppRng::<Xorshift128Plus, MockTimer, 5>::new(
            Xorshift128Plus::default(),
            MockTimer::new(std::vec![5, 5, 5, 5, 5]),
            42,
        );

        let out_a: std::vec::Vec<u8> = (0..16).map(|_| a.next_byte()).collect();
        let out_b: std::vec::Vec<u8> = (0..16).map(|_| b.next_byte()).collect();
        assert_ne!(out_a, out_b);
    }

    #[test]
    fn diagnostics_report_the_last_cycle() {
        let mut rng = QppRng::<Xorshift128Plus, MockTimer, 5>::new(
            Xorshift128Plus::default(),
            MockTimer::new(std::vec![50]),
            1,
        );
        let _ = rng.next_byte();
        let diag = rng.diagnostics();
        assert!(diag.last_permutation_count > 0);
        assert!(diag.last_jitter_ns.is_some());
        assert_eq!(diag.permutation_size_bits, permutation_entropy_bits(5));
    }

    #[test]
    fn rand_core_rng_trait_is_usable_end_to_end() {
        // Exercises the blanket `Rng` impl over `TryRng<Error =
        // Infallible>` that `rng-core`'s `QppRngSource: Rng` bound
        // relies on.
        let mut rng = QppRng::<Xorshift128Plus, MockTimer, 6>::new(
            Xorshift128Plus::default(),
            MockTimer::new(std::vec![10, 20, 30]),
            7,
        );
        let _: u32 = rng.next_u32();
        let _: u64 = rng.next_u64();
        let mut buf = [0u8; 32];
        rng.fill_bytes(&mut buf);
        // Not a randomness assertion (that's test-harness/stats' job) --
        // just confirms `fill_bytes` actually wrote something.
        assert!(buf.iter().any(|&b| b != 0));
    }

    #[test]
    fn platform_timer_end_to_end_runs_without_panicking() {
        // Smoke test with the *real* platform timer: exercises the
        // full seed-evolution/convergence loop against genuine system
        // jitter, without asserting anything about output quality
        // (that's the NIST SP 800-90B/22 battery's job).
        let mut rng = QppRngXorshift::from_seed(0xC0FF_EE00_1234_5678_9ABC_DEF0_1122_3344);
        let mut buf = [0u8; 64];
        rand_core::Rng::fill_bytes(&mut rng, &mut buf);
        assert!(buf.iter().any(|&b| b != buf[0]));
    }

    /// Regression guard for a real failure mode found while validating
    /// this crate: because [`QppRng::evolve_seed`] feeds the seed's own
    /// timing-jitter byte straight back into itself, a build (or CPU/OS
    /// state) where that byte's variance collapses towards zero makes
    /// the 128-bit seed lock onto a fixed point -- every subsequent
    /// cycle reseeds to the *same* state, so `n_p` (and hence the output
    /// byte) freezes too. This reproduced reliably under this
    /// workspace's `lto = true, codegen-units = 1` release profile
    /// before the `qpp-rng-reference` package override (workspace
    /// `Cargo.toml`) and the `core::hint::black_box` barrier in
    /// `run_convergence_cycle` were added; one byte value covered
    /// >70% of a 5,000-byte sample.
    ///
    /// This isn't a substitute for the NIST SP 800-90B/22 battery in
    /// `test-harness/stats` -- it's a fast, coarse tripwire against
    /// reintroducing *that specific* collapse, using the real platform
    /// timer so it also catches a future build-profile change quietly
    /// undoing the mitigation.
    #[test]
    fn real_timer_output_does_not_collapse_onto_one_byte_value() {
        let mut rng = QppRngXorshift::from_seed(0x5EED_0000_1111_2222_3333_4444_5555_6666);
        const SAMPLES: u32 = 2000;
        let mut counts = [0u32; 256];
        for _ in 0..SAMPLES {
            counts[rng.next_byte() as usize] += 1;
        }
        let max = *counts.iter().max().unwrap();
        assert!(
            // Check if the byte value dominated the sample, 1/10 of the time.
            max < SAMPLES / 10,
            "byte value dominated the sample ({max}/{SAMPLES} occurrences) -- \
             the seed-evolution fixed point regressed"
        );
    }
}
