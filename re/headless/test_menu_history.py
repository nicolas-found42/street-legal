"""Read-only menu observer rejects incomplete lists and pending dialog frames."""
import unittest
from types import SimpleNamespace
from menu_history import MenuHistory


class MemoryObserver:
    def __init__(self):
        self.mem = {}
        self.names = {}
        self.frame_entries = {}
        self.class_names = {100: 'java.game.GameLogic', 200: 'java.game.MainMenu'}
        self.host = SimpleNamespace(icount=9)
        self.stats = {'native_calls': 3}
        self.natives_log = SimpleNamespace(records=2)
        self.all_log = SimpleNamespace(records=3)

    def try_u32(self, address):
        return self.mem.get(address)

    def cstr(self, address):
        return self.names.get(address)

    def class_name(self, address):
        return self.class_names.get(address, '?')


class MenuHistoryTest(unittest.TestCase):
    def setUp(self):
        self.o = MemoryObserver()
        self.h = MenuHistory(self.o)
        # JVM -> empty thread-list sentinel; GameLogic -> actualState variable.
        self.o.mem.update({0x1018: 0x2000, 0x2008: 0x2010, 0x2014: 0,
                           0x101c: 0x2100, 0x2108: 0x2110, 0x2114: 0,
                           100 + 0x1d0: 0x3000, 0x300c: 0x4000,
                           0x4000: 1, 0x4004: 0x5000, 0x4008: 1, 0x5000: 0x6000,
                           0x600c: 0x7000, 0x6008: 0x8000, 0x800c: 200})
        self.o.names[0x7000] = 'actualState'

    def test_completed_state_records_exact_cutoffs_without_writes(self):
        before = dict(self.o.mem)
        self.h.sample(0x1000, 2)
        row = self.h.snapshots[-1]
        self.assertTrue(row['complete'])
        self.assertEqual(row['iteration_completed'], 1)
        self.assertEqual((row['all_records'], row['filtered_records']), (3, 2))
        self.assertEqual(before, self.o.mem)

    def test_idle_count_requires_subsequent_same_object_samples(self):
        self.h.sample(0x1000, 2)
        self.assertEqual(self.h.snapshots[-1]['idle_frames'], 0)
        self.h.sample(0x1000, 3)
        self.assertEqual(self.h.snapshots[-1]['idle_frames'], 1)
        self.o.mem.update({0x6008: 0x8100, 0x810c: 200})
        self.h.sample(0x1000, 4)
        self.assertEqual(self.h.snapshots[-1]['idle_frames'], 0)

    def test_invalid_cycle_cannot_claim_completion(self):
        self.o.mem.update({0x2008: 0x9000, 0x9004: 0x9000})
        self.h.sample(0x1000, 2)
        self.assertFalse(self.h.snapshots[-1]['complete'])

    def test_live_dialog_frame_blocks_completion(self):
        self.o.class_names[300] = 'java.game.MainMenuDialog'
        self.o.mem.update({0x2008: 0x9000, 0x9004: 0x2010,
                           0x900c: 0xa000, 0xa008: 0xb000, 0xb004: 0xa010, 0xa014: 0,
                           0xb034: 300, 0xb038: 0xc000})
        self.o.names[0xc000] = '<init>'
        self.h.sample(0x1000, 2)
        self.assertFalse(self.h.snapshots[-1]['complete'])
        self.assertEqual(self.h.snapshots[-1]['pending'][0]['class'], 'java.game.MainMenuDialog')

    def test_waiting_dialog_still_blocks_completion(self):
        self.o.class_names[300] = 'java.game.MainMenuDialog'
        self.o.mem.update({0x2108: 0x9000, 0x9004: 0x2110,
                           0x900c: 0xa000, 0xa008: 0xb000,
                           0xb004: 0xa010, 0xa014: 0, 0xb034: 300})
        self.h.sample(0x1000, 2)
        self.assertFalse(self.h.snapshots[-1]['complete'])
        self.assertEqual(len(self.h.snapshots[-1]['pending']), 1)

    def test_null_link_is_not_a_valid_tail(self):
        self.o.mem.update({0x2008: 0x9000, 0x9004: 0})
        self.h.sample(0x1000, 2)
        self.assertFalse(self.h.snapshots[-1]['complete'])

    def test_original_empty_bottom_frame_is_not_a_pending_method(self):
        self.o.mem.update({0x2008: 0x9000, 0x9004: 0x2010,
                           0x900c: 0xa000, 0xa008: 0xb000,
                           0xb004: 0xa010, 0xa014: 0, 0xb034: 0,
                           0xb02c: 0, 0xb030: 0, 0xb03c: 0x9000})
        self.o.frame_entries[0xb000] = 0
        self.h.sample(0x1000, 2)
        self.assertTrue(self.h.snapshots[-1]['complete'])
        self.assertEqual(self.h.snapshots[-1]['base_frames'], [0xb000])
        self.o.mem[0xb02c] = 1
        self.h.sample(0x1000, 3)
        self.assertFalse(self.h.snapshots[-1]['complete'])

    def test_unreadable_live_frame_cannot_claim_completion(self):
        self.o.mem.update({0x2008: 0x9000, 0x9004: 0x2010,
                           0x900c: 0xa000, 0xa008: 0xb000, 0xb004: 0xa010, 0xa014: 0})
        self.h.sample(0x1000, 2)
        self.assertFalse(self.h.snapshots[-1]['complete'])
        self.assertEqual(self.h.snapshots[-1]['unknown_frames'], [0xb000])

    def test_variable_count_cannot_exceed_capacity(self):
        self.o.mem[0x4000] = 0
        self.h.sample(0x1000, 2)
        self.assertFalse(self.h.snapshots[-1]['complete'])

    def test_duplicate_actual_state_is_ambiguous(self):
        self.o.mem.update({0x4000: 2, 0x4008: 2, 0x5004: 0x6100,
                           0x610c: 0x7000, 0x6108: 0x8000})
        self.h.sample(0x1000, 2)
        self.assertFalse(self.h.snapshots[-1]['complete'])

    def test_unreadable_state_cannot_claim_completion(self):
        del self.o.mem[0x6008]
        self.h.sample(0x1000, 2)
        self.assertFalse(self.h.snapshots[-1]['complete'])


if __name__ == '__main__':
    unittest.main()
