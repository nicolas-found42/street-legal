from tests.support import *
from validators import boot as boot_module, core, retirement
import check as acceptance_check


class RetirementTests(unittest.TestCase):
    def populate(self, root):
        (root / "crates/slr-game/src").mkdir(parents=True)
        (root / "Cargo.toml").write_text('[workspace]\nmembers = ["crates/slr-game"]\n')
        (root / "Cargo.lock").write_text('version = 4\n')
        (root / "crates/slr-game/Cargo.toml").write_text('[package]\nname = "slr-game"\nversion = "0.1.0"\n')
        (root / "crates/slr-game/src/main.rs").write_text('fn main() {}\n')

    def test_directory_deletion_alone_cannot_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.populate(root)
            with patch.object(core, "ROOT", root):
                self.assertEqual(retirement.retirement_result(None)["result"], "not_run")
                for prerequisite in (None, {"result": "replay_required"}, {"result": "stale"}, {"result": "failed"}):
                    self.assertEqual(retirement.retirement_result(root / "out", prerequisite, live_execution=True)["result"], "blocked")

    def test_retired_dependencies_and_runtime_imports_cannot_hide(self):
        cases = {
            "Cargo.toml": '[workspace]\nmembers = ["crates/slr-core"]\n',
            "Cargo.lock": '[[package]]\nname = "macroquad"\nversion = "0.4.0"\n',
            "crates/slr-game/Cargo.toml": '[package]\nname = "slr-game"\nversion = "0.1.0"\n[dependencies.old]\npackage = "slr-core"\nversion = "0.1.0"\n',
            "crates/slr-game/src/main.rs": 'use macroquad::prelude::*;\nfn main() {}\n',
        }
        for relative, contents in cases.items():
            with self.subTest(path=relative), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                self.populate(root)
                (root / relative).write_text(contents)
                self.assertTrue(retirement.retirement_shape(root))
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.populate(root)
            (root / "crates/slr-core").mkdir()
            self.assertTrue(retirement.retirement_shape(root))
            (root / "crates/slr-core").rmdir()
            (root / "crates/slr-game/Cargo.toml").unlink()
            self.assertTrue(retirement.retirement_shape(root))

    def test_s10_dispatch_executes_s1_and_does_not_trust_supplied_bundle(self):
        fresh = {"result": "failed", "reason": "fresh boot failed"}
        with patch.object(boot_module, "run_boot", return_value=fresh) as boot, patch.object(retirement, "retirement_result", return_value={"result": "blocked", "reason": "x"}) as blocked:
            results = acceptance_check.evaluate(BASE["rows"], Path("stale-receipt"), ("S10",), Path("out"))
            boot.assert_called_once_with(Path("out"))
            self.assertEqual(blocked.call_args.args[1]["result"], "failed")
            self.assertTrue(blocked.call_args.kwargs["live_execution"])
            self.assertEqual({row["id"]: row["result"] for row in results}["S10"], "blocked")

    def run_host_case(self, root, failure=None):
        self.populate(root)
        boot = root / "boot"
        (boot / "runtime").mkdir(parents=True)
        for name in ("runtime.calllog", "runtime-all.calllog"):
            (boot / "runtime" / name).write_text('[1, 2]\n')
        manifest = {"source": {"revision": "synthetic"}, "configuration": {"stock": "stock"}, "stock_inputs_sha256": "stock",
            "child_arguments": ["boot-binary", "stock", "--output", "old-output", "--run-id", "old-run"]}
        metadata = {"packages": [{"id": name, "name": name, "targets": [{"name": "street-legal", "kind": ["bin"]}]} for name in ("slr-game", "slr-vm", "slr-io", "slr-calllog")],
            "workspace_members": ["slr-game"], "resolve": {"nodes": [{"id": "slr-game", "dependencies": ["slr-vm", "slr-io", "slr-calllog"]}]}}
        tool = SimpleNamespace(bundle_manifest=lambda _: manifest, tree_identity=lambda _: {"sha256": "stock"},
            environment_identity=lambda _: {"cargo": "synthetic", "rustc": "synthetic"},
            file_identity=lambda p: {"sha256": core.digest(p)}, verify_artifacts=lambda *_: None,
            correlate=lambda *_: {"diagnostic_parse_errors": [], "missing_or_empty_streams": [], "has_capture_end_event": True,
                "events_written": 2, "states_written": 1, "last_sequence": 2},
            load_records=lambda p: json.loads(p.read_text()))
        def execute(command, **kwargs):
            output = ""
            if command[1:2] == ["metadata"]:
                document = copy.deepcopy(metadata)
                if failure == "graph":
                    document["packages"].append({"id": "old", "name": "macroquad"})
                if failure == "host":
                    document["resolve"]["nodes"][0]["dependencies"] = []
                output = json.dumps(document)
            elif command[1:2] == ["test"]:
                output = RUST_OK
                if failure == "skipped":
                    output += "\nSLR_ORACLE_SKIPPED: missing capture\n"
            elif command[1:2] == ["build"]:
                binary = Path(kwargs["env"]["CARGO_TARGET_DIR"]) / "debug/street-legal"
                binary.parent.mkdir(parents=True)
                binary.write_bytes(b"synthetic replacement host")
            elif command[0] != "cargo":
                runtime = Path(command[command.index("--output") + 1])
                runtime.mkdir()
                for name in ("runtime.calllog", "runtime-all.calllog"):
                    (runtime / name).write_text('[1]' if failure == "prefix" else '[1, 2]')
                (runtime / "capture-end.json").write_text(json.dumps({"schema_version": 1, "run_id": runtime.parent.name,
                    "status": "completed", "truncated": failure == "capture", "pending_records": 0,
                    "events_written": 2, "states_written": 1, "last_sequence": 2, "filtered_records": 2, "all_records": 2}))
                output = "BOOT MainMenu dialog complete at frame 31; idle frames=" + ("299" if failure == "idle" else "300")
            kwargs["stdout"].write(output)
            return SimpleNamespace(returncode=1 if failure == "checks" and command[1:2] == ["fmt"] else 0)
        with patch.object(core, "ROOT", root), patch.object(core, "boot_tools", return_value=tool), patch.object(boot_module, "boot_result", return_value={"result": "replay_required"}), patch.object(core, "stock_identity", return_value={}), patch.object(core, "candidate_source_matches", return_value=failure != "stale"), patch.object(subprocess, "run", side_effect=execute):
            return retirement.retirement_result(root / "out", {"result": "passed", "bundle": str(boot)}, live_execution=True)

    def test_fresh_replacement_host_and_checks_are_required(self):
        for failure in (None, "graph", "host", "skipped", "prefix", "idle", "capture", "checks", "stale"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as tmp:
                observed = self.run_host_case(Path(tmp), failure)
                self.assertEqual(observed["result"], "passed" if failure is None else "stale" if failure == "stale" else "failed", observed)



