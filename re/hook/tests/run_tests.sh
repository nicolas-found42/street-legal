#!/bin/sh
# Data-free tests for the hook layer (no Wine, no game data).
# 1. slrlog.h (host-compiled) and the doc's Python reference emitter produce
#    byte-identical logs.
# 2. The Rust reader accepts the log and slr-calllog-diff reports identical.
# 3. Publish order (W4): slr_detour stores the trampoline into the caller's
#    slot before it patches the target entry, and every call site hands over
#    the slot its hook reads (no assign-after-return).
# 4. Lock span (W5): slr_native_hook_all holds g_nat_cs across slr_detour, so
#    two overlapping calls cannot both see hooked == 0 and double-patch one
#    entry; the hooked state is published before the lock is dropped.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../../.." && pwd)
T=${TMPDIR:-/tmp}/slrhook-emit-$$
mkdir -p "$T"
trap 'rm -rf "$T"' EXIT
cc -std=c99 -Wall -Wextra -Werror -o "$T/emit_test" "$HERE/emit_test.c"
"$T/emit_test" "$T/c.slrlog"
python3 -I "$HERE/emit_ref.py" "$T/py.slrlog"
cmp "$T/c.slrlog" "$T/py.slrlog" && echo "OK: C emitter == Python reference ($(wc -c < "$T/c.slrlog") bytes)"
(cd "$ROOT" && cargo run -q -p slr-calllog --bin slr-calllog-diff -- "$T/py.slrlog" "$T/c.slrlog")
python3 -I - "$HERE/../slrhook.c" "$HERE/../hook_native.c" <<'EOF'
import re, sys
sh, nat = (open(p).read().splitlines() for p in sys.argv[1:3])
line = lambda ls, pat: next(i for i, l in enumerate(ls, 1) if re.search(pat, l))
pub = line(sh, r"\*publish = tramp;")
patch = line(sh, r"^\s*t\[0\] = 0xE9;")
assert pub < patch, f"slrhook.c: slot store ({pub}) must precede entry patch ({patch})"
assert not any(re.search(r"=\s*slr_detour\(", l) for l in nat), \
    "hook_native.c: a trampoline is assigned after slr_detour returns"
sites = [l for l in nat if "slr_detour(" in l]
assert len(sites) == 3, f"hook_native.c: expected 3 detour call sites, found {len(sites)}"
assert any("&e->tramp);" in l for l in sites), "non-alias entry must publish &e->tramp"
assert any("&g_reg_tramp);" in l for l in sites), "registrar must publish &g_reg_tramp"
assert any(l.rstrip().endswith("NULL);") for l in sites), "alias tail thunk must pass NULL"
print("OK: W4 publish order (slot before patch, 3 call sites)")
EOF
python3 -I - "$HERE/../hook_native.c" <<'EOF'
import re, sys
src = open(sys.argv[1]).read()

def body(name):
    """Lines of the named function body, brace-matched from its definition."""
    m = re.search(r"^(?:static )?[A-Za-z_][A-Za-z0-9_ *]*\b" + re.escape(name) + r"\(", src, re.M)
    assert m, f"hook_native.c: {name} definition not found"
    i = src.index("{", m.end())
    d, j = 0, i
    while j < len(src):
        d += (src[j] == "{") - (src[j] == "}")
        if d == 0:
            break
        j += 1
    return [(n, l) for n, l in enumerate(src[i + 1:j].splitlines(), 1)]

def has(lines, pat):
    return [n for n, l in lines if re.search(pat, l)]

# No state re-check after the lock was released: that was the W5 shape.
assert not re.search(r"!\s*e->hooked", src), \
    "hook_native.c: hooked state re-checked after the lock was released (W5 regression)"

# hook_all itself: one lock region per entry, the patch inside it, no patch here.
h = body("slr_native_hook_all")
enter = has(h, r"EnterCriticalSection\(&g_nat_cs\)")
leave = has(h, r"LeaveCriticalSection\(&g_nat_cs\)")
call = has(h, r"^\s*nat_hook_one\(")
assert len(enter) == 1, f"hook_all: expected 1 EnterCriticalSection(&g_nat_cs), found {len(enter)}"
assert len(leave) == 1, f"hook_all: expected 1 LeaveCriticalSection(&g_nat_cs), found {len(leave)}"
assert len(call) == 1, f"hook_all: expected 1 nat_hook_one call, found {len(call)}"
assert enter[0] < call[0] < leave[0], \
    f"hook_all: patch call (line {call[0]}) must sit between lock ({enter[0]}) and release ({leave[0]})"
assert not has(h, r"slr_detour\("), "hook_all: the detour must run inside the locked helper"
assert len(re.findall(r"\bnat_hook_one\(", src)) == 2, \
    "hook_native.c: nat_hook_one must have exactly one call site (in hook_all's lock)"

# The helper: no lock operations at all, so the lock cannot be dropped mid-patch.
p = body("nat_hook_one")
assert not has(p, r"CriticalSection"), "nat_hook_one: must not take or drop g_nat_cs"
sites = has(p, r"slr_detour\(")
assert len(sites) == 2, f"nat_hook_one: expected 2 detour sites, found {len(sites)}"
for n in sites:
    nxt = next((k for k, l in p if k > n and re.search(r"^\s*return;?\s*$", l)), len(p) + 1)
    span = [l for k, l in p if n < k < nxt]
    assert any(re.search(r"e->hooked = [12-]", l) for l in span), \
        f"nat_hook_one line {n}: hooked state must be committed before the branch ends"
print("OK: W5 lock span (1 lock region per entry, both detour sites and their state "
      "stores inside the locked helper)")
EOF
