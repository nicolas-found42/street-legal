#!/bin/sh
# Run the integrated headless boot and compare its native-call log against the
# original oracle's script-native log (the headless-original run's
# `observed/script-natives.slrlog`). Usage: re/boot-compare.sh [original.slrlog]
# Output: re/out/boot/{runtime.txt,runtime.calllog,compare.txt}. Never touches stock/ or reference/.
set -u
cd "$(dirname "$0")/.."
ORIG="${SLR_ORIGINAL_LOG:-re/out/headless/boot/observed/script-natives.slrlog}"
if [ $# -gt 0 ] && [ -f "$1" ]; then ORIG="$1"; shift; fi
LIMIT="${SLR_BOOT_SECONDS:-120}"
mkdir -p re/out/boot
cargo build -q -p slr-boot 2>&1 | grep -E "^error" -A8
perl -e 'alarm shift; exec @ARGV' "$LIMIT" target/debug/slr-boot stock \
  --config-instance-id 0xffff0080 --game-init-instance-id 0xffff007f \
  "$@" > re/out/boot/runtime.txt 2>&1
echo "boot exit=$? (142 = hit the ${LIMIT}s watchdog)"
tail -1 re/out/boot/runtime.txt | cut -c1-600
if [ -f "$ORIG" ]; then
  cargo run -q -p slr-calllog --bin slr-calllog-diff -- --mask-refs re/out/native_api.tsv "$ORIG" re/out/boot/runtime.calllog > re/out/boot/compare.txt 2>&1
  echo "--- original vs port call log"; head -12 re/out/boot/compare.txt | cut -c1-300
  ALL="$(dirname "$ORIG")/script-natives-all.slrlog"
  if [ -f "$ALL" ]; then
    cargo run -q -p slr-calllog --bin slr-calllog-diff -- --mask-refs re/out/native_api.tsv "$ALL" re/out/boot/runtime-all.calllog > re/out/boot/compare-all.txt 2>&1
    echo "--- unfiltered (incl. render/sound/input) original vs port"; head -12 re/out/boot/compare-all.txt | cut -c1-300
  fi
else
  echo "no original log at $ORIG"
fi
