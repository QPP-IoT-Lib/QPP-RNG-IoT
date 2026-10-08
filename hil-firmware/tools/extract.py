"""Turn capture.py record files into plain byte streams for stats-cli/ent.

  python3 extract.py ref    <in.bin> <out.bin>  # reference trace -> default (oversample 5) output bytes
  python3 extract.py iot    <in.bin> <out.bin>  # IoT records -> output bytes
  python3 extract.py hwrng  <in.bin> <out.bin>  # nRF rawsrc records -> on-chip RNG bytes
  python3 extract.py jitter <in.bin> <out.bin>  # rawsrc records -> low byte of each jitter sample

The reference firmware runs with oversample = 1 so every convergence
cycle is visible; the paper-default generator XORs 5 consecutive
`n_p mod 256` values per output byte, which is reproduced here exactly
(the cycles themselves don't depend on how they're grouped).
"""

import struct
import sys


def records(path):
    data = open(path, "rb").read()
    return [struct.unpack_from("<II", data, i) for i in range(0, len(data) - 7, 8)]


def main():
    mode, src, dst = sys.argv[1:4]
    recs = records(src)
    out = bytearray()
    if mode == "ref":
        for i in range(0, len(recs) - 4, 5):
            b = 0
            for n_p, _dt in recs[i:i + 5]:
                b ^= n_p & 0xFF
            out.append(b)
    elif mode == "iot":
        for word, _ticks in recs:
            out += struct.pack("<I", word)
    elif mode == "hwrng":
        out += bytes(r[1] & 0xFF for r in recs)
    elif mode == "jitter":
        out += bytes(r[0] & 0xFF for r in recs[1:])
    else:
        sys.exit(f"unknown mode {mode}")
    open(dst, "wb").write(out)
    print(f"{dst}: {len(out)} bytes")


if __name__ == "__main__":
    main()
