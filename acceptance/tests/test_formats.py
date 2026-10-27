from tests.support import *
import copy
from unittest import mock
import formats

class FormatsTests(unittest.TestCase):
    def census(self):
        return {"schema": 1, "files": 2, "bytes": 12, "unexplained": [], "decoders": {"a": {"files": 2, "bytes": 12, "class": "game", "units": {"records": 5}}}}

    def test_exact_census_and_every_changed_dimension(self):
        golden = self.census()
        self.assertEqual(formats.census_difference(golden, copy.deepcopy(golden)), [])
        variants = []
        for field, value in [("schema", 2), ("files", 3), ("bytes", 13), ("unexplained", [{"path": "x"}])]:
            v = copy.deepcopy(golden); v[field] = value; variants.append(v)
        for field, value in [("files", 1), ("bytes", 13), ("class", "documentation"), ("units", {"records": 4})]:
            v = copy.deepcopy(golden); v["decoders"]["a"][field] = value; variants.append(v)
        v = copy.deepcopy(golden); v["decoders"] = {}; variants.append(v)
        for v in variants:
            with self.subTest(v=v):
                self.assertTrue(formats.census_difference(golden, v))

    def test_unknown_audio_loaders_remain_gaps(self):
        for key in ("wav", "mp3"):
            self.assertIn(key, formats.G2_LOADER_GAPS)
            self.assertNotIn(key, formats.G2_NO_EXE_LOADER)

    def test_failed_build_produces_retained_failure(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            tool = mock.Mock()
            tool.git_identity.return_value = ({"head": "abc"}, b"")
            with mock.patch.object(formats, "boot_tools", return_value=tool), mock.patch.object(formats, "stock_identity", return_value={"files": []}), mock.patch.object(formats.subprocess, "run", return_value=mock.Mock(returncode=101)):
                verdict = formats.formats_result(root)
            self.assertEqual(verdict["result"], "failed")
            receipt = json.loads((Path(verdict["bundle"]) / "receipt.json").read_text())
            self.assertEqual(receipt["commands"][0]["returncode"], 101)
