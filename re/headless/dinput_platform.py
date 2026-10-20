"""Declared DirectInput 8 devices: a keyboard and a mouse with no input events.

`DirectInput8Create` and the IDirectInput8A / IDirectInputDevice8A methods the
original uses follow the documented contracts (acquire state, data-format
sizes, buffered-data semantics). The attached hardware and its capabilities
come from the platform profile's `input` section; there is no host input. All
device states are declared quiescent: keys up, mouse still, buttons released.
Each GetDeviceState is recorded as a DirectInput poll in the call log.
"""

from __future__ import annotations

import struct

import calllog

DI_OK = 0
DIERR_INVALIDPARAM = 0x80070057
DIERR_NOTACQUIRED = 0x8007000C
DIERR_NOTINITIALIZED = 0x80070015
DIERR_DEVICENOTREG = 0x80040154
DIERR_OTHERAPPHASPRIO = 0x80070005
DIERR_UNSUPPORTED = 0x80004001

GUID_SYS_MOUSE = bytes.fromhex("602b1d6fa0d5cf11bfc7444553540000")
GUID_SYS_KEYBOARD = bytes.fromhex("612b1d6fa0d5cf11bfc7444553540000")
IID_IDIRECTINPUT8A = struct.pack(
    "<IHH", 0xBF798030, 0x483A, 0x4DA2
) + bytes.fromhex("aa995d64ed369700")
DIRECTINPUT_VERSION = 0x0800
DIPROP_BUFFERSIZE = 1


class InputProfile:
    def __init__(self, data):
        if not isinstance(data, dict) or set(data) != {
            "keyboard", "mouse", "game_controllers", "keyboard_layout", "cursor_position"
        }:
            raise ValueError(
                "input needs exactly keyboard, mouse, game_controllers, "
                "keyboard_layout and cursor_position"
            )
        position = data["cursor_position"]
        if (
            not isinstance(position, list)
            or len(position) != 2
            or any(type(v) is not int or not -32768 <= v <= 32767 for v in position)
        ):
            raise ValueError("cursor_position is [x, y] signed 16-bit coordinates")
        self.cursor_position = tuple(position)
        if type(data["keyboard_layout"]) is not int or not 0 < data["keyboard_layout"] <= 0xFFFFFFFF:
            raise ValueError("keyboard_layout must be a nonzero HKL word")
        self.keyboard_layout = data["keyboard_layout"]
        if data["game_controllers"] != []:
            raise ValueError("only an empty game controller list is modelled")
        self.devices = {}
        fields = ("flags", "devtype", "axes", "buttons", "povs")
        for name in ("keyboard", "mouse"):
            spec = data[name]
            if not isinstance(spec, dict) or set(spec) != set(fields):
                raise ValueError(f"{name} needs exactly {', '.join(fields)}")
            for key in fields:
                if type(spec[key]) is not int or not 0 <= spec[key] <= 0xFFFFFFFF:
                    raise ValueError(f"{name}.{key} must be an unsigned 32-bit integer")
            self.devices[name] = {key: spec[key] for key in fields}


class DirectInputPlatform:
    def __init__(self, host, boundary, profile):
        self.host = host
        self.boundary = boundary
        self.profile = profile
        self.interfaces = 0
        self.devices = []
        self.polls = 0
        self.cursor = 0
        self.cursor_count = 0  # mouse installed: display counter starts at 0
        self.cursor_pos = list(profile.cursor_position)
        self.clip = None

    def register(self, startup):
        for name, module, nargs, handler in (
            ("DirectInput8Create", "dinput8.dll", 5, self.create),
            ("GetKeyboardLayout", "user32.dll", 1, self.keyboard_layout),
            ("SetCursor", "user32.dll", 1, self.set_cursor),
            ("ShowCursor", "user32.dll", 1, self.show_cursor),
            ("ClipCursor", "user32.dll", 1, self.clip_cursor),
            ("SetCursorPos", "user32.dll", 2, self.set_cursor_pos),
            ("GetCursorPos", "user32.dll", 1, self.get_cursor_pos),
            ("GetAsyncKeyState", "user32.dll", 1, self.async_key_state),
            ("GetKeyboardState", "user32.dll", 1, self.keyboard_state),
        ):
            startup.apis[name] = (nargs, handler)
            startup.api_modules[name] = module
            if startup.enable_all:
                startup.enabled.add(name)

    def set_cursor(self, args):
        handle = args[0]
        known = set(self.host.desktop.handles.values()) if self.host.desktop else set()
        if handle and handle not in known:
            raise self.boundary("unknown_cursor_handle", handle=handle)
        previous = self.cursor
        self.cursor = handle
        return previous

    def show_cursor(self, args):
        self.cursor_count += 1 if args[0] else -1
        return self.cursor_count & 0xFFFFFFFF

    def clip_cursor(self, args):
        if args[0]:
            left, top, right, bottom = struct.unpack("<4i", self.host.read(args[0], 16))
            if left >= right or top >= bottom:
                return 0
            self.clip = (left, top, right, bottom)
        else:
            self.clip = None
        return 1

    def set_cursor_pos(self, args):
        x, y = (v - (1 << 32) if v >= 1 << 31 else v for v in args)
        if self.clip is not None:
            left, top, right, bottom = self.clip
            x = min(max(x, left), right - 1)
            y = min(max(y, top), bottom - 1)
        self.cursor_pos = [x, y]
        return 1

    def get_cursor_pos(self, args):
        if not args[0]:
            return 0
        self.host.write(args[0], struct.pack("<2i", *self.cursor_pos))
        return 1

    def async_key_state(self, args):
        if args[0] > 0xFF:
            raise self.boundary("unsupported_argument", api="GetAsyncKeyState", args=args)
        return 0  # declared: every key is up

    def keyboard_state(self, args):
        if not args[0]:
            return 0
        self.host.write(args[0], bytes(256))
        return 1

    def keyboard_layout(self, args):
        if args[0] != 0:  # only the calling thread's layout is modelled
            raise self.boundary("unsupported_argument", api="GetKeyboardLayout", args=args)
        return self.profile.keyboard_layout

    def create(self, args):
        hinst, version, riid, out, outer = args
        host = self.host
        if hinst != host.base or version != DIRECTINPUT_VERSION or outer:
            raise self.boundary("unsupported_argument", api="DirectInput8Create", args=args)
        if not riid or not out or host.read(riid, 16) != IID_IDIRECTINPUT8A:
            raise self.boundary("unsupported_iid", api="DirectInput8Create", iid=riid)
        if self.interfaces >= 1:
            raise self.boundary("second_directinput_interface")
        self.interfaces += 1
        obj = host.com.new_object("IDirectInput8A", InterfaceModel(self))
        host.write_u32(out, obj.pointer)
        return DI_OK


class InterfaceModel:
    def __init__(self, platform):
        self.platform = platform
        self.created = {}

    def CreateDevice(self, obj, a):
        guid, out, outer = a
        host = self.platform.host
        if outer or not out or not guid:
            return DIERR_INVALIDPARAM
        raw = host.read(guid, 16)
        if raw == GUID_SYS_KEYBOARD:
            kind = "keyboard"
        elif raw == GUID_SYS_MOUSE:
            kind = "mouse"
        else:
            return DIERR_DEVICENOTREG
        if kind in self.created:
            raise self.platform.boundary("second_device_of_kind", device=kind)
        model = DeviceModel(self.platform, kind)
        device = host.com.new_object("IDirectInputDevice8A", model)
        self.created[kind] = device
        self.platform.devices.append(device)
        host.write_u32(out, device.pointer)
        return DI_OK

    def EnumDevices(self, obj, a):
        devtype, callback, ref, flags = a
        # DI8DEVCLASS_GAMECTRL, attached devices only: no controller is declared,
        # so the callback is never invoked.
        if devtype != 4 or flags != 1 or not callback:
            raise self.platform.boundary("unsupported_enum_devices", args=a)
        return DI_OK


class DeviceModel:
    def __init__(self, platform, kind):
        self.platform = platform
        self.kind = kind
        self.spec = platform.profile.devices[kind]
        self.acquired = False
        self.data_size = None
        self.coop = None
        self.buffer_size = 0

    def GetCapabilities(self, obj, a):
        pointer = a[0]
        host = self.platform.host
        if not pointer or host.read_u32(pointer) not in (44, 24):
            return DIERR_INVALIDPARAM
        s = self.spec
        words = [44, s["flags"], s["devtype"], s["axes"], s["buttons"], s["povs"], 0, 0, 0, 0, 0]
        host.write_u32s(pointer, words[: host.read_u32(pointer) // 4])
        return DI_OK

    def SetDataFormat(self, obj, a):
        pointer = a[0]
        host = self.platform.host
        if not pointer:
            return DIERR_INVALIDPARAM
        size, objsize, flags, data_size, count = host.read_u32s(pointer, 5)
        expected = (0x100, 0x100) if self.kind == "keyboard" else (0x10, 7)
        if size != 0x18 or objsize != 0x10 or (data_size, count) != expected:
            raise self.platform.boundary(
                "unsupported_data_format", device=self.kind, words=[size, objsize, flags, data_size, count]
            )
        if self.acquired:
            return 0x80070005  # DIERR_ACCESSDENIED while acquired
        self.data_size = data_size
        return DI_OK

    def SetCooperativeLevel(self, obj, a):
        hwnd, flags = a
        if hwnd not in self.platform.host.windows:
            return DIERR_INVALIDPARAM
        exclusive = flags & 3
        background = flags & 0xC
        if exclusive not in (1, 2) or background not in (4, 8) or flags & ~0x1F:
            return DIERR_INVALIDPARAM
        self.coop = flags
        return DI_OK

    def SetProperty(self, obj, a):
        guid, header = a
        host = self.platform.host
        if guid != DIPROP_BUFFERSIZE or not header:
            raise self.platform.boundary("unsupported_property", guid=guid)
        size, header_size, how_obj, how, value = host.read_u32s(header, 5)
        if size != 20 or header_size != 16 or how_obj != 0 or how != 0:
            return DIERR_INVALIDPARAM
        if self.acquired:
            return 0x80070005
        self.buffer_size = value
        return DI_OK

    def Acquire(self, obj, a):
        if self.data_size is None or self.coop is None:
            raise self.platform.boundary("acquire_before_setup", device=self.kind)
        self.acquired = True
        return DI_OK

    def Unacquire(self, obj, a):
        self.acquired = False
        return DI_OK

    def GetDeviceState(self, obj, a):
        size, pointer = a
        host = self.platform.host
        if not pointer or size != self.data_size:
            return DIERR_INVALIDPARAM
        if not self.acquired:
            return DIERR_NOTACQUIRED
        state = bytes(size)  # declared quiescent device
        host.write(pointer, state)
        self.platform.polls += 1
        if host.calllog is not None:
            host.calllog.record(calllog.dinput_poll(0 if self.kind == "keyboard" else 1, state))
        return DI_OK

    def GetDeviceData(self, obj, a):
        size, data, count_pointer, flags = a
        host = self.platform.host
        if size != 20 or not count_pointer or flags & ~1:
            return DIERR_INVALIDPARAM
        if not self.acquired:
            return DIERR_NOTACQUIRED
        if data and host.read_u32(count_pointer) == 0xFFFFFFFF:
            return DIERR_INVALIDPARAM
        host.write_u32(count_pointer, 0)  # declared: no buffered events
        return DI_OK
