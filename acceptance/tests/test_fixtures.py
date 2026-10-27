from tests.support import *
from validators import ledger


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
            self.assertEqual(set(ledger.fixture_sources(root)), expected)

    def test_required_original_inputs_cannot_be_omitted(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            required = self.populate_required_inputs(root)
            self.assertTrue(required <= set(ledger.fixture_sources(root)))
            for path in sorted(required):
                with self.subTest(path=path.relative_to(root).as_posix()):
                    contents = path.read_bytes()
                    path.unlink()
                    try:
                        with self.assertRaises((OSError, ValueError)):
                            ledger.fixture_sources(root)
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
                            ledger.fixture_sources(root)
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
            self.assertEqual(set(ledger.fixture_sources(root, excluded_roots=(output,))), required)



