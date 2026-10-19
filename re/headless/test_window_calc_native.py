"""Optional original-code calc/create continuations; captures remain ignored."""

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
HERE = OUTPUT_ROOT / "window-calc"
CASES = HERE / "calc-cases.json"
MEDIA = OUTPUT_ROOT / "hypothetical-cd-toc.json"
GRAPHICS = OUTPUT_ROOT / "d3d/single-accepted.json"


@unittest.skipUnless(
    EXE.exists() and CASES.exists() and MEDIA.exists() and GRAPHICS.exists(),
    "local original and ignored calc captures absent",
)
class OriginalCalculationTests(unittest.TestCase):
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
        self.assertEqual(calls, case["default_calls"])
        self.assertEqual(trace["pending_window_callbacks"], case["pending_count"])
        calc = next(e for e in calls if e["args"][1] == 0x83)
        self.assertEqual(
            run.read_u32s(calc["args"][3], 4), calc["desktop"]["output_rect"]
        )
        create = next(
            e
            for e in trace["events"]
            if e.get("import") == "user32.dll!CreateWindowExA"
        )
        self.assertNotIn("result", create)
        if case["pending_count"]:
            self.assertNotIn("result", calls[-1])
            self.assertEqual(
                run.read_u32s(calls[-1]["args"][3], 12),
                calls[0]["desktop"]["create_struct_words"],
            )
        self.assertEqual(trace["desktop_scenario"], run.desktop_profile.metadata)
        self.assertFalse(trace["desktop_scenario"]["physical_desktop_observed"])
        self.assertIn(case["callbacks"][0]["target"], trace["trail"])
        self.assertEqual(trace["status"], "incomplete")
        self.assertTrue(trace["winmain_reached"])
        self.assertFalse(trace["main_menu_reached"])

    def test_original_identity_rect_and_create_boundary_repeat(self):
        self.assertLessEqual(CASES.stat().st_size, 32768)
        cases = json.loads(CASES.read_text())
        self.assertEqual(len(cases), 2)
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
                first = run.run(5000000)
                self.check_trace(run, first, case)
                self.assertEqual(first, scenario().run(5000000))
                blob = json.dumps(first, sort_keys=True, indent=2) + "\n"
                (HERE / (profile.stem + "-startup.json")).write_text(blob)
                (HERE / (profile.stem + "-startup.sha256")).write_text(
                    hashlib.sha256(blob.encode()).hexdigest() + "\n"
                )


if __name__ == "__main__":
    unittest.main()
