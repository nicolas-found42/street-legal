"""The oracle must execute the signed operand the corpus actually requested."""
import pathlib
import struct
import subprocess
import tempfile
import unittest

import run_unicorn


def _c_encoder() -> str:
    """The real `put80` from the Rosetta probe, minus its x86 execution code."""
    source = pathlib.Path(__file__).with_name("rosetta").joinpath("probe.c").read_text()
    return source[source.index("static void put80("):source.index("static void fld_slot(")]


def _run_c(program: str) -> None:
    with tempfile.TemporaryDirectory() as directory:
        root = pathlib.Path(directory)
        (root / "encoding.c").write_text(program)
        subprocess.run(["cc", "-std=c99", "-Wall", "-Wextra", "-Werror",
                        str(root / "encoding.c"), "-o", str(root / "encoding")], check=True)
        subprocess.run([str(root / "encoding")], check=True)


class OperandEncodingTests(unittest.TestCase):
    def test_unicorn_zero_encoding_preserves_sign(self):
        for sign in (0, 1):
            encoded = run_unicorn.op_to_f80_bytes({"sign": sign, "exp": "zero", "sig": 0})
            self.assertEqual(encoded, struct.pack("<QH6x", 0, sign << 15))

    def test_loaded_negative_zero_stays_negative(self):
        for op in ("fld", "fst", "fsqrt"):
            for pc in (24, 53, 64):
                vector = {"id": 0, "op": op, "pc": pc, "rc": 0,
                          "a": {"sign": 1, "exp": "zero", "sig": 0}}
                result = run_unicorn.run_vector(run_unicorn.make_uc(pc, 0), vector)
                self.assertEqual(result["st"], {"sign": 1, "biased": 0, "sig": "0x0000000000000000"})

    def test_rosetta_encoder_preserves_zero_sign_on_host(self):
        # Compile the actual pure-C encoder without its x86 execution code, so
        # serialization is testable even when Rosetta/x87 hardware is absent.
        program = '#include <string.h>\n' + _c_encoder() + '''
int main(void) {
    unsigned char bytes[16];
    put80(bytes, 1, 1, 0, 0);
    if (bytes[9] != 0x80) return 1;
    for (int i = 0; i < 16; ++i) if (i != 9 && bytes[i] != 0) return 2;
    put80(bytes, 0, 1, 0, 0);
    for (int i = 0; i < 16; ++i) if (bytes[i] != 0) return 3;
    return 0;
}
'''
        _run_c(program)

    def test_rosetta_encoder_agrees_with_the_unicorn_encoder(self):
        """Every special class must serialize to the same 10-byte image.

        NaN is the class that matters most here: an encoder without a NaN
        branch silently produces a finite exponent (0x3FFF = 1.5), which makes
        a whole oracle leg vacuously agree on NaN rows.
        """
        operands = [
            {"sign": 0, "exp": "zero", "sig": 0},
            {"sign": 1, "exp": "zero", "sig": 0},
            {"sign": 0, "exp": "inf", "sig": 0x8000000000000000},
            {"sign": 1, "exp": "inf", "sig": 0x8000000000000000},
            {"sign": 0, "exp": "nan", "sig": 0xC000000000000000},
            {"sign": 1, "exp": "nan", "sig": 0xC000000000000000},
            {"sign": 0, "exp": "nan", "sig": 0x8000000000000001},
            {"sign": 0, "exp": 0, "sig": 0x8000000000000000},
            {"sign": 1, "exp": 10, "sig": 0xC000000000000000},
            {"sign": 0, "biased": 1, "sig": 0x4000000000000000},
        ]
        rows = []
        for o in operands:
            if "biased" in o:
                kind, exp = 0, o["biased"] - 16383
            elif isinstance(o["exp"], str):
                kind = {"zero": 1, "inf": 2, "nan": 3}[o["exp"]]
                exp = 0
            else:
                kind, exp = 0, o["exp"]
            rows.append((o, kind, exp))
        lines = ["#include <string.h>", _c_encoder(), "int main(void) {",
                 "    unsigned char b[16];"]
        for index, (o, kind, exp) in enumerate(rows):
            lines.append(
                f'    put80(b, {o["sign"]}, {kind}, {exp}, 0x{o["sig"]:016x}ULL);')
            expect = run_unicorn.op_to_f80_bytes(o)[:10]
            lines.append("    {")
            lines.append("        static const unsigned char want[10] = {"
                         + ", ".join(f"0x{b:02x}" for b in expect) + "};")
            lines.append("        if (memcmp(b, want, 10)) return %d;" % (index + 1))
            lines.append("    }")
        lines.append("    return 0;\n}")
        _run_c("\n".join(lines))

    def test_unicorn_encodes_nan_as_nan(self):
        encoded = run_unicorn.op_to_f80_bytes({"sign": 0, "exp": "nan", "sig": 0xC000000000000000})
        exponent = struct.unpack("<H", encoded[8:10])[0] & 0x7FFF
        self.assertEqual(exponent, 0x7FFF, "NaN operand must not get a finite exponent")


if __name__ == "__main__":
    unittest.main()
