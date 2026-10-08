# How the test harness and `xtask` actually work

A from-the-code walkthrough of everything under `crates/test-harness/*`,
`crates/candidates`, `crates/conditioning`, `crates/qpp-rng-firmware`, and
`xtask` — what each piece does, why it's shaped the way it is, and the
concrete code that does it. Complements
`qpp-rng-testing-architecture.md` (the *plan*, written before any of this
existed) with a description of what actually got built.

## The one-paragraph mental model

[`candidates`](../crates/test-harness/candidates/src/lib.rs) is a single
list of "every QPP-RNG configuration under test." Five independent
crates each answer one question about every entry in that list -- **stats** ("is the output statistically sound?"),
**bench** ("how fast
is it?"), **footprint** ("how much flash/RAM/cycles does it cost?"), **differential** ("is it internally consistent and
panic-free?") -- and
write their answers to disk as JSON. **report** reads all four JSON
outputs back in, joins them by candidate name, and renders one
Markdown/CSV comparison table. **xtask** is the conductor: it builds
each target, then runs stats → bench → footprint → differential →
report in sequence, mostly by calling straight into these crates as
libraries rather than shelling out to each one's own CLI.

---

## 1. The trait foundation: `rng-core`

Everything hangs off two traits in
[crates/rng-core/src/lib.rs](../crates/rng-core/src/lib.rs):

```rust
pub trait QppRngSource: Rng {
    fn diagnostics(&self) -> RngDiagnostics;
}

pub struct RngDiagnostics {
    pub permutation_size_bits: u8,
    pub last_permutation_count: u64,
    pub last_jitter_ns: Option<u64>,
}
```

`Rng` comes from `rand_core` 0.10 (`Rng: TryRng<Error = Infallible>`,
with a blanket impl providing `next_u32`/`next_u64`/`fill_bytes` for any
infallible `TryRng`). `QppRngSource` adds one thing on top: a
`diagnostics()` call exposing what happened internally during the last
byte generated, without that bookkeeping costing anything when nobody's
asking (the field is computed either way in `qpp-rng-reference`, but
nothing about the trait *requires* runtime overhead for it).

Critically, neither `Rng` nor `QppRngSource` has generic methods or a
`Self: Sized` bound, which makes `Box<dyn QppRngSource>` legal Rust --
that's what makes the registry in the next section possible at all.

## 2. `candidates` — the shared registry

[crates/test-harness/candidates/src/lib.rs](../crates/test-harness/candidates/src/lib.rs)
isn't one of the boxes in the original test-harness diagram -- it's glue
every other crate needs, the same way `xtask` sits outside the diagram
but drives it.

```rust
pub struct Candidate {
    pub name: &'static str,
    pub implementation: &'static str,
    pub array_size: usize,
    pub make: fn(seed: u128) -> Box<dyn QppRngSource>,
}

pub fn all_candidates() -> Vec<Candidate> {
    vec![
        Candidate {
            name: "reference-xorshift128plus",
            make: |seed| Box::new(QppRngXorshift::from_seed(seed)),
            ..
        },
        Candidate {
            name: "reference-nextx48",
            make: |seed| Box::new(QppRngNextX48::from_seed(seed)),
            ..
        },
        Candidate {
            name: "reference-xorshift128plus-sha256-conditioned",
            make: |seed| Box::new(Sha256Conditioner::new(QppRngXorshift::from_seed(seed))),
            ..
        },
        Candidate {
            name: "reference-nextx48-sha256-conditioned",
            make: |seed| Box::new(Sha256Conditioner::new(QppRngNextX48::from_seed(seed))),
            ..
        },
    ]
}
```

Four entries today: two raw `qpp-rng-reference` configurations (differing only in which internal PRNG draws permutation
pads --
XORSHIFT128+ or NEXT_X48) and each wrapped in the SHA-256 conditioner.
`stats`, `bench`, `footprint`, and `differential`'s parity checks all
just call `candidates::all_candidates()` and loop -- none of them
hardcode a candidate name, so a fifth entry (a real `qpp-rng-iot`
variant, whenever one exists) needs no changes anywhere else.

**The one thing this registry can't do**: `entropy_timer::HighResTimer`
is a *generic type parameter* on `QppRng<P, T, N>`, not a trait object.
Once `Candidate::make` erases everything down to `Box<dyn QppRngSource>`, there's no way to swap in a scripted mock
clock for
determinism testing -- the concrete `T` is already gone. That's why
`differential::determinism` constructs its own generic instances
directly (§8 below) instead of going through this registry.

## 3. `conditioning` — the SHA-256 conditioner

[crates/conditioning/src/lib.rs](../crates/conditioning/src/lib.rs)
exists because raw `qpp-rng-reference` output has a real, structural
bias: `next_byte()`'s extraction step is `n_p mod 256`, where `n_p` (the
number of Fisher-Yates draws until the permutation-sort walk returns to
identity) is geometrically distributed with mean `N!`. For the paper's
default `N=5` that mean (120) is well under 256, so a single draw's
mod-256 residue is heavily skewed on bit 7 specifically (confirmed by
reproducing the exact bias with a pure Monte-Carlo simulation of just
the extraction math, no timer involved). `oversample` XOR-ing shrinks
it a lot but not to zero.

```rust
pub struct Sha256Conditioner<R> {
    source: R,
    out_buf: [u8; OUTPUT_BLOCK_BYTES],   // 32 bytes -- one SHA-256 digest
    out_pos: usize,
}

impl<R: Rng> TryRng for Sha256Conditioner<R> {
    type Error = Infallible;
    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Infallible> {
        // refill() pulls 64 raw bytes, hashes them, serves the 32-byte
        // digest out before pulling the next block
    }
}

impl<R: QppRngSource> QppRngSource for Sha256Conditioner<R> {
    fn diagnostics(&self) -> RngDiagnostics { self.source.diagnostics() }  // delegates
}
```

64 raw bytes compressed to 32 conditioned bytes (SHA-256's digest size)
-- a 2:1 ratio, checked against real measured raw min-entropy (6.6-7.2
bits/byte from SP 800-90B) rather than picked arbitrarily: `64 ×
6.6 ≈ 425` bits of input entropy into a 256-bit output clears the
standard "output size + 64 bits" SP 800-90B/C safety margin by 100+
bits for both reference configs.

Kept as its **own crate**, not folded into `qpp-rng-reference`, on
purpose: it keeps the raw entropy source -- the thing Tier 2's SP
800-90B evaluation is actually characterizing -- untouched and
independently re-testable, and lets `candidates` register both the raw
and conditioned forms side by side.

**One documented trap**: don't re-run `ea_non_iid` on *conditioned*
output and trust the number. SP 800-90B's estimators are built to find
exploitable short-range predictability in a *raw* noise source; a good
hash defeats that kind of pattern-matching in both directions and can
make a fine source score lower on one specific sub-estimator for
reasons that have nothing to do with real entropy loss (this happened
during development -- see the crate's own doc comment for the numbers).
Tier 1/ENT/STS, which test *uniformity*, are the right checks to run on
conditioned output.

## 4. `test-harness/stats`

### 4.1 Sample generation

[sample.rs](../crates/test-harness/stats/src/sample.rs):
`generate_sample` streams raw bytes from any `QppRngSource` in 64 KB
chunks (no `n_bytes`-sized allocation) and refuses fewer than
`MIN_SAMPLE_BYTES = 1_000_000` -- SP 800-90B's minimum recommended
sample size. `generate_all_candidate_samples` loops
`candidates::all_candidates()` and writes one `.bin` file per
candidate.

### 4.2 Tier 1 -- fast native smoke tests

[tier1.rs](../crates/test-harness/stats/src/tier1.rs), run on every
`cargo test`, no external dependencies:

```rust
pub const ALPHA: f64 = 0.01;   // NIST SP 800-22's conventional significance level

pub fn monobit_frequency(bytes: &[u8]) -> Tier1Metric {
    let n = bytes.len() * 8;
    let sum: i64 = bits(bytes).map(|b| if b { 1 } else { -1 }).sum();
    let s_obs = (sum.unsigned_abs() as f64) / (n as f64).sqrt();
    let p_value = erfc(s_obs / std::f64::consts::SQRT_2);
    Tier1Metric { statistic: s_obs, p_value: Some(p_value), pass: p_value >= ALPHA }
}
```

Five checks total: `monobit_frequency`, `runs_test`,
`chi_square_byte_uniformity` (256-bin goodness of fit, 255 degrees of
freedom), `serial_correlation` (ENT's lag-1 formula), and
`shannon_entropy_bits_per_byte`. The math (`erfc`, the regularized
incomplete gamma function for the chi-square p-value) lives in
[mathfns.rs](../crates/test-harness/stats/src/mathfns.rs) and is tested
two ways: against NIST SP 800-22's own worked examples (§2.1.4's
`n=10` and §2.1.8's `n=100` cases, pulled directly from the PDF), and
against two independent closed-form identities (chi-square with 1
degree of freedom equals a squared normal, so its survival function is
`erfc`; with 2 degrees of freedom it's `Exponential(1/2)`).

**Why Tier 1 is the trustworthy number, not just a rough smoke test**:
it runs once over the *entire* sample directly, so it's extremely
sensitive even to small aggregate bias. It's what actually caught the
MSB modulo-bias, consistently, across every run this whole project.

### 4.3 Tier 2 -- external tool orchestration

[tier2.rs](../crates/test-harness/stats/src/tier2.rs) shells out to
`ent`, `ea_iid`/`ea_non_iid` (NIST SP 800-90B), and `assess` (NIST STS).
Every wrapper follows the same shape: `find_tool` scans `PATH`, and a
missing tool produces `Ok(ToolRun { tool_path: None, .. })`, not an
`Err` -- absence is a normal, reportable state.

`run_sp800_22` is the most involved, because `assess` is menu-driven
over stdin, not flag-driven, and three real bugs were found and fixed
here by testing against the real binary rather than trusting assumptions:

```rust
// STS needs its own templates/ directory (for the NonOverlappingTemplate
// test) present in the CWD, or it silently writes garbage-looking rows
// instead of erroring -- copy it in from next to the `assess` binary:
if let Some(sts_root) = tool_path.parent() {
let templates_src = sts_root.join("templates");
if templates_src.is_dir() & & ! work_dir.join("templates").is_dir() {
copy_dir_recursive( & templates_src, & work_dir.join("templates")) ?;
}
}

// assess's own scanf("%s", ...) truncates any path containing a space --
// always work from a copy in a guaranteed space-free temp path:
let temp_sample_path = std::env::temp_dir().join(sample_path.file_name()...);
std::fs::copy( & sample_path, & temp_sample_path) ?;

// Derive the bitstream count from the real file size instead of
// hardcoding 1 -- both uses the whole sample AND gives STS's own
// uniformity checks real statistical power:
let num_bitstreams = ((sample_bytes * 8) / bitstream_len_bits as u64).max(1);

let stdin_script = format!("0\n{sample_path}\n1\n0\n{num_bitstreams}\n1\n");
//                          ^generator ^path  ^all ^skip  ^bitstreams ^binary
//                           =Input File      tests  params            mode
```

Six stdin answers, not four -- confirmed by reading STS's own C source (`generatorOptions` → `chooseTests` →
`fixParameters` →
`openOutputStreams` → `fileBasedBitStreams`), since guessing wrong here
doesn't error, it just hangs forever on the next unanswered prompt (which looks identical to "still computing" from the
outside).

### 4.4 Putting it together

[report.rs](../crates/test-harness/stats/src/report.rs)'s
`run_full_battery` runs Tier 1 (always) plus whichever Tier 2 tools
`Tier2Options` enables, and folds them into one `StatReport`.
`StatReport::overall_pass()` treats a tool that *ran but didn't parse*
as "can't confirm" (false), not "ignore it" -- a silent parser failure
shouldn't disappear into an accidental pass.

[stats_cli.rs](../crates/test-harness/stats/src/bin/stats_cli.rs)
exposes `generate-samples`, `tier1`, and `full` as subcommands -- the
process boundary `xtask` *could* shell across, though in practice
`xtask` calls the library functions directly (§11).

## 5. `test-harness/bench`

[benches/qpp_rng_benches.rs](../crates/test-harness/bench/benches/qpp_rng_benches.rs)
defines three criterion groups over every candidate: `throughput`
(steady-state, one generator reused across iterations), `latency_per_call`
(one `next_byte()` on an already-warm generator), and
`time_to_first_byte` (construction + first byte together, via
`iter_batched`, for a caller that spins up a fresh generator per use).

```rust
fn fast_config() -> Criterion {
    Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(200))
        .measurement_time(Duration::from_secs(1))
}
```

Criterion's defaults (100 samples, 5s measurement) would make this file
take minutes -- `qpp-rng-reference` runs at roughly 10-300 KB/s
depending on system load, since the entropy signal *is* the cost of the
convergence loop. This trades precision for a `cargo bench` that
finishes in well under a minute.

[lib.rs](../crates/test-harness/bench/src/lib.rs)'s
`export_from_criterion_dir` reads criterion's own on-disk JSON
(`target/criterion/<group>/<candidate>/new/{estimates,benchmark}.json`)
back into a `BenchReport` -- this is a plain library call, not a
CLI-to-CLI handoff, since criterion's `harness = false` binary has its
own `main()` and can't be linked into `xtask` directly.

## 6. `test-harness/footprint`

Four measurements:

- **[cycles.rs](../crates/test-harness/footprint/src/cycles.rs)**:
  wraps any `entropy_timer::HighResTimer`, timing a `fill_bytes` call
  from the outside. Deliberately outside-in (not reaching into
  `QppRng`'s own private jitter timer) so it works uniformly for every
  candidate, including future non-timing-based ones.
- **[size.rs](../crates/test-harness/footprint/src/size.rs)**: `cargo
  size`/`cargo bloat` wrappers. `parse_size_output` handles *two*
  formats -- discovered the hard way, when the parser (written against
  Linux's Berkeley `text data bss` columns) silently returned nothing
  on macOS's real Mach-O output (`__TEXT __DATA __OBJC others`):
  ```rust
  if header.starts_with("__TEXT") {
      (parse_col(0), parse_col(1), None)   // no bss-equivalent column in Mach-O
  } else {
      (parse_col(0), parse_col(1), parse_col(2))
  }
  ```
- **[stack.rs](../crates/test-harness/footprint/src/stack.rs)**: a
  real fill-pattern high-water-mark primitive (`paint`/`high_water_mark`,
  the `0xAA`-sentinel technique), plus a `cargo call-stack` wrapper.
  Neither produces real numbers today: `call-stack` needs a `no_std`
  ELF built with `-Z emit-stack-sizes` on a very specific pinned
  nightly (`nightly-2023-11-13` -- it refuses anything newer), and the
  fill-pattern approach needs to paint the *real* hardware stack at
  boot, which only makes sense on real embedded firmware.
- **[report.rs](../crates/test-harness/footprint/src/report.rs)**:
  `FootprintReport::headline_text_bytes()` prefers `cargo size`'s
  number, falling back to `cargo bloat`'s per-crate figure.

## 7. `qpp-rng-firmware` -- the missing `[[bin]]`

`cargo-bloat`/`cargo-size`/`cargo-call-stack` inspect a *linked
executable's* `.text`/`.data`/`.bss` sections. `qpp-rng-reference` and
`qpp-rng-iot` are `[lib]` targets -- no such sections exist until
something with a real `fn main()` links them in.
[crates/qpp-rng-firmware](../crates/qpp-rng-firmware) is that
something: a `[lib]` with one shared `dump_sample` helper, plus **one
`[[bin]]` per candidate**, each named to match `Candidate::name`
exactly:

```toml
[[bin]]
name = "reference-xorshift128plus"
path = "src/bin/xorshift128plus.rs"
```

```rust
// src/bin/xorshift128plus.rs
fn main() {
    qpp_rng_firmware::dump_sample(qpp_rng_reference::QppRngXorshift::from_seed(qpp_rng_firmware::SEED));
}
```

**Why one binary per candidate, not one binary with a `--candidate`
flag**: if every candidate's code were reachable from one `main()`, the
linker couldn't dead-code-eliminate the ones not being measured --
`cargo size` would report "all candidates combined" for every single
one. Confirmed this reasoning holds in practice, not just in theory:
`cargo bloat`'s per-crate breakdown shows `qpp-rng-reference` costing
exactly `732` bytes in both the xorshift128+ binary and its conditioned
variant, and a different `628` bytes in both NEXT_X48 binaries --
consistent within each PRNG choice, different across them, exactly as
dead-code elimination should produce.

## 8. `test-harness/differential`

Not about randomness quality -- about implementation correctness.

**[mock_clock.rs](../crates/test-harness/differential/src/mock_clock.rs)**:
a scripted `HighResTimer` replaying a fixed delta sequence, used
wherever a test needs `QppRng`'s convergence-cycle timing pinned to an
exact, reproducible value instead of real (non-reproducible) jitter.

**[determinism.rs](../crates/test-harness/differential/src/determinism.rs)**:
runs each of the four configurations *twice* against an identical mock
script and seed, and requires identical output:

```rust
fn check_xorshift128plus_determinism(seed: u128, deltas: &[u64], n_bytes: usize) -> DeterminismResult {
    let mut a = QppRng::<Xorshift128Plus, MockClock, DEFAULT_ARRAY_SIZE>::new(..);
    let mut b = QppRng::<Xorshift128Plus, MockClock, DEFAULT_ARRAY_SIZE>::new(..);  // same seed, same script
    ...
        diff("reference-xorshift128plus", &buf_a, &buf_b)
}
```

Constructed directly and generically (mirroring, not reusing, the
`candidates` registry) for exactly the reason in §2 -- `MockClock`
needs the concrete generic type the registry has already erased. The
two `-sha256-conditioned` checks wrap the same construction in
`Sha256Conditioner`, which works cleanly since the conditioner is
itself generic over `R: Rng`.

**[parity.rs](../crates/test-harness/differential/src/parity.rs)**: *does*
use the registry (real timer, no mock-clock need) to check structural
invariants across every candidate -- does `fill_bytes` on an empty
buffer panic, does it actually write to the buffer (checked via
whole-buffer inequality against a sentinel, not per-byte, since roughly
`1/256` of bytes coincidentally matching any fixed sentinel is normal,
not a sign of a no-op), and does `diagnostics()` self-report
consistently (`permutation_size_bits` independently re-derived from
`array_size` and compared, not trusted blindly).

**[fuzz.rs](../crates/test-harness/differential/src/fuzz.rs)**: proptest
across every array width `N` from 1 to 8, gated `#[cfg(test)]` (no
public API, so it doesn't add dead weight to normal builds). `N≥7` gets
a much smaller buffer-length strategy than `N≤6` -- a convergence
cycle's expected cost is `O(N!)` draws, so `N=8`'s `40320`-draw mean
would otherwise turn one fuzz function into a multi-minute outlier for
no extra coverage value.

## 9. `test-harness/report`

**[ingest.rs](../crates/test-harness/report/src/ingest.rs)**: reads
back `stats.json`, criterion's directory (via `bench`'s own exporter
function, called directly -- no JSON round-trip needed since both live
in the same process), the footprint JSON files, and `differential.json`.
Every function is forgiving of a *missing* file (a track that was
skipped renders as "N/A"); only a file that exists but fails to parse
is a real error.

**[table.rs](../crates/test-harness/report/src/table.rs)**:
`build_comparison_table` joins everything by candidate name into a
`BTreeMap`, and:

```rust
pub fn overall_pass(&self) -> Option<bool> {
    let gates = [self.tier1_pass, self.deterministic, self.api_parity_pass];
    if gates.iter().all(Option::is_none) {
        return None;   // nothing to judge, not "vacuously true"
    }
    Some(gates.into_iter().flatten().all(|p| p))
}
```

This used to return plain `bool`, and `[None, None, None].flatten().all(...)`
being vacuously `true` in Rust meant a row with *zero* real data (once,
literally a stray candidate name that only ever showed up in stale
`target/criterion` output from an unrelated old bench run) rendered as
a clean pass. Fixed by returning `Option<bool>` explicitly.

**[markdown.rs](../crates/test-harness/report/src/markdown.rs)** /
**[csv.rs](../crates/test-harness/report/src/csv.rs)**: render the same
`ComparisonTable` as a GFM table and RFC 4180 CSV respectively -- CSV
hand-rolled rather than a dependency, since every field here is a
number, `true`/`false`, or a `[a-z0-9-]+` candidate name with no real
quoting edge cases.

## 10. `xtask`

A **separate top-level Cargo workspace** (root `Cargo.toml` explicitly
`exclude`s it), so its own dependencies (`xshell`, `clap`) don't affect
the library workspace's resolution. `.cargo/config.toml` aliases
`cargo xtask` to `cargo run --manifest-path xtask/Cargo.toml --`, so it
still feels like one command from the workspace root.

**[target_matrix.rs](../xtask/src/target_matrix.rs)**: defines the
host/QEMU/hardware-in-loop matrix. Only `host` has actually been
exercised; the others are real, considered definitions (correct triples,
plausible `probe-rs` chip identifiers where that tool actually applies
-- explicitly *not* for the Arduino Uno entry, since classic AVR chips
have no SWD/JTAG for `probe-rs`'s flash+RTT model to use) but explicitly
flagged as unverified.

**[hil.rs](../xtask/src/hil.rs)**: `ProbeRsFlasher` (flash + run via
`probe-rs`), `RttTelemetry`/`UartTelemetry` (pull raw bytes back off a
target). Real code against each tool's documented CLI, never run
against actual hardware.

**[compare.rs](../xtask/src/compare.rs)** is the orchestrator. `cargo
xtask compare` runs, in order:

1. `build_target` -- `cargo build -p qpp-rng-reference -p qpp-rng-iot`
   for each requested target (skipping cleanly if that target's
   toolchain isn't installed).
2. `run_stats` -- calls `stats::sample::generate_all_candidate_samples`
   and `stats::report::run_full_battery` **directly as library
   functions**, not via `stats-cli`.
3. `run_bench` -- the one step that *has* to shell out (`cargo bench`
   has its own `main()`), first deleting `target/criterion` so stale
   data from an unrelated prior run never resurfaces as a phantom row.
4. `run_footprint` -- loops every candidate, measuring cycles in-process
   and calling `footprint::size::run_cargo_size`/`run_cargo_bloat`
   against that candidate's own `qpp-rng-firmware` binary.
5. `run_differential` -- both `cargo test -p differential` (the proptest
   suite) and a direct `differential::run_all` call for the JSON report.
6. `run_report` -- `report::ingest::*` + `report::build_comparison_table`
    + `report::markdown::to_markdown`/`csv::to_csv`, all direct calls.

Most of this pipeline is library calls, not process-to-process CLI
hand-offs, specifically so `xtask` can build one unified
`ComparisonTable` without round-tripping every intermediate result
through a JSON file on disk. `xshell` is reserved for the handful of
things that genuinely need a separate process: cross-compiling, running
criterion's own binary, and (in `hil.rs`) talking to `probe-rs`.

---

## Known limitations, gathered in one place

- Raw candidates carry a real, structural MSB bias (§3) -- always
  condition before using QPP-RNG output as key material.
- Nothing has been validated on anything but this one host machine.
  Timing-jitter entropy is platform-sensitive by nature (see
  `qpp-rng-reference`'s own documented near-failure on a Windows
  desktop's `QueryPerformanceCounter`), and no hardware-in-loop run has
  happened yet.
- `qpp-rng-iot` is still the literal `cargo new` stub.
- `cargo call-stack` and the manual stack high-water-mark technique
  both need real embedded firmware to produce anything at all.
- `.text` size is page-aligned and thus identical across binaries on
  macOS's Mach-O format; `cargo bloat`'s per-crate breakdown is the
  number that actually discriminates on this platform.
