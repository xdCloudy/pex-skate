import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tools.asset_pipeline.character_glb import ABIN, add_reparented_bones


class CharacterGlbBoneMappingTests(unittest.TestCase):
    def test_render_pose_joints_replace_unused_finger_slots_with_matching_limbs(self):
        expected = [
            "RIGHTTOEBASE_REPARENTED",
            "LEFTTOEBASE_REPARENTED",
            "RIGHTHAND_REPARENTED",
            "LEFTHAND_REPARENTED",
        ]
        worlds = {index: object() for index in range(32)}
        original_low_joints = dict(worlds)
        reference = {index: object() for index in range(len(ABIN.BONE_NAMES))}
        names = list(ABIN.BONE_NAMES)

        add_reparented_bones(worlds, names, reference)

        self.assertEqual(names[32:36], expected)
        self.assertEqual([worlds[i] for i in range(32, 36)],
                         [reference[i] for i in (20, 24, 12, 16)])
        self.assertTrue(all(worlds[index] is value for index, value in original_low_joints.items()))


if __name__ == "__main__":
    unittest.main()
