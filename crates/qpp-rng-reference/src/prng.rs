//! Internal PRNGs used to draw permutation pads.
//!
//! These are *not* the entropy source. They exist purely to turn the
//! evolving 128-bit seed (see [`crate::QppRng`]) into the stream of
//! pseudo-random draws that the Fisher–Yates shuffle consumes to build
//! each permutation pad -- exactly the role XORSHIFT128+ and NEXT_X48
//! play in the paper ("Fisher–Yates permutation pad generation": *"The
//! Fisher–Yates shuffle generates permutation pads $P_i$ from 128-bit
//! seeds $s_i$ using XORSHIFT128+"*). All unpredictability in the final
//! output comes from the timing jitter folded into the seed between
//! cycles (see [`crate::QppRng::next_byte`]), not from these generators'
//! own statistical properties.

/// A reseedable pseudo-random word source consuming a 128-bit seed.
///
/// Implementors back the permutation-pad draws inside a convergence
/// cycle. [`InternalPrng::next_u32`] has a default implementation built
/// on [`InternalPrng::next_u64`]; override it only if a generator can
/// produce 32-bit words more directly.
pub trait InternalPrng: Default {
    /// (Re)initializes the generator's internal state from the current
    /// 128-bit seed. Called once at the start of every convergence
    /// cycle (see "Independence" in the paper's "Random number
    /// extraction" section).
    fn seed(&mut self, seed: u128);

    /// Returns the next pseudo-random 64-bit word from the stream.
    fn next_u64(&mut self) -> u64;

    /// Returns the next pseudo-random 32-bit word from the stream.
    ///
    /// Takes the upper 32 bits of [`next_u64`](Self::next_u64) by
    /// default, since low-order bits are the weakest bits for
    /// LCG-family generators (this matters concretely for
    /// [`NextX48`], whose `next_u64` is built from two LCG draws).
    fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }
}

/// Vigna's `xorshift128+` (2016), cited by the paper as the default
/// permutation-pad generator (ref. 21).
///
/// Reference construction (`xorshift128plus.c`): a 2-word xorshift
/// generator with a final addition step, chosen for speed and a
/// 2^128 - 1 period.
#[derive(Debug, Clone, Default)]
pub struct Xorshift128Plus {
    state: [u64; 2],
}

impl InternalPrng for Xorshift128Plus {
    fn seed(&mut self, seed: u128) {
        let mut s0 = (seed >> 64) as u64;
        let mut s1 = seed as u64;
        if s0 == 0 && s1 == 0 {
            // xorshift128+ is undefined for the all-zero state (it's a
            // fixed point -- every subsequent draw would also be zero).
            // Fall back to fixed, well-mixed nonzero constants rather
            // than silently producing a degenerate stream.
            s0 = 0x9E3779B97F4A7C15; // golden-ratio splitmix64 constant
            s1 = 0xBF58476D1CE4E5B9;
        }
        self.state = [s0, s1];
    }

    fn next_u64(&mut self) -> u64 {
        let mut s1 = self.state[0];
        let s0 = self.state[1];
        let result = s0.wrapping_add(s1);
        self.state[0] = s0;
        s1 ^= s1 << 23;
        self.state[1] = s1 ^ s0 ^ (s1 >> 18) ^ (s0 >> 5);
        result
    }
}

/// `NEXT_X48`: the 48-bit linear congruential generator described by
/// `java.util.Random` (multiplier `0x5DEECE66D`, increment `0xB`, 48-bit
/// state), which the paper cites (ref. 22) as its lightweight
/// alternative to XORSHIFT128+.
///
/// Only consumes the low 64 bits of the shared 128-bit seed -- its
/// internal state is inherently narrower than XORSHIFT128+'s.
#[derive(Debug, Clone, Default)]
pub struct NextX48 {
    state: u64, // low 48 bits significant
}

const LCG_MULTIPLIER: u64 = 0x5DEECE66D;
const LCG_INCREMENT: u64 = 0xB;
const LCG_MASK48: u64 = (1u64 << 48) - 1;

impl NextX48 {
    /// Mirrors `java.util.Random.next(bits)`: advance the LCG once and
    /// return its top `bits` bits.
    fn next_bits(&mut self, bits: u32) -> u32 {
        self.state = (self
            .state
            .wrapping_mul(LCG_MULTIPLIER)
            .wrapping_add(LCG_INCREMENT))
            & LCG_MASK48;
        (self.state >> (48 - bits)) as u32
    }
}

impl InternalPrng for NextX48 {
    fn seed(&mut self, seed: u128) {
        // java.util.Random's constructor scrambles the input seed with
        // the multiplier before the first advance; replicate that here.
        let raw = seed as u64; // low 64 bits of the shared 128-bit seed
        self.state = (raw ^ LCG_MULTIPLIER) & LCG_MASK48;
    }

    fn next_u64(&mut self) -> u64 {
        // Mirrors java.util.Random.nextLong(): two 32-bit draws packed
        // into one 64-bit word.
        let hi = self.next_bits(32) as u64;
        let lo = self.next_bits(32) as u64;
        (hi << 32) | lo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xorshift128plus_all_zero_seed_does_not_stick_at_zero() {
        let mut prng = Xorshift128Plus::default();
        prng.seed(0);
        assert_ne!(prng.next_u64(), 0);
    }

    #[test]
    fn xorshift128plus_is_deterministic_per_seed() {
        let mut a = Xorshift128Plus::default();
        let mut b = Xorshift128Plus::default();
        a.seed(0x1234_5678_9abc_def0_1122_3344_5566_7788);
        b.seed(0x1234_5678_9abc_def0_1122_3344_5566_7788);
        for _ in 0..64 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn xorshift128plus_different_seeds_diverge() {
        let mut a = Xorshift128Plus::default();
        let mut b = Xorshift128Plus::default();
        a.seed(1);
        b.seed(2);
        let seq_a: [u64; 8] = core::array::from_fn(|_| a.next_u64());
        let seq_b: [u64; 8] = core::array::from_fn(|_| b.next_u64());
        assert_ne!(seq_a, seq_b);
    }

    #[test]
    fn next_x48_is_deterministic_per_seed() {
        let mut a = NextX48::default();
        let mut b = NextX48::default();
        a.seed(42);
        b.seed(42);
        for _ in 0..64 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn next_x48_different_seeds_diverge() {
        let mut a = NextX48::default();
        let mut b = NextX48::default();
        a.seed(1);
        b.seed(2);
        let seq_a: [u64; 8] = core::array::from_fn(|_| a.next_u64());
        let seq_b: [u64; 8] = core::array::from_fn(|_| b.next_u64());
        assert_ne!(seq_a, seq_b);
    }
}
