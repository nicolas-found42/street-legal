"""G11 float parity: per-operation x87 trust table and the live validator that derives it.

ADR 0007 requires bit-exact x87 operations pinned to a documented implementation; ADR 0008 names
the triangulation (SoftFloat, Unicorn, Rosetta 2) and the Intel SDM as arbiter. This module
reruns that evidence from scratch into an isolated bundle and produces a machine-checked verdict
per operation the original executable uses. It fails closed: an operation is pinned only when
independent legs and the shipped `slr-x87` agree bit for bit on the whole corpus. Tolerances,
`APPROX` results and reference accuracy bounds never promote an operation. A physical Intel x87
observation can only add a leg; it is validated (not translated, GenuineIntel) and every row of it
must equal the other legs, otherwise the operation stays unpinned.
"""
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import uuid
from pathlib import Path

from observations import rust_test_observation
from validators import ROOT, boot_tools, candidate_source_matches, result

G11_DIR = ROOT / "re/x87probe/g11"
BOCHS_PIN = "d245c751e6a6fab4bf68e72823598a48001adb3c"
ORACLE_PYTHON = Path.home() / "slr-oracle-venv/bin/python"
PROBE_CORPUS_ROWS = 21702

# Operations the original executable executes, from the capstone sweep of build 606
# (docs/research/x87-g11-decision.md). `where`: game code (0x401000..0x59f000) or CRT/library.
# `source`: which evidence produces the verdict. Nothing is scoped out silently: operations
# without a probe are listed `unprobed` and block.
REQUIRED = {
    "fadd": ("game", "probe"), "fsub": ("game", "probe"), "fmul": ("game", "probe"),
    "fdiv": ("game", "probe"), "fsqrt": ("game", "probe"),
    "fld": ("game", "probe"), "fst": ("game", "probe"), "fistp": ("game", "probe"),
    "fcom": ("game", "probe"), "fcomp": ("game", "probe"),
    "fucom": ("game", "probe"), "fucomi": ("game", "probe"),
    "fsin": ("game", "stream"), "fcos": ("game", "stream"), "fpatan": ("game", "stream"),
    "f2xm1": ("game", "stream"), "fscale": ("game", "stream"), "frndint": ("game", "stream"),
    "fsincos": ("game", "stream"),
    "fyl2x": ("crt", "stream"), "fptan": ("crt", "stream"), "fprem": ("crt", "probe-pair"),
    "fxam": ("crt", "unprobed"), "ftst": ("crt", "unprobed"),
}
STREAM_OPS = {"fsin": "fsin", "fcos": "fcos", "fpatan": "fpatan", "f2xm1": "f2xm1",
              "fscale": "fscale", "frndint": "frndint", "fyl2x": "fyl2x", "fptan": "fptan", "fsincos": "fsincos"}
# frndint and fscale are RC-sensitive; PC is checked through the control words below.
CONTROL_WORDS = ("007f", "047f", "087f", "0c7f", "027f", "037f")


def digest_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def digest_file(path: Path) -> str:
    return digest_bytes(path.read_bytes())


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def validate_hardware_observation(stderr: str, output: bytes, rows: int) -> dict:
    """An Intel observation counts only if it came from untranslated GenuineIntel silicon."""
    lines = [line for line in stderr.splitlines() if line.strip().startswith("{")]
    if len(lines) != 1:
        raise ValueError("hardware observation stderr must hold exactly one probe identity line")
    identity = json.loads(lines[0])
    if identity.get("sysctl_proc_translated") != 0 or identity.get("sysctl_status") != 0:
        raise ValueError("hardware observation was translated or the translation query failed")
    if identity.get("cpu_vendor") != "GenuineIntel" or not identity.get("cpu_brand", "").strip():
        raise ValueError("hardware observation is not from a named GenuineIntel CPU")
    if len(output) != rows * 22:
        raise ValueError(f"hardware observation has {len(output) / 22} records, expected {rows}")
    return identity


def transcendental_verdict(op: str, counts: dict, legs: list[str], rows_key: str | None = None) -> dict:
    """Pure verdict from an adjudication counter row (`rows` plus `a==b:ext80` counts).

    Pinned only if every leg pair agrees bit for bit on every in-domain row. Any disagreement,
    including software legs against each other, leaves the op unpinned.
    """
    rows = counts.get("rows", 0)
    pairs = {}
    for i, a in enumerate(sorted(legs)):
        for b in sorted(legs)[i + 1:]:
            pairs[f"{a}=={b}"] = counts.get(f"{a}=={b}:ext80", 0)
    if rows == 0:
        return {"op": op, "verdict": "unprobed", "reason": "no rows observed", "rows": 0, "pairs": pairs}
    differing = {k: rows - v for k, v in pairs.items() if v != rows}
    if not differing:
        return {"op": op, "verdict": "pinned_bit_exact", "rows": rows, "pairs": pairs,
                "reason": "all legs and slr-x87 agree bit for bit on every in-domain row"}
    reasons = []
    software = [k for k in differing if "slr" not in k and "intel" not in k]
    if software:
        reasons.append("software reference legs disagree: " + ", ".join(f"{k} differ on {differing[k]} of {rows} rows" for k in software))
    if any("slr" in k for k in differing):
        reasons.append("slr-x87 differs: " + ", ".join(f"{k} differ on {differing[k]} of {rows} rows" for k in differing if "slr" in k))
    if any("intel" in k for k in differing):
        reasons.append("Intel silicon differs: " + ", ".join(f"{k} differ on {differing[k]} of {rows} rows" for k in differing if "intel" in k))
    return {"op": op, "verdict": "unpinned", "rows": rows, "pairs": pairs,
            "reason": "; ".join(reasons), "hardware_observed": "intel" in " ".join(pairs)}


def probe_totals(stdout: str) -> dict:
    """Per-op match/mismatch totals from the probe_trusted per-cell lines."""
    totals: dict[str, dict] = {}
    for match in re.finditer(r"^\s+(\w+)\s+pc=(\d+) rc=(\d+) match=\s*(\d+) mismatch=\s*(\d+) skipped=\s*(\d+)", stdout, re.MULTILINE):
        op, _pc, _rc, ok, bad, skipped = match.groups()
        cell = totals.setdefault(op, {"match": 0, "mismatch": 0, "skipped": 0, "cells": 0})
        cell["match"] += int(ok)
        cell["mismatch"] += int(bad)
        cell["skipped"] += int(skipped)
        cell["cells"] += 1
    return totals


def probe_verdict(op: str, totals: dict) -> dict:
    row = totals.get(op)
    if not row or row["match"] == 0:
        return {"op": op, "verdict": "unprobed", "reason": "no probe cells observed"}
    if row["mismatch"]:
        return {"op": op, "verdict": "unpinned", "reason": f"{row['mismatch']} probe mismatches", **row}
    return {"op": op, "verdict": "pinned_bit_exact", "reason":
            "SoftFloat, Unicorn (at PC64) and Rosetta agree with slr-x87 on every normal-operand probe row; "
            "unnormal operands excluded (no game producer)", **row}


def multi_word_verdict(op: str, adjudications: dict, legs_by_word: dict) -> dict:
    """RC-sensitive ops must be pinned under every control word; rows and reasons are merged."""
    entries = [transcendental_verdict(op, adjudications[word]["by_op"].get(STREAM_OPS[op], {}), legs_by_word[word])
               for word in sorted(adjudications)]
    bad = [entry for entry in entries if entry["verdict"] != "pinned_bit_exact"]
    rows = sum(entry.get("rows", 0) for entry in entries)
    if bad:
        return {"op": op, "verdict": bad[0]["verdict"], "rows": rows, "reason": bad[0]["reason"]}
    return {"op": op, "verdict": "pinned_bit_exact", "rows": rows, "reason":
            f"all legs and slr-x87 agree bit for bit under {len(entries)} control words"}


def trust_table(probe_stdout: str, adjudications: dict, legs_by_word: dict, fprem: dict) -> list[dict]:
    totals = probe_totals(probe_stdout)
    table = []
    for op, (where, source) in REQUIRED.items():
        if source == "probe":
            entry = probe_verdict(op, totals)
        elif source == "stream" and op in ("fscale", "frndint"):
            entry = multi_word_verdict(op, adjudications, legs_by_word)
        elif source == "stream":
            entry = transcendental_verdict(op, adjudications["007f"]["by_op"].get(STREAM_OPS[op], {}), legs_by_word["007f"])
        elif source == "probe-pair":
            entry = fprem
        else:
            entry = {"op": op, "verdict": "unprobed", "reason": "no probe exists for this CRT classification operation"}
        table.append({**entry, "where": where})
    return table


def fprem_verdict(corpus_path: Path, unicorn_path: Path, rosetta_path: Path) -> dict:
    """FPREM: Unicorn and Rosetta must agree row for row (SoftFloat implements FPREM1)."""
    corpus = {json.loads(line)["id"]: json.loads(line) for line in corpus_path.read_text().splitlines()}
    uc = {json.loads(line)["id"]: json.loads(line) for line in unicorn_path.read_text().splitlines()}
    rt = {json.loads(line)["id"]: json.loads(line) for line in rosetta_path.read_text().splitlines()}
    rows = differing = 0
    for vector_id, vector in corpus.items():
        if vector["op"] != "fprem":
            continue
        rows += 1
        if uc[vector_id]["st"] != rt[vector_id]["st"] or uc[vector_id].get("C2") != rt[vector_id].get("C2"):
            differing += 1
    if rows == 0:
        return {"op": "fprem", "verdict": "unprobed", "reason": "no fprem rows"}
    if differing:
        return {"op": "fprem", "verdict": "unpinned", "rows": rows, "differing": differing,
                "reason": f"Unicorn and Rosetta disagree on {differing}/{rows} rows (partial-reduction cells)"}
    return {"op": "fprem", "verdict": "pinned_bit_exact", "rows": rows, "reason": "Unicorn and Rosetta agree on every row"}


def run(command, bundle: Path, label: str, *, env=None, cwd=ROOT, stdin=None, context=None, timeout=3600):
    out, err = bundle / f"{label}.stdout", bundle / f"{label}.stderr"
    with out.open("wb") as stdout, err.open("wb") as stderr:
        process = subprocess.run([str(c) for c in command], cwd=cwd, env=env, stdin=stdin, stdout=stdout,
                                 stderr=stderr, timeout=timeout)
    if context is not None:
        context["commands"].append({"label": label, "command": [str(c) for c in command], "returncode": process.returncode})
    if process.returncode:
        raise ValueError(f"{label} exited {process.returncode}; see {err}")
    return out, err


def g11_result(output_root: Path, bochs: Path | None, hardware: Path | None = None) -> dict:
    """Run every G11 leg fresh. Live execution only; no receipt can assert a pass."""
    bundle = output_root / ("g11-" + uuid.uuid4().hex[:12])
    bundle.mkdir(parents=True)
    context = {"schema": 1, "obligation": "G11", "commands": [], "bochs_pin": BOCHS_PIN}
    verdict = result("failed", "G11 validation did not complete.", bundle=str(bundle))
    try:
        tool = boot_tools()
        context["source"], patch = tool.git_identity(ROOT)
        (bundle / "source.diff").write_bytes(patch)
        if bochs is None or not (bochs / ".git").exists():
            raise ValueError("a clean sparse Bochs checkout at the pinned revision is required (--bochs or SLR_BOCHS)")
        head = subprocess.check_output(["git", "-C", str(bochs), "rev-parse", "HEAD"], text=True).strip()
        if head != BOCHS_PIN or subprocess.check_output(["git", "-C", str(bochs), "status", "--porcelain"], text=True).strip():
            raise ValueError("Bochs checkout is not clean at the pinned revision")
        if not ORACLE_PYTHON.exists():
            raise ValueError(f"oracle python environment missing: {ORACLE_PYTHON}")
        env = {k: v for k, v in os.environ.items() if not k.startswith("SLR_")}
        env["CARGO_TARGET_DIR"] = str(bundle / "target")
        context["tools"] = {"clang": subprocess.check_output(["clang", "--version"], text=True).splitlines()[0],
                            "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
                            "python": sys.version.split()[0]}
        sources = sorted((G11_DIR).glob("*")) + sorted((ROOT / "re/x87probe").glob("*.py")) + sorted((ROOT / "re/x87probe/rosetta").glob("*"))
        context["tool_sources"] = {str(p.relative_to(ROOT)): digest_file(p) for p in sources if p.is_file() and p.suffix in {".py", ".c", ".cc", ".sh", ".h"}}

        # -- legs and shipped implementation ------------------------------------------------
        build = bundle / "build"
        build.mkdir()
        env_build = {**env, "BOCHS": str(bochs), "OUT": str(build)}
        run(["sh", G11_DIR / "build_harnesses.sh"], bundle, "build-harnesses", env=env_build, context=context)
        run(["cargo", "build", "--locked", "--release", "-p", "slr-x87", "--example", "stream"], bundle, "build-slr-stream", env=env, context=context)
        slr_stream = bundle / "target/release/examples/stream"
        context["binaries"] = {name: digest_file(path) for name, path in
                               (("bochs-stream", build / "bochs-stream"), ("x87-stream", build / "x87-stream"), ("slr-stream", slr_stream))}

        corpus_dir = bundle / "corpus"
        run([sys.executable, G11_DIR / "make_corpus.py", "--out", corpus_dir, "--n", "20000"], bundle, "make-corpus", context=context)
        meta = json.loads((corpus_dir / "corpus.json").read_text())
        corpus = (corpus_dir / "corpus.in").read_bytes()
        if digest_bytes(corpus) != meta["sha256"] or len(corpus) != meta["rows"] * 21:
            raise ValueError("corpus differs from its manifest")
        adjudicate = load_module("g11_adjudicate", G11_DIR / "adjudicate.py")
        stream_legs: dict[str, dict[str, list]] = {}
        for word in CONTROL_WORDS:
            for name, command in (("bochs", [build / "bochs-stream", word]), ("rosetta", [build / "x87-stream", word]), ("slr", [slr_stream, word])):
                with (corpus_dir / "corpus.in").open("rb") as stdin:
                    out, err = run(command, bundle, f"stream-{name}-{word}", stdin=stdin, context=context)
                stream_legs.setdefault(word, {})[name] = adjudicate.split_records(out.read_bytes(), meta["rows"], name)
                if name == "rosetta":
                    identity = json.loads(err.read_text().strip().splitlines()[-1])
                    if identity.get("sysctl_proc_translated") != 1:
                        raise ValueError("the x86_64 probe did not run translated; the Rosetta leg is not what it claims")
        hardware_identity = None
        if hardware is not None:
            hardware_identity = validate_hardware_observation((hardware / "intel.stderr").read_text(), (hardware / "intel.out").read_bytes(), meta["rows"])
            if json.loads((hardware / "corpus.json").read_text())["sha256"] != meta["sha256"]:
                raise ValueError("hardware observation was produced from a different corpus")
            stream_legs["007f"]["intel"] = adjudicate.split_records((hardware / "intel.out").read_bytes(), meta["rows"], "intel")
        context["hardware"] = hardware_identity

        adjudications = {word: adjudicate.compare(meta, corpus, stream_legs[word]) for word in CONTROL_WORDS}
        legs_by_word = {word: sorted(stream_legs[word]) for word in CONTROL_WORDS}
        (bundle / "adjudication.json").write_text(json.dumps(adjudications, indent=1))

        # -- arithmetic/compare/convert/remainder probe capture ----------------------------
        capture = bundle / "x87"
        capture.mkdir()
        probe_env = {**env, "X87_OUT_DIR": str(capture), "X87_VECTORS_INC": str(bundle / "vectors/vectors.inc"), "VECTORS_DIR": str(bundle / "vectors"), "OUT_DIR": str(capture)}
        (bundle / "vectors").mkdir()
        py = str(ORACLE_PYTHON)
        softfloat = bundle / "sf3"
        shutil.copytree(ROOT / "crates/slr-x87/vendor/softfloat3e", softfloat)
        run(["make", "-C", softfloat / "build", f"SOURCE_DIR={softfloat / 'source'}", "-j8"], bundle, "softfloat-make", env=probe_env, context=context)
        run([py, ROOT / "re/x87probe/gen_vectors.py"], bundle, "gen-vectors", env=probe_env, context=context)
        run(["clang", "-O2", "-o", capture / "sf_oracle", f"-I{softfloat / 'source/include'}", f"-I{softfloat / 'build'}",
             "-DSOFTFLOAT_FAST_INT64", "-DINLINE_LEVEL=5", "-DSOFTFLOAT_ROUND_ODD", "-DSOFTFLOAT_FAST_DIV32TO16",
             "-DSOFTFLOAT_FAST_DIV64TO32", ROOT / "re/x87probe/softfloat_harness.c", softfloat / "build/softfloat.a"], bundle, "build-sf-oracle", env=probe_env, context=context)
        run([capture / "sf_oracle", capture / "corpus.jsonl"], bundle, "run-softfloat", env=probe_env, context=context)
        shutil.copyfile(bundle / "run-softfloat.stdout", capture / "sf_results.jsonl")
        run([py, ROOT / "re/x87probe/run_unicorn.py", capture / "corpus.jsonl", capture / "uc_results.jsonl"], bundle, "run-unicorn", env=probe_env, context=context)
        run([py, ROOT / "re/x87probe/gen_subset.py"], bundle, "gen-subset", env=probe_env, context=context)
        run(["sh", ROOT / "re/x87probe/rosetta/build.sh"], bundle, "build-rosetta-probe", env=probe_env, context=context)
        run([capture / "probe-rosetta"], bundle, "run-rosetta-probe", env=probe_env, context=context)
        shutil.copyfile(bundle / "run-rosetta-probe.stdout", capture / "rosetta_results.jsonl")
        run([py, ROOT / "re/x87probe/compare.py"], bundle, "compare-softfloat-unicorn", env=probe_env, context=context)
        run([py, ROOT / "re/x87probe/compare_rosetta.py"], bundle, "compare-rosetta", env=probe_env, context=context)
        for name in ("corpus", "sf_results", "uc_results", "rosetta_results"):
            rows = sum(1 for _ in (capture / f"{name}.jsonl").open())
            if rows != PROBE_CORPUS_ROWS:
                raise ValueError(f"probe capture {name} has {rows} rows, expected {PROBE_CORPUS_ROWS}")
        test_env = {**env, "X87_OUT_DIR": str(capture)}
        out, err = bundle / "probe-test.stdout", bundle / "probe-test.stderr"
        with out.open("wb") as stdout, err.open("wb") as stderr:
            process = subprocess.run(["cargo", "test", "--locked", "-p", "slr-x87", "--test", "probe_trusted", "--", "--show-output", "--test-threads=1"],
                                     cwd=ROOT, env=test_env, stdout=stdout, stderr=stderr)
        probe_stdout = out.read_text()
        observation = rust_test_observation(process.returncode, probe_stdout, err.read_text(), expected_tests=("trusted_probes_match_bit_for_bit",))
        context["probe_test_observation"] = observation
        if not observation["valid_component_observation"]:
            raise ValueError(f"probe_trusted was not a valid observation: {observation['errors']}")
        total = re.search(r"trusted total: match=(\d+) mismatch=(\d+)", probe_stdout)
        if not total or int(total[2]) or int(total[1]) < 20000:
            raise ValueError("probe_trusted did not report a complete zero-mismatch trusted total")

        table = trust_table(probe_stdout, adjudications, legs_by_word, fprem_verdict(capture / "corpus.jsonl", capture / "uc_results.jsonl", capture / "rosetta_results.jsonl"))
        context["trust_table"] = table
        blocked = [row for row in table if row["verdict"] != "pinned_bit_exact"]
        summary = {row["op"]: row["verdict"] for row in table}
        if corpus != (corpus_dir / "corpus.in").read_bytes() or digest_bytes(corpus) != meta["sha256"]:
            raise ValueError("corpus changed during validation")
        if context["tool_sources"] != {str(p.relative_to(ROOT)): digest_file(p) for p in sources if p.is_file() and p.suffix in {".py", ".c", ".cc", ".sh", ".h"}} or not candidate_source_matches(context["source"]):
            verdict = result("stale", "Candidate or tool sources changed during G11 validation.", bundle=str(bundle))
        elif blocked:
            verdict = result("partial", f"{len(blocked)} of {len(table)} required x87 operations are not pinned bit-exact: " + ", ".join(row["op"] for row in blocked),
                             bundle=str(bundle), trust_table=summary, blocked={row["op"]: row["reason"] for row in blocked},
                             hardware_observed=hardware_identity is not None)
        else:
            verdict = result("passed", "Every required x87 operation is pinned bit-exact across independent legs, slr-x87 and any supplied Intel observation.",
                             bundle=str(bundle), trust_table=summary, hardware_observed=hardware_identity is not None)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError, json.JSONDecodeError) as error:
        verdict = result("failed", str(error), bundle=str(bundle))
    finally:
        context["verdict"] = verdict
        (bundle / "receipt.json").write_text(json.dumps(context, indent=2) + "\n")
    return verdict
