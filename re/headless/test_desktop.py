"""Explicit desktop profile and Win32 resource/window boundary tests."""

import hashlib
import json
import struct
import sys
import tempfile
import unittest
from dataclasses import FrozenInstanceError
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from desktop import DesktopProfile, decode_image


def synthetic(**changes):
    data = {
        "label": "hypothetical-desktop-unit",
        "resource_language": 1033,
        "cursor_size": [32, 32],
        "icon_size": [32, 32],
        "system_arrow_available": True,
        "black_brush_available": True,
    }
    data.update(changes)
    return DesktopProfile(data)


class DesktopProfileTests(unittest.TestCase):
    def test_desktop_inputs_are_explicit_bounded_and_immutable(self):
        profile = synthetic()
        self.assertEqual(profile.cursor_size, (32, 32))
        self.assertFalse(profile.metadata["physical_desktop_observed"])
        for changes in (
            {"label": "actual-host"},
            {"resource_language": True},
            {"resource_language": 65536},
            {"cursor_size": [0, 32]},
            {"icon_size": [257, 32]},
            {"system_arrow_available": 1},
            {"black_brush_available": None},
            {"unknown": 0},
        ):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                synthetic(**changes)
        with self.assertRaises(FrozenInstanceError):
            profile.resource_language = 0
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            data = {
                "label": "hypothetical-file",
                "resource_language": 1033,
                "cursor_size": [32, 32],
                "icon_size": [32, 32],
                "system_arrow_available": True,
                "black_brush_available": True,
            }
            path.write_text(json.dumps(data))
            self.assertEqual(DesktopProfile.load(path), DesktopProfile(data))
            path.write_text('{"label":"hypothetical-file",' + json.dumps(data)[1:])
            with self.assertRaisesRegex(ValueError, "duplicate desktop profile key"):
                DesktopProfile.load(path)
            path.write_text(json.dumps(data) + " " * 8192)
            with self.assertRaisesRegex(ValueError, "exceeds 8 KiB"):
                DesktopProfile.load(path)


class ImageResourceTests(unittest.TestCase):
    def test_single_bitmap_group_has_exact_identity_size_and_hotspot_validation(self):
        bitmap = (
            struct.pack("<IiiHHIIiiII", 40, 8, 16, 1, 1, 0, 64, 0, 0, 0, 0) + b"\0" * 72
        )
        payload = struct.pack("<HH", 1, 2) + bitmap
        group = struct.pack("<HHHHHHHIH", 0, 2, 1, 8, 16, 1, 1, len(payload), 7)
        result = decode_image(group, payload, "cursor", (8, 8))
        self.assertEqual(
            result,
            {
                "image_id": 7,
                "width": 8,
                "height": 8,
                "bits_per_pixel": 1,
                "hotspot": [1, 2],
                "group_sha256": hashlib.sha256(group).hexdigest(),
                "payload_sha256": hashlib.sha256(payload).hexdigest(),
            },
        )
        icon = struct.pack("<HHHBBBBHHIH", 0, 1, 1, 8, 8, 2, 0, 1, 1, len(bitmap), 7)
        self.assertEqual(decode_image(icon, bitmap, "icon", (8, 8))["hotspot"], None)
        for changed_group, changed_payload, size in (
            (group[:-1], payload, (8, 8)),
            (
                group[:14] + struct.pack("<I", len(payload) - 1) + group[18:],
                payload[:-1],
                (8, 8),
            ),
            (group[:2] + struct.pack("<H", 1) + group[4:], payload, (8, 8)),
            (group, payload[:-1], (8, 8)),
            (group, payload, (16, 16)),
            (group[:8] + struct.pack("<H", 17) + group[10:], payload, (8, 8)),
            (group, struct.pack("<HH", 9, 2) + bitmap, (8, 8)),
            (group, payload[:4] + struct.pack("<I", 108) + payload[8:], (8, 8)),
        ):
            with self.subTest(size=size), self.assertRaises(ValueError):
                decode_image(changed_group, changed_payload, "cursor", size)


from startup import REPO, Boundary, Startup

EXE = REPO / "reference/raw/StreetLegal_Redline.bak.exe"


@unittest.skipUnless(EXE.exists(), "read-only original executable absent")
class DesktopApiTests(unittest.TestCase):
    def test_declared_predefined_objects_have_shared_handles_and_refusals(self):
        self.assertNotIn("LoadCursorA", Startup(EXE).apis)
        run = Startup(EXE, desktop_profile=synthetic())

        def call(name, args):
            address = next(
                address
                for address, full in run.import_names.items()
                if full
                == ("gdi32.dll" if name == "GetStockObject" else "user32.dll")
                + "!"
                + name
            )
            result = run.call(address, args, convention="stdcall", limit=100)
            self.assertEqual(result.esp_delta, len(args) * 4)
            return result.eax

        arrow = call("LoadCursorA", [0, 32512])
        self.assertNotEqual(arrow, 0)
        self.assertEqual(call("LoadCursorA", [0, 32512]), arrow)
        brush = call("GetStockObject", [4])
        self.assertNotEqual(brush, 0)
        self.assertNotEqual(brush, arrow)
        self.assertEqual(call("GetStockObject", [4]), brush)
        for name, args in (("LoadCursorA", [0, 32513]), ("GetStockObject", [0])):
            with self.assertRaises(Boundary):
                call(name, args)
            self.assertNotIn("result", run.events[-1])
        refused = Startup(
            EXE,
            desktop_profile=synthetic(
                system_arrow_available=False, black_brush_available=False
            ),
        )
        for name, args in (("LoadCursorA", [0, 32512]), ("GetStockObject", [4])):
            address = next(
                address
                for address, full in refused.import_names.items()
                if full.endswith("!" + name)
            )
            self.assertEqual(refused.call(address, args, limit=100).eax, 0)

    def test_registration_preserves_class_attributes_and_rejects_duplicate_names(self):
        run = Startup(EXE, desktop_profile=synthetic())
        name = run.alloc(32)
        run.write(name, b"UnitWindow\0")
        callback = run.alloc(16)
        run.write(callback, b"\xb8\x00\x00\x00\x00\xc2\x10\x00")
        block = run.alloc(40)
        words = [0, callback, 0, 0, run.base, 0, 0, 0, 0, name]
        run.write_u32s(block, words)
        address = next(
            address
            for address, full in run.import_names.items()
            if full == "user32.dll!RegisterClassA"
        )
        result = run.call(address, [block], convention="stdcall", limit=100)
        self.assertEqual(result.esp_delta, 4)
        self.assertTrue(0xC000 <= result.eax <= 0xFFFF)
        self.assertEqual(run.read_u32s(block, 10), words)
        self.assertEqual(run.events[-1]["desktop"]["attributes"], words)
        self.assertEqual(run.events[-1]["desktop"]["class_name"], "UnitWindow")
        self.assertEqual(run.events[-1]["desktop"]["wndproc"], callback)
        run.write(name, b"unitwindow\0")
        self.assertEqual(run.call(address, [block], limit=100).eax, 0)
        self.assertEqual(run.last_error, 1410)
        run.write(name, b"OtherWindow\0")
        run.write_u32(block + 8, 4)
        with self.assertRaises(Boundary):
            run.call(address, [block], limit=100, regs={"eax": 123})
        self.assertEqual(run.reg("eax"), 123)
        self.assertNotIn("result", run.events[-1])
        run.write_u32(block + 8, 0)
        run.write_u32(block + 20, 0x1234)
        with self.assertRaises(Boundary) as error:
            run.call(address, [block], limit=100)
        self.assertEqual(error.exception.facts["kind"], "unknown_class_object_handle")

    def test_popup_creation_executes_callback_with_create_struct_then_stops_at_unknown_phase(
        self,
    ):
        run = Startup(EXE, desktop_profile=synthetic())
        name = run.alloc(32)
        run.write(name, b"UnitWindow\0")
        title = run.alloc(32)
        run.write(title, b"Unit title\0")
        observed = run.alloc(12)
        callback = run.alloc(80)
        # Independent synthetic x86 callback records Msg, lpCreateParams and X.
        code = (
            b"\x8b\x44\x24\x08\xa3"
            + struct.pack("<I", observed)
            + b"\x8b\x44\x24\x10\x8b\x08\x89\x0d"
            + struct.pack("<I", observed + 4)
            + b"\x8b\x40\x1c\xa3"
            + struct.pack("<I", observed + 8)
            + b"\xb8\x01\x00\x00\x00\xc2\x10\x00"
        )
        run.write(callback, code)
        block = run.alloc(40)
        run.write_u32s(block, [0, callback, 0, 0, run.base, 0, 0, 0, 0, name])
        register = next(
            a for a, n in run.import_names.items() if n == "user32.dll!RegisterClassA"
        )
        atom = run.call(register, [block], limit=100).eax
        create = next(
            a for a, n in run.import_names.items() if n == "user32.dll!CreateWindowExA"
        )
        args = [
            8,
            atom,
            title,
            0x90000000,
            3,
            0xFFFFFFFE,
            10,
            12,
            0,
            0,
            run.base,
            0x1234,
        ]
        with self.assertRaises(Boundary) as error:
            run.call(create, args, limit=100, regs={"eax": 123})
        self.assertEqual(
            error.exception.facts["kind"], "unsupported_window_creation_phase"
        )
        self.assertEqual(error.exception.facts["callback_result"], 1)
        self.assertEqual(run.read_u32s(observed, 3), [0x81, 0x1234, 3])
        self.assertEqual(run.reg("eax"), 123)
        self.assertEqual(run.reg("eip"), create)
        self.assertEqual(run.read_u32s(run.reg("esp") + 4, 12), args)
        event = next(
            e
            for e in reversed(run.events)
            if e.get("import") == "user32.dll!CreateWindowExA"
        )
        self.assertNotIn("result", event)
        words = event["desktop"]["create_struct_words"]
        self.assertEqual(
            words,
            [0x1234, run.base, 0, 0, 12, 10, 0xFFFFFFFE, 3, 0x90000000, title, atom, 8],
        )
        self.assertEqual(run.events[-1]["callback_result"], 1)
        self.assertEqual(run.events[-1]["thread"], 1)
        self.assertNotEqual(run.events[-1]["hwnd"], 0)
        # A rejecting callback also stops before unmodeled cleanup/Win32 return.
        rejecting = run.alloc(16)
        run.write(rejecting, b"\x31\xc0\xc2\x10\x00")
        other = run.alloc(32)
        run.write(other, b"RefusingWindow\0")
        run.write_u32s(block, [0, rejecting, 0, 0, run.base, 0, 0, 0, 0, other])
        run.call(register, [block], limit=100)
        args[1] = other
        with self.assertRaises(Boundary) as error:
            run.call(create, args, limit=100)
        self.assertEqual(error.exception.facts["callback_result"], 0)
        self.assertEqual(error.exception.facts["phase"], "after_WM_NCCREATE_refusal")

    def test_desktop_trace_marks_declared_inputs_without_changing_default_shape(self):
        base = Startup(EXE).run(limit=1)
        self.assertNotIn("desktop_scenario", base)
        trace = Startup(EXE, desktop_profile=synthetic()).run(limit=1)
        self.assertEqual(trace["desktop_scenario"], synthetic().metadata)
        self.assertEqual(trace["pending_window_callbacks"], 0)
        self.assertEqual(trace["status"], "incomplete")
        self.assertFalse(trace["main_menu_reached"])

    def test_unknown_class_and_unsupported_shape_do_not_invent_success(self):
        run = Startup(EXE, desktop_profile=synthetic())
        name = run.alloc(32)
        run.write(name, b"UnknownWindow\0")
        address = next(
            a for a, n in run.import_names.items() if n == "user32.dll!CreateWindowExA"
        )
        args = [8, name, 0, 0x90000000, 0, 0, 0, 0, 0, 0, run.base, 0]
        self.assertEqual(run.call(address, args, limit=100).eax, 0)
        self.assertEqual(run.last_error, 1407)
        args[3] = 0
        with self.assertRaises(Boundary) as error:
            run.call(address, args, limit=100, regs={"eax": 123})
        self.assertEqual(error.exception.facts["kind"], "unsupported_popup_shape")
        self.assertEqual(run.reg("eax"), 123)
        self.assertNotIn("result", run.events[-1])
        run.import_names[address] = "kernel32.dll!CreateWindowExA"
        with self.assertRaises(Boundary) as error:
            run.call(address, args, limit=100)
        self.assertEqual(error.exception.facts["kind"], "unsupported_import")

    def test_event_budget_stops_before_entering_window_callback(self):
        run = Startup(EXE, desktop_profile=synthetic(), record_limit=2)
        name = run.alloc(32)
        run.write(name, b"BoundedWindow\0")
        procedure = run.alloc(16)
        run.write(procedure, b"\x31\xc0\xc2\x10\x00")
        block = run.alloc(40)
        run.write_u32s(block, [0, procedure, 0, 0, run.base, 0, 0, 0, 0, name])
        register = next(
            a for a, n in run.import_names.items() if n == "user32.dll!RegisterClassA"
        )
        run.call(register, [block], limit=100)
        create = next(
            a for a, n in run.import_names.items() if n == "user32.dll!CreateWindowExA"
        )
        with self.assertRaises(Boundary) as error:
            run.call(
                create,
                [8, name, 0, 0x90000000, 0, 0, 0, 0, 0, 0, run.base, 0],
                limit=100,
            )
        self.assertEqual(error.exception.facts["kind"], "record_limit")
        self.assertEqual(len(run.events), 2)
        self.assertNotIn("result", run.events[-1])
        self.assertFalse(any("callback" in event for event in run.events))


if __name__ == "__main__":
    unittest.main()
