//! `differential-cli` -- the process boundary `xtask` shells across for
//! the determinism + parity checks (the proptest fuzz suite runs
//! through plain `cargo test -p differential`, not this CLI -- see
//! `differential::report`'s module doc for why).

use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "differential-cli", about = "QPP-RNG determinism + API parity checks")]
struct Cli {
    #[arg(long, default_value_t = 0x5EED_0000_1111_2222_3333_4444_5555_6666)]
    seed: u128,
    /// Comma-separated jitter-clock delta script.
    #[arg(long, value_delimiter = ',', default_value = "101,43,999,7,256,12")]
    deltas: Vec<u64>,
    #[arg(long, default_value_t = 256)]
    n_bytes: usize,
    #[arg(long)]
    out: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let report = differential::run_all(cli.seed, &cli.deltas, cli.n_bytes);

    for d in &report.determinism {
        eprintln!(
            "determinism: {:32} deterministic={} first_divergence={:?}",
            d.candidate, d.deterministic, d.first_divergence_index
        );
    }
    for p in &report.parity {
        eprintln!("parity:      {:32} pass={} errors={:?}", p.candidate, p.all_passed(), p.errors);
    }

    std::fs::write(&cli.out, serde_json::to_string_pretty(&report)?)?;
    println!("wrote differential report to {}", cli.out.display());

    if !report.all_passed() {
        std::process::exit(1);
    }
    Ok(())
}
