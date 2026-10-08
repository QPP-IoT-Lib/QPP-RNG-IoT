#!/usr/bin/env bash
# Progress of the long captures started with long-capture.sh.
#   hil-firmware/tools/long-status.sh
set -uo pipefail
dir="$(cd "$(dirname "$0")/../../hil-results/long" && pwd)"
for b in nrf52840 mega2560 nano; do
  pid=""
  [ -f "$dir/$b.pid" ] && pid="$(cat "$dir/$b.pid")"
  if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then state="running (pid $pid)"; else state="not running"; fi
  echo "== $b: $state"
  for phase in rawsrc iot; do
    m="$dir/$b-$phase.meta"
    [ -f "$m" ] || continue
    rec=$(sed -n 's/^records=//p' "$m"); rate=$(sed -n 's/^records_per_second=//p' "$m"); st=$(sed -n 's/^status=//p' "$m")
    target=1000001; [ "$phase" = iot ] && target=250000
    eta=""
    if [ "$st" = running ] && awk "BEGIN{exit !($rate > 0)}"; then
      eta=$(awk -v r="$rec" -v t="$target" -v s="$rate" 'BEGIN{h=(t-r)/s/3600; printf "  ~%.1f h left", h}')
    fi
    printf '   %-6s %9s / %-8s %-10s %s%s\n' "$phase" "$rec" "$target" "$st" "($rate rec/s)" "$eta"
  done
  grep -E 'attempt|giving up|Error' "$dir/$b.log" 2>/dev/null | tail -2 | sed 's/^/   ! /' || true
done
