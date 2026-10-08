# /// script
# requires-python = ">=3.9"
# dependencies = ["pyserial>=3.5"]
# ///
"""Capture QPP-HIL records from a board's UART.

Opening the port asserts DTR, which auto-resets an Arduino, so every
capture starts from the firmware's fixed seed. For boards without
auto-reset (the nRF52840 MDK), pass --reset-cmd to reset it via the
debug probe after the port is open.

Usage:
  uv run capture.py --port /dev/cu.usbmodem11201 --records 2000 --out run1
Writes run1.bin (records, 8 bytes each: two little-endian u32s with the
0xA5 sync byte stripped) and run1.meta (header line, timing, status).
Both are updated while the capture runs; .meta's `status=` is `running`
until it ends, then `complete`, `timeout`, `stalled`,
`health-test-failure` or `serial-error: ...`.
"""

import argparse
import os
import subprocess
import sys
import time

import serial

SYNC = 0xA5
REC = 9


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", required=True)
    ap.add_argument("--baud", type=int, default=115200)
    ap.add_argument("--records", type=int, required=True)
    ap.add_argument("--timeout", type=float, default=600.0, help="give up after this many seconds")
    ap.add_argument("--reset-cmd", help="shell command that resets the board once the port is open")
    ap.add_argument("--out", required=True)
    ap.add_argument("--stall-timeout", type=float, default=120.0,
                    help="stop if no bytes arrive for this many seconds")
    ap.add_argument("--flush-every", type=int, default=1000,
                    help="flush data and rewrite the progress .meta every N records")
    args = ap.parse_args()

    s = serial.Serial(args.port, args.baud, timeout=0.5)
    s.reset_input_buffer()
    if args.reset_cmd:
        subprocess.run(args.reset_cmd, shell=True, check=True, capture_output=True)
        s.reset_input_buffer()

    t_start = time.monotonic()
    deadline = t_start + args.timeout

    # Header line.
    header = b""
    while not header.endswith(b"\n"):
        c = s.read(1)
        if c:
            header += c
        elif time.monotonic() > deadline:
            sys.exit(f"timed out waiting for header, got {header!r}")
        if len(header) > 200:
            # Junk from before the reset; keep only the tail.
            header = header[-100:]
    header = header[header.rfind(b"QPPHIL"):].decode(errors="replace").strip()

    buf = bytearray()
    n = 0
    t_first = None
    t_last_data = time.monotonic()
    bad_sync = 0
    stop_reason = "complete"
    out = open(args.out + ".bin", "wb")

    def write_meta(final):
        elapsed = (time.monotonic() - t_first) if t_first else 0.0
        with open(args.out + ".meta.tmp", "w") as f:
            f.write(f"header={header}\n")
            f.write(f"records={n}\n")
            f.write(f"bad_sync_bytes={bad_sync}\n")
            f.write(f"seconds={elapsed:.3f}\n")
            f.write(f"records_per_second={(n - 1) / elapsed if elapsed > 0 and n > 1 else 0:.3f}\n")
            f.write(f"status={stop_reason if final else 'running'}\n")
        os.replace(args.out + ".meta.tmp", args.out + ".meta")
        return elapsed

    # Records go to disk as they arrive (flushed with the progress
    # meta every --flush-every records), so a USB drop or a crash hours
    # into a long capture keeps everything received up to that point.
    try:
        while n < args.records:
            if time.monotonic() > deadline:
                stop_reason = "timeout"
                break
            chunk = s.read(max(1, s.in_waiting))
            if chunk:
                t_last_data = time.monotonic()
                buf += chunk
            elif time.monotonic() - t_last_data > args.stall_timeout:
                stop_reason = "stalled"
                break
            if b"HEALTH-FAIL" in buf:
                stop_reason = "health-test-failure"
                break
            while len(buf) >= REC:
                if buf[0] != SYNC:
                    del buf[0]
                    bad_sync += 1
                    continue
                if t_first is None:
                    t_first = time.monotonic()
                out.write(buf[1:REC])
                del buf[:REC]
                n += 1
                if n % args.flush_every == 0:
                    out.flush()
                    write_meta(final=False)
                if n >= args.records:
                    break
    except (serial.SerialException, OSError) as e:
        stop_reason = f"serial-error: {e}"
    finally:
        out.close()
        try:
            s.close()
        except Exception:
            pass

    elapsed = write_meta(final=True)
    print(f"{args.out}: {n} records, header={header!r}, bad_sync={bad_sync}, {elapsed:.1f}s, {stop_reason}")
    if n < args.records:
        sys.exit(f"only got {n}/{args.records} records ({stop_reason})")

if __name__ == "__main__":
    main()
