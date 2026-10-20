"""Clock observation preserves original execution and retains exact QPC inputs."""
import re
import struct
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace

from unicorn import UC_HOOK_CODE
from unicorn import x86_const as X

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'oracle'))
from oracle import DEFAULT_EXE, Oracle
from clock_history import ClockHistory


class OracleObserver:
    def __init__(self, oracle):
        self.oracle = oracle
        self.uc = oracle.uc
        self.host = self
        self.thread_id = 1
        self.elapsed_ms = 50
        self.menu_history = SimpleNamespace(chain=lambda p, n: ([], True))

    @property
    def icount(self):
        return self.oracle.icount

    def hook(self, address, callback):
        self.uc.hook_add(UC_HOOK_CODE, callback, None, address, address)

    def reg(self, name):
        return self.uc.reg_read(getattr(X, 'UC_X86_REG_' + name.upper()))

    def try_u32(self, address):
        try:
            return self.oracle.read_u32(address)
        except Exception:
            return None

    def cstr(self, address):
        return self.oracle.read_cstr(address).decode('latin-1')

    def class_name(self, address):
        return '?'


class ClockHistoryTest(unittest.TestCase):
    @staticmethod
    def world(observe):
        oracle = Oracle()
        oracle.write_u32(0x5E7050, 0x300F0000)
        oracle.write_u32(0x5E7048, 0x300F0010)
        oracle.write(0x76A288, struct.pack('<Q', 1000))

        def frequency(o, args):
            o.write(args[0], struct.pack('<Q', 1000000))
            return 1

        def counter(o, args):
            o.write(args[0], struct.pack('<Q', 123456789))
            return 1

        oracle.hook(0x300F0000, frequency, 'stdcall', 1)
        oracle.hook(0x300F0010, counter, 'stdcall', 1)
        observer = OracleObserver(oracle)
        history = ClockHistory(observer, start=0) if observe else None
        result = oracle.call(0x5516C0, float_ret=True)
        return oracle, result, history

    @unittest.skipUnless(Path(DEFAULT_EXE).is_file(), 'local original build606 executable required')
    def test_original_clock_return_and_execution_are_unchanged(self):
        plain, a, _ = self.world(False)
        observed, b, history = self.world(True)
        self.assertEqual((a.eax, a.edx, a.st0_raw, a.icount, a.esp_delta, a.fpcw, a.fpsw),
                         (b.eax, b.edx, b.st0_raw, b.icount, b.esp_delta, b.fpcw, b.fpsw))
        self.assertEqual(a.mem_writes, b.mem_writes)
        self.assertEqual(plain.read(0x76A280, 24), observed.read(0x76A280, 24))
        row = history.sidecar()
        self.assertFalse(row['alters_execution'])
        self.assertFalse(row['truncated'])
        self.assertEqual(row['pending_clocks'], 0)
        self.assertEqual(len(row['events']), 1)
        event = row['events'][0]
        self.assertEqual(event['return_st0_le'], b.st0_raw.hex())
        self.assertEqual(event['external_input']['counter'], 123456789)
        self.assertEqual(event['external_input']['frequency'], 1000000)
        self.assertEqual(event['external_input']['baseline'], 1000)

    @unittest.skipUnless(Path(DEFAULT_EXE).is_file(), 'local original build606 executable required')
    def test_typed_vm_sites_are_original_clock_callers(self):
        source = (Path(__file__).resolve().parents[2] / 'crates/slr-vm/src/clock.rs').read_text()
        mappings = re.findall(r'Self::(\w+) => Some\((0x[0-9a-fA-F_]+)\)', source)
        self.assertGreaterEqual(len(mappings), 8)
        oracle = Oracle()
        for role, literal in mappings:
            caller = int(literal.replace('_', ''), 16)
            instruction = oracle.read(caller - 5, 5)
            self.assertEqual(instruction[0], 0xe8, role)
            offset = struct.unpack('<i', instruction[1:])[0]
            self.assertEqual((caller + offset) & 0xffffffff, 0x5516c0, role)

    @unittest.skipUnless(Path(DEFAULT_EXE).is_file(), 'local original build606 executable required')
    def test_outside_window_records_nothing(self):
        oracle = Oracle()
        history = ClockHistory(OracleObserver(oracle), start=100, end=200, limit=1)
        history.record({'kind': 'outside'})
        self.assertEqual(history.sidecar()['total'], 0)
        oracle.icount = 100
        history.record({'kind': 'one'})
        history.record({'kind': 'two'})
        self.assertEqual(history.sidecar()['total'], 2)
        self.assertEqual(len(history.sidecar()['events']), 1)
        self.assertTrue(history.sidecar()['truncated'])


if __name__ == '__main__':
    unittest.main()
