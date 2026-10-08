#!/usr/bin/env bash
# Flash one Arduino with the QPP-RNG IoT firmware and capture one chunk
# of its output. Run once per board per chunk; boards can run in parallel.
#
#   ./run-chunk.sh <mega2560|nano> <serial-port> [records]   (records default 20000)
#
# 1 record = 4 output bytes. At ~1.85 records/s, 20000 records is ~3 h and
# 80 KB; 250000 records (the full 1,000,000 bytes) is ~37.5 h in one go.
# Each run resets the board, which starts a fresh, independent stretch of
# output, so chunks can simply be concatenated afterwards (see README.md).
#
# Needs: avrdude, uv (https://docs.astral.sh/uv/). Set NOFLASH=1 to skip
# flashing if the board already runs this firmware.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
board="${1:?board: mega2560 or nano}"
port="${2:?serial port, e.g. /dev/ttyACM0}"
records="${3:-20000}"

case "$board" in
  mega2560) args=(-p atmega2560 -c wiring -D) ;;
  nano)     args=(-p atmega328p -c arduino) ;;
  *) echo "unknown board $board" >&2; exit 2 ;;
esac

if [ "${NOFLASH:-0}" != 1 ]; then
  (cd "$here" && { shasum -a 256 -c SHA256SUMS >/dev/null 2>&1 || sha256sum -c SHA256SUMS >/dev/null 2>&1; }) \
    || { echo "hex checksum mismatch" >&2; exit 1; }
  avrdude "${args[@]}" -P "$port" -b 115200 -U "flash:w:$here/$board-iot.hex:i"
fi

# Standalone copies (made by package.sh) have capture.py next to this
# script; inside the repo it lives in ../tools.
capture="$here/capture.py"; [ -f "$capture" ] || capture="$here/../tools/capture.py"
[ -f "$capture" ] || { echo "capture.py not found" >&2; exit 1; }

mkdir -p "$here/out"
chunk="$here/out/$board-iot-$(date +%Y%m%d-%H%M%S)"
echo "capturing $records records ($((records * 4)) bytes) to $chunk.bin"
uv run -q "$capture" --port "$port" --records "$records" --timeout $((records / 1 + 3600)) --out "$chunk"
echo "done: $(grep -E '^status=' "$chunk.meta")"
