"""Linked fixtures must not turn worktree run outputs into writes to main."""
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("link-worktree.sh")


class WorktreeLinksTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="slr-links-")
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.main = root / "main source"
        self.worker = root / "worker checkout"
        (self.worker / "tools").mkdir(parents=True)
        shutil.copyfile(SCRIPT, self.worker / "tools/link-worktree.sh")

    def put(self, name, value="fixture"):
        path = self.main / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value)

    def run_links(self, main=None):
        return subprocess.run(
            ["sh", str(self.worker / "tools/link-worktree.sh"), str(main or self.main)],
            capture_output=True, text=True,
        )

    def test_fixtures_link_but_runs_and_indexes_stay_local(self):
        for name in ("stock/input", "reference/input", "re/out/boot/snapshot.json"):
            self.put(name)
        for area in ("boot", "menu", "acceptance", "headless"):
            self.put(f"re/out/{area}/runs/old/manifest.json")
            self.put(f"re/out/{area}/test-runs/old/result.json")
            self.put(f"re/out/{area}/runs-index.jsonl", "main-index\n")
            self.put(f"re/out/{area}/inputs-store/old", "shared-input")
            self.put(f"re/out/{area}/d3d9-stream/old/run.slrlog")
            self.put(f"re/out/{area}/snapshot-run/old/metadata.json")
        result = self.run_links()
        self.assertEqual(result.returncode, 0, result.stderr)
        for name in ("stock", "reference", "re/out/boot/snapshot.json"):
            self.assertTrue((self.worker / name).is_symlink(), name)
        for area in ("boot", "menu", "acceptance", "headless"):
            output = self.worker / "re/out" / area
            for name in ("runs", "test-runs", "inputs-store", "d3d9-stream", "snapshot-run"):
                self.assertFalse((output / name).is_symlink(), f"{area}/{name}")
                (output / name / "new-worker-run").mkdir(parents=True)
                self.assertFalse((self.main / "re/out" / area / name / "new-worker-run").exists())
            (output / "runs-index.jsonl").write_text("worker-index\n")
            self.assertEqual((self.main / "re/out" / area / "runs-index.jsonl").read_text(), "main-index\n")
        self.assertEqual(self.run_links().returncode, 0)

    def test_existing_shared_output_root_is_rejected(self):
        self.put("re/out/boot/snapshot.json")
        (self.worker / "re/out").mkdir(parents=True)
        (self.worker / "re/out/boot").symlink_to(self.main / "re/out/boot")
        result = self.run_links()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("shared output", result.stderr)

    def test_shared_output_ancestor_is_rejected_before_any_write(self):
        self.put("re/out/boot/snapshot.json")
        (self.worker / "re").mkdir()
        (self.worker / "re/out").symlink_to(self.main / "re/out")
        result = self.run_links()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("shared output", result.stderr)
        self.assertFalse((self.main / "re/out/boot/runs").exists())

    def test_existing_shared_input_store_is_rejected(self):
        self.put("re/out/boot/inputs-store/old")
        (self.worker / "re/out/boot").mkdir(parents=True)
        (self.worker / "re/out/boot/inputs-store").symlink_to(self.main / "re/out/boot/inputs-store")
        result = self.run_links()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("shared output", result.stderr)

    def test_existing_shared_run_directory_is_rejected(self):
        self.put("re/out/boot/runs/old/manifest.json")
        (self.worker / "re/out/boot").mkdir(parents=True)
        (self.worker / "re/out/boot/runs").symlink_to(self.main / "re/out/boot/runs")
        result = self.run_links()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("shared output", result.stderr)

    def test_empty_source_does_not_create_a_literal_glob_link(self):
        self.main.mkdir()
        result = self.run_links()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(list((self.worker / "re/out").iterdir()), [])

    def test_main_checkout_is_refused(self):
        result = self.run_links(self.worker)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("main checkout", result.stderr)


if __name__ == "__main__":
    unittest.main()
