"""Passive history hooks observe original allocator/recycler without changing it."""
import sys
import unittest
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).parent))
sys.path.insert(0, str(Path(__file__).parents[1] / "oracle"))
from oracle import DEFAULT_EXE, Oracle
from resource_history import ResourceHistory
from unicorn import UC_HOOK_CODE
from unicorn import x86_const as X


class ObserverAdapter:
    def __init__(self, oracle):
        self.oracle = oracle
        self.host = SimpleNamespace(icount=0)
        self.native_pending = [{"class": "test.Resource", "name": "create"}]
        self.stats = {"native_calls": 12}

    def hook(self, address, callback):
        self.oracle.uc.hook_add(UC_HOOK_CODE, callback, None, address, address)

    def reg(self, name):
        return self.oracle.uc.reg_read(getattr(X, "UC_X86_REG_" + name.upper()))

    def u32(self, address):
        return self.oracle.read_u32(address)


def run(observed, limit=50000):
    o = Oracle()
    o.hook_alloc()
    manager = o.alloc(0xDB0)
    o.write_u32(manager + 0xDAC, 1)

    def realloc(oc, args):
        pointer = oc.alloc(args[1])
        if args[0]:
            oc.write(pointer, oc.read(args[0], oc.heap_sizes[args[0]]))
        return pointer

    o.hook(0x54F580, realloc, "cdecl", 2)
    history = ResourceHistory(ObserverAdapter(o), limit) if observed else None
    allocated = [o.call(0x536ED0, [], convention="thiscall", this=manager).eax for _ in range(2)]
    o.call(0x536F90, [allocated[0]], convention="thiscall", this=manager)
    o.call(0x536F90, [0x1234], convention="thiscall", this=manager)
    allocated.append(o.call(0x536ED0, [], convention="thiscall", this=manager).eax)
    counters = o.read_u32s(manager + 0xDA4, 3)
    free_stack = o.read(o.read_u32(manager + 0xDA0), counters[0] * 2)
    return (allocated, counters, free_stack), history


@unittest.skipUnless(Path(DEFAULT_EXE).exists(), "local original executable absent")
class ResourceHistoryTests(unittest.TestCase):
    def test_original_results_and_free_stack_are_unchanged(self):
        plain, _ = run(False)
        observed, history = run(True)
        self.assertEqual(observed, plain)
        self.assertEqual(observed[0], [0xFFFF0080, 0xFFFF007F, 0xFFFF0080])
        events = history.events
        self.assertEqual([e["operation"] for e in events], ["allocate", "allocate", "recycle", "recycle", "allocate"])
        self.assertEqual([(e["before"], e["after"]) for e in events], [(0, 127), (127, 126), (126, 127), (127, 127), (127, 126)])
        self.assertTrue(all(e["native"] == "test.Resource.create" and e["native_ordinal"] == 13 for e in events))
        self.assertEqual(history.sidecar()["pending"], 0)
        self.assertFalse(history.sidecar()["truncated"])

    def test_record_budget_marks_incomplete_history_without_changing_execution(self):
        plain, _ = run(False)
        observed, history = run(True, limit=1)
        self.assertEqual(observed, plain)
        self.assertEqual(len(history.events), 1)
        self.assertEqual(history.total, 5)
        self.assertTrue(history.sidecar()["truncated"])
        self.assertEqual(history.sidecar()["pending"], 0)


if __name__ == "__main__":
    unittest.main()
