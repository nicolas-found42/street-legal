"""Data-free checks of the CLI boundary; no game/decompiler output in fixtures."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import sys
import subprocess


SPEC = importlib.util.spec_from_file_location("kuna_export", Path(__file__).with_name("export.py"))
export = importlib.util.module_from_spec(SPEC)
if SPEC.loader:
    SPEC.loader.exec_module(export)


class ExportTests(unittest.TestCase):
    def test_missing_and_failed_outputs_stay_explicit(self):
        targets = {0x401000: "FUN_00401000", 0x402000: "FUN_00402000", 0x403000: "FUN_00403000"}
        rows = export.normalize({"functions": [
            {"address": 0x401000, "code": "void example(void) {}", "error": None},
            {"address_hex": "0x402000", "code": None, "error": "budget exceeded"},
        ]}, targets)
        self.assertEqual([r["status"] for r in rows], ["c_emitted", "error", "error"])
        self.assertIn("missing", rows[2]["error"])

    def test_warnings_are_not_clean_output(self):
        rows = export.normalize({"functions": [{"address": 0x401000,
            "code": "void example(void) // warn: Exceeded instruction limit\n{}"}]}, {0x401000: "FUN_00401000"})
        self.assertEqual(rows[0]["status"], "c_with_warnings")

    def test_address_spellings_resolve_the_same_entry(self):
        for value in ("FUN_00401000", "LAB_00401000", "sub_401000", "0x401000", "00401000", 0x401000):
            self.assertEqual(export.address(value), 0x401000)

    def test_unrecovered_jump_dispatch_is_flagged(self):
        rows = export.normalize({"functions": [{"address": 0x401000,
            "code": "return (*table[index])(); // jump-as-call"}]}, {0x401000: "FUN_00401000"})
        self.assertEqual(rows[0]["status"], "c_with_warnings")

    def test_noncontiguous_body_size_is_not_used_as_an_end(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "index.txt"
            p.write_text("00401000 FUN_00401000 17\n00402000 FUN_00402000 6\n")
            definitions = export.definitions(export.read_index(p))
            self.assertEqual(definitions, "0x401000=FUN_00401000\n0x402000=FUN_00402000\n")

    def test_stalled_process_is_terminated_and_not_reported_as_success(self):
        with tempfile.TemporaryDirectory() as d:
            result = export.run_bounded([sys.executable, "-c", "import time; time.sleep(10)"],
                                        Path(d) / "result.json", 0.05, 512)
            self.assertIn("wall time", result["error"])
            self.assertNotEqual(result["exit_code"], 0)

    def test_c_emission_total_includes_diagnostic_bodies(self):
        with tempfile.TemporaryDirectory() as d:
            targets = {0x401000: "FUN_00401000", 0x402000: "FUN_00402000"}
            rows = export.normalize({"functions": [
                {"address": 0x401000, "code": "void example(void) {}"},
                {"address": 0x402000, "code": "return (*table[index])(); // jump-as-call"},
            ]}, targets)
            summary = export.summarize(Path(d), targets, {"functions": []}, rows)
            self.assertEqual(summary["c_emitted"], 2)
            self.assertEqual(summary["c_without_recognized_diagnostics"], 1)
            self.assertEqual(summary["c_with_warnings"], 1)

    def test_worker_remains_bounded_after_its_parent_exits(self):
        with tempfile.TemporaryDirectory() as d:
            result = export.run_bounded([sys.executable, "-c",
                "import subprocess,sys; subprocess.Popen([sys.executable,'-c','import time; time.sleep(10)'])"],
                Path(d) / "result.json", 0.05, 512)
            self.assertIn("wall time", result["error"])

    def test_partial_inventory_cache_is_rebuilt_by_the_export_command(self):
        area = export.ROOT / "re/out/kuna"
        area.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=area) as d:
            directory = Path(d)
            binary = directory / "synthetic.bin"
            binary.write_bytes(b"synthetic input")
            index = directory / "index.txt"
            index.write_text("00401000 FUN_00401000 6\n")
            out = directory / "export"
            out.mkdir()
            (out / "default-functions.json").write_text('{"functions":[')
            fake = directory / "fake-kuna"
            fake.write_text(f"#!{sys.executable}\n" + '''import json,sys
if '--version' in sys.argv:
    print('synthetic-kuna 0')
else:
    row = {'address': 4198400, 'name': 'FUN_00401000'}
    if sys.argv[1] == 'decompile-all':
        row['code'] = 'void synthetic(void) {}'
    print(json.dumps({'count': 1, 'functions': [row], 'error': None}))
''')
            fake.chmod(0o700)
            result = subprocess.run([sys.executable, str(Path(__file__).with_name("export.py")),
                "export", "--binary", str(binary), "--index", str(index), "--out", str(out),
                "--kuna", str(fake)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            import json
            summary = json.loads((out / "summary.json").read_text())
            self.assertEqual(summary["c_emitted"], 1)
            self.assertEqual(summary["pending"], 0)


if __name__ == "__main__":
    unittest.main()
