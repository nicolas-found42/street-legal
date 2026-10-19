"""Explicit creation-continuation inputs and public Win32 callback behavior."""

import struct
import sys
import tempfile
import unittest
from dataclasses import FrozenInstanceError
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from desktop import DesktopProfile
from startup import REPO, Boundary, Startup
from window_messages import CreationProfile


def desktop(**changes):
    data = {
        "label": "hypothetical-message-unit",
        "resource_language": 1033,
        "cursor_size": [32, 32],
        "icon_size": [32, 32],
        "system_arrow_available": True,
        "black_brush_available": True,
    }
    data.update(changes)
    return DesktopProfile(data)


class CreationProfileTests(unittest.TestCase):
    def test_optional_creation_inputs_are_explicit_and_leave_old_profile_shape_unchanged(
        self,
    ):
        self.assertNotIn("creation", desktop().metadata)
        data = {"default_nccreate_result": 1, "after_nccreate": "nccalcsize_rect"}
        profile = desktop(creation=data)
        self.assertEqual(profile.metadata["creation"], data)
        data["default_nccreate_result"] = 0
        self.assertEqual(profile.creation.default_nccreate_result, 1)
        with self.assertRaises(FrozenInstanceError):
            profile.creation.default_nccreate_result = 0
        for value in (
            None,
            {},
            {"default_nccreate_result": True, "after_nccreate": "stop"},
            {"default_nccreate_result": 2, "after_nccreate": "stop"},
            {"default_nccreate_result": 1, "after_nccreate": "all"},
            {"default_nccreate_result": 1, "after_nccreate": "stop", "extra": 0},
        ):
            with self.subTest(value=value), self.assertRaises(ValueError):
                desktop(creation=value)
        self.assertEqual(
            CreationProfile(
                {"default_nccreate_result": 0, "after_nccreate": "stop"}
            ).default_nccreate_result,
            0,
        )

    def test_nested_creation_json_keys_cannot_override_prior_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "desktop.json"
            path.write_text(
                '{"creation":{"default_nccreate_result":0,"default_nccreate_result":1,"after_nccreate":"stop"}}'
            )
            with self.assertRaisesRegex(ValueError, "duplicate desktop profile key"):
                DesktopProfile.load(path)


EXE = REPO / "reference/raw/StreetLegal_Redline.bak.exe"


@unittest.skipUnless(EXE.exists(), "read-only original executable absent")
class DefaultWindowProcedureTests(unittest.TestCase):
    def scenario(self, result=1, next_phase="stop", prefix=b""):
        run = Startup(
            EXE,
            desktop_profile=desktop(
                creation={
                    "default_nccreate_result": result,
                    "after_nccreate": next_phase,
                }
            ),
        )
        address = lambda name: next(
            a for a, n in run.import_names.items() if n == "user32.dll!" + name
        )
        name = run.alloc(32)
        run.write(name, b"UnitMessageWindow\0")
        procedure = run.alloc(128)
        # Independent synthetic WndProc forwards all four stack arguments.
        code = (
            prefix
            + b"\xff\x74\x24\x10" * 4
            + b"\xb8"
            + struct.pack("<I", address("DefWindowProcA"))
            + b"\xff\xd0\xc2\x10\x00"
        )
        run.write(procedure, code)
        block = run.alloc(40)
        run.write_u32s(block, [0, procedure, 0, 0, run.base, 0, 0, 0, 0, name])
        run.call(address("RegisterClassA"), [block], limit=100)
        args = [8, name, 0, 0x90000000, 3, 0xFFFFFFFE, 10, 12, 0, 0, run.base, 0x1234]
        return run, address, args

    def test_declared_default_result_flows_through_actual_callback_and_stops_before_completion(
        self,
    ):
        self.assertNotIn("DefWindowProcA", Startup(EXE, desktop_profile=desktop()).apis)
        for result, phase in (
            (0, "after_WM_NCCREATE_refusal"),
            (1, "after_WM_NCCREATE_acceptance"),
        ):
            with self.subTest(result=result):
                run, address, args = self.scenario(result)
                with self.assertRaises(Boundary) as error:
                    run.call(
                        address("CreateWindowExA"), args, limit=200, regs={"eax": 123}
                    )
                self.assertEqual(
                    error.exception.facts["kind"], "unsupported_window_creation_phase"
                )
                self.assertEqual(error.exception.facts["phase"], phase)
                self.assertEqual(error.exception.facts["callback_result"], result)
                event = next(
                    e
                    for e in run.events
                    if e.get("import") == "user32.dll!DefWindowProcA"
                )
                self.assertEqual(event["result"], result)
                self.assertEqual(event["desktop"]["declared_nccreate_result"], result)
                self.assertEqual(
                    event["desktop"]["create_struct_words"],
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
                self.assertEqual(run.events[-1]["callback_result"], result)
                self.assertEqual(run.reg("eax"), 123)
                create = next(
                    e
                    for e in run.events
                    if e.get("import") == "user32.dll!CreateWindowExA"
                )
                self.assertNotIn("result", create)

    def test_default_processing_requires_owned_active_callback_and_exact_dll(self):
        run, address, _ = self.scenario()
        pointer = address("DefWindowProcA")
        with self.assertRaises(Boundary) as error:
            run.call(pointer, [1, 0x81, 0, 0], limit=100, regs={"eax": 123})
        self.assertEqual(
            error.exception.facts["kind"], "unsupported_default_proc_context"
        )
        self.assertEqual(run.reg("eax"), 123)
        self.assertNotIn("result", run.events[-1])
        run.import_names[pointer] = "kernel32.dll!DefWindowProcA"
        with self.assertRaises(Boundary) as error:
            run.call(pointer, [1, 0x81, 0, 0], limit=100)
        self.assertEqual(error.exception.facts["kind"], "unsupported_import")

    def test_finished_owned_creation_does_not_authorize_a_later_default_call(self):
        run, address, args = self.scenario()
        with self.assertRaises(Boundary):
            run.call(address("CreateWindowExA"), args, limit=200)
        forwarded = next(
            e["args"]
            for e in run.events
            if e.get("import") == "user32.dll!DefWindowProcA"
        )
        with self.assertRaises(Boundary) as error:
            run.call(address("DefWindowProcA"), forwarded, limit=100, regs={"eax": 123})
        self.assertEqual(
            error.exception.facts["kind"], "unsupported_default_proc_context"
        )
        self.assertEqual(run.reg("eax"), 123)
        self.assertNotIn("result", run.events[-1])

    def test_explicit_next_rect_reaches_original_callback_but_unknown_default_message_stops(
        self,
    ):
        run, address, args = self.scenario(1, "nccalcsize_rect")
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=300)
        self.assertEqual(
            error.exception.facts["kind"], "unsupported_default_window_message"
        )
        self.assertEqual(error.exception.facts["message"], 0x83)
        self.assertFalse(error.exception.facts["default_result_supplied"])
        callbacks = [e for e in run.events if "callback" in e]
        self.assertEqual(
            [e["callback"] for e in callbacks], ["WM_NCCREATE", "WM_NCCALCSIZE"]
        )
        self.assertEqual(callbacks[1]["args"][:3], [callbacks[0]["hwnd"], 0x83, 0])
        rect = callbacks[1]["args"][3]
        self.assertEqual(
            callbacks[1]["structure"],
            {"type": "RECT", "address": rect, "words": [3, 0xFFFFFFFE, 13, 10]},
        )
        self.assertEqual(run.read_u32s(rect, 4), [3, 0xFFFFFFFE, 13, 10])
        self.assertEqual(
            [e["callback_result"] for e in run.events if "callback_return" in e], [1]
        )
        self.assertEqual(run.events[-1]["args"], callbacks[1]["args"])
        self.assertNotIn("result", run.events[-1])
        self.assertEqual(len(run.pending_window_callbacks), 1)
        create = next(
            e for e in run.events if e.get("import") == "user32.dll!CreateWindowExA"
        )
        self.assertNotIn("result", create)

    def test_refusal_does_not_deliver_configured_next_message(self):
        run, address, args = self.scenario(0, "nccalcsize_rect")
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=200)
        self.assertEqual(error.exception.facts["phase"], "after_WM_NCCREATE_refusal")
        self.assertEqual(
            [e["callback"] for e in run.events if "callback" in e], ["WM_NCCREATE"]
        )

    def test_modified_create_struct_is_not_silently_accepted(self):
        run, address, args = self.scenario(prefix=bytes.fromhex("8b442410c70009000000"))
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=200)
        self.assertEqual(error.exception.facts["kind"], "unsupported_create_struct")
        self.assertEqual(error.exception.facts["words"][0], 9)
        self.assertNotIn("result", run.events[-1])

    def test_actual_second_callback_result_and_rect_output_stop_before_completion(self):
        for result in (0, 0x55):
            # Independent WndProc branch changes RECT.left, returning its own LRESULT.
            body = (
                bytes.fromhex("8b542410c7022a000000b8")
                + struct.pack("<I", result)
                + bytes.fromhex("c21000")
            )
            prefix = bytes.fromhex("817c24088300000075") + bytes([len(body)]) + body
            run, address, args = self.scenario(1, "nccalcsize_rect", prefix)
            with self.assertRaises(Boundary) as error:
                run.call(address("CreateWindowExA"), args, limit=300, regs={"eax": 123})
            self.assertEqual(error.exception.facts["phase"], "after_WM_NCCALCSIZE")
            self.assertEqual(error.exception.facts["callback_result"], result)
            self.assertEqual(
                error.exception.facts["rect_words"], [42, 0xFFFFFFFE, 13, 10]
            )
            self.assertFalse(error.exception.facts["win32_result_supplied"])
            self.assertEqual(
                [e["callback_return"] for e in run.events if "callback_return" in e],
                ["WM_NCCREATE", "WM_NCCALCSIZE"],
            )
            self.assertEqual(run.reg("eax"), 123)
            self.assertEqual(run.pending_window_callbacks, {})

    def test_callback_return_cannot_exceed_event_budget(self):
        run, address, args = self.scenario()
        run.record_limit = 4
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=300)
        self.assertEqual(error.exception.facts["kind"], "record_limit")
        self.assertEqual(len(run.events), 4)
        self.assertFalse(any("callback_return" in e for e in run.events))

    def test_wrong_callback_stack_cleanup_stops_before_restoring_api_context(self):
        run, address, args = self.scenario()
        procedure = next(iter(run.desktop.classes.values()))["wndproc"]
        # cdecl-style ret is deliberately not the four-word stdcall WNDPROC ABI.
        run.write(procedure, bytes.fromhex("b801000000c3"))
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=200)
        self.assertEqual(error.exception.facts["kind"], "window_callback_stack")
        self.assertEqual(
            error.exception.facts["expected_esp"] - error.exception.facts["actual_esp"],
            16,
        )
        self.assertEqual(len(run.pending_window_callbacks), 1)
        self.assertFalse(any("callback_return" in e for e in run.events))


if __name__ == "__main__":
    unittest.main()
