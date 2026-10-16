import json
import os
import re
import shutil
import struct
import subprocess
from pathlib import Path

VERSION = 1
RATE = 44100
CHUNK = 8192
QUALITY = 0.5
MPF = 'data/audio/music/ipod.mpf'
MUS = 'data/audio/music/Ipod_Stream.mus'
LANGUAGE = 'data/fe/languages/english/LANGUAGE_English_Global_skate3ng.BIN'
LABELS = 'data/fe/languages/labels/LANGUAGE_Labels_Global_skate3ng.BIN'
FILES = [MPF, MUS, 'data/big/miscboot.big', 'data/big/db.big']


def label_hash(text):
    h = 0xFFFFFFFF
    for c in text.encode('latin-1'):
        h = (h * 33 + c) & 0xFFFFFFFF
    return h


def string_table(data):
    count = struct.unpack_from('<I', data, 8)[0]
    base = 0x24 + 8 * count
    out = {}
    for i in range(count):
        h, offset = struct.unpack_from('<II', data, 0x24 + 8 * i)
        at = base + offset
        end = data.index(b'\0', at)
        out[h] = data[at:end].decode('latin-1')
    return out


def clean(text):
    text = re.sub(r'[\x00-\x1f\x7f-\x9f]', '', text or '').replace('\xa0', ' ')
    return re.sub(r'\s+', ' ', text).strip()


def chains(mpf):
    if mpf[:4] != b'PFDx' or mpf[4] != 5:
        raise ValueError('not a version 5 PFDx music map')
    u32 = lambda at: struct.unpack_from('>I', mpf, at)[0]
    first, nodes_end, after = u32(0x14), u32(0x18), u32(0x1c)
    offsets = [o for o in (struct.unpack_from('>H', mpf, at)[0] * 4 for at in range(first, nodes_end - 1, 2)) if o >= nodes_end]
    nodes = []
    for i, at in enumerate(offsets):
        end = offsets[i + 1] if i + 1 < len(offsets) else after
        nodes.append([u32(x) for x in range(at, end, 4)])
    songs = []
    for start, w in enumerate(nodes):
        if w[1] != 0x8101 or w[0] & 0xFFFF != 0x20:
            continue
        streams, seen, i = [], set(), w[-1] & 0xFFFF
        while i < len(nodes) and i not in seen and nodes[i][0] >> 16 != 0xFFFF:
            seen.add(i)
            streams.append(nodes[i][0] >> 16)
            i = nodes[i][-1] & 0xFFFF
        if streams:
            songs.append(streams)
    return songs


def playlist(game_root, work):
    from tools.owned_game.big import BigArchive
    from .vlt import convert as convert_vlt
    db = BigArchive(Path(game_root) / 'data/big/db.big')
    wanted = {'skaterschema.bin', 'skaterschema.vlt', 'skatercollections.bin', 'skatercollections.vlt'}
    out = Path(work) / 'db'
    db.extract_entries([e for e in db.entries if Path(e.path).name.lower() in wanted], out, overwrite=True)
    stem = out / 'data/db'
    rows = convert_vlt(stem / 'skaterschema', stem / 'skatercollections', ['aud_playlists', 'ipod_playlist'])['collections']
    for row in rows:
        if row['class'] == 'aud_playlists' and row['key'] == 'ipod_playlist':
            return [int(item[:8], 16) for item in row['fields']['ipod_playlist']['array']['items']]
    raise ValueError('no ipod_playlist in the game database')


def song_names(game_root):
    from tools.owned_game.big import BigArchive
    boot = BigArchive(Path(game_root) / 'data/big/miscboot.big')
    entries = {e.path.lower(): e for e in boot.entries}
    labels = string_table(boot.read(entries[LABELS.lower()]))
    texts = string_table(boot.read(entries[LANGUAGE.lower()]))
    by_name = {name: texts.get(h, '') for h, name in labels.items()}
    out = {}
    for name in by_name:
        if name.startswith('IPOD_') and name.endswith('_TITLE'):
            stem = name[:-len('_TITLE')]
            out[label_hash(stem)] = (stem, by_name[name], by_name.get(stem + '_ARTIST', ''))
            spaced = stem.replace('-', '_')
            out.setdefault(label_hash(spaced), out[label_hash(stem)])
    return {h: (stem, clean(artist), clean(title)) for h, (stem, title, artist) in out.items()}


def song_id(stem):
    return re.sub(r'[^a-z0-9]+', '_', stem[len('IPOD_'):].lower()).strip('_')


def _decode(vgmstream, mpf, folder, first, last):
    flags = {'creationflags': subprocess.CREATE_NO_WINDOW} if os.name == 'nt' else {}
    done = subprocess.run([str(vgmstream), '-i', '-s', str(first), '-S', str(last), '-o', '?s.wav', str(mpf)], cwd=folder,
                          capture_output=True, text=True, errors='replace', **flags)
    if done.returncode:
        raise RuntimeError(f'vgmstream failed: {done.stdout[-300:]}{done.stderr[-300:]}')


def encode(soundfile, numpy, parts, path):
    path.parent.mkdir(parents=True, exist_ok=True)
    frames = 0
    with soundfile.SoundFile(str(path), 'w', RATE, 2, format='OGG', subtype='VORBIS', compression_level=1 - QUALITY) as out:
        for part in parts:
            x, rate = soundfile.read(str(part), dtype='float32', always_2d=True)
            if rate != RATE:
                raise RuntimeError(f'{part.name}: {rate} Hz')
            if x.shape[1] == 1:
                x = numpy.repeat(x, 2, axis=1)
            x = numpy.ascontiguousarray(x[:, :2])
            for k in range(0, len(x), CHUNK):
                out.write(x[k:k + CHUNK])
            frames += len(x)
    return frames / RATE


def songs(game_root, work):
    game_root = Path(game_root)
    order = chains((game_root / MPF).read_bytes())
    ids = playlist(game_root, work)
    names = song_names(game_root)
    found = [names[h] for h in ids if h in names]
    if len(found) != len(ids) or len(order) != len(ids):
        raise ValueError(f'{len(ids)} songs in the list, {len(found)} named, {len(order)} in the music file')
    found.sort(key=lambda n: n[1].lower())
    return [(song_id(stem), artist, title, streams) for (stem, artist, title), streams in zip(found, order)]


def installed(gmod):
    gmod = Path(gmod)
    try:
        manifest = json.loads((gmod / 'garrysmod' / 'data' / 'skategm' / 'skate3_music.json').read_text(encoding='utf-8'))
    except (OSError, ValueError):
        return False
    folder = gmod / 'garrysmod' / 'addons' / 'skategm_s3music'
    return manifest.get('version') == VERSION and bool(manifest.get('songs')) and all(
        (folder / s['file']).is_file() for s in manifest['songs'])


def build(game_root, gmod, vgmstream, work, report=print):
    import numpy
    import soundfile
    game_root, gmod, work = Path(game_root), Path(gmod), Path(work)
    plan = songs(game_root, work)
    addon = gmod / 'garrysmod' / 'addons' / 'skategm_s3music'
    stage = addon.with_name(addon.name + '.partial')
    shutil.rmtree(stage, ignore_errors=True)
    stage.mkdir(parents=True)
    (stage / 'addon.json').write_text(json.dumps({'title': 'SkateGM - Skate 3 music (from your game)', 'type': 'effects', 'tags': []}),
                                      encoding='utf-8')
    music = work / 'music'
    music.mkdir(parents=True, exist_ok=True)
    mpf = (game_root / MPF).resolve()
    listed = []
    for n, (sid, artist, title, streams) in enumerate(plan, 1):
        report(f'Skate 3 music: {n} of {len(plan)}, {artist} - {title}')
        folder = music / sid
        shutil.rmtree(folder, ignore_errors=True)
        folder.mkdir()
        _decode(vgmstream, mpf, folder, min(streams), max(streams))
        parts = [folder / f'{s}.wav' for s in streams]
        missing = [p.name for p in parts if not p.is_file()]
        if missing:
            raise RuntimeError(f'{sid}: vgmstream did not decode {", ".join(missing[:3])}')
        rel = f'sound/skate3/music/{sid}.ogg'
        seconds = encode(soundfile, numpy, parts, stage / rel)
        shutil.rmtree(folder, ignore_errors=True)
        listed.append({'id': sid, 'file': rel, 'artist': artist, 'title': title, 'seconds': round(seconds, 2)})
    manifest = {'version': VERSION, 'songs': listed}
    shutil.rmtree(addon, ignore_errors=True)
    stage.rename(addon)
    data_dir = gmod / 'garrysmod' / 'data' / 'skategm'
    data_dir.mkdir(parents=True, exist_ok=True)
    (data_dir / 'skate3_music.json').write_text(json.dumps(manifest, ensure_ascii=False), encoding='utf-8')
    report(f'Skate 3 music: {len(listed)} songs')
    return manifest
