//! Spring-bone approximation of Mason's coat cloth.
//!
//! The coat rig (E-019) adds 46 cloth bones to the skeleton. The `coat_*` bones
//! form ten hanging strands that branch off `spine`; no clip animates them, so
//! without this module the flaps follow the spine rigidly. The game drives them
//! with its own cloth simulation (`mason_coat.sim_pc`), which is not decoded
//! yet. Until it is, this module runs a world-space verlet sim on the coat
//! joints:
//! - each strand root is pinned to its animated position, every other joint is
//!   a particle pulled softly toward the rigid (animated) pose;
//! - distance constraints keep bone lengths, ring links between neighbouring
//!   strands keep the skirt shape, capsules on the pelvis and legs push flaps
//!   out of the body;
//! - the result is written back as joint rotations before transform
//!   propagation, so skinning uses it unchanged.
//!
//! The `scarf_*` bones are single joints sitting on the neck with no length to
//! swing; they stay rigid.
//!
//! Every tuning constant is a HYPOTHESIS tuned by eye, to be replaced by the
//! `mason_coat.sim_pc` values once that file is decoded.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::character::PlayerVisual;

/// HYPOTHESIS: tuned by eye. Substep length of the cloth integration.
const SUBSTEP: f32 = 1.0 / 120.0;
/// Upper bound on substeps per frame, so a long frame cannot stall.
const MAX_SUBSTEPS: u32 = 8;
/// Constraint iterations per substep.
const ITERATIONS: u32 = 2;
const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);
/// HYPOTHESIS: tuned by eye. Velocity damping, per second.
const DAMPING: f32 = 3.0;
/// HYPOTHESIS: tuned by eye. Angular frequency (rad/s) of the spring toward the
/// animated pose, by depth below the pinned strand root (depth 1, 2, 3 and
/// deeper). Kept near a 0.4 m pendulum (sqrt(g / L) ~ 5) so the flaps swing.
const STIFFNESS: [f32; 3] = [5.0, 3.5, 2.5];
/// Weight of the ring links between strands; below 1 they stay soft.
const RING_WEIGHT: f32 = 0.5;
/// HYPOTHESIS: tuned by eye. Body colliders as (from bone, to bone, radius).
/// Radii leave every coat joint outside them in the bind pose.
const CAPSULES: [(&str, &str, f32); 5] = [
    ("pelvis", "spine1", 0.11),
    ("l-thigh", "l-calf", 0.08),
    ("r-thigh", "r-calf", 0.08),
    ("l-calf", "l-foot", 0.06),
    ("r-calf", "r-foot", 0.06),
];
/// Cloth thickness added to the capsule radii.
const THICKNESS: f32 = 0.02;
/// An anchor that moves further than this in one frame was teleported.
const TELEPORT_DISTANCE: f32 = 1.0;
/// Strands around the body, by joint-name prefix, front left to front right.
/// The coat is open at the front, so the ring does not wrap from `rf` to `lf`.
const RING: [&str; 10] = [
    "coat_lf", "coat_ls", "coat_ls1", "coat_lb1", "coat_lb", "coat_rb", "coat_rb1", "coat_rs1", "coat_rs", "coat_rf",
];

/// Live tuning, edited from the control panel.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct CoatSettings {
    pub enabled: bool,
    pub damping: f32,
    /// Multiplies the spring constants of [`STIFFNESS`].
    pub stiffness: f32,
}

impl Default for CoatSettings {
    fn default() -> Self {
        // Off by default for now (user request 2026-10-08); the control panel checkbox turns it on.
        Self { enabled: false, damping: DAMPING, stiffness: 1.0 }
    }
}

#[derive(Clone, Debug)]
pub struct Particle {
    pub joint: Entity,
    /// Parent particle; `None` for a strand root, which hangs off `anchor`.
    pub parent: Option<usize>,
    /// The body bone the strand hangs from.
    pub anchor: Entity,
    pub children: Vec<usize>,
    /// Bind translation relative to the parent joint.
    pub rest_local: Vec3,
    /// 0 for the pinned strand root.
    pub depth: usize,
    pub pos: Vec3,
    pub prev: Vec3,
    /// Position in the rigid animated pose.
    pub target: Vec3,
    prev_target: Vec3,
}

impl Particle {
    pub fn new(joint: Entity, parent: Option<usize>, anchor: Entity, rest_local: Vec3, depth: usize) -> Self {
        Self {
            joint,
            parent,
            anchor,
            children: Vec::new(),
            rest_local,
            depth,
            pos: Vec3::ZERO,
            prev: Vec3::ZERO,
            target: Vec3::ZERO,
            prev_target: Vec3::ZERO,
        }
    }

    pub fn pinned(&self) -> bool {
        self.parent.is_none()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Capsule {
    pub a: Vec3,
    pub b: Vec3,
    pub radius: f32,
}

/// The coat strands of one character, on its [`PlayerVisual`] entity.
#[derive(Component, Clone, Debug)]
pub struct CoatSim {
    /// Parent first.
    pub particles: Vec<Particle>,
    /// Soft distance links (a, b, rest length) between neighbouring strands.
    pub ring: Vec<(usize, usize, f32)>,
    /// Collider bones and radii.
    colliders: Vec<(Entity, Entity, f32)>,
    /// Snap to the animated pose on the next frame.
    reset: bool,
    last_anchor: Option<Vec3>,
    /// Substep length of the previous frame, for the verlet velocity term.
    last_h: f32,
}

impl CoatSim {
    pub fn new(mut particles: Vec<Particle>) -> Self {
        for i in 0..particles.len() {
            if let Some(p) = particles[i].parent {
                particles[p].children.push(i);
            }
        }
        Self { particles, ring: Vec::new(), colliders: Vec::new(), reset: true, last_anchor: None, last_h: SUBSTEP }
    }
}

/// Builds the [`CoatSim`] once the player's glTF scene has been instantiated.
pub fn attach_coat_sim(
    mut commands: Commands,
    visual: Query<Entity, (With<PlayerVisual>, Without<CoatSim>)>,
    children: Query<&Children>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    transforms: Query<&Transform>,
) {
    let Ok(root) = visual.single() else { return };
    let joints: HashMap<&str, Entity> = children
        .iter_descendants(root)
        .filter_map(|e| names.get(e).ok().map(|n| (n.as_str(), e)))
        .collect();
    // The bone entities appear once the glTF scene has been instantiated.
    if !joints.contains_key("spine") {
        return;
    }
    let is_coat = |e: Entity| names.get(e).is_ok_and(|n| n.as_str().starts_with("coat_"));

    // Depth below the strand root; sorting by it puts parents first.
    let mut coat: Vec<(usize, Entity)> = joints
        .values()
        .copied()
        .filter(|e| is_coat(*e))
        .map(|e| (parents.iter_ancestors(e).take_while(|a| is_coat(*a)).count(), e))
        .collect();
    coat.sort_by_key(|(depth, e)| (*depth, names.get(*e).map(|n| n.as_str().to_owned()).unwrap_or_default()));
    let index: HashMap<Entity, usize> = coat.iter().enumerate().map(|(i, (_, e))| (*e, i)).collect();

    let mut particles: Vec<Particle> = Vec::with_capacity(coat.len());
    for &(depth, joint) in &coat {
        // Every coat joint descends from the visual root, so it has a parent.
        let parent = parents.get(joint).map(ChildOf::parent).unwrap_or(root);
        let parent_index = index.get(&parent).copied();
        let anchor = parent_index.map_or(parent, |p| particles[p].anchor);
        let rest_local = transforms.get(joint).map_or(Vec3::ZERO, |t| t.translation);
        particles.push(Particle::new(joint, parent_index, anchor, rest_local, depth));
    }
    let mut sim = CoatSim::new(particles);

    let names_of: Vec<String> =
        coat.iter().map(|(_, e)| names.get(*e).map(|n| n.as_str().to_owned()).unwrap_or_default()).collect();
    sim.ring = ring_links(&sim.particles, &names_of);
    sim.colliders = CAPSULES
        .iter()
        .filter_map(|(a, b, r)| Some((*joints.get(a)?, *joints.get(b)?, *r)))
        .collect();
    info!("coat sim: {} particles, {} ring links, {} colliders", sim.particles.len(), sim.ring.len(), sim.colliders.len());
    commands.entity(root).insert(sim);
}

/// Strand prefix of a joint name: `coat_ls1_04` -> `coat_ls1`.
fn strand_prefix(name: &str) -> &str {
    name.rfind('_').map_or(name, |i| &name[..i])
}

/// Links particles at the same depth on neighbouring strands of [`RING`]. A
/// strand is the path from a tip back to its root, so the forked back strands
/// `coat_lb`/`coat_rb` share `coat_b_02`/`_03`.
pub fn ring_links(particles: &[Particle], names: &[String]) -> Vec<(usize, usize, f32)> {
    // Offsets from the anchor in the bind pose.
    let mut bind = vec![Vec3::ZERO; particles.len()];
    for (i, p) in particles.iter().enumerate() {
        bind[i] = p.parent.map_or(Vec3::ZERO, |q| bind[q]) + p.rest_local;
    }
    let mut strands: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, p) in particles.iter().enumerate() {
        if !p.children.is_empty() {
            continue;
        }
        let mut strand = vec![i];
        while let Some(q) = particles[*strand.last().unwrap()].parent {
            strand.push(q);
        }
        strand.reverse();
        strands.insert(strand_prefix(&names[i]), strand);
    }
    let mut links = Vec::new();
    for pair in RING.windows(2) {
        let (Some(a), Some(b)) = (strands.get(pair[0]), strands.get(pair[1])) else { continue };
        for (&i, &j) in a.iter().zip(b) {
            let same_anchor = particles[i].anchor == particles[j].anchor;
            if i != j && same_anchor && !(particles[i].pinned() && particles[j].pinned()) {
                links.push((i, j, bind[i].distance(bind[j])));
            }
        }
    }
    links
}

/// Closest point to `p` on segment `a`-`b`.
fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
    a + ab * t
}

/// Moves `p` out of the capsule; returns it unchanged when outside.
pub fn push_out(p: Vec3, capsule: &Capsule) -> Vec3 {
    let c = closest_on_segment(p, capsule.a, capsule.b);
    let d = p - c;
    let r = capsule.radius + THICKNESS;
    let len = d.length();
    if len >= r || len < 1e-6 {
        return p;
    }
    c + d / len * r
}

/// Snaps every particle onto its target, at rest.
pub fn reset_particles(particles: &mut [Particle]) {
    for p in particles {
        p.pos = p.target;
        p.prev = p.target;
        p.prev_target = p.target;
    }
}

/// Advances the particles by `dt` toward their current `target`s. Targets are
/// blended from last frame's across the substeps so pinned roots move smoothly.
pub fn integrate(sim: &mut CoatSim, capsules: &[Capsule], settings: &CoatSettings, dt: f32) {
    if dt <= 0.0 {
        return;
    }
    let steps = ((dt / SUBSTEP).ceil() as u32).clamp(1, MAX_SUBSTEPS);
    let h = dt / steps as f32;
    let keep = (-settings.damping * h).exp();
    // Spring acceleration omega^2 * offset, integrated over one substep.
    let pull: Vec<f32> = STIFFNESS.iter().map(|w| (w * w * settings.stiffness * h * h).min(1.0)).collect();
    // Verlet velocity from the previous substep length.
    let mut scale = h / sim.last_h;
    for step in 1..=steps {
        let s = step as f32 / steps as f32;
        for p in &mut sim.particles {
            let target = p.prev_target.lerp(p.target, s);
            if p.pinned() {
                p.prev = p.pos;
                p.pos = target;
                continue;
            }
            let velocity = (p.pos - p.prev) * scale * keep;
            p.prev = p.pos;
            p.pos += velocity + GRAVITY * h * h;
            let k = pull[(p.depth - 1).min(pull.len() - 1)];
            p.pos += (target - p.pos) * k;
        }
        scale = 1.0;
        for _ in 0..ITERATIONS {
            solve_constraints(sim, capsules);
        }
    }
    for p in &mut sim.particles {
        p.prev_target = p.target;
    }
    sim.last_h = h;
}

fn solve_constraints(sim: &mut CoatSim, capsules: &[Capsule]) {
    for &(i, j, rest) in &sim.ring {
        let (a, b) = (sim.particles[i].pos, sim.particles[j].pos);
        let d = b - a;
        let len = d.length();
        if len < 1e-6 {
            continue;
        }
        let correction = d * ((len - rest) / len) * RING_WEIGHT;
        match (sim.particles[i].pinned(), sim.particles[j].pinned()) {
            (false, false) => {
                sim.particles[i].pos += correction * 0.5;
                sim.particles[j].pos -= correction * 0.5;
            }
            (false, true) => sim.particles[i].pos += correction,
            (true, false) => sim.particles[j].pos -= correction,
            (true, true) => {}
        }
    }
    for i in 0..sim.particles.len() {
        let Some(parent) = sim.particles[i].parent else { continue };
        let mut pos = sim.particles[i].pos;
        for capsule in capsules {
            pos = push_out(pos, capsule);
        }
        // Bone length wins over collision: only the child moves.
        let anchor = sim.particles[parent].pos;
        let rest = sim.particles[i].rest_local.length();
        let d = pos - anchor;
        if d.length() > 1e-6 {
            pos = anchor + d.normalize() * rest;
        }
        sim.particles[i].pos = pos;
    }
}

/// Local rotation of every particle's joint that aims it at its simulated
/// children. `anchor_global` gives the anchors' current global transforms.
pub fn joint_rotations(particles: &[Particle], anchor_global: impl Fn(Entity) -> GlobalTransform) -> Vec<Quat> {
    let mut globals: Vec<GlobalTransform> = Vec::with_capacity(particles.len());
    let mut rotations = Vec::with_capacity(particles.len());
    for p in particles {
        let parent = p.parent.map_or_else(|| anchor_global(p.anchor), |q| globals[q]);
        let origin = parent.transform_point(p.rest_local);
        let parent_rot = parent.rotation();
        let mut rest_dir = Vec3::ZERO;
        let mut sim_dir = Vec3::ZERO;
        for &c in &p.children {
            rest_dir += parent_rot * particles[c].rest_local;
            sim_dir += particles[c].pos - origin;
        }
        let local = match (rest_dir.try_normalize(), sim_dir.try_normalize()) {
            (Some(from), Some(to)) => parent_rot.inverse() * Quat::from_rotation_arc(from, to) * parent_rot,
            _ => Quat::IDENTITY,
        };
        rotations.push(local);
        globals.push(parent.mul_transform(Transform::from_translation(p.rest_local).with_rotation(local)));
    }
    rotations
}

/// Fills each particle's `target` from the anchors' global transforms: the pose
/// the coat had when it followed the spine rigidly.
fn set_targets(particles: &mut [Particle], anchor_global: impl Fn(Entity) -> GlobalTransform) {
    for i in 0..particles.len() {
        let p = &particles[i];
        let anchor = anchor_global(p.anchor);
        particles[i].target = match p.parent {
            None => anchor.transform_point(p.rest_local),
            Some(q) => particles[q].target + anchor.affine().transform_vector3(p.rest_local),
        };
    }
}

/// Steps the coat and writes the joint rotations. Runs after animation and
/// before transform propagation, so the spine and legs are this frame's pose.
pub fn simulate_coat(
    time: Res<Time>,
    settings: Res<CoatSettings>,
    mut sims: Query<&mut CoatSim>,
    mut transforms: ParamSet<(TransformHelper, Query<&mut Transform>)>,
) {
    for mut sim in &mut sims {
        let sim = &mut *sim;
        if !settings.enabled {
            let mut query = transforms.p1();
            for p in &sim.particles {
                if let Ok(mut t) = query.get_mut(p.joint) {
                    if t.rotation != Quat::IDENTITY {
                        t.rotation = Quat::IDENTITY;
                    }
                }
            }
            sim.reset = true;
            continue;
        }

        let helper = transforms.p0();
        let mut anchors: HashMap<Entity, GlobalTransform> = HashMap::new();
        for p in &sim.particles {
            if !anchors.contains_key(&p.anchor) {
                if let Ok(g) = helper.compute_global_transform(p.anchor) {
                    anchors.insert(p.anchor, g);
                }
            }
        }
        let capsules: Vec<Capsule> = sim
            .colliders
            .iter()
            .filter_map(|(a, b, radius)| {
                let a = helper.compute_global_transform(*a).ok()?.translation();
                let b = helper.compute_global_transform(*b).ok()?.translation();
                Some(Capsule { a, b, radius: *radius })
            })
            .collect();
        let anchor_global = |e: Entity| anchors.get(&e).copied().unwrap_or_default();

        set_targets(&mut sim.particles, anchor_global);
        let anchor_pos = sim.particles.first().map(|p| anchor_global(p.anchor).translation());
        let teleported = match (sim.last_anchor, anchor_pos) {
            (Some(a), Some(b)) => a.distance(b) > TELEPORT_DISTANCE,
            _ => false,
        };
        sim.last_anchor = anchor_pos;
        if sim.reset || teleported {
            reset_particles(&mut sim.particles);
            sim.reset = false;
        } else {
            integrate(sim, &capsules, &settings, time.delta_secs());
        }

        let rotations = joint_rotations(&sim.particles, anchor_global);
        let mut query = transforms.p1();
        for (p, rotation) in sim.particles.iter().zip(rotations) {
            if let Ok(mut t) = query.get_mut(p.joint) {
                t.rotation = rotation;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANCHOR: Entity = Entity::PLACEHOLDER;

    /// One pinned root and three free joints hanging 0.13 m apart.
    fn strand() -> CoatSim {
        let down = Vec3::new(0.0, -0.13, 0.0);
        let particles = (0..4usize)
            .map(|d| {
                let rest = if d == 0 { Vec3::new(0.0, 0.0, 0.15) } else { down };
                Particle::new(Entity::PLACEHOLDER, d.checked_sub(1), ANCHOR, rest, d)
            })
            .collect();
        CoatSim::new(particles)
    }

    fn place(sim: &mut CoatSim, anchor: Vec3) {
        set_targets(&mut sim.particles, |_| GlobalTransform::from_translation(anchor));
    }

    #[test]
    fn rest_pose_stays_put() {
        let mut sim = strand();
        place(&mut sim, Vec3::ZERO);
        reset_particles(&mut sim.particles);
        // Gravity sags the strand only a little against the pull and the
        // straight-down bind pose keeps it from bending at all.
        for _ in 0..120 {
            integrate(&mut sim, &[], &CoatSettings::default(), 1.0 / 60.0);
        }
        for p in &sim.particles {
            assert!(p.pos.distance(p.target) < 1e-3, "{:?} vs {:?}", p.pos, p.target);
        }
        let rotations = joint_rotations(&sim.particles, |_| GlobalTransform::IDENTITY);
        for q in rotations {
            assert!(q.angle_between(Quat::IDENTITY) < 0.01);
        }
    }

    #[test]
    fn tip_lags_behind_a_moving_anchor() {
        let mut sim = strand();
        place(&mut sim, Vec3::ZERO);
        reset_particles(&mut sim.particles);
        let mut anchor = Vec3::ZERO;
        for _ in 0..6 {
            anchor.x += 0.05; // 3 m/s
            place(&mut sim, anchor);
            integrate(&mut sim, &[], &CoatSettings::default(), 1.0 / 60.0);
        }
        let tip = &sim.particles[3];
        assert!(tip.pos.x < tip.target.x - 0.02, "tip {:?} target {:?}", tip.pos, tip.target);
        let rotations = joint_rotations(&sim.particles, |_| GlobalTransform::from_translation(anchor));
        assert!(rotations[1].angle_between(Quat::IDENTITY) > 0.05);
    }

    #[test]
    fn bone_lengths_hold() {
        let mut sim = strand();
        place(&mut sim, Vec3::ZERO);
        reset_particles(&mut sim.particles);
        for i in 0..60 {
            place(&mut sim, Vec3::new((i as f32 * 0.3).sin() * 0.3, 0.0, 0.0));
            integrate(&mut sim, &[], &CoatSettings::default(), 1.0 / 60.0);
        }
        for p in &sim.particles[1..] {
            let parent = sim.particles[p.parent.unwrap()].pos;
            let rest = p.rest_local.length();
            assert!((p.pos.distance(parent) - rest).abs() < rest * 0.01);
        }
    }

    #[test]
    fn capsule_pushes_to_surface() {
        let capsule = Capsule { a: Vec3::ZERO, b: Vec3::Y, radius: 0.1 };
        let p = push_out(Vec3::new(0.05, 0.5, 0.0), &capsule);
        assert!((p.x - (0.1 + THICKNESS)).abs() < 1e-5 && (p.y - 0.5).abs() < 1e-5);
        let outside = Vec3::new(0.5, 0.5, 0.0);
        assert_eq!(push_out(outside, &capsule), outside);
    }

    #[test]
    fn ring_links_neighbouring_strands_and_skips_shared_joints() {
        // coat_b_02 -> coat_b_03 -> {coat_lb_01, coat_rb_01}
        let names = ["coat_b_02", "coat_b_03", "coat_lb_01", "coat_rb_01"].map(String::from);
        let parents = [None, Some(0), Some(1), Some(1)];
        let rests = [Vec3::new(0.0, 0.0, -0.12), Vec3::NEG_Y * 0.12, Vec3::new(-0.02, -0.13, 0.0), Vec3::new(0.02, -0.13, 0.0)];
        let particles = (0..4usize)
            .map(|i| Particle::new(ANCHOR, parents[i], ANCHOR, rests[i], [0, 1, 2, 2][i]))
            .collect();
        let sim = CoatSim::new(particles);
        let links = ring_links(&sim.particles, &names);
        assert_eq!(links.len(), 1);
        let (a, b, rest) = links[0];
        assert_eq!((a, b), (2, 3));
        assert!((rest - 0.04).abs() < 1e-5);
    }
}
