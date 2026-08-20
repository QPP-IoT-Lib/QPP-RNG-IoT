//! CSV exporter for [`ComparisonTable`]. Hand-rolled rather than a
//! `csv` crate dependency: RFC 4180 escaping is a handful of lines, and
//! every field this table ever produces is a plain number, `true`/
//! `false`, or a `[a-z0-9-]+` candidate name -- there's no real
//! quoting-edge-case surface here to justify the dependency.

use crate::table::ComparisonTable;

const HEADERS: &[&str] = &[
    "candidate",
    "tier1_pass",
    "min_entropy_bits",
    "sp800_22_pass_rate",
    "ent_entropy_bits_per_byte",
    "shannon_entropy_bits_per_byte",
    "throughput_bytes_per_sec",
    "latency_per_call_ns",
    "time_to_first_byte_ns",
    "text_bytes",
    "stack_high_water_mark_bytes",
    "ticks_per_output_byte",
    "deterministic",
    "api_parity_pass",
    "overall_pass",
];

pub fn to_csv(table: &ComparisonTable) -> String {
    let mut out = String::new();
    out.push_str(&HEADERS.join(","));
    out.push('\n');

    for row in &table.rows {
        let fields = [
            csv_escape(&row.candidate),
            opt_str(row.tier1_pass.map(|b| b.to_string())),
            opt_str(row.min_entropy_bits.map(|v| v.to_string())),
            opt_str(row.sp800_22_pass_rate.map(|v| v.to_string())),
            opt_str(row.ent_entropy_bits_per_byte.map(|v| v.to_string())),
            opt_str(row.shannon_entropy_bits_per_byte.map(|v| v.to_string())),
            opt_str(row.throughput_bytes_per_sec.map(|v| v.to_string())),
            opt_str(row.latency_per_call_ns.map(|v| v.to_string())),
            opt_str(row.time_to_first_byte_ns.map(|v| v.to_string())),
            opt_str(row.text_bytes.map(|v| v.to_string())),
            opt_str(row.stack_high_water_mark_bytes.map(|v| v.to_string())),
            opt_str(row.ticks_per_output_byte.map(|v| v.to_string())),
            opt_str(row.deterministic.map(|b| b.to_string())),
            opt_str(row.api_parity_pass.map(|b| b.to_string())),
            row.overall_pass().to_string(),
        ];
        out.push_str(&fields.join(","));
        out.push('\n');
    }

    out
}

fn opt_str(value: Option<String>) -> String {
    value.unwrap_or_default()
}

/// RFC 4180 field escaping: wrap in quotes (doubling any embedded
/// quotes) whenever the field contains a comma, quote, or newline.
fn csv_escape(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::ComparisonRow;

    #[test]
    fn renders_header_and_one_row_with_blank_missing_fields() {
        let table = ComparisonTable {
            rows: vec![ComparisonRow {
                candidate: "reference-xorshift128plus".into(),
                tier1_pass: Some(true),
                ..Default::default()
            }],
        };
        let csv = to_csv(&table);
        let mut lines = csv.lines();
        assert_eq!(lines.next().unwrap(), HEADERS.join(","));
        let data_line = lines.next().unwrap();
        assert!(data_line.starts_with("reference-xorshift128plus,true,"));
        // min_entropy_bits (3rd column) was never set -> blank field.
        assert!(data_line.contains(",true,,"));
    }

    #[test]
    fn escapes_fields_containing_commas_and_quotes() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("a\"b"), "\"a\"\"b\"");
    }
}
