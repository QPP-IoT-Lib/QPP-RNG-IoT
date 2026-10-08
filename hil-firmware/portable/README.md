# Portable Arduino run: QPP-RNG IoT output

Everything needed to run the `iot` firmware on the Mega 2560 and the
Nano from a machine that has no Rust toolchain, to collect the 1 MB of
output (250,000 records) that NIST SP 800-90B `ea_iid` and SP 800-22
need. At ~1.85 records/s that is ~37.5 h per board; both boards can run
at the same time on one machine.

To use it elsewhere, make a standalone copy and unpack it there:

```bash
hil-firmware/portable/package.sh        # writes qpp-hil-portable.tar.gz
```

## Needs (on the machine that runs it)
- `avrdude` (Windows/Linux/Mac) and `uv`
- Mac/Linux shell for `run-chunk.sh` (on Windows, run the two commands
  inside it by hand: the `avrdude` line, then `uv run capture.py ...`)
- the machine must **not sleep or hibernate** during a run
  (Mac: `caffeinate -ims ./run-chunk.sh ...`; Linux: `systemd-inhibit`)
- Linux: your user needs serial access (`dialout` group)

## Run
```bash
./run-chunk.sh mega2560 /dev/ttyACM0 20000    # ~3 h; port names differ per OS
./run-chunk.sh nano     /dev/ttyUSB0 20000
```
- Mega 2560: Arduino USB shows up as `ttyACM*` / `COMn`; Nano (FT232R): `ttyUSB*`.
- Do one long run with `250000` or several chunks. Every run resets the
  board, which starts independent output, so chunks join cleanly. Chunks
  add up to 250,000 records: e.g. 13 × 20000 (a partial last chunk is fine).
- Each chunk writes `out/<board>-iot-<timestamp>.bin` (8 bytes/record) plus a `.meta`
  with `status=` (`complete`, `stalled`, `timeout`, `serial-error: ...`,
  `health-test-failure`). The `.bin` is flushed every 1000 records, so an
  interrupted chunk keeps everything up to the interruption.
- First line printed by the firmware (in `.meta` as `header=`) must read
  `QPPHIL avr iot-wdt N=6 credit=1 samples_per_byte=8 ...`.
- Try a short first chunk, e.g. `./run-chunk.sh nano <port> 20` (~15 s).

## After the runs: assemble
```bash
cat out/mega2560-iot-*.bin > mega2560-iot-all.bin      # records; drop any partial last chunk if needed
python3 extract.py iot mega2560-iot-all.bin mega2560-iot.bin
head -c 1000000 mega2560-iot.bin > mega2560-iot-1MB.bin
```
Copy the `.bin`/`.meta` files back to `hil-results/long/` on the main
machine for analysis (`ea_iid`, `ea_non_iid`, `stats-cli full`).

## Provenance of the hex files
`mega2560-iot.hex` and `nano-iot.hex` were built from commit **b8e0a82**
(`hil-firmware/tools/run.sh <board> iot`; byte-identical to that build)
and are the firmware that was run on both boards and verified on
hardware: output differs across resets, 7.4 B/s, no health-test
failures. `SHA256SUMS` pins them, and the scripts check it before
flashing.

Later commits refactor the firmware source into `common/` and produce
different (not yet hardware-tested) `iot` binaries. Don't replace these
hex files with a rebuild until the board has passed
`hil-firmware/tools/smoke.sh`. To refresh them from the tested build:

```bash
hil-firmware/tools/smoke.sh nano          # flashes + checks the current source
avr-objcopy -O ihex -R .eeprom hil-firmware/avr/target/atmega328p/avr-none/release/qpp-hil-avr.elf hil-firmware/portable/nano-iot.hex
#                      (Mega: target/atmega2560/..., mega2560-iot.hex)
(cd hil-firmware/portable && shasum -a 256 *.hex > SHA256SUMS)
```

The credited entropy (1 bit per watchdog sample) comes from the
Mega/Nano raw-source assessments in `hil-results/REPORT.md`. Flashing
overwrites each board's program (originals: `hil-results/flash-backups/`).
