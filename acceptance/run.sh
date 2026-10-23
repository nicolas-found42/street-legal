#!/bin/sh
# Compute whole-game acceptance results; inventory progress cannot assert pass.
# Exit 1 while any obligation lacks a passing validator, 2 on invalid inventory.
# --table reports without gating; --json emits computed results.
set -eu
cd "$(dirname "$0")/.."
exec "${PY:-python3}" acceptance/check.py "$@"
