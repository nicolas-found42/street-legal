#!/bin/sh
# Compatibility entry point. Every invocation creates a unique run bundle;
# exit zero requires both complete native logs to match the supplied oracle.
set -eu
cd "$(dirname "$0")/.."
exec "${PY:-python3}" re/boot-diagnose.py "$@"
