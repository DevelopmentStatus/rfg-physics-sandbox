"""Convert every extracted destructible chunk into sandbox assets.

Input:  LocalData/Extracted/Chunks (Extractors/extract_chunks.py) and misc.vpp_pc's rfg_materials.xtbl.
Output: LocalData/Assets/chunks/
  <name>.glb    one glTF mesh per render submesh ("s<i>"); fracture-surface blocks go in "s<i>_crack"
  <name>.json   structure: destroyables -> pieces (mass, centre of mass, flags, material, hull points,
                links), links (yield, area, flags), dlods; general (static) objects
  tex/<t>.png   diffuse textures, shared between chunks
  catalog.json  every converted chunk with its source archive, bounds and piece counts
  materials.json the rfg_materials.xtbl values the stress code reads

Space: game space is left-handed; Bevy's is right-handed. Positions, normals and the dlod
orientation are mirrored in Z (rotation R -> S R S with S = diag(1, 1, -1)), triangle winding reversed.

Load-time rules applied here are the game's own (E-023, `FUN_006ece30`):
  - piece material = rfg_materials entry whose lowercase CRC32 name hash equals the subpiece hash
  - piece "mass" field holds a volume; above 0.004 the game multiplies it by the material Density
  - material flag `shatter` sets piece flags 0x42; a material without `structural` sets piece flag 0x08
  - a link between pieces whose 0x40 flag differs starts broken (link flag 0x04)
Usage: python Extractors/build_chunks.py [name ...]
"""
import json
import re
import struct
import sys
import zlib
from pathlib import Path

import numpy as np
from PIL import Image
from scipy.spatial import ConvexHull, QhullError

sys.path.insert(0, str(Path(__file__).parent))
from asset_status import read_json  # noqa: E402
import rfg_chunk  # noqa: E402
import rfg_peg  # noqa: E402

PROJECT = Path(__file__).resolve().parents[1]
SOURCE = PROJECT / 'LocalData/Extracted/Chunks'
OUT = PROJECT / 'LocalData/Assets/chunks'
MATERIAL_FLAGS = ('shatter', 'no_material_pickup', 'indestructible', 'bullet_damage', 'melee_damage',
                  'structural', 'flammable', 'transparent', 'electrifiable', 'crushable', 'allow_jitter',
                  'allow_regrow')  # order of the Flags element in rfg_materials.xtbl's TableDescription
VOLUME_TO_MASS_MIN = 0.004  # f32 at 0x00b4cff8
CRACK_SHADER = 0x0088120A   # fracture-surface shader (crack_alpha.tga); its geometry overhangs the piece
HULL_POINTS = 48


def shader_hash(name):
    return zlib.crc32(name.lower().encode(), 0xFFFFFFFF) ^ 0xFFFFFFFF  # FUN_00a03310


# Render material `shader` = name hash of the effect (`rfg-*.fxo_kg`). Alpha-blended glass: the
# `rfg-glass` pixel shader outputs alpha = Diffuse_Map.a (x Tint_color.a) and colour = ambient x
# Diffuse_Map x Diffuse_Color + specular (E-031). `rfg-eosglass` (tower glass) writes opaque colour.
BLENDED_GLASS = {shader_hash('rfg-glass'), shader_hash('rfg-glass-etched')}
CONSTANT_NAMES = {shader_hash(n): n for n in ('Diffuse_Color', 'Specular_Color', 'Specular_Alpha', 'Specular_Power')}


def glass_material(m, mat):
    """Applies the blended-glass shader's look to a glTF material dict (in place)."""
    values = {CONSTANT_NAMES.get(h): c for h, c in zip(m['constant_names'], m['constants'])}
    diffuse = values.get('Diffuse_Color', (1.0, 1.0, 1.0, 1.0))
    power = values.get('Specular_Power', (100.0,))[0]
    pbr = mat.setdefault('pbrMetallicRoughness', {})
    pbr['baseColorFactor'] = [float(diffuse[0]), float(diffuse[1]), float(diffuse[2]), 1.0]
    # Blinn-Phong exponent -> GGX roughness (approximation of the game's specular lobe).
    pbr['roughnessFactor'] = float(min(1.0, max(0.05, (2.0 / (power + 2.0)) ** 0.25)))
    pbr['metallicFactor'] = 0.0
    mat['alphaMode'] = 'BLEND'


def name_hash(text):
    """FUN_00a03310: CRC32 table, zero seed, no final xor, over the lowercased string."""
    return zlib.crc32(text.lower().encode(), 0xFFFFFFFF) ^ 0xFFFFFFFF


def read_materials():
    text = (PROJECT / 'LocalData/Extracted/MiscV3/rfg_materials.xtbl').read_text()
    table = text[:text.index('<TableTemplates>')]
    out = []
    for block in re.findall(r'<Material>(.*?)</Material>', table, re.S):
        def field(tag, default):
            m = re.search(rf'<{tag}>([^<]*)</{tag}>', block)
            return type(default)(m.group(1)) if m else default
        flags = re.findall(r'<Flag>([^<]+)</Flag>', block)
        out.append(dict(name=field('Name', ''), density=field('Density', 0.0),
                        link_strength=field('LinkStrength', 0) * 1000,  # FUN_006dcd90 stores x1000
                        brittleness=field('Brittleness', 1.0), restitution=field('Restitution', 0.05),
                        friction=field('Friction', 1.0),
                        flags=sum(1 << MATERIAL_FLAGS.index(f) for f in flags if f in MATERIAL_FLAGS)))
    return out


class Textures:
    """Diffuse lookup across every extracted texture bank; decoded once to tex/<name>.png."""

    def __init__(self):
        self.where = {}
        for cpu in sorted((SOURCE / 'textures').glob('*.c*_pc')):
            if cpu.suffix not in ('.cvbm_pc', '.cpeg_pc'):
                continue
            data = cpu.read_bytes()
            count = struct.unpack_from('<I', data, 0x10)[0]
            names = data[0x18 + 0x30 * count:].split(b'\0')
            for i in range(count):
                self.where.setdefault(names[i].decode('ascii', 'replace').lower(), (cpu, i))
        self.done = {}

    def png(self, tga):
        key = tga.lower()
        if key in self.done:
            return self.done[key]
        result = None
        if key in self.where:
            cpu, index = self.where[key]
            gpu = cpu.with_name(cpu.name.replace('.cvbm_pc', '.gvbm_pc').replace('.cpeg_pc', '.gpeg_pc'))
            try:
                t = read_bank(cpu.read_bytes(), gpu.read_bytes())[index]
                image = rfg_peg.decode(t)
                stem = Path(key).stem
                Image.fromarray(image).save(OUT / 'tex' / f'{stem}.png')
                result = f'tex/{stem}.png'
            except (ValueError, OSError) as error:
                print(f'  texture {tga}: {error}')
        self.done[key] = result
        return result


def read_bank(cc, gg):
    count = struct.unpack_from('<I', cc, 0x10)[0]
    names = cc[0x18 + 0x30 * count:].split(b'\0')
    out = []
    for i in range(count):
        b = 0x18 + 0x30 * i
        off, w, h, pixel = struct.unpack_from('<IHHH', cc, b)
        size = struct.unpack_from('<I', cc, b + 0x1c)[0]
        out.append(dict(name=names[i].decode(), w=w, h=h, format=pixel, data=gg[off:off + size]))
    return out


def pick_diffuse(textures):
    """Material texture lists mix diffuse/normal/specular/decal maps; the base map is the first `_d`
    that is not a decal (`dcl_`), else the first texture without a normal/specular suffix."""
    names = [t for t in textures if not t.startswith('?')]
    for t in names:
        s = t.lower()
        if s.endswith('_d.tga') and not s.startswith('dcl_'):
            return t
    for t in names:
        s = t.lower()
        if not s.endswith(('_n.tga', '_s.tga', 'alpha.tga')) and not s.startswith('dcl_') and s != 'missing.tga':
            return t
    return None


def mirror_rows(rows):
    """Game row-major orientation (rows = right, up, forward) -> Bevy column-major quaternion input."""
    r = np.array(rows, dtype=np.float64).reshape(3, 3)
    s = np.diag([1.0, 1.0, -1.0])
    m = s @ r.T @ s  # columns are the mirrored basis vectors
    return m


def quat_from_matrix(m):
    u, _, vt = np.linalg.svd(m)  # re-orthonormalise (stored rows carry float noise)
    m = u @ vt
    if np.linalg.det(m) < 0:
        m = -m
    t = np.trace(m)
    if t > 0:
        s = np.sqrt(t + 1.0) * 2
        q = [(m[2, 1] - m[1, 2]) / s, (m[0, 2] - m[2, 0]) / s, (m[1, 0] - m[0, 1]) / s, 0.25 * s]
    elif m[0, 0] > m[1, 1] and m[0, 0] > m[2, 2]:
        s = np.sqrt(1.0 + m[0, 0] - m[1, 1] - m[2, 2]) * 2
        q = [0.25 * s, (m[0, 1] + m[1, 0]) / s, (m[0, 2] + m[2, 0]) / s, (m[2, 1] - m[1, 2]) / s]
    elif m[1, 1] > m[2, 2]:
        s = np.sqrt(1.0 + m[1, 1] - m[0, 0] - m[2, 2]) * 2
        q = [(m[0, 1] + m[1, 0]) / s, 0.25 * s, (m[1, 2] + m[2, 1]) / s, (m[0, 2] - m[2, 0]) / s]
    else:
        s = np.sqrt(1.0 + m[2, 2] - m[0, 0] - m[1, 1]) * 2
        q = [(m[0, 2] + m[2, 0]) / s, (m[1, 2] + m[2, 1]) / s, 0.25 * s, (m[1, 0] - m[0, 1]) / s]
    return [round(float(v), 6) for v in q]


def fz(v):
    return [round(float(v[0]), 5), round(float(v[1]), 5), round(float(-v[2]), 5)]


def strip_to_list(strip):
    tris = []
    for i in range(len(strip) - 2):
        a, b, c = strip[i], strip[i + 1], strip[i + 2]
        if a == b or b == c or a == c:
            continue
        if i % 2:
            b, c = c, b
        tris += [a, c, b]  # reversed for the Z mirror
    return tris


class Glb:
    def __init__(self):
        self.bin = bytearray()
        self.doc = dict(asset=dict(version='2.0', generator='BevyRFGPhysics build_chunks'),
                        buffers=[], bufferViews=[], accessors=[], meshes=[], materials=[],
                        images=[], textures=[], samplers=[dict(magFilter=9729, minFilter=9987,
                                                               wrapS=10497, wrapT=10497)],
                        nodes=[], scenes=[dict(nodes=[])], scene=0)

    def accessor(self, array, kind, target, minmax=False):
        array = np.ascontiguousarray(array)
        while len(self.bin) % 4:
            self.bin.append(0)
        view = dict(buffer=0, byteOffset=len(self.bin), byteLength=array.nbytes, target=target)
        self.bin += array.tobytes()
        self.doc['bufferViews'].append(view)
        component = {np.dtype('float32'): 5126, np.dtype('uint32'): 5125, np.dtype('uint16'): 5123}[array.dtype]
        acc = dict(bufferView=len(self.doc['bufferViews']) - 1, componentType=component,
                   count=len(array), type=kind)
        if minmax:
            acc['min'] = array.min(0).tolist()
            acc['max'] = array.max(0).tolist()
        self.doc['accessors'].append(acc)
        return len(self.doc['accessors']) - 1

    def bytes(self):
        self.doc['buffers'] = [dict(byteLength=len(self.bin))]
        if not self.doc['images']:
            for key in ('images', 'textures', 'samplers'):
                self.doc.pop(key)
        js = json.dumps(self.doc, separators=(',', ':')).encode()
        js += b' ' * (-len(js) % 4)
        while len(self.bin) % 4:
            self.bin.append(0)
        total = 12 + 8 + len(js) + 8 + len(self.bin)
        return (struct.pack('<III', 0x46546C67, 2, total) + struct.pack('<II', len(js), 0x4E4F534A) + js +
                struct.pack('<II', len(self.bin), 0x004E4942) + bytes(self.bin))


def convert(name, materials_table, textures):
    cpu = (SOURCE / 'chunks' / f'{name}.cchk_pc').read_bytes()
    gpu = (SOURCE / 'chunks' / f'{name}.gchk_pc').read_bytes()
    x = rfg_chunk.read_chunk(cpu, gpu)
    mesh = x['mesh']
    stride = mesh['stride']
    raw = np.frombuffer(x['vertices'], np.uint8).reshape(-1, stride)
    pos = raw[:, :12].copy().view('<f4').reshape(-1, 3).astype(np.float32)
    nrm = raw[:, 12:15].astype(np.float32) / 127.5 - 1
    uv = raw[:, 20:24].copy().view('<i2').reshape(-1, 2).astype(np.float32) / 1024.0
    pos_b = pos * np.array([1, 1, -1], np.float32)
    nrm_b = nrm * np.array([1, 1, -1], np.float32)
    nrm_b /= np.maximum(np.linalg.norm(nrm_b, axis=1, keepdims=True), 1e-6)
    uv_b = np.stack([uv[:, 0], 1.0 + uv[:, 1]], 1).astype(np.float32)  # glTF V = 1 + stored V (E-020)
    indices = np.array(x['indices'], dtype=np.int64)

    glb = Glb()
    crack_materials = set()
    for i, m in enumerate(x['materials']):
        if m['shader'] == CRACK_SHADER:
            crack_materials.add(i)
        tex = pick_diffuse(m['textures'])
        uri = textures.png(tex) if tex else None
        mat = dict(name=f'm{i}', doubleSided=False,
                   pbrMetallicRoughness=dict(metallicFactor=0.0, roughnessFactor=0.9))
        if uri:
            glb.doc['images'].append(dict(uri=uri))
            glb.doc['textures'].append(dict(sampler=0, source=len(glb.doc['images']) - 1))
            mat['pbrMetallicRoughness']['baseColorTexture'] = dict(index=len(glb.doc['textures']) - 1)
        else:
            mat['pbrMetallicRoughness']['baseColorFactor'] = [0.55, 0.53, 0.5, 1.0]
        if i in crack_materials:
            mat['alphaMode'] = 'MASK'
        elif m['shader'] in BLENDED_GLASS:
            glass_material(m, mat)
        glb.doc['materials'].append(mat)
    if not glb.doc['materials']:
        glb.doc['materials'].append(dict(name='default'))

    mesh_index = {}  # submesh -> (gltf mesh, crack gltf mesh or None, hull points)

    def submesh(si):
        if si in mesh_index:
            return mesh_index[si]
        sub = mesh['submeshes'][si]
        parts = {False: [], True: []}
        solid = []
        for block in sub['blocks']:
            strip = indices[block['start']:block['start'] + block['count']]
            if mesh['topology'] == 0:
                tris = strip_to_list(list(strip))
            else:
                tris = list(strip.reshape(-1, 3)[:, [0, 2, 1]].ravel())
            if not tris:
                continue
            crack = block['material'] in crack_materials
            parts[crack].append((block['material'], tris))
            if not crack:
                solid += tris
        out = []
        for crack in (False, True):
            if not parts[crack]:
                out.append(None)
                continue
            used = np.unique(np.concatenate([np.array(t) for _, t in parts[crack]]))
            remap = np.full(len(pos_b), -1, np.int64)
            remap[used] = np.arange(len(used))
            a_pos = glb.accessor(pos_b[used], 'VEC3', 34962, minmax=True)
            a_nrm = glb.accessor(nrm_b[used], 'VEC3', 34962)
            a_uv = glb.accessor(uv_b[used], 'VEC2', 34962)
            prims = []
            for material, tris in parts[crack]:
                idx = remap[np.array(tris)].astype(np.uint32 if len(used) > 65535 else np.uint16)
                prims.append(dict(attributes=dict(POSITION=a_pos, NORMAL=a_nrm, TEXCOORD_0=a_uv),
                                  indices=glb.accessor(idx, 'SCALAR', 34963),
                                  material=min(material, len(glb.doc['materials']) - 1)))
            glb.doc['meshes'].append(dict(name=f's{si}' + ('_crack' if crack else ''), primitives=prims))
            out.append(len(glb.doc['meshes']) - 1)
        hull = []
        if solid:
            pts = pos_b[np.unique(np.array(solid))].astype(np.float64)
            hull = hull_points(pts)
        mesh_index[si] = (out[0], out[1], hull)
        return mesh_index[si]

    mat_hash = {name_hash(m['name']): i for i, m in enumerate(materials_table)}
    default_material = next(i for i, m in enumerate(materials_table) if m['name'] == 'DEFAULT')
    destroyables = []
    for di, d in enumerate(x['destroyables']):
        ident = next((idf['name'] for idf in x['identifiers']
                      if idf['destroyable'] == 1 and idf['object_index'] == di), f'destroyable_{di}')
        owner = {}
        for li, q in enumerate(d['dlods']):
            for k in range(q['first'], q['first'] + q['count']):
                owner[k] = li
        dlods = [dict(pos=fz(q['pos']), rot=quat_from_matrix(mirror_rows(q['orient'])),
                      mesh=submesh(q['render_subpiece'])[0]) for q in d['dlods']]
        pieces = []
        for k, s in enumerate(d['subpieces']):
            material = mat_hash.get(s['material_hash'], default_material)
            mdef = materials_table[material]
            flags = s['flags']
            volume = s['mass']
            mass = mdef['density'] * volume if volume > VOLUME_TO_MASS_MIN else volume
            if mdef['flags'] & 1:
                flags |= 0x42
            if not mdef['flags'] & (1 << 5):
                flags |= 0x08
            render, crack, hull = submesh(s['render_subpiece'])
            pieces.append(dict(dlod=owner.get(k, -1), mesh=render, crack=crack, hull=hull,
                               pos=fz(s['pos']), com=fz(s['com']),
                               bmin=fz((s['bmin'][0], s['bmin'][1], s['bmax'][2])),
                               bmax=fz((s['bmax'][0], s['bmax'][1], s['bmin'][2])),
                               volume=round(volume, 6), mass=round(mass, 4), material=material,
                               flags=flags, links=s['links']))
        links = []
        for link in d['links']:
            flags = link['flags']
            if (pieces[link['a']]['flags'] ^ pieces[link['b']]['flags']) & 0x40:
                flags |= 4
            links.append([link['a'], link['b'], link['yield_max'], round(link['area'], 5), flags])
        destroyables.append(dict(name=ident, mass=d['mass'], dlods=dlods, pieces=pieces, links=links))

    general = []
    for g in x['general']:
        # Non-destructible parts (machinery bodies, motors): mesh_subpieces[0] is the render submesh.
        si = g['mesh_subpieces'][0]
        render, crack, hull = submesh(si) if 0 <= si < len(mesh['submeshes']) else (None, None, [])
        general.append(dict(name=g['name'], pos=fz(g['pos']), rot=quat_from_matrix(mirror_rows(g['orient'])),
                            mesh=render, hull=hull))

    # Every mesh is referenced by entities the sandbox creates; one scene node per mesh keeps Bevy's
    # glTF loader happy and lets the file be previewed in other viewers.
    for mi in range(len(glb.doc['meshes'])):
        glb.doc['nodes'].append(dict(mesh=mi))
        glb.doc['scenes'][0]['nodes'].append(mi)
    (OUT / f'{name}.glb').write_bytes(glb.bytes())

    all_pts = [np.array(p['pos']) for d in destroyables for p in d['pieces']]
    lo = np.min([d for d in all_pts], 0) if all_pts else np.zeros(3)
    hi = np.max([d for d in all_pts], 0) if all_pts else np.zeros(3)
    bmin = np.min([p['bmin'] for d in destroyables for p in d['pieces']], 0) if all_pts else lo
    bmax = np.max([p['bmax'] for d in destroyables for p in d['pieces']], 0) if all_pts else hi
    doc = dict(schema=1, name=name, destroyables=destroyables, general=general,
               bmin=[round(float(v), 3) for v in bmin], bmax=[round(float(v), 3) for v in bmax])
    (OUT / f'{name}.json').write_text(json.dumps(doc, separators=(',', ':')))
    return dict(name=name, bmin=doc['bmin'], bmax=doc['bmax'], destroyables=len(destroyables),
                pieces=sum(len(d['pieces']) for d in destroyables),
                links=sum(len(d['links']) for d in destroyables), general=len(general))


def hull_points(pts):
    """At most HULL_POINTS vertices of the convex hull (the collider the sandbox builds)."""
    if len(pts) < 4:
        return [[round(float(v), 4) for v in p] for p in pts]
    try:
        hull = ConvexHull(pts)
        verts = pts[hull.vertices]
    except QhullError:
        verts = pts
    if len(verts) > HULL_POINTS:
        # Keep the extreme points in evenly spread directions so thin walls stay thin.
        dirs = fibonacci(HULL_POINTS)
        keep = np.unique(np.argmax(verts @ dirs.T, axis=0))
        verts = verts[keep]
    return [[round(float(v), 4) for v in p] for p in verts]


def fibonacci(n):
    i = np.arange(n) + 0.5
    phi = np.arccos(1 - 2 * i / n)
    theta = np.pi * (1 + 5 ** 0.5) * i
    return np.stack([np.cos(theta) * np.sin(phi), np.cos(phi), np.sin(theta) * np.sin(phi)], 1)


def main():
    assert OUT.resolve().is_relative_to(PROJECT / 'LocalData')
    (OUT / 'tex').mkdir(parents=True, exist_ok=True)
    index = json.loads((SOURCE / 'index.json').read_text())
    materials_table = read_materials()
    (OUT / 'materials.json').write_text(json.dumps(materials_table, indent=1))
    textures = Textures()
    names = sys.argv[1:] or sorted(n[:-8] for n in index['chunks'] if n.endswith('.cchk_pc'))
    catalog_path = OUT / 'catalog.json'
    previous = read_json(catalog_path)
    catalog = {c['name']: c for c in previous if isinstance(c, dict) and isinstance(c.get('name'), str)} if isinstance(previous, list) else {}
    failed = []
    for name in names:
        try:
            entry = convert(name, materials_table, textures)
        except Exception as error:  # Save successful conversions, but fail the overall step.
            failed.append(name)
            catalog.pop(name, None)
            print(f'{name}: FAILED {error!r}')
            continue
        entry['source'] = index['chunks'][f'{name}.cchk_pc']['source']
        catalog[name] = entry
        print(f"{name}: {entry['destroyables']} destroyables, {entry['pieces']} pieces, {entry['links']} links")
    catalog_path.write_text(json.dumps(sorted(catalog.values(), key=lambda c: c['name']), indent=0))
    if failed:
        sys.exit('Chunk conversion failed: ' + ', '.join(failed))


if __name__ == '__main__':
    main()
