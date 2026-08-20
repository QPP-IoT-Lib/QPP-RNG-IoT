//! `cargo xtask compare`: build every requested target, then run
//! stats -> bench -> footprint -> differential -> report in sequence.
//!
//! ## Library calls vs. shelling out
//!
//! Every `test-harness/*` crate is a normal Rust library (in addition
//! to exposing a CLI binary other tooling/CI can call directly -- see
//! each crate's `src/bin/*_cli.rs`). `xtask` depends on all of them
//! directly and calls their functions in-process wherever that's
//! possible, rather than spawning `cargo run -p stats --bin stats-cli`
//! as a subprocess for every step -- it's the same code either way, one
//! less process boundary, and it's how `xtask` gets to build one
//! unified [`report::ComparisonTable`] without round-tripping every
//! intermediate result through a JSON file on disk.
//!
//! `xshell` is reserved for the two things that genuinely need a
//! separate process: `cargo build --target <triple>` (cross-compilation
//! is `rustc`'s job, not something to reimplement) and `cargo bench -p
//! bench` (criterion's `harness = false` benchmark binary has its own
//! `main`, so it can't be called as a library function -- `bench`'s own
//! `export_from_criterion_dir` is what turns its on-disk output back
//! into something this module can use directly). Everything that shells
//! out to an external tool with its own CLI (the NIST/ENT/`cargo-bloat`/
//! `cargo-size`/`cargo-call-stack` family) does so inside
//! `stats::tier2`/`footprint::size`/`footprint::stack` already, using
//! `std::process::Command` -- see those crates' module docs for why
//! that split is where each tool's own crate said it would be, not
//! here. `xshell` shows up again in [`crate::hil`], for `probe-rs`
//! specifically, since the hardware-in-loop rung has no library-level
//! equivalent to call into.

use std::path::PathBuf;

use anyhow::Context;
use xshell::{cmd, Shell};

use crate::target_matrix::{find_target, is_target_installed, target_matrix, Rung};

pub struct CompareArgs {
    pub targets: Vec<String>,
    pub sample_bytes: usize,
    pub seed: u128,
    pub skip_stats: bool,
    pub skip_bench: bool,
    pub skip_footprint: bool,
    pub skip_differential: bool,
    pub skip_report: bool,
    pub release: bool,
    pub out_dir: PathBuf,
    pub dry_run: bool,
}

pub fn run_compare(args: CompareArgs) -> anyhow::Result<()> {
    std::fs::create_dir_all(&args.out_dir)?;
    let sh = Shell::new()?;

    let targets = resolve_targets(&args.targets)?;
    for target in &targets {
        build_target(&sh, target, args.release, args.dry_run)?;
    }

    let ran_host = targets.iter().any(|t| t.rung == Rung::Host);
    if !ran_host {
        println!(
            "note: no `host` rung requested -- stats/bench/footprint/differential all need to \
             execute the candidates themselves (real timing jitter, real proptest cases), which \
             only the host rung does today. QEMU/hardware-in-loop rungs above were built-only; \
             see crate::hil for the flash/telemetry path a real on-target run would use."
        );
        return Ok(());
    }

    let stats_path = args.out_dir.join("stats.json");
    if !args.skip_stats {
        run_stats(&args, &stats_path)?;
    }

    if !args.skip_bench {
        run_bench(&sh, args.dry_run)?;
    }

    let mut footprint_paths = Vec::new();
    if !args.skip_footprint {
        footprint_paths = run_footprint(&args)?;
    }

    let differential_path = args.out_dir.join("differential.json");
    if !args.skip_differential {
        run_differential(&sh, &args, &differential_path)?;
    }

    if !args.skip_report {
        run_report(&args, &stats_path, &footprint_paths, &differential_path)?;
    }

    Ok(())
}

fn resolve_targets(names: &[String]) -> anyhow::Result<Vec<crate::target_matrix::TargetSpec>> {
    if names.is_empty() {
        return Ok(vec![find_target("host").expect("host is always in the matrix")]);
    }
    names
        .iter()
        .map(|n| {
            find_target(n).ok_or_else(|| {
                let known: Vec<_> = target_matrix().into_iter().map(|t| t.name).collect();
                anyhow::anyhow!("unknown target {n:?}; known targets: {}", known.join(", "))
            })
        })
        .collect()
}

fn build_target(
    sh: &Shell,
    target: &crate::target_matrix::TargetSpec,
    release: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    let mut cmd_parts = vec!["cargo".to_string(), "build".to_string()];
    cmd_parts.extend(["-p".into(), "qpp-rng-reference".into()]);
    cmd_parts.extend(["-p".into(), "qpp-rng-iot".into()]);
    if release {
        cmd_parts.push("--release".into());
    }
    if let Some(triple) = target.triple {
        if !dry_run && !is_target_installed(triple) {
            println!(
                "skipping build for {} ({triple}): not installed (`rustup target add {triple}`)",
                target.name
            );
            return Ok(());
        }
        cmd_parts.push("--target".into());
        cmd_parts.push(triple.to_string());
    }

    println!("[{}] {}", target.name, cmd_parts.join(" "));
    if dry_run {
        return Ok(());
    }

    let (program, rest) = cmd_parts.split_first().unwrap();
    Shell::change_dir(sh, workspace_root());
    xshell::cmd!(sh, "{program} {rest...}")
        .run()
        .with_context(|| format!("building for target {}", target.name))?;
    Ok(())
}

fn run_stats(args: &CompareArgs, out_path: &std::path::Path) -> anyhow::Result<()> {
    println!(
        "[stats] generating {} candidate sample(s) of {} bytes each -- this runs each \
         candidate's real jitter timer and will take a while (minutes, not seconds)",
        candidates::all_candidates().len(),
        args.sample_bytes
    );
    if args.dry_run {
        return Ok(());
    }

    let samples_dir = args.out_dir.join("samples");
    let sts_work_dir = args.out_dir.join("sts-work");
    std::fs::create_dir_all(&sts_work_dir)?;

    let files = stats::sample::generate_all_candidate_samples(&samples_dir, args.sample_bytes, args.seed)?;

    let mut reports = Vec::new();
    for f in &files {
        println!("[stats] running Tier 1 + Tier 2 for {}...", f.candidate);
        let report = stats::report::run_full_battery(
            f.candidate,
            &f.path,
            stats::Tier2Options::all(),
            &sts_work_dir,
        )?;
        println!(
            "[stats]   tier1_pass={} min_entropy={:?}",
            report.tier1.all_passed(),
            report.min_entropy_estimate()
        );
        reports.push(report);
    }

    std::fs::write(out_path, serde_json::to_string_pretty(&reports)?)?;
    Ok(())
}

fn run_bench(sh: &Shell, dry_run: bool) -> anyhow::Result<()> {
    let criterion_dir = workspace_root().join("target/criterion");
    println!(
        "[bench] removing {} (criterion accumulates every group it's ever seen, including from \
         unrelated past runs -- report::ingest::ingest_bench reads all of it indiscriminately, \
         so a stale group here would otherwise show up as a phantom row in the comparison) \
         then cargo bench -p bench",
        criterion_dir.display()
    );
    if dry_run {
        return Ok(());
    }
    std::fs::remove_dir_all(&criterion_dir).ok(); // fine if it didn't exist yet
    Shell::change_dir(sh, workspace_root());
    cmd!(sh, "cargo bench -p bench").run().context("running cargo bench -p bench")?;
    Ok(())
}

/// `qpp-rng-firmware` (`crates/qpp-rng-firmware/`) has one `[[bin]]` per
/// registered candidate, each named to match `Candidate::name`
/// *exactly* and each calling into only that one candidate's code --
/// see that crate's `src/lib.rs` module doc for why one binary per
/// candidate is necessary (a single binary that picked a candidate at
/// runtime would make every candidate's code reachable at once, which
/// defeats the whole point: the linker could no longer dead-code-
/// eliminate the ones not being measured, so every candidate would
/// report the same "all of them combined" size). The name match means
/// no separate lookup table is needed here -- `candidate.name` doubles
/// as the `--bin` argument directly.
const FIRMWARE_MANIFEST: &str = "crates/qpp-rng-firmware/Cargo.toml";
const FIRMWARE_CRATE: &str = "qpp-rng-reference";

fn run_footprint(args: &CompareArgs) -> anyhow::Result<Vec<PathBuf>> {
    use entropy_timer::{HighResTimer, PlatformTimer};

    println!(
        "[footprint] measuring ticks/output-byte plus real cargo-size/cargo-bloat/cargo-call-stack \
         numbers (one qpp-rng-firmware [[bin]] per candidate) for {} candidate(s)",
        candidates::all_candidates().len()
    );
    if args.dry_run {
        return Ok(Vec::new());
    }

    let dir = args.out_dir.join("footprint");
    std::fs::create_dir_all(&dir)?;
    let firmware_manifest = workspace_root().join(FIRMWARE_MANIFEST);

    let mut paths = Vec::new();
    for candidate in candidates::all_candidates() {
        let mut rng = (candidate.make)(args.seed);
        let mut timer = PlatformTimer;
        timer.init();
        let cycles = footprint::cycles::measure_ticks_per_byte(rng.as_mut(), &mut timer, 10_000);

        let size_report = footprint::size::run_cargo_size(&firmware_manifest, candidate.name, None)?;
        let bloat_report = footprint::size::run_cargo_bloat(
            &firmware_manifest,
            candidate.name,
            FIRMWARE_CRATE,
            None,
        )?;
        let call_stack_report =
            footprint::stack::run_cargo_call_stack(&firmware_manifest, candidate.name)?;
        println!(
            "[footprint]   {:46} {:8.2} ticks/byte  .text={:?}  bloat_crate_bytes={:?}",
            candidate.name, cycles.ticks_per_output_byte, size_report.text_bytes, bloat_report.crate_bytes
        );

        let report = footprint::FootprintReport {
            candidate: candidate.name.to_string(),
            target_triple: None,
            size: size_report,
            bloat: bloat_report,
            call_stack: call_stack_report,
            cycles: Some(cycles),
        };
        let path = dir.join(format!("{}.json", candidate.name));
        std::fs::write(&path, serde_json::to_string_pretty(&report)?)?;
        paths.push(path);
    }
    Ok(paths)
}

fn run_differential(sh: &Shell, args: &CompareArgs, out_path: &std::path::Path) -> anyhow::Result<()> {
    println!("[differential] cargo test -p differential (determinism + parity unit tests + proptest fuzz suite)");
    if !args.dry_run {
        Shell::change_dir(sh, workspace_root());
        cmd!(sh, "cargo test -p differential")
            .run()
            .context("running cargo test -p differential")?;
    }

    println!("[differential] running determinism + parity checks directly for the report");
    if args.dry_run {
        return Ok(());
    }

    let deltas = [101u64, 43, 999, 7, 256, 12];
    let report = differential::run_all(args.seed, &deltas, 256);
    std::fs::write(out_path, serde_json::to_string_pretty(&report)?)?;
    Ok(())
}

fn run_report(
    args: &CompareArgs,
    stats_path: &std::path::Path,
    footprint_paths: &[PathBuf],
    differential_path: &std::path::Path,
) -> anyhow::Result<()> {
    println!("[report] aggregating stats + bench + footprint + differential");
    if args.dry_run {
        return Ok(());
    }

    let stats = report::ingest::ingest_stats(stats_path)?;
    let bench = report::ingest::ingest_bench(&workspace_root().join("target/criterion"))?;
    let footprint = report::ingest::ingest_footprint(footprint_paths)?;
    let differential = report::ingest::ingest_differential(differential_path)?;

    let table = report::build_comparison_table(&stats, &bench, &footprint, differential.as_ref());

    let md_path = args.out_dir.join("comparison.md");
    let csv_path = args.out_dir.join("comparison.csv");
    std::fs::write(&md_path, report::markdown::to_markdown(&table))?;
    std::fs::write(&csv_path, report::csv::to_csv(&table))?;

    println!("[report] wrote {} and {}", md_path.display(), csv_path.display());
    for row in &table.rows {
        println!(
            "[report]   {:32} overall={}",
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

/// `xtask` is invoked as `cargo run --manifest-path xtask/Cargo.toml`
/// (or via a `[alias] xtask = "run --manifest-path xtask/Cargo.toml
/// --"`-style Cargo alias), so its own CWD is `xtask/`, not the
/// workspace root every other crate/tool call below assumes.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask/Cargo.toml always has a parent directory")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_targets_defaults_to_host_when_empty() {
        let targets = resolve_targets(&[]).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].name, "host");
    }

    #[test]
    fn resolve_targets_rejects_unknown_names() {
        let result = resolve_targets(&["not-a-real-target".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn resolve_targets_accepts_every_matrix_entry() {
        for t in target_matrix() {
            let resolved = resolve_targets(&[t.name.to_string()]).unwrap();
            assert_eq!(resolved[0].name, t.name);
        }
    }

    #[test]
    fn workspace_root_contains_the_workspace_manifest() {
        assert!(workspace_root().join("Cargo.toml").is_file());
    }
}
