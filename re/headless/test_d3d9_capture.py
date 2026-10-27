"""Data-free tests of the capture wrapper's guards (no oracle is run)."""
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import d3d9_capture as capture  # noqa: E402


class CaptureGuards(unittest.TestCase):
    def test_unknown_scenario_is_rejected(self):
        with self.assertRaises(ValueError):
            capture.capture("no-such-scenario", capture.OUTPUT_ROOT / "d3d9-test")

    def test_output_outside_the_worktree_oracle_root_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp, self.assertRaises(ValueError):
            capture.capture("menu-idle-600M", Path(tmp) / "run")

    def test_symlinked_output_root_is_rejected_before_writing(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo, shared = Path(tmp) / "repo", Path(tmp) / "shared"
            (repo / "re/out").mkdir(parents=True)
            shared.mkdir()
            (repo / "re/out/headless").symlink_to(shared)
            with patch.object(capture, "REPO", repo), patch.object(capture, "OUTPUT_ROOT", repo / "re/out/headless"):
                with self.assertRaisesRegex(ValueError, "inside this worktree"):
                    capture.capture("menu-idle-600M", repo / "re/out/headless/new-run")
            self.assertEqual(list(shared.iterdir()), [])

    def test_wrong_executable_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake = Path(tmp) / "fake.exe"
            fake.write_bytes(b"MZ")
            root = Path(tmp) / "repo"
            output = root / "re/out/headless"
            output.mkdir(parents=True)
            with patch.object(capture, "REPO", root), patch.object(capture, "OUTPUT_ROOT", output):
                with self.assertRaisesRegex(ValueError, "identity"):
                    capture.capture("menu-idle-600M", output / "never-created", exe=fake)

    def test_command_pins_the_declared_scenario(self):
        command = capture.build_command(capture.SCENARIOS["menu-idle-1800M"], Path("exe"), Path("p.json"), Path("o"), "py")
        self.assertIn("--fast-count", command)
        self.assertEqual(command[command.index("--limit") + 1], "1800000000")
        self.assertEqual(command[command.index("--calllog") + 1], "o/run.slrlog")

    def test_manifest_schema_and_scenarios_are_declared(self):
        self.assertEqual(capture.MANIFEST_SCHEMA, "slr-d3d9-capture-1")
        self.assertIn("menu-idle-1800M", capture.SCENARIOS)


if __name__ == "__main__":
    unittest.main()
