#!/bin/sh
# Run the stock exe under Wine with the slrhook proxy DLL.
#
#   re/hook/run.sh <run-name> [timeout-seconds]      env: SLR_HOOK_* (see hook.h)
#
# Layout under re/out/wine (git-ignored; see docs/hook-layer.md):
#   wine64/Wine Stable.app   pinned Wine 11.0 (Gcenx 11.0_1)
#   game-pristine/           untouched COPY of stock/ (never stock/ itself)
#   prefix-pristine/         wineboot-initialised prefix
#   hook/dinput8.dll         build.sh output
#   runs/<name>/             per-run APFS clones of game + prefix, logs, hook.slrlog
# Every run starts from fresh clones so two runs see byte-identical inputs.
set -eu

# HARD GATE: no Wine launches without explicit, per-invocation authorization.
# The hooked exe crashed in live sessions (see slr-notes/decision-m0-11-static-only.md).
# This is a refusal, not a retry: nothing below runs, and nothing relaunches Wine.
if [ "${SLR_ALLOW_WINE:-0}" != "1" ]; then
	echo "refusing to launch Wine: this script only runs on explicit authorization." >&2
	echo "M0-11 is static-only until the user authorizes a live run." >&2
	echo "set SLR_ALLOW_WINE=1 to acknowledge and override (one launch, unattended)." >&2
	echo "see slr-notes/decision-m0-11-static-only.md" >&2
	exit 4
fi

NAME=${1:?run name}
TMO=${2:-120}
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
W=$ROOT/re/out/wine
RUN=$W/runs/$NAME
WINE="$W/wine64/Wine Stable.app/Contents/Resources/wine/bin/wine"
[ -x "$WINE" ] || { echo "no wine under $W (see docs/hook-layer.md section 2)"; exit 3; }
[ -f "$W/hook/dinput8.dll" ] || { echo "run re/hook/build.sh first"; exit 3; }
rm -rf "$RUN"
mkdir -p "$RUN"
cp -cR "$W/game-pristine" "$RUN/game"
cp -cR "$W/prefix-pristine" "$RUN/prefix"
cp "$W/hook/dinput8.dll" "$RUN/game/dinput8.dll"
export WINEPREFIX="$RUN/prefix"
export WINEDLLOVERRIDES="dinput8=n,b;mscoree,mshtml="
export WINEDEBUG=${WINEDEBUG:--all}
# Never show the "serious problem" dialog or start winedbg: an unattended
# run must fail silently. (prefix-pristine also has HKLM AeDebug Auto=0; this
# covers a prefix that lost the setting.)
export WINEDLLOVERRIDES="$WINEDLLOVERRIDES;winedbg=d"
export SLR_HOOK_LOG=${SLR_HOOK_LOG:-Z:$RUN/hook.slrlog}
export SLR_HOOK_TEXT=${SLR_HOOK_TEXT:-Z:$RUN/hook.txt}
cd "$RUN/game"
EXE=${SLR_EXE:-StreetLegal_Redline.exe}
# Exactly one launch, no retry loop: a crash is evidence to keep, not a reason to
# relaunch. Any future re-run is a new, separately authorized invocation.
set +e
perl -e 'alarm shift; exec @ARGV' "$TMO" "$WINE" "Z:$RUN/game/$EXE" > "$RUN/stdout.log" 2> "$RUN/stderr.log"
echo $? > "$RUN/exit.code"
"$(dirname "$WINE")/wineserver" -k >/dev/null 2>&1
exit 0
