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
    write_collision_instances,
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
            instances_payload = b'authored prop collision instances'
            (props / 'University.instances').write_bytes(instances_payload)

            self.assertEqual(prop_manifest_entry(private, 'University'), {
                'path': 'props/University.skate',
                'sha256': hashlib.sha256(payload).hexdigest(),
                'collision_path': 'private/native-props/University.collision',
                'collision_sha256': hashlib.sha256(collision_payload).hexdigest(),
                'instances_path': 'private/native-props/University.instances',
                'instances_sha256': hashlib.sha256(instances_payload).hexdigest(),
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

    def test_collision_instances_store_stable_ids_local_meshes_and_transforms(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'University.instances'
            triangles = np.array([[[0, 0, 0], [2, 0, 0], [0, 3, 0]]], dtype='f4')
            transform = np.eye(4, dtype='f4')
            transform[3, :3] = [10, 20, 30]
            self.assertEqual(write_collision_instances(path, [(7, transform, triangles)]), 1)
            data = path.read_bytes()
            fixture = Path(__file__).parent / 'fixtures' / 'pexcol3-one-instance.bin'
            self.assertEqual(data, fixture.read_bytes())
            self.assertEqual(struct.unpack('<8sI', data[:12]), (b'PEXCOL3\0', 1))
            identity, *record = struct.unpack('<Q16fI', data[12:12 + struct.calcsize('<Q16fI')])
            self.assertEqual(identity, 7)
            self.assertEqual(record[-1], 1)
            np.testing.assert_array_equal(np.frombuffer(data[-36:], dtype='<f4').reshape(1, 3, 3), triangles)

    def test_empty_collision_instance_package_removes_stale_file(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'University.instances'
            path.write_bytes(b'stale')
            self.assertEqual(write_collision_instances(path, []), 0)
            self.assertFalse(path.exists())

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
