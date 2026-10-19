"""Optional actual-original desktop probes; resource vectors remain ignored."""

import hashlib
import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from desktop import DesktopProfile
from graphics import GraphicsProfile
from media import MediaProfile
from startup import OUTPUT_ROOT, REPO, Boundary, Startup

EXE = REPO / "reference/raw/StreetLegal_Redline.bak.exe"
HERE = OUTPUT_ROOT / "window"
CASES = HERE / "resource-api-cases.json"
PROFILE = HERE / "desktop-profile.json"


@unittest.skipUnless(
    EXE.exists() and CASES.exists() and PROFILE.exists(),
    "local original and ignored resource cases/profile absent",
)
class OriginalDesktopProbeTests(unittest.TestCase):
    def test_pe_cursor_and_icon_identity_is_recorded_without_modifying_resources(self):
        self.assertLessEqual(CASES.stat().st_size, 16384)
        cases = json.loads(CASES.read_text())
        self.assertTrue(1 <= len(cases) <= 64)
        run = Startup(EXE, desktop_profile=DesktopProfile.load(PROFILE))
        handles = []
        for case in cases:
            with self.subTest(api=case["api"], identifier=case["identifier"]):
                address = next(
                    address
                    for address, name in run.import_names.items()
                    if name == "user32.dll!" + case["api"]
                )
                args = [run.base, case["identifier"]]
                result = run.call(address, args, convention="stdcall", limit=100)
                self.assertEqual(result.esp_delta, 8)
                self.assertNotEqual(result.eax, 0)
                self.assertEqual(run.events[-1]["args"], args)
                actual = run.events[-1]["desktop"]
                for key in (
                    "identifier",
                    "language",
                    "image_id",
                    "group_sha256",
                    "payload_sha256",
                ):
                    self.assertEqual(actual[key], case[key])
                self.assertEqual(actual["source"], "pe")
                self.assertIsNone(actual["output_write"])
                self.assertEqual(run.call(address, args, limit=100).eax, result.eax)
                handles.append(result.eax)
        self.assertEqual(len(set(handles)), len(handles))

    def check_prefix(self, run, trace, expected):
        self.assertEqual(trace["instructions"], expected["instruction_counter"])
        self.assertEqual(trace["stop"], expected["stop"])
        self.assertEqual(
            [e for e in trace["events"] if "desktop" in e], expected["desktop_events"]
        )
        callback = next(
            e for e in trace["events"] if e.get("callback") == "WM_NCCREATE"
        )
        self.assertEqual(callback, expected["callback_event"])
        self.assertEqual(
            run.read_u32s(run.reg("esp") + 4, 4), expected["default_proc_args"]
        )
        self.assertEqual(trace["desktop_scenario"], run.desktop_profile.metadata)
        self.assertEqual(trace["pending_window_callbacks"], 1)
        self.assertEqual(trace["status"], "incomplete")
        self.assertTrue(trace["winmain_reached"])
        self.assertFalse(trace["main_menu_reached"])
        create = next(
            e
            for e in trace["events"]
            if e.get("import") == "user32.dll!CreateWindowExA"
        )
        self.assertNotIn("result", create)
        self.assertIn(callback["target"], trace["trail"])

    @unittest.skipUnless(
        (HERE / "prefix-expected.json").exists()
        and (OUTPUT_ROOT / "hypothetical-cd-toc.json").exists()
        and (OUTPUT_ROOT / "d3d/single-accepted.json").exists(),
        "local ignored original prefix captures absent",
    )
    def test_original_cursor_window_and_callback_prefix_repeats_before_default_proc(
        self,
    ):
        path = HERE / "prefix-expected.json"
        self.assertLessEqual(path.stat().st_size, 65536)
        expected = json.loads(path.read_text())

        def scenario():
            return Startup(
                EXE,
                media_profile=MediaProfile.load(
                    OUTPUT_ROOT / "hypothetical-cd-toc.json"
                ),
                graphics_profile=GraphicsProfile.load(
                    OUTPUT_ROOT / "d3d/single-accepted.json"
                ),
                desktop_profile=DesktopProfile.load(PROFILE),
                record_limit=20000,
            )

        run = scenario()
        first = run.run(5000000)
        self.check_prefix(run, first, expected)
        second = scenario().run(5000000)
        self.assertEqual(first, second)
        blob = json.dumps(first, sort_keys=True, indent=2) + "\n"
        (HERE / "repeated-startup.json").write_text(blob)
        (HERE / "repeated-startup.sha256").write_text(
            hashlib.sha256(blob.encode()).hexdigest() + "\n"
        )

    def test_missing_unknown_and_unscaled_resource_paths_stop_or_refuse_explicitly(
        self,
    ):
        cases = json.loads(CASES.read_text())
        first = next(c for c in cases if c["api"] == "LoadCursorA")
        run = Startup(EXE, desktop_profile=DesktopProfile.load(PROFILE))
        address = next(
            a for a, n in run.import_names.items() if n == "user32.dll!LoadCursorA"
        )
        absent = next(
            i
            for i in range(1, 65536)
            if i not in {c["identifier"] for c in cases if c["api"] == "LoadCursorA"}
        )
        self.assertEqual(run.call(address, [run.base, absent], limit=100).eax, 0)
        self.assertEqual(run.last_error, 1814)
        self.assertEqual(run.events[-1]["desktop"]["lookup"], "missing")
        for args in ([run.base, 0x10000], [0x1234, first["identifier"]]):
            with self.assertRaises(Boundary) as error:
                run.call(address, args, limit=100, regs={"eax": 123})
            self.assertEqual(error.exception.facts["kind"], "unsupported_image_lookup")
            self.assertEqual(run.reg("eax"), 123)
            self.assertNotIn("result", run.events[-1])
        data = json.loads(PROFILE.read_text())
        data["cursor_size"] = [16, 16]
        scaled = Startup(EXE, desktop_profile=DesktopProfile(data))
        address = next(
            a for a, n in scaled.import_names.items() if n == "user32.dll!LoadCursorA"
        )
        with self.assertRaises(Boundary) as error:
            scaled.call(address, [scaled.base, first["identifier"]], limit=100)
        self.assertEqual(error.exception.facts["kind"], "unsupported_pe_image")


if __name__ == "__main__":
    unittest.main()
