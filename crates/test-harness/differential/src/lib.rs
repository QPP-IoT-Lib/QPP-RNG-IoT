//! Differential/property-based testing track of the QPP-RNG test
//! harness (see `qpp-rng-testing-architecture.md` §5.4): not about
//! randomness *quality* (that's `test-harness/stats`) -- about
//! implementation correctness and parity.
//!
//! - [`strategies`] -- shared `proptest` seed corpora / input
//!   generators.
//! - [`mock_clock`] -- a scripted `HighResTimer` for pinning exact
//!   jitter sequences.
//! - [`determinism`] -- cross-implementation determinism comparator
//!   (same script + seed in -> same output out, per implementation).
//! - [`fuzz`] -- `proptest`-driven panic/overflow fuzzing.
//! - [`parity`] -- structural API/error-handling parity checks across
//!   every registered candidate.
//! - [`report`] -- folds determinism + parity into one
//!   [`report::DifferentialReport`].

pub mod determinism;
pub mod mock_clock;
pub mod parity;
pub mod report;
pub mod strategies;

// Fuzz checks are proptest `#[test]` functions only, with no public API
// of their own (see `report`'s module doc for why they're not folded
// into `DifferentialReport`) -- gating the whole module behind
// `cfg(test)` keeps it out of ordinary (non-test) builds entirely,
// rather than compiling it unused every time.
#[cfg(test)]
mod fuzz;

pub use report::{run_all, DifferentialReport};
