//! `stats-cli` -- the process boundary `xtask` shells out across.
//!
//! Kept as a thin CLI over the `stats` library so `xtask` can drive
//! per-target-triple builds of this exact same code (see the crate's
//! `[[bin]]` entry) without linking the harness into `xtask` itself.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use stats::report::{self, Tier2Options};
use stats::sample;

#[derive(Parser)]
#[command(name = "stats-cli", about = "QPP-RNG statistical quality harness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate one SP 800-90B-sized sample file per registered
    /// candidate.
    GenerateSamples {
        #[arg(long, default_value = "target/qpp-rng-samples")]
        dir: PathBuf,
        #[arg(long, default_value_t = sample::MIN_SAMPLE_BYTES)]
        bytes: usize,
        #[arg(long, default_value_t = 0x5EED_0000_1111_2222_3333_4444_5555_6666)]
        seed: u128,
    },
    /// Run Tier 1's fast native smoke tests over one sample file.
    Tier1 {
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Run Tier 1 + every enabled Tier 2 external tool over every sample
    /// file in `--dir` and write one JSON array of `StatReport`s.
    Full {
        #[arg(long, default_value = "target/qpp-rng-samples")]
        dir: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        skip_sp800_90b: bool,
        #[arg(long)]
        skip_sp800_22: bool,
        #[arg(long)]
        skip_ent: bool,
        #[arg(long, default_value = "target/qpp-rng-sts-work")]
        sts_work_dir: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::GenerateSamples { dir, bytes, seed } => {
            let files = sample::generate_all_candidate_samples(&dir, bytes, seed)?;
            for f in &files {
                println!("{:32} {:>10} bytes  {}", f.candidate, f.len_bytes, f.path.display());
            }
        }
        Command::Tier1 { file, out } => {
            let bytes = std::fs::read(&file)?;
            let report = stats::tier1::run_tier1(&bytes);
            let json = serde_json::to_string_pretty(&report)?;
            match out {
                Some(path) => std::fs::write(path, json)?,
                None => println!("{json}"),
            }
            if !report.all_passed() {
                std::process::exit(1);
            }
        }
        Command::Full {
            dir,
            out,
            skip_sp800_90b,
            skip_sp800_22,
            skip_ent,
            sts_work_dir,
        } => {
            std::fs::create_dir_all(&sts_work_dir)?;
            let mut opts = Tier2Options::all();
            if skip_sp800_90b {
                opts.sp800_90b_iid = false;
                opts.sp800_90b_non_iid = false;
            }
            if skip_sp800_22 {
                opts.sp800_22 = false;
            }
            if skip_ent {
                opts.ent = false;
            }

            let mut reports = Vec::new();
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("bin") {
                    continue;
                }
                let candidate = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                eprintln!("running Tier 1 + Tier 2 for {candidate}...");
                let report = report::run_full_battery(&candidate, &path, opts, &sts_work_dir)?;
                eprintln!(
                    "  tier1 pass={} min_entropy={:?} overall_pass={}",
                    report.tier1.all_passed(),
                    report.min_entropy_estimate(),
                    report.overall_pass()
                );
                reports.push(report);
            }

            std::fs::write(&out, serde_json::to_string_pretty(&reports)?)?;
            println!("wrote {} candidate report(s) to {}", reports.len(), out.display());
        }
    }
    Ok(())
}
