# QPP-RNG Module: Test Harness & Implementation Plan

## 0. Assumptions (flag if wrong, and I'll adjust)

- Target: `no_std`-capable Rust, eventually running on constrained MCUs (Cortex-M class or similar), but development/validation happens primarily on host (Linux/macOS) first.
- "Original QPP-RNG" = a faithful port of Kuang & Lou's design: Fisher-Yates-shuffle-based permutation sort, elapsed-time jitter harvesting, permutation-count-mod-256 output, optionally the IID variant that uses jitter only to refresh an LCG-seeded pad rather than as the direct output.
- Goal of phase 1 is **not** picking a winner yet — it's building a harness rigorous enough that the comparison is trustworthy, since a "faster" IoT variant that quietly loses entropy is worse than useless in a crypto library.
- You'll eventually want this RNG feeding a PQC KEM/signature scheme (Kyber/Dilithium-class) as a seed source or DRBG conditioning input — the harness should therefore validate against DRBG-seeding-grade standards (SP 800-90B), not just "looks random."

Open questions at the bottom (§9) — worth answering before locking the harness design, particularly the target MCU and whether you have a hardware TRNG peripheral available as a fallback/comparison point.

---

## 1. Why the harness comes before the optimization

QPP-RNG's security claim rests entirely on **empirical entropy quality**, not an algebraic hardness assumption — it's a TRNG, not a DRBG. That means every "IoT-optimized" variant (smaller permutation width, cheaper timer, precomputed pads, etc.) is a new entropy-source claim that needs its own SP 800-90B-style validation. You can't extrapolate "the original passed, so a faster version probably does too." A harness that makes re-running that validation cheap and consistent is what makes rapid iteration on IoT variants *safe* rather than reckless.

---

## 2. Evaluation matrix

Every candidate implementation (reference + each IoT variant) gets scored on the same axes:

| Axis | What it measures | Primary tool |
|---|---|---|
| **Statistical randomness (IID)** | Is raw output independent & identically distributed? | NIST SP 800-90B IID track |
| **Statistical randomness (non-IID fallback)** | Min-entropy estimate if IID assumption fails | NIST SP 800-90B non-IID track |
| **General statistical quality** | Broad pass/fail battery | NIST SP 800-22, ENT |
| **Throughput** | Bytes/sec sustained generation | Criterion benchmarks |
| **Latency** | Time to first byte / time per call (matters for constrained event loops) | Criterion, cycle counters |
| **Footprint — flash/code size** | `.text` size of the RNG module | `cargo bloat`, `cargo size` |
| **Footprint — RAM/stack** | Peak stack depth, static RAM | `cargo call-stack` or manual instrumentation, linker map |
| **Power proxy** | CPU cycles consumed per output byte | Cycle counters (DWT on Cortex-M, `mcycle` on RISC-V, `rdtsc`-equivalent on host) |
| **Jitter source health** | Does the entropy source still produce usable jitter on a simpler MCU with less cache/pipeline complexity? | Custom instrumentation — this is the axis most likely to break for IoT variants |
| **Determinism/portability** | Same code, different platform → still passes stats? | Cross-target runs (host, QEMU, hardware-in-loop) |
| **API/behavioral parity** | Do variants diverge in output shape, error handling, panics? | Differential/property-based tests |

The statistical and jitter-health axes are non-negotiable gates. Everything else is a trade-off you can make consciously once those pass.

---

## 3. Workspace layout

```
qpp-rng/
├── Cargo.toml                     # workspace
├── crates/
│   ├── rng-core/                  # no_std trait definitions, shared types
│   ├── qpp-rng-reference/         # faithful port of Kuang & Lou's original design ("ground truth")
│   ├── qpp-rng-iot/                # IoT-optimized variants, each behind a feature flag
│   │   └── src/
│   │       ├── variant_lut.rs      # precomputed/table-driven permutation instead of runtime sort
│   │       ├── variant_narrow.rs   # smaller permutation width (e.g. n=4 instead of n=8)
│   │       └── variant_hw_jitter.rs# ADC-noise or hardware-timer jitter instead of sort-time jitter
│   ├── entropy-timer/              # abstracts the jitter clock: std Instant vs. Cortex-M DWT cycle counter vs. RISC-V mcycle
│   ├── test-harness/
│   │   ├── stats/                  # orchestrates SP800-90B / SP800-22 / ENT, plus fast native smoke tests
│   │   ├── bench/                  # criterion: throughput & latency
│   │   ├── footprint/              # size/RAM/cycle measurement
│   │   ├── differential/           # proptest-based cross-implementation comparison
│   │   └── report/                 # aggregates everything into one comparison report
│   └── xtask/                      # `cargo xtask compare` — runs the full matrix across all variants & targets
```

Keeping `qpp-rng-reference` and `qpp-rng-iot` as separate crates (not feature flags on one crate) is deliberate: it keeps the "ground truth" implementation untouched and auditable, and stops IoT-specific `#[cfg]` complexity from leaking into the reference port you're benchmarking against.

---

## 4. Core trait abstractions

All implementations converge on a shared interface so the harness can treat them uniformly.

```rust
// crates/rng-core/src/lib.rs
#![no_std]

use rand_core::RngCore; // reuse the ecosystem-standard trait for compatibility

/// Every QPP-RNG variant implements this on top of RngCore, so it slots
/// into the wider Rust crypto ecosystem (e.g. as a rand_core::CryptoRng
/// source once it's been validated enough to claim that marker).
pub trait QppRngSource: RngCore {
    /// Diagnostics needed by the test harness — deliberately kept out of
    /// the hot path so it costs nothing when not being tested.
    fn diagnostics(&self) -> RngDiagnostics;
}

#[derive(Debug, Clone, Copy)]
pub struct RngDiagnostics {
    pub permutation_size_bits: u8,
    pub last_permutation_count: u64,
    pub last_jitter_ns: Option<u64>, // None if the variant uses a non-timing jitter source
}

/// Abstracts the jitter/timing source so the same RNG code can run on
/// host (std::time::Instant), Cortex-M (DWT cycle counter), or RISC-V
/// (mcycle CSR) without the RNG logic caring which.
pub trait JitterClock {
    /// Cheapest possible high-resolution tick — need not be wall-clock.
    fn tick(&mut self) -> u64;
}
```

Do **not** implement `rand_core::CryptoRng` on any variant until it has cleared the SP 800-90B gate — that marker trait is effectively a security claim to downstream code, and it's easy to forget to remove it if a variant later fails re-validation.

---

## 5. Test harness detail

### 5.1 Statistical quality track

Two tiers, deliberately different cost/rigor trade-offs:

**Tier 1 — fast native smoke tests** (pure Rust, run on every `cargo test`, catch obvious regressions in seconds): monobit frequency, runs test, chi-square goodness-of-fit, serial correlation, Shannon entropy estimate. `tinyrand`'s built-in suite is a good reference for scope — small and fast, not a substitute for the real battery.

**Tier 2 — authoritative battery** (run in CI nightly / pre-merge to main, minutes not seconds): generate a large sample (≥1M bytes per SP 800-90B guidance) to a file per implementation, then shell out to:
- NIST's own SP 800-90B reference tool (IID and non-IID tracks)
- NIST SP 800-22 STS reference implementation
- `ent` (Fourmilab)

Rust orchestration is just `std::process::Command` calls plus output parsing — don't reimplement the min-entropy estimators, they're intricate and the whole point is comparing against the trusted reference tooling the original QPP-RNG papers themselves were validated against.

```rust
// crates/test-harness/stats/src/lib.rs (sketch)
pub struct StatReport {
    pub sp800_90b_iid_pass: bool,
    pub min_entropy_estimate: f64,
    pub sp800_22_pass_rate: f32, // fraction of sub-tests passed
    pub shannon_entropy_bits: f64,
}

pub fn run_full_battery(sample_path: &Path) -> anyhow::Result<StatReport> { /* shells out */ }
```

### 5.2 Performance & footprint track

- **Throughput/latency**: `criterion` benchmarks per implementation, run on host first, mirrored on-target later.
- **Code/flash size**: `cargo bloat --release` per crate, or `cargo size` against the actual embedded target triple.
- **Stack/RAM**: `cargo call-stack` (worst-case stack analysis) for the no_std variants; manual high-water-mark instrumentation (fill-pattern + check) as a fallback if that tool doesn't support your target.
- **Cycle count as power proxy**: since you likely can't easily measure real power draw early on, cycles-per-output-byte via the `JitterClock`/DWT counter is a reasonable stand-in until you have hardware-in-loop power measurement.

### 5.3 Cross-target track

Three rungs, increasing fidelity:
1. **Host** — fastest iteration, x86/ARM64 dev machine.
2. **QEMU emulation** — catches `no_std`/linking/ABI issues and lets you sanity-check on an emulated Cortex-M before touching hardware.
3. **Hardware-in-loop** — the only place where jitter-source health is actually meaningful, since sort-timing jitter comes from cache/pipeline effects that QEMU won't model realistically. This is where you'll find out whether a "smaller/simpler MCU with less microarchitectural noise" starves the original algorithm's entropy source — plausibly the single most important thing this whole harness exists to catch.

### 5.4 Differential / property-based testing

Not about randomness quality — about implementation correctness and parity:
- `proptest` fuzzing for panics/overflow across all variants with the same seed corpus.
- Determinism checks: same `JitterClock` mock sequence in → same output out, for variants that are meant to be deterministic given fixed jitter input (useful for unit tests even though production entropy is non-deterministic by design).
- API/error-handling parity checks so swapping the feature flag doesn't silently change behavior downstream.

### 5.5 Reporting

`test-harness/report` aggregates all of the above per implementation into one comparison artifact (Markdown + CSV) so "original vs. variant X" is a single glance, not a spelunking exercise across five tools' output formats. Worth wiring `cargo xtask compare` to regenerate this on demand.

---

## 6. Candidate IoT-optimization directions to test against baseline

Concrete variants worth harnessing (not a commitment — the point of phase 1 is having the tooling to judge these fairly):

1. **Narrower permutation width** (e.g., 4-bit instead of 8-bit permutation space) — smaller state, less entropy per pad, needs SP 800-90B re-validation, not an assumption.
2. **Lookup-table-driven permutation generation** instead of runtime Fisher-Yates sort — trades flash for cycles; changes *where* the jitter timing signal comes from, which is exactly the kind of change that needs the jitter-health axis checked.
3. **Hardware jitter source** (ADC noise, a dedicated TRNG peripheral if the target MCU has one) instead of sort-time jitter — likely the most promising IoT direction if the hardware supports it, since it sidesteps the "does this simple MCU even have enough microarchitectural jitter" risk entirely.
4. **Pad reuse / batching strategies** to amortize sort cost across multiple output bytes — throughput win, needs IID re-validation since reuse patterns are exactly what can break independence.

---

## 7. Milestone roadmap

| Phase | Deliverable |
|---|---|
| 1 | Workspace skeleton, `rng-core` traits, `entropy-timer` abstraction, `xtask` scaffold |
| 2 | Port `qpp-rng-reference` faithfully; get it passing Tier 1 smoke tests |
| 3 | Stand up the full harness (stats Tier 2, bench, footprint, cross-target); establish reference baseline numbers |
| 4 | Implement 2–3 IoT variants from §6, each gated behind the harness before proceeding to the next |
| 5 | Run full comparison matrix; produce the aggregate report; pick a direction (or combination) for the library |
| 6 | Harden the winning variant, add `CryptoRng` marker only after it clears every gate, integrate into the wider crypto library |

---

## 8. Open questions

- **Target MCU(s)**: Cortex-M0/M3/M4? RISC-V? Does it have a hardware TRNG peripheral we should include as a comparison baseline (option 3 in §6)?
- **`no_std` from day one, or std-first with a later port?** Affects whether Tier 2 stats tooling can run directly on-target or only ever on host with on-target sample collection.
- **Threat model / security level target**: is this feeding key generation directly, or conditioning a DRBG seed? Changes how strict the SP 800-90B gate needs to be before anything ships.
- **Existing library structure**: does this RNG module need to conform to a specific trait/API already used elsewhere in your quantum-safe library (e.g., matching whatever seeds your KEM/signature implementations)?
