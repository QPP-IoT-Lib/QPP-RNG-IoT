//! Cross-implementation determinism comparator: feeds an identical
//! [`MockClock`] script into two fresh instances of the *same*
//! configuration and diffs their output.
//!
//! ## Reading "cross-implementation" here
//!
//! Different implementations/PRNG configurations are *expected* to
//! diverge from each other even under an identical clock script and
//! seed (see `qpp-rng-reference`'s own
//! `different_jitter_scripts_diverge`-style reasoning applied the other
//! direction: different internal PRNGs consume the same jitter bytes
//! differently). So "compare every implementation" means "check every
//! implementation is internally deterministic" -- run each one twice
//! against the same script and require identical output -- not "check
//! that different algorithms agree with each other", which isn't a
//! property any of them claim.
//!
//! ## Why this can't go through the `candidates` registry
//!
//! [`candidates::Candidate::make`] returns `Box<dyn QppRngSource>` built
//! on the real [`entropy_timer::PlatformTimer`] -- see that crate's
//! module doc for why that erasure makes it unusable for mock-clock
//! testing. This module instead constructs `qpp-rng-reference`'s two
//! configurations directly and generically over [`MockClock`], mirroring
//! (deliberately duplicating, in the small way Rust's lack of
//! higher-kinded generics forces) the two entries in
//! `candidates::all_candidates`.

use qpp_rng_reference::prng::{NextX48, Xorshift128Plus};
use qpp_rng_reference::{DEFAULT_ARRAY_SIZE, QppRng};
use rand_core::Rng;
use serde::{Deserialize, Serialize};

use crate::mock_clock::MockClock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterminismResult {
    pub candidate: String,
    pub deterministic: bool,
    /// Byte index of the first mismatch, if any.
    pub first_divergence_index: Option<usize>,
    pub n_bytes: usize,
}

fn diff(candidate: &str, buf_a: &[u8], buf_b: &[u8]) -> DeterminismResult {
    let first_divergence_index = buf_a.iter().zip(buf_b.iter()).position(|(a, b)| a != b);
    DeterminismResult {
        candidate: candidate.to_string(),
        deterministic: first_divergence_index.is_none(),
        first_divergence_index,
        n_bytes: buf_a.len(),
    }
}

fn check_xorshift128plus_determinism(seed: u128, deltas: &[u64], n_bytes: usize) -> DeterminismResult {
    let mut a = QppRng::<Xorshift128Plus, MockClock, DEFAULT_ARRAY_SIZE>::new(
        Xorshift128Plus::default(),
        MockClock::new(deltas.to_vec()),
        seed,
    );
    let mut b = QppRng::<Xorshift128Plus, MockClock, DEFAULT_ARRAY_SIZE>::new(
        Xorshift128Plus::default(),
        MockClock::new(deltas.to_vec()),
        seed,
    );
    let mut buf_a = vec![0u8; n_bytes];
    let mut buf_b = vec![0u8; n_bytes];
    a.fill_bytes(&mut buf_a);
    b.fill_bytes(&mut buf_b);
    diff("reference-xorshift128plus", &buf_a, &buf_b)
}

fn check_nextx48_determinism(seed: u128, deltas: &[u64], n_bytes: usize) -> DeterminismResult {
    let mut a = QppRng::<NextX48, MockClock, DEFAULT_ARRAY_SIZE>::new(
        NextX48::default(),
        MockClock::new(deltas.to_vec()),
        seed,
    );
    let mut b = QppRng::<NextX48, MockClock, DEFAULT_ARRAY_SIZE>::new(
        NextX48::default(),
        MockClock::new(deltas.to_vec()),
        seed,
    );
    let mut buf_a = vec![0u8; n_bytes];
    let mut buf_b = vec![0u8; n_bytes];
    a.fill_bytes(&mut buf_a);
    b.fill_bytes(&mut buf_b);
    diff("reference-nextx48", &buf_a, &buf_b)
}

/// Runs the determinism check against every `qpp-rng-reference`
/// configuration this crate knows how to construct generically. Add a
/// new `check_*_determinism` function here (matching one new entry in
/// `candidates::all_candidates`) whenever a new implementation gains a
/// `HighResTimer`-generic constructor -- see the module doc.
pub fn check_all_determinism(seed: u128, deltas: &[u64], n_bytes: usize) -> Vec<DeterminismResult> {
    vec![
        check_xorshift128plus_determinism(seed, deltas, n_bytes),
        check_nextx48_determinism(seed, deltas, n_bytes),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_reference_configuration_is_deterministic_under_a_fixed_script() {
        let results = check_all_determinism(
            0x1122_3344_5566_7788_99AA_BBCC_DDEE_FF00,
            &[101, 43, 999, 7, 256, 12],
            32,
        );
        assert_eq!(results.len(), 2);
        for r in &results {
            assert!(r.deterministic, "{r:?} was not deterministic");
            assert!(r.first_divergence_index.is_none());
        }
    }
}
