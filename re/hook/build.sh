#!/bin/sh
# Build the slrhook proxy DLL (i686 mingw-w64). Output: $1 (default re/out/wine/hook/dinput8.dll).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${1:-$ROOT/re/out/wine/hook}
mkdir -p "$OUT/gen"
python3 "$HERE/gen_com.py" > "$OUT/gen/com_tables.h"
python3 "$HERE/gen_native.py" > "$HERE/native_table.h"
SRCS=$(ls "$HERE"/*.c)
i686-w64-mingw32-gcc -shared -O1 -g -std=gnu11 -Wall -Wextra -Wno-unused-parameter -Wno-cast-function-type \
  -mno-sse -mfpmath=387 -mstackrealign -static-libgcc -I"$HERE" -I"$OUT/gen" \
  -o "$OUT/dinput8.dll" $SRCS -Wl,--kill-at -lwinmm -lole32 -luuid -ldxguid
ls -la "$OUT/dinput8.dll"
