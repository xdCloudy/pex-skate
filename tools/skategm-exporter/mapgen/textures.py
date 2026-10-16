import hashlib
import os
import shutil
import subprocess
import sys
import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import numpy as np
from PIL import Image

from .sourcetools import ToolError


def _settings(texture):
    lines = []
    if texture.normal_map:
        lines.append('normal 1')
    if texture.sky:
        lines += ['clamps 1', 'clampt 1', 'nomip 1', 'nolod 1']
    return '\n'.join(lines) + '\n' if lines else ''


def _has_alpha(texture):
    return texture.alpha and texture.rgba.shape[2] == 4 and int(texture.rgba[..., 3].min()) < 255


def write_sources(surfaces, game):
    sources = []
    for texture in surfaces.textures.values():
        tga = Path(game) / 'materialsrc' / (texture.name + '.tga')
        tga.parent.mkdir(parents=True, exist_ok=True)
        rgba = np.ascontiguousarray(texture.rgba)
        image = Image.fromarray(rgba, 'RGBA')
        if not _has_alpha(texture):
            image = image.convert('RGB')
        image.save(tga)
        settings = _settings(texture)
        if settings:
            tga.with_suffix('.txt').write_text(settings, encoding='ascii')
        sources.append(tga)
    return sources


VTEX_TRIES = 3


VTF_CACHE_KEY = b'vtf1'
NATIVE_FLAGS = {'clamps': 0x4, 'clampt': 0x8, 'nomip': 0x100, 'nolod': 0x200}
NATIVE_SOURCE = Path(__file__).with_name('native') / 'vtfenc.rs'
_native = None


def _native_library():
    global _native
    if _native is not None:
        return _native or None
    import ctypes
    candidates = [Path(__file__).with_name('native') / 'vtfenc.dll']
    if getattr(sys, '_MEIPASS', None):
        candidates.insert(0, Path(sys._MEIPASS) / 'mapgen' / 'native' / 'vtfenc.dll')
    built = Path(__file__).resolve().parents[1] / 'target' / 'native' / 'vtfenc.dll'
    candidates.append(built)
    if not any(c.is_file() for c in candidates) and NATIVE_SOURCE.is_file() and shutil.which('rustc'):
        compile_native(NATIVE_SOURCE, built)
    _native = False
    for c in candidates:
        if c.is_file():
            try:
                lib = ctypes.CDLL(str(c))
            except OSError:
                continue
            lib.skate_vtf_size.argtypes = [ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint32]
            lib.skate_vtf_size.restype = ctypes.c_size_t
            lib.skate_vtf.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint32, ctypes.c_uint32,
                                      ctypes.c_void_p, ctypes.c_size_t]
            lib.skate_vtf.restype = ctypes.c_ssize_t
            lib.key = hashlib.sha256(c.read_bytes()).hexdigest().encode()
            _native = lib
            break
    return _native or None


def compile_native(source, out):
    out = Path(out)
    out.parent.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(['rustc', '--edition', '2024', '--crate-type', 'cdylib', '-C', 'opt-level=3', '-C', 'panic=abort',
                             '-C', 'target-feature=+crt-static', str(source), '-o', str(out)], capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f'rustc could not build {Path(source).name}: {result.stderr}')
    return out


def native_vtf(rgba, alpha, flags):
    import ctypes
    lib = _native_library()
    h, w = rgba.shape[:2]
    if rgba.shape[2] == 3:
        full = np.empty((h, w, 4), np.uint8)
        full[..., :3] = rgba
        full[..., 3] = 255
        rgba = full
    rgba = np.ascontiguousarray(rgba, np.uint8)
    size = lib.skate_vtf_size(w, h, 1 if alpha else 0)
    out = ctypes.create_string_buffer(size)
    n = lib.skate_vtf(rgba.ctypes.data, w, h, 1 if alpha else 0, flags, out, size)
    if n <= 0:
        raise RuntimeError(f'vtfenc failed ({n})')
    return out.raw[:n]


def _native_flags(settings):
    flags = 0
    for line in settings.splitlines():
        parts = line.split()
        if not parts:
            continue
        if len(parts) != 2 or parts[0] not in NATIVE_FLAGS or parts[1] != '1':
            return None
        flags |= NATIVE_FLAGS[parts[0]]
    return flags


def stage(tga, rgba, settings, sources, pixels, alpha=False):
    tga = Path(tga)
    tga.parent.mkdir(parents=True, exist_ok=True)
    if settings:
        tga.with_suffix('.txt').write_text(settings, encoding='ascii')
    pixels[tga] = (np.ascontiguousarray(rgba), bool(alpha))
    if tga not in sources:
        sources.append(tga)


def _write_tga(tga, rgba, alpha):
    image = Image.fromarray(np.ascontiguousarray(rgba[..., :4] if alpha else rgba[..., :3]), 'RGBA' if alpha else 'RGB')
    image.save(tga)


def _cache_key(tga, prepared=None):
    h = hashlib.sha256(VTF_CACHE_KEY)
    if prepared is not None:
        rgba, alpha = prepared
        h.update(_native_library().key + repr((rgba.shape, alpha)).encode() + rgba.tobytes())
    else:
        h.update(Path(tga).read_bytes())
    txt = Path(tga).with_suffix('.txt')
    h.update(txt.read_bytes() if txt.is_file() else b'')
    return h.hexdigest()


def _load_tga(tga):
    with Image.open(tga) as im:
        alpha = im.mode in ('RGBA', 'LA')
        return np.asarray(im.convert('RGBA')), alpha


def compile_textures(tools, sources, report=print, workers=None, cache=None, pixels=None):
    workers = workers or os.cpu_count() or 4
    cache = Path(cache) if cache else None
    if cache:
        cache.mkdir(parents=True, exist_ok=True)
    pixels = pixels or {}
    lib = None if os.environ.get('SKATEGM_VTEX') == '1' else _native_library()
    if lib is None and os.environ.get('SKATEGM_VTEX') != '1':
        raise RuntimeError('the texture encoder (vtfenc.dll) is missing: every PC must encode textures the same way, '
                           'or players get a different map from the server')
    hits, natives = [], []

    def one(tga):
        tga = Path(tga)
        txt = tga.with_suffix('.txt')
        flags = _native_flags(txt.read_text(encoding='ascii') if txt.is_file() else '') if lib else None
        prepared = pixels.get(tga)
        if flags is not None and prepared is None and tga.is_file():
            prepared = _load_tga(tga)
        native = flags is not None and prepared is not None
        if not native and not tga.is_file():
            _write_tga(tga, *prepared)
        key = cache and _cache_key(tga, prepared if native else None)
        out = _vtf_for(tools.game, tga)
        out.parent.mkdir(parents=True, exist_ok=True)
        if key and (cache / f'{key}.vtf').is_file():
            shutil.copyfile(cache / f'{key}.vtf', out)
            hits.append(tga)
            return
        if native:
            rgba, alpha = prepared
            out.write_bytes(native_vtf(rgba, alpha, flags))
            natives.append(tga)
        else:
            compile_one(tga)
        if key:
            part = cache / f'{key}.{os.getpid()}.{threading.get_ident()}.part'
            shutil.copyfile(out, part)
            os.replace(part, cache / f'{key}.vtf')

    def compile_one(tga):
        for attempt in range(VTEX_TRIES):
            try:
                tools.run('vtex', ['-nopause', '-game', tools.game, tga], check=lambda out: 'SUCCESS' in out.upper())
                return
            except ToolError:
                if attempt == VTEX_TRIES - 1:
                    raise

    report(f'compiling {len(sources)} textures')
    with ThreadPoolExecutor(max_workers=workers) as pool:
        list(pool.map(one, sources))
    if hits:
        report(f'{len(hits)} textures reused from the cache')
    if natives:
        report(f'{len(natives)} textures encoded natively')
    missing = [s for s in sources if not _vtf_for(tools.game, s).is_file()]
    if missing:
        raise RuntimeError('vtex did not produce ' + ', '.join(str(m) for m in missing[:5]))


def _vtf_for(game, tga):
    rel = Path(tga).relative_to(Path(game) / 'materialsrc')
    return Path(game) / 'materials' / rel.with_suffix('.vtf')


def _value(value):
    if isinstance(value, float):
        return f'{value:g}'
    return str(value)


def vmt_text(material):
    lines = [f'"{material.shader}"', '{']
    for key, value in material.params.items():
        lines.append(f'\t"{key}" "{_value(value)}"')
    if material.surfaceprop:
        lines.append(f'\t"$surfaceprop" "{material.surfaceprop}"')
    lines.append('}')
    return '\n'.join(lines) + '\n'


def write_materials(surfaces, game):
    written = []
    for material in surfaces.materials.values():
        path = Path(game) / 'materials' / (material.name + '.vmt')
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(vmt_text(material), encoding='ascii')
        written.append(path)
    return written
