//! Criterion benchmark groups comparing every registered candidate
//! side by side (see `qpp-rng-testing-architecture.md` §5.2).
//!
//! Three groups, matching the two latency notions the test-harness
//! breakdown asks for plus one throughput group:
//!
//! - `throughput` -- steady-state bytes/sec, reusing one constructed
//!   generator across iterations (a real caller doesn't reconstruct
//!   the generator for every buffer).
//! - `latency_per_call` -- steady-state time for one `next_byte()`
//!   call on an already-constructed, already-"warmed up" generator.
//! - `time_to_first_byte` -- construction (`Candidate::make`, which
//!   includes `entropy_timer::calibrate_resolution`) plus the very
//!   first output byte, timed together via `iter_batched` so
//!   per-iteration construction cost isn't amortized away. This is
//!   what matters for a caller that spins up a fresh generator per use
//!   (e.g. seeding one key generation) rather than keeping one alive.
//!
//! ## Why sample sizes here are small
//!
//! `qpp-rng-reference`'s generators run at roughly 10-15 KB/s on a
//! typical host (each output byte costs `oversample` convergence
//! cycles, each a geometric-distributed Fisher-Yates walk averaging
//! `N!` draws before hitting the identity permutation again -- see
//! `qpp-rng-reference`'s crate docs). Criterion's usual defaults
//! (100 samples, 5s measurement time per benchmark) would make this
//! file take several minutes to run; the [`Criterion::default()`]
//! override below trades statistical precision for a `cargo bench` run
//! that finishes in well under a minute, which matters more for a
//! benchmark meant to be re-run on every candidate change.

use std::time::Duration;

use candidates::all_candidates;
use criterion::{criterion_group, criterion_main, BatchSize, Criterion, Throughput};
use rand_core::Rng;

/// Shared seed for every benchmarked instance. Fine to hardcode: none
/// of these candidates claim the seed itself is the entropy source
/// (see `qpp-rng-reference`'s "Seed evolution" fidelity note) -- the
/// benchmarks measure wall-clock cost, not output quality (that's
/// `test-harness/stats`'s job).
const SEED: u128 = 0x5EED_0000_1111_2222_3333_4444_5555_6666;

/// Bytes generated per `throughput` iteration. Small enough to keep
/// each iteration in the low-single-digit milliseconds at this
/// generator family's real speed.
const THROUGHPUT_SAMPLE_BYTES: usize = 32;

fn fast_config() -> Criterion {
    Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(200))
        .measurement_time(Duration::from_secs(1))
}

fn throughput_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput");
    group.throughput(Throughput::Bytes(THROUGHPUT_SAMPLE_BYTES as u64));
    for candidate in all_candidates() {
        let mut rng = (candidate.make)(SEED);
        let mut buf = [0u8; THROUGHPUT_SAMPLE_BYTES];
        group.bench_function(candidate.name, |b| {
            b.iter(|| {
                rng.fill_bytes(&mut buf);
                std::hint::black_box(&buf);
            })
        });
    }
    group.finish();
}

fn latency_per_call_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("latency_per_call");
    for candidate in all_candidates() {
        let mut rng = (candidate.make)(SEED);
        group.bench_function(candidate.name, |b| {
            b.iter(|| {
                let mut buf = [0u8; 1];
                rng.fill_bytes(&mut buf);
                std::hint::black_box(buf)
            })
        });
    }
    group.finish();
}

fn time_to_first_byte_benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("time_to_first_byte");
    for candidate in all_candidates() {
        let make = candidate.make;
        group.bench_function(candidate.name, |b| {
            b.iter_batched(
                || make(SEED),
                |mut rng| {
                    let mut buf = [0u8; 1];
                    rng.fill_bytes(&mut buf);
                    std::hint::black_box(buf)
                },
                BatchSize::PerIteration,
            )
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = fast_config();
    targets = throughput_benches, latency_per_call_benches, time_to_first_byte_benches
}
criterion_main!(benches);
