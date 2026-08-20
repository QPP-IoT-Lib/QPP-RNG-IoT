//! Reporting track of the QPP-RNG test harness (see
//! `qpp-rng-testing-architecture.md` §5.5): aggregates every other
//! `test-harness/*` crate's output into one comparison artifact.
//!
//! - [`ingest`] -- reads back `stats`/`bench`/`footprint`/
//!   `differential`'s results.
//! - [`table`] -- joins everything into one [`table::ComparisonTable`],
//!   keyed by candidate.
//! - [`markdown`] / [`csv`] -- render that table as Markdown and CSV.
//!
//! `src/bin/report_cli.rs` is the `cargo xtask compare` pipeline's last
//! step.

pub mod csv;
pub mod ingest;
pub mod markdown;
pub mod table;

pub use table::{build_comparison_table, ComparisonRow, ComparisonTable};
