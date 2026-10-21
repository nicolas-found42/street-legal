"""Passive resource lifetime bounds and read-only queue projection."""
import unittest
from resource_lifetime_history import ResourceLifetimeHistory

class Observer:
    def __init__(self):
        self.memory = {}
        self.hooks = {}
        self.host = type('Host', (), {'icount':100})()
        self.loop_iterations = 0
        self.stats = {'native_calls':7}
    def hook(self, address, callback): self.hooks[address] = callback
    def reg(self, name): return {'ecx':0x2000, 'esp':0x1000}[name]
    def try_u32(self, address): return self.memory.get(address)

class LifetimeTests(unittest.TestCase):
    def test_records_fifo_queue_references_without_mutation(self):
        o = Observer()
        o.memory.update({0x2078:0x3000,0x3004:0x4000,0x3014:0xffff0041,0x3018:0x5000,0x4004:0})
        before = dict(o.memory)
        h = ResourceLifetimeHistory(o,start=0,end=200)
        h.life(None,0x4277f0,None,None)
        self.assertEqual(o.memory,before)
        queue=h.sidecar()['events'][0]['queues']['0x78']
        self.assertEqual(len(queue),1)
        self.assertEqual(queue[0]['words'][5:7],[0xffff0041,0x5000])
        self.assertFalse(h.sidecar()['alters_execution'])

    def test_bounds_and_capacity_are_explicit(self):
        o=Observer();h=ResourceLifetimeHistory(o,start=99,end=101,limit=1)
        h.life(None,0x536f90,None,None);h.life(None,0x536f90,None,None)
        self.assertEqual(h.sidecar()['total'],2)
        self.assertTrue(h.sidecar()['truncated'])
        o.host.icount=102;h.life(None,0x536f90,None,None)
        self.assertEqual(h.sidecar()['total'],2)
        with self.assertRaises(ValueError): ResourceLifetimeHistory(o,limit=0)

if __name__ == '__main__': unittest.main()
