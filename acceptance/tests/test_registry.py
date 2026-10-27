"""The registry must fail closed for every validator, whoever writes it."""
from tests.support import *
from validators import core, registry
from validators.registry import Validator


def rows(*ids):
    return [{"id": rid, "kind": "feature", "title": rid, "status": "open", "evidence": []} for rid in ids]


class RegistryTests(unittest.TestCase):
    def setUp(self):
        self.patches = [patch.object(registry, "REGISTRY", {}), patch.object(registry, "_loaded", True)]
        for item in self.patches:
            item.start()
            self.addCleanup(item.stop)

    def add(self, rid, fn, **kwargs):
        registry.validator(rid, **kwargs)(fn)

    def outcome(self, rid="S2", **kwargs):
        return {row["id"]: row for row in registry.evaluate(rows(rid), **kwargs)}[rid]

    def test_unregistered_row_is_unsupported(self):
        self.assertEqual(self.outcome()["result"], "unsupported")

    def test_registered_validator_result_is_used(self):
        self.add("S2", lambda ctx: core.result("passed", "ok"))
        self.assertEqual(self.outcome()["result"], "passed")

    def test_validator_exception_is_failed_not_passed(self):
        def boom(ctx):
            raise RuntimeError("no data")
        self.add("S2", boom)
        observed = self.outcome()
        self.assertEqual(observed["result"], "failed")
        self.assertIn("no data", observed["reason"])

    def test_malformed_results_are_failed(self):
        for bad in (None, "passed", {"result": "ok", "reason": "x"}, {"result": "passed"}, {"result": "passed", "reason": "  "},
                    {"reason": "x"}, {"result": "pass", "reason": "x"}):
            with self.subTest(bad=bad):
                registry.REGISTRY.clear()
                self.add("S2", lambda ctx, bad=bad: bad)
                self.assertEqual(self.outcome()["result"], "failed")

    def test_validator_cannot_rewrite_inventory_fields(self):
        for key in ("id", "kind", "title", "progress", "supporting_evidence"):
            with self.subTest(key=key):
                registry.REGISTRY.clear()
                self.add("S2", lambda ctx, key=key: {"result": "passed", "reason": "x", key: "forged"})
                observed = self.outcome()
                self.assertEqual(observed["result"], "failed")
                self.assertEqual(observed["id"], "S2")

    def test_core_result_cannot_override_result(self):
        with self.assertRaises(ValueError):
            core.result("failed", "x", result="passed")

    def test_unknown_or_duplicate_registration_is_rejected(self):
        with self.assertRaises(ValueError):
            self.add("S99", lambda ctx: None)
        self.add("S2", lambda ctx: core.result("passed", "ok"))
        with self.assertRaises(ValueError):
            self.add("S2", lambda ctx: core.result("passed", "other"))
        with self.assertRaises(ValueError):
            self.add("S3", lambda ctx: None, requires=("S99",))

    def test_execute_only_for_executable_validators(self):
        self.add("S2", lambda ctx: core.result("passed", "ok"))
        with self.assertRaises(ValueError):
            registry.evaluate(rows("S2"), execute=("S2",))
        with self.assertRaises(ValueError):
            registry.evaluate(rows("S3"), execute=("S3",))

    def test_requires_run_first_and_make_prerequisite_live(self):
        seen = []
        self.add("S3", lambda ctx: seen.append(("S3", ctx.live("S2"), ctx.results["S2"]["result"])) or core.result("failed", "x"), executable=True, requires=("S2",))
        self.add("S2", lambda ctx: seen.append(("S2", ctx.live())) or core.result("failed", "y"), executable=True)
        registry.evaluate(rows("S3", "S2"), execute=("S3",))
        self.assertEqual(seen, [("S2", True), ("S3", True, "failed")])
        seen.clear()
        registry.evaluate(rows("S3", "S2"))
        self.assertEqual(seen, [("S2", False), ("S3", False, "failed")])

    def test_dependency_cycle_is_an_error(self):
        registry.REGISTRY["S2"] = Validator("S2", lambda ctx: None, False, ("S3",), "t")
        registry.REGISTRY["S3"] = Validator("S3", lambda ctx: None, False, ("S2",), "t")
        with self.assertRaises(ValueError):
            registry.evaluate(rows("S2", "S3"))


class RealRegistryTests(unittest.TestCase):
    def test_every_registered_id_is_a_contract_obligation_and_modules_are_discoverable(self):
        loaded = registry.load_all()
        self.assertTrue({"S1", "S10", "S11", "G1"} <= set(loaded))
        self.assertTrue(set(loaded) <= set(registry.known_ids()))
        self.assertEqual(set(registry.executable_ids()), {rid for rid, v in loaded.items() if v.executable})
        self.assertIn("S10", registry.executable_ids())
        self.assertEqual(registry.order(["S10", "S1"]), ["S1", "S10"])
