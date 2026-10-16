import hashlib
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tools.asset_pipeline.install import prop_manifest_entry


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

            self.assertEqual(prop_manifest_entry(private, 'University'), {
                'path': 'props/University.skate',
                'sha256': hashlib.sha256(payload).hexdigest(),
            })


if __name__ == '__main__':
    unittest.main()
