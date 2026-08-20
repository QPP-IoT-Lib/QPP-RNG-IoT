//! Panic/overflow fuzz harness: `proptest`-driven property tests over
//! arbitrary seeds, jitter-clock scripts, and output lengths.
//!
//! Every test here uses [`crate::mock_clock::MockClock`] rather than a
//! real timer, for two reasons: it makes each case deterministic and
//! shrinkable (a real timer's jitter isn't reproducible, so a proptest
//! failure wouldn't shrink to a stable minimal case), and it makes
//! every case fast regardless of buffer length (no real time is spent
//! waiting on anything -- the cost is purely the CPU work of the
//! Fisher-Yates convergence walk itself), which is what keeps this
//! feasible to run on every `cargo test` rather than needing an
//! `#[ignore]` opt-in the way `stats::sample`'s real-timer test does.
//!
//! `N` is a `QppRng` const generic parameter, so it can't be drawn from
//! a `proptest` strategy directly -- each `N` worth fuzzing gets its own
//! `proptest!` block below instead.

use proptest::prelude::*;
use qpp_rng_reference::prng::{NextX48, Xorshift128Plus};
use qpp_rng_reference::QppRng;
use rand_core::Rng;

use crate::mock_clock::MockClock;
use crate::strategies::{any_seed, buffer_len, jitter_deltas};

/// Output-length strategy, scaled down for larger `N`. A convergence
/// cycle's expected cost is `O(N!)` draws (the Fisher-Yates walk is a
/// random walk on `S_N` with per-draw success probability `1/N!`; see
/// `qpp-rng-reference`'s crate docs), so `buffer_len()`'s full
/// `0..=512` range is fine at `N=5` (`5! = 120`) but would make an
/// `N=8` case (`8! = 40320`) run up to ~336x more convergence draws per
/// byte -- multiplied across a `proptest` case count, that turns one
/// fuzz function into a multi-minute outlier. Capping the range instead
/// of lowering the case count keeps coverage of "many bytes" (at small
/// `N`) and "many cases" (at every `N`) both intact.
fn buffer_len_for_array_size(n: usize) -> std::ops::RangeInclusive<usize> {
    if n >= 7 { 0..=16 } else { 0..=512 }
}

macro_rules! fuzz_no_panic_for_array_size {
    ($test_name:ident, $prng:ty, $n:expr) => {
        proptest! {
            #![proptest_config(ProptestConfig { cases: 48, .. ProptestConfig::default() })]
            #[test]
            fn $test_name(seed in any_seed(), deltas in jitter_deltas(), len in buffer_len_for_array_size($n)) {
                let mut rng = QppRng::<$prng, MockClock, $n>::new(
                    <$prng>::default(),
                    MockClock::new(deltas),
                    seed,
                );
                let mut buf = vec![0u8; len];
                rng.fill_bytes(&mut buf);
                // Reaching here (no panic/overflow) is the property
                // under test; also confirm fill_bytes honored the
                // requested length rather than silently truncating.
                prop_assert_eq!(buf.len(), len);
            }
        }
    };
}

// N < 6 exercises the oversample=5 path (DEFAULT_OVERSAMPLE); N >= 6
// exercises the single-cycle-per-byte path. N=1 is the degenerate case
// (a single-element array is always already the identity permutation --
// every convergence cycle should still terminate immediately without
// dividing by zero or looping forever).
fuzz_no_panic_for_array_size!(xorshift128plus_n1_never_panics, Xorshift128Plus, 1);
fuzz_no_panic_for_array_size!(xorshift128plus_n2_never_panics, Xorshift128Plus, 2);
fuzz_no_panic_for_array_size!(xorshift128plus_n5_never_panics, Xorshift128Plus, 5);
fuzz_no_panic_for_array_size!(xorshift128plus_n6_never_panics, Xorshift128Plus, 6);
fuzz_no_panic_for_array_size!(xorshift128plus_n8_never_panics, Xorshift128Plus, 8);

fuzz_no_panic_for_array_size!(nextx48_n1_never_panics, NextX48, 1);
fuzz_no_panic_for_array_size!(nextx48_n5_never_panics, NextX48, 5);
fuzz_no_panic_for_array_size!(nextx48_n6_never_panics, NextX48, 6);

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, .. ProptestConfig::default() })]

    /// `next_u32`/`next_u64` specifically (not just `fill_bytes`) --
    /// exercises `TryRng`'s other two required methods, which
    /// `qpp-rng-reference` implements independently on top of
    /// `try_fill_bytes` (see its `TryRng` impl).
    #[test]
    fn next_u32_and_next_u64_never_panic(seed in any_seed(), deltas in jitter_deltas()) {
        let mut rng = QppRng::<Xorshift128Plus, MockClock, 5>::new(
            Xorshift128Plus::default(),
            MockClock::new(deltas),
            seed,
        );
        let _ = rng.next_u32();
        let _ = rng.next_u64();
    }

    /// `oversample` is a caller-tunable knob (`with_oversample`); fuzz
    /// it too, since it changes how many convergence cycles get XORed
    /// together per byte.
    #[test]
    fn custom_oversample_never_panics(
        seed in any_seed(),
        deltas in jitter_deltas(),
        oversample in 1u8..=20,
        len in buffer_len(),
    ) {
        let mut rng = QppRng::<Xorshift128Plus, MockClock, 5>::new(
            Xorshift128Plus::default(),
            MockClock::new(deltas),
            seed,
        )
        .with_oversample(oversample);
        let mut buf = vec![0u8; len];
        rng.fill_bytes(&mut buf);
        prop_assert_eq!(buf.len(), len);
    }
}
