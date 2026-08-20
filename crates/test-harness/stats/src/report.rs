//! Folds Tier 1's native metrics and Tier 2's external-tool runs into
//! one [`StatReport`] per candidate -- the "Result parser/normalizer"
//! this crate owns per the test-harness breakdown.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::tier1::{self, Tier1Report};
use crate::tier2::{self, EntResult, Sp80022Result, Sp80090bResult, Sp80090bTrack, ToolRun};

/// The full statistical picture for one candidate's sample: Tier 1's
/// always-available native metrics, plus whichever Tier 2 tools were
/// actually found on this machine (each `None` only if the sample bytes
/// were never handed to that stage at all -- a *found-but-unparsed* tool
/// run is still `Some`, see [`tier2::ToolRun`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatReport {
    pub candidate: String,
    pub sample_path: String,
    pub sample_len_bytes: usize,
    pub tier1: Tier1Report,
    pub sp800_90b_iid: Option<ToolRun<Sp80090bResult>>,
    pub sp800_90b_non_iid: Option<ToolRun<Sp80090bResult>>,
    pub sp800_22: Option<ToolRun<Sp80022Result>>,
    pub ent: Option<ToolRun<EntResult>>,
}

impl StatReport {
    /// The single most conservative min-entropy estimate available:
    /// SP 800-90B's non-IID track if it ran and parsed, else the IID
    /// track, else `None` if neither tool was available/parseable.
    /// Non-IID is preferred when both exist because it makes no
    /// independence assumption -- see `qpp-rng-testing-architecture.md`
    /// §2's row on the non-IID track being the IID assumption's
    /// fallback.
    pub fn min_entropy_estimate(&self) -> Option<f64> {
        self.sp800_90b_non_iid
            .as_ref()
            .and_then(|t| t.parsed.as_ref())
            .and_then(|p| p.min_entropy_bits_per_symbol)
            .or_else(|| {
                self.sp800_90b_iid
                    .as_ref()
                    .and_then(|t| t.parsed.as_ref())
                    .and_then(|p| p.min_entropy_bits_per_symbol)
            })
    }

    pub fn sp800_22_pass_rate(&self) -> Option<f32> {
        self.sp800_22
            .as_ref()
            .and_then(|t| t.parsed.as_ref())
            .map(|p| p.pass_rate)
    }

    pub fn ent_entropy_bits_per_byte(&self) -> Option<f64> {
        self.ent
            .as_ref()
            .and_then(|t| t.parsed.as_ref())
            .map(|p| p.entropy_bits_per_byte)
    }

    /// Coarse overall verdict: Tier 1 passing is required; any Tier 2
    /// tool that actually ran is also required to have parsed a result
    /// (a tool that ran but this crate failed to parse is deliberately
    /// treated as "can't confirm pass", not silently ignored).
    pub fn overall_pass(&self) -> bool {
        if !self.tier1.all_passed() {
            return false;
        }
        fn ran_but_unparsed<T>(run: &Option<ToolRun<T>>) -> bool {
            run.as_ref()
                .is_some_and(|r| r.tool_path.is_some() && r.parsed.is_none())
        }
        !ran_but_unparsed(&self.sp800_90b_iid)
            && !ran_but_unparsed(&self.sp800_90b_non_iid)
            && !ran_but_unparsed(&self.sp800_22)
            && !ran_but_unparsed(&self.ent)
    }
}

/// Options controlling which Tier 2 tools [`run_full_battery`]
/// attempts. Every field defaults to `true` in [`Tier2Options::all`];
/// turn individual tools off for a faster/partial run (e.g. CI without
/// `ent` installed).
#[derive(Debug, Clone, Copy)]
pub struct Tier2Options {
    pub sp800_90b_iid: bool,
    pub sp800_90b_non_iid: bool,
    pub sp800_22: bool,
    pub ent: bool,
    /// Bits per symbol handed to the SP 800-90B tool; `8` for this
    /// workspace's byte-oriented candidates.
    pub bits_per_symbol: u8,
    /// Bitstream length (bits) handed to `assess`.
    pub sts_bitstream_len_bits: usize,
}

impl Tier2Options {
    pub fn all() -> Self {
        Self {
            sp800_90b_iid: true,
            sp800_90b_non_iid: true,
            sp800_22: true,
            ent: true,
            bits_per_symbol: 8,
            sts_bitstream_len_bits: 1_000_000,
        }
    }

    pub fn none() -> Self {
        Self {
            sp800_90b_iid: false,
            sp800_90b_non_iid: false,
            sp800_22: false,
            ent: false,
            ..Self::all()
        }
    }
}

/// Runs Tier 1 (always) and every enabled Tier 2 tool (best-effort --
/// see [`crate::tier2`]'s module doc) over `sample_path`, folding the
/// results into one [`StatReport`].
pub fn run_full_battery(
    candidate: &str,
    sample_path: &Path,
    tier2_opts: Tier2Options,
    sts_work_dir: &Path,
) -> anyhow::Result<StatReport> {
    let bytes = std::fs::read(sample_path)?;
    let tier1_report = tier1::run_tier1(&bytes);

    let sp800_90b_iid = tier2_opts
        .sp800_90b_iid
        .then(|| tier2::run_sp800_90b(sample_path, tier2_opts.bits_per_symbol, Sp80090bTrack::Iid))
        .transpose()?;
    let sp800_90b_non_iid = tier2_opts
        .sp800_90b_non_iid
        .then(|| {
            tier2::run_sp800_90b(sample_path, tier2_opts.bits_per_symbol, Sp80090bTrack::NonIid)
        })
        .transpose()?;
    let sp800_22 = tier2_opts
        .sp800_22
        .then(|| tier2::run_sp800_22(sample_path, tier2_opts.sts_bitstream_len_bits, sts_work_dir))
        .transpose()?;
    let ent = tier2_opts.ent.then(|| tier2::run_ent(sample_path)).transpose()?;

    Ok(StatReport {
        candidate: candidate.to_string(),
        sample_path: sample_path.display().to_string(),
        sample_len_bytes: bytes.len(),
        tier1: tier1_report,
        sp800_90b_iid,
        sp800_90b_non_iid,
        sp800_22,
        ent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_full_battery_with_tier2_disabled_only_runs_tier1() {
        let dir = std::env::temp_dir().join(format!("qpp-rng-stats-report-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sample_path = dir.join("sample.bin");
        std::fs::write(&sample_path, vec![7u8; 5000]).unwrap();

        let report =
            run_full_battery("test-candidate", &sample_path, Tier2Options::none(), &dir).unwrap();

        assert_eq!(report.candidate, "test-candidate");
        assert_eq!(report.sample_len_bytes, 5000);
        assert!(report.sp800_90b_iid.is_none());
        assert!(report.sp800_90b_non_iid.is_none());
        assert!(report.sp800_22.is_none());
        assert!(report.ent.is_none());
        assert!(report.min_entropy_estimate().is_none());
        // Constant-byte input fails Tier 1 -> overall_pass must be false.
        assert!(!report.overall_pass());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stat_report_round_trips_through_json() {
        let report = StatReport {
            candidate: "c".into(),
            sample_path: "p".into(),
            sample_len_bytes: 10,
            tier1: tier1::run_tier1(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
            sp800_90b_iid: None,
            sp800_90b_non_iid: None,
            sp800_22: None,
            ent: None,
        };
        let json = serde_json::to_string(&report).unwrap();
        let back: StatReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.candidate, "c");
        assert_eq!(back.sample_len_bytes, 10);
    }
}
