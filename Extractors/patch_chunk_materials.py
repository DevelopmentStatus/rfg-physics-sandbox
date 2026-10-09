"""Re-applies build_chunks' material rules (blended glass) to already built chunk GLBs without
rebuilding geometry. Rewrites only the GLB JSON chunk.
Usage: python Extractors/patch_chunk_materials.py [chunk names...]
"""
import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import build_chunks as bc  # noqa: E402
import rfg_chunk  # noqa: E402


def rewrite(path, materials):
    data = path.read_bytes()
    magic, version, _ = struct.unpack_from('<4sII', data, 0)
    assert magic == b'glTF' and version == 2
    json_len, json_type = struct.unpack_from('<II', data, 12)
    assert json_type == 0x4E4F534A
    doc = json.loads(data[20:20 + json_len])
    rest = data[20 + json_len:]
    changed = 0
    for mat in doc.get('materials', []):
        name = mat.get('name', '')
        if not name.startswith('m'):
            continue
        i = int(name[1:])
        if i < len(materials) and materials[i]['shader'] in bc.BLENDED_GLASS and mat.get('alphaMode') != 'MASK':
            bc.glass_material(materials[i], mat)
            changed += 1
    if not changed:
        return 0
    text = json.dumps(doc, separators=(',', ':')).encode()
    text += b' ' * (-len(text) % 4)
    out = struct.pack('<4sII', b'glTF', 2, 12 + 8 + len(text) + len(rest)) + struct.pack('<II', len(text), 0x4E4F534A) + text + rest
    path.write_bytes(out)
    return changed


def main():
    names = sys.argv[1:] or [p.stem for p in sorted(bc.OUT.glob('*.glb'))]
    total = 0
    for name in names:
        cpu = bc.SOURCE / 'chunks' / f'{name}.cchk_pc'
        gpu = cpu.with_suffix('.gchk_pc')
        glb = bc.OUT / f'{name}.glb'
        if not (cpu.exists() and gpu.exists() and glb.exists()):
            continue
        x = rfg_chunk.read_chunk(cpu.read_bytes(), gpu.read_bytes())
        n = rewrite(glb, x['materials'])
        if n:
            total += 1
            print(f'{name}: {n} glass materials')
    print(f'{total} chunks patched')


if __name__ == '__main__':
    main()
