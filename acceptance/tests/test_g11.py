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

    def test_native_intel_allows_only_documented_missing_translation_flag(self):
        import errno
        identity = {"sysctl_proc_translated": 0, "sysctl_status": -1,
                    "sysctl_errno": errno.ENOENT, "cpu_brand": "Intel test CPU",
                    "cpu_vendor": "GenuineIntel"}
        def read(value):
            return g11.validate_hardware_observation(json.dumps(value), bytes(22), 1)
        self.assertEqual(read(identity), identity)
        mutations = [
            {"sysctl_errno": errno.EACCES}, {"sysctl_errno": errno.EIO},
            {"sysctl_errno": 0}, {"sysctl_errno": None},
            {"sysctl_status": -2}, {"sysctl_status": False},
            {"sysctl_proc_translated": 1}, {"sysctl_proc_translated": False},
            {"cpu_vendor": "AuthenticAMD"}, {"cpu_vendor": "Apple"},
            {"cpu_brand": ""}, {"cpu_brand": 123}, {"cpu_brand": "?"},
        ]
        for mutation in mutations:
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                read(identity | mutation)
        failed_without_errno = dict(identity)
        del failed_without_errno["sysctl_errno"]
        with self.assertRaises(ValueError):
            read(failed_without_errno)
        with self.assertRaises(ValueError):
            read(identity | {"sysctl_status": 0})
        self.assertEqual(read(identity | {"sysctl_status": 0, "sysctl_errno": 0})["sysctl_status"], 0)

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


class OperatorOptionsTests(unittest.TestCase):
    def test_registry_passes_hardware_options_only_to_live_gate(self):
        bochs = Path("/operator/bochs")
        hardware = Path("/operator/intel")
        with tempfile.TemporaryDirectory() as tmp, patch.object(g11, "g11_result", return_value={"result": "blocked", "reason": "fixture control"}) as run:
            output = Path(tmp)
            options = {"bochs": bochs, "intel_observation": hardware}
            acceptance_check.evaluate(copy.deepcopy(BASE["rows"]), output_root=output, options=options)
            run.assert_not_called()
            results = acceptance_check.evaluate(copy.deepcopy(BASE["rows"]), execute=["G11"], output_root=output, options=options)
            run.assert_called_once_with(output, bochs, hardware)
        self.assertEqual(next(r for r in results if r["id"] == "G11")["result"], "blocked")


class StreamTrustTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        import struct
        cls.struct = struct
        cls.adjudicate = g11.load_module("test_g11_adjudicate", g11.G11_DIR / "adjudicate.py")

    def compare(self, op, operand, r0, r1a, r1b):
        meta = {"rows": 1, "index": [{"op": op, "spec": "authored", "first": 0, "rows": 1}]}
        corpus = b"\0" + operand + bytes(10)
        legs = {"bochs": [r0 + r1a + bytes(2)], "rosetta": [r0 + r1b + bytes(2)]}
        return self.adjudicate.compare(meta, corpus, legs)["by_op"]

    def value(self, sign, exponent, significand=1 << 63):
        return self.struct.pack("<QH", significand, (sign << 15) | exponent)

    def test_both_results_of_two_result_operations_must_match(self):
        one, two = self.value(0, 0x3fff), self.value(0, 0x4000)
        for op in ("fptan", "fsincos"):
            with self.subTest(op=op):
                counts = self.compare(op, one, one, one, two)[op]
                self.assertEqual(counts["bochs==rosetta:ext80"], 0)
                self.assertEqual(counts["bochs==rosetta:f32"], 0)
                self.assertEqual(g11.transcendental_verdict(op, counts, ["bochs", "rosetta"])["verdict"], "unpinned")

    def test_defined_exceptional_operands_cannot_leave_denominator(self):
        one, two = self.value(0, 0x3fff), self.value(0, 0x4000)
        for operand in (self.value(1, 0x3fff), self.value(0, 0x7fff), self.value(1, 0x7fff), self.value(0, 0x7fff, 0xc000000000000001)):
            with self.subTest(operand=operand.hex()):
                counts = self.compare("fyl2x", operand, one, one, two)
                self.assertIn("fyl2x", counts)
        nan = self.value(0, 0x7fff, 0xc000000000000001)
        self.assertIn("f2xm1", self.compare("f2xm1", nan, one, one, one))
        self.assertIn("f2xm1:out-of-domain", self.compare("f2xm1", two, one, one, one))

    def test_fsincos_is_a_required_game_operation(self):
        self.assertEqual(g11.REQUIRED["fsincos"], ("game", "stream"))
        self.assertEqual(g11.STREAM_OPS["fsincos"], "fsincos")

    def test_binary_edge_corpus_includes_special_second_operands(self):
        make = g11.load_module("test_g11_make_corpus", g11.G11_DIR / "make_corpus.py")
        corpus, _ = make.build(0)
        records = {corpus[i:i + 21] for i in range(0, len(corpus), 21)}
        one = self.value(0, 0x3fff)
        nan = self.value(0, 0x7fff, 0xc000000000000000)
        for op in make.C.BINARY:
            self.assertIn(bytes([make.C.OPS[op]]) + one + nan, records)
