# Hardware-in-the-loop results: QPP-RNG on three IoT boards

Recorded 2026-10-08 on the bench attached to this machine. Nothing here
is committed; every number below can be regenerated with the commands in
[Reproducing](#reproducing).

## TL;DR

1. **The reference QPP-RNG (`qpp-rng-reference`) has zero entropy on all
   three MCUs.** From a reset, every board produced a **bit-for-bit
   identical** `(n_p, Δt)` trace on every run. On bare-metal cores with
   no cache, OS or other bus masters, the convergence loop's cycle count
   is a pure function of the seed, so the generator turns into a PRNG
   keyed by its compile-time seed. Statistical tests can't see this:
   the deterministic output passes Tier-1 and scores ~4.9 bits/sample on
   per-run min-entropy estimators. Only the cross-reset comparison shows
   it. The host (Apple Silicon/macOS) is fine: the same seed gives a
   different stream on every run.
2. **The fix is a physically independent clock, not more compute.** On
   AVR, the watchdog's 128 kHz RC oscillator, timestamped by the
   crystal-driven Timer1, jitters by σ ≈ 11–25 cycles per 16 ms period
   and gives **1.6–2.2 bits/sample** of min-entropy (NIST `ea_non_iid`, 1 M samples per board; 2.1–3.0 on the first 20k
   samples).
3. **`qpp-rng-iot` is implemented** (it was a `cargo new` stub). It keeps
   the QPP structure and draws its jitter from a `JitterSource`. It adds
   SP 800-90B continuous health tests, makes the walk about 7.8× cheaper
   per pad draw on AVR, and uses `N = 6` to remove the reference's
   structural `n_p mod 256` bias. On both AVRs its output **differs
   across resets** (0/40 positional matches) and **passes every Tier-1
   and `ent` test** on 10 kB per board, at **7.4 B/s**. That rate is set
   by the entropy source and backed by ≥ 8 credited bits per byte; the
   reference's 6 B/s on the same boards carried 0 bits.
4. **nRF52840:** its LFRC-vs-CYCCNT cross-clock source turned out weak (0.61 bits/sample per `ea_non_iid`), but its
   on-chip hardware RNG
   assesses at 6.89 bits/byte. With that as the `JitterSource`, the IoT variant
   differs across resets (0/200 positional matches) and passes every
   Tier-1/`ent` test on 50 kB at **101 B/s**. The raw RNG bytes alone *fail* the runs test (p = 0.006); after the QPP
   walk they pass.

## The bench

| Board                   | MCU / clock                                   | Port / probe                                                                         | Flash/serial path                         |
|-------------------------|-----------------------------------------------|--------------------------------------------------------------------------------------|-------------------------------------------|
| Arduino Mega 2560       | ATmega2560, 16 MHz                            | `/dev/cu.usbmodem11201`                                                              | avrdude `wiring` @115200, USART0          |
| Arduino Nano (FT232R)   | ATmega328P (sig `1E 95 0F`, optiboot), 16 MHz | `/dev/cu.usbserial-AB0LRIQV`                                                         | avrdude `arduino` @115200, USART0         |
| makerdiary nRF52840 MDK | nRF52840 Cortex-M4F, 64 MHz (HFXO on)         | DAPLink CMSIS-DAP + `/dev/cu.usbmodem111102`, `/dev/cu.usbmodem21102` after a replug | probe-rs; UART0 TX = P0.20 → DAPLink VCOM |

The original flash contents of all three boards were saved before
anything was written: `hil-results/flash-backups/`
(`mega2560-original.hex`, `nano328p-original.hex`,
`nrf52840-original.bin`).

The host workspace baseline (`cargo test --workspace --release`) passed
in full before any change, and passes again after.

## 1. Reference generator: determinism across resets

Firmware: `hil-firmware/{avr,nrf52840}` in `ref` mode. It runs
`QppRngXorshift::from_seed(SEED)`, the paper's default (`N = 5`,
xorshift128+, calibrated `k`), with `oversample = 1` so every
convergence cycle's `(n_p, Δt)` reaches the host. The default
oversample-5 output is the XOR of 5 consecutive `n_p mod 256`, so it is
fully determined by this trace. The reference crate is built at the
paper-mandated `-O0`, the same pin the root workspace uses.

| Board                              | Runs (resets) | Records/run | Identical?        | Cycles/pad draw | Compute-bound output |
|------------------------------------|---------------|-------------|-------------------|-----------------|----------------------|
| Mega 2560                          | 3             | 200–300     | **yes, 100%**     | 4,374           | 5.95 B/s             |
| Nano                               | 2             | 300         | **yes, 100%**     | 4,212           | 6.15 B/s             |
| Nano, reference at `opt-level="s"` | 2             | 300         | **yes, 100%**     | 4,259           | 5.87 B/s             |
| nRF52840                           | 3             | 2,000       | **yes, 100%**     | 364             | 287 B/s              |
| Host (M-series Mac), control       | 3             | 64 B        | no, all different | n/a             | n/a                  |

Why: `Δt` is a deterministic function of the PRNG draws (number of
pads, Lemire rejections, Timer1-overflow ISR hits). All of those are
functions of the seed, and the next seed is a function of `Δt`. There's
no independent noise anywhere in the loop. The Nano's trace differs from
the Mega's (different code layout gives different cycle counts), but
each board repeats itself exactly. Building the reference at `"s"`
instead of `-O0` changes neither the determinism nor, notably, the
speed: the 64/128-bit arithmetic (u64 Lemire products, xorshift128+,
the u128 seed shift) dominates on an 8-bit core either way.

Statistics are blind to this. The nRF trace's `Δt` low byte scores
4.93 bits/sample (MCV/t-tuple) on the same estimators used below, and
the reconstructed default output passes all Tier-1 tests that apply at
400 bytes.

## 2. Candidate entropy source: cross-clock jitter

`rawsrc` mode records, per sample, a timing measurement of an oscillator
that is independent of the CPU clock:

- **AVR**: watchdog interrupt (128 kHz RC oscillator, shortest ~16 ms
  period), timestamped with Timer1 counting the 16 MHz crystal (`hil-firmware/avr/c/wdt_jitter.c`). Sample = period in
  CPU cycles.
- **nRF52840**: RTC0 on the 32.768 kHz LFRC (RC oscillator), CYCCNT at
  each 32nd edge (~1 ms), plus one byte from the on-chip RNG peripheral (thermal noise, bias correction on) per sample.

20,000 samples per board (~5.5 min per AVR, 35 s on the nRF):

| Source                | Mean period            | σ    | σ of drift-free component¹ | Lag-1 autocorr | Python subset (min of MCV, t-tuple, Lag, MultiMCW) | **NIST `ea_non_iid` (min(H_original, 8·H_bitstring))** | Binding estimator          | Credited |
|-----------------------|------------------------|------|----------------------------|----------------|----------------------------------------------------|--------------------------------------------------------|----------------------------|----------|
| Mega 2560 WDT         | 271,514 cyc (16.97 ms) | 18.3 | 10.7                       | 0.65           | 2.14                                               | 2.14 (20k); **1.62 bits/sample (1 M)**                 | t-tuple                    | 1        |
| Nano WDT              | 265,950 cyc (16.62 ms) | 45.1 | 24.9                       | 0.69           | 2.98                                               | 2.98 (20k); **2.24 bits/sample (1 M)**                 | t-tuple                    | 1        |
| nRF52840 LFRC ×32 ²   | 111,514 cyc            | 37.0 | 51.1                       | **−0.91**      | 0.72                                               | 0.61 (20k); **0.60 bits/sample (1 M)**                 | MultiMMC                   | unused   |
| nRF52840 hardware RNG | n/a                    | n/a  | n/a                        | n/a            | 6.97                                               | 6.89 (20k); **7.24 bits/byte (1 M)**                   | bitstring compression (×8) | 4        |

¹ RMS of successive differences ÷ √2, which removes slow thermal drift.

² Only 42 distinct values, with strong negative lag-1 correlation. The
32.768 kHz RC oscillator is far more stable than the AVR watchdog's, so
what varies is mostly the quantization of polling the RTC `COUNTER`
register (a timestamp that lands late on one edge lands early on the
next). This capture also includes the RNG read between samples, so it
is not a clean LFRC-only measurement, but there's no reason to prefer
this source when the chip has a TRNG. Credited: unused.

Assessed values are from NIST's own `ea_non_iid -v <file> 8` (full
output in `hil-results/raw/ea_non_iid-*.txt`); every input is 20,000
samples, below the tool's 1 M guidance, which it warns about. The quick
pure-Python subset in `hil-firmware/tools/entropy.py` agrees exactly on
MCV and t-tuple, which were the binding estimators on both AVRs. It
misses the estimators that bound the nRF sources (MultiMMC,
compression), and its MultiMCW is less precise, so use it only as a
bench-side sanity check. **Credited:
1 bit/sample on AVR**, about half the lower board's assessment, and **4 bits/byte for the nRF52840 hardware RNG** (the
highest value the
health-test table covers; ~2.9 bits below the assessment).

A second, independent capture on each board also differed from the
first from the very first sample (nRF RNG: 79/20,000 positional
matches, against 78 expected by chance), which is the property the
reference lacks.

The raw samples fail Tier-1 badly (χ² ≈ 10⁵, serial correlation
0.4–0.65), as they should. They are a raw noise source with slow drift,
not uniform bytes, which is why the generator below conditions them.

## 3. `qpp-rng-iot`: what changed and why

Code: `crates/qpp-rng-iot/src/{lib,pad,health}.rs`, 15 unit tests.

| Change             | Reference                                          | IoT                                                                                                 | Why                                                                                                       |
|--------------------|----------------------------------------------------|-----------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------------------------|
| Entropy source     | timing of its own loop, CPU-clocked                | `JitterSource` trait: an independent oscillator measured in CPU ticks                               | §1: own-loop timing is deterministic on MCUs                                                              |
| Health tests       | none                                               | SP 800-90B §4.4 RCT + APT (W = 512, α = 2⁻²⁰) on every raw sample → `HealthError`                   | a stuck or degraded source must fail loudly, not silently                                                 |
| Pad PRNG           | xorshift128+, 128-bit seed, 64-bit Lemire          | xorshift32 seeded by fmix32 of a 64-bit seed register, 8×8-bit Lemire with a lookup-table threshold | PRNG only expands the seed; u64/u128 math is what kills 8-bit cores                                       |
| Walk               | build pad array → gather into new array → `memcmp` | Fisher–Yates swaps applied directly to the state (`compose_fresh_pad`) + early-exit identity check  | provably the same walk and the same draws (tested); half the memory traffic                               |
| `N`                | 5 (mean n_p = 120 < 256)                           | 6 (mean 720)                                                                                        | `n_p mod 256` at N = 5: bit 7 set 25.7% per cycle, still 52.8% after XOR-ing 4. At N = 6: 45.6% → 50.003% |
| Entropy accounting | 1 timing sample per walk, 5 walks/byte, fixed      | samples/byte = ⌈8 / credited bits⌉, spread over a fixed 4 walks/byte                                | decouples "enough entropy" from "enough compute"                                                          |
| API                | `Rng` (infallible)                                 | `TryRng<Error = HealthError>`; `PanicOnHealthFailure` wrapper implements `QppRngSource`             | errors are reportable on-device                                                                           |

The first version failed its own host-side uniformity test (χ² = 428 on
40k bytes with `N = 5`), which led to the `N = 6` change. The
`n_p mod 256` bias figures above were computed exactly from the
geometric distribution, not estimated.

### Optimization history on the ATmega2560 (compute only, synthetic source)

| Step                                                                 | Timer1 ticks per output byte | vs. previous |
|----------------------------------------------------------------------|------------------------------|--------------|
| First IoT build (N = 6, 8 walks/byte, `256 % bound` per draw)        | 10,733,218                   | n/a          |
| Lemire threshold from lookup table (AVR has no divider)              | 4,585,746                    | 2.3×         |
| Fused pad generation + composition, early-exit identity check        | 2,863,887                    | 1.6×         |
| 4 walks/byte, 2 samples per walk (accounting decoupled from compute) | **1,621,365**                | 1.8×         |

Overall **6.6×** less compute per byte. Per pad draw: reference ≈ 1,090
cycles per Fisher–Yates index → IoT ≈ 113, i.e. **9.7× cheaper per
index** and about 7.8× per pad, even though each IoT pad has one more
index (N = 6). A further `opt-level=3` build and a `u8` loop index made
no measurable difference (LLVM had already narrowed both).

At 1.62 M cycles (101 ms) per byte against 8 watchdog samples (8 × 17 ms = 136 ms) per byte, the AVR generator is now
**source-bound**:
the walk finishes while the watchdog ISR is still buffering the next
samples.

## 4. `qpp-rng-iot` on hardware

Firmware: `iot` mode, 4 walks of N = 6 per byte. AVR: watchdog source,
credit 1 bit/sample → 8 samples/byte. nRF52840: hardware-RNG source,
credit 4 bits → 4 samples/byte.

| Board     | Cross-reset (2 resets)                  | Bytes captured    | Throughput   | Health failures | Flash (whole firmware) |
|-----------|-----------------------------------------|-------------------|--------------|-----------------|------------------------|
| Mega 2560 | **different**, 0/40 positional matches  | 10,000 (22.6 min) | **7.36 B/s** | 0               | 6,470 B                |
| Nano      | **different**, 0/40 positional matches  | 10,000 (22.2 min) | **7.52 B/s** | 0               | 6,330 B                |
| nRF52840  | **different**, 0/200 positional matches | 50,000 (8.2 min)  | **101 B/s**  | 0               | 2,940 B                |

Firmware sizes include UART code, the startup compute benchmark and two
monomorphizations (real and synthetic source). The reference firmware is
8,912 B (Mega), 8,780 B (Nano) and 3,292 B (nRF52840), built at -O0.

Throughput, side by side (output bytes/s; entropy-backed means at least
8 credited raw bits behind each byte):

| Board     | Reference (0 bits of entropy) | IoT (entropy-backed) | IoT bound by                                                                            |
|-----------|-------------------------------|----------------------|-----------------------------------------------------------------------------------------|
| Mega 2560 | 5.95                          | 7.36                 | watchdog rate (compute 101 ms/byte < sampling 136 ms/byte)                              |
| Nano      | 6.15                          | 7.52                 | watchdog rate                                                                           |
| nRF52840  | 287                           | 101                  | compute: 680k cycles/byte (10.6 ms); the RNG supplies 4 bytes in ~3 ms (see note below) |

On the nRF52840 the IoT variant is slower than the reference, because
`N = 6` walks are 6× longer than `N = 5`. Per pad draw the IoT walk is
still 1.5× cheaper there (236 vs 364 cycles). If throughput matters more
than keeping the QPP structure on this chip, the RNG peripheral alone
is limited to roughly 1.5 kB/s with bias correction on (recalled from
the datasheet's ~0.7 ms/byte, not re-checked; the raw capture's 1.74 ms
per record, of which ~1 ms is the 32-tick LFRC wait, is consistent
with it).

Statistics on the outputs (workspace `stats-cli tier1` plus Fourmilab
`ent`):

| Board                                            | Monobit p | Runs p       | χ² (255 dof) p      | Serial corr. | Shannon (bits/B) | ent mean | ent π error |
|--------------------------------------------------|-----------|--------------|---------------------|--------------|------------------|----------|-------------|
| Mega 2560                                        | 0.91 ✅   | 0.45 ✅      | 277.4, p = 0.16 ✅  | −0.0025 ✅   | 7.980 ✅         | 126.87   | 0.27%       |
| Nano                                             | 0.87 ✅   | 0.99 ✅      | 243.5, p = 0.69 ✅  | −0.0020 ✅   | 7.982 ✅         | 127.39   | 0.27%       |
| nRF52840 (50 kB)                                 | 0.17 ✅   | 0.80 ✅      | 239.8, p = 0.74 ✅  | −0.0010 ✅   | 7.997 ✅         | 127.56   | 0.45%       |
| *nRF52840 raw RNG bytes, for comparison (20 kB)* | 0.26 ✅   | **0.006 ❌** | 294.6, p = 0.045 ✅ | −0.0005 ✅   | 7.989 ✅         | 127.84   | 0.74%       |

Caveats, stated plainly:

- 10–50 kB is far below SP 800-90B's 1 MB guidance. At 7.4 B/s, 1 MB
  takes about 38 hours per AVR board (about 2.9 hours on the nRF).
  These are smoke-test numbers, not a validation.
- Passing uniformity tests is necessary, not sufficient. Section 1 shows
  a zero-entropy generator passing them too. The evidence that matters
  is (a) cross-reset divergence and (b) the raw-source entropy estimate
  in section 2. The output's entropy claim rests on (b).
- The credited values rest on `ea_non_iid` runs over 20,000 samples per
  source. Repeat them on ≥ 1 M samples (about 4.7 h of watchdog capture
  per AVR) before quoting them as final.
- Output is raw entropy-source output. Condition it (the existing
  `conditioning::Sha256Conditioner`, 2:1) before using it as key
  material: about 3.7 B/s of full-entropy output on AVR, roughly 9 s
  to seed a 256-bit DRBG at boot.

### NIST SP 800-90B IID track (`ea_iid`) on the outputs

Full output: `hil-results/raw/ea_iid-*.txt`.

| Stream                                      | Permutation tests (19) | LRS  | χ² goodness of fit | χ² independence                | H_original |
|---------------------------------------------|------------------------|------|--------------------|--------------------------------|------------|
| Mega 2560 IoT (10 kB)                       | pass                   | pass | p = 0.15           | **score 951 / 735 dof, fail**  | 6.91       |
| Nano IoT (10 kB)                            | pass                   | pass | p = 0.38           | **score 946 / 735 dof, fail**  | 7.05       |
| nRF52840 IoT (50 kB)                        | pass                   | pass | p = 0.86           | p = 0.016, pass                | 7.54       |
| nRF52840 raw RNG (20 kB)                    | pass                   | pass | pass               | pass                           | 7.26       |
| *control:* `/dev/urandom`, 3 × 10 kB        | pass                   | pass | n/a                | **score 861–960, fail 3/3**    | n/a        |
| *control:* nRF IoT output, first/last 10 kB | pass                   | pass | n/a                | **score 1010 / 954, fail 2/2** | n/a        |

The AVR χ² independence failures are a **sample-size artifact, not a
property of the generator**. The same tool fails the identical way on
10 kB of `/dev/urandom` every time, and on 10 kB slices of the nRF
stream that passes at 50 kB. With 256 symbols and 10,000 samples the
expected count per symbol pair is ~0.15, so the test pools nearly all
pairs into a few bins, and its p-value isn't calibrated there (NIST
specifies ≥ 1 M samples). A direct check on the AVR outputs (pairwise
χ² on high/low nibbles and on bits 0 and 7, at lags 1–5 and 8) found
no consistent dependence. The one outlier (Nano, bit 7 at lag 3,
z = 5.3) appears on neither the other board nor any other lag. A
longer AVR capture is still needed to close this out properly.

SP 800-22 (`assess`) was not run. The harness drives it with
1,000,000-bit streams, and the largest capture here is 400,000 bits; at
these lengths several STS tests don't apply and the rest have little
power.

### NIST SP 800-90B non-IID track on the outputs

Run through the workspace harness itself (`stats-cli full
--skip-sp800-22`, from a login shell, no workarounds; JSON in
`hil-results/raw/stats-cli-full-iot.json`) and directly with
`ea_non_iid -v` (`hil-results/raw/ea_non_iid-*-iot.txt`):

| Stream                                | `ea_non_iid`                              | Binding estimator                 |
|---------------------------------------|-------------------------------------------|-----------------------------------|
| Mega 2560 IoT (10 kB)                 | 5.81 bits/byte                            | bitstring compression (0.726 × 8) |
| Nano IoT (10 kB)                      | 5.75 bits/byte                            | bitstring compression (0.718 × 8) |
| nRF52840 IoT (50 kB)                  | 6.47 bits/byte                            | n/a                               |
| *control:* `/dev/urandom`, 12 × 10 kB | **5.69 – 6.64** (compression 0.71 – 0.85) | bitstring compression             |
| *control:* `/dev/urandom`, 50 kB      | 6.48                                      | n/a                               |

At these sample sizes `ea_non_iid` cannot score even perfect randomness
above ~6.5 bits/byte, and the AVR outputs fall inside the spread of
`/dev/urandom` at the same size: 3 of the 12 control samples score
lower. Every byte-level estimator (MCV, t-tuple, LRS, MultiMCW, Lag,
MultiMMC, LZ78Y) scores the AVR outputs on par with `/dev/urandom`
(6.9–7.8 against 7.1–7.8). These numbers say nothing against the
generator, but they also can't confirm 8 bits/byte. For outputs, the
entropy claim rests on the raw-source assessment above, as SP 800-90B
intends; the earlier note in `docs/how-it-works.md` §3 about running
`ea_non_iid` on processed output applies here too.

### NIST tool setup on this Mac

Fixed during this session: `/etc/paths.d/sp800-90b` and `sp800-22` now
point to `~/adrian-software/...`, and Homebrew `libomp` is installed.
`ea_iid`, `ea_non_iid` and `assess` resolve in any login shell, and
`stats-cli full` finds all three plus `ent` without help. (As
`docs/environment-setup.md` gotcha #4 says, `paths.d` only reaches login
shells; a process started before the change, or an IDE launched from
the Dock, still needs `PATH` set explicitly.) The first `ea_*` runs
above were made before the fix with
`DYLD_LIBRARY_PATH` pointing at a conda `libomp`; the harness re-run
after the fix reproduced the same numbers.

### Harness fix: IID verdicts were discarded

`stats::tier2::run_sp800_90b` kept only `ea_iid`'s entropy numbers and
dropped its `** Passed` / `** Failed` verdict lines. So the Mega/Nano
χ² independence failure showed up as `overall_pass=true` with no trace
in the JSON. More importantly, `StatReport::min_entropy_estimate()`
would fall back to the IID-track estimate when the non-IID track was
missing, even if the IID tests had rejected the IID assumption, which
SP 800-90B §5 doesn't allow. Changes:

- `Sp80090bResult` gains `iid_tests_passed: Option<bool>`
  (`#[serde(default)]`, so older JSON still loads), parsed from the
  verdict lines.
- `min_entropy_estimate()` only falls back to the IID track if its tests
  didn't fail. (It already preferred non-IID, so no existing number
  changes.)
- 3 new tests (`tier2::tests::iid_verdict_*`,
  `report::tests::failed_iid_track_is_not_used_as_min_entropy_fallback`).
  The workspace now passes 118 tests; `xtask` still builds.

## Long captures (started 2026-10-08 10:16)

Unattended runs via `hil-firmware/tools/long-capture.sh <board>` under
`caffeinate -ims` (this Mac's system sleep is set to 1 minute). Each
board runs two phases: first 1,000,001 raw-source records (1 M samples
for SP 800-90B), then 250,000 IoT records (1,000,000 output bytes).
Files are written incrementally to `hil-results/long/`
(`<board>-{rawsrc,iot}.{bin,meta}`, `status=` in the `.meta`, log in
`<board>.log`). `hil-firmware/tools/long-status.sh` prints progress and ETAs.

| Board     | Raw phase                             | IoT phase                                     | Expected finish                                              |
|-----------|---------------------------------------|-----------------------------------------------|--------------------------------------------------------------|
| nRF52840  | **done**, 1,000,001 records in 29 min | **done**, 1,000,000 bytes in 2.75 h (101 B/s) | finished 13:30 on 2026-10-08                                 |
| Mega 2560 | running, 58.9 rec/s                   | queued                                        | raw ~15:05 on 10-08; IoT ~37.5 h later, ~04:40 on 2026-10-10 |
| Nano      | running, 60.1 rec/s                   | queued                                        | raw ~15:00 on 10-08; IoT ~37 h later, ~04:00 on 2026-10-10   |

The nRF reflashed from the raw to the IoT firmware with both AVRs
streaming and no DAPLink errors; it is now on its own USB root rather
than the shared Genesys hub.

### Results so far: nRF52840 raw sources, 1 M samples (no sample-size warning)

| Source             | `ea_non_iid`                                                                                          | `ea_iid`                                                                            | Credited |
|--------------------|-------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------|----------|
| Hardware RNG       | **7.24 bits/byte** (MCV 7.88, t-tuple 7.35, LRS 7.72, predictors 7.91–7.95; binding: 8 × H_bitstring) | **all IID tests pass**, χ² independence p = 0.94, χ² goodness-of-fit pass; H = 7.88 | 4        |
| LFRC-RTC vs CYCCNT | **0.60 bits/sample** (MultiMMC)                                                                       | n/a                                                                                 | unused   |

Full output: `hil-results/long/ea_*-1M.txt`. These replace the
20k-sample estimates in section 2 for the nRF (6.89 → 7.24 for the
RNG; 0.61 → 0.60 for the LFRC). The credit of 4 bits/byte now has more
than 3 bits of margin.

### Results: nRF52840 IoT output, 1,000,000 bytes (complete, 13:30)

Full battery via `stats-cli full` (`hil-results/long/stats-full-nrf52840-iot.json`,
STS report in `sts-finalAnalysisReport-nrf52840-iot.txt`):

| Test                           | Result                                                                                                |
|--------------------------------|-------------------------------------------------------------------------------------------------------|
| Tier 1                         | all pass: monobit p = 0.37, runs p = 0.61, χ² p = 0.50, serial corr. 0.0009, Shannon 7.9998 bits/byte |
| `ent`                          | 7.999817 bits/byte, χ² exceeded 49.98% of the time, π error 0.02%                                     |
| `ea_iid`                       | **all IID tests pass**; H = 7.866 bits/byte                                                           |
| `ea_non_iid`                   | **7.277 bits/byte**                                                                                   |
| SP 800-22 (8 streams × 1 Mbit) | 1440 / 1452 stream-tests passed (99.17%)                                                              |

All 12 SP 800-22 failures are one stream out of 8 failing one sub-test (10 of the 148 NonOverlappingTemplate templates,
2 RandomExcursions
variants). At α = 0.01 over 1,452 stream-tests about 14.5 failures are
expected by chance, and none of the failures repeat across streams. A
single failing stream out of 8 is flagged by STS's own proportion
threshold (0.885), but with this many sub-tests that's expected.
Repeating the run on a fresh 1 MB would confirm.

The `ea_non_iid` value of 7.28 for the generator's output is the
SP 800-90B-assessed figure for the nRF52840 variant: close to the
7.24 measured on its raw hardware RNG, which means the QPP walk neither
added nor destroyed measurable bias.

### Results: AVR raw sources, 1 M samples each (complete, 15:07)

`ea_non_iid`, no sample-size warning (`hil-results/long/ea_non_iid-{mega2560,nano}-wdt-1M.txt`):

| Source             | MCV  | t-tuple  | LRS  | MultiMCW | Lag  | MultiMMC | LZ78Y | **Assessed**         | Credited |
|--------------------|------|----------|------|----------|------|----------|-------|----------------------|----------|
| Mega 2560 watchdog | 4.56 | **1.62** | 1.86 | 3.30     | 2.01 | 2.31     | 3.78  | **1.62 bits/sample** | 1        |
| Nano watchdog      | 4.91 | **2.24** | 3.38 | 2.93     | 3.30 | 2.92     | 2.93  | **2.24 bits/sample** | 1        |

The binding estimator is t-tuple on both boards, as before, but it
finds more structure in 1 M samples than in 20k (the watchdog period
drifts slowly: lag-1 autocorrelation 0.65–0.69), so the assessed values
dropped by 24–25% against the first estimates. The credit of 1 bit per
sample is still covered, but the margin is 1.6× on the Mega and 2.2× on
the Nano, not the 2–3× the 20k numbers suggested. With the credit at
1 bit and 8 samples per byte, each output byte is backed by ≥ 12.9
assessed bits on the Mega and ≥ 17.9 on the Nano. The AVR 1 MB output
runs (on another machine) use this same credit, so they stay valid. No
firmware change is needed.

To assess the rest once each phase completes:

```bash
L=hil-results/long
python3 hil-firmware/tools/extract.py jitter $L/mega2560-rawsrc.bin $L/mega2560-wdt-jitter-lowbyte-1M.bin
python3 hil-firmware/tools/extract.py iot    $L/nrf52840-iot.bin    $L/nrf52840-iot-1MB.bin
(cd $L && ea_non_iid -v mega2560-wdt-jitter-lowbyte-1M.bin 8 && ea_iid -v nrf52840-iot-1MB.bin 8)
# or the whole battery, including SP 800-22, once a file holds >= 1,000,000 bits per stream:
cargo run --release -p stats --bin stats-cli -- full --dir <dir-with-1MB-files> --out $L/stats-full.json
```

### Decision: AVR output phases moved to another machine

At 14:14 on 2026-10-08 the Mega/Nano `long-capture.sh` scripts were
stopped so the 37 h IoT-output phases would not start on this Mac. Their
raw-source captures were left running to completion (a separate
`caffeinate` keeps the Mac awake for them). The boards will still hold
the `rawsrc` firmware when done. `hil-firmware/portable/` has what
the other machine needs (prebuilt hex files with checksums,
`run-chunk.sh`, `package.sh` to build a standalone tarball, README with
the assemble steps). The hex files are byte-identical to the build of
the sources that ran on the boards (commit b8e0a82). `run-chunk.sh` has
not been run against a board yet; a short first chunk is recommended.

## Open items

1. **USB reliability.** The nRF's DAPLink dropped off USB HID three
   times, twice while both AVRs were streaming on the same Genesys USB
   2.1 hub. It recovered after a replug (new port `usbmodem21102`). For
   long runs, put the nRF on its own port.
2. Long captures are running (see above); assess each phase as it
   completes. The AVR IoT phases need ~37 h each for 1 MB.
3. Optional promotion: the AVR watchdog and nRF RNG sources live in the
   HIL firmware today. If the library should ship them, they belong next
   to `entropy-timer`'s per-platform C shims.
4. `candidates`/`xtask` don't know about the IoT variant yet. Its
   entropy only exists on hardware, so a host-side candidate would
   measure the mock source rather than the design.

## Reproducing

```bash
# Build + flash (board: mega2560 | nano | nrf52840; mode: ref | rawsrc | iot)
hil-firmware/tools/run.sh mega2560 iot

# Capture (opening the port auto-resets an Arduino)
uv run hil-firmware/tools/capture.py --port /dev/cu.usbmodem11201 --records 2500 --out hil-results/raw/mega-iot-long

# Convert + test
python3 hil-firmware/tools/extract.py iot hil-results/raw/mega-iot-long.bin hil-results/samples/mega2560-iot.bin
cargo run --release -p stats --bin stats-cli -- tier1 --file hil-results/samples/mega2560-iot.bin
ent hil-results/samples/mega2560-iot.bin

# Raw-source min-entropy (SP 800-90B subset)
python3 -c "import sys,struct; sys.path.insert(0,'hil-firmware/tools'); import entropy; \
d=open('hil-results/raw/mega-rawsrc-b.bin','rb').read(); \
print(entropy.assess([struct.unpack_from('<I',d,i)[0]&0xff for i in range(8,len(d),8)]))"

# Restore a board's original firmware
avrdude -p atmega2560 -c wiring -P /dev/cu.usbmodem11201 -b 115200 -D -U flash:w:hil-results/flash-backups/mega2560-original.hex:i
avrdude -p atmega328p -c arduino -P /dev/cu.usbserial-AB0LRIQV -b 115200 -U flash:w:hil-results/flash-backups/nano328p-original.hex:i
probe-rs download --chip nRF52840_xxAA --binary-format bin --base-address 0 hil-results/flash-backups/nrf52840-original.bin
```

Toolchain used: rustc 1.98.1 stable, 1.100.0-nightly (2026-09-08) for
`avr-none` with `-Z build-std=core`, Homebrew avr-gcc 14.3.0, avrdude
8.3, probe-rs 0.32.0, Apple clang as the Cortex-M C compiler (the
`cortex_m_dwt.c` shim only needs `<stdint.h>`, so no arm-none-eabi-gcc
is required).

## Files

| Path                                          | What                                                                                          |
|-----------------------------------------------|-----------------------------------------------------------------------------------------------|
| `crates/qpp-rng-iot/`                         | the IoT variant (was a stub)                                                                  |
| `hil-firmware/avr/`, `hil-firmware/nrf52840/` | HIL firmware, own Cargo workspaces (root `Cargo.toml` now excludes `hil-firmware`)            |
| `hil-firmware/tools/run.sh`                   | build + flash one board/mode                                                                  |
| `hil-firmware/tools/capture.py`               | serial capture (record framing, reset handling)                                               |
| `hil-firmware/tools/extract.py`               | records → byte streams                                                                        |
| `hil-firmware/tools/entropy.py`               | SP 800-90B estimator subset                                                                   |
| `hil-firmware/common/`                        | `no_std` code shared by both firmwares (UART record protocol, `iot` run loop)                 |
| `hil-firmware/tools/`                         | run.sh / smoke.sh / long-capture.sh / long-status.sh / boards.sh, plus the Python tools above |
| `hil-firmware/portable/`                      | prebuilt Arduino IoT firmware + run script for the long run on another machine                |
| `hil-results/raw/`                            | every capture (`.bin` records + `.meta`)                                                      |
| `hil-results/samples/`                        | byte streams that the stats were run on                                                       |
| `hil-results/flash-backups/`                  | original board firmware                                                                       |
