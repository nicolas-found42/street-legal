"""Declared D3D9 device model: profile validation and COM semantics."""

import copy
import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from make_boot_profile import PROFILE
from platform_fixtures import (
    EXE, HWND, Boundary, call, create_d3d, create_device, make_startup, profile_data,
)
from platform_profile import D3DCAPS9_FIELDS, PlatformProfile

D3DERR_INVALIDCALL = 0x8876086C
D3DERR_NOTAVAILABLE = 0x8876086A


class ProfileTests(unittest.TestCase):
    def test_declared_profile_validates_and_is_hypothetical(self):
        profile = PlatformProfile(copy.deepcopy(PROFILE))
        self.assertEqual(len(profile.d3d.caps_blob), 304)
        self.assertFalse(profile.metadata["physical_gpu_observed"])
        self.assertEqual(profile.d3d.unspecified_caps_fields, ["Reserved5"])
        words = struct.unpack("<76I", profile.d3d.caps_blob)
        index = [n for n, _ in D3DCAPS9_FIELDS].index("MaxTextureWidth")
        self.assertEqual(words[index], 4096)

    def test_validation_refuses_malformed_declarations(self):
        def with_d3d(mutate):
            data = copy.deepcopy(PROFILE)
            mutate(data["d3d"])
            return data

        bad = [
            lambda d: d["caps"].update(NoSuchField=1),
            lambda d: d["caps"].update(DeviceType=2),
            lambda d: d["caps"].update(MaxTextureWidth=-1),
            lambda d: d["caps"].update(MaxVertexW="x"),
            lambda d: d.pop("available_texture_mem"),
            lambda d: d.update(available_texture_mem=10),
            lambda d: d["adapter_identifier"].update(DeviceIdentifier="nope"),
            lambda d: d["adapter_identifier"].update(Driver="x" * 600),
            lambda d: d["format_checks"].update(default_result=5),
            lambda d: d["format_checks"]["overrides"].append({"method": "Other", "result": 0}),
            lambda d: d.update(supported_query_types=[99]),
            lambda d: d.update(extra=1),
        ]
        for mutate in bad:
            with self.subTest(mutate=mutate), self.assertRaises(ValueError):
                PlatformProfile(with_d3d(mutate))
        for data in (
            {**copy.deepcopy(PROFILE), "label": "real-gpu"},
            {**copy.deepcopy(PROFILE), "unknown": 1},
            {**copy.deepcopy(PROFILE), "window": {"message_delivery": "native"}},
        ):
            with self.assertRaises(ValueError):
                PlatformProfile(data)

    def test_duplicate_json_keys_are_rejected(self):
        import tempfile

        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "p.json"
            path.write_text('{"label": "hypothetical-a", "label": "hypothetical-b"}')
            with self.assertRaises(ValueError):
                PlatformProfile.load(path)


@unittest.skipUnless(EXE.exists(), "local original executable absent")
class InterfaceTests(unittest.TestCase):
    def setUp(self):
        self.startup = make_startup()
        self.d3d = create_d3d(self.startup)

    def test_identifier_layout_and_whql_flag(self):
        out = self.startup.alloc(1096)
        platform = self.startup.d3d
        self.assertEqual(platform.interface_call(5, [self.d3d, 0, 0, out]), 0)
        blob = self.startup.read(out, 1096)
        self.assertTrue(blob.startswith(b"hypothetical_d3d9.dll\0"))
        self.assertEqual(blob[512:512 + 17], b"Hypothetical Dire")
        self.assertEqual(struct.unpack("<II", blob[1056:1064]), (1, 1))
        self.assertEqual(blob[1092:1096], bytes(4))
        self.assertEqual(platform.interface_call(5, [self.d3d, 0, 3, out]), D3DERR_INVALIDCALL)
        self.assertEqual(platform.interface_call(5, [self.d3d, 1, 0, out]), D3DERR_INVALIDCALL)
        self.assertEqual(platform.interface_call(5, [self.d3d, 0, 0, 0]), D3DERR_INVALIDCALL)

    def test_caps_are_written_whole_and_other_device_types_stop(self):
        out = self.startup.alloc(304)
        platform = self.startup.d3d
        self.assertEqual(platform.interface_call(14, [self.d3d, 0, 1, out]), 0)
        self.assertEqual(self.startup.read(out, 304), platform.profile.caps_blob)
        with self.assertRaises(Boundary):
            platform.interface_call(14, [self.d3d, 0, 2, out])
        self.assertEqual(platform.interface_call(14, [self.d3d, 1, 1, out]), D3DERR_INVALIDCALL)

    def test_format_checks_use_declared_default_and_ordered_overrides(self):
        data = copy.deepcopy(PROFILE["d3d"])
        data["format_checks"] = {
            "default_result": 0,
            "overrides": [
                {"method": "CheckDeviceFormat", "format": 0x31545844, "result": D3DERR_NOTAVAILABLE},
                {"method": "CheckDepthStencilMatch", "depth_stencil_format": 75, "result": D3DERR_NOTAVAILABLE},
            ],
        }
        startup = make_startup(d3d=data)
        d3d = create_d3d(startup)
        p = startup.d3d
        self.assertEqual(p.interface_call(10, [d3d, 0, 1, 22, 0, 3, 21]), 0)
        self.assertEqual(p.interface_call(10, [d3d, 0, 1, 22, 0, 3, 0x31545844]), D3DERR_NOTAVAILABLE)
        self.assertEqual(p.interface_call(12, [d3d, 0, 1, 22, 21, 71]), 0)
        self.assertEqual(p.interface_call(12, [d3d, 0, 1, 22, 21, 75]), D3DERR_NOTAVAILABLE)

    def test_create_device_validation_and_x87_precision_change(self):
        self.startup.set_fpcw(0x27F)
        result, device = create_device(self.startup, Windowed=0)
        self.assertEqual(result, 0)
        self.assertEqual(self.startup.uc_fpcw(), 0x7F)  # single precision, no FPU_PRESERVE
        self.assertEqual(self.startup.d3d.fpu_events[0]["before"], 0x27F)

    def test_fpu_preserve_keeps_the_callers_control_word(self):
        startup = make_startup()
        startup.set_fpcw(0x27F)
        result, _ = create_device(startup, flags=0x84 | 2)
        self.assertEqual(result, 0)
        self.assertEqual(startup.uc_fpcw(), 0x27F)
        self.assertEqual(startup.d3d.fpu_events, [])

    def test_unenumerated_fullscreen_mode_is_refused_not_invented(self):
        result, device = create_device(self.startup, BackBufferWidth=1280, BackBufferHeight=1024)
        self.assertEqual(result, D3DERR_INVALIDCALL)
        self.assertIsNone(device)
        self.assertEqual(self.startup.d3d.last_call["refusal"], "fullscreen_mode_not_enumerated")

    def test_creation_refusals_and_stops(self):
        startup = make_startup()
        d3d = create_d3d(startup)
        out = startup.alloc(4)
        pp = startup.alloc(56)
        p = startup.d3d
        startup.write(pp, bytes(56))
        self.assertEqual(p.interface_call(16, [d3d, 0, 1, HWND, 0x84, pp, 0]), D3DERR_INVALIDCALL)
        with self.assertRaises(Boundary):
            p.interface_call(16, [d3d, 0, 1, 0x1234, 0x84, pp, out])  # unknown window
        with self.assertRaises(Boundary):
            p.interface_call(16, [d3d, 0, 1, HWND, 0x20 | 0x40, pp, out])  # ambiguous vertex processing
        with self.assertRaises(Boundary):
            p.interface_call(16, [d3d, 0, 1, HWND, 0x84 | 0x8000, pp, out])
        result, _ = create_device(make_startup())
        self.assertEqual(result, 0)

    def test_second_device_is_refused_by_name(self):
        startup = make_startup()
        create_device(startup)
        with self.assertRaises(Boundary):
            create_device(startup)


@unittest.skipUnless(EXE.exists(), "local original executable absent")
class ThunkTests(unittest.TestCase):
    """Executes real x86 through the generated COM vtable thunks."""

    STUB = bytes.fromhex("8b442404 50 8b08 ff5110 c3".replace(" ", ""))  # slot 4(this)

    def run_slot(self, startup, device, slot):
        code = bytearray(self.STUB)
        code[9] = 4 * slot
        address = startup.scratch(len(code))
        startup.write(address, bytes(code))
        return startup.call(address, [device.pointer], limit=100000)

    def test_vtable_call_reaches_the_model_and_returns_through_stdcall(self):
        startup = make_startup()
        _, device = create_device(startup)
        result = self.run_slot(startup, device, 4)  # GetAvailableTextureMem
        self.assertEqual(result.eax, 256 * 1024 * 1024)
        self.assertEqual(startup.com.counts["IDirect3DDevice9.GetAvailableTextureMem"], 1)

    def test_methods_without_a_model_handler_stop_by_name(self):
        startup = make_startup()
        _, device = create_device(startup)
        with self.assertRaises(Boundary) as stop:
            self.run_slot(startup, device, 16)  # Reset
        self.assertEqual(stop.exception.facts["kind"], "unsupported_com_method")
        self.assertEqual(stop.exception.facts["method"], "Reset")
        self.assertFalse(stop.exception.facts["method_result_supplied"])

    def test_calls_are_recorded_in_the_hook_layer_convention(self):
        import tempfile

        import calllog

        with tempfile.TemporaryDirectory() as directory:
            from graphics import GraphicsProfile
            from make_boot_profile import GRAPHICS
            from startup import Startup

            log = Path(directory) / "d.slrlog"
            startup = Startup(
                EXE,
                graphics_profile=GraphicsProfile(copy.deepcopy(GRAPHICS)),
                platform_profile=PlatformProfile(profile_data()),
                calllog_path=log,
            )
            startup.windows[HWND] = {"class": "T", "title": "t", "visible": True}
            _, device = create_device(startup)
            self.run_slot(startup, device, 4)
            startup.calllog.close()
            records = calllog.Reader(log.read_bytes()).records()
        self.assertEqual(records, [("d3d9", 0, "GetAvailableTextureMem", [])])


@unittest.skipUnless(EXE.exists(), "local original executable absent")
class DeviceTests(unittest.TestCase):
    def setUp(self):
        self.startup = make_startup()
        result, self.device = create_device(self.startup)
        self.assertEqual(result, 0)

    def out(self, size=4):
        return self.startup.alloc(size)

    def test_documented_default_state_and_set_get_roundtrip(self):
        s, d = self.startup, self.device
        out = self.out()
        self.assertEqual(call(s, d, "GetRenderState", 22, out), 0)
        self.assertEqual(s.read_u32(out), 3)  # D3DCULL_CCW
        call(s, d, "GetRenderState", 7, out)
        self.assertEqual(s.read_u32(out), 1)  # ZENABLE follows EnableAutoDepthStencil
        call(s, d, "GetRenderState", 154, out)
        self.assertEqual(s.read_u32(out), struct.unpack("<I", struct.pack("<f", 1.0))[0])
        call(s, d, "GetRenderState", 166, out)  # POINTSIZE_MAX = caps.MaxPointSize
        self.assertEqual(struct.unpack("<f", s.read(out, 4))[0], 8192.0)
        call(s, d, "SetRenderState", 22, 1)
        call(s, d, "GetRenderState", 22, out)
        self.assertEqual(s.read_u32(out), 1)
        with self.assertRaises(Boundary):
            call(s, d, "SetRenderState", 999, 0)
        self.assertEqual(call(s, d, "GetRenderState", 22, 0), D3DERR_INVALIDCALL)

    def test_texture_stage_and_sampler_defaults(self):
        s, d = self.startup, self.device
        out = self.out()
        call(s, d, "GetTextureStageState", 0, 1, out)
        self.assertEqual(s.read_u32(out), 4)  # stage 0 COLOROP MODULATE
        call(s, d, "GetTextureStageState", 1, 1, out)
        self.assertEqual(s.read_u32(out), 1)  # other stages DISABLE
        call(s, d, "GetTextureStageState", 3, 11, out)
        self.assertEqual(s.read_u32(out), 3)  # TEXCOORDINDEX = stage
        self.assertEqual(call(s, d, "GetTextureStageState", 9, 1, out), D3DERR_INVALIDCALL)
        call(s, d, "GetSamplerState", 0, 1, out)
        self.assertEqual(s.read_u32(out), 1)  # WRAP
        call(s, d, "SetSamplerState", 0, 1, 3)
        call(s, d, "GetSamplerState", 0, 1, out)
        self.assertEqual(s.read_u32(out), 3)

    def test_scene_and_draw_rules(self):
        s, d = self.startup, self.device
        self.assertEqual(call(s, d, "EndScene"), D3DERR_INVALIDCALL)
        self.assertEqual(call(s, d, "BeginScene"), 0)
        self.assertEqual(call(s, d, "BeginScene"), D3DERR_INVALIDCALL)
        self.assertEqual(call(s, d, "Present", 0, 0, 0, 0), D3DERR_INVALIDCALL)
        self.assertEqual(call(s, d, "DrawPrimitiveUP", 4, 1, 0x1000, 24), 0)
        self.assertEqual(call(s, d, "EndScene"), 0)
        with self.assertRaises(Boundary):
            call(s, d, "DrawPrimitiveUP", 4, 1, 0x1000, 24)
        self.assertEqual(call(s, d, "Present", 0, 0, 0, 0), 0)
        self.assertEqual(call(s, d, "Clear", 0, 0, 3, 0, 0, 0), 0)
        self.assertEqual(call(s, d, "Clear", 1, 0, 3, 0, 0, 0), D3DERR_INVALIDCALL)
        self.assertEqual(call(s, d, "Clear", 0, 0, 0x10, 0, 0, 0), D3DERR_INVALIDCALL)

    def test_buffer_lock_returns_real_memory_with_documented_state_rules(self):
        s, d = self.startup, self.device
        pp = self.out()
        self.assertEqual(call(s, d, "CreateIndexBuffer", 64, 0, 101, 1, pp, 0), 0)
        ib = s.com.objects[s.read_u32(pp)]
        out = self.out()
        self.assertEqual(call(s, ib, "Lock", 8, 16, out, 0), 0)
        base = s.read_u32(out) - 8
        s.write(s.read_u32(out), b"\xab" * 16)  # the memory is real and writable
        self.assertEqual(call(s, ib, "Lock", 0, 0, out, 0), D3DERR_INVALIDCALL)  # already locked
        self.assertEqual(call(s, ib, "Unlock"), 0)
        self.assertEqual(call(s, ib, "Unlock"), D3DERR_INVALIDCALL)
        self.assertEqual(call(s, ib, "Lock", 60, 8, out, 0), D3DERR_INVALIDCALL)  # past the end
        self.assertEqual(call(s, ib, "Lock", 0, 0, out, 0), 0)
        self.assertEqual(s.read_u32(out), base)
        self.assertEqual(s.read(base + 8, 16), b"\xab" * 16)  # contents persist
        self.assertEqual(call(s, d, "CreateVertexBuffer", 0, 0, 0, 1, pp, 0), D3DERR_INVALIDCALL)

    def test_texture_levels_pitch_and_rect_locking(self):
        s, d = self.startup, self.device
        pp = self.out()
        self.assertEqual(call(s, d, "CreateTexture", 64, 32, 0, 0, 21, 1, pp, 0), 0)
        tex = s.com.objects[s.read_u32(pp)]
        self.assertEqual(call(s, tex, "GetLevelCount"), 7)  # 64x32 -> 1x1 chain
        desc = self.out(32)
        self.assertEqual(call(s, tex, "GetLevelDesc", 1, desc), 0)
        self.assertEqual(struct.unpack("<8I", s.read(desc, 32)), (21, 3, 0, 1, 0, 0, 32, 16))
        locked = self.out(8)
        self.assertEqual(call(s, tex, "LockRect", 0, locked, 0, 0), 0)
        pitch, bits = struct.unpack("<2I", s.read(locked, 8))
        self.assertEqual(pitch, 64 * 4)
        self.assertEqual(call(s, tex, "LockRect", 0, locked, 0, 0), D3DERR_INVALIDCALL)
        self.assertEqual(call(s, tex, "UnlockRect", 0), 0)
        rect = self.out(16)
        s.write(rect, struct.pack("<4i", 4, 2, 8, 4))
        self.assertEqual(call(s, tex, "LockRect", 0, locked, rect, 0), 0)
        self.assertEqual(struct.unpack("<2I", s.read(locked, 8))[1], bits + 2 * pitch + 4 * 4)
        self.assertEqual(call(s, tex, "UnlockRect", 0), 0)
        self.assertEqual(call(s, tex, "LockRect", 9, locked, 0, 0), D3DERR_INVALIDCALL)
        # level 1 follows level 0 in one chain; the surface view aliases it
        surf = self.out()
        self.assertEqual(call(s, tex, "GetSurfaceLevel", 1, surf), 0)
        level = s.com.objects[s.read_u32(surf)]
        self.assertEqual(level.refcount, 2)

    def test_compressed_texture_pitch_uses_blocks(self):
        s, d = self.startup, self.device
        pp = self.out()
        self.assertEqual(call(s, d, "CreateTexture", 16, 16, 1, 0, 0x31545844, 1, pp, 0), 0)
        tex = s.com.objects[s.read_u32(pp)]
        locked = self.out(8)
        call(s, tex, "LockRect", 0, locked, 0, 0)
        self.assertEqual(struct.unpack("<2I", s.read(locked, 8))[0], 4 * 8)  # 4 blocks x 8 bytes
        with self.assertRaises(Boundary):
            call(s, d, "CreateTexture", 4, 4, 1, 0, 0x12345678, 1, pp, 0)

    def test_default_pool_texture_is_not_lockable_unless_dynamic(self):
        s, d = self.startup, self.device
        pp = self.out()
        call(s, d, "CreateTexture", 8, 8, 1, 0, 21, 0, pp, 0)
        tex = s.com.objects[s.read_u32(pp)]
        self.assertEqual(call(s, tex, "LockRect", 0, self.out(8), 0, 0), D3DERR_INVALIDCALL)

    def test_texture_lod_only_exists_in_the_managed_pool(self):
        s, d = self.startup, self.device
        pp = self.out()
        call(s, d, "CreateTexture", 8, 8, 4, 0, 21, 1, pp, 0)
        managed = s.com.objects[s.read_u32(pp)]
        self.assertEqual(call(s, managed, "SetLOD", 2), 0)
        self.assertEqual(call(s, managed, "SetLOD", 1), 2)
        call(s, d, "CreateTexture", 8, 8, 4, 0, 21, 2, pp, 0)
        other = s.com.objects[s.read_u32(pp)]
        self.assertEqual(call(s, other, "SetLOD", 2), 0)
        self.assertEqual(call(s, other, "GetLOD"), 0)

    def test_com_reference_counting_for_bound_resources(self):
        s, d = self.startup, self.device
        pp = self.out()
        call(s, d, "CreateTexture", 8, 8, 1, 0, 21, 1, pp, 0)
        tex = s.com.objects[s.read_u32(pp)]
        self.assertEqual(tex.refcount, 1)
        call(s, d, "SetTexture", 0, tex.pointer)
        self.assertEqual(tex.refcount, 2)
        call(s, d, "SetTexture", 0, 0)
        self.assertEqual(tex.refcount, 1)

    def test_backbuffer_render_target_and_depth_surfaces(self):
        s, d = self.startup, self.device
        out = self.out()
        self.assertEqual(call(s, d, "GetBackBuffer", 0, 0, 0, out), 0)
        back = s.com.objects[s.read_u32(out)]
        self.assertEqual(call(s, d, "GetBackBuffer", 1, 0, 0, out), D3DERR_INVALIDCALL)
        self.assertEqual(call(s, d, "GetRenderTarget", 0, out), 0)
        self.assertIs(s.com.objects[s.read_u32(out)], back)
        desc = self.out(32)
        call(s, back, "GetDesc", desc)
        self.assertEqual(struct.unpack("<8I", s.read(desc, 32))[6:], (800, 600))
        self.assertEqual(call(s, d, "GetDepthStencilSurface", out), 0)
        call(s, d, "GetDisplayMode", 0, desc)
        self.assertEqual(struct.unpack("<4I", s.read(desc, 16)), (800, 600, 60, 22))
        self.assertEqual(call(s, d, "CreateQuery", 9, out), D3DERR_NOTAVAILABLE)


if __name__ == "__main__":
    unittest.main()
