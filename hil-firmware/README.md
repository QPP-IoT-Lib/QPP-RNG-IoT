# Hardware-in-the-loop (HIL) testing

Firmware and host tooling for running QPP-RNG on real boards and
measuring what it actually produces there. The host harness
(`crates/test-harness`) can't see the main failure this directory exists
to catch: on bare-metal MCUs the reference generator's timing is
deterministic, so its output repeats exactly after every reset while
still passing statistical tests. Results and analysis:
`hil-results/REPORT.md`.

Supported boards: Arduino Mega 2560 (`mega2560`), Arduino Nano /
ATmega328P (`nano`), makerdiary nRF52840 MDK (`nrf52840`).

## Layout

| Path | What |
|---|---|
| `avr/` | firmware for the two Arduinos (Mega 2560, Nano); `-Z build-std=core` on nightly, avr-gcc for the C shims |
| `nrf52840/` | firmware for the nRF52840 MDK (`thumbv7em-none-eabihf`, stable Rust) |
| `common/` | `no_std` code both firmwares share: the UART record protocol and the whole `iot` run loop; boards supply their UART and jitter source |
| `tools/` | host side: build/flash script, capture and analysis scripts (below) |
| `portable/` | prebuilt Arduino IoT firmware plus a run script, for long runs on another machine |

`avr/` and `nrf52840/` are separate Cargo workspaces (the root
workspace `exclude`s `hil-firmware`) because they only build for their
bare-metal target.

## Firmware modes

Each firmware has three modes, selected with a Cargo feature (no
feature = reference trace). All stream the same record format over the
board's UART at 115200 baud: one ASCII header line starting with
`QPPHIL`, then records of `0xA5` + two little-endian `u32`.

| Mode | Feature | Streams | Used to |
|---|---|---|---|
| reference trace | (none) | `n_p`, `Δt` of each convergence cycle of `qpp-rng-reference` | check output repeatability across resets |
| raw source | `rawsrc` | jitter sample, plus the nRF's hardware RNG byte | assess entropy sources with SP 800-90B |
| IoT | `iot` | 4 output bytes of `qpp-rng-iot`, and the ticks it took | measure speed and test the output |

## Workflow

```bash
# Build + flash one board/mode (board: mega2560 | nano | nrf52840; mode: ref | rawsrc | iot)
hil-firmware/tools/run.sh nano iot

# Capture. Opening the port auto-resets an Arduino; the nRF is reset through
# the debug probe, hence --reset-cmd.
uv run hil-firmware/tools/capture.py --port /dev/cu.usbserial-XXXX --records 2500 --out hil-results/raw/nano-iot
uv run hil-firmware/tools/capture.py --port /dev/cu.usbmodemXXXX --records 2500 \
    --reset-cmd "probe-rs reset --chip nRF52840_xxAA" --out hil-results/raw/nrf-iot

# Turn records into plain byte streams (modes: ref | iot | hwrng | jitter)
python3 hil-firmware/tools/extract.py iot hil-results/raw/nano-iot.bin hil-results/samples/nano-iot.bin

# Test them: the workspace harness (Tier 1 + ent + NIST SP 800-90B / SP 800-22) ...
cargo run --release -p stats --bin stats-cli -- full --dir hil-results/samples --out hil-results/stats.json
# ... or NIST directly (ea_non_iid -v <file> 8). tools/entropy.py is a quick, partial stand-in:
python3 -c "import sys; sys.path.insert(0,'hil-firmware/tools'); import entropy; \
print(entropy.assess(list(open('hil-results/samples/nano-iot.bin','rb').read()[:20000])))"
```

After any change to a firmware, to `common/` or to `qpp-rng-iot`, check
the real board (a minute each; it flashes the `iot` firmware, captures
twice across a reset, and checks header, framing and that the two
outputs differ):

```bash
hil-firmware/tools/smoke.sh nano        # also: mega2560, nrf52840
```

Long unattended runs (a raw-source phase, then an output phase, per
board), with progress and ETA:

```bash
caffeinate -ims hil-firmware/tools/long-capture.sh nrf52840     # also: mega2560, nano
hil-firmware/tools/long-status.sh
```

Serial ports and the nRF reset command are in `tools/boards.sh`
(override with `MEGA_PORT` / `NANO_PORT`). The nRF's port is whichever
`usbmodem` device isn't the Mega.

## Tooling needed

`rustup` (stable, plus nightly with `rust-src` for AVR), avr-gcc 14 and
`avrdude` (AVR boards; `brew install avr-gcc@14 avrdude`), `probe-rs`
(nRF52840), `uv` (runs `capture.py` with its own pyserial). Optional:
`ent` and the NIST SP 800-90B / SP 800-22 tools (see
`docs/environment-setup.md`). The nRF's C shim compiles with plain
clang, no arm-none-eabi-gcc.

## Things worth knowing

- Flashing replaces whatever program a board was running. Back it up
  first (`avrdude -U flash:r:...`, `probe-rs read`); the restore
  commands used for this project are in `hil-results/REPORT.md`.
- Credited entropy per raw sample (`CREDITED_BITS` in each `iot`
  firmware) comes from NIST `ea_non_iid` on 1 M raw samples per source
  and sets both the health-test cutoffs and how many samples go into
  each output byte. Re-assess and update it for any new board or
  source.
- The nRF52840 MDK's DAPLink USB can drop out under load; if
  `probe-rs` stops seeing the probe, replug the board (the serial port
  name may change).
