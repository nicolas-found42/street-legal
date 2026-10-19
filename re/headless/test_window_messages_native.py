"""Optional actual-original creation scenarios; derived captures stay ignored."""

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
HERE = OUTPUT_ROOT / "window-next"
CASES = HERE / "creation-cases.json"
MEDIA = OUTPUT_ROOT / "hypothetical-cd-toc.json"
GRAPHICS = OUTPUT_ROOT / "d3d/single-accepted.json"


@unittest.skipUnless(
    EXE.exists() and CASES.exists() and MEDIA.exists() and GRAPHICS.exists(),
    "local original and ignored declared creation captures absent",
)
class OriginalCreationMessageTests(unittest.TestCase):
    def check_trace(self, run, trace, case):
        self.assertEqual(trace["instructions"], case["instruction_counter"])
        self.assertEqual(trace["stop"], case["stop"])
        self.assertEqual(
            [e for e in trace["events"] if "callback" in e or "callback_return" in e],
            case["callbacks"],
        )
        calls = [
            e for e in trace["events"] if e.get("import") == "user32.dll!DefWindowProcA"
        ]
        self.assertEqual(len(calls), len(case["default_calls"]))
        for actual, expected in zip(calls, case["default_calls"]):
            self.assertEqual(actual["args"], expected["args"])
            self.assertEqual(actual["return_site"], expected["return_site"])
            self.assertEqual(actual.get("result"), expected.get("result"))
            self.assertIsNone(actual["desktop"]["output_write"])
        self.assertEqual(
            calls[0]["desktop"]["create_struct_words"], case["create_words"]
        )
        self.assertEqual(
            calls[0]["desktop"]["declared_nccreate_result"],
            run.desktop_profile.creation.default_nccreate_result,
        )
        self.assertEqual(trace["pending_window_callbacks"], case["pending_count"])
        if case["pending_rect"] is not None:
            self.assertEqual(
                run.read_u32s(calls[-1]["args"][3], 4), case["pending_rect"]
            )
            self.assertEqual(calls[-1]["desktop"]["rect_words"], case["pending_rect"])
            self.assertNotIn("result", calls[-1])
        create = next(
            e
            for e in trace["events"]
            if e.get("import") == "user32.dll!CreateWindowExA"
        )
        self.assertNotIn("result", create)
        self.assertIn(case["callbacks"][0]["target"], trace["trail"])
        self.assertEqual(trace["desktop_scenario"], run.desktop_profile.metadata)
        self.assertFalse(trace["desktop_scenario"]["physical_desktop_observed"])
        self.assertEqual(trace["status"], "incomplete")
        self.assertTrue(trace["winmain_reached"])
        self.assertFalse(trace["main_menu_reached"])

    def test_original_default_procedure_and_declared_next_message_repeat(self):
        self.assertLessEqual(CASES.stat().st_size, 32768)
        cases = json.loads(CASES.read_text())
        self.assertEqual(len(cases), 3)
        for case in cases:
            profile = HERE / case["profile"]
            self.assertEqual(profile.parent, HERE)

            def scenario(profile=profile):
                return Startup(
                    EXE,
                    media_profile=MediaProfile.load(MEDIA),
                    graphics_profile=GraphicsProfile.load(GRAPHICS),
                    desktop_profile=DesktopProfile.load(profile),
                    record_limit=20000,
                )

            with self.subTest(profile=profile.name):
                run = scenario()
                trace = run.run(5000000)
                self.check_trace(run, trace, case)
                blob = json.dumps(trace, sort_keys=True, indent=2) + "\n"
                (HERE / (profile.stem + "-startup.json")).write_text(blob)
                (HERE / (profile.stem + "-startup.sha256")).write_text(
                    hashlib.sha256(blob.encode()).hexdigest() + "\n"
                )
                # Every scenario repeats; no callback result is accepted on faith.
                self.assertEqual(trace, scenario().run(5000000))


if __name__ == "__main__":
    unittest.main()
