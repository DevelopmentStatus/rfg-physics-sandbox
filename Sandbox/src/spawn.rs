//! Spawnable test objects and the shared spawn path (ImGui today, the
//! debugger interface later).

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::character::{Facing, Player};
use crate::rng::Rng;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpawnKind {
    Box,
    Sphere,
    Barrel,
    Stack,
    BrickWall,
}

impl SpawnKind {
    pub const ALL: [SpawnKind; 5] =
        [SpawnKind::Box, SpawnKind::Sphere, SpawnKind::Barrel, SpawnKind::Stack, SpawnKind::BrickWall];

    pub fn label(self) -> &'static str {
        match self {
            SpawnKind::Box => "Crate",
            SpawnKind::Sphere => "Ball",
            SpawnKind::Barrel => "Barrel",
            SpawnKind::Stack => "Crate stack (3x3)",
            SpawnKind::BrickWall => "Brick wall (8x6)",
        }
    }
}

#[derive(Message, Clone, Copy)]
pub struct SpawnRequest(pub SpawnKind);

/// Marks every entity the reset command must remove and rebuild.
#[derive(Component)]
pub struct WorldEntity;

/// Dynamic bodies created through the spawner, for the object counter.
#[derive(Component)]
pub struct Spawned;

#[derive(Resource)]
pub struct SpawnRng(pub Rng);

pub fn spawn_dynamic(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    name: &'static str,
    mesh: Mesh,
    collider: Collider,
    color: Color,
    transform: Transform,
) {
    commands.spawn((
        WorldEntity,
        Spawned,
        Name::new(name),
        RigidBody::Dynamic,
        collider,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(color)),
        transform,
    ));
}

pub fn spawn_kind(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    rng: &mut Rng,
    kind: SpawnKind,
    at: Vec3,
    yaw: f32,
) {
    let base = Quat::from_rotation_y(yaw);
    match kind {
        SpawnKind::Box => {
            let tilt = Quat::from_euler(EulerRot::XYZ, rng.next_signed() * 0.3, yaw, rng.next_signed() * 0.3);
            spawn_dynamic(
                commands,
                meshes,
                materials,
                "Crate",
                Cuboid::new(1.0, 1.0, 1.0).into(),
                Collider::cuboid(1.0, 1.0, 1.0),
                Color::srgb(0.65, 0.5, 0.3),
                Transform::from_translation(at).with_rotation(tilt),
            );
        }
        SpawnKind::Sphere => spawn_dynamic(
            commands,
            meshes,
            materials,
            "Ball",
            Sphere::new(0.5).into(),
            Collider::sphere(0.5),
            Color::srgb(0.3, 0.55, 0.85),
            Transform::from_translation(at),
        ),
        SpawnKind::Barrel => spawn_dynamic(
            commands,
            meshes,
            materials,
            "Barrel",
            Cylinder::new(0.4, 1.0).into(),
            Collider::cylinder(0.4, 1.0),
            Color::srgb(0.75, 0.2, 0.15),
            Transform::from_translation(at).with_rotation(base),
        ),
        SpawnKind::Stack => {
            for layer in 0..3 {
                for col in 0..3 {
                    let local = Vec3::new(col as f32 * 1.02 - 1.02, 0.5 + layer as f32 * 1.0, 0.0);
                    spawn_dynamic(
                        commands,
                        meshes,
                        materials,
                        "Crate",
                        Cuboid::new(1.0, 1.0, 1.0).into(),
                        Collider::cuboid(1.0, 1.0, 1.0),
                        Color::srgb(0.6 + rng.next_f32() * 0.1, 0.45, 0.28),
                        Transform::from_translation(at.with_y(0.0) + base * local).with_rotation(base),
                    );
                }
            }
        }
        SpawnKind::BrickWall => {
            let (w, h, d) = (1.0, 0.5, 0.5);
            for row in 0..6 {
                for col in 0..8 {
                    let shift = if row % 2 == 0 { 0.0 } else { w * 0.5 };
                    let local = Vec3::new((col as f32 - 3.5) * (w + 0.01) + shift, h * 0.5 + row as f32 * (h + 0.005), 0.0);
                    spawn_dynamic(
                        commands,
                        meshes,
                        materials,
                        "Brick",
                        Cuboid::new(w, h, d).into(),
                        Collider::cuboid(w, h, d),
                        Color::srgb(0.55 + rng.next_f32() * 0.15, 0.3, 0.25),
                        Transform::from_translation(at.with_y(0.0) + base * local).with_rotation(base),
                    );
                }
            }
        }
    }
}

pub fn handle_spawn_requests(
    mut requests: MessageReader<SpawnRequest>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rng: ResMut<SpawnRng>,
    player: Single<(&Transform, &Facing), With<Player>>,
) {
    let (transform, facing) = *player;
    let forward = Quat::from_rotation_y(facing.0) * Vec3::NEG_Z;
    for request in requests.read() {
        let drop_height = match request.0 {
            SpawnKind::Stack | SpawnKind::BrickWall => 0.0,
            _ => 2.0,
        };
        let distance = match request.0 {
            SpawnKind::Stack | SpawnKind::BrickWall => 6.0,
            _ => 3.5,
        };
        let at = transform.translation + forward * distance + Vec3::Y * drop_height;
        spawn_kind(&mut commands, &mut meshes, &mut materials, &mut rng.0, request.0, at, facing.0);
    }
}
