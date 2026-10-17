import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from xiso import DIRECTORY, MAGIC, SECTOR, Xiso, XisoError, extract_files, extract_matching
from convert import extract_optional_disc_files


def make_entry(name, sector, size, attributes=0):
    encoded = name.encode("latin-1")
    record = struct.pack("<HHIIBB", 0, 0, sector, size, attributes, len(encoded)) + encoded
    return record + bytes((-len(record)) % 4)


def make_directory(entries):
    """Build a directory's linked entry tree from (name, sector, size, attrs)."""
    offsets = []
    cursor = 0
    for name, *_ in entries:
        offsets.append(cursor)
        cursor += (14 + len(name.encode("latin-1")) + 3) & ~3
    records = []
    for index, (name, sector, size, attributes) in enumerate(entries):
        right = offsets[index + 1] // 4 if index + 1 < len(entries) else 0
        encoded = name.encode("latin-1")
        record = struct.pack("<HHIIBB", 0, right, sector, size, attributes, len(encoded)) + encoded
        records.append(record + bytes((-len(record)) % 4))
    return b"".join(records)


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

    def write_skate_data_tree(self):
        image = bytearray(64 * SECTOR)
        root_sector = 40
        big = make_directory([
                ("miscload.big", 50, 3, 0),
                ("miscboot.big", 51, 3, 0),
                ("db.big", 52, 3, 0),
            ])
        anim = make_directory([("skater.abk", 53, 4, 0)])
        content = make_directory([
                ("createacharacter.big", 54, 3, 0),
                ("worldDIST_University.big", 55, 4, 0),
                ("worldDIST_Industrial.big", 56, 4, 0),
                ("worlddmo.big", 57, 3, 0),
                ("unrelated.bin", 58, 3, 0),
            ])
        directories = {
            41: make_directory([
                ("big", 42, len(big), DIRECTORY),
                ("anim", 43, len(anim), DIRECTORY),
                ("content", 44, len(content), DIRECTORY),
            ]),
            42: big,
            43: anim,
            44: content,
        }
        root = make_directory([("data", 41, len(directories[41]), DIRECTORY)])
        descriptor = MAGIC + struct.pack("<II", root_sector, len(root)) + bytes(8)
        image[32 * SECTOR:32 * SECTOR + len(descriptor)] = descriptor
        image[root_sector * SECTOR:root_sector * SECTOR + len(root)] = root
        for sector, directory in directories.items():
            start = sector * SECTOR
            image[start:start + len(directory)] = directory
        for sector, content in {
            50: b"load", 51: b"boot", 52: b"db!", 53: b"anim",
            54: b"cac", 55: b"univ", 56: b"indu", 57: b"dmo", 58: b"no!",
        }.items():
            start = sector * SECTOR
            image[start:start + len(content)] = content
        self.image_path.write_bytes(image)

    def test_lists_and_extracts_a_file_from_a_valid_xdvdfs_image(self):
        self.write_image([make_entry("payload.bin", 41, 5)])

        with Xiso(self.image_path) as disc:
            self.assertEqual(list(disc.walk()), [("payload.bin", 5)])
            output = Path(self.temp.name) / "out.bin"
            disc.extract("payload.bin", output)

        self.assertEqual(output.read_bytes(), b"skate")

    def test_extract_matching_selects_case_insensitive_direct_children(self):
        self.write_image([
            make_entry("worldDIST_University.big", 41, 5),
            make_entry("readme.txt", 41, 5),
        ])
        output = Path(self.temp.name) / "out"
        files = extract_matching(self.image_path, "", "worlddist_*.BIG", output)
        self.assertEqual(files, ["worldDIST_University.big"])
        self.assertEqual((output / "worldDIST_University.big").read_bytes(), b"skate")

    def test_extract_matching_reports_missing_patterns(self):
        self.write_image([make_entry("readme.txt", 41, 5)])
        with self.assertRaisesRegex(XisoError, "no files matching"):
            extract_matching(self.image_path, "", "worldDIST_*.big", self.temp.name)

    def test_disc_conversion_inputs_extract_only_required_paths_and_world_archives(self):
        self.write_skate_data_tree()
        output = Path(self.temp.name) / "disc"
        files = extract_files(
            self.image_path,
            ["data/anim", "data/big/miscload.big", "data/big/miscboot.big", "data/big/db.big", "data/content/createacharacter.big"],
            output,
        )
        maps = extract_matching(self.image_path, "data/content", "worldDIST_*.big", output)
        optional = extract_optional_disc_files(
            self.image_path, ["data/content/worlddmo.big"], output
        )

        self.assertEqual(
            files,
            [
                "data/anim/skater.abk",
                "data/big/miscload.big",
                "data/big/miscboot.big",
                "data/big/db.big",
                "data/content/createacharacter.big",
            ],
        )
        self.assertEqual(
            maps,
            ["data/content/worldDIST_Industrial.big", "data/content/worldDIST_University.big"],
        )
        self.assertEqual((output / "data/anim/skater.abk").read_bytes(), b"anim")
        self.assertEqual((output / "data/content/worldDIST_University.big").read_bytes(), b"univ")
        self.assertEqual(optional, ["data/content/worlddmo.big"])
        self.assertEqual((output / "data/content/worlddmo.big").read_bytes(), b"dmo")
        self.assertFalse((output / "data/content/unrelated.bin").exists())

    def test_optional_disc_file_does_not_fail_when_absent(self):
        self.write_image([make_entry("readme.txt", 41, 5)])
        output = Path(self.temp.name) / "optional"

        self.assertEqual(
            extract_optional_disc_files(
                self.image_path, ["data/content/worlddmo.big"], output
            ),
            [],
        )

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
