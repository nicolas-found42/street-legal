"""Data-free coverage of the oracle producer's aligned input readback."""
from tests.support import *
import importlib.util

class RayBoxProbeTests(unittest.TestCase):
    def test_every_word_of_separately_aligned_input_arrays_is_checked(self):
        spec=importlib.util.spec_from_file_location('ray_box_probe',ROOT/'re/oracle/native_ray_box_probe.py')
        probe=importlib.util.module_from_spec(spec)
        spec.loader.exec_module(probe)
        class Reader:
            memory=bytearray(range(32))
            def read(self,address,size):
                return bytes(self.memory[address:address+size])
        reader=Reader()
        before=probe.read_inputs(reader,0,16)
        self.assertEqual(len(before),24)
        for address in [0,4,8,16,20,24]:
            with self.subTest(word_address=address):
                reader.memory[address]^=1
                self.assertNotEqual(probe.read_inputs(reader,0,16),before)
                reader.memory[address]^=1
