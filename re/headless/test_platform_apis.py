"""Declared DirectInput, DirectSound, registry, module, window and COM-class APIs."""

import copy
import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import calllog
from dinput_platform import GUID_SYS_KEYBOARD, GUID_SYS_MOUSE, IID_IDIRECTINPUT8A
from make_boot_profile import PROFILE
from platform_fixtures import EXE, HWND, Boundary, call, make_startup, profile_data
from platform_profile import PlatformProfile

DS_PRIO = 0x88780046
DS_INVALID = 0x80070057


def put_guid(startup, raw):
    pointer = startup.alloc(16)
    startup.write(pointer, raw)
    return pointer


def put_cstr(startup, text):
    pointer = startup.alloc(len(text) + 1)
    startup.write(pointer, text.encode() + b"\0")
    return pointer


def api(startup, name, *args):
    return startup.apis[name][1](list(args))


class ProfileSectionTests(unittest.TestCase):
    def test_sections_reject_incomplete_or_wider_declarations(self):
        for section, value in (
            ("input", {"keyboard": {}, "mouse": {}, "game_controllers": [], "keyboard_layout": 1, "cursor_position": [0, 0]}),
            ("input", {**PROFILE["input"], "game_controllers": [{"name": "pad"}]}),
            ("input", {**PROFILE["input"], "cursor_position": [0]}),
            ("sound", {"caps": {}, "write_lead_ms": 1}),
            ("sound", {**PROFILE["sound"], "write_lead_ms": 5000}),
            ("registry", {"keys": {"HKLM\\x": {"v": {"type": "REG_BINARY", "data": 1}}}}),
            ("registry", {"keys": {"BADROOT\\x": {}}}),
            ("modules", {"absent": ["Upper.dll"], "exports": {}}),
            ("com", {"unregistered_clsids": ["not-a-guid"]}),
            ("window", {"message_delivery": "native"}),
        ):
            with self.subTest(section=section, value=value), self.assertRaises(ValueError):
                PlatformProfile(profile_data(**{section: value}))


@unittest.skipUnless(EXE.exists(), "local original executable absent")
class Win32Tests(unittest.TestCase):
    def setUp(self):
        self.startup = make_startup()

    def test_show_window_returns_previous_visibility_and_requires_declared_message_delivery(self):
        s = self.startup
        self.assertEqual(api(s, "ShowWindow", HWND, 0), 1)
        self.assertEqual(api(s, "ShowWindow", HWND, 0), 0)
        self.assertEqual(api(s, "ShowWindow", HWND, 5), 0)
        self.assertEqual(api(s, "ShowWindow", HWND, 5), 1)
        with self.assertRaises(Boundary):
            api(s, "ShowWindow", 0x999, 0)
        with self.assertRaises(Boundary):
            api(s, "ShowWindow", HWND, 3)
        self.assertEqual(api(s, "SetFocus", HWND), 0)
        self.assertEqual(api(s, "SetFocus", HWND), HWND)
        # without the declaration the API is not registered at all
        bare = make_startup(window=None)
        self.assertNotIn("ShowWindow", bare.apis)

    def test_registry_only_knows_declared_keys_and_values(self):
        data = copy.deepcopy(PROFILE["registry"])
        data["keys"]["HKLM\\Software\\Test"] = {
            "Answer": {"type": "REG_DWORD", "data": 42},
            "Name": {"type": "REG_SZ", "data": "abc"},
        }
        s = make_startup(registry=data)
        out = s.alloc(4)
        key = put_cstr(s, "Software\\Test")
        self.assertEqual(api(s, "RegOpenKeyA", 0x80000002, key, out), 0)
        handle = s.read_u32(out)
        kind, size, value = s.alloc(4), s.alloc(4), s.alloc(16)
        s.write_u32(size, 4)
        self.assertEqual(api(s, "RegQueryValueExA", handle, put_cstr(s, "ANSWER"), 0, kind, value, size), 0)
        self.assertEqual((s.read_u32(kind), s.read_u32(value), s.read_u32(size)), (4, 42, 4))
        s.write_u32(size, 2)  # too small
        self.assertEqual(api(s, "RegQueryValueExA", handle, put_cstr(s, "Name"), 0, kind, value, size), 234)
        self.assertEqual(s.read_u32(size), 4)
        self.assertEqual(api(s, "RegQueryValueExA", handle, put_cstr(s, "Missing"), 0, 0, 0, 0), 2)
        self.assertEqual(api(s, "RegCloseKey", handle), 0)
        self.assertEqual(api(s, "RegCloseKey", handle), 6)
        self.assertEqual(api(s, "RegOpenKeyA", 0x80000002, put_cstr(s, "Software\\Nope"), out), 2)
        self.assertEqual(api(s, "RegOpenKeyA", 0x80000001, key, out), 2)  # HKCU is not HKLM

    def test_absent_modules_load_nothing_and_declared_exports_resolve(self):
        s = self.startup
        self.assertEqual(s.module_handle([put_cstr(s, "D3D9D.dll")]), 0)
        self.assertEqual(s.last_error, 126)
        self.assertEqual(api(s, "LoadLibraryA", put_cstr(s, "d3d9d.dll")), 0)
        handle = api(s, "LoadLibraryA", put_cstr(s, "d3d9.dll"))
        self.assertEqual(handle, s.module_handle([put_cstr(s, "d3d9.dll")]))
        address = s.proc_address([handle, put_cstr(s, "DebugSetMute")])
        self.assertEqual(s.import_names[address], "d3d9.dll!DebugSetMute")
        with self.assertRaises(Boundary):
            s.proc_address([handle, put_cstr(s, "Direct3DShaderValidatorCreate9")])
        with self.assertRaises(Boundary):
            s.module_handle([put_cstr(s, "unknown.dll")])
        self.assertEqual(api(s, "FreeLibrary", handle), 1)
        with self.assertRaises(Boundary):
            api(s, "FreeLibrary", handle)

    def test_get_full_path_name_follows_the_documented_length_rules(self):
        s = self.startup
        buffer, part = s.alloc(260), s.alloc(4)
        name = put_cstr(s, "Data\\.\\..\\system.rpk")
        self.assertEqual(api(s, "GetFullPathNameA", name, 0, 0, 0), len("C:\\game\\system.rpk") + 1)
        n = api(s, "GetFullPathNameA", name, 260, buffer, part)
        self.assertEqual(s.read_cstr(buffer), b"C:\\game\\system.rpk")
        self.assertEqual(n, 18)
        self.assertEqual(s.read_cstr(s.read_u32(part)), b"system.rpk")
        self.assertEqual(api(s, "GetFullPathNameA", put_cstr(s, "D:/a/b"), 260, buffer, 0), 6)
        self.assertEqual(s.read_cstr(buffer), b"D:\\a\\b")

    def test_co_initialize_ex_modes_and_declared_unregistered_classes(self):
        s = self.startup
        self.assertEqual(api(s, "CoInitializeEx", 0, 2), 0)
        self.assertEqual(api(s, "CoInitializeEx", 0, 2), 1)  # S_FALSE
        self.assertEqual(api(s, "CoInitializeEx", 0, 0), 0x80010106)  # RPC_E_CHANGED_MODE
        out = s.alloc(4)
        s.write_u32(out, 0xDEAD)
        clsid = put_guid(s, bytes.fromhex("b3eb36e44f52ce119f530020af0ba770"))
        iid = put_guid(s, bytes(16))
        self.assertEqual(api(s, "CoCreateInstance", clsid, 0, 1, iid, out), 0x80040154)
        self.assertEqual(s.read_u32(out), 0)  # ppv is NULL on failure
        with self.assertRaises(Boundary):
            api(s, "CoCreateInstance", put_guid(s, bytes(range(16))), 0, 1, iid, out)
        fresh = make_startup()
        self.assertEqual(
            api(fresh, "CoCreateInstance", clsid, 0, 1, iid, fresh.alloc(4)), 0x800401F0
        )


@unittest.skipUnless(EXE.exists(), "local original executable absent")
class InputTests(unittest.TestCase):
    def setUp(self):
        self.startup = make_startup()
        s = self.startup
        out = s.alloc(4)
        self.assertEqual(
            api(s, "DirectInput8Create", s.base, 0x800, put_guid(s, IID_IDIRECTINPUT8A), out, 0), 0
        )
        self.di = s.com.objects[s.read_u32(out)]

    def device(self, guid):
        s = self.startup
        out = s.alloc(4)
        self.assertEqual(call(s, self.di, "CreateDevice", put_guid(s, guid), out, 0), 0)
        return s.com.objects[s.read_u32(out)]

    def test_creation_arguments_are_checked(self):
        s = self.startup
        out = s.alloc(4)
        with self.assertRaises(Boundary):
            api(s, "DirectInput8Create", s.base, 0x700, put_guid(s, IID_IDIRECTINPUT8A), out, 0)
        with self.assertRaises(Boundary):
            api(s, "DirectInput8Create", s.base, 0x800, put_guid(s, bytes(16)), out, 0)
        with self.assertRaises(Boundary):
            api(s, "DirectInput8Create", s.base, 0x800, put_guid(s, IID_IDIRECTINPUT8A), out, 0)  # second

    def test_unknown_device_guid_is_not_registered(self):
        s = self.startup
        self.assertEqual(
            call(s, self.di, "CreateDevice", put_guid(s, bytes(range(16))), s.alloc(4), 0), 0x80040154
        )

    def test_keyboard_acquire_state_and_poll_recording(self):
        with tempfile.TemporaryDirectory() as directory:
            s = make_startup()
            s.calllog = calllog.Writer(Path(directory) / "i.slrlog")
            out = s.alloc(4)
            api(s, "DirectInput8Create", s.base, 0x800, put_guid(s, IID_IDIRECTINPUT8A), out, 0)
            di = s.com.objects[s.read_u32(out)]
            call(s, di, "CreateDevice", put_guid(s, GUID_SYS_KEYBOARD), out, 0)
            kb = s.com.objects[s.read_u32(out)]
            state = s.alloc(256)
            with self.assertRaises(Boundary):
                call(s, kb, "Acquire")  # no data format yet
            fmt = s.alloc(24)
            s.write(fmt, struct.pack("<6I", 0x18, 0x10, 2, 0x100, 0x100, 0))
            self.assertEqual(call(s, kb, "SetDataFormat", fmt), 0)
            self.assertEqual(call(s, kb, "SetCooperativeLevel", HWND, 6), 0)
            self.assertEqual(call(s, kb, "SetCooperativeLevel", HWND, 3), 0x80070057)
            self.assertEqual(call(s, kb, "GetDeviceState", 256, state), 0x8007000C)  # not acquired
            self.assertEqual(call(s, kb, "Acquire"), 0)
            self.assertEqual(call(s, kb, "GetDeviceState", 16, state), 0x80070057)
            self.assertEqual(call(s, kb, "GetDeviceState", 256, state), 0)
            self.assertEqual(s.read(state, 256), bytes(256))
            s.calllog.close()
            records = calllog.Reader(s.calllog.stream.name and Path(s.calllog.path).read_bytes()).records()
        self.assertEqual(records, [("dinput", 0, bytes(256))])

    def test_mouse_state_size_and_empty_game_controller_enumeration(self):
        s = self.startup
        mouse = self.device(GUID_SYS_MOUSE)
        fmt = s.alloc(24)
        s.write(fmt, struct.pack("<6I", 0x18, 0x10, 2, 0x10, 7, 0))
        self.assertEqual(call(s, mouse, "SetDataFormat", fmt), 0)
        bad = s.alloc(24)
        s.write(bad, struct.pack("<6I", 0x18, 0x10, 2, 0x20, 7, 0))
        with self.assertRaises(Boundary):
            call(s, mouse, "SetDataFormat", bad)
        call(s, mouse, "SetCooperativeLevel", HWND, 6)
        call(s, mouse, "Acquire")
        out = s.alloc(16)
        self.assertEqual(call(s, mouse, "GetDeviceState", 16, out), 0)
        count = s.alloc(4)
        s.write_u32(count, 10)
        self.assertEqual(call(s, mouse, "GetDeviceData", 20, 0, count, 0), 0)
        self.assertEqual(s.read_u32(count), 0)
        self.assertEqual(call(s, self.di, "EnumDevices", 4, 0x1234, 0, 1), 0)  # no callback occurs
        with self.assertRaises(Boundary):
            call(s, self.di, "EnumDevices", 5, 0x1234, 0, 1)
        caps = s.alloc(44)
        s.write_u32(caps, 44)
        self.assertEqual(call(s, mouse, "GetCapabilities", caps), 0)
        self.assertEqual(s.read_u32s(caps, 6), [44, 1, 0x102, 3, 4, 0])

    def test_user32_input_apis_use_declared_state(self):
        s = self.startup
        self.assertEqual(api(s, "GetKeyboardLayout", 0), 0x04090409)
        with self.assertRaises(Boundary):
            api(s, "GetKeyboardLayout", 5)
        point = s.alloc(8)
        self.assertEqual(api(s, "GetCursorPos", point), 1)
        self.assertEqual(struct.unpack("<2i", s.read(point, 8)), (512, 384))
        rect = s.alloc(16)
        s.write(rect, struct.pack("<4i", 0, 0, 100, 100))
        self.assertEqual(api(s, "ClipCursor", rect), 1)
        api(s, "SetCursorPos", 500, 500)
        api(s, "GetCursorPos", point)
        self.assertEqual(struct.unpack("<2i", s.read(point, 8)), (99, 99))  # clamped to the clip rectangle
        self.assertEqual(api(s, "ShowCursor", 0), 0xFFFFFFFF)
        self.assertEqual(api(s, "ShowCursor", 1), 0)
        keys = s.alloc(256)
        s.write(keys, b"\x01" * 256)
        self.assertEqual(api(s, "GetKeyboardState", keys), 1)
        self.assertEqual(s.read(keys, 256), bytes(256))
        self.assertEqual(api(s, "GetAsyncKeyState", 0x1B), 0)
        self.assertEqual(api(s, "SetCursor", 0), 0)
        with self.assertRaises(Boundary):
            api(s, "SetCursor", 0xBAD)


@unittest.skipUnless(EXE.exists(), "local original executable absent")
class SoundTests(unittest.TestCase):
    def setUp(self):
        self.startup = make_startup()
        s = self.startup
        s.com.sink = None
        s.d3d.arena  # buffer memory comes from the resource arena
        out = s.alloc(4)
        self.assertEqual(api(s, "ord1", 0, out, 0), 0)
        self.ds = s.com.objects[s.read_u32(out)]

    def desc(self, flags, nbytes=0, channels=2, rate=22050, bits=16, size=36, with_format=True):
        s = self.startup
        fmt = 0
        if with_format:
            fmt = s.alloc(16)
            align = channels * bits // 8
            s.write(fmt, struct.pack("<HHIIHH", 1, channels, rate, rate * align, align, bits))
        pointer = s.alloc(36)
        s.write(pointer, struct.pack("<9I", size, flags, nbytes, 0, fmt, 0, 0, 0, 0))
        return pointer

    def buffer(self, **kwargs):
        s = self.startup
        out = s.alloc(4)
        result = call(s, self.ds, "CreateSoundBuffer", self.desc(**kwargs), out, 0)
        return result, (s.com.objects[s.read_u32(out)] if result == 0 else None)

    def test_caps_follow_the_declared_hardware(self):
        s = self.startup
        caps = s.alloc(96)
        self.assertEqual(call(s, self.ds, "GetCaps", caps), DS_INVALID)
        s.write_u32(caps, 96)
        self.assertEqual(call(s, self.ds, "GetCaps", caps), 0)
        words = s.read_u32s(caps, 24)
        self.assertEqual((words[0], words[1], words[5]), (96, 0xF3F, 0))  # no hardware mixing declared

    def test_primary_buffer_needs_a_cooperative_level_and_play_writes_format(self):
        s = self.startup
        result, _ = self.buffer(flags=1, with_format=False)
        self.assertEqual(result, DS_PRIO)
        self.assertEqual(call(s, self.ds, "SetCooperativeLevel", HWND, 2), 0)
        self.assertEqual(call(s, self.ds, "SetCooperativeLevel", HWND, 9), DS_INVALID)
        result, primary = self.buffer(flags=1 | 0x10, with_format=False)
        self.assertEqual(result, 0)
        fmt = s.alloc(16)
        s.write(fmt, struct.pack("<HHIIHH", 1, 2, 22050, 88200, 4, 16))
        self.assertEqual(call(s, primary, "SetFormat", fmt), 0)
        self.assertEqual(call(s, primary, "Play", 0, 0, 1), 0)
        with self.assertRaises(Boundary):
            self.buffer(flags=1, with_format=False)  # a second primary buffer is not modelled

    def test_secondary_buffer_validation_and_three_d_mono_rule(self):
        call(self.startup, self.ds, "SetCooperativeLevel", HWND, 2)
        self.assertEqual(self.buffer(flags=0x80, nbytes=2, with_format=True)[0], DS_INVALID)
        self.assertEqual(self.buffer(flags=0x80, nbytes=4096, with_format=False)[0], DS_INVALID)
        self.assertEqual(self.buffer(flags=0x10 | 0x80, nbytes=4096, channels=2)[0], DS_INVALID)
        self.assertEqual(self.buffer(flags=0x10 | 0x80, nbytes=4096, channels=1)[0], 0)
        with self.assertRaises(Boundary):
            self.buffer(flags=0x80000, nbytes=4096)
        self.assertEqual(self.buffer(flags=0x80, nbytes=4097, channels=2, bits=16)[0], DS_INVALID)

    def test_control_flags_gate_volume_pan_and_frequency(self):
        s = self.startup
        _, plain = self.buffer(flags=0, nbytes=4096)
        self.assertEqual(call(s, plain, "SetVolume", 0), 0x8878001E)
        _, full = self.buffer(flags=0x80 | 0x40 | 0x20, nbytes=4096)
        self.assertEqual(call(s, full, "SetVolume", (-500) & 0xFFFFFFFF), 0)
        self.assertEqual(call(s, full, "SetVolume", 1), DS_INVALID)
        out = s.alloc(4)
        call(s, full, "GetVolume", out)
        self.assertEqual(s.read_u32(out), (-500) & 0xFFFFFFFF)
        self.assertEqual(call(s, full, "SetPan", 10001), DS_INVALID)
        self.assertEqual(call(s, full, "SetFrequency", 99), DS_INVALID)
        self.assertEqual(call(s, full, "SetFrequency", 44100), 0)
        self.assertEqual(call(s, full, "SetFrequency", 0), 0)
        call(s, full, "GetFrequency", out)
        self.assertEqual(s.read_u32(out), 22050)

    def test_lock_wraps_and_unlock_must_match(self):
        s = self.startup
        _, buf = self.buffer(flags=0, nbytes=1024)
        p1, b1, p2, b2 = (s.alloc(4) for _ in range(4))
        self.assertEqual(call(s, buf, "Lock", 1000, 100, p1, b1, p2, b2, 0), 0)
        self.assertEqual((s.read_u32(b1), s.read_u32(b2)), (24, 76))
        self.assertEqual(s.read_u32(p2), buf.model.memory)
        self.assertEqual(s.read_u32(p1), buf.model.memory + 1000)
        with self.assertRaises(Boundary):
            call(s, buf, "Lock", 0, 4, p1, b1, p2, b2, 0)  # relock
        with self.assertRaises(Boundary):
            call(s, buf, "Unlock", s.read_u32(p1), 1, 0, 0)
        self.assertEqual(
            call(s, buf, "Unlock", s.read_u32(p1), 24, s.read_u32(p2), 76), 0
        )
        self.assertEqual(call(s, buf, "Unlock", 0, 0, 0, 0), DS_INVALID)
        self.assertEqual(call(s, buf, "Lock", 2000, 4, p1, b1, p2, b2, 0), DS_INVALID)
        self.assertEqual(call(s, buf, "Lock", 0, 0, p1, b1, p2, b2, 2), 0)
        self.assertEqual(s.read_u32(b1), 1024)
        self.assertEqual(s.read_u32(p2), 0)

    def test_play_cursor_follows_the_virtual_clock_and_stops_at_the_end(self):
        s = self.startup
        _, buf = self.buffer(flags=0, nbytes=44100 * 2, channels=1, rate=22050)  # 2 s at 44100 B/s
        play, write, status = s.alloc(4), s.alloc(4), s.alloc(4)
        call(s, buf, "GetCurrentPosition", play, write)
        self.assertEqual(s.read_u32(play), 0)
        call(s, buf, "Play", 0, 0, 0)
        s.elapsed_ms += 500
        call(s, buf, "GetCurrentPosition", play, write)
        self.assertEqual(s.read_u32(play), 22050)
        lead = 10 * 44100 // 1000  # declared write lead of 10 ms in bytes (441, block aligned to 440)
        self.assertEqual(s.read_u32(write), 22050 + lead - lead % 2)
        call(s, buf, "GetStatus", status)
        self.assertEqual(s.read_u32(status), 1)
        s.elapsed_ms += 5000  # past the end of a non-looping buffer
        call(s, buf, "GetStatus", status)
        self.assertEqual(s.read_u32(status), 0)
        call(s, buf, "Play", 0, 0, 1)
        s.elapsed_ms += 3000
        call(s, buf, "GetStatus", status)
        self.assertEqual(s.read_u32(status), 5)  # playing and looping
        call(s, buf, "Stop")
        call(s, buf, "GetStatus", status)
        self.assertEqual(s.read_u32(status), 0)

    def test_duplicates_share_memory_and_three_d_interfaces_need_the_flag(self):
        s = self.startup
        _, buf = self.buffer(flags=0x10 | 0x80, nbytes=4096, channels=1)
        out = s.alloc(4)
        self.assertEqual(call(s, self.ds, "DuplicateSoundBuffer", buf.pointer, out), 0)
        dup = s.com.objects[s.read_u32(out)]
        self.assertEqual(dup.model.memory, buf.model.memory)
        self.assertIsNot(dup, buf)
        iid = put_guid(s, bytes.fromhex("86fa9a278149ce11a5210020af0be560"))
        self.assertEqual(call(s, buf, "QueryInterface", iid, out), 0)
        three_d = s.com.objects[s.read_u32(out)]
        self.assertEqual(three_d.interface, "IDirectSound3DBuffer")
        _, plain = self.buffer(flags=0, nbytes=4096)
        self.assertEqual(call(s, plain, "QueryInterface", iid, out), 0x80004002)
        self.assertEqual(s.read_u32(out), 0)


if __name__ == "__main__":
    unittest.main()
