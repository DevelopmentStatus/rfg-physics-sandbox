//! Samples devices once per rendered frame into resources that the fixed-step
//! gameplay systems consume, so pausing or stepping never drops or repeats an
//! input edge.

use avian3d::{math::AdjustPrecision, prelude::*};
use bevy::{
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions},
};
use dear_imgui_bevy::prelude::ImguiInputCapture;

use crate::character::{HALF_HEIGHT, Player};
use crate::clock::Command;
use crate::tools::{ActiveTool, Tool};
use crate::SimRequest;

const MOUSE_SENSITIVITY: f32 = 0.003;
const PITCH_MIN: f32 = -1.2;
const PITCH_MAX: f32 = 0.9;
const CAMERA_DISTANCE: f32 = 4.5;
const CAMERA_SHOULDER: f32 = 0.7;
const CAMERA_HEIGHT: f32 = 0.4;

#[derive(Resource, Default)]
pub struct PlayerInput {
    /// x = strafe right, y = forward.
    pub move_axis: Vec2,
    pub sprint: bool,
    /// Latched on press, cleared when the fixed step consumes it.
    pub jump: bool,
    pub attack: bool,
    /// Secondary attack (vertical hammer swing). Binding is a guess.
    pub attack_alt: bool,
}

#[derive(Resource)]
pub struct CameraRig {
    pub yaw: f32,
    pub pitch: f32,
}

impl Default for CameraRig {
    fn default() -> Self {
        Self { yaw: 0.0, pitch: -0.25 }
    }
}

/// True while the mouse is locked and drives the camera; the ImGui panels are
/// usable while it is false. Like a normal game: clicking into the scene locks
/// it, Esc toggles it, and losing window focus (Alt+Tab) frees it.
#[derive(Resource, Default)]
pub struct MouseLook(pub bool);

pub fn sim_hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<ImguiInputCapture>,
    mut requests: MessageWriter<SimRequest>,
) {
    if capture.aggregate().wants_keyboard_input() {
        return;
    }
    let bindings = [
        (KeyCode::KeyP, Command::TogglePause),
        (KeyCode::Enter, Command::Resume),
        (KeyCode::KeyN, Command::Step),
        (KeyCode::KeyR, Command::Reset),
    ];
    for (key, command) in bindings {
        if keys.just_pressed(key) {
            requests.write(SimRequest(command));
        }
    }
}

pub fn tool_hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    capture: Res<ImguiInputCapture>,
    mut active: ResMut<ActiveTool>,
) {
    if capture.aggregate().wants_keyboard_input() {
        return;
    }
    let bindings = [
        (KeyCode::Digit1, Tool::Hammer),
        (KeyCode::Digit2, Tool::Rifle),
        (KeyCode::Digit3, Tool::RemoteCharge),
        (KeyCode::Digit4, Tool::Grenade),
    ];
    for (key, tool) in bindings {
        if keys.just_pressed(key) {
            active.0 = tool;
        }
    }
}

pub fn sample_player_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    capture: Res<ImguiInputCapture>,
    look: Res<MouseLook>,
    mut input: ResMut<PlayerInput>,
) {
    let typing = capture.aggregate().wants_keyboard_input();
    let mut axis = Vec2::ZERO;
    if !typing {
        axis.y += keys.pressed(KeyCode::KeyW) as i32 as f32;
        axis.y -= keys.pressed(KeyCode::KeyS) as i32 as f32;
        axis.x += keys.pressed(KeyCode::KeyD) as i32 as f32;
        axis.x -= keys.pressed(KeyCode::KeyA) as i32 as f32;
        if keys.just_pressed(KeyCode::Space) {
            input.jump = true;
        }
    }
    input.move_axis = axis.normalize_or_zero();
    input.sprint = !typing && keys.pressed(KeyCode::ShiftLeft);
    // With the cursor free, a click either belongs to an ImGui panel or locks
    // the mouse (toggle_mouse_look); only locked clicks attack.
    let in_game = look.0;
    if in_game && mouse.just_pressed(MouseButton::Left) {
        input.attack = true;
    }
    if in_game && mouse.just_pressed(MouseButton::Right) {
        input.attack_alt = true;
    }
}

/// Runs after [`sample_player_input`], so the click that locks the mouse does
/// not also swing. A locked cursor is hidden, confined to the window and held
/// at its centre (Windows has no true pointer lock, so the cursor would
/// otherwise stay wherever it was, possibly on another monitor). Camera motion
/// comes from raw mouse deltas, so recentring does not move the camera.
pub fn toggle_mouse_look(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    capture: Res<ImguiInputCapture>,
    mut look: ResMut<MouseLook>,
    window: Single<(&mut Window, &mut CursorOptions)>,
) {
    let (mut window, mut cursor) = window.into_inner();
    // Esc always toggles, even while ImGui has keyboard focus, so the cursor
    // can never get stuck.
    if keys.just_pressed(KeyCode::Escape) {
        look.0 = !look.0;
    }
    if !window.focused {
        look.0 = false;
    } else if !look.0 && mouse.just_pressed(MouseButton::Left) && !capture.aggregate().wants_pointer_input() {
        look.0 = true;
    }
    let (visible, grab) = if look.0 {
        (false, CursorGrabMode::Confined)
    } else {
        (true, CursorGrabMode::None)
    };
    if cursor.visible != visible || cursor.grab_mode != grab {
        cursor.visible = visible;
        cursor.grab_mode = grab;
    }
    if look.0 {
        let centre = Vec2::new(window.width(), window.height()) * 0.5;
        if window.cursor_position().is_none_or(|p| p.distance(centre) > 1.0) {
            window.set_cursor_position(Some(centre));
        }
    }
}

pub fn orbit_camera(
    motion: Res<AccumulatedMouseMotion>,
    look: Res<MouseLook>,
    mut rig: ResMut<CameraRig>,
    spatial: SpatialQuery,
    player: Single<(Entity, &Transform), With<Player>>,
    mut camera: Single<&mut Transform, (With<Camera3d>, Without<Player>)>,
) {
    if look.0 {
        rig.yaw -= motion.delta.x * MOUSE_SENSITIVITY;
        rig.pitch = (rig.pitch - motion.delta.y * MOUSE_SENSITIVITY).clamp(PITCH_MIN, PITCH_MAX);
    }
    let (entity, player_transform) = player.into_inner();
    let rotation = Quat::from_euler(EulerRot::YXZ, rig.yaw, rig.pitch, 0.0);
    // Aim point is around head height, offset to the right shoulder.
    let pivot = player_transform.translation + Vec3::Y * (HALF_HEIGHT * 0.5 + CAMERA_HEIGHT)
        + rotation * Vec3::X * CAMERA_SHOULDER;
    let back = rotation * Vec3::Z;
    let mut distance = CAMERA_DISTANCE;
    if let Ok(dir) = Dir3::new(back) {
        let filter = SpatialQueryFilter::from_excluded_entities([entity]);
        if let Some(hit) = spatial.cast_ray(pivot.adjust_precision(), dir, CAMERA_DISTANCE, true, &filter) {
            distance = (hit.distance - 0.3).max(0.3);
        }
    }
    camera.translation = pivot + back * distance;
    camera.rotation = rotation;
}
