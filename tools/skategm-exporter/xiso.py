"""Reading files straight out of an Xbox / Xbox 360 disc image (XDVDFS).

    python xiso.py <game.iso> [path inside the disc ...]

Lists the disc, or extracts the given files and folders into the current
folder. Handles plain (extract-xiso rebuilt) images and full disc dumps
(XGD1, XGD2, XGD3).
"""
import os
import struct
import sys

SECTOR = 2048
MAGIC = b"MICROSOFT*XBOX*MEDIA"
PARTITIONS = (0, 0xFD90000, 0x2080000, 0x18300000)
DIRECTORY = 0x10


class XisoError(Exception):
    pass


class Xiso:
    def __init__(self, path):
        self.path = path
        self.f = open(path, "rb")
        for base in PARTITIONS:
            self.f.seek(base + 32 * SECTOR)
            head = self.f.read(28)
            if head[:20] == MAGIC:
                self.base = base
                self.root_sector, self.root_size = struct.unpack_from("<II", head, 20)
                break
        else:
            raise XisoError("this isn't an Xbox 360 disc image (no XDVDFS volume found)")

    def close(self):
        self.f.close()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

    def _read(self, sector, offset, size):
        self.f.seek(self.base + sector * SECTOR + offset)
        return self.f.read(size)

    def listdir(self, sector, size):
        """{lower-case name: (name, sector, size, is_dir)} of one directory"""
        if size == 0:
            return {}
        table = self._read(sector, 0, size)
        out = {}
        seen = set()
        stack = [0]
        while stack:
            at = stack.pop()
            if at in seen or at + 14 > len(table):
                continue
            seen.add(at)
            left, right, start, length, attrs, nlen = struct.unpack_from("<HHIIBB", table, at)
            if left == 0xFFFF and right == 0xFFFF and start == 0xFFFFFFFF:
                continue
            name = table[at + 14:at + 14 + nlen].decode("latin-1")
            out[name.lower()] = (name, start, length, bool(attrs & DIRECTORY))
            if left and left != 0xFFFF:
                stack.append(left * 4)
            if right and right != 0xFFFF:
                stack.append(right * 4)
        return out

    def find(self, path):
        """(path as spelled on the disc, sector, size, is_dir), or None"""
        sector, size, is_dir = self.root_sector, self.root_size, True
        names = []
        for part in [p for p in path.replace("\\", "/").split("/") if p]:
            if not is_dir:
                return None
            entry = self.listdir(sector, size).get(part.lower())
            if entry is None:
                return None
            name, sector, size, is_dir = entry
            names.append(name)
        return "/".join(names), sector, size, is_dir

    def walk(self, path=""):
        """(path, size) of every file under path, spelled as on the disc"""
        entry = self.find(path)
        if entry is None:
            return
        path, sector, size, is_dir = entry
        if not is_dir:
            yield path, size
            return
        for _, (name, s, n, d) in sorted(self.listdir(sector, size).items()):
            child = (path.rstrip("/") + "/" + name) if path else name
            if d:
                yield from self.walk(child)
            else:
                yield child, n

    def extract(self, path, destination, progress=None):
        entry = self.find(path)
        if entry is None or entry[3]:
            raise XisoError("not on the disc: " + path)
        _, sector, size, _ = entry
        os.makedirs(os.path.dirname(destination) or ".", exist_ok=True)
        self.f.seek(self.base + sector * SECTOR)
        left = size
        with open(destination, "wb") as out:
            while left:
                chunk = self.f.read(min(left, 8 << 20))
                if not chunk:
                    raise XisoError("the disc image ends early (incomplete download?): " + path)
                out.write(chunk)
                left -= len(chunk)
                if progress:
                    progress(len(chunk))


def extract_files(iso, paths, destination, report=None):
    """Copy files and folders (paths inside the disc) from the image into
    destination, keeping their paths. Returns the files copied."""
    with Xiso(iso) as disc:
        files = []
        for p in paths:
            found = list(disc.walk(p))
            if not found:
                raise XisoError("not on the disc: " + p)
            files += found
        total = sum(n for _, n in files) or 1
        done = [0, -1]

        def progress(n):
            done[0] += n
            pct = done[0] * 100 // total
            if report and pct // 10 != done[1]:
                done[1] = pct // 10
                report("Reading the disc image: %d%%" % pct)

        for p, _ in files:
            disc.extract(p, os.path.join(destination, *p.split("/")), progress)
        return [p for p, _ in files]


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    if len(sys.argv) == 2:
        with Xiso(sys.argv[1]) as disc:
            for p, n in disc.walk():
                print("%12d  %s" % (n, p))
        return
    for p in extract_files(sys.argv[1], sys.argv[2:], ".", print):
        print(p)


if __name__ == "__main__":
    main()
