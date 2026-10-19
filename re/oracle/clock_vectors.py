"""Original build606 performance-counter clock observations.

Only QueryPerformanceFrequency/QueryPerformanceCounter imports are supplied.
The original helper, signed integer loads and x87 arithmetic execute unchanged.
No derived JSONL is committed. This does not initialize or choose a host clock.
"""

import argparse
import hashlib
import json
import struct
from pathlib import Path

from oracle import Oracle

REPO = Path(__file__).resolve().parents[2]
EXE = REPO / "reference/raw/StreetLegal_Redline.bak.exe"
SHA = "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
OUT = REPO / "re/out/counter-clock/clock.jsonl"
CWS = tuple(
    0x7F | pc | rc for pc in (0, 0x200, 0x300) for rc in (0, 0x400, 0x800, 0xC00)
)
CASES = (
    ("ordinary", 1000, 20, 520, 1000, 520),
    ("nonintegral", 3, 1, 2, 3, 2),
    ("large-cancel", 10_000_000, 2**62, 2**62 + 1, 10_000_000, 2**62 + 1),
    ("wide-delta", 2**63 - 1, -(2**63), 2**63 - 1, 2**63 - 1, 2**63 - 1),
    ("signed", -17, 37, -91, -17, -91),
    ("freq-zero", 0, 0, 1, 0, 1),
    ("zero-zero", 0, 1, 1, 0, 1),
    ("failed-retains", 1000, 10, 410, None, None),
    ("failed-writes", 1000, 10, 410, 500, 910),
)


def observations():
    for cw in CWS:
        for name, frequency, start, now, write_frequency, write_now in CASES:
            yield {
                "id": f"{name}-{cw:04x}",
                "name": name,
                "cw": cw,
                "exe_sha256": SHA,
                "input": {
                    "frequency": frequency,
                    "start": start,
                    "now": now,
                    "write_frequency": write_frequency,
                    "write_now": write_now,
                    "status": 0 if name.startswith("failed") else 1,
                },
            }


def execute(record, exe):
    o = Oracle(str(exe))
    assert o.read_u32(0x5F0910) == 0x447A0000  # The executable's 1000.0f operand.
    data = record["input"]
    for address, key in (
        (0x76A280, "frequency"),
        (0x76A288, "start"),
        (0x76A290, "now"),
    ):
        o.write(address, struct.pack("<q", data[key]))
    calls = []

    def api(name, key):
        def call(oc, args):
            value = data[key]
            calls.append(
                {"api": name, "out": args[0], "return": data["status"], "write": value}
            )
            if value is not None:
                oc.write(args[0], struct.pack("<q", value))
            return data["status"]

        return call

    for iat, name, key in (
        (0x5E7050, "QueryPerformanceFrequency", "write_frequency"),
        (0x5E7048, "QueryPerformanceCounter", "write_now"),
    ):
        thunk = o.read_u32(iat)
        assert o.import_names[thunk] == "kernel32.dll!" + name
        o.hook(thunk, api(name, key), "stdcall", 1)
    result = o.call(0x5516C0, fpu_cw=record["cw"], limit=1000)
    assert result.esp_delta == 0 and result.fpu_top_depth == 1
    return {
        "st0_raw": result.st0_raw.hex(),
        "globals": [
            struct.unpack("<q", o.read(a, 8))[0] for a in (0x76A280, 0x76A288, 0x76A290)
        ],
        "fpcw": result.fpcw,
        "calls": calls,
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("action", choices=("gen", "verify"))
    ap.add_argument("--exe", type=Path, default=EXE)
    ap.add_argument("--file", type=Path, default=OUT)
    args = ap.parse_args()
    if hashlib.sha256(args.exe.read_bytes()).hexdigest() != SHA:
        ap.error("expected original build606 executable")
    wanted = list(observations())
    if args.action == "gen":
        rows = [{**record, "result": execute(record, args.exe)} for record in wanted]
        args.file.parent.mkdir(parents=True, exist_ok=True)
        args.file.write_text(
            "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows)
        )
        print(f"generated {len(rows)} original clock rows: {args.file}")
    else:
        rows = [json.loads(line) for line in args.file.read_text().splitlines()]
        if len(rows) != len(wanted):
            raise SystemExit(f"incomplete clock rows: {len(rows)}/{len(wanted)}")
        for recorded, original_input in zip(rows, wanted):
            if {k: v for k, v in recorded.items() if k != "result"} != original_input:
                raise SystemExit("clock vector identity/input mismatch")
            actual = execute(recorded, args.exe)
            if actual != recorded["result"]:
                raise AssertionError((recorded["id"], actual, recorded["result"]))
        print(f"verify: {len(rows)}/{len(rows)} identical")


if __name__ == "__main__":
    main()
