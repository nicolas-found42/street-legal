from tests.support import *
from validators import core
from observations import rust_test_observation


class ObservationTests(unittest.TestCase):
    OK = "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s"

    def test_component_success_stays_component_evidence(self):
        self.assertTrue(rust_test_observation(0, self.OK, "")["valid_component_observation"])

    def test_execution_status_cannot_override_checked_validator_result(self):
        for execution_status in ("completed", "failed", "passing"):
            observed = core.result("failed", "native streams differ", status=execution_status)
            self.assertEqual(observed["result"], "failed")
            self.assertEqual(observed["status"], execution_status)
        with self.assertRaises(ValueError):
            core.result("failed", "native streams differ", result="passed")

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



