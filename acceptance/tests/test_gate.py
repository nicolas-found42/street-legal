from tests.support import *
from validators import core
import check as acceptance_check


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



