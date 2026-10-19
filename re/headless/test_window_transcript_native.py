"""Ignored independent original-code captures for explicit platform transcripts."""

import hashlib
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from desktop import DesktopProfile
from graphics import GraphicsProfile
from media import MediaProfile
from startup import OUTPUT_ROOT, REPO, Startup

EXE = REPO / "reference/raw/StreetLegal_Redline.bak.exe"
HERE = OUTPUT_ROOT / "window-transcript-native"
CASES = HERE / "cases.json"
MEDIA = OUTPUT_ROOT / "hypothetical-cd-toc.json"
GRAPHICS = OUTPUT_ROOT / "d3d/single-accepted.json"


class ObservedStartup(Startup):
    """Memory-write observation only; never replaces any original instruction."""

    def __init__(self, *args, **kwargs):
        self.native_writes = []
        super().__init__(*args, **kwargs)

    def _on_write(self, uc, access, address, size, value, ud):
        pc = self.reg("eip")
        if 0x4B8000 <= pc < 0x4B82A4 and 0x600000 <= address < 0x780000:
            self.native_writes.append(
                {
                    "pc": pc,
                    "address": address,
                    "bytes": size,
                    "value": value & ((1 << (8 * size)) - 1),
                }
            )


@unittest.skipUnless(
    EXE.exists() and CASES.exists() and MEDIA.exists() and GRAPHICS.exists(),
    "local original and ignored transcript captures absent",
)
class OriginalTranscriptTests(unittest.TestCase):
    def check_trace(self, run, trace, case):
        self.assertEqual(trace["instructions"], case["instruction_counter"])
        self.assertEqual(trace["stop"], case["stop"])
        self.assertEqual(
            [e for e in trace["events"] if "callback" in e or "callback_return" in e],
            case["callbacks"],
        )
        calls = [
            {k: e[k] for k in ("args", "return_site", "thread", "result")}
            for e in trace["events"]
            if e.get("import") == "user32.dll!DefWindowProcA"
        ]
        self.assertEqual(calls, case["default_calls"])
        self.assertEqual(run.native_writes, case["native_writes"])
        self.assertEqual(run.read_u32s(0x649588, 4), case["state"]["rect_words"])
        self.assertEqual(run.read_u32(0x617724), case["state"]["activateapp_word"])
        create = next(
            e
            for e in trace["events"]
            if e.get("import") == "user32.dll!CreateWindowExA"
        )
        self.assertEqual(create["args"], case["create_args"])
        self.assertEqual(create["return_site"], case["create_return_site"])
        self.assertEqual("result" in create, case["api_result_supplied"])
        handle = next(e["hwnd"] for e in trace["events"] if "callback" in e)
        self.assertEqual(
            trace["declared_window_api_results"],
            [{"hwnd": handle, "api_return_supplied": case["api_result_supplied"]}],
        )
        if case["api_result_supplied"]:
            self.assertEqual(create["result"], handle)
            self.assertEqual(
                create["return_assumption"], "declared-platform-api-result"
            )
            self.assertFalse(create["physical_desktop_observed"])
            self.assertFalse(create["windows_notification_sequence_established"])
            self.assertEqual(
                run.windows[handle]["creation"], "declared_api_return_supplied"
            )
        self.assertNotIn(
            next(
                a
                for a, n in run.import_names.items()
                if n == "user32.dll!CreateWindowExA"
            ),
            run.hooks,
        )
        if case["boundary_raw_args"] is not None:
            self.assertEqual(
                run.read_u32s(run.reg("esp") + 4, 2), case["boundary_raw_args"]
            )
        self.assertEqual(trace["pending_window_callbacks"], case["pending_callbacks"])
        self.assertEqual(trace["desktop_scenario"], run.desktop_profile.metadata)
        self.assertFalse(trace["desktop_scenario"]["physical_desktop_observed"])
        self.assertFalse(
            trace["desktop_scenario"]["windows_notification_sequence_established"]
        )
        self.assertFalse(trace["desktop_scenario"]["unlisted_notifications_modeled"])
        self.assertEqual(trace["status"], "incomplete")
        self.assertTrue(trace["winmain_reached"])
        self.assertFalse(trace["main_menu_reached"])

    def test_original_transcripts_preserve_callbacks_writes_and_refusal_and_repeat(
        self,
    ):
        self.assertLessEqual(CASES.stat().st_size, 65536)
        cases = json.loads(CASES.read_text())
        self.assertEqual(len(cases), 5)
        for case in cases:
            path = HERE / case["profile"]
            self.assertEqual(path.parent, HERE)

            def scenario(path=path):
                return ObservedStartup(
                    EXE,
                    media_profile=MediaProfile.load(MEDIA),
                    graphics_profile=GraphicsProfile.load(GRAPHICS),
                    desktop_profile=DesktopProfile.load(path),
                    record_limit=20000,
                )

            with self.subTest(profile=path.name):
                run = scenario()
                trace = run.run(5000000)
                self.check_trace(run, trace, case)
                again = scenario()
                second = again.run(5000000)
                self.check_trace(again, second, case)
                self.assertEqual(trace, second)
                blob = json.dumps(trace, sort_keys=True, indent=2) + "\n"
                (HERE / (path.stem + "-startup.json")).write_text(blob)
                (HERE / (path.stem + "-startup.sha256")).write_text(
                    hashlib.sha256(blob.encode()).hexdigest() + "\n"
                )
                receipt = {
                    "profile": path.name,
                    "instructions": trace["instructions"],
                    "stop": trace["stop"],
                    "callbacks": [
                        e
                        for e in trace["events"]
                        if "callback" in e or "callback_return" in e
                    ],
                    "actual_native_writes": run.native_writes,
                    "api_results": trace["declared_window_api_results"],
                    "whole_json_sha256": hashlib.sha256(blob.encode()).hexdigest(),
                }
                (HERE / (path.stem + "-receipt.json")).write_text(
                    json.dumps(receipt, indent=2) + "\n"
                )


if __name__ == "__main__":
    unittest.main()
