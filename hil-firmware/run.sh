#!/usr/bin/env bash
# Build + flash one HIL firmware mode onto one board.
#
#   hil-firmware/run.sh <board> <mode>
#     board: mega2560 | nano | nrf52840
#     mode:  ref | rawsrc | iot
#
# Ports/programmers below match the bench setup these results were
# recorded on (see hil-results/REPORT.md); override with env vars
# MEGA_PORT / NANO_PORT if yours differ.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
board="$1"
mode="$2"

features=""
case "$mode" in
  ref) ;;
  rawsrc | iot) features="--features $mode" ;;
  *) echo "unknown mode $mode" >&2; exit 2 ;;
esac

MEGA_PORT="${MEGA_PORT:-/dev/cu.usbmodem11201}"
NANO_PORT="${NANO_PORT:-/dev/cu.usbserial-AB0LRIQV}"
AVR_GCC_BIN="${AVR_GCC_BIN:-$(brew --prefix avr-gcc@14 2>/dev/null)/bin}"

case "$board" in
  mega2560 | nano)
    if [ "$board" = mega2560 ]; then
      mcu=atmega2560; port="$MEGA_PORT"; prog=wiring; extra=-D
    else
      mcu=atmega328p; port="$NANO_PORT"; prog=arduino; extra=
    fi
    cd "$here/avr"
    # Separate target dirs per MCU so the two boards' builds don't
    # invalidate each other.
    PATH="$AVR_GCC_BIN:$PATH" CC_avr_none=avr-gcc AR_avr_none=avr-ar \
      RUSTFLAGS="-C target-cpu=$mcu" AVR_MCU=$mcu \
      CARGO_TARGET_DIR="target/$mcu" \
      cargo +nightly build --release $features
    elf="target/$mcu/avr-none/release/qpp-hil-avr.elf"
    avr-size "$elf"
    avrdude -q -q -p "$mcu" -c "$prog" -P "$port" -b 115200 $extra -U "flash:w:$elf:e"
    ;;
  nrf52840)
    cd "$here/nrf52840"
    llvm_bin="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin"
    AR_thumbv7em_none_eabihf="$llvm_bin/llvm-ar" cargo build --release $features
    elf=target/thumbv7em-none-eabihf/release/qpp-hil-nrf52840
    "$llvm_bin/llvm-size" "$elf"
    probe-rs download --chip nRF52840_xxAA "$elf"
    ;;
  *) echo "unknown board $board" >&2; exit 2 ;;
esac
