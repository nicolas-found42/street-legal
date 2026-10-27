"""S9: live native macOS display checks with authored, fully checked pixels."""
from __future__ import annotations
import json
import os
import platform
import subprocess
import sys
import uuid
from pathlib import Path
from validators.core import ROOT, boot_tools, digest, result

TARGET = "aarch64-apple-darwin"
PROFILES = ((640, 480, False), (960, 540, True))


def positive(value, label):
    if type(value) is not int or value <= 0:
        raise ValueError(f"{label} must be a positive integer")
    return value


def letterbox(logical, drawable):
    lw, lh = logical
    dw, dh = drawable
    if lw * dh <= dw * lh:
        w, h = max(1, dh * lw // lh), dh
    else:
        w, h = dw, max(1, dw * lh // lw)
    return {"x": (dw-w)//2, "y": (dh-h)//2, "w": w, "h": h}


def validate_probe(folder: Path, logical, initial_fullscreen):
    rows = [json.loads(line) for line in (folder / "session-00000.jsonl").read_text().splitlines()]
    if not rows or [r.get("sequence") for r in rows] != list(range(1, len(rows)+1)):
        raise ValueError("native journal sequence is incomplete")
    if any(r.get("schema_version") != 1 for r in rows):
        raise ValueError("native journal schema mismatch")
    def events(kind):
        return [r for r in rows if r.get("kind") == kind]
    start = events("display-probe-start")
    if len(start) != 1 or start[0]["data"] != {"os": "macos", "arch": "aarch64", "authored_inputs": True, "stock_or_original_assets_used": False}:
        raise ValueError("probe did not declare native macOS arm64 authored execution")
    if not any("backend: Metal" in r["data"].get("info", "") and "device_type: Cpu" not in r["data"].get("info", "") for r in events("gpu-adapter")):
        raise ValueError("native Metal adapter observation missing")
    if len(events("window-created")) != 1:
        raise ValueError("real window creation missing")
    if any(events(kind) for kind in ("gpu-error", "surface-error", "worker-stall")):
        raise ValueError("GPU, surface or worker failure")
    exits = events("ui-exit")
    scripted = events("display-script-exit")
    if len(exits) != 1 or exits[0]["data"].get("error") is not None or len(scripted) != 1 or scripted[0]["data"].get("presents") != 220:
        raise ValueError("scripted display run did not complete cleanly")
    requests = events("display-request")
    if len(requests) != 2 or [r["data"].get("fullscreen") for r in requests] != [not initial_fullscreen, initial_fullscreen] or any(r["data"].get("applied") is not True for r in requests):
        raise ValueError("display mode requests were not applied")
    captures = events("drawable-captured")
    if len(captures) != 3 or [r["data"].get("name") for r in captures] != ["initial", "toggled", "restored"]:
        raise ValueError("three ordered native captures are required")
    pixels = json.loads((folder / "pixels.json").read_text())
    if pixels.get("schema") != 1 or pixels.get("authored_inputs") is not True or pixels.get("stock_or_original_assets_used") is not False:
        raise ValueError("decoded pixel report scope mismatch")
    images = pixels.get("images", [])
    if len(images) != 3:
        raise ValueError("three decoded PNG reports are required")
    observations = []
    for i, (capture, image) in enumerate(zip(captures, images)):
        data = capture["data"]
        mode = initial_fullscreen if i != 1 else not initial_fullscreen
        if data.get("fullscreen") is not mode:
            raise ValueError("observed display mode differs from requested scenario")
        width, height = positive(data.get("width"), "width"), positive(data.get("height"), "height")
        positive(data.get("frame_id"), "frame ID")
        if data.get("presents") != (10, 100, 200)[i]:
            raise ValueError("capture is not at the declared presentation step")
        layouts = [r for r in events("display-layout") if r["sequence"] < capture["sequence"]]
        if not layouts:
            raise ValueError("capture has no observed native layout")
        layout = layouts[-1]["data"]
        if layout.get("logical") != list(logical) or layout.get("drawable") != [width, height] or layout.get("fullscreen") is not mode:
            raise ValueError("capture/layout geometry or mode mismatch")
        expected = letterbox(logical, (width, height))
        if data.get("letterbox") != expected or layout.get("letterbox") != expected:
            raise ValueError("logical aspect/letterbox does not match the drawable")
        monitor = layout.get("monitor")
        if not isinstance(monitor, dict) or not isinstance(monitor.get("size"), list) or len(monitor["size"]) != 2:
            raise ValueError("physical monitor dimensions missing")
        for dimension in monitor["size"]:
            positive(dimension, "monitor dimension")
        if mode and monitor["size"] != [width, height]:
            raise ValueError("fullscreen does not cover the observed monitor")
        if not mode:
            scale = layout.get("scale_factor")
            if type(scale) not in (int, float) or scale <= 0 or (width, height) != (round(logical[0]*scale), round(logical[1]*scale)):
                raise ValueError("windowed resolution differs from the declared logical size and scale")
        filename = f"drawable-{data['name']}.png"
        if data.get("file") != filename or image.get("file") != filename or image.get("name") != data["name"]:
            raise ValueError("capture PNG identity mismatch")
        if image.get("authored_pixels_match") is not True or image.get("width") != width or image.get("height") != height:
            raise ValueError("decoded authored PNG pixels/geometry mismatch")
        if image.get("png_sha256") != digest(folder / filename) or image.get("rgba_sha256") != data.get("rgba_sha256") or len(image.get("rgba_sha256", "")) != 64:
            raise ValueError("capture PNG or decoded pixels changed")
        observations.append({"fullscreen": mode, "width": width, "height": height, "letterbox": expected, "png_sha256": image["png_sha256"]})
    if observations[0] != observations[2] | {"png_sha256": observations[0]["png_sha256"]}:
        raise ValueError("display geometry did not restore")
    return {"logical": list(logical), "initial_fullscreen": initial_fullscreen, "captures": observations, "complete": True}


def window_result(output_root: Path):
    if sys.platform != "darwin" or platform.machine() != "arm64":
        return result("not_run", "S9 requires actual macOS arm64 window/display execution.")
    root = output_root.resolve()
    if not root.is_relative_to(ROOT.resolve() / "re/out"):
        return result("failed", "Window evidence output must stay inside this worktree's re/out.")
    bundle = root / ("window-" + uuid.uuid4().hex[:12])
    bundle.mkdir(parents=True)
    context = {"schema": 1, "obligation": "S9", "coverage": "shared production native window/display backend only", "stock_exe": {"used": False, "reason": "authored framebuffer inputs; no stock/original scenes"}, "oracle": {"kind": "none", "boundary": "physical winit/Metal window, not a headless shim"}, "input_schedule": "two logical resolutions; windowed/fullscreen startup; actions after 10/30/100/130/200/220 presentations; opaque red frames below ID60, blue thereafter; producer nominal16ms; no RNG", "commands": []}
    verdict = result("failed", "Native window validation did not complete.", bundle=str(bundle))
    try:
        tool = boot_tools()
        context["source"], patch = tool.git_identity(ROOT)
        (bundle / "source.diff").write_bytes(patch)
        context["untracked_source_archive"] = tool.archive_untracked_sources(ROOT, bundle, context["source"])
        env = {k: v for k, v in os.environ.items() if not k.startswith("SLR_")}
        target = ROOT / "target-platform"
        env.update(CARGO_TARGET_DIR=str(target), CARGO_INCREMENTAL="0")
        command = ["cargo", "build", "--locked", "--target", TARGET, "-p", "slr-d3d9", "--example", "display_probe"]
        with (bundle / "build.stdout").open("w") as out, (bundle / "build.stderr").open("w") as err:
            built = subprocess.run(command, cwd=ROOT, env=env, stdout=out, stderr=err, timeout=600)
        context["commands"].append({"command": command, "returncode": built.returncode})
        if built.returncode:
            raise ValueError("native display probe build failed")
        binary = target / TARGET / "debug/examples/display_probe"
        context["binary"] = {"path": str(binary), "sha256": digest(binary)}
        observations = []
        for width, height, initial in PROFILES:
            folder = bundle / f"{width}x{height}-{'fullscreen' if initial else 'windowed'}"
            folder.mkdir()
            toggle = "windowed" if initial else "fullscreen"
            restore = "fullscreen" if initial else "windowed"
            parameters = {"SLR_LOGICAL_SIZE": f"{width}x{height}", "SLR_FULLSCREEN": "1" if initial else "0", "SLR_DISPLAY_SCRIPT": f"10:capture=initial,30:{toggle},100:capture=toggled,130:{restore},200:capture=restored,220:exit", "SLR_D3D9_STREAM": str(folder / "calls.slrlog")}
            (folder / "parameters.json").write_text(json.dumps(parameters, indent=2)+"\n")
            command = [str(binary), str(folder)]
            with (folder / "stdout").open("w") as out, (folder / "stderr").open("w") as err:
                run = subprocess.run(command, cwd=ROOT, env={**env, **parameters}, stdout=out, stderr=err, timeout=45)
            context["commands"].append({"command": command, "returncode": run.returncode, "parameters": parameters})
            if run.returncode:
                raise ValueError(f"native display probe exited {run.returncode}: {folder.name}")
            observations.append(validate_probe(folder, (width, height), initial))
        context["observations"] = observations
        context["source_after"] = tool.git_identity(ROOT)[0]
        context["binary_after_sha256"] = digest(binary)
        if context["source_after"] != context["source"] or context["binary_after_sha256"] != context["binary"]["sha256"]:
            verdict = result("stale", "Source or native executable changed during display verification.", bundle=str(bundle))
        else:
            verdict = result("passed", "Real macOS arm64/Metal windowed and fullscreen startup, transitions, restoration, resolutions, aspect bars and every authored PNG pixel verified; S9 only.", bundle=str(bundle), original_scene_parity=False, prime_integration=False, physical_input_parity=False)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        verdict = result("failed", str(error), bundle=str(bundle))
    finally:
        context["verdict"] = verdict
        (bundle / "receipt.json").write_text(json.dumps(context, indent=2)+"\n")
    return verdict
