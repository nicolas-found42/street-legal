"""Declared CREATE results and original visible-notification callback boundary."""

import struct
import sys
import unittest
from dataclasses import FrozenInstanceError
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from startup import REPO, Boundary, Startup
from test_window_messages import desktop

EXE = REPO / "reference/raw/StreetLegal_Redline.bak.exe"


def creation(result=0, after="stop"):
    return {
        "default_nccreate_result": 1,
        "after_nccreate": "nccalcsize_rect",
        "nccalcsize": {
            "rect_output": "identity",
            "default_result": 0,
            "after": "wm_create",
        },
        "wm_create": {"default_result": result, "after": after},
    }


class CreateProfileTests(unittest.TestCase):
    def test_signed_create_result_and_next_choice_are_explicit_immutable_and_optional(
        self,
    ):
        data = creation(-1, "wm_showwindow")
        profile = desktop(creation=data)
        self.assertEqual(profile.creation.metadata, data)
        data["wm_create"]["default_result"] = 0
        self.assertEqual(profile.creation.wm_create.default_result, -1)
        with self.assertRaises(FrozenInstanceError):
            profile.creation.wm_create.after = "stop"
        old = creation()
        del old["wm_create"]
        self.assertEqual(desktop(creation=old).creation.metadata, old)
        for value in (
            None,
            {},
            {"default_result": False, "after": "stop"},
            {"default_result": 1, "after": "stop"},
            {"default_result": 4294967295, "after": "stop"},
            {"default_result": 0, "after": "hwnd"},
            {"default_result": 0, "after": "stop", "extra": 1},
        ):
            bad = creation()
            bad["wm_create"] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                desktop(creation=bad)
        for calc in (
            None,
            {"rect_output": "identity", "default_result": 0, "after": "stop"},
        ):
            bad = creation()
            bad["nccalcsize"] = calc
            with self.assertRaises(ValueError):
                desktop(creation=bad)


@unittest.skipUnless(EXE.exists(), "read-only original absent")
class VisibleMessageTests(unittest.TestCase):
    def scenario(self, result=0, after="stop", prefix=b"", style=0x90000000):
        run = Startup(EXE, desktop_profile=desktop(creation=creation(result, after)))
        address = lambda name: next(
            a for a, n in run.import_names.items() if n == "user32.dll!" + name
        )
        name = run.alloc(32)
        run.write(name, b"UnitVisibleWindow\0")
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
        return (
            run,
            address,
            [8, name, 0, style, 3, 0xFFFFFFFE, 10, 12, 0, 0, run.base, 0x1234],
        )

    def test_declared_zero_and_minus_one_flow_through_callback_then_stop_without_api_result(
        self,
    ):
        for result, raw in ((0, 0), (-1, 0xFFFFFFFF)):
            run, address, args = self.scenario(result)
            with self.assertRaises(Boundary) as error:
                run.call(address("CreateWindowExA"), args, limit=500, regs={"eax": 123})
            self.assertEqual(error.exception.facts["phase"], "after_WM_CREATE")
            self.assertEqual(error.exception.facts["callback_result"], raw)
            self.assertFalse(error.exception.facts["win32_result_supplied"])
            calls = [
                e for e in run.events if e.get("import") == "user32.dll!DefWindowProcA"
            ]
            self.assertEqual(calls[-1]["result"], raw)
            self.assertEqual(calls[-1]["desktop"]["declared_lresult"], raw)
            self.assertEqual(
                calls[-1]["desktop"]["create_struct_words"],
                [
                    0x1234,
                    run.base,
                    0,
                    0,
                    12,
                    10,
                    0xFFFFFFFE,
                    3,
                    0x90000000,
                    0,
                    args[1],
                    8,
                ],
            )
            self.assertIsNone(calls[-1]["desktop"]["output_write"])
            self.assertEqual(run.events[-1]["callback_return"], "WM_CREATE")
            self.assertEqual(run.reg("eax"), 123)
            self.assertNotIn(
                "result",
                next(
                    e
                    for e in run.events
                    if e.get("import") == "user32.dll!CreateWindowExA"
                ),
            )

    def test_acceptance_delivers_visible_notification_but_refusal_does_not(self):
        run, address, args = self.scenario(after="wm_showwindow")
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=600)
        self.assertEqual(
            error.exception.facts["kind"], "unsupported_default_window_message"
        )
        self.assertEqual(error.exception.facts["message"], 0x18)
        self.assertFalse(error.exception.facts["default_result_supplied"])
        callbacks = [e for e in run.events if "callback" in e]
        self.assertEqual(
            [e["callback"] for e in callbacks],
            ["WM_NCCREATE", "WM_NCCALCSIZE", "WM_CREATE", "WM_SHOWWINDOW"],
        )
        self.assertEqual(callbacks[-1]["args"], [callbacks[0]["hwnd"], 0x18, 1, 0])
        self.assertNotIn("result", run.events[-1])
        self.assertIsNone(run.events[-1]["desktop"]["output_write"])
        self.assertEqual(len(run.pending_window_callbacks), 1)
        run, address, args = self.scenario(-1, "wm_showwindow")
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=600)
        self.assertEqual(error.exception.facts["phase"], "after_WM_CREATE")
        self.assertEqual(error.exception.facts["callback_result"], 0xFFFFFFFF)
        self.assertFalse(any(e.get("callback") == "WM_SHOWWINDOW" for e in run.events))

    def test_invisible_popup_cannot_take_explicit_visible_delivery_path(self):
        run, address, args = self.scenario(after="wm_showwindow", style=0x80000000)
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=600)
        self.assertEqual(
            error.exception.facts["kind"], "unsupported_visible_notification"
        )
        self.assertEqual(error.exception.facts["style"], 0x80000000)
        self.assertFalse(any(e.get("callback") == "WM_SHOWWINDOW" for e in run.events))

    def test_changed_create_structure_stops_before_declared_result(self):
        body = bytes.fromhex("8b442410c70009000000")
        prefix = bytes.fromhex("817c24080100000075") + bytes([len(body)]) + body
        run, address, args = self.scenario(prefix=prefix)
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=600)
        self.assertEqual(error.exception.facts["kind"], "unsupported_create_struct")
        self.assertEqual(error.exception.facts["words"][0], 9)
        self.assertNotIn("result", run.events[-1])

    def test_application_notification_result_stops_before_any_creation_completion(self):
        body = bytes.fromhex("b807000000c21000")
        prefix = bytes.fromhex("817c24081800000075") + bytes([len(body)]) + body
        run, address, args = self.scenario(after="wm_showwindow", prefix=prefix)
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=600, regs={"eax": 123})
        self.assertEqual(error.exception.facts["phase"], "after_WM_SHOWWINDOW")
        self.assertEqual(error.exception.facts["callback_result"], 7)
        self.assertFalse(error.exception.facts["win32_result_supplied"])
        self.assertEqual(run.reg("eax"), 123)
        self.assertEqual(run.pending_window_callbacks, {})


if __name__ == "__main__":
    unittest.main()
