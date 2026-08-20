//! Footprint track of the QPP-RNG test harness (see
//! `qpp-rng-testing-architecture.md` §5.2):
//!
//! - [`size`] -- code/flash size, via `cargo size`/`cargo bloat`.
//! - [`stack`] -- stack/RAM usage, via `cargo call-stack` (`no_std`-only
//!   static analysis) plus a manual fill-pattern high-water-mark
//!   fallback.
//! - [`cycles`] -- cycles-per-output-byte as a power proxy, via
//!   `entropy_timer`'s platform timer.
//! - [`report`] -- folds all three into one [`report::FootprintReport`]
//!   per candidate.
//!
//! `src/bin/footprint_cli.rs` exposes the pieces that need a real
//! built-binary path as a CLI `xtask` shells out to per target triple.

pub mod cycles;
pub mod report;
pub mod size;
pub mod stack;
mod toolshell;

pub use report::FootprintReport;
