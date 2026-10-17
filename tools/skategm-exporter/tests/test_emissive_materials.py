import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tools.asset_pipeline.map_writer import emissive_strength


class EmissiveMaterialTests(unittest.TestCase):
    def test_incandescent_shaders_emit(self):
        self.assertEqual(emissive_strength("incandescent.default"), 1.0)
        self.assertEqual(emissive_strength("Incandescent.BacklitUVScroll"), 1.0)

    def test_unlit_shader_emits(self):
        self.assertEqual(emissive_strength("unlit"), 1.0)

    def test_lit_and_missing_shaders_do_not_emit(self):
        self.assertEqual(emissive_strength("environment.default"), 0.0)
        self.assertEqual(emissive_strength(None), 0.0)


if __name__ == "__main__":
    unittest.main()
