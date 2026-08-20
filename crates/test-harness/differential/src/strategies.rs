//! Shared `proptest` strategy definitions -- seed corpora and input
//! generators reused across [`crate::fuzz`] and any future property
//! test in this crate, so every check draws from the same input space
//! instead of each test file inventing its own.

use proptest::prelude::*;

/// Any 128-bit seed, uniform over the full range (including the
/// documented all-zero-half edge cases `Xorshift128Plus::seed` special-
/// cases -- see `qpp-rng-reference::prng`).
pub fn any_seed() -> impl Strategy<Value = u128> {
    any::<u128>()
}

/// A jitter-clock delta script: 1 to 24 deltas, each `0..=1_000_000`
/// (native timer units). Includes `0` deliberately -- a real timer
/// backend that reads back-to-back with no observable advance (a
/// coarse/frozen counter) is exactly the degenerate case
/// `entropy_timer::calibrate_resolution` exists to detect, and
/// `QppRng`'s convergence loop must not panic (divide-by-zero,
/// overflow, ...) when it happens.
pub fn jitter_deltas() -> impl Strategy<Value = Vec<u64>> {
    prop::collection::vec(0u64..=1_000_000, 1..=24)
}

/// An output buffer length worth fuzzing: `0..=512` bytes, covering the
/// zero-length edge case up to a few full oversample cycles' worth of
/// output.
pub fn buffer_len() -> impl Strategy<Value = usize> {
    0usize..=512
}

/// Native timer resolution `k`, as `entropy_timer::normalize_tick` and
/// `HighResTimer::init` produce: nonzero (`k = 0` would divide by zero
/// in `normalize_tick`, which is exactly why every real backend's
/// `init` and `calibrate_resolution`'s fallback path both guarantee
/// `k >= 1`).
pub fn timer_resolution() -> impl Strategy<Value = u8> {
    1u8..=u8::MAX
}
