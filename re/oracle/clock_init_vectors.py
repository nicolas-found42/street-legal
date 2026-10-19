"""Original clock initializer and initialized-read state sequences.

The only replacements are the two named performance-counter import thunks.
Both original helpers execute unchanged; output JSONL remains local and ignored.
"""

import argparse
import hashlib
import json
import struct
from pathlib import Path

from clock_vectors import EXE, OUT, SHA, observations
from oracle import Oracle

OUTPUT = OUT.with_name("clock_init.jsonl")


def cases():
    for record in observations():
        if record["cw"] != 0x7F:
            continue
        data = record["input"]
        yield {
            "id": "init-" + record["id"],
            "exe_sha256": SHA,
            "initial": [data["frequency"], data["start"], data["now"]],
            "operations": [
                {
                    "kind": "init",
                    "frequency": data["write_frequency"],
                    "counter": data["write_now"],
                    "status": data["status"],
                }
            ],
        }
    for name, initial, operations in (
        ("init-read", [9, 777, 333], [("init", 1000, 100, 1), ("read", 1000, 350, 1)]),
        (
            "failed-init-read",
            [1000, 80, 333],
            [("init", None, None, 0), ("read", 1000, 350, 1)],
        ),
        (
            "reinitialize",
            [1000, 9, 22],
            [
                ("init", 1000, 100, 1),
                ("read", 1000, 350, 1),
                ("init", 2000, 700, 1),
                ("read", 2000, 1300, 1),
            ],
        ),
        (
            "failed-write-init-read",
            [1000, 80, 333],
            [("init", 500, 900, 0), ("read", None, None, 0)],
        ),
    ):
        yield {
            "id": name,
            "exe_sha256": SHA,
            "initial": initial,
            "operations": [
                {"kind": k, "frequency": f, "counter": c, "status": s}
                for k, f, c, s in operations
            ],
        }


def execute(record, exe):
    o = Oracle(str(exe))
    addresses = (0x76A280, 0x76A288, 0x76A290)
    for address, value in zip(addresses, record["initial"]):
        o.write(address, struct.pack("<q", value))
    results = []
    for operation in record["operations"]:
        calls = []

        def api(name, key, operation=operation, calls=calls):
            def call(oc, args):
                value = operation[key]
                calls.append(
                    {
                        "api": name,
                        "destination": args[0],
                        "write": value,
                        "status": operation["status"],
                    }
                )
                if value is not None:
                    oc.write(args[0], struct.pack("<q", value))
                return operation["status"]

            return call

        for iat, name, key in (
            (0x5E7050, "QueryPerformanceFrequency", "frequency"),
            (0x5E7048, "QueryPerformanceCounter", "counter"),
        ):
            thunk = o.read_u32(iat)
            assert o.import_names[thunk] == "kernel32.dll!" + name
            o.hook(thunk, api(name, key), "stdcall", 1)
        kind = operation["kind"]
        result = o.call(
            0x551690 if kind == "init" else 0x5516C0, fpu_cw=0x7F, limit=1000
        )
        assert result.esp_delta == 0
        results.append(
            {
                "kind": kind,
                "value": result.eax if kind == "init" else result.st0_raw.hex(),
                "state": [struct.unpack("<q", o.read(a, 8))[0] for a in addresses],
                "calls": calls,
            }
        )
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("gen", "verify"))
    parser.add_argument("--exe", type=Path, default=EXE)
    parser.add_argument("--file", type=Path, default=OUTPUT)
    args = parser.parse_args()
    if hashlib.sha256(args.exe.read_bytes()).hexdigest() != SHA:
        parser.error("expected original build606 executable")
    wanted = list(cases())
    if args.action == "gen":
        rows = [{**record, "result": execute(record, args.exe)} for record in wanted]
        args.file.parent.mkdir(parents=True, exist_ok=True)
        args.file.write_text(
            "".join(json.dumps(row, sort_keys=True) + "\n" for row in rows)
        )
        print(f"generated {len(rows)} original initialization sequences")
    else:
        rows = [json.loads(line) for line in args.file.read_text().splitlines()]
        if len(rows) != len(wanted):
            raise SystemExit(
                f"incomplete clock initializer rows: {len(rows)}/{len(wanted)}"
            )
        for recorded, original in zip(rows, wanted):
            if {k: v for k, v in recorded.items() if k != "result"} != original:
                raise SystemExit("initializer vector identity/input mismatch")
            actual = execute(recorded, args.exe)
            if recorded["result"] != actual:
                raise AssertionError((recorded["id"], recorded["result"], actual))
        print(f"verify: {len(rows)}/{len(rows)} identical")


if __name__ == "__main__":
    main()
