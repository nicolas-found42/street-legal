from tests.support import *
import check as acceptance_check
import g4

class G4Tests(unittest.TestCase):
    NATIVES = [
        {"class": "java.game.parts.WheelRef", "name": "setForce", "signature": "(F)V", "address": "LAB_00000001"},
        {"class": "java.lang.Math", "name": "sqrt", "signature": "(F)F", "address": "LAB_00000002"},
        {"class": "java.lang.Math", "name": "cosish", "signature": "(F)F", "address": "LAB_00000003"},
        {"class": "java.lang.Math", "name": "untested", "signature": "(F)F", "address": "LAB_00000004"},
    ]

    def ledger(self):
        return {
            "LAB_00000002": {"sources": ["crates/a/math.rs"], "status": "passing", "tests": [("a", "t_exact")], "oracles": []},
            "LAB_00000003": {"sources": ["crates/a/vec.rs"], "status": "passing", "tests": [("a", "t_trig")], "oracles": []},
            "LAB_00000004": {"sources": ["crates/a/x.rs"], "status": "ported (no test yet)", "tests": [], "oracles": []},
            "FUN_00000010": {"sources": ["crates/slr-physics/src/cfg/x.rs"], "status": "passing", "tests": [("slr-physics", "t_cfg")], "oracles": []},
        }

    def verdicts(self, observed, fixtures=None):
        return {row["address"]: row["verdict"] for row in g4.classify(
            self.NATIVES, self.ledger(), {"FUN_00000010"}, observed, {"crates/a/vec.rs"}, fixtures)}

    def test_each_failure_mode_is_distinct_and_only_exact_passing_counts(self):
        observed = {("a", "t_exact"), ("a", "t_trig"), ("slr-physics", "t_cfg")}
        got = self.verdicts(observed)
        self.assertEqual(got["LAB_00000001"], "unimplemented")
        self.assertEqual(got["LAB_00000002"], "passing")
        self.assertEqual(got["LAB_00000003"], "blocked_by_g11")
        self.assertEqual(got["LAB_00000004"], "no_test")
        self.assertEqual(got["FUN_00000010"], "passing")

    def test_a_ledger_claim_is_not_enough_without_the_observed_test(self):
        got = self.verdicts({("a", "t_exact")})
        self.assertEqual(got["LAB_00000003"], "no_test")
        self.assertEqual(got["FUN_00000010"], "no_test")

    def test_fixture_without_rows_for_the_address_cannot_pass(self):
        observed = {("a", "t_exact"), ("slr-physics", "t_cfg")}
        self.assertEqual(self.verdicts(observed, fixtures=set())["FUN_00000010"], "no_test")
        self.assertEqual(self.verdicts(observed, fixtures={"FUN_00000010"})["FUN_00000010"], "passing")

    def test_native_table_must_be_complete(self):
        with self.assertRaises(ValueError):
            g4.physics_natives("a\tb\tc\td\n")

    def test_ledger_parser_reads_status_sources_and_tests(self):
        text = "| `FUN_00000010` | `crates/x/a.rs:f:1`<br>`crates/x/b.rs:g:2` | passing | `c::t1`<br>`c::t2` (unicorn) |\n"
        row = g4.ledger_rows(text)["FUN_00000010"]
        self.assertEqual(row["sources"], ["crates/x/a.rs", "crates/x/b.rs"])
        self.assertEqual(row["tests"], [("c", "t1"), ("c", "t2")])

    def test_real_native_table_has_physics_functions_and_g4_never_passes_vacuously(self):
        table = ROOT / "re/out/native_api.tsv"
        if not table.is_file():
            self.skipTest("native registration table not present")
        natives = g4.physics_natives(table.read_text())
        self.assertGreater(len(natives), 100)
        verdicts = g4.classify(natives, {}, set(), set(), set())
        self.assertTrue(all(row["verdict"] == "unimplemented" for row in verdicts))

    def test_table_reports_g4_as_not_run(self):
        with tempfile.TemporaryDirectory() as tmp:
            rows = acceptance_check.evaluate(copy.deepcopy(BASE["rows"]), output_root=Path(tmp))
        self.assertEqual({r["id"]: r["result"] for r in rows}["G4"], "not_run")
