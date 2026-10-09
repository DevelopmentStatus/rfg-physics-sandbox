//! Camera shake as the game evaluates it (`FUN_004b9810`, E-030). Shakes come from
//! `camera_shake.xtbl` (converted to `LocalData/Assets/camera_shakes.json` by
//! `Extractors/build_tables.py`).
//!
//! - Up to five shakes play at once (`FUN_004b93e0`); starting one that is already playing restarts it.
//! - Each frame, a shake's Strength_Graph is sampled linearly at its age. Every channel's amplitude
//!   is graph strength x 0.1 x scale x the channel's per-axis value; across shakes the largest
//!   magnitude per axis wins.
//! - The channels' noise:
//!   - Jitter: new random targets every 33 ms, linear blend.
//!   - Destabilization: random targets every 1000/Frequency ms, cosine blend.
//!   - Oscillation1/2: the target flips sign every 1000/Frequency ms, cosine blend.
//!   - Wobble: a sign that flips every frame, with Variation of it randomised.
//! - Direct adds its angles as they are.
//! - The sum (pitch, roll, yaw) in radians rotates the camera in its own frame.
//! - Not yet ported: Wander / Wander_Direct, which turn the aim itself (hammer shakes leave them
//!   at zero), blur, and controller rumble.

use std::collections::HashMap;

use bevy::prelude::*;
use serde::Deserialize;

use crate::rng::Rng;

const TABLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../LocalData/Assets/camera_shakes.json");
const SLOTS: usize = 5;
/// `DAT_00b4c650`: graph strength scale for every channel but Wander.
const STRENGTH_SCALE: f32 = 0.1;
/// Jitter retarget period in ms (`0x21`).
const JITTER_MS: f64 = 33.0;
/// `DAT_00b4c878`: below this a channel counts as silent.
const EPSILON: f32 = 1e-4;

#[derive(Deserialize, Clone, Copy, Default)]
struct Axes {
    #[serde(default)]
    pitch: f32,
    #[serde(default)]
    roll: f32,
    #[serde(default)]
    yaw: f32,
    #[serde(default)]
    frequency: f32,
    #[serde(default)]
    variation: f32,
}

impl Axes {
    fn vec(&self) -> Vec3 {
        Vec3::new(self.pitch, self.roll, self.yaw)
    }
}

#[derive(Deserialize, Clone, Copy, Default)]
struct Strength {
    time: f32,
    wobble: f32,
    destable: f32,
    jitter: f32,
    oscillation1: f32,
    oscillation2: f32,
    direct: f32,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct ShakeDef {
    Wobble: Axes,
    Destabilization: Axes,
    Jitter: Axes,
    Oscillation1: Axes,
    Oscillation2: Axes,
    Direct: Axes,
    graph: Vec<Strength>,
}

/// Start a shake by its `camera_shake.xtbl` name.
#[derive(Message, Clone)]
pub struct ShakeRequest {
    pub name: &'static str,
    pub scale: f32,
}

/// One noise channel: the previous and current targets and the retarget timer (`FUN_009ee7d0`).
#[derive(Default)]
struct Noise {
    previous: Vec3,
    target: Vec3,
    /// Clock time (ms) of the next retarget; `None` = unset.
    due: Option<f64>,
}

impl Noise {
    /// Advances the timer; on expiry `retarget` picks the next target. Returns the remaining ms.
    fn step(&mut self, now: f64, period: f64, retarget: impl FnOnce(Vec3) -> Vec3) -> f64 {
        let expired = self.due.is_none_or(|due| now >= due);
        if expired {
            self.previous = self.target;
            self.target = retarget(self.target);
            // The next period is shortened by the overshoot, unless that leaves nothing.
            let next = match self.due {
                Some(due) if period - (now - due) >= 0.0 => period - (now - due),
                _ => period,
            };
            self.due = Some(now + next);
        }
        (self.due.unwrap() - now).max(0.0)
    }
}

#[derive(Resource)]
pub struct CameraShake {
    defs: HashMap<String, ShakeDef>,
    names: Vec<String>,
    /// Active shakes: (index into `names`, age in seconds, scale).
    slots: [Option<(usize, f32, f32)>; SLOTS],
    clock_ms: f64,
    jitter: Noise,
    destab: Noise,
    osc: [Noise; 2],
    wobble_sign: Vec3,
    rng: Rng,
    /// Last applied offset, for the HUD.
    pub angles: Vec3,
}

impl CameraShake {
    pub fn load() -> Self {
        let defs: HashMap<String, ShakeDef> = std::fs::read_to_string(TABLE)
            .ok()
            .and_then(|t| serde_json::from_str(&t).map_err(|e| warn!("camera_shakes.json: {e}")).ok())
            .unwrap_or_default();
        if defs.is_empty() {
            warn!("No camera shakes loaded; run ExtractAssets.bat");
        }
        let names = defs.keys().cloned().collect();
        Self {
            defs,
            names,
            slots: [None; SLOTS],
            clock_ms: 0.0,
            jitter: Noise::default(),
            destab: Noise::default(),
            osc: [
                Noise { target: Vec3::ONE, ..default() },
                Noise { target: Vec3::ONE, ..default() },
            ],
            wobble_sign: Vec3::ONE,
            rng: Rng::new(0x5a4b),
            angles: Vec3::ZERO,
        }
    }

    /// `FUN_004b93e0`: reuse the slot already playing this shake (restarting it) or the first free one.
    pub fn start(&mut self, name: &str, scale: f32) {
        let Some(index) = self.names.iter().position(|n| n == name) else { return };
        let scale = scale.max(0.0);
        if let Some(slot) = self.slots.iter_mut().find(|s| s.is_none_or(|(i, _, _)| i == index)) {
            *slot = Some((index, 0.0, scale));
        }
    }

    /// One frame of `FUN_004b9810`; returns (pitch, roll, yaw) in radians.
    fn evaluate(&mut self, dt: f32) -> Vec3 {
        let mut jitter = Vec3::ZERO;
        let mut destab = Vec3::ZERO;
        let mut destab_freq: f32 = 0.0;
        let mut osc = [Vec3::ZERO; 2];
        let mut osc_freq = [0.0f32; 2];
        let mut wobble = Vec3::ZERO;
        let mut variation: f32 = 0.0;
        let mut angles = Vec3::ZERO;
        let max_abs = |acc: &mut Vec3, v: Vec3| {
            for i in 0..3 {
                if v[i].abs() > acc[i].abs() {
                    acc[i] = v[i];
                }
            }
        };
        for slot in &mut self.slots {
            let Some((index, age, scale)) = *slot else { continue };
            let def = &self.defs[&self.names[index]];
            let graph = &def.graph;
            // Last element at or before `age`; the shake ends when that is the final element.
            let mut i = None;
            for (k, e) in graph.iter().enumerate() {
                if e.time > age {
                    break;
                }
                if k + 1 == graph.len() {
                    *slot = None;
                    break;
                }
                i = Some(k);
            }
            if slot.is_none() {
                continue;
            }
            let Some(i) = i else { continue };
            let (a, b) = (graph[i], graph[i + 1]);
            let f = (age - a.time) / (b.time - a.time);
            let s = |pick: fn(&Strength) -> f32| (pick(&a) * (1.0 - f) + pick(&b) * f) * scale;
            let k = STRENGTH_SCALE;
            max_abs(&mut jitter, def.Jitter.vec() * s(|e| e.jitter) * k);
            max_abs(&mut destab, def.Destabilization.vec() * s(|e| e.destable) * k);
            destab_freq = destab_freq.max(def.Destabilization.frequency);
            max_abs(&mut osc[0], def.Oscillation1.vec() * s(|e| e.oscillation1) * k);
            osc_freq[0] = osc_freq[0].max(def.Oscillation1.frequency);
            max_abs(&mut osc[1], def.Oscillation2.vec() * s(|e| e.oscillation2) * k);
            osc_freq[1] = osc_freq[1].max(def.Oscillation2.frequency);
            max_abs(&mut wobble, def.Wobble.vec() * s(|e| e.wobble) * k);
            variation = variation.max(def.Wobble.variation);
            let direct = s(|e| e.direct) * k;
            if direct.abs() >= EPSILON {
                angles += def.Direct.vec() * direct;
            }
            *slot = Some((index, age + dt, scale));
        }

        let now = self.clock_ms;
        let silent = |v: Vec3| v.abs().max_element() < EPSILON;
        let rng = &mut self.rng;
        let mut random = || Vec3::new(rng.next_f32(), rng.next_f32(), rng.next_f32()) * 2.0 - 1.0;
        let cosine = |remaining: f64, period: f64| {
            let g = ((((1.0 - remaining / period) * std::f64::consts::PI).cos() + 1.0) * 0.5) as f32;
            g
        };

        if silent(jitter) {
            self.jitter = Noise::default();
        } else {
            let remaining = self.jitter.step(now, JITTER_MS, |_| random());
            let f = (remaining / JITTER_MS) as f32;
            angles += jitter * (self.jitter.target * (1.0 - f) + self.jitter.previous * f);
        }
        if silent(destab) || destab_freq <= 0.0 {
            self.destab = Noise::default();
        } else {
            let period = (1000.0 / destab_freq as f64).trunc();
            let remaining = self.destab.step(now, period, |_| random());
            let g = cosine(remaining, period);
            angles += destab * (self.destab.previous * g + self.destab.target * (1.0 - g));
        }
        if silent(wobble) {
            self.wobble_sign = Vec3::ONE;
        } else {
            self.wobble_sign = -self.wobble_sign;
            let r = Vec3::new(rng_f(&mut self.rng), rng_f(&mut self.rng), rng_f(&mut self.rng)) * self.wobble_sign;
            angles += wobble * (r * variation + self.wobble_sign * (1.0 - variation));
        }
        for c in 0..2 {
            if silent(osc[c]) || osc_freq[c] <= 0.0 {
                self.osc[c] = Noise { target: Vec3::ONE, ..default() };
            } else {
                let period = (1000.0 / osc_freq[c] as f64).trunc();
                let remaining = self.osc[c].step(now, period, |t| -t);
                let g = cosine(remaining, period);
                angles += osc[c] * (self.osc[c].previous * g + self.osc[c].target * (1.0 - g));
            }
        }
        self.clock_ms += dt as f64 * 1000.0;
        angles
    }
}

fn rng_f(rng: &mut Rng) -> f32 {
    rng.next_f32()
}

/// `FUN_009f0d30(pitch, roll, yaw)`: the game's rotation rows (right, up, forward) for the angles.
fn game_rows(pitch: f32, roll: f32, yaw: f32) -> Mat3 {
    let (sa, ca) = pitch.sin_cos();
    let (sb, cb) = roll.sin_cos();
    let (sc, cc) = yaw.sin_cos();
    // Rows as columns of `Mat3::from_cols`, transposed below.
    let rows = [
        Vec3::new(cb * cc + sa * sb * sc, ca * sb, sa * sb * cc - cb * sc),
        Vec3::new(sa * cb * sc - sb * cc, ca * cb, sb * sc + sa * cb * cc),
        Vec3::new(ca * sc, -sa, ca * cc),
    ];
    Mat3::from_cols(rows[0], rows[1], rows[2])
}

/// The shake as a rotation in the camera's own Bevy frame (game space mirrored in Z).
pub fn local_rotation(angles: Vec3) -> Quat {
    // Columns of `game_rows` are the shaken camera axes in the camera frame (game space).
    let r = game_rows(angles.x, angles.y, angles.z);
    let s = Mat3::from_diagonal(Vec3::new(1.0, 1.0, -1.0));
    Quat::from_mat3(&(s * r * s)).normalize()
}

pub fn start_shakes(mut requests: MessageReader<ShakeRequest>, mut shake: ResMut<CameraShake>) {
    for request in requests.read() {
        shake.start(request.name, request.scale);
    }
}

/// Runs after the camera rig has placed the camera for this frame.
pub fn apply_shake(
    time: Res<Time>,
    mut shake: ResMut<CameraShake>,
    mut camera: Single<&mut Transform, With<Camera3d>>,
) {
    let angles = shake.evaluate(time.delta_secs());
    shake.angles = angles;
    if angles != Vec3::ZERO {
        camera.rotation = camera.rotation * local_rotation(angles);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_pitch_looks_down() {
        let q = local_rotation(Vec3::new(0.1, 0.0, 0.0));
        let forward = q * Vec3::NEG_Z;
        assert!(forward.y < -0.09 && forward.y > -0.11, "{forward}");
    }

    #[test]
    fn zero_angles_are_identity() {
        assert!(local_rotation(Vec3::ZERO).angle_between(Quat::IDENTITY) < 1e-6);
    }
}
