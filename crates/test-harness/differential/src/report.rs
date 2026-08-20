//! Folds [`crate::determinism`] and [`crate::parity`] into one
//! [`DifferentialReport`]. The proptest-based [`crate::fuzz`] checks
//! deliberately aren't part of this report -- a property test's result
//! is binary pass/fail for the whole `cargo test` run (with a shrunk
//! counterexample on failure), not a per-candidate figure that belongs
//! in a comparison table.

use serde::{Deserialize, Serialize};

use crate::determinism::DeterminismResult;
use crate::parity::ParityResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DifferentialReport {
    pub determinism: Vec<DeterminismResult>,
    pub parity: Vec<ParityResult>,
}

impl DifferentialReport {
    pub fn all_passed(&self) -> bool {
        self.determinism.iter().all(|d| d.deterministic)
            && self.parity.iter().all(|p| p.all_passed())
    }
}

/// Runs every determinism and parity check and folds them into one
/// report.
pub fn run_all(seed: u128, deltas: &[u64], n_bytes: usize) -> DifferentialReport {
    DifferentialReport {
        determinism: crate::determinism::check_all_determinism(seed, deltas, n_bytes),
        parity: crate::parity::check_all_parity(seed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_all_passes_for_a_reasonable_default_script() {
        let report = run_all(0x1234_5678, &[10, 20, 30, 40], 32);
        assert!(report.all_passed(), "{report:?}");
    }
}
