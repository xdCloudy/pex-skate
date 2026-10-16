import struct
import sys
from pathlib import Path

from . import EXPORTER


def _imports():
    if str(EXPORTER) not in sys.path:
        sys.path.insert(0, str(EXPORTER))


def _big4_class():
    _imports()
    from tools.owned_game.big import BigArchive, BigEntry
    from tools.owned_game.binary import FormatError
    from tools.owned_game.refpack import decompress as decompress_refpack

    class Big4Archive(BigArchive):
        def __init__(self, path):
            self.path = Path(path).resolve()
            self.file_size = self.path.stat().st_size
            with self.path.open('rb') as stream:
                head = stream.read(16)
                if len(head) != 16 or head[:4] not in (b'BIG4', b'BIGF'):
                    raise FormatError(f'{self.path}: not a BIG4 archive')
                count, header_size = struct.unpack('>II', head[8:16])
                if header_size > self.file_size or count > 1_000_000:
                    raise FormatError(f'{self.path}: damaged BIG4 header')
                self.metadata = head + stream.read(header_size - 16)
            self.entries = self._parse_big4(count)

        def _parse_big4(self, count):
            data, at, out = self.metadata, 16, []
            for index in range(count):
                if at + 8 > len(data):
                    raise FormatError(f'{self.path}: BIG4 index cut short')
                offset, size = struct.unpack_from('>II', data, at)
                end = data.find(b'\0', at + 8)
                if end < 0:
                    raise FormatError(f'{self.path}: BIG4 name cut short')
                name = data[at + 8:end].decode('latin-1').replace('\\', '/')
                if offset + size > self.file_size:
                    raise FormatError(f'{name}: payload exceeds BIG4 archive')
                out.append(BigEntry(index=index, path=name, offset=offset, stored_size=size, unpacked_size=size, compression=0))
                at = end + 1
            return out

        def read(self, entry):
            with self.path.open('rb') as stream:
                stream.seek(entry.offset)
                data = stream.read(entry.stored_size)
            if len(data) != entry.stored_size:
                raise FormatError(f'{entry.path}: truncated BIG4 payload')
            if len(data) >= 5 and data[1] == 0xFB and data[0] & 0x3E == 0x10:
                size = int.from_bytes(data[2:5], 'big')
                return decompress_refpack(data, size)
            return data

    return Big4Archive


def open_archive(path):
    with open(path, 'rb') as f:
        magic = f.read(4)
    if magic in (b'BIG4', b'BIGF'):
        return _big4_class()(path)
    _imports()
    from tools.owned_game.big import BigArchive
    return BigArchive(path)
