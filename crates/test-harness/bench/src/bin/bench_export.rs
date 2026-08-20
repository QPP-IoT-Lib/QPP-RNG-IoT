//! `bench-export` -- reads back whatever `cargo bench -p bench` just
//! wrote to `target/criterion` and writes one `BenchReport` JSON file.
//! The process boundary `xtask` shells across between "run the
//! benchmarks" and "fold the results into the comparison report".

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "bench-export", about = "Export criterion's JSON output into a BenchReport")]
struct Cli {
    /// Criterion's output directory, normally `target/criterion`.
    #[arg(long, default_value = "target/criterion")]
    criterion_dir: PathBuf,
    #[arg(long)]
    out: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let report = bench::export_from_criterion_dir(&cli.criterion_dir)?;
    std::fs::write(&cli.out, serde_json::to_string_pretty(&report)?)?;
    println!(
        "wrote {} measurement(s) to {}",
        report.measurements.len(),
        cli.out.display()
    );
    Ok(())
}
