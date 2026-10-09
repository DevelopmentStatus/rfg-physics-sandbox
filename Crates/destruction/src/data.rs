//! Converted chunk data (`Extractors/build_chunks.py`, schema 1). Positions are in the chunk's
//! own space, already mirrored to Bevy's right-handed axes; piece hulls are in their dlod's space.

use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub struct ChunkFile {
    pub schema: u32,
    pub name: String,
    pub destroyables: Vec<DestroyableDef>,
    #[serde(default)]
    pub general: Vec<GeneralDef>,
    pub bmin: [f32; 3],
    pub bmax: [f32; 3],
}

#[derive(Deserialize, Clone, Debug)]
pub struct DestroyableDef {
    pub name: String,
    pub dlods: Vec<DlodDef>,
    pub pieces: Vec<PieceDef>,
    /// `[a, b, yield_max, area, flags]`.
    pub links: Vec<(u16, u16, i32, f32, u8)>,
}

/// A placed group of pieces. Its mesh is the intact look of the whole group.
#[derive(Deserialize, Clone, Debug)]
pub struct DlodDef {
    pub pos: [f32; 3],
    pub rot: [f32; 4],
    pub mesh: Option<usize>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct PieceDef {
    pub dlod: i32,
    pub mesh: Option<usize>,
    /// Fracture-surface geometry, shown once the piece has lost a neighbour.
    pub crack: Option<usize>,
    pub hull: Vec<[f32; 3]>,
    pub pos: [f32; 3],
    pub com: [f32; 3],
    pub bmin: [f32; 3],
    pub bmax: [f32; 3],
    pub volume: f32,
    /// Kilograms after the game's load rule (density x volume above 0.004 m^3).
    pub mass: f32,
    pub material: usize,
    pub flags: u8,
    pub links: Vec<u16>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct GeneralDef {
    pub name: String,
    pub pos: [f32; 3],
    pub rot: [f32; 4],
    #[serde(default)]
    pub mesh: Option<usize>,
    #[serde(default)]
    pub hull: Vec<[f32; 3]>,
}

/// One `rfg_materials.xtbl` entry as the game stores it (`FUN_006dcd90`).
#[derive(Deserialize, Clone, Debug)]
pub struct MaterialDef {
    pub name: String,
    pub density: f32,
    /// `LinkStrength` x 1000.
    pub link_strength: i64,
    pub brittleness: f32,
    pub restitution: f32,
    pub friction: f32,
    pub flags: u32,
}

/// Subpiece flag bits (byte 0x3F of the 64-byte subpiece record).
pub mod piece_flags {
    /// Shatters: every link breaks once it is hit (set with 0x40 for `shatter` materials).
    pub const SHATTER: u8 = 0x02;
    /// Material is not `structural`: no contribution to the destroyable's mass.
    pub const NON_STRUCTURAL: u8 = 0x08;
    /// Foundation: infinite hit points, left out of stress sums.
    pub const FOUNDATION: u8 = 0x20;
    /// `shatter` material (glass); links to non-glass pieces start broken.
    pub const GLASS: u8 = 0x40;
}

/// Base link flag bits (byte 0x0C of the 16-byte link record).
pub mod link_flags {
    /// The stress system never breaks this link.
    pub const NO_STRESS_BREAK: u8 = 0x02;
    /// Broken (also set at load between glass and non-glass pieces).
    pub const BROKEN: u8 = 0x04;
}
