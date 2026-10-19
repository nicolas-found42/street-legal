"""Bounded original-executable startup with strict deterministic Win32 boundaries.

No native/CRT/game bodies are replaced. Every unresolved import is a named stop,
never the permissive zero-return default in the single-function Oracle.
Outputs are diagnostic startup traces, not evidence of MainMenu acceptance.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import struct
import sys
import unicodedata
from collections import deque
from datetime import datetime, timedelta
from pathlib import Path

from desktop import Desktop, DesktopProfile, WindowCallback
from graphics import Direct3D9, GraphicsProfile
from media import MciDevice, MediaProfile
from window_messages import CREATION_MESSAGE_NAMES

sys.path.insert(0, str(Path(__file__).parents[1] / "oracle"))
from oracle import TEB_BASE, UC_PROT_ALL, Oracle, OracleError

ENTRY = 0x5514C0
WINMAIN = 0x58C700
THREAD_RETURN = 0x7FFF0100
WINDOW_CALLBACK_RETURN = 0x7FFF0200
REPO = Path(__file__).resolve().parents[2]
OUTPUT_ROOT = REPO / "re/out/headless"
EXE_SHA256 = "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"


def scratch_output(path):
    root = OUTPUT_ROOT.resolve()
    target = Path(path).resolve()
    if not root.is_relative_to(REPO / "re/out") or not target.is_relative_to(root):
        raise ValueError("output must stay inside this worktree re/out/headless/")
    return target


class Boundary(OracleError):
    def __init__(self, kind, **facts):
        self.facts = {"kind": kind, **facts}
        super().__init__(str(self.facts))


class Startup(Oracle):
    def __init__(
        self,
        exe,
        enabled=None,
        *,
        media_profile=None,
        graphics_profile=None,
        desktop_profile=None,
        record_limit=10000,
    ):
        if type(record_limit) is not int or not 1 <= record_limit <= 100000:
            raise ValueError("event limit must be an integer from 1 to 100000")
        if media_profile is not None and not isinstance(media_profile, MediaProfile):
            raise ValueError("media_profile must be a validated MediaProfile")
        self.media_profile = media_profile
        if graphics_profile is not None and not isinstance(
            graphics_profile, GraphicsProfile
        ):
            raise ValueError("graphics_profile must be a validated GraphicsProfile")
        self.graphics_profile = graphics_profile
        if desktop_profile is not None and not isinstance(
            desktop_profile, DesktopProfile
        ):
            raise ValueError("desktop_profile must be a validated DesktopProfile")
        self.desktop_profile = desktop_profile
        self.desktop_apis = set()
        self.pending_window_callbacks = {}
        self.enable_all = enabled is None
        self.com_methods = {}
        self.exe_sha256 = hashlib.sha256(Path(exe).read_bytes()).hexdigest()
        if self.exe_sha256 != EXE_SHA256:
            raise ValueError("expected read-only original build606 .bak.exe")
        self.events = []
        self.trail = deque(maxlen=32)
        self.winmain = False
        self.record_limit = record_limit
        super().__init__(str(exe))
        # The PE's IAT is resolved into emulator thunks, exactly as Oracle does;
        # all thunk entries are intercepted before those default bodies execute.
        self.modules = {self.base: "StreetLegal_Redline.exe"}
        for index, entry in enumerate(self.pe.DIRECTORY_ENTRY_IMPORT):
            self.modules[0x11000000 + index * 0x10000] = entry.dll.decode().lower()
        self.dynamic_exports = {}
        self.tls = {}
        self.locks = {}
        self.environments = {}
        self.last_error = 0
        self.heaps = {}
        self.top_exception_filter = 0
        self.com_depth = 0
        self.stock = Path(exe).absolute().parents[2] / "stock"
        self.overlay = {}
        self.files = {}
        self.next_file_handle = 0x20000000
        self.semaphores = {}
        self.next_semaphore = 0x32000000
        self.elapsed_ms = 0
        self.thread_id = 1
        self.threads = {1: {"status": "runnable", "tls": self.tls, "last_error": 0}}
        self.write_u32s(TEB_BASE + 0x20, [1, 1])
        self.next_quantum = 1000
        self.tls_slots = set()
        self.finds = {}
        self.next_find_handle = 0x60000000
        self.mutexes = {}
        self.named_mutexes = {}
        self.next_mutex = 0x31000000
        self.windows = {}
        self.apis = {
            "MessageBoxA": (4, self.message_box),
            "FindWindowA": (2, self.find_window),
            "GetDriveTypeA": (1, self.drive_type),
            "GetSystemInfo": (1, self.system_info),
            "GlobalMemoryStatus": (1, self.memory_status),
            "GetFileSize": (2, self.file_size),
            "SetFilePointer": (4, self.file_seek),
            "CreateMutexA": (3, self.mutex_create),
            "ReleaseMutex": (1, self.mutex_release),
            "FindFirstFileA": (2, self.find_first),
            "FindNextFileA": (2, self.find_next),
            "FindClose": (1, self.find_close),
            "GetTickCount": (0, lambda a: self.clock_ms() & 0xFFFFFFFF),
            "QueryPerformanceFrequency": (1, self.performance_frequency),
            "QueryPerformanceCounter": (1, self.performance_counter),
            "CreateThread": (6, self.thread_create),
            "CreateSemaphoreA": (4, self.semaphore_create),
            "ReleaseSemaphore": (3, self.semaphore_release),
            "WaitForSingleObject": (2, self.wait_single),
            "GetSystemTime": (1, self.system_time),
            "CreateFileA": (7, self.file_create),
            "WriteFile": (5, self.file_write),
            "ReadFile": (5, self.file_read),
            "CloseHandle": (1, self.file_close),
            "CoInitialize": (1, self.com_init),
            "CoUninitialize": (0, self.com_uninit),
            "SetUnhandledExceptionFilter": (1, self.exception_filter),
            "GetVersion": (0, self.get_version),
            "GetVersionExA": (1, self.get_version_ex),
            "HeapCreate": (3, self.heap_create),
            "HeapAlloc": (3, self.heap_alloc),
            "HeapFree": (3, self.heap_free),
            "HeapReAlloc": (4, self.heap_realloc),
            "HeapSize": (3, self.heap_size),
            "InitializeCriticalSection": (1, self.cs_init),
            "EnterCriticalSection": (1, self.cs_enter),
            "LeaveCriticalSection": (1, self.cs_leave),
            "DeleteCriticalSection": (1, self.cs_delete),
            "TlsAlloc": (0, self.tls_alloc),
            "TlsGetValue": (1, self.tls_get),
            "TlsSetValue": (2, self.tls_set),
            "GetCurrentThreadId": (0, lambda a: self.thread_id),
            "GetLastError": (0, lambda a: self.last_error),
            "SetLastError": (1, self.set_last_error),
            "GetStartupInfoA": (1, self.startup_info),
            "GetStdHandle": (1, self.std_handle),
            "GetFileType": (1, self.file_type),
            "GetCommandLineA": (0, self.command_line),
            "GetModuleFileNameA": (3, self.module_filename),
            "IsProcessorFeaturePresent": (1, self.processor_feature),
            "GetProcAddress": (2, self.proc_address),
            "GetModuleHandleA": (1, self.module_handle),
            "SetHandleCount": (1, lambda a: a[0]),
            "GetEnvironmentStringsW": (0, self.environment_strings),
            "FreeEnvironmentStringsW": (1, self.environment_free),
            "WideCharToMultiByte": (8, self.wide_to_multi),
            "GetACP": (0, lambda a: 1252),
            "GetCPInfo": (2, self.cp_info),
            "GetStringTypeW": (4, self.string_type),
            "MultiByteToWideChar": (6, self.multi_to_wide),
            "LCMapStringW": (6, self.map_string),
        }
        self.enabled = set(self.apis) if enabled is None else enabled
        self.api_modules = {name: "kernel32.dll" for name in self.apis}
        self.api_modules.update(CoInitialize="ole32.dll", CoUninitialize="ole32.dll")
        self.api_modules.update(FindWindowA="user32.dll", MessageBoxA="user32.dll")
        self.mci = (
            MciDevice(media_profile, self, Boundary)
            if media_profile is not None
            else None
        )
        if self.mci is not None:
            self.apis["mciSendCommandA"] = (4, self.mci_command)
            self.api_modules["mciSendCommandA"] = "winmm.dll"
            if enabled is None:
                self.enabled.add("mciSendCommandA")
        self.graphics = (
            Direct3D9(graphics_profile, self, Boundary, self.allocate_graphics_method)
            if graphics_profile is not None
            else None
        )
        if self.graphics is not None:
            self.apis["Direct3DCreate9"] = (1, self.direct3d_create)
            self.api_modules["Direct3DCreate9"] = "d3d9.dll"
            if enabled is None:
                self.enabled.add("Direct3DCreate9")

        self.desktop = (
            Desktop(desktop_profile, self, Boundary)
            if desktop_profile is not None
            else None
        )
        if self.desktop is not None:
            for name, nargs, module, handler in (
                ("LoadCursorA", 2, "user32.dll", self.desktop.cursor),
                ("LoadIconA", 2, "user32.dll", self.desktop.icon),
                ("RegisterClassA", 1, "user32.dll", self.desktop.register_class),
                ("CreateWindowExA", 12, "user32.dll", self.desktop.create_window),
                ("GetStockObject", 1, "gdi32.dll", self.desktop.stock_object),
            ):
                self.apis[name] = (nargs, handler)
                self.api_modules[name] = module
                self.desktop_apis.add(name)
                if enabled is None:
                    self.enabled.add(name)
            if self.desktop.messages is not None:
                self.apis["DefWindowProcA"] = (4, self.desktop.default_window_proc)
                self.api_modules["DefWindowProcA"] = "user32.dll"
                self.desktop_apis.add("DefWindowProcA")
                if enabled is None:
                    self.enabled.add("DefWindowProcA")

    def begin_window_callback(self, request):
        if len(request.args) != 4 or request.args[1] not in CREATION_MESSAGE_NAMES:
            raise Boundary("unsupported_window_callback", args=list(request.args))
        if (
            self.thread_id in self.pending_window_callbacks
            or len(self.pending_window_callbacks) >= 4
        ):
            raise Boundary("window_callback_depth", per_thread_limit=1, total_limit=4)
        if len(self.events) >= self.record_limit:
            raise Boundary("record_limit", limit=self.record_limit)
        context = self.uc.context_save()
        stack = (self.reg("esp") - 4 * (len(request.args) + 1) - 64) & ~15
        self.write_u32s(stack, [WINDOW_CALLBACK_RETURN, *request.args])
        self.pending_window_callbacks[self.thread_id] = {
            "context": context,
            "request": request,
            "return_esp": stack + 4 * (len(request.args) + 1),
        }
        self.events.append(
            {
                "callback": CREATION_MESSAGE_NAMES[request.args[1]],
                "target": request.target,
                "args": list(request.args),
                "hwnd": request.hwnd,
                "thread": self.thread_id,
            }
        )
        if request.args[1] == 0x83:
            self.events[-1]["structure"] = {
                "type": "RECT",
                "address": request.args[3],
                "words": self.read_u32s(request.args[3], 4),
            }
        self.set_reg("esp", stack)
        self.set_reg("eip", request.target)

    def finish_window_callback(self, uc):
        frame = self.pending_window_callbacks.get(self.thread_id)
        if frame is None:
            self._err = Boundary("unexpected_window_callback_return")
            uc.emu_stop()
            return
        if self.reg("esp") != frame["return_esp"]:
            self._err = Boundary(
                "window_callback_stack",
                expected_esp=frame["return_esp"],
                actual_esp=self.reg("esp"),
            )
            uc.emu_stop()
            return
        self.pending_window_callbacks.pop(self.thread_id)
        result = self.reg("eax")
        request = frame["request"]
        self.uc.context_restore(frame["context"])
        self.set_reg("eip", self.reg("eip"))
        self.icount += 1
        self.events.append(
            {
                "callback_return": CREATION_MESSAGE_NAMES[request.args[1]],
                "callback_result": result,
                "hwnd": request.hwnd,
                "thread": self.thread_id,
            }
        )
        try:
            next_request = self.desktop.finish_callback(request, result)
            if not isinstance(next_request, WindowCallback):
                raise Boundary("unsupported_window_callback_action")
            self.begin_window_callback(next_request)
        except Boundary as error:
            self._err = error
            uc.emu_stop()
        except (OracleError, ValueError) as error:
            self._err = Boundary("window_callback_error", message=str(error))
            uc.emu_stop()

    def direct3d_create(self, args):
        if self.graphics is None:
            raise Boundary("unsupported_import", name="d3d9.dll!Direct3DCreate9")
        return self.graphics.create(args)

    def allocate_graphics_method(self, slot, name, nargs):
        address = 0x7F000000 + self._thunk_slots * 16
        if address >= 0x7F010000:
            raise Boundary("thunk_limit")
        self._thunk_slots += 1
        key = "IDirect3D9." + name
        self.apis[key] = (nargs, lambda args: self.graphics.call(slot, args))
        self.api_modules[key] = "com.IDirect3D9"
        self.import_names[address] = "com.IDirect3D9!" + key
        self.com_methods[address] = (slot, name)
        if self.enable_all:
            self.enabled.add(key)
        return address

    def mci_command(self, args):
        if self.mci is None:
            raise Boundary("unsupported_import", name="winmm.dll!mciSendCommandA")
        try:
            return self.mci.send_command(args)
        finally:
            if (
                self.events
                and self.events[-1].get("import") == "winmm.dll!mciSendCommandA"
            ):
                self.events[-1]["mci"] = self.mci.last_call

    def _on_write(self, uc, access, address, size, value, ud):
        # A bounded boot must not retain millions of unneeded memory writes.
        pass

    def _on_code(self, uc, address, size, ud):
        if len(self.events) >= self.record_limit:
            self._err = Boundary("record_limit", limit=self.record_limit)
            uc.emu_stop()
            return
        if address == THREAD_RETURN:
            self.threads[self.thread_id]["status"] = "terminated"
            self.threads[self.thread_id]["exit_code"] = self.reg("eax")
            self.schedule()
            return
        if (
            self.icount >= self.next_quantum
            and len(self.threads) > 1
            and self.schedule()
        ):
            return
        if self.limit and self.icount >= self.limit:
            self._err = Boundary("instruction_limit", limit=self.limit, eip=address)
            uc.emu_stop()
            return
        if address == WINDOW_CALLBACK_RETURN:
            self.finish_window_callback(uc)
            return
        if 0x7F000000 <= address < 0x7F010000 and address not in self.import_names:
            self._err = Boundary("unexpected_thunk_entry", eip=address)
            uc.emu_stop()
            return
        self.trail.append(address)
        if address == WINMAIN:
            self.winmain = True
        name = self.import_names.get(address)
        if name:
            self.icount += 1
            return_site = self.read_u32(self.reg("esp"))
            short = name.split("!", 1)[1]
            api = self.apis.get(short)
            if (
                api is None
                or short not in self.enabled
                or name != self.api_modules[short] + "!" + short
            ):
                self._err = Boundary(
                    "unsupported_import", name=name, return_site=return_site
                )
                uc.emu_stop()
                return
            if len(self.events) >= self.record_limit:
                self._err = Boundary("record_limit", limit=self.record_limit)
                uc.emu_stop()
                return
            nargs, handler = api
            args = self.read_u32s(self.reg("esp") + 4, nargs)
            event = {
                "import": name,
                "return_site": return_site,
                "args": args,
                "thread": self.thread_id,
            }
            self.events.append(event)
            if (
                address in self.com_methods
                or short == "Direct3DCreate9"
                or short in self.desktop_apis
            ):
                method = short
                if address in self.com_methods:
                    slot, method = self.com_methods[address]
                    event.pop("import")
                    event.update(com="IDirect3D9", slot=slot, method=method)
                try:
                    result = handler(args)
                    if isinstance(result, WindowCallback):
                        self.begin_window_callback(result)
                        return
                except Boundary as error:
                    self._err = error
                    uc.emu_stop()
                    return
                except (OracleError, ValueError) as error:
                    self._err = Boundary(
                        "desktop_error" if short in self.desktop_apis else "com_error",
                        method=method,
                        message=str(error),
                    )
                    uc.emu_stop()
                    return
                finally:
                    if address in self.com_methods:
                        event["graphics"] = self.graphics.last_call
                    elif short in self.desktop_apis:
                        event["desktop"] = self.desktop.last_call
                event["result"] = result
                self.hooks[address] = (lambda oracle, actual: result, "stdcall", nargs)
                self._run_hook(address)
                return

            # Reject unsupported calls before _run_hook can simulate a return.
            try:
                result = handler(args)
            except Boundary as error:
                self._err = error
                uc.emu_stop()
                return
            except (OracleError, UnicodeError, ValueError) as error:
                self._err = Boundary("platform_error", api=name, message=str(error))
                uc.emu_stop()
                return
            event["result"] = result
            self.hooks[address] = (lambda oracle, actual: result, "stdcall", nargs)
            self._run_hook(address)
            if not self._err and self.threads[self.thread_id]["status"] != "runnable":
                self.schedule()
            return
        super()._on_code(uc, address, size, ud)

    def get_version(self, args):
        # Explicit NT5.1/build2600 environment profile, not host OS detection.
        return (2600 << 16) | (1 << 8) | 5

    def exception_filter(self, args):
        previous = self.top_exception_filter
        self.top_exception_filter = args[0]
        return previous

    def com_init(self, args):
        if args[0]:
            raise Boundary("unsupported_argument", api="CoInitialize", args=args)
        previous = self.com_depth
        self.com_depth += 1
        return int(previous > 0)

    def com_uninit(self, args):
        if not self.com_depth:
            raise Boundary("unbalanced_com_uninitialize")
        self.com_depth -= 1

    def system_time(self, args):
        # Explicit virtual epoch: 1ms per 1000 emulated instructions, UTC.
        value = datetime(2003, 1, 1) + timedelta(milliseconds=self.clock_ms())
        self.write(
            args[0],
            struct.pack(
                "<8H",
                value.year,
                value.month,
                (value.weekday() + 1) % 7,
                value.day,
                value.hour,
                value.minute,
                value.second,
                value.microsecond // 1000,
            ),
        )

    def clock_ms(self):
        return self.icount // 1000 + self.elapsed_ms

    def performance_frequency(self, args):
        self.write(args[0], struct.pack("<Q", 1000000))
        return 1

    def performance_counter(self, args):
        self.write(args[0], struct.pack("<Q", self.icount + self.elapsed_ms * 1000))
        return 1

    def memory_status(self, args):
        self.write_u32s(
            args[0],
            [
                32,
                25,
                0x20000000,
                0x18000000,
                0x40000000,
                0x30000000,
                0x7FFF0000,
                0x70000000,
            ],
        )

    def system_info(self, args):
        self.write_u32s(
            args[0], [0, 4096, 0x10000, 0x7FFEFFFF, 1, 1, 586, 65536, 0x03030006]
        )

    def find_window(self, args):
        cls, title = (
            self.read_cstr(p, 32768).decode("cp1252") if p > 0xFFFF else p for p in args
        )
        for handle, state in self.windows.items():
            if (not cls or cls == state["class"]) and (
                not title or title == state["title"]
            ):
                return handle
        return 0

    def drive_type(self, args):
        if not args[0]:
            return 3
        root = self.read_cstr(args[0], 260).decode("cp1252").replace("\\", "/").lower()
        if len(root) != 3 or root[1:] != ":/" or not "a" <= root[0] <= "z":
            raise Boundary("unsupported_drive_root", path=root)
        if (
            self.media_profile is not None
            and root[0] == self.media_profile.drive.lower()
        ):
            return 5
        return 3 if root[0] == "c" else 1

    def message_box(self, args):
        owner, text, caption, style = args
        raise Boundary(
            "dialog_request",
            api="user32.dll!MessageBoxA",
            owner=owner,
            text=self.read_cstr(text, 32768).decode("cp1252"),
            caption=self.read_cstr(caption, 32768).decode("cp1252"),
            style=style,
        )

    def virtual_path(self, pointer):
        raw = self.read_cstr(pointer, 4096).decode("cp1252").replace("\\", "/").lower()
        raw = raw.removeprefix("c:/game/")
        if raw.startswith("/") or ":" in raw or any(p == ".." for p in raw.split("/")):
            raise Boundary("unsupported_path", path=raw)
        parts = [p for p in raw.split("/") if p not in ("", ".")]
        if not parts:
            raise Boundary("unsupported_path", path=raw)
        return "/".join(parts)

    def stock_path(self, key):
        root = self.stock.resolve()
        path = root
        for part in key.split("/"):
            if not path.is_dir():
                return None
            matches = [p for p in path.iterdir() if p.name.lower() == part]
            if len(matches) > 1:
                raise Boundary("ambiguous_stock_path", path=key)
            if not matches:
                return None
            path = matches[0]
            if not path.resolve().is_relative_to(root):
                raise Boundary("stock_symlink_escape", path=key)
        return path

    def file_bytes(self, key):
        if key in self.overlay:
            return bytes(self.overlay[key])
        path = self.stock_path(key)
        if path is None or not path.is_file():
            return None
        if path.stat().st_size > 64 * 1024 * 1024:
            raise Boundary("file_size_limit", path=key)
        return path.read_bytes()

    def file_create(self, args):
        pointer, access, share, security, creation, flags, template = args
        if (
            access & ~0xC0000000
            or share & ~3
            or security
            or template
            or flags not in (0, 128)
            or creation not in (1, 2, 3, 4, 5)
        ):
            raise Boundary("unsupported_argument", api="CreateFileA", args=args)
        key = self.virtual_path(pointer)
        if "*" in key or "?" in key:
            raise Boundary("unsupported_file_path", path=key)
        directory, _, _ = key.rpartition("/")
        parent = self.stock_path(directory) if directory else self.stock.resolve()
        if parent is None or not parent.is_dir():
            self.last_error = 3
            return 0xFFFFFFFF
        entry = self.stock_path(key)
        if entry is not None and entry.is_dir():
            self.last_error = 5
            return 0xFFFFFFFF
        for opened in self.files.values():
            if opened["path"] == key and (
                (access & 0x80000000 and not opened["share"] & 1)
                or (access & 0x40000000 and not opened["share"] & 2)
                or (opened["access"] & 0x80000000 and not share & 1)
                or (opened["access"] & 0x40000000 and not share & 2)
            ):
                self.last_error = 32
                return 0xFFFFFFFF
        existing = self.file_bytes(key)
        if (creation == 1 and existing is not None) or (
            creation in (3, 5) and existing is None
        ):
            self.last_error = 80 if creation == 1 else 2
            return 0xFFFFFFFF
        if creation in (1, 2, 5) or existing is None:
            if not access & 0x40000000:
                raise Boundary("unsupported_create_without_write", path=key)
            self.overlay[key] = bytearray()
        if creation in (2, 4):
            self.last_error = 183 if existing is not None else 0
        handle = self.next_file_handle
        self.next_file_handle += 4
        self.files[handle] = {
            "path": key,
            "position": 0,
            "access": access,
            "share": share,
        }
        if self.events:
            self.events[-1]["path"] = key
        return handle

    def checked_file(self, handle, access=0):
        state = self.files.get(handle)
        if state is None:
            raise Boundary("invalid_file_handle", handle=handle)
        if access and not state["access"] & access:
            raise Boundary("file_access_denied", handle=handle)
        return state

    def file_write(self, args):
        handle, source, count, written, overlapped = args
        if overlapped or not written or count > 16 * 1024 * 1024:
            raise Boundary("unsupported_argument", api="WriteFile", args=args)
        self.write_u32(written, 0)
        state = self.checked_file(handle, 0x40000000)
        data = self.read(source, count) if count else b""
        key = state["path"]
        position = state["position"]
        end = position + count
        if count:
            if end > 64 * 1024 * 1024:
                raise Boundary("file_size_limit", path=key)
            target = self.overlay.get(key)
            original = (self.file_bytes(key) or b"") if target is None else target
            # The first write materializes the entire stock file, including its
            # untouched tail. Existing overlay bytes are already in the sum.
            counted = len(target) if target is not None else 0
            projected = sum(len(v) for v in self.overlay.values())
            projected += max(len(original), end) - counted
            if projected > 64 * 1024 * 1024:
                raise Boundary("overlay_size_limit")
            if target is None:
                target = bytearray(original)
                self.overlay[key] = target
            if end > len(target):
                target.extend(bytes(end - len(target)))
            target[position:end] = data
        state["position"] = end
        self.write_u32(written, count)
        if self.events:
            self.events[-1].update(
                path=key,
                offset=position,
                bytes=count,
                sha256=hashlib.sha256(data).hexdigest(),
            )
        return 1

    def file_read(self, args):
        handle, dest, count, read, overlapped = args
        if overlapped or not read or count > 16 * 1024 * 1024:
            raise Boundary("unsupported_argument", api="ReadFile", args=args)
        self.write_u32(read, 0)
        state = self.checked_file(handle, 0x80000000)
        key = state["path"]
        position = state["position"]
        data = self.file_bytes(key)[position : position + count]
        self.write(dest, data)
        self.write_u32(read, len(data))
        state["position"] += len(data)
        if self.events:
            self.events[-1].update(
                path=key,
                offset=position,
                bytes=len(data),
                sha256=hashlib.sha256(data).hexdigest(),
            )
        return 1

    def file_close(self, args):
        if any(thread.get("wait") == args[0] for thread in self.threads.values()):
            # Win32 leaves closing a handle during its pending wait undefined.
            # Preserve the object and waiter rather than inventing a result.
            raise Boundary("unsupported_close_pending_wait", handle=args[0])
        if args[0] in self.mutexes:
            state = self.mutexes.pop(args[0])
            if not any(other is state for other in self.mutexes.values()):
                self.named_mutexes = {
                    name: other
                    for name, other in self.named_mutexes.items()
                    if other is not state
                }
            return 1
        if args[0] in self.semaphores:
            del self.semaphores[args[0]]
            return 1
        self.checked_file(args[0])
        del self.files[args[0]]
        return 1

    def file_size(self, args):
        handle, high = args
        state = self.checked_file(handle)
        if high:
            self.write_u32(high, 0)
        return len(self.file_bytes(state["path"]))

    def file_seek(self, args):
        handle, low, high, method = args
        state = self.checked_file(handle)
        if method not in (0, 1, 2):
            raise Boundary("unsupported_argument", api="SetFilePointer", args=args)
        offset = struct.unpack("<i", struct.pack("<I", low))[0]
        if high:
            raw = (self.read_u32(high) << 32) | low
            offset = raw if raw < 1 << 63 else raw - (1 << 64)
        base = (0, state["position"], len(self.file_bytes(state["path"])))[method]
        position = base + offset
        if position < 0:
            self.last_error = 131
            return 0xFFFFFFFF
        if position > 64 * 1024 * 1024:
            raise Boundary("file_position_limit", position=position)
        state["position"] = position
        if high:
            self.write_u32(high, 0)
        return position

    def find_first(self, args):
        source, dest = args
        key = self.virtual_path(source)
        directory, _, pattern = key.rpartition("/")
        extension = re.fullmatch(r"\*\.[a-z0-9]{1,16}", pattern)
        if (
            "*" in directory
            or "?" in key
            or ("*" in pattern and pattern not in ("*", "*.*") and extension is None)
        ):
            raise Boundary("unsupported_find_pattern", pattern=key)
        parent = self.stock_path(directory) if directory else self.stock.resolve()
        if parent is None or not parent.is_dir():
            self.last_error = 3
            return 0xFFFFFFFF
        entries = {}
        for child in parent.iterdir():
            if len(entries) >= 4096:
                raise Boundary("directory_entry_limit", limit=4096, path=key)
            if not child.resolve().is_relative_to(self.stock.resolve()):
                raise Boundary("stock_symlink_escape", path=key)
            name = child.name.lower()
            if name in entries:
                raise Boundary("ambiguous_stock_path", path=key)
            entries[name] = (child.name, child.is_dir(), child.stat().st_size)
        for overlay, data in self.overlay.items():
            folder, _, name = overlay.rpartition("/")
            if folder == directory:
                entries[name] = (name, False, len(data))
        if len(entries) > 4096:
            raise Boundary("directory_entry_limit", limit=4096, path=key)
        if pattern in ("*", "*.*") and directory:
            entries.update({".": (".", True, 0), "..": ("..", True, 0)})
        values = [
            entry
            for name, entry in sorted(entries.items())
            if pattern in ("*", "*.*")
            or name == pattern
            or (extension is not None and name.endswith(pattern[1:]))
        ]
        if not values:
            self.last_error = 2
            return 0xFFFFFFFF
        if len(self.finds) >= 128:
            raise Boundary("find_handle_limit", limit=128)
        handle = self.next_find_handle
        self.next_find_handle += 4
        self.finds[handle] = values
        self.write_find(dest, self.finds[handle].pop(0))
        if self.events:
            self.events[-1]["path"] = key
        return handle

    def write_find(self, dest, entry):
        name, is_dir, size = entry
        encoded = name.encode("cp1252")
        if len(encoded) >= 260:
            raise Boundary("filename_limit", name=name)
        timestamp = (
            int((datetime(2003, 1, 1) - datetime(1601, 1, 1)).total_seconds())
            * 10000000
        )
        data = bytearray(320)
        struct.pack_into(
            "<I3Q4I",
            data,
            0,
            0x10 if is_dir else 0x20,
            timestamp,
            timestamp,
            timestamp,
            0,
            0 if is_dir else size,
            0,
            0,
        )
        data[44 : 44 + len(encoded)] = encoded
        self.write(dest, data)

    def find_next(self, args):
        handle, dest = args
        if handle not in self.finds:
            raise Boundary("invalid_find_handle", handle=handle)
        if not self.finds[handle]:
            self.last_error = 18
            return 0
        self.write_find(dest, self.finds[handle].pop(0))
        return 1

    def find_close(self, args):
        if args[0] not in self.finds:
            raise Boundary("invalid_find_handle", handle=args[0])
        del self.finds[args[0]]
        return 1

    def semaphore_create(self, args):
        security, initial, maximum, name = args
        if (
            security
            or name
            or maximum > 0x7FFFFFFF
            or not 0 <= initial <= maximum
            or maximum == 0
        ):
            raise Boundary("unsupported_argument", api="CreateSemaphoreA", args=args)
        handle = self.next_semaphore
        self.next_semaphore += 4
        self.semaphores[handle] = {"count": initial, "maximum": maximum}
        return handle

    def mutex_create(self, args):
        security, owned, name = args
        if security or owned not in (0, 1):
            raise Boundary("unsupported_argument", api="CreateMutexA", args=args)
        key = self.read_cstr(name, 260).decode("cp1252") if name else None
        if key is not None and key in self.named_mutexes:
            state = self.named_mutexes[key]
            self.last_error = 183
        else:
            state = {
                "owner": self.thread_id if owned else None,
                "depth": int(bool(owned)),
            }
            if key is not None:
                self.named_mutexes[key] = state
            self.last_error = 0
        handle = self.next_mutex
        self.next_mutex += 4
        self.mutexes[handle] = state
        return handle

    def mutex_release(self, args):
        handle = args[0]
        if handle not in self.mutexes:
            raise Boundary("invalid_mutex_handle", handle=handle)
        state = self.mutexes[handle]
        if state["owner"] != self.thread_id:
            self.last_error = 288
            return 0
        state["depth"] -= 1
        if not state["depth"]:
            state["owner"] = None
            for thread, waiting in self.threads.items():
                if (
                    waiting.get("wait") in self.mutexes
                    and self.mutexes[waiting["wait"]] is state
                ):
                    waiting["status"] = "runnable"
                    del waiting["wait"]
                    state.update(owner=thread, depth=1)
                    break
        return 1

    def thread_create(self, args):
        security, stack_size, start, parameter, flags, id_out = args
        if security or stack_size or flags or len(self.threads) >= 16:
            raise Boundary("unsupported_argument", api="CreateThread", args=args)
        if not self.text_range[0] <= start < self.text_range[1]:
            raise Boundary("unsupported_thread_entry", address=start)
        thread = max(self.threads) + 1
        low = 0x40000000 + thread * 0x200000
        top = low + 0x100000
        self.uc.mem_map(low, 0x100000, UC_PROT_ALL)
        saved = self.uc.context_save()
        for register in ("eax", "ebx", "ecx", "edx", "esi", "edi", "ebp"):
            self.set_reg(register, 0)
        self.set_reg("esp", top - 16)
        self.set_reg("eip", start)
        self.write_u32s(top - 16, [THREAD_RETURN, parameter])
        self._fpu_init()
        context = self.uc.context_save()
        self.uc.context_restore(saved)
        teb = bytearray(0x1000)
        struct.pack_into("<4I", teb, 0, 0xFFFFFFFF, top, low, 0)
        struct.pack_into("<I", teb, 0x18, TEB_BASE)
        struct.pack_into("<2I", teb, 0x20, 1, thread)
        self.threads[thread] = {
            "status": "runnable",
            "context": context,
            "teb": bytes(teb),
            "tls": {},
            "last_error": 0,
        }
        if id_out:
            self.write_u32(id_out, thread)
        return 0x50000000 + thread * 4

    def schedule(self):
        current = self.threads[self.thread_id]
        current.update(
            context=self.uc.context_save(),
            teb=self.read(TEB_BASE, 0x1000),
            tls=self.tls,
            last_error=self.last_error,
            com_depth=self.com_depth,
        )
        ready = sorted(t for t, s in self.threads.items() if s["status"] == "runnable")
        if not ready:
            self._err = Boundary(
                "scheduler_no_runnable_threads",
                threads={str(t): s["status"] for t, s in self.threads.items()},
            )
            self.uc.emu_stop()
            return True
        chosen = next((t for t in ready if t > self.thread_id), ready[0])
        self.next_quantum = self.icount + 1000
        if chosen == self.thread_id:
            return False
        previous = self.thread_id
        state = self.threads[chosen]
        self.uc.context_restore(state["context"])
        self.write(TEB_BASE, state["teb"])
        # context_restore alone does not invalidate Unicorn's current TB.
        self.set_reg("eip", self.reg("eip"))
        self.tls = state["tls"]
        self.last_error = state["last_error"]
        self.com_depth = state.get("com_depth", 0)
        self.thread_id = chosen
        self.events.append(
            {"schedule": {"from": previous, "to": chosen}, "instruction": self.icount}
        )
        return True

    def semaphore_release(self, args):
        handle, count, previous = args
        if handle not in self.semaphores:
            raise Boundary("invalid_semaphore", handle=handle)
        state = self.semaphores[handle]
        if count == 0 or count > 0x7FFFFFFF:
            raise Boundary("unsupported_argument", api="ReleaseSemaphore", args=args)
        if state["count"] + count > state["maximum"]:
            self.last_error = 298
            return 0
        if previous:
            self.write_u32(previous, state["count"])
        state["count"] += count
        for waiting in self.threads.values():
            if waiting.get("wait") == handle and state["count"]:
                waiting["status"] = "runnable"
                del waiting["wait"]
                state["count"] -= 1
        return 1

    def wait_single(self, args):
        handle, timeout = args
        if handle in self.mutexes:
            state = self.mutexes[handle]
            if state["owner"] in (None, self.thread_id):
                state["owner"] = self.thread_id
                state["depth"] += 1
                return 0
        elif handle in self.semaphores:
            state = self.semaphores[handle]
            if state["count"]:
                state["count"] -= 1
                return 0
        else:
            raise Boundary("unsupported_wait_handle", handle=handle)
        if timeout == 0xFFFFFFFF:
            self.threads[self.thread_id].update(status="blocked", wait=handle)
            return 0
        if timeout:
            raise Boundary("unsupported_finite_wait", timeout=timeout, handle=handle)
        return 258

    def get_version_ex(self, args):
        pointer = args[0]
        size = self.read_u32(pointer)
        if size not in (148, 156):
            raise Boundary("unsupported_argument", api="GetVersionExA", size=size)
        self.write(pointer, struct.pack("<5I", size, 5, 1, 2600, 2) + bytes(size - 20))
        if size == 156:
            self.write(pointer + 154, b"\x01")
        return 1

    def heap_create(self, args):
        flags, initial, maximum = args
        if flags & ~0x40005 or maximum or initial > 64 * 1024 * 1024:
            raise Boundary("unsupported_argument", api="HeapCreate", args=args)
        handle = 0x10000000 + len(self.heaps) * 4
        self.heaps[handle] = {}
        return handle

    def heap_alloc(self, args):
        heap, flags, size = args
        if heap not in self.heaps:
            raise Boundary("invalid_handle", api="HeapAlloc", handle=heap)
        if flags & ~0x0D:
            raise Boundary("unsupported_argument", api="HeapAlloc", args=args)
        pointer = self.alloc(max(size, 1))
        self.heaps[heap][pointer] = size
        if flags & 8:
            self.write(pointer, bytes(size))
        return pointer

    def heap_free(self, args):
        heap, flags, pointer = args
        if flags & ~1:
            raise Boundary("unsupported_argument", api="HeapFree", args=args)
        if heap not in self.heaps or pointer not in self.heaps[heap]:
            raise Boundary("invalid_handle", api="HeapFree", args=args)
        del self.heaps[heap][pointer]
        return 1

    def heap_size(self, args):
        heap, flags, pointer = args
        if flags & ~1:
            raise Boundary("unsupported_argument", api="HeapSize", args=args)
        if heap not in self.heaps or pointer not in self.heaps[heap]:
            raise Boundary("invalid_handle", api="HeapSize", args=args)
        return self.heaps[heap][pointer]

    def heap_realloc(self, args):
        heap, flags, pointer, size = args
        old = self.heap_size([heap, 0, pointer])
        if flags & 0x10:
            raise Boundary(
                "unsupported_argument", api="HeapReAlloc in-place-only", args=args
            )
        new = self.heap_alloc([heap, flags, size])
        self.write(new, self.read(pointer, min(old, size)))
        self.heap_free([heap, 0, pointer])
        return new

    def set_last_error(self, args):
        self.last_error = args[0]

    def tls_alloc(self, args):
        slot = len(self.tls_slots)
        if slot >= 1088:
            self.last_error = 8
            return 0xFFFFFFFF
        self.tls_slots.add(slot)
        self.tls[slot] = 0
        return slot

    def tls_get(self, args):
        if args[0] not in self.tls_slots:
            raise Boundary("invalid_tls_slot", slot=args[0])
        self.last_error = 0
        return self.tls.get(args[0], 0)

    def tls_set(self, args):
        if args[0] not in self.tls_slots:
            raise Boundary("invalid_tls_slot", slot=args[0])
        self.tls[args[0]] = args[1]
        return 1

    def cs_init(self, args):
        pointer = args[0]
        if pointer in self.locks:
            raise Boundary("critical_section_reinitialized", pointer=pointer)
        self.write(pointer, struct.pack("<6I", 0, 0xFFFFFFFF, 0, 0, 0, 0))
        self.locks[pointer] = {"owner": None, "depth": 0}

    def write_lock(self, pointer):
        state = self.locks[pointer]
        waits = sum(t.get("cs_wait") == pointer for t in self.threads.values())
        self.write(
            pointer + 4,
            struct.pack(
                "<3I",
                (state["depth"] - 1 + waits) & 0xFFFFFFFF,
                state["depth"],
                state["owner"] or 0,
            ),
        )

    def cs_enter(self, args):
        pointer = args[0]
        if pointer not in self.locks:
            raise Boundary("invalid_critical_section", pointer=pointer)
        state = self.locks[pointer]
        if state["owner"] not in (None, self.thread_id):
            self.threads[self.thread_id].update(status="blocked", cs_wait=pointer)
        else:
            state["owner"] = self.thread_id
            state["depth"] += 1
        self.write_lock(pointer)

    def cs_leave(self, args):
        pointer = args[0]
        state = self.locks.get(pointer)
        if state is None or state["owner"] != self.thread_id:
            raise Boundary("unowned_critical_section", pointer=pointer)
        state["depth"] -= 1
        if not state["depth"]:
            state["owner"] = None
            for thread, waiting in self.threads.items():
                if waiting.get("cs_wait") == pointer:
                    waiting["status"] = "runnable"
                    del waiting["cs_wait"]
                    state.update(owner=thread, depth=1)
                    break
        self.write_lock(pointer)

    def cs_delete(self, args):
        pointer = args[0]
        if pointer not in self.locks:
            raise Boundary("invalid_critical_section", pointer=pointer)
        if self.locks[pointer]["owner"] is not None:
            raise Boundary("owned_critical_section", pointer=pointer)
        del self.locks[pointer]

    def startup_info(self, args):
        self.write(args[0], bytes(68))
        self.write_u32(args[0], 68)
        self.write_u32s(args[0] + 56, [0x100, 0x104, 0x108])

    def std_handle(self, args):
        handles = {0xFFFFFFF6: 0x100, 0xFFFFFFF5: 0x104, 0xFFFFFFF4: 0x108}
        if args[0] not in handles:
            raise Boundary("unsupported_argument", api="GetStdHandle", args=args)
        return handles[args[0]]

    def file_type(self, args):
        if args[0] in (0x100, 0x104, 0x108):
            return 2
        raise Boundary("invalid_handle", api="GetFileType", handle=args[0])

    def command_line(self, args):
        if not hasattr(self, "command_buffer"):
            text = b'"C:\\game\\StreetLegal_Redline.exe"\0'
            self.command_buffer = self.alloc(len(text))
            self.write(self.command_buffer, text)
        return self.command_buffer

    def module_handle(self, args):
        if args[0] == 0:
            return self.base
        name = self.read_cstr(args[0], 256).decode("latin1").lower()
        if not name.endswith(".dll") and not name.endswith(".exe"):
            name += ".dll"
        for handle, module in self.modules.items():
            if module.lower() == name:
                return handle
        raise Boundary("unsupported_module", name=name)

    def proc_address(self, args):
        handle, pointer = args
        module = self.modules.get(handle)
        if module is None:
            raise Boundary("invalid_module_handle", handle=handle)
        name = (
            self.read_cstr(pointer, 256).decode("ascii")
            if pointer > 0xFFFF
            else f"ord{pointer}"
        )
        full = module + "!" + name
        if name not in self.apis or module != self.api_modules[name]:
            raise Boundary("unsupported_export", name=full)
        for address, existing in self.import_names.items():
            if existing == full:
                return address
        if full not in self.dynamic_exports:
            address = 0x7F000000 + self._thunk_slots * 16
            if address >= 0x7F010000:
                raise Boundary("thunk_limit")
            self._thunk_slots += 1
            self.import_names[address] = full
            self.dynamic_exports[full] = address
        return self.dynamic_exports[full]

    def environment_strings(self, args):
        block = "TEMP=C:\\scratch\0TMP=C:\\scratch\0\0".encode("utf-16le")
        pointer = self.alloc(len(block))
        self.write(pointer, block)
        self.environments[pointer] = block
        return pointer

    def environment_free(self, args):
        pointer = args[0]
        if pointer not in self.environments:
            raise Boundary("invalid_environment_block", pointer=pointer)
        del self.environments[pointer]
        return 1

    def read_wide(self, pointer, count):
        if count == 0xFFFFFFFF:
            words = []
            for i in range(32768):
                word = self.read(pointer + 2 * i, 2)
                words.append(word)
                if word == b"\0\0":
                    return b"".join(words).decode("utf-16le")
            raise Boundary("string_limit", encoding="UTF16")
        if count > 32768:
            raise Boundary("string_limit", encoding="UTF16")
        return self.read(pointer, count * 2).decode("utf-16le")

    def wide_to_multi(self, args):
        cp, flags, source, count, dest, capacity, default, used = args
        if cp not in (0, 1252, 65001) or flags not in (0, 0x220) or default:
            raise Boundary("unsupported_argument", api="WideCharToMultiByte", args=args)
        text = self.read_wide(source, count)
        if flags == 0x220 and any(unicodedata.combining(c) for c in text):
            raise Boundary("unsupported_combining_text", api="WideCharToMultiByte")
        fallback = False
        chunks = []
        for c in text:
            code = ord(c)
            if cp == 65001:
                piece = c.encode("utf-8")
            elif code in (0x81, 0x8D, 0x8F, 0x90, 0x9D):
                piece = bytes([code])
            elif code == 0x0191:
                piece = b"\x83"
            else:
                try:
                    piece = c.encode("cp1252")
                except UnicodeEncodeError:
                    if code != 0x039C:
                        raise Boundary(
                            "unsupported_cp1252_character", code=code
                        ) from None
                    piece = b"?"
                    fallback = True
            chunks.append(piece)
        data = b"".join(chunks)
        if used:
            self.write_u32(used, int(fallback))
        if capacity == 0:
            return len(data)
        if capacity < len(data):
            self.last_error = 122
            return 0
        self.write(dest, data)
        return len(data)

    def cp_info(self, args):
        cp, pointer = args
        if cp not in (0, 1252):
            raise Boundary("unsupported_argument", api="GetCPInfo", codepage=cp)
        self.write(pointer, struct.pack("<I", 1) + b"?\0" + bytes(14))
        return 1

    def string_type(self, args):
        info, source, count, dest = args
        if info != 1:
            raise Boundary("unsupported_argument", api="GetStringTypeW", info=info)
        text = self.read_wide(source, count)
        result = []
        for ch in text:
            code = ord(ch)
            if code > 127 and ch not in "".join(
                chr(b)
                if b in (0x81, 0x8D, 0x8F, 0x90, 0x9D)
                else bytes([b]).decode("cp1252")
                for b in range(128, 256)
            ):
                raise Boundary("unsupported_character", api="GetStringTypeW", code=code)
            category = unicodedata.category(ch)
            flags = 0
            if ch.isupper():
                flags |= 1
            if ch.islower():
                flags |= 2
            if ch.isdecimal():
                flags |= 4
            if ch in " \t\n\r\v\f" or category == "Zs":
                flags |= 8
            if category[0] in "PS":
                flags |= 0x10
            if category == "Cc":
                flags |= 0x20
            if ch in " \t" or category == "Zs":
                flags |= 0x40
            if ch in "0123456789abcdefABCDEF":
                flags |= 0x80
            if ch.isalpha():
                flags |= 0x100
            if flags == 0:
                flags = 0x200
            result.append(flags)
        self.write(dest, struct.pack("<" + "H" * len(result), *result))
        return 1

    def multi_to_wide(self, args):
        cp, flags, source, count, dest, capacity = args
        if cp not in (0, 1252, 65001) or flags not in (0, 1, 8):
            raise Boundary("unsupported_argument", api="MultiByteToWideChar", args=args)
        if count != 0xFFFFFFFF and count > 32768:
            raise Boundary("string_limit", encoding="multibyte")
        data = (
            self.read_cstr(source, 32768) + b"\0"
            if count == 0xFFFFFFFF
            else self.read(source, count)
        )
        text = (
            data.decode("utf-8", errors="strict")
            if cp == 65001
            else "".join(
                chr(b)
                if b in (0x81, 0x8D, 0x8F, 0x90, 0x9D)
                else bytes([b]).decode("cp1252")
                for b in data
            )
        )
        output = text.encode("utf-16le")
        required = len(output) // 2
        if capacity == 0:
            return required
        if capacity < required:
            self.last_error = 122
            return 0
        self.write(dest, output)
        return required

    def map_string(self, args):
        locale, flags, source, count, dest, capacity = args
        if locale not in (0, 0x409, 0x400, 0x800) or flags not in (0x100, 0x200):
            raise Boundary("unsupported_argument", api="LCMapStringW", args=args)
        text = self.read_wide(source, count)
        converted = [c.lower() if flags == 0x100 else c.upper() for c in text]
        mapped = "".join(v if len(v) == 1 else c for c, v in zip(text, converted))
        output = mapped.encode("utf-16le")
        required = len(output) // 2
        if capacity == 0:
            return required
        if capacity < required:
            self.last_error = 122
            return 0
        self.write(dest, output)
        return required

    def module_filename(self, args):
        module, dest, capacity = args
        if module not in (0, self.base):
            raise Boundary("unsupported_module_handle", handle=module)
        path = b"C:\\game\\StreetLegal_Redline.exe"
        if capacity == 0:
            self.last_error = 122
            return 0
        self.write(dest, (path + b"\0")[:capacity])
        return min(len(path), capacity)

    def processor_feature(self, args):
        # Conservative single-thread x87 process profile; OS SSE paths disabled.
        features = {0: 0, 1: 0, 2: 1, 3: 1, 6: 0, 7: 0, 8: 1, 9: 0, 10: 0}
        if args[0] not in features:
            raise Boundary("unsupported_processor_feature", feature=args[0])
        return features[args[0]]

    def run(self, limit=1000000):
        if limit <= 0:
            raise ValueError("instruction limit must be positive")
        try:
            result = self.call(ENTRY, limit=limit)
            stop = {"kind": "returned", "eax": result.eax}
        except Boundary as error:
            stop = error.facts
        except OracleError as error:
            stop = {"kind": "execution_error", "message": str(error)}
        result = {
            "oracle": "unicorn",
            "entry": ENTRY,
            "status": "incomplete",
            "exe_sha256": self.exe_sha256,
            "profile": "nt51-c-only-single-cpu-v1",
            "winmain_reached": self.winmain,
            "main_menu_reached": False,
            "instructions": self.icount,
            "stop": stop,
            "events": self.events,
            "trail": list(self.trail),
        }
        if self.media_profile is not None:
            result["platform_base_profile"] = result["profile"]
            result["profile"] = "nt51-hypothetical-mci-v1"
            result["media_scenario"] = self.media_profile.metadata
        if self.graphics_profile is not None:
            result["platform_base_profile"] = "nt51-c-only-single-cpu-v1"
            result["profile"] = "nt51-hypothetical-d3d9-v1"
            result["graphics_scenario"] = {
                **self.graphics_profile.metadata,
                "observed_creation_sdk_versions": self.graphics.creation_versions.copy(),
            }
        if self.desktop_profile is not None:
            result["platform_base_profile"] = "nt51-c-only-single-cpu-v1"
            result["profile"] = "nt51-hypothetical-window-prefix-v1"
            result["desktop_scenario"] = self.desktop_profile.metadata
            result["pending_window_callbacks"] = len(self.pending_window_callbacks)
        if (
            self.media_profile is not None
            or self.desktop_profile is not None
            or self.graphics_profile is not None
            or self.record_limit != 10000
        ):
            result["event_limit"] = self.record_limit
        return result


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--exe", type=Path, default=REPO / "reference/raw/StreetLegal_Redline.bak.exe"
    )
    ap.add_argument("--out", type=Path, default=OUTPUT_ROOT / "startup.json")
    ap.add_argument("--limit", type=int, default=1000000)
    ap.add_argument(
        "--event-limit",
        type=int,
        default=10000,
        help="maximum API/schedule events (1..100000)",
    )
    ap.add_argument(
        "--media-profile",
        type=Path,
        help="explicit bounded hypothetical MCI scenario JSON",
    )
    ap.add_argument(
        "--graphics-profile",
        type=Path,
        help="explicit bounded hypothetical IDirect3D9 enumeration scenario JSON",
    )
    ap.add_argument(
        "--desktop-profile",
        type=Path,
        help="explicit bounded hypothetical cursor/resource/window scenario JSON",
    )
    args = ap.parse_args()
    output = scratch_output(args.out)
    try:
        profile = (
            MediaProfile.load(args.media_profile)
            if args.media_profile is not None
            else None
        )
        graphics = (
            GraphicsProfile.load(args.graphics_profile)
            if args.graphics_profile is not None
            else None
        )
        desktop = (
            DesktopProfile.load(args.desktop_profile)
            if args.desktop_profile is not None
            else None
        )
        result = Startup(
            args.exe,
            media_profile=profile,
            graphics_profile=graphics,
            desktop_profile=desktop,
            record_limit=args.event_limit,
        ).run(args.limit)
    except (ValueError, OSError) as error:
        ap.error(str(error))
    blob = json.dumps(result, sort_keys=True, indent=2) + "\n"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(blob)
    summary = {
        "instructions": result["instructions"],
        "winmain_reached": result["winmain_reached"],
        "stop": result["stop"],
        "sha256": hashlib.sha256(blob.encode()).hexdigest(),
    }
    if "media_scenario" in result:
        summary.update(
            profile=result["profile"],
            media_scenario=result["media_scenario"],
            event_limit=result["event_limit"],
        )
    if "graphics_scenario" in result:
        summary.update(
            profile=result["profile"],
            graphics_scenario=result["graphics_scenario"],
            event_limit=result["event_limit"],
        )
    if "desktop_scenario" in result:
        summary.update(
            profile=result["profile"],
            desktop_scenario=result["desktop_scenario"],
            pending_window_callbacks=result["pending_window_callbacks"],
        )
    print(json.dumps(summary))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
