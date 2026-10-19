"""Public profile and Win32 seams for a declared finite creation transcript."""

import copy
import struct
import sys
import unittest
from dataclasses import FrozenInstanceError
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from startup import REPO, Boundary, Startup
from test_window_messages import desktop
from test_window_visible import creation

EXE = REPO / "reference/raw/StreetLegal_Redline.bak.exe"


def show(after="stop", messages=None, api_return="stop"):
    result = {"default_result": 0, "expected_callback_result": 0, "after": after}
    if after == "transcript":
        result["transcript"] = {"messages": messages or [], "api_return": api_return}
    return result


def profile(data):
    config = creation(after="wm_showwindow")
    config["wm_showwindow"] = data
    return desktop(creation=config)


def application_result(message, result):
    body = b"\xb8" + struct.pack("<I", result) + b"\xc2\x10\x00"
    return (
        b"\x81\x7c\x24\x08"
        + struct.pack("<I", message)
        + b"\x75"
        + bytes([len(body)])
        + body
    )


class TranscriptProfileTests(unittest.TestCase):
    def test_optional_show_stop_requires_explicit_results_and_freezes_metadata(self):
        data = show()
        result = profile(data)
        self.assertEqual(result.creation.metadata["wm_showwindow"], data)
        self.assertFalse(result.metadata["windows_notification_sequence_established"])
        self.assertFalse(result.metadata["unlisted_notifications_modeled"])
        data["default_result"] = 9
        self.assertEqual(result.creation.wm_showwindow.default_result, 0)
        with self.assertRaises(FrozenInstanceError):
            result.creation.wm_showwindow.after = "transcript"
        for field in ("default_result", "expected_callback_result", "after"):
            bad = show()
            del bad[field]
            with self.subTest(field=field), self.assertRaises(ValueError):
                profile(bad)

    def test_finite_transcript_and_typed_windowpos_are_explicit_and_deeply_immutable(
        self,
    ):
        entry = {
            "message": 70,
            "wparam": 0,
            "windowpos": ["topmost", -3, 4, 10, 12, 16],
            "default_memory": "identity",
            "default_result": 0,
            "expected_callback_result": 0,
        }
        data = show("transcript", [entry], "owned_provisional_handle")
        value = profile(data)
        self.assertEqual(value.creation.metadata["wm_showwindow"], data)
        data["transcript"]["messages"][0]["windowpos"][1] = 99
        self.assertEqual(
            value.creation.wm_showwindow.metadata["transcript"]["messages"][0][
                "windowpos"
            ][1],
            -3,
        )
        with self.assertRaises(FrozenInstanceError):
            value.creation.wm_showwindow.transcript.messages[0].wparam = 3
        for bad in (
            show(
                "transcript",
                [
                    {
                        "message": 999,
                        "wparam": 0,
                        "lparam": 0,
                        "default_result": 0,
                        "expected_callback_result": 0,
                    }
                ],
            ),
            show("transcript", [entry] * 9),
            show("transcript", [], "constant_success"),
            dict(show(), transcript={"messages": [], "api_return": "stop"}),
            dict(show("transcript"), default_result=False),
        ):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                profile(bad)

    def test_message_words_shapes_and_continuation_dependencies_fail_closed(self):
        scalar = {
            "message": 3,
            "wparam": 0,
            "lparam": 0,
            "default_result": 0,
            "expected_callback_result": 0,
        }
        pointer = {
            "message": 71,
            "wparam": 0,
            "windowpos": ["top", 0, 0, 10, 12, 0],
            "default_memory": "identity",
            "default_result": 0,
            "expected_callback_result": 0,
        }
        for template, key, value in (
            (scalar, "message", True),
            (scalar, "wparam", False),
            (scalar, "wparam", -1),
            (scalar, "lparam", 0x100000000),
            (scalar, "lparam", None),
            (scalar, "default_result", False),
            (scalar, "expected_callback_result", 1),
            (scalar, "extra", 0),
            (pointer, "wparam", 1),
            (pointer, "default_memory", "mutate"),
            (pointer, "windowpos", ["bottom", 0, 0, 10, 12, 0]),
            (pointer, "windowpos", ["top", 8193, 0, 10, 12, 0]),
            (pointer, "windowpos", ["top", 0, 0, 10, 12, 65536]),
            (pointer, "windowpos", ["top", 0, 0, 10, 12, False]),
        ):
            entry = copy.deepcopy(template)
            entry[key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                profile(show("transcript", [entry]))
        for field in scalar:
            entry = dict(scalar)
            del entry[field]
            with self.subTest(missing=field), self.assertRaises(ValueError):
                profile(show("transcript", [entry]))
        for message in (3, 5, 28, 6, 7, 8):
            entry = dict(scalar, message=message)
            self.assertEqual(
                profile(show("transcript", [entry]))
                .creation.wm_showwindow.transcript.messages[0]
                .message,
                message,
            )
        config = creation()
        config["wm_showwindow"] = show()
        with self.assertRaises(ValueError):
            desktop(creation=config)


@unittest.skipUnless(EXE.exists(), "read-only original absent")
class TranscriptApiTests(unittest.TestCase):
    def scenario(self, data, prefix=b"", create_result=0):
        config = creation(create_result, "wm_showwindow")
        config["wm_showwindow"] = data
        run = Startup(EXE, desktop_profile=desktop(creation=config))
        address = lambda name: next(
            a for a, n in run.import_names.items() if n == "user32.dll!" + name
        )
        name = run.alloc(32)
        run.write(name, b"UnitTranscript\0")
        procedure = run.alloc(256)
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
            [8, name, 0, 0x90000000, 3, 0, 10, 12, 0, 0, run.base, 0x1234],
        )

    def test_explicit_show_zero_executes_callback_and_records_consumed_default_then_stops(
        self,
    ):
        run, address, args = self.scenario(show())
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=1000, regs={"eax": 123})
        self.assertEqual(error.exception.facts["phase"], "after_WM_SHOWWINDOW")
        self.assertEqual(error.exception.facts["callback_result"], 0)
        self.assertFalse(error.exception.facts["win32_result_supplied"])
        self.assertEqual(
            run.events[-1]["declared_callback"],
            {
                "default_result": 0,
                "expected_callback_result": 0,
                "default_consumed": True,
            },
        )
        self.assertEqual(run.reg("eax"), 123)
        create = next(
            e for e in run.events if e.get("import") == "user32.dll!CreateWindowExA"
        )
        self.assertNotIn("result", create)
        self.assertEqual(run.pending_window_callbacks, {})
        handle = next(
            e["hwnd"] for e in run.events if e.get("callback") == "WM_SHOWWINDOW"
        )
        self.assertEqual(run.windows[handle]["creation"], "after_WM_SHOWWINDOW")

    def test_finite_messages_use_explicit_defaults_then_return_owned_handle_once(self):
        messages = [
            {
                "message": 3,
                "wparam": 0,
                "lparam": 0,
                "default_result": 0,
                "expected_callback_result": 0,
            },
            {
                "message": 70,
                "wparam": 0,
                "windowpos": ["topmost", -3, 4, 10, 12, 16],
                "default_memory": "identity",
                "default_result": 0,
                "expected_callback_result": 0,
            },
        ]
        run, address, args = self.scenario(
            show("transcript", messages, "owned_provisional_handle")
        )
        first_call = run.call(
            address("CreateWindowExA"), args, convention="stdcall", limit=1500
        )
        first = first_call.eax
        self.assertEqual(first_call.esp_delta, 48)
        self.assertIn(first, run.windows)
        self.assertEqual(run.windows[first]["creation"], "declared_api_return_supplied")
        self.assertEqual(
            [x["callback"] for x in run.events if "callback" in x],
            [
                "WM_NCCREATE",
                "WM_NCCALCSIZE",
                "WM_CREATE",
                "WM_SHOWWINDOW",
                "WM_MOVE",
                "WM_WINDOWPOSCHANGING",
            ],
        )
        returns = [e for e in run.events if "callback_return" in e]
        self.assertEqual([e["callback_result"] for e in returns], [1, 0, 0, 0, 0, 0])
        self.assertTrue(
            all(e["declared_callback"]["default_consumed"] for e in returns[3:])
        )
        move = next(e for e in run.events if e.get("callback") == "WM_MOVE")
        self.assertEqual(move["args"], [first, 3, 0, 0])
        pointer = next(
            e for e in run.events if e.get("callback") == "WM_WINDOWPOSCHANGING"
        )["args"][3]
        self.assertEqual(
            run.read_u32s(pointer, 7), [first, 0xFFFFFFFF, 0xFFFFFFFD, 4, 10, 12, 16]
        )
        calls = [
            e for e in run.events if e.get("import") == "user32.dll!CreateWindowExA"
        ]
        self.assertEqual(calls[-1]["result"], first)
        self.assertEqual(calls[-1]["args"], args)
        self.assertEqual(calls[-1]["return_assumption"], "declared-platform-api-result")
        self.assertNotIn(address("CreateWindowExA"), run.hooks)
        second = run.call(
            address("CreateWindowExA"), args, convention="stdcall", limit=1500
        ).eax
        self.assertNotEqual(second, first)
        callback = next(
            e for e in run.events if e.get("callback") == "WM_WINDOWPOSCHANGING"
        )
        self.assertEqual(
            callback["structure"],
            {
                "type": "WINDOWPOS",
                "address": pointer,
                "words": [first, 0xFFFFFFFF, 0xFFFFFFFD, 4, 10, 12, 16],
            },
        )
        self.assertEqual(run.pending_window_callbacks, {})

    def test_original_application_results_control_refusal_and_unused_defaults(self):
        data = show("transcript", [], "owned_provisional_handle")
        run, address, args = self.scenario(data, create_result=-1)
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=1000)
        self.assertEqual(error.exception.facts["callback_result"], 0xFFFFFFFF)
        self.assertFalse(any(e.get("callback") == "WM_SHOWWINDOW" for e in run.events))
        run, address, args = self.scenario(data, application_result(1, 0xFFFFFFFF))
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=1000)
        self.assertEqual(error.exception.facts["callback_result"], 0xFFFFFFFF)
        self.assertFalse(any(e.get("callback") == "WM_SHOWWINDOW" for e in run.events))
        run, address, args = self.scenario(
            data, application_result(1, 0) + application_result(24, 0), -1
        )
        result = run.call(
            address("CreateWindowExA"), args, convention="stdcall", limit=1000
        ).eax
        self.assertIn(result, run.windows)
        self.assertFalse(run.events[-1]["declared_callback"]["default_consumed"])
        self.assertFalse(
            any(
                e.get("import") == "user32.dll!DefWindowProcA"
                and e["args"][1] in (1, 24)
                for e in run.events
            )
        )
        run, address, args = self.scenario(data, application_result(24, 7))
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=1000)
        self.assertEqual(
            error.exception.facts["kind"], "unexpected_declared_callback_result"
        )
        self.assertEqual(error.exception.facts["actual"], 7)
        self.assertNotIn(address("CreateWindowExA"), run.hooks)

    def test_changed_windowpos_stops_before_supplying_its_default(self):
        message = {
            "message": 70,
            "wparam": 0,
            "windowpos": ["top", 0, 0, 10, 12, 0],
            "default_memory": "identity",
            "default_result": 0,
            "expected_callback_result": 0,
        }
        body = bytes.fromhex("8b442410c70009000000")
        prefix = bytes.fromhex("817c24084600000075") + bytes([len(body)]) + body
        run, address, args = self.scenario(
            show("transcript", [message], "owned_provisional_handle"), prefix
        )
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=1500)
        self.assertEqual(
            error.exception.facts["kind"], "unsupported_declared_windowpos"
        )
        self.assertEqual(error.exception.facts["words"][0], 9)
        self.assertNotIn("result", run.events[-1])

    def test_changed_suspended_twelve_arguments_cannot_receive_declared_api_result(
        self,
    ):
        run, address, args = self.scenario(
            show("transcript", [], "owned_provisional_handle")
        )
        capture = run.alloc(4)
        block = next(
            e["args"][0]
            for e in run.events
            if e.get("import") == "user32.dll!RegisterClassA"
        )
        procedure = run.read_u32(block + 4)
        # Independent caller saves its argument-stack address. The SHOW body
        # changes the suspended API's first argument; no engine field is touched.
        body = b"\xa1" + struct.pack("<I", capture) + bytes.fromhex("c70009000000")
        prefix = bytes.fromhex("817c24081800000075") + bytes([len(body)]) + body
        run.write(
            procedure,
            prefix
            + b"\xff\x74\x24\x10" * 4
            + b"\xb8"
            + struct.pack("<I", address("DefWindowProcA"))
            + bytes.fromhex("ffd0c21000"),
        )
        caller = run.alloc(128)
        code = b"".join(b"\x68" + struct.pack("<I", x) for x in reversed(args))
        code += (
            b"\x89\x25"
            + struct.pack("<I", capture)
            + b"\xb8"
            + struct.pack("<I", address("CreateWindowExA"))
            + bytes.fromhex("ffd0c3")
        )
        run.write(caller, code)
        with self.assertRaises(Boundary) as error:
            run.call(caller, limit=1500)
        self.assertEqual(
            error.exception.facts["kind"], "unsupported_suspended_creation_call"
        )
        self.assertNotIn(address("CreateWindowExA"), run.hooks)

    def test_a_second_default_call_has_no_undeclared_zero_fallback(self):
        run, address, args = self.scenario(
            show("transcript", [], "owned_provisional_handle")
        )
        block = next(
            e["args"][0]
            for e in run.events
            if e.get("import") == "user32.dll!RegisterClassA"
        )
        procedure = run.read_u32(block + 4)
        forward = (
            b"\xff\x74\x24\x10" * 4
            + b"\xb8"
            + struct.pack("<I", address("DefWindowProcA"))
            + bytes.fromhex("ffd0")
        )
        body = forward + forward + bytes.fromhex("c21000")
        run.write(
            procedure,
            bytes.fromhex("817c24081800000075")
            + bytes([len(body)])
            + body
            + forward
            + bytes.fromhex("c21000"),
        )
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=1500)
        self.assertEqual(error.exception.facts["kind"], "repeated_declared_default")
        self.assertNotIn("result", run.events[-1])

    def test_unlisted_arguments_and_later_callback_mismatch_stop_before_api_return(
        self,
    ):
        for offset in (4, 8, 12, 16):
            # Change one of the four supplied SHOW arguments before delegation.
            body = bytes.fromhex("c74424") + bytes([offset]) + struct.pack("<I", 123)
            prefix = bytes.fromhex("817c24081800000075") + bytes([len(body)]) + body
            run, address, args = self.scenario(
                show("transcript", [], "owned_provisional_handle"), prefix
            )
            with self.subTest(offset=offset), self.assertRaises(Boundary) as error:
                run.call(address("CreateWindowExA"), args, limit=1500)
            self.assertEqual(
                error.exception.facts["kind"], "unsupported_default_proc_context"
            )
            self.assertNotIn("result", run.events[-1])
        message = {
            "message": 3,
            "wparam": 0,
            "lparam": 0,
            "default_result": 0,
            "expected_callback_result": 0,
        }
        run, address, args = self.scenario(
            show("transcript", [message], "owned_provisional_handle"),
            application_result(3, 7),
        )
        with self.assertRaises(Boundary) as error:
            run.call(address("CreateWindowExA"), args, limit=1500)
        self.assertEqual(
            error.exception.facts["kind"], "unexpected_declared_callback_result"
        )
        self.assertEqual(error.exception.facts["message"], 3)
        self.assertEqual(error.exception.facts["actual"], 7)
        self.assertFalse(run.events[-1]["declared_callback"]["default_consumed"])
        self.assertFalse(
            any(
                "result" in e
                for e in run.events
                if e.get("import") == "user32.dll!CreateWindowExA"
            )
        )


if __name__ == "__main__":
    unittest.main()
