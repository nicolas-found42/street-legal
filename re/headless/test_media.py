"""Public profile/config and WinMM API seams; no physical CD observations."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from media import MediaProfile
from startup import OUTPUT_ROOT, REPO, Boundary, Startup

EXE = Path(__file__).parents[2] / "reference/raw/StreetLegal_Redline.bak.exe"


def synthetic(**changes):
    data = {
        "label": "hypothetical-unit-test",
        "drive": "D",
        "track_lengths_ms": [10, 20, 30],
        "total_ms": 60,
    }
    data.update(changes)
    return MediaProfile(data)


class MediaProfileTests(unittest.TestCase):
    def test_profile_requires_bounded_declared_hypothetical_inputs(self):
        profile = synthetic()
        self.assertEqual(profile.drive, "D")
        self.assertEqual(profile.tracks, (10, 20, 30))
        self.assertEqual(profile.total, 60)
        self.assertEqual(profile.metadata["kind"], "hypothetical")
        self.assertFalse(profile.metadata["physical_media_observed"])
        failures = {
            stage: 276
            for stage in ("open", "set", "status:count", "status:total", "close")
        }
        failures.update({f"status:track:{track}": 276 for track in range(1, 129)})
        largest = synthetic(track_lengths_ms=[1] * 128, total_ms=128, failures=failures)
        self.assertEqual(len(largest.tracks), 128)
        for changes in (
            {"label": "observed-disc"},
            {"drive": "C"},
            {"drive": "dd"},
            {"track_lengths_ms": []},
            {"track_lengths_ms": [True]},
            {"track_lengths_ms": [1] * 129},
            {"total_ms": 59},
            {"total_ms": 0x80000000},
            {"unknown": 1},
            {"failures": {"guessed-command": 276}},
        ):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                synthetic(**changes)

    def test_profile_load_rejects_duplicate_keys_and_oversized_files_and_freezes_inputs(
        self,
    ):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "profile.json"
            data = {
                "label": "hypothetical-unit-test",
                "drive": "D",
                "track_lengths_ms": [10, 20, 30],
                "total_ms": 60,
            }
            path.write_text(json.dumps(data))
            valid = path.read_bytes()
            profile = MediaProfile.load(path)
            data["track_lengths_ms"][0] = 99
            self.assertEqual(profile.tracks, (10, 20, 30))
            with self.assertRaises(AttributeError):
                profile.total = 1
            self.assertEqual(profile.metadata, MediaProfile.load(path).metadata)
            path.write_bytes(valid[:-1] + b',"drive":"E"}')
            with self.assertRaises(ValueError):
                MediaProfile.load(path)
            path.write_bytes(valid + b" " * 16385)
            with self.assertRaises(ValueError):
                MediaProfile.load(path)


@unittest.skipUnless(EXE.exists(), "read-only original executable absent")
class MciApiTests(unittest.TestCase):
    def test_malformed_open_parameters_stop_named_without_creating_or_writing(self):
        run = Startup(EXE, media_profile=synthetic())
        drive = run.alloc(4)
        run.write(drive, b"D:\0")
        other_drive = run.alloc(4)
        run.write(other_drive, b"E:\0")
        for words in (
            [0, 0xDEADBEEF, 0, drive, 0],
            [1, 0xDEADBEEF, 0x204, drive, 0],
            [0, 0xDEADBEEF, 0x204, other_drive, 0],
            [0, 0xDEADBEEF, 0x204, drive, 1],
        ):
            with self.subTest(words=words):
                packet = run.alloc(20)
                run.write_u32s(packet, words)
                before = run.read(packet, 20)
                with self.assertRaises(Boundary) as error:
                    run.mci_command([0, 0x803, 0x3302, packet])
                self.assertEqual(
                    error.exception.facts["kind"], "unsupported_mci_open_parameters"
                )
                self.assertEqual(error.exception.facts["device_type"], words[2])
                self.assertEqual(run.read(packet, 20), before)
                self.assertEqual(run.mci.devices, {})
                self.assertIsNone(run.mci.last_call["output_write"])

    def open_device(self, profile=None):
        run = Startup(EXE, media_profile=profile or synthetic())
        drive = run.alloc(4)
        run.write(drive, b"D:\0")
        packet = run.alloc(20)
        run.write_u32s(packet, [0, 0xDEADBEEF, 516, drive, 0])
        before = run.read_u32s(packet, 5)
        self.assertEqual(run.mci_command([0, 0x803, 0x3302, packet]), 0)
        after = run.read_u32s(packet, 5)
        device = after[1]
        self.assertEqual(after, [before[0], device, *before[2:]])
        return run, device

    def test_open_close_uses_owned_device_id_and_only_writes_open_output_field(self):
        run, device = self.open_device()
        self.assertEqual(run.mci_command([device, 0x804, 0, 0]), 0)
        self.assertEqual(run.mci_command([device, 0x804, 0, 0]), 257)
        self.assertEqual(run.mci_command([device, 0x814, 0x100, 0]), 257)

    def set_ms(self, run, device):
        packet = run.alloc(12)
        run.write_u32s(packet, [0, 0, 0])
        before = run.read(packet, 12)
        self.assertEqual(run.mci_command([device, 0x80D, 0x400, packet]), 0)
        self.assertEqual(run.read(packet, 12), before)

    def test_set_time_format_and_track_vs_total_status_only_write_return_word(self):
        run, device = self.open_device()
        self.set_ms(run, device)
        for flags, item, track, expected in (
            (0x100, 3, 99, 3),
            (0x110, 1, 2, 20),
            (0x100, 1, 99, 60),
        ):
            packet = run.alloc(16)
            run.write_u32s(packet, [0, 0xABCDEF01, item, track])
            self.assertEqual(run.mci_command([device, 0x814, flags, packet]), 0)
            self.assertEqual(run.read_u32s(packet, 4), [0, expected, item, track])

    def test_optional_profile_registers_exact_winmm_abi_and_declares_hypothesis(self):
        default = Startup(EXE)
        self.assertNotIn("mciSendCommandA", default.apis)
        run = Startup(EXE, media_profile=synthetic())
        self.assertEqual(run.apis["mciSendCommandA"][0], 4)
        self.assertEqual(run.api_modules["mciSendCommandA"], "winmm.dll")
        path = run.alloc(4)
        run.write(path, b"D:\\\0")
        self.assertEqual(default.drive_type([0]), 3)
        self.assertEqual(run.drive_type([path]), 5)
        trace = run.run(limit=38)
        self.assertEqual(trace["profile"], "nt51-hypothetical-mci-v1")
        self.assertEqual(trace["media_scenario"], synthetic().metadata)
        self.assertEqual(trace["event_limit"], 10000)
        self.assertFalse(trace["main_menu_reached"])
        self.assertEqual(trace["status"], "incomplete")
        disabled = Startup(EXE, enabled=set(), media_profile=synthetic())
        self.assertNotIn("mciSendCommandA", disabled.enabled)

    def test_event_budget_is_explicit_bounded_and_stops_before_more_work(self):
        for limit in (0, -1, True, 100001):
            with self.assertRaises(ValueError):
                Startup(EXE, record_limit=limit)
        trace = Startup(EXE, record_limit=1).run(limit=1000)
        self.assertEqual(trace["stop"], {"kind": "record_limit", "limit": 1})
        self.assertEqual(trace["instructions"], 39)
        self.assertEqual(len(trace["events"]), 1)
        self.assertEqual(trace["event_limit"], 1)

    def test_invalid_track_and_declared_api_failure_do_not_write_outputs(self):
        run, device = self.open_device(synthetic(failures={"status:track:2": 276}))
        self.set_ms(run, device)
        for track, error in ((0, 282), (4, 282), (2, 276)):
            packet = run.alloc(16)
            run.write_u32s(packet, [0, 0xAABBCCDD, 1, track])
            before = run.read(packet, 16)
            run.events.append({"import": "winmm.dll!mciSendCommandA"})
            self.assertEqual(run.mci_command([device, 0x814, 0x110, packet]), error)
            self.assertEqual(run.read(packet, 16), before)
            self.assertIsNone(run.events[-1]["mci"]["output_write"])

    def test_unknown_mci_commands_flags_items_and_unset_format_stop_named(self):
        run, device = self.open_device()
        packet = run.alloc(16)
        run.write_u32s(packet, [0, 0, 1, 1])
        for args, kind in (
            ([device, 0xDEAD, 0, 0], "unsupported_mci_command"),
            ([device, 0x814, 0x111, packet], "unsupported_mci_status_flags"),
            ([device, 0x814, 0x110, packet], "unsupported_mci_time_format"),
        ):
            with self.assertRaises(Boundary) as error:
                run.mci_command(args)
            self.assertEqual(error.exception.facts["kind"], kind)
        self.set_ms(run, device)
        run.write_u32s(packet, [0, 0, 99, 0])
        with self.assertRaises(Boundary) as error:
            run.mci_command([device, 0x814, 0x100, packet])
        self.assertEqual(error.exception.facts["kind"], "unsupported_mci_status_item")

    def test_failed_open_preserves_packet_and_close_failure_keeps_device_owned(self):
        run = Startup(EXE, media_profile=synthetic(failures={"open": 276}))
        drive = run.alloc(4)
        run.write(drive, b"D:\0")
        packet = run.alloc(20)
        run.write_u32s(packet, [0, 0xDEADBEEF, 516, drive, 0])
        before = run.read(packet, 20)
        self.assertEqual(run.mci_command([0, 0x803, 0x3302, packet]), 276)
        self.assertEqual(run.read(packet, 20), before)
        run, device = self.open_device(synthetic(failures={"close": 276}))
        self.assertEqual(run.mci_command([device, 0x804, 0, 0]), 276)
        self.set_ms(run, device)

    def test_mci_open_device_budget_and_ids_remain_owned(self):
        run = Startup(EXE, media_profile=synthetic())
        drive = run.alloc(4)
        run.write(drive, b"D:\0")
        packet = run.alloc(20)
        run.write_u32s(packet, [0, 0, 516, drive, 0])
        ids = []
        for _ in range(16):
            self.assertEqual(run.mci_command([0, 0x803, 0x3302, packet]), 0)
            ids.append(run.read_u32(packet + 4))
        self.assertEqual(len(set(ids)), 16)
        with self.assertRaises(Boundary) as error:
            run.mci_command([0, 0x803, 0x3302, packet])
        self.assertEqual(error.exception.facts["kind"], "mci_device_limit")
        self.assertEqual(run.mci_command([ids[0], 0x804, 0, 0]), 0)
        self.assertEqual(run.mci_command([0, 0x803, 0x3302, packet]), 0)
        self.assertNotIn(run.read_u32(packet + 4), ids)

    def test_extension_search_is_case_insensitive_and_bounded(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            (directory / "One.CLASS").write_bytes(b"one")
            (directory / "two.class").write_bytes(b"two")
            (directory / "source.java").write_bytes(b"source")
            run = Startup(EXE)
            run.stock = directory
            pattern = run.alloc(32)
            run.write(pattern, b"*.class\0")
            data = run.alloc(320)
            handle = run.find_first([pattern, data])
            self.assertEqual(run.read_cstr(data + 44), b"One.CLASS")
            self.assertEqual(run.find_next([handle, data]), 1)
            self.assertEqual(run.read_cstr(data + 44), b"two.class")
            self.assertEqual(run.find_next([handle, data]), 0)
            self.assertEqual(run.last_error, 18)
            self.assertEqual(run.find_close([handle]), 1)
            run.write(pattern, b"*?.class\0")
            with self.assertRaises(Boundary):
                run.find_first([pattern, data])
            for index in range(4094):
                (directory / f"extra{index}").touch()
            run.write(pattern, b"*.class\0")
            with self.assertRaises(Boundary) as error:
                run.find_first([pattern, data])
            self.assertEqual(error.exception.facts["kind"], "directory_entry_limit")

    def test_overlay_search_limit_counts_real_names_before_synthetic_dots(self):
        with tempfile.TemporaryDirectory() as temp:
            run = Startup(EXE)
            run.stock = Path(temp)
            run.overlay = {f"entry{index}.class": bytearray() for index in range(4096)}
            pattern = run.alloc(32)
            run.write(pattern, b"*.class\0")
            data = run.alloc(320)
            handle = run.find_first([pattern, data])
            self.assertNotEqual(handle, 0xFFFFFFFF)
            self.assertEqual(run.find_close([handle]), 1)
            before = run.read(data, 320)
            run.overlay["one-too-many.class"] = bytearray()
            with self.assertRaises(Boundary) as error:
                run.find_first([pattern, data])
            self.assertEqual(error.exception.facts["kind"], "directory_entry_limit")
            self.assertEqual(run.read(data, 320), before)

    def test_optional_winmm_thunk_uses_stdcall_and_exact_dll(self):
        run = Startup(EXE, media_profile=synthetic())
        address = next(
            a
            for a, name in run.import_names.items()
            if name == "winmm.dll!mciSendCommandA"
        )
        drive = run.alloc(4)
        run.write(drive, b"D:\0")
        packet = run.alloc(20)
        run.write_u32s(packet, [0, 0, 516, drive, 0])
        result = run.call(address, [0, 0x803, 0x3302, packet], limit=10)
        self.assertEqual(result.eax, 0)
        self.assertEqual(result.esp_delta, 16)
        self.assertEqual(
            run.events[-1]["mci"]["output_write"], {"address": packet + 4, "value": 1}
        )
        before = run.read(packet, 20)
        run.import_names[address] = "kernel32.dll!mciSendCommandA"
        with self.assertRaises(Boundary) as error:
            run.call(address, [0, 0x803, 0x3302, packet], limit=10)
        self.assertEqual(error.exception.facts["kind"], "unsupported_import")
        self.assertEqual(error.exception.facts["name"], "kernel32.dll!mciSendCommandA")
        self.assertEqual(run.read(packet, 20), before)

    def test_cli_loads_explicit_profile_and_records_its_label_and_event_budget(self):
        OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=OUTPUT_ROOT) as temp:
            path = Path(temp)
            config = path / "profile.json"
            out = path / "trace.json"
            config.write_text(
                json.dumps(
                    {
                        "label": "hypothetical-cli-test",
                        "drive": "D",
                        "track_lengths_ms": [10, 20, 30],
                        "total_ms": 60,
                    }
                )
            )
            before = config.read_bytes()
            result = subprocess.run(
                [
                    sys.executable,
                    str(REPO / "re/headless/startup.py"),
                    "--media-profile",
                    str(config),
                    "--event-limit",
                    "5",
                    "--limit",
                    "38",
                    "--out",
                    str(out),
                ],
                capture_output=True,
                text=True,
                check=True,
            )
            trace = json.loads(out.read_text())
            self.assertEqual(trace["media_scenario"]["label"], "hypothetical-cli-test")
            self.assertFalse(trace["media_scenario"]["physical_media_observed"])
            self.assertEqual(trace["event_limit"], 5)
            self.assertEqual(trace["stop"]["kind"], "instruction_limit")
            self.assertEqual(config.read_bytes(), before)
            self.assertEqual(
                json.loads(result.stdout)["media_scenario"], trace["media_scenario"]
            )

    def test_search_handles_are_bounded_and_do_not_alias_after_close(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            (directory / "one.class").write_bytes(b"one")
            run = Startup(EXE)
            run.stock = directory
            pattern = run.alloc(16)
            run.write(pattern, b"*.class\0")
            data = run.alloc(320)
            handles = [run.find_first([pattern, data]) for _ in range(128)]
            self.assertEqual(len(set(handles)), 128)
            with self.assertRaises(Boundary) as error:
                run.find_first([pattern, data])
            self.assertEqual(error.exception.facts["kind"], "find_handle_limit")
            self.assertEqual(run.find_close([handles[0]]), 1)
            self.assertNotIn(run.find_first([pattern, data]), handles)


if __name__ == "__main__":
    unittest.main()
