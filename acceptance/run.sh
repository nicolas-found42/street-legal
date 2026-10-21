#!/bin/sh
# The acceptance gate. Exits nonzero while any obligation in
# acceptance/matrix.json is unmet, and exits 2 if the matrix makes a claim it
# cannot evidence. `--table` prints the inventory without gating.
set -eu
cd "$(dirname "$0")/.."
exec "${PY:-python3}" acceptance/check.py "$@"
