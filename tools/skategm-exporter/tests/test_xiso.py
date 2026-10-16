import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from xiso import DIRECTORY, MAGIC, SECTOR, Xiso, XisoError


def make_entry(name, sector, size, attributes=0):
    encoded = name.encode("latin-1")
    record = struct.pack("<HHIIBB", 0, 0, sector, size, attributes, len(encoded)) + encoded
    return record + bytes((-len(record)) % 4)


class XisoTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.image_path = Path(self.temp.name) / "fixture.iso"

    def tearDown(self):
        self.temp.cleanup()

    def write_image(
        self, root_entries, root_sector=40, root_size=None, file_data=b"skate", image_size=42 * SECTOR
    ):
        root = b"".join(root_entries)
        if root_size is None:
            root_size = len(root)
        image = bytearray(image_size)
        descriptor = MAGIC + struct.pack("<II", root_sector, root_size) + bytes(8)
        image[32 * SECTOR:32 * SECTOR + len(descriptor)] = descriptor
        root_start = root_sector * SECTOR
        if root_start + len(root) <= len(image):
            image[root_start:root_start + len(root)] = root
        image[41 * SECTOR:41 * SECTOR + len(file_data)] = file_data
        self.image_path.write_bytes(image)

    def test_lists_and_extracts_a_file_from_a_valid_xdvdfs_image(self):
        self.write_image([make_entry("payload.bin", 41, 5)])

        with Xiso(self.image_path) as disc:
            self.assertEqual(list(disc.walk()), [("payload.bin", 5)])
            output = Path(self.temp.name) / "out.bin"
            disc.extract("payload.bin", output)

        self.assertEqual(output.read_bytes(), b"skate")

    def test_rejects_path_traversal_names(self):
        self.write_image([make_entry("../outside", 41, 5)])

        with Xiso(self.image_path) as disc:
            with self.assertRaisesRegex(XisoError, "unsafe XDVDFS path name"):
                disc.listdir(disc.root_sector, disc.root_size)

    def test_rejects_reserved_windows_device_names(self):
        self.write_image([make_entry("CON.txt", 41, 5)])

        with Xiso(self.image_path) as disc:
            with self.assertRaisesRegex(XisoError, "unsafe XDVDFS path name"):
                disc.listdir(disc.root_sector, disc.root_size)

    def test_rejects_extents_outside_the_disc_image(self):
        self.write_image([make_entry("payload.bin", 999, 5)])

        with Xiso(self.image_path) as disc:
            with self.assertRaisesRegex(XisoError, "outside the image"):
                disc.listdir(disc.root_sector, disc.root_size)

    def test_rejects_truncated_directory_entries(self):
        self.write_image([struct.pack("<HHIIBB", 0, 0, 41, 5, 0, 8) + b"short"])

        with Xiso(self.image_path) as disc:
            with self.assertRaisesRegex(XisoError, "truncated XDVDFS directory entry"):
                disc.listdir(disc.root_sector, disc.root_size)

    def test_rejects_root_extents_outside_the_image(self):
        self.write_image([], root_sector=999, root_size=1)

        with self.assertRaisesRegex(XisoError, "outside the image"):
            Xiso(self.image_path)

    def test_rejects_unbounded_directory_tables(self):
        self.write_image(
            [], root_size=64 * 1024 * 1024 + 1, image_size=64 * 1024 * 1024 + 42 * SECTOR
        )

        with Xiso(self.image_path) as disc:
            with self.assertRaisesRegex(XisoError, "64 MiB safety limit"):
                disc.listdir(disc.root_sector, disc.root_size)

    def test_rejects_cyclic_directory_trees(self):
        entry = make_entry("self", 40, 20, DIRECTORY)
        self.write_image([entry])

        with Xiso(self.image_path) as disc:
            with self.assertRaisesRegex(XisoError, "cyclic XDVDFS directory tree"):
                list(disc.walk())


if __name__ == "__main__":
    unittest.main()
