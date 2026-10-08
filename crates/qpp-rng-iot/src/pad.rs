//! Byte-oriented permutation-pad generation for 8/32-bit MCUs.
//!
//! `qpp-rng-reference` draws each Fisher–Yates index with a 64-bit
//! Lemire reduction over `xorshift128+` output -- 64-bit multiplies and
//! 128-bit state, which on an 8-bit AVR cost hundreds of instructions
//! per index. Here every index of an `N <= 8` shuffle is bounded by at
//! most 8, so a single random *byte* and one 8x8->16-bit multiply (one
//! `mul` instruction on AVR, a single-cycle `umull` on Cortex-M) is
//! enough for an exactly unbiased Lemire draw. One `xorshift32` step
//! yields four bytes -- for `N = 5`, exactly one pad's worth.

/// Marsaglia's `xorshift32` (13, 17, 5). 32-bit state keeps every
/// operation within a Cortex-M register and a handful of AVR registers.
/// Like the reference crate's internal PRNGs, it is *not* the entropy
/// source: it only expands the jitter-fed seed into pad draws.
#[derive(Debug, Clone)]
pub struct Xorshift32 {
    state: u32,
    buf: u32,
    left: u8,
}

impl Xorshift32 {
    pub fn new(seed: u32) -> Self {
        Self {
            state: if seed == 0 { 0x9E37_79B9 } else { seed },
            buf: 0,
            left: 0,
        }
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    pub fn next_u8(&mut self) -> u8 {
        if self.left == 0 {
            self.buf = self.next_u32();
            self.left = 4;
        }
        let b = self.buf as u8;
        self.buf >>= 8;
        self.left -= 1;
        b
    }
}

/// Lemire rejection thresholds `256 mod bound` for every bound a
/// width-8-or-less shuffle uses. Looked up rather than computed: AVR
/// has no divide instruction, and a software `u16 % u16` per draw was
/// ~85% of the convergence walk's run time on the ATmega2560.
const THRESHOLD: [u8; 9] = {
    let mut t = [0u8; 9];
    let mut b = 1;
    while b <= 8 {
        t[b] = (256 % b) as u8;
        b += 1;
    }
    t
};

/// Uniform draw in `0..bound` (`1 <= bound <= 8`) from random bytes,
/// via Lemire's multiply-and-reject on 8 bits: exactly unbiased.
#[inline]
pub(crate) fn bounded_u8(prng: &mut Xorshift32, bound: u8) -> u8 {
    let threshold = THRESHOLD[bound as usize];
    loop {
        let m = u16::from(prng.next_u8()) * u16::from(bound);
        if (m as u8) >= threshold {
            return (m >> 8) as u8;
        }
    }
}

/// One uniformly random permutation of `0..N` (Durstenfeld Fisher–Yates).
/// The walk itself uses [`compose_fresh_pad`]; this is the reference
/// form it is tested against.
#[cfg(test)]
pub(crate) fn generate_pad<const N: usize>(prng: &mut Xorshift32) -> [u8; N] {
    let mut perm: [u8; N] = core::array::from_fn(|i| i as u8);
    let mut i = N;
    while i > 1 {
        i -= 1;
        let j = bounded_u8(prng, (i + 1) as u8) as usize;
        perm.swap(i, j);
    }
    perm
}

/// `result[i] = base[perm[i]]`, same right-multiplication walk on S_N as
/// `qpp-rng-reference::permutation::apply_permutation`.
#[cfg(test)]
pub(crate) fn apply_pad<const N: usize>(base: &[u8; N], perm: &[u8; N]) -> [u8; N] {
    core::array::from_fn(|i| base[perm[i] as usize])
}

/// Draws a fresh Fisher–Yates pad and composes it onto `state` in one
/// pass: exactly `apply_pad(state, &generate_pad(prng))`, consuming the
/// same PRNG draws, but without materializing the pad.
///
/// Why that's equal: `generate_pad` builds the pad by performing the
/// position swaps `(i, j_i)` on the identity array, and performing the
/// same position swaps on any array `b` yields `b[pad[k]]` at every
/// position `k` -- swapping positions commutes with what values those
/// positions hold. Skipping the pad array and the gather halves the
/// memory traffic per draw, which on AVR is most of the walk's cost.
#[inline]
pub(crate) fn compose_fresh_pad<const N: usize>(state: &mut [u8; N], prng: &mut Xorshift32) {
    // u8 index, not usize: keeps the Lemire product an 8x8 multiply (a
    // single `mul` on AVR, where usize is 16 bits wide).
    let mut i = N as u8;
    while i > 1 {
        i -= 1;
        let j = bounded_u8(prng, i + 1);
        state.swap(i as usize, j as usize);
    }
}

/// `state == identity`, short-circuiting on the first mismatch (which,
/// for a uniformly random state, is position 0 with probability
/// `(N-1)/N`).
#[inline]
pub(crate) fn is_identity<const N: usize>(state: &[u8; N]) -> bool {
    let mut i = 0;
    while i < N {
        if state[i] != i as u8 {
            return false;
        }
        i += 1;
    }
    true
}

/// MurmurHash3's 32-bit finalizer. `xorshift32` is linear over GF(2),
/// so seeds that differ only in their newest jitter byte would start
/// out producing related pad streams; one non-linear avalanche step at
/// reseed time removes that.
#[inline]
pub(crate) fn fmix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_fresh_pad_equals_generate_then_apply() {
        let mut state_a: [u8; 6] = [3, 0, 5, 1, 4, 2];
        let mut state_b = state_a;
        let mut pa = Xorshift32::new(0xA5A5_1234);
        let mut pb = pa.clone();
        for _ in 0..10_000 {
            let pad: [u8; 6] = generate_pad(&mut pa);
            state_a = apply_pad(&state_a, &pad);
            compose_fresh_pad(&mut state_b, &mut pb);
            assert_eq!(state_a, state_b);
        }
        // Same number of PRNG draws consumed, too.
        assert_eq!(pa.next_u32(), pb.next_u32());
    }

    #[test]
    fn is_identity_matches_array_equality() {
        let id: [u8; 5] = [0, 1, 2, 3, 4];
        assert!(is_identity(&id));
        assert!(!is_identity(&[0u8, 1, 2, 4, 3]));
        assert!(!is_identity(&[1u8, 0, 2, 3, 4]));
    }

    #[test]
    fn xorshift32_never_sticks_at_zero() {
        let mut p = Xorshift32::new(0);
        assert_ne!(p.next_u32(), 0);
    }

    #[test]
    fn pads_are_bijections() {
        let mut p = Xorshift32::new(12345);
        for _ in 0..2000 {
            let pad: [u8; 8] = generate_pad(&mut p);
            let mut seen = [false; 8];
            for &v in &pad {
                assert!(!seen[v as usize]);
                seen[v as usize] = true;
            }
        }
    }

    #[test]
    fn bounded_u8_is_uniform_for_every_pad_bound() {
        let mut p = Xorshift32::new(7);
        for bound in 2..=8u8 {
            let mut counts = [0u32; 8];
            let samples = 80_000u32;
            for _ in 0..samples {
                let v = bounded_u8(&mut p, bound);
                assert!(v < bound);
                counts[v as usize] += 1;
            }
            let expected = samples as f64 / bound as f64;
            for &c in &counts[..bound as usize] {
                let dev = (c as f64 - expected).abs() / expected;
                assert!(dev < 0.03, "bound {bound}: {counts:?}");
            }
        }
    }

    #[test]
    fn all_120_pads_of_width_5_are_reachable_and_roughly_uniform() {
        let mut p = Xorshift32::new(99);
        let mut counts = [0u32; 3125];
        let samples = 120_000u32;
        for _ in 0..samples {
            let pad: [u8; 5] = generate_pad(&mut p);
            let idx = pad.iter().fold(0usize, |a, &d| a * 5 + d as usize);
            counts[idx] += 1;
        }
        assert_eq!(counts.iter().filter(|&&c| c > 0).count(), 120);
        for &c in counts.iter().filter(|&&c| c > 0) {
            assert!((800..1200).contains(&c), "pad count {c} far from 1000");
        }
    }
}
