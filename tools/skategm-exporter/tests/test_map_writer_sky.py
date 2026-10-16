import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
import zlib

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tools.asset_pipeline.map_writer import sky_extension


def read_stored(stream, expected):
    method, size = struct.unpack('<II', stream.read(8))
    data = stream.read(size)
    if method == 0:
        result = data
    elif method == 1:
        result = zlib.decompress(data)
    else:
        raise AssertionError(f'unexpected storage method {method}')
    if len(result) != expected:
        raise AssertionError(f'expected {expected} stored bytes, got {len(result)}')
    return result


class RetailSkyExtensionTests(unittest.TestCase):
    def write_fixture(self, root):
        metadata = {
            'width': 2,
            'height': 2,
            'sun_width': 2,
            'sun_height': 1,
            'positions': [[0, 0, 1], [1, 0, 1], [0, 1, 1]],
            'uvs': [[0, 0], [1, 0], [0, 1]],
            'indices': [0, 1, 2],
            'environment': {
                'fog': {
                    'fog_near': [30],
                    'fog_far': [250],
                    'fog_power': [1.5],
                    'fog_max': [0.8],
                    'fog_colour': [0.2, 0.3, 0.4, 0.8],
                },
                'sun_direction': [0, 1, 0],
                'sun_scale': 0.035,
                'multiplier': 1.0,
            },
        }
        path = root / 'University.json'
        path.write_text(json.dumps(metadata), encoding='utf-8')
        path.with_suffix('.rgba').write_bytes(bytes(range(16)))
        path.with_suffix('.sun.rgba').write_bytes(bytes(range(8)))
        return path

    def test_encodes_sky_mesh_textures_and_environment(self):
        with tempfile.TemporaryDirectory() as directory:
            payload = sky_extension(self.write_fixture(Path(directory)))
        self.assertIsNotNone(payload)
        stream = __import__('io').BytesIO(payload)
        self.assertEqual(struct.unpack('<6I', stream.read(24)), (3, 3, 2, 2, 2, 1))
        values = struct.unpack('<13f', stream.read(52))
        for actual, expected in zip(values[:4], (30.0, 250.0, 1.5, 0.8), strict=True):
            self.assertAlmostEqual(actual, expected)
        for actual, expected in zip(values[8:], (0.0, 1.0, 0.0, 0.035, 1.0), strict=True):
            self.assertAlmostEqual(actual, expected)
        self.assertEqual(read_stored(stream, 16), bytes(range(8, 16)) + bytes(range(8)))
        self.assertEqual(read_stored(stream, 8), bytes(range(8)))
        vertices = read_stored(stream, 3 * 5 * 4)
        self.assertEqual(struct.unpack('<15f', vertices)[:5], (0.0, 0.0, 1.0, 0.0, 1.0))
        self.assertEqual(struct.unpack('<3I', read_stored(stream, 12)), (0, 1, 2))
        self.assertEqual(stream.read(), b'')

    def test_rejects_out_of_range_sky_indices(self):
        with tempfile.TemporaryDirectory() as directory:
            path = self.write_fixture(Path(directory))
            metadata = json.loads(path.read_text(encoding='utf-8'))
            metadata['indices'] = [0, 1, 3]
            path.write_text(json.dumps(metadata), encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'index array'):
                sky_extension(path)

    def test_rejects_zero_sun_direction(self):
        with tempfile.TemporaryDirectory() as directory:
            path = self.write_fixture(Path(directory))
            metadata = json.loads(path.read_text(encoding='utf-8'))
            metadata['environment']['sun_direction'] = [0, 0, 0]
            path.write_text(json.dumps(metadata), encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'sun direction'):
                sky_extension(path)


if __name__ == '__main__':
    unittest.main()
