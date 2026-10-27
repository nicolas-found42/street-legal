# Build the Rosetta x87 probe (no Wine needed: the probe uses host x87 asm,
# so an x86_64 Mach-O binary runs under Rosetta 2 directly).
#
# Requires Rosetta 2 (`softwareupdate --install-rosetta --agree-to-license`)
# and the Xcode command-line clang. Output: re/out/x87/probe-rosetta
# (git-ignored). Run gen_subset.py first (needs re/out/x87/corpus.jsonl):
#   ~/slr-oracle-venv/bin/python re/x87probe/gen_vectors.py
#   ~/slr-oracle-venv/bin/python re/x87probe/gen_subset.py  # -> rosetta/vectors.inc (git-ignored)
#
# The win32/mingw path is retained for a host that has Wine; both branches run
# the identical per-row stubs. Pass "--win32" to build the PE instead.
set -e
HERE=$(dirname "$0")
ROOT=$(cd "$HERE/../../.." && pwd)
OUT_DIR="${OUT_DIR:-$ROOT/re/out/x87}"
mkdir -p "$OUT_DIR"
if [ "$1" = "--win32" ]; then
    i686-w64-mingw32-gcc -O1 -m32 -o "$OUT_DIR/probe.exe" "$HERE/probe.c"
    file "$OUT_DIR/probe.exe"
else
    # -arch x86_64: Rosetta 2 translates the process; the x87 blocks execute
    # in the translated image, which is the third oracle ADR 0008 names.
    clang -arch x86_64 -O1 -I "${VECTORS_DIR:-$HERE}" -I "$HERE" -o "$OUT_DIR/probe-rosetta" "$HERE/probe.c"
    file "$OUT_DIR/probe-rosetta"
fi
