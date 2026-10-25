#!/usr/bin/env python3
"""Adversarial data-free regressions for the whole-game acceptance gate."""
from __future__ import annotations

import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from types import SimpleNamespace

from observations import rust_test_observation
from validators import BOOT_ORACLE, known_boot_oracle, boot_result, native_registration_rows, node_observation
import validators
import check as acceptance_check

ROOT = Path(__file__).resolve().parents[1]
CHECK = ROOT / "acceptance/check.py"
BASE = json.loads((ROOT / "acceptance/matrix.json").read_text())


class GateTests(unittest.TestCase):
    def setUp(self):
        self.data = copy.deepcopy(BASE)
        for row in self.data["rows"]:
            row["status"] = "open"
            row["evidence"] = []

    def invoke(self, expected=2, *args):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "matrix.json"
            path.write_text(json.dumps(self.data))
            result = subprocess.run([sys.executable, str(CHECK), "--matrix", str(path), *args], capture_output=True, text=True)
        self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
        return result

    def test_complete_honest_inventory_is_incomplete(self):
        self.invoke(1)

    def test_table_is_explicitly_report_only(self):
        result = self.invoke(0, "--table", "--json")
        report = json.loads(result.stdout)
        self.assertFalse(report["accepted"])
        self.assertTrue(report["report_only"])
        self.assertEqual(len(report["results"]), 54)
        results = {row["id"]: row["result"] for row in report["results"]}
        self.assertEqual(results["S1"], "not_run")
        self.assertEqual(results["S11"], "not_run")
        self.assertEqual(results["S10"], "failed")
        self.assertTrue(all(value != "passed" for value in results.values()))

    def test_single_row_cannot_substitute_for_contract(self):
        self.data["rows"] = self.data["rows"][:1]
        self.invoke()

    def test_any_omitted_obligation_is_rejected(self):
        for index in range(54):
            with self.subTest(index=index):
                rows = self.data["rows"]
                self.data["rows"] = rows[:index] + rows[index+1:]
                self.invoke()
                self.data["rows"] = rows

    def test_document_is_not_executable_passing_evidence(self):
        self.data["rows"][0].update(status="passed", evidence=["docs/fidelity.md"])
        self.invoke()

    def test_all_passed_assertions_are_rejected(self):
        for row in self.data["rows"]:
            row.update(status="passed", evidence=["docs/fidelity.md"])
        self.invoke()

    def test_table_cannot_hide_unsupported_passing_claim(self):
        self.data["rows"][0]["status"] = "passed"
        self.invoke(2, "--table")

    def test_inventory_cannot_supply_commands_or_results(self):
        for key in ("command", "validator", "result", "passed"):
            with self.subTest(key=key):
                self.data["rows"][0][key] = "true"
                self.invoke()
                del self.data["rows"][0][key]

    def test_supporting_document_does_not_promote_result(self):
        self.data["rows"][0].update(status="partial", evidence=["docs/fidelity.md"])
        report = json.loads(self.invoke(1, "--json").stdout)
        self.assertEqual(report["results"][0]["result"], "unsupported")

    def test_unknown_status_vocabulary_cannot_waive_pass(self):
        self.data["statuses"] = ["done"]
        self.data["rows"][0]["status"] = "done"
        self.invoke()

    def test_feature_tiers_cannot_be_dropped(self):
        for index in range(30):
            with self.subTest(index=index):
                tiers = self.data["rows"][index]["tiers"]
                self.data["rows"][index]["tiers"] = []
                self.invoke()
                self.data["rows"][index]["tiers"] = tiers

    def test_kind_cannot_change_obligation(self):
        self.data["rows"][0]["kind"] = "supporting"
        self.invoke()

    def test_duplicate_cannot_replace_missing_row(self):
        self.data["rows"][-1] = self.data["rows"][0]
        self.invoke()

    def test_missing_evidence(self):
        self.data["rows"][0]["evidence"] = ["acceptance/not-present.json"]
        self.invoke()

    def test_directory_is_not_evidence(self):
        self.data["rows"][0]["evidence"] = ["docs"]
        self.invoke()

    def test_absolute_evidence_is_rejected(self):
        self.data["rows"][0]["evidence"] = [str(ROOT / "docs/fidelity.md")]
        self.invoke()

    def test_traversal_evidence_is_rejected(self):
        self.data["rows"][0]["evidence"] = ["acceptance/../docs/fidelity.md"]
        self.invoke()

    def test_malformed_inputs_fail_with_gate_error(self):
        baseline = copy.deepcopy(self.data)
        for broken in (None, [], {**baseline, "rows": None}, {**baseline, "rows": [None]}, {**baseline, "rows": [{"id": []}]}):
            with self.subTest(broken=broken):
                self.data = broken
                self.invoke()

    def test_wrong_schema(self):
        self.data["schema"] = 1
        self.invoke()

    def test_unknown_arguments_rejected(self):
        self.invoke(2, "--typo")


class ObservationTests(unittest.TestCase):
    OK = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s"

    def test_component_success_stays_component_evidence(self):
        self.assertTrue(rust_test_observation(0, self.OK, "")["valid_component_observation"])

    def test_execution_status_cannot_override_checked_validator_result(self):
        for execution_status in ("completed", "failed", "passing"):
            observed = validators.result("failed", "native streams differ", status=execution_status)
            self.assertEqual(observed["result"], "failed")
            self.assertEqual(observed["status"], execution_status)
        with self.assertRaises(ValueError):
            validators.result("failed", "native streams differ", result="passed")

    def test_missing_fixture_cannot_pass_on_zero_exit(self):
        for stream in (0, 1):
            outputs = [self.OK, ""]
            outputs[stream] += "\nSLR_ORACLE_SKIPPED: snapshot fixtures missing\n"
            self.assertFalse(rust_test_observation(0, *outputs)["valid_component_observation"])

    def test_legacy_skip_cannot_pass_on_zero_exit(self):
        for marker in ("SKIP: missing stock", "SKIP missing original capture"):
            self.assertFalse(rust_test_observation(0, self.OK, marker)["valid_component_observation"])

    def test_filtered_suites_require_successful_expected_tests(self):
        filtered = self.OK.replace("0 filtered out", "5 filtered out")
        self.assertFalse(rust_test_observation(0, filtered, "")["valid_component_observation"])
        self.assertFalse(rust_test_observation(0, filtered, "", ("required::oracle",))["valid_component_observation"])
        self.assertTrue(rust_test_observation(0, "test required::oracle ... ok\n" + filtered, "", ("required::oracle",))["valid_component_observation"])

    def test_empty_ignored_failed_or_nonzero_cannot_pass(self):
        for code, output in ((0, ""), (0, self.OK.replace("1 passed", "0 passed")), (0, self.OK.replace("0 ignored", "1 ignored")), (0, self.OK.replace("0 failed", "1 failed")), (1, self.OK)):
            with self.subTest(code=code, output=output):
                self.assertFalse(rust_test_observation(code, output, "")["valid_component_observation"])


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
            with patch.object(validators, "ROOT", root):
                self.assertEqual(validators.retirement_result(None)["result"], "not_run")
                for prerequisite in (None, {"result": "replay_required"}, {"result": "stale"}, {"result": "failed"}):
                    self.assertEqual(validators.retirement_result(root / "out", prerequisite, live_execution=True)["result"], "blocked")

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
                self.assertTrue(validators.retirement_shape(root))
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.populate(root)
            (root / "crates/slr-core").mkdir()
            self.assertTrue(validators.retirement_shape(root))
            (root / "crates/slr-core").rmdir()
            (root / "crates/slr-game/Cargo.toml").unlink()
            self.assertTrue(validators.retirement_shape(root))

    def test_s10_dispatch_executes_s1_and_does_not_trust_supplied_bundle(self):
        fresh = {"result": "failed", "reason": "fresh boot failed"}
        with patch.object(acceptance_check, "run_boot", return_value=fresh) as boot, patch.object(acceptance_check, "retirement_result", return_value={"result": "blocked"}) as retirement:
            acceptance_check.evaluate(BASE["rows"], Path("stale-receipt"), ("S10",), Path("out"))
            boot.assert_called_once_with(Path("out"))
            self.assertEqual(retirement.call_args.args[1]["result"], "failed")
            self.assertTrue(retirement.call_args.kwargs["live_execution"])

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
            file_identity=lambda p: {"sha256": validators.digest(p)}, verify_artifacts=lambda *_: None,
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
                output = ObservationTests.OK
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
        with patch.object(validators, "ROOT", root), patch.object(validators, "boot_tools", return_value=tool), patch.object(validators, "boot_result", return_value={"result": "replay_required"}), patch.object(validators, "stock_identity", return_value={}), patch.object(validators, "candidate_source_matches", return_value=failure != "stale"), patch.object(validators.subprocess, "run", side_effect=execute):
            return validators.retirement_result(root / "out", {"result": "passed", "bundle": str(boot)}, live_execution=True)

    def test_fresh_replacement_host_and_checks_are_required(self):
        for failure in (None, "graph", "host", "skipped", "prefix", "idle", "capture", "checks", "stale"):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as tmp:
                observed = self.run_host_case(Path(tmp), failure)
                self.assertEqual(observed["result"], "passed" if failure is None else "stale" if failure == "stale" else "failed", observed)


class FixtureSourcesTests(unittest.TestCase):
    def populate_required_inputs(self, root):
        required = [
            "re/out/native_api.tsv",
            *("re/out/headless/" + relative for relative in (
                "hypothetical-cd-toc.json",
                "boot/graphics-profile.json",
                "window-transcript-native/hypothetical-seven-notifications-declared-handle-profile.json",
                "boot/platform-profile.json",
                "clock-investigation/menu1800M/observed/original-facts.json",
                "finalizer-investigation/gc-probe.json",
                "finalizer-investigation/string-add.json",
                "finalizer-investigation/long-observed/original-facts.json",
                "finalizer-investigation/callers-observed/original-facts.json",
                "finalizer-investigation/frame-long-observed/frame-timer-inputs.jsonl",
                "finalizer-investigation/frame-long-observed/original-facts.json",
                "oracle-33d8aa8/observed/script-natives.slrlog",
                "package-investigation/full-observed/original-facts.json",
                "allocation-history/long-observed/original-facts.json",
            )),
        ]
        for relative in required:
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"synthetic fixture\n")
        (root / required[0]).write_text("\n".join(
            f"test.Class\tm{i}\t()V\tFUN_{0x400000 + i:08x}" for i in range(353)))
        return {root / relative for relative in required}

    def test_media_archives_excluded_without_dropping_boot_or_unknown_inputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            expected = self.populate_required_inputs(root)
            # These actual default-test inputs and their byte companions must
            # survive the media exclusion; no fixture extension is narrowed.
            retained = (
                "boot/native-frame-original.json",
                "boot/native-cursor-original.json",
                "boot/native.pages", "boot/native.ckpt", "boot/native.bin",
                "headless/clock-investigation/menu1800M/clock-inputs.jsonl",
                "headless/clock-investigation/menu1800M/clock-companion.json",
                "unknown-original/nested/.hidden-input",
                ".hidden-original/nested/payload.unfamiliar",
                "gameplay-comparison-original/fixture.json",
                "implementation-showcase-2026-10-23/original.json",
                "critique-acceptance-original/.fixture",
            )
            for relative in retained:
                path = root / "re/out" / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"required original input\n")
                expected.add(path)
            for family in (
                "gameplay-comparison-2026-10-22",
                "implementation-showcase-2026-10-22",
                "critique-acceptance-2026-10-22",
            ):
                for relative in ("videos/gameplay.mp4", "screens/frame.png", "build/nested/large.rlib", ".hidden/data.json"):
                    path = root / "re/out" / family / relative
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(b"not a production fixture\n")
            self.assertEqual(set(validators.fixture_sources(root)), expected)

    def test_required_original_inputs_cannot_be_omitted(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            required = self.populate_required_inputs(root)
            self.assertTrue(required <= set(validators.fixture_sources(root)))
            for path in sorted(required):
                with self.subTest(path=path.relative_to(root).as_posix()):
                    contents = path.read_bytes()
                    path.unlink()
                    try:
                        with self.assertRaises((OSError, ValueError)):
                            validators.fixture_sources(root)
                    finally:
                        path.write_bytes(contents)

    def test_empty_required_inputs_fail(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            required = self.populate_required_inputs(root)
            for path in sorted(required):
                with self.subTest(path=path.relative_to(root).as_posix()):
                    contents = path.read_bytes()
                    path.write_bytes(b"")
                    try:
                        with self.assertRaises(ValueError):
                            validators.fixture_sources(root)
                    finally:
                        path.write_bytes(contents)

    def test_output_bundles_are_not_copied_back_into_the_candidate(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            required = self.populate_required_inputs(root)
            output = root / "re/out/acceptance"
            nested = output / "ledger-123/candidate/re/out/vector.json"
            nested.parent.mkdir(parents=True)
            nested.write_text("old validation output")
            self.assertEqual(set(validators.fixture_sources(root, excluded_roots=(output,))), required)


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
        tool = validators.boot_tools()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for command in (["git", "init", "-q", str(root)], ["git", "-C", str(root), "config", "user.email", "test@example.invalid"], ["git", "-C", str(root), "config", "user.name", "Acceptance Test"]):
                subprocess.run(command, check=True, capture_output=True)
            (root / "source.rs").write_text("base\n")
            subprocess.run(["git", "-C", str(root), "add", "source.rs"], check=True, capture_output=True)
            subprocess.run(["git", "-C", str(root), "commit", "-qm", "base"], check=True, capture_output=True)
            expected = tool.git_identity(root)[0]
            with patch.object(validators, "ROOT", root), patch.object(validators, "boot_tools", return_value=tool):
                self.assertTrue(validators.candidate_source_matches(expected))
                (root / "source.rs").write_text("changed tracked source\n")
                self.assertFalse(validators.candidate_source_matches(expected))
                expected = tool.git_identity(root)[0]
                (root / "new.rs").write_text("untracked source\n")
                self.assertFalse(validators.candidate_source_matches(expected))
                expected = tool.git_identity(root)[0]
                (root / "new.rs").write_text("changed untracked bytes\n")
                self.assertFalse(validators.candidate_source_matches(expected))

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
