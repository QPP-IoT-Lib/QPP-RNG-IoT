//! A SP 800-90B-style conditioning component: compresses a raw entropy
//! source's output through SHA-256 to remove structural bias before it
//! reaches anything that needs full-strength (uniform) bytes.
//!
//! ## Why this exists, and why it's a separate crate from `qpp-rng-reference`
//!
//! `test-harness/stats`'s Tier 1 battery found a real, reproducible bias
//! in `qpp-rng-reference`'s raw output: bit 7 of each output byte sits
//! at ~48.6% ones instead of 50%. Root cause (confirmed with a pure
//! Monte Carlo simulation of just the extraction math, no timer/
//! hardware involved): `next_byte()`'s extraction step is `n_p mod
//! 256`, and `n_p` (draws until the Fisher-Yates convergence walk
//! returns to identity) is geometrically distributed with mean `N!`.
//! For `N=5` that mean (120) is well under 256, so a *single* draw's
//! `mod 256` residue is heavily skewed (~26% ones on bit 7, not 50%) --
//! the geometric tail decays before it wraps evenly around 256.
//! XORing `oversample` (5, by default) independent draws together
//! already shrinks that a lot (the bias shrinks geometrically, as
//! `(1 - 2p)^oversample`), which is why it's `~1.3` percentage points
//! instead of `~24` -- but not all the way to zero.
//!
//! SP 800-90B's own methodology is explicit that this is fine, and
//! expected, for a raw entropy source: raw noise is allowed to be
//! biased, as long as its *min-entropy* is high enough -- which is
//! exactly why Tier 2's `ea_non_iid`/`ea_iid` min-entropy estimate
//! (measured at 6.6-7.3 bits/byte for this port, out of a possible 8)
//! is the real gate, not Tier 1's SP 800-22-style uniformity check.
//! Real hardware TRNGs (Intel's RDRAND, ARM's TRNG peripherals, ...)
//! universally pair a biased-but-entropic raw source with a downstream
//! *conditioning component* to produce uniform output for anything
//! that actually needs it -- that pairing, not "make the raw source
//! itself pass a uniformity test", is the standard architecture this
//! crate follows.
//!
//! Keeping this conditioner in its own crate, rather than folding it
//! into `qpp-rng-reference`, is deliberate for the same reason
//! `qpp-rng-reference` and `qpp-rng-iot` are kept separate (see the
//! testing architecture doc, §3): it keeps the *raw* entropy source
//! -- the thing Tier 2's SP 800-90B evaluation is actually
//! characterizing -- untouched and independently re-testable, and lets
//! `test-harness/candidates` register both the raw and conditioned
//! forms of the same candidate side by side for direct comparison
//! (see that crate's registry).
//!
//! ## The construction
//!
//! A plain hash-based compression conditioner: accumulate
//! [`INPUT_BLOCK_BYTES`] raw bytes from the wrapped source, compress
//! them through SHA-256, and serve the resulting [`OUTPUT_BLOCK_BYTES`]
//! (32) conditioned bytes out before pulling the next block. `64 -> 32`
//! is a conservative 2:1 compression ratio -- comfortable headroom
//! given this port's raw source already measures 6.6-7.3 min-entropy
//! bits/byte (Tier 2's own numbers), well above what a 2:1 hash
//! compression needs to fully recover 8 bits/byte of conditioned
//! output. This is *not* a formally-proven information-theoretic
//! extractor (that would need a per-candidate min-entropy bound baked
//! into the ratio, which this crate can't know generically); it's the
//! same category of construction SP 800-90B's own "vetted conditioning
//! components" discussion describes (hash-based compression), applied
//! with a conservative safety margin rather than the bare theoretical
//! minimum -- concretely, checked against `test-harness/stats`'s own
//! Tier 2 numbers for the two `qpp-rng-reference` configs this wraps
//! today: `64 * 6.638` and `64 * 7.244` raw min-entropy bits
//! (xorshift128+, NEXT_X48 respectively, from `ea_non_iid`) both clear
//! `256 + 64` (SP 800-90B/C's common "output size + 64 bits" margin
//! rule of thumb for a 256-bit SHA-256 output) with **over 100 bits to
//! spare** in both cases.
//!
//! ## Do not re-run SP 800-90B's entropy-source estimators on this
//! ## conditioner's *output* and expect the number to mean anything
//!
//! This tripped us up once already, so it's worth stating plainly:
//! `ea_iid`/`ea_non_iid` are built to characterize a **raw, unconditioned**
//! noise source, by looking for exploitable short-range predictability
//! in it. A cryptographic hash is specifically designed to defeat
//! exactly that kind of pattern-matching -- in *both* directions. It can
//! make a genuinely weak raw source's hashed output look artificially
//! excellent (which is the well-known danger of "testing" a conditioner
//! this way instead of doing the entropy-budget accounting above), and,
//! observed directly on this port's two candidates, it can also make a
//! perfectly fine source score *lower* on one specific non-IID
//! sub-estimator than its own raw input did, for reasons that reflect
//! the estimator's own sensitivity to hashed-block structure, not a
//! real entropy loss. (Concretely: raw NEXT_X48 measured 7.24
//! non-IID min-entropy bits/byte; its SHA-256-conditioned output
//! measured *6.64* -- a drop that means "wrong tool for this input",
//! not "conditioning made it worse".) [`Sha256Conditioner`]'s output
//! *is* fair game for Tier 1's native checks and ENT/STS -- those test
//! uniformity, which is exactly the property conditioning targets, and
//! is exactly what improved (Tier 1 flips from failing to passing on
//! both wrapped candidates; STS's pass rate on xorshift128+ goes from
//! ~7% raw to ~99.5% conditioned). The raw-source min-entropy estimate
//! is the only number [`Sha256Conditioner`]'s own design should be
//! judged against, and that number has to come from the *unconditioned*
//! candidate, before wrapping.
//!
//! [`Sha256Conditioner`] wraps any [`rand_core::Rng`], and additionally
//! implements [`QppRngSource`] (delegating
//! [`QppRngSource::diagnostics`] to the wrapped source) whenever the
//! source it wraps does too -- so it's a drop-in replacement anywhere a
//! raw candidate is used today, with conditioned output instead of raw.

#![no_std]

use core::convert::Infallible;

use rand_core::{Rng, TryRng};
use rng_core::{QppRngSource, RngDiagnostics};
use sha2::{Digest, Sha256};

/// Raw bytes pulled from the wrapped source per conditioning block.
pub const INPUT_BLOCK_BYTES: usize = 64;

/// Conditioned bytes produced per block -- SHA-256's digest size, fixed
/// by the hash function itself.
pub const OUTPUT_BLOCK_BYTES: usize = 32;

/// Wraps `R` and serves SHA-256-conditioned output instead of `R`'s raw
/// bytes. See the crate root doc for the construction and the bias this
/// exists to remove.
pub struct Sha256Conditioner<R> {
    source: R,
    out_buf: [u8; OUTPUT_BLOCK_BYTES],
    /// Bytes of `out_buf` already served; `OUTPUT_BLOCK_BYTES` means
    /// the buffer is empty and the next read must pull a fresh block.
    out_pos: usize,
}

impl<R> Sha256Conditioner<R> {
    /// Wraps `source`. No bytes are pulled from it until the first
    /// output byte is actually requested.
    pub fn new(source: R) -> Self {
        Self {
            source,
            out_buf: [0u8; OUTPUT_BLOCK_BYTES],
            out_pos: OUTPUT_BLOCK_BYTES,
        }
    }

    /// Unwraps back to the underlying raw source.
    pub fn into_inner(self) -> R {
        self.source
    }

    /// Borrows the underlying raw source -- useful for reading
    /// [`QppRngSource::diagnostics`] directly without going through the
    /// delegating impl below, or for any other source-specific API the
    /// conditioner itself doesn't expose.
    pub fn inner(&self) -> &R {
        &self.source
    }
}

impl<R: Rng> Sha256Conditioner<R> {
    fn refill(&mut self) {
        let mut block = [0u8; INPUT_BLOCK_BYTES];
        self.source.fill_bytes(&mut block);
        let digest = Sha256::digest(block);
        self.out_buf.copy_from_slice(digest.as_slice());
        self.out_pos = 0;
    }
}

impl<R: Rng> TryRng for Sha256Conditioner<R> {
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
        let mut written = 0;
        while written < dst.len() {
            if self.out_pos >= OUTPUT_BLOCK_BYTES {
                self.refill();
            }
            let available = OUTPUT_BLOCK_BYTES - self.out_pos;
            let take = available.min(dst.len() - written);
            dst[written..written + take].copy_from_slice(&self.out_buf[self.out_pos..self.out_pos + take]);
            self.out_pos += take;
            written += take;
        }
        Ok(())
    }
}

impl<R: QppRngSource> QppRngSource for Sha256Conditioner<R> {
    /// Delegates to the wrapped raw source. Diagnostics describe the
    /// underlying permutation-sort/jitter mechanics -- still meaningful
    /// and useful to report after conditioning, since conditioning
    /// changes the *output bytes*, not the entropy-harvesting process
    /// those diagnostics describe.
    fn diagnostics(&self) -> RngDiagnostics {
        self.source.diagnostics()
    }
}

#[cfg(test)]
mod tests {
    // `#![no_std]` removes the automatic `extern crate std;` this crate
    // would otherwise get; test builds still link std (the test
    // harness needs it regardless), so re-adding it here just for this
    // module is enough to use `std::vec::Vec` below without lifting the
    // `no_std` attribute for the whole crate.
    extern crate std;

    use super::*;

    /// A deterministic, cheaply-constructible `Rng` for tests -- a
    /// wrapping byte counter, not tied to any real candidate, so these
    /// tests characterize the conditioner's own logic.
    struct CountingRng {
        counter: u8,
    }
    impl TryRng for CountingRng {
        type Error = Infallible;
        fn try_next_u32(&mut self) -> Result<u32, Infallible> {
            let mut b = [0u8; 4];
            self.try_fill_bytes(&mut b)?;
            Ok(u32::from_le_bytes(b))
        }
        fn try_next_u64(&mut self) -> Result<u64, Infallible> {
            let mut b = [0u8; 8];
            self.try_fill_bytes(&mut b)?;
            Ok(u64::from_le_bytes(b))
        }
        fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
            for b in dst.iter_mut() {
                *b = self.counter;
                self.counter = self.counter.wrapping_add(1);
            }
            Ok(())
        }
    }

    #[test]
    fn fill_bytes_of_exactly_one_block_matches_a_direct_sha256_call() {
        let mut cond = Sha256Conditioner::new(CountingRng { counter: 0 });
        let mut out = [0u8; OUTPUT_BLOCK_BYTES];
        cond.fill_bytes(&mut out);

        let expected_input: [u8; INPUT_BLOCK_BYTES] = core::array::from_fn(|i| i as u8);
        let expected = Sha256::digest(expected_input);
        assert_eq!(&out[..], expected.as_slice());
    }

    #[test]
    fn fill_bytes_spanning_multiple_blocks_produces_the_right_length_and_content() {
        let mut cond = Sha256Conditioner::new(CountingRng { counter: 0 });
        let n = OUTPUT_BLOCK_BYTES * 3 + 5; // spans 4 conditioning blocks
        // `std::` paths resolve fine here even under this crate's
        // `#![no_std]`: that attribute only suppresses the automatic
        // `std` prelude, not `std` itself, which `cargo test`'s harness
        // links regardless.
        let mut out: std::vec::Vec<u8> = std::vec![0u8; n];
        cond.fill_bytes(&mut out);
        assert_eq!(out.len(), n);

        // Re-derive independently: block i's conditioned bytes are
        // SHA-256 of the counting source's i-th 64-byte block.
        let mut src = CountingRng { counter: 0 };
        let mut expected: std::vec::Vec<u8> = std::vec::Vec::new();
        while expected.len() < n {
            let mut block = [0u8; INPUT_BLOCK_BYTES];
            src.fill_bytes(&mut block);
            expected.extend_from_slice(Sha256::digest(block).as_slice());
        }
        expected.truncate(n);
        assert_eq!(out, expected);
    }

    #[test]
    fn same_source_state_produces_deterministic_conditioned_output() {
        let mut a = Sha256Conditioner::new(CountingRng { counter: 0 });
        let mut b = Sha256Conditioner::new(CountingRng { counter: 0 });
        let mut out_a = [0u8; 100];
        let mut out_b = [0u8; 100];
        a.fill_bytes(&mut out_a);
        b.fill_bytes(&mut out_b);
        assert_eq!(out_a, out_b);
    }

    #[test]
    fn different_source_state_diverges_conditioned_output() {
        let mut a = Sha256Conditioner::new(CountingRng { counter: 0 });
        let mut b = Sha256Conditioner::new(CountingRng { counter: 7 });
        let mut out_a = [0u8; 100];
        let mut out_b = [0u8; 100];
        a.fill_bytes(&mut out_a);
        b.fill_bytes(&mut out_b);
        assert_ne!(out_a, out_b);
    }

    #[test]
    fn into_inner_and_inner_round_trip() {
        let cond = Sha256Conditioner::new(CountingRng { counter: 42 });
        assert_eq!(cond.inner().counter, 42);
        let inner = cond.into_inner();
        assert_eq!(inner.counter, 42);
    }
}
