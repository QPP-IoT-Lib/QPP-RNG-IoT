#!/usr/bin/env bash
# One-minute check that a board runs the IoT firmware correctly.
#
#   hil-firmware/tools/smoke.sh <mega2560|nano|nrf52840> [records]
#
# Flashes the `iot` firmware (skip with NOFLASH=1), resets the board and
# captures `records` records (default 10; 1 record = 4 output bytes) twice,
# then checks that
#   - the header line says `iot` and the capture completed cleanly,
#   - no framing errors occurred,
#   - the two resets produced different output (the property the reference
#     generator lacks on bare-metal MCUs).
# Run it after any change to the firmware or to qpp-rng-iot, since these
# can only be exercised on the real hardware.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
. "$here/boards.sh"

board="${1:?board: mega2560 | nano | nrf52840}"
records="${2:-10}"
port="$(port_for "$board")"
[ -n "$port" ] || { echo "no serial port found for $board" >&2; exit 1; }

if [ "${NOFLASH:-0}" != 1 ]; then
  "$here/run.sh" "$board" iot >/dev/null
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

reset=()
[ "$board" = nrf52840 ] && reset=(--reset-cmd "$NRF_RESET_CMD")

for run in a b; do
  uv run -q "$here/capture.py" --port "$port" --records "$records" --timeout 300 \
    ${reset[@]+"${reset[@]}"} --out "$tmp/$run" >/dev/null
done

python3 - "$tmp" "$board" <<'PY'
import sys

tmp, board = sys.argv[1], sys.argv[2]

def meta(run):
    return dict(l.rstrip("\n").split("=", 1) for l in open(f"{tmp}/{run}.meta") if "=" in l)

def words(run):
    d = open(f"{tmp}/{run}.bin", "rb").read()
    return b"".join(d[i:i + 4] for i in range(0, len(d), 8))

ma, mb = meta("a"), meta("b")
wa, wb = words("a"), words("b")
n = min(len(wa), len(wb))
same = sum(x == y for x, y in zip(wa, wb))
checks = [
    ("header says iot", " iot" in ma["header"] and " iot" in mb["header"]),
    ("both captures complete", ma["status"] == mb["status"] == "complete"),
    ("no framing errors", ma["bad_sync_bytes"] == mb["bad_sync_bytes"] == "0"),
    ("output differs across resets", n > 0 and wa != wb),
    ("not mostly identical bytes", n > 0 and same <= max(4, n // 8)),
]
print(f"{board}: {ma['header']}")
print(f"  {ma['records_per_second']} records/s; run a {wa[:8].hex()}..., run b {wb[:8].hex()}...; "
      f"{same}/{n} same-position bytes (chance: ~{n / 256:.1f})")
bad = [name for name, ok in checks if not ok]
for name, ok in checks:
    print(f"  {'PASS' if ok else 'FAIL'}  {name}")
sys.exit(1 if bad else 0)
PY
