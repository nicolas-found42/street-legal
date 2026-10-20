"""Package history observes real name registration, including absent files."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
sys.path.insert(0, str(Path(__file__).parents[1] / "oracle"))
from oracle import DEFAULT_EXE, Oracle
from package_history import PackageHistory
from test_resource_history import ObserverAdapter


class Adapter(ObserverAdapter):
    def cstr(self, address, limit=256):
        return self.oracle.read_cstr(address, limit).decode("latin1")


def run(observed, limit=50000):
    o = Oracle()
    o.hook_alloc()
    manager = o.alloc(0x100100)
    history = PackageHistory(Adapter(o), limit) if observed else None
    indices = []
    for name in ["system.rpk", "frontend.rpk", "missing-observer-test.rpk", "system.rpk"]:
        pointer = o.alloc(len(name) + 1)
        o.write(pointer, name.encode() + b"\0")
        indices.append(o.call(0x538380, [pointer], convention="thiscall", this=manager).eax)
    return (indices, o.read_u32(manager + 0xFFE20), o.read(manager + 0xFFE94, 3 * 0x70)), history


@unittest.skipUnless(Path(DEFAULT_EXE).exists(), "local original executable absent")
class PackageHistoryTests(unittest.TestCase):
    def test_header_observer_preserves_already_loaded_package_fast_path(self):
        def run_headers(observed):
            o = Oracle()
            package = o.alloc(0x70)
            o.write_u32(package + 8, 12)
            o.write(package + 0xC, b"declared-loaded.rpk\0")
            o.write_u32(package + 0x50, 2)
            history = PackageHistory(Adapter(o), limit=1) if observed else None
            results = [o.call(0x544170, [], convention="thiscall", this=package).eax
                       for _ in range(2)]
            return (results, o.read(package, 0x70)), history
        plain, _ = run_headers(False)
        observed, history = run_headers(True)
        self.assertEqual(observed, plain)
        self.assertEqual(observed[0], [2, 2])
        self.assertEqual(history.header_total, 2)
        self.assertEqual(len(history.header_loads), 1)
        self.assertEqual(history.header_loads[0]["name"], "declared-loaded.rpk")
        self.assertEqual(history.header_loads[0]["index"], 12)
        self.assertEqual(history.header_loads[0]["state"], 2)
        self.assertTrue(history.sidecar()["truncated"])

    def test_original_indices_count_and_package_bytes_unchanged(self):
        plain, _ = run(False)
        observed, history = run(True)
        self.assertEqual(observed, plain)
        self.assertEqual(observed[:2], ([0, 1, 2, 0], 3))
        self.assertEqual([(e["before"], e["after"]) for e in history.events],
                         [(0, 1), (1, 2), (2, 3), (3, 3)])
        self.assertEqual([e["index"] for e in history.creations], [0, 1, 2])
        self.assertEqual(history.creations[2]["name"], "missing-observer-test.rpk")
        self.assertEqual(history.sidecar()["pending"], 0)
        self.assertFalse(history.sidecar()["truncated"])

    def test_truncation_is_explicit_and_preserves_original(self):
        plain, _ = run(False)
        observed, history = run(True, limit=1)
        self.assertEqual(observed, plain)
        self.assertEqual(history.total, 4)
        self.assertEqual(history.creation_total, 3)
        self.assertEqual(len(history.events), 1)
        self.assertEqual(len(history.creations), 1)
        self.assertTrue(history.sidecar()["truncated"])
        self.assertEqual(history.sidecar()["pending"], 0)


if __name__ == "__main__":
    unittest.main()
