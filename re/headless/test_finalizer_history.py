"""Observe actual original GC sweep/enqueue over a declared completed mark set."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
sys.path.insert(0, str(Path(__file__).parents[1] / "oracle"))
from oracle import DEFAULT_EXE, Oracle
from test_resource_history import ObserverAdapter
from finalizer_history import FinalizerHistory


class Adapter(ObserverAdapter):
    loop_iterations = 7

    def class_name(self, word):
        return "test.Finalizable"


def run(observed):
    o = Oracle()
    o.hook_alloc()
    jvm, cls = o.alloc(0x100), o.alloc(0x100)
    o.write_u32(cls + 0xC, jvm)

    def chain(words):
        root = o.alloc(0x20)
        all_words = [root + 4, *words, root + 0x10]
        for previous, following in zip(all_words, all_words[1:]):
            o.write_u32(previous + 4, following)
            o.write_u32(following + 8, previous)
        return root

    objects = [o.alloc(0x20) for _ in range(3)]
    for word, flags in zip(objects, [0, 1, 0]):
        o.write_u32(word + 0xC, cls)
        o.write_u32(word + 0x18, flags)
    normal, queue = chain(objects), chain([])
    o.write_u32(jvm + 0x10, normal)
    o.write_u32(jvm + 0x44, queue)
    o.write_u32(jvm + 0x34, 3)
    o.write_u32(jvm + 0x38, 1000)
    context = o.alloc(12)
    o.write_u32s(context, [64, o.alloc(256), 0])
    o.write_u32(jvm + 0x20, context)
    history = FinalizerHistory(Adapter(o)) if observed else None
    # The declared mark worklist is empty; object two is already marked.
    result = o.call(0x417F80, [], convention="thiscall", this=jvm).eax
    fields = [(o.read_u32(word + 0x18), o.read_u32(word + 4), o.read_u32(word + 8))
              for word in objects]
    return (result, fields, o.read(normal, 0x20), o.read(queue, 0x20),
            o.read_u32(jvm + 0x20), o.read_u32(jvm + 0x34)), history, objects


@unittest.skipUnless(Path(DEFAULT_EXE).exists(), "local original executable absent")
class FinalizerHistoryTests(unittest.TestCase):
    def test_sweep_enqueues_unmarked_in_order_clears_live_mark_and_is_passive(self):
        plain, _, _ = run(False)
        observed, history, objects = run(True)
        self.assertEqual(observed, plain)
        self.assertEqual(observed[0], 2)
        self.assertEqual([fields[0] for fields in observed[1]], [8, 0, 8])
        self.assertEqual(observed[4], 0)
        self.assertEqual(observed[5], (3 - 1000) & 0xFFFFFFFF)
        enqueues = [event for event in history.events if event["operation"] == "enqueue"]
        self.assertEqual([event["word"] for event in enqueues], [objects[0], objects[2]])
        self.assertFalse(history.sidecar()["alters_execution"])


if __name__ == "__main__":
    unittest.main()
