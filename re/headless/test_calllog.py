"""SLRLOG emitter byte-compatibility with docs/call-log.md and the Rust reader."""

import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import calllog

REPO = Path(__file__).resolve().parents[2]


class EncodingTests(unittest.TestCase):
    def test_header_and_documented_native_example_are_byte_exact(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "x.slrlog"
            writer = calllog.Writer(path)
            writer.record(calllog.native_call("java.game.GameType", "addCall", [1], void=True))
            writer.close()
            data = path.read_bytes()
        # docs/call-log.md section 5 builds exactly this record by hand
        body = struct.pack("<I", 18) + b"java.game.GameType" + struct.pack("<I", 7) + b"addCall"
        body += struct.pack("<I", 1) + b"\x00" + struct.pack("<i", 1) + b"\x00"
        assert data == b"SLRLOG" + struct.pack("<H", 1) + b"\x00" + struct.pack("<I", len(body)) + body

    def test_empty_log_is_eight_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "empty.slrlog"
            calllog.Writer(path).close()
            self.assertEqual(path.read_bytes(), b"SLRLOG\x01\x00")

    def test_floats_are_bit_patterns_and_words_become_signed_ints(self):
        self.assertEqual(calllog.encode_value(calllog.Float32(5.0)), b"\x02\x00\x00\xa0\x40")
        negative_zero = calllog.Float32(bits=0x80000000)
        self.assertEqual(calllog.encode_value(negative_zero), b"\x02\x00\x00\x00\x80")
        self.assertEqual(calllog.encode_value(0xFFFFFFFF), b"\x00\xff\xff\xff\xff")
        self.assertEqual(calllog.encode_value(True), b"\x06\x01")
        self.assertEqual(calllog.encode_value("é"), b"\x04\x02\x00\x00\x00\xc3\xa9")
        self.assertEqual(calllog.encode_value(b"ab"), b"\x05\x02\x00\x00\x00ab")
        with self.assertRaises(ValueError):
            calllog.encode_value(1 << 32)
        with self.assertRaises(ValueError):
            calllog.encode_values([0] * 65)

    def test_roundtrip_of_every_family_the_oracle_writes(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "all.slrlog"
            writer = calllog.Writer(path)
            writer.record(calllog.native_call("c", "n", [1, calllog.Float32(2.0)], 3))
            writer.record(calllog.d3d9_call(1, "DrawPrimitive", [4, 0, 1]))
            writer.record(calllog.dsound_call(0, "Play", [0, 0, 1]))
            writer.record(calllog.dinput_poll(0, bytes(256)))
            writer.record(calllog.file_io(0, "system.rpk", [1, 2]))
            writer.close()
            records = calllog.Reader(path.read_bytes()).records()
        self.assertEqual(records[0][:3], ("native", "c", "n"))
        self.assertEqual(records[0][4], ("int", 3))
        self.assertEqual(records[1], ("d3d9", 1, "DrawPrimitive", [("int", 4), ("int", 0), ("int", 1)]))
        self.assertEqual(records[3][2], bytes(256))
        self.assertEqual(records[4][2], "system.rpk")

    def test_reader_rejects_truncation_and_unknown_tags(self):
        good = b"SLRLOG\x01\x00" + b"\x09" + struct.pack("<I", 0)
        with self.assertRaises(ValueError):
            calllog.Reader(good).records()
        with self.assertRaises(ValueError):
            calllog.Reader(b"SLRLOG\x01\x00\x00\x05\x00\x00\x00ab").records()
        with self.assertRaises(ValueError):
            calllog.Reader(b"BADLOG\x01\x00")


def rust_inspector():
    for profile in ("debug", "release"):
        candidate = REPO / "target" / profile / "slr-calllog-inspect"
        if candidate.exists():
            return candidate
    return None


@unittest.skipUnless(rust_inspector(), "slr-calllog-inspect not built locally")
class RustReaderTests(unittest.TestCase):
    def test_python_emitted_log_is_accepted_by_the_rust_inspector(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "rust.slrlog"
            writer = calllog.Writer(path)
            writer.record(calllog.native_call("java.lang.System", "currentTime", [], 5))
            writer.record(calllog.native_call("java.sound.Sound", "setVolume", [0, calllog.Float32(1.0)], void=True))
            writer.record(calllog.d3d9_call(0, "SetRenderState", [7, 1]))
            writer.record(calllog.dsound_call(1, "IDirectSound3DBuffer.SetPosition", [calllog.Float32(1.0)]))
            writer.record(calllog.dinput_poll(1, bytes(16)))
            writer.record(calllog.file_io(0, "a.rpk", [0]))
            writer.close()
            result = subprocess.run([str(rust_inspector()), str(path)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("currentTime", result.stdout)
        self.assertIn("SetRenderState", result.stdout)


if __name__ == "__main__":
    unittest.main()
