//! Dear ImGui control panel plus a small in-game HUD.

use avian3d::prelude::*;
use bevy::prelude::*;
use dear_imgui_bevy::prelude::*;
use dear_imgui_rs::Condition;

use crate::character::{Grounded, Player};
use crate::cloth::CoatSettings;
use crate::clock::Command;
use crate::input::MouseLook;
use crate::spawn::{Spawned, SpawnKind, SpawnRequest};
use crate::tools::{ActiveTool, LastHit, Tool};
use crate::{Clock, SimRequest};

#[derive(Component)]
pub struct Hud;

pub fn setup_hud(mut commands: Commands) {
    commands.spawn((
        Hud,
        Text::new(""),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(12.0),
            left: Val::Px(12.0),
            ..default()
        },
    ));
}

pub fn update_hud(
    clock: Res<Clock>,
    tool: Res<ActiveTool>,
    held: Res<crate::weapons::HeldWeapon>,
    look: Res<MouseLook>,
    mut hud: Single<&mut Text, With<Hud>>,
) {
    let state = if clock.0.is_paused() { "PAUSED" } else { "RUNNING" };
    let mode = if look.0 { "mouse locked (Esc or Alt+Tab frees it)" } else { "cursor free (click the scene to lock)" };
    let tools: String = [Tool::Hammer, Tool::Rifle, Tool::RemoteCharge, Tool::Grenade]
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mark = if *t == tool.0 { ">" } else { " " };
            let todo = if t.implemented() { "" } else { " (not implemented)" };
            format!("{mark}{} {}{todo}\n", i + 1, t.name())
        })
        .collect();
    hud.0 = format!(
        "{state}  tick {}\n{tools}Held model: {}  ([ / ] cycle)\n{mode}\nWASD move  Shift sprint  Space jump  LMB/RMB hammer swing\nP pause  Enter resume  N step  R reset",
        clock.0.tick(),
        held.name()
    );
}

pub fn control_panel(
    frame: ImguiFrame<'_>,
    clock: Res<Clock>,
    mut tool: ResMut<ActiveTool>,
    last: Res<LastHit>,
    look: Res<MouseLook>,
    player: Single<(&Transform, &LinearVelocity, &Grounded), With<Player>>,
    spawned: Query<(), With<Spawned>>,
    mut sim: MessageWriter<SimRequest>,
    mut spawns: MessageWriter<SpawnRequest>,
    mut coat: ResMut<CoatSettings>,
) {
    let ui = frame.ui();
    let (transform, velocity, grounded) = *player;
    ui.window("Sandbox")
        .size([300.0, 420.0], Condition::FirstUseEver)
        .position([12.0, 12.0], Condition::FirstUseEver)
        .build(|| {
            ui.text(format!(
                "{}  tick {}",
                if clock.0.is_paused() { "PAUSED" } else { "RUNNING" },
                clock.0.tick()
            ));
            if ui.button("Pause/Resume") {
                sim.write(SimRequest(Command::TogglePause));
            }
            ui.same_line();
            if ui.button("Step") {
                sim.write(SimRequest(Command::Step));
            }
            ui.same_line();
            if ui.button("Reset") {
                sim.write(SimRequest(Command::Reset));
            }

            ui.separator();
            ui.text("Spawn (in front of player)");
            for kind in SpawnKind::ALL {
                if ui.button(kind.label()) {
                    spawns.write(SpawnRequest(kind));
                }
            }
            ui.text(format!("Spawned bodies: {}", spawned.iter().count()));

            ui.separator();
            ui.text("Tool");
            for (i, t) in [Tool::Hammer, Tool::Rifle, Tool::RemoteCharge, Tool::Grenade].into_iter().enumerate() {
                if ui.radio_button_bool(format!("{} {}", i + 1, t.name()), tool.0 == t) {
                    tool.0 = t;
                }
            }

            ui.separator();
            // Spring-bone approximation; off restores the rigid coat.
            let mut settings = *coat;
            ui.checkbox("Coat cloth sim", &mut settings.enabled);
            ui.slider("Coat damping", 0.0, 10.0, &mut settings.damping);
            ui.slider("Coat stiffness", 0.1, 4.0, &mut settings.stiffness);
            if settings != *coat {
                *coat = settings;
            }

            ui.separator();
            let p = transform.translation;
            ui.text(format!("pos {:.2} {:.2} {:.2}", p.x, p.y, p.z));
            ui.text(format!("vel {:.2} {:.2} {:.2}  grounded {}", velocity.x, velocity.y, velocity.z, grounded.0));
            if let Some(hit) = last.0 {
                ui.text(format!(
                    "last hit {:?} speed {:.1} structure dmg {:.0}",
                    hit.target, hit.speed, hit.structure_damage
                ));
            }
            ui.text(if look.0 { "Mouse locked (Esc)" } else { "Mouse free (Esc)" });
        });
}
