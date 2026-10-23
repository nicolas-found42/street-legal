#!/usr/bin/env python3
"""Data-free coverage integrity and instruction traversal controls."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from inventory import graph, pseudo_blocks, validated_kuna, ghidra_index


class TinyImage:
    base = 0x1000
    imports = {}

    def __init__(self, code):
        self.data = code

    def is_code(self, address):
        return self.base <= address < self.base + len(self.data)


class InventoryTests(unittest.TestCase):
    def test_calls_discover_real_target_and_indirect_jump_stays_unresolved(self):
        # call 0x1020; jmp eax. A misleading CALL after the terminal jump is
        # unreachable and must not create a discovered entry at 0x1030.
        code = bytearray(b"\x90" * 64)
        code[:12] = b"\xe8\x1b\x00\x00\x00\xff\xe0\xe8\x24\x00\x00\x00"
        code[32] = 0xc3
        result = graph(TinyImage(bytes(code)), {0x1000})
        self.assertEqual(result["entries_scanned"], [0x1000, 0x1020])
        self.assertEqual(result["edges"], [{"from": "0x00001000", "site": "0x00001000",
                                            "to": "0x00001020", "kind": "direct_call"}])
        self.assertEqual(result["indirect"][0]["resolution"], "unresolved")
        self.assertEqual(result["indirect"][0]["site"], "0x00001005")

    def test_conditional_paths_are_followed_without_contiguous_body_guess(self):
        # jne 0x1005; ret; data; data; ret -- branch lands beyond first ret.
        result = graph(TinyImage(b"\x75\x03\xc3\xcc\xcc\xc3"), {0x1000})
        self.assertEqual(result["decoded_instruction_addresses"], 3)
        self.assertEqual(result["gaps"], [])

    def test_absolute_iat_slot_does_not_claim_resolved_game_dispatch(self):
        image = TinyImage(b"\xff\x15\x00\x20\x00\x00\xc3")
        image.imports = {0x2000: "KERNEL32.dll!Sleep"}
        result = graph(image, {0x1000})
        self.assertEqual(result["indirect"][0]["resolution"], "import_slot")
        self.assertEqual(result["entries_scanned"], [0x1000])

    def test_tail_to_known_entry_records_original_branch_site(self):
        result = graph(TinyImage(b"\xeb\x01\xcc\xc3"), {0x1000, 0x1003})
        self.assertEqual(result["edges"], [{"from": "0x00001000", "site": "0x00001000",
                                            "to": "0x00001003", "kind": "direct_tail"}])

    def test_wrong_binary_and_changed_cache_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path, manifest = Path(directory) / "inventory.json", Path(directory) / "manifest.json"
            path.write_text(json.dumps({"count": 1, "functions": [{"address": 0x1000}]}))
            identity = {"binary_sha256": "correct", "exit_code": 0,
                        "inventory_sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
            manifest.write_text(json.dumps(identity))
            self.assertEqual(set(validated_kuna(path, manifest, "correct")[0]), {0x1000})
            with self.assertRaisesRegex(ValueError, "another executable"):
                validated_kuna(path, manifest, "wrong")
            path.write_text(path.read_text() + "\n")
            with self.assertRaisesRegex(ValueError, "content/exit"):
                validated_kuna(path, manifest, "correct")

    def test_missing_failed_and_duplicate_discovery_cannot_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            path, manifest = Path(directory) / "inventory.json", Path(directory) / "manifest.json"
            documents = [{"count": 0, "functions": []},
                         {"count": 2, "functions": [{"address": 4096}]},
                         {"count": 2, "functions": [{"address": 4096}, {"address": 4096}]}]
            for document in documents:
                path.write_text(json.dumps(document))
                manifest.write_text(json.dumps({"binary_sha256": "correct", "exit_code": 0,
                                               "inventory_sha256": hashlib.sha256(path.read_bytes()).hexdigest()}))
                with self.assertRaises(ValueError):
                    validated_kuna(path, manifest, "correct")

    def test_c_emission_and_warnings_never_imply_semantics(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "pseudo.c"
            path.write_text("//==== 00401000 FUN_00401000\nvoid f(){}\n"
                            "//==== 00401010 FUN_00401010\n/* WARNING: missing flow */\nvoid g(){}\n"
                            "//==== 00401020 FUN_00401020\n// FAILED timeout\n")
            rows = pseudo_blocks(path)
            self.assertEqual(rows[0x401000]["semantic_recovery"], "unverified")
            self.assertTrue(rows[0x401010]["diagnostics"])
            self.assertEqual(rows[0x401020]["status"], "failed")

    def test_game_error_strings_are_not_decompiler_diagnostics(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "pseudo.c"
            path.write_text('//==== 00401000 FUN_00401000\nvoid f(){log("FAILED WARNING");}\n')
            self.assertEqual(pseudo_blocks(path)[0x401000]["diagnostics"], [])

    def test_ghidra_address_count_is_never_used_as_contiguous_extent(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "index.txt"
            path.write_text("00401000 FUN_00401000 3\n")
            self.assertEqual(ghidra_index(path), {0x401000: {"symbol": "FUN_00401000", "body_address_count": 3}})
            path.write_text(path.read_text() * 2)
            with self.assertRaisesRegex(ValueError, "duplicate"):
                ghidra_index(path)


if __name__ == "__main__":
    unittest.main()
