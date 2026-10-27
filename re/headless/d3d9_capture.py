#!/usr/bin/env python3
"""Capture the headless original's D3D9 call stream for a named scripted scenario.

The headless oracle (`startup.py`, the unmodified build-606 exe under Unicorn with
the declared platform scenario) already logs every D3D9 COM call it makes as
SLRLOG kind-0x01 records (`d3d9_device.py` supplies the declared device). This
wrapper runs one registered scenario, retains the log with a provenance manifest
(commit, branch, worktree, exe/profile/input hashes, command line, timings) and
optionally copies the bundle into the evidence archive. It never edits the
oracle, never commits captures, and writes only below this worktree's
`re/out/headless/` (startup.py enforces that) plus the named evidence root.

    ~/slr-oracle-venv/bin/python re/headless/d3d9_capture.py menu-idle-1800M \
        --out re/out/headless/d3d9-stream/<run> [--evidence-root ~/Documents/street-legal-evidence]

Scenarios are fixed instruction budgets of the same declared startup profile the
boot-to-MainMenu work uses; none is a Windows observation, and the original's
frames here are those of the declared platform scenario, not a real GPU.
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import importlib.util
import json
import os
import platform
import shutil
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
OUTPUT_ROOT = REPO / "re/out/headless"
ORIGINAL_EXE_SHA256 = "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
MANIFEST_SCHEMA = "slr-d3d9-capture-1"

# name -> instruction budget. The 1.8G budget is the scenario whose archived log
# (re/out/headless/clock-investigation/menu1800M/run.slrlog) the port work uses.
SCENARIOS = {
    "menu-idle-1800M": {"limit": 1_800_000_000, "event_limit": 100_000, "event_ring": 5000},
    "menu-idle-600M": {"limit": 600_000_000, "event_limit": 100_000, "event_ring": 5000},
}
INPUTS = {
    "media_profile": "re/out/headless/hypothetical-cd-toc.json",
    "desktop_profile": "re/out/headless/window-transcript-native/hypothetical-seven-notifications-declared-handle-profile.json",
}


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def git(*args: str) -> str:
    return subprocess.run(["git", "-C", str(REPO), *args], capture_output=True, text=True, check=True).stdout.strip()


def source_identity() -> dict:
    spec = importlib.util.spec_from_file_location("render_capture_diagnostics", REPO / "re/boot-diagnose.py")
    diagnostics = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(diagnostics)
    return diagnostics.git_identity(REPO)[0]


def build_command(scenario: dict, exe: Path, platform_profile: Path, out: Path, python: str) -> list[str]:
    graphics_profile = platform_profile.with_name("graphics-profile.json")
    rel = lambda p: str(Path(p))
    return [
        python, str(REPO / "re/headless/startup.py"),
        "--exe", rel(exe),
        "--media-profile", INPUTS["media_profile"],
        "--graphics-profile", rel(graphics_profile),
        "--desktop-profile", INPUTS["desktop_profile"],
        "--platform-profile", rel(platform_profile),
        "--calllog", rel(out / "run.slrlog"),
        "--out", rel(out / "run.json"),
        "--limit", str(scenario["limit"]),
        "--event-limit", str(scenario["event_limit"]),
        "--event-ring", str(scenario["event_ring"]),
        "--fast-count",
    ]


def capture(name: str, out: Path, *, exe: Path | None = None, python: str | None = None, session: str | None = None) -> dict:
    if name not in SCENARIOS:
        raise ValueError(f"unknown scenario {name!r}; known: {', '.join(sorted(SCENARIOS))}")
    scenario = SCENARIOS[name]
    out = out.resolve()
    if not out.is_relative_to(REPO.resolve() / "re/out/headless"):
        raise ValueError("capture output must be inside this worktree's re/out/headless/")
    exe = (exe or REPO / "reference/raw/StreetLegal_Redline.bak.exe").resolve()
    if sha256(exe) != ORIGINAL_EXE_SHA256:
        raise ValueError("original executable identity mismatch")
    for key, relative in INPUTS.items():
        if not (REPO / relative).is_file():
            raise ValueError(f"declared platform input missing: {key} ({relative})")
    if out.exists() and any(out.iterdir()):
        raise ValueError(f"refusing to reuse a non-empty capture directory: {out}")
    out.mkdir(parents=True, exist_ok=True)
    python = python or sys.executable
    platform_profile = out / "platform-profile.json"
    subprocess.run([python, str(REPO / "re/headless/make_boot_profile.py"), str(platform_profile)],
                   cwd=REPO, check=True, stdout=subprocess.DEVNULL)
    command = build_command(scenario, exe, platform_profile.relative_to(REPO), out.relative_to(REPO), python)
    source_before = source_identity()
    inputs_before = {key: sha256(REPO / relative) for key, relative in INPUTS.items()}
    started = datetime.datetime.now(datetime.timezone.utc)
    t0 = time.monotonic()
    with (out / "stdout.txt").open("w") as stdout, (out / "stderr.txt").open("w") as stderr:
        process = subprocess.run(command, cwd=REPO, stdout=stdout, stderr=stderr)
    elapsed = time.monotonic() - t0
    manifest = {
        "schema": MANIFEST_SCHEMA,
        "scenario": name,
        "scenario_parameters": scenario,
        "status": "completed" if process.returncode == 0 else "failed",
        "returncode": process.returncode,
        "started_utc": started.isoformat(),
        "elapsed_seconds": round(elapsed, 1),
        "command": command,
        "source": source_before,
        "source_after": source_identity(),
        "session": session,
        "host": {"platform": platform.platform(), "python": sys.version.split()[0]},
        "original_exe_sha256": ORIGINAL_EXE_SHA256,
        "declared_inputs": {key: {"path": rel, "sha256": sha256(REPO / rel)} for key, rel in INPUTS.items()},
        "generated_inputs": {
            name: {"path": name + ".json", "sha256": sha256(out / (name + ".json")),
                   "generator": "re/headless/make_boot_profile.py"}
            for name in ("platform-profile", "graphics-profile")
        },
        "caveats": [
            "Frames are the declared headless platform scenario, not a Windows or GPU observation.",
            "D3D9 records carry call arguments only: vertex/index/texture payloads, shader constants and "
            "UP-draw vertex data are not in the log.",
        ],
    }
    if process.returncode == 0:
        run = json.loads((out / "run.json").read_text())
        stop = run.get("stop", {})
        if run.get("exe_sha256") != ORIGINAL_EXE_SHA256 or run.get("oracle") != "unicorn":
            raise ValueError("oracle run is not the pinned original under Unicorn")
        if stop.get("kind") != "instruction_limit" or run.get("instructions", 0) < scenario["limit"]:
            raise ValueError(f"oracle stopped early: {stop}")
        manifest["oracle_run"] = {"instructions": run["instructions"], "stop": stop,
                                  "d3d_device": run.get("d3d_device"), "main_menu_reached": run.get("main_menu_reached"),
                                  "status": run.get("status")}
        manifest["artifacts"] = {n: {"bytes": (out / n).stat().st_size, "sha256": sha256(out / n)}
                                 for n in ("run.slrlog", "run.json", "stdout.txt", "stderr.txt")}
    moved = manifest["source_after"] != source_before or sha256(exe) != ORIGINAL_EXE_SHA256 or any(sha256(REPO / relative) != inputs_before[key] for key, relative in INPUTS.items())
    if moved:
        manifest.update(status="stale", problem="candidate source or declared inputs changed during capture")
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    if process.returncode != 0:
        raise RuntimeError(f"oracle exited {process.returncode}; see {out}/stderr.txt")
    if moved:
        raise ValueError("candidate source or declared inputs changed during capture")
    return manifest


def archive(out: Path, evidence_root: Path, tag: str) -> Path:
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    target = evidence_root.expanduser() / f"{tag}-{stamp}"
    target.mkdir(parents=True, exist_ok=False)
    for entry in out.iterdir():
        if entry.is_file():
            shutil.copy2(entry, target / entry.name)
    return target


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("scenario", choices=sorted(SCENARIOS))
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--evidence-root", type=Path)
    parser.add_argument("--session", help="session identity recorded in the manifest")
    args = parser.parse_args(argv)
    manifest = capture(args.scenario, args.out, session=args.session)
    print(json.dumps({"status": manifest["status"], "out": str(args.out), "log_sha256": manifest["artifacts"]["run.slrlog"]["sha256"]}))
    if args.evidence_root:
        print(archive(args.out, args.evidence_root, f"render-g6s5-oracle-{args.scenario}"))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
