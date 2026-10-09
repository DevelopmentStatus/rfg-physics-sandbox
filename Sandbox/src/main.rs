#![allow(clippy::too_many_arguments, clippy::type_complexity)] // Bevy system signatures

mod animation;
mod buildings;
mod character;
mod cloth;
mod clock;
mod input;
mod camera_shake;
mod perf;
mod rng;
mod spawn;
mod tools;
mod ui;
mod weapons;
mod world;

use avian3d::prelude::*;
use bevy::{app::FixedMain, prelude::*};
use dear_imgui_bevy::prelude::*;

use clock::{Command, SimClock, TICK_SECONDS};

pub const SEED: u64 = 0x5EED_0001;

#[derive(Resource)]
pub struct Clock(pub SimClock);

/// A control request. The keyboard and the ImGui panel write these now; the
/// debugger interface can write them later without touching the simulation.
#[derive(Message, Clone, Copy)]
pub struct SimRequest(pub Command);

fn main() {
    let mut app = App::new();
    // Game-derived assets are converted into LocalData/Assets and loaded from
    // there at runtime; they are never copied into the source tree.
    app.add_plugins((
        DefaultPlugins.set(AssetPlugin {
            file_path: "../LocalData/Assets".into(),
            ..default()
        }),
        PhysicsPlugins::default(),
    ));
    app.add_plugins((buildings::BuildingsPlugin, perf::PerfPlugin));
    app.try_install_imgui(ImguiPlugin::default())
        .expect("the Dear ImGui configuration is valid");

    app.insert_resource(Time::<Fixed>::from_hz(1.0 / TICK_SECONDS))
        .insert_resource(Clock(SimClock::new()))
        .insert_resource(input::PlayerInput::default())
        .insert_resource(input::CameraRig::default())
        .insert_resource(input::MouseLook::default())
        .insert_resource(tools::ActiveTool(tools::Tool::Hammer))
        .insert_resource(tools::Equip::new(tools::Tool::Hammer))
        .insert_resource(tools::ToolUse::default())
        .insert_resource(tools::HammerState::default())
        .insert_resource(tools::LastHit::default())
        .insert_resource(weapons::HeldWeapon(weapons::weapon_for_tool(tools::Tool::Hammer)))
        .insert_resource(weapons::WeaponCatalog::load())
        .insert_resource(camera_shake::CameraShake::load())
        .add_message::<camera_shake::ShakeRequest>()
        .insert_resource(world::PendingReset::default())
        .insert_resource(cloth::CoatSettings::default())
        .add_message::<SimRequest>()
        .add_message::<spawn::SpawnRequest>()
        .add_message::<tools::HitEvent>()
        .add_systems(Startup, (world::setup_world, ui::setup_hud, animation::load_mason))
        .add_systems(
            Update,
            (
                (input::sim_hotkeys, input::tool_hotkeys, input::sample_player_input, input::toggle_mouse_look).chain(),
                apply_requests,
                world::reset_world,
                spawn::handle_spawn_requests,
                run_step,
                character::face_visual,
                animation::build_clips.run_if(not(resource_exists::<animation::Clips>)),
                animation::attach_animator.run_if(resource_exists::<animation::Clips>),
                animation::advance_equip,
                weapons::sync_tool,
                weapons::cycle_hotkeys,
                weapons::attach_weapon,
                cloth::attach_coat_sim,
                animation::animate.run_if(resource_exists::<animation::Clips>),
                (input::orbit_camera, camera_shake::start_shakes, camera_shake::apply_shake).chain(),
                ui::update_hud,
            )
                .chain(),
        )
        .add_systems(
            PostUpdate,
            (perf::span_start::<{ perf::SPAN_COAT }>, cloth::simulate_coat, perf::span_end::<{ perf::SPAN_COAT }>)
                .chain()
                .after(bevy::app::AnimationSystems)
                .before(TransformSystems::Propagate),
        )
        .add_systems(
            FixedUpdate,
            (count_tick, character::move_player, tools::hammer_system).chain(),
        );

    let pass = app.imgui_primary_pass().expect("the ImGui pass registry must be active");
    app.add_imgui_systems(&pass, pass.system(ui::control_panel))
        .expect("the panel must bind to this App and pass");
    app.add_imgui_systems(&pass, pass.system(buildings::panel))
        .expect("the buildings panel must bind to this App and pass");
    app.add_imgui_systems(&pass, pass.system(perf::panel))
        .expect("the performance panel must bind to this App and pass");
    app.run();
}

fn count_tick(mut clock: ResMut<Clock>) {
    clock.0.count_tick();
}

fn apply_requests(
    mut requests: MessageReader<SimRequest>,
    mut clock: ResMut<Clock>,
    mut pending: ResMut<world::PendingReset>,
    mut virtual_time: ResMut<Time<Virtual>>,
) {
    for request in requests.read() {
        clock.0.apply(request.0);
        if request.0 == Command::Reset {
            pending.0 = true;
        }
    }
    // Pausing virtual time stops FixedUpdate and physics together.
    if clock.0.is_paused() != virtual_time.is_paused() {
        if clock.0.is_paused() {
            virtual_time.pause();
        } else {
            virtual_time.unpause();
        }
    }
}

/// Runs exactly one fixed tick (gameplay and physics) for a Step command.
fn run_step(world: &mut World) {
    if !world.resource_mut::<Clock>().0.take_step() {
        return;
    }
    let step = world.resource::<Time<Fixed>>().timestep();
    world.resource_mut::<Time<Fixed>>().advance_by(step);
    *world.resource_mut::<Time>() = world.resource::<Time<Fixed>>().as_generic();
    world.run_schedule(FixedMain);
    *world.resource_mut::<Time>() = world.resource::<Time<Virtual>>().as_generic();
}
