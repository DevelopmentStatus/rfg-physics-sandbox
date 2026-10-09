"""Convert extracted RFG skinned character parts (ccmesh/gcmesh + rig) to glTF 2.0.

Formats decoded from the files themselves (see Research/evidence_registry.md,
E-008..E-010). Left-handed game space is mapped to Bevy's right-handed space by
negating Z (positions, normals, translations) and reversing triangle winding.

Usage: python Extractors/rfg_model.py <dir/> <rig> <output.glb> <part,part,...> [--static]
Outputs must resolve inside the project's LocalData folder.
"""
import json
import struct

import numpy as np
import sys
from pathlib import Path

PROJECT = Path(__file__).resolve().parents[1]

def find_submeshes(cc, index_total, max_index):
    """Locate the submesh table: consecutive 5-word records (material, start, count, vmin, vmax) whose
    ranges tile the index buffer exactly and reach the highest referenced vertex. The table sits at 0x9C
    for the player body and at 0xF4 for multi-record weapon meshes, so search instead of assuming (E-014)."""
    for o in list(range(0x9C, 0x9D)) + list(range(0x80, len(cc) - 20, 4)):
        for n in range(1, 17):
            if o + 20 * n > len(cc):
                break
            recs = [struct.unpack_from('<5I', cc, o + 20 * i) for i in range(n)]
            pos = 0
            for r in recs:
                if r[1] != pos or r[2] == 0 or r[3] > r[4]:
                    break
                pos += r[2]
            else:
                if pos == index_total and max(r[4] for r in recs) == max_index:
                    return [dict(material=r[0], start=r[1], count=r[2], vmin=r[3], vmax=r[4]) for r in recs]
    raise ValueError('Submesh table not found')


MESH_MAGIC = 0xFAC351A9


def read_mesh(cc, gc):
    """Returns dict(vertices, indices, submeshes) from ccmesh/gcmesh bytes."""
    magic, version = struct.unpack_from('<II', cc, 0)
    if magic != MESH_MAGIC or version != 3:
        raise ValueError('Unexpected mesh header')
    gpu_hash = struct.unpack_from('<I', cc, 0x44)[0]
    gpu_size = struct.unpack_from('<I', cc, 0x4C)[0]
    if gpu_size != len(gc) or struct.unpack_from('<I', gc, 0)[0] != gpu_hash:
        raise ValueError('ccmesh does not describe this gcmesh')
    index_total = struct.unpack_from('<I', cc, 0x64)[0]
    max_index = max(struct.unpack_from(f'<{index_total}H', gc, 0x10))
    subs = find_submeshes(cc, index_total, max_index)
    # The vertex buffer starts at the 16-byte aligned end of the index buffer.
    vstart = struct.unpack_from('<I', cc, 0x60)[0]
    if not 0x10 + index_total * 2 <= vstart < 0x10 + index_total * 2 + 16:
        raise ValueError('Unexpected index buffer size')
    vertex_count = max_index + 1
    if len(gc) - vstart - vertex_count * 32 not in (0, 4):
        raise ValueError('Vertex buffer size does not match 32-byte stride')
    indices = struct.unpack_from(f'<{index_total}H', gc, 0x10)
    if max(indices) >= vertex_count:
        raise ValueError('Index out of range')
    verts = []
    for v in range(vertex_count):
        o = vstart + v * 32
        px, py, pz = struct.unpack_from('<3f', gc, o)
        n = struct.unpack_from('<3B', gc, o + 12)          # unsigned bytes, n = b / 127.5 - 1
        t = struct.unpack_from('<4B', gc, o + 16)          # tangent + handedness
        w = struct.unpack_from('<4B', gc, o + 20)          # bone weights, sum 254..256 (rounded)
        j = struct.unpack_from('<4B', gc, o + 24)          # bone indices (skeleton order)
        u, vv = struct.unpack_from('<2h', gc, o + 28)      # UV, 1/1024 units
        verts.append(dict(p=(px, py, pz), n=tuple(c / 127.5 - 1 for c in n),
                          t=tuple(c / 127.5 - 1 for c in t),
                          w=tuple(c / (sum(w) or 1) for c in w), j=j, uv=(u / 1024.0, vv / 1024.0)))
    return dict(vertices=verts, indices=indices, submeshes=subs)


STATIC_MAGIC = 0xC0FFEE11


def read_static_mesh(cs, gs):
    """Static (unskinned) mesh from csmesh/gsmesh bytes (E-014): same header fields as ccmesh at
    0x14/0x44/0x4C/0x60/0x64/0x9C, 24-byte vertices: float3 position, 3 unsigned-byte normal + pad,
    4-byte tangent, int16x2 UV (1/1024). Returns the same dict shape as read_mesh."""
    magic, version = struct.unpack_from('<II', cs, 0)
    if magic != STATIC_MAGIC or version != 4:
        raise ValueError('Unexpected static mesh header')
    if struct.unpack_from('<I', cs, 0x44)[0] != struct.unpack_from('<I', gs, 0)[0]             or struct.unpack_from('<I', cs, 0x4C)[0] != len(gs):
        raise ValueError('csmesh does not describe this gsmesh')
    vstart, index_total = struct.unpack_from('<II', cs, 0x60)
    indices = struct.unpack_from(f'<{index_total}H', gs, 0x10)
    subs = find_submeshes(cs, index_total, max(indices))
    vertex_count = max(indices) + 1
    if vstart + vertex_count * 24 > len(gs):
        raise ValueError('Vertex buffer shorter than the indices need')
    verts = []
    for v in range(vertex_count):
        o = vstart + v * 24
        p = struct.unpack_from('<3f', gs, o)
        n = struct.unpack_from('<3B', gs, o + 12)
        u, vv = struct.unpack_from('<2h', gs, o + 20)
        verts.append(dict(p=p, n=tuple(c / 127.5 - 1 for c in n), t=(0, 0, 0, 0), w=(1, 0, 0, 0),
                          j=(0, 0, 0, 0), uv=(u / 1024.0, vv / 1024.0)))
    return dict(vertices=verts, indices=indices, submeshes=subs)


def find_records(data, count, table):
    """Offset of the first bone record: the header size varies between rigs, so
    search for count-1 consecutive 36-byte records with valid names and parents."""
    for start in range(0x40, 0x400, 4):
        k = start
        for n in range(count - 1):
            if k + 36 > len(data):
                break
            name_off, parent, marker = (struct.unpack_from('<I', data, k)[0],
                                        struct.unpack_from('<i', data, k + 28)[0],
                                        struct.unpack_from('<i', data, k + 32)[0])
            if name_off not in table or marker != -1 or not 0 <= parent <= n + 1:
                break
            k += 36
        else:
            return start
    raise ValueError('Bone records not found')


def read_rig(data):
    """Returns bones: list of dict(name, parent, local_t, bind_pos). Bind rotations are identity."""
    count = struct.unpack_from('<I', data, 0x24)[0]
    names_base = data.find(b'root\0')
    table = {}
    i = names_base
    while i < len(data):
        if data[i] == 0:
            i += 1
            continue
        j = data.index(b'\0', i)
        table[i - names_base] = data[i:j].decode('ascii')
        i = j + 1
    bones = [dict(name='root', parent=-1, local_t=(0.0, 0.0, 0.0), bind_pos=(0.0, 0.0, 0.0))]
    k = find_records(data, count, table)  # first record (pelvis); records are 36 bytes
    while len(bones) < count:
        name_off = struct.unpack_from('<I', data, k)[0]
        if name_off not in table:
            raise ValueError(f'Bad bone name offset at {k:#x}')
        a = struct.unpack_from('<3f', data, k + 4)   # negated model-space bind position
        b = struct.unpack_from('<3f', data, k + 16)  # local translation to parent
        parent = struct.unpack_from('<i', data, k + 28)[0]
        bones.append(dict(name=table[name_off], parent=parent, local_t=b, bind_pos=tuple(-c for c in a)))
        k += 36
    for index, bone in enumerate(bones[1:], 1):
        if not 0 <= bone['parent'] < index:
            raise ValueError(f'Bone {bone["name"]} has parent {bone["parent"]} that does not precede it')
        # The stored local offset is zero for children of the root, so derive it
        # from the bind positions (checked equal to the stored one elsewhere).
        parent_pos = bones[bone['parent']]['bind_pos']
        derived = tuple(c - p for c, p in zip(bone['bind_pos'], parent_pos))
        if bone['parent'] != 0 and any(abs(d - s) > 1e-3 for d, s in zip(derived, bone['local_t'])):
            raise ValueError(f'Bone {bone["name"]}: stored local offset disagrees with bind positions')
        bone['local_t'] = derived
    return bones


def read_rig_tags(data, bones):
    """Prop-point tags after the bone records of a `*.rig_pc` (count at 0x30): 60-byte records of
    name offset, row-major 3x3 rotation (rows are the tag's axes in the parent bone's frame),
    position, parent bone (index into `read_rig` bones, implicit root = 0), -1 (E-029).
    `L_hand`/`R_hand` are where held items attach (`FUN_007cea00`)."""
    count = struct.unpack_from('<I', data, 0x30)[0]
    names_base = data.find(b'root\x00')
    # The tag records follow the last bone record, which ends with the -1 marker.
    k = names_base - count * 60
    tags = []
    for _ in range(count):
        name_off = struct.unpack_from('<I', data, k)[0]
        rot = struct.unpack_from('<9f', data, k + 4)
        pos = struct.unpack_from('<3f', data, k + 40)
        parent, marker = struct.unpack_from('<ii', data, k + 52)
        if marker != -1:
            raise ValueError(f'Bad rig tag record at {k:#x}')
        name = data[names_base + name_off:data.index(b'\x00', names_base + name_off)].decode('ascii')
        tags.append(dict(name=name, parent=bones[parent]['name'], rot=rot, pos=pos))
        k += 60
    return tags


# Prop-point names looked up by the game (`FUN_007295d0`, `FUN_007baf30`); stored as name hashes.
MESH_TAG_NAMES = ('muzzle_flash', 'weapon_handle', 'secondary_handle', 'destruction', 'destruction2', 'blood')


def read_mesh_tags(header):
    """Prop-point tags at the end of a static mesh header (`*.csmesh_pc`): u32 count, then 52-byte
    records of name hash (lowercase CRC32), row-major 3x3 rotation, position (E-029)."""
    import zlib
    names = {zlib.crc32(n.encode(), 0xFFFFFFFF) ^ 0xFFFFFFFF: n for n in MESH_TAG_NAMES}  # FUN_00a03310
    for off in range(len(header) - 4, 0, -4):
        count = struct.unpack_from('<I', header, off)[0]
        if 1 <= count <= 32 and off + 4 + count * 52 == len(header):
            tags = []
            for r in range(count):
                h, *v = struct.unpack_from('<I12f', header, off + 4 + r * 52)
                tags.append(dict(name=names.get(h, f'{h:08x}'), rot=tuple(v[:9]), pos=tuple(v[9:])))
            return tags
    return []


def tag_to_bevy(tag):
    """(translation, rotation quaternion xyzw) of a tag in Bevy space (Z mirrored)."""
    r = np.array(tag['rot'], dtype=np.float64).reshape(3, 3).T  # columns = tag axes
    s = np.diag([1.0, 1.0, -1.0])
    return np.array(flipz(tag['pos'])), s @ r @ s


def read_part_rig(data):
    """Bones of a clothing-part rig such as `mason_coat.rig_pc`: every bone has a
    record (no implicit root) and the name table follows the records directly.
    Returns list of dict(name, parent, bind_pos); parents index this list."""
    count = struct.unpack_from('<I', data, 0x24)[0]
    for start in range(0x40, 0x400, 4):
        base = start + count * 36
        if base > len(data):
            break
        bones = []
        for r in range(count):
            k = start + r * 36
            name_off = struct.unpack_from('<I', data, k)[0]
            parent, marker = struct.unpack_from('<ii', data, k + 28)
            end = data.find(b'\0', base + name_off)
            if marker != -1 or not -1 <= parent < r or end <= base + name_off:
                break
            name = data[base + name_off:end]
            if not name.replace(b'-', b'').replace(b'_', b'').isalnum():
                break
            pos = struct.unpack_from('<3f', data, k + 4)  # negated model-space bind position
            bones.append(dict(name=name.decode(), parent=parent, bind_pos=tuple(-c for c in pos)))
        if len(bones) == count:
            return bones
    raise ValueError('Part rig records not found')


def merge_part_rig(bones, part):
    """Appends the part's bones that the body skeleton lacks (coat flaps, scarf)
    as children of their part-rig parents. Returns part index -> skeleton index."""
    index = {b['name']: i for i, b in enumerate(bones)}
    remap = []
    for bone in part:
        if bone['name'] not in index:
            parent = remap[bone['parent']]
            local = tuple(c - p for c, p in zip(bone['bind_pos'], bones[parent]['bind_pos']))
            bones.append(dict(name=bone['name'], parent=parent, local_t=local, bind_pos=bone['bind_pos']))
            index[bone['name']] = len(bones) - 1
        remap.append(index[bone['name']])
    return remap


def flipz(v):
    return (v[0], v[1], -v[2])


def build_glb(parts, bones, extra=None, skinned=True):
    """Returns GLB bytes for several skinned parts sharing one skeleton.

    parts: list of (name, mesh). `extra` may hold materials/images/animations."""
    bin_chunks, views, accessors = [], [], []

    def add(data, target=None):
        pad = (-sum(len(c) for c in bin_chunks)) % 4
        if pad:
            bin_chunks.append(b'\0' * pad)
        offset = sum(len(c) for c in bin_chunks)
        bin_chunks.append(data)
        view = dict(buffer=0, byteOffset=offset, byteLength=len(data))
        if target:
            view['target'] = target
        views.append(view)
        return len(views) - 1

    def accessor(data, comp, kind, target=None, minmax=None):
        size = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4, 'MAT4': 16}[kind]
        width = {5126: 4, 5123: 2, 5121: 1}[comp]
        acc = dict(bufferView=add(data, target), componentType=comp,
                   count=len(data) // (size * width), type=kind)
        if minmax:
            acc['min'], acc['max'] = minmax
        accessors.append(acc)
        return len(accessors) - 1

    materials = (extra or {}).get('materials') or [dict(
        pbrMetallicRoughness=dict(baseColorFactor=[0.7, 0.7, 0.7, 1], metallicFactor=0, roughnessFactor=0.9),
        doubleSided=True)]
    meshes, nodes = [], []
    for bone in bones:
        parent = bones[bone['parent']] if bone['parent'] >= 0 else None
        local = bone['local_t'] if parent else bone['bind_pos']
        nodes.append(dict(name=bone['name'], translation=list(flipz(local))))
    for index, bone in enumerate(bones):
        if bone['parent'] >= 0:
            nodes[bone['parent']].setdefault('children', []).append(index)
    scene_nodes = [0] if skinned else []
    for name, mesh in parts:
        verts = mesh['vertices']
        pos = b''.join(struct.pack('<3f', *flipz(v['p'])) for v in verts)
        nrm = b''.join(struct.pack('<3f', *flipz(v['n'])) for v in verts)
        # Stored V lies in [-1, 0]; 1 + V puts it in glTF's top-left [0, 1] range. Checked by
        # overlaying the body UVs on mason_body1_d (gloves land on the glove texture, E-020).
        uv = b''.join(struct.pack('<2f', v['uv'][0], 1.0 + v['uv'][1]) for v in verts)
        jts = b''.join(struct.pack('<4B', *v['j']) for v in verts)
        wts = b''.join(struct.pack('<4f', *v['w']) for v in verts)
        xs = [flipz(v['p']) for v in verts]
        lo = [min(p[i] for p in xs) for i in range(3)]
        hi = [max(p[i] for p in xs) for i in range(3)]
        a_pos = accessor(pos, 5126, 'VEC3', 34962, (lo, hi))
        a_nrm = accessor(nrm, 5126, 'VEC3', 34962)
        a_uv = accessor(uv, 5126, 'VEC2', 34962)
        a_jts = accessor(jts, 5121, 'VEC4', 34962)
        a_wts = accessor(wts, 5126, 'VEC4', 34962)
        primitives = []
        for sub in mesh['submeshes']:
            strip = mesh['indices'][sub['start']:sub['start'] + sub['count']]
            flipped = []
            for i in range(len(strip) - 2):
                a, b, c = strip[i], strip[i + 1], strip[i + 2]
                if a == b or b == c or a == c:
                    continue  # degenerate triangle joining strips
                if i % 2:
                    b, c = c, b  # strip winding alternates
                flipped += [a, c, b]  # winding reverses with the Z flip
            a_idx = accessor(struct.pack(f'<{len(flipped)}H', *flipped), 5123, 'SCALAR', 34963)
            attrs = dict(POSITION=a_pos, NORMAL=a_nrm, TEXCOORD_0=a_uv)
            if skinned:
                attrs.update(JOINTS_0=a_jts, WEIGHTS_0=a_wts)
            primitives.append(dict(attributes=attrs, indices=a_idx, material=sub.get('gltf_material', 0)))
        meshes.append(dict(name=name, primitives=primitives))
        node = dict(name=name, mesh=len(meshes) - 1)
        if skinned:
            node['skin'] = 0
        scene_nodes.append(len(nodes))
        nodes.append(node)
    doc = dict(asset=dict(version='2.0', generator='BevyRFGPhysics rfg_model.py'),
               scene=0, scenes=[dict(nodes=scene_nodes)], nodes=nodes, meshes=meshes,
               materials=materials, accessors=accessors, bufferViews=views)
    if skinned:
        ibm = b''.join(struct.pack('<16f', 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0,
                                   *(-c for c in flipz(b['bind_pos'])), 1) for b in bones)
        a_ibm = accessor(ibm, 5126, 'MAT4')
        doc['skins'] = [dict(joints=list(range(len(bones))), inverseBindMatrices=a_ibm, skeleton=0)]
    for key in ('images', 'textures', 'samplers'):
        if extra and extra.get(key):
            doc[key] = extra[key]
    # clips: list of (name, [(bone, (times, quats), (times, translations))]) in game space.
    animations = []
    for clip_name, channels in (extra or {}).get('clips', []):
        samplers, targets = [], []
        for bone, rot, pos in channels:
            for path, (times, values) in (('rotation', rot), ('translation', pos)):
                a_in = accessor(struct.pack(f'<{len(times)}f', *times), 5126, 'SCALAR',
                                minmax=([min(times)], [max(times)]))
                if path == 'rotation':
                    data = b''.join(struct.pack('<4f', -q[0], -q[1], q[2], q[3]) for q in values)
                    a_out = accessor(data, 5126, 'VEC4')
                else:
                    data = b''.join(struct.pack('<3f', *flipz(v)) for v in values)
                    a_out = accessor(data, 5126, 'VEC3')
                samplers.append(dict(input=a_in, output=a_out, interpolation='LINEAR'))
                targets.append(dict(sampler=len(samplers) - 1, target=dict(node=bone, path=path)))
        animations.append(dict(name=clip_name, samplers=samplers, channels=targets))
    if animations:
        doc['animations'] = animations
    blob = b''.join(bin_chunks)
    blob += b'\0' * ((-len(blob)) % 4)
    doc['buffers'] = [dict(byteLength=len(blob))]
    js = json.dumps(doc, separators=(',', ':')).encode()
    js += b' ' * ((-len(js)) % 4)
    total = 12 + 8 + len(js) + 8 + len(blob)
    return struct.pack('<III', 0x46546C67, 2, total) + struct.pack('<II', len(js), 0x4E4F534A) + js \
        + struct.pack('<II', len(blob), 0x004E4942) + blob


def main(prefix, rig_path, out_path, parts, skinned):
    out = Path(out_path).resolve()
    if not out.is_relative_to(PROJECT / 'LocalData'):
        raise ValueError('Output must be inside LocalData')
    bones = read_rig(Path(rig_path).read_bytes())
    meshes = []
    for part in parts:
        mesh = read_mesh(Path(f'{prefix}{part}.ccmesh_pc').read_bytes(),
                         Path(f'{prefix}{part}.gcmesh_pc').read_bytes())
        for v in mesh['vertices']:
            if any(w > 0 and j >= len(bones) for j, w in zip(v['j'], v['w'])):
                raise ValueError(f'{part}: vertex references a joint outside the skeleton')
        meshes.append((part, mesh))
        print(f'{part}: {len(mesh["vertices"])} vertices, {len(mesh["indices"])} indices, '
              f'{len(mesh["submeshes"])} submeshes')
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(build_glb(meshes, bones, skinned=skinned))
    print(f'{len(bones)} bones -> {out}')


if __name__ == '__main__':
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    main(args[0], args[1], args[2], args[3].split(','), '--static' not in sys.argv)
