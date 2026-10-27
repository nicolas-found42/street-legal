"""Data-free output guards; never executes the original."""
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parent))
import dsound_capture as capture

class CaptureGuards(unittest.TestCase):
    def test_labels_cannot_escape(self):
        for label in ('../escape','/tmp/capture','a/b','','..','.' ): 
            with self.subTest(label=label),self.assertRaises(ValueError):capture.output_directory(label)
    def test_shared_headless_or_audio_directory_is_rejected(self):
        for child in ('headless','headless/audio'):
            with tempfile.TemporaryDirectory() as folder:
                root=Path(folder)/'repo';shared=Path(folder)/'shared';shared.mkdir();link=root/'re/out'/child;link.parent.mkdir(parents=True);link.symlink_to(shared)
                with patch.object(capture,'REPO',root),patch.object(capture,'OUT',root/'re/out/headless/audio'),self.assertRaisesRegex(ValueError,'inside this worktree'):capture.output_directory('run')
                self.assertEqual(list(shared.iterdir()),[])
    def test_nonempty_destination_cannot_be_reused(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder);out=root/'re/out/headless/audio';run=out/'run';run.mkdir(parents=True);(run/'proof').write_text('preserve')
            with patch.object(capture,'REPO',root),patch.object(capture,'OUT',out),self.assertRaisesRegex(ValueError,'nonempty'):capture.output_directory('run')
    def test_fresh_destination_is_worktree_local(self):
        with tempfile.TemporaryDirectory() as folder:
            root=Path(folder)
            with patch.object(capture,'REPO',root),patch.object(capture,'OUT',root/'re/out/headless/audio'):
                self.assertEqual(capture.output_directory('boot-2100M'),(root/'re/out/headless/audio/boot-2100M').resolve())
