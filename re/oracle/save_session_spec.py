#!/usr/bin/env python3
"""Generator for `save_sessions.json`, the scripted save/config sessions of
G9 / S8 (see `save_session.py` and docs/saves-e2e.md).

The spec is authored data (no original-derived bytes): a list of native-level
sessions, each an ordered list of `File` native calls, plus the resource-kind
table the manager lookups use. Both implementations run the same spec:

  - original: `re/oracle/save_session.py run` (real exe natives under Unicorn)
  - port:     `cargo run -p slr-io --example save_session` (`FileHost`)

Write sessions mirror what the stock scripts issue (`Config.saveConfig`,
`ControlSet.save`, `Vehicle.save`: docs/formats.md SDAT); read sessions mirror
the writes (`mirror()`) and are seeded from the files the other side wrote
(cross-load) or from stock saves.

  save_session_spec.py            # rewrite save_sessions.json
  save_session_spec.py --check    # exit 1 when the checked-in file differs
"""
from __future__ import annotations

import argparse
import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = os.path.join(HERE, "save_sessions.json")

OPTIONS_TYPE = 0xFEDCBA98
VEHICLE_TYPE = 0x88664422
CONTROLS_TYPE = 0x4C525443  # "CTRL"

# The manager's initial descriptor array is dense. Unused slots are authored
# profile inputs, rather than holes whose names/registration state are guessed.
KINDS = {"0": "declared/seed0.rpk", "1": "declared/seed1.rpk", "2": "declared/seed2.rpk",
         "3": "data/cars.rpk", "4": "data/parts.rpk", "5": "declared/seed5.rpk", "6": "system/sys.rpk"}


def f(x: float) -> int:
    return struct.unpack("<I", struct.pack("<f", x))[0]


def wi(file, value):
    return {"op": "write_int", "file": file, "value": value & 0xFFFFFFFF}


def wf(file, bits):
    return {"op": "write_float", "file": file, "bits": bits & 0xFFFFFFFF}


def ws(file, data: bytes):
    return {"op": "write_string", "file": file, "hex": data.hex()}


def wr(file, ref_id, kind):
    return {"op": "write_res_ref", "file": file, "ref": {"id": ref_id, "kind": kind}}


def wr_null(file):
    return {"op": "write_res_ref", "file": file, "ref": None}


def opn(file, name, mode):
    return {"op": "open", "file": file, "name": name, "mode": mode}


def cls(file):
    return {"op": "close", "file": file}


MIRROR = {"write_int": "read_int", "write_float": "read_float",
          "write_string": "read_string", "write_res_ref": "read_res_id"}


def mirror(ops, file):
    """The reads that load back what `ops` (writes to `file`) stored."""
    return [{"op": MIRROR[o["op"]], "file": file} for o in ops if o["op"] in MIRROR]


def options_body():
    """`Config.saveConfig`: (id=12, ver=0), one word per field, footer words."""
    body = [wi("f", 12), wi("f", 0)]
    floats = [1.0, 0.25, 800.0, 600.0, 0.5, -1.0, 100.0, 0.0001, 3.5, 12.75]
    for i in range(37):
        if i % 4 == 0:
            body.append(wf("f", f(floats[i % len(floats)])))
        else:
            body.append(wi("f", (i * 2654435761) & 0xFFFFFFFF if i % 3 else i))
    return body


def controls_body():
    """`ControlSet.save`: (id=16, ver=2), device names, a struct run, vasp."""
    body = [wi("f", 16), wi("f", 2)]
    body += [ws("f", b"Keyboard"), ws("f", b"Mouse (USB)"), ws("f", b"")]
    body.append(wi("f", 4))
    for i in range(4):
        body += [wi("f", i), wi("f", i + 10), wi("f", 0), wi("f", i * 3)]
        body += [wf("f", f(-1.0)), wf("f", f(1.0)), wf("f", f(0.0)), wf("f", f(100.0)), wf("f", f(0.05))]
    body.append(wi("f", 2))
    for i in range(2):
        body += [wi("f", i + 1)] + [wf("f", f(0.5 * (j + i))) for j in range(5)]
    return body


def vehicle_body():
    """`Vehicle.save`: (id=5, ver=3), part records with resource references:
    manager-kind ids (resource table), script-side ids (JPEG slot run, kinds 5
    and 7), a repeat of each, a zero id, a null ref and an unsaveable kind."""
    body = [wi("f", 5), wi("f", 3), ws("f", b"Test Car"), wf("f", f(1234.5))]
    body += [
        wr("f", (3 << 16) | 7, 5), wr("f", (3 << 16) | 9, 5), wr("f", (4 << 16) | 1, 5),
        wi("f", 0xDEADBEEF),
        wr("f", (0xFFFF << 16) | 7, 5), wr("f", (0xFFFF << 16) | 8, 7), wr("f", (0xFFFF << 16) | 7, 5),
        wr("f", (6 << 16) | 2, 5), wr("f", (3 << 16) | 7, 5),
        wr("f", 0, 5), wr_null("f"), wr("f", (0xFFFF << 16) | 9, 3),
        wf("f", f(-0.0)), wf("f", f(float("inf"))), wi("f", 0x88664422),
    ]
    return body


def float_edge_body():
    return [
        wf("f", 0x00000000), wf("f", 0x80000000), wf("f", 0x00000001), wf("f", 0x007FFFFF),
        wf("f", 0x00800000), wf("f", 0x7F7FFFFF), wf("f", 0x7F800000), wf("f", 0xFF800000),
        wf("f", 0x7FC00000), wf("f", 0xFFC12345),
        wi("f", 0xFFFFFFFF), wi("f", 0x80000000), wi("f", 0),
    ]


def string_edge_body():
    return [
        ws("f", b""), ws("f", b"a"), ws("f", bytes(range(1, 128))), ws("f", b"\x80\xff\xfe high bytes"),
        ws("f", b"x" * 255), ws("f", b"save/game/options"), ws("f", b"name with spaces"),
    ]


def write_session(sid, name, body, type_word=None):
    ops = [opn("f", name, 1)]
    if type_word is not None:
        ops.append(wi("f", type_word))
    ops += body
    ops.append(cls("f"))
    return {"id": sid, "ops": ops, "_body": body, "_name": name, "_type": type_word}


def read_session(sid, writer, cross=True):
    """Read back `writer`'s file. Seeded from the writer's output."""
    name = writer["_name"]
    ops = [opn("g", name, 0)]
    if writer["_type"] is not None:
        ops.append({"op": "read_int", "file": "g"})
    ops += mirror(writer["_body"], "g")
    ops.append(cls("g"))
    return {"id": sid, "seed": {name: {"from": "session", "session": writer["id"], "name": name}},
            "ops": ops, "cross": cross}


def build():
    opt = write_session("options_write", "save/game/options_s8", options_body(), OPTIONS_TYPE)
    ctl = write_session("controls_write", "save/game/controls_s8", controls_body(), CONTROLS_TYPE)
    veh = write_session("vehicle_write", "save/cars/car_s8", vehicle_body(), VEHICLE_TYPE)
    flt = write_session("float_edge_write", "save/game/floats_s8", float_edge_body(), OPTIONS_TYPE)
    stg = write_session("string_edge_write", "save/game/strings_s8", string_edge_body(), OPTIONS_TYPE)
    # signalling NaNs are written raw (the bytes are compared) but read back as
    # words: x87 FLD quiets them on hardware, which Unicorn's FPU does not model
    snan = write_session("snan_write", "save/game/snan_s8", [wf("f", 0x7F800001), wf("f", 0xFFBFFFFF)], OPTIONS_TYPE)
    snan_read = read_session("snan_read", snan)
    snan_read["ops"] = [o if o["op"] != "read_float" else {**o, "op": "read_int"} for o in snan_read["ops"]]
    empty = write_session("empty_write", "save/game/empty_s8", [], None)
    # two files open at once, interleaved, then a truncating rewrite of the first
    inter = {"id": "interleaved_write", "ops": [
        opn("a", "save/game/a_s8", 1), opn("b", "save/game/b_s8", 1),
        wi("a", 1), wi("b", 2), wr("a", (3 << 16) | 1, 5), wr("b", (4 << 16) | 1, 5),
        wr("a", (0xFFFF << 16) | 1, 5), wf("b", f(2.5)), cls("b"), wi("a", 3), cls("a"),
        opn("a", "save/game/a_s8", 1), wi("a", 99), cls("a"),
    ]}
    # native edge arms: closed / unopened file objects, invalid mode, reopen read<-write
    arms = {"id": "native_arms", "ops": [
        wi("u", 1), wf("u", f(1.0)), ws("u", b"x"), wr("u", (3 << 16) | 1, 5),
        {"op": "read_int", "file": "u"}, {"op": "read_string", "file": "u"},
        {"op": "read_res_id", "file": "u"}, cls("u"),
        opn("m", "save/game/arm_s8", 7),
        opn("m", "save/game/arm_s8", 1), cls("m"), wi("m", 5),
        opn("m", "save/game/arm_s8", 1), wi("m", 5), cls("m"), opn("m", "save/game/arm_s8", 0), cls("m"),
        opn("n", "save/game/does_not_exist_s8", 0),
    ]}
    sessions = [opt, ctl, veh, flt, snan, stg, empty, inter, arms]
    reads = [
        snan_read,
        read_session("options_read", opt), read_session("controls_read", ctl),
        read_session("vehicle_read", veh), read_session("float_edge_read", flt),
        read_session("string_edge_read", stg),
    ]
    # reads that return the footer words past the body / re-read after the end
    over = read_session("vehicle_overread", veh)
    over["ops"] = over["ops"][:-1] + [{"op": "read_int", "file": "g"}] * 4 + [{"op": "read_res_id", "file": "g"},
                                                                              {"op": "read_string", "file": "g"}, cls("g")]
    reads.append(over)
    # stock saves (local-only inputs, never committed): the options file and the vehicles
    stock = [{"id": "stock_options_read", "seed": {"save/game/options": {"from": "stock", "path": "save/game/options"}},
              "ops": [opn("g", "save/game/options", 0)] + [{"op": "read_int", "file": "g"}] * 40 +
              [cls("g"), opn("g", "save/game/options", 0)] + [{"op": "read_float", "file": "g"}] * 40 + [cls("g")]}]
    for car in ("ROC_SS_1", "ROC_GT2_1", "ROC_ST_1", "ROC_MC_1", "ROC_DS_1", "ROC_GT3_1", "ROC_SD_1", "ROC_DS_2"):
        path = f"cars/racers/ROC_cars/{car}"
        stock.append({"id": f"stock_vehicle_{car}_read", "seed": {path: {"from": "stock", "path": path}},
                      "ops": [opn("g", path, 0)] + [{"op": "read_int", "file": "g"}] * 6 +
                      [{"op": "read_res_id", "file": "g"}, {"op": "read_int", "file": "g"}, cls("g")],
                      "stock": True})
    # A read-only authored SDAT with unseen archive names, case reuse and a
    # reopen: no writer-side manager state can supply the expected mapping.
    footer = bytearray(struct.pack("<II", 0, 3))
    for index, path in enumerate(("new/UNKNOWN.rpk", "NEW/unknown.rpk", "new/other.rpk"), 1):
        raw = path.encode() + b"\0"
        footer.extend(struct.pack("<II", index, len(raw)) + raw)
    raw = bytes.fromhex("5344415400010200") + struct.pack("<III", 0x10007, 0x20009, 0x3000B)
    raw += footer + struct.pack("<I", len(footer))
    name = "save/cars/unseen_s8"
    unknown = {"id": "unseen_archives", "seed": {name: {"from": "hex", "hex": raw.hex()}}, "ops": []}
    for _ in range(2):
        unknown["ops"] += [opn("g", name, 0)] + [{"op": "read_res_id", "file": "g"}] * 3 + [cls("g")]
    stock.append(unknown)
    out = []
    for s in sessions + reads + stock:
        s = {k: v for k, v in s.items() if not k.startswith("_")}
        out.append(s)
    return {"schema": 1, "kinds": KINDS, "sessions": out}


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args(argv)
    text = json.dumps(build(), indent=1, sort_keys=True) + "\n"
    if args.check:
        current = open(SPEC).read() if os.path.exists(SPEC) else ""
        if current != text:
            print("save_sessions.json is stale; run save_session_spec.py", file=sys.stderr)
            return 1
        return 0
    with open(SPEC, "w") as handle:
        handle.write(text)
    print(f"wrote {SPEC}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
