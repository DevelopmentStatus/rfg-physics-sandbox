//! Test arena: ground, ramps, pillars, a starter wall, and the player.

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::character::spawn_player;
use crate::input::{CameraRig, PlayerInput};
use crate::rng::Rng;
use crate::spawn::{SpawnKind, SpawnRng, WorldEntity, spawn_kind};
use crate::tools::{HammerState, LastHit};
use crate::SEED;

fn spawn_static(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    name: &'static str,
    size: Vec3,
    color: Color,
    transform: Transform,
) {
    commands.spawn((
        WorldEntity,
        Name::new(name),
        RigidBody::Static,
        Collider::cuboid(size.x, size.y, size.z),
        Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
        MeshMaterial3d(materials.add(color)),
        transform,
    ));
}

pub fn build_world(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    assets: &AssetServer,
) {
    spawn_static(
        commands, meshes, materials, "Ground",
        Vec3::new(120.0, 1.0, 120.0), Color::srgb(0.32, 0.33, 0.3),
        Transform::from_xyz(0.0, -0.5, 0.0),
    );
    spawn_static(
        commands, meshes, materials, "Ramp",
        Vec3::new(6.0, 0.5, 10.0), Color::srgb(0.4, 0.4, 0.45),
        Transform::from_xyz(14.0, 1.0, 0.0).with_rotation(Quat::from_rotation_x(0.22)),
    );
    for (i, height) in [0.4, 0.8, 1.2].into_iter().enumerate() {
        spawn_static(
            commands, meshes, materials, "Step",
            Vec3::new(4.0, height, 2.0), Color::srgb(0.42, 0.42, 0.46),
            Transform::from_xyz(-12.0, height * 0.5, -(i as f32) * 2.0),
        );
    }
    for x in [-6.0, 6.0] {
        spawn_static(
            commands, meshes, materials, "Pillar",
            Vec3::new(1.0, 5.0, 1.0), Color::srgb(0.5, 0.5, 0.52),
            Transform::from_xyz(x, 2.5, -16.0),
        );
    }

    let mut rng = Rng::new(SEED);
    spawn_kind(commands, meshes, materials, &mut rng, SpawnKind::BrickWall, Vec3::new(0.0, 0.0, -8.0), 0.0);
    spawn_kind(commands, meshes, materials, &mut rng, SpawnKind::Stack, Vec3::new(-7.0, 0.0, -4.0), 0.0);
    spawn_kind(commands, meshes, materials, &mut rng, SpawnKind::Barrel, Vec3::new(5.0, 0.5, -3.0), 0.0);
    commands.insert_resource(SpawnRng(rng));

    let player = spawn_player(commands, assets);
    commands.entity(player).insert(WorldEntity);
}

pub fn setup_world(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Res<AssetServer>,
) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 4.0, 14.0).looking_at(Vec3::new(0.0, 1.0, 8.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight { illuminance: 12_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(8.0, 20.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    build_world(&mut commands, &mut meshes, &mut materials, &assets);
}

/// Set by the reset command; consumed by `reset_world`.
#[derive(Resource, Default)]
pub struct PendingReset(pub bool);

pub fn reset_world(
    mut pending: ResMut<PendingReset>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    assets: Res<AssetServer>,
    entities: Query<Entity, With<WorldEntity>>,
    mut hammer: ResMut<HammerState>,
    mut input: ResMut<PlayerInput>,
    mut rig: ResMut<CameraRig>,
    mut last: ResMut<LastHit>,
) {
    if !std::mem::take(&mut pending.0) {
        return;
    }
    for entity in &entities {
        commands.entity(entity).despawn();
    }
    hammer.reset();
    *input = PlayerInput::default();
    *rig = CameraRig::default();
    last.0 = None;
    build_world(&mut commands, &mut meshes, &mut materials, &assets);
}
