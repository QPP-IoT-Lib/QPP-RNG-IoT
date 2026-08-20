//! Small `Command`-shelling helpers shared by [`crate::size`] and
//! [`crate::stack`]'s external-tool wrappers.
//!
//! This is a near-duplicate of `test-harness/stats`'s equivalent
//! private helpers (`find_tool`, `first_float_token`, ...). Kept
//! separate rather than factored into a shared crate on purpose: each
//! `test-harness/*` crate is meant to be independently buildable/
//! testable per the harness's own layout (see the workspace's testing
//! architecture doc, §3), and this is ~40 lines, not enough shared
//! surface to justify a cross-cutting dependency between two otherwise
//! unrelated harness crates.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;

/// Searches `PATH` for the first name in `candidate_names` that
/// resolves to an executable file.
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

pub fn run_capture(program: &Path, args: &[&str]) -> anyhow::Result<(bool, String, String)> {
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

/// Scans `text` for the first substring matching `\d+(\.\d+)?` starting
/// at or after byte offset `from`, and parses it. See `stats::tier2`'s
/// `first_float_token` for why a character scan beats naive
/// whitespace-splitting for tool output that mixes numbers with
/// adjoining punctuation/units.
pub fn first_number_from(text: &str, from: usize) -> Option<f64> {
    let chars: Vec<char> = text[from.min(text.len())..].chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_number_from_skips_leading_text() {
        assert_eq!(first_number_from("Size: 12.5KiB more text", 0), Some(12.5));
    }

    #[test]
    fn first_number_from_returns_none_when_absent() {
        assert_eq!(first_number_from("no digits here", 0), None);
    }

    #[test]
    fn find_tool_returns_none_for_a_name_that_cannot_exist() {
        assert!(find_tool(&["definitely-not-a-real-binary-xyz"]).is_none());
    }
}
