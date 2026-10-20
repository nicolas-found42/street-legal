"""Declared Win32 platform pieces needed after window creation.

Each API has an exact DLL/export and stdcall argument count, validated
arguments, and documented semantics. Declared hypotheses (which modules are
absent, which registry keys exist, how window messages are delivered) come from
the platform profile; an API outside the declared sections stops by name.
"""

from __future__ import annotations

import ntpath
import struct

ERROR_SUCCESS = 0
ERROR_FILE_NOT_FOUND = 2
ERROR_MOD_NOT_FOUND = 126
ERROR_MORE_DATA = 234
ERROR_INVALID_HANDLE = 6
S_FALSE = 1
RPC_E_CHANGED_MODE = 0x80010106
HKEY_ROOTS = {
    0x80000000: "HKCR",
    0x80000001: "HKCU",
    0x80000002: "HKLM",
    0x80000003: "HKU",
}
CWD = "C:\\game"


def normalize_key(path):
    parts = [p for p in path.replace("/", "\\").split("\\") if p]
    return "\\".join(parts).lower()


class Win32Platform:
    def __init__(self, host, boundary, profile):
        self.host = host
        self.boundary = boundary
        self.profile = profile
        self.focus = 0
        self.key_handles = {}
        self.next_key = 0x7A000000
        self.library_counts = {}
        self.com_mode = None
        self.last_call = {}
        self.registry_writes = []
        self.api_table = {
            "ShowWindow": ("user32.dll", 2, self.show_window, profile.window is not None),
            "SetFocus": ("user32.dll", 1, self.set_focus, profile.window is not None),
            "GetFullPathNameA": ("kernel32.dll", 4, self.full_path, True),
            "LoadLibraryA": ("kernel32.dll", 1, self.load_library, profile.modules is not None),
            "FreeLibrary": ("kernel32.dll", 1, self.free_library, profile.modules is not None),
            "CoInitializeEx": ("ole32.dll", 2, self.co_initialize_ex, True),
            "CoCreateInstance": ("ole32.dll", 5, self.co_create_instance, profile.com is not None),
            "RegOpenKeyA": ("advapi32.dll", 3, self.reg_open_key, profile.registry is not None),
            "RegOpenKeyExA": ("advapi32.dll", 5, self.reg_open_key_ex, profile.registry is not None),
            "RegQueryValueExA": ("advapi32.dll", 6, self.reg_query_value, profile.registry is not None),
            "RegCloseKey": ("advapi32.dll", 1, self.reg_close, profile.registry is not None),
        }
        if profile.modules is not None:
            for module, names in profile.modules.exports.items():
                for name in names:
                    self.api_table[name] = (module, 0, self.module_stub, True)

    def register(self, startup):
        for name, (module, nargs, handler, active) in self.api_table.items():
            if not active:
                continue
            startup.apis[name] = (nargs, handler)
            startup.api_modules[name] = module
            if startup.enable_all:
                startup.enabled.add(name)

    # ---------------------------------------------------------------- modules
    def module_absent(self, name):
        return self.profile.modules is not None and name in self.profile.modules.absent

    def load_library(self, args):
        host = self.host
        if not args[0]:
            raise self.boundary("unsupported_argument", api="LoadLibraryA", args=args)
        name = host.read_cstr(args[0], 260).decode("cp1252").lower()
        if not name.endswith(".dll"):
            name += ".dll"
        if self.module_absent(name):
            host.last_error = ERROR_MOD_NOT_FOUND
            return 0
        for handle, module in host.modules.items():
            if module.lower() == name:
                self.library_counts[handle] = self.library_counts.get(handle, 0) + 1
                return handle
        raise self.boundary("unsupported_module", name=name)

    def free_library(self, args):
        handle = args[0]
        if self.library_counts.get(handle, 0) == 0:
            raise self.boundary("invalid_module_handle", handle=handle)
        self.library_counts[handle] -= 1
        return 1

    def module_stub(self, args):
        # Declared export stubs (for example retail d3d9.dll!DebugSetMute) do nothing.
        return None

    # ----------------------------------------------------------------- window
    def show_window(self, args):
        handle, command = args
        state = self.host.windows.get(handle)
        if state is None:
            raise self.boundary("unknown_window_handle", api="ShowWindow", handle=handle)
        if command not in (0, 1, 5):
            raise self.boundary("unsupported_show_command", command=command)
        self.require_elided_messages("ShowWindow")
        previous = int(state.get("visible", False))
        state["visible"] = command != 0
        self.last_call = {"api": "ShowWindow", "previous_visible": bool(previous), "messages": "elided"}
        return previous

    def set_focus(self, args):
        handle = args[0]
        if handle and handle not in self.host.windows:
            raise self.boundary("unknown_window_handle", api="SetFocus", handle=handle)
        self.require_elided_messages("SetFocus")
        previous = self.focus
        self.focus = handle
        self.last_call = {"api": "SetFocus", "previous": previous, "messages": "elided"}
        return previous

    def require_elided_messages(self, api):
        if self.profile.window is None or self.profile.window.message_delivery != "elided":
            raise self.boundary("undeclared_window_message_delivery", api=api)

    # -------------------------------------------------------------- file path
    def full_path(self, args):
        pointer, capacity, buffer, part = args
        host = self.host
        if not pointer:
            raise self.boundary("unsupported_argument", api="GetFullPathNameA", args=args)
        raw = host.read_cstr(pointer, 4096).decode("cp1252")
        if not raw:
            host.last_error = 3
            return 0
        if raw[1:2] == ":" and raw[2:3] not in ("\\", "/"):
            raise self.boundary("unsupported_drive_relative_path", path=raw)
        full = ntpath.normpath(raw if ntpath.isabs(raw) else ntpath.join(CWD, raw))
        data = full.encode("cp1252")
        if capacity <= len(data):
            return len(data) + 1
        host.write(buffer, data + b"\0")
        if part:
            index = full.rfind("\\")
            host.write_u32(part, buffer + index + 1 if index >= 0 and full[-1] != "\\" else 0)
        return len(data)

    # ------------------------------------------------------------------- COM
    def co_initialize_ex(self, args):
        reserved, mode = args
        host = self.host
        if reserved or mode not in (0, 2):
            raise self.boundary("unsupported_argument", api="CoInitializeEx", args=args)
        wanted = "sta" if mode == 2 else "mta"
        if host.com_depth and self.com_mode is None:
            self.com_mode = "sta"  # earlier CoInitialize is apartment threaded
        if host.com_depth and self.com_mode != wanted:
            return RPC_E_CHANGED_MODE
        previous = host.com_depth
        host.com_depth += 1
        self.com_mode = wanted
        return S_FALSE if previous else 0

    def co_create_instance(self, args):
        clsid, outer, context, iid, out = args
        host = self.host
        if not clsid or not iid or not out or outer:
            raise self.boundary("unsupported_argument", api="CoCreateInstance", args=args)
        if host.com_depth == 0:
            return 0x800401F0  # CO_E_NOTINITIALIZED
        wanted = host.read(clsid, 16)
        if wanted in self.profile.com.unregistered:
            host.write_u32(out, 0)  # documented: *ppv is NULL on failure
            self.last_call = {"api": "CoCreateInstance", "clsid": wanted.hex(), "result": "REGDB_E_CLASSNOTREG"}
            return 0x80040154
        raise self.boundary("unsupported_clsid", clsid=wanted.hex())

    # --------------------------------------------------------------- registry
    def key_path(self, root, subkey):
        if root in HKEY_ROOTS:
            base = HKEY_ROOTS[root]
        elif root in self.key_handles:
            base = self.key_handles[root]
        else:
            return None
        return base.lower() + ("\\" + normalize_key(subkey) if subkey else "")

    def _open(self, args, nargs):
        root, subkey_pointer, out = args[0], args[1], args[-1]
        host = self.host
        subkey = host.read_cstr(subkey_pointer, 260).decode("cp1252") if subkey_pointer else ""
        path = self.key_path(root, subkey)
        if path is None or not out:
            return ERROR_INVALID_HANDLE
        if path not in self.profile.registry.keys:
            return ERROR_FILE_NOT_FOUND
        if len(self.key_handles) >= 64:
            raise self.boundary("registry_handle_budget", limit=64)
        handle = self.next_key
        self.next_key += 4
        self.key_handles[handle] = path
        host.write_u32(out, handle)
        self.last_call = {"api": "RegOpenKey", "path": path, "handle": handle}
        return ERROR_SUCCESS

    def reg_open_key(self, args):
        return self._open(args, 3)

    def reg_open_key_ex(self, args):
        root, sub, options, sam, out = args
        if options:
            raise self.boundary("unsupported_argument", api="RegOpenKeyExA", args=args)
        return self._open([root, sub, out], 5)

    def reg_query_value(self, args):
        handle, name_pointer, reserved, type_out, data, size_pointer = args
        host = self.host
        if reserved:
            return 87  # ERROR_INVALID_PARAMETER
        path = self.key_handles.get(handle)
        if path is None and handle in HKEY_ROOTS:
            path = HKEY_ROOTS[handle].lower()
        if path is None:
            return ERROR_INVALID_HANDLE
        name = host.read_cstr(name_pointer, 260).decode("cp1252").lower() if name_pointer else ""
        values = self.profile.registry.keys.get(path)
        if values is None or name not in values:
            return ERROR_FILE_NOT_FOUND
        kind, payload = values[name]
        if type_out:
            host.write_u32(type_out, kind)
        if not size_pointer:
            if data:
                return 87
            return ERROR_SUCCESS
        capacity = host.read_u32(size_pointer)
        host.write_u32(size_pointer, len(payload))
        if data:
            if capacity < len(payload):
                return ERROR_MORE_DATA
            host.write(data, payload)
        return ERROR_SUCCESS

    def reg_close(self, args):
        if args[0] not in self.key_handles:
            return ERROR_INVALID_HANDLE
        del self.key_handles[args[0]]
        return ERROR_SUCCESS


def encode_registry_value(kind, value):
    if kind == "REG_DWORD":
        return 4, struct.pack("<I", value)
    if kind == "REG_SZ":
        return 1, value.encode("cp1252") + b"\0"
    raise ValueError("unsupported registry value type")
