# Build the Rosetta x87 probe exe (win32 console, i686).
# Requires mingw-w64 (brew install mingw-w64). Output: re/out/wine/probe.exe (git-ignored).
set -e
HERE=$(dirname "$0")
# HERE=re/x87probe/rosetta -> repo root is three levels up
ROOT=$(cd "$HERE/../../.." && pwd)
OUT_DIR="${1:-$ROOT/re/out/wine}"
mkdir -p "$OUT_DIR"
i686-w64-mingw32-gcc -O1 -m32 -o "$OUT_DIR/probe.exe" "$HERE/probe.c"
ls -la "$OUT_DIR/probe.exe"
file "$OUT_DIR/probe.exe"
# NOTE: run ../gen_subset.py first (needs re/out/x87/corpus.jsonl):
#   ~/slr-oracle-venv/bin/python re/x87probe/gen_vectors.py
#   ~/slr-oracle-venv/bin/python re/x87probe/gen_subset.py  # -> rosetta/vectors.inc (generated, git-ignored)
