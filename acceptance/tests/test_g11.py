from tests.support import *
import check as acceptance_check
import g11

class G11Tests(unittest.TestCase):
    def counts(self, rows, **pairs):
        out = {"rows": rows}
        for name, value in pairs.items():
            out[name.replace("_eq_", "==") + ":ext80"] = value
        return out

    def test_transcendental_pins_only_on_complete_agreement(self):
        legs = ["bochs", "rosetta", "slr"]
        ok = g11.transcendental_verdict("fsin", self.counts(10, bochs_eq_rosetta=10, bochs_eq_slr=10, rosetta_eq_slr=10), legs)
        self.assertEqual(ok["verdict"], "pinned_bit_exact")
        for key in ("bochs_eq_rosetta", "bochs_eq_slr", "rosetta_eq_slr"):
            with self.subTest(key=key):
                pairs = dict(bochs_eq_rosetta=10, bochs_eq_slr=10, rosetta_eq_slr=10)
                pairs[key] = 9
                self.assertEqual(g11.transcendental_verdict("fsin", self.counts(10, **pairs), legs)["verdict"], "unpinned")

    def test_a_tolerance_style_near_total_agreement_is_not_a_pin(self):
        legs = ["bochs", "rosetta"]
        verdict = g11.transcendental_verdict("fcos", self.counts(1_000_000, bochs_eq_rosetta=999_999), legs)
        self.assertEqual(verdict["verdict"], "unpinned")

    def test_empty_observation_is_unprobed_not_pinned(self):
        self.assertEqual(g11.transcendental_verdict("fsin", {}, ["bochs", "rosetta"])["verdict"], "unprobed")

    def test_hardware_disagreement_keeps_the_op_unpinned(self):
        legs = ["bochs", "intel", "rosetta"]
        counts = self.counts(10, bochs_eq_rosetta=10, bochs_eq_intel=7, intel_eq_rosetta=7)
        verdict = g11.transcendental_verdict("fsin", counts, legs)
        self.assertEqual(verdict["verdict"], "unpinned")
        self.assertIn("Intel silicon differs", verdict["reason"])

    def test_hardware_observation_must_be_untranslated_genuine_intel(self):
        good = '{"sysctl_proc_translated":0,"sysctl_status":0,"cpu_brand":"Intel(R) Core(TM) i7","cpu_vendor":"GenuineIntel"}\n'
        self.assertEqual(g11.validate_hardware_observation(good, bytes(22 * 3), 3)["cpu_vendor"], "GenuineIntel")
        bad = {
            "translated": good.replace('translated":0', 'translated":1'),
            "apple": good.replace("GenuineIntel", "?"),
            "amd": good.replace("GenuineIntel", "AuthenticAMD"),
            "no brand": good.replace("Intel(R) Core(TM) i7", " "),
            "query failed": good.replace('status":0', 'status":-1'),
            "two lines": good + good,
            "empty": "",
        }
        for name, text in bad.items():
            with self.subTest(name=name), self.assertRaises(ValueError):
                g11.validate_hardware_observation(text, bytes(22 * 3), 3)
        with self.assertRaises(ValueError):
            g11.validate_hardware_observation(good, bytes(22 * 2), 3)

    def test_probe_mismatch_or_missing_cells_cannot_pin(self):
        text = "  fadd   pc=24 rc=0 match=   10 mismatch=   0 skipped=   1 quirk-negzero=0\n  fadd   pc=64 rc=0 match=    5 mismatch=   1 skipped=   0 quirk-negzero=0\n"
        totals = g11.probe_totals(text)
        self.assertEqual(totals["fadd"]["match"], 15)
        self.assertEqual(g11.probe_verdict("fadd", totals)["verdict"], "unpinned")
        self.assertEqual(g11.probe_verdict("fmul", totals)["verdict"], "unprobed")

    def test_every_required_op_has_a_verdict_and_unprobed_ops_block(self):
        stream = {"by_op": {}}
        table = g11.trust_table("", {w: stream for w in g11.CONTROL_WORDS}, {w: [] for w in g11.CONTROL_WORDS},
                                {"op": "fprem", "verdict": "unprobed", "reason": "x"})
        self.assertEqual({row["op"] for row in table}, set(g11.REQUIRED))
        self.assertTrue(all(row["verdict"] != "pinned_bit_exact" for row in table))

    def test_live_validator_without_pinned_bochs_cannot_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            with patch.object(g11, "boot_tools") as tools:
                tools.return_value.git_identity.return_value = ({}, b"")
                outcome = g11.g11_result(Path(tmp), None)
            self.assertEqual(outcome["result"], "failed")

    def test_table_reports_g11_as_not_run(self):
        with tempfile.TemporaryDirectory() as tmp:
            rows = acceptance_check.evaluate(copy.deepcopy(BASE["rows"]), output_root=Path(tmp))
        self.assertEqual({r["id"]: r["result"] for r in rows}["G11"], "not_run")
