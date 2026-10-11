//! Reference-generator trace on a Linux host, in the HIL `ref` format.
//!
//! Runs `QppRngXorshift::from_seed(SEED).with_oversample(1)` exactly as
//! `hil-firmware/avr` does in `ref` mode and writes one 8-byte record per
//! convergence cycle: `n_p` and `Δt` (native timer ticks) as
//! little-endian u32s, the format `hil-firmware/tools/extract.py ref`
//! reads. Prints the calibrated timer resolution and the cycle rate.
//!
//!   cargo run --release -p qpp-rng-firmware --example pi_trace -- <cycles> <out.bin>

use entropy_timer::{HighResTimer, PlatformTimer, calibrate_resolution};
use qpp_rng_reference::{DEFAULT_CALIBRATION_SAMPLES, QppRngXorshift};
use rng_core::QppRngSource;
use std::io::{BufWriter, Write};
use std::time::Instant;

/// Same seed as the HIL reference firmware.
const SEED: u128 = 0x5EED_0000_1111_2222_3333_4444_5555_6666;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cycles: u64 = args.get(1).expect("cycles").parse().expect("cycles");
    let out = args.get(2).expect("out.bin");

    // `QppRng` keeps its k private; this is the same calibration
    // `from_seed` runs, on a separate timer instance.
    let mut timer = PlatformTimer;
    let nominal = timer.init();
    let k = calibrate_resolution(&mut timer, DEFAULT_CALIBRATION_SAMPLES, nominal);
    eprintln!("timer nominal_k={nominal} calibrated_k={k}");

    let mut rng = QppRngXorshift::from_seed(SEED).with_oversample(1);
    let mut w = BufWriter::new(std::fs::File::create(out).expect("create out"));
    let t = Instant::now();
    for _ in 0..cycles {
        let _ = rng.next_byte();
        let d = rng.diagnostics();
        w.write_all(&(d.last_permutation_count as u32).to_le_bytes()).unwrap();
        w.write_all(&(d.last_jitter_ns.unwrap_or(0) as u32).to_le_bytes()).unwrap();
    }
    w.flush().unwrap();
    let s = t.elapsed().as_secs_f64();
    eprintln!(
        "cycles={cycles} seconds={s:.3} cycles_per_s={:.1} default_output_bytes_per_s={:.1}",
        cycles as f64 / s,
        cycles as f64 / s / 5.0
    );
}
