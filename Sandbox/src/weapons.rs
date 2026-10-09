//! Weapon models attached to the character's hand bones.
//!
//! Meshes and textures are converted from the user's own game files into
//! `LocalData/Assets/weapons/` by `Extractors/build_assets.py` (E-014). The hand
//! comes from the weapon table: `sledgehammer` carries the "held in left hand"
//! flag (E-003), everything else is held in the right hand. Both hand bones
//! (`l-handprop`, `r-handprop`) exist in `mason.rig_pc` (E-009).

use std::collections::HashMap;

use bevy::prelude::*;
use serde::Deserialize;

use crate::character::PlayerVisual;
use crate::tools::{Equip, Tool};

pub struct WeaponDef {
    pub key: &'static str,
    pub label: &'static str,
    pub hand_bone: &'static str,
}

const LEFT: &str = "l-handprop";
const RIGHT: &str = "r-handprop";

/// Fallback grip when `weapons.json` has none: the pose-derived estimate of E-018, which lacks the
/// hand tag's roll about the handle (about 43 degrees).
const PROP_TO_MESH: Quat = Quat::from_xyzw(-std::f32::consts::FRAC_1_SQRT_2, 0.0, 0.0, std::f32::consts::FRAC_1_SQRT_2);

/// Order matches `Extractors/build_assets.py`.
pub const WEAPONS: &[WeaponDef] = &[
    WeaponDef { key: "sledgehammer", label: "Sledgehammer", hand_bone: LEFT },
    WeaponDef { key: "sledge_upgrade_2", label: "Sledgehammer (upgrade 2)", hand_bone: LEFT },
    WeaponDef { key: "sledge_upgrade_3", label: "Sledgehammer (upgrade 3)", hand_bone: LEFT },
    WeaponDef { key: "assault_rifle", label: "Assault Rifle", hand_bone: RIGHT },
    WeaponDef { key: "edf_pistol", label: "EDF Pistol", hand_bone: RIGHT },
    WeaponDef { key: "shotgun", label: "Shotgun", hand_bone: RIGHT },
    WeaponDef { key: "sniper_rifle", label: "Sniper Rifle", hand_bone: RIGHT },
    WeaponDef { key: "rail_driver", label: "Rail Driver", hand_bone: RIGHT },
    WeaponDef { key: "rpg_launcher", label: "RPG Launcher", hand_bone: RIGHT },
    WeaponDef { key: "nano_rifle", label: "Nano Rifle", hand_bone: RIGHT },
    WeaponDef { key: "arc_welder", label: "Arc Welder", hand_bone: RIGHT },
    WeaponDef { key: "grinder", label: "Grinder", hand_bone: RIGHT },
    WeaponDef { key: "enforcer", label: "Enforcer", hand_bone: RIGHT },
    WeaponDef { key: "gutter", label: "Gutter", hand_bone: RIGHT },
    WeaponDef { key: "gauss_rifle", label: "Gauss Rifle", hand_bone: RIGHT },
    WeaponDef { key: "autoblaster", label: "Autoblaster", hand_bone: RIGHT },
    WeaponDef { key: "remote_detonator", label: "Remote Detonator", hand_bone: RIGHT },
];

const CATALOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../LocalData/Assets/weapons.json");

#[derive(Deserialize)]
pub struct Grip {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
}

/// Per weapon, from `Extractors/build_assets.py`.
#[derive(Deserialize)]
pub struct WeaponInfo {
    /// Weapon root relative to the hand-prop bone: the mesh's `weapon_handle` tag placed on the
    /// rig's `l-hand`/`r-hand` tag, as the game attaches held items (E-029).
    pub grip: Option<Grip>,
    /// Prop points in mesh space (`destruction`, `muzzle_flash`, ...).
    #[serde(default)]
    pub tags: HashMap<String, [f32; 3]>,
}

#[derive(Resource, Default)]
pub struct WeaponCatalog(pub HashMap<String, WeaponInfo>);

impl WeaponCatalog {
    pub fn load() -> Self {
        let parsed = std::fs::read_to_string(CATALOG).map_err(|e| e.to_string()).and_then(|text| {
            serde_json::from_str::<HashMap<String, WeaponInfo>>(&text).map_err(|e| e.to_string())
        });
        match parsed {
            Ok(map) => Self(map),
            Err(error) => {
                warn!("weapons.json not loaded ({error}); using the fallback grip");
                Self::default()
            }
        }
    }

    pub fn grip(&self, key: &str) -> Transform {
        match self.0.get(key).and_then(|w| w.grip.as_ref()) {
            Some(g) => Transform::from_translation(Vec3::from(g.translation))
                .with_rotation(Quat::from_array(g.rotation).normalize()),
            None => Transform::from_rotation(PROP_TO_MESH),
        }
    }

    pub fn tag(&self, key: &str, name: &str) -> Option<Vec3> {
        self.0.get(key)?.tags.get(name).map(|&p| Vec3::from(p))
    }
}

/// Index into [`WEAPONS`] of the weapon shown in the character's hand.
#[derive(Resource, Default)]
pub struct HeldWeapon(pub Option<usize>);

impl HeldWeapon {
    pub fn name(&self) -> &'static str {
        self.0.map_or("none", |i| WEAPONS[i].label)
    }
}

/// Marks a spawned weapon scene root.
#[derive(Component)]
pub struct WeaponModel(pub usize);

/// Default model for each quick-select tool.
pub fn weapon_for_tool(tool: Tool) -> Option<usize> {
    let key = match tool {
        Tool::Hammer => "sledgehammer",
        Tool::Rifle => "assault_rifle",
        Tool::RemoteCharge => "remote_detonator",
        Tool::Grenade => return None,
    };
    WEAPONS.iter().position(|w| w.key == key)
}

/// `[` and `]` step through every converted weapon model.
pub fn cycle_hotkeys(keys: Res<ButtonInput<KeyCode>>, mut held: ResMut<HeldWeapon>) {
    let count = WEAPONS.len();
    let current = held.0.map_or(0, |i| i as isize);
    if keys.just_pressed(KeyCode::BracketRight) {
        held.0 = Some((current + 1).rem_euclid(count as isize) as usize);
    } else if keys.just_pressed(KeyCode::BracketLeft) {
        held.0 = Some((current - 1).rem_euclid(count as isize) as usize);
    }
}

/// The equipped tool selects its default weapon model. The model changes when
/// the holster clip ends, not when the key is pressed.
pub fn sync_tool(equip: Res<Equip>, mut held: ResMut<HeldWeapon>, mut shown: Local<Option<Tool>>) {
    if *shown != Some(equip.tool) {
        *shown = Some(equip.tool);
        held.0 = weapon_for_tool(equip.tool);
    }
}

/// Keeps exactly one weapon model parented to the right hand bone. The model
/// follows the animated bone, so it moves with whatever pose the skeleton has.
pub fn attach_weapon(
    mut commands: Commands,
    assets: Res<AssetServer>,
    held: Res<HeldWeapon>,
    visual: Query<Entity, With<PlayerVisual>>,
    children: Query<&Children>,
    names: Query<&Name>,
    existing: Query<(Entity, &WeaponModel)>,
    catalog: Res<WeaponCatalog>,
) {
    let wanted = held.0;
    for (entity, model) in &existing {
        if Some(model.0) != wanted {
            commands.entity(entity).despawn();
        }
    }
    let Some(index) = wanted else { return };
    if existing.iter().any(|(_, m)| m.0 == index) {
        return;
    }
    let Ok(root) = visual.single() else { return };
    let def = &WEAPONS[index];
    // The bone entities appear once the glTF scene has been instantiated.
    let bone = children
        .iter_descendants(root)
        .find(|e| names.get(*e).is_ok_and(|n| n.as_str() == def.hand_bone));
    let Some(bone) = bone else { return };
    commands.entity(bone).with_children(|parent| {
        parent.spawn((
            WeaponModel(index),
            Name::new(format!("weapon:{}", def.key)),
            WorldAssetRoot(assets.load(format!("weapons/{}.glb#Scene0", def.key))),
            catalog.grip(def.key),
        ));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prop_rotation_is_minus_quarter_turn_about_x() {
        let expected = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        assert!((Vec4::from(PROP_TO_MESH) - Vec4::from(expected)).length() < 1e-6);
        // Rifle barrel (Bevy -Z after the mirror) ends up along -Y of the prop bone.
        assert!((PROP_TO_MESH * Vec3::NEG_Z - Vec3::NEG_Y).length() < 1e-5);
    }
}
