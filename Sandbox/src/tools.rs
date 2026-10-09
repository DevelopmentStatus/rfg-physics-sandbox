//! Tool belt (hotkeys 1-4) and the hammer.
//!
//! Hammer numbers come from the original game's tables and animation trigger
//! data; see `Research/evidence_registry.md` (E-001..E-003, E-006). Anything
//! not yet measured is marked UNMEASURED.

use avian3d::{math::AdjustPrecision, prelude::*};
use bevy::prelude::*;

use crate::character::{Facing, HALF_HEIGHT, Player};
use crate::input::PlayerInput;

/// Animation frame rate used to turn trigger frames into seconds.
/// HYPOTHESIS (medium confidence): 30 fps. Run-cycle foot triggers are 12
/// frames apart, which only gives a plausible cadence at 30 fps. To confirm
/// from the executable's animation player.
pub const ANIM_FPS: f32 = 30.0;

/// One hammer swing: trigger frames from `anim_files.xtbl` (E-006) and hit
/// data from the Player AnimSet of the matching `melee.xtbl` move (E-001/2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwingSpec {
    pub name: &'static str,
    /// Trigger "impact start": first hit test.
    pub impact_start: f32,
    /// Trigger "secondary impact": second hit test.
    pub secondary_impact: f32,
    /// Trigger "impact end".
    pub impact_end: f32,
    /// Trigger "release movement": player input is free again.
    pub release_movement: f32,
    pub hit_range: f32,
    pub secondary_range: f32,
    pub hit_radius: f32,
    pub hit_height: f32,
    pub structure_damage: f32,
    pub impact_force: f32,
    /// `CameraShake` of the Player AnimSet, played when a sweep hits (`FUN_007d8770`).
    pub camera_shake: &'static str,
}

/// `sledgehammer_melee_side`, action `melee`, `plyr_hmr_mele.animx` (52 frames).
pub const SWING_SIDE: SwingSpec = SwingSpec {
    name: "side",
    impact_start: 5.0,
    secondary_impact: 8.0,
    impact_end: 10.0,
    release_movement: 22.0,
    hit_range: 2.0,
    secondary_range: 1.9,
    hit_radius: 0.4,
    hit_height: 1.4,
    structure_damage: 1500.0,
    impact_force: 350.0,
    camera_shake: "hammer_alt_impact",
};

/// `sledgehammer_melee_vert`, action `melee2`, `plyr_hmr_mele_b.animx` (56 frames).
pub const SWING_VERT: SwingSpec = SwingSpec {
    name: "vert",
    impact_start: 6.0,
    secondary_impact: 9.0,
    impact_end: 11.0,
    release_movement: 26.0,
    hit_range: 2.2,
    secondary_range: 2.0,
    hit_radius: 0.4,
    hit_height: 1.3,
    structure_damage: 1500.0,
    impact_force: 300.0,
    camera_shake: "hammer_impact",
};

// UNMEASURED: velocity change given to a struck body. The engine's impulse
// model for ImpactForce is not reverse engineered yet.
pub const HAMMER_SPEED: f32 = 9.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Hammer,
    Rifle,
    RemoteCharge,
    Grenade,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Hammer => "Sledgehammer",
            Tool::Rifle => "Rifle",
            Tool::RemoteCharge => "Remote Charge",
            Tool::Grenade => "Grenade",
        }
    }

    pub fn implemented(self) -> bool {
        self == Tool::Hammer
    }
}

/// The tool the player asked for (hotkeys, panel). [`Equip`] follows it
/// through the holster and draw animations.
#[derive(Resource)]
pub struct ActiveTool(pub Tool);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwitchPhase {
    /// Putting the old tool away (`<set>_hlstr`).
    Holster,
    /// Taking the new tool out (`<set>_drw`).
    Draw,
}

/// The tool in Mason's hands. Switching plays the old tool's holster clip,
/// swaps the model, then plays the new tool's draw clip; tools cannot be used
/// until the draw finishes.
#[derive(Resource, Debug)]
pub struct Equip {
    pub tool: Tool,
    switch: Option<(Tool, SwitchPhase, f32)>,
}

impl Equip {
    pub fn new(tool: Tool) -> Self {
        Self { tool, switch: None }
    }

    pub fn ready(&self) -> bool {
        self.switch.is_none()
    }

    /// Current switch phase and its elapsed seconds.
    pub fn phase(&self) -> Option<(SwitchPhase, f32)> {
        self.switch.map(|(_, phase, t)| (phase, t))
    }

    /// Advances a switch toward `requested`. `length` gives the clip length in
    /// seconds of a phase for a tool (holster: old tool, draw: new tool).
    pub fn advance(&mut self, requested: Tool, dt: f32, length: impl Fn(Tool, SwitchPhase) -> f32) {
        match self.switch {
            None if requested != self.tool => self.switch = Some((requested, SwitchPhase::Holster, 0.0)),
            None => {}
            Some((to, SwitchPhase::Holster, t)) => {
                let t = t + dt;
                if t >= length(self.tool, SwitchPhase::Holster) {
                    self.tool = to;
                    self.switch = Some((to, SwitchPhase::Draw, 0.0));
                } else {
                    // A new request while holstering just changes the destination.
                    self.switch = Some((requested, SwitchPhase::Holster, t));
                }
            }
            Some((to, SwitchPhase::Draw, t)) => {
                let t = t + dt;
                if requested != to {
                    self.switch = Some((requested, SwitchPhase::Holster, 0.0));
                } else if t >= length(to, SwitchPhase::Draw) {
                    self.switch = None;
                } else {
                    self.switch = Some((to, SwitchPhase::Draw, t));
                }
            }
        }
    }
}

/// A non-hammer tool was used on this tick (`Some(true)` = secondary). Only the
/// animation consumes it for now; those tools have no gameplay effect yet.
#[derive(Resource, Default)]
pub struct ToolUse(pub Option<bool>);

/// Reported to the destruction pipeline later; for now it feeds the HUD.
#[derive(Message, Clone, Copy)]
pub struct HitEvent {
    pub target: Entity,
    #[allow(dead_code)] // read by the destruction pipeline once it exists
    pub direction: Vec3,
    pub speed: f32,
    pub structure_damage: f32,
}

#[derive(Resource, Default)]
pub struct LastHit(pub Option<HitEvent>);

/// A swing trigger that fired on a tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwingEvent {
    ImpactStart,
    SecondaryImpact,
    ImpactEnd,
    ReleaseMovement,
}

/// Swing state machine driven by animation trigger frames.
#[derive(Resource, Default, Debug)]
pub struct HammerState {
    active: Option<(SwingSpec, f32)>,
}

impl HammerState {
    pub fn is_swinging(&self) -> bool {
        self.active.is_some()
    }

    /// Player input is blocked until the "release movement" trigger.
    pub fn movement_locked(&self) -> bool {
        self.active.is_some()
    }

    pub fn spec(&self) -> Option<SwingSpec> {
        self.active.map(|(spec, _)| spec)
    }

    /// Starts `start` when idle and returns the triggers crossed this tick.
    pub fn advance(&mut self, dt: f32, start: Option<SwingSpec>) -> Vec<SwingEvent> {
        let mut events = Vec::new();
        if self.active.is_none() {
            match start {
                Some(spec) => self.active = Some((spec, 0.0)),
                None => return events,
            }
        }
        let Some((spec, t)) = self.active else {
            return events;
        };
        let before = t * ANIM_FPS;
        let after = (t + dt) * ANIM_FPS;
        // A trigger fires on the first tick whose frame reaches it.
        let crossed = |frame: f32| before < frame - 1e-3 && after >= frame - 1e-3;
        if crossed(spec.impact_start) {
            events.push(SwingEvent::ImpactStart);
        }
        if crossed(spec.secondary_impact) {
            events.push(SwingEvent::SecondaryImpact);
        }
        if crossed(spec.impact_end) {
            events.push(SwingEvent::ImpactEnd);
        }
        if crossed(spec.release_movement) {
            events.push(SwingEvent::ReleaseMovement);
            self.active = None;
        } else {
            self.active = Some((spec, t + dt));
        }
        events
    }

    pub fn reset(&mut self) {
        self.active = None;
    }
}

/// Hits already dealt in this swing: the attacker's counter at `human+0x970` (reset when the attack
/// starts, `FUN_007dbe30`; incremented per hit result, `FUN_007d82d0`).
#[derive(Default)]
pub struct SwingHits {
    count: u32,
    /// Weapon `destruction` / `destruction2` tag positions on the previous tick (`FUN_007d9010`).
    previous: [Option<Vec3>; 2],
    sweeping: bool,
    /// Loose bodies already pushed this swing (non-building results are de-duplicated).
    bodies: Vec<Entity>,
}

/// `FUN_007d7d90`: the attack's per-target damage times a falloff over the hits of one swing,
/// ((1 - min(n / 100, 1)) * 0.8 + 0.2).
pub fn swing_damage(damage: f32, hits_so_far: u32) -> f32 {
    let f = (hits_so_far as f32 / 100.0).clamp(0.0, 1.0);
    ((1.0 - f) * 0.8 + 0.2) * damage
}

/// Melee hit detection as in `FUN_007d9600` / `FUN_007d8770` (E-029):
/// - between the animation's impact-start and impact-end triggers, every tick sweeps a sphere of
///   HitRadius from each weapon `destruction` tag's previous position to its current one;
/// - on the secondary-impact trigger, a sphere sweeps from the body at HitHeight along the facing
///   for the SecondaryImpact HitRange.
/// Every result is a hit: building pieces each take StructureDamage (no de-duplication for
/// buildings), other bodies are pushed once per swing.
#[allow(clippy::too_many_arguments)]
pub fn hammer_system(
    mut input: ResMut<PlayerInput>,
    equip: Res<Equip>,
    mut tool_use: ResMut<ToolUse>,
    mut state: ResMut<HammerState>,
    player: Single<(Entity, &Transform, &Facing), With<Player>>,
    spatial: SpatialQuery,
    mut bodies: Query<(Forces, &ComputedMass)>,
    pieces: Query<&crate::buildings::PieceRef>,
    weapons: Query<(&GlobalTransform, &crate::weapons::WeaponModel)>,
    catalog: Res<crate::weapons::WeaponCatalog>,
    mut hits: MessageWriter<HitEvent>,
    mut last: ResMut<LastHit>,
    mut structural: MessageWriter<crate::buildings::StructuralHit>,
    mut swings: Local<u32>,
    mut swing: Local<SwingHits>,
    mut shakes: MessageWriter<crate::camera_shake::ShakeRequest>,
) {
    let attack = std::mem::take(&mut input.attack) && equip.ready();
    let attack_alt = std::mem::take(&mut input.attack_alt) && equip.ready();
    let hammer = equip.tool == Tool::Hammer;
    if !hammer && (attack || attack_alt) {
        tool_use.0 = Some(attack_alt);
    }
    let side = attack && hammer;
    let vert = attack_alt && hammer;
    // Binding of the vertical swing to the right mouse button is a guess; the
    // original input mapping is not yet determined.
    let start = if side {
        Some(SWING_SIDE)
    } else if vert {
        Some(SWING_VERT)
    } else {
        None
    };
    let was_idle = !state.is_swinging();
    if was_idle && start.is_some() {
        *swings += 1;
        *swing = SwingHits::default();
    }
    let events = state.advance(crate::clock::TICK_SECONDS as f32, start);
    let spec = if was_idle { start } else { state.spec() };
    let Some(spec) = spec else {
        // The swing ended on this tick; its trigger events still need a spec.
        return;
    };
    let (entity, transform, facing) = player.into_inner();
    let forward = Quat::from_rotation_y(facing.0) * Vec3::NEG_Z;
    let feet = transform.translation - Vec3::Y * HALF_HEIGHT;
    let filter = SpatialQueryFilter::from_excluded_entities([entity]);

    // Weapon prop points in world space (the hammer's head).
    let tag_points: Vec<Vec3> = weapons
        .iter()
        .next()
        .map(|(pose, model)| {
            let key = crate::weapons::WEAPONS[model.0].key;
            ["destruction", "destruction2"]
                .iter()
                .filter_map(|name| catalog.tag(key, name).map(|p| pose.transform_point(p)))
                .collect()
        })
        .unwrap_or_default();

    let mut sweeps: Vec<(Vec3, Vec3)> = Vec::new();
    for event in &events {
        match event {
            SwingEvent::ImpactStart => {
                swing.sweeping = true;
                swing.previous = [None, None];
            }
            SwingEvent::ImpactEnd => swing.sweeping = false,
            SwingEvent::SecondaryImpact => {
                let from = feet + Vec3::Y * spec.hit_height;
                sweeps.push((from, from + forward * spec.secondary_range));
            }
            SwingEvent::ReleaseMovement => {}
        }
    }
    if swing.sweeping {
        for (i, &p) in tag_points.iter().enumerate().take(2) {
            if let Some(prev) = swing.previous[i] {
                sweeps.push((prev, p));
            }
            swing.previous[i] = Some(p);
        }
    }

    let shape = Collider::sphere(spec.hit_radius);
    for (from, to) in sweeps {
        let delta = to - from;
        let length = delta.length();
        let Ok(dir) = Dir3::new(delta) else { continue };
        let config = ShapeCastConfig::from_max_distance(length.adjust_precision());
        let found = spatial.shape_hits(
            &shape,
            from.adjust_precision(),
            Quat::IDENTITY.adjust_precision(),
            dir,
            64,
            &config,
            &filter,
        );
        let direction = dir.as_vec3();
        let before = swing.count;
        for result in &found {
            let point = result.point1;
            let point = if result.distance <= 0.0 { from } else { point };
            if let Ok(piece) = pieces.get(result.entity) {
                structural.write(crate::buildings::StructuralHit {
                    center: point,
                    radius: spec.hit_radius,
                    damage: swing_damage(spec.structure_damage, swing.count),
                    direction,
                    impact_force: spec.impact_force,
                    swing: *swings,
                    piece: Some(*piece),
                });
                swing.count += 1;
                continue;
            }
            if swing.bodies.contains(&result.entity) {
                continue;
            }
            let Ok((mut forces, mass)) = bodies.get_mut(result.entity) else {
                continue;
            };
            swing.bodies.push(result.entity);
            swing.count += 1;
            let impulse = direction * HAMMER_SPEED * mass.value();
            forces.apply_linear_impulse_at_point(impulse.adjust_precision(), point.adjust_precision());
            let hit = HitEvent {
                target: result.entity,
                direction,
                speed: HAMMER_SPEED,
                structure_damage: spec.structure_damage,
            };
            hits.write(hit);
            last.0 = Some(hit);
        }
        // The player's sweep that hit anything starts (or restarts) the attack's camera shake.
        if swing.count > before {
            shakes.write(crate::camera_shake::ShakeRequest { name: spec.camera_shake, scale: 1.0 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = crate::clock::TICK_SECONDS as f32;

    /// Runs a swing to completion; returns (tick, event) pairs.
    fn run(spec: SwingSpec) -> Vec<(u32, SwingEvent)> {
        let mut state = HammerState::default();
        let mut out = Vec::new();
        let mut first = Some(spec);
        for tick in 0..600 {
            for e in state.advance(DT, first.take()) {
                out.push((tick, e));
            }
            if !state.is_swinging() {
                break;
            }
        }
        out
    }

    fn lengths(_: Tool, phase: SwitchPhase) -> f32 {
        match phase {
            SwitchPhase::Holster => 0.5,
            SwitchPhase::Draw => 0.75,
        }
    }

    #[test]
    fn switch_holsters_then_draws() {
        let mut equip = Equip::new(Tool::Hammer);
        equip.advance(Tool::Rifle, 0.0, lengths);
        assert_eq!(equip.phase(), Some((SwitchPhase::Holster, 0.0)));
        equip.advance(Tool::Rifle, 0.4, lengths);
        assert_eq!(equip.tool, Tool::Hammer, "model swaps only after the holster clip");
        equip.advance(Tool::Rifle, 0.2, lengths);
        assert_eq!(equip.tool, Tool::Rifle);
        assert_eq!(equip.phase().map(|p| p.0), Some(SwitchPhase::Draw));
        assert!(!equip.ready());
        equip.advance(Tool::Rifle, 0.8, lengths);
        assert!(equip.ready());
    }

    #[test]
    fn changing_request_mid_draw_holsters_again() {
        let mut equip = Equip::new(Tool::Hammer);
        for _ in 0..3 {
            equip.advance(Tool::Rifle, 0.3, lengths);
        }
        assert_eq!(equip.tool, Tool::Rifle);
        equip.advance(Tool::RemoteCharge, 0.1, lengths);
        assert_eq!(equip.phase(), Some((SwitchPhase::Holster, 0.0)));
        for _ in 0..10 {
            equip.advance(Tool::RemoteCharge, 0.3, lengths);
        }
        assert_eq!(equip.tool, Tool::RemoteCharge);
        assert!(equip.ready());
    }

    #[test]
    fn same_tool_request_is_a_no_op() {
        let mut equip = Equip::new(Tool::Hammer);
        equip.advance(Tool::Hammer, 1.0, lengths);
        assert!(equip.ready());
        assert_eq!(equip.tool, Tool::Hammer);
    }

    #[test]
    fn idle_without_attack_does_nothing() {
        let mut state = HammerState::default();
        assert!(state.advance(DT, None).is_empty());
        assert!(!state.is_swinging());
    }

    #[test]
    fn triggers_fire_once_in_frame_order() {
        let events: Vec<_> = run(SWING_SIDE).into_iter().map(|(_, e)| e).collect();
        assert_eq!(
            events,
            vec![
                SwingEvent::ImpactStart,
                SwingEvent::SecondaryImpact,
                SwingEvent::ImpactEnd,
                SwingEvent::ReleaseMovement
            ]
        );
    }

    #[test]
    fn side_swing_timing_matches_trigger_frames() {
        let ticks = run(SWING_SIDE);
        let seconds = |e: SwingEvent| {
            let tick = ticks.iter().find(|(_, x)| *x == e).unwrap().0;
            (tick + 1) as f32 * DT
        };
        // 60 Hz ticks against 30 fps frames: within one tick of frame / 30.
        assert!((seconds(SwingEvent::ImpactStart) - 5.0 / 30.0).abs() <= DT);
        assert!((seconds(SwingEvent::SecondaryImpact) - 8.0 / 30.0).abs() <= DT);
        assert!((seconds(SwingEvent::ImpactEnd) - 10.0 / 30.0).abs() <= DT);
        assert!((seconds(SwingEvent::ReleaseMovement) - 22.0 / 30.0).abs() <= DT);
    }

    #[test]
    fn vert_swing_is_slower_than_side() {
        let last = |spec| run(spec).last().unwrap().0;
        assert!(last(SWING_VERT) > last(SWING_SIDE));
    }

    #[test]
    fn attack_during_swing_is_ignored_until_released() {
        let mut state = HammerState::default();
        state.advance(DT, Some(SWING_SIDE));
        let mut starts = 0;
        for _ in 0..30 {
            if state.advance(DT, Some(SWING_SIDE)).contains(&SwingEvent::ImpactStart) {
                starts += 1;
            }
        }
        assert_eq!(starts, 1);
    }

    #[test]
    fn movement_unlocks_at_release_movement() {
        let mut state = HammerState::default();
        state.advance(DT, Some(SWING_SIDE));
        assert!(state.movement_locked());
        for _ in 0..60 {
            state.advance(DT, None);
        }
        assert!(!state.movement_locked());
    }

    #[test]
    fn can_swing_again_after_release() {
        let mut state = HammerState::default();
        state.advance(DT, Some(SWING_SIDE));
        for _ in 0..60 {
            state.advance(DT, None);
        }
        state.advance(DT, Some(SWING_SIDE));
        assert!(state.is_swinging());
    }

    #[test]
    fn reset_returns_to_idle() {
        let mut state = HammerState::default();
        state.advance(DT, Some(SWING_SIDE));
        state.reset();
        assert!(!state.is_swinging());
    }
}
