"""Semantic DirectSound call stream (`slr-dsound-stream/1`) for the headless original.

The SLRLOG DirectSound records the hook layer defines carry raw argument words:
struct pointers, arena addresses, locked-memory pointers. Those cannot be equal
between the original and a native port, and they omit the contents of
`DSBUFFERDESC` and `WAVEFORMATEX`. This recorder wraps the COM thunk dispatch of
`ComHost` and writes one canonical event per `IDirectSound*` call:

    {"i", "call", "this", "args", "ret", "out", "meta"}

`this`, `args`, `ret` and `out` are the compared fields (docs/audio.md). `meta`
(instruction count, clock, exe call site, script-native count, locked-region
digest) is evidence for humans and for locating ported code; it is never
compared. Canonicalisation is an identity relabelling, not a tolerance:

- COM objects are named by creation order: `ds`, `buf#k` (every primary,
  secondary and duplicate buffer, k from 0), `listener`, `3d#k` (the 3D
  interface of `buf#k`).
- Window handles are named `window#j` by first appearance; `null` is `null`.
- Pointers are never recorded, only nullness; `Lock`/`Unlock` additionally
  record whether `Unlock` returned the pointers `Lock` handed out.
- Floats are exact bit patterns (`f32:0x...`); other words are unsigned
  decimal integers; GUIDs are named when known, hex otherwise.

A method or struct this module does not decode stops the run (`Boundary`); it
is never summarised or dropped. Nothing here changes what the model returns.
"""

from __future__ import annotations

import hashlib
import json
import struct

IID_NAMES = {
    bytes.fromhex("84fa9a278149ce11a5210020af0be560"): "IID_IDirectSound3DListener",
    bytes.fromhex("86fa9a278149ce11a5210020af0be560"): "IID_IDirectSound3DBuffer",
    bytes.fromhex("0000000000000000c000000000000046"): "IID_IUnknown",
}
SCHEMA = "slr-dsound-stream/1"


def f32(word):
    return "f32:0x%08x" % word


def signed(word):
    return word - (1 << 32) if word >= 1 << 31 else word


# method -> (argument names, kinds); kinds: u, s (signed), f (float bits),
# in:<n> (read struct), win, buf (COM object), outw (out DWORD/LONG slot),
# outobj (out COM pointer slot). Everything not listed raises.
_U, _S, _F = "u", "s", "f"
LISTENER_SET = {
    "SetDistanceFactor": ((_F, _U), ("factor", "apply")),
    "SetDopplerFactor": ((_F, _U), ("factor", "apply")),
    "SetRolloffFactor": ((_F, _U), ("factor", "apply")),
    "SetOrientation": ((_F, _F, _F, _F, _F, _F, _U), ("front_x", "front_y", "front_z", "top_x", "top_y", "top_z", "apply")),
    "SetPosition": ((_F, _F, _F, _U), ("x", "y", "z", "apply")),
    "SetVelocity": ((_F, _F, _F, _U), ("x", "y", "z", "apply")),
    "CommitDeferredSettings": ((), ()),
}
BUFFER3D_SET = {
    "SetConeAngles": ((_U, _U, _U), ("inside", "outside", "apply")),
    "SetConeOrientation": ((_F, _F, _F, _U), ("x", "y", "z", "apply")),
    "SetConeOutsideVolume": ((_S, _U), ("volume", "apply")),
    "SetMaxDistance": ((_F, _U), ("distance", "apply")),
    "SetMinDistance": ((_F, _U), ("distance", "apply")),
    "SetMode": ((_U, _U), ("mode", "apply")),
    "SetPosition": ((_F, _F, _F, _U), ("x", "y", "z", "apply")),
    "SetVelocity": ((_F, _F, _F, _U), ("x", "y", "z", "apply")),
}
BUFFER_SET = {
    "SetCurrentPosition": ((_U,), ("position",)),
    "SetVolume": ((_S,), ("volume",)),
    "SetPan": ((_S,), ("pan",)),
    "SetFrequency": ((_U,), ("frequency",)),
    "Play": ((_U, _U, _U), ("reserved", "priority", "flags")),
    "Stop": ((), ()),
    "Restore": ((), ()),
}
BUFFER_GET = {  # one out DWORD/LONG slot each
    "GetVolume": ("volume", True),
    "GetPan": ("pan", True),
    "GetFrequency": ("frequency", False),
    "GetStatus": ("status", False),
}


class DirectSoundTrace:
    def __init__(self, host, path, boundary, provenance=None):
        """`path=None` keeps events only in `captured` (used by the vector rigs)."""
        self.host = host
        self.boundary = boundary
        self.events = 0
        self.captured = []
        self.names = {}
        self.windows = {}
        self.buffer_count = 0
        self.locks = {}
        self.stream = open(path, "w") if path is not None else None
        header = {"schema": SCHEMA, **(provenance or {})}
        if self.stream is not None:
            self.stream.write(json.dumps(header, sort_keys=True) + "\n")
        self.original_invoke = host.com.invoke
        host.com.invoke = self.invoke

    def close(self):
        if self.stream is not None:
            self.stream.close()

    def emit(self, event):
        self.captured.append(event)
        if self.stream is not None:
            self.stream.write(json.dumps(event, sort_keys=True) + "\n")

    # ------------------------------------------------------------ naming
    def name_of(self, pointer):
        if not pointer:
            return "null"
        try:
            return self.names[pointer]
        except KeyError:
            raise self.boundary("dsound_trace_unnamed_object", pointer=pointer) from None

    def window_name(self, handle):
        if not handle:
            return "null"
        return self.windows.setdefault(handle, f"window#{len(self.windows)}")

    def name_new(self, obj):
        if obj.interface == "IDirectSound":
            label = "ds"
        elif obj.interface == "IDirectSoundBuffer":
            label = f"buf#{self.buffer_count}"
            self.buffer_count += 1
        elif obj.interface == "IDirectSound3DListener":
            label = "listener"
        elif obj.interface == "IDirectSound3DBuffer":
            label = "3d#" + self.name_of(obj.model.buffer.obj.pointer).split("#")[1]
        else:
            raise self.boundary("dsound_trace_unknown_interface", interface=obj.interface)
        self.names[obj.pointer] = label
        return label

    # ------------------------------------------------------------ decoding
    def read_struct_fields(self, pointer, layout, size):
        data = self.host.read(pointer, size)
        return dict(zip(layout, struct.unpack("<" + "I" * len(layout), data)))

    def wfx(self, pointer):
        if not pointer:
            return None
        tag, channels, rate, avg, align, bits = struct.unpack("<HHIIHH", self.host.read(pointer, 16))
        return {"tag": tag, "channels": channels, "rate": rate, "avg_bytes_per_sec": avg, "block_align": align, "bits": bits}

    def desc(self, pointer):
        if not pointer:
            return None
        size = self.host.read_u32(pointer)
        if size not in (20, 36):
            raise self.boundary("dsound_trace_desc_size", size=size)
        _, flags, nbytes, reserved, fmt = self.host.read_u32s(pointer, 5)
        out = {"size": size, "flags": flags, "bytes": nbytes, "reserved": reserved, "format": self.wfx(fmt)}
        if size == 36:
            out["guid_3d_algorithm"] = bytes(self.host.read(pointer + 20, 16)).hex()
        return out

    def words(self, pointer, count):
        return self.host.read_u32s(pointer, count) if pointer else None

    def pre(self, interface, method, a):
        """Arguments before the call: (compared args, scratch for post)."""
        h = self.host
        if interface == "IDirectSound":
            if method == "CreateSoundBuffer":
                return {"desc": self.desc(a[0]), "outer": a[2] != 0, "out_given": a[1] != 0}, {"out": a[1]}
            if method == "GetCaps":
                return {"size": h.read_u32(a[0]) if a[0] else None}, {"out": a[0]}
            if method == "DuplicateSoundBuffer":
                return {"source": self.name_of(a[0]), "out_given": a[1] != 0}, {"out": a[1]}
            if method == "SetCooperativeLevel":
                return {"window": self.window_name(a[0]), "level": a[1]}, {}
            if method == "Compact":
                return {}, {}
            if method == "GetSpeakerConfig":
                return {}, {"out": a[0]}
            if method == "SetSpeakerConfig":
                return {"config": a[0]}, {}
            if method == "Initialize":
                return {"guid_given": a[0] != 0}, {}
        elif interface == "IDirectSoundBuffer":
            if method == "GetCaps":
                return {"size": h.read_u32(a[0]) if a[0] else None}, {"out": a[0]}
            if method == "GetCurrentPosition":
                return {"play_given": a[0] != 0, "write_given": a[1] != 0}, {"play": a[0], "write": a[1]}
            if method == "GetFormat":
                return {"size_allocated": a[1], "buffer_given": a[0] != 0, "written_given": a[2] != 0}, {"fmt": a[0], "written": a[2]}
            if method in BUFFER_GET:
                return {}, {"out": a[0]}
            if method == "Lock":
                offset, nbytes, p1, b1, p2, b2, flags = a
                return {"offset": offset, "bytes": nbytes, "flags": flags,
                        "ptr1_slot": p1 != 0, "len1_slot": b1 != 0, "ptr2_slot": p2 != 0, "len2_slot": b2 != 0}, \
                    {"p1": p1, "b1": b1, "p2": p2, "b2": b2}
            if method == "Unlock":
                return {"len1": a[1], "len2": a[3], "ptr1_given": a[0] != 0, "ptr2_given": a[2] != 0}, {"p1": a[0], "p2": a[2], "len1": a[1], "len2": a[3]}
            if method == "SetFormat":
                return {"format": self.wfx(a[0])}, {}
            if method == "QueryInterface":
                iid = bytes(h.read(a[0], 16)) if a[0] else None
                return {"iid": IID_NAMES.get(iid, iid.hex() if iid else None), "out_given": a[1] != 0}, {"out": a[1]}
            if method in BUFFER_SET:
                kinds, names = BUFFER_SET[method]
                return self.scalar(kinds, names, a), {}
        elif interface == "IDirectSound3DListener" and method in LISTENER_SET:
            kinds, names = LISTENER_SET[method]
            return self.scalar(kinds, names, a), {}
        elif interface == "IDirectSound3DBuffer" and method in BUFFER3D_SET:
            kinds, names = BUFFER3D_SET[method]
            return self.scalar(kinds, names, a), {}
        raise self.boundary("dsound_trace_undecoded_method", interface=interface, method=method, args=a)

    @staticmethod
    def scalar(kinds, names, a):
        out = {}
        for kind, name, word in zip(kinds, names, a):
            out[name] = f32(word) if kind == _F else signed(word) if kind == _S else word
        return out

    def post(self, interface, method, scratch, result):
        h = self.host
        out = {}
        if result != 0:
            return out  # a failed call claims no out slots
        if interface == "IDirectSound":
            if method in ("CreateSoundBuffer", "DuplicateSoundBuffer"):
                pointer = h.read_u32(scratch["out"])
                out["object"] = self.name_new(h.com.objects[pointer])
            elif method == "GetCaps":
                out["caps"] = list(h.read_u32s(scratch["out"], 24))
            elif method == "GetSpeakerConfig":
                out["config"] = h.read_u32(scratch["out"])
        elif interface == "IDirectSoundBuffer":
            if method == "GetCaps":
                out["caps"] = list(h.read_u32s(scratch["out"], 5))
            elif method == "GetCurrentPosition":
                if scratch["play"]:
                    out["play"] = h.read_u32(scratch["play"])
                if scratch["write"]:
                    out["write"] = h.read_u32(scratch["write"])
            elif method == "GetFormat":
                if scratch["fmt"]:
                    out["format"] = self.wfx(scratch["fmt"])
                if scratch["written"]:
                    out["written"] = h.read_u32(scratch["written"])
            elif method in BUFFER_GET:
                name, is_signed = BUFFER_GET[method]
                word = h.read_u32(scratch["out"])
                out[name] = signed(word) if is_signed else word
            elif method == "Lock":
                p1 = h.read_u32(scratch["p1"])
                p2 = h.read_u32(scratch["p2"]) if scratch["p2"] else 0
                out["len1"] = h.read_u32(scratch["b1"])
                out["len2"] = h.read_u32(scratch["b2"]) if scratch["b2"] else 0
                out["ptr1"] = p1 != 0
                out["ptr2"] = p2 != 0
                self.locks[scratch["_this"]] = (p1, out["len1"], p2, out["len2"])
            elif method == "Unlock":
                locked = self.locks.pop(scratch["_this"], None)
                out["matches_lock"] = locked == (scratch["p1"], scratch["len1"], scratch["p2"], scratch["len2"])
            elif method == "QueryInterface":
                pointer = h.read_u32(scratch["out"])
                if pointer:
                    obj = h.com.objects[pointer]
                    out["object"] = self.names.get(pointer) or self.name_new(obj)
        return out

    # ------------------------------------------------------------ hook
    def invoke(self, address):
        interface, _slot, method, params = self.host.com.thunks[address]
        if not interface.startswith("IDirectSound"):
            return self.original_invoke(address)
        host = self.host
        esp = host.reg("esp")
        return_site = host.read_u32(esp)
        args = host.read_u32s(esp + 4, 1 + len(params))
        this = self.name_of(args[0])
        icount, clock = host.icount, host.clock_ms()
        if method in ("AddRef", "Release"):
            event_args, scratch = {}, {}
        else:
            event_args, scratch = self.pre(interface, method, args[1:])
        scratch["_this"] = args[0]
        meta_extra = {}
        if interface == "IDirectSoundBuffer" and method == "Unlock" and args[1]:
            region = bytes(host.read(args[1], args[2])) + (bytes(host.read(args[3], args[4])) if args[3] else b"")
            meta_extra["region_sha256"] = hashlib.sha256(region).hexdigest()
        key, site, call_args, result = self.original_invoke(address)
        out = {} if method in ("AddRef", "Release") else self.post(interface, method, scratch, result)
        observer = getattr(host, "observer", None)
        meta = {
            "icount": icount,
            "clock_ms": clock,
            "site": "0x%x" % return_site,
            "natives": observer.stats["native_calls"] if observer is not None else None,
            **meta_extra,
        }
        event = {
            "i": self.events,
            "call": f"{interface}.{method}",
            "this": this,
            "args": event_args,
            "ret": result,
            "out": out,
            "meta": meta,
        }
        self.events += 1
        self.emit(event)
        return key, site, call_args, result

    def attach_create(self, startup):
        """Record `DirectSoundCreate` (dsound ordinal 1), an API rather than a COM method."""
        nargs, handler = startup.apis["ord1"]

        def create(args):
            guid, out, outer = args
            icount, clock = self.host.icount, self.host.clock_ms()
            result = handler(args)
            observer = getattr(self.host, "observer", None)
            event = {
                "i": self.events,
                "call": "DirectSoundCreate",
                "this": "api",
                "args": {"guid_given": guid != 0, "outer": outer != 0, "out_given": out != 0},
                "ret": result,
                "out": {"object": self.name_new(self.host.com.objects[self.host.read_u32(out)])} if result == 0 else {},
                "meta": {"icount": icount, "clock_ms": clock, "site": None,
                         "natives": observer.stats["native_calls"] if observer is not None else None},
            }
            self.events += 1
            self.emit(event)
            return result

        startup.apis["ord1"] = (nargs, create)
