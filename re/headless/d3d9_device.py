"""Declared D3D9 device for the headless original: no GPU, only COM semantics.

This models the documented IDirect3D9 / IDirect3DDevice9 contracts the original
exercises: object lifetime, resource memory (Lock/Unlock return real emulator
memory of the documented size and pitch), scene and lock state validation, and
the documented default device state. Rendering is not performed. Hardware
facts (caps, adapter identity, format support) come only from the declared
`d3d` section of the platform profile. A method without a handler stops by
name; nothing is answered by default.

Microsoft documentation followed: IDirect3D9 / IDirect3DDevice9 reference,
D3DRENDERSTATETYPE / D3DTEXTURESTAGESTATETYPE / D3DSAMPLERSTATETYPE default
values, D3DCREATE_FPU_PRESERVE (without it Direct3D sets single-precision
round-to-nearest on the creating thread), D3DLOCKED_RECT / D3DSURFACE_DESC.
"""

from __future__ import annotations

import struct

from platform_profile import D3DCAPS9_FIELDS

D3D_OK = 0
D3DERR_NOTAVAILABLE = 0x8876086A
D3DERR_INVALIDCALL = 0x8876086C
D3DERR_NOTFOUND = 0x88760866

D3DCREATE_FPU_PRESERVE = 0x2
D3DCREATE_MULTITHREADED = 0x4
D3DCREATE_PUREDEVICE = 0x10
D3DCREATE_SOFTWARE_VP = 0x20
D3DCREATE_HARDWARE_VP = 0x40
D3DCREATE_MIXED_VP = 0x80
D3DCREATE_KNOWN = 0x2 | 0x4 | 0x10 | 0x20 | 0x40 | 0x80 | 0x100 | 0x800
D3DDEVCAPS_HWTRANSFORMANDLIGHT = 0x10000

ARENA_BASE = 0x60000000
ARENA_SIZE = 0x10000000

# format -> (bytes per pixel, None) or (None, block bytes) for DXT
FORMATS = {
    20: (3, None), 21: (4, None), 22: (4, None), 23: (2, None), 24: (2, None),
    25: (2, None), 26: (2, None), 27: (1, None), 28: (1, None), 29: (2, None),
    30: (2, None), 31: (4, None), 32: (4, None), 33: (4, None), 34: (4, None),
    35: (4, None), 36: (8, None), 40: (2, None), 41: (1, None), 50: (1, None),
    51: (2, None), 52: (1, None), 60: (2, None), 61: (2, None), 62: (4, None),
    63: (4, None), 64: (4, None), 67: (4, None), 70: (2, None), 71: (4, None),
    73: (2, None), 75: (4, None), 77: (4, None), 79: (4, None), 80: (2, None),
    81: (2, None), 82: (4, None), 83: (4, None), 110: (8, None), 111: (2, None),
    112: (4, None), 113: (8, None), 114: (4, None), 115: (8, None), 116: (16, None),
    0x31545844: (None, 8), 0x32545844: (None, 16), 0x33545844: (None, 16),
    0x34545844: (None, 16), 0x35545844: (None, 16),
}
DEPTH_FORMATS = {70, 71, 73, 75, 77, 79, 80, 82, 83}

# D3DRENDERSTATETYPE defaults (SDK documentation); floats stored as bit words.
def _f(value):
    return struct.unpack("<I", struct.pack("<f", value))[0]


RENDER_STATE_DEFAULTS = {
    7: None,  # ZENABLE: TRUE iff EnableAutoDepthStencil
    8: 3, 9: 2, 14: 1, 15: 0, 16: 1, 19: 2, 20: 1, 22: 3, 23: 4, 24: 0, 25: 8,
    26: 0, 27: 0, 28: 0, 29: 0, 34: 0, 35: 0, 36: _f(0.0), 37: _f(1.0), 38: _f(1.0),
    48: 0, 52: 0, 53: 1, 54: 1, 55: 1, 56: 8, 57: 0, 58: 0xFFFFFFFF, 59: 0xFFFFFFFF,
    60: 0xFFFFFFFF, 136: 1, 137: 1, 139: 0, 140: 0, 141: 1, 142: 1, 143: 0,
    145: 1, 146: 2, 147: 0, 148: 0, 151: 0, 152: 0, 154: _f(1.0), 155: _f(1.0),
    156: 0, 157: 0, 158: _f(1.0), 159: _f(0.0), 160: _f(0.0), 161: 1, 162: 0xFFFFFFFF,
    163: 0, 165: 0, 166: None,  # POINTSIZE_MAX: caps MaxPointSize
    167: 0, 168: 0xF, 170: _f(0.0), 171: 1, 172: 3, 173: 1, 174: 0, 175: _f(0.0),
    176: 0, 178: _f(1.0), 179: _f(1.0), 180: _f(0.0), 181: _f(0.0), 182: _f(0.0),
    183: _f(0.0), 184: 0, 185: 0, 186: 1, 187: 1, 188: 1, 189: 8, 190: 0xF,
    191: 0xF, 192: 0xF, 193: 0xFFFFFFFF, 194: 0, 195: _f(0.0), 206: 0, 207: 2,
    208: 1, 209: 1,
}
for _w in tuple(range(128, 136)) + tuple(range(198, 206)):
    RENDER_STATE_DEFAULTS[_w] = 0


def texture_stage_default(stage, kind):
    table = {
        1: 4 if stage == 0 else 1, 2: 2, 3: 1, 4: 2 if stage == 0 else 1, 5: 2, 6: 1,
        7: _f(0.0), 8: _f(0.0), 9: _f(0.0), 10: _f(0.0), 11: stage, 22: _f(0.0),
        23: _f(0.0), 24: 0, 26: 1, 27: 1, 28: 1, 32: 0,
    }
    return table.get(kind)


SAMPLER_DEFAULTS = {1: 1, 2: 1, 3: 1, 4: 0, 5: 1, 6: 1, 7: 0, 8: _f(0.0), 9: 0, 10: 1,
                    11: 0, 12: 0, 13: 256}
IDENTITY = struct.pack("<16f", 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)
FPU_SINGLE_PRECISION_CW = 0x007F  # PC=24 bits, RC=nearest, all exceptions masked


def pitch_and_size(fmt, width, height):
    if fmt not in FORMATS:
        raise KeyError(fmt)
    bpp, block = FORMATS[fmt]
    if block:
        pitch = max(1, (width + 3) // 4) * block
        return pitch, max(1, (height + 3) // 4) * pitch
    return width * bpp, width * bpp * height


def level_count(width, height):
    count = 1
    while width > 1 or height > 1:
        width, height = max(1, width // 2), max(1, height // 2)
        count += 1
    return count


class Arena:
    """Resource memory outside the game heap; allocations are never reused."""

    def __init__(self, host):
        self.host = host
        self.next = ARENA_BASE
        self.used = 0

    def alloc(self, size, boundary):
        size = max(16, (size + 15) & ~15)
        if self.next + size > ARENA_BASE + ARENA_SIZE:
            raise boundary("d3d_resource_arena_exhausted", requested=size)
        pointer = self.next
        self.next += size
        self.used += size
        return pointer


class D3D9Platform:
    """Owns the declared hardware, the device object and its resources."""

    def __init__(self, host, boundary, profile, graphics):
        self.host = host
        self.boundary = boundary
        self.profile = profile
        self.graphics = graphics
        self.arena = Arena(host)
        self.device = None
        self.device_events = []
        self.creations = []
        self.last_call = {}
        self.fpu_events = []
        self.refcount = 1  # the reference Direct3DCreate9 returned

    # ----------------------------------------------------------- IDirect3D9
    def interface_call(self, slot, args):
        """Slots beyond the enumeration set; args[0] is the validated `this`."""
        values = args[1:]
        if slot == 1:
            self.refcount += 1
            return self.refcount
        if slot == 2:
            if self.refcount == 0:
                raise self.boundary("com_release_underflow", interface="IDirect3D9")
            self.refcount -= 1
            return self.refcount
        if slot == 5:
            adapter, flags, pointer = values
            if adapter >= self.graphics.profile.adapter_count or not pointer or flags & ~2:
                return D3DERR_INVALIDCALL
            blob = bytearray(self.profile.identifier_blob)
            if not flags & 2:
                blob[1092:1096] = bytes(4)  # WHQLLevel is reported only with D3DENUM_WHQL_LEVEL
            self.host.write(pointer, bytes(blob))
            self.last_call["output_write"] = {"address": pointer, "bytes": len(blob)}
            return D3D_OK
        if slot == 10:
            adapter, devtype, adapter_format, usage, rtype, fmt = values
            if adapter >= self.graphics.profile.adapter_count or devtype not in (1, 2, 3):
                return D3DERR_INVALIDCALL
            return self.format_answer(
                "CheckDeviceFormat",
                {"adapter_format": adapter_format, "usage": usage,
                 "resource_type": rtype, "format": fmt},
            )
        if slot == 11:
            adapter, devtype, fmt, windowed, ms_type, quality = values
            if adapter >= self.graphics.profile.adapter_count or devtype not in (1, 2, 3):
                return D3DERR_INVALIDCALL
            result = self.profile.multisample_default
            if result == D3D_OK and quality:
                self.host.write_u32(quality, 1)
            return result
        if slot == 12:
            adapter, devtype, adapter_format, rt_format, ds_format = values
            if adapter >= self.graphics.profile.adapter_count or devtype not in (1, 2, 3):
                return D3DERR_INVALIDCALL
            return self.format_answer(
                "CheckDepthStencilMatch",
                {"adapter_format": adapter_format, "render_target_format": rt_format,
                 "depth_stencil_format": ds_format},
            )
        if slot == 14:
            adapter, devtype, pointer = values
            if adapter >= self.graphics.profile.adapter_count or devtype not in (1, 2, 3) or not pointer:
                return D3DERR_INVALIDCALL
            if devtype != 1:
                raise self.boundary("undeclared_device_type_caps", device_type=devtype)
            self.host.write(pointer, self.profile.caps_blob)
            self.last_call["output_write"] = {"address": pointer, "bytes": len(self.profile.caps_blob)}
            return D3D_OK
        if slot == 16:
            return self.create_device(values)
        raise self.boundary(
            "unsupported_com_method",
            interface="IDirect3D9",
            slot=slot,
            args=args,
            method_result_supplied=False,
        )

    def refuse(self, reason):
        self.last_call["refusal"] = reason
        return D3DERR_INVALIDCALL

    def format_answer(self, method, fields):
        for rule_method, match, result in self.profile.format_overrides:
            if rule_method == method and all(fields.get(k) == v for k, v in match):
                return result
        return self.profile.format_default

    def create_device(self, values):
        adapter, devtype, hwnd, flags, pparams, out = values
        host = self.host
        if not out or not pparams:
            return self.refuse("null_output_or_parameters")
        if adapter >= self.graphics.profile.adapter_count or devtype not in (1, 2, 3):
            return self.refuse("adapter_or_device_type")
        if devtype != 1:
            raise self.boundary("undeclared_device_type_creation", device_type=devtype)
        if flags & ~D3DCREATE_KNOWN:
            raise self.boundary("unsupported_create_flags", flags=flags)
        vp = flags & (D3DCREATE_SOFTWARE_VP | D3DCREATE_HARDWARE_VP | D3DCREATE_MIXED_VP)
        if bin(vp).count("1") != 1:
            raise self.boundary("ambiguous_vertex_processing", flags=flags)
        if vp & (D3DCREATE_HARDWARE_VP | D3DCREATE_MIXED_VP) and not (
            self.profile.caps_words[7] & D3DDEVCAPS_HWTRANSFORMANDLIGHT
        ):
            raise self.boundary("undeclared_hw_vertex_processing_refusal", flags=flags)
        if self.device is not None:
            raise self.boundary("second_device_unsupported")
        if hwnd not in self.host.known_window_handles():
            raise self.boundary("unknown_focus_window", hwnd=hwnd)
        names = (
            "BackBufferWidth BackBufferHeight BackBufferFormat BackBufferCount "
            "MultiSampleType MultiSampleQuality SwapEffect hDeviceWindow Windowed "
            "EnableAutoDepthStencil AutoDepthStencilFormat Flags "
            "FullScreen_RefreshRateInHz PresentationInterval"
        ).split()
        params = dict(zip(names, host.read_u32s(pparams, 14)))
        self.last_call['present_parameters'] = params
        if params["BackBufferFormat"] not in (21, 22, 23, 24, 25):
            raise self.boundary("unsupported_backbuffer_format", params=params)
        if params["MultiSampleType"] != 0:
            raise self.boundary("multisampled_device_unsupported", params=params)
        if params["BackBufferCount"] > 3 or not (1 <= params["BackBufferWidth"] <= 8192) or not (
            1 <= params["BackBufferHeight"] <= 8192
        ):
            return self.refuse("backbuffer_size_or_count")
        if params["Windowed"] not in (0, 1) or params["EnableAutoDepthStencil"] not in (0, 1):
            return self.refuse("boolean_fields")
        if not params["Windowed"]:
            modes = self.graphics.profile.modes.get((adapter, params["BackBufferFormat"] if params["BackBufferFormat"] in (22, 23, 24) else 22), ())
            wanted = (params["BackBufferWidth"], params["BackBufferHeight"])
            refresh = params["FullScreen_RefreshRateInHz"]
            if not any(m[:2] == wanted and (refresh in (0, m[2])) for m in modes):
                return self.refuse("fullscreen_mode_not_enumerated")
        if params["EnableAutoDepthStencil"] and params["AutoDepthStencilFormat"] not in DEPTH_FORMATS:
            return self.refuse("depth_stencil_format")
        model = DeviceModel(self, params, flags, hwnd, adapter)
        obj = self.host.com.new_object("IDirect3DDevice9", model)
        model.obj = obj
        self.device = obj
        host.write_u32(out, obj.pointer)
        self.creations.append({"flags": flags, "params": params, "adapter": adapter})
        if not flags & D3DCREATE_FPU_PRESERVE:
            before = host.uc_fpcw()
            host.set_fpcw(FPU_SINGLE_PRECISION_CW)
            self.fpu_events.append(
                {"thread": host.thread_id, "before": before, "after": FPU_SINGLE_PRECISION_CW}
            )
        self.last_call["device"] = obj.id
        return D3D_OK


class DeviceModel:
    """IDirect3DDevice9 handlers named after the COM methods."""

    def __init__(self, platform, params, flags, hwnd, adapter):
        self.platform = platform
        self.host = platform.host
        self.params = params
        self.flags = flags
        self.hwnd = hwnd
        self.adapter = adapter
        self.in_scene = False
        self.obj = None
        self.render_states = {}
        self.stage_states = {}
        self.samplers = {}
        self.transforms = {}
        self.viewport = (0, 0, params["BackBufferWidth"], params["BackBufferHeight"], _f(0.0), _f(1.0))
        self.material = None
        self.lights = {}
        self.light_enable = {}
        self.textures = {}
        self.streams = {}
        self.indices = None
        self.fvf = 0
        self.declaration = None
        self.vertex_shader = None
        self.pixel_shader = None
        self.vs_consts = {}
        self.ps_consts = {}
        self.scissor = (0, 0, params["BackBufferWidth"], params["BackBufferHeight"])
        self.clip_planes = {}
        self.targets = {}
        self.depth = None
        self.back_buffers = []
        self.draws = 0
        self.presents = 0
        self.clears = 0
        self.stream_freq = {}
        self.palettes = {}

    # -- helpers
    def new(self, interface, model, state=None):
        child = self.host.com.new_object(interface, model, state)
        return child

    def put(self, pointer, child):
        if not pointer:
            return D3DERR_INVALIDCALL
        self.host.write_u32(pointer, child.pointer)
        return D3D_OK

    def addref(self, child):
        if child is not None:
            child.refcount += 1

    def release(self, child):
        if child is not None:
            self.host.com_release(child)

    def lookup(self, pointer, interface=None):
        if not pointer:
            return None
        obj = self.host.com.objects.get(pointer)
        if obj is None:
            raise self.platform.boundary("invalid_com_argument", pointer=pointer)
        return obj

    def surface(self, width, height, fmt, pool, usage, kind, owner=None, level=0):
        pitch, size = pitch_and_size(fmt, width, height)
        memory = self.platform.arena.alloc(size, self.platform.boundary)
        return self.new(
            "IDirect3DSurface9",
            SurfaceModel(self, width, height, fmt, pool, usage, kind, memory, pitch, owner, level),
        )

    def render_state_default(self, state):
        if state == 7:
            return self.params["EnableAutoDepthStencil"]
        if state == 166:
            return self._max_point_size()
        return RENDER_STATE_DEFAULTS.get(state)

    def _max_point_size(self):
        index = [n for n, _ in D3DCAPS9_FIELDS].index("MaxPointSize")
        return self.platform.profile.caps_words[index]

    # -- identity / queries
    def TestCooperativeLevel(self, obj, a):
        return D3D_OK

    def GetAvailableTextureMem(self, obj, a):
        return self.platform.profile.available_texture_mem

    def GetDeviceCaps(self, obj, a):
        if not a[0]:
            return D3DERR_INVALIDCALL
        self.host.write(a[0], self.platform.profile.caps_blob)
        return D3D_OK

    def GetDirect3D(self, obj, a):
        if not a[0]:
            return D3DERR_INVALIDCALL
        (pointer,) = self.platform.graphics.objects
        self.platform.refcount += 1  # documented: the returned interface is AddRef'd
        self.host.write_u32(a[0], pointer)
        return D3D_OK

    def GetCreationParameters(self, obj, a):
        if not a[0]:
            return D3DERR_INVALIDCALL
        self.host.write_u32s(a[0], [self.adapter, 1, self.hwnd, self.flags])
        return D3D_OK

    def GetDisplayMode(self, obj, a):
        swapchain, out = a
        if swapchain != 0 or not out:
            return D3DERR_INVALIDCALL
        p = self.params
        if p["Windowed"]:
            raise self.platform.boundary("undeclared_desktop_display_mode")
        fmt = 22 if p["BackBufferFormat"] in (21, 22) else p["BackBufferFormat"]
        modes = self.platform.graphics.profile.modes.get((self.adapter, fmt), ())
        refresh = p["FullScreen_RefreshRateInHz"]
        match = next(
            (m for m in modes if m[:2] == (p["BackBufferWidth"], p["BackBufferHeight"])
             and refresh in (0, m[2])),
            None,
        )
        if match is None:
            raise self.platform.boundary("display_mode_not_declared")
        self.host.write_u32s(out, [match[0], match[1], match[2], fmt])
        return D3D_OK

    # -- scene
    def BeginScene(self, obj, a):
        if self.in_scene:
            return D3DERR_INVALIDCALL
        self.in_scene = True
        return D3D_OK

    def EndScene(self, obj, a):
        if not self.in_scene:
            return D3DERR_INVALIDCALL
        self.in_scene = False
        return D3D_OK

    def Present(self, obj, a):
        if self.in_scene:
            return D3DERR_INVALIDCALL
        self.presents += 1
        return D3D_OK

    def Clear(self, obj, a):
        count, rects, flags, color, z, stencil = a
        if (count == 0) != (rects == 0):
            return D3DERR_INVALIDCALL
        if flags & ~7:
            return D3DERR_INVALIDCALL
        if flags & 6 and not self.params["EnableAutoDepthStencil"]:
            return D3DERR_INVALIDCALL
        self.clears += 1
        return D3D_OK

    def _draw(self, name):
        if not self.in_scene:
            raise self.platform.boundary("draw_outside_scene", method=name)
        self.draws += 1
        return D3D_OK

    def DrawPrimitive(self, obj, a):
        return self._draw("DrawPrimitive")

    def DrawIndexedPrimitive(self, obj, a):
        return self._draw("DrawIndexedPrimitive")

    def DrawPrimitiveUP(self, obj, a):
        if not a[2] or not a[3]:
            return D3DERR_INVALIDCALL
        return self._draw("DrawPrimitiveUP")

    def DrawIndexedPrimitiveUP(self, obj, a):
        if not a[4] or not a[6] or not a[7]:
            return D3DERR_INVALIDCALL
        return self._draw("DrawIndexedPrimitiveUP")

    def ValidateDevice(self, obj, a):
        if not a[0]:
            return D3DERR_INVALIDCALL
        self.host.write_u32(a[0], 1)
        return D3D_OK

    # -- state families
    def SetRenderState(self, obj, a):
        if a[0] not in RENDER_STATE_DEFAULTS:
            raise self.platform.boundary("unknown_render_state", state=a[0])
        self.render_states[a[0]] = a[1]
        return D3D_OK

    def GetRenderState(self, obj, a):
        state, out = a
        if state not in RENDER_STATE_DEFAULTS or not out:
            return D3DERR_INVALIDCALL
        value = self.render_states.get(state, self.render_state_default(state))
        self.host.write_u32(out, value)
        return D3D_OK

    def SetTextureStageState(self, obj, a):
        stage, kind, value = a
        if stage >= 8 or texture_stage_default(stage, kind) is None:
            return D3DERR_INVALIDCALL
        self.stage_states[(stage, kind)] = value
        return D3D_OK

    def GetTextureStageState(self, obj, a):
        stage, kind, out = a
        default = texture_stage_default(stage, kind) if stage < 8 else None
        if default is None or not out:
            return D3DERR_INVALIDCALL
        self.host.write_u32(out, self.stage_states.get((stage, kind), default))
        return D3D_OK

    def SetSamplerState(self, obj, a):
        sampler, kind, value = a
        if (sampler >= 16 and sampler not in range(256, 261)) or kind not in SAMPLER_DEFAULTS:
            return D3DERR_INVALIDCALL
        self.samplers[(sampler, kind)] = value
        return D3D_OK

    def GetSamplerState(self, obj, a):
        sampler, kind, out = a
        if (sampler >= 16 and sampler not in range(256, 261)) or kind not in SAMPLER_DEFAULTS or not out:
            return D3DERR_INVALIDCALL
        self.host.write_u32(out, self.samplers.get((sampler, kind), SAMPLER_DEFAULTS[kind]))
        return D3D_OK

    def SetTransform(self, obj, a):
        if not a[1]:
            return D3DERR_INVALIDCALL
        self.transforms[a[0]] = self.host.read(a[1], 64)
        return D3D_OK

    def GetTransform(self, obj, a):
        if not a[1]:
            return D3DERR_INVALIDCALL
        self.host.write(a[1], self.transforms.get(a[0], IDENTITY))
        return D3D_OK

    def SetViewport(self, obj, a):
        if not a[0]:
            return D3DERR_INVALIDCALL
        self.viewport = struct.unpack("<6I", self.host.read(a[0], 24))
        return D3D_OK

    def GetViewport(self, obj, a):
        if not a[0]:
            return D3DERR_INVALIDCALL
        self.host.write_u32s(a[0], list(self.viewport))
        return D3D_OK

    def SetMaterial(self, obj, a):
        if not a[0]:
            return D3DERR_INVALIDCALL
        self.material = self.host.read(a[0], 68)
        return D3D_OK

    def GetMaterial(self, obj, a):
        if not a[0]:
            return D3DERR_INVALIDCALL
        self.host.write(a[0], self.material or bytes(68))
        return D3D_OK

    def SetLight(self, obj, a):
        if not a[1]:
            return D3DERR_INVALIDCALL
        self.lights[a[0]] = self.host.read(a[1], 104)
        return D3D_OK

    def GetLight(self, obj, a):
        if a[0] not in self.lights or not a[1]:
            return D3DERR_INVALIDCALL
        self.host.write(a[1], self.lights[a[0]])
        return D3D_OK

    def LightEnable(self, obj, a):
        # Enabling a never-set light index creates a default light (documented
        # as such); the default light's contents are not read by the original here.
        self.light_enable[a[0]] = int(bool(a[1]))
        return D3D_OK

    def GetLightEnable(self, obj, a):
        if a[0] not in self.light_enable or not a[1]:
            return D3DERR_INVALIDCALL
        self.host.write_u32(a[1], self.light_enable[a[0]])
        return D3D_OK

    def SetClipPlane(self, obj, a):
        if not a[1]:
            return D3DERR_INVALIDCALL
        self.clip_planes[a[0]] = self.host.read(a[1], 16)
        return D3D_OK

    def SetScissorRect(self, obj, a):
        if not a[0]:
            return D3DERR_INVALIDCALL
        self.scissor = tuple(struct.unpack("<4i", self.host.read(a[0], 16)))
        return D3D_OK

    def SetFVF(self, obj, a):
        self.fvf = a[0]
        self.declaration = None
        return D3D_OK

    def GetFVF(self, obj, a):
        if not a[0]:
            return D3DERR_INVALIDCALL
        self.host.write_u32(a[0], self.fvf)
        return D3D_OK

    def SetVertexShaderConstantF(self, obj, a):
        start, data, count = a
        if not data or start + count > 256:
            return D3DERR_INVALIDCALL
        raw = self.host.read(data, 16 * count)
        for i in range(count):
            self.vs_consts[start + i] = raw[16 * i : 16 * i + 16]
        return D3D_OK

    def SetPixelShaderConstantF(self, obj, a):
        start, data, count = a
        if not data or start + count > 224:
            return D3DERR_INVALIDCALL
        raw = self.host.read(data, 16 * count)
        for i in range(count):
            self.ps_consts[start + i] = raw[16 * i : 16 * i + 16]
        return D3D_OK

    # -- bound resources (the device holds COM references to them)
    def SetTexture(self, obj, a):
        stage, pointer = a
        if stage >= 16 and stage not in range(256, 261):
            return D3DERR_INVALIDCALL
        new = self.lookup(pointer)
        old = self.textures.get(stage)
        self.addref(new)
        if new is None:
            self.textures.pop(stage, None)
        else:
            self.textures[stage] = new
        self.release(old)
        return D3D_OK

    def GetTexture(self, obj, a):
        stage, out = a
        if not out:
            return D3DERR_INVALIDCALL
        current = self.textures.get(stage)
        if current is None:
            self.host.write_u32(out, 0)
        else:
            self.addref(current)
            self.host.write_u32(out, current.pointer)
        return D3D_OK

    def SetStreamSource(self, obj, a):
        stream, pointer, offset, stride = a
        if stream >= 16:
            return D3DERR_INVALIDCALL
        new = self.lookup(pointer)
        old = self.streams.get(stream, (None,))[0]
        self.addref(new)
        self.streams[stream] = (new, offset, stride)
        if new is None:
            self.streams.pop(stream)
        self.release(old)
        return D3D_OK

    def SetStreamSourceFreq(self, obj, a):
        self.stream_freq[a[0]] = a[1]
        return D3D_OK

    def SetIndices(self, obj, a):
        new = self.lookup(a[0])
        old = self.indices
        self.addref(new)
        self.indices = new
        self.release(old)
        return D3D_OK

    def SetVertexShader(self, obj, a):
        new = self.lookup(a[0])
        old = self.vertex_shader
        self.addref(new)
        self.vertex_shader = new
        self.release(old)
        return D3D_OK

    def SetPixelShader(self, obj, a):
        new = self.lookup(a[0])
        old = self.pixel_shader
        self.addref(new)
        self.pixel_shader = new
        self.release(old)
        return D3D_OK

    def SetVertexDeclaration(self, obj, a):
        new = self.lookup(a[0])
        old = self.declaration
        self.addref(new)
        self.declaration = new
        self.release(old)
        return D3D_OK

    # -- resource creation
    def CreateVertexBuffer(self, obj, a):
        length, usage, fvf, pool, out, shared = a
        if not out or shared or length == 0 or pool > 3:
            return D3DERR_INVALIDCALL
        memory = self.platform.arena.alloc(length, self.platform.boundary)
        child = self.new("IDirect3DVertexBuffer9", BufferModel(self, "vertex", length, usage, fvf, pool, memory))
        return self.put(out, child)

    def CreateIndexBuffer(self, obj, a):
        length, usage, fmt, pool, out, shared = a
        if not out or shared or length == 0 or pool > 3 or fmt not in (101, 102):
            return D3DERR_INVALIDCALL
        memory = self.platform.arena.alloc(length, self.platform.boundary)
        child = self.new("IDirect3DIndexBuffer9", BufferModel(self, "index", length, usage, fmt, pool, memory))
        return self.put(out, child)

    def CreateTexture(self, obj, a):
        width, height, levels, usage, fmt, pool, out, shared = a
        if not out or shared or width == 0 or height == 0 or pool > 3:
            return D3DERR_INVALIDCALL
        if fmt not in FORMATS:
            raise self.platform.boundary("unsupported_texture_format", format=fmt)
        full = level_count(width, height)
        if levels == 0:
            levels = full
        elif levels > full:
            return D3DERR_INVALIDCALL
        if usage & 0x400:  # D3DUSAGE_AUTOGENMIPMAP exposes a single level
            levels = 1
        child = self.new(
            "IDirect3DTexture9",
            TextureModel(self, width, height, levels, usage, fmt, pool),
        )
        return self.put(out, child)

    def CreateVertexShader(self, obj, a):
        code, out = a
        if not code or not out:
            return D3DERR_INVALIDCALL
        child = self.new("IDirect3DVertexShader9", ShaderModel(self, "vertex", code))
        return self.put(out, child)

    def CreatePixelShader(self, obj, a):
        code, out = a
        if not code or not out:
            return D3DERR_INVALIDCALL
        child = self.new("IDirect3DPixelShader9", ShaderModel(self, "pixel", code))
        return self.put(out, child)

    def CreateVertexDeclaration(self, obj, a):
        elements, out = a
        if not elements or not out:
            return D3DERR_INVALIDCALL
        count = 0
        while True:
            word0, word1 = self.host.read_u32s(elements + 8 * count, 2)
            count += 1
            if (word0 == 0xFF and word1 == 17) or count > 64:
                break
        child = self.new(
            "IDirect3DVertexDeclaration9",
            ShaderModel(self, "declaration", elements, size=8 * count),
        )
        return self.put(out, child)

    def CreateQuery(self, obj, a):
        kind, out = a
        if kind not in self.platform.profile.supported_query_types:
            return D3DERR_NOTAVAILABLE
        raise self.platform.boundary("declared_query_object_unmodelled", query_type=kind)

    def CreateRenderTarget(self, obj, a):
        width, height, fmt, ms, quality, lockable, out, shared = a
        if not out or shared or not width or not height or ms != 0:
            return D3DERR_INVALIDCALL
        if fmt not in FORMATS:
            raise self.platform.boundary("unsupported_surface_format", format=fmt)
        child = self.surface(width, height, fmt, 0, 1, "rendertarget")
        child.model.lockable = bool(lockable)
        return self.put(out, child)

    def CreateDepthStencilSurface(self, obj, a):
        width, height, fmt, ms, quality, discard, out, shared = a
        if not out or shared or not width or not height or ms != 0 or fmt not in DEPTH_FORMATS:
            return D3DERR_INVALIDCALL
        return self.put(out, self.surface(width, height, fmt, 0, 2, "depth"))

    def CreateOffscreenPlainSurface(self, obj, a):
        width, height, fmt, pool, out, shared = a
        if not out or shared or not width or not height or pool not in (0, 2, 3):
            return D3DERR_INVALIDCALL
        if fmt not in FORMATS:
            raise self.platform.boundary("unsupported_surface_format", format=fmt)
        return self.put(out, self.surface(width, height, fmt, pool, 0, "offscreen"))

    def SetRenderTarget(self, obj, a):
        index, pointer = a
        if index != 0 or not pointer:
            return D3DERR_INVALIDCALL
        new = self.lookup(pointer)
        if new.model.usage & 1 == 0:
            return D3DERR_INVALIDCALL
        self.GetRenderTarget_ensure()
        old = self.targets[0]
        self.addref(new)
        self.targets[0] = new
        self.release(old)
        return D3D_OK

    def SetDepthStencilSurface(self, obj, a):
        new = self.lookup(a[0])
        old = self.depth
        self.addref(new)
        self.depth = new
        self.release(old)
        return D3D_OK

    def back_buffer(self, index):
        while len(self.back_buffers) <= index:
            self.back_buffers.append(
                self.surface(
                    self.params["BackBufferWidth"], self.params["BackBufferHeight"],
                    self.params["BackBufferFormat"], 0, 1, "backbuffer",
                )
            )
        return self.back_buffers[index]

    def GetRenderTarget_ensure(self):
        if 0 not in self.targets:
            self.targets[0] = self.back_buffer(0)

    # -- render targets
    def GetBackBuffer(self, obj, a):
        swapchain, index, kind, out = a
        if swapchain != 0 or kind != 0 or not out or index >= max(1, self.params["BackBufferCount"]):
            return D3DERR_INVALIDCALL
        child = self.back_buffer(index)
        self.addref(child)
        self.host.write_u32(out, child.pointer)
        return D3D_OK

    def GetRenderTarget(self, obj, a):
        index, out = a
        if index != 0 or not out:
            return D3DERR_INVALIDCALL
        self.GetRenderTarget_ensure()
        child = self.targets[0]
        self.addref(child)
        self.host.write_u32(out, child.pointer)
        return D3D_OK

    def GetDepthStencilSurface(self, obj, a):
        if not a[0]:
            return D3DERR_INVALIDCALL
        if not self.params["EnableAutoDepthStencil"]:
            return D3DERR_NOTFOUND
        if self.depth is None:
            self.depth = self.surface(
                self.params["BackBufferWidth"], self.params["BackBufferHeight"],
                self.params["AutoDepthStencilFormat"], 0, 2, "depth",
            )
        self.addref(self.depth)
        self.host.write_u32(a[0], self.depth.pointer)
        return D3D_OK


class BufferModel:
    def __init__(self, device, kind, length, usage, fmt, pool, memory):
        self.device = device
        self.kind = kind
        self.length = length
        self.usage = usage
        self.fmt = fmt
        self.pool = pool
        self.memory = memory
        self.locked = False
        self.lock_count = 0

    def Lock(self, obj, a):
        offset, size, out, flags = a
        if not out or self.locked or offset > self.length or offset + size > self.length:
            return D3DERR_INVALIDCALL
        self.locked = True
        self.lock_count += 1
        self.device.host.write_u32(out, self.memory + offset)
        return D3D_OK

    def Unlock(self, obj, a):
        if not self.locked:
            return D3DERR_INVALIDCALL
        self.locked = False
        return D3D_OK

    def PreLoad(self, obj, a):
        return None

    def GetDesc(self, obj, a):
        if not a[0]:
            return D3DERR_INVALIDCALL
        size_type = 8 if self.kind == "vertex" else 10  # D3DRTYPE_VERTEXBUFFER / INDEXBUFFER
        if self.kind == "vertex":
            words = [100, size_type, self.usage, self.pool, self.length, self.fmt]
        else:
            words = [self.fmt, size_type, self.usage, self.pool, self.length]
        self.device.host.write_u32s(a[0], words)
        return D3D_OK

    def GetType(self, obj, a):
        return 8 if self.kind == "vertex" else 10

    def GetDevice(self, obj, a):
        raise self.device.platform.boundary("unsupported_com_method", method="GetDevice")


class ShaderModel:
    def __init__(self, device, kind, source, size=None):
        self.device = device
        self.kind = kind
        self.source = source
        self.size = size


class SurfaceModel:
    def __init__(self, device, width, height, fmt, pool, usage, kind, memory, pitch, owner, level):
        self.device = device
        self.width = width
        self.height = height
        self.fmt = fmt
        self.pool = pool
        self.usage = usage
        self.kind = kind
        self.memory = memory
        self.pitch = pitch
        self.owner = owner
        self.level = level
        self.locked = False

    def GetDesc(self, obj, a):
        if not a[0]:
            return D3DERR_INVALIDCALL
        self.device.host.write_u32s(
            a[0], [self.fmt, 1, self.usage, self.pool, 0, 0, self.width, self.height]
        )
        return D3D_OK

    def LockRect(self, obj, a):
        locked, rect, flags = a
        if not locked or self.locked:
            return D3DERR_INVALIDCALL
        if self.pool == 0 and not self.usage & 0x200 and self.kind == "texture-level":
            return D3DERR_INVALIDCALL
        offset = 0
        if rect:
            left, top, right, bottom = struct.unpack("<4i", self.device.host.read(rect, 16))
            if not (0 <= left < right <= self.width and 0 <= top < bottom <= self.height):
                return D3DERR_INVALIDCALL
            bpp, block = FORMATS[self.fmt]
            if block:
                if left % 4 or top % 4:
                    return D3DERR_INVALIDCALL
                offset = (top // 4) * self.pitch + (left // 4) * block
            else:
                offset = top * self.pitch + left * bpp
        self.locked = True
        self.device.host.write_u32s(locked, [self.pitch, self.memory + offset])
        return D3D_OK

    def UnlockRect(self, obj, a):
        if not self.locked:
            return D3DERR_INVALIDCALL
        self.locked = False
        return D3D_OK

    def GetType(self, obj, a):
        return 1  # D3DRTYPE_SURFACE


class TextureModel:
    def __init__(self, device, width, height, levels, usage, fmt, pool):
        self.device = device
        self.width = width
        self.height = height
        self.levels = levels
        self.usage = usage
        self.fmt = fmt
        self.pool = pool
        self.surfaces = {}
        self.lod = 0
        self.priority = 0
        w, h = width, height
        self.chain = []
        total = 0
        for _ in range(levels):
            pitch, size = pitch_and_size(fmt, w, h)
            self.chain.append((w, h, pitch, size, total))
            total += (size + 15) & ~15
            w, h = max(1, w // 2), max(1, h // 2)
        self.memory = device.platform.arena.alloc(total, device.platform.boundary)

    def level(self, index):
        if index not in self.surfaces:
            w, h, pitch, size, offset = self.chain[index]
            surface = self.device.new(
                "IDirect3DSurface9",
                SurfaceModel(self.device, w, h, self.fmt, self.pool, self.usage,
                             "texture-level", self.memory + offset, pitch, self, index),
            )
            self.surfaces[index] = surface
        return self.surfaces[index]

    def GetLevelCount(self, obj, a):
        return self.levels

    def GetLevelDesc(self, obj, a):
        level, out = a
        if level >= self.levels or not out:
            return D3DERR_INVALIDCALL
        w, h = self.chain[level][:2]
        self.device.host.write_u32s(out, [self.fmt, 3, self.usage, self.pool, 0, 0, w, h])
        return D3D_OK

    def GetSurfaceLevel(self, obj, a):
        level, out = a
        if level >= self.levels or not out:
            return D3DERR_INVALIDCALL
        child = self.level(level)
        child.refcount += 1
        self.device.host.write_u32(out, child.pointer)
        return D3D_OK

    def LockRect(self, obj, a):
        level, locked, rect, flags = a
        if level >= self.levels:
            return D3DERR_INVALIDCALL
        child = self.level(level)
        return child.model.LockRect(child, [locked, rect, flags])

    def UnlockRect(self, obj, a):
        level = a[0]
        if level >= self.levels:
            return D3DERR_INVALIDCALL
        child = self.level(level)
        return child.model.UnlockRect(child, [])

    def GetType(self, obj, a):
        return 3  # D3DRTYPE_TEXTURE

    def SetLOD(self, obj, a):
        # Only managed-pool textures keep an LOD; for others the call returns 0.
        if self.pool != 1:
            return 0
        previous = self.lod
        self.lod = min(a[0], self.levels - 1)
        return previous

    def GetLOD(self, obj, a):
        return self.lod if self.pool == 1 else 0

    def SetPriority(self, obj, a):
        if self.pool != 1:
            return 0
        previous = self.priority
        self.priority = a[0]
        return previous

    def GetPriority(self, obj, a):
        return self.priority if self.pool == 1 else 0

    def PreLoad(self, obj, a):
        return None
