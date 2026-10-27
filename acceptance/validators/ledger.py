"""S11 helpers: native registration fixtures, fixture freezing and isolated
ledger collection/regeneration."""
from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import sys
import uuid
from pathlib import Path

from . import core

def native_registration_rows(text: str) -> list[list[str]]:
    rows = [line.split("\t") for line in text.splitlines()]
    if len(rows) != 353 or any(len(row) != 4 for row in rows):
        raise ValueError("native registration fixture must contain exactly 353 complete rows")
    if len({tuple(row[:3]) for row in rows}) != 353:
        raise ValueError("native registration fixture has duplicate signatures")
    if any(not re.fullmatch(r"(?:FUN|LAB)_[0-9a-fA-F]{8}", row[3]) for row in rows):
        raise ValueError("native registration fixture has unresolved original addresses")
    return rows


def fixture_sources(root: Path, excluded_roots=()) -> list[Path]:
    """Frozen inputs made available to the isolated ledger collector.

    Whole-game run products and the explicitly named media/research archives
    below are excluded. Boot's root vector files and the snapshot corpus are
    included. Unknown directories remain included, including hidden inputs;
    do not replace this with an extension allowlist or a research-name glob.
    A test requiring anything else must fail or emit the skip marker, which
    the observing Cargo wrapper rejects.
    """
    native_registration_rows((root / "re/out/native_api.tsv").read_text())
    output = []
    excluded_roots = [path.resolve() for path in excluded_roots]

    def keep(path):
        return not any(path.resolve().is_relative_to(excluded) for excluded in excluded_roots)

    def files_under(directory):
        for base, directories, files in os.walk(directory):
            base = Path(base)
            directories[:] = [name for name in directories if keep(base / name)]
            yield from (base / name for name in files if keep(base / name) and (base / name).is_file())

    excluded = {
        "checkpoints", "handoff-2026-10-20", "headless",
        # Video downloads, an isolated approximation capture/build, and static
        # critique research; none is an input to the production test collector.
        # Keep exclusions exact so new oracle fixtures are retained by default.
        "gameplay-comparison-2026-10-22",
        "implementation-showcase-2026-10-22",
        "critique-acceptance-2026-10-22",
    }
    for child in (root / "re/out").iterdir():
        if child.name in excluded or child.name == "test-results.json" or not keep(child):
            continue
        if child.name == "boot":
            output.extend(p for p in child.iterdir() if p.is_file() and p.suffix in {".json", ".jsonl", ".bin", ".slrlog", ".ckpt", ".pages"})
        elif child.is_dir():
            output.extend(files_under(child))
        elif child.suffix in {".json", ".jsonl", ".tsv"}:
            output.append(child)
    # Narrow original captures used by registered library/integration tests;
    # do not copy hundreds of GiB of unrelated headless run products.
    headless = root / "re/out/headless"
    clock = headless / "clock-investigation/menu1800M"
    output.extend(p for p in clock.iterdir() if p.is_file() and p.suffix in {".json", ".jsonl"})
    for relative in (
        "hypothetical-cd-toc.json",
        "boot/graphics-profile.json",
        "window-transcript-native/hypothetical-seven-notifications-declared-handle-profile.json",
        "boot/platform-profile.json",
        "clock-investigation/menu1800M/observed/original-facts.json",
        "finalizer-investigation/gc-probe.json",
        "finalizer-investigation/string-add.json",
        "finalizer-investigation/long-observed/original-facts.json",
        "finalizer-investigation/callers-observed/original-facts.json",
        "finalizer-investigation/frame-long-observed/frame-timer-inputs.jsonl",
        "finalizer-investigation/frame-long-observed/original-facts.json",
        "oracle-33d8aa8/observed/script-natives.slrlog",
        "package-investigation/full-observed/original-facts.json",
        "allocation-history/long-observed/original-facts.json",
    ):
        path = headless / relative
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"required original capture missing or empty: {relative}")
        output.append(path)
    return sorted({path for path in output if keep(path)})


def ledger_result(output_root: Path) -> dict:
    """Run collection/regeneration against retained source and frozen fixtures.

    Live execution only: there is no load-passed-receipt entry point.
    """
    tool = core.boot_tools()
    bundle = output_root / ("ledger-" + uuid.uuid4().hex[:12])
    bundle.mkdir(parents=True)
    candidate = bundle / "candidate"
    candidate.mkdir()
    context = {"schema": 1, "obligation": "S11", "commands": []}
    verdict = core.result("failed", "Ledger validation did not complete.", bundle=str(bundle))
    try:
        context["source"], patch = tool.git_identity(core.ROOT)
        (bundle / "source.diff").write_bytes(patch)
        context["source_untracked"] = tool.archive_untracked_sources(core.ROOT, bundle, context["source"])
        names = subprocess.check_output(["git", "-C", str(core.ROOT), "ls-files", "--cached", "--others", "--exclude-standard", "-z"]).split(b"\0")
        for name in names:
            if not name:
                continue
            path = core.ROOT / os.fsdecode(name)
            if not path.exists():  # retained in the binary source diff
                continue
            if path.is_symlink() or not path.is_file():
                raise ValueError(f"source is not an ordinary file: {path}")
            destination = candidate / path.relative_to(core.ROOT)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, destination)
        if not core.candidate_source_matches(context["source"]):
            raise ValueError("source changed while preserving candidate")
        context["stock"] = core.stock_identity(core.ROOT / "stock")
        context["compatibility"] = tool.tree_identity(core.ROOT / "reference/raw")
        context["fixtures"] = []
        for path in fixture_sources(core.ROOT, excluded_roots=(output_root,)):
            identity = tool.file_identity(path)
            relative = path.relative_to(core.ROOT)
            destination = candidate / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, destination)
            if core.digest(destination) != identity["sha256"]:
                raise ValueError(f"fixture changed while freezing: {relative}")
            context["fixtures"].append({**identity, "snapshot": relative.as_posix()})
        (candidate / "stock").symlink_to(core.ROOT / "stock", target_is_directory=True)
        (candidate / "reference").symlink_to(core.ROOT / "reference", target_is_directory=True)
        (bundle / "commands").mkdir()
        (bundle / "bin").mkdir()
        cargo = shutil.which("cargo")
        if not cargo:
            raise ValueError("Cargo unavailable")
        # Every child cargo test is observed, including output collect hides.
        wrapper = bundle / "bin/cargo"
        wrapper.write_text(f'''#!{sys.executable}
import json, pathlib, subprocess, sys, uuid
sys.path.insert(0, {str(candidate / "acceptance")!r})
from observations import rust_test_observation
command = [{cargo!r}, *sys.argv[1:]]
r = subprocess.run(command, capture_output=True, text=True)
base = pathlib.Path({str(bundle / "commands")!r}) / uuid.uuid4().hex
base.with_suffix('.stdout').write_text(r.stdout)
base.with_suffix('.stderr').write_text(r.stderr)
observation = rust_test_observation(r.returncode, r.stdout, r.stderr) if 'test' in sys.argv[1:] else None
base.with_suffix('.json').write_text(json.dumps({{'command': command, 'returncode': r.returncode, 'observation': observation}}, indent=2))
sys.stdout.write(r.stdout); sys.stderr.write(r.stderr)
sys.exit(r.returncode if observation is None or observation['valid_component_observation'] else 2)
''')
        wrapper.chmod(0o755)
        environment = {key: value for key, value in os.environ.items() if not key.startswith("SLR_")}
        environment.update({"PATH": str(bundle / "bin") + os.pathsep + os.environ.get("PATH", ""), "CARGO_TARGET_DIR": str(bundle / "target"), "SLR_STOCK": str(core.ROOT / "stock"), "SLR_REFERENCE": str(core.ROOT / "reference/raw"), "SLR_REQUIRE_ORACLE_FIXTURES": "1"})
        context["configuration"] = {key: environment[key] for key in ("CARGO_TARGET_DIR", "SLR_STOCK", "SLR_REFERENCE", "RUSTFLAGS", "RUSTUP_TOOLCHAIN") if key in environment}
        context["tools"] = {"python": sys.version, "cargo": subprocess.check_output([cargo, "--version"], text=True).strip(), "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip()}
        context["status"] = "running"
        (bundle / "receipt.json").write_text(json.dumps(context, indent=2) + "\n")
        def execute(label, args):
            command = [cargo, "run", "--locked", "-p", "slr-ledger", "--", *args]
            with (bundle / (label + ".stdout")).open("w") as stdout, (bundle / (label + ".stderr")).open("w") as stderr:
                process = subprocess.run(command, cwd=candidate, env=environment, stdout=stdout, stderr=stderr)
            context["commands"].append({"command": command, "cwd": str(candidate), "returncode": process.returncode, "stdout": label + ".stdout", "stderr": label + ".stderr"})
            if process.returncode:
                raise ValueError(f"{label} exited {process.returncode}; see retained outputs")
        expected_doc = (candidate / "docs/fidelity.md").read_bytes()
        execute("collect", ["collect"])
        observations = [json.loads(path.read_text()) for path in (bundle / "commands").glob("*.json")]
        if not observations or any(not row.get("observation", {}).get("valid_component_observation") for row in observations):
            raise ValueError("fresh collector did not produce valid observed test runs")
        rows = json.loads((candidate / "re/out/test-results.json").read_text())
        if not rows or any(row.get("status") != "pass" or not row.get("test") or not row.get("oracle") for row in rows):
            raise ValueError("fresh collection contains missing or failing evidence rows")
        context["evidence_rows"] = len(rows)
        execute("regen", ["regen"])
        generated = (candidate / "docs/fidelity.md").read_bytes()
        execute("regen-idempotence", ["regen"])
        if generated != (candidate / "docs/fidelity.md").read_bytes():
            raise ValueError("ledger regeneration is not idempotent")
        if generated != expected_doc:
            raise ValueError("freshly regenerated ledger differs from candidate fidelity.md")
        if context["stock"] != core.stock_identity(core.ROOT / "stock") or context["compatibility"] != tool.tree_identity(core.ROOT / "reference/raw"):
            raise ValueError("stock or compatibility corpus changed during validation")
        for identity in context["fixtures"]:
            if core.digest(Path(identity["path"])) != identity["sha256"]:
                raise ValueError(f"oracle fixture changed during validation: {identity['path']}")
        if not core.candidate_source_matches(context["source"]):
            verdict = core.result("stale", "Candidate source changed during isolated S11 verification.", bundle=str(bundle))
        else:
            verdict = core.result("passed", "Fresh observed collector, current-source ledger equality and idempotent regeneration verified.", bundle=str(bundle), evidence_rows=len(rows))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        verdict = core.result("failed", str(error), bundle=str(bundle))
    finally:
        context["status"] = "completed"
        context["verdict"] = verdict
        (bundle / "receipt.json").write_text(json.dumps(context, indent=2) + "\n")
    return verdict
