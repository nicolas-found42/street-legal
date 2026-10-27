from tests.support import *
from validators import boot, core, ledger
from validators.boot import BOOT_ORACLE, known_boot_oracle, boot_result, node_observation
from validators.ledger import native_registration_rows


class ProvenanceTests(unittest.TestCase):
    def test_node_observation_rejects_prefix_empty_malformed_and_changed_nodes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            port, original = root / "stderr", root / "original.jsonl"
            row = {"ic": 1, "c": "test.Class", "m": "run", "n": 0, "o": 1, "i": 0}
            original.write_text(json.dumps(row) + "\n" + json.dumps({**row, "ic": 2, "n": 1}) + "\n")
            port.write_text("STEP test.Class.run()V idx=0 op=0001 imm=0 line=0\n")
            self.assertFalse(node_observation(port, original, 2)["equal"])
            self.assertTrue(node_observation(port, original, 1)["equal"])
            port.write_text("STEP test.Class.run()V idx=0 op=0002 imm=0 line=0\n")
            self.assertFalse(node_observation(port, original, 1)["equal"])
            for bad in ("", "STEP malformed\n"):
                port.write_text(bad)
                with self.assertRaises(ValueError):
                    node_observation(port, original, 1)

    def test_field_initializer_nodes_are_observed_not_dropped(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            port, original = root / "stderr", root / "original.jsonl"
            port.write_text("STEP test.Class.<field init body1> idx=0 op=0001 imm=0 line=0\n")
            original.write_text(json.dumps({"ic": 1, "c": "test.Class", "m": "<static field init>", "n": 0, "o": 1, "i": 0}) + "\n")
            observed = node_observation(port, original, 1)
            self.assertEqual(observed["port_nodes"], 1)
            self.assertEqual(observed["dropped_nodes"], 0)

    def test_native_table_cannot_drop_duplicate_or_unresolve_rows(self):
        rows = [f"test.Class\tm{i}\t()V\tFUN_{0x400000 + i:08x}" for i in range(353)]
        self.assertEqual(len(native_registration_rows("\n".join(rows))), 353)
        for broken in (rows[:-1], rows + [rows[0]], rows[:-1] + [rows[0]], rows[:-1] + ["test.Class\tlast\t()V\tunknown"]):
            with self.subTest(broken=broken[-1]):
                with self.assertRaises(ValueError):
                    native_registration_rows("\n".join(broken))

    def test_dirty_tracked_and_untracked_bytes_invalidate_candidate(self):
        tool = core.boot_tools()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for command in (["git", "init", "-q", str(root)], ["git", "-C", str(root), "config", "user.email", "test@example.invalid"], ["git", "-C", str(root), "config", "user.name", "Acceptance Test"]):
                subprocess.run(command, check=True, capture_output=True)
            (root / "source.rs").write_text("base\n")
            subprocess.run(["git", "-C", str(root), "add", "source.rs"], check=True, capture_output=True)
            subprocess.run(["git", "-C", str(root), "commit", "-qm", "base"], check=True, capture_output=True)
            expected = tool.git_identity(root)[0]
            with patch.object(core, "ROOT", root), patch.object(core, "boot_tools", return_value=tool):
                self.assertTrue(core.candidate_source_matches(expected))
                (root / "source.rs").write_text("changed tracked source\n")
                self.assertFalse(core.candidate_source_matches(expected))
                expected = tool.git_identity(root)[0]
                (root / "new.rs").write_text("untracked source\n")
                self.assertFalse(core.candidate_source_matches(expected))
                expected = tool.git_identity(root)[0]
                (root / "new.rs").write_text("changed untracked bytes\n")
                self.assertFalse(core.candidate_source_matches(expected))

    def test_short_equal_logs_cannot_substitute_for_named_oracle(self):
        inputs = {name: {"sha256": value} for name, value in BOOT_ORACLE.items()}
        known_boot_oracle(inputs)
        for name in BOOT_ORACLE:
            with self.subTest(name=name):
                changed = copy.deepcopy(inputs)
                changed[name]["sha256"] = "0" * 64
                with self.assertRaises(ValueError):
                    known_boot_oracle(changed)

    def test_missing_oracle_identity_is_rejected(self):
        with self.assertRaises(ValueError):
            known_boot_oracle({})

    def test_missing_boot_bundle_cannot_pass(self):
        self.assertEqual(boot_result(None)["result"], "not_run")

    def test_malformed_boot_bundle_cannot_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(boot_result(Path(tmp))["result"], "invalid_evidence")


if __name__ == "__main__":
    unittest.main(verbosity=2)

