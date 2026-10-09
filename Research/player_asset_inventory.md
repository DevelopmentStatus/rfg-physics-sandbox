# Campaign player (Alec Mason): data inventory

Compiled 2026-10-07 before building the character in Bevy. Everything below was found in
the reference install (`RFG.exe` SHA256 `0606BBED...6C72D`); game-derived files stay under
`LocalData/` and are not published. Status: DONE = decoded and verified, PARTIAL = located
and partly understood, TODO = located, format not yet decoded, GAP = not yet found.

## 1. Identity (verified, `misc.vpp_pc` -> `character.xtbl`)

The campaign player is the `rfg_PC` entry (the multiplayer Mason is `mp_guerilla_engineer_mason`).

| Field | Value |
|---|---|
| Model | `Mason.cmeshx` -> `mason.ccmesh_pc` / `mason.gcmesh_pc` |
| Rig | `Mason.rigx` -> `mason.rig_pc` (NOT `player.rig_pc`) |
| Anim set | `Player.xtbl` (`player.xtbl`, group per weapon: `Hammer` has 117 entries) |
| Start weapon | `Equip_Item_Name = sledgehammer`; `Last_Equip_Item_Name = charge_placer` |
| Stats | `max_hit_points 550`, `knock_down_hits 30`, `max_speed 5`, `Height 1.0`, `AimErrorPct 100` |
| Footsteps | `GUR_PC` |
| Props | `Torso` = `Mason_Coat` (100% chance, worn by default); `Backpack` (4 jetpack-type items) 0%; `Armor L1` = `Mason_Armor_L1` 0%; `Armor L2` = `Mason_Armor_L2` 0% |
| Head | `Base_Head base_hires_head`, `Name Mason_head` |

The default look is body + coat + head. The armor and backpacks are optional upgrades, so the
earlier sandbox test that drew `mason_armor_l1` was not the default appearance.

## 2. Mesh and skeleton assets

| Part | Files | Where | Status |
|---|---|---|---|
| Body (legs, hands) | `mason.ccmesh_pc` + `mason.gcmesh_pc` | `humans.vpp_pc` -> `rfg_PC.str2_pc` | DONE: 2703 vertices, 4 submeshes (triangle strips), 32-byte vertices (E-008) |
| Skeleton | `mason.rig_pc` (4006 B, 53 bones) | `misc.vpp_pc` | DONE for bone names, parents and bind positions: the generic `player.rig_pc` (52 bones) plus a final `camera` bone (E-009). Bind rotations and the trailing tables (second `head/l-calf/...` set, 18 entries) not decoded |
| Coat | `mason_coat.ccmesh_pc` + `.gcmesh_pc` (3403 vertices, 2 submeshes), `mason_coat.rig_pc` (3416 B, 64 entries), `Mason_Coat` item flag `inherit_bone_transforms` | `rfg_PC.str2_pc`, `misc.vpp_pc`, `items_3d.xtbl` | DONE (E-019): coat rig decoded and merged into the skeleton (99 joints), jacket on the body1 atlas, scarf on body2 (E-020). Cloth simulation not ported: flaps follow their bones rigidly |
| Coat cloth simulation | `mason_coat.sim_pc.str2_pc` (10032 B) | `cloth_sim.vpp_pc` (and DLC copy) | TODO |
| Head | `base_hires_head.str2_pc` (422016 B), `base_hires_head.rig_pc` (500 B, 5 bones), `mason_head.rig_pc` (300 B) | `items.vpp_pc`, `misc.vpp_pc` | DONE (E-020): face, eyelashes and eyeballs bound onto the body skeleton; `.morph_pc` facial morphs not decoded |
| Optional armor | `mason_armor_l1/l2.ccmesh_pc` + `.gcmesh_pc`, rigs `mason_armor_l1/l2.rig_pc` (24 bones) | `rfg_PC.str2_pc`, `misc.vpp_pc` | DONE for geometry (fits the body rig); off by default |
| Sledgehammer | `sledgehammer.smeshx` -> `sledgehammer.str2_pc` (17970 B) and `sledgehammer_high.str2_pc` (321995 B); upgrades `sledge_upgrade_*` | `items.vpp_pc` | TODO: static mesh format; weapon flag `held in left hand`, `Attachment_prop holster-hammer` |

## 3. Textures and materials

| Item | Files | Status |
|---|---|---|
| Body textures | `mason.cpeg_pc` (420 B) + `mason.gpeg_pc` (1,310,752 B): six textures `Mason_Body1_[N/D/S].tga`, `Mason_Body2_[N/D/S].tga` (normal, diffuse, specular) | PARTIAL: `cpeg` directory read (GEKV header, 6 records of width/height/format/offset); pixel formats not yet decoded |
| Head textures | `mason_head.cpeg_pc` + `mason_head.gpeg_pc` (1,048,592 B), `mason_head.mat_pc` (1664 B); eyes and eyelashes in `always_loaded` peg | DONE (E-020): face DXT1, eyelashes DXT5 |
| Armor textures | `mason_armor_l1.cpeg_pc` + `.gpeg_pc` | TODO |
| Materials | `.mat_pc` format | TODO |

## 4. Animation data

- Animation clips are `*.anim_pc` in `anims.vpp_pc` (2582 clips; DLC copy 2580). Format `ANIM`, version 10; frame count at offset 6; track payload not decoded (E-007). Per-clip triggers, sounds and variants are in `anim_files.xtbl` (decoded, E-006).
- Hammer set coverage: the `Hammer` group plus `Default` group and its blend trees reference 151 clips. 100 are present in `anims.vpp_pc`. The 51 missing are all `plyr_hmr_cvr*` cover clips (the hammer sets `disable player cover`) plus one unrelated ambient clip, so no locomotion, melee, jump or crouch clip is missing. Full list: `LocalData/Reports/hammer_anim_coverage.json`.
- Locomotion (E-011), blend tree `Player_move_forward_HMR` (`anim_blend_trees.xtbl`), control input range 0 to 1.5, ramp speed 5:

  | Clip | Active input range (weight) |
  |---|---|
  | `plyr_hmr_std` | 0 to 0.01 (1.0) |
  | `plyr_hmr_wlk` (+ `plyr_rfl_wlk`) | 0.01 to 0.4 (1.0), fades to 0 at 0.5; blend time 0.3 |
  | `plyr_hmr_jog` | 0.4 (0.4) to 0.5 to 0.6 (1.0), fades to 0 at 1.0 |
  | `plyr_hmr_run` | 0.6 to 1.0001 (peak 1.0) to 1.5 |
  | `plyr_hmr_spnt` | 1.0001 to 1.5 (1.0) |

  The unit of the control input (probably speed divided by `max_speed 5`, scaled by the weapon's `Player_Move_Speed 1.01`) is a hypothesis.
- Melee/jump/crouch clips for the hammer: `plyr_hmr_mele`, `_mele_b`, `_melemv`, `_melemv_b`, `_crch_mele`, `_jmp*`, `_jmprun*`, `_crch`, `_crchrd_wlk*`, `_rd_*` (ready/aimed variants), all present.

## 5. Skeleton-related facts worth keeping

- 53 bones in `mason.rig_pc`: root, pelvis, spine/spine1/spine2, neck, head, clavicles, upper-arm twist 1-2, fore-twist 1, hands, fingers, thumbs, `l/r-handprop` (weapon/prop attach points), `spinebend`, eyes, `camera`.
- Game space is left-handed with the character's left at -X and facing +Z. Bevy conversion is a Z flip.
- Height: the body mesh spans y 0.00 to 1.74 (metres assumed); pelvis bind height 0.959.

## 6. Open items before the character is complete

1. ~~Decode the `ANIM` track encoding~~ DONE (E-017): decoder ported from the game code; 314 hammer/rifle/remote-charge clips baked into `mason.glb` and played in the sandbox.
2. Decode `peg` pixel formats to build body, head and coat textures.
3. Extract and decode the head STR2 (`base_hires_head`) and the sledgehammer STR2 (static mesh).
4. ~~Decode `mason_coat.rig_pc`~~ DONE (E-019). Cloth data (`mason_coat.sim_pc`) still open.
5. Read `camera.xtbl` for the third-person camera, and the `.mat_pc` format for materials.
6. Confirm the animation frame rate and the unit of the blend-tree control input.
