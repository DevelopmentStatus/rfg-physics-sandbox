//! Destructible buildings and props from the original game (680 chunks, `Extractors/build_chunks.py`)
//! laid out on lots east of the arena and streamed in around the player.
//!
//! Structure rules (stress waves, parent yield, cluster breaks, islands, hit points) are the game's,
//! implemented in `rfg_destruction` from the executable (Docs/destruction_reference.md). Items marked
//! UNVERIFIED are stand-ins until their original code is decoded.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use avian3d::{math::AdjustPrecision, prelude::*};
use bevy::gltf::{Gltf, GltfMaterial, GltfMesh};
use bevy::prelude::*;
use dear_imgui_bevy::prelude::*;
use dear_imgui_rs::Condition;
use rfg_destruction::data::piece_flags;
use rfg_destruction::{ANCHORED, ChunkFile, MaterialDef, StressControl, Structure};
use serde::Deserialize;

use crate::character::Player;
use crate::perf::{SPAN_BUILDINGS_FIXED, SPAN_BUILDINGS_UPDATE, span_end, span_start};
use crate::rng::Rng;
use crate::spawn::WorldEntity;

const ASSET_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../LocalData/Assets/chunks");
/// West edge of the district (the arena ground ends at x = 60).
const DISTRICT_X: f32 = 75.0;
const DISTRICT_ROW_WIDTH: f32 = 700.0;
const LOT_GAP: f32 = 10.0;
/// Hull points move this far toward their centre so touching pieces do not start interpenetrating.
const HULL_SHRINK: f32 = 0.015;
const MAX_LOADS_IN_FLIGHT: usize = 3;

pub struct BuildingsPlugin;

impl Plugin for BuildingsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(BuildingSettings::default())
            .insert_resource(Library::load())
            .insert_resource(District::default())
            .insert_resource(Demolition::default())
            .add_message::<StructuralHit>()
            .add_systems(
                Update,
                (
                    span_start::<SPAN_BUILDINGS_UPDATE>,
                    start_lot,
                    ensure_ground,
                    stream_lots,
                    finish_loads,
                    show_cracks,
                    span_end::<SPAN_BUILDINGS_UPDATE>,
                )
                    .chain(),
            )
            .add_systems(
                FixedUpdate,
                (
                    span_start::<SPAN_BUILDINGS_FIXED>,
                    demolish_scenario,
                    apply_structural_hits,
                    impact_damage,
                    run_stress,
                    span_end::<SPAN_BUILDINGS_FIXED>,
                )
                    .chain()
                    .after(crate::tools::hammer_system),
            );
    }
}

/// Structural damage. With `piece` set it is one hit result of a melee sweep: that piece takes the
/// damage at `center` (`FUN_006e2080` kind 5 -> `FUN_006d4290`). Without it, every piece within
/// `radius` of `center` is damaged (scripted demolition); `swing` then makes a run damage once.
#[derive(Message, Clone, Copy, Debug)]
pub struct StructuralHit {
    pub center: Vec3,
    pub radius: f32,
    pub damage: f32,
    pub direction: Vec3,
    pub impact_force: f32,
    pub swing: u32,
    pub piece: Option<PieceRef>,
}

#[derive(Resource, Clone)]
pub struct BuildingSettings {
    pub control: StressControl,
    pub stress_enabled: bool,
    /// UNVERIFIED: contact impulse (N s) -> structural damage for debris impacts.
    pub impact_scale: f32,
    pub impact_threshold: f32,
    /// UNVERIFIED: velocity given to pieces a hit knocks loose, per unit of melee ImpactForce.
    pub knock_speed_per_force: f32,
    pub stream_radius: f32,
    /// Link-break overflow damages the neighbour (the game passes this per damage event).
    pub spill: bool,
}

impl Default for BuildingSettings {
    fn default() -> Self {
        Self {
            control: StressControl::DEFAULT,
            stress_enabled: true,
            impact_scale: 0.0, // off: the stand-in cascaded through whole districts
            impact_threshold: 2000.0,
            knock_speed_per_force: 0.012,
            stream_radius: 110.0,
            spill: true,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct CatalogEntry {
    pub name: String,
    pub bmin: [f32; 3],
    pub bmax: [f32; 3],
    pub destroyables: usize,
    pub pieces: usize,
    pub links: usize,
    pub source: String,
}

impl CatalogEntry {
    pub fn group(&self) -> &'static str {
        let s = self.source.as_str();
        if s.starts_with("terr01") {
            "Campaign (Tharsis)"
        } else if s.starts_with("mp_common") {
            "Multiplayer common"
        } else if s.starts_with("mp_") {
            "Multiplayer maps"
        } else if s.starts_with("wc") {
            "Wrecking Crew"
        } else {
            "Other"
        }
    }
}

/// Everything converted, and where each chunk stands in the district.
#[derive(Resource)]
pub struct Library {
    pub dir: PathBuf,
    pub catalog: Vec<CatalogEntry>,
    pub materials: Arc<Vec<MaterialDef>>,
    /// World position of each chunk's origin (its lowest point sits on the ground).
    pub lots: Vec<Vec3>,
    pub ground_min: Vec2,
    pub ground_max: Vec2,
    pub error: Option<String>,
}

impl Library {
    fn load() -> Self {
        let dir = PathBuf::from(ASSET_DIR);
        let read = || -> Result<(Vec<CatalogEntry>, Vec<MaterialDef>), String> {
            let catalog = std::fs::read_to_string(dir.join("catalog.json")).map_err(|e| e.to_string())?;
            let materials = std::fs::read_to_string(dir.join("materials.json")).map_err(|e| e.to_string())?;
            Ok((
                serde_json::from_str(&catalog).map_err(|e| e.to_string())?,
                serde_json::from_str(&materials).map_err(|e| e.to_string())?,
            ))
        };
        let (mut catalog, materials, error) = match read() {
            Ok((c, m)) => (c, m, None),
            Err(e) => (Vec::new(), Vec::new(), Some(format!("{}: {e} (run ExtractAssets.bat)", dir.display()))),
        };
        catalog.sort_by(|a, b| (a.group(), &a.name).cmp(&(b.group(), &b.name)));
        // Shelf packing: rows along +X, each row as deep as its deepest lot.
        let mut lots = Vec::with_capacity(catalog.len());
        let (mut x, mut z, mut row_depth) = (DISTRICT_X, -40.0f32, 0.0f32);
        let mut group = "";
        for entry in &catalog {
            let size = Vec3::from(entry.bmax) - Vec3::from(entry.bmin);
            if x + size.x > DISTRICT_X + DISTRICT_ROW_WIDTH || (entry.group() != group && x > DISTRICT_X) {
                x = DISTRICT_X;
                z -= row_depth + LOT_GAP + if entry.group() != group { 30.0 } else { 0.0 };
                row_depth = 0.0;
            }
            group = entry.group();
            let corner = Vec3::new(x, 0.0, z - size.z);
            lots.push(corner - Vec3::from(entry.bmin));
            x += size.x + LOT_GAP;
            row_depth = row_depth.max(size.z);
        }
        let ground_min = Vec2::new(DISTRICT_X - 20.0, z - row_depth - 20.0);
        let ground_max = Vec2::new(DISTRICT_X + DISTRICT_ROW_WIDTH + 20.0, 20.0);
        Self { dir, catalog, materials: Arc::new(materials), lots, ground_min, ground_max, error }
    }

    fn lot_center(&self, i: usize) -> Vec3 {
        let e = &self.catalog[i];
        self.lots[i] + (Vec3::from(e.bmin) + Vec3::from(e.bmax)) * 0.5
    }
}

#[derive(Default)]
enum Slot {
    #[default]
    Unloaded,
    Loading(Handle<Gltf>, Arc<ChunkFile>),
    Spawned(Box<Building>),
}

#[derive(Resource, Default)]
pub struct District {
    slots: Vec<Slot>,
    ground: Option<Entity>,
    materials: HashMap<AssetId<GltfMaterial>, Handle<StandardMaterial>>,
    rng: Option<Rng>,
    last_swing: Option<u32>,
    pub stats: DistrictStats,
}

#[derive(Default, Clone)]
pub struct DistrictStats {
    pub spawned: usize,
    pub pieces: usize,
    pub dynamic_bodies: usize,
    pub stress_passes: u64,
    pub links_broken_by_stress: u64,
    pub last_report: String,
}

struct Building {
    root: Entity,
    destroyables: Vec<DestroyableRuntime>,
    damaged: bool,
}

type Parts = Arc<Vec<Vec<(Handle<Mesh>, Handle<StandardMaterial>)>>>;

struct DestroyableRuntime {
    lot: u32,
    index: u16,
    parts: Parts,
    materials: Arc<Vec<MaterialDef>>,
    structure: Structure,
    pieces: Vec<Option<Entity>>,
    bodies: HashMap<u32, Entity>,
    next_stress: f64,
    /// Passes left at the short repeat delay after a break (`+0x112` of the game's object).
    active: u8,
    cracked: Vec<bool>,
}

#[derive(Component)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PieceRef {
    pub lot: u32,
    pub destroyable: u16,
    pub piece: u32,
}

#[derive(Component)]
struct CrackSurface;

#[derive(Component)]
struct DistrictGround;

/// `RFG_START_LOT=<chunk name>` puts the player in front of that building once at startup and makes
/// it the demolition target; `RFG_DEMOLISH=<n>` also starts a support-ring demolition of n hits.
fn start_lot(
    library: Res<Library>,
    mut demolition: ResMut<Demolition>,
    mut done: Local<bool>,
    mut player: Query<(&mut Transform, &mut LinearVelocity), With<Player>>,
) {
    if *done {
        return;
    }
    let Ok(name) = std::env::var("RFG_START_LOT") else {
        *done = true;
        return;
    };
    let Ok((mut t, mut v)) = player.single_mut() else { return };
    *done = true;
    let Some(i) = library.catalog.iter().position(|e| e.name == name) else {
        warn!("RFG_START_LOT: no chunk named {name}");
        return;
    };
    t.translation = teleport_target(&library, i);
    v.0 = Vec3::ZERO.adjust_precision();
    demolition.target = Some(i);
    if let Some(n) = std::env::var("RFG_DEMOLISH").ok().and_then(|v| v.parse::<i32>().ok()) {
        demolition.hits = n;
        demolition.request = Some(DemolishPattern::SupportRing);
    }
}

/// Which support pieces a scripted demolition hits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DemolishPattern {
    /// Pieces linked directly to a root piece, swept along X then Z.
    SupportRing,
    /// The support ring on the -X half only, so the building leans and topples that way.
    OneSide,
    /// Every support-ring piece at once (one hit each, same frame).
    AllAtOnce,
}

/// Scripted, repeatable support-removal runs started from the Buildings panel, for comparing
/// collapse behaviour with the original game. Hits use the hammer's numbers (Structure 1500).
#[derive(Resource)]
pub struct Demolition {
    pub target: Option<usize>,
    pub hits: i32,
    pub interval: f32,
    pub radius: f32,
    pub damage: f32,
    request: Option<DemolishPattern>,
    run: Option<DemolishRun>,
    /// Hit ids count down from u32::MAX so they never match a hammer swing or each other.
    serial: u32,
}

struct DemolishRun {
    lot: usize,
    pattern: DemolishPattern,
    targets: Vec<Vec3>,
    next: usize,
    at: f64,
}

impl Default for Demolition {
    fn default() -> Self {
        Self { target: None, hits: 40, interval: 0.1, radius: 0.6, damage: 1500.0, request: None, run: None, serial: u32::MAX }
    }
}

fn demolish_scenario(
    library: Res<Library>,
    district: Res<District>,
    time: Res<Time>,
    mut demolition: ResMut<Demolition>,
    mut hits: MessageWriter<StructuralHit>,
) {
    let d = &mut *demolition;
    if let (Some(pattern), Some(lot)) = (d.request, d.target) {
        // Waits here until the building has streamed in.
        if let Some(Slot::Spawned(building)) = district.slots.get(lot) {
            d.request = None;
            if let Some(runtime) = building.destroyables.iter().max_by_key(|r| r.pieces.len()) {
                let s = &runtime.structure;
                let mut ring: Vec<Vec3> = (0..s.pieces.len())
                    .filter(|&i| {
                        !s.destroyed[i]
                            && !s.root[i]
                            && s.pieces[i].flags & piece_flags::FOUNDATION == 0
                            && s.live_links(i).any(|(_, q)| s.root[q])
                    })
                    .map(|i| (s.pieces[i].bmin + s.pieces[i].bmax) * 0.5)
                    .collect();
                ring.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.z.total_cmp(&b.z)));
                if pattern == DemolishPattern::OneSide && !ring.is_empty() {
                    let mid = (ring[0].x + ring[ring.len() - 1].x) * 0.5;
                    ring.retain(|p| p.x <= mid);
                }
                let targets: Vec<Vec3> = if pattern == DemolishPattern::AllAtOnce {
                    ring
                } else {
                    let count = d.hits.max(1) as usize;
                    let step = (ring.len() / count).max(1);
                    ring.into_iter().step_by(step).take(count).collect()
                };
                let targets = targets.into_iter().map(|p| library.lots[lot] + p).collect::<Vec<_>>();
                info!("demolish {:?}: {} hits on {}", pattern, targets.len(), library.catalog[lot].name);
                d.run = Some(DemolishRun { lot, pattern, targets, next: 0, at: time.elapsed_secs_f64() + 0.5 });
            }
        } else if d.run.is_some() {
            d.run = None;
        }
    }
    let Some(run) = d.run.as_mut() else { return };
    // The building was unloaded or the world reset.
    if !matches!(district.slots.get(run.lot), Some(Slot::Spawned(_))) {
        d.run = None;
        return;
    }
    while run.next < run.targets.len() && time.elapsed_secs_f64() >= run.at {
        hits.write(StructuralHit {
            center: run.targets[run.next],
            radius: d.radius,
            damage: d.damage,
            direction: Vec3::NEG_Z,
            impact_force: 350.0,
            swing: d.serial,
            piece: None,
        });
        d.serial = d.serial.wrapping_sub(1);
        run.next += 1;
        if run.pattern != DemolishPattern::AllAtOnce {
            run.at += d.interval as f64;
        }
    }
    if run.next >= run.targets.len() {
        d.run = None;
    }
}

fn teleport_target(library: &Library, i: usize) -> Vec3 {
    let e = &library.catalog[i];
    let centre = library.lot_center(i);
    let depth = e.bmax[2] - e.bmin[2];
    Vec3::new(centre.x, 1.2, library.lots[i].z + e.bmax[2] + depth.min(20.0) * 0.5 + 6.0)
}

fn ensure_ground(
    mut commands: Commands,
    mut district: ResMut<District>,
    library: Res<Library>,
    grounds: Query<(), With<DistrictGround>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if district.ground.is_some_and(|g| grounds.get(g).is_ok()) {
        return;
    }
    // First run, or the world was reset (reset despawns every WorldEntity): start over.
    district.slots = (0..library.catalog.len()).map(|_| Slot::Unloaded).collect();
    district.rng = Some(Rng::new(crate::SEED ^ 0xB11D));
    district.last_swing = None;
    let size = library.ground_max - library.ground_min;
    let center = (library.ground_min + library.ground_max) * 0.5;
    let ground = commands
        .spawn((
            WorldEntity,
            DistrictGround,
            Name::new("District ground"),
            RigidBody::Static,
            Collider::cuboid(size.x, 1.0, size.y),
            Mesh3d(meshes.add(Cuboid::new(size.x, 1.0, size.y))),
            MeshMaterial3d(materials.add(Color::srgb(0.45, 0.36, 0.3))),
            Transform::from_xyz(center.x, -0.5, center.y),
        ))
        .id();
    district.ground = Some(ground);
}

fn stream_lots(
    mut commands: Commands,
    mut district: ResMut<District>,
    library: Res<Library>,
    settings: Res<BuildingSettings>,
    assets: Res<AssetServer>,
    player: Query<&Transform, With<Player>>,
) {
    let Ok(player) = player.single() else { return };
    let here = player.translation;
    let mut in_flight = district.slots.iter().filter(|s| matches!(s, Slot::Loading(..))).count();
    let district = &mut *district;
    for (i, slot) in district.slots.iter_mut().enumerate() {
        let entry = &library.catalog[i];
        let half = (Vec3::from(entry.bmax) - Vec3::from(entry.bmin)).xz().length() * 0.5;
        let distance = library.lot_center(i).xz().distance(here.xz()) - half;
        match slot {
            Slot::Unloaded if distance < settings.stream_radius && in_flight < MAX_LOADS_IN_FLIGHT => {
                let path = library.dir.join(format!("{}.json", entry.name));
                match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| {
                    serde_json::from_str::<ChunkFile>(&t).map_err(|e| e.to_string())
                }) {
                    Ok(def) => {
                        *slot = Slot::Loading(assets.load(format!("chunks/{}.glb", entry.name)), Arc::new(def));
                        in_flight += 1;
                    }
                    Err(e) => {
                        warn!("{}: {e}", path.display());
                    }
                }
            }
            Slot::Spawned(building) if distance > settings.stream_radius + 40.0 && !building.damaged => {
                commands.entity(building.root).despawn();
                *slot = Slot::Unloaded;
            }
            _ => {}
        }
    }
}

fn finish_loads(
    mut commands: Commands,
    mut district: ResMut<District>,
    library: Res<Library>,
    gltfs: Res<Assets<Gltf>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    gltf_materials: Res<Assets<GltfMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Res<AssetServer>,
) {
    let district = &mut *district;
    for i in 0..district.slots.len() {
        let Slot::Loading(handle, def) = &district.slots[i] else { continue };
        if !assets.is_loaded_with_dependencies(handle) {
            if assets.load_state(handle).is_failed() {
                warn!("{}: glb failed to load", library.catalog[i].name);
                district.slots[i] = Slot::Unloaded;
            }
            continue;
        }
        let Some(gltf) = gltfs.get(handle) else { continue };
        // Mesh index -> (mesh, material) per primitive.
        let mut parts: Vec<Vec<(Handle<Mesh>, Handle<StandardMaterial>)>> = Vec::new();
        for mesh in &gltf.meshes {
            let Some(mesh) = gltf_meshes.get(mesh) else {
                parts.push(Vec::new());
                continue;
            };
            parts.push(
                mesh.primitives
                    .iter()
                    .map(|p| {
                        let material = p.material.as_ref().map_or_else(Handle::default, |m| {
                            district
                                .materials
                                .entry(m.id())
                                .or_insert_with(|| {
                                    let g = gltf_materials.get(m);
                                    // Alpha mode comes from the converter: MASK for fracture
                                    // surfaces, BLEND for `rfg-glass` panes (E-031).
                                    materials.add(StandardMaterial {
                                        base_color: g.map_or(Color::srgb(0.55, 0.53, 0.5), |g| g.base_color),
                                        base_color_texture: g.and_then(|g| g.base_color_texture.clone()),
                                        perceptual_roughness: g.map_or(0.9, |g| g.perceptual_roughness),
                                        metallic: g.map_or(0.0, |g| g.metallic),
                                        alpha_mode: g.map_or(AlphaMode::Opaque, |g| g.alpha_mode),
                                        ..default()
                                    })
                                })
                                .clone()
                        });
                        (p.mesh.clone(), material)
                    })
                    .collect(),
            );
        }
        let building = spawn_building(&mut commands, i, library.lots[i], def, Arc::new(parts), library.materials.clone());
        district.slots[i] = Slot::Spawned(Box::new(building));
    }
    let mut stats = DistrictStats { ..district.stats.clone() };
    stats.spawned = 0;
    stats.pieces = 0;
    stats.dynamic_bodies = 0;
    for slot in &district.slots {
        if let Slot::Spawned(b) = slot {
            stats.spawned += 1;
            for d in &b.destroyables {
                stats.pieces += d.pieces.len();
                stats.dynamic_bodies += d.bodies.len();
            }
        }
    }
    district.stats = stats;
}

fn piece_collider(hull: &[[f32; 3]]) -> Option<Collider> {
    if hull.len() < 4 {
        return None;
    }
    let centre = hull.iter().map(|p| Vec3::from(*p)).sum::<Vec3>() / hull.len() as f32;
    let points = hull
        .iter()
        .map(|p| {
            let p = Vec3::from(*p);
            let d = p - centre;
            (p - d.normalize_or_zero() * HULL_SHRINK.min(d.length() * 0.5)).adjust_precision()
        })
        .collect();
    Collider::convex_hull(points)
}

fn spawn_building(
    commands: &mut Commands,
    lot: usize,
    origin: Vec3,
    def: &ChunkFile,
    parts: Parts,
    materials: Arc<Vec<MaterialDef>>,
) -> Building {
    let root = commands
        .spawn((WorldEntity, Name::new(def.name.clone()), Transform::from_translation(origin), Visibility::default()))
        .id();
    let mut destroyables = Vec::new();
    for (di, d) in def.destroyables.iter().enumerate() {
        let base = d.pieces.iter().map(|p| p.bmin[1]).fold(f32::INFINITY, f32::min);
        let ground = -origin.y;
        // Roots: pieces resting on the ground plane, and foundation pieces. The game takes roots
        // from its contact records with the terrain/world (FUN_006d08c0); HYPOTHESIS for placement.
        let structure = Structure::new(d, &materials, |_, p| {
            p.bmin.y <= base.max(ground) + 0.15 || p.flags & piece_flags::FOUNDATION != 0
        });
        let mut runtime = DestroyableRuntime {
            lot: lot as u32,
            index: di as u16,
            parts: parts.clone(),
            materials: materials.clone(),
            cracked: vec![false; d.pieces.len()],
            structure,
            pieces: Vec::with_capacity(d.pieces.len()),
            bodies: HashMap::new(),
            next_stress: 0.0,
            active: 0,
        };
        for (pi, p) in d.pieces.iter().enumerate() {
            let transform = d
                .dlods
                .get(p.dlod.max(0) as usize)
                .map_or(Transform::IDENTITY, |q| Transform::from_translation(Vec3::from(q.pos)).with_rotation(Quat::from_array(q.rot)));
            let id = spawn_piece(commands, &runtime, p, pi, transform, root, true);
            runtime.pieces.push(Some(id));
        }
        // Pieces the load-time rules leave unattached (glass panes cut off by pre-broken links)
        // become loose bodies at once.
        let loose: Vec<u32> = {
            let mut v: Vec<u32> = runtime.structure.island.iter().copied().filter(|&i| i != ANCHORED).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        for island in loose {
            let members: Vec<usize> =
                (0..runtime.pieces.len()).filter(|&p| runtime.structure.island[p] == island).collect();
            detach(commands, &mut runtime, island, &members, origin, d, Vec3::ZERO);
        }
        destroyables.push(runtime);
    }
    // Non-destructible parts of the chunk (machinery bodies and the like).
    for g in &def.general {
        let mut part = commands.spawn((
            WorldEntity,
            Name::new(g.name.clone()),
            Transform::from_translation(Vec3::from(g.pos)).with_rotation(Quat::from_array(g.rot)),
            Visibility::default(),
            ChildOf(root),
        ));
        if let Some(collider) = piece_collider(&g.hull) {
            part.insert((collider, RigidBody::Static));
        }
        let id = part.id();
        if let Some(mesh) = g.mesh.and_then(|m| parts.get(m)) {
            for (mesh, material) in mesh {
                commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), ChildOf(id)));
            }
        }
    }
    Building { root, destroyables, damaged: false }
}

/// One piece: collider (static while part of the building) plus its render and fracture meshes.
/// Pieces are respawned rather than re-parented when they change body: moving live colliders
/// between rigid bodies trips Avian 0.7's contact-island bookkeeping.
fn spawn_piece(
    commands: &mut Commands,
    runtime: &DestroyableRuntime,
    p: &rfg_destruction::data::PieceDef,
    index: usize,
    transform: Transform,
    parent: Entity,
    anchored: bool,
) -> Entity {
    let mut piece = commands.spawn((
        WorldEntity,
        PieceRef { lot: runtime.lot, destroyable: runtime.index, piece: index as u32 },
        transform,
        Visibility::default(),
        ChildOf(parent),
    ));
    let collider = piece_collider(&p.hull).unwrap_or_else(|| {
        // Degenerate hull (fewer than 4 points): use the piece bounds in its dlod frame.
        let size = (Vec3::from(p.bmax) - Vec3::from(p.bmin) - 0.1).max(Vec3::splat(0.02));
        Collider::cuboid(size.x.adjust_precision(), size.y.adjust_precision(), size.z.adjust_precision())
    });
    {
        piece.insert((collider, Mass(p.mass.max(1.0))));
        if anchored {
            piece.insert(RigidBody::Static);
        }
        let friction = runtime.materials.get(p.material).map_or(0.8, |m| m.friction);
        piece.insert(Friction::new(friction.adjust_precision()));
    }
    let id = piece.id();
    if let Some(mesh) = p.mesh.and_then(|m| runtime.parts.get(m)) {
        for (mesh, material) in mesh {
            commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), ChildOf(id)));
        }
    }
    if let Some(crack) = p.crack.and_then(|m| runtime.parts.get(m)) {
        for (mesh, material) in crack {
            commands.spawn((CrackSurface, Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Visibility::Hidden, ChildOf(id)));
        }
    }
    id
}

/// Moves pieces into a new dynamic body. Positions are rebuilt from the chunk data (anchored pieces
/// never move) or from the body they leave, taken from `GlobalTransform` next frame by Avian.
fn detach(
    commands: &mut Commands,
    runtime: &mut DestroyableRuntime,
    island: u32,
    members: &[usize],
    origin: Vec3,
    def: &rfg_destruction::DestroyableDef,
    velocity: Vec3,
) {
    let centre = members.iter().map(|&p| Vec3::from(def.pieces[p].pos)).sum::<Vec3>() / members.len().max(1) as f32;
    let body = commands
        .spawn((
            WorldEntity,
            Name::new("Debris"),
            RigidBody::Dynamic,
            Transform::from_translation(origin + centre),
            Visibility::default(),
            LinearVelocity(velocity.adjust_precision()),
        ))
        .id();
    for &p in members {
        let Some(entity) = runtime.pieces[p] else { continue };
        let piece = &def.pieces[p];
        let local = def
            .dlods
            .get(piece.dlod.max(0) as usize)
            .map_or(Transform::IDENTITY, |q| Transform::from_translation(Vec3::from(q.pos) - centre).with_rotation(Quat::from_array(q.rot)));
        commands.entity(entity).despawn();
        let id = spawn_piece(commands, runtime, piece, p, local, body, false);
        runtime.pieces[p] = Some(id);
        runtime.cracked[p] = false;
    }
    runtime.bodies.insert(island, body);
}

/// Moves pieces that left a dynamic body into a body of their own, keeping their world pose.
fn split_dynamic(
    commands: &mut Commands,
    runtime: &mut DestroyableRuntime,
    island: u32,
    members: &[usize],
    def: &rfg_destruction::DestroyableDef,
    poses: &Query<&GlobalTransform>,
    velocity: Vec3,
) {
    let world: Vec<(usize, Transform)> = members
        .iter()
        .filter_map(|&p| runtime.pieces[p].and_then(|e| poses.get(e).ok()).map(|g| (p, g.compute_transform())))
        .collect();
    if world.is_empty() {
        return;
    }
    let centre = world.iter().map(|(_, t)| t.translation).sum::<Vec3>() / world.len() as f32;
    let body = commands
        .spawn((
            WorldEntity,
            Name::new("Debris"),
            RigidBody::Dynamic,
            Transform::from_translation(centre),
            Visibility::default(),
            LinearVelocity(velocity.adjust_precision()),
        ))
        .id();
    for (p, t) in world {
        if let Some(entity) = runtime.pieces[p] {
            let local = Transform::from_translation(t.translation - centre).with_rotation(t.rotation);
            commands.entity(entity).despawn();
            let id = spawn_piece(commands, runtime, &def.pieces[p], p, local, body, false);
            runtime.pieces[p] = Some(id);
            runtime.cracked[p] = false;
        }
    }
    runtime.bodies.insert(island, body);
}

/// After links broke: split off what lost its support, re-check cracks, wake the stress timer.
fn settle(
    commands: &mut Commands,
    runtime: &mut DestroyableRuntime,
    origin: Vec3,
    def: &rfg_destruction::DestroyableDef,
    poses: &Query<&GlobalTransform>,
    velocities: &Query<&LinearVelocity>,
    rng: &mut Rng,
    knock: Option<(Vec3, Vec3, f32)>,
) {
    let splits = runtime.structure.resolve_islands();
    for split in splits {
        let push = knock.map_or(Vec3::ZERO, |(center, dir, speed)| {
            let near = split.pieces.iter().any(|&p| {
                let piece = &runtime.structure.pieces[p];
                center.clamp(piece.bmin, piece.bmax).distance(center) < 1.5
            });
            if near { dir * speed } else { Vec3::ZERO }
        });
        if split.from == ANCHORED {
            detach(commands, runtime, split.to, &split.pieces, origin, def, push);
        } else {
            let inherited = runtime
                .bodies
                .get(&split.from)
                .and_then(|b| velocities.get(*b).ok())
                .map_or(Vec3::ZERO, |v| Vec3::new(v.x as f32, v.y as f32, v.z as f32));
            split_dynamic(commands, runtime, split.to, &split.pieces, def, poses, inherited + push);
        }
    }
    runtime.active = (runtime.active + 1 + (rng.next_f32() * 4.0) as u8).clamp(1, 9);
    runtime.next_stress = 0.0;
}

/// Melee and explosion hits: the building damage handler (`FUN_006e2080`, cases 4 and 5) truncates
/// `damage * 100.0` (double at 0x00b4c708) to integer piece damage. Bullets (case 3) use 200, or 7.5
/// on shatter pieces. E-028.
pub const MELEE_DAMAGE_SCALE: f32 = 100.0;

/// The game's `__ftol` truncation of the scaled weapon damage, then the damage record's 16-bit
/// packing (`FUN_00619990`: 11-bit mantissa, shift in the top 5 bits; unpacked in `FUN_006d5010`).
pub fn piece_damage(weapon_damage: f32) -> i32 {
    let raw = (weapon_damage as f64 * MELEE_DAMAGE_SCALE as f64) as i32 as u32;
    let shift = if raw & !0x7ff != 0 { (31 - raw.leading_zeros()) - 10 } else { 0 };
    let packed = (raw >> shift) | (shift << 11);
    ((packed & 0x7ff) << (packed >> 11)) as i32
}

fn apply_structural_hits(
    mut commands: Commands,
    mut hits: MessageReader<StructuralHit>,
    mut district: ResMut<District>,
    library: Res<Library>,
    settings: Res<BuildingSettings>,
    poses: Query<&GlobalTransform>,
    velocities: Query<&LinearVelocity>,
) {
    let district = &mut *district;
    for hit in hits.read() {
        if hit.piece.is_none() && district.last_swing == Some(hit.swing) {
            continue;
        }
        let mut any = false;
        let mut rng = district.rng.take().unwrap_or_else(|| Rng::new(crate::SEED));
        for (i, slot) in district.slots.iter_mut().enumerate() {
            let Slot::Spawned(building) = slot else { continue };
            let origin = library.lots[i];
            let Some(def) = load_def(&library, i) else { continue };
            for (di, runtime) in building.destroyables.iter_mut().enumerate() {
                // Damage is tested in the anchored building's frame; loose bodies are left alone
                // until their own pose tracking is added.
                let local = hit.center - origin;
                let damage = piece_damage(hit.damage);
                if let Some(target) = hit.piece {
                    if target.lot != runtime.lot || target.destroyable != runtime.index {
                        continue;
                    }
                    let piece = target.piece as usize;
                    if piece >= runtime.structure.pieces.len() || runtime.structure.destroyed[piece] {
                        continue;
                    }
                    runtime.structure.damage_piece(piece, damage, local, &library.materials, settings.spill, 0);
                } else if runtime.structure.apply_damage(local, hit.radius, damage, &library.materials, settings.spill).is_empty() {
                    continue;
                }
                any = true;
                building.damaged = true;
                let knock = Some((local, hit.direction, hit.impact_force * settings.knock_speed_per_force));
                settle(&mut commands, runtime, origin, &def.destroyables[di], &poses, &velocities, &mut rng, knock);
            }
        }
        district.rng = Some(rng);
        if any {
            district.last_swing = Some(hit.swing);
        }
    }
}

/// UNVERIFIED stand-in for the game's contact damage: hard contacts damage the pieces involved.
fn impact_damage(
    mut commands: Commands,
    collisions: Collisions,
    pieces: Query<&PieceRef>,
    mut district: ResMut<District>,
    library: Res<Library>,
    settings: Res<BuildingSettings>,
    poses: Query<&GlobalTransform>,
    velocities: Query<&LinearVelocity>,
) {
    if settings.impact_scale <= 0.0 {
        return;
    }
    let mut events: Vec<(u32, u16, u32, f32)> = Vec::new();
    for pair in collisions.iter() {
        let impulse = pair.total_normal_impulse_magnitude() as f32;
        if impulse < settings.impact_threshold {
            continue;
        }
        for collider in [pair.collider1, pair.collider2] {
            if let Ok(p) = pieces.get(collider) {
                events.push((p.lot, p.destroyable, p.piece, impulse));
            }
        }
    }
    if events.is_empty() {
        return;
    }
    let district = &mut *district;
    let mut rng = district.rng.take().unwrap_or_else(|| Rng::new(crate::SEED));
    let mut touched: Vec<(usize, usize)> = Vec::new();
    for (lot, d, piece, impulse) in events {
        let Some(Slot::Spawned(building)) = district.slots.get_mut(lot as usize) else { continue };
        let Some(runtime) = building.destroyables.get_mut(d as usize) else { continue };
        let s = &mut runtime.structure;
        let piece = piece as usize;
        if s.destroyed[piece] {
            continue;
        }
        let amount = ((impulse - settings.impact_threshold) * settings.impact_scale) as i32;
        let point = (s.pieces[piece].bmin + s.pieces[piece].bmax) * 0.5;
        s.damage_piece(piece, amount, point, &library.materials, settings.spill, 0);
        building.damaged = true;
        touched.push((lot as usize, d as usize));
    }
    touched.sort_unstable();
    touched.dedup();
    for (lot, d) in touched {
        let Some(def) = load_def(&library, lot) else { continue };
        if let Slot::Spawned(building) = &mut district.slots[lot] {
            settle(&mut commands, &mut building.destroyables[d], library.lots[lot], &def.destroyables[d], &poses, &velocities, &mut rng, None);
        }
    }
    district.rng = Some(rng);
}

fn run_stress(
    mut commands: Commands,
    mut district: ResMut<District>,
    library: Res<Library>,
    settings: Res<BuildingSettings>,
    time: Res<Time>,
    poses: Query<&GlobalTransform>,
    velocities: Query<&LinearVelocity>,
) {
    if !settings.stress_enabled {
        return;
    }
    let now = time.elapsed_secs_f64();
    let district = &mut *district;
    let mut rng = district.rng.take().unwrap_or_else(|| Rng::new(crate::SEED));
    for (i, slot) in district.slots.iter_mut().enumerate() {
        let Slot::Spawned(building) = slot else { continue };
        if !building.damaged {
            continue; // an untouched building is in equilibrium (checked by rfg_destruction's tests)
        }
        for di in 0..building.destroyables.len() {
            let runtime = &mut building.destroyables[di];
            if runtime.next_stress > now {
                continue;
            }
            let report = runtime.structure.stress_pass(&settings.control);
            district.stats.stress_passes += 1;
            let c = &settings.control;
            // FUN_006f0310: busy objects repeat after about Repeat_Delay_Min (+-50 %), idle ones
            // after Repeat_Delay_Max.
            let delay_ms = if runtime.active > 0 {
                runtime.active -= 1;
                c.repeat_delay_min_ms as f32 * (0.5 + rng.next_f32())
            } else {
                c.repeat_delay_max_ms as f32
            };
            runtime.next_stress = now + delay_ms as f64 / 1000.0;
            if report.broken_links.is_empty() {
                continue;
            }
            district.stats.links_broken_by_stress += report.broken_links.len() as u64;
            district.stats.last_report = format!(
                "{} #{di}: {} links ({} parent-yield, {} cluster), worst ratio {:.2}",
                library.catalog[i].name,
                report.broken_links.len(),
                report.parent_yield_breaks,
                report.cluster_breaks,
                report.worst_ratio
            );
            if let Some(def) = load_def(&library, i) {
                settle(&mut commands, runtime, library.lots[i], &def.destroyables[di], &poses, &velocities, &mut rng, None);
            }
        }
    }
    district.rng = Some(rng);
}

fn show_cracks(
    mut district: ResMut<District>,
    children: Query<&Children>,
    mut cracks: Query<&mut Visibility, With<CrackSurface>>,
) {
    for slot in &mut district.slots {
        let Slot::Spawned(building) = slot else { continue };
        if !building.damaged {
            continue;
        }
        for runtime in &mut building.destroyables {
            for p in 0..runtime.pieces.len() {
                if runtime.cracked[p] || !runtime.structure.is_cracked(p) {
                    continue;
                }
                runtime.cracked[p] = true;
                let Some(entity) = runtime.pieces[p] else { continue };
                for child in children.get(entity).into_iter().flatten() {
                    if let Ok(mut v) = cracks.get_mut(*child) {
                        *v = Visibility::Inherited;
                    }
                }
            }
        }
    }
}

/// Chunk definitions are re-read on demand to keep only one copy (the slot's) resident.
fn load_def(library: &Library, lot: usize) -> Option<Arc<ChunkFile>> {
    thread_local! {
        static CACHE: std::cell::RefCell<HashMap<usize, Arc<ChunkFile>>> = Default::default();
    }
    CACHE.with(|cache| {
        if let Some(def) = cache.borrow().get(&lot) {
            return Some(def.clone());
        }
        let path = library.dir.join(format!("{}.json", library.catalog[lot].name));
        let def: ChunkFile = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
        let def = Arc::new(def);
        cache.borrow_mut().insert(lot, def.clone());
        Some(def)
    })
}

#[derive(Default)]
pub struct PanelState {
    filter: String,
}

pub fn panel(
    frame: ImguiFrame<'_>,
    library: Res<Library>,
    district: Res<District>,
    mut settings: ResMut<BuildingSettings>,
    mut demolition: ResMut<Demolition>,
    mut player: Query<(&mut Transform, &mut LinearVelocity), With<Player>>,
    mut state: Local<PanelState>,
) {
    let ui = frame.ui();
    ui.window("Buildings")
        .size([380.0, 560.0], Condition::FirstUseEver)
        .position([330.0, 12.0], Condition::FirstUseEver)
        .build(|| {
            if let Some(error) = &library.error {
                ui.text_wrapped(format!("No chunk assets: {error}"));
                return;
            }
            let s = &district.stats;
            ui.text(format!(
                "{} chunks on lots east of the arena\nloaded {}  pieces {}  loose bodies {}",
                library.catalog.len(),
                s.spawned,
                s.pieces,
                s.dynamic_bodies
            ));
            ui.text(format!("stress passes {}  links broken by stress {}", s.stress_passes, s.links_broken_by_stress));
            if !s.last_report.is_empty() {
                ui.text_wrapped(&s.last_report);
            }
            ui.separator();
            ui.checkbox("Stress system (game rules)", &mut settings.stress_enabled);
            let mut preset = match settings.control {
                c if c == StressControl::DEFAULT => 0,
                c if c == StressControl::MULTI => 1,
                c if c == StressControl::WRECKING_CREW_LOW => 2,
                c if c == StressControl::WRECKING_CREW_HIGH => 3,
                _ => 0,
            };
            for (i, label) in ["Default", "Multi", "Wrecking Crew low", "Wrecking Crew high"].iter().enumerate() {
                if ui.radio_button_bool(*label, preset == i) {
                    preset = i;
                }
            }
            settings.control = [
                StressControl::DEFAULT,
                StressControl::MULTI,
                StressControl::WRECKING_CREW_LOW,
                StressControl::WRECKING_CREW_HIGH,
            ][preset];
            ui.text(format!("Weapon damage x{MELEE_DAMAGE_SCALE} (from FUN_006e2080)"));
            ui.text_disabled("UNVERIFIED conversions (not yet decoded):");
            ui.checkbox("Damage spills over broken links", &mut settings.spill);
            ui.slider("Impact damage scale", 0.0, 200.0, &mut settings.impact_scale);
            ui.slider("Impact threshold (N s)", 100.0, 20000.0, &mut settings.impact_threshold);
            ui.slider("Stream radius (m)", 40.0, 300.0, &mut settings.stream_radius);
            ui.separator();
            demolition_ui(ui, &library, &district, &mut demolition, &mut player);
            ui.separator();
            ui.input_text("Filter", &mut state.filter).build();
            let filter = state.filter.to_lowercase();
            ui.child_window("lots").size([0.0, 0.0]).build(ui, || {
                let mut group = "";
                for (i, e) in library.catalog.iter().enumerate() {
                    if !filter.is_empty() && !e.name.to_lowercase().contains(&filter) {
                        continue;
                    }
                    if e.group() != group {
                        group = e.group();
                        ui.text_disabled(group);
                    }
                    let label = format!("{}  ({} pieces, {} links, {} objects)##{i}", e.name, e.pieces, e.links, e.destroyables);
                    if ui.selectable_config(label).selected(demolition.target == Some(i)).build() {
                        demolition.target = Some(i);
                        if let Ok((mut t, mut v)) = player.single_mut() {
                            t.translation = teleport_target(&library, i);
                            v.0 = Vec3::ZERO.adjust_precision();
                        }
                    }
                }
            });
        });
}

fn demolition_ui(
    ui: &dear_imgui_rs::Ui,
    library: &Library,
    district: &District,
    demolition: &mut Demolition,
    player: &mut Query<(&mut Transform, &mut LinearVelocity), With<Player>>,
) {
    ui.text("Demolition");
    let player_pos = player.single().ok().map(|(t, _)| t.translation);
    if ui.button("Target nearest") {
        if let Some(p) = player_pos {
            demolition.target = (0..library.catalog.len())
                .filter(|&i| matches!(district.slots.get(i), Some(Slot::Spawned(_))))
                .min_by(|&a, &b| library.lot_center(a).distance(p).total_cmp(&library.lot_center(b).distance(p)));
        }
    }
    ui.same_line();
    match demolition.target {
        Some(i) => {
            let loaded = matches!(district.slots.get(i), Some(Slot::Spawned(_)));
            ui.text(format!("{}{}", library.catalog[i].name, if loaded { "" } else { "  (not loaded)" }));
        }
        None => ui.text_disabled("pick a building in the list"),
    }
    ui.slider("Hits", 1, 400, &mut demolition.hits);
    ui.slider("Seconds between hits", 0.0, 1.0, &mut demolition.interval);
    ui.slider("Hit radius (m)", 0.1, 3.0, &mut demolition.radius);
    ui.slider("Structure damage", 100.0, 20000.0, &mut demolition.damage);
    ui.text_disabled("Hammer = Structure 1500, radius 0.6 m");
    let Some(target) = demolition.target else { return };
    for (label, pattern) in [
        ("Support ring", DemolishPattern::SupportRing),
        ("One side", DemolishPattern::OneSide),
        ("All supports at once", DemolishPattern::AllAtOnce),
    ] {
        if ui.button(label) {
            demolition.request = Some(pattern);
            demolition.run = None;
            if !matches!(district.slots.get(target), Some(Slot::Spawned(_))) {
                if let Ok((mut t, mut v)) = player.single_mut() {
                    t.translation = teleport_target(library, target);
                    v.0 = Vec3::ZERO.adjust_precision();
                }
            }
        }
        ui.same_line();
    }
    if ui.button("Stop") {
        demolition.request = None;
        demolition.run = None;
    }
    if let Some(run) = &demolition.run {
        ui.text(format!("{:?}: hit {}/{}", run.pattern, run.next, run.targets.len()));
    } else if demolition.request.is_some() {
        ui.text("waiting for the building to load...");
    }
    ui.text_disabled("R resets the world (all buildings)");
}
