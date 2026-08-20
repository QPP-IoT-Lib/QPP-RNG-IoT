//! Tier 2: shells out to the trusted external reference tools this
//! whole harness exists to defer to (see `qpp-rng-testing-architecture.md`
//! §5.1: *"don't reimplement the min-entropy estimators, they're
//! intricate and the whole point is comparing against the trusted
//! reference tooling"*):
//!
//! - the NIST SP 800-90B `SP800-90B_EntropyAssessment` reference tool
//!   (`ea_iid` / `ea_non_iid`),
//! - the NIST SP 800-22 Statistical Test Suite (`assess`), and
//! - Fourmilab `ent`.
//!
//! ## Honesty about what's verified here
//!
//! None of these three tools is installed in the environment this crate
//! was developed in, so **the output parsers below are best-effort,
//! written against each tool's documented/commonly-observed output
//! format, not verified against a real run**. `ent`'s plain-text report
//! format is stable and well-documented enough that [`parse_ent_output`]
//! should be treated as reliable; the NIST tools' exact wording varies
//! more across versions, so every parsed result also carries
//! [`ToolRun::raw_metrics`] -- every `label: number` / `label = number`
//! line the tool printed, regardless of whether this module recognized
//! the label -- specifically so a wrong guess about an exact field name
//! degrades to "the number is still there under its real key" instead of
//! silently losing data. **Before trusting a parsed field in a real
//! report, diff it against a real tool run.**
//!
//! ## A missing tool is not an error
//!
//! [`find_tool`] returning `None` produces a [`ToolRun`] with
//! `tool_path: None` and no parsed result -- not an `Err`. A tool being
//! unavailable on this machine is an expected, reportable state (the
//! `report` crate renders it as "N/A"), not a harness failure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// The outcome of attempting to run one external tool: whether it was
/// found, what it printed, and (best-effort) what was parsed out of
/// that.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRun<T> {
    pub tool_path: Option<PathBuf>,
    pub raw_stdout: String,
    pub raw_stderr: String,
    pub exit_success: bool,
    /// Every `label: number` / `label = number` line found in stdout,
    /// keyed by the trimmed label exactly as printed. See the module
    /// doc's "Honesty about what's verified here" note.
    pub raw_metrics: BTreeMap<String, f64>,
    pub parsed: Option<T>,
}

impl<T> ToolRun<T> {
    fn missing() -> Self {
        Self {
            tool_path: None,
            raw_stdout: String::new(),
            raw_stderr: String::new(),
            exit_success: false,
            raw_metrics: BTreeMap::new(),
            parsed: None,
        }
    }
}

/// Searches `PATH` for the first name in `candidate_names` that resolves
/// to an executable file. Deliberately not the `which` crate -- this is
/// a dozen lines and it's the only thing this module needs from it.
pub fn find_tool(candidate_names: &[&str]) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for name in candidate_names {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// Scans `text` line by line for `label: number` or `label = number`
/// patterns and collects them into a map. Units/trailing text after the
/// number (e.g. `"47.32 percent"`) are ignored -- only the first
/// parseable float on the line, after the separator, is kept.
fn extract_key_value_pairs(text: &str) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        for sep in [':', '='] {
            if let Some((label, rest)) = line.split_once(sep) {
                let label = label.trim();
                if label.is_empty() || label.len() > 80 {
                    continue;
                }
                if let Some(value) = first_float_token(rest) {
                    out.insert(label.to_string(), value);
                }
                break;
            }
        }
    }
    out
}

/// Scans `rest` for the first substring matching `-?\d+(\.\d+)?` and
/// parses it, ignoring every other character around it (units,
/// parentheses, trailing punctuation, ...).
///
/// A naive "split on whitespace/`,`/`(` then strip non-numeric
/// characters" approach breaks on tokens like `"0.0)."` (from ent's
/// `"...uncorrelated = 0.0)."`) -- stripping non-numeric characters
/// leaves `"0.0."`, which fails to parse. Scanning for a numeric run
/// directly sidesteps needing to enumerate every punctuation character
/// that might border a number in some tool's output.
fn first_float_token(rest: &str) -> Option<f64> {
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let is_number_start =
            chars[i].is_ascii_digit() || (chars[i] == '-' && chars.get(i + 1).is_some_and(char::is_ascii_digit));
        if !is_number_start {
            i += 1;
            continue;
        }
        let start = i;
        if chars[i] == '-' {
            i += 1;
        }
        while i < chars.len() && chars[i].is_ascii_digit() {
            i += 1;
        }
        if chars.get(i) == Some(&'.') && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
            i += 1;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
        }
        let token: String = chars[start..i].iter().collect();
        if let Ok(v) = token.parse::<f64>() {
            return Some(v);
        }
    }
    None
}

fn run_command_capture(program: &Path, args: &[&std::ffi::OsStr]) -> anyhow::Result<(bool, String, String)> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to spawn {}", program.display()))?;
    Ok((
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

// ---------------------------------------------------------------------
// ent (Fourmilab)
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EntResult {
    pub entropy_bits_per_byte: f64,
    pub chi_square: f64,
    pub chi_square_exceed_percent: f64,
    pub arithmetic_mean: f64,
    pub monte_carlo_pi: f64,
    pub monte_carlo_pi_error_percent: f64,
    pub serial_correlation: f64,
}

/// Runs `ent <sample_path>` and parses its plain-text report.
pub fn run_ent(sample_path: &Path) -> anyhow::Result<ToolRun<EntResult>> {
    let Some(tool_path) = find_tool(&["ent"]) else {
        return Ok(ToolRun::missing());
    };
    let (exit_success, raw_stdout, raw_stderr) =
        run_command_capture(&tool_path, &[sample_path.as_os_str()])?;
    let raw_metrics = extract_key_value_pairs(&raw_stdout);
    let parsed = parse_ent_output(&raw_stdout);
    Ok(ToolRun {
        tool_path: Some(tool_path),
        raw_stdout,
        raw_stderr,
        exit_success,
        raw_metrics,
        parsed,
    })
}

/// Parses `ent`'s standard (non-`-t`) report format:
///
/// ```text
/// Entropy = 7.999826 bits per byte.
///
/// Chi square distribution for 1048576 samples is 254.91, and randomly
/// would exceed this value 47.32 percent of the times.
///
/// Arithmetic mean value of data bytes is 127.5108 (127.5 = random).
/// Monte Carlo value for Pi is 3.140501258 (error 0.03 percent).
/// Serial correlation coefficient is 0.000151 (totally uncorrelated = 0.0).
/// ```
pub fn parse_ent_output(stdout: &str) -> Option<EntResult> {
    let entropy_bits_per_byte = find_float_after(stdout, "Entropy = ")?;
    // Not "distribution for": that marker is immediately followed by
    // the *sample count* ("...distribution for 1048576 samples is
    // 254.91..."), which first_float_token would greedily match before
    // ever reaching the chi-square statistic itself.
    let chi_square = find_float_after(stdout, "samples is ")?;
    let chi_square_exceed_percent = find_float_after(stdout, "exceed this value ")?;
    let arithmetic_mean = find_float_after(stdout, "data bytes is ")?;
    let monte_carlo_pi = find_float_after(stdout, "value for Pi is ")?;
    let monte_carlo_pi_error_percent = find_float_after(stdout, "(error ")?;
    let serial_correlation = find_float_after(stdout, "coefficient is ")?;

    Some(EntResult {
        entropy_bits_per_byte,
        chi_square,
        chi_square_exceed_percent,
        arithmetic_mean,
        monte_carlo_pi,
        monte_carlo_pi_error_percent,
        serial_correlation,
    })
}

fn find_float_after(text: &str, marker: &str) -> Option<f64> {
    let idx = text.find(marker)?;
    first_float_token(&text[idx + marker.len()..])
}

// ---------------------------------------------------------------------
// NIST SP 800-90B reference tool (ea_iid / ea_non_iid)
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Sp80090bTrack {
    Iid,
    NonIid,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Sp80090bResult {
    pub track: Sp80090bTrack,
    /// Overall assessed min-entropy, bits per symbol -- the tools'
    /// final `min(...)` line under either track. `None` if the expected
    /// marker text wasn't found (see module doc); check `raw_metrics`
    /// on the enclosing [`ToolRun`] in that case.
    pub min_entropy_bits_per_symbol: Option<f64>,
    pub h_original: Option<f64>,
    pub h_bitstring: Option<f64>,
}

/// Runs the SP 800-90B reference tool's IID or non-IID track over
/// `sample_path`.
///
/// `bits_per_symbol` is the tool's `<bits per symbol>` CLI argument --
/// `8` for this workspace's byte-oriented candidates. Looks for `ea_iid`
/// / `ea_non_iid` on `PATH` (the names the
/// `usnistgov/SP800-90B_EntropyAssessment` build produces); if your
/// build installs them under different names, extend the candidate list
/// below.
pub fn run_sp800_90b(
    sample_path: &Path,
    bits_per_symbol: u8,
    track: Sp80090bTrack,
) -> anyhow::Result<ToolRun<Sp80090bResult>> {
    let candidate_names: &[&str] = match track {
        Sp80090bTrack::Iid => &["ea_iid"],
        Sp80090bTrack::NonIid => &["ea_non_iid"],
    };
    let Some(tool_path) = find_tool(candidate_names) else {
        return Ok(ToolRun::missing());
    };

    let bits_arg = bits_per_symbol.to_string();
    let (exit_success, raw_stdout, raw_stderr) = run_command_capture(
        &tool_path,
        &[sample_path.as_os_str(), std::ffi::OsStr::new(&bits_arg)],
    )?;
    let raw_metrics = extract_key_value_pairs(&raw_stdout);

    let parsed = Some(Sp80090bResult {
        track,
        min_entropy_bits_per_symbol: raw_metrics
            .iter()
            .filter(|(k, _)| k.starts_with("min("))
            .map(|(_, v)| *v)
            .next_back(),
        h_original: raw_metrics.get("H_original").copied(),
        h_bitstring: raw_metrics.get("H_bitstring").copied(),
    });

    Ok(ToolRun {
        tool_path: Some(tool_path),
        raw_stdout,
        raw_stderr,
        exit_success,
        raw_metrics,
        parsed,
    })
}

// ---------------------------------------------------------------------
// NIST SP 800-22 Statistical Test Suite (`assess`)
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Sp80022Result {
    pub tests_passed: usize,
    pub tests_total: usize,
    pub pass_rate: f32,
}

/// The 15 per-test subdirectories STS 2.1.2's `openOutputStreams`
/// (`src/utilities.c`) opens `stats.txt`/`results.txt` in with plain
/// `fopen(path, "w")` -- which fails outright ("THE OUTPUT DIRECTORY
/// DOES NOT EXIST") if the directory isn't already there. The shipped
/// `sts-2.1.2/experiments/AlgorithmTesting/` tree comes with these
/// pre-created; a fresh `work_dir` does not, so [`run_sp800_22`] has to
/// recreate this exact skeleton itself before invoking `assess`. Names
/// taken directly from the shipped tree, not guessed.
const STS_TEST_DIRS: &[&str] = &[
    "ApproximateEntropy",
    "BlockFrequency",
    "CumulativeSums",
    "FFT",
    "Frequency",
    "LinearComplexity",
    "LongestRun",
    "NonOverlappingTemplate",
    "OverlappingTemplate",
    "RandomExcursions",
    "RandomExcursionsVariant",
    "Rank",
    "Runs",
    "Serial",
    "Universal",
];

/// Runs the STS reference implementation's `assess` binary over
/// `sample_path` and parses `finalAnalysisReport.txt` from its working
/// directory.
///
/// `assess` is menu-driven over stdin rather than flag-driven. The
/// prompt sequence below was pinned down by reading STS 2.1.2's actual
/// source (`src/utilities.c`'s `generatorOptions` ->
/// `chooseTests` -> `fixParameters` -> `openOutputStreams` ->
/// `invokeTestSuite`/`fileBasedBitStreams` call chain) and confirmed
/// against a real build, not guessed: **six** answers, not four --
/// `fixParameters` inserts a "Select Test (0 to continue)" parameter-
/// customization prompt right after "apply all tests" (answer `0` to
/// decline customizing any test's block length), and
/// `fileBasedBitStreams` asks for the input encoding (ASCII `0`/`1`
/// text vs. raw binary) right at the end, after "how many bitstreams".
/// Getting this wrong doesn't error out -- `assess` just blocks forever
/// on the next unanswered prompt, which looks identical to "still
/// computing" from the outside (this cost real debugging time to
/// notice: the process kept accumulating a little CPU forever without
/// producing output).
///
/// `sample_path`'s bytes must be raw binary (this workspace's
/// `stats::sample` writes exactly that), matching the final "Binary"
/// mode answer below -- an ASCII `0`/`1`-text sample would need mode
/// `0` instead.
///
/// The report STS writes to
/// `experiments/AlgorithmTesting/finalAnalysisReport.txt` is read from
/// `work_dir` (the directory `assess` is invoked from; STS resolves its
/// `experiments/` output relative to CWD) -- see [`STS_TEST_DIRS`] for
/// why this function creates that whole subtree first rather than just
/// `work_dir` itself.
///
/// One more confirmed-against-a-real-build quirk: `assess` exits with
/// status `1` even on a fully successful run (its `main` just doesn't
/// return `0`) -- so [`ToolRun::exit_success`] being `false` here is
/// *not* a signal anything went wrong. [`Sp80022Result::pass_rate`]
/// parsing successfully out of a real `finalAnalysisReport.txt` is the
/// actual success signal, which is exactly what
/// [`StatReport::overall_pass`](crate::report::StatReport::overall_pass)
/// already keys off (`parsed.is_none()`, not `exit_success`) -- this
/// note exists so a future reader doesn't "fix" that by making it check
/// `exit_success` too.
pub fn run_sp800_22(
    sample_path: &Path,
    bitstream_len_bits: usize,
    work_dir: &Path,
) -> anyhow::Result<ToolRun<Sp80022Result>> {
    let Some(tool_path) = find_tool(&["assess"]) else {
        return Ok(ToolRun::missing());
    };

    std::fs::create_dir_all(work_dir)?;
    for dir in STS_TEST_DIRS {
        std::fs::create_dir_all(work_dir.join("experiments/AlgorithmTesting").join(dir))?;
    }

    // assess resolves the "User Prescribed Input File" path relative to
    // *its own* CWD -- which `.current_dir(work_dir)` below deliberately
    // points at `work_dir`, not wherever this process's CWD happens to
    // be. A relative `sample_path` would therefore resolve against the
    // wrong directory, so canonicalize it first.
    //
    // Separately, and independent of the above: assess reads the path
    // with C's `scanf("%s", file)`, which stops at the first whitespace
    // -- there is no way to pass it a path containing a space at all,
    // quoting included, short of patching assess's own source. A path
    // under a directory like ".../Undergrad Thesis/..." gets silently
    // truncated at the space, so assess opens a nonexistent path and
    // fails with its own "File Error" message rather than anything this
    // crate raises. Sidestep the whole limitation by always working
    // from a copy in `std::env::temp_dir()` (guaranteed space-free on
    // every platform this crate targets), regardless of whether the
    // real sample path happens to contain one.
    let sample_path = sample_path
        .canonicalize()
        .with_context(|| format!("canonicalizing {}", sample_path.display()))?;
    let temp_sample_path = std::env::temp_dir().join(
        sample_path
            .file_name()
            .context("sample path has no file name")?,
    );
    std::fs::copy(&sample_path, &temp_sample_path).with_context(|| {
        format!(
            "copying {} to space-free temp path {}",
            sample_path.display(),
            temp_sample_path.display()
        )
    })?;
    let sample_path = temp_sample_path;

    // Verified six-answer `assess` interactive flow -- see this
    // function's doc for how each line was confirmed against real
    // source/output:
    //   [0] Input File -> path -> apply all tests (1) ->
    //   skip parameter customization (0) -> 1 bitstream -> Binary (1)
    let stdin_script = format!(
        "0\n{}\n1\n0\n1\n1\n",
        sample_path
            .to_str()
            .context("sample path is not valid UTF-8")?
    );

    let mut child = Command::new(&tool_path)
        .arg(bitstream_len_bits.to_string())
        .current_dir(work_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn {}", tool_path.display()))?;

    {
        use std::io::Write;
        let stdin = child
            .stdin
            .as_mut()
            .context("failed to open assess stdin")?;
        stdin.write_all(stdin_script.as_bytes())?;
    }

    let output = child.wait_with_output()?;
    let raw_stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let raw_stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let raw_metrics = extract_key_value_pairs(&raw_stdout);

    let report_path = work_dir.join("experiments/AlgorithmTesting/finalAnalysisReport.txt");
    let parsed = std::fs::read_to_string(&report_path)
        .ok()
        .as_deref()
        .and_then(parse_sts_final_report);

    Ok(ToolRun {
        tool_path: Some(tool_path),
        raw_stdout,
        raw_stderr,
        exit_success: output.status.success(),
        raw_metrics,
        parsed,
    })
}

/// Parses STS's `finalAnalysisReport.txt` summary table, which ends
/// each per-test row with a `<passed>/<total>` proportion column (e.g.
/// `... 100/100 * FREQUENCY`). Sums those across every row to get an
/// overall pass rate.
fn parse_sts_final_report(report: &str) -> Option<Sp80022Result> {
    let mut passed_total = 0usize;
    let mut tests_total = 0usize;
    let mut any_row = false;

    for line in report.lines() {
        for token in line.split_whitespace() {
            if let Some((p, t)) = token.split_once('/')
                && let (Ok(p), Ok(t)) = (p.parse::<usize>(), t.parse::<usize>())
                    && t > 0 && p <= t {
                        passed_total += p;
                        tests_total += t;
                        any_row = true;
                    }
        }
    }

    if !any_row {
        return None;
    }

    Some(Sp80022Result {
        tests_passed: passed_total,
        tests_total,
        pass_rate: passed_total as f32 / tests_total as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_ENT_OUTPUT: &str = "\
Entropy = 7.999826 bits per byte.

Optimum compression would reduce the size
of this 1048576 byte file by 0 percent.

Chi square distribution for 1048576 samples is 254.91, and randomly
would exceed this value 47.32 percent of the times.

Arithmetic mean value of data bytes is 127.5108 (127.5 = random).
Monte Carlo value for Pi is 3.140501258 (error 0.03 percent).
Serial correlation coefficient is 0.000151 (totally uncorrelated = 0.0).
";

    #[test]
    fn parses_a_representative_ent_report() {
        let parsed = parse_ent_output(SAMPLE_ENT_OUTPUT).expect("should parse");
        assert!((parsed.entropy_bits_per_byte - 7.999826).abs() < 1e-6);
        assert!((parsed.chi_square - 254.91).abs() < 1e-6);
        assert!((parsed.chi_square_exceed_percent - 47.32).abs() < 1e-6);
        assert!((parsed.arithmetic_mean - 127.5108).abs() < 1e-6);
        assert!((parsed.monte_carlo_pi - 3.140501258).abs() < 1e-6);
        assert!((parsed.monte_carlo_pi_error_percent - 0.03).abs() < 1e-6);
        assert!((parsed.serial_correlation - 0.000151).abs() < 1e-9);
    }

    #[test]
    fn extract_key_value_pairs_ignores_units_and_keeps_first_number() {
        let map = extract_key_value_pairs(SAMPLE_ENT_OUTPUT);
        assert!((map["Entropy"] - 7.999826).abs() < 1e-6);
        assert!((map["Serial correlation coefficient is 0.000151 (totally uncorrelated"] - 0.0)
            .abs()
            < 1e-9);
    }

    #[test]
    fn find_tool_returns_none_for_a_name_that_cannot_exist() {
        assert!(find_tool(&["definitely-not-a-real-binary-name-xyz"]).is_none());
    }

    #[test]
    fn missing_tool_produces_ok_not_err() {
        // find_tool for "ent" may or may not exist in the sandbox this
        // is run in; either way run_ent must not error out.
        let result = run_ent(Path::new("/nonexistent/sample.bin"));
        assert!(result.is_ok());
    }

    #[test]
    fn parse_sts_final_report_sums_pass_fractions() {
        let report = "\
------------------------------------------------------------------------------
RESULTS FOR THE UNIFORMITY OF P-VALUES AND THE PROPORTION OF PASSING SEQUENCES
------------------------------------------------------------------------------
  C1  C2  C3  C4  C5  C6  C7  C8  C9 C10  P-VALUE  PROPORTION  STATISTICAL TEST
------------------------------------------------------------------------------
   1   1   0   1   1   1   0   1   1   1  0.534146     10/10   Frequency
   1   0   1   1   1   0   1   1   1   1  0.213309     10/10   BlockFrequency
";
        let parsed = parse_sts_final_report(report).expect("should parse");
        assert_eq!(parsed.tests_passed, 20);
        assert_eq!(parsed.tests_total, 20);
        assert!((parsed.pass_rate - 1.0).abs() < 1e-6);
    }
}
