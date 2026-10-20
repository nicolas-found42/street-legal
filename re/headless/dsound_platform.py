"""Declared DirectSound device: COM semantics and buffer memory, no audio output.

`DirectSoundCreate` (dsound.dll ordinal 1) and the IDirectSound /
IDirectSoundBuffer / IDirectSound3DListener / IDirectSound3DBuffer methods the
original uses follow the documented contracts (primary/secondary rules, lock
wrap-around, control-flag checks, 3D-buffers-are-mono, play cursor driven by
the virtual clock). The hardware (DSCAPS) and play-to-write lead come only
from the platform profile's `sound` section. Nothing is rendered or mixed.
"""

from __future__ import annotations

import struct

import calllog

DS_OK = 0
DSERR_INVALIDPARAM = 0x80070057
DSERR_OUTOFMEMORY = 0x8007000E
DSERR_CONTROLUNAVAIL = 0x8878001E
DSERR_PRIOLEVELNEEDED = 0x88780046
DSERR_BADFORMAT = 0x88780064
DSERR_INVALIDCALL = 0x88780032
DSERR_UNINITIALIZED = 0x887800AA
DSERR_NOINTERFACE = 0x80004002
DSERR_UNSUPPORTED = 0x80004001
DSERR_BUFFERTOOSMALL = 0x88780084

DSBCAPS_PRIMARYBUFFER = 0x1
DSBCAPS_STATIC = 0x2
DSBCAPS_LOCHARDWARE = 0x4
DSBCAPS_LOCSOFTWARE = 0x8
DSBCAPS_CTRL3D = 0x10
DSBCAPS_CTRLFREQUENCY = 0x20
DSBCAPS_CTRLPAN = 0x40
DSBCAPS_CTRLVOLUME = 0x80
DSBCAPS_CTRLPOSITIONNOTIFY = 0x100
DSBCAPS_KNOWN = 0x1 | 0x2 | 0x4 | 0x8 | 0x10 | 0x20 | 0x40 | 0x80 | 0x100 | 0x4000 | 0x8000 | 0x10000 | 0x20000

DSCAPS_FIELDS = (
    "dwFlags dwMinSecondarySampleRate dwMaxSecondarySampleRate dwPrimaryBuffers "
    "dwMaxHwMixingAllBuffers dwMaxHwMixingStaticBuffers dwMaxHwMixingStreamingBuffers "
    "dwFreeHwMixingAllBuffers dwFreeHwMixingStaticBuffers dwFreeHwMixingStreamingBuffers "
    "dwMaxHw3DAllBuffers dwMaxHw3DStaticBuffers dwMaxHw3DStreamingBuffers "
    "dwFreeHw3DAllBuffers dwFreeHw3DStaticBuffers dwFreeHw3DStreamingBuffers "
    "dwTotalHwMemBytes dwFreeHwMemBytes dwMaxContigFreeHwMemBytes "
    "dwUnlockTransferRateHwBuffers dwPlayCpuOverheadSwBuffers dwReserved1 dwReserved2"
).split()
DSCAPS_SIZE = 96

IID_IDIRECTSOUND3DLISTENER = bytes.fromhex("84fa9a278149ce11a5210020af0be560")
IID_IDIRECTSOUND3DBUFFER = bytes.fromhex("86fa9a278149ce11a5210020af0be560")


class SoundProfile:
    def __init__(self, data):
        if not isinstance(data, dict) or set(data) != {"caps", "write_lead_ms"}:
            raise ValueError("sound needs exactly caps and write_lead_ms")
        caps = data["caps"]
        if not isinstance(caps, dict) or set(caps) != set(DSCAPS_FIELDS):
            raise ValueError("sound caps need exactly the DSCAPS fields")
        for name in DSCAPS_FIELDS:
            if type(caps[name]) is not int or not 0 <= caps[name] <= 0xFFFFFFFF:
                raise ValueError(f"{name} must be an unsigned 32-bit integer")
        self.caps_words = tuple(caps[n] for n in DSCAPS_FIELDS)
        lead = data["write_lead_ms"]
        if type(lead) is not int or not 0 <= lead <= 1000:
            raise ValueError("write_lead_ms must be 0..1000")
        self.write_lead_ms = lead


def ds_kind(interface):
    return 1 if interface in ("IDirectSound3DBuffer", "IDirectSound3DListener") else 0


class DirectSoundPlatform:
    def __init__(self, host, boundary, profile):
        self.host = host
        self.boundary = boundary
        self.profile = profile
        self.interfaces = 0
        self.buffers = []
        self.primary = None
        self.listener = None
        self.coop_level = None
        self.arena = host.d3d.arena if host.d3d is not None else None

    def register(self, startup):
        startup.apis["ord1"] = (3, self.create)
        startup.api_modules["ord1"] = "dsound.dll"
        if startup.enable_all:
            startup.enabled.add("ord1")

    def create(self, args):
        guid, out, outer = args
        host = self.host
        if guid or outer or not out:
            raise self.boundary("unsupported_argument", api="DirectSoundCreate", args=args)
        if host.calllog is not None:
            host.calllog.record(calllog.dsound_call(0, "DirectSoundCreate", [guid != 0]))
        if self.interfaces >= 1:
            raise self.boundary("second_directsound_interface")
        self.interfaces += 1
        obj = host.com.new_object("IDirectSound", DirectSoundModel(self))
        host.write_u32(out, obj.pointer)
        return DS_OK

    def alloc(self, size):
        if self.arena is None:
            raise self.boundary("sound_buffer_arena_missing")
        return self.arena.alloc(size, self.boundary)


class DirectSoundModel:
    def __init__(self, platform):
        self.platform = platform

    def GetCaps(self, obj, a):
        pointer = a[0]
        host = self.platform.host
        if not pointer or host.read_u32(pointer) != DSCAPS_SIZE:
            return DSERR_INVALIDPARAM
        host.write_u32s(pointer, [DSCAPS_SIZE, *self.platform.profile.caps_words])
        return DS_OK

    def SetCooperativeLevel(self, obj, a):
        hwnd, level = a
        if hwnd not in self.platform.host.windows or level not in (1, 2, 3, 4):
            return DSERR_INVALIDPARAM
        self.platform.coop_level = level
        return DS_OK

    def CreateSoundBuffer(self, obj, a):
        pointer, out, outer = a
        host = self.platform.host
        if outer or not pointer or not out:
            return DSERR_INVALIDPARAM
        size = host.read_u32(pointer)
        if size not in (20, 36):
            return DSERR_INVALIDPARAM
        _, flags, nbytes, _reserved, fmt_pointer = host.read_u32s(pointer, 5)
        if flags & ~DSBCAPS_KNOWN:
            raise self.platform.boundary("unsupported_buffer_flags", flags=flags)
        if flags & DSBCAPS_PRIMARYBUFFER:
            if nbytes or fmt_pointer or flags & (DSBCAPS_STATIC):
                return DSERR_INVALIDPARAM
            if self.platform.coop_level is None:
                return DSERR_PRIOLEVELNEEDED
            if self.platform.primary is not None:
                raise self.platform.boundary("second_primary_buffer")
            model = BufferModel(self.platform, flags, 0, None, primary=True)
            buffer = host.com.new_object("IDirectSoundBuffer", model)
            self.platform.primary = buffer
        else:
            if nbytes < 4 or nbytes > 0x0FFFFFFF or not fmt_pointer:
                return DSERR_INVALIDPARAM
            fmt = parse_format(host, fmt_pointer)
            if fmt is None or fmt["tag"] != 1:
                return DSERR_BADFORMAT
            if flags & DSBCAPS_CTRL3D and fmt["channels"] != 1:
                return DSERR_INVALIDPARAM
            if nbytes % fmt["align"]:
                return DSERR_INVALIDPARAM
            model = BufferModel(self.platform, flags, nbytes, fmt, primary=False)
            buffer = host.com.new_object("IDirectSoundBuffer", model)
        model.obj = buffer
        self.platform.buffers.append(buffer)
        host.write_u32(out, buffer.pointer)
        return DS_OK


    def DuplicateSoundBuffer(self, obj, a):
        original_pointer, out = a
        host = self.platform.host
        original = host.com.objects.get(original_pointer)
        if original is None or original.interface != "IDirectSoundBuffer" or not out:
            return DSERR_INVALIDPARAM
        source = original.model
        if source.primary:
            return DSERR_INVALIDPARAM
        model = BufferModel(self.platform, source.flags, source.nbytes, source.format, primary=False, memory=source.memory)
        model.volume, model.pan, model.frequency = source.volume, source.pan, source.frequency
        buffer = host.com.new_object("IDirectSoundBuffer", model)
        model.obj = buffer
        self.platform.buffers.append(buffer)
        host.write_u32(out, buffer.pointer)
        return DS_OK


def parse_format(host, pointer):
    raw = host.read(pointer, 16)
    tag, channels, rate, avg, align, bits = struct.unpack("<HHIIHH", raw)
    if channels not in (1, 2) or bits not in (8, 16) or rate == 0 or align != channels * bits // 8:
        return None
    if avg != rate * align:
        return None
    return {"tag": tag, "channels": channels, "rate": rate, "avg": avg, "align": align, "bits": bits}


class BufferModel:
    def __init__(self, platform, flags, nbytes, fmt, primary, memory=None):
        self.platform = platform
        self.flags = flags
        self.nbytes = nbytes
        self.format = fmt
        self.primary = primary
        self.memory = memory if memory is not None else (platform.alloc(nbytes) if nbytes else 0)
        self.volume = 0
        self.pan = 0
        self.frequency = fmt["rate"] if fmt else 0
        self.playing = False
        self.looping = False
        self.start_ms = 0
        self.base = 0
        self.locked = None
        self.obj = None
        self.listener3d = None
        self.three_d = None

    # position model: the play cursor advances with the virtual clock
    def position(self):
        host = self.platform.host
        if not self.playing or not self.format:
            return self.base
        elapsed = host.clock_ms() - self.start_ms
        rate = self.format["avg"] * self.frequency // self.format["rate"]
        moved = elapsed * rate // 1000
        moved -= moved % self.format["align"]
        total = self.base + moved
        if total >= self.nbytes and not self.looping:
            self.playing = False
            self.base = 0
            return 0
        return total % self.nbytes

    def GetCaps(self, obj, a):
        pointer = a[0]
        host = self.platform.host
        if not pointer or host.read_u32(pointer) != 20:
            return DSERR_INVALIDPARAM
        host.write_u32s(pointer, [20, self.flags, self.nbytes, 0, 0])
        return DS_OK

    def GetCurrentPosition(self, obj, a):
        play, write = a
        host = self.platform.host
        if self.primary:
            raise self.platform.boundary("primary_buffer_position_unmodelled")
        position = self.position()
        if play:
            host.write_u32(play, position)
        if write:
            lead = self.platform.profile.write_lead_ms * self.format["avg"] // 1000
            lead -= lead % self.format["align"]
            host.write_u32(write, (position + lead) % self.nbytes)
        return DS_OK

    def GetFormat(self, obj, a):
        pointer, size, written = a
        host = self.platform.host
        if self.format is None:
            raise self.platform.boundary("primary_format_unset")
        data = struct.pack(
            "<HHIIHH", self.format["tag"], self.format["channels"], self.format["rate"],
            self.format["avg"], self.format["align"], self.format["bits"],
        )
        if pointer:
            if size < 16:
                return DSERR_INVALIDPARAM
            host.write(pointer, data)
        if written:
            host.write_u32(written, 16)
        return DS_OK

    def SetFormat(self, obj, a):
        pointer = a[0]
        if not self.primary:
            return DSERR_INVALIDCALL
        if self.platform.coop_level not in (2, 4):
            return DSERR_PRIOLEVELNEEDED
        fmt = parse_format(self.platform.host, pointer) if pointer else None
        if fmt is None or fmt["tag"] != 1:
            return DSERR_BADFORMAT
        self.format = fmt
        self.frequency = fmt["rate"]
        return DS_OK

    def GetVolume(self, obj, a):
        if not a[0]:
            return DSERR_INVALIDPARAM
        if not self.flags & DSBCAPS_CTRLVOLUME:
            return DSERR_CONTROLUNAVAIL
        self.platform.host.write_u32(a[0], self.volume)
        return DS_OK

    def SetVolume(self, obj, a):
        value = a[0] - (1 << 32) if a[0] >= 1 << 31 else a[0]
        if not self.flags & DSBCAPS_CTRLVOLUME:
            return DSERR_CONTROLUNAVAIL
        if not -10000 <= value <= 0:
            return DSERR_INVALIDPARAM
        self.volume = value
        return DS_OK

    def GetPan(self, obj, a):
        if not a[0]:
            return DSERR_INVALIDPARAM
        if not self.flags & DSBCAPS_CTRLPAN:
            return DSERR_CONTROLUNAVAIL
        self.platform.host.write_u32(a[0], self.pan & 0xFFFFFFFF)
        return DS_OK

    def SetPan(self, obj, a):
        value = a[0] - (1 << 32) if a[0] >= 1 << 31 else a[0]
        if not self.flags & DSBCAPS_CTRLPAN:
            return DSERR_CONTROLUNAVAIL
        if not -10000 <= value <= 10000:
            return DSERR_INVALIDPARAM
        self.pan = value
        return DS_OK

    def GetFrequency(self, obj, a):
        if not a[0]:
            return DSERR_INVALIDPARAM
        if not self.flags & DSBCAPS_CTRLFREQUENCY:
            return DSERR_CONTROLUNAVAIL
        self.platform.host.write_u32(a[0], self.frequency)
        return DS_OK

    def SetFrequency(self, obj, a):
        value = a[0]
        if not self.flags & DSBCAPS_CTRLFREQUENCY:
            return DSERR_CONTROLUNAVAIL
        if value == 0:
            value = self.format["rate"]  # DSBFREQUENCY_ORIGINAL
        if not 100 <= value <= 200000:
            return DSERR_INVALIDPARAM
        self.base = self.position()
        self.start_ms = self.platform.host.clock_ms()
        self.frequency = value
        return DS_OK

    def GetStatus(self, obj, a):
        if not a[0]:
            return DSERR_INVALIDPARAM
        self.position()  # lets a finished non-looping buffer stop
        status = (1 if self.playing else 0) | (4 if self.playing and self.looping else 0)
        self.platform.host.write_u32(a[0], status)
        return DS_OK

    def Lock(self, obj, a):
        offset, nbytes, p1, b1, p2, b2, flags = a
        host = self.platform.host
        if self.primary:
            raise self.platform.boundary("primary_buffer_lock_unmodelled")
        if flags & ~3 or not p1 or not b1:
            return DSERR_INVALIDPARAM
        if self.locked is not None:
            raise self.platform.boundary("relock_without_unlock")
        if flags & 1:
            raise self.platform.boundary("lock_from_write_cursor_unmodelled")
        if flags & 2:
            offset, nbytes = 0, self.nbytes
        if offset >= self.nbytes or nbytes > self.nbytes or nbytes == 0 and not flags & 2:
            return DSERR_INVALIDPARAM
        first = min(nbytes, self.nbytes - offset)
        second = nbytes - first
        host.write_u32(p1, self.memory + offset)
        host.write_u32(b1, first)
        if p2:
            host.write_u32(p2, self.memory if second else 0)
        if b2:
            host.write_u32(b2, second)
        self.locked = (self.memory + offset, first, self.memory if second else 0, second)
        return DS_OK

    def Unlock(self, obj, a):
        if self.locked is None:
            return DSERR_INVALIDPARAM
        p1, b1, p2, b2 = a
        if (p1, b1) != self.locked[:2] or (p2, b2) != (self.locked[2], self.locked[3]):
            raise self.platform.boundary("unlock_arguments_differ", args=a, expected=self.locked)
        self.locked = None
        return DS_OK

    def Play(self, obj, a):
        reserved1, priority, flags = a
        if reserved1 or flags & ~0x1F:
            return DSERR_INVALIDPARAM
        if flags & ~1 and not self.primary:
            raise self.platform.boundary("unsupported_play_flags", flags=flags)
        if self.primary:
            self.playing = True
            self.looping = True  # the primary buffer plays continuously
            return DS_OK
        if not self.playing:
            self.base = self.position()
            self.start_ms = self.platform.host.clock_ms()
        self.playing = True
        self.looping = bool(flags & 1)
        return DS_OK

    def Stop(self, obj, a):
        if self.playing and not self.primary:
            self.base = self.position()
        self.playing = False
        return DS_OK

    def SetCurrentPosition(self, obj, a):
        if self.primary:
            return DSERR_INVALIDCALL
        if a[0] >= self.nbytes or a[0] % self.format["align"]:
            return DSERR_INVALIDPARAM
        self.base = a[0]
        self.start_ms = self.platform.host.clock_ms()
        return DS_OK

    def Restore(self, obj, a):
        return DS_OK  # buffers are never lost in this device

    def QueryInterface(self, obj, a):
        riid, out = a
        host = self.platform.host
        if not riid or not out:
            return DSERR_INVALIDPARAM
        iid = host.read(riid, 16)
        if iid == IID_IDIRECTSOUND3DLISTENER:
            if not self.primary or not self.flags & DSBCAPS_CTRL3D:
                host.write_u32(out, 0)
                return DSERR_NOINTERFACE
            if self.platform.listener is None:
                model = ListenerModel(self.platform)
                self.platform.listener = host.com.new_object("IDirectSound3DListener", model)
            else:
                self.platform.listener.refcount += 1
            host.write_u32(out, self.platform.listener.pointer)
            return DS_OK
        if iid == IID_IDIRECTSOUND3DBUFFER:
            if self.primary or not self.flags & DSBCAPS_CTRL3D:
                host.write_u32(out, 0)
                return DSERR_NOINTERFACE
            if self.three_d is None:
                self.three_d = host.com.new_object("IDirectSound3DBuffer", ThreeDBufferModel(self))
            else:
                self.three_d.refcount += 1
            host.write_u32(out, self.three_d.pointer)
            return DS_OK
        raise self.platform.boundary("unsupported_query_interface", iid=iid.hex())


class ListenerModel:
    def __init__(self, platform):
        self.platform = platform
        self.params = {}


class ThreeDBufferModel:
    def __init__(self, buffer):
        self.buffer = buffer
        self.params = {}
