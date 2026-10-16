import hashlib
from pathlib import Path
import sys
import struct
import tempfile
import unittest
import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tools.asset_pipeline.install import prop_manifest_entry
from tools.asset_pipeline.dynamic_props import (
    collision_bounds,
    collision_triangles,
    write_collision_boxes,
    write_collision_triangles,
)


class PropCatalogTests(unittest.TestCase):
    def test_missing_optional_prop_package_has_no_catalog_entry(self):
        with tempfile.TemporaryDirectory() as directory:
            self.assertIsNone(prop_manifest_entry(Path(directory), 'University'))

    def test_prop_catalog_entry_names_the_client_path_and_hashes_package(self):
        with tempfile.TemporaryDirectory() as directory:
            private = Path(directory)
            props = private / 'native-props'
            props.mkdir()
            payload = b'portable authored district props'
            (props / 'University.skate').write_bytes(payload)
            collision_payload = b'authored prop collision boxes'
            (props / 'University.collision').write_bytes(collision_payload)

            self.assertEqual(prop_manifest_entry(private, 'University'), {
                'path': 'props/University.skate',
                'sha256': hashlib.sha256(payload).hexdigest(),
                'collision_path': 'private/native-props/University.collision',
                'collision_sha256': hashlib.sha256(collision_payload).hexdigest(),
            })

    def test_world_mesh_bounds_are_exported_as_collision_bounds(self):
        meshes = [{'index': 0}, {'index': 1}]
        arrays = {
            'vertices_0': np.array([[0, 1, 2], [1, 2, 3]], dtype='f4'),
            'vertices_1': np.array([[-1, 0, 1], [2, 3, 4]], dtype='f4'),
        }
        self.assertEqual(collision_bounds(arrays, meshes), [[-1.0, 0.0, 1.0], [2.0, 3.0, 4.0]])

    def test_opaque_prop_collision_uses_transformed_mesh_triangles(self):
        meshes = [{'index': 0, 'alpha_mode': 0}]
        arrays = {
            'vertices_0': np.array([[1, 2, 3], [4, 2, 3], [1, 5, 3]], dtype='f4'),
            'faces_0': np.array([[0, 1, 2]], dtype='u4'),
        }
        np.testing.assert_array_equal(
            collision_triangles(arrays, meshes),
            np.array([[[1, 2, 3], [4, 2, 3], [1, 5, 3]]], dtype='f4'),
        )

    def test_cutout_and_degenerate_prop_faces_are_not_solid_collision(self):
        arrays = {
            'vertices_0': np.array([[0, 0, 0], [1, 0, 0], [2, 0, 0]], dtype='f4'),
            'faces_0': np.array([[0, 1, 2]], dtype='u4'),
            'vertices_1': np.array([[0, 0, 0], [1, 0, 0], [0, 1, 0]], dtype='f4'),
            'faces_1': np.array([[0, 1, 2]], dtype='u4'),
        }
        self.assertEqual(len(collision_triangles(arrays, [{'index': 0, 'alpha_mode': 0}])), 0)
        self.assertEqual(len(collision_triangles(arrays, [{'index': 1, 'alpha_mode': 1}])), 0)

    def test_collision_mesh_package_has_bounded_triangle_records(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'University.collision'
            triangles = [np.array([[[1, 2, 3], [4, 2, 3], [1, 5, 3]]], dtype='f4')]
            self.assertEqual(write_collision_triangles(path, triangles), 1)
            data = path.read_bytes()
            self.assertEqual(struct.unpack('<8sI', data[:12]), (b'PEXCOL2\0', 1))
            self.assertEqual(struct.unpack('<9f', data[12:]), (1.0, 2.0, 3.0, 4.0, 2.0, 3.0, 1.0, 5.0, 3.0))

    def test_collision_package_has_bounded_binary_bounds_records(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'University.collision'
            instances = [
                {'collision_bounds': [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]},
                {'model_asset': 'decoration-without-collision'},
            ]
            write_collision_boxes(path, instances)
            data = path.read_bytes()
            self.assertEqual(struct.unpack('<8sI', data[:12]), (b'PEXCOL1\0', 1))
            self.assertEqual(struct.unpack('<6f', data[12:]), (1.0, 2.0, 3.0, 4.0, 5.0, 6.0))


if __name__ == '__main__':
    unittest.main()
