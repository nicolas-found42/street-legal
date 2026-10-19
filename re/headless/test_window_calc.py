"""Explicit identity-RECT default processing and bounded next-message delivery."""

import struct
import sys
import unittest
from dataclasses import FrozenInstanceError
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from startup import REPO, Boundary, Startup
from test_window_messages import desktop

EXE = REPO / "reference/raw/StreetLegal_Redline.bak.exe"


class CalculationProfileTests(unittest.TestCase):
    def test_explicit_calc_inputs_are_immutable_and_optional(self):
        old = {"default_nccreate_result": 1, "after_nccreate": "nccalcsize_rect"}
        self.assertEqual(desktop(creation=old).creation.metadata, old)
        calc = {"rect_output": "identity", "default_result": 0, "after": "wm_create"}
        profile = desktop(creation={**old, "nccalcsize": calc})
        self.assertEqual(profile.creation.metadata, {**old, "nccalcsize": calc})
        calc["after"] = "stop"
        self.assertEqual(profile.creation.nccalcsize.after, "wm_create")
        with self.assertRaises(FrozenInstanceError):
            profile.creation.nccalcsize.after = "stop"
        for value in (
            None,
            {},
            {"rect_output": "identity", "default_result": False, "after": "stop"},
            {"rect_output": "identity", "default_result": 1, "after": "stop"},
            {"rect_output": "shrink", "default_result": 0, "after": "stop"},
            {"rect_output": "identity", "default_result": 0, "after": "show"},
            {
                "rect_output": "identity",
                "default_result": 0,
                "after": "stop",
                "extra": 1,
            },
        ):
            with self.subTest(value=value), self.assertRaises(ValueError):
                desktop(creation={**old, "nccalcsize": value})
        with self.assertRaises(ValueError):
            desktop(
                creation={
                    **old,
                    "after_nccreate": "stop",
                    "nccalcsize": {
                        "rect_output": "identity",
                        "default_result": 0,
                        "after": "stop",
                    },
                }
            )


@unittest.skipUnless(EXE.exists(), "read-only original absent")
class IdentityRectangleTests(unittest.TestCase):
    def scenario(self, result=1, prefix=b"", after="stop"):
        data = {
            "default_nccreate_result": result,
            "after_nccreate": "nccalcsize_rect",
            "nccalcsize": {
                "rect_output": "identity",
                "default_result": 0,
                "after": after,
            },
        }
        run = Startup(EXE, desktop_profile=desktop(creation=data))
        address = lambda name: next(
            a for a, n in run.import_names.items() if n == "user32.dll!" + name
        )
        name = run.alloc(32)
        run.write(name, b"UnitCalcWindow\0")
        procedure = run.alloc(128)
        run.write(
            procedure,
            prefix
            + b"\xff\x74\x24\x10" * 4
            + b"\xb8"
            + struct.pack("<I", address("DefWindowProcA"))
            + b"\xff\xd0\xc2\x10\x00",
        )
        block = run.alloc(40)
        run.write_u32s(block, [0, procedure, 0, 0, run.base, 0, 0, 0, 0, name])
        run.call(address("RegisterClassA"), [block], limit=100)
        args = [8, name, 0, 0x90000000, 3, 0xFFFFFFFE, 10, 12, 0, 0, run.base, 0x1234]
        return run, address, args

    def test_identity_rect_writes_exact_words_and_returns_zero_through_callback(self):
        run, address, args = self.scenario()
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=300, regs={"eax": 123})
        self.assertEqual(error.exception.facts["phase"], "after_WM_NCCALCSIZE")
        self.assertEqual(error.exception.facts["callback_result"], 0)
        self.assertEqual(error.exception.facts["rect_words"], [3, 0xFFFFFFFE, 13, 10])
        event = [
            e for e in run.events if e.get("import") == "user32.dll!DefWindowProcA"
        ][-1]
        self.assertEqual(event["result"], 0)
        self.assertEqual(event["desktop"]["input_rect"], [3, 0xFFFFFFFE, 13, 10])
        self.assertEqual(event["desktop"]["output_rect"], [3, 0xFFFFFFFE, 13, 10])
        self.assertEqual(
            event["desktop"]["output_write"], {"address": event["args"][3], "bytes": 16}
        )
        self.assertEqual(run.reg("eax"), 123)

    def test_declared_next_create_reuses_create_struct_and_stops_before_default_result(
        self,
    ):
        run, address, args = self.scenario(after="wm_create")
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=400, regs={"eax": 123})
        self.assertEqual(
            error.exception.facts["kind"], "unsupported_default_window_message"
        )
        self.assertEqual(error.exception.facts["message"], 1)
        self.assertFalse(error.exception.facts["default_result_supplied"])
        callbacks = [e for e in run.events if "callback" in e]
        self.assertEqual(
            [e["callback"] for e in callbacks],
            ["WM_NCCREATE", "WM_NCCALCSIZE", "WM_CREATE"],
        )
        self.assertEqual(
            callbacks[-1]["args"], [callbacks[0]["hwnd"], 1, 0, callbacks[0]["args"][3]]
        )
        self.assertEqual(
            run.events[-1]["desktop"]["create_struct_words"],
            [0x1234, run.base, 0, 0, 12, 10, 0xFFFFFFFE, 3, 0x90000000, 0, args[1], 8],
        )
        self.assertNotIn("result", run.events[-1])
        self.assertEqual(
            [e["callback_result"] for e in run.events if "callback_return" in e], [1, 0]
        )
        self.assertEqual(len(run.pending_window_callbacks), 1)
        create = next(
            e for e in run.events if e.get("import") == "user32.dll!CreateWindowExA"
        )
        self.assertNotIn("result", create)

    def test_identity_output_preserves_adjacent_words(self):
        # Only the NCCALCSIZE branch writes independent guards beyond its RECT.
        body = bytes.fromhex("8b542410c74210efbeaddec74214bebafeca")
        prefix = bytes.fromhex("817c24088300000075") + bytes([len(body)]) + body
        run, address, args = self.scenario(prefix=prefix)
        with self.assertRaises(Boundary):
            run.call(address("CreateWindowExA"), args, limit=400)
        calls = [
            e for e in run.events if e.get("import") == "user32.dll!DefWindowProcA"
        ]
        pointer = calls[-1]["args"][3]
        self.assertEqual(
            run.read_u32s(pointer, 6), [3, 0xFFFFFFFE, 13, 10, 0xDEADBEEF, 0xCAFEBABE]
        )
        self.assertEqual(
            run.read_u32s(calls[0]["args"][3], 12),
            [0x1234, run.base, 0, 0, 12, 10, 0xFFFFFFFE, 3, 0x90000000, 0, args[1], 8],
        )

    def test_changed_wparam_is_rejected_before_rectangle_write_or_result(self):
        body = bytes.fromhex("c744240c01000000")
        prefix = bytes.fromhex("817c24088300000075") + bytes([len(body)]) + body
        run, address, args = self.scenario(prefix=prefix)
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=400)
        self.assertEqual(
            error.exception.facts["kind"], "unsupported_default_proc_context"
        )
        self.assertEqual(run.events[-1]["args"][1:3], [0x83, 1])
        self.assertIsNone(run.events[-1]["desktop"]["output_write"])
        self.assertNotIn("result", run.events[-1])

    def test_callback_refusal_and_nonzero_calc_return_cannot_deliver_create(self):
        run, address, args = self.scenario(result=0, after="wm_create")
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=300)
        self.assertEqual(error.exception.facts["phase"], "after_WM_NCCREATE_refusal")
        self.assertEqual(
            [e["callback"] for e in run.events if "callback" in e], ["WM_NCCREATE"]
        )
        body = bytes.fromhex("b855000000c21000")
        prefix = bytes.fromhex("817c24088300000075") + bytes([len(body)]) + body
        run, address, args = self.scenario(prefix=prefix, after="wm_create")
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=400)
        self.assertEqual(error.exception.facts["phase"], "after_WM_NCCALCSIZE")
        self.assertEqual(error.exception.facts["callback_result"], 0x55)
        self.assertEqual(
            [e["callback"] for e in run.events if "callback" in e],
            ["WM_NCCREATE", "WM_NCCALCSIZE"],
        )

    def test_application_create_return_stops_before_hwnd_or_refusal_cleanup(self):
        for result in (0, 0xFFFFFFFF):
            body = b"\xb8" + struct.pack("<I", result) + bytes.fromhex("c21000")
            prefix = bytes.fromhex("817c24080100000075") + bytes([len(body)]) + body
            run, address, args = self.scenario(prefix=prefix, after="wm_create")
            with self.assertRaises(Boundary) as error:
                run.call(address("CreateWindowExA"), args, limit=400, regs={"eax": 123})
            self.assertEqual(error.exception.facts["phase"], "after_WM_CREATE")
            self.assertEqual(error.exception.facts["callback_result"], result)
            self.assertFalse(error.exception.facts["win32_result_supplied"])
            self.assertEqual(run.events[-1]["callback_return"], "WM_CREATE")
            self.assertEqual(run.reg("eax"), 123)
            create = next(
                e for e in run.events if e.get("import") == "user32.dll!CreateWindowExA"
            )
            self.assertNotIn("result", create)
