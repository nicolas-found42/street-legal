"""G6/S5 validator: D3D9-equivalent call streams against the hooked original.

Live execution only (`--execute G6` / `--execute S5`). It builds the stream tool
and the port in an isolated target, freshly captures the headless original's
D3D9 stream for each registered scene, freshly runs the port with its translator
stream enabled, canonicalizes both with `slr-d3d9-stream` and compares them
frame by frame. A receipt, an old capture or an equal prefix cannot pass.

Honest limits are part of the result, not footnotes:
* The original's frames come from the declared headless platform scenario. Its
  D3D9 records carry call arguments only (no vertex/index/texture payloads,
  shader constants or UP-draw vertex data), so equality of two streams is
  equality of call/state/draw structure, not of pixels or buffer contents.
* No screenshot of the original can be produced headlessly, and human review
  receipts are never fabricated: G6 stays blocked until reviewed screenshots
  bound to this run's port image exist.
"""
from __future__ import annotations

import datetime
import hashlib
import json
import shutil
import subprocess
import sys
import uuid
from pathlib import Path

from validators.core import boot_tools

ROOT = Path(__file__).resolve().parents[1]
CANON_SCHEMA = "slr-d3d9-canon-1"
ORACLE_PYTHON = Path.home() / "slr-oracle-venv/bin/python"
REVIEW_ROOT = ROOT / "re/out/render-reviews"

# Canonical whole-stream digest of the 1.8G-instruction menu scenario. It is a
# hash of derived frame hashes (no original data) and identical for the archived
# and a fresh capture; a different value means the oracle or canonicalizer moved.
MENU_IDLE_1800M_STREAM_SHA256 = "d8663b4f7f1e0d52ea7ac877ed2cf6ed69516a7c3cc030d5518206ebc0619ae3"

SCENES = {
    "menu-idle": {
        "obligations": ["P01"],
        "oracle_scenario": "menu-idle-1800M",
        "oracle_stream_sha256": MENU_IDLE_1800M_STREAM_SHA256,
        # Last presented frame of the scenario vs the port's menu snapshot frame.
        "oracle_frame": 349,
        "port_frame": 0,
        "port_args": ["stock", "--menu-frame"],
        "alignment": "oracle frame 349 (the last presented frame of the 1.8G-instruction scenario) against "
        "the port's first frame; the pairing is an assumption of this scene definition",
    },
}
BLOCKED_SCENES = {
    "garage": "The port renders the garage (New Career path, Tier D presentation) but no scripted-input path "
    "drives the headless original past the main-menu prefix, so there is no original garage stream to compare.",
    "race": "No original race stream: the headless original does not reach gameplay and the port cannot drive.",
    "free-ride": "Same as race.",
    "day-night-sky-fog": "Needs gameplay worlds on both sides (P23).",
    "cameras-hud": "Needs gameplay cameras on both sides (P27).",
}


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def result(outcome: str, reason: str, **details) -> dict:
    if "result" in details:
        raise ValueError("details cannot replace the checked validator result")
    return {"result": outcome, "reason": reason, **details}


# ---- pure verdict logic (exercised by the data-free self-tests) -------------------------


def scene_verdict(name: str, report: dict, oracle_digest: str, expected_digest: str) -> dict:
    """Classify one scene comparison. Anything not provably equal is not equal."""
    if report.get("schema") != CANON_SCHEMA or report.get("kind") != "comparison":
        return {"scene": name, "status": "invalid", "reason": "comparison report has an unknown schema"}
    if oracle_digest != expected_digest:
        return {"scene": name, "status": "invalid",
                "reason": "fresh original stream differs from the pinned scenario digest (oracle or canonicalizer changed)"}
    compared = report.get("frames_compared")
    if not isinstance(compared, int) or compared < 1:
        return {"scene": name, "status": "invalid", "reason": "no frames were compared"}
    if report.get("equal") is True and report.get("frames_equal") == compared:
        return {"scene": name, "status": "equal", "frames_compared": compared}
    first = next((f for f in report.get("frames", []) if f.get("status") != "equal"), None)
    divergence = (first or {}).get("first_divergence") or {}
    return {
        "scene": name, "status": "diverged", "frames_compared": compared,
        "frames_equal": report.get("frames_equal"),
        "first_divergence": {
            "pair": (first or {}).get("pair"),
            "event_index": divergence.get("event_index"),
            "expected_event": divergence.get("expected_event"),
            "got_event": divergence.get("got_event"),
            "differences": (divergence.get("differences") or [])[:6],
            "length_mismatch": divergence.get("length_mismatch"),
        },
    }


def review_receipt_problem(receipt: object, scene: str, port_png_sha256: str) -> str | None:
    """Why a human screenshot-review receipt cannot count, or None if it can.

    A receipt only counts if it names a reviewer, the scene, the exact port image
    of this run and a reference screenshot from the original. Nothing here can
    prove the reviewer is human; that is the reviewer's attestation.
    """
    if not isinstance(receipt, dict) or receipt.get("schema") != "slr-render-review-1":
        return "not a slr-render-review-1 receipt"
    if receipt.get("scene") != scene:
        return "receipt names a different scene"
    if not isinstance(receipt.get("reviewer"), str) or not receipt["reviewer"].strip():
        return "receipt has no reviewer"
    if receipt.get("port_png_sha256") != port_png_sha256:
        return "receipt does not review this run's port image"
    reference = receipt.get("original_reference")
    if not isinstance(reference, dict) or not reference.get("sha256") or not reference.get("provenance"):
        return "receipt lacks an original-screenshot reference with provenance"
    if receipt.get("verdict") not in ("match", "accepted-with-notes"):
        return "receipt verdict is not a match"
    return None


def review_status(scene: str, port_png_sha256: str | None, review_root: Path = REVIEW_ROOT) -> dict:
    path = review_root / f"{scene}.json"
    if port_png_sha256 is None:
        return {"scene": scene, "status": "no_port_image"}
    if not path.is_file():
        return {"scene": scene, "status": "missing", "expected_path": str(path)}
    try:
        problem = review_receipt_problem(json.loads(path.read_text()), scene, port_png_sha256)
    except (OSError, ValueError) as error:
        problem = str(error)
    return {"scene": scene, "status": "invalid" if problem else "present", **({"problem": problem} if problem else {})}


def combine(verdicts: list[dict], reviews: list[dict], harness: dict) -> dict:
    """Map scene verdicts, reviews and harness checks to the G6 and S5 results.

    G6 passes only when every registered scene's stream is equal AND every scene
    has a valid review receipt AND no required scene is blocked.
    S5 (harness) is at best partial while screenshots of the original cannot be
    produced; it fails if the stream half is not verified.
    """
    details = {"scenes": verdicts, "reviews": reviews, "blocked_scenes": BLOCKED_SCENES, "harness": harness}
    stream_ok = harness.get("ok") is True
    if not stream_ok:
        failed = result("failed", "Stream harness verification failed: " + str(harness.get("problem")), **details)
        return {"G6": failed, "S5": dict(failed)}
    invalid = [v for v in verdicts if v["status"] == "invalid"]
    diverged = [v for v in verdicts if v["status"] == "diverged"]
    s5 = result(
        "partial",
        "Call-stream half of the harness verified (fresh original capture reproduces its pinned stream, port stream "
        "emitted, canonical comparison reports first divergences). The screenshot half against the original is "
        "unavailable: the headless original renders no pixels and no reviewed reference exists.",
        **details,
    )
    if invalid:
        return {"G6": result("failed", "Invalid scene evidence: " + "; ".join(v["reason"] for v in invalid), **details),
                "S5": result("failed", "Invalid scene evidence: " + "; ".join(v["reason"] for v in invalid), **details)}
    reasons = []
    if diverged:
        reasons.append("streams diverge in " + ", ".join(v["scene"] for v in diverged))
    reasons.append("blocked scenes without an original stream: " + ", ".join(BLOCKED_SCENES))
    unreviewed = [r["scene"] for r in reviews if r["status"] != "present"]
    if unreviewed:
        reasons.append("no valid human screenshot review for " + ", ".join(unreviewed))
    all_equal = verdicts and all(v["status"] == "equal" for v in verdicts)
    if all_equal and not BLOCKED_SCENES and not unreviewed:
        g6 = result("passed", "Every registered scene's D3D9 stream equals the hooked original's and has a valid review.", **details)
    else:
        g6 = result("failed" if diverged else "blocked", "; ".join(reasons), **details)
    return {"G6": g6, "S5": s5}


# ---- live execution --------------------------------------------------------------------


def run(command, *, env=None, cwd=ROOT, log: Path | None = None, check=True) -> subprocess.CompletedProcess:
    if log is None:
        process = subprocess.run(command, cwd=cwd, env=env, capture_output=True, text=True)
    else:
        with log.with_suffix(".stdout").open("w") as out, log.with_suffix(".stderr").open("w") as err:
            process = subprocess.run(command, cwd=cwd, env=env, stdout=out, stderr=err)
    if check and process.returncode:
        raise ValueError(f"{Path(command[0]).name} exited {process.returncode}: {' '.join(map(str, command[:6]))}")
    return process


def git_identity() -> dict:
    return boot_tools().git_identity(ROOT)[0]


def canon_digest(tool: Path, log: Path, out: Path) -> dict:
    run([str(tool), "canon", str(log), "--out", str(out)])
    end = json.loads(out.read_text().splitlines()[-1])
    if end.get("end") is not True:
        raise ValueError("canonical stream is truncated")
    return end


def run_render(output_root: Path, *, evidence_root: Path | None = None, session: str | None = None,
               stock_check=None) -> dict:
    bundle = output_root / ("render-" + uuid.uuid4().hex[:12])
    bundle.mkdir(parents=True)
    manifest = {"schema": "slr-render-g6s5-1", "status": "running", "source": git_identity(), "session": session,
                "started_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(), "scenes": {}}
    harness = {"ok": False, "problem": "validation did not complete"}
    verdicts, reviews = [], []
    try:
        if not ORACLE_PYTHON.is_file():
            raise ValueError(f"oracle Python environment missing: {ORACLE_PYTHON}")
        if stock_check is not None:
            manifest["stock"] = stock_check()["manifest_sha256"]
        target = bundle / "target"
        run(["cargo", "build", "--release", "--locked", "-p", "slr-d3d9", "-p", "slr-boot", "--target-dir", str(target)],
            log=bundle / "build")
        tool, boot = target / "release/slr-d3d9-stream", target / "release/slr-boot"
        manifest["tools"] = {"stream_tool_sha256": sha256_file(tool), "slr_boot_sha256": sha256_file(boot)}
        for name, scene in SCENES.items():
            sdir = bundle / name
            sdir.mkdir()
            oracle_dir = ROOT / "re/out/headless/d3d9-stream" / f"{bundle.name}-{name}"
            capture = [str(ORACLE_PYTHON), str(ROOT / "re/headless/d3d9_capture.py"), scene["oracle_scenario"],
                       "--out", str(oracle_dir), "--session", session or "acceptance"]
            run(capture, log=sdir / "oracle-capture")
            shutil.copytree(oracle_dir, sdir / "oracle")
            oracle_log = sdir / "oracle/run.slrlog"
            oracle_canon = canon_digest(tool, oracle_log, sdir / "oracle-canon.jsonl")
            inventory = sdir / "inventory.json"
            run([str(tool), "inventory", str(oracle_log), "--out", str(inventory)])
            port_runs = []
            for attempt in (1, 2):
                pdir = sdir / f"port-{attempt}"
                pdir.mkdir()
                env = {k: v for k, v in __import__("os").environ.items() if not k.startswith("SLR_")}
                env["SLR_D3D9_STREAM"] = str(pdir / "port.slrlog")
                png = pdir / "port.png"
                run([str(boot), scene["port_args"][0], scene["port_args"][1], str(png),
                     "--output", str(pdir / "runtime"), "--run-id", f"render-g6s5-{attempt}"], env=env, log=pdir / "port")
                port_runs.append({"log": pdir / "port.slrlog", "png": png})
            digests = [canon_digest(tool, p["log"], sdir / f"port-{i}-canon.jsonl")["stream_sha256"]
                       for i, p in enumerate(port_runs, 1)]
            if digests[0] != digests[1]:
                raise ValueError(f"port stream is not reproducible in {name}")
            comparison = sdir / "comparison.json"
            run([str(tool), "compare", str(oracle_log), str(port_runs[0]["log"]), "--skip-expected",
                 str(scene["oracle_frame"]), "--skip-got", str(scene["port_frame"]), "--max-frames", "1",
                 "--out", str(comparison)], check=False)
            report = json.loads(comparison.read_text())
            verdict = scene_verdict(name, report, oracle_canon["stream_sha256"], scene["oracle_stream_sha256"])
            png_sha = sha256_file(port_runs[0]["png"])
            verdict["port_png_sha256"] = png_sha
            verdicts.append(verdict)
            reviews.append(review_status(name, png_sha))
            manifest["scenes"][name] = {"alignment": scene["alignment"], "oracle_stream_sha256": oracle_canon["stream_sha256"],
                                        "port_stream_sha256": digests[0], "port_png_sha256": png_sha,
                                        "oracle_manifest": json.loads((sdir / "oracle/manifest.json").read_text())}
        harness = {"ok": True}
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        harness = {"ok": False, "problem": str(error)}
    outcome = combine(verdicts, reviews, harness)
    if git_identity() != manifest["source"]:
        outcome = {rid: result("stale", "Candidate source changed during render validation.") for rid in ("G6", "S5")}
    manifest.update(status="completed", harness=harness, verdicts=verdicts, reviews=reviews,
                    results={k: {"result": v["result"], "reason": v["reason"]} for k, v in outcome.items()})
    (bundle / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    for value in outcome.values():
        value["bundle"] = str(bundle)
    if evidence_root is not None:
        stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        target_dir = evidence_root.expanduser() / f"render-g6s5-{stamp}"
        shutil.copytree(bundle, target_dir, ignore=shutil.ignore_patterns("target"))
        for value in outcome.values():
            value["evidence_archive"] = str(target_dir)
    return outcome


def not_run(which: str) -> dict:
    return result("not_run", f"Run --execute {which} for a fresh original capture, port run and canonical comparison; receipts cannot assert pass.")


def main(argv: list[str]) -> int:
    """Standalone entry (also usable where check.py's symlink-refusing inventory load cannot run,
    such as a worktree linked to local inputs). Exit 0 only if both G6 and S5 passed."""
    import argparse

    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--output-root", type=Path, default=ROOT / "re/out/acceptance")
    parser.add_argument("--evidence-root", type=Path)
    parser.add_argument("--session")
    parser.add_argument("--check-stock", action="store_true", help="verify the stock install manifest first (slow)")
    args = parser.parse_args(argv)
    stock_check = None
    if args.check_stock:
        from validators import stock_identity

        stock_check = lambda: stock_identity(ROOT / "stock")  # noqa: E731
    outcome = run_render(args.output_root.resolve(), evidence_root=args.evidence_root, session=args.session,
                         stock_check=stock_check)
    print(json.dumps(outcome, indent=2))
    return 0 if all(v["result"] == "passed" for v in outcome.values()) else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
