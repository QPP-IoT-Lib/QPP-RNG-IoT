//! Code/flash size probes: wraps `cargo size` (from `cargo-binutils`,
//! backed by GNU/LLVM `size`) and `cargo bloat --crates`.
//!
//! ## Honesty about what's verified here
//!
//! Same caveat as `stats::tier2`: neither tool is installed in the
//! environment this crate was developed in. [`run_cargo_size`]'s parser
//! targets `size`'s stable, long-standing Berkeley-format columns
//! (`text data bss dec hex filename`) and should be reliable; `cargo
//! bloat`'s `--crates` table format is less formally specified, so
//! [`run_cargo_bloat`] is a best-effort parse -- verify it against a
//! real installed `cargo-bloat` before trusting it unattended.
//!
//! ## Both tools need a *binary*, not a library
//!
//! `qpp-rng-reference`/`qpp-rng-iot` are library crates with no `[[bin]]`
//! target -- there is nothing for `size`/`cargo bloat` to measure until
//! something links one of them into an executable (a minimal firmware
//! image is exactly what the hardware-in-loop rung of `xtask`'s target
//! matrix needs anyway). Both functions below take an already-built
//! binary path rather than trying to build one themselves, so this
//! crate doesn't have to guess at a linker script/target
//! configuration it has no way to know on every embedded target.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::toolshell::{find_tool, first_number_from, run_capture};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SizeReport {
    pub tool_path: Option<String>,
    pub text_bytes: Option<u64>,
    pub data_bytes: Option<u64>,
    pub bss_bytes: Option<u64>,
    pub raw_stdout: String,
}

/// Runs `cargo size --manifest-path <manifest> --release [--target
/// <triple>]` (no extra flags -> `size`'s default Berkeley output) over
/// an already-built binary's containing package, and parses `text`/
/// `data`/`bss` out of the summary line.
///
/// `manifest_path` is the `Cargo.toml` of the crate that produces
/// `binary_name`; `binary_name` selects which target within it (`cargo
/// size --bin <binary_name>` under the hood).
pub fn run_cargo_size(
    manifest_path: &Path,
    binary_name: &str,
    target_triple: Option<&str>,
) -> anyhow::Result<SizeReport> {
    let Some(tool_path) = find_tool(&["cargo-size"]) else {
        return Ok(SizeReport::default());
    };

    let mut args = vec![
        "size".to_string(),
        "--manifest-path".to_string(),
        manifest_path.display().to_string(),
        "--release".to_string(),
        "--bin".to_string(),
        binary_name.to_string(),
    ];
    if let Some(triple) = target_triple {
        args.push("--target".to_string());
        args.push(triple.to_string());
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

    let (_success, raw_stdout, _raw_stderr) = run_capture(&tool_path, &arg_refs)?;
    let (text_bytes, data_bytes, bss_bytes) = parse_size_berkeley(&raw_stdout);

    Ok(SizeReport {
        tool_path: Some(tool_path.display().to_string()),
        text_bytes,
        data_bytes,
        bss_bytes,
        raw_stdout,
    })
}

/// Parses `size`'s default Berkeley-format table:
///
/// ```text
///    text    data     bss     dec     hex filename
///    9042     248      24    9314    2462 target/release/foo
/// ```
///
/// Takes the first data row after the header (`cargo size` normally
/// prints exactly one).
fn parse_size_berkeley(stdout: &str) -> (Option<u64>, Option<u64>, Option<u64>) {
    let mut lines = stdout.lines();
    // Skip down to the header line so a leading "Compiling..." banner
    // (some cargo-size versions print build progress to stdout too)
    // doesn't get mistaken for the data row.
    for line in lines.by_ref() {
        if line.trim_start().starts_with("text") {
            break;
        }
    }
    let Some(data_line) = lines.next() else {
        return (None, None, None);
    };
    let cols: Vec<&str> = data_line.split_whitespace().collect();
    let parse_col = |i: usize| cols.get(i).and_then(|s| s.parse::<u64>().ok());
    (parse_col(0), parse_col(1), parse_col(2))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BloatReport {
    pub tool_path: Option<String>,
    pub crate_bytes: Option<u64>,
    pub raw_stdout: String,
}

/// Runs `cargo bloat --manifest-path <manifest> --release --crates
/// [--target <triple>] --bin <binary_name>` and extracts `crate_name`'s
/// row from the `--crates` breakdown table.
pub fn run_cargo_bloat(
    manifest_path: &Path,
    binary_name: &str,
    crate_name: &str,
    target_triple: Option<&str>,
) -> anyhow::Result<BloatReport> {
    let Some(tool_path) = find_tool(&["cargo-bloat"]) else {
        return Ok(BloatReport::default());
    };

    let mut args = vec![
        "bloat".to_string(),
        "--manifest-path".to_string(),
        manifest_path.display().to_string(),
        "--release".to_string(),
        "--crates".to_string(),
        "--bin".to_string(),
        binary_name.to_string(),
    ];
    if let Some(triple) = target_triple {
        args.push("--target".to_string());
        args.push(triple.to_string());
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

    let (_success, raw_stdout, _raw_stderr) = run_capture(&tool_path, &arg_refs)?;
    let crate_bytes = find_crate_row_bytes(&raw_stdout, crate_name);

    Ok(BloatReport {
        tool_path: Some(tool_path.display().to_string()),
        crate_bytes,
        raw_stdout,
    })
}

/// Finds the `--crates` table row for `crate_name` (matched with `-`
/// normalized to `_`, since `cargo bloat` reports Rust crate
/// identifiers) and returns its `Size` column, converted to bytes.
fn find_crate_row_bytes(stdout: &str, crate_name: &str) -> Option<u64> {
    let normalized = crate_name.replace('-', "_");
    for line in stdout.lines() {
        let trimmed = line.trim_end();
        if trimmed
            .rsplit(char::is_whitespace)
            .next()
            .is_some_and(|last_col| last_col == normalized)
        {
            return parse_size_with_unit(trimmed);
        }
    }
    None
}

/// Parses the first `<number><unit>` token in `line` (e.g. `"40.1KiB"`,
/// `"512B"`) into a byte count, using `cargo bloat`'s binary-prefix
/// units (`B`/`KiB`/`MiB`/`GiB`).
fn parse_size_with_unit(line: &str) -> Option<u64> {
    for (unit, multiplier) in [
        ("KiB", 1024.0),
        ("MiB", 1024.0 * 1024.0),
        ("GiB", 1024.0 * 1024.0 * 1024.0),
        ("B", 1.0),
    ] {
        if let Some(unit_pos) = line.find(unit) {
            // Walk backwards from the unit to find where the numeric
            // token before it starts.
            let prefix = &line[..unit_pos];
            let num_start = prefix
                .rfind(|c: char| !(c.is_ascii_digit() || c == '.'))
                .map(|i| i + 1)
                .unwrap_or(0);
            if let Some(value) = first_number_from(&prefix[num_start..], 0) {
                return Some((value * multiplier).round() as u64);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_SIZE_OUTPUT: &str = "\
   text    data     bss     dec     hex filename
   9042     248      24    9314    2462 target/release/qpp-rng-example
";

    #[test]
    fn parses_berkeley_size_table() {
        let (text, data, bss) = parse_size_berkeley(SAMPLE_SIZE_OUTPUT);
        assert_eq!(text, Some(9042));
        assert_eq!(data, Some(248));
        assert_eq!(bss, Some(24));
    }

    #[test]
    fn parse_size_berkeley_handles_a_leading_build_banner() {
        let with_banner = format!("   Compiling foo v0.1.0\n{SAMPLE_SIZE_OUTPUT}");
        let (text, _, _) = parse_size_berkeley(&with_banner);
        assert_eq!(text, Some(9042));
    }

    const SAMPLE_BLOAT_OUTPUT: &str = "\
File  .text     Size Crate
 4.2%  10.5%  40.1KiB qpp_rng_reference
 2.1%   5.3%  20.0KiB entropy_timer
93.7%  84.2% 320.7KiB std
100.0%  100.0% 380.8KiB .text section size, the file size is 1.2MiB
";

    #[test]
    fn finds_the_matching_crate_row_and_converts_units() {
        let bytes = find_crate_row_bytes(SAMPLE_BLOAT_OUTPUT, "qpp-rng-reference");
        // 40.1 KiB = 40.1 * 1024 bytes, rounded.
        assert_eq!(bytes, Some((40.1_f64 * 1024.0).round() as u64));
    }

    #[test]
    fn missing_crate_row_returns_none() {
        assert_eq!(find_crate_row_bytes(SAMPLE_BLOAT_OUTPUT, "not-a-real-crate"), None);
    }

    #[test]
    fn parse_size_with_unit_handles_plain_bytes() {
        assert_eq!(parse_size_with_unit(" 4.2%  10.5%    512B some_crate"), Some(512));
    }
}
