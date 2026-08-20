//! Markdown report generator: renders a [`ComparisonTable`] as one
//! GitHub-flavored Markdown table, "original vs. variant X" at a glance
//! per `qpp-rng-testing-architecture.md` §5.5.

use crate::table::ComparisonTable;

const COLUMNS: &[&str] = &[
    "Candidate",
    "Tier1",
    "MinEntropy (bits)",
    "SP800-22",
    "ENT (bits/B)",
    "Shannon (bits/B)",
    "Throughput (B/s)",
    "Latency/call",
    "TTFB",
    ".text (bytes)",
    "Stack HWM (bytes)",
    "Ticks/byte",
    "Deterministic",
    "API parity",
    "Overall",
];

pub fn to_markdown(table: &ComparisonTable) -> String {
    let mut out = String::new();
    out.push_str("| ");
    out.push_str(&COLUMNS.join(" | "));
    out.push_str(" |\n");
    out.push('|');
    for _ in COLUMNS {
        out.push_str(" --- |");
    }
    out.push('\n');

    for row in &table.rows {
        let cells = [
            row.candidate.clone(),
            fmt_bool(row.tier1_pass),
            fmt_opt(row.min_entropy_bits, |v| format!("{v:.3}")),
            fmt_opt(row.sp800_22_pass_rate, |v| format!("{:.1}%", v * 100.0)),
            fmt_opt(row.ent_entropy_bits_per_byte, |v| format!("{v:.4}")),
            fmt_opt(row.shannon_entropy_bits_per_byte, |v| format!("{v:.4}")),
            fmt_opt(row.throughput_bytes_per_sec, |v| format!("{v:.0}")),
            fmt_opt(row.latency_per_call_ns, |v| format!("{v:.1} ns")),
            fmt_opt(row.time_to_first_byte_ns, |v| format!("{v:.1} ns")),
            fmt_opt(row.text_bytes, |v| v.to_string()),
            fmt_opt(row.stack_high_water_mark_bytes, |v| v.to_string()),
            fmt_opt(row.ticks_per_output_byte, |v| format!("{v:.2}")),
            fmt_bool(row.deterministic),
            fmt_bool(row.api_parity_pass),
            fmt_bool(row.overall_pass()),
        ];
        out.push_str("| ");
        out.push_str(&cells.join(" | "));
        out.push_str(" |\n");
    }

    out
}

fn fmt_opt<T>(value: Option<T>, f: impl FnOnce(T) -> String) -> String {
    value.map(f).unwrap_or_else(|| "—".to_string())
}

fn fmt_bool(value: Option<bool>) -> String {
    match value {
        Some(true) => "✅".to_string(),
        Some(false) => "❌".to_string(),
        None => "—".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::ComparisonRow;

    #[test]
    fn renders_a_header_row_and_one_data_row() {
        let table = ComparisonTable {
            rows: vec![ComparisonRow {
                candidate: "reference-xorshift128plus".into(),
                tier1_pass: Some(true),
                min_entropy_bits: Some(7.91),
                deterministic: Some(true),
                api_parity_pass: Some(true),
                ..Default::default()
            }],
        };
        let md = to_markdown(&table);
        assert!(md.starts_with("| Candidate |"));
        assert!(md.contains("reference-xorshift128plus"));
        assert!(md.contains("7.910"));
        assert!(md.contains("✅"));
        assert!(md.contains("—")); // fields left None render as an em dash
    }

    #[test]
    fn empty_table_still_renders_a_header() {
        let md = to_markdown(&ComparisonTable::default());
        assert!(md.starts_with("| Candidate |"));
        assert_eq!(md.lines().count(), 2); // header + separator, no data rows
    }
}
