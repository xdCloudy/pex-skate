import os
import struct
from pathlib import Path

AINET_VERSION = 37


def pack(tools, bsp, files, out):
    listing = Path(bsp).with_suffix('.pack.txt')
    with open(listing, 'w', encoding='utf-8', newline='\n') as f:
        for internal, external in sorted(files.items()):
            f.write(internal.replace('\\', '/') + '\n')
            f.write(str(Path(external).resolve()) + '\n')
    out = Path(out)
    partial = out.with_name(out.name + '.partial')
    partial.unlink(missing_ok=True)
    tools.run('bspzip', ['-game', tools.game, '-addorupdatelist', bsp, listing, partial])
    if not partial.is_file():
        raise RuntimeError('bspzip did not write ' + str(out))
    os.replace(partial, out)
    node_graph(out)
    return out


def node_graph(bsp):
    bsp = Path(bsp)
    with open(bsp, 'rb') as f:
        head = f.read(8 + 64 * 16 + 4)
    revision = struct.unpack_from('<i', head, 8 + 64 * 16)[0]
    graph = bsp.parent / 'graphs' / (bsp.stem + '.ain')
    graph.parent.mkdir(parents=True, exist_ok=True)
    graph.write_bytes(struct.pack('<iiii', AINET_VERSION, revision, 0, 0))
    return graph


def extract(tools, bsp, out):
    Path(out).mkdir(parents=True, exist_ok=True)
    tools.run('bspzip', ['-game', tools.game, '-extractfiles', bsp, out])


def game_files(game, folders):
    game = Path(game)
    files = {}
    for folder in folders:
        root = game / folder
        if not root.is_dir():
            continue
        for path in root.rglob('*'):
            if path.is_file() and path.suffix.lower() in ('.vtf', '.vmt', '.mdl', '.vvd', '.vtx', '.phy'):
                files[path.relative_to(game).as_posix()] = path
    return files
