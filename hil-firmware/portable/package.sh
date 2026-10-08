#!/usr/bin/env bash
# Make a self-contained copy of this directory (hex files, checksums,
# run-chunk.sh, README, plus capture.py and extract.py from ../tools) to
# copy to the machine that will do the long Arduino run.
#
#   hil-firmware/portable/package.sh [output.tar.gz]     (default: qpp-hil-portable.tar.gz)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
out="$(cd "$(dirname "${1:-qpp-hil-portable.tar.gz}")" && pwd)/$(basename "${1:-qpp-hil-portable.tar.gz}")"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
dst="$stage/qpp-hil-portable"
mkdir "$dst"
cp "$here"/{mega2560-iot.hex,nano-iot.hex,SHA256SUMS,run-chunk.sh,README.md} "$dst/"
cp "$here"/../tools/{capture.py,extract.py} "$dst/"
(cd "$dst" && { shasum -a 256 -c SHA256SUMS || sha256sum -c SHA256SUMS; } >/dev/null) || { echo "hex checksum mismatch" >&2; exit 1; }
tar -C "$stage" -czf "$out" qpp-hil-portable
echo "wrote $out"
