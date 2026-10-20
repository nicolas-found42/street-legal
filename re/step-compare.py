#!/usr/bin/env python3
"""Compare executed script nodes: the port's SLR_TRACE_STEPS stderr against the
original oracle's observed/script-steps.jsonl. Prints the first divergence with
context. Usage: re/step-compare.py <port-stderr.txt> <script-steps.jsonl> [context]
Compared fields: class, method (field-init bodies collapse), node index, opword, imm.
DROP=cls.method,... removes engine-initiated calls the port does not make.
Threads are compared as one global sequence; the first divergence may be a
scheduling-order difference, which the printed context makes visible."""
import json, re, sys

port_path, orig_path = sys.argv[1], sys.argv[2]
ctx = int(sys.argv[3]) if len(sys.argv) > 3 else 6
step = re.compile(r"^STEP (\S+?)(\(.*\)\S*)? idx=(\d+) op=([0-9a-f]+) imm=(\d+) line=(\d+)")

def norm_method(label):
    return "FI" if "field init" in label else label

def port_nodes():
    for line in open(port_path, errors="replace"):
        m = step.match(line)
        if not m:
            continue
        full = m.group(1)
        if "<field init" in line.split(" idx=")[0]:
            cls = line.split(" idx=")[0][5:].split(".<field init")[0]
            yield (cls, "FI", int(m.group(3)), int(m.group(4), 16), int(m.group(5)))
            continue
        cls, _, meth = full.rpartition(".")
        yield (cls, meth, int(m.group(3)), int(m.group(4), 16), int(m.group(5)))

def orig_nodes():
    for line in open(orig_path):
        d = json.loads(line)
        meth = "FI" if "field init" in d["m"] else d["m"]
        yield (d["c"], meth, d["n"], d["o"], d["i"] & 0xFFFFFFFF)

import os
drop_fi = os.environ.get("KEEP_FI") is None
drops = {d for d in os.environ.get("DROP", "").split(",") if d}
def keep(n):
    if drop_fi and n[1] == "FI":
        return False
    return f"{n[0]}.{n[1]}" not in drops
p = [n for n in port_nodes() if keep(n)]
o = [n for n in orig_nodes() if keep(n)]
print(f"port nodes={len(p)} original nodes={len(o)}")
for i, (a, b) in enumerate(zip(p, o)):
    if a != b:
        print(f"FIRST DIVERGENCE at node #{i}")
        for j in range(max(0, i - ctx), min(len(p), i + ctx)):
            mark = ">>" if j == i else "  "
            print(f"{mark} port {p[j]}")
            if j < len(o):
                print(f"{mark} orig {o[j]}")
        sys.exit(1)
print("sequences identical over the common prefix", min(len(p), len(o)))
