//! Fisher–Yates permutation pad generation and application.
//!
//! Implements the paper's "Fisher–Yates permutation pad generation":
//! each pad $P_i$ is a uniformly random permutation of `0..N`, drawn
//! from an [`InternalPrng`] stream via a Durstenfeld (in-place)
//! Fisher–Yates shuffle with unbiased bounded-range reduction (Lemire's
//! method, cited by the paper as ref. 17).

use crate::prng::InternalPrng;

/// Draws a uniform random value in `0..bound` from `prng`, using
/// Lemire's rejection method so the result is exactly unbiased
/// regardless of `bound` (no modulo bias, unlike a plain `% bound`).
///
/// `bound` must be nonzero; only ever called here with `bound = i + 1`
/// for `i >= 1`, i.e. `bound >= 2`.
fn bounded<P: InternalPrng>(prng: &mut P, bound: u32) -> u32 {
    debug_assert!(bound > 0);
    let mut x = prng.next_u32();
    let mut m = (x as u64) * (bound as u64);
    let mut l = m as u32;
    if l < bound {
        // Reject draws that would land in the range that can't be
        // evenly divided among `bound` buckets.
        let threshold = bound.wrapping_neg() % bound;
        while l < threshold {
            x = prng.next_u32();
            m = (x as u64) * (bound as u64);
            l = m as u32;
        }
    }
    (m >> 32) as u32
}

/// Generates one permutation pad of width `N` -- a uniformly random
/// bijection on `0..N` -- via a Durstenfeld Fisher–Yates shuffle of the
/// identity array.
pub(crate) fn generate_permutation<P: InternalPrng, const N: usize>(prng: &mut P) -> [u8; N] {
    let mut perm: [u8; N] = core::array::from_fn(|i| i as u8);
    let mut i = N;
    while i > 1 {
        i -= 1;
        let j = bounded(prng, (i + 1) as u32) as usize;
        perm.swap(i, j);
    }
    perm
}

/// Applies permutation `perm` to `base` by position-gather:
/// `result[i] = base[perm[i]]`.
///
/// Repeated application of independently-drawn permutations this way is
/// a right-multiplication random walk on the symmetric group $S_N$ --
/// see the "Convergence walk" fidelity note in the crate root docs.
pub(crate) fn apply_permutation<const N: usize>(base: &[u8; N], perm: &[u8; N]) -> [u8; N] {
    core::array::from_fn(|i| base[perm[i] as usize])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prng::Xorshift128Plus;

    /// A trivial counting stream: exercises `generate_permutation`
    /// against a non-random-looking but fully deterministic sequence,
    /// to check the shuffle logic itself (not the PRNG's quality).
    #[derive(Default)]
    struct CountingPrng {
        next: u64,
    }

    impl InternalPrng for CountingPrng {
        fn seed(&mut self, seed: u128) {
            self.next = seed as u64;
        }
        fn next_u64(&mut self) -> u64 {
            self.next = self.next.wrapping_add(0x9E3779B97F4A7C15);
            self.next
        }
    }

    #[test]
    fn generate_permutation_is_always_a_bijection() {
        let mut prng = CountingPrng::default();
        prng.seed(1);
        for _ in 0..1000 {
            let perm: [u8; 8] = generate_permutation(&mut prng);
            let mut seen = [false; 8];
            for &v in &perm {
                assert!(!seen[v as usize], "duplicate value {v} in {perm:?}");
                seen[v as usize] = true;
            }
        }
    }

    #[test]
    fn generate_permutation_is_a_bijection_with_a_real_prng() {
        let mut prng = Xorshift128Plus::default();
        prng.seed(0xDEAD_BEEF_CAFE_F00D_1234_5678_9ABC_DEF0);
        for _ in 0..1000 {
            let perm: [u8; 5] = generate_permutation(&mut prng);
            let mut seen = [false; 5];
            for &v in &perm {
                assert!(!seen[v as usize]);
                seen[v as usize] = true;
            }
        }
    }

    #[test]
    fn apply_permutation_identity_is_a_no_op() {
        let identity: [u8; 5] = core::array::from_fn(|i| i as u8);
        let result = apply_permutation(&identity, &identity);
        assert_eq!(result, identity);
    }

    #[test]
    fn bounded_draws_stay_within_range_and_cover_it() {
        let mut prng = Xorshift128Plus::default();
        prng.seed(7);
        const BOUND: u32 = 6;
        const SAMPLES: u32 = 60_000;
        let mut counts = [0u32; BOUND as usize];
        for _ in 0..SAMPLES {
            let v = bounded(&mut prng, BOUND);
            assert!(v < BOUND);
            counts[v as usize] += 1;
        }
        // Loose uniformity sanity check (not a substitute for the
        // NIST SP 800-90B/22 battery in test-harness/stats) -- just
        // guards against a badly biased range reduction.
        let expected = SAMPLES / BOUND;
        for &c in &counts {
            assert!(
                c > expected / 2 && c < expected * 3 / 2,
                "bucket count {c} too far from expected {expected}: {counts:?}"
            );
        }
    }
}
