//! `xtask` -- the QPP-RNG test harness orchestrator. Not part of the
//! main workspace (see the root `Cargo.toml`'s `exclude` comment); run
//! it with:
//!
//! ```text
//! cargo run --manifest-path xtask/Cargo.toml -- <subcommand>
//! ```
//!
//! or add a `.cargo/config.toml` alias (`[alias] xtask = "run
//! --manifest-path xtask/Cargo.toml --"`) so `cargo xtask <subcommand>`
//! works from the workspace root, matching the testing architecture
//! doc's `cargo xtask compare` invocation.
//!
//! See [`crate::compare`] for what `compare` actually does and why most
//! of it is direct library calls into the `test-harness/*` crates
//! rather than shelled-out subprocesses, and [`crate::hil`] for the
//! hardware-in-loop flash/telemetry path.

mod compare;
mod hil;
mod target_matrix;

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask", about = "QPP-RNG test harness orchestrator")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build every requested target, then run stats -> bench ->
    /// footprint -> differential -> report in sequence.
    Compare {
        /// Target names from the matrix (see `xtask targets`). Defaults
        /// to `host` alone -- only the host rung can actually execute
        /// the harness today (see `crate::compare`'s module doc).
        #[arg(long, value_delimiter = ',')]
        target: Vec<String>,
        #[arg(long, default_value_t = stats::sample::MIN_SAMPLE_BYTES)]
        sample_bytes: usize,
        #[arg(long, default_value_t = 0x5EED_0000_1111_2222_3333_4444_5555_6666)]
        seed: u128,
        #[arg(long)]
        skip_stats: bool,
        #[arg(long)]
        skip_bench: bool,
        #[arg(long)]
        skip_footprint: bool,
        #[arg(long)]
        skip_differential: bool,
        #[arg(long)]
        skip_report: bool,
        /// Build in debug instead of `--release`. Only affects the
        /// per-target build step -- stats/bench/footprint always run
        /// the workspace's normal `[profile.release]`/`[profile.bench]`
        /// (see the root `Cargo.toml`'s package-specific `opt-level`
        /// overrides, which the whole jitter-timing story depends on).
        #[arg(long)]
        debug: bool,
        #[arg(long, default_value = "target/qpp-rng-compare")]
        out_dir: PathBuf,
        /// Print every command/step that would run without running
        /// anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// List the target matrix (host x QEMU x hardware-in-loop).
    Targets,
    /// Flash a built binary onto real hardware via `probe-rs run` and
    /// stream its output until it exits or `--timeout-secs` elapses.
    HilFlash {
        #[arg(long)]
        chip: String,
        #[arg(long)]
        binary: PathBuf,
        #[arg(long, default_value_t = 30)]
        timeout_secs: u64,
    },
    /// Flash without running (`probe-rs download`), for boards that
    /// need a separate target-specific reset afterward.
    HilDownload {
        #[arg(long)]
        chip: String,
        #[arg(long)]
        binary: PathBuf,
    },
    /// Pull `--bytes` raw sample bytes back off a target over UART.
    HilUartSample {
        #[arg(long)]
        device: PathBuf,
        #[arg(long, default_value_t = 115_200)]
        baud: u32,
        #[arg(long)]
        bytes: usize,
        #[arg(long, default_value_t = 30)]
        timeout_secs: u64,
        #[arg(long)]
        out: PathBuf,
    },
    /// Pull `--bytes` raw sample bytes back off a target over RTT
    /// (see `crate::hil::RttTelemetry`'s doc for the raw-channel-0
    /// assumption this makes about the firmware).
    HilRttSample {
        #[arg(long)]
        chip: String,
        #[arg(long)]
        elf: PathBuf,
        #[arg(long)]
        bytes: usize,
        #[arg(long, default_value_t = 30)]
        timeout_secs: u64,
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Compare {
            target,
            sample_bytes,
            seed,
            skip_stats,
            skip_bench,
            skip_footprint,
            skip_differential,
            skip_report,
            debug,
            out_dir,
            dry_run,
        } => compare::run_compare(compare::CompareArgs {
            targets: target,
            sample_bytes,
            seed,
            skip_stats,
            skip_bench,
            skip_footprint,
            skip_differential,
            skip_report,
            release: !debug,
            out_dir,
            dry_run,
        }),
        Command::Targets => {
            let host_triple = target_matrix::host_triple().unwrap_or_else(|_| "unknown".to_string());
            println!("{:24} {:17} {:26} {:16} description", "name", "rung", "triple", "qemu/probe-rs");
            for t in target_matrix::target_matrix() {
                let triple = match t.triple {
                    Some(triple) => triple.to_string(),
                    None => format!("(host: {host_triple})"),
                };
                let hw = t
                    .qemu_machine
                    .or(t.probe_rs_chip)
                    .unwrap_or("-");
                println!("{:24} {:17} {:26} {:16} {}", t.name, t.rung.to_string(), triple, hw, t.description);
            }
            Ok(())
        }
        Command::HilFlash { chip, binary, timeout_secs } => {
            let flasher = hil::ProbeRsFlasher;
            let output = flasher.flash_and_run(&chip, &binary, Duration::from_secs(timeout_secs))?;
            print!("{output}");
            Ok(())
        }
        Command::HilDownload { chip, binary } => {
            hil::ProbeRsFlasher.download(&chip, &binary)
        }
        Command::HilUartSample { device, baud, bytes, timeout_secs, out } => {
            let telemetry = hil::UartTelemetry { device_path: device, baud };
            let samples = telemetry.read_samples(bytes, Duration::from_secs(timeout_secs))?;
            std::fs::write(&out, &samples)?;
            println!("wrote {} bytes to {}", samples.len(), out.display());
            Ok(())
        }
        Command::HilRttSample { chip, elf, bytes, timeout_secs, out } => {
            let telemetry = hil::RttTelemetry { chip, elf_path: elf };
            let samples = telemetry.read_samples(bytes, Duration::from_secs(timeout_secs))?;
            std::fs::write(&out, &samples)?;
            println!("wrote {} bytes to {}", samples.len(), out.display());
            Ok(())
        }
    }
}
