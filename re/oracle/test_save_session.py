"""Run the original manager behind the native save-session harness."""
import unittest
from pathlib import Path

import save_session


@unittest.skipUnless(Path(save_session.iov._default_exe()).is_file(), "local original executable absent")
class OriginalManagerTests(unittest.TestCase):
    def test_registration_and_loaded_descriptor_lookup_execute_original_code(self):
        world = save_session.SessionWorld(save_session.iov._default_exe(), save_session.Vfs(), {"0": "seed0.rpk"})
        oracle = world.o
        self.assertNotIn(0x538380, oracle.hooks)
        self.assertNotIn(0x5383F0, oracle.hooks)
        indices = []
        for name in ("NEW.rpk", "new.rpk", "other.rpk", "NEW.rpk"):
            address = save_session._alloc_cstr(oracle, name.encode())
            indices.append(oracle.call(0x538380, [address], convention="thiscall", this=world.manager).eax)
        self.assertEqual(indices, [1, 1, 2, 1])
        self.assertEqual(oracle.read_u32(world.manager + 0xFFE20), 3)
        descriptor = oracle.call(0x5383F0, [7], convention="thiscall", this=world.manager).eax
        self.assertEqual(descriptor, world.manager + 0xFFE94)
        self.assertEqual(oracle.read_cstr(descriptor + 0xC), b"seed0.rpk")

    def test_initial_profile_rejects_sparse_and_case_alias_indices(self):
        for kinds in ({"1": "sparse.rpk"}, {"0": "same.rpk", "1": "SAME.rpk"}):
            with self.subTest(kinds=kinds), self.assertRaises(ValueError):
                save_session.SessionWorld(save_session.iov._default_exe(), save_session.Vfs(), kinds)


if __name__ == "__main__":
    unittest.main()
