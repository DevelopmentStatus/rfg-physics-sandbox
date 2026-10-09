//! Mason's animation, using the original RFG clips.
//!
//! `Extractors/build_assets.py` decodes the game's `.anim_pc` clips with the
//! decoder ported from the executable (E-017) and bakes the hammer, rifle and
//! remote-charge sets into `mason.glb`. This module picks clips by name and
//! drives their weights itself:
//! - locomotion blends idle/walk/jog/run/sprint with the ranges of the
//!   `Player_move_forward_HMR` blend tree (E-011), on one shared cycle phase so
//!   the feet stay in step while blending;
//! - one-shot clips (holster/draw, swings, firing, landing) play on top and can
//!   be cut by movement once their "release movement" point has passed;
//! - every change crossfades over [`BLEND_SECONDS`].

use std::collections::HashMap;

use avian3d::prelude::LinearVelocity;
use bevy::{animation::RepeatAnimation, gltf::Gltf, prelude::*};

use crate::character::{Grounded, Player, PlayerVisual};
use crate::input::PlayerInput;
use crate::tools::{ActiveTool, Equip, HammerState, SwitchPhase, Tool, ToolUse, ANIM_FPS};

/// Crossfade time between clips. The blend tree uses 0.15-0.3 s (E-011).
const BLEND_SECONDS: f32 = 0.2;
/// `rfg_PC` max_speed (E-010). HYPOTHESIS: the blend-tree control input is
/// horizontal speed divided by this.
const MAX_SPEED: f32 = 5.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Idle,
    Walk,
    Jog,
    Run,
    Sprint,
    InAir,
    Land,
    Draw,
    Holster,
    Primary,
    Secondary,
}

const LOCOMOTION: [Action; 5] = [Action::Idle, Action::Walk, Action::Jog, Action::Run, Action::Sprint];

/// Clip name for a tool's action (player.xtbl groups Hammer, Rifle, remote charge).
pub fn clip_name(tool: Tool, action: Action) -> &'static str {
    use Action::*;
    match tool {
        Tool::Hammer => match action {
            Idle => "plyr_hmr_std",
            Walk => "plyr_hmr_wlk",
            Jog => "plyr_hmr_jog",
            Run => "plyr_hmr_run",
            Sprint => "plyr_hmr_spnt",
            InAir => "plyr_hmr_jmp_inair",
            Land => "plyr_hmr_jmp_lnd",
            Draw => "plyr_hmr_drw",
            Holster => "plyr_hmr_hlstr",
            Primary => "plyr_hmr_mele",
            Secondary => "plyr_hmr_mele_b",
        },
        Tool::Rifle => match action {
            Idle => "plyr_rfl_std",
            Walk => "plyr_rfl_wlk",
            Jog => "plyr_rfl_jog",
            Run => "plyr_rfl_run",
            Sprint => "plyr_rfl_spnt",
            InAir => "plyr_rfl_jmp_inair",
            Land => "plyr_rfl_jmp_lnd",
            Draw => "plyr_rfl_drw",
            Holster => "plyr_rfl_hlstr",
            Primary => "plyr_rfl_sht",
            Secondary => "plyr_rfl_melee",
        },
        // The grenade has no clip set of its own here; it borrows the
        // remote-charge throw.
        Tool::RemoteCharge | Tool::Grenade => match action {
            Idle => "plyr_rmtchrg_nrd_std",
            Walk => "plyr_rmtchrg_nrd_wlk",
            Jog => "plyr_rmtchrg_jog",
            Run => "plyr_rmtchrg_nrd_run",
            Sprint => "plyr_rmtchrg_spnt",
            InAir => "plyr_rmtchrg_jmp_air",
            Land => "plyr_rmtchrg_jmp_lnd",
            Draw => "plyr_rmtchrg_drw",
            Holster => "plyr_rmtchrg_hlstr",
            Primary => "plyr_rmtchrg_thrw",
            Secondary => "plyr_rmtchrg_dtnt",
        },
    }
}

/// Piecewise-linear weight of each locomotion clip for blend-tree input `c`,
/// normalized to sum to 1. Ranges from `Player_move_forward_HMR` (E-011).
pub fn locomotion_weights(c: f32) -> [f32; 5] {
    fn ramp(points: &[(f32, f32)], c: f32) -> f32 {
        if c <= points[0].0 {
            return points[0].1;
        }
        for pair in points.windows(2) {
            let ((x0, y0), (x1, y1)) = (pair[0], pair[1]);
            if c <= x1 {
                return y0 + (y1 - y0) * (c - x0) / (x1 - x0);
            }
        }
        points[points.len() - 1].1
    }
    let raw = [
        ramp(&[(0.0, 1.0), (0.01, 1.0), (0.05, 0.0)], c),
        ramp(&[(0.01, 0.0), (0.05, 1.0), (0.4, 1.0), (0.5, 0.0)], c),
        ramp(&[(0.4, 0.0), (0.5, 1.0), (0.6, 1.0), (1.0, 0.0)], c),
        ramp(&[(0.6, 0.0), (1.0, 1.0), (1.5, 0.0)], c),
        ramp(&[(1.0, 0.0), (1.5, 1.0)], c),
    ];
    let sum: f32 = raw.iter().sum();
    raw.map(|w| w / sum)
}

#[derive(Resource)]
pub struct MasonGltf(pub Handle<Gltf>);

#[derive(Clone, Copy, Debug)]
pub struct Clip {
    pub node: AnimationNodeIndex,
    pub duration: f32,
}

/// Every clip in `mason.glb`, by name, in one animation graph.
#[derive(Resource)]
pub struct Clips {
    graph: Handle<AnimationGraph>,
    by_name: HashMap<String, Clip>,
}

impl Clips {
    pub fn get(&self, tool: Tool, action: Action) -> Option<Clip> {
        self.by_name.get(clip_name(tool, action)).copied()
    }

    /// Holster/draw clip length, used by [`Equip`]; 0 when missing.
    pub fn switch_length(&self, tool: Tool, phase: SwitchPhase) -> f32 {
        let action = match phase {
            SwitchPhase::Holster => Action::Holster,
            SwitchPhase::Draw => Action::Draw,
        };
        self.get(tool, action).map_or(0.0, |c| c.duration)
    }
}

#[derive(Clone, Copy, Debug)]
struct OneShot {
    clip: Clip,
    t: f32,
    /// Movement input may cut the clip after this many seconds.
    release: f32,
    /// Set while a holster/draw drives the clip time.
    switch: bool,
}

/// Per-player animation state, on the entity with the `AnimationPlayer`.
#[derive(Component, Default)]
pub struct Animator {
    weights: HashMap<AnimationNodeIndex, f32>,
    oneshot: Option<OneShot>,
    /// Shared locomotion cycle phase in [0, 1).
    phase: f32,
    in_air_time: f32,
    was_grounded: bool,
    was_swinging: bool,
}

pub fn load_mason(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(MasonGltf(assets.load("mason.glb")));
}

pub fn build_clips(
    mut commands: Commands,
    mason: Res<MasonGltf>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    clips: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    if !assets.is_loaded_with_dependencies(&mason.0) {
        return;
    }
    let Some(gltf) = gltfs.get(&mason.0) else { return };
    let named: Vec<_> = gltf.named_animations.iter().collect();
    let (graph, nodes) = AnimationGraph::from_clips(named.iter().map(|(_, h)| (*h).clone()));
    let by_name = named
        .iter()
        .zip(nodes)
        .map(|((name, handle), node)| {
            let duration = clips.get(*handle).map_or(0.0, |c| c.duration());
            (name.to_string(), Clip { node, duration })
        })
        .collect::<HashMap<_, _>>();
    info!("mason.glb: {} animation clips", by_name.len());
    commands.insert_resource(Clips { graph: graphs.add(graph), by_name });
}

/// Gives the player's `AnimationPlayer` (added by the glTF loader) the graph.
pub fn attach_animator(
    mut commands: Commands,
    clips: Res<Clips>,
    players: Query<Entity, (With<AnimationPlayer>, Without<Animator>)>,
    parents: Query<&ChildOf>,
    visuals: Query<(), With<PlayerVisual>>,
) {
    for entity in &players {
        if parents.iter_ancestors(entity).any(|a| visuals.contains(a)) {
            commands
                .entity(entity)
                .insert((AnimationGraphHandle(clips.graph.clone()), Animator::default()));
        }
    }
}

/// Moves [`Equip`] through holster and draw at the clips' real lengths.
pub fn advance_equip(time: Res<Time>, active: Res<ActiveTool>, clips: Option<Res<Clips>>, mut equip: ResMut<Equip>) {
    if equip.ready() && active.0 == equip.tool {
        return; // leave change detection alone while nothing happens
    }
    let length = |tool, phase| clips.as_ref().map_or(0.0, |c| c.switch_length(tool, phase));
    equip.advance(active.0, time.delta_secs(), length);
}

pub fn animate(
    time: Res<Time>,
    clips: Res<Clips>,
    equip: Res<Equip>,
    hammer: Res<HammerState>,
    input: Res<PlayerInput>,
    mut tool_use: ResMut<ToolUse>,
    player: Single<(&LinearVelocity, &Grounded), With<Player>>,
    mut animators: Query<(&mut AnimationPlayer, &mut Animator)>,
) {
    let dt = time.delta_secs();
    let (velocity, grounded) = player.into_inner();
    let speed = Vec2::new(velocity.x, velocity.z).length();
    let moving = input.move_axis != Vec2::ZERO;
    let used = tool_use.0.take();
    for (mut player, mut anim) in &mut animators {
        let anim = &mut *anim;

        // One-shot selection, most important first.
        if let Some((phase, t)) = equip.phase() {
            let action = if phase == SwitchPhase::Holster { Action::Holster } else { Action::Draw };
            if let Some(clip) = clips.get(equip.tool, action) {
                anim.oneshot = Some(OneShot { clip, t, release: f32::INFINITY, switch: true });
            }
        } else {
            if anim.oneshot.is_some_and(|o| o.switch) {
                anim.oneshot = None;
            }
            if hammer.is_swinging() && !anim.was_swinging {
                let spec = hammer.spec().expect("a swinging hammer has a spec");
                let action = if spec.name == "vert" { Action::Secondary } else { Action::Primary };
                if let Some(clip) = clips.get(Tool::Hammer, action) {
                    let release = spec.release_movement / ANIM_FPS;
                    anim.oneshot = Some(OneShot { clip, t: 0.0, release, switch: false });
                }
            } else if let Some(secondary) = used {
                let action = if secondary { Action::Secondary } else { Action::Primary };
                if let Some(clip) = clips.get(equip.tool, action) {
                    anim.oneshot = Some(OneShot { clip, t: 0.0, release: clip.duration * 0.5, switch: false });
                }
            } else if grounded.0 && !anim.was_grounded && anim.in_air_time > 0.25 && anim.oneshot.is_none() {
                if let Some(clip) = clips.get(equip.tool, Action::Land) {
                    anim.oneshot = Some(OneShot { clip, t: 0.0, release: 0.0, switch: false });
                }
            }
        }
        anim.was_swinging = hammer.is_swinging();
        anim.in_air_time = if grounded.0 { 0.0 } else { anim.in_air_time + dt };
        anim.was_grounded = grounded.0;

        if let Some(shot) = &mut anim.oneshot {
            if !shot.switch {
                shot.t += dt;
            }
            let cut = moving && shot.t >= shot.release;
            if !shot.switch && (shot.t >= shot.clip.duration || cut) {
                anim.oneshot = None;
            }
        }

        // Target weights for this frame.
        let mut targets: HashMap<AnimationNodeIndex, f32> = HashMap::new();
        let mut locomotion: Vec<(Clip, f32)> = Vec::new();
        if let Some(shot) = anim.oneshot {
            targets.insert(shot.clip.node, 1.0);
        } else if !grounded.0 && anim.in_air_time > 0.1 {
            if let Some(clip) = clips.get(equip.tool, Action::InAir) {
                targets.insert(clip.node, 1.0);
            }
        } else {
            let weights = locomotion_weights(speed / MAX_SPEED);
            for (action, w) in LOCOMOTION.iter().zip(weights) {
                if let Some(clip) = clips.get(equip.tool, *action) {
                    if w > 0.0 {
                        targets.insert(clip.node, w);
                        locomotion.push((clip, w));
                    }
                }
            }
            // Advance the shared phase by the blended cycle length.
            let cycle: f32 = locomotion.iter().map(|(c, w)| c.duration * w).sum();
            if cycle > 0.0 {
                anim.phase = (anim.phase + dt / cycle).fract();
            }
        }

        // Crossfade current weights toward the targets.
        let step = if BLEND_SECONDS > 0.0 { dt / BLEND_SECONDS } else { 1.0 };
        let mut nodes: Vec<AnimationNodeIndex> = anim.weights.keys().copied().collect();
        nodes.extend(targets.keys().filter(|n| !anim.weights.contains_key(n)));
        for node in nodes {
            let target = targets.get(&node).copied().unwrap_or(0.0);
            let current = anim.weights.get(&node).copied().unwrap_or(0.0);
            let w = if current < target { (current + step).min(target) } else { (current - step).max(target) };
            if w <= 0.0 && target <= 0.0 {
                anim.weights.remove(&node);
                player.stop(node);
                continue;
            }
            anim.weights.insert(node, w);
            if !player.is_playing_animation(node) {
                player.start(node);
            }
        }

        // Drive clip times: locomotion on the shared phase, one-shots on their
        // own clock, the in-air loop free-running.
        let locomotion_nodes: HashMap<AnimationNodeIndex, f32> =
            locomotion.iter().map(|(c, _)| (c.node, c.duration)).collect();
        let phase = anim.phase;
        for (node, w) in &anim.weights {
            let Some(active) = player.animation_mut(*node) else { continue };
            active.set_weight(*w);
            if let Some(duration) = locomotion_nodes.get(node) {
                active.set_speed(0.0).set_seek_time(phase * duration);
            } else if let Some(shot) = anim.oneshot.filter(|s| s.clip.node == *node) {
                active.set_speed(0.0).set_seek_time(shot.t.min(shot.clip.duration));
            } else {
                active.set_speed(1.0).set_repeat(RepeatAnimation::Forever);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn weights_always_sum_to_one() {
        for i in 0..=200 {
            let c = i as f32 * 0.01;
            let sum: f32 = locomotion_weights(c).iter().sum();
            assert!(close(sum, 1.0), "c={c} sum={sum}");
        }
    }

    #[test]
    fn standing_still_is_pure_idle() {
        assert_eq!(locomotion_weights(0.0), [1.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn blend_tree_peaks() {
        // Walk 0.05-0.4, jog 0.5-0.6, run at 1.0, sprint from 1.5 (E-011).
        assert!(close(locomotion_weights(0.2)[1], 1.0));
        assert!(close(locomotion_weights(0.55)[2], 1.0));
        assert!(close(locomotion_weights(1.0)[3], 1.0));
        assert!(close(locomotion_weights(1.7)[4], 1.0));
    }

    #[test]
    fn every_tool_has_every_action() {
        // Names must match clips baked by Extractors/build_assets.py.
        for tool in [Tool::Hammer, Tool::Rifle, Tool::RemoteCharge, Tool::Grenade] {
            for action in [
                Action::Idle,
                Action::Walk,
                Action::Jog,
                Action::Run,
                Action::Sprint,
                Action::InAir,
                Action::Land,
                Action::Draw,
                Action::Holster,
                Action::Primary,
                Action::Secondary,
            ] {
                assert!(clip_name(tool, action).starts_with("plyr_"));
            }
        }
    }
}
