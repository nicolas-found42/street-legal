"""Tests at the original-executable/platform boundary; no Wine invocation."""

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from startup import OUTPUT_ROOT, Boundary, Startup, scratch_output

EXE = Path(__file__).parents[2] / "reference/raw/StreetLegal_Redline.bak.exe"


@unittest.skipUnless(EXE.exists(), "read-only original executable absent")
class StartupTests(unittest.TestCase):
    def test_unknown_import_stops_before_executing_default_zero_thunk(self):
        run = Startup(EXE, enabled=set()).run(limit=1000)
        self.assertEqual(run["stop"]["kind"], "unsupported_import")
        self.assertEqual(run["stop"]["name"], "kernel32.dll!GetVersion")
        self.assertEqual(run["stop"]["return_site"], 0x5D8262)
        self.assertEqual(run["instructions"], 39)
        self.assertFalse(run["winmain_reached"])
        self.assertFalse(run["main_menu_reached"])

    def test_quantum_with_only_current_thread_ready_does_not_bypass_import_guard(self):
        runner = Startup(EXE, enabled=set())
        runner.threads[2] = {"status": "blocked"}
        runner.next_quantum = 38
        trace = runner.run(limit=1000)
        self.assertEqual(trace["stop"]["name"], "kernel32.dll!GetVersion")
        self.assertEqual(trace["instructions"], 39)

    def test_rejected_import_preserves_entry_cpu_stack_and_has_no_result(self):
        from oracle import SENTINEL, STACK_TOP

        for payload, expected in (
            (b"invalid\0", "unsupported_drive_root"),
            (b"\x81\0", "platform_error"),
        ):
            with self.subTest(payload=payload):
                run = Startup(EXE)
                root = run.alloc(16)
                run.write(root, payload)
                address = next(
                    va
                    for va, name in run.import_names.items()
                    if name == "kernel32.dll!GetDriveTypeA"
                )
                entry_esp = ((STACK_TOP - 0x100 - 8) & ~0xF) - 4
                with self.assertRaises(Boundary) as error:
                    run.call(address, [root], regs={"eax": 0x12345678})
                self.assertEqual(error.exception.facts["kind"], expected)
                self.assertNotIn("result", run.events[-1])
                self.assertEqual(run.reg("eip"), address)
                self.assertEqual(run.reg("esp"), entry_esp)
                self.assertEqual(run.reg("eax"), 0x12345678)
                self.assertEqual(run.read_u32s(entry_esp, 2), [SENTINEL, root])

    def test_heap_boundary_keeps_allocations_and_rejects_invalid_handles(self):
        run = Startup(EXE)
        heap = run.heap_create([1, 4096, 0])
        first = run.heap_alloc([heap, 8, 12])
        self.assertEqual(run.read(first, 12), bytes(12))
        run.write(first, b"test")
        self.assertEqual(run.heap_size([heap, 0, first]), 12)
        with self.assertRaises(Boundary):
            run.heap_alloc([0xDEAD, 0, 4])

    def test_single_thread_platform_state_survives_tls_and_recursive_lock_calls(self):
        run = Startup(EXE)
        slot = run.tls_alloc([])
        self.assertEqual(slot, 0)
        self.assertEqual(run.tls_get([slot]), 0)
        run.tls_set([slot, 0x1234])
        self.assertEqual(run.tls_get([slot]), 0x1234)
        lock = run.alloc(24)
        run.cs_init([lock])
        run.cs_enter([lock])
        run.cs_enter([lock])
        self.assertEqual(run.read_u32(lock + 8), 2)
        run.cs_leave([lock])
        run.cs_leave([lock])
        self.assertEqual(run.read_u32(lock + 4), 0xFFFFFFFF)

    def test_exception_filter_returns_previous_handler(self):
        run = Startup(EXE)
        self.assertEqual(run.exception_filter([0x401000]), 0)
        self.assertEqual(run.exception_filter([0x402000]), 0x401000)
        self.assertEqual(run.exception_filter([0]), 0x402000)

    def test_com_apartment_balances_initialization_and_rejects_reserved_argument(self):
        run = Startup(EXE)
        self.assertEqual(run.com_init([0]), 0)
        self.assertEqual(run.com_init([0]), 1)
        run.com_uninit([])
        run.com_uninit([])
        self.assertEqual(run.com_init([0]), 0)
        with self.assertRaises(Boundary):
            run.com_init([1])

    def test_file_overlay_writes_do_not_modify_read_only_stock(self):
        run = Startup(EXE)
        path = run.alloc(32)
        run.write(path, b"diag.log\0")
        original = (EXE.parents[2] / "stock/diag.log").read_bytes()
        handle = run.file_create([path, 0xC0000000, 2, 0, 2, 128, 0])
        data = run.alloc(8)
        run.write(data, b"abc")
        count = run.alloc(4)
        self.assertEqual(run.file_write([handle, data, 3, count, 0]), 1)
        self.assertEqual(run.read_u32(count), 3)
        self.assertEqual(run.overlay["diag.log"], b"abc")
        self.assertEqual((EXE.parents[2] / "stock/diag.log").read_bytes(), original)
        run.file_close([handle])
        with self.assertRaises(Boundary):
            run.file_write([handle, data, 3, count, 0])
        run.write(path, b"..\\outside\0")
        with self.assertRaises(Boundary):
            run.file_create([path, 0x80000000, 1, 0, 3, 128, 0])

    def test_system_time_is_deterministic_profile_not_host_wall_clock(self):
        run = Startup(EXE)
        target = run.alloc(16)
        run.system_time([target])
        self.assertEqual(run.read_u32s(target, 4), [0x000107D3, 0x00010003, 0, 0])
        run.icount = 2500
        run.system_time([target])
        self.assertEqual(
            run.read_u32s(target, 4), [0x000107D3, 0x00010003, 0, 0x00020000]
        )

    def test_overlay_limit_counts_untouched_stock_bytes_before_first_write(self):
        OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=OUTPUT_ROOT) as temp:
            stock = Path(temp)
            original = b"S" * (2 * 1024 * 1024)
            (stock / "stock.bin").write_bytes(original)
            run = Startup(EXE)
            run.stock = stock
            path, written = run.alloc(32), run.alloc(4)
            source = run.alloc(16 * 1024 * 1024)
            run.write(path, b"overlay.bin\0")
            overlay = run.file_create([path, 0x40000000, 0, 0, 2, 128, 0])
            for megabytes in (16, 16, 16, 15):
                self.assertEqual(
                    run.file_write(
                        [overlay, source, megabytes * 1024 * 1024, written, 0]
                    ),
                    1,
                )
            run.file_close([overlay])
            run.write(path, b"stock.bin\0")
            handle = run.file_create([path, 0x40000000, 0, 0, 3, 128, 0])
            self.assertEqual(run.file_write([handle, 0, 0, written, 0]), 1)
            self.assertNotIn("stock.bin", run.overlay)
            with self.assertRaises(Boundary) as error:
                run.file_write([handle, source, 1, written, 0])
            self.assertEqual(error.exception.facts["kind"], "overlay_size_limit")
            self.assertEqual(run.read_u32(written), 0)
            self.assertEqual(run.files[handle]["position"], 0)
            self.assertNotIn("stock.bin", run.overlay)
            self.assertEqual((stock / "stock.bin").read_bytes(), original)
            self.assertEqual(len(run.overlay["overlay.bin"]), 63 * 1024 * 1024)

    def test_zero_byte_write_after_seek_does_not_extend_the_file(self):
        run = Startup(EXE)
        path, written, source = run.alloc(32), run.alloc(4), run.alloc(4)
        run.write(path, b"zero-write.bin\0")
        handle = run.file_create([path, 0x40000000, 0, 0, 2, 128, 0])
        run.write(source, b"ab")
        run.file_write([handle, source, 2, written, 0])
        self.assertEqual(run.file_seek([handle, 10, 0, 0]), 10)
        self.assertEqual(run.file_write([handle, 0, 0, written, 0]), 1)
        self.assertEqual(run.read_u32(written), 0)
        self.assertEqual(run.file_size([handle, 0]), 2)
        self.assertEqual(run.overlay["zero-write.bin"], b"ab")
        self.assertEqual(run.file_seek([handle, 0, 0, 1]), 10)

    def test_semaphore_count_and_blocking_wait_are_explicit(self):
        run = Startup(EXE)
        handle = run.semaphore_create([0, 1, 1, 0])
        self.assertEqual(run.wait_single([handle, 0xFFFFFFFF]), 0)
        self.assertEqual(run.wait_single([handle, 0]), 258)
        self.assertEqual(run.wait_single([handle, 0xFFFFFFFF]), 0)
        self.assertEqual(run.threads[1]["status"], "blocked")
        previous = run.alloc(4)
        self.assertEqual(run.semaphore_release([handle, 1, previous]), 1)
        self.assertEqual(run.read_u32(previous), 0)
        self.assertEqual(run.threads[1]["status"], "runnable")
        self.assertEqual(run.semaphore_release([handle, 1, previous]), 1)
        self.assertEqual(run.semaphore_release([handle, 1, previous]), 0)
        self.assertEqual(run.last_error, 298)
        self.assertEqual(run.wait_single([handle, 0]), 0)

    def test_original_worker_executes_on_its_own_stack_before_next_boundary(self):
        run = Startup(EXE)
        run.enabled.discard("GetTickCount")
        trace = run.run()
        self.assertEqual(trace["stop"]["name"], "kernel32.dll!GetTickCount")
        self.assertEqual(trace["stop"]["return_site"], 0x554EB5)
        self.assertEqual(run.thread_id, 2)
        self.assertGreaterEqual(run.reg("esp"), 0x40400000)
        self.assertLess(run.reg("esp"), 0x40500000)
        self.assertIn(0x554EA0, trace["trail"])
        self.assertEqual(run.tls_get([0]), 0)
        self.assertEqual(run.threads[1]["tls"][0], 0x20000000)

    def test_performance_clock_is_consistent_with_virtual_tick_clock(self):
        run = Startup(EXE)
        target = run.alloc(8)
        run.icount = 12345
        run.elapsed_ms = 7
        self.assertEqual(run.performance_frequency([target]), 1)
        self.assertEqual(run.read_u32s(target, 2), [1000000, 0])
        self.assertEqual(run.performance_counter([target]), 1)
        self.assertEqual(run.read_u32s(target, 2), [19345, 0])
        self.assertEqual(run.clock_ms(), 19)

    def test_file_enumeration_returns_metadata_and_requires_owned_search_handle(self):
        run = Startup(EXE)
        path = run.alloc(64)
        data = run.alloc(320)
        run.write(path, b"Music\\Garage_Shop\\*.*\0")
        handle = run.find_first([path, data])
        self.assertNotEqual(handle, 0xFFFFFFFF)
        self.assertEqual(run.read_cstr(data + 44), b".")
        self.assertEqual(run.read_u32(data), 0x10)
        self.assertEqual(run.find_next([handle, data]), 1)
        self.assertEqual(run.read_cstr(data + 44), b"..")
        self.assertEqual(run.find_close([handle]), 1)
        with self.assertRaises(Boundary):
            run.find_next([handle, data])

    def test_named_mutex_preserves_owner_and_recursive_depth(self):
        run = Startup(EXE)
        name = run.alloc(32)
        run.write(name, b"StreetLegalMutex\0")
        first = run.mutex_create([0, 1, name])
        second = run.mutex_create([0, 0, name])
        self.assertEqual(run.last_error, 183)
        self.assertEqual(run.wait_single([second, 0]), 0)
        self.assertEqual(run.mutex_release([first]), 1)
        self.assertEqual(run.mutexes[first]["depth"], 1)
        self.assertEqual(run.mutex_release([second]), 1)
        self.assertEqual(run.mutex_release([first]), 0)
        self.assertEqual(run.last_error, 288)

    def test_named_mutex_lifetime_ends_with_its_last_handle(self):
        run = Startup(EXE)
        name = run.alloc(32)
        run.write(name, b"LifetimeMutex\0")
        first = run.mutex_create([0, 1, name])
        second = run.mutex_create([0, 0, name])
        self.assertEqual(run.file_close([first]), 1)
        run.threads[2] = {"status": "runnable"}
        run.thread_id = 2
        self.assertEqual(run.wait_single([second, 0]), 258)
        run.thread_id = 1
        self.assertEqual(run.mutex_release([second]), 1)
        self.assertEqual(run.file_close([second]), 1)
        run.thread_id = 2
        fresh = run.mutex_create([0, 1, name])
        self.assertEqual(run.last_error, 0)
        self.assertEqual(run.wait_single([fresh, 0]), 0)
        self.assertEqual(run.mutex_release([fresh]), 1)
        self.assertEqual(run.mutex_release([fresh]), 1)
        self.assertEqual(run.file_close([fresh]), 1)

    def test_closing_a_handle_with_a_pending_wait_stops_before_mutation(self):
        for kind in ("mutex", "semaphore"):
            with self.subTest(kind=kind):
                run = Startup(EXE)
                handle = (
                    run.mutex_create([0, 1, 0])
                    if kind == "mutex"
                    else run.semaphore_create([0, 0, 1, 0])
                )
                run.threads[2] = {"status": "runnable"}
                run.thread_id = 2
                run.wait_single([handle, 0xFFFFFFFF])
                run.thread_id = 1
                with self.assertRaises(Boundary) as error:
                    run.file_close([handle])
                self.assertEqual(
                    error.exception.facts["kind"], "unsupported_close_pending_wait"
                )
                self.assertEqual(run.threads[2]["status"], "blocked")
                self.assertEqual(run.threads[2]["wait"], handle)
                self.assertEqual(
                    run.mutex_release([handle])
                    if kind == "mutex"
                    else run.semaphore_release([handle, 1, 0]),
                    1,
                )
                self.assertEqual(run.threads[2]["status"], "runnable")
                self.assertEqual(run.file_close([handle]), 1)

    def test_closing_another_mutex_alias_preserves_the_waited_handle(self):
        run = Startup(EXE)
        name = run.alloc(32)
        run.write(name, b"WaitingMutex\0")
        first = run.mutex_create([0, 1, name])
        waited = run.mutex_create([0, 0, name])
        run.threads[2] = {"status": "runnable"}
        run.thread_id = 2
        run.wait_single([waited, 0xFFFFFFFF])
        run.thread_id = 1
        self.assertEqual(run.file_close([first]), 1)
        self.assertEqual(run.mutex_release([waited]), 1)
        self.assertEqual(run.threads[2]["status"], "runnable")
        self.assertEqual(run.file_close([waited]), 1)

    def test_memory_status_uses_explicit_32_bit_profile(self):
        run = Startup(EXE)
        target = run.alloc(32)
        run.memory_status([target])
        self.assertEqual(
            run.read_u32s(target, 8),
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

    def test_system_info_exposes_single_processor_x86_address_profile(self):
        run = Startup(EXE)
        target = run.alloc(36)
        run.system_info([target])
        self.assertEqual(
            run.read_u32s(target, 9),
            [0, 4096, 0x10000, 0x7FFEFFFF, 1, 1, 586, 65536, 0x03030006],
        )

    def test_file_size_and_signed_seek_preserve_position_and_eof(self):
        run = Startup(EXE)
        path = run.alloc(32)
        run.write(path, b"probe.bin\0")
        handle = run.file_create([path, 0xC0000000, 0, 0, 2, 128, 0])
        source = run.alloc(8)
        run.write(source, b"abc")
        count = run.alloc(4)
        run.file_write([handle, source, 3, count, 0])
        self.assertEqual(run.file_size([handle, count]), 3)
        self.assertEqual(run.read_u32(count), 0)
        self.assertEqual(run.file_seek([handle, 0xFFFFFFFF, 0, 1]), 2)
        self.assertEqual(run.file_read([handle, source, 3, count, 0]), 1)
        self.assertEqual(run.read_u32(count), 1)
        self.assertEqual(run.read(source, 1), b"c")
        self.assertEqual(run.file_seek([handle, 0xFFFFFFFC, 0, 0]), 0xFFFFFFFF)
        self.assertEqual(run.last_error, 131)
        self.assertEqual(run.files[handle]["position"], 3)

    def test_instruction_budget_stops_before_import_execution(self):
        trace = Startup(EXE).run(limit=38)
        self.assertEqual(trace["stop"]["kind"], "instruction_limit")
        self.assertEqual(trace["instructions"], 38)
        self.assertEqual(trace["events"], [])

    def test_virtual_desktop_has_no_external_cd_player_window(self):
        run = Startup(EXE)
        name = run.alloc(32)
        run.write(name, b"SJE_CdPlayerClass\0")
        self.assertEqual(run.find_window([name, 0]), 0)

    def test_drive_profile_exposes_only_fixed_c_drive(self):
        run = Startup(EXE)
        path = run.alloc(8)
        for root, expected in ((b"A:\\\0", 1), (b"C:\\\0", 3), (b"D:\\\0", 1)):
            run.write(path, root)
            self.assertEqual(run.drive_type([path]), expected)

    def test_original_prefix_is_repeatable_but_cannot_claim_main_menu(self):
        first = Startup(EXE).run(limit=500000)
        second = Startup(EXE).run(limit=500000)
        self.assertEqual(first, second)
        self.assertEqual(first["instructions"], 466336)
        self.assertTrue(first["winmain_reached"])
        self.assertEqual(first["stop"]["kind"], "dialog_request")
        self.assertEqual(first["stop"]["style"], 16)
        self.assertEqual(first["status"], "incomplete")
        self.assertFalse(first["main_menu_reached"])

    def test_output_path_cannot_write_to_protected_or_external_locations(self):
        self.assertEqual(
            scratch_output(OUTPUT_ROOT / "probe.json"), OUTPUT_ROOT / "probe.json"
        )
        for path in (
            EXE,
            EXE.parents[2] / "stock/diag.log",
            Path("/tmp/slr-trace.json"),
        ):
            with self.assertRaises(ValueError):
                scratch_output(path)

    def test_critical_section_blocks_other_thread_and_transfers_ownership(self):
        run = Startup(EXE)
        lock = run.alloc(24)
        run.cs_init([lock])
        run.cs_enter([lock])
        run.threads[2] = {"status": "runnable"}
        run.thread_id = 2
        run.cs_enter([lock])
        self.assertEqual(run.threads[2]["status"], "blocked")
        self.assertEqual(run.read_u32s(lock + 4, 3), [1, 1, 1])
        with self.assertRaises(Boundary):
            run.cs_leave([lock])
        run.thread_id = 1
        run.cs_leave([lock])
        self.assertEqual(run.threads[2]["status"], "runnable")
        self.assertEqual(run.read_u32s(lock + 4, 3), [0, 1, 2])
        run.thread_id = 2
        run.cs_leave([lock])
        self.assertEqual(run.read_u32s(lock + 4, 3), [0xFFFFFFFF, 0, 0])

    def test_wrong_dll_export_cannot_use_same_named_api_model(self):
        run = Startup(EXE)
        address = next(
            a for a, n in run.import_names.items() if n == "kernel32.dll!GetVersion"
        )
        run.import_names[address] = "user32.dll!GetVersion"
        trace = run.run()
        self.assertEqual(trace["stop"]["kind"], "unsupported_import")
        self.assertEqual(trace["stop"]["name"], "user32.dll!GetVersion")

    def test_finite_wait_is_explicitly_unsupported_until_scheduling_has_timers(self):
        run = Startup(EXE)
        handle = run.semaphore_create([0, 0, 1, 0])
        with self.assertRaises(Boundary) as error:
            run.wait_single([handle, 10])
        self.assertEqual(error.exception.facts["kind"], "unsupported_finite_wait")
        self.assertEqual(run.elapsed_ms, 0)

    def test_stock_read_and_enumeration_reject_symlink_escape(self):
        OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=OUTPUT_ROOT) as temp:
            base = Path(temp)
            stock = base / "stock"
            stock.mkdir()
            external = base / "external"
            external.write_bytes(b"outside")
            (stock / "escape").symlink_to(external)
            run = Startup(EXE)
            run.stock = stock
            with self.assertRaises(Boundary):
                run.file_bytes("escape")
            pattern = run.alloc(4)
            run.write(pattern, b"*.*\0")
            with self.assertRaises(Boundary):
                run.find_first([pattern, run.alloc(320)])

    def test_file_creation_requires_existing_parent_and_respects_sharing(self):
        run = Startup(EXE)
        path = run.alloc(64)
        run.write(path, b"missing-dir\\new.bin\0")
        self.assertEqual(
            run.file_create([path, 0x40000000, 0, 0, 2, 128, 0]), 0xFFFFFFFF
        )
        self.assertEqual(run.last_error, 3)
        run.write(path, b"sharing-probe.bin\0")
        first = run.file_create([path, 0x40000000, 0, 0, 2, 128, 0])
        self.assertNotEqual(first, 0xFFFFFFFF)
        self.assertEqual(
            run.file_create([path, 0x40000000, 0, 0, 3, 128, 0]), 0xFFFFFFFF
        )
        self.assertEqual(run.last_error, 32)


if __name__ == "__main__":
    unittest.main()
