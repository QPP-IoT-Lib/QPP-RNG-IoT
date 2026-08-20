//! Performance track of the QPP-RNG test harness: criterion benchmark
//! groups (in `benches/qpp_rng_benches.rs`) for throughput and latency,
//! plus a result exporter that folds criterion's own JSON output into
//! one shared [`BenchReport`] the `report` crate can ingest.
//!
//! This crate deliberately does **not** reimplement any timing --
//! criterion already does that well. It only reads back what criterion
//! already wrote to `target/criterion/**/new/{estimates,benchmark}.json`
//! and reshapes it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// One criterion benchmark's result, reshaped into this harness's
/// vocabulary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchMeasurement {
    /// Criterion's benchmark group name -- one of this crate's benches
    /// (`throughput`, `latency_per_call`, `time_to_first_byte`; see
    /// `benches/qpp_rng_benches.rs`).
    pub group: String,
    /// The candidate name (criterion's `function_id`), matching
    /// [`candidates::Candidate::name`].
    pub candidate: String,
    pub mean_ns: f64,
    pub mean_std_error_ns: f64,
    pub median_ns: f64,
    pub std_dev_ns: f64,
    /// Present only for benchmarks that called
    /// `BenchmarkGroup::throughput` (the `throughput` group) -- bytes
    /// processed per call divided by mean time per call.
    pub throughput_bytes_per_sec: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BenchReport {
    pub measurements: Vec<BenchMeasurement>,
}

impl BenchReport {
    /// Groups measurements by [`BenchMeasurement::candidate`], for
    /// report tables that want one row per candidate.
    pub fn by_candidate(&self) -> BTreeMap<&str, Vec<&BenchMeasurement>> {
        let mut map: BTreeMap<&str, Vec<&BenchMeasurement>> = BTreeMap::new();
        for m in &self.measurements {
            map.entry(m.candidate.as_str()).or_default().push(m);
        }
        map
    }
}

// ---------------------------------------------------------------------
// Criterion's on-disk JSON shapes, trimmed to the fields this crate
// reads. Deliberately independent structs rather than depending on
// `criterion` itself from a non-dev context (this lib is meant to be
// usable from `report`/`xtask` without pulling criterion's full runtime
// dependency tree into a plain result-reading path).
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct RawEstimate {
    point_estimate: f64,
    standard_error: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct RawEstimates {
    mean: RawEstimate,
    median: RawEstimate,
    std_dev: RawEstimate,
}

#[derive(Debug, Clone, Deserialize)]
enum RawThroughput {
    // Kept for exhaustive-match parity with `criterion::Throughput` --
    // these variants aren't produced by this crate's own benches (only
    // `Bytes` is), but a future bench group might use them, and
    // deserialization must still succeed rather than error out.
    #[allow(dead_code)]
    Bits(u64),
    Bytes(u64),
    BytesDecimal(u64),
    #[allow(dead_code)]
    Elements(u64),
}

#[derive(Debug, Clone, Deserialize)]
struct RawBenchmarkId {
    group_id: String,
    function_id: Option<String>,
    value_str: Option<String>,
    throughput: Option<RawThroughput>,
}

/// Walks `criterion_dir` (normally `target/criterion`) for every
/// `<group>/<function>/new/{estimates,benchmark}.json` pair criterion
/// wrote and folds them into one [`BenchReport`].
///
/// Silently skips any `new/` directory missing one of the two files
/// (an interrupted/partial criterion run) rather than failing the whole
/// export over one incomplete benchmark.
pub fn export_from_criterion_dir(criterion_dir: &Path) -> anyhow::Result<BenchReport> {
    let mut measurements = Vec::new();
    for new_dir in find_new_dirs(criterion_dir)? {
        let estimates_path = new_dir.join("estimates.json");
        let benchmark_path = new_dir.join("benchmark.json");
        if !estimates_path.is_file() || !benchmark_path.is_file() {
            continue;
        }

        let estimates: RawEstimates = serde_json::from_str(
            &fs::read_to_string(&estimates_path)
                .with_context(|| format!("reading {}", estimates_path.display()))?,
        )
        .with_context(|| format!("parsing {}", estimates_path.display()))?;
        let id: RawBenchmarkId = serde_json::from_str(
            &fs::read_to_string(&benchmark_path)
                .with_context(|| format!("reading {}", benchmark_path.display()))?,
        )
        .with_context(|| format!("parsing {}", benchmark_path.display()))?;

        let candidate = id
            .function_id
            .or(id.value_str)
            .unwrap_or_else(|| id.group_id.clone());

        let throughput_bytes_per_sec = match id.throughput {
            Some(RawThroughput::Bytes(n)) | Some(RawThroughput::BytesDecimal(n)) => {
                Some(n as f64 / (estimates.mean.point_estimate * 1e-9))
            }
            _ => None,
        };

        measurements.push(BenchMeasurement {
            group: id.group_id,
            candidate,
            mean_ns: estimates.mean.point_estimate,
            mean_std_error_ns: estimates.mean.standard_error,
            median_ns: estimates.median.point_estimate,
            std_dev_ns: estimates.std_dev.point_estimate,
            throughput_bytes_per_sec,
        });
    }

    measurements.sort_by(|a, b| (&a.group, &a.candidate).cmp(&(&b.group, &b.candidate)));
    Ok(BenchReport { measurements })
}

/// Recursively collects every directory named `new` under `root`
/// (criterion's convention for "most recent run's data", as opposed to
/// `base`, the saved-baseline copy -- see criterion's
/// `analysis::copy_new_dir_to_base`). Skips the top-level `report`
/// directory, which holds criterion's HTML output, not benchmark data.
fn find_new_dirs(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some("report") {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some("new") {
                out.push(path);
            } else {
                stack.push(path);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fake_criterion_output(root: &Path, group: &str, function: &str, bytes_throughput: Option<u64>) {
        let new_dir = root.join(group).join(function).join("new");
        fs::create_dir_all(&new_dir).unwrap();

        let estimates = serde_json::json!({
            "mean": {
                "confidence_interval": {"confidence_level": 0.95, "lower_bound": 90.0, "upper_bound": 110.0},
                "point_estimate": 100.0,
                "standard_error": 1.5
            },
            "median": {
                "confidence_interval": {"confidence_level": 0.95, "lower_bound": 90.0, "upper_bound": 110.0},
                "point_estimate": 99.0,
                "standard_error": 1.4
            },
            "median_abs_dev": {
                "confidence_interval": {"confidence_level": 0.95, "lower_bound": 1.0, "upper_bound": 2.0},
                "point_estimate": 1.5,
                "standard_error": 0.1
            },
            "slope": null,
            "std_dev": {
                "confidence_interval": {"confidence_level": 0.95, "lower_bound": 5.0, "upper_bound": 10.0},
                "point_estimate": 7.0,
                "standard_error": 0.2
            }
        });
        fs::write(new_dir.join("estimates.json"), serde_json::to_string(&estimates).unwrap()).unwrap();

        let throughput = bytes_throughput.map(|n| serde_json::json!({"Bytes": n}));
        let benchmark = serde_json::json!({
            "group_id": group,
            "function_id": function,
            "value_str": null,
            "throughput": throughput,
            "full_id": format!("{group}/{function}"),
            "directory_name": format!("{group}/{function}"),
            "title": format!("{group}/{function}"),
        });
        fs::write(new_dir.join("benchmark.json"), serde_json::to_string(&benchmark).unwrap()).unwrap();
    }

    #[test]
    fn export_reads_mean_and_computes_throughput() {
        let dir = std::env::temp_dir().join(format!("qpp-rng-bench-export-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        write_fake_criterion_output(&dir, "throughput", "reference-xorshift128plus", Some(64));
        write_fake_criterion_output(&dir, "latency_per_call", "reference-xorshift128plus", None);

        let report = export_from_criterion_dir(&dir).unwrap();
        assert_eq!(report.measurements.len(), 2);

        let by_candidate = report.by_candidate();
        let rows = &by_candidate["reference-xorshift128plus"];
        assert_eq!(rows.len(), 2);

        let throughput_row = rows.iter().find(|m| m.group == "throughput").unwrap();
        assert!((throughput_row.mean_ns - 100.0).abs() < 1e-9);
        // 64 bytes / (100ns) = 64 / 100e-9 s = 6.4e8 bytes/sec
        let expected_bps = 64.0 / (100.0 * 1e-9);
        assert!(
            (throughput_row.throughput_bytes_per_sec.unwrap() - expected_bps).abs() < 1.0,
            "{:?}",
            throughput_row.throughput_bytes_per_sec
        );

        let latency_row = rows.iter().find(|m| m.group == "latency_per_call").unwrap();
        assert!(latency_row.throughput_bytes_per_sec.is_none());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn export_on_missing_directory_returns_empty_report_not_error() {
        let report = export_from_criterion_dir(Path::new("/definitely/does/not/exist")).unwrap();
        assert!(report.measurements.is_empty());
    }

    #[test]
    fn export_skips_the_report_html_directory() {
        let dir = std::env::temp_dir().join(format!("qpp-rng-bench-export-test2-{}", std::process::id()));
        fs::create_dir_all(dir.join("report").join("new")).unwrap();
        // A `report/new` dir with no estimates/benchmark json should be
        // silently skipped, not error the whole export.
        let result = export_from_criterion_dir(&dir);
        assert!(result.is_ok());
        assert!(result.unwrap().measurements.is_empty());
        fs::remove_dir_all(&dir).ok();
    }
}
