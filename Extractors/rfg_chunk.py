"""Read RFG destructible chunk files (`*.cchk_pc` + `*.gchk_pc`), original 32-bit PC build.

Buildings and destructible props are chunks. A chunk holds one render mesh shared by all of its
pieces, its materials, and one or more destroyables. A destroyable lists subpieces (rigid pieces
with mass, centre of mass, bounds, physical material), links between subpieces (the structural
bonds, with a yield limit and contact area), and dlods (placed groups of subpieces with a
position and orientation). Collision shapes are a Havok 5.5 packfile (not decoded here).

Layout reference: rfg-modding/RFGM.Formats (MIT), written for the 64-bit Re-Mars-tered build
(chunk version 56). This build is version 55 with 4-byte pointers; every section boundary is
checked against the offsets the header declares, so a mismatch raises instead of guessing
(see Research/evidence_registry.md, E-021).
"""
import struct

MAGIC = 0xB0CEEFA5
HAVOK_MAGIC = 0x484B504D  # 'MPKH'? read as u32 1212891981


class ChunkError(ValueError):
    pass


class Reader:
    def __init__(self, data, pos=0):
        self.data, self.pos = data, pos

    def take(self, fmt):
        values = struct.unpack_from('<' + fmt, self.data, self.pos)
        self.pos += struct.calcsize('<' + fmt)
        return values

    def one(self, fmt):
        return self.take(fmt)[0]

    def align(self, n):
        self.pos = (self.pos + n - 1) // n * n

    def cstr(self):
        end = self.data.index(b'\0', self.pos)
        text = self.data[self.pos:end].decode('ascii')
        self.pos = end + 1
        return text


def read_header(b):
    r = Reader(b)
    (magic, version, source_version, render_checksum, render_cpu_offset, render_cpu_size,
     collision_checksum, collision_offset, collision_size, destruction_checksum,
     destruction_offset, destruction_size) = r.take('12I')
    if magic != MAGIC:
        raise ChunkError('not a chunk file')
    if (version, source_version) != (55, 20):
        raise ChunkError(f'unverified chunk version {version}/{source_version}')
    r.pos = 0x130
    bmin, bmax = r.take('3f'), r.take('3f')
    # 32-bit runtime pointer fields; only the counts and the GPU size are meaningful on disk.
    fields = dict(zip((
        'render_mesh', 'material_map', 'materials', 'num_render_materials', 'num_general_objects',
        'general_objects', 'num_base_destroyables', 'base_destroyables', 'rbb_parent_lods',
        'next_chunk', 'prev_chunk', 'uid_refcount', 'stream_data', 'data_size', 'num_script_points',
        'script_points', 'rbb_shards', 'num_object_identifiers', 'object_identifiers',
        'num_object_skirts', 'object_skirts', 'mopp_handle', 'texture_names_size', 'texture_names',
        'num_light_clip_objects', 'light_clip_objects', 'render_gpu_offset', 'render_gpu_size'),
        r.take('28I')))
    for key in [k for k in fields if k.startswith('num_')]:
        fields[key] = struct.unpack('<i', struct.pack('<I', fields[key]))[0]
    return dict(version=version, render_cpu_offset=render_cpu_offset, render_cpu_size=render_cpu_size,
                collision_offset=collision_offset, collision_size=collision_size,
                destruction_offset=destruction_offset, destruction_size=destruction_size,
                bmin=bmin, bmax=bmax, **fields)


def read_mesh_config(r):
    start = r.pos
    version, check, cpu_size, gpu_size, num_sub, _, num_vertices = r.take('7I')
    stride0, vertex_format, num_uv, stride1 = r.take('4B')
    _, num_indices, _ = r.take('3I')
    index_size, topology, num_blocks = r.take('BBH')
    r.align(16)
    subs = []
    for _ in range(num_sub):
        n, *f, _ = r.take('I9fI')
        subs.append(dict(num_blocks=n, offset=tuple(f[0:3]), bmin=tuple(f[3:6]), bmax=tuple(f[6:9])))
    blocks = []
    for _ in range(sum(s['num_blocks'] for s in subs)):
        material, _, start_index, count, vmin, vmax = r.take('HH4I')
        blocks.append(dict(material=material, start=start_index, count=count, vmin=vmin, vmax=vmax))
    first = 0
    for s in subs:
        s['blocks'] = blocks[first:first + s['num_blocks']]
        first += s['num_blocks']
    if r.one('I') != check or r.pos - start != cpu_size:
        raise ChunkError('mesh config hash/size mismatch')
    return dict(version=version, gpu_size=gpu_size, num_vertices=num_vertices, stride=stride0,
                vertex_format=vertex_format, num_uv=num_uv, stride1=stride1, num_indices=num_indices,
                index_size=index_size, topology=topology, submeshes=subs)


def read_materials(r):
    r.align(16)
    names_size = r.one('I')
    names_start = r.pos
    names = {}
    for i, raw in enumerate(r.data[names_start:names_start + names_size].split(b'\0')):
        pass
    pos = names_start
    while pos < names_start + names_size:
        end = r.data.index(b'\0', pos)
        if end > pos:
            names[pos - names_start] = r.data[pos:end].decode('ascii')
        pos = end + 1
    r.pos = names_start + names_size
    r.align(16)
    _, count = r.take('2I')
    r.pos += count * 4
    r.align(16)
    r.pos += count * 4  # 8 bytes per material in the 64-bit build, 4 here
    materials = []
    if count:
        r.align(16)
    for _ in range(count):
        r.align(16)
        start = r.pos
        size, shader, name_hash, flags = r.take('4I')
        num_textures, num_constants, max_constants = r.take('HBB')
        r.take('3I')
        textures = []
        for _ in range(num_textures):
            name_offset, name_hash_t, index = r.take('3I')
            textures.append(names.get(name_offset, f'?{name_offset}'))
        constant_names = r.take(f'{num_constants}I')
        r.align(16)
        constants = [r.take('4f') for _ in range(max_constants)]
        r.align(16)
        if r.pos != start + size:
            raise ChunkError('material size mismatch')
        materials.append(dict(shader=shader, name_hash=name_hash, flags=flags, textures=textures,
                              constant_names=constant_names, constants=constants))
    return materials


def read_destroyables(b, h):
    r = Reader(b, h['destruction_offset'])
    r.align(128)
    base = r.pos
    count = r.one('i')
    r.pos += count * 4  # one runtime pointer per destroyable (8 bytes + 4 in the 64-bit build)
    out = []
    for _ in range(count):
        r.align(16)
        (aabb, objects, extra, num_objects, links_off, num_links, dlods_off, num_dlods,
         instance, transform, mass) = r.take('3Ii I i I i 2I f'.replace(' ', ''))
        r.align(128)
        subpieces = []
        for _ in range(num_objects):
            f = r.take('13f')
            dlod_key, _, material, shape, num_links_s, flags = r.take('2I4B')
            subpieces.append(dict(bmin=f[0:3], bmax=f[3:6], pos=f[6:9], com=f[9:12], mass=f[12],
                                  dlod_key=dlod_key, material=material, shape=shape,
                                  num_links=num_links_s, flags=flags))
        for s in subpieces:
            shape_offset, collision_model, render_subpiece, material_hash = r.take('IHHI')
            s.update(shape_offset=shape_offset, collision_model=collision_model,
                     render_subpiece=render_subpiece, material_hash=material_hash)
        for s in subpieces:
            s['links'] = list(r.take(f'{s["num_links"]}H'))
        r.align(4)
        links = []
        for _ in range(num_links):
            yield_max, area, a, c, flags = r.take('ifhhB3x')
            links.append(dict(yield_max=yield_max, area=area, a=a, b=c, flags=flags))
        r.align(4)
        dlods = []
        for _ in range(num_dlods):
            name_hash, *f = r.take('I12f')
            render_subpiece, first, count_pieces = r.take('HHB3x')
            dlods.append(dict(name_hash=name_hash, pos=tuple(f[0:3]), orient=tuple(f[3:12]),
                              render_subpiece=render_subpiece, first=first, count=count_pieces))
        r.align(4)
        r.pos = skip_rbb_tree(b, r.pos)
        r.align(4)
        r.take('5I')  # instance data: objects, links, dlods, size, buffer
        size = struct.unpack_from('<I', b, r.pos - 8)[0]
        r.pos += size
        r.align(16)
        transform_size = r.one('I')
        r.pos += transform_size - 4
        r.align(16)
        out.append(dict(mass=mass, subpieces=subpieces, links=links, dlods=dlods))
    r.align(64)
    if r.pos != h['collision_offset']:
        raise ChunkError(f'destroyables end at {r.pos:#x}, collision data declared at {h["collision_offset"]:#x}')
    return out


def skip_rbb_tree(b, pos):
    """Walk the AABB tree (20-byte nodes: i32 object count, 6 x i16 bounds, i32 offset).
    Count 0 = inner node: its two children (40 bytes) follow in the stream depth-first.
    Count > 0 = leaf: `count` u32 object indices follow. Returns the end of the tree."""
    cursor = pos + 20

    def visit(node):
        nonlocal cursor
        num = struct.unpack_from('<i', b, node)[0]
        if num < 0:
            return
        if num > 0:
            cursor += num * 4
            return
        children = cursor
        cursor += 40
        visit(children)
        visit(children + 20)
    visit(pos)
    return cursor


def read_tail(b, h, r):
    """Script points and object identifiers after the Havok block."""
    r.pos = h['collision_offset']
    if struct.unpack_from('<I', b, r.pos)[0] != 1212891981:
        raise ChunkError('no Havok packfile at the collision offset')
    size = struct.unpack_from('<I', b, r.pos + 8)[0]
    if size != h['collision_size']:
        raise ChunkError('Havok size disagrees with header')
    r.pos += size
    r.align(64)
    script_points = []
    for _ in range(h['num_script_points']):
        script_points.append(r.take('16I'))  # layout unverified; kept raw
    r.align(64)
    identifiers = []
    for _ in range(h['num_object_identifiers']):
        uid = r.one('I')
        name = r.data[r.pos:r.pos + 24].split(b'\0', 1)[0].decode('ascii', 'replace')
        r.pos += 24
        index, destroyable, num_snaps = r.take('i2I')
        snaps = [r.take('12f') for _ in range(10)][:num_snaps]
        identifiers.append(dict(uid=uid, name=name, object_index=index, destroyable=destroyable, snaps=snaps))
    return identifiers


def read_chunk(cpu, gpu):
    h = read_header(cpu)
    if h['render_gpu_size'] != len(gpu) and (h['render_gpu_size'] + 15) // 16 * 16 != len(gpu) and \
            (h['render_gpu_size'] + 2047) // 2048 * 2048 != len(gpu):
        raise ChunkError('gpu file size mismatch')
    r = Reader(cpu, h['render_cpu_offset'])
    mesh = read_mesh_config(r)
    materials = read_materials(r)
    r.align(64)
    general = []
    for _ in range(h['num_general_objects']):
        # 88 bytes here (96 in the 64-bit build): name offset, bmin, bmax, pos, 3x3 orient,
        # 2 collision models, flags, 2 mesh subpieces, pad.
        f = r.take('I9f9f')
        models, flags, meshes = r.take('2H'), r.one('H'), r.take('2h')
        r.pos += 2
        general.append(dict(bmin=f[1:4], bmax=f[4:7], pos=f[7:10], orient=f[10:19],
                            collision_models=models, flags=flags, mesh_subpieces=meshes))
    for g in general:
        g['name'] = r.cstr()
    destroyables = read_destroyables(cpu, h)
    identifiers = read_tail(cpu, h, Reader(cpu))
    vertices, indices = read_gpu(gpu, mesh)
    return dict(header=h, mesh=mesh, materials=materials, general=general,
                destroyables=destroyables, identifiers=identifiers,
                vertices=vertices, indices=indices)


def read_gpu(gpu, mesh):
    size = mesh['index_size']
    if size not in (2, 4):
        raise ChunkError('unverified index size')
    n = mesh['num_indices']
    indices = struct.unpack_from(f'<{n}{"H" if size == 2 else "I"}', gpu, 16)
    vstart = (16 + n * size + 15) // 16 * 16
    stride = mesh['stride']
    count = mesh['num_vertices']
    if vstart + count * stride > len(gpu):
        raise ChunkError('vertex buffer out of range')
    return gpu[vstart:vstart + count * stride], indices
