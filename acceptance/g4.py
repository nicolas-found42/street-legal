"""G4 physics level 1: every physics native differential-tested, 0 ulp on trusted operations.

The validator enumerates every native of the physics classes from the original registration table
(`re/out/native_api.tsv`, 353 rows), joins them with the generated fidelity ledger, regenerates the
cfg-handler oracle vectors fresh into an isolated bundle, runs the named tests under the strict
observation wrapper and classifies each function:

- `passing`: ledger row passing and every named test observed passing in this run;
- `blocked_by_g11`: passing, but its implementation calls an `APPROX` x87 transcendental, so a
  zero-ulp claim is not available until G11 pins that operation (ADR 0007);
- `no_test`: ported with no differential test;
- `unimplemented`: no ledger row for the original address (capture-only / missing).

G4 passes only when every physics function is `passing`. Exact-arithmetic natives are reported
separately from transcendental-dependent ones, so G11 progress moves exactly the second group.
"""
from __future__ import annotations

import json
import importlib.util
import os
import re
import subprocess
import uuid
from collections import Counter, defaultdict
from pathlib import Path

from observations import rust_test_observation
from validators.core import ORIGINAL_EXE
from validators import ROOT, boot_tools, candidate_source_matches, digest, result

# Classes whose natives drive the vehicle/world simulation (docs/physics.md sections 8-9). `Math`
# is included because the Pacejka/RK4 paths reach `sqrt` and the CRT RNG through it; `SfxTable`
# and `DynoData` feed the engine/dyno model.
PHYSICS_CLASSES = (
    "java.game.parts.WheelRef", "java.game.parts.Part", "java.game.parts.bodypart.Chassis",
    "java.game.parts.DynoData", "java.game.parts.SfxTable", "java.game.Vehicle",
    "java.util.resource.PhysicsRef", "java.util.resource.GroundRef", "java.lang.Math",
)
ROW = re.compile(r"^\| `((?:FUN|LAB)_[0-9a-f]{8})` \| (.*?) \| (.*?) \| (.*?) \|$")
TEST = re.compile(r"`([\w-]+)::([\w:]+)`")
APPROX_CALL = re.compile(r"\bapprox_(?:sin|cos|patan|f2xm1|fyl2x)\b|\btrans::approx_")


def physics_natives(native_api: str) -> list[dict]:
    rows = [line.split("\t") for line in native_api.splitlines()]
    if len(rows) != 353 or any(len(row) != 4 for row in rows):
        raise ValueError("native registration table must hold exactly 353 complete rows")
    return [{"class": cls, "name": name, "signature": sig, "address": addr}
            for cls, name, sig, addr in rows if cls in PHYSICS_CLASSES]


def verified_physics_natives(native_api: str, executable: Path) -> list[dict]:
    """Bind the complete denominator to the extraction-verified original bytes."""
    if digest(executable) != ORIGINAL_EXE:
        raise ValueError("original executable identity mismatch")
    spec = importlib.util.spec_from_file_location("g4_registration", ROOT / "re/build_native_table.py")
    extraction = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(extraction)
    binary = executable.read_bytes()
    image_base, _, sections = extraction.parse_pe(binary)
    expected = []
    for site, cls, name, signature, implementation in extraction.binary_rows(binary):
        if site not in extraction.CONTROLLER_SITES:
            expected.append((*(extraction.cstr(binary, sections, image_base, word) for word in (cls, name, signature)), implementation))
    validate_native_rows(native_api, expected)
    return physics_natives(native_api)


def validate_native_rows(native_api: str, expected: list[tuple]) -> None:
    """Every registered row, including its class, signature and address must match."""
    actual = []
    for line in native_api.splitlines():
        fields = line.split("\t")
        if len(fields) != 4 or not re.fullmatch(r"(?:FUN|LAB)_[0-9a-f]{8}", fields[3]):
            raise ValueError("malformed native registration row")
        actual.append((*fields[:3], int(fields[3][4:], 16)))
    if len(expected) != 353 or len(set(expected)) != 353 or actual != expected:
        raise ValueError("native registration table differs from the verified original executable")


def ledger_rows(fidelity: str) -> dict[str, dict]:
    rows = {}
    for line in fidelity.splitlines():
        match = ROW.match(line)
        if match:
            address, location, status, evidence = match.groups()
            sources = [part.split(":")[0].strip("`") for part in location.split("<br>")]
            rows[address] = {"sources": sources, "status": status, "tests": TEST.findall(evidence),
                             "oracles": re.findall(r"\(([^)]*)\)\s*$", evidence)}
    return rows


def approx_files(root: Path) -> set[str]:
    """Production sources that call an APPROX transcendental (tests excluded)."""
    found = set()
    for path in sorted((root / "crates").rglob("*.rs")):
        relative = path.relative_to(root).as_posix()
        if "/tests/" in relative or relative.startswith(("crates/slr-x87/", "crates/slr-core/", "crates/slr-game/")):
            continue
        if APPROX_CALL.search(path.read_text()):
            found.add(relative)
    return found


def classify(natives: list[dict], ledger: dict[str, dict], physics_rows: set[str], observed: set[tuple[str, str]], approx: set[str], fixtures: set[str] | None = None) -> list[dict]:
    """Per-function verdicts. `observed`: (crate, test) pairs seen passing in this run."""
    functions = [{"kind": "native", **native} for native in natives]
    seen = {native["address"] for native in natives}
    functions += [{"kind": "port", "class": "slr-physics cfg", "name": address, "address": address}
                  for address in sorted(physics_rows - seen)]
    verdicts = []
    for function in functions:
        row = ledger.get(function["address"])
        entry = {**function}
        if row is None:
            entry.update(verdict="unimplemented", reason="no ledger row: not ported, or capture-only")
        elif not row["status"].startswith("passing"):
            entry.update(verdict="no_test", reason=row["status"], sources=row["sources"])
        elif fixtures is not None and function["address"] in physics_rows and function["address"] not in fixtures:
            entry.update(verdict="no_test", sources=row["sources"],
                         reason="the freshly generated oracle vectors contain no row for this exe address")
        elif not row["tests"] or any(test not in observed for test in row["tests"]):
            missing = [f"{crate}::{name}" for crate, name in row["tests"] if (crate, name) not in observed]
            entry.update(verdict="no_test", sources=row["sources"], reason="named tests not observed passing this run: " + ", ".join(missing or ["none named"]))
        elif any(source in approx for source in row["sources"]):
            entry.update(verdict="blocked_by_g11", sources=row["sources"], tests=[f"{c}::{n}" for c, n in row["tests"]],
                         reason="implementation calls an APPROX transcendental; zero-ulp needs G11 pinned")
        else:
            entry.update(verdict="passing", sources=row["sources"], tests=[f"{c}::{n}" for c, n in row["tests"]],
                         oracles=row["oracles"], reason="differential test observed passing")
        verdicts.append(entry)
    return verdicts


def summarize(verdicts: list[dict]) -> dict:
    by_class = defaultdict(Counter)
    for entry in verdicts:
        by_class[entry["class"]][entry["verdict"]] += 1
    return {cls: dict(counts) for cls, counts in sorted(by_class.items())}


def run_tests(bundle: Path, env: dict, crate: str, filters: list[str], label: str) -> dict:
    command = ["cargo", "test", "--locked", "-p", crate, "--", "--show-output", "--test-threads=1", *filters]
    stdout, stderr = bundle / f"{label}.stdout", bundle / f"{label}.stderr"
    with stdout.open("wb") as out, stderr.open("wb") as err:
        process = subprocess.run(command, cwd=ROOT, env=env, stdout=out, stderr=err, timeout=3600)
    text = stdout.read_text() + "\n" + stderr.read_text()
    passed = set(re.findall(r"^test (\S+) \.\.\. ok$", text, re.MULTILINE))
    # Filters match by substring, so the filtered-out count is expected; every requested name must
    # match at least one passing test (checked by the caller), and skips/failures still invalidate.
    named = [path for path in passed if any(path == name or path.endswith("::" + name) for name in filters)]
    observation = rust_test_observation(process.returncode, stdout.read_text(), stderr.read_text(), expected_tests=named)
    return {"command": command, "returncode": process.returncode, "observation": observation, "passed": passed}


def fixture_addresses(vectors: Path) -> set[str]:
    """Exe addresses (as ledger symbols) that the freshly generated cfg vectors actually exercise."""
    covered = set()
    for name in ("handlers_stock.jsonl", "handlers_modded.jsonl", "leaves_car.jsonl", "leaves_part.jsonl"):
        for line in (vectors / name).read_text().splitlines():
            for row in json.loads(line)["lines"]:
                if row.get("exe_addr"):
                    covered.add("FUN_%08x" % int(row["exe_addr"], 16))
    for line in (vectors / "leaves_direct.jsonl").read_text().splitlines():
        covered.add("FUN_%08x" % int(json.loads(line)["exe_addr"], 16))
    return covered


def g4_result(output_root: Path) -> dict:
    bundle = output_root / ("g4-" + uuid.uuid4().hex[:12])
    bundle.mkdir(parents=True)
    context = {"schema": 1, "obligation": "G4", "commands": []}
    verdict = result("failed", "G4 validation did not complete.", bundle=str(bundle))
    try:
        tool = boot_tools()
        context["source"], patch = tool.git_identity(ROOT)
        (bundle / "source.diff").write_bytes(patch)
        native_api = ROOT / "re/out/native_api.tsv"
        executable = ROOT / "reference/raw/StreetLegal_Redline.bak.exe"
        natives = verified_physics_natives(native_api.read_text(), executable)
        context["original_exe_sha256"] = digest(executable)
        context["native_api_sha256"] = digest(native_api)
        fidelity = (ROOT / "docs/fidelity.md").read_text()
        ledger = ledger_rows(fidelity)
        physics_rows = {address for address, row in ledger.items() if any(s.startswith("crates/slr-physics/") for s in row["sources"])}
        oracle_python = Path.home() / "slr-oracle-venv/bin/python"
        if not oracle_python.exists():
            raise ValueError("oracle python environment missing")
        # Fresh cfg-handler oracle vectors into the isolated bundle (never the shared re/out).
        vectors = bundle / "vectors"
        vectors.mkdir()
        env = {key: value for key, value in os.environ.items() if not key.startswith("SLR_")}
        env.update({"CARGO_TARGET_DIR": str(bundle / "target"), "SLR_CFG_VECTORS_OUT": str(vectors),
                    "SLR_CFG_HANDLER_VECTORS": str(vectors), "SLR_REQUIRE_ORACLE_FIXTURES": "1",
                    "SLR_STOCK": str(ROOT / "stock"), "SLR_REFERENCE": str(ROOT / "reference/raw")})
        for label, args in (("gen-all", ["gen", "all"]),):
            with (bundle / f"{label}.stdout").open("wb") as out, (bundle / f"{label}.stderr").open("wb") as err:
                process = subprocess.run([str(oracle_python), str(ROOT / "re/oracle/cfg_handler_vectors.py"), *args], cwd=ROOT, env=env, stdout=out, stderr=err, timeout=3600)
            context["commands"].append({"label": label, "returncode": process.returncode})
            if process.returncode:
                raise ValueError(f"{label} exited {process.returncode}")
        context["vectors"] = {p.name: digest(p) for p in sorted(vectors.iterdir())}
        observed: set[tuple[str, str]] = set()
        tests_by_crate: dict[str, set[str]] = defaultdict(set)
        for row in ledger.values():
            if row["status"].startswith("passing"):
                for crate, name in row["tests"]:
                    tests_by_crate[crate].add(name)
        wanted = {address for address in ({n["address"] for n in natives} | physics_rows)}
        needed_crates = defaultdict(set)
        for address in wanted:
            row = ledger.get(address)
            if row and row["status"].startswith("passing"):
                for crate, name in row["tests"]:
                    needed_crates[crate].add(name)
        for crate, names in sorted(needed_crates.items()):
            outcome = run_tests(bundle, env, crate, sorted(names), f"tests-{crate}")
            context["commands"].append({"label": f"tests-{crate}", "returncode": outcome["returncode"]})
            if not outcome["observation"]["valid_component_observation"]:
                context.setdefault("invalid_runs", {})[crate] = outcome["observation"]["errors"]
                continue
            for name in names:
                if any(path == name or path.endswith("::" + name) for path in outcome["passed"]):
                    observed.add((crate, name))
        verdicts = classify(natives, ledger, physics_rows, observed, approx_files(ROOT), fixture_addresses(vectors))
        counts = Counter(entry["verdict"] for entry in verdicts)
        context["functions"] = verdicts
        (bundle / "functions.json").write_text(json.dumps(verdicts, indent=1))
        summary = summarize(verdicts)
        if digest(native_api) != context["native_api_sha256"] or digest(executable) != context["original_exe_sha256"] or not candidate_source_matches(context["source"]):
            verdict = result("stale", "Candidate changed during G4 validation.", bundle=str(bundle))
        elif counts["passing"] == len(verdicts):
            verdict = result("passed", "Every physics native and physics port function passed its fresh differential test.", bundle=str(bundle), counts=dict(counts))
        else:
            verdict = result("partial", f"{counts['passing']} of {len(verdicts)} physics functions pass; "
                             + ", ".join(f"{counts[key]} {key}" for key in ("blocked_by_g11", "no_test", "unimplemented") if counts[key]),
                             bundle=str(bundle), counts=dict(counts), by_class=summary,
                             invalid_runs=context.get("invalid_runs", {}))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        verdict = result("failed", str(error), bundle=str(bundle))
    finally:
        context["verdict"] = verdict
        (bundle / "receipt.json").write_text(json.dumps(context, indent=2) + "\n")
    return verdict
