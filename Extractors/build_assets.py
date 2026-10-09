"""Build the sandbox's runtime assets from the user's own extracted game data.

Writes into LocalData/Assets (never into the source tree):
  mason.glb            skinned Mason body, coat and head, textured, with animation clips (E-008..E-020)
  weapons/<key>.glb    static weapon meshes with their high-res diffuse texture (E-014)
  weapons.json         per weapon: file, holding hand bone, grip (root relative to the hand bone),
                       prop-point tags (mesh space), and mesh bounds
Usage: python Extractors/build_assets.py
Prerequisites (see Research/player_asset_inventory.md): LocalData/Extracted/{Player,MiscV3,Items}.
"""
import base64
import io
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
import archive_v3  # noqa: E402
import rfg_anim  # noqa: E402
import rfg_model  # noqa: E402
import rfg_peg  # noqa: E402

PROJECT = Path(__file__).resolve().parents[1]
EXTRACTED = PROJECT / 'LocalData/Extracted'
OUT = PROJECT / 'LocalData/Assets'

from weapon_specs import WEAPONS


SAMPLER = dict(magFilter=9729, minFilter=9987, wrapS=10497, wrapT=10497)


def png_uri(img):
    buf = io.BytesIO()
    Image.fromarray(img).save(buf, 'PNG')
    return 'data:image/png;base64,' + base64.b64encode(buf.getvalue()).decode()


def textured_material(img):
    return dict(pbrMetallicRoughness=dict(baseColorTexture=dict(index=0), metallicFactor=0.0, roughnessFactor=0.85),
                alphaMode='OPAQUE', doubleSided=True), dict(images=[dict(uri=png_uri(img))],
                                                           textures=[dict(sampler=0, source=0)], samplers=[SAMPLER])


def str2_members(path):
    entries, _ = archive_v3.decode(path.read_bytes())
    return {name: value for name, value, _ in entries}


def diffuse(bank_members, bank):
    """Decode the *_d texture of a `<bank>.cpeg_pc/.gpeg_pc` pair."""
    pair = {n.rsplit('.', 1)[0]: n for n in bank_members}
    stem = next(s for s in bank_members if s.endswith('.cpeg_pc')).rsplit('.', 1)[0]
    tmp = PROJECT / 'LocalData/Scratch' / f'peg_{stem}'
    tmp.mkdir(parents=True, exist_ok=True)
    (tmp / f'{stem}.cpeg_pc').write_bytes(bank_members[f'{stem}.cpeg_pc'])
    (tmp / f'{stem}.gpeg_pc').write_bytes(bank_members[f'{stem}.gpeg_pc'])
    for t in rfg_peg.read_peg(str(tmp / stem)):
        if t['name'].lower().endswith('_d.tga'):
            return rfg_peg.dxt1(t['data'], t['w'], t['h'])
    raise ValueError(f'No diffuse texture in {bank}')


def build_mason():
    base = EXTRACTED / 'Player/rfg_PC/mason'
    bones = rfg_model.read_rig((EXTRACTED / 'MiscV3/mason.rig_pc').read_bytes())
    mesh = rfg_model.read_mesh(Path(f'{base}.ccmesh_pc').read_bytes(), Path(f'{base}.gcmesh_pc').read_bytes())
    tex = {t['name']: t for t in rfg_peg.read_peg(str(base))}
    d1, d2 = (rfg_peg.dxt1(tex[f'mason_body{i}_d.tga']['data'], tex[f'mason_body{i}_d.tga']['w'],
                           tex[f'mason_body{i}_d.tga']['h']) for i in (1, 2))
    # Submeshes 0/1 sample the body1 atlas, 2/3 the body2 atlas (UV ranges, E-013).
    for i, sub in enumerate(mesh['submeshes']):
        sub['gltf_material'] = 0 if i < 2 else 1
    # Clips only animate the 53 body bones; compute them before the coat bones are added.
    clips = player_clips(bones)
    # The coat (worn by default, E-010) has its own 64-bone rig: 18 body bones plus
    # 46 cloth bones (coat flaps, scarf). The cloth bones join the skeleton and follow
    # their parents rigidly; the game's cloth simulation (mason_coat.sim_pc) is not ported.
    coat = rfg_model.read_mesh(Path(f'{base}_coat.ccmesh_pc').read_bytes(), Path(f'{base}_coat.gcmesh_pc').read_bytes())
    remap = rfg_model.merge_part_rig(bones, rfg_model.read_part_rig((EXTRACTED / 'MiscV3/mason_coat.rig_pc').read_bytes()))
    for v in coat['vertices']:
        v['j'] = tuple(remap[j] if w > 0 else 0 for j, w in zip(v['j'], v['w']))
    # Coat submesh 0 (jacket) samples the body1 atlas, submesh 1 (scarf) the red cloth of
    # body2: their UV wireframes fit those atlases exactly (E-020).
    for sub in coat['submeshes']:
        sub['gltf_material'] = sub['material']
    head, head_images = mason_head(bones)
    images = [d1, d2] + head_images
    mats = [dict(pbrMetallicRoughness=dict(baseColorTexture=dict(index=k), metallicFactor=0.0, roughnessFactor=0.85),
                 doubleSided=True) for k in range(len(images))]
    mats[4].update(alphaMode='MASK', alphaCutoff=0.5)  # eyelash cards
    extra = dict(materials=mats, images=[dict(uri=png_uri(img)) for img in images],
                 textures=[dict(sampler=0, source=k) for k in range(len(images))], samplers=[SAMPLER],
                 clips=clips)
    parts = [('mason', mesh), ('mason_coat', coat), ('mason_head', head)]
    return rfg_model.build_glb(parts, bones, extra=extra, skinned=True), bones


def mason_head(bones):
    """Mason's head (character.xtbl: Name Mason_head, Base_Head base_hires_head).

    base_hires_head.str2_pc holds the mesh (submeshes: face, eyelash strip, eyeballs) and a
    .morph_pc (facial morph targets, not decoded). Its 5-bone rig (spine2, neck, head, l-eye,
    r-eye) names bones the body already has, so the head binds onto the body skeleton by name.
    Textures: mason_head.cpeg_pc (face) and always_loaded.cpeg_pc (eyes, shared eyelashes),
    matching the names in mason_head.mat_pc. Returns the mesh and images 2..4 of mason.glb."""
    members = str2_members(EXTRACTED / 'Items/base_hires_head.str2_pc')
    head = rfg_model.read_mesh(members['base_hires_head.ccmesh_pc'], members['base_hires_head.gcmesh_pc'])
    count = len(bones)
    remap = rfg_model.merge_part_rig(bones, rfg_model.read_part_rig((EXTRACTED / 'MiscV3/mason_head.rig_pc').read_bytes()))
    assert len(bones) == count, 'head rig names a bone the body lacks'
    for v in head['vertices']:
        v['j'] = tuple(remap[j] if w > 0 else 0 for j, w in zip(v['j'], v['w']))
    for sub in head['submeshes']:  # face, eyelashes, eyes (by skinning: the eyeballs follow l-eye/r-eye)
        sub['gltf_material'] = {0: 2, 1: 4, 2: 3}[sub['material']]
    textures = {t['name']: t for t in rfg_peg.read_peg(str(EXTRACTED / 'Player/rfg_PC/mason_head'))}
    textures.update({t['name']: t for t in rfg_peg.read_peg(str(EXTRACTED / 'MiscV3/always_loaded'))})
    names = ('mason_face_d.tga', 'mason_eye_d.tga', 'voice_eyelashes_d.tga')
    return head, [rfg_peg.decode(textures[n]) for n in names]


# Animation sets baked into mason.glb, by clip-name prefix (player.xtbl groups:
# Hammer = hmr, Rifle = rfl, remote charge = rmtchrg).
CLIP_PREFIXES = ('plyr_hmr_', 'plyr_rfl_', 'plyr_rmtchrg_')


def player_clips(bones):
    clips = []
    for path in sorted((EXTRACTED / 'AnimsV3').glob('plyr_*.anim_pc')):
        name = path.stem
        if not name.startswith(CLIP_PREFIXES):
            continue
        try:
            clip = rfg_anim.parse(path.read_bytes())
        except rfg_anim.AnimError as error:
            print(f'{name}: skipped ({error})')
            continue
        clips.append((name, rfg_anim.gltf_channels(clip, bones)))
    print(f'{len(clips)} animation clips')
    return clips


def quat_xyzw(r):
    """Rotation matrix (column vectors) -> unit quaternion [x, y, z, w]."""
    w = np.sqrt(max(0.0, 1 + r[0, 0] + r[1, 1] + r[2, 2])) / 2
    x = np.copysign(np.sqrt(max(0.0, 1 + r[0, 0] - r[1, 1] - r[2, 2])) / 2, r[2, 1] - r[1, 2])
    y = np.copysign(np.sqrt(max(0.0, 1 - r[0, 0] + r[1, 1] - r[2, 2])) / 2, r[0, 2] - r[2, 0])
    z = np.copysign(np.sqrt(max(0.0, 1 - r[0, 0] - r[1, 1] + r[2, 2])) / 2, r[1, 0] - r[0, 1])
    q = np.array([x, y, z, w])
    return (q / np.linalg.norm(q)).round(6).tolist()


def weapon_grip(hand_tag, tags):
    """Weapon root relative to the hand-prop bone: the weapon's `weapon_handle` tag is placed on
    the character's hand tag (`FUN_007c5e80` -> `FUN_00774120`), so root = hand_tag * handle^-1 (E-029)."""
    t_hand, r_hand = rfg_model.tag_to_bevy(hand_tag)
    handle = next((t for t in tags if t['name'] == 'weapon_handle'), None)
    t_handle, r_handle = rfg_model.tag_to_bevy(handle) if handle else (np.zeros(3), np.eye(3))
    r = r_hand @ r_handle.T
    t = t_hand - r @ t_handle
    return dict(translation=t.round(6).tolist(), rotation=quat_xyzw(r), from_tag=bool(handle))


def build_weapon(key, spec, hand_tags):
    archive, stem, bank, hand = spec
    members = str2_members(EXTRACTED / f'Items/{archive}.str2_pc')
    tags = []
    if f'{stem}.csmesh_pc' in members:
        mesh = rfg_model.read_static_mesh(members[f'{stem}.csmesh_pc'], members[f'{stem}.gsmesh_pc'])
        tags = rfg_model.read_mesh_tags(members[f'{stem}.csmesh_pc'])
    else:  # skinned weapon mesh: keep the bind pose, drop the skinning
        mesh = rfg_model.read_mesh(members[f'{stem}.ccmesh_pc'], members[f'{stem}.gcmesh_pc'])
    image = diffuse(str2_members(EXTRACTED / f'Items/{bank}.str2_pc'), bank)
    material, extra = textured_material(image)
    extra['materials'] = [material]
    glb = rfg_model.build_glb([(key, mesh)], [], extra=extra, skinned=False)
    xs = np.array([rfg_model.flipz(v['p']) for v in mesh['vertices']])
    points = {t['name']: rfg_model.tag_to_bevy(t)[0].round(5).tolist() for t in tags}
    return glb, dict(file=f'weapons/{key}.glb', hand=hand, grip=weapon_grip(hand_tags[hand], tags),
                     tags=points, min=xs.min(0).round(4).tolist(),
                     max=xs.max(0).round(4).tolist(), triangles=len(mesh['indices']) - 2 * len(mesh['submeshes']))


def main():
    assert OUT.resolve().is_relative_to(PROJECT / 'LocalData')
    (OUT / 'weapons').mkdir(parents=True, exist_ok=True)
    glb, bones = build_mason()
    (OUT / 'mason.glb').write_bytes(glb)
    print('mason.glb', len(glb), 'bytes,', len(bones), 'bones')
    rig = (EXTRACTED / 'MiscV3/mason.rig_pc').read_bytes()
    hand_tags = {t['parent']: t for t in rfg_model.read_rig_tags(rig, rfg_model.read_rig(rig))
                 if t['name'] in ('l-hand', 'r-hand')}
    catalog = {}
    failed = []
    for key, spec in WEAPONS.items():
        try:
            glb, info = build_weapon(key, spec, hand_tags)
        except Exception as error:  # Save successful conversions, but fail the overall step.
            failed.append(key)
            print(f'{key}: FAILED {error!r}')
            continue
        (OUT / info['file']).write_bytes(glb)
        catalog[key] = info
        print(f'{key}: {info["file"]} {len(glb)} bytes bounds {info["min"]}..{info["max"]}')
    for info in catalog.values():  # some meshes carry NaN vertices; keep the file valid JSON
        for k in ('min', 'max'):
            if any(v != v for v in info[k]):
                info[k] = None
    (OUT / 'weapons.json').write_text(json.dumps(catalog, indent=1, allow_nan=False))
    if failed:
        sys.exit('Weapon conversion failed: ' + ', '.join(failed))


if __name__ == '__main__':
    main()
