import json
import math
import os
import shutil
import struct
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from pathlib import Path

import numpy as np
from PIL import Image

from . import decode, environment, games, pack, scene, skatecol, skybox, textures, thumbnail, vmf
from .build import Timer, spawn_point
from .coords import Frame
from .source import GameSource
from .sourcetools import Tools
from .tone import filmic, mean_albedo

FORMAT = 2
ICONS = Path(__file__).parent / 'icons'
BUILD = 9
POS_SCALE = 32.0
UV_SCALE = 65536.0
TILE = 5040.0
CELL = 8192.0
BOX = 15872.0
BOX_LOW = -6144.0
BOX_HIGH = 15872.0
LIGHTMAP_SCALE = 4.0
ATLAS_MAX = 2048
PAD = 2
HALVE_FROM = 512
LIGHT_BUDGET = 120e6
PAGE_ALBEDO_MIN = 0.12
SETTLE_SLACK = 0.25
SETTLE_MOST = 30.0
MAX_VERTS = 65535
STREAM_DIR = 'skategm_stream'
LIT, CUTOUT, FOLIAGE, DECAL, UNLIT, UNLIT_CUTOUT, WATER = 0, 1, 2, 3, 4, 5, 6
DECAL_LIFT = 0.5
TILEABLE_DECALS = 'environment.decal_tileable'
PLACEHOLDER_SIZE = 16
PLACEHOLDER_DARK = 12.0
SKIPPED_SHADERS = ('glare.',)


@dataclass(frozen=True)
class District:
    id: str
    title: str
    offset: tuple = (0.0, 0.0)
    centre: tuple = (0.0, 0.0)
    reach: float = 900.0
    spawn_at: tuple = ()


@dataclass(frozen=True)
class World:
    name: str
    title: str
    districts: tuple
    origin: tuple
    spawn_heading: float = 0.0
    extra: dict = field(default_factory=dict)
    game: str = 'skate3'
    light_budget: float = 0.0
    thumb: tuple = ()

    @property
    def map_name(self):
        return games.get(self.game).prefix + self.name


WORLDS = [
    World('portcarverton', 'Port Carverton', (
        District('DIST_University', 'University', (0.0, 0.0), (202.0, -200.0), 1100.0),
        District('DIST_DownTown', 'Downtown', (7000.0, 0.0), (-24.0, 323.0), 1000.0),
        District('DIST_Industrial', 'Industrial', (-7000.0, 0.0), (-158.0, -145.0), 1000.0),
    ), (202.0, 0.0, -200.0), thumb=(-554, -805, 3138, 28, 40, 75)),
    World('newsanvan', 'New San Vanelona', (
        District('BAM', 'New San Vanelona', (0.0, 0.0), (-620.0, -254.0), 3200.0, (1000.0, -50.0)),
    ), (1000.0, 0.0, -50.0), game='skate2', light_budget=320e6, thumb=(-2600, -500, 2100, 28, 40, 75)),
    World('university_streamed', 'University (streamed)', (
        District('DIST_University', 'University', (0.0, 0.0), (202.0, -200.0), 1100.0),
    ), (202.0, 0.0, -200.0)),
]


def by_name(name):
    for w in WORLDS:
        if name in (w.map_name, w.name):
            return w
    raise KeyError('no streamed world named ' + name)


def _mode(material):
    if 'tree' in material.shader:
        return FOLIAGE
    if material.alpha_mode:
        return CUTOUT
    return LIT


def _emissive(material):
    return 'incandescent' in material.shader or material.shader == 'unlit'


def shelf_pack(rects, size):
    order = sorted(range(len(rects)), key=lambda i: (-rects[i][1], -rects[i][0], i))
    x = y = shelf = 0
    out = {}
    for i in order:
        w, h = rects[i]
        if w > size or h > size:
            return None
        if x + w > size:
            x, y, shelf = 0, y + shelf, 0
        if y + h > size:
            return None
        out[i] = (x, y)
        x += w
        shelf = max(shelf, h)
    return out


def page_table(albedo):
    light = np.arange(256, dtype=np.float64)[:, None] / 255.0 * LIGHTMAP_SCALE
    a = np.asarray(albedo, np.float64)
    toned = filmic(light * a) / a
    gamma = np.power(np.maximum(toned, 0.0), 1.0 / 2.2) / 2.0
    return np.clip(np.round(gamma * 255.0), 0, 255).astype(np.uint8)


def page_image(rgba, albedo):
    h, w = rgba.shape[:2]
    table = page_table(albedo)
    out = np.empty((h, w, 3), np.uint8)
    for c in range(3):
        out[..., c] = table[rgba[..., c], c]
    if max(w, h) >= HALVE_FROM and w >= 2 and h >= 2:
        f = out[:h // 2 * 2, :w // 2 * 2].astype(np.uint16)
        out = ((f[0::2, 0::2] + f[1::2, 0::2] + f[0::2, 1::2] + f[1::2, 1::2] + 2) // 4).astype(np.uint8)
    return out


def halve(img):
    h, w = img.shape[:2]
    if h < 2 or w < 2:
        return img
    f = img[:h // 2 * 2, :w // 2 * 2].astype(np.uint16)
    return ((f[0::2, 0::2] + f[1::2, 0::2] + f[0::2, 1::2] + f[1::2, 1::2] + 2) // 4).astype(np.uint8)


def fit_budget(images, budget):
    while sum(i.shape[0] * i.shape[1] for i in images.values()) > budget:
        top = max(max(i.shape[:2]) for i in images.values())
        if top <= 32:
            break
        for k in sorted(images):
            if max(images[k].shape[:2]) >= top:
                images[k] = halve(images[k])
    return images


def _padded(img):
    return np.pad(img, ((PAD, PAD), (PAD, PAD), (0, 0)), mode='edge')


def morton(x, y):
    x &= 0xFFFF
    y &= 0xFFFF
    out = 0
    for i in range(16):
        out |= ((x >> i) & 1) << (2 * i) | ((y >> i) & 1) << (2 * i + 1)
    return out


class Solid:
    def rgba(self, rgb):
        out = np.empty((4, 4, 4), np.uint8)
        out[..., :3] = rgb
        out[..., 3] = 255
        return out


WATER_SHADERS = ('ocean.', 'water.')
WATER_ALPHA = 0.78
WATER_LIFT = 0.5
WATER_COLOUR = (38, 64, 78)
WATER_SHADE = 0.8


def is_water(material):
    return (material.shader or '').lower().startswith(WATER_SHADERS)


class Materials:
    def __init__(self, prefix):
        self.prefix = prefix
        self.vmts = {}
        self.listing = []
        self.index = {}
        self.textures = {}
        self.clamped = set()

    def flush(self, game, sources, pixels):
        for path, entry in self.textures.items():
            if entry is None or entry[3]:
                continue
            writer, tid, alpha, _ = entry[:4]
            tga = Path(game) / 'materialsrc' / (path + '.tga')
            rgba = writer.rgba(tid)
            keep = bool(alpha and int(rgba[..., 3].min()) < 255)
            textures.stage(tga, rgba, 'clamps 1\nclampt 1\n' if path in self.clamped else '', sources, pixels, keep)
            self.textures[path] = [None, tid, alpha, True]

    def texture(self, name, writer, tid, alpha, clamp=False):
        path = f'{self.prefix}/{name}'
        if clamp:
            self.clamped.add(path)
        entry = self.textures.get(path)
        if entry is None:
            self.textures[path] = [writer, tid, alpha, False]
        elif alpha and not entry[2]:
            self.textures[path] = [writer, tid, True, False]
        return path

    def solid(self, rgb):
        rgb = tuple(int(c) for c in rgb)
        path = f'{self.prefix}/w_{rgb[0]:02x}{rgb[1]:02x}{rgb[2]:02x}'
        if path not in self.textures:
            self.textures[path] = [Solid(), rgb, False, False]
        return path

    def material(self, kind, texture_path, cutoff=0.5):
        key = (kind, texture_path)
        if key in self.index:
            return self.index[key]
        name = f'{self.prefix}/m{len(self.listing)}'
        shader = 'UnlitGeneric' if kind in (UNLIT, UNLIT_CUTOUT, WATER) else 'LightmappedGeneric'
        params = {'$basetexture': texture_path}
        if kind in (CUTOUT, FOLIAGE, UNLIT_CUTOUT):
            params.update({'$alphatest': 1, '$alphatestreference': round(cutoff, 3)})
        if kind == FOLIAGE:
            params['$nocull'] = 1
        if kind == DECAL:
            params.update({'$translucent': 1, '$decal': 1})
        if kind == WATER:
            params.update({'$translucent': 1, '$alpha': WATER_ALPHA, '$nocull': 1})
        self.vmts[name] = (shader, params)
        self.index[key] = len(self.listing)
        self.listing.append({'name': name, 'kind': kind})
        return self.index[key]


TINT_EMPTY = 0.2


def layered_light(layer, chroma):
    c = np.asarray(chroma, np.float64) / 255.0
    total = c.sum(-1, keepdims=True)
    tint = np.where(total > TINT_EMPTY, c * 3.0 / np.maximum(total, 1e-6), 1.0)
    light = np.asarray(layer, np.float64)[..., None] * tint / 3.0
    return np.clip(np.round(light), 0, 255).astype(np.uint8)


class Writer:
    def __init__(self, scene_, frame):
        self.scene = scene_
        self.frame = frame
        self._albedo = {}
        self._rgba = {}

    def rgba(self, tid):
        if tid not in self._rgba:
            if '+' in tid:
                self._rgba[tid] = self.layered(tid)
            else:
                self._rgba[tid] = self.scene.textures[tid].rgba()
        return self._rgba[tid]

    def layered(self, key):
        layer, tint, component = key.split('+')
        base = self.rgba(layer)
        h, w = base.shape[:2]
        colour = self.rgba(tint)
        if colour.shape[:2] != (h, w):
            colour = np.asarray(Image.fromarray(np.ascontiguousarray(colour[..., :3])).resize((w, h), Image.BILINEAR))
        out = np.empty((h, w, 4), np.uint8)
        out[..., :3] = layered_light(base[..., int(component)], colour[..., :3])
        out[..., 3] = 255
        return out

    def component(self, layer, uvs):
        page = self.rgba(layer)
        h, w = page.shape[:2]
        uv = np.clip(np.abs(np.asarray(uvs, np.float64)), 0.0, 1.0)
        xs = np.minimum((uv[:, 0] * w).astype(np.int64), w - 1)
        ys = np.minimum((uv[:, 1] * h).astype(np.int64), h - 1)
        means = page[ys, xs, :3].reshape(-1, 3).mean(0)
        return int(np.argmax(means))

    def water_colour(self, material):
        env = material.textures.get('environment') or material.textures.get('specular')
        if env and env in self.scene.textures:
            mean = self.rgba(env)[..., :3].reshape(-1, 3).mean(0) * WATER_SHADE
            return tuple(int(round(c)) for c in mean)
        return WATER_COLOUR

    def placeholder(self, tid):
        if not hasattr(self, '_placeholder'):
            self._placeholder = {}
        if tid not in self._placeholder:
            rgba = self.rgba(tid)
            self._placeholder[tid] = (max(rgba.shape[:2]) <= PLACEHOLDER_SIZE and float(rgba[..., :3].mean()) < PLACEHOLDER_DARK
                                      and int(rgba[..., 3].min()) < 128)
        return self._placeholder[tid]

    def visible(self, tid):
        if not hasattr(self, '_visible'):
            self._visible = {}
        if tid not in self._visible:
            self._visible[tid] = int(self.rgba(tid)[..., 3].max()) > 8
        return self._visible[tid]

    def albedo(self, tid):
        if tid not in self._albedo:
            self._albedo[tid] = mean_albedo(self.rgba(tid))
        return self._albedo[tid]


class Atlases:
    def __init__(self, report, budget=None):
        self.report = report
        self.budget = budget or LIGHT_BUDGET
        self.pages = {}
        self.where = {}
        self.sheets = []

    def note(self, lm, diffuse, centre, writer):
        albedo = writer.albedo(diffuse)
        entry = self.pages.get(lm)
        if entry is None:
            self.pages[lm] = [writer, albedo.copy(), np.array(centre, np.float64), 1]
        else:
            entry[1] = entry[1] + albedo
            entry[2] = entry[2] + centre
            entry[3] += 1

    def build(self, name_for):
        keys = [k for k in self.pages if k not in self.where]
        cells = {}
        for lm in keys:
            _, _, total, n = self.pages[lm]
            c = total / n
            cells[lm] = (int(math.floor(c[0] / (TILE * 2))) + 0x8000, int(math.floor(c[1] / (TILE * 2))) + 0x8000)
        keys.sort(key=lambda k: (morton(*cells[k]), k))
        def one(lm):
            w, albedo, _, n = self.pages[lm]
            return page_image(w.rgba(lm), np.maximum(albedo / n, PAGE_ALBEDO_MIN))
        with ThreadPoolExecutor(max_workers=os.cpu_count() or 4) as pool:
            images = dict(zip(keys, pool.map(one, keys)))
        fit_budget(images, self.budget)
        images = {k: _padded(v) for k, v in images.items()}
        texels = 0
        start = 0
        while start < len(keys):
            lo, hi = start + 1, len(keys)
            best = start + 1
            while lo <= hi:
                mid = (lo + hi) // 2
                if shelf_pack([(images[k].shape[1], images[k].shape[0]) for k in keys[start:mid]], ATLAS_MAX) is not None:
                    best, lo = mid, mid + 1
                else:
                    hi = mid - 1
            chosen = keys[start:best]
            rects = [(images[k].shape[1], images[k].shape[0]) for k in chosen]
            size = 64
            while shelf_pack(rects, size) is None:
                size *= 2
            placed = shelf_pack(rects, size)
            canvas = np.zeros((size, size, 3), np.uint8)
            index = len(self.sheets)
            for j, lm in enumerate(chosen):
                x, y = placed[j]
                img = images[lm]
                h, w_ = img.shape[:2]
                canvas[y:y + h, x:x + w_] = img
                self.where[lm] = (index, (x + PAD) / size, (y + PAD) / size, (w_ - 2 * PAD) / size, (h - 2 * PAD) / size)
            self.sheets.append(name_for(index, canvas))
            texels += size * size
            start = best
        self.report(f'{len(keys)} lightmap pages in {len(self.sheets)} atlases ({texels / 1e6:.1f} M texels)')
        self.pages = {}


def _tile_key(points):
    c = (points.min(0) + points.max(0)) / 2
    return int(math.floor(c[0] / TILE)), int(math.floor(c[1] / TILE))


def assign(meshes, frame):
    tiles = {}
    for mesh in meshes:
        if not len(mesh.faces):
            continue
        p = frame.point(mesh.positions[np.unique(mesh.faces)])
        tiles.setdefault(_tile_key(p), []).append(mesh)
    return tiles


class TileBuilder:
    def __init__(self, writer, materials, atlases, key, lod):
        self.w = writer
        self.m = materials
        self.atlases = atlases
        self.key = key
        self.lod = lod
        self.origin = np.array([key[0] * TILE + TILE / 2, key[1] * TILE + TILE / 2, 0.0])
        self.groups = {}

    def add(self, mesh):
        material = self.w.scene.materials[mesh.material]
        if is_water(material):
            return self.add_water(mesh, material)
        diffuse = material.textures.get('diffuse')
        if not diffuse or self.w.placeholder(diffuse) or (material.shader or '').startswith(SKIPPED_SHADERS):
            return
        mode = _mode(material)
        alpha = mode != LIT
        positions = self.w.frame.point(mesh.positions) - self.origin
        lm = material.textures.get('lightmap')
        tint = material.textures.get('chromaticity')
        if lm and tint and mesh.lightmap_uvs is not None:
            channel = mesh.lightmap_channel if mesh.lightmap_channel is not None else self.w.component(lm, mesh.lightmap_uvs)
            lm = f'{lm}+{tint}+{channel}'
        page = None
        if lm and mesh.lightmap_uvs is not None and not _emissive(material):
            page = lm
            self.atlases.note(lm, diffuse, positions.mean(0) + self.origin, self.w)
        tex = self.m.texture('d_' + diffuse[2:], self.w, diffuse, alpha)
        kind = (UNLIT_CUTOUT if alpha else UNLIT) if page is None else mode
        mat = self.m.material(kind, tex, material.alpha_cutoff)
        lm_uv = np.clip(np.abs(mesh.lightmap_uvs), 0.0, 1.0) if page is not None else None
        self.groups.setdefault((mat, page), []).append((positions, mesh.uvs, lm_uv, mesh.faces))
        decal = material.textures.get('decal')
        if decal and mesh.decal_uvs is not None and page is not None and self.w.visible(decal):
            n = self.w.frame.direction(mesh.normals)
            n = n / np.maximum(np.sqrt((n * n).sum(1, keepdims=True)), 1e-12)
            clamp = not (material.shader or '').startswith(TILEABLE_DECALS)
            dtex = self.m.texture(('dc_' if clamp else 'dl_') + decal[2:], self.w, decal, True, clamp)
            dmat = self.m.material(DECAL, dtex)
            self.groups.setdefault((dmat, page), []).append((positions + n * DECAL_LIFT, mesh.decal_uvs, lm_uv, mesh.faces))

    def add_water(self, mesh, material):
        if not len(mesh.faces):
            return
        positions = self.w.frame.point(mesh.positions.astype(np.float64)) - self.origin + np.array([0.0, 0.0, WATER_LIFT])
        tex = self.m.solid(self.w.water_colour(material))
        mat = self.m.material(WATER, tex)
        uvs = np.zeros((len(mesh.positions), 2), np.float32)
        self.groups.setdefault((mat, None), []).append((positions, uvs, None, mesh.faces))

    def write(self, path):
        place = self.atlases.where
        merged = {}
        for (mat, page), parts in self.groups.items():
            sheet = place[page][0] if page is not None else 255
            for positions, uvs, lm_uv, faces in parts:
                lm = None
                if page is not None:
                    _, ox, oy, sw, sh = place[page]
                    lm = np.stack([ox + lm_uv[:, 0] * sw, oy + lm_uv[:, 1] * sh], 1)
                merged.setdefault((mat, sheet), []).append((positions, uvs, lm, faces))
        kinds = self.m.listing
        groups = []
        tris = 0
        for (mat, sheet), parts in sorted(merged.items(), key=lambda kv: (kinds[kv[0][0]]['kind'] == DECAL, kv[0][0], kv[0][1])):
            pos = np.concatenate([p[0] for p in parts]).astype(np.float32)
            uv = np.concatenate([p[1] for p in parts]).astype(np.float32)
            lm = None
            if sheet != 255:
                lm = np.clip(np.round(np.concatenate([p[2] for p in parts]) * 65535.0), 0, 65535).astype(np.uint16)
            offsets = np.cumsum([0] + [len(p[0]) for p in parts[:-1]])
            faces = np.concatenate([p[3] + o for p, o in zip(parts, offsets)]).astype(np.int64)[:, ::-1]
            for chunk in split_faces(np.ascontiguousarray(faces)):
                used, inverse = np.unique(chunk.reshape(-1), return_inverse=True)
                groups.append((mat, sheet, pos[used], uv[used], None if lm is None else lm[used],
                               inverse.reshape(-1).astype(np.uint16)))
                tris += len(chunk)
        out = [b'SGT2', struct.pack('<I', len(groups))]
        lo = np.full(3, np.inf)
        hi = np.full(3, -np.inf)
        for mat, sheet, pos, uv, lm, idx in groups:
            out.append(struct.pack('<HBBII', mat, 1 if lm is not None else 0, sheet if lm is not None else 0, len(pos), len(idx)))
            if lm is not None:
                v = np.empty(len(pos), dtype=[('p', '<i4', 3), ('t', '<i4', 2), ('l', '<u2', 2)])
                v['l'] = lm
            else:
                v = np.empty(len(pos), dtype=[('p', '<i4', 3), ('t', '<i4', 2)])
            v['p'] = np.round(pos.astype(np.float64) * POS_SCALE)
            v['t'] = np.round(np.clip(uv.astype(np.float64), -30000, 30000) * UV_SCALE)
            out.append(v.tobytes())
            out.append(idx.astype('<u2').tobytes())
            lo = np.minimum(lo, pos.min(0))
            hi = np.maximum(hi, pos.max(0))
        Path(path).write_bytes(b''.join(out))
        return {'tris': int(tris), 'groups': len(groups), 'mats': sorted({int(g[0]) for g in groups}),
                'atl': sorted({int(g[1]) for g in groups if g[4] is not None}),
                'lo': (lo + self.origin).round(1).tolist() if groups else None,
                'hi': (hi + self.origin).round(1).tolist() if groups else None}


def split_faces(faces, max_verts=MAX_VERTS):
    start = 0
    n = len(faces)
    while start < n:
        end = min(n, start + max_verts // 3)
        if end < n:
            lo, hi = end, n
            while lo < hi:
                mid = (lo + hi + 1) // 2
                if len(np.unique(faces[start:mid])) <= max_verts:
                    lo = mid
                else:
                    hi = mid - 1
            end = max(lo, start + 1)
        yield faces[start:end]
        start = end


def _sky(source, district, frame, work, report):
    sky = environment.load(source, district, Path(work) / 'env')
    return sky, skybox.render(sky, frame)


def write_vmf(path, spawn, heading, skyname, sky, frame, title, info):
    v = vmf.Vmf()
    v.shell((-BOX, -BOX, BOX_LOW), (BOX, BOX, BOX_HIGH))
    pitch, yaw = (-50.0, 30.0)
    if sky is not None:
        s = frame.direction(sky.sun)
        d = -s / math.sqrt(float((s * s).sum()))
        pitch = round(math.degrees(math.asin(max(-1.0, min(1.0, float(d[2]))))), 1)
        yaw = round(math.degrees(math.atan2(float(d[1]), float(d[0]))), 1)
        colour = skybox.linear_to_srgb8(np.asarray(sky.fog_colour))
        v.entity('env_fog_controller', origin=spawn + np.array([0, 0, 540.0]), angles=(0, 0, 0), spawnflags=1, fogenable=1,
                 fogblend=0, fogcolor=f'{colour[0]} {colour[1]} {colour[2]}', fogcolor2=f'{colour[0]} {colour[1]} {colour[2]}',
                 fogstart=round(sky.fog_near / 0.0254), fogend=round(sky.fog_far / 0.0254),
                 fogmaxdensity=round(sky.fog_max, 3), farz=-1, foglerptime=0, use_angles=0, targetname='fog')
    v.entity('light_environment', origin=spawn + np.array([0, 0, 512.0]), angles=(0, yaw, 0), pitch=pitch,
             _light='255 238 214 300', _ambient='170 190 220 120', _lightHDR='-1 -1 -1 1', _ambientHDR='-1 -1 -1 1')
    v.entity('shadow_control', origin=spawn + np.array([0, 0, 520.0]), angles=(pitch, yaw, 0), color='90 90 100',
             distance=96, disableallshadows=0)
    for dx, dy in ((0, 0), (64, 0), (-64, 0), (0, 64), (0, -64)):
        v.entity('info_player_start', origin=spawn + np.array([dx, dy, 0.0]), angles=(0, heading, 0))
    v.entity('info_target', origin=spawn, targetname='skategm_info', skategm_title=title, **info)
    Path(path).write_text(v.text(skyname), encoding='ascii')


class _NoSpawn:
    spawn = ()


PIECE = TILE / 8
PIECE_SPLIT = 2000
GRID_CELL = 96.0
GRID_LEVELS = 4
GRID_GAP = 72.0
GRID_NONE = 32767


def ground_grid(col, lo, hi, path, seed=7, either_side=False):
    t = np.asarray(col, np.float64)
    n = np.cross(t[:, 1] - t[:, 0], t[:, 2] - t[:, 0])
    area2 = np.sqrt((n * n).sum(1))
    up = (area2 > 1e-6) & ((np.abs(n[:, 2]) if either_side else n[:, 2]) > 0.5 * area2)
    t, area = t[up], area2[up] / 2
    inside = (t[:, :, 0].max(1) >= lo[0]) & (t[:, :, 0].min(1) <= hi[0]) & (t[:, :, 1].max(1) >= lo[1]) & (t[:, :, 1].min(1) <= hi[1])
    t, area = t[inside], area[inside]
    count = np.maximum(1, np.ceil(area / (GRID_CELL * GRID_CELL / 4))).astype(np.int64)
    count = np.minimum(count, 4096)
    idx = np.repeat(np.arange(len(t)), count)
    rng = np.random.default_rng(seed)
    r1, r2 = rng.random(len(idx)), rng.random(len(idx))
    flip = r1 + r2 > 1
    r1[flip], r2[flip] = 1 - r1[flip], 1 - r2[flip]
    a, b, c = t[idx, 0], t[idx, 1], t[idx, 2]
    pts = a + (b - a) * r1[:, None] + (c - a) * r2[:, None]
    pts = np.concatenate([pts, t.mean(1)])
    nx = int(math.ceil((hi[0] - lo[0]) / GRID_CELL))
    ny = int(math.ceil((hi[1] - lo[1]) / GRID_CELL))
    ix = np.floor((pts[:, 0] - lo[0]) / GRID_CELL).astype(np.int64)
    iy = np.floor((pts[:, 1] - lo[1]) / GRID_CELL).astype(np.int64)
    ok = (ix >= 0) & (iy >= 0) & (ix < nx) & (iy < ny)
    cell = (iy * nx + ix)[ok]
    z = pts[ok, 2]
    order = np.lexsort((-z, cell))
    cell, z = cell[order], z[order]
    grid = np.full((nx * ny, GRID_LEVELS), GRID_NONE, np.int16)
    starts = np.flatnonzero(np.r_[True, cell[1:] != cell[:-1]])
    ends = np.r_[starts[1:], len(cell)]
    for s_, e_ in zip(starts, ends):
        kept = []
        for v in z[s_:e_]:
            if not kept or kept[-1] - v > GRID_GAP:
                kept.append(v)
                if len(kept) == GRID_LEVELS:
                    break
        grid[cell[s_], :len(kept)] = np.clip(np.round(np.asarray(kept) / 2), -32767, 32766).astype(np.int16)
    with open(path, 'wb') as f:
        f.write(b'SGG1' + struct.pack('<fffIII', float(lo[0]), float(lo[1]), GRID_CELL, nx, ny, GRID_LEVELS))
        f.write(grid.astype('<i2').tobytes())
    return nx, ny


def _piece_file(path, tris, corner):
    q = np.round((tris - corner) * POS_SCALE).astype(np.int64).reshape(-1, 3)
    verts, inv = np.unique(q, axis=0, return_inverse=True)
    if len(verts) > MAX_VERTS:
        raise RuntimeError(f'ground piece {path.name} has {len(verts)} vertices')
    faces = inv.reshape(-1, 3)
    faces = faces[(faces[:, 0] != faces[:, 1]) & (faces[:, 1] != faces[:, 2]) & (faces[:, 0] != faces[:, 2])]
    with open(path, 'wb') as f:
        f.write(b'SGC1' + struct.pack('<II', len(verts), len(faces)))
        f.write(verts.astype('<i4').tobytes())
        f.write(faces.astype('<u2').tobytes())


def ground_pieces(col, out_dir, piece=PIECE, split=PIECE_SPLIT):
    t = np.asarray(col, np.float64).reshape(-1, 3, 3)
    n = np.cross(t[:, 1] - t[:, 0], t[:, 2] - t[:, 0])
    t = t[np.sqrt((n * n).sum(1)) > 0.02]
    lo = np.floor(t[:, :, :2].min(1) / piece).astype(np.int64)
    hi = np.floor(t[:, :, :2].max(1) / piece).astype(np.int64)
    span = hi - lo + 1
    reps = span[:, 0] * span[:, 1]
    idx = np.repeat(np.arange(len(t)), reps)
    first = np.repeat(np.cumsum(reps) - reps, reps)
    k = np.arange(len(idx)) - first
    px = lo[idx, 0] + k % span[idx, 0]
    py = lo[idx, 1] + k // span[idx, 0]
    order = np.lexsort((idx, py, px))
    idx, px, py = idx[order], px[order], py[order]
    starts = np.flatnonzero(np.r_[True, (px[1:] != px[:-1]) | (py[1:] != py[:-1])])
    ends = np.r_[starts[1:], len(idx)]
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    def emit(ix, iy, path, tris, corner, size, found):
        if len(tris) <= split or len(path) >= 4:
            name = f'g{ix}_{iy}' + (f'_{path}' if path else '') + '.bin'
            _piece_file(out / name, tris, corner)
            if path:
                found.append(path)
            return
        half = size / 2
        xy = tris[:, :, :2] - corner[:2]
        for sub in range(4):
            x0, y0 = (sub % 2) * half, (sub // 2) * half
            inside = (xy[:, :, 0].max(1) >= x0) & (xy[:, :, 0].min(1) < x0 + half) & (xy[:, :, 1].max(1) >= y0) & (xy[:, :, 1].min(1) < y0 + half)
            if inside.any():
                emit(ix, iy, path + str(sub), tris[inside], corner + np.array([x0, y0, 0.0]), half, found)

    def top(span):
        s_, e_ = span
        ix, iy = int(px[s_]), int(py[s_])
        found = []
        emit(ix, iy, '', t[idx[s_:e_]], np.array([ix * piece, iy * piece, 0.0]), piece, found)
        return f'{ix} {iy}', found

    split_index = {}
    with ThreadPoolExecutor(max_workers=os.cpu_count() or 4) as pool:
        for key, found in pool.map(top, list(zip(starts, ends))):
            if found:
                split_index[key] = found
    return split_index


def open_spawn(col, centre, clear=220.0, headroom=300.0, tries=4000):
    t = np.asarray(col, np.float64)
    n = np.cross(t[:, 1] - t[:, 0], t[:, 2] - t[:, 0])
    a2 = np.sqrt((n * n).sum(1))
    c = t.mean(1)
    flat = (a2 > 2 * 6000.0) & (n[:, 2] > 0.985 * a2)
    steep = (a2 > 1e-6) & (np.abs(n[:, 2]) < 0.7 * a2)
    cand = np.flatnonzero(flat)
    if not len(cand):
        return None
    d = np.hypot(c[cand, 0] - centre[0], c[cand, 1] - centre[1])
    good = []
    for k in cand[np.argsort(d, kind='stable')][:tries]:
        p = c[k]
        box = (np.abs(c[:, 0] - p[0]) < clear) & (np.abs(c[:, 1] - p[1]) < clear)
        over = box & (t[:, :, 2].min(1) > p[2] + 4) & (t[:, :, 2].min(1) < p[2] + headroom)
        wall = box & steep & (t[:, :, 2].max(1) > p[2] + 6) & (t[:, :, 2].min(1) < p[2] + 150)
        bumps = box & (t[:, :, 2].max(1) > p[2] + 6) & (t[:, :, 2].min(1) < p[2] + 150)
        if not over.any() and not wall.any() and not bumps.any():
            good.append(np.array([p[0], p[1], p[2] + 16.0]))
            if len(good) == 10:
                break
    if not good:
        return None
    return min(enumerate(good), key=lambda e: (round(float(e[1][2]) / 64.0), e[0]))[1]


class PropSet:
    """Skate 3's movable objects, fixed in place: drawn with their tile, and
    fed to the engine as entities near the skater (collision, rails)."""

    def __init__(self):
        self.templates = []
        self.index = {}
        self.instances = []

    def template(self, key, meshes):
        if key not in self.index:
            raw = Frame()
            tris = [raw.raw(m.positions.astype(np.float64))[m.faces] for m in meshes if len(m.faces)]
            self.index[key] = len(self.templates)
            self.templates.append(np.concatenate(tris) if tris else np.zeros((0, 3, 3)))
        return self.index[key]

    def write(self, path):
        out = [b'SGP1', struct.pack('<I', len(self.templates))]
        for t in self.templates:
            q = np.round(np.asarray(t, np.float64).reshape(-1, 9) * POS_SCALE).astype('<i4')
            out.append(struct.pack('<I', len(q)))
            out.append(q.tobytes())
        Path(path).write_bytes(b''.join(out))


def placed_mesh(mesh, matrix):
    from .scene import Mesh
    m = np.asarray(matrix, np.float64)
    return Mesh(mesh.material, (mesh.positions.astype(np.float64) @ m[:3, :3] + m[3, :3]).astype(np.float32), mesh.faces,
                mesh.uvs, (mesh.normals.astype(np.float64) @ m[:3, :3]).astype(np.float32), None, mesh.name)


class Floors:
    CELL = 4.0

    def __init__(self, triangles):
        t = np.asarray(triangles, np.float64).reshape(-1, 3, 3)
        n = np.cross(t[:, 1] - t[:, 0], t[:, 2] - t[:, 0])
        keep = np.abs(n[:, 1]) > 0.5 * np.sqrt((n * n).sum(1))
        self.t = t[keep]
        lo = np.floor(self.t[:, :, [0, 2]].min(1) / self.CELL).astype(np.int64)
        hi = np.floor(self.t[:, :, [0, 2]].max(1) / self.CELL).astype(np.int64)
        self.cells = {}
        for i in range(len(self.t)):
            for cx in range(lo[i, 0], hi[i, 0] + 1):
                for cz in range(lo[i, 1], hi[i, 1] + 1):
                    self.cells.setdefault((cx, cz), []).append(i)

    def heights(self, x, z):
        idx = self.cells.get((int(math.floor(x / self.CELL)), int(math.floor(z / self.CELL))))
        if not idx:
            return np.zeros(0)
        t = self.t[idx]
        a, b, c = t[:, 0], t[:, 1], t[:, 2]
        d = (b[:, 2] - c[:, 2]) * (a[:, 0] - c[:, 0]) + (c[:, 0] - b[:, 0]) * (a[:, 2] - c[:, 2])
        ok = np.abs(d) > 1e-12
        d = np.where(ok, d, 1.0)
        w1 = ((b[:, 2] - c[:, 2]) * (x - c[:, 0]) + (c[:, 0] - b[:, 0]) * (z - c[:, 2])) / d
        w2 = ((c[:, 2] - a[:, 2]) * (x - c[:, 0]) + (a[:, 0] - c[:, 0]) * (z - c[:, 2])) / d
        w3 = 1 - w1 - w2
        inside = ok & (w1 >= -1e-6) & (w2 >= -1e-6) & (w3 >= -1e-6)
        return (w1 * a[:, 1] + w2 * b[:, 1] + w3 * c[:, 1])[inside]

    def drop(self, points, slack=SETTLE_SLACK, most=SETTLE_MOST):
        p = np.asarray(points, np.float64)
        lo, hi = p.min(0), p.max(0)
        bottom = lo[1]
        mid = (lo + hi) / 2
        span = (hi - lo) * 0.3
        best = None
        for dx, dz in ((0, 0), (-1, -1), (1, -1), (-1, 1), (1, 1)):
            h = self.heights(mid[0] + dx * span[0], mid[2] + dz * span[2])
            h = h[h <= bottom + slack]
            if len(h):
                top = float(h.max())
                best = top if best is None else max(best, top)
        if best is None:
            return 0.0
        gap = bottom - best
        return gap if slack < gap < most else 0.0


def settled(matrix, gap):
    m = np.array(matrix, np.float64)
    m[3, 1] -= gap
    return m


def add_props(source, cache, frame, materials, atlases, near_builders, builders, propset, work, report, floors=None):
    from . import clip, props as dmo
    if not source.game.dmo:
        report(f'movable objects: {source.game.title} has none to read')
        return 0
    loaded = dmo.load(source, cache, clip.Area(), work / 'dmo', report)
    if not loaded.instances:
        return 0
    holder = scene.Scene(Path(cache), '', loaded.textures, loaded.materials, [])
    writer = Writer(holder, frame)
    count = settled_count = 0
    for inst in loaded.instances:
        template = loaded.templates.get(inst.template)
        if template is None:
            continue
        matrix = np.asarray(inst.matrix, np.float64)
        placed = [placed_mesh(m, matrix) for m in template.meshes]
        if floors is not None and placed:
            gap = floors.drop(np.concatenate([m.positions for m in placed]))
            if gap:
                matrix = settled(matrix, gap)
                placed = [placed_mesh(m, matrix) for m in template.meshes]
                settled_count += 1
        centre = frame.point(matrix[3, :3][None])[0]
        key = (int(math.floor(centre[0] / TILE)), int(math.floor(centre[1] / TILE)))
        tb = near_builders.get(key)
        if tb is None:
            tb = TileBuilder(writer, materials, atlases, key, 0)
            near_builders[key] = tb
            builders.append(tb)
        tb.w = writer
        for mesh in placed:
            tb.add(mesh)
        t = propset.template(inst.template, template.meshes)
        rot = dmo.source_rotation(matrix)
        p, y, r = dmo.angles(rot)
        propset.instances.append([t, round(float(centre[0]), 2), round(float(centre[1]), 2), round(float(centre[2]), 2), p, y, r])
        count += 1
    report(f'{count} movable objects ({len(loaded.templates)} kinds), fixed in place; {settled_count} settled onto the ground')
    return count


THUMB_REACH = 40000.0


def thumb_view(world, spawn):
    if world.thumb:
        x, y, z, pitch, yaw = world.thumb[:5]
        fov = world.thumb[5] if len(world.thumb) > 5 else 75.0
        return np.array([x, y, z], np.float64), float(pitch), float(yaw), float(fov)
    return np.asarray(spawn, np.float64) + np.array([-700.0, -700.0, 600.0]), 30.0, 45.0, 75.0


def thumb_parts(sc, writer, frame, eye, reach=THUMB_REACH):
    parts = []
    for mesh in sc.meshes:
        material = sc.materials[mesh.material]
        diffuse = material.textures.get('diffuse')
        if not diffuse or not len(mesh.faces) or writer.placeholder(diffuse):
            continue
        positions = frame.point(mesh.positions.astype(np.float64))
        if np.abs(positions.mean(0) - eye).max() > reach:
            continue
        light = None
        lm = material.textures.get('lightmap')
        if lm and mesh.lightmap_uvs is not None and not _emissive(material):
            tint = material.textures.get('chromaticity')
            if tint:
                channel = mesh.lightmap_channel if mesh.lightmap_channel is not None else writer.component(lm, mesh.lightmap_uvs)
                lm = f'{lm}+{tint}+{channel}'
            light = writer.rgba(lm)
        parts.append((positions, mesh.faces, mesh.uvs, mesh.lightmap_uvs, writer.rgba(diffuse), light))
    return parts


def icon(world, source, addon, work, view=None, report=print):
    from dataclasses import replace
    if view:
        world = replace(world, thumb=tuple(view))
    if not world.thumb:
        raise ValueError('give a camera (thumb) for the icon')
    eye, pitch, yaw, fov = thumb_view(world, None)
    chosen = world.districts[0]
    for d in world.districts:
        frame = district_frame(world, d)
        centre = frame.point(np.array([[d.centre[0], 0.0, d.centre[1]]]))[0]
        reach = d.reach / 0.0254
        if np.all(np.abs(eye[:2] - centre[:2]) <= reach):
            chosen = d
    for d in (chosen,):
        frame = district_frame(world, d)
        sc = scene.load(decode.decode(source, d.id, Path(work) / 'districts', report), collision=False)
        image = thumbnail.render_lit(thumb_parts(sc, Writer(sc, frame), frame, eye), (eye, pitch, yaw, fov))
        out = Path(addon) / 'maps' / 'thumb' / f'{world.map_name}.png'
        out.parent.mkdir(parents=True, exist_ok=True)
        image.save(out)
        report(f'icon: {out}')
        return out


def water_triangles(sc, frame):
    tris = [frame.point(m.positions.astype(np.float64))[m.faces] for m in sc.meshes
            if len(m.faces) and m.material in sc.materials and is_water(sc.materials[m.material])]
    return np.concatenate(tris) if tris else np.zeros((0, 3, 3))


def district_frame(world, d):
    f = Frame()
    f.offset = f.raw(np.asarray(world.origin, np.float64)) - f.raw(np.array([d.offset[0], 0.0, d.offset[1]]))
    return f


def build(world, game_root, gmod, addon_dir, work_dir, report=print, workers=None, keep=False, lod1=True):
    timer = Timer(report)
    work = Path(work_dir)
    name = world.map_name
    build_dir = work / 'build' / name
    if build_dir.exists():
        shutil.rmtree(build_dir)
    game = build_dir / 'game'
    src = build_dir / 'src'
    src.mkdir(parents=True)
    tools = Tools(gmod, game, log=build_dir / 'tools.log')
    source = game_root if isinstance(game_root, GameSource) else GameSource(game_root, work / 'disc')
    addon = Path(addon_dir)
    stream_out = addon / STREAM_DIR / name
    stage = addon / STREAM_DIR / (name + '.partial')
    if stage.exists():
        shutil.rmtree(stage)
    stage.mkdir(parents=True)
    prefix = f'skategm/{name}'
    materials = Materials(prefix)
    atlases = Atlases(report, world.light_budget)
    builders = []
    collisions, surfaces_, rails = [], [], []
    background = ThreadPoolExecutor(max_workers=2)
    pending = []
    districts = []
    sources = []
    pixels = {}
    first_sky = None
    tiles_info = {}
    propset = PropSet()
    thumb_done = False

    def atlas_name(i, canvas):
        textures.stage(game / 'materialsrc' / prefix / 'lm' / f'a{i}.tga', canvas, 'nomip 1\nnolod 1\n', sources, pixels)
        return f'{prefix}/lm/a{i}'

    for d in world.districts:
        frame = district_frame(world, d)
        cache = decode.decode(source, d.id, work / 'districts', report)
        full = scene.load(cache)
        timer.done(f'{d.id} decode')
        for lod in ((0, 1) if lod1 else (0,)):
            sc = full
            if lod == 1:
                proxy = decode.decode_proxy(source, d.id, work / 'districts', report)
                if proxy is None:
                    continue
                sc = scene.load(proxy, collision=False, fallback_textures=full.textures)
            writer = Writer(sc, frame)
            groups = assign(sc.meshes, frame)
            for key, meshes in sorted(groups.items()):
                tb = TileBuilder(writer, materials, atlases, key, lod)
                for mesh in meshes:
                    tb.add(mesh)
                if tb.groups:
                    builders.append(tb)
            report(f'{d.id} LOD{lod}: {len(groups)} tiles')
        near_builders = {tb.key: tb for tb in builders if tb.lod == 0}
        add_props(source, cache, frame, materials, atlases, near_builders, builders, propset, work, report,
                  Floors(full.collision.triangles))
        col = frame.point(full.collision.triangles.reshape(-1, 3)).reshape(-1, 3, 3)
        collisions.append(col)
        surfaces_.append(full.collision.surfaces)
        rails += [(frame.point(r.points), r.closed, r.native) for r in full.rails]
        centre = frame.point(np.array([[d.centre[0], 0.0, d.centre[1]]]))[0]
        reach = d.reach / 0.0254
        lo = centre[:2] - reach
        hi = centre[:2] + reach
        seek = centre
        if d.spawn_at:
            seek = frame.point(np.array([[d.spawn_at[0], 0.0, d.spawn_at[1]]]))[0]
        near = col[(np.abs(col[:, :, :2] - seek[:2]).max(axis=(1, 2)) < 6000)]
        spot = open_spawn(near, seek)
        spawn = np.round(spot if spot is not None else spawn_point(near, _NoSpawn(), frame))
        info = {'id': d.id, 'title': d.title, 'spawn': spawn.tolist(), 'lo': lo.round().tolist(), 'hi': hi.round().tolist()}
        ground = f'ground_{d.id.lower()}.bin'
        pending.append(background.submit(ground_grid, col.reshape(-1, 3, 3), lo, hi, stage / ground))
        info['ground'] = ground
        water = water_triangles(full, frame)
        if len(water):
            name_w = f'water_{d.id.lower()}.bin'
            pending.append(background.submit(ground_grid, water, lo, hi, stage / name_w, either_side=True))
            info['water'] = name_w
        sea = environment.sea(source, d.id, work / 'env')
        if sea is not None:
            info['sea'] = {'z': round(float(frame.point(np.array([[0.0, sea[0], 0.0]]))[0, 2]), 1),
                           'colour': [int(c) for c in sea[1]]}
        sky, faces = _sky(source, d.id, frame, work, report)
        if faces:
            skyname = name + '_' + d.id.lower().removeprefix('dist_')
            for face_name, rgba in faces.items():
                textures.stage(game / 'materialsrc' / 'skybox' / f'{skyname}{face_name}.tga', np.ascontiguousarray(rgba)[..., :3],
                               'clamps 1\nclampt 1\nnomip 1\nnolod 1\n', sources, pixels)
                materials.vmts[f'skybox/{skyname}{face_name}'] = ('UnlitGeneric', {'$basetexture': f'skybox/{skyname}{face_name}',
                                                                                  '$nofog': 1, '$ignorez': 1})
            info['sky'] = skyname
            if first_sky is None:
                first_sky = (skyname, sky, frame, spawn)
        if sky is not None:
            colour = skybox.linear_to_srgb8(np.asarray(sky.fog_colour))
            info['fog'] = {'start': round(sky.fog_near / 0.0254), 'end': round(sky.fog_far / 0.0254),
                           'max': round(float(sky.fog_max), 3), 'colour': [int(c) for c in colour]}
        districts.append(info)
        eye, pitch, yaw, fov = thumb_view(world, districts[0]['spawn'])
        inside = lo[0] <= eye[0] <= hi[0] and lo[1] <= eye[1] <= hi[1]
        shipped = ICONS / f'{name}.png'
        if not thumb_done and shipped.is_file():
            (addon / 'maps' / 'thumb').mkdir(parents=True, exist_ok=True)
            shutil.copy2(shipped, addon / 'maps' / 'thumb' / f'{name}.png')
            thumb_done = True
        if not thumb_done and (inside if world.thumb else len(districts) == 1):
            image = thumbnail.render_lit(thumb_parts(full, Writer(full, frame), frame, eye), (eye, pitch, yaw, fov))
            (addon / 'maps' / 'thumb').mkdir(parents=True, exist_ok=True)
            image.save(addon / 'maps' / 'thumb' / f'{name}.png')
            thumb_done = True
            timer.done(f'{d.id} icon')
        atlases.build(atlas_name)
        for tb in builders:
            stem = f't{tb.key[0]}_{tb.key[1]}_{tb.lod}'
            tinfo = tb.write(stage / f'{stem}.bin')
            tinfo['file'] = f'{stem}.bin'
            if str(tb.lod) in tiles_info.get(f'{tb.key[0]} {tb.key[1]}', {}):
                raise RuntimeError(f'two districts share tile {tb.key}')
            tiles_info.setdefault(f'{tb.key[0]} {tb.key[1]}', {})[str(tb.lod)] = tinfo
        builders = []
        materials.flush(game, sources, pixels)
        full = sc = writer = None
        timer.done(f'{d.id} tiles')
        textures.compile_textures(tools, sources, report, workers, cache=work / 'vtfcache', pixels=pixels)
        sources.clear()
        pixels.clear()
        timer.done(f'{d.id} textures')
    collision = np.concatenate(collisions)
    pieces_job = background.submit(ground_pieces, collision, stage / 'phys')
    for mname, (shader, params) in materials.vmts.items():
        p = game / 'materials' / (mname + '.vmt')
        p.parent.mkdir(parents=True, exist_ok=True)
        lines = [f'"{shader}"', '{'] + [f'\t"{k}" "{v}"' for k, v in params.items()] + ['}']
        p.write_text('\n'.join(lines) + '\n', encoding='ascii')
    timer.done('textures')
    surf = np.concatenate(surfaces_)
    if first_sky:
        skyname, sky, frame, spawn = first_sky
    else:
        skyname, sky, frame, spawn = 'sky_day01_01', None, district_frame(world, world.districts[0]), np.array(districts[0]['spawn'])
    write_vmf(src / f'{name}.vmf', spawn, world.spawn_heading, skyname, sky, frame, world.title, {'skategm_stream': name})
    base = src / name
    tools.run('vbsp', ['-game', game, base], check=lambda o: '**** leaked ****' not in o)
    tools.run('vvis', ['-fast', '-game', game, base])
    tools.run('vrad', ['-fast', '-ldr', '-game', game, base])
    timer.done('compile')
    sk3c = build_dir / 'skate3.sk3c'
    sk3c.write_bytes(skatecol.encode(collision, surf, rails))
    propset.write(stage / 'props.bin')
    split_pieces = pieces_job.result()
    for job in pending:
        job.result()
    background.shutdown()
    report(f'physics ground: {len(split_pieces)} pieces split')
    index = {'format': FORMAT, 'map': name, 'title': world.title, 'tile': TILE, 'cell': CELL,
             'props': {'file': 'props.bin', 'instances': propset.instances}, 'phys': {'dir': 'phys', 'piece': PIECE, 'split': split_pieces},
             'atlases': atlases.sheets, 'spawn': [float(x) for x in spawn], 'districts': districts, 'skyname': skyname,
             'materials': materials.listing, 'tiles': tiles_info, 'triangles': int(len(collision)), 'rails': len(rails)}
    (stage / 'index.json').write_text(json.dumps(index, separators=(',', ':')), encoding='utf-8')
    info_path = build_dir / 'skate3.json'
    info_path.write_text(json.dumps({'format': 1, 'map': name, 'title': world.title, 'stream': name,
                                     'spawn': [float(x) for x in spawn], 'triangles': int(len(collision))}, indent=1))
    files = {skatecol.PACK_PATH: sk3c, 'skategm/skate3.json': info_path}
    files.update({k: v for k, v in pack.game_files(game, ['materials']).items()
                  if k.startswith(f'materials/skybox/{skyname}')})
    (addon / 'maps').mkdir(parents=True, exist_ok=True)
    final = addon / 'maps' / f'{name}.bsp'
    pack.pack(tools, base.with_suffix('.bsp'), files, final)
    mats = addon / 'materials' / prefix
    if mats.exists():
        shutil.rmtree(mats)
    shutil.copytree(game / 'materials' / prefix, mats)
    sky_out = addon / 'materials' / 'skybox'
    sky_out.mkdir(parents=True, exist_ok=True)
    for f in (game / 'materials' / 'skybox').glob(f'{name}_*'):
        shutil.copy(f, sky_out / f.name)
    if stream_out.exists():
        shutil.rmtree(stream_out)
    stage.rename(stream_out)
    timer.done('pack')
    if not keep:
        shutil.rmtree(build_dir, ignore_errors=True)
    report(f'built {final}: {len(tiles_info)} tiles, {len(collision)} collision triangles')
    return final, timer.phases
