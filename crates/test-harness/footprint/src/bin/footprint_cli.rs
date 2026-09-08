//! `footprint-cli` -- the process boundary `xtask` shells across for
//! size/stack/cycle measurements, one target triple at a time.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use entropy_timer::{HighResTimer, PlatformTimer};
use footprint::report::FootprintReport;
use footprint::{cycles, size, stack};

#[derive(Parser)]
#[command(name = "footprint-cli", about = "QPP-RNG footprint (size/stack/cycles) harness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Cycles/nanoseconds per output byte for one registered candidate,
    /// via the real platform timer.
    Cycles {
        #[arg(long)]
        candidate: String,
        #[arg(long, default_value_t = 10_000)]
        bytes: usize,
        #[arg(long, default_value_t = 0x5EED_0000_1111_2222_3333_4444_5555_6666)]
        seed: u128,
    },
    /// Full footprint report for one candidate against an already-built
    /// binary (see `footprint::size`'s module doc for why a binary path
    /// is required rather than inferred).
    Full {
        #[arg(long)]
        candidate: String,
        #[arg(long)]
        manifest_path: PathBuf,
        #[arg(long)]
        bin: String,
        /// Crate name to look up in `cargo bloat --crates`'s table
        /// (defaults to the package crate name backing `--bin`, e.g.
        /// `qpp-rng-reference`).
        #[arg(long)]
        crate_name: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long, default_value_t = 10_000)]
        cycle_sample_bytes: usize,
        #[arg(long, default_value_t = 0x5EED_0000_1111_2222_3333_4444_5555_6666)]
        seed: u128,
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Cycles { candidate, bytes, seed } => {
            let Some(c) = candidates::find(&candidate) else {
                anyhow::bail!("unknown candidate {candidate:?}");
            };
            let mut rng = (c.make)(seed);
            let mut timer = PlatformTimer;
            timer.init();
            let report = cycles::measure_ticks_per_byte(rng.as_mut(), &mut timer, bytes);
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::Full {
            candidate,
            manifest_path,
            bin,
            crate_name,
            target,
            cycle_sample_bytes,
            seed,
            out,
        } => {
            let size_report = size::run_cargo_size(&manifest_path, &bin, target.as_deref())?;
            let bloat_report =
                size::run_cargo_bloat(&manifest_path, &bin, &crate_name, target.as_deref())?;
            let call_stack_report = stack::run_cargo_call_stack(&manifest_path, &bin)?;

            // Cycle counting only makes sense against a candidate this
            // process can actually construct and run -- i.e. the host
            // build, not a cross-compiled target binary this process
            // can't execute. Skip it (leave `cycles: None`) whenever a
            // cross target was requested.
            let cycles_report = if target.is_none() {
                match candidates::find(&candidate) {
                    Some(c) => {
                        let mut rng = (c.make)(seed);
                        let mut timer = PlatformTimer;
                        timer.init();
                        Some(cycles::measure_ticks_per_byte(
                            rng.as_mut(),
                            &mut timer,
                            cycle_sample_bytes,
                        ))
                    }
                    None => None,
                }
            } else {
                None
            };

            let report = FootprintReport {
                candidate,
                target_triple: target,
                size: size_report,
                bloat: bloat_report,
                call_stack: call_stack_report,
                cycles: cycles_report,
            };
            std::fs::write(&out, serde_json::to_string_pretty(&report)?)?;
            println!("wrote footprint report to {}", out.display());
        }
    }
    Ok(())
}
