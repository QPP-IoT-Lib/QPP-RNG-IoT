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
    let (text_bytes, data_bytes, bss_bytes) = parse_size_output(&raw_stdout);

    Ok(SizeReport {
        tool_path: Some(tool_path.display().to_string()),
        text_bytes,
        data_bytes,
        bss_bytes,
        raw_stdout,
    })
}

/// Parses `cargo size`'s summary table -- in either of the two formats
/// its underlying `size` binary can print, depending on the object
/// format of the binary being measured:
///
/// **Linux/GNU, Berkeley format** (ELF binaries):
/// ```text
///    text    data     bss     dec     hex filename
///    9042     248      24    9314    2462 target/release/foo
/// ```
///
/// **macOS (Mach-O binaries)** -- confirmed against a real build of
/// this workspace's `qpp-rng-firmware` `[[bin]]`, not guessed, after
/// the Berkeley-only assumption this parser originally shipped with
/// turned out to silently return `(None, None, None)` on every macOS
/// run:
/// ```text
/// __TEXT   __DATA   __OBJC   others       dec          hex
/// 262144   16384    0        4295065600   4295344128   10005c000
/// ```
/// Mach-O's summary has no BSS-equivalent column -- `__DATA` bundles
/// initialized and uninitialized data together, and `others` is
/// unrelated segments (`__LINKEDIT` and similar), not bss -- so
/// `bss_bytes` comes back `None` on this branch, honestly, rather than
/// mapping "others" to it and reporting a number that doesn't mean bss
/// at all.
///
/// Takes the first data row after the header (`cargo size` normally
/// prints exactly one).
fn parse_size_output(stdout: &str) -> (Option<u64>, Option<u64>, Option<u64>) {
    let mut lines = stdout.lines();
    // Skip down to the header line so a leading "Compiling..." banner
    // (some cargo-size versions print build progress to stdout too)
    // doesn't get mistaken for the data row.
    let header = lines.by_ref().find_map(|line| {
        let trimmed = line.trim_start();
        (trimmed.starts_with("text") || trimmed.starts_with("__TEXT")).then_some(trimmed)
    });
    let Some(header) = header else {
        return (None, None, None);
    };
    let Some(data_line) = lines.next() else {
        return (None, None, None);
    };
    let cols: Vec<&str> = data_line.split_whitespace().collect();
    let parse_col = |i: usize| cols.get(i).and_then(|s| s.parse::<u64>().ok());

    if header.starts_with("__TEXT") {
        (parse_col(0), parse_col(1), None)
    } else {
        (parse_col(0), parse_col(1), parse_col(2))
    }
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
        let (text, data, bss) = parse_size_output(SAMPLE_SIZE_OUTPUT);
        assert_eq!(text, Some(9042));
        assert_eq!(data, Some(248));
        assert_eq!(bss, Some(24));
    }

    #[test]
    fn parse_size_berkeley_handles_a_leading_build_banner() {
        let with_banner = format!("   Compiling foo v0.1.0\n{SAMPLE_SIZE_OUTPUT}");
        let (text, _, _) = parse_size_output(&with_banner);
        assert_eq!(text, Some(9042));
    }

    /// Real `cargo size --release -p qpp-rng-firmware --bin
    /// qpp-rng-sample-dump-host` output, captured on this workspace's
    /// own `[[bin]]` on Apple Silicon macOS -- the format
    /// `parse_size_output`'s original Berkeley-only assumption silently
    /// returned `(None, None, None)` against, on the very first real
    /// run it ever saw.
    const SAMPLE_MACHO_SIZE_OUTPUT: &str =
        "__TEXT\t__DATA\t__OBJC\tothers\tdec\thex\n262144\t16384\t0\t4295065600\t4295344128\t10005c000\t\n";

    #[test]
    fn parses_macho_size_table() {
        let (text, data, bss) = parse_size_output(SAMPLE_MACHO_SIZE_OUTPUT);
        assert_eq!(text, Some(262144));
        assert_eq!(data, Some(16384));
        // No bss-equivalent column in Mach-O's summary -- see
        // parse_size_output's doc for why "others" isn't it.
        assert_eq!(bss, None);
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
