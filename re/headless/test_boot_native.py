"""Original-code prefix tests: stock scripts under the declared post-window scenario.

These execute the real build 606 executable through window creation, D3D9
device creation, input/sound initialisation, system.rpk loading and the Config
constructor. They skip explicitly when the local ignored profiles, the stock
tree or the executable are absent. They establish prefix behaviour under the
declared hypothetical scenarios only; nothing here is boot-to-MainMenu.
"""

import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import calllog
from desktop import DesktopProfile
from graphics import GraphicsProfile
from make_boot_profile import GRAPHICS, PROFILE
from media import MediaProfile
from platform_profile import PlatformProfile
from startup import OUTPUT_ROOT, REPO, Startup

EXE = REPO / "reference/raw/StreetLegal_Redline.bak.exe"
MEDIA = OUTPUT_ROOT / "hypothetical-cd-toc.json"
DESKTOP = OUTPUT_ROOT / "window-transcript-native/hypothetical-seven-notifications-declared-handle-profile.json"
STOCK = REPO / "stock"
READY = EXE.exists() and MEDIA.exists() and DESKTOP.exists() and (STOCK / "system.rpk").exists()
CONFIG_ID = 0xFFFF0080
GAMEINIT_ID = 0xFFFF007F


def run_prefix(limit, observe_dir=None, calllog_path=None):
    profile = PlatformProfile(json.loads(json.dumps(PROFILE)))
    startup = Startup(
        EXE,
        media_profile=MediaProfile.load(MEDIA),
        graphics_profile=GraphicsProfile(json.loads(json.dumps(GRAPHICS))),
        desktop_profile=DesktopProfile.load(DESKTOP),
        platform_profile=profile,
        observe_dir=observe_dir,
        calllog_path=calllog_path,
        record_limit=100000,
        event_ring=2000,
        fast_count=True,
    )
    return startup, startup.run(limit)


@unittest.skipUnless(READY, "local executable, stock tree or ignored desktop/media profiles absent")
class ScriptPrefixTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.observe = Path(cls.directory.name) / "observed"
        cls.log = Path(cls.directory.name) / "boot.slrlog"
        # 4.0M block-counted instructions: past WinMain, window, D3D9 device, input,
        # sound, system.rpk, the JVM boot function and the Config constructor.
        cls.startup, cls.result = run_prefix(4_000_000, cls.observe, cls.log)
        cls.facts = json.loads((cls.observe / "original-facts.json").read_text())

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def test_original_reaches_device_creation_and_applies_the_documented_fpu_change(self):
        device = self.result["d3d_device"]
        self.assertTrue(device["created"])
        self.assertEqual(device["creations"][0]["flags"], 0x84)  # MIXED | MULTITHREADED, no FPU_PRESERVE
        self.assertEqual(device["creations"][0]["params"]["BackBufferWidth"], 800)
        self.assertEqual(device["fpu_control_word_changes"][0]["after"], 0x7F)
        self.assertFalse(self.result["platform_scenario"]["physical_gpu_observed"])
        self.assertEqual(self.result["instruction_counter_mode"], "block")

    def test_original_executes_stock_script_code_and_builds_config(self):
        facts = self.facts
        self.assertFalse(facts["alters_execution"])
        self.assertGreater(facts["first_thread_run_icount"], 0)
        config = facts["resource_creations"][0]
        self.assertEqual(config["name"], "Config")
        self.assertEqual(config["resource_instance_id"], CONFIG_ID)
        boot = facts["boot_function"]
        self.assertEqual(boot["words"]["0x100"], CONFIG_ID)
        self.assertEqual(boot["words"]["0x104"], config["returned_node"])
        self.assertEqual(boot["words"]["0x110"], 0x3F800000)  # time scale 1.0f
        natives = facts["native_calls"]
        self.assertEqual(
            (natives[0]["class"], natives[0]["name"]),
            ("java.util.resource.ResourceRef", "newNative"),
        )
        self.assertEqual(natives[1]["name"], "open")
        reads = [(c["name"], c["result"]) for c in natives if c["class"] == "java.io.File"]
        self.assertIn(("readInt", 800), reads)
        self.assertIn(("readInt", 600), reads)

    def test_config_getters_and_unfiltered_native_stream_are_recorded(self):
        records = calllog.Reader((self.observe / "script-natives-all.slrlog").read_bytes()).records()
        self.assertGreater(len(records), 40)
        names = {(r[1], r[2]) for r in records}
        self.assertIn(("java.sound.Sound", "setVolume"), names)  # a class the port's filter drops
        filtered = calllog.Reader((self.observe / "script-natives.slrlog").read_bytes()).records()
        self.assertTrue(all(not r[1].startswith("java.sound.") for r in filtered))
        self.assertEqual(
            [r for r in records if not r[1].startswith(("java.sound.", "java.render."))
             and r[1] not in ("java.io.Input", "java.io.MouseCursor", "java.io.Controller")],
            filtered,
        )

    def test_logs_are_valid_slrlog_with_interleaved_platform_families(self):
        records = calllog.Reader(self.log.read_bytes()).records()
        families = {r[0] for r in records}
        self.assertTrue({"native", "d3d9"} <= families)
        # Config is constructed before graphics initialization in the original.
        self.assertEqual(records[0][:3], ("native", "java.util.resource.ResourceRef", "newNative"))
        create = next(i for i, record in enumerate(records)
                      if record[:3] == ("d3d9", 0, "Direct3DCreate9"))
        self.assertGreater(create, 0)
        self.assertEqual(self.result["calllog"]["records"], len(records))

    def test_original_file_io_and_own_diagnostics_are_retained(self):
        """The exe's file layer and its own `diag.log` are evidence, not gaps.

        `docs/call-log.md` records that the original writes `diag.log` through
        `0x5513b0`; this run intercepts the same writes, so the bytes are the
        original's own output rather than a hook-side reconstruction.
        """
        self.assertGreater(self.result["file_log"]["records"], 0)
        records = calllog.Reader(Path(self.result["file_log"]["path"]).read_bytes()).records()
        operations = {record[1] for record in records if record[0] == "file"}
        self.assertEqual(operations, {0, 1, 2, 3}, "open/read/write/close are all recorded")
        opened = [record for record in records if record[0] == "file" and record[1] == 0]
        self.assertTrue(any(record[2].endswith("diag.log") for record in opened))
        diag = Path(self.observe) / "original-diag.log"
        self.assertTrue(diag.exists())
        text = diag.read_text()
        self.assertIn("SL2 build 606", text)
        self.assertIn("diag.log", self.result["observed_files"])

    def test_prefix_is_repeatable_byte_for_byte(self):
        # `calllog`, `observer`, `file_log` and `observed_files` describe the
        # capture this particular invocation asked for, not the emulated run.
        _, again = run_prefix(4_000_000)
        keep = lambda r: {k: v for k, v in r.items()
                          if k not in ("calllog", "observer", "file_log", "observed_files")}
        digest = lambda r: hashlib.sha256(json.dumps(keep(r), sort_keys=True).encode()).hexdigest()
        self.assertEqual(digest(self.result), digest(again))


if __name__ == "__main__":
    unittest.main()
