"""S8 / G9: SDAT save and config byte-identity and cross-loading.

S8 runs the scripted native-level sessions in `re/oracle/save_sessions.json`
through the original `java.io.File` natives (Unicorn, `re/oracle/save_session.py`)
and through the port (`cargo run -p slr-io --example save_session`), then
checks, from the raw outputs of this invocation:

- every operation's result, the native file struct and the rebuilt resource
  table / slot run, the companion load requests and the log are equal;
- every file either side wrote is byte-identical (recomputed from the files on
  disk, not from recorded hashes) and well-formed SDAT (header, the exe's type
  words, the footer's own total);
- cross-load: the original reads the files the port wrote and the port reads
  the files the original wrote, with the same results as the same-side reads;
- the stock options save and the eight stock vehicle saves load identically.

The sessions are the native calls `Config.saveConfig`, `ControlSet.save` and
`Vehicle.save` issue. G9's contract says "the same scripted session" through
the original scripts, which needs the VM boot: `gate_result` reports it as
blocked, with the dependencies, and never as passed on S8's evidence alone.
"""
from __future__ import annotations

import hashlib
import json
import os
import subprocess
import sys
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = ROOT / "re/oracle/save_sessions.json"
SPEC_GENERATOR = ROOT / "re/oracle/save_session_spec.py"
DRIVER = ROOT / "re/oracle/save_session.py"
ORACLE_PYTHON = Path(os.environ.get("SLR_ORACLE_PYTHON", Path.home() / "slr-oracle-venv/bin/python"))

SDAT_HEADER = bytes.fromhex("5344415400010200")
# session id -> the type word the script writes first (docs/formats.md SDAT)
TYPE_WORDS = {
    "options_write": 0xFEDCBA98,
    "controls_write": 0x4C525443,
    "vehicle_write": 0x88664422,
}
REQUIRED_NATIVES = {
    "open", "close", "write_int", "write_float", "write_string", "write_res_ref",
    "read_int", "read_float", "read_string", "read_res_id",
}
REQUIRED_SESSIONS = {
    "options_write", "options_read", "controls_write", "controls_read", "vehicle_write",
    "vehicle_read", "float_edge_write", "float_edge_read", "string_edge_write",
    "string_edge_read", "interleaved_write", "native_arms", "stock_options_read",
} | {f"stock_vehicle_{car}_read" for car in (
    "ROC_SS_1", "ROC_GT2_1", "ROC_ST_1", "ROC_MC_1", "ROC_DS_1", "ROC_GT3_1", "ROC_SD_1", "ROC_DS_2")}
STOCK_OPTIONS_TYPE = 0xFEDCBA98

G9_BLOCKERS = (
    "G9 needs the same scripted session through the original scripts "
    "(Config.saveConfig / ControlSet.save / Vehicle.save, and the career save and load "
    "flow): the stock scripts do not boot to a state where they can run until the native-VM "
    "replay frontier (crates/slr-boot, crates/slr-vm) completes.",
    "Vehicle saves carry JPEG companions (<save>.<n>). Their bytes come from the "
    "script-side screenshot/texture natives and the original's JPEG encoder over a rendered "
    "surface; the port has no equivalent producer until the render frontier (D3D9, M5) "
    "exists. File.open's companion loader requests are compared; companion bytes are not.",
)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def digest(path: Path) -> str:
    return sha256(path.read_bytes())


def result(outcome: str, reason: str, **details) -> dict:
    if "result" in details:
        raise ValueError("details cannot replace the checked validator result")
    return {"result": outcome, "reason": reason, **details}


def require(condition, message: str) -> None:
    if not condition:
        raise ValueError(message)


def check_spec(spec: dict) -> dict:
    """The spec must exercise every native and every required session."""
    require(spec.get("schema") == 1 and isinstance(spec.get("sessions"), list), "malformed session spec")
    ids = [session.get("id") for session in spec["sessions"]]
    require(len(ids) == len(set(ids)), "duplicate session ids")
    require(REQUIRED_SESSIONS <= set(ids), f"spec lacks sessions: {sorted(REQUIRED_SESSIONS - set(ids))}")
    ops = {op["op"] for session in spec["sessions"] for op in session["ops"]}
    require(REQUIRED_NATIVES <= ops, f"spec never calls: {sorted(REQUIRED_NATIVES - ops)}")
    for session in spec["sessions"]:
        require(session["ops"], f"empty session {session['id']}")
    return {session["id"]: session for session in spec["sessions"]}


def check_run(run: dict, spec: dict, spec_sha: str, implementation: str, exe_sha: str | None) -> None:
    results = run["results"]
    require(results.get("schema") == 1, f"{implementation}: unsupported results schema")
    require(results.get("implementation") == implementation, f"{implementation}: wrong implementation tag")
    require(results.get("spec_sha256") == spec_sha, f"{implementation}: ran a different spec")
    if exe_sha is not None:
        require(results.get("exe_sha256") == exe_sha, f"{implementation}: not the build-606 executable")
    by_id = check_spec(spec)
    sessions = results.get("sessions")
    require(isinstance(sessions, dict) and set(sessions) == set(by_id), f"{implementation}: session set differs from spec")
    for sid, sess in by_id.items():
        ops = sessions[sid].get("ops")
        require(isinstance(ops, list) and len(ops) == len(sess["ops"]),
                f"{implementation}/{sid}: dropped or extra operations")
        for index, (done, wanted) in enumerate(zip(ops, sess["ops"])):
            require(done.get("op") == wanted["op"] and done.get("file") == wanted["file"],
                    f"{implementation}/{sid}: operation {index} is not the scripted one")


def written_files(run: dict, spec_by_id: dict) -> dict:
    """{(session, name): bytes}, hashed from disk and checked against the record."""
    out = {}
    for sid, sess in run["results"]["sessions"].items():
        seeded = set(spec_by_id[sid].get("seed", {}))
        for name, recorded in sess["files"].items():
            if name in seeded:
                continue
            path = run["dir"] / "files" / sid / name
            require(path.is_file(), f"{sid}/{name}: recorded file is missing")
            data = path.read_bytes()
            require(sha256(data) == recorded, f"{sid}/{name}: recorded hash does not match the file")
            out[(sid, name)] = data
    return out


def check_sdat(sid: str, name: str, data: bytes) -> None:
    require(data[:8] == SDAT_HEADER, f"{sid}/{name}: not a version-2 SDAT header")
    require(len(data) >= 20, f"{sid}/{name}: shorter than header plus empty footer")
    total = int.from_bytes(data[-4:], "little")
    start = len(data) - 4 - total
    require(8 <= start <= len(data) - 12, f"{sid}/{name}: footer total {total} does not fit the file")
    expected_type = TYPE_WORDS.get(sid)
    if expected_type is not None:
        require(int.from_bytes(data[8:12], "little") == expected_type, f"{sid}/{name}: wrong type word")


def compare(runs: dict, spec: dict, spec_sha: str, exe_sha: str | None) -> dict:
    """Pure comparison of four complete runs; raises ValueError on any difference.

    runs: original, port (own files), original_cross (original reading the
    port's files), port_cross (port reading the original's files); each
    {"results": results.json, "dir": Path}.
    """
    by_id = check_spec(spec)
    for name, implementation in (("original", "original"), ("port", "port"),
                                 ("original_cross", "original"), ("port_cross", "port")):
        check_run(runs[name], spec, spec_sha, implementation, exe_sha if implementation == "original" else None)
    original, port = runs["original"]["results"]["sessions"], runs["port"]["results"]["sessions"]
    compared_ops = 0
    for sid in by_id:
        for index, (a, b) in enumerate(zip(original[sid]["ops"], port[sid]["ops"])):
            require(a == b, f"{sid}: operation {index} differs between original and port: {a} != {b}")
            compared_ops += 1
        require(original[sid]["files"] == port[sid]["files"], f"{sid}: file sets or hashes differ")
    files_original = written_files(runs["original"], by_id)
    files_port = written_files(runs["port"], by_id)
    require(files_original.keys() == files_port.keys(), "original and port wrote different files")
    for key, data in files_original.items():
        require(files_port[key] == data, f"{key[0]}/{key[1]}: bytes differ between original and port")
        check_sdat(key[0], key[1], data)
    require(set(TYPE_WORDS) <= {sid for sid, _ in files_original}, "options, controls and vehicle saves must be written")
    # cross-load: the other side's files, the same results as the same-side read
    cross_reads = [sid for sid, sess in by_id.items() if any(s.get("from") == "session" for s in sess.get("seed", {}).values())]
    require(len(cross_reads) >= 5, "too few cross-load sessions")
    for sid in cross_reads:
        for label, cross, own in (("original reads port files", "original_cross", "original"),
                                  ("port reads original files", "port_cross", "port")):
            got = runs[cross]["results"]["sessions"][sid]["ops"]
            want = runs[own]["results"]["sessions"][sid]["ops"]
            require(got == want, f"{sid}: cross-load differs ({label})")
    cross_files = {
        "original_cross": written_files(runs["original_cross"], by_id),
        "port_cross": written_files(runs["port_cross"], by_id),
    }
    for name, files in cross_files.items():
        require(files == files_original, f"{name}: re-run wrote different files than the first run")
    # the comparison must have looked at real data, not an all-error session
    for sid in cross_reads:
        ops = original[sid]["ops"]
        require(ops[0]["op"] == "open" and ops[0]["result"] == 1 and ops[0]["handle"], f"{sid}: seeded file did not open")
        require(any(op.get("result") not in (None, 0) for op in ops[1:]), f"{sid}: every read came back empty")
    tables = original["vehicle_read"]["ops"][0]["tables"]
    require(len(tables["resources"]) >= 3 and len(tables["slots"]) >= 2, "vehicle_read rebuilt no tables")
    require(any(op.get("loads") for op in original["vehicle_read"]["ops"][:1]), "vehicle_read requested no companion loads")
    stock = original["stock_options_read"]["ops"]
    require(stock[0]["result"] == 1 and stock[1]["result"] == STOCK_OPTIONS_TYPE, "stock options did not load")
    for sid in REQUIRED_SESSIONS:
        if sid.startswith("stock_vehicle_"):
            first = original[sid]["ops"][0]
            require(first["result"] == 1, f"{sid}: stock vehicle did not open")
            require(first["tables"]["slots"] and first["tables"]["resources"], f"{sid}: stock vehicle rebuilt no tables")
    return {
        "sessions": len(by_id), "operations_compared": compared_ops,
        "files_compared": len(files_original), "cross_load_sessions": len(cross_reads),
        "bytes_compared": sum(len(data) for data in files_original.values()),
    }


def load_run(directory: Path) -> dict:
    return {"results": json.loads((directory / "results.json").read_text()), "dir": directory}


def run_command(command, bundle: Path, label: str, context: dict, **kwargs):
    with (bundle / f"{label}.stdout").open("w") as out, (bundle / f"{label}.stderr").open("w") as err:
        process = subprocess.run(command, stdout=out, stderr=err, **kwargs)
    context["commands"].append({"command": [str(part) for part in command], "returncode": process.returncode,
                                "stdout": f"{label}.stdout", "stderr": f"{label}.stderr"})
    require(process.returncode == 0, f"{label} exited {process.returncode}; see retained outputs")


def saves_result(output_root: Path, stock_identity, candidate_identity, candidate_matches, original_exe: str) -> dict:
    """S8: execute and compare fresh original and port sessions in this invocation.

    The caller supplies validators' stock/source helpers so this module stays
    importable by data-free self-tests.
    """
    bundle = output_root / ("saves-" + uuid.uuid4().hex[:12])
    bundle.mkdir(parents=True)
    context = {"schema": 1, "obligation": "S8", "commands": [], "scope": "native-level scripted sessions"}
    verdict = result("failed", "Saves validation did not complete.", bundle=str(bundle))
    try:
        context["source"], patch = candidate_identity()
        (bundle / "source.diff").write_bytes(patch)
        context["stock"] = {"manifest_sha256": stock_identity(ROOT / "stock")["manifest_sha256"]}
        exe = ROOT / "reference/raw/StreetLegal_Redline.bak.exe"
        require(exe.is_file() and digest(exe) == original_exe, "oracle executable is not the build-606 original")
        require(ORACLE_PYTHON.is_file(), f"oracle Python missing: {ORACLE_PYTHON} (set SLR_ORACLE_PYTHON)")
        run_command([sys.executable, SPEC_GENERATOR, "--check"], bundle, "spec-check", context, cwd=ROOT)
        spec_bytes = SPEC.read_bytes()
        spec = json.loads(spec_bytes)
        spec_sha = sha256(spec_bytes)
        env = {key: value for key, value in os.environ.items() if not key.startswith("SLR_")}
        env["CARGO_TARGET_DIR"] = str(bundle / "target")
        run_command(["cargo", "build", "--locked", "-p", "slr-io", "--example", "save_session"], bundle, "build",
                    context, cwd=ROOT, env=env)
        binary = bundle / "target/debug/examples/save_session"
        require(binary.is_file(), "port runner was not built")
        context["binary_sha256"] = digest(binary)

        def original(name, seed_from=None):
            command = [ORACLE_PYTHON, DRIVER, "run", "--spec", SPEC, "--out", bundle / name,
                       "--stock", ROOT / "stock", "--exe", exe]
            if seed_from:
                command += ["--seed-from", bundle / seed_from]
            run_command(command, bundle, name, context, cwd=ROOT)

        def port(name, seed_from=None):
            command = [binary, "--spec", SPEC, "--out", bundle / name, "--stock", ROOT / "stock"]
            if seed_from:
                command += ["--seed-from", bundle / seed_from]
            run_command(command, bundle, name, context, cwd=ROOT)

        original("original")
        port("port")
        original("original_cross", seed_from="port")
        port("port_cross", seed_from="original")
        runs = {name: load_run(bundle / name) for name in ("original", "port", "original_cross", "port_cross")}
        context["comparison"] = compare(runs, spec, spec_sha, original_exe)
        require(context["stock"]["manifest_sha256"] == stock_identity(ROOT / "stock")["manifest_sha256"],
                "stock changed during validation")
        if not candidate_matches(context["source"]):
            verdict = result("stale", "Candidate source changed during saves validation.", bundle=str(bundle))
        else:
            verdict = result(
                "passed",
                "Fresh original (Unicorn) and port runs of the native-level scripted save/config sessions are "
                "byte-identical and cross-load both ways. Scope: native-level; script-level sessions (G9) remain blocked.",
                bundle=str(bundle), **context["comparison"])
    except (OSError, ValueError, KeyError, TypeError, IndexError, subprocess.SubprocessError) as error:
        verdict = result("failed", str(error), bundle=str(bundle))
    finally:
        context["verdict"] = verdict
        (bundle / "receipt.json").write_text(json.dumps(context, indent=2, default=str) + "\n")
    return verdict


def gate_result(s8: dict | None) -> dict:
    """G9 is never passed by native-level evidence; report the exact blockers."""
    return result("blocked", " ".join(G9_BLOCKERS), prerequisite=s8, blockers=list(G9_BLOCKERS))
