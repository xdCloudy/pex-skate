import struct
import sys
import unittest
from pathlib import Path


EXPORTER_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(EXPORTER_ROOT))
from tools.asset_pipeline.map_writer import encode_world_records


class WorldRecordTests(unittest.TestCase):
    def test_authored_light_and_route_are_encoded_in_skate14_order(self):
        light = {
            "name": "district lamp",
            "kind": 1,
            "position": [1, 2, 3],
            "direction": [0, -1, 0],
            "color": [0.8, 0.7, 0.6],
            "intensity": 4,
            "range": 12,
            "radius": 0.2,
            "inner_cos": 0.9,
            "outer_cos": 0.6,
        }
        route = {
            "name": "plaza loop",
            "closed": True,
            "skaters": 2,
            "speed": 3.5,
            "spacing": 6,
            "points": [[0, 1, 2], [3, 4, 5]],
        }
        light_count, route_count, payload = encode_world_records([light], [route])

        self.assertEqual((light_count, route_count), (1, 1))
        offset = 0
        light_name_length = struct.unpack_from("<I", payload, offset)[0]
        offset += 4
        self.assertEqual(payload[offset:offset + light_name_length], b"district lamp")
        offset += light_name_length
        self.assertEqual(struct.unpack_from("<I", payload, offset)[0], 1)
        offset += 4
        actual_light = struct.unpack_from("<14f", payload, offset)
        expected_light = (1, 2, 3, 0, -1, 0, 0.8, 0.7, 0.6, 4, 12, 0.2, 0.9, 0.6)
        for actual, expected in zip(actual_light, expected_light):
            self.assertAlmostEqual(actual, expected)
        offset += 14 * 4
        route_name_length = struct.unpack_from("<I", payload, offset)[0]
        offset += 4
        self.assertEqual(payload[offset:offset + route_name_length], b"plaza loop")
        offset += route_name_length
        self.assertEqual(struct.unpack_from("<IIffI", payload, offset), (1, 2, 3.5, 6, 2))
        offset += struct.calcsize("<IIffI")
        self.assertEqual(struct.unpack_from("<6f", payload, offset), (0, 1, 2, 3, 4, 5))
        self.assertEqual(offset + 6 * 4, len(payload))

    def test_invalid_light_and_route_records_are_rejected(self):
        invalid_light = {"name": "bad", "kind": 3}
        invalid_route = {"name": "bad", "points": [[0, 0, 0], [float("nan"), 0, 0]]}
        with self.assertRaisesRegex(ValueError, "light identity"):
            encode_world_records([invalid_light], [])
        with self.assertRaisesRegex(ValueError, "Non-finite authored point"):
            encode_world_records([], [invalid_route])


if __name__ == "__main__":
    unittest.main()
