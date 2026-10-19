"""Declared graphics scenario and public IDirect3D9 COM ABI tests."""

import json
import sys
import tempfile
import unittest
from dataclasses import FrozenInstanceError
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from graphics import COM_SLOTS, GraphicsProfile
from startup import REPO, Boundary, Startup

EXE = REPO / "reference/raw/StreetLegal_Redline.bak.exe"


def synthetic(**changes):
    data = {
        "label": "hypothetical-graphics-unit",
        "creation_sdk_version": 31,
        "creation_result": "interface",
        "adapter_count": 1,
        "mode_lists": {"0:22": [[320, 240, 0, 22]]},
        "display_modes": {"0": [320, 240, 0, 22]},
        "type_checks": {"0,1,22,22,0": 0, "0,1,24,24,0": 0x8876086A},
    }
    data.update(changes)
    return GraphicsProfile(data)


class GraphicsProfileTests(unittest.TestCase):
    def test_graphics_scenario_requires_explicit_bounded_hypothetical_inputs(self):
        profile = synthetic()
        self.assertEqual(profile.modes[(0, 22)], ((320, 240, 0, 22),))
        self.assertFalse(profile.metadata["physical_gpu_observed"])
        self.assertEqual(profile.metadata["declared_creation_sdk_version"], 31)
        self.assertEqual(profile.metadata["abi_reference_sdk_macro"], 32)
        for changes in (
            {"label": "physical-gpu"},
            {"creation_sdk_version": 0},
            {"creation_result": "success"},
            {"adapter_count": 0},
            {"adapter_count": 5},
            {"mode_lists": {"0:22": [[0, 240, 60, 22]]}},
            {"mode_lists": {"0:22": [[320, 240, 60, 24]]}},
            {"mode_lists": {"0:22": [[320, 240, 60, 22]] * 33}},
            {"type_checks": {"0,1,22,22,0": 1}},
            {"type_checks": {"0,1,22,23,0": 0}},
            {"unknown": 0},
        ):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                synthetic(**changes)

    def test_json_loading_is_bounded_duplicate_rejecting_and_immutable(self):
        data = {
            "label": "hypothetical-file",
            "creation_sdk_version": 31,
            "creation_result": "interface",
            "adapter_count": 1,
            "mode_lists": {"0:22": [[320, 240, 0, 22]]},
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "profile.json"
            path.write_text(json.dumps(data))
            profile = GraphicsProfile.load(path)
            self.assertEqual(profile, GraphicsProfile(data))
            data["mode_lists"]["0:22"][0][0] = 999
            self.assertEqual(profile.modes[(0, 22)][0][0], 320)
            with self.assertRaises(TypeError):
                profile.modes[(0, 22)] = ()
            with self.assertRaises(FrozenInstanceError):
                profile.label = "changed"
            path.write_text(
                '{"label":"hypothetical-file","label":"hypothetical-file","creation_sdk_version":31,"creation_result":"interface","adapter_count":1,"mode_lists":{}}'
            )
            with self.assertRaisesRegex(ValueError, "duplicate graphics profile key"):
                GraphicsProfile.load(path)
            path.write_text(json.dumps(data) + " " * 65536)
            with self.assertRaisesRegex(ValueError, "exceeds 64 KiB"):
                GraphicsProfile.load(path)


@unittest.skipUnless(EXE.exists(), "read-only original executable absent")
class Direct3D9ApiTests(unittest.TestCase):
    def test_creation_is_optional_version_checked_and_has_guarded_seventeen_slots(self):
        self.assertNotIn("Direct3DCreate9", Startup(EXE).apis)
        run = Startup(EXE, graphics_profile=synthetic())
        pointer = run.direct3d_create([31])
        self.assertNotEqual(pointer, 0)
        table = run.read_u32(pointer)
        addresses = run.read_u32s(table, 17)
        self.assertEqual(len(set(addresses)), 17)
        with self.assertRaises(Boundary) as error:
            run.direct3d_create([32])
        self.assertEqual(
            error.exception.facts["kind"], "undeclared_d3d_creation_version"
        )
        null = Startup(EXE, graphics_profile=synthetic(creation_result="null"))
        self.assertEqual(null.direct3d_create([31]), 0)
        for slot, args in ((14, [0, 1, 0]), (16, [0, 1, 0, 0, 0, 0])):
            with self.assertRaises(Boundary) as error:
                run.call(addresses[slot], [pointer, *args], limit=100)
            self.assertEqual(error.exception.facts["kind"], "unsupported_com_method")
            self.assertEqual(error.exception.facts["slot"], slot)
            self.assertFalse(error.exception.facts["method_result_supplied"])

    def test_public_com_queries_observe_stack_abi_outputs_and_refusals(self):
        run = Startup(EXE, graphics_profile=synthetic())
        pointer = run.direct3d_create([31])
        addresses = run.read_u32s(run.read_u32(pointer), 17)
        output = run.alloc(24)
        sentinel = b"\xa5" * 24

        def call(slot, *args):
            result = run.call(
                addresses[slot], [pointer, *args], convention="stdcall", limit=100
            )
            self.assertEqual(result.esp_delta, len([pointer, *args]) * 4)
            return result.eax

        self.assertEqual(call(4), 1)
        self.assertEqual(call(6, 0, 22), 1)
        self.assertEqual(call(6, 1, 22), 0)
        run.write(output, sentinel)
        self.assertEqual(call(7, 0, 22, 0, output + 4), 0)
        self.assertEqual(run.read_u32s(output + 4, 4), [320, 240, 0, 22])
        self.assertEqual(run.read(output, 4), sentinel[:4])
        self.assertEqual(run.read(output + 20, 4), sentinel[:4])
        self.assertEqual(
            run.events[-1]["graphics"]["output_write"],
            {"address": output + 4, "words": [320, 240, 0, 22]},
        )
        for slot, args in (
            (7, (1, 22, 0, output + 4)),
            (7, (0, 22, 1, output + 4)),
            (7, (0, 22, 0, 0)),
            (8, (1, output + 4)),
            (8, (0, 0)),
        ):
            with self.subTest(slot=slot, args=args):
                run.write(output, sentinel)
                self.assertEqual(call(slot, *args), 0x8876086C)
                self.assertEqual(run.read(output, 24), sentinel)
                self.assertIsNone(run.events[-1]["graphics"]["output_write"])
        self.assertEqual(call(8, 0, output + 4), 0)
        self.assertEqual(run.read_u32s(output + 4, 4), [320, 240, 0, 22])
        self.assertEqual(call(9, 0, 1, 22, 22, 0), 0)
        self.assertEqual(call(9, 0, 1, 24, 24, 0), 0x8876086A)
        self.assertEqual(call(9, 1, 1, 22, 22, 0), 0x8876086C)
        self.assertEqual(call(9, 0, 0, 22, 22, 0), 0x8876086C)

    def test_undeclared_queries_and_unmodeled_device_do_not_supply_results(self):
        run = Startup(EXE, graphics_profile=synthetic())
        pointer = run.direct3d_create([31])
        addresses = run.read_u32s(run.read_u32(pointer), 17)
        output = run.alloc(24)
        sentinel = b"\xa5" * 24
        cases = (
            (6, [pointer, 0, 24], "undeclared_mode_list"),
            (7, [pointer, 0, 24, 0, output], "undeclared_mode_list"),
            (9, [pointer, 0, 1, 23, 23, 0], "undeclared_device_type_check"),
            (16, [pointer, 0, 1, 0, 0, output, output], "unsupported_com_method"),
            (4, [pointer + 4], "invalid_com_this"),
        )
        for slot, args, kind in cases:
            with self.subTest(slot=slot, kind=kind):
                run.write(output, sentinel)
                with self.assertRaises(Boundary) as error:
                    run.call(addresses[slot], args, limit=100, regs={"eax": 0x12345678})
                self.assertEqual(error.exception.facts["kind"], kind)
                self.assertEqual(run.reg("eax"), 0x12345678)
                self.assertEqual(run.reg("eip"), addresses[slot])
                self.assertEqual(run.read_u32s(run.reg("esp") + 4, len(args)), args)
                self.assertEqual(run.read(output, 24), sentinel)
                self.assertNotIn("result", run.events[-1])
        missing = Startup(EXE, graphics_profile=synthetic(display_modes={}))
        p = missing.direct3d_create([31])
        addresses = missing.read_u32s(missing.read_u32(p), 17)
        with self.assertRaises(Boundary) as error:
            missing.call(addresses[8], [p, 0, missing.alloc(16)], limit=100)
        self.assertEqual(error.exception.facts["kind"], "undeclared_display_mode")

    def test_optional_creation_import_uses_exact_dll_and_com_methods_are_not_exports(
        self,
    ):
        run = Startup(EXE, graphics_profile=synthetic())
        address = next(
            address
            for address, name in run.import_names.items()
            if name == "d3d9.dll!Direct3DCreate9"
        )
        result = run.call(address, [31], convention="stdcall", limit=100)
        self.assertEqual(result.esp_delta, 4)
        self.assertIn(result.eax, run.graphics.objects)
        self.assertEqual(run.graphics.creation_versions, [31])
        run.import_names[address] = "kernel32.dll!Direct3DCreate9"
        with self.assertRaises(Boundary) as error:
            run.call(address, [31], limit=100)
        self.assertEqual(error.exception.facts["kind"], "unsupported_import")
        name = run.alloc(64)
        run.write(name, b"IDirect3D9.GetAdapterModeCount\0")
        module = next(
            handle for handle, module in run.modules.items() if module == "d3d9.dll"
        )
        with self.assertRaises(Boundary) as error:
            run.proc_address([module, name])
        self.assertEqual(error.exception.facts["kind"], "unsupported_export")
        trace = run.run(limit=1)
        self.assertEqual(
            trace["graphics_scenario"],
            {**synthetic().metadata, "observed_creation_sdk_versions": [31]},
        )
        self.assertEqual(trace["event_limit"], 10000)
        self.assertFalse(trace["main_menu_reached"])
        self.assertEqual(trace["status"], "incomplete")

    def test_all_other_com_slots_are_guarded_and_interface_ownership_is_bounded(self):
        run = Startup(EXE, graphics_profile=synthetic())
        pointers = [run.direct3d_create([31]) for _ in range(4)]
        self.assertEqual(len(set(pointers)), 4)
        with self.assertRaises(Boundary) as error:
            run.direct3d_create([31])
        self.assertEqual(error.exception.facts["kind"], "d3d_interface_budget")
        addresses = run.read_u32s(run.read_u32(pointers[0]), 17)
        for slot, (_, nargs) in enumerate(COM_SLOTS):
            if slot in (4, 6, 7, 8, 9):
                continue
            with self.subTest(slot=slot), self.assertRaises(Boundary) as error:
                run.call(addresses[slot], [pointers[0]] + [0] * (nargs - 1), limit=100)
            self.assertEqual(error.exception.facts["kind"], "unsupported_com_method")
            self.assertEqual(error.exception.facts["slot"], slot)
            self.assertFalse(error.exception.facts["method_result_supplied"])
        with self.assertRaises(Boundary) as error:
            run.call(
                addresses[7],
                [pointers[0], 0, 22, 0, 0xDEAD0000],
                limit=100,
                regs={"eax": 123},
            )
        self.assertEqual(error.exception.facts["kind"], "com_error")
        self.assertEqual(run.reg("eax"), 123)
        self.assertNotIn("result", run.events[-1])
        self.assertIsNone(run.events[-1]["graphics"]["output_write"])

    def test_creation_version_mismatch_supplies_no_implicit_null_result(self):
        run = Startup(EXE, graphics_profile=synthetic(creation_sdk_version=32))
        address = next(
            address
            for address, name in run.import_names.items()
            if name == "d3d9.dll!Direct3DCreate9"
        )
        with self.assertRaises(Boundary) as error:
            run.call(address, [31], limit=100, regs={"eax": 123})
        self.assertEqual(
            error.exception.facts["kind"], "undeclared_d3d_creation_version"
        )
        self.assertEqual(run.reg("eax"), 123)
        self.assertEqual(run.reg("eip"), address)
        self.assertNotIn("result", run.events[-1])
        self.assertEqual(run.read_u32(run.reg("esp") + 4), 31)
        self.assertNotEqual(run.call(address, [32], limit=100).eax, 0)


if __name__ == "__main__":
    unittest.main()
