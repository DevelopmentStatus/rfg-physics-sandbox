//! Third-person character: kinematic capsule driven by Avian's move-and-slide.
//! All numbers are placeholders until measured from RFG.

use avian3d::{math::AdjustPrecision, prelude::*};
use bevy::prelude::*;

use crate::clock::TICK_SECONDS;
use crate::input::{CameraRig, PlayerInput};

pub const RADIUS: f32 = 0.35;
pub const BODY_LENGTH: f32 = 1.0;
/// Distance from the capsule center to the feet.
pub const HALF_HEIGHT: f32 = RADIUS + BODY_LENGTH * 0.5;
pub const SPAWN_POINT: Vec3 = Vec3::new(0.0, HALF_HEIGHT + 0.05, 8.0);

pub const WALK_SPEED: f32 = 5.0;
pub const SPRINT_SPEED: f32 = 8.5;
pub const GROUND_ACCEL: f32 = 60.0;
pub const AIR_ACCEL: f32 = 12.0;
pub const GRAVITY: f32 = 25.0;
pub const JUMP_SPEED: f32 = 8.0;
const GROUND_PROBE: f32 = 0.12;

#[derive(Component)]
pub struct Player;

/// Yaw the character body faces, in radians.
#[derive(Component, Default)]
pub struct Facing(pub f32);

#[derive(Component, Default)]
pub struct Grounded(pub bool);

/// World-space horizontal wish direction for stick input and camera yaw.
pub fn wish_direction(axis: Vec2, yaw: f32) -> Vec3 {
    let rotation = Quat::from_rotation_y(yaw);
    let local = Vec3::new(axis.x, 0.0, -axis.y);
    (rotation * local).normalize_or_zero()
}

/// One fixed tick of character velocity: accelerate horizontally toward the
/// wish velocity, apply gravity, and jump when grounded.
pub fn step_velocity(velocity: Vec3, wish: Vec3, speed: f32, grounded: bool, jump: bool, dt: f32) -> Vec3 {
    let mut v = velocity;
    let target = wish * speed;
    let accel = if grounded { GROUND_ACCEL } else { AIR_ACCEL };
    let horizontal = Vec3::new(v.x, 0.0, v.z);
    let delta = target - horizontal;
    let max_change = accel * dt;
    let change = if delta.length() <= max_change { delta } else { delta.normalize() * max_change };
    v.x += change.x;
    v.z += change.z;

    if grounded {
        v.y = if jump { JUMP_SPEED } else { v.y.max(0.0) };
    } else {
        v.y -= GRAVITY * dt;
    }
    v
}

pub fn spawn_player(commands: &mut Commands, assets: &AssetServer) -> Entity {
    let body = commands
        .spawn((
            Player,
            Name::new("Player"),
            Facing::default(),
            Grounded::default(),
            Collider::capsule(RADIUS, BODY_LENGTH),
            RigidBody::Kinematic,
            CustomPositionIntegration,
            TransformInterpolation,
            Transform::from_translation(SPAWN_POINT),
            Visibility::default(),
        ))
        .id();
    // The visual lives on a child so the capsule stays unrotated for physics.
    // The converted model has its feet at y = 0 (E-008), the capsule center is
    // HALF_HEIGHT above the feet.
    commands.entity(body).with_children(|parent| {
        parent.spawn((
            PlayerVisual,
            WorldAssetRoot(assets.load("mason.glb#Scene0")),
            Transform::from_xyz(0.0, -HALF_HEIGHT, 0.0),
        ));
    });
    body
}

#[derive(Component)]
pub struct PlayerVisual;

pub fn move_player(
    mut input: ResMut<PlayerInput>,
    rig: Res<CameraRig>,
    spatial: SpatialQuery,
    move_and_slide: MoveAndSlide,
    mut players: Query<
        (Entity, &mut Transform, &mut LinearVelocity, &Collider, &mut Grounded, &mut Facing),
        With<Player>,
    >,
    swinging: Res<crate::tools::HammerState>,
) {
    let dt = TICK_SECONDS as f32;
    for (entity, mut transform, mut velocity, collider, mut grounded, mut facing) in &mut players {
        let filter = SpatialQueryFilter::from_excluded_entities([entity]);
        let origin = transform.translation.adjust_precision();
        grounded.0 = velocity.y <= 0.1
            && spatial
                .cast_ray(origin, Dir3::NEG_Y, HALF_HEIGHT + GROUND_PROBE, true, &filter)
                .is_some();

        let wish = if swinging.movement_locked() {
            Vec3::ZERO
        } else {
            wish_direction(input.move_axis, rig.yaw)
        };
        let speed = if input.sprint { SPRINT_SPEED } else { WALK_SPEED };
        // The swing animation carries the Zero_Input flag: no player movement
        // until its "release movement" trigger (E-006).
        let speed = if swinging.movement_locked() { 0.0 } else { speed };
        let jump = std::mem::take(&mut input.jump) && grounded.0;
        velocity.0 = step_velocity(velocity.0, wish, speed, grounded.0, jump, dt);

        if swinging.is_swinging() {
            facing.0 = rig.yaw;
        } else if wish != Vec3::ZERO {
            let target = (-wish.x).atan2(-wish.z);
            facing.0 = turn_toward(facing.0, target, 14.0 * dt);
        }

        let out = move_and_slide.move_and_slide(
            collider,
            transform.translation.adjust_precision(),
            transform.rotation.adjust_precision(),
            velocity.0,
            core::time::Duration::from_secs_f64(TICK_SECONDS),
            &MoveAndSlideConfig::default(),
            &filter,
            |_| MoveAndSlideHitResponse::Accept,
        );
        transform.translation = out.position;
        velocity.0 = out.projected_velocity;
    }
}

pub fn face_visual(
    players: Query<(&Facing, &Children), With<Player>>,
    mut visuals: Query<&mut Transform, With<PlayerVisual>>,
) {
    for (facing, children) in &players {
        for child in children.iter() {
            if let Ok(mut transform) = visuals.get_mut(child) {
                transform.rotation = Quat::from_rotation_y(facing.0);
            }
        }
    }
}

/// Rotates `current` toward `target` by at most `max_step` radians along the
/// shortest arc.
pub fn turn_toward(current: f32, target: f32, max_step: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let mut diff = (target - current) % tau;
    if diff > std::f32::consts::PI {
        diff -= tau;
    } else if diff < -std::f32::consts::PI {
        diff += tau;
    }
    current + diff.clamp(-max_step, max_step)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = TICK_SECONDS as f32;

    #[test]
    fn forward_input_follows_camera_yaw() {
        let ahead = wish_direction(Vec2::new(0.0, 1.0), 0.0);
        assert!((ahead - Vec3::NEG_Z).length() < 1e-5);
        let turned = wish_direction(Vec2::new(0.0, 1.0), std::f32::consts::FRAC_PI_2);
        assert!((turned - Vec3::NEG_X).length() < 1e-5);
    }

    #[test]
    fn diagonal_wish_is_unit_length() {
        assert!((wish_direction(Vec2::new(1.0, 1.0), 0.3).length() - 1.0).abs() < 1e-5);
        assert_eq!(wish_direction(Vec2::ZERO, 1.0), Vec3::ZERO);
    }

    #[test]
    fn grounded_character_reaches_walk_speed_without_overshoot() {
        let mut v = Vec3::ZERO;
        for _ in 0..120 {
            v = step_velocity(v, Vec3::NEG_Z, WALK_SPEED, true, false, DT);
        }
        assert!((v.z + WALK_SPEED).abs() < 1e-4);
        assert!(v.x.abs() < 1e-6 && v.y == 0.0);
    }

    #[test]
    fn jump_only_works_when_grounded() {
        let grounded = step_velocity(Vec3::ZERO, Vec3::ZERO, 0.0, true, true, DT);
        assert_eq!(grounded.y, JUMP_SPEED);
        let airborne = step_velocity(Vec3::ZERO, Vec3::ZERO, 0.0, false, true, DT);
        assert!(airborne.y < 0.0);
    }

    #[test]
    fn jump_arc_returns_to_start_height() {
        let mut v = step_velocity(Vec3::ZERO, Vec3::ZERO, 0.0, true, true, DT);
        let mut y = 0.0;
        let mut ticks = 0;
        while ticks < 600 {
            y += v.y * DT;
            if y <= 0.0 {
                break;
            }
            v = step_velocity(v, Vec3::ZERO, 0.0, false, false, DT);
            ticks += 1;
        }
        // Flight time is about 2 * JUMP_SPEED / GRAVITY = 0.64 s.
        let seconds = ticks as f32 * DT;
        assert!((seconds - 2.0 * JUMP_SPEED / GRAVITY).abs() < 0.1, "{seconds}");
    }

    #[test]
    fn turn_toward_takes_shortest_arc() {
        let pi = std::f32::consts::PI;
        let r = turn_toward(pi - 0.1, -pi + 0.1, 1.0);
        assert!((r - (pi + 0.1)).abs() < 1e-5);
        assert_eq!(turn_toward(0.0, 0.2, 1.0), 0.2);
        assert_eq!(turn_toward(0.0, 3.0, 0.5), 0.5);
    }
}
