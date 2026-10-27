"""check.py's symlink-escape rejection holds, except for the declared evidence root."""
import os
import check
from tests.support import *


def git(*args, cwd):
    subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True,
        env={**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t", "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"})


class EvidenceRootTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp()).resolve()
        self.addCleanup(lambda: __import__("shutil").rmtree(self.tmp, ignore_errors=True))
        self.main = self.tmp / "main"
        self.main.mkdir()
        git("init", "-q", cwd=self.main)
        (self.main / "f").write_text("x")
        git("add", "f", cwd=self.main)
        git("commit", "-qm", "init", cwd=self.main)
        git("worktree", "add", "-q", str(self.tmp / "wt"), "-b", "wt", cwd=self.main)
        self.wt = self.tmp / "wt"
        (self.main / "re/out/run").mkdir(parents=True)
        (self.main / "re/out/run/a.json").write_text("{}")
        (self.main / "stock").mkdir()
        (self.main / "stock/.slr-manifest").write_text("m")
        (self.main / "docs").mkdir()
        (self.main / "docs/secret.md").write_text("s")
        (self.wt / "re/out").mkdir(parents=True)
        (self.wt / "re/out/run").symlink_to(self.main / "re/out/run")
        (self.wt / "stock").symlink_to(self.main / "stock")
        (self.wt / "docs").mkdir()
        self.outside = self.tmp / "outside"
        self.outside.mkdir()
        (self.outside / "x.json").write_text("{}")
        self.patcher = patch.object(check, "ROOT", self.wt)
        self.patcher.start()
        self.addCleanup(self.patcher.stop)

    def test_default_rejects_every_symlink_escape(self):
        for entry in ("re/out/run/a.json", "stock/.slr-manifest"):
            with self.subTest(entry=entry), self.assertRaises(ValueError):
                check.local_file(entry)

    def test_declared_main_checkout_admits_linked_prefixes_only(self):
        root = check.evidence_root(self.main)
        self.assertEqual(check.local_file("re/out/run/a.json", root), self.wt / "re/out/run/a.json")
        self.assertEqual(check.local_file("stock/.slr-manifest", root), self.wt / "stock/.slr-manifest")

    def test_symlink_outside_linked_prefix_is_still_rejected(self):
        (self.wt / "docs/secret.md").symlink_to(self.main / "docs/secret.md")
        with self.assertRaises(ValueError):
            check.local_file("docs/secret.md", check.evidence_root(self.main))

    def test_symlink_leaving_the_evidence_root_is_rejected(self):
        (self.wt / "re/out/escape").symlink_to(self.outside)
        with self.assertRaises(ValueError):
            check.local_file("re/out/escape/x.json", check.evidence_root(self.main))
        # Same prefix of a different part of the main checkout is also not enough.
        (self.wt / "re/out/other").symlink_to(self.main / "docs")
        with self.assertRaises(ValueError):
            check.local_file("re/out/other/secret.md", check.evidence_root(self.main))

    def test_chained_main_symlink_escape_is_rejected(self):
        (self.main / "re/out/hop").symlink_to(self.outside)
        (self.wt / "re/out/hop").symlink_to(self.main / "re/out/hop")
        with self.assertRaises(ValueError):
            check.local_file("re/out/hop/x.json", check.evidence_root(self.main))

    def test_evidence_root_must_be_a_checkout_of_this_repository(self):
        other = self.tmp / "other"
        other.mkdir()
        git("init", "-q", cwd=other)
        for bad in (other, self.outside, self.tmp / "missing", self.wt):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                check.evidence_root(bad)

    def test_in_repo_evidence_through_symlink_stays_rejected_even_with_root(self):
        (self.wt / "docs/link.md").symlink_to(self.outside / "x.json")
        with self.assertRaises(ValueError):
            check.local_file("docs/link.md", check.evidence_root(self.main))

    def test_missing_or_traversal_still_rejected_with_root(self):
        root = check.evidence_root(self.main)
        for entry in ("re/out/run/missing.json", "../main/f", "/etc/passwd", "re/out/run"):
            with self.subTest(entry=entry), self.assertRaises(ValueError):
                check.local_file(entry, root)

    def test_matrix_cannot_declare_an_evidence_root(self):
        (self.wt / "docs/fidelity.md").write_text((ROOT / "docs/fidelity.md").read_text())
        data = copy.deepcopy(BASE)
        data["evidence_root"] = str(self.main)
        data["rows"][0]["evidence"] = ["re/out/run/a.json"]
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "m.json"
            path.write_text(json.dumps(data))
            _, problems, absent = check.load(path)
        self.assertTrue(absent, "matrix-declared root must not authorize symlinked evidence")
