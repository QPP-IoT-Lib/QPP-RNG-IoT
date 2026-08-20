//! Aggregation/normalization: joins every ingested source by candidate
//! name into one [`ComparisonTable`], the "single glance" comparison
//! artifact `qpp-rng-testing-architecture.md` §5.5 asks for.

use std::collections::BTreeMap;

use bench::BenchReport;
use differential::DifferentialReport;
use footprint::FootprintReport;
use serde::{Deserialize, Serialize};
use stats::StatReport;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComparisonRow {
    pub candidate: String,

    // -- stats --
    pub tier1_pass: Option<bool>,
    pub min_entropy_bits: Option<f64>,
    pub sp800_22_pass_rate: Option<f32>,
    pub ent_entropy_bits_per_byte: Option<f64>,
    pub shannon_entropy_bits_per_byte: Option<f64>,

    // -- bench --
    pub throughput_bytes_per_sec: Option<f64>,
    pub latency_per_call_ns: Option<f64>,
    pub time_to_first_byte_ns: Option<f64>,

    // -- footprint --
    pub text_bytes: Option<u64>,
    pub stack_high_water_mark_bytes: Option<u64>,
    pub ticks_per_output_byte: Option<f64>,

    // -- differential --
    pub deterministic: Option<bool>,
    pub api_parity_pass: Option<bool>,
}

impl ComparisonRow {
    /// `Some(true)` only if every gate that actually ran passed;
    /// `Some(false)` if any ran gate failed. A gate that never ran
    /// (`None`) doesn't count against this -- see [`ComparisonTable`]'s
    /// module doc on missing tracks rendering as "N/A", not "fail".
    ///
    /// Returns `None` -- not `Some(true)` -- when *none* of the three
    /// gates ran at all. `[None, None, None].flatten().all(...)` is
    /// vacuously `true` in plain Rust, which would otherwise render a
    /// row with zero actual data (e.g. a stray candidate name that only
    /// ever showed up in stale `target/criterion` output from an
    /// unrelated bench run, never in any of this crate's own gates) as
    /// a clean pass -- exactly the kind of row that has nothing to
    /// judge, not something that earned a checkmark.
    pub fn overall_pass(&self) -> Option<bool> {
        let gates = [self.tier1_pass, self.deterministic, self.api_parity_pass];
        if gates.iter().all(Option::is_none) {
            return None;
        }
        Some(gates.into_iter().flatten().all(|p| p))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComparisonTable {
    pub rows: Vec<ComparisonRow>,
}

/// Joins every ingested source into one [`ComparisonTable`], keyed by
/// candidate name. A candidate present in only some sources (e.g. Tier
/// 2 tools ran but footprint didn't) still gets one row, with the
/// missing fields left `None`.
pub fn build_comparison_table(
    stats: &[StatReport],
    bench: &BenchReport,
    footprint: &[FootprintReport],
    differential: Option<&DifferentialReport>,
) -> ComparisonTable {
    let mut rows: BTreeMap<String, ComparisonRow> = BTreeMap::new();

    fn row_for<'a>(rows: &'a mut BTreeMap<String, ComparisonRow>, candidate: &str) -> &'a mut ComparisonRow {
        rows.entry(candidate.to_string())
            .or_insert_with(|| ComparisonRow {
                candidate: candidate.to_string(),
                ..Default::default()
            })
    }

    for s in stats {
        let row = row_for(&mut rows, &s.candidate);
        row.tier1_pass = Some(s.tier1.all_passed());
        row.min_entropy_bits = s.min_entropy_estimate();
        row.sp800_22_pass_rate = s.sp800_22_pass_rate();
        row.ent_entropy_bits_per_byte = s.ent_entropy_bits_per_byte();
        row.shannon_entropy_bits_per_byte = Some(s.tier1.shannon_entropy_bits_per_byte.statistic);
    }

    for (candidate, measurements) in bench.by_candidate() {
        let row = row_for(&mut rows, candidate);
        for m in measurements {
            match m.group.as_str() {
                "throughput" => row.throughput_bytes_per_sec = m.throughput_bytes_per_sec,
                "latency_per_call" => row.latency_per_call_ns = Some(m.mean_ns),
                "time_to_first_byte" => row.time_to_first_byte_ns = Some(m.mean_ns),
                _ => {}
            }
        }
    }

    for f in footprint {
        let row = row_for(&mut rows, &f.candidate);
        row.text_bytes = f.headline_text_bytes();
        row.stack_high_water_mark_bytes = f.call_stack.max_labeled_bytes;
        row.ticks_per_output_byte = f.cycles.as_ref().map(|c| c.ticks_per_output_byte);
    }

    if let Some(d) = differential {
        for det in &d.determinism {
            row_for(&mut rows, &det.candidate).deterministic = Some(det.deterministic);
        }
        for par in &d.parity {
            row_for(&mut rows, &par.candidate).api_parity_pass = Some(par.all_passed());
        }
    }

    ComparisonTable {
        rows: rows.into_values().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_comparison_table_joins_by_candidate_name() {
        let stats = vec![StatReport {
            candidate: "reference-xorshift128plus".into(),
            sample_path: "x".into(),
            sample_len_bytes: 100,
            tier1: stats::tier1::run_tier1(&[1, 2, 3, 4, 5, 6, 7, 8]),
            sp800_90b_iid: None,
            sp800_90b_non_iid: None,
            sp800_22: None,
            ent: None,
        }];

        let table = build_comparison_table(&stats, &BenchReport::default(), &[], None);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].candidate, "reference-xorshift128plus");
        assert!(table.rows[0].tier1_pass.is_some());
        assert!(table.rows[0].throughput_bytes_per_sec.is_none());
    }

    #[test]
    fn overall_pass_ignores_tracks_that_never_ran() {
        let row = ComparisonRow {
            candidate: "c".into(),
            tier1_pass: Some(true),
            deterministic: None, // differential wasn't run for this candidate
            api_parity_pass: Some(true),
            ..Default::default()
        };
        assert_eq!(row.overall_pass(), Some(true));
    }

    #[test]
    fn overall_pass_is_false_if_any_ran_gate_failed() {
        let row = ComparisonRow {
            candidate: "c".into(),
            tier1_pass: Some(true),
            deterministic: Some(false),
            api_parity_pass: Some(true),
            ..Default::default()
        };
        assert_eq!(row.overall_pass(), Some(false));
    }

    #[test]
    fn overall_pass_is_none_when_no_gate_ran_at_all() {
        // Regression guard: a row with zero real data (e.g. a stray
        // candidate name that only ever showed up in stale
        // `target/criterion` bench output, never in any of this
        // crate's own gates) must not render as a silent pass.
        let row = ComparisonRow {
            candidate: "qpp-rng-reference-direct-jitter/16B".into(),
            ..Default::default()
        };
        assert_eq!(row.overall_pass(), None);
    }
}
