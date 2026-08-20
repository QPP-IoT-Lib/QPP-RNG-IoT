//! Shared logic for every `[[bin]]` in this crate -- see the crate's
//! `Cargo.toml` and each `src/bin/*.rs` file for why there are several
//! near-identical binaries instead of one.
//!
//! ## Why one binary per candidate, not one binary that picks a
//! ## candidate at runtime
//!
//! A single binary taking, say, `--candidate reference-nextx48` on the
//! command line would make *every* candidate's code reachable from
//! `main()` at once -- which means the linker can no longer dead-code-
//! eliminate the candidates you didn't ask for. `cargo size`/`cargo
//! bloat` would then report the size of *all* candidates' code
//! combined, for every candidate, which is exactly the wrong number:
//! not one of them actually costs that much flash on its own. The only
//! way to get an honest, individually-attributable size is a binary
//! that only ever calls into the one implementation being measured --
//! hence one `[[bin]]` per candidate, each a few lines calling
//! [`dump_sample`] with a different concrete type. `xtask::compare`'s
//! `FIRMWARE_TARGETS` table is what runs `cargo size`/`cargo bloat`
//! against each of these in turn.

use rand_core::Rng;
use std::io::Write;

/// Matches this workspace's other harness defaults (`stats`, `bench`,
/// `footprint`, `differential`, `xtask`) so a footprint run's seed lines
/// up with everything else if it's ever worth cross-referencing.
pub const SEED: u128 = 0x5EED_0000_1111_2222_3333_4444_5555_6666;

/// Small and arbitrary -- these binaries exist to be measured statically
/// (code/data size), not to demonstrate real throughput; see
/// `test-harness/bench`/`test-harness/footprint::cycles` for that.
pub const SAMPLE_BYTES: usize = 64;

/// Generates [`SAMPLE_BYTES`] from `rng` and writes them to stdout. The
/// one piece of logic every `[[bin]]` in this crate shares -- kept
/// tiny and dependency-free (no `clap`/`anyhow`) on purpose, since
/// anything pulled in here inflates every binary's measured `.text`
/// size with code that has nothing to do with the candidate under
/// test.
pub fn dump_sample<R: Rng>(mut rng: R) {
    let mut buf = [0u8; SAMPLE_BYTES];
    rng.fill_bytes(&mut buf);
    std::io::stdout()
        .write_all(&buf)
        .expect("writing sample bytes to stdout failed");
}
