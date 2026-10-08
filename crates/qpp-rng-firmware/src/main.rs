//! The `[[bin]]` target `footprint::size`/`footprint::stack` need: a
//! real, linked executable that actually calls into `qpp-rng-reference`,
//! for `cargo bloat`/`cargo size`/`cargo call-stack` to measure. Those
//! tools inspect a linked binary's `.text`/`.data`/`.bss` sections --
//! `qpp-rng-reference` and `qpp-rng-iot` are libraries with no such
//! sections of their own until *something* links them into a real
//! executable. This is that something, kept as its own crate rather
//! than added into either library (same reasoning as
//! `crates/conditioning`'s module doc: keep the "ground truth"
//! implementation free of anything that isn't the algorithm itself).
//!
//! ## What it actually does
//!
//! As little as possible: construct one candidate, generate a small
//! fixed number of raw bytes, write them to stdout. No CLI-argument
//! parsing, no `clap`, no `anyhow` -- every dependency this binary pulls
//! in beyond `qpp-rng-reference` itself would inflate the `.text` size
//! `cargo bloat`/`cargo size` report with code that has nothing to do
//! with the RNG, defeating the point of measuring it.
//!
//! ## Host-only today, on purpose
//!
//! This `[[bin]]` builds for whatever host this workspace is compiled
//! on -- it is *not* the embedded firmware the hardware-in-loop rung of
//! `xtask`'s target matrix ultimately needs. A real board target needs
//! `#![no_std] #![no_main]`, a board-specific HAL/runtime crate
//! (`esp-hal`, `avr-hal`, ...), and a linker script -- real, separate
//! work, not something to bolt onto this file speculatively without a
//! specific board to build it against. What *is* useful today: this
//! gives `cargo bloat -p qpp-rng-firmware --bin qpp-rng-sample-dump-host`/
//! `cargo size` something real to measure on host right now, and it's
//! the natural starting point (the same `QppRngXorshift::from_seed`
//! call, same crate dependency) once a board-specific `[[bin]]` gets
//! added alongside this one.

use qpp_rng_reference::QppRngXorshift;
use rand_core::Rng;
use std::io::Write;

/// Matches this workspace's other harness defaults (`stats`, `bench`,
/// `footprint`, `differential`, `xtask`) so a footprint run's seed lines
/// up with everything else if it's ever worth cross-referencing.
const SEED: u128 = 0x5EED_0000_1111_2222_3333_4444_5555_6666;

/// Small and arbitrary -- this binary exists to be measured statically
/// (code/data size), not to demonstrate real throughput; see
/// `test-harness/bench`/`test-harness/footprint::cycles` for that.
const SAMPLE_BYTES: usize = 64;

fn main() {
    let mut rng = QppRngXorshift::from_seed(SEED);
    let mut buf = [0u8; SAMPLE_BYTES];
    rng.fill_bytes(&mut buf);
    std::io::stdout()
        .write_all(&buf)
        .expect("writing sample bytes to stdout failed");
}
