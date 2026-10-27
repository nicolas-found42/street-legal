"""Data-free checks of the input oracle's result encoding."""
import struct
import unittest
from types import SimpleNamespace

from input_device_rig import Queue, Rig


class FloatResultEncodingTests(unittest.TestCase):
    def test_float_result_preserves_the_raw_zero_sign(self):
        for significand, exponent, expected in (
            (0, 0, 0),
            (0, 0x8000, 0x80000000),
            (1 << 63, 0x3FFF, 0x3F800000),
            (1 << 63, 0xBFFF, 0xBF800000),
        ):
            with self.subTest(exponent=exponent):
                raw = struct.pack("<QH", significand, exponent)
                rig = Rig.__new__(Rig)
                rig.o = SimpleNamespace(icount=0, call=lambda *_: SimpleNamespace(st0_raw=raw))
                rig.answers = Queue()
                rig.diff = lambda: []
                row = rig.run({"op": "axis", "args": [2, 20]})
                self.assertEqual(row["st0_raw"], raw.hex())
                self.assertEqual(row["st0_f32"], expected)


if __name__ == "__main__":
    unittest.main()
