//! `report-cli` -- `cargo xtask compare`'s final step: ingest every
//! other track's output and render the comparison Markdown/CSV.

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "report-cli", about = "Aggregate the QPP-RNG test-harness tracks into one comparison report")]
struct Cli {
    /// JSON array written by `stats-cli full --out <path>`. Omit (or
    /// point at a nonexistent path) to render that track as N/A.
    #[arg(long, default_value = "target/qpp-rng-reports/stats.json")]
    stats: PathBuf,
    /// Criterion's output directory, normally `target/criterion`.
    #[arg(long, default_value = "target/criterion")]
    criterion_dir: PathBuf,
    /// One or more footprint report JSON files (`footprint-cli full
    /// --out <path>`), one per candidate/target combination.
    #[arg(long)]
    footprint: Vec<PathBuf>,
    /// JSON written by `differential-cli --out <path>`.
    #[arg(long, default_value = "target/qpp-rng-reports/differential.json")]
    differential: PathBuf,
    #[arg(long, default_value = "target/qpp-rng-reports/comparison.md")]
    out_markdown: PathBuf,
    #[arg(long, default_value = "target/qpp-rng-reports/comparison.csv")]
    out_csv: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let stats = report::ingest::ingest_stats(&cli.stats)?;
    let bench = report::ingest::ingest_bench(&cli.criterion_dir)?;
    let footprint = report::ingest::ingest_footprint(&cli.footprint)?;
    let differential = report::ingest::ingest_differential(&cli.differential)?;

    let table = report::build_comparison_table(&stats, &bench, &footprint, differential.as_ref());

    if let Some(parent) = cli.out_markdown.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&cli.out_markdown, report::markdown::to_markdown(&table))?;
    if let Some(parent) = cli.out_csv.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&cli.out_csv, report::csv::to_csv(&table))?;

    println!(
        "compared {} candidate(s); wrote {} and {}",
        table.rows.len(),
        cli.out_markdown.display(),
        cli.out_csv.display()
    );
    for row in &table.rows {
        println!(
            "  {:32} overall={}",
            row.candidate,
            match row.overall_pass() {
                Some(true) => "pass",
                Some(false) => "FAIL",
                None => "N/A (no gate ran)",
            }
        );
    }

    Ok(())
}
