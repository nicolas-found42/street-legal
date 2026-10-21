"""Evidence serialization must retain original floating-point payload words."""
import unittest
import json
from pathlib import Path
import tempfile

import particle_vectors


class DomainDumpTests(unittest.TestCase):
    def test_sphere_preserves_all_nan_radius_words(self):
        words = [0x5F113C, 0, 0, 0, 0x7FC01234, 0xFFC05678, 0x7FFFFFFF, 0xFFFFFFFF]

        class Memory:
            def read_u32(self, address):
                return words[(address - 0x1000) // 4]

        result = particle_vectors._dump_domain(Memory(), 0x1000)
        self.assertEqual(result, {"k": "sphere", "v": words[1:]})

    def test_legacy_normalized_vectors_cannot_verify(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "particle_parse.jsonl"
            path.write_text(json.dumps({"result": {}}) + "\n")
            with self.assertRaisesRegex(ValueError, "legacy normalized vectors"):
                particle_vectors.verify(out_dir=directory)

    def test_generation_preserves_existing_vectors(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "particle_parse.jsonl"
            path.write_bytes(b"historical evidence\n")
            with self.assertRaisesRegex(FileExistsError, "preserve existing vectors"):
                particle_vectors.gen(out_dir=directory)
            self.assertEqual(path.read_bytes(), b"historical evidence\n")


if __name__ == "__main__":
    unittest.main()
