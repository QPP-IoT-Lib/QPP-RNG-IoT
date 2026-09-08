//! API/error-handling parity checker: structural invariants every
//! [`QppRngSource`] candidate is expected to uphold, checked uniformly
//! across [`candidates::all_candidates`].
//!
//! Every candidate today shares `TryRng::Error = Infallible` (see
//! `rng-core`'s trait definitions), so there's no *error-handling*
//! divergence to check yet -- but the structural checks below (buffer
//! filling, diagnostics self-consistency) are exactly the kind of thing
//! that silently breaks when a new variant's feature flag swaps one
//! implementation in for another, which is the scenario this checker
//! exists to guard.

use rand_core::Rng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParityResult {
    pub candidate: String,
    pub handles_zero_length_buffer: bool,
    pub writes_to_the_buffer: bool,
    pub diagnostics_report_nonzero_activity: bool,
    pub diagnostics_permutation_size_matches_array_size: bool,
    pub errors: Vec<String>,
}

impl ParityResult {
    pub fn all_passed(&self) -> bool {
        self.handles_zero_length_buffer
            && self.writes_to_the_buffer
            && self.diagnostics_report_nonzero_activity
            && self.diagnostics_permutation_size_matches_array_size
    }
}

/// Runs every structural parity check against one candidate.
pub fn check_parity(candidate: &candidates::Candidate, seed: u128) -> ParityResult {
    let mut errors = Vec::new();
    let mut rng = (candidate.make)(seed);

    let handles_zero_length_buffer = {
        let mut empty: [u8; 0] = [];
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rng.fill_bytes(&mut empty);
        }));
        if let Err(e) = &result {
            errors.push(format!("fill_bytes(&mut []) panicked: {}", panic_message(e)));
        }
        result.is_ok()
    };

    let writes_to_the_buffer = {
        // Sentinel-fill a buffer, then confirm the buffer as a *whole*
        // no longer equals its all-sentinel starting state. Checking
        // "every individual byte changed" instead would be the wrong,
        // far stricter test: for a real uniformly-distributed output,
        // roughly `len / 256` bytes are expected to coincidentally
        // equal any fixed sentinel value by chance (at 256 bytes,
        // that's an expected ~1 collision, not zero) -- that's normal,
        // not a sign fill_bytes left anything untouched. The buffer
        // matching its pre-fill state in *every* position, on the
        // other hand, is astronomically unlikely for any real
        // generator and would only happen if fill_bytes were silently
        // a no-op.
        const SENTINEL: u8 = 0x5A;
        let before = [SENTINEL; 256];
        let mut buf = before;
        rng.fill_bytes(&mut buf);
        let changed = buf != before;
        if !changed {
            errors.push("fill_bytes left the buffer exactly as it was (looks like a no-op)".to_string());
        }
        changed
    };

    let diag = rng.diagnostics();
    let diagnostics_report_nonzero_activity = diag.last_permutation_count > 0;
    if !diagnostics_report_nonzero_activity {
        errors.push("diagnostics().last_permutation_count was 0 after generating output".to_string());
    }

    let expected_bits = permutation_entropy_bits(candidate.array_size);
    let diagnostics_permutation_size_matches_array_size = diag.permutation_size_bits == expected_bits;
    if !diagnostics_permutation_size_matches_array_size {
        errors.push(format!(
            "diagnostics().permutation_size_bits = {}, expected floor(log2({}!)) = {}",
            diag.permutation_size_bits, candidate.array_size, expected_bits
        ));
    }

    ParityResult {
        candidate: candidate.name.to_string(),
        handles_zero_length_buffer,
        writes_to_the_buffer,
        diagnostics_report_nonzero_activity,
        diagnostics_permutation_size_matches_array_size,
        errors,
    }
}

/// Runs [`check_parity`] over every registered candidate.
pub fn check_all_parity(seed: u128) -> Vec<ParityResult> {
    candidates::all_candidates()
        .into_iter()
        .map(|c| check_parity(&c, seed))
        .collect()
}

/// Mirrors `qpp-rng-reference`'s private `permutation_entropy_bits` so
/// this checker can independently verify [`RngDiagnostics`]'s claim
/// rather than trusting the same formula that produced it.
///
/// [`RngDiagnostics`]: rng_core::RngDiagnostics
fn permutation_entropy_bits(n: usize) -> u8 {
    let factorial: u64 = (1..=n as u64).product::<u64>().max(1);
    if factorial <= 1 {
        0
    } else {
        (63 - factorial.leading_zeros()) as u8
    }
}

fn panic_message(e: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = e.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = e.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_candidate_passes_parity() {
        for result in check_all_parity(0xC0FF_EE00_1234_5678_9ABC_DEF0_1122_3344) {
            assert!(result.all_passed(), "{result:?}");
        }
    }

    #[test]
    fn permutation_entropy_bits_matches_known_values() {
        assert_eq!(permutation_entropy_bits(0), 0);
        assert_eq!(permutation_entropy_bits(1), 0); // 1! = 1
        assert_eq!(permutation_entropy_bits(5), 6); // 5! = 120, floor(log2(120)) = 6
        assert_eq!(permutation_entropy_bits(6), 9); // 6! = 720, floor(log2(720)) = 9
    }
}
