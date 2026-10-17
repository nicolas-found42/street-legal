#!/usr/bin/env bash
# Create the shared oracle venv and install pinned Python deps.
# Tooling lives outside the Cargo workspace (ADR 0008: uncommon/GPL-licensed
# code is never linked into Rust crates; Unicorn via this Python venv is fine).
#
# Usage:
#   re/oracle/setup_venv.sh [VENV_DIR]     # default: ~/slr-oracle-venv
#
# Pins (2026-10-17, Apple Silicon mac; x86_64 Linux uses the same pins):
#   unicorn==2.1.4  capstone==5.0.7  pefile==2024.8.26
set -euo pipefail
VENV_DIR="${1:-$HOME/slr-oracle-venv}"

if command -v uv >/dev/null 2>&1; then
    uv venv "$VENV_DIR"
    uv pip install --python "$VENV_DIR/bin/python" pip 2>/dev/null \
        || "$VENV_DIR/bin/python" -m ensurepip 2>/dev/null \
        || true
else
    python3 -m venv "$VENV_DIR"
fi
"$VENV_DIR/bin/pip" install --upgrade pip
"$VENV_DIR/bin/pip" install "unicorn==2.1.4" "capstone==5.0.7" "pefile==2024.8.26"
"$VENV_DIR/bin/python" - <<'EOF'
import unicorn, capstone, pefile
from unicorn import uc_version
print("unicorn", unicorn.__version__, "lib", uc_version())
print("capstone", capstone.__version__)
print("pefile", pefile.__version__)
EOF
echo "venv ready at $VENV_DIR"
