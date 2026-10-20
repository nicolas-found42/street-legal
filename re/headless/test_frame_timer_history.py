"""Observe original timer reset and final frame sample without changing either."""
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).parent))
sys.path.insert(0, str(Path(__file__).parents[1] / "oracle"))
from oracle import DEFAULT_EXE, Oracle, STACK_TOP
from test_resource_history import ObserverAdapter
from frame_timer_history import FrameTimerHistory


def run(observed):
    o = Oracle()
    counter = [500]
    def qpf(oc, args):
        oc.write_u32s(args[0], [10000, 0])
        return 1
    def qpc(oc, args):
        counter[0] += 500
        oc.write_u32s(args[0], [counter[0], 0])
        return 1
    o.hook(o.read_u32(0x5E7050), qpf, 'stdcall', 1)
    o.hook(o.read_u32(0x5E7048), qpc, 'stdcall', 1)
    adapter = ObserverAdapter(o)
    adapter.host = SimpleNamespace(icount=0, thread_id=1)
    history = FrameTimerHistory(adapter) if observed else None
    o.call(0x552310, [7], convention='cdecl')
    # Execute the unchanged final caller fragment and its FST m32, ending at
    # the next instruction. This avoids fabricating the captured return address.
    o.set_reg('esp', STACK_TOP - 0x100)
    o.uc.emu_start(0x428EC9, 0x428ED6)
    assert o._err is None
    result = (o.read_f32(0x63C534), o.read(0x76A298, 8), o.read(0x76A3D8, 8))
    # An ordinary invocation of the same timer has a different caller and must
    # not become a final-frame input row.
    o.call(0x552330, [7], convention='cdecl')
    return result, history


@unittest.skipUnless(Path(DEFAULT_EXE).exists(), "local original executable absent")
class FrameTimerHistoryTests(unittest.TestCase):
    def test_raw_inputs_and_real_final_caller_are_passive(self):
        plain, _ = run(False)
        observed, history = run(True)
        self.assertEqual(observed, plain)
        self.assertAlmostEqual(observed[0], .05, places=7)
        self.assertEqual(len(history.rows), 1)
        row = history.rows[0]
        self.assertEqual((row['frame'], row['counter'], row['start'], row['frequency']),
                         (1, 1500, 1000, 10000))
        self.assertEqual(row['caller'], 0x428ED0)
        self.assertNotIn('return_st0_le', row)
        self.assertEqual(history.sidecar()['pending'], 0)
        self.assertEqual(history.sidecar()['errors'], [])
        self.assertEqual(history.callers['0x00428ed0'], 1)
        self.assertEqual(history.callers['0x7fff0000'], 1)


if __name__ == '__main__':
    unittest.main()
