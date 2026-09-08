//! Data ingestion layer: reads back whatever the other four
//! `test-harness/*` crates already wrote to disk (or, for `bench`,
//! reads criterion's output directly through `bench`'s own exporter
//! function) into this crate's own in-memory types.
//!
//! Every function here is forgiving about a missing/absent input file:
//! a track that wasn't run for this invocation (e.g. `--skip-bench`)
//! should render as "N/A" in the final report, not abort the whole
//! `report` step. Only a file that exists but fails to *parse* is
//! treated as a real error.

use std::path::Path;

use anyhow::Context;

use bench::BenchReport;
use differential::DifferentialReport;
use footprint::FootprintReport;
use stats::StatReport;

/// Reads the JSON array `stats-cli full --out <path>` writes.
pub fn ingest_stats(path: &Path) -> anyhow::Result<Vec<StatReport>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// Runs `bench`'s own criterion-JSON exporter directly against
/// `criterion_dir` (normally `target/criterion`). Unlike the other
/// three ingestion functions, this doesn't read an intermediate file
/// `report` wrote itself -- `bench::export_from_criterion_dir` is cheap
/// and safe to call directly, so there's no need for a `bench-export`
/// round trip through disk just to hand the same data to `report` in
/// the same process tree `xtask` already controls.
pub fn ingest_bench(criterion_dir: &Path) -> anyhow::Result<BenchReport> {
    bench::export_from_criterion_dir(criterion_dir)
}

/// Reads every footprint report JSON file in `paths` (one per
/// candidate/target-triple combination `footprint-cli full` was run
/// against).
pub fn ingest_footprint(paths: &[std::path::PathBuf]) -> anyhow::Result<Vec<FootprintReport>> {
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        out.push(serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?);
    }
    Ok(out)
}

/// Reads the JSON `differential-cli --out <path>` writes.
pub fn ingest_differential(path: &Path) -> anyhow::Result<Option<DifferentialReport>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(Some(
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_stats_on_a_missing_file_returns_an_empty_vec_not_an_error() {
        let result = ingest_stats(Path::new("/definitely/does/not/exist.json"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn ingest_differential_on_a_missing_file_returns_none_not_an_error() {
        let result = ingest_differential(Path::new("/definitely/does/not/exist.json"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn ingest_bench_on_a_missing_directory_returns_an_empty_report() {
        let result = ingest_bench(Path::new("/definitely/does/not/exist"));
        assert!(result.is_ok());
        assert!(result.unwrap().measurements.is_empty());
    }
}
