#!/usr/bin/env python3
"""Original DirectInput device layer under Unicorn with scripted COM devices.

Runs the exe's own input functions (`FUN_00556150` init, `FUN_005565a0` /
`FUN_00556710` enumeration callbacks, `FUN_00556bc0` poll, `FUN_00556e00`
last-key, the 0x556ec0..0x558130 query block, `FUN_00556b10` shutdown) against
fake `IDirectInput8A` / `IDirectInputDevice8A` objects whose every call is
recorded. Devices, device states, HRESULTs, timers and keyboard-layout answers
come from a declared, deterministic scenario (JSON-serialisable); nothing is
observed from hardware. Marshalling, branching and arithmetic are original x86.

The recorded *transcript* is the DirectInput call stream: one canonical string
per COM / input-Win32 call (arguments decoded, result). The port's
`slr-input` device layer must produce the identical strings for the same
scenario; `re/oracle/input_device_vectors.py` writes the vectors and
`crates/slr-input` replays them.

Pointer-valued words in the device table are normalised to the same stable
handles the port uses: the IDirectInput8 object is 0x1000, devices 0x1001...
in creation order.
"""
from __future__ import annotations

import struct
import sys
from collections import defaultdict, deque
from fractions import Fraction
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from oracle import CW_D3D9, Oracle  # noqa: E402
from unicorn import x86_const as X  # noqa: E402

FAKE_BASE = 0x7D000000
FAKE_SIZE = 0x10000

TABLE = 0x76F9B0
RECORD = 0x7A8
MAX_DEVICES = 16
TABLE_END = TABLE + RECORD * MAX_DEVICES  # 0x777430
GLOBALS = [  # (address, words): every global the layer reads or writes
    (0x76F9A0, 4),  # dt float, qpf(8 at 0x76f9a8), pad
    (0x777430, 8),  # layout, count, dt-scale, di, qpc(8), time, ff, error count
    (0x7686E4, 6),  # controller runtime words
]

IID_DI8A = bytes.fromhex("30809fbba4d08e2f" "aa995d64ed369700")[:0]  # unused marker

GUID_NAMES = {}


def u32(value):
    return value & 0xFFFFFFFF


def round_f32(value: Fraction) -> int:
    """Round an exact rational to the nearest-even f32; return its bit pattern."""
    if value == 0:
        return 0
    sign = 0
    if value < 0:
        sign = 0x80000000
        value = -value
    exp = value.numerator.bit_length() - value.denominator.bit_length()
    while Fraction(2) ** exp > value:
        exp -= 1
    while Fraction(2) ** (exp + 1) <= value:
        exp += 1
    if exp < -126:  # subnormal
        quantum = Fraction(2) ** -149
        q = value / quantum
    else:
        quantum = Fraction(2) ** (exp - 23)
        q = value / quantum
    n = q.numerator // q.denominator
    rem = q - n
    if rem > Fraction(1, 2) or (rem == Fraction(1, 2) and n & 1):
        n += 1
    if exp < -126:
        return sign | n
    if n == 1 << 24:
        n = 1 << 23
        exp += 1
    return sign | ((exp + 127) << 23) | (n & 0x7FFFFF)


class CallInto:
    """Hook result: run guest `target(*args)` (stdcall) then `cont(eax)`."""

    def __init__(self, target, args, cont):
        self.target, self.args, self.cont = target, args, cont


class Done:
    def __init__(self, eax):
        self.eax = eax


class RigOracle(Oracle):
    """Oracle whose hooks may call back into guest code (enumeration callbacks)."""

    K = FAKE_BASE + FAKE_SIZE - 0x100

    def __init__(self, *a, **k):
        super().__init__(*a, **k)
        self.uc.mem_map(FAKE_BASE, FAKE_SIZE, 7)
        self._frames = []
        self.uc.mem_write(self.K, b"\xc3")  # ends the translation block at the continuation point
        self.hooks[self.K] = (None, "k", 0)

    def _enter(self, target, args, esp2):
        uc = self.uc
        esp = esp2 - 4 * len(args) - 4
        uc.mem_write(esp, struct.pack("<I", self.K) + b"".join(struct.pack("<I", u32(a)) for a in args))
        uc.reg_write(X.UC_X86_REG_ESP, esp)
        uc.reg_write(X.UC_X86_REG_EIP, target)

    def _finish(self, res, ret, esp2, cont_frame):
        uc = self.uc
        if isinstance(res, CallInto):
            cont_frame[2] = res.cont
            self._enter(res.target, res.args, esp2)
            return
        if cont_frame in self._frames:
            self._frames.remove(cont_frame)
        if isinstance(res, Done):
            res = res.eax
        uc.reg_write(X.UC_X86_REG_ESP, esp2)
        if isinstance(res, float):
            self._fpu_push(res)
        elif res is not None:
            uc.reg_write(X.UC_X86_REG_EAX, u32(res))
        uc.reg_write(X.UC_X86_REG_EIP, ret)

    def _run_hook(self, va):
        uc = self.uc
        if va == self.K:
            frame = self._frames[-1]
            ret, esp2, cont = frame
            self._finish(cont(self.reg("eax")), ret, esp2, frame)
            return
        fn, conv, nargs = self.hooks[va]
        esp = uc.reg_read(X.UC_X86_REG_ESP)
        ret = struct.unpack("<I", bytes(uc.mem_read(esp, 4)))[0]
        words = list(struct.unpack(f"<{nargs}I", bytes(uc.mem_read(esp + 4, 4 * nargs)))) if nargs else []
        res = fn(self, words)
        cleanup = 0 if conv == "cdecl" else 4 * nargs
        frame = [ret, esp + 4 + cleanup, None]
        if isinstance(res, CallInto):
            self._frames.append(frame)
        self._finish(res, ret, esp + 4 + cleanup, frame)


class Queue:
    """Named deterministic answer queues; an exhausted queue yields its default."""

    def __init__(self):
        self.q = defaultdict(deque)

    def feed(self, mapping):
        for key, values in mapping.items():
            self.q[key].extend(values)

    def pop(self, key, default):
        queue = self.q[key]
        return queue.popleft() if queue else default


class Rig:
    VT_DI = {0x08: ("Release", 1), 0x0C: ("CreateDevice", 4), 0x10: ("EnumDevices", 5)}
    VT_DEV = {
        0x08: ("Release", 1), 0x0C: ("GetCapabilities", 2), 0x10: ("EnumObjects", 4),
        0x18: ("SetProperty", 3), 0x1C: ("Acquire", 1), 0x20: ("Unacquire", 1),
        0x24: ("GetDeviceState", 3), 0x28: ("GetDeviceData", 5), 0x2C: ("SetDataFormat", 2),
        0x34: ("SetCooperativeLevel", 3), 0x64: ("Poll", 1),
    }
    CALLBACK_DEVICES = 0x5565A0
    CALLBACK_OBJECTS = 0x556710

    def __init__(self, scenario):
        self.sc = scenario
        env = scenario["env"]
        self.o = o = RigOracle(fpu_cw=CW_D3D9)
        o.hook_alloc()
        self.calls = []
        self.answers = Queue()
        self.objects = {}  # guest pointer -> ("di"|label, handle)
        self.handle_by_ptr = {}
        self.next_handle = 0x1000
        self.devices = []  # created device labels in order
        self.gamepads = list(scenario["devices"].get("gamepads", []))
        self.created = defaultdict(int)
        self._vtables(env)
        o.write_u32(0x6495A0, env["hwnd"])
        o.write_u32(0x777448, env["ff"])  # ForceFeedBack config word
        self._imports(env)
        self._snapshot = self.snapshot()

    # ------------------------------------------------------------ recording
    def log(self, text):
        self.calls.append(text)

    # ------------------------------------------------------------ imports
    def _hook_import(self, name, nargs, fn):
        for va, full in self.o.import_names.items():
            if full.endswith("!" + name):
                self.o.hooks[va] = (fn, "stdcall", nargs)
                return
        raise RuntimeError("import not found: " + name)

    def _imports(self, env):
        o = self.o

        def create(o_, a):
            hinst, version, riid, out, outer = a
            self.log(f"DirectInput8Create(version={version:#x},outer={outer})")
            ptr = self._new_object("di", self.vt_di)
            o.write_u32(out, ptr)
            return 0

        self._hook_import("DirectInput8Create", 5, create)
        self._hook_import("GetModuleHandleA", 1, lambda o_, a: 0x400000)
        self._hook_import("GetKeyboardLayout", 1, lambda o_, a: self._layout(a))

        def qpf(o_, a):
            value = self.sc["env"]["qpf"]
            o.write(a[0], struct.pack("<Q", value))
            self.log(f"win.QueryPerformanceFrequency()={value}")
            return 1

        def qpc(o_, a):
            value = self.answers.pop("qpc", 0)
            o.write(a[0], struct.pack("<Q", value))
            self.log(f"win.QueryPerformanceCounter()={value}")
            return 1

        def tgt(o_, a):
            value = self.answers.pop("time", 0)
            self.log(f"win.timeGetTime()={value}")
            return value

        self._hook_import("QueryPerformanceFrequency", 1, qpf)
        self._hook_import("QueryPerformanceCounter", 1, qpc)
        self._hook_import("timeGetTime", 0, tgt)

        def mapvk(o_, a):
            result = self.answers.pop("mapvk", 0)
            self.log(f"win.MapVirtualKeyExA({a[0]:#x},{a[1]},{a[2]:#x})={result:#x}")
            return result

        def kbstate(o_, a):
            data = bytes.fromhex(self.answers.pop("kbstate", "00" * 256))
            ok = self.answers.pop("kbstate.ok", 1)
            if ok:
                o.write(a[0], data)
            self.log(f"win.GetKeyboardState()={ok} {data.hex() if ok else ''}".rstrip())
            return ok

        def toascii(o_, a):
            vk, sc, state, out, flags, layout = a[:6]
            ret, word = self.answers.pop("toascii", [0, 0])
            word &= 0xFFFF
            o.write(out, struct.pack("<H", word & 0xFFFF))  # a WORD buffer
            self.log(f"win.ToAsciiEx(vk={vk:#x},sc={sc:#x},flags={flags},layout={layout:#x})={ret} word={word:#x}")
            return ret

        self._hook_import("MapVirtualKeyExA", 3, mapvk)
        self._hook_import("GetKeyboardState", 1, kbstate)
        self._hook_import("ToAsciiEx", 6, toascii)

    def _layout(self, a):
        value = self.sc["env"]["layout"]
        self.log(f"win.GetKeyboardLayout({a[0]})={value:#x}")
        return value

    # ------------------------------------------------------------ COM objects
    def _vtables(self, env):
        o = self.o
        self.slot = 0

        def build(table, kind):
            vt = o.alloc(0x200)
            for off in range(0, 0x200, 4):
                addr = FAKE_BASE + self.slot * 16
                self.slot += 1
                o.write_u32(vt + off, addr)
                o.hooks[addr] = (self._unmodelled(kind, off), "stdcall", 0)
            for off, (name, nargs) in table.items():
                addr = o.read_u32(vt + off)
                o.hooks[addr] = (self._method(kind, name), "stdcall", nargs)
            return vt

        self.vt_di = build(self.VT_DI, "di")
        self.vt_dev = build(self.VT_DEV, "dev")

    def _unmodelled(self, kind, off):
        def fn(o, a):
            raise RuntimeError(f"unmodelled {kind} vtable +{off:#x}")
        return fn

    def _new_object(self, label, vtable):
        ptr = self.o.alloc(16)
        self.o.write_u32(ptr, vtable)
        handle = self.next_handle
        self.next_handle += 1
        self.objects[ptr] = label
        self.handle_by_ptr[ptr] = handle
        return ptr

    def _method(self, kind, name):
        handler = getattr(self, f"{kind}_{name}")

        def fn(o, a):
            this = a[0]
            label = self.objects.get(this)
            if label is None:
                raise RuntimeError(f"{kind}.{name} on unknown object {this:#x}")
            return handler(label, this, *a[1:])
        return fn

    def hr(self, label, method, default=0):
        return u32(self.answers.pop(f"{label}.{method}.hr", default))

    # ---- IDirectInput8A
    def di_Release(self, label, this):
        self.log("di.Release()")
        return 0

    def di_CreateDevice(self, label, this, guid, out, outer):
        raw = self.o.read(guid, 16).hex()
        kind = {"612b1d6fa0d5cf11bfc7444553540000": "kbd", "602b1d6fa0d5cf11bfc7444553540000": "mouse"}.get(raw)
        if kind is None:
            for index, pad in enumerate(self.gamepads):
                if pad["instance_guid"] == raw:
                    kind = f"joy{index}"
        hr = self.hr("di", "CreateDevice", 0 if kind else 0x80040154)
        if hr != 0:
            self.log(f"di.CreateDevice({raw},outer={outer})=>{hr:#x}")
            return hr
        ptr = self._new_object(kind, self.vt_dev)
        self.o.write_u32(out, ptr)
        self.devices.append(kind)
        self.log(f"di.CreateDevice({raw},outer={outer})=>0 {kind}")
        return 0

    def di_EnumDevices(self, label, this, devtype, callback, ref, flags):
        self.log(f"di.EnumDevices(devtype={devtype},callback={callback:#x},ref={ref:#x},flags={flags})")
        pads = list(enumerate(self.gamepads))
        o = self.o

        def step(prev=None):
            while pads:
                index, pad = pads.pop(0)
                inst = o.alloc(0x250)
                o.write_u32(inst, 0x244)
                o.write(inst + 4, bytes.fromhex(pad["instance_guid"]))
                o.write(inst + 0x14, bytes.fromhex(pad["product_guid"]))
                o.write_u32(inst + 0x24, pad["devtype"])
                o.write(inst + 0x28, pad["name"].encode() + b"\0")
                self.log(f"di.EnumDevices.callback(instance={pad['instance_guid']},devtype={pad['devtype']:#x},name={pad['name']})")
                return CallInto(callback, [inst, ref], lambda eax: self._after_enum(eax, step))
            return Done(0)

        first = step()
        return 0 if isinstance(first, Done) else first

    def _after_enum(self, eax, step):
        self.log(f"di.EnumDevices.callback=>{eax}")
        if eax == 0:  # DIENUM_STOP; any other value continues (declared platform rule)
            return 0
        res = step()
        return 0 if isinstance(res, Done) else res

    # ---- IDirectInputDevice8A
    def dev_Release(self, label, this):
        self.log(f"{label}.Release()")
        return self.hr(label, "Release")

    def dev_GetCapabilities(self, label, this, ptr):
        size = self.o.read_u32(ptr)
        hr = self.hr(label, "GetCapabilities")
        if label.startswith("joy"):
            caps = self.gamepads[int(label[3:])]["caps"]
        else:
            caps = self.sc["devices"]["caps"][label]
        if hr == 0:
            words = [44, caps["flags"], caps["devtype"], caps["axes"], caps["buttons"], caps["povs"], 0, 0, 0, 0, 0]
            self.o.write_u32s(ptr, words[: size // 4])
        self.log(f"{label}.GetCapabilities(size={size})=>{hr:#x}" + (f" {self.o.read_u32s(ptr, size // 4)}" if hr == 0 else ""))
        return hr

    def dev_EnumObjects(self, label, this, callback, ref, flags):
        pad = self.gamepads[int(label[3:])]
        self.log(f"{label}.EnumObjects(callback={callback:#x},ref={self.handle_of(ref)},flags={flags})")
        objects = [obj for obj in pad["objects"] if obj["type"] & flags]
        pending = list(objects)
        o = self.o

        def step():
            if not pending:
                return Done(0)
            obj = pending.pop(0)
            inst = o.alloc(0x120)
            o.write_u32(inst, 0x108)
            o.write(inst + 4, bytes.fromhex(obj["guid"]))
            o.write_u32(inst + 0x14, obj.get("ofs", 0))
            o.write_u32(inst + 0x18, obj["type"])
            o.write_u32(inst + 0x1C, obj.get("flags", 0))
            o.write(inst + 0x20, obj["name"].encode() + b"\0")
            self.log(f"{label}.EnumObjects.callback(guid={obj['guid']},type={obj['type']:#x},flags={obj.get('flags', 0):#x},name={obj['name']})")
            return CallInto(callback, [inst, ref], lambda eax: self._after_objects(label, eax, step))

        first = step()
        return 0 if isinstance(first, Done) else first

    def _after_objects(self, label, eax, step):
        self.log(f"{label}.EnumObjects.callback=>{eax}")
        if eax == 0:
            return 0
        res = step()
        return 0 if isinstance(res, Done) else res

    def handle_of(self, ptr):
        """Symbolic form of a guest value that is a device-record pointer."""
        if TABLE <= ptr < TABLE_END:
            return f"rec{(ptr - TABLE) // RECORD}"
        return f"{ptr:#x}"

    def dev_SetProperty(self, label, this, guid, header):
        words = self.o.read_u32s(header, 5)
        hr = self.hr(label, "SetProperty")
        self.log(f"{label}.SetProperty(prop={guid:#x},{words})=>{hr:#x}")
        return hr

    def dev_Acquire(self, label, this):
        hr = self.hr(label, "Acquire")
        self.log(f"{label}.Acquire()=>{hr:#x}")
        return hr

    def dev_Unacquire(self, label, this):
        hr = self.hr(label, "Unacquire")
        self.log(f"{label}.Unacquire()=>{hr:#x}")
        return hr

    def dev_Poll(self, label, this):
        hr = self.hr(label, "Poll")
        self.log(f"{label}.Poll()=>{hr:#x}")
        return hr

    def dev_GetDeviceState(self, label, this, size, ptr):
        hr = self.hr(label, "GetDeviceState")
        report = ""
        if hr == 0:
            data = bytes.fromhex(self.answers.pop(f"{label}.state", "00" * size))
            assert len(data) == size, (label, len(data), size)
            self.o.write(ptr, data)
            report = " " + data.hex()
        self.log(f"{label}.GetDeviceState({size})=>{hr:#x}{report}")
        return hr

    def dev_GetDeviceData(self, label, this, cb, data, inout, flags):
        count_in = self.o.read_u32(inout)
        hr = self.hr(label, "GetDeviceData")
        events = []
        if hr == 0:
            events = self.answers.pop(f"{label}.data", [])[:count_in]
            for i, (ofs, value) in enumerate(events):
                self.o.write(data + i * cb, struct.pack("<5I", ofs, value, 0, 0, 0))
            self.o.write_u32(inout, len(events))
        self.log(f"{label}.GetDeviceData(cb={cb},in={count_in},flags={flags})=>{hr:#x} events={events}")
        return hr

    def dev_SetDataFormat(self, label, this, fmt):
        words = self.o.read_u32s(fmt, 5)
        hr = self.hr(label, "SetDataFormat")
        self.log(f"{label}.SetDataFormat(format={fmt:#x},{words})=>{hr:#x}")
        return hr

    def dev_SetCooperativeLevel(self, label, this, hwnd, flags):
        hr = self.hr(label, "SetCooperativeLevel")
        self.log(f"{label}.SetCooperativeLevel(hwnd={hwnd:#x},flags={flags:#x})=>{hr:#x}")
        return hr

    # ------------------------------------------------------------ state
    def snapshot(self):
        o = self.o
        words = {}
        for base, end in ((TABLE, TABLE_END), (0x76F9A0, 0x76F9B0), (0x777430, 0x777458), (0x7686E4, 0x7686FC)):
            for addr in range(base, end, 4):
                words[addr] = o.read_u32(addr)
        return words

    def normalised(self, word):
        return self.handle_by_ptr.get(word, word)

    def diff(self):
        now = self.snapshot()
        out = []
        for addr in sorted(now):
            if now[addr] != self._snapshot.get(addr):
                out.append([addr, self.normalised(now[addr])])
        self._snapshot = now
        return out

    # ------------------------------------------------------------ steps
    def run(self, step):
        o = self.o
        self.calls = []
        self.answers.feed(step.get("feed", {}))
        op = step["op"]
        a = step.get("args", [])
        before_icount = o.icount
        ret = None
        st0 = None
        if op == "init":
            res = o.call(0x556150, [], "cdecl", limit=2_000_000)
            ret = u32(res.eax)
        elif op == "poll":
            res = o.call(0x556BC0, [], "cdecl", limit=2_000_000)
            st0 = res.st0_raw
        elif op == "lastkey":
            ret = u32(o.call(0x556E00, [], "cdecl", limit=2_000_000).eax)
        elif op == "isdown":
            ret = u32(o.call(0x556EC0, [("u32", a[0]), ("u32", a[1]), ("u32", a[2])], "cdecl").eax)
        elif op == "axis":
            res = o.call(0x557430, [("u32", a[0]), ("u32", a[1])], "cdecl")
            st0 = res.st0_raw
        elif op == "isaxis":
            ret = u32(o.call(0x557990, [("u32", a[0]), ("u32", a[1])], "cdecl").eax)
        elif op == "axisname":
            buf = o.alloc(0x200)
            o.write(buf, b"\xcc" * 0x200)
            ret = u32(o.call(0x5579F0, [("u32", a[0]), ("u32", a[1]), ("u32", buf)], "cdecl").eax)
            ret = [ret, o.read(buf, 0x100).hex()]
        elif op == "activeaxis":
            ret = u32(o.call(0x557AA0, [("u32", a[0])], "cdecl", limit=2_000_000).eax)
        elif op == "devname":
            ptr = u32(o.call(0x558100, [("u32", a[0])], "cdecl").eax)
            ret = None if ptr == 0 else o.read_cstr(ptr, 0x40).hex()
        elif op == "devkind":
            ret = u32(o.call(0x558130, [("u32", a[0])], "cdecl").eax)
        elif op == "shutdown":
            ret = u32(o.call(0x556B10, [], "cdecl").eax)
        elif op == "write":  # declared state injection: [addr, word]
            for addr, word in a:
                o.write_u32(addr, word)
        else:
            raise ValueError(op)
        row = {"op": op, "args": a, "ret": ret, "calls": self.calls, "diff": self.diff()}
        if st0 is not None:
            from oracle import ext_bytes_to_fraction  # noqa: E402
            frac = ext_bytes_to_fraction(st0)
            row["st0_f32"] = round_f32(frac) if frac is not None else None
            if frac == 0:
                # Fraction has no signed zero; retain the original x87 sign.
                row["st0_f32"] = (st0[9] & 0x80) << 24
            row["st0_raw"] = st0.hex()
        return row
