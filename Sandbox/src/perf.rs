//! In-game performance monitor (ImGui "Performance" window): frame time, how many fixed steps the
//! simulation runs per frame, wall time of the fixed loop and of the buildings systems, Avian's
//! per-stage physics timings, and entity / body / collider counts.

use std::collections::VecDeque;
use std::time::Instant;

use avian3d::prelude::*;
use bevy::app::RunFixedMainLoopSystems;
use bevy::diagnostic::{DiagnosticsStore, EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use dear_imgui_bevy::prelude::*;
use dear_imgui_rs::Condition;

const HISTORY: usize = 240;

/// Timed spans, indexed by the const parameter of [`span_start`] / [`span_end`].
pub const SPAN_FIXED_LOOP: usize = 0;
pub const SPAN_BUILDINGS_UPDATE: usize = 1;
pub const SPAN_BUILDINGS_FIXED: usize = 2;
pub const SPAN_COAT: usize = 3;
const SPAN_NAMES: [&str; 4] = [
    "Fixed loop (gameplay + physics)",
    "Buildings: streaming / loading",
    "Buildings: damage / stress",
    "Coat cloth",
];

pub struct PerfPlugin;

impl Plugin for PerfPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin::default(),
            PhysicsDiagnosticsPlugin,
        ))
        .insert_resource(Perf::default())
        .add_systems(
            RunFixedMainLoop,
            (
                span_start::<SPAN_FIXED_LOOP>.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
                span_end::<SPAN_FIXED_LOOP>.in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
            ),
        )
        .add_systems(FixedFirst, count_fixed_step)
        .add_systems(Last, end_frame);
    }
}

#[derive(Resource)]
pub struct Perf {
    starts: [Option<Instant>; SPAN_NAMES.len()],
    acc: [f32; SPAN_NAMES.len()],
    fixed_steps: u32,
    frame_start: Option<Instant>,
    /// Last frame's values.
    pub spans_ms: [f32; SPAN_NAMES.len()],
    pub steps_last_frame: u32,
    pub frame_ms: VecDeque<f32>,
    pub fixed_ms: VecDeque<f32>,
    pub steps: VecDeque<f32>,
}

impl Default for Perf {
    fn default() -> Self {
        Self {
            starts: [None; SPAN_NAMES.len()],
            acc: [0.0; SPAN_NAMES.len()],
            fixed_steps: 0,
            frame_start: None,
            spans_ms: [0.0; SPAN_NAMES.len()],
            steps_last_frame: 0,
            frame_ms: VecDeque::with_capacity(HISTORY),
            fixed_ms: VecDeque::with_capacity(HISTORY),
            steps: VecDeque::with_capacity(HISTORY),
        }
    }
}

fn push(history: &mut VecDeque<f32>, value: f32) {
    if history.len() == HISTORY {
        history.pop_front();
    }
    history.push_back(value);
}

pub fn span_start<const ID: usize>(mut perf: ResMut<Perf>) {
    perf.starts[ID] = Some(Instant::now());
}

pub fn span_end<const ID: usize>(mut perf: ResMut<Perf>) {
    if let Some(start) = perf.starts[ID].take() {
        perf.acc[ID] += start.elapsed().as_secs_f32() * 1000.0;
    }
}

fn count_fixed_step(mut perf: ResMut<Perf>) {
    perf.fixed_steps += 1;
}

fn end_frame(mut perf: ResMut<Perf>) {
    let perf = &mut *perf;
    let now = Instant::now();
    if let Some(start) = perf.frame_start.replace(now) {
        push(&mut perf.frame_ms, (now - start).as_secs_f32() * 1000.0);
    }
    perf.spans_ms = perf.acc;
    perf.acc = [0.0; SPAN_NAMES.len()];
    perf.steps_last_frame = std::mem::take(&mut perf.fixed_steps);
    push(&mut perf.fixed_ms, perf.spans_ms[SPAN_FIXED_LOOP]);
    push(&mut perf.steps, perf.steps_last_frame as f32);
}

pub fn panel(
    frame: ImguiFrame<'_>,
    perf: Res<Perf>,
    diagnostics: Res<DiagnosticsStore>,
    bodies: Query<&RigidBody>,
    colliders: Query<(), With<Collider>>,
    fixed: Res<Time<Fixed>>,
    mut show_physics: Local<bool>,
) {
    let ui = frame.ui();
    ui.window("Performance")
        .size([360.0, 520.0], Condition::FirstUseEver)
        .position([720.0, 12.0], Condition::FirstUseEver)
        .build(|| {
            if cfg!(debug_assertions) {
                ui.text_colored([1.0, 0.7, 0.2, 1.0], "Dev build (dependencies optimized; Play.bat release for full speed)");
            } else {
                ui.text("Release build");
            }
            let n = perf.frame_ms.len().max(1) as f32;
            let avg = perf.frame_ms.iter().sum::<f32>() / n;
            let worst = perf.frame_ms.iter().copied().fold(0.0, f32::max);
            ui.text(format!(
                "FPS {:.1}   frame {:.1} ms avg, {:.1} ms worst (last {} frames)",
                1000.0 / avg.max(0.001),
                avg,
                worst,
                perf.frame_ms.len()
            ));
            let frames: Vec<f32> = perf.frame_ms.iter().copied().collect();
            ui.plot_lines_config("frame ms", &frames).scale_min(0.0).graph_size([0.0, 50.0]).build();

            let avg_steps = perf.steps.iter().sum::<f32>() / perf.steps.len().max(1) as f32;
            ui.text(format!(
                "Fixed steps per frame {:.2} avg (tick {:.1} ms)",
                avg_steps,
                fixed.timestep().as_secs_f32() * 1000.0
            ));
            if avg_steps > 1.5 {
                ui.text_colored(
                    [1.0, 0.4, 0.3, 1.0],
                    "Simulation is falling behind: each frame runs several fixed steps",
                );
            }
            let fixed_ms: Vec<f32> = perf.fixed_ms.iter().copied().collect();
            ui.plot_lines_config("fixed ms", &fixed_ms).scale_min(0.0).graph_size([0.0, 40.0]).build();

            ui.separator();
            ui.text("Last frame (ms)");
            for (name, ms) in SPAN_NAMES.iter().zip(perf.spans_ms) {
                ui.text(format!("{ms:8.2}  {name}"));
            }

            ui.separator();
            let (mut dynamic, mut fixed_bodies, mut kinematic) = (0, 0, 0);
            for body in &bodies {
                match body {
                    RigidBody::Dynamic => dynamic += 1,
                    RigidBody::Static => fixed_bodies += 1,
                    RigidBody::Kinematic => kinematic += 1,
                }
            }
            let entities = diagnostics
                .get(&EntityCountDiagnosticsPlugin::ENTITY_COUNT)
                .and_then(|d| d.value())
                .unwrap_or(0.0);
            ui.text(format!("Entities {entities:.0}   colliders {}", colliders.iter().count()));
            ui.text(format!("Rigid bodies: dynamic {dynamic}, static {fixed_bodies}, kinematic {kinematic}"));

            ui.separator();
            ui.checkbox("Physics stage timings (Avian)", &mut show_physics);
            if *show_physics {
                for d in diagnostics.iter().filter(|d| d.path().as_str().starts_with("avian")) {
                    if let Some(v) = d.smoothed() {
                        ui.text(format!("{v:9.2} {:<3} {}", d.suffix, d.path().as_str().trim_start_matches("avian/")));
                    }
                }
            }
        });
}
