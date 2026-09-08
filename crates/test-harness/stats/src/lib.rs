//! Statistical quality track of the QPP-RNG test harness (see
//! `qpp-rng-testing-architecture.md` §5.1).
//!
//! - [`sample`] -- pulls raw byte streams from every [`candidates`]
//!   entry and writes SP 800-90B-sized sample files.
//! - [`tier1`] -- fast, pure-Rust native smoke tests (monobit, runs,
//!   chi-square, serial correlation, Shannon entropy), run on every
//!   `cargo test`.
//! - [`tier2`] -- orchestrates the external NIST SP 800-90B/SP 800-22
//!   and ENT reference tools.
//! - [`report`] -- folds Tier 1 + Tier 2 into one [`report::StatReport`]
//!   per candidate.
//!
//! `src/bin/stats_cli.rs` exposes all of this as a CLI `xtask` shells
//! out to.

mod mathfns;
pub mod report;
pub mod sample;
pub mod tier1;
pub mod tier2;

pub use report::{run_full_battery, StatReport, Tier2Options};
