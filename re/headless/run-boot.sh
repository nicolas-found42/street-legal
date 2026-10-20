#!/bin/sh
# Furthest declared headless-original run. Usage: re/headless/run-boot.sh [extra startup.py args]
# Inputs (ignored, regenerate with make_boot_profile.py and the desktop/graphics/media tools):
set -e
cd "$(dirname "$0")/../.."
PY="${PY:-$HOME/slr-oracle-venv/bin/python}"
OUT=re/out/headless
"$PY" re/headless/make_boot_profile.py "$OUT/boot/platform-profile.json" >/dev/null
exec "$PY" re/headless/startup.py \
  --media-profile "$OUT/hypothetical-cd-toc.json" \
  --graphics-profile "$OUT/boot/graphics-profile.json" \
  --desktop-profile "$OUT/window-transcript-native/hypothetical-seven-notifications-declared-handle-profile.json" \
  --platform-profile "$OUT/boot/platform-profile.json" \
  --calllog "$OUT/boot/boot.slrlog" \
  --observe-dir "$OUT/boot/observed" \
  --out "$OUT/boot/boot.json" \
  --event-limit 100000 --event-ring 5000 --fast-count "$@"
