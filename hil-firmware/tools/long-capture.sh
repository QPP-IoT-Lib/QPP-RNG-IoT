#!/usr/bin/env bash
# Long, unattended captures for one board: first the raw entropy source
# (for a >= 1 M-sample SP 800-90B non-IID assessment), then the IoT
# generator's output (towards >= 1 MB for the IID track and SP 800-22).
#
#   hil-firmware/tools/long-capture.sh <mega2560 | nano | nrf52840>
#
# Writes hil-results/long/<board>-{rawsrc,iot}.{bin,meta} incrementally
# (`status=running` in the .meta until each phase ends) and logs to
# hil-results/long/<board>.log. Run it under `caffeinate -ims` so the
# Mac doesn't sleep. Stop it any time with `kill <pid>` (pid in
# hil-results/long/<board>.pid); data captured so far is kept.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"   # hil-firmware/tools/
out_dir="$here/../../hil-results/long"
board="$1"
mkdir -p "$out_dir"
echo $$ > "$out_dir/$board.pid"

. "$here/boards.sh"

# 1,000,001 raw records = 1 M jitter deltas (the first record has no
# predecessor); 250,000 IoT records = 1,000,000 output bytes.
RAW_RECORDS=1000001
IOT_RECORDS=250000

log() { echo "[$(date '+%F %T')] $*"; }

retry() {
  local tries=$1; shift
  for i in $(seq 1 "$tries"); do
    "$@" && return 0
    log "attempt $i/$tries failed: $*"
    sleep 20
  done
  return 1
}

capture() { # mode records timeout_seconds
  local mode=$1 records=$2 timeout=$3 port reset=()
  port="$(port_for "$board")"
  [ -n "$port" ] || { log "no serial port found for $board"; return 1; }
  [ "$board" = nrf52840 ] && reset=(--reset-cmd "$NRF_RESET_CMD")
  log "capturing $records $mode records from $port"
  uv run -q "$here/capture.py" --port "$port" --records "$records" \
    --timeout "$timeout" ${reset[@]+"${reset[@]}"} --out "$out_dir/$board-$mode"
}

phase() { # mode records timeout_seconds
  local mode=$1
  log "=== phase $mode: flashing"
  if ! retry 5 "$here/run.sh" "$board" "$mode"; then
    log "flashing $mode failed, giving up on this phase"
    return 1
  fi
  capture "$@"
  log "phase $mode ended: $(grep -E '^(records|status)=' "$out_dir/$board-$mode.meta" | tr '\n' ' ')"
}

log "long capture starting on $board"
case "$board" in
  mega2560 | nano)
    phase rawsrc "$RAW_RECORDS" $((6 * 3600))
    phase iot "$IOT_RECORDS" $((48 * 3600))
    ;;
  nrf52840)
    phase rawsrc "$RAW_RECORDS" 3600
    phase iot "$IOT_RECORDS" $((4 * 3600))
    ;;
  *) echo "unknown board $board" >&2; exit 2 ;;
esac
log "all phases done on $board"
rm -f "$out_dir/$board.pid"
