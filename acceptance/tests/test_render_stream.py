"""Data-free adversarial tests of the G6/S5 verdict logic (imported by selftest.py)."""
from __future__ import annotations

import json
import tempfile
import unittest
import subprocess
import copy
from unittest.mock import patch
import check as acceptance_check
from tests.support import BASE
from pathlib import Path

import render_stream as rs

DIGEST = rs.MENU_IDLE_1800M_STREAM_SHA256


def report(equal=True, compared=1, equal_frames=1, frames=None):
    return {"schema": rs.CANON_SCHEMA, "kind": "comparison", "equal": equal, "frames_compared": compared,
            "frames_equal": equal_frames, "frames": frames or []}


def diverged_report():
    return report(False, 1, 0, [{"pair": 0, "status": "diverged", "first_divergence": {
        "event_index": 0, "expected_event": "clear", "got_event": "begin_scene",
        "differences": [{"field": "state.rs.27"}]}}])


def receipt(**overrides):
    value = {"schema": "slr-render-review-1", "scene": "menu-idle", "reviewer": "A. Person",
             "port_png_sha256": "ab" * 32, "verdict": "match",
             "original_reference": {"sha256": "cd" * 32, "provenance": "screenshot of the real original"}}
    value.update(overrides)
    return value


class SceneVerdictTests(unittest.TestCase):
    def test_equal_requires_every_condition(self):
        self.assertEqual(rs.scene_verdict("s", report(), DIGEST, DIGEST)["status"], "equal")
        self.assertEqual(rs.scene_verdict("s", report(equal=False), DIGEST, DIGEST)["status"], "diverged")
        self.assertEqual(rs.scene_verdict("s", report(compared=0, equal_frames=0), DIGEST, DIGEST)["status"], "invalid")
        self.assertEqual(rs.scene_verdict("s", report(equal_frames=0), DIGEST, DIGEST)["status"], "diverged")

    def test_a_moved_oracle_digest_is_invalid_even_if_streams_agree(self):
        self.assertEqual(rs.scene_verdict("s", report(), "0" * 64, DIGEST)["status"], "invalid")

    def test_unknown_schema_is_invalid(self):
        self.assertEqual(rs.scene_verdict("s", {"schema": "x"}, DIGEST, DIGEST)["status"], "invalid")

    def test_divergence_summary_names_the_first_difference(self):
        verdict = rs.scene_verdict("s", diverged_report(), DIGEST, DIGEST)
        self.assertEqual(verdict["first_divergence"]["expected_event"], "clear")
        self.assertEqual(verdict["first_divergence"]["differences"][0]["field"], "state.rs.27")


class ReviewReceiptTests(unittest.TestCase):
    def test_valid_receipt_binds_scene_image_and_reference(self):
        self.assertIsNone(rs.review_receipt_problem(receipt(), "menu-idle", "ab" * 32))

    def test_receipts_that_do_not_bind_this_run_are_rejected(self):
        for bad, why in ((receipt(scene="garage"), "scene"), (receipt(port_png_sha256="00" * 32), "image"),
                         (receipt(reviewer=" "), "reviewer"), (receipt(verdict="mismatch"), "verdict"),
                         (receipt(original_reference={}), "reference"), ("not a receipt", "shape")):
            with self.subTest(why=why):
                self.assertIsNotNone(rs.review_receipt_problem(bad, "menu-idle", "ab" * 32))

    def test_review_status_reads_only_the_declared_location(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.assertEqual(rs.review_status("menu-idle", "ab" * 32, root)["status"], "missing")
            (root / "menu-idle.json").write_text(json.dumps(receipt()))
            self.assertEqual(rs.review_status("menu-idle", "ab" * 32, root)["status"], "present")
            self.assertEqual(rs.review_status("menu-idle", "00" * 32, root)["status"], "invalid")
            (root / "menu-idle.json").write_text("{")
            self.assertEqual(rs.review_status("menu-idle", "ab" * 32, root)["status"], "invalid")
            self.assertEqual(rs.review_status("menu-idle", None, root)["status"], "no_port_image")


class CombineTests(unittest.TestCase):
    def verdicts(self, status="equal"):
        return [{"scene": "menu-idle", "status": status}]

    def test_a_failed_harness_fails_both_rows(self):
        out = rs.combine([], [], {"ok": False, "problem": "boom"})
        self.assertEqual((out["G6"]["result"], out["S5"]["result"]), ("failed", "failed"))

    def test_divergence_fails_g6_and_leaves_s5_partial(self):
        out = rs.combine(self.verdicts("diverged"), [{"scene": "menu-idle", "status": "missing"}], {"ok": True})
        self.assertEqual(out["G6"]["result"], "failed")
        self.assertEqual(out["S5"]["result"], "partial")

    def test_equal_streams_alone_cannot_pass_g6(self):
        out = rs.combine(self.verdicts(), [{"scene": "menu-idle", "status": "missing"}], {"ok": True})
        self.assertNotEqual(out["G6"]["result"], "passed")
        self.assertEqual(out["G6"]["result"], "blocked")
        self.assertIn("no valid human screenshot review", out["G6"]["reason"])

    def test_blocked_scenes_keep_g6_from_passing_even_with_reviews(self):
        out = rs.combine(self.verdicts(), [{"scene": "menu-idle", "status": "present"}], {"ok": True})
        self.assertNotEqual(out["G6"]["result"], "passed")
        self.assertIn("garage", out["G6"]["reason"])

    def test_invalid_evidence_fails_both(self):
        out = rs.combine(self.verdicts("invalid") and [{"scene": "s", "status": "invalid", "reason": "x"}], [], {"ok": True})
        self.assertEqual((out["G6"]["result"], out["S5"]["result"]), ("failed", "failed"))

    def test_s5_is_never_passed_without_original_screenshots(self):
        out = rs.combine(self.verdicts(), [{"scene": "menu-idle", "status": "present"}], {"ok": True})
        self.assertEqual(out["S5"]["result"], "partial")

    def test_not_run_is_not_a_pass(self):
        self.assertEqual(rs.not_run("G6")["result"], "not_run")


if __name__ == "__main__":
    unittest.main()


class RegistryRenderTests(unittest.TestCase):
    def test_g6_executes_one_live_s5_and_cannot_promote_partial_support(self):
        from validators.rows import s5
        observed = {"S5": {"result": "partial", "reason": "bounded test observation"},
                    "G6": {"result": "passed", "reason": "adversarial child claim"}}
        with tempfile.TemporaryDirectory() as tmp, patch.object(s5, "run_render", return_value=observed) as run:
            rows = acceptance_check.evaluate(copy.deepcopy(BASE["rows"]), execute=["G6"], output_root=Path(tmp))
        self.assertEqual(run.call_count, 1)
        self.assertEqual(next(row for row in rows if row["id"] == "S5")["result"], "partial")
        self.assertEqual(next(row for row in rows if row["id"] == "G6")["result"], "blocked")

    def test_source_identity_detects_edits_while_dirty_paths_stay_the_same(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            source = root / "source.rs"
            source.write_text("base")
            subprocess.run(["git", "-C", str(root), "add", "source.rs"], check=True)
            subprocess.run(["git", "-C", str(root), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture"], check=True)
            with patch.object(rs, "ROOT", root):
                source.write_text("first edit")
                before = rs.git_identity()
                source.write_text("second edit")
                after = rs.git_identity()
            self.assertNotEqual(before, after)
