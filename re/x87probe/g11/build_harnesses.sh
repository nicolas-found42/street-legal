#!/bin/sh
# Build the three streaming legs into OUT (default: a fresh directory under the evidence root).
#   x87-stream     x86_64 Mach-O: Rosetta 2 on Apple Silicon, native x87 on an Intel Mac
#   bochs-stream   native model of the pinned Bochs FPU (needs a sparse Bochs checkout at BOCHS)
# Usage: BOCHS=/path/to/bochs-checkout OUT=/path/to/out sh build_harnesses.sh
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
OUT=${OUT:?set OUT}
mkdir -p "$OUT"
clang -arch x86_64 -O0 -Wall -Wextra -Werror -o "$OUT/x87-stream" "$HERE/x87_stream.c"
if [ -n "${BOCHS:-}" ]; then
  PIN=d245c751e6a6fab4bf68e72823598a48001adb3c
  [ "$(git -C "$BOCHS" rev-parse HEAD)" = "$PIN" ] || { echo "BOCHS checkout is not at $PIN" >&2; exit 2; }
  U=$BOCHS/bochs/cpu
  python3 "$HERE/bochs_build.py" "$BOCHS" "$OUT"
fi
