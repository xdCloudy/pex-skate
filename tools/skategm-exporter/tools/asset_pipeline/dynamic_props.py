"""Export authored district DMO locators and their referenced model geometry.

This preserves initial placement, not DMO simulation. Native 0x825876D0 reads
the locator matrix and template ID at +112; RX2 EB001D records are 128 bytes.
No name matching, random placement, ground snapping or collision synthesis.
"""
import argparse
import copy
import hashlib
import json
import struct
import sys
import tempfile
from pathlib import Path

import numpy as np
from .map_writer import write
from tools.owned_game.big import BigArchive

MAX_PROP_COLLISION_TRIANGLES = 250_000
MAX_PROP_COLLISION_INSTANCES = 8_192


def sections(raw):
    if raw[:7] != b'\x89RW4xb2':
        raise ValueError('Expected Xbox RX2')
    count = struct.unpack_from('>I', raw, 32)[0]
    table = struct.unpack_from('>I', raw, 48)[0]
    if table + count*24 > len(raw):
        raise ValueError('Truncated RX2 section table')
    return [struct.unpack_from('>6I', raw, table+i*24) for i in range(count)]


def matrix(raw, at):
    value = np.array(struct.unpack_from('>16f', raw, at)).reshape(4, 4)
    if not np.isfinite(value).all() or not np.allclose(value[:, 3], [0, 0, 0, 1], atol=1e-5):
        raise ValueError('Invalid DMO affine matrix')
    if abs(np.linalg.det(value[:3, :3])) < 1e-8:
        raise ValueError('Singular DMO matrix')
    return value


def records(raw, kind, stride):
    for offset, _, size, _, _, type_id in sections(raw):
        if type_id != kind:
            continue
        if offset+size > len(raw) or size < 32:
            raise ValueError('Invalid DMO section extent')
        count = struct.unpack_from('>I', raw, offset+4)[0]
        start, strings = struct.unpack_from('>2I', raw, offset+12)
        if start != 32 or strings != start+count*stride or strings > size:
            raise ValueError('Unexpected DMO record layout')
        for index in range(count):
            yield offset, offset+start+index*stride, size


def locators(raw):
    result = []
    for base, at, size in records(raw, 0xEB001D, 128):
        name_offset = struct.unpack_from('>I', raw, at+124)[0]
        if not 32 <= name_offset < size:
            raise ValueError('Invalid DMO locator name offset')
        end = raw.index(b'\0', base+name_offset, base+size)
        result.append(dict(instance_id=f'{struct.unpack_from(">Q", raw, at+96)[0]:016X}',
            locator_id=f'{struct.unpack_from(">Q", raw, at+104)[0]:016X}',
            template_id=f'{struct.unpack_from(">Q", raw, at+112)[0]:016X}',
            matrix=matrix(raw, at).tolist(), name=raw[base+name_offset:end].decode('utf-8'),
            source_offset=at, bounds=np.array(struct.unpack_from('>8f', raw, at+64)).reshape(2, 4)[:, :3].tolist()))
    return result


def template_meshes(raw):
    """Resolve EB000D tInstance -> EB0001 model -> EB0023 mesh -> declaration."""
    table = sections(raw)
    result = {}
    for _, at, _ in records(raw, 0xEB000D, 160):
        key = f'{struct.unpack_from(">Q", raw, at+104)[0]:016X}'
        index = struct.unpack_from('>I', raw, at+128)[0]
        if index >= len(table) or table[index][5] != 0xEB0001:
            raise ValueError('Unresolved DMO model reference')
        base, _, size, _, _, _ = table[index]
        start = struct.unpack_from('>I', raw, base+36)[0]
        count = struct.unpack_from('>H', raw, base+48)[0]
        if base+size > len(raw) or start+count*8 > size:
            raise ValueError('Invalid DMO model mesh table')
        mesh_info = []
        for j in range(count):
            mesh_index = struct.unpack_from('>I', raw, base+start+j*8)[0]
            if mesh_index >= len(table) or table[mesh_index][5] != 0xEB0023:
                raise ValueError('Unresolved DMO mesh reference')
            descriptor = struct.unpack_from('>I', raw, table[mesh_index][0]+40)[0]
            if descriptor >= len(table) or table[descriptor][5] != 0x200E9:
                raise ValueError('Unresolved DMO vertex declaration')
            mesh_info.append(table[descriptor][0])
        if key in result:
            raise ValueError('Duplicate DMO template ID')
        result[key] = dict(mesh_info=mesh_info, matrix=matrix(raw, at),
                          model_matrix=matrix(raw, base+struct.unpack_from('>I', raw, base+32)[0]))
    return result


def transform_mesh(arrays, index, transform):
    # NpzFile.items() reads every array before the filter runs.
    values = {key: np.array(arrays[key], copy=True) for key in arrays if key.endswith('_'+str(index))}
    key = f'vertices_{index}'
    values[key] = (values[key] @ transform[:3, :3] + transform[3, :3]).astype('f4')
    normal_matrix = np.linalg.inv(transform[:3, :3]).T
    for prefix in ('normals', 'retail_normals'):
        key = f'{prefix}_{index}'
        if key in values:
            n = values[key] @ normal_matrix
            values[key] = (n/np.maximum(np.linalg.norm(n, axis=1, keepdims=True), 1e-20)).astype('f4')
    if np.linalg.det(transform[:3, :3]) < 0:
        values[f'faces_{index}'] = values[f'faces_{index}'][:, [0, 2, 1]]
    return values


def collision_bounds(arrays, meshes):
    lower = np.full(3, np.inf, dtype=np.float64)
    upper = np.full(3, -np.inf, dtype=np.float64)
    found = False
    for mesh in meshes:
        vertices = arrays.get(f"vertices_{mesh['index']}")
        if vertices is None or not len(vertices):
            continue
        lower = np.minimum(lower, np.min(vertices, axis=0))
        upper = np.maximum(upper, np.max(vertices, axis=0))
        found = True
    if not found or not np.isfinite(lower).all() or not np.isfinite(upper).all():
        return None
    size = upper - lower
    # Tiny decals and zero-thickness planes should not become solid boxes.
    if np.any(size < 0.04) or np.any(size > 128.0):
        return None
    return [lower.tolist(), upper.tolist()]


def collision_triangles(arrays, meshes):
    """Return bounded world-space collision faces from opaque authored meshes."""
    parts = []
    count = 0
    for mesh in meshes:
        # Cutout foliage and blended meshes often consist of thin visual cards;
        # treating those as solid would make authored vegetation unskateable.
        if mesh.get('alpha_mode', 0) != 0:
            continue
        index = mesh['index']
        vertices = arrays.get(f'vertices_{index}')
        faces = arrays.get(f'faces_{index}')
        if vertices is None or faces is None or not len(faces):
            continue
        vertices = np.asarray(vertices, dtype=np.float32)
        faces = np.asarray(faces)
        if vertices.ndim != 2 or vertices.shape[1] != 3 or faces.ndim != 2 or faces.shape[1] != 3:
            raise ValueError('Invalid authored prop collision mesh dimensions')
        if len(faces) > MAX_PROP_COLLISION_TRIANGLES - count:
            return None
        if not np.issubdtype(faces.dtype, np.integer):
            if not np.isfinite(faces).all() or not np.equal(faces, np.floor(faces)).all():
                raise ValueError('Non-integer authored prop collision index')
        faces = faces.astype(np.int64, copy=False)
        if faces.min() < 0 or faces.max() >= len(vertices):
            raise ValueError('Authored prop collision index is out of range')
        triangles = vertices[faces]
        if not np.isfinite(triangles).all() or np.max(np.abs(triangles)) > 1_000_000.0:
            raise ValueError('Invalid authored prop collision vertex')
        edges = triangles[:, [1, 2, 0]] - triangles
        edge_lengths = np.linalg.norm(edges, axis=2)
        area2 = np.linalg.norm(np.cross(edges[:, 0], edges[:, 1]), axis=1)
        usable = (np.max(edge_lengths, axis=1) <= 128.0) & (area2 > 1e-6)
        triangles = np.ascontiguousarray(triangles[usable], dtype='<f4')
        if len(triangles):
            parts.append(triangles)
            count += len(triangles)
    return np.concatenate(parts, axis=0) if parts else np.empty((0, 3, 3), dtype='<f4')


def write_collision_boxes(path, instances):
    boxes = [item['collision_bounds'] for item in instances if 'collision_bounds' in item]
    if len(boxes) > 8_192:
        path.unlink(missing_ok=True)
        return 0
    temporary = path.with_suffix(path.suffix + '.new')
    try:
        with temporary.open('wb') as output:
            output.write(struct.pack('<8sI', b'PEXCOL1\0', len(boxes)))
            for lower, upper in boxes:
                values = (*lower, *upper)
                if not np.isfinite(values).all():
                    raise ValueError('Non-finite authored prop collision bounds')
                output.write(struct.pack('<6f', *values))
        if boxes:
            temporary.replace(path)
        else:
            temporary.unlink(missing_ok=True)
            path.unlink(missing_ok=True)
    finally:
        temporary.unlink(missing_ok=True)
    return len(boxes)


def write_collision_triangles(path, triangles):
    count = sum(len(part) for part in triangles)
    if count > MAX_PROP_COLLISION_TRIANGLES:
        path.unlink(missing_ok=True)
        return 0
    temporary = path.with_suffix(path.suffix + '.new')
    try:
        with temporary.open('wb') as output:
            output.write(struct.pack('<8sI', b'PEXCOL2\0', count))
            for part in triangles:
                values = np.asarray(part, dtype='<f4')
                if values.ndim != 3 or values.shape[1:] != (3, 3) or not np.isfinite(values).all():
                    raise ValueError('Invalid authored prop collision triangles')
                output.write(values.tobytes(order='C'))
        if count:
            temporary.replace(path)
        else:
            temporary.unlink(missing_ok=True)
            path.unlink(missing_ok=True)
    finally:
        temporary.unlink(missing_ok=True)
    return count


def write_collision_instances(path, instances):
    """Write per-instance local colliders so the runtime can move DMO bodies."""
    if len(instances) > MAX_PROP_COLLISION_INSTANCES:
        path.unlink(missing_ok=True)
        return 0
    count = sum(len(triangles) for _, _, triangles in instances)
    if count > MAX_PROP_COLLISION_TRIANGLES:
        path.unlink(missing_ok=True)
        return 0
    temporary = path.with_suffix(path.suffix + '.new')
    try:
        with temporary.open('wb') as output:
            output.write(struct.pack('<8sI', b'PEXCOL3\0', len(instances)))
            for identity, transform, triangles in instances:
                matrix = np.asarray(transform, dtype=np.float32)
                values = np.asarray(triangles, dtype='<f4')
                if matrix.shape != (4, 4) or not np.isfinite(matrix).all():
                    raise ValueError('Invalid authored prop instance transform')
                if values.ndim != 3 or values.shape[1:] != (3, 3) or not np.isfinite(values).all():
                    raise ValueError('Invalid authored prop instance collision triangles')
                output.write(struct.pack('<Q16fI', identity, *matrix.reshape(-1), len(values)))
                output.write(values.tobytes(order='C'))
        if instances:
            temporary.replace(path)
        else:
            temporary.unlink(missing_ok=True)
            path.unlink(missing_ok=True)
    finally:
        temporary.unlink(missing_ok=True)
    return count


def catalog(cache_roots):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'vendor/utt'))
    import rx2_parser
    from .backdrop import texture_groups
    templates, textures = {}, {}
    for root in cache_roots:
        m = json.loads((root/'manifest.json').read_text())
        for key, value in m['textures'].items():
            entry = dict(value)
            for path_key in ('rgba', 'png'):
                if path_key in entry:
                    entry[path_key] = str((root/entry[path_key]).resolve())
            if key in textures and Path(textures[key]['rgba']).read_bytes() != Path(entry['rgba']).read_bytes():
                raise ValueError('Conflicting DMO texture '+key)
            textures[key] = entry
        for model in m['models']:
            raw = (root/model['rx2']).read_bytes()
            table = rx2_parser.RX2File(raw)
            table.parse()
            roles = set(role for mesh in model['meshes'] for role in mesh['retail_texture_ids'])
            bindings = texture_groups(raw, table, roles)
            for key, definition in template_meshes(raw).items():
                meshes = [copy.deepcopy(mesh) for mesh in model['meshes'] if mesh['source_offsets']['mesh_info'] in definition['mesh_info']]
                for mesh in meshes:
                    # DMO resources have a high-bit namespace that display names
                    # omit. Bind the binary channel GUID; never strip that bit.
                    group = bindings[mesh['retail_material_group_index']]
                    mesh['retail_texture_ids'] = {role:f'0x{group[role]:016x}' for role in mesh['retail_texture_ids']}
                    mesh['texture_id'] = mesh['retail_texture_ids'].get('diffuse', mesh['retail_texture_ids'].get('transparent'))
                if len(meshes) != len(definition['mesh_info']):
                    raise ValueError('DMO model geometry omitted '+key)
                if key in templates:
                    raise ValueError('Conflicting DMO template '+key)
                templates[key] = dict(definition, meshes=meshes, npz=root/model['npz'], asset_id=model['asset_id'])
    return templates, textures


def save_catalog(cache_roots, output):
    """Build once per installation; JSON keeps this local cache non-executable."""
    templates, textures = catalog(cache_roots)
    serialised = {key: dict(value, matrix=value['matrix'].tolist(),
        model_matrix=value['model_matrix'].tolist(), npz=str(value['npz'].resolve()))
        for key, value in templates.items()}
    temporary = output.with_suffix('.new')
    temporary.write_text(json.dumps(dict(version=1, templates=serialised, textures=textures)), encoding='utf-8')
    temporary.replace(output)


def load_catalog(path):
    data = json.loads(path.read_text(encoding='utf-8'))
    if data['version'] != 1:
        raise ValueError('Unsupported DMO catalog version')
    for value in data['templates'].values():
        value['matrix'] = np.array(value['matrix'], dtype=np.float64)
        value['model_matrix'] = np.array(value['model_matrix'], dtype=np.float64)
        value['npz'] = Path(value['npz'])
    return data['templates'], data['textures']


def export(manifest_path, cache_roots, output, *, catalog_path=None):
    district = json.loads(manifest_path.read_text())
    templates, textures = catalog(cache_roots) if catalog_path is None else load_catalog(catalog_path)
    placements = {}
    for source in district['simulation_assets']:
        raw = (manifest_path.parent/source['rx2']).read_bytes()
        for item in locators(raw):
            item['source_asset'] = source['asset_id']
            item['source_sha256'] = hashlib.sha256(raw).hexdigest()
            key = item['instance_id']
            if key in placements and placements[key] != item:
                raise ValueError('Conflicting DMO locator '+key)
            placements[key] = item
    report = dict(map=district['map_name'], instances=[], unresolved=[], simulation='initial placement only')
    with tempfile.TemporaryDirectory(prefix='skate-dmo-') as work:
        root = Path(work); models = []; used = set(); collision_meshes = []
        collision_instances = []
        collision_triangle_count = 0; collision_fallback = False
        for number, item in enumerate(placements.values()):
            template = templates.get(item['template_id'])
            if template is None:
                report['unresolved'].append(item)
                continue
            transform = template['model_matrix'] @ template['matrix'] @ np.array(item['matrix'])
            arrays = {}; meshes = copy.deepcopy(template['meshes'])
            with np.load(template['npz'], allow_pickle=False) as original:
                for mesh in meshes:
                    arrays.update(transform_mesh(original, mesh['index'], transform))
                    used.update(mesh['retail_texture_ids'].values())
            path = f'{number}.npz'; np.savez(root/path, **arrays)
            models.append(dict(asset_id=item['instance_id'], meshes=meshes, npz=path))
            bounds = collision_bounds(arrays, meshes)
            instance = dict(item, model_asset=template['asset_id'], meshes=len(meshes))
            if bounds is not None:
                instance['collision_bounds'] = bounds
            report['instances'].append(instance)
            if not collision_fallback:
                mesh_triangles = collision_triangles(arrays, meshes)
                if mesh_triangles is None:
                    if bounds is not None:
                        collision_fallback = True
                        collision_meshes.clear()
                        collision_triangle_count = 0
                elif not len(mesh_triangles):
                    continue
                elif collision_triangle_count + len(mesh_triangles) > MAX_PROP_COLLISION_TRIANGLES:
                    collision_fallback = True
                    collision_meshes.clear()
                    collision_instances.clear()
                    collision_triangle_count = 0
                else:
                    collision_meshes.append(mesh_triangles)
                    instance_matrix = np.asarray(transform, dtype=np.float64)
                    inverse_instance = np.linalg.inv(instance_matrix[:3, :3])
                    local_triangles = (mesh_triangles.reshape(-1, 3) - instance_matrix[3, :3]) @ inverse_instance
                    local_triangles = np.asarray(local_triangles.reshape(-1, 3, 3), dtype='<f4')
                    collision_instances.append((int(item['instance_id'], 16), transform, local_triangles))
                    collision_triangle_count += len(mesh_triangles)
        if models:
            manifest = dict(map_name=district['map_name'], district_name=district['district_name'],
                models=models, textures={key:textures[key] for key in sorted(used)},
                normal_texture_policy=dict(excluded_texture_ids=[]), grind_splines=[])
            (root/'manifest.json').write_text(json.dumps(manifest))
            # Publish only a complete package; preserve the previous one on failure.
            temporary = output.with_suffix('.skate.new')
            try:
                write(root/'manifest.json', temporary, None, render_only=True)
                temporary.replace(output)
            finally:
                temporary.unlink(missing_ok=True)
            collision_path = output.with_suffix('.collision')
            if collision_fallback:
                report['collision_boxes'] = write_collision_boxes(collision_path, report['instances'])
                report['collision_fallback'] = 'district bounds used because mesh collision exceeded conversion limits'
                output.with_suffix('.instances').unlink(missing_ok=True)
            else:
                report['collision_triangles'] = write_collision_triangles(collision_path, collision_meshes)
                report['collision_instances'] = write_collision_instances(
                    output.with_suffix('.instances'), collision_instances)
        else:
            output.unlink(missing_ok=True)
            output.with_suffix('.collision').unlink(missing_ok=True)
            output.with_suffix('.instances').unlink(missing_ok=True)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.with_suffix('.json').write_text(json.dumps(report, indent=2))
    return len(report['instances']), len(report['unresolved'])


def prepare_catalog(game_root, work):
    vendor = Path(__file__).resolve().parents[1]/'vendor'
    sys.path.insert(0, str(vendor/'university/tools/vanilla_map_extraction/tools'))
    from prepare_hawaiian_dream import prepare
    archive = BigArchive(game_root/'data/content/worlddmo.big')
    archive.extract_entries(archive.entries, work/'raw')
    roots = []
    for stream in (work/'raw/data/content/world/dmo').iterdir():
        root = work/'cache'/stream.name
        prepare(stream_directory=stream, output_root=root, utt_root=vendor/'utt',
                district_name=stream.name, map_name=stream.name, raw_texture_cache=True)
        roots.append(root)
    save_catalog(roots, work/'catalog.json')
    return roots


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--manifest', type=Path, required=True)
    p.add_argument('--cache', type=Path, nargs='+', required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    print('DMO placed/unresolved:', export(a.manifest, a.cache, a.output))
