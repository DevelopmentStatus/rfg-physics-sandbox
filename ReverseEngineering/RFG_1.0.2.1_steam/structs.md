# Structures, vtables and constants (RFG.exe 1.0.2.1, 32-bit)

Offsets are bytes from the start of the record. Everything here was read from code and data of the game and
checked against extracted tables; confidence and sources are in [`../../Research/evidence_registry.md`](../../Research/evidence_registry.md)
(IDs in brackets). Function addresses are listed in `symbols.csv`.

## Records parsed from `.xtbl` tables

### Material record, 0x4C bytes (`FUN_006dcd90`, `rfg_materials.xtbl`) [E-023]
| Offset | Field |
|---|---|
| +0x20 | name hash (`FUN_00a03310`) |
| +0x30 | Density |
| +0x34 | 1 / Brittleness |
| +0x38 | Restitution |
| +0x3C | Friction |
| +0x40 | LinkStrength x 1000 |
| +0x48 | flags (bit order assumed from the xtbl TableDescription; flag table at 0x00b7e010, in `.data`) |

### Melee attack record, 0x50 bytes (`FUN_007d7610`, `melee.xtbl`) [E-028]
| Offset | Field |
|---|---|
| +0x14 | HitRadius |
| +0x28 | ImpactForce |
| +0x34 | DefaultDamage |
| +0x48 | StructureDamage |
| +0x4C | flags (bit 4 = structural damage) |

### Camera shake record, 0x280 bytes (`FUN_004b8920`, `camera_shake.xtbl`) [E-030]
Wobble (pitch/roll/yaw) with Variation at +0x28, Destabilization (+frequency) at +0x38, Wander +0x48, Jitter +0x58,
Oscillation1 +0x64, Oscillation2 +0x74, Direct +0x84, Wander_Direct +0x90, graph of 0x30-byte elements at +0x9C, element
count at +0x27C. Five shake slots are evaluated per frame by `FUN_004b9810`.

## Runtime objects

### Human (player/NPC) [E-028, E-029]
| Offset | Field |
|---|---|
| +0x96C | pointer to the current melee attack record |
| +0x970 | number of hits so far in this attack (reset by `FUN_007dbe30`, used by `FUN_007d7d90`) |

### Destroyable subpiece [E-023, E-028]
| Offset | Field |
|---|---|
| +0x30 | volume (multiplied by Density when above 0.004 at 0x00b4cff8) |
| +0x3F | bit 0x40 = shatter piece |

Piece flags set at load by `FUN_006ece30`: 0x42 for shatter materials, 0x08 when the material is not structural.
Other piece flags seen: 0x20 foundation, 0x80 destroyed, 0x1000 object accepts melee/bullets (see [E-028]).
Link flags: 0x04 = starts broken (pieces whose 0x40 flag differs).

### Building damage handler vtable `0x00b21870` [E-028]
| Slot offset | Function |
|---|---|
| +0x80 | `FUN_006e2080` (damage float, kind, position, direction, attacker, piece). Kinds: 3 bullet, 4 explosion, 5 melee |

### Damage record [E-028]
A type-1 record holds the damage as an 11-bit mantissa plus a 5-bit shift (`FUN_00619990` encodes, `FUN_006d5010` decodes), so
damage above 2047 loses low bits. A whole-piece record (type 5) is used for explosions on shatter pieces. Breaks are queued as event type 0xC.

## Constants (addresses)
| Address | Value | Meaning |
|---|---|---|
| 0x00b4c708 | f64 100.0 | melee/explosion damage -> piece damage |
| 0x00af0298 | f32 200 | bullet multiplier |
| 0x00af5ec0 | f32 7.5 | bullet multiplier on shatter pieces |
| 0x00b4cff8 | f32 0.004 | volume threshold for mass |
| 0x00aeeb1c, 0x00aeef00 | 3 m, 10 m^3 | "large piece" limits of `FUN_006da810` |
| 0x00b4d078 | f64 | gravity used by the stress pass |
| 0x00d9eb50 | 256 entries | animation per-component scale table |
| 0x00b648b0 | 256 x u32 | CRC32 table |
| 0x018b80c8 | table | contact records (`FUN_006d08c0`, `FUN_006d09c0`); not decoded |
| 0x0088120A | hash (not an address) | CRC of the `rfg-composite` shader name |

Hash function: CRC32 with the standard table, zero seed, no final xor, over the lowercased string (`FUN_00a03310`).
Shader name hashes [E-031]: `rfg-glass` 0xE31B6EAE, `rfg-eosglass` 0x863F9C13, `rfg-composite` 0x0088120A.

## File formats (decoders in `Extractors/`)
- Archive v3 (VPP/STR2): `Extractors/archive_v3.py`. Chunks v55: `rfg_chunk.py` [E-022]. Meshes `ccmesh/gcmesh/csmesh`: `rfg_model.py`
  [E-008, E-014]. Rig and tags: `rfg_model.py` [E-009, E-019, E-029]. Textures `cpeg/gpeg`: `rfg_peg.py` [E-013, E-020].
  Animation `anim_pc`: `rfg_anim.py` [E-017]; the time unit is 1/80 frame (times are multiplied by 0.0125).
- Rig tags: count at +0x30 in `*.rig_pc`, 60-byte records after the bone records (name offset, row-major 3x3 rotation, position,
  parent bone, -1). Static mesh tags: at the end of `*.csmesh_pc`, u32 count then 52-byte records (name CRC, 3x3, position) [E-029].
