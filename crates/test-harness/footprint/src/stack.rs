//! Stack/RAM analysis: `cargo call-stack`'s static worst-case analysis
//! where it applies, plus a manual fill-pattern high-water-mark
//! primitive as the fallback the testing architecture doc calls for
//! (§5.2: *"manual high-water-mark instrumentation as a fallback if
//! that tool doesn't support your target"*).
//!
//! `cargo call-stack` only works on `no_std`/no-runtime-support ELF
//! binaries built with `-Z emit-stack-sizes` (nightly), a whole-program
//! static analysis rather than a runtime measurement -- it can't run at
//! all on the host-`std` binaries this workspace's crates currently
//! produce. The fill-pattern primitive below is the one piece of this
//! module actually exercised by this crate's own tests, since it's
//! runtime instrumentation any target (host included) can execute.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::toolshell::{find_tool, run_capture};

// ---------------------------------------------------------------------
// Fill-pattern high-water-mark instrumentation
// ---------------------------------------------------------------------

/// Byte pattern painted over a stack region before running the
/// workload under measurement. `0xAA` (`0b10101010`) is the conventional
/// choice for this technique (e.g. FreeRTOS's `uxTaskGetStackHighWaterMark`
/// fill value) -- distinctive enough that it's exceedingly unlikely to
/// arise from real computed data left behind by accident, so a leftover
/// `0xAA` byte is a reliable "never touched" signal.
pub const FILL_PATTERN: u8 = 0xAA;

/// Paints `region` with [`FILL_PATTERN`]. Call this before running the
/// workload whose stack usage is being measured.
///
/// # On real hardware
/// `region` is normally not an ordinary buffer but the linker-defined
/// stack area itself (e.g. the span between a linker script's
/// `_stack_start`/`_stack_end` symbols), painted once at reset before
/// the runtime hands control to `main`. Wiring that up is board/linker-
/// script-specific and is `xtask`'s hardware-in-loop rung's job, not
/// this crate's -- what's provided here is the portable
/// paint-then-scan logic itself, which is identical regardless of where
/// the region comes from.
pub fn paint(region: &mut [u8]) {
    region.fill(FILL_PATTERN);
}

/// Scans `region` for how many bytes, from the start, are no longer
/// [`FILL_PATTERN`] -- the peak stack depth reached while `region` was
/// live, in bytes.
///
/// # Convention
/// `region[0]` must be the *deepest* address the stack can reach (the
/// end of the stack region nearest overflow -- typically the lowest
/// address, since most targets this workspace cares about grow the
/// stack downward) and `region[region.len() - 1]` the shallowest
/// (nearest the initial stack pointer). Usage always starts from the
/// shallow end and grows toward the deep end, so the first untouched
/// byte scanning from `region[0]` marks exactly how deep usage reached.
///
/// Returns `region.len()` (not `None`/an error) if every byte was
/// touched -- that's the correct "at least this much, possibly a stack
/// overflow past the end of `region`" reading, not a distinct failure
/// mode the caller needs to special-case.
pub fn high_water_mark(region: &[u8]) -> usize {
    region
        .iter()
        .position(|&b| b == FILL_PATTERN)
        .unwrap_or(region.len())
}

/// Runs `workload` with [`paint`]/[`high_water_mark`] bracketing it over
/// `scratch` (a caller-provided, `scratch.len()`-byte stand-in for a
/// stack region -- see [`high_water_mark`]'s convention note). This is
/// a measurement of `workload`'s *use of `scratch` specifically*, not
/// of the real call stack: on host `std`, there's no portable, safe way
/// to paint the actual OS-managed stack out from under a running
/// program. Meaningful high-water-mark instrumentation of the *real*
/// stack needs the on-target wiring described on [`paint`].
pub fn measure_high_water_mark(scratch: &mut [u8], workload: impl FnOnce(&mut [u8])) -> usize {
    paint(scratch);
    workload(scratch);
    high_water_mark(scratch)
}

// ---------------------------------------------------------------------
// cargo call-stack
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CallStackReport {
    pub tool_path: Option<String>,
    /// The largest single number found in the tool's DOT output --
    /// `cargo call-stack` labels each node with its cumulative
    /// worst-case stack usage in bytes, so the graph-wide maximum is a
    /// reasonable (if approximate -- see the module doc's caveat)
    /// stand-in for "worst-case stack depth" without this crate parsing
    /// full DOT graph syntax.
    pub max_labeled_bytes: Option<u64>,
    pub raw_stdout: String,
}

/// Runs `cargo call-stack --bin <binary_name>` against a `no_std`
/// package built with `-Z emit-stack-sizes` (nightly-only) and does a
/// best-effort scan of its DOT output for the largest stack-usage
/// label.
///
/// Only meaningful for `no_std`/bare-metal targets -- see the module
/// doc. Returns a default (all-`None`) [`CallStackReport`] if the tool
/// isn't installed, exactly like every other Tier 2-style external-tool
/// wrapper in this workspace: a missing tool is a reportable state, not
/// an error.
pub fn run_cargo_call_stack(manifest_path: &Path, binary_name: &str) -> anyhow::Result<CallStackReport> {
    let Some(tool_path) = find_tool(&["cargo-call-stack"]) else {
        return Ok(CallStackReport::default());
    };

    let (_success, raw_stdout, _raw_stderr) = run_capture(
        &tool_path,
        &[
            "call-stack",
            "--manifest-path",
            manifest_path.to_str().unwrap_or_default(),
            "--bin",
            binary_name,
        ],
    )?;

    let max_labeled_bytes = raw_stdout
        .lines()
        .filter_map(|line| crate::toolshell::first_number_from(line, 0))
        .map(|v| v as u64)
        .max();

    Ok(CallStackReport {
        tool_path: Some(tool_path.display().to_string()),
        max_labeled_bytes,
        raw_stdout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_water_mark_of_untouched_region_is_zero() {
        let region = [FILL_PATTERN; 128];
        assert_eq!(high_water_mark(&region), 0);
    }

    #[test]
    fn high_water_mark_of_fully_touched_region_is_its_length() {
        let region = [0u8; 128];
        assert_eq!(high_water_mark(&region), 128);
    }

    #[test]
    fn high_water_mark_reports_deepest_touched_byte() {
        let mut region = [FILL_PATTERN; 128];
        // Simulate a workload that touched the first 40 bytes (deepest
        // end of the region, per the documented convention).
        for b in &mut region[..40] {
            *b = 0x00;
        }
        assert_eq!(high_water_mark(&region), 40);
    }

    #[test]
    fn measure_high_water_mark_paints_then_scans_around_a_workload() {
        let mut scratch = vec![0u8; 64]; // starts dirty, unlike FILL_PATTERN
        let depth = measure_high_water_mark(&mut scratch, |region| {
            // A "workload" that recurses/writes 10 bytes deep.
            for b in &mut region[..10] {
                *b = 0x42;
            }
        });
        assert_eq!(depth, 10);
    }

    #[test]
    fn measure_high_water_mark_of_a_no_op_workload_is_zero() {
        let mut scratch = vec![0u8; 32];
        let depth = measure_high_water_mark(&mut scratch, |_region| {});
        assert_eq!(depth, 0);
    }

    #[test]
    fn missing_call_stack_tool_is_ok_not_err() {
        let result = run_cargo_call_stack(Path::new("/nonexistent/Cargo.toml"), "does-not-matter");
        assert!(result.is_ok());
    }
}
