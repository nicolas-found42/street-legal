"""Passive nativeplayer observation bounds and resource association."""
import unittest
from player_frame_history import PlayerFrameHistory, GET_INFO, CONTROL, ANIMATE, QUERY

class Observer:
    def __init__(self):
        self.hooks = {}
        self.memory = {}
        self.host = type('Host', (), {'icount':99})()
        self.loop_iterations = 1
        self.stats = {'native_calls':12}
    def hook(self, address, callback): self.hooks[address] = callback
    def reg(self, name): assert name == 'esp'; return 0x1000
    def try_u32(self, address): return self.memory.get(address)

class PlayerFrameTests(unittest.TestCase):
    def test_reads_real_arguments_and_keeps_owner_without_mutation(self):
        o = Observer()
        o.memory.update({0x1000:0x4264fc,0x1004:0x2000,0x1008:8,0x2008:0xffff0069,0x200c:0x3000})
        h = PlayerFrameHistory(o)
        self.assertEqual(set(o.hooks), {QUERY,GET_INFO,CONTROL,ANIMATE})
        h.on_query(None,QUERY,None,None)
        o.memory.update({0x1004:0x4000,0x1008:8})
        for offset in (0x230c,0x21ac,0x250c,0x2424): o.memory[0x4000+offset] = 0
        for index in range(8): o.memory[0x4000+0x2468+index*16] = 0
        before = dict(o.memory)
        h.on_entry(None,GET_INFO,None,None)
        o.memory[0x1008] = 0x80000000
        expected = dict(o.memory)
        h.on_entry(None,ANIMATE,None,None)
        self.assertEqual(o.memory, expected)
        self.assertEqual(h.events[0]['resource']['id'],0xffff0069)
        self.assertEqual(h.events[1]['resource'],h.events[0]['resource'])
        self.assertEqual(h.events[1]['argument'],0x80000000)
        self.assertEqual(h.events[1]['render_refs'],[0]*8)
        self.assertEqual(h.events[1]['frame'],1)
        self.assertFalse(h.sidecar()['alters_execution'])
        self.assertEqual(before[0x1008],8)
    def test_limit_discloses_dropped_events_and_unreadable_fields(self):
        o = Observer(); o.memory[0x1004] = 0x4000
        h = PlayerFrameHistory(o,limit=1)
        h.on_entry(None,CONTROL,None,None)
        h.on_entry(None,CONTROL,None,None)
        self.assertEqual(h.sidecar()['total'],2)
        self.assertTrue(h.sidecar()['truncated'])
        self.assertEqual(len(h.events),1)
        self.assertIsNone(h.events[0]['fields']['0x230c'])
        with self.assertRaises(ValueError): PlayerFrameHistory(o,limit=0)

if __name__ == '__main__': unittest.main()
