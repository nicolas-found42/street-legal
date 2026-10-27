"""Whole-install format census and live loader observations; gaps stay partial."""
from __future__ import annotations
import json, os, subprocess, uuid
from pathlib import Path
from validators.core import ROOT, boot_tools, digest, result, stock_identity

# --- G2: formats ----------------------------------------------------------

G2_GOLDEN = ROOT / "acceptance/g2-census-stock.json"
# Exe loaders with function differential evidence: (package, test binary, test
# names, vector environment variables resolved against the repository root).
# A format outside this table and outside G2_NO_EXE_LOADER has no differential
# evidence and keeps G2 partial.
G2_DIFFERENTIAL = {
    "ptx": ("slr-fmt-texture", "ptx_oracle", ["ptx_loader_matches_oracle_vectors"], {"SLR_PTX_VECTORS": "re/out/ptx"}),
    "bon/skl/matrixdata": ("slr-fmt-skeleton", "oracle", ["skeleton_reader_matches_oracle", "animation_reader_matches_oracle", "precision_control_is_observable"], {"SLR_SKEL_VECTORS": "re/out/skel"}),
    "sdat": ("slr-fmt-sdat", "sdat_oracle", ["file_natives_match_oracle_vectors", "slot_run_store_matches_oracle_vectors", "file_close_matches_oracle_vectors"], {"SLR_SDAT_VECTORS": "re/out/sdat"}),
}
# Standard media and bundled documents have no exe-side decoder to compare
# against (ADR 0009: decoded output is tier D); their evidence is the census.
G2_NO_EXE_LOADER = {
    "dds": "D3DX; decoded pixels are tier D", "png": "static libpng; tier D", "tga": "tier D",
    "avi": "WINMM/Cinepak codec; tier D",
}
# Formats whose exe loader has not yet been isolated or compared. Each must
# disappear from this list before G2 can pass; none is a documented exception.
G2_LOADER_GAPS = (
    "rpk", "scx/scy", "class", "cfg", "trc", "walk/rest", "spl", "scm", "wav", "mp3",
)


def census_run(binary: Path, root: Path) -> dict:
    """Run the census binary over one install root and return its JSON report."""
    process = subprocess.run([str(binary), str(root)], capture_output=True, text=True)
    if process.returncode not in (0, 1):
        raise ValueError(f"census exited {process.returncode}: {process.stderr.strip()[:400]}")
    return json.loads(process.stdout)


def census_difference(golden: dict, actual: dict) -> list[str]:
    """Exact census comparison. Counts are never tolerance-compared."""
    problems = []
    if golden.get("schema") != 1 or actual.get("schema") != 1:
        problems.append("census schema is not 1")
    if actual.get("unexplained"):
        problems.append(f"{len(actual['unexplained'])} files are neither decoded nor explained")
    for key in ("files", "bytes"):
        if golden.get(key) != actual.get(key):
            problems.append(f"{key}: golden {golden.get(key)} != actual {actual.get(key)}")
    covered = sum(entry["files"] for entry in actual.get("decoders", {}).values())
    if covered != actual.get("files") or covered != golden.get("files"):
        problems.append(f"decoder files {covered} do not account for every file")
    for name in sorted(set(golden.get("decoders", {})) | set(actual.get("decoders", {}))):
        if golden.get("decoders", {}).get(name) != actual.get("decoders", {}).get(name):
            problems.append(f"decoder {name}: golden {golden.get('decoders', {}).get(name)} != actual {actual.get('decoders', {}).get(name)}")
    return problems


def formats_result(output_root: Path) -> dict:
    """G2: census over every stock file, differential loader tests, writer round trips.

    Passes only when the census equals the checked-in stock counts with zero
    unexplained files, every registered differential test executes (no skipped
    oracle fixture) and no loader gap remains. Anything less is reported as
    partial with the exact gaps; nothing here consults a receipt.
    """
    from observations import rust_test_observation
    bundle = output_root / ("formats-" + uuid.uuid4().hex[:12])
    bundle.mkdir(parents=True)
    context = {"schema": 1, "obligation": "G2", "commands": []}
    verdict = result("failed", "Formats validation did not complete.", bundle=str(bundle))
    try:
        tool = boot_tools()
        context["source"], patch = tool.git_identity(ROOT)
        (bundle / "source.diff").write_bytes(patch)
        context["stock"] = {key: value for key, value in stock_identity(ROOT / "stock").items() if key != "files"}
        golden = json.loads(G2_GOLDEN.read_text())
        environment = {key: value for key, value in os.environ.items() if not key.startswith("SLR_")}
        environment["CARGO_TARGET_DIR"] = str(bundle / "target")
        environment["SLR_STOCK"] = str(ROOT / "stock")
        environment["SLR_REQUIRE_ORACLE_FIXTURES"] = "1"
        def cargo(label, args, extra=None, tests=False, tolerate=()):
            stdout, stderr = bundle / (label + ".stdout"), bundle / (label + ".stderr")
            with stdout.open("w") as out, stderr.open("w") as err:
                process = subprocess.run(["cargo", *args], cwd=ROOT, env={**environment, **(extra or {})}, stdout=out, stderr=err, timeout=3600)
            entry = {"command": ["cargo", *args], "returncode": process.returncode, "stdout": stdout.name, "stderr": stderr.name}
            context["commands"].append(entry)
            if tests:
                return entry, stdout.read_text(), stderr.read_text()
            if process.returncode:
                raise ValueError(f"{label} exited {process.returncode}")
            return entry, stdout.read_text(), stderr.read_text()
        cargo("census-build", ["build", "--locked", "--release", "-p", "slr-formats", "--bin", "slr-census"])
        binary = bundle / "target/release/slr-census"
        context["census_binary"] = {"sha256": digest(binary)}
        actual = census_run(binary, ROOT / "stock")
        (bundle / "census.json").write_text(json.dumps(actual, indent=2, sort_keys=True) + "\n")
        problems = census_difference(golden, actual)
        gaps = []
        differential = {}
        for name, (package, binary_name, names, variables) in G2_DIFFERENTIAL.items():
            extra = {key: str(ROOT / value) for key, value in variables.items()}
            entry, out, err = cargo("diff-" + binary_name, ["test", "--locked", "--release", "-p", package, "--test", binary_name, "--", *names, "--exact"], extra, tests=True)
            observation = rust_test_observation(entry["returncode"], out, err, expected_tests=names)
            differential[name] = observation
            if not observation["valid_component_observation"]:
                gaps.append(f"{name}: differential evidence did not execute cleanly: {observation['errors']}")
        entry, out, err = cargo("corpus", ["test", "--locked", "--release", "-p", "slr-formats", "--test", "corpus"], tests=True)
        corpus = rust_test_observation(entry["returncode"], out, err)
        if not corpus["valid_component_observation"]:
            gaps.append(f"corpus tests: {corpus['errors']}")
        gaps.extend(f"{name}: exe loader has no differential evidence" for name in G2_LOADER_GAPS)
        context.update(census_problems=problems, gaps=gaps, differential=differential, corpus=corpus)
        if tool.git_identity(ROOT)[0] != context["source"]:
            verdict = result("stale", "Candidate changed during formats verification.", bundle=str(bundle))
        elif problems:
            verdict = result("failed", "Census differs from the checked-in stock counts.", problems=problems, bundle=str(bundle))
        elif gaps:
            verdict = result("partial", "Census over every stock file equals the golden counts with zero unexplained files; loader evidence gaps remain.", gaps=gaps, no_exe_loader=G2_NO_EXE_LOADER, bundle=str(bundle), files=actual["files"])
        else:
            verdict = result("passed", "Census, differential loader tests and writer round trips verified.", bundle=str(bundle), files=actual["files"])
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        verdict = result("failed", str(error), bundle=str(bundle))
    finally:
        context["verdict"] = verdict
        (bundle / "receipt.json").write_text(json.dumps(context, indent=2) + "\n")
    return verdict
