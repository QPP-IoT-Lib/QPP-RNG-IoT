//! Folds [`crate::size`], [`crate::stack`], and [`crate::cycles`] into
//! one [`FootprintReport`] per candidate -- the "Footprint result
//! aggregator" from the test-harness breakdown.

use serde::{Deserialize, Serialize};

use crate::cycles::CycleCountReport;
use crate::size::{BloatReport, SizeReport};
use crate::stack::CallStackReport;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FootprintReport {
    pub candidate: String,
    pub target_triple: Option<String>,
    pub size: SizeReport,
    pub bloat: BloatReport,
    pub call_stack: CallStackReport,
    /// `None` if a cycle-count measurement wasn't requested/available
    /// for this run (e.g. no built binary to measure against on this
    /// target rung yet).
    pub cycles: Option<CycleCountReport>,
}

impl FootprintReport {
    /// A `.text`-size figure to headline in reports: prefers `cargo
    /// size`'s number (directly from the linked binary) and falls back
    /// to `cargo bloat`'s per-crate figure (a narrower "this crate's
    /// share of the binary" number) when `size` didn't run/parse.
    pub fn headline_text_bytes(&self) -> Option<u64> {
        self.size.text_bytes.or(self.bloat.crate_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprint_report_round_trips_through_json() {
        let report = FootprintReport {
            candidate: "reference-xorshift128plus".into(),
            target_triple: Some("x86_64-unknown-linux-gnu".into()),
            size: SizeReport {
                tool_path: None,
                text_bytes: Some(1234),
                data_bytes: Some(56),
                bss_bytes: Some(7),
                raw_stdout: String::new(),
            },
            bloat: BloatReport::default(),
            call_stack: CallStackReport::default(),
            cycles: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let back: FootprintReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.headline_text_bytes(), Some(1234));
    }

    #[test]
    fn headline_text_bytes_falls_back_to_bloat() {
        let report = FootprintReport {
            candidate: "c".into(),
            target_triple: None,
            size: SizeReport::default(),
            bloat: BloatReport {
                tool_path: None,
                crate_bytes: Some(999),
                raw_stdout: String::new(),
            },
            call_stack: CallStackReport::default(),
            cycles: None,
        };
        assert_eq!(report.headline_text_bytes(), Some(999));
    }
}
