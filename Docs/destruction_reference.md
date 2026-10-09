# Destructible chunks: pipeline and recovered rules

Status 2026-10-08. Evidence: `Research/evidence_registry.md` E-021..E-028.

## Pipeline

```text
game cache (*.vpp_pc, nested *.str2_pc)
  -> Extractors/extract_chunks.py   LocalData/Extracted/Chunks/{chunks,textures}/ + index.json
  -> Extractors/build_chunks.py     LocalData/Assets/chunks/<name>.glb/.json, tex/*.png, catalog.json, materials.json
  -> Crates/destruction             structure state, stress pass, damage, islands (no Bevy)
  -> Sandbox/src/buildings.rs       lots east of the arena, streaming, rigid bodies, ImGui "Buildings" panel
```

Rebuild everything: `python Extractors/extract_chunks.py` then `python Extractors/build_chunks.py` (about 40 min).
One chunk: `python Extractors/build_chunks.py 0203house01_a`.

## Sandbox

- All 680 chunks have a lot in the district east of the arena (x > 75), grouped by source
  (campaign, MP common, MP maps, Wrecking Crew). Buildings within the stream radius are spawned;
  undamaged ones are unloaded when the player walks away. The panel's list teleports to a lot.
- Every piece is its own collider (convex hull of its render vertices, shrunk 1.5 cm). Anchored
  pieces are static; a group that loses its connection to a root becomes one dynamic body with
  the pieces' masses. Pieces are respawned, not re-parented, when they change body (Avian 0.7
  asserts in its contact-island bookkeeping otherwise).
- The hammer sends a `StructuralHit` (HitRadius sphere, StructureDamage) on its impact triggers.
- Solver-only experiment: `cargo run -p rfg_destruction --example demolish -- <chunk> [steps] [damage]`.

## Fidelity status

| Part | Status |
|---|---|
| Chunk geometry, materials, dlod placement | Decoded, all 680 verified against section offsets |
| Load rules (material lookup, mass, flags, pre-broken glass links) | From code (`FUN_006ece30`) |
| Stress pass (waves, lever, parent yield, clusters, timing) | From code; full pass per run instead of the game's 400-pieces-per-frame slices |
| Piece damage, link damage, overflow spill, big-piece rule | From code (`FUN_006d4290`) |
| Islands splitting off | From code (`FUN_006d0aa0`) |
| Root pieces | HYPOTHESIS: ground contact + foundation flag + lowest pieces of any group no root reaches. Game: world contact records |
| Weapon value -> piece damage | From code: melee/explosion x100, bullets x200 (x7.5 shatter) (`FUN_006e2080`, E-028) |
| Melee hit volume | From code: sphere sweeps of the hammer's `destruction` tags + SecondaryImpact sweep, per-piece hits, swing falloff (E-029) |
| Weapon grip | From the rig `l-hand`/`r-hand` tags and the mesh `weapon_handle` tag (E-029) |
| Camera shake | From code: `camera_shake.xtbl` evaluated as `FUN_004b9810` (E-030); Wander, blur and rumble not ported |
| Glass rendering | `rfg-glass` panes alpha-blended from diffuse alpha (E-031); lighting is Bevy PBR, not the game shader |
| Glass blow-out | Known, not ported: piece damage > 200,000 shatters glass within sqrt(damage) x 0.04 m (`FUN_006d3f60`) |
| Debris impact damage | UNVERIFIED stand-in, off by default (it cascaded) |
| Collision shapes | Render-vertex hulls; the Havok 5.5 shapes in the chunk are not decoded |
| Bending / dynamic links (`Allow_Dynamic_Link`) | Not started |
| Fracture-surface visibility | Shown once a piece loses a link (HYPOTHESIS) |

## Next steps

1. Decode the contact records (`FUN_006d08c0`, `FUN_006d09c0`, table 0x018b80c8) for roots and
   debris impact damage.
2. Decode the Havok packfile for exact collision hulls.
3. In-game reference trials on one building (same supports removed) to compare collapse timing.
