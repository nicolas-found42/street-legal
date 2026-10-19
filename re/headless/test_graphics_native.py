"""Original enumeration over local, explicit hypothetical scenario inputs.

Derived configurations and expected mode records stay in ignored scratch JSON.
These are bounded prefix tests, never physical GPU or MainMenu acceptance.
"""

import copy
import hashlib
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from graphics import GraphicsProfile
from media import MediaProfile
from startup import OUTPUT_ROOT, REPO, Startup

EXE = REPO / "reference/raw/StreetLegal_Redline.bak.exe"
MEDIA = OUTPUT_ROOT / "hypothetical-cd-toc.json"
HERE = OUTPUT_ROOT / "d3d"
INPUTS = (
    HERE / "single-accepted.json",
    HERE / "empty-refused.json",
    HERE / "creation-null.json",
    HERE / "enumeration-expected.json",
)


@unittest.skipUnless(
    EXE.exists() and MEDIA.exists() and all(p.exists() for p in INPUTS),
    "local original and ignored hypothetical graphics/media captures absent",
)
class OriginalGraphicsProbeTests(unittest.TestCase):
    def source(self, path):
        self.assertLessEqual(path.stat().st_size, 65536)
        return json.loads(path.read_text())

    def scenario(self, config):
        return Startup(
            EXE,
            media_profile=MediaProfile.load(MEDIA),
            graphics_profile=GraphicsProfile(config),
            record_limit=20000,
        )

    def check_prefix(self, run, trace, expected):
        self.assertEqual(trace["stop"], expected["stop"])
        self.assertEqual(trace["instructions"], expected["counter"])
        self.assertEqual(trace["status"], "incomplete")
        self.assertTrue(trace["winmain_reached"])
        self.assertFalse(trace["main_menu_reached"])
        self.assertEqual(
            trace["graphics_scenario"],
            {**run.graphics_profile.metadata, "observed_creation_sdk_versions": [31]},
        )
        self.assertEqual(run.read_u32(0x6495F4), len(expected["mode_records"]))
        modes = run.read_u32(0x6466A4)
        actual = [
            run.read_u32s(modes + 20 * index, 5)
            for index in range(len(expected["mode_records"]))
        ]
        self.assertEqual(actual, expected["mode_records"])
        calls = []
        for event in trace["events"]:
            if event.get("com") != "IDirect3D9":
                continue
            args = event["args"][1:]
            if event["slot"] in (7, 8):
                args = args[:-1]
            calls.append(
                {key: event[key] for key in ("slot", "method", "return_site", "result")}
                | {"args_without_pointers": args}
            )
        if "calls" in expected:
            self.assertEqual(calls, expected["calls"])
        return actual

    def test_original_enumerator_filters_refusals_and_empty_modes(self):
        source = self.source(INPUTS[0])
        expected = self.source(INPUTS[3])
        receipts = []
        for label, config in (
            ("accept", source),
            (
                "refuse",
                {
                    **copy.deepcopy(source),
                    "type_checks": {key: 0x8876086A for key in source["type_checks"]},
                },
            ),
            ("empty", self.source(INPUTS[1])),
        ):
            with self.subTest(case=label):
                run = self.scenario(config)
                trace = run.run(limit=5000000)
                records = self.check_prefix(run, trace, expected[label])
                receipts.append(
                    {
                        "case": label,
                        "instruction_counter": trace["instructions"],
                        "mode_records": records,
                        "stop": trace["stop"],
                    }
                )
        (HERE / "native-enumeration-cases.json").write_text(
            json.dumps(receipts, indent=2) + "\n"
        )

    def test_explicit_graphics_prefix_repeats_and_creation_null_follows_original_fault(
        self,
    ):
        config = self.source(INPUTS[0])
        run = self.scenario(config)
        first = run.run(limit=5000000)
        self.check_prefix(run, first, self.source(INPUTS[3])["accept"])
        second = self.scenario(config).run(limit=5000000)
        self.assertEqual(first, second)
        blob = json.dumps(first, sort_keys=True, indent=2) + "\n"
        (HERE / "repeated-startup.json").write_text(blob)
        (HERE / "repeated-startup.sha256").write_text(
            hashlib.sha256(blob.encode()).hexdigest() + "\n"
        )
        null = self.scenario(self.source(INPUTS[2]))
        trace = null.run(limit=5000000)
        self.assertEqual(trace["stop"]["kind"], "execution_error")
        self.assertIn("0x4b897c", trace["stop"]["message"])
        self.assertEqual(null.graphics.objects, set())
        self.assertFalse(trace["main_menu_reached"])
        self.assertEqual(trace["status"], "incomplete")
        (HERE / "null-creation-startup.json").write_text(
            json.dumps(trace, sort_keys=True, indent=2) + "\n"
        )


if __name__ == "__main__":
    unittest.main()
