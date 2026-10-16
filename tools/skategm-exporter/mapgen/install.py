import hashlib
import json
import os
import shutil
from dataclasses import asdict
from pathlib import Path

from . import games
from . import regions as region_list
from .build import build
from .source import GameSource
from .sourcetools import ToolError

BUILD_VERSION = 4
STREAM_WORLDS = ('sgm_skate3_portcarverton', 'sgm_skate2_newsanvan')


# how the installer lists them (an ordered list, not a field on the maps: a
# new Region field would change every map's key and rebuild them all)
SECTIONS = (
    ('Entire Maps', ('sgm_skate3_portcarverton', 'sgm_skate2_newsanvan')),
    ('Map Segments', ('sgm_skate3_university', 'sgm_skate3_unibowl', 'sgm_skate3_ultramegapark',
                      'sgm_skate3_industrial', 'sgm_skate3_quarry')),
    ('Skate Parks', ('sgm_skate3_maloof', 'sgm_skate3_skateschool', 'sgm_skate3_blackbox')),
)


def sections():
    """[(title, [maps])] in SECTIONS order; any map not listed goes under Other Maps"""
    every = {r.map_name: r for r in catalog()}
    out, placed = [], set()
    for title, names in SECTIONS:
        mine = [every[n] for n in names if n in every]
        placed.update(r.map_name for r in mine)
        if mine:
            out.append((title, mine))
    rest = [r for name, r in every.items() if name not in placed]
    if rest:
        out.append(('Other Maps', rest))
    return out


def catalog(game=None):
    from . import stream
    every = list(region_list.REGIONS) + [w for w in stream.WORLDS if w.map_name in STREAM_WORLDS]
    if game is None:
        return every
    return [r for r in every if r.game == games.get(game).id]


def find(name):
    from . import stream
    try:
        return region_list.by_name(name)
    except KeyError:
        return stream.by_name(name)


def is_stream(region):
    from . import stream
    return isinstance(region, stream.World)
ADDON = 'skategm_maps'
ADDON_JSON = {'title': 'SkateGM: Skate maps (built on this PC)', 'type': 'map', 'tags': ['fun'], 'ignore': []}


def region_key(region):
    extra = {}
    if is_stream(region):
        from . import stream
        extra = {'stream': stream.FORMAT, 'stream_build': stream.BUILD}
    fields = asdict(region)
    if fields.get('game') == games.SKATE3.id:
        del fields['game']
    text = json.dumps({'build': BUILD_VERSION, 'region': fields, **extra}, sort_keys=True, default=str)
    return hashlib.sha256(text.encode()).hexdigest()[:16]


def addon_dir(gmod):
    return Path(gmod) / 'garrysmod' / 'addons' / ADDON


def _state_path(gmod):
    return addon_dir(gmod) / 'maps.json'


def read_state(gmod):
    try:
        return json.loads(_state_path(gmod).read_text(encoding='utf-8'))
    except (OSError, ValueError):
        return {}


PARALLEL = 2
FREE_NEEDED = 10 * 2**30 # scratch space: ~4 GB a map while it builds, two at once


def prepare_shared(source, regions, work, report=print, workers=None):
    from . import decode, environment, props, scene
    wanted = set()
    for r in regions:
        wanted |= {d.id for d in r.districts} if is_stream(r) else {r.district}
    for district in sorted(wanted):
        root = decode.decode(source, district, work / 'districts', report, workers)
        scene.load(root, collision=True)
        if any((is_stream(r) and district in {d.id for d in r.districts}) or
               (not is_stream(r) and r.district == district and r.backdrop and len(r.corners)) for r in regions):
            decode.decode_proxy(source, district, work / 'districts', report, workers)
        environment.load(source, district, work / 'env')
        environment.water(source, district, work / 'env')
    if source.game.dmo:
        props.catalog(source, work / 'dmo', report)


def sources_by_game(sources, work):
    if isinstance(sources, dict):
        out = {}
        for gid, path in sources.items():
            if path:
                out[games.get(gid).id] = GameSource(path, Path(work) / 'disc', gid)
        return out
    source = GameSource(sources, Path(work) / 'disc')
    return {source.game.id: source}


def _child(name, game, game_id, gmod, maps, work, workers, queue):
    try:
        region = find(name)
        source = GameSource(game, Path(work) / 'disc', game_id)
        say = lambda m: queue.put(('log', name, str(m)))
        if is_stream(region):
            from . import stream
            final, phases = stream.build(region, source, gmod, Path(maps).parent, work, report=say, workers=workers)
        else:
            final, phases, _ = build(region, source, gmod, maps, work, report=say, workers=workers)
        queue.put(('done', name, str(final), phases))
    except Exception as error:
        queue.put(('fail', name, _failure(error)))


def _failure(error):
    import traceback
    return (str(error) or type(error).__name__) + '\n' + traceback.format_exc()


def _prepare_child(game, game_id, names, work, workers, queue):
    key = f'prepare:{game_id}'
    try:
        source = GameSource(game, Path(work) / 'disc', game_id)
        prepare_shared(source, [find(n) for n in names], Path(work), lambda m: queue.put(('log', key, str(m))), workers)
        queue.put(('done', key, '', {}))
    except Exception as error:
        queue.put(('fail', key, _failure(error)))


def tool_safe_work(work, report=print):
    if str(work.resolve()).isascii():
        return work
    fallback = Path(os.environ.get('ProgramData', 'C:/ProgramData')) / 'SkateGM' / 'mapgen'
    report(f"(building in {fallback}: Source's map tools can't read the letters in {work})")
    return fallback


def install_maps(sources, gmod, work, report=print, names=None, force=False, parallel=PARALLEL):
    import multiprocessing
    import queue as queue_module
    addon = addon_dir(gmod)
    maps = addon / 'maps'
    maps.mkdir(parents=True, exist_ok=True)
    (addon / 'addon.json').write_text(json.dumps(ADDON_JSON, indent='\t'), encoding='utf-8')
    work = tool_safe_work(Path(work), report)
    if not str(Path(gmod).resolve()).isascii():
        raise RuntimeError(f"Garry's Mod is in a folder whose name has letters Source's map tools can't read ({gmod}): "
                           'maps can only be built from a Steam library with a plain (ASCII) path')
    by_game = sources_by_game(sources, work)
    chosen = [find(n) for n in names] if names else catalog()
    skipped = [r for r in chosen if r.game not in by_game]
    for r in skipped:
        report(f'{r.map_name}: skipped (no {games.get(r.game).title} disc given)')
    chosen = [r for r in chosen if r.game in by_game]
    state = read_state(gmod)
    built, failed = [], []
    retire(maps, state, report)
    _state_path(gmod).write_text(json.dumps(state, indent=1), encoding='utf-8')
    todo = []
    for region in chosen:
        target = maps / f'{region.map_name}.bsp'
        if not force and state.get(region.map_name, {}).get('key') == region_key(region) and target.is_file():
            report(f'{region.map_name}: already built')
        else:
            todo.append(region)
    if todo:
        work.mkdir(parents=True, exist_ok=True)
        free = shutil.disk_usage(work).free
        if free < FREE_NEEDED:
            raise RuntimeError(f'building maps needs about {FREE_NEEDED // 2**30} GB free on the drive with {work} '
                               f'(it has {free / 2**30:.1f} GB): free some space and try again')
    queue = multiprocessing.Queue()
    running = {}
    preparing = [gid for gid in by_game if any(r.game == gid for r in todo)]
    ready = set()
    pending = list(todo)
    while preparing or pending or running:
        while len(running) < max(1, parallel):
            if preparing:
                gid = preparing.pop(0)
                source = by_game[gid]
                report(f"Reading {source.game.title}'s districts...")
                proc = multiprocessing.Process(target=_prepare_child, args=(str(source.path), source.game.id,
                                                                            [r.map_name for r in todo if r.game == gid], str(work),
                                                                            max(1, (os.cpu_count() or 4) // max(1, parallel)), queue))
                proc.start()
                running[f'prepare:{gid}'] = proc
                continue
            region = next((r for r in pending if r.game in ready), None)
            if region is None:
                break
            pending.remove(region)
            report(f'Building {region.map_name} ({region.title})...')
            workers = max(1, (os.cpu_count() or 4) // max(1, parallel))
            source = by_game[region.game]
            proc = multiprocessing.Process(target=_child, args=(region.map_name, str(source.path), source.game.id, str(gmod),
                                                                str(maps), str(work), workers, queue))
            proc.start()
            running[region.map_name] = proc
        try:
            message = queue.get(timeout=1)
        except queue_module.Empty:
            for name, proc in list(running.items()):
                if not proc.is_alive():
                    message = ('fail', name, f'the build process stopped unexpectedly (exit code {proc.exitcode})')
                    break
            else:
                continue
        kind, name = message[0], message[1]
        if name not in running:
            continue
        if name.startswith('prepare:'):
            if kind == 'log':
                report(message[2])
                continue
            running.pop(name).join()
            gid = name.split(':', 1)[1]
            if kind == 'fail':
                for r in [r for r in pending if r.game == gid]:
                    pending.remove(r)
                    report(f'{r.map_name} could not be built: {message[2]}')
                    failed.append(r.map_name)
            else:
                ready.add(gid)
            continue
        region = find(name)
        if kind == 'log':
            continue
        running.pop(name).join()
        if kind == 'fail':
            report(f'{region.map_name} could not be built: {message[2]}')
            failed.append(region.map_name)
            continue
        final, phases = Path(message[2]), message[3]
        (Path(gmod) / 'garrysmod' / 'maps' / 'graphs' / f'{region.map_name}.ain').unlink(missing_ok=True)
        state[region.map_name] = {'key': region_key(region), 'title': region.title, 'sha256': _sha256(final),
                                  'seconds': phases}
        _state_path(gmod).write_text(json.dumps(state, indent=1), encoding='utf-8')
        report(f'{region.map_name} built')
        built.append(region.map_name)
    for leftover in ('build', 'districts', 'disc'):
        shutil.rmtree(work / leftover, ignore_errors=True)
    return built, failed


def retire(maps, state, report=print):
    current = {r.map_name for r in catalog()}
    for path in sorted(maps.glob('sgm_*.bsp')):
        if path.stem not in current:
            path.unlink(missing_ok=True)
            (maps / 'thumb' / f'{path.stem}.png').unlink(missing_ok=True)
            (maps / 'graphs' / f'{path.stem}.ain').unlink(missing_ok=True)
            shutil.rmtree(maps.parent / 'skategm_stream' / path.stem, ignore_errors=True)
            shutil.rmtree(maps.parent / 'materials' / 'skategm' / path.stem, ignore_errors=True)
            report(f'{path.stem}: removed (no longer one of the maps)')
    for name in [n for n in state if n not in current]:
        del state[name]


def remove_maps(gmod):
    shutil.rmtree(addon_dir(gmod), ignore_errors=True)


def _sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for block in iter(lambda: f.read(1 << 20), b''):
            h.update(block)
    return h.hexdigest()
