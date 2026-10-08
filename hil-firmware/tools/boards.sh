# Per-board serial ports and reset command. Sourced by run.sh,
# long-capture.sh and smoke.sh -- not run directly.
#
# Defaults match the bench these results were recorded on; override
# MEGA_PORT / NANO_PORT in the environment if yours differ.

MEGA_PORT="${MEGA_PORT:-/dev/cu.usbmodem11201}"
NANO_PORT="${NANO_PORT:-/dev/cu.usbserial-AB0LRIQV}"

# Opening an Arduino's port resets it (DTR); the nRF52840 has no such
# line, so it is reset through the debug probe, retrying because its
# DAPLink USB is flaky.
NRF_RESET_CMD='for i in 1 2 3 4 5; do probe-rs reset --chip nRF52840_xxAA && exit 0; sleep 5; done; exit 1'

# The nRF52840 MDK's DAPLink has dropped off USB before and comes back
# on a new port, so resolve its port at use time: the usbmodem device
# that isn't the Mega. Prints nothing if there is none.
nrf_port() {
  local p
  for p in /dev/cu.usbmodem*; do
    [ -e "$p" ] && [ "$p" != "$MEGA_PORT" ] && { echo "$p"; return; }
  done
  return 0
}

# port_for <mega2560|nano|nrf52840>
port_for() {
  case "$1" in
    mega2560) echo "$MEGA_PORT" ;;
    nano) echo "$NANO_PORT" ;;
    nrf52840) nrf_port ;;
    *) echo "unknown board $1" >&2; return 2 ;;
  esac
}
