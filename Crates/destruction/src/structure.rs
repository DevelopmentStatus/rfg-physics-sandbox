//! Runtime state of one destroyable and the game's structural stress pass.
//!
//! Recovered from the original executable (Research/evidence_registry.md E-021..E-027):
//! - `FUN_006edbd0`/`FUN_006edf50`/`FUN_006eda80`: breadth-first waves from the root pieces over
//!   unbroken links; each piece records its depth and a parity bit.
//! - `FUN_006ee5b0`: waves are processed deepest first. A piece's accumulated load (its own mass
//!   at its position plus what pieces above pushed into it) is split evenly between its supports
//!   (linked pieces one wave closer to the roots). For every support it adds
//!   `Force_Scalar * share * 9.8 * lever` to its stress and the link's remaining strength to its
//!   strength. If the summed support strength is below `Parent_Yield_Scalar * own mass`, all of
//!   its breakable support links break ("parent yield").
//! - `FUN_006ee420`: lever = |d| * max(1 - max(d.y / |d|, 0)^2, 0.25), d = load centre minus the
//!   centre of the overlap of the two pieces' bounds.
//! - `FUN_006eeff0`: pieces of one wave joined by unbroken links form a cluster. When the
//!   cluster's summed strength is below its rounded summed stress, every breakable link from the
//!   cluster to the wave below breaks. Foundation pieces (flag 0x20) are left out of both sums.
//! - `FUN_006d0aa0`: after links break, groups that can no longer reach a root split off as
//!   separate rigid bodies.

use glam::Vec3;

use crate::data::{DestroyableDef, MaterialDef, link_flags, piece_flags};

/// `rfg_stress_controls.xtbl` (row `Default` is what campaign buildings use).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StressControl {
    pub objects_per_frame: u32,
    pub repeat_delay_min_ms: u32,
    pub repeat_delay_max_ms: u32,
    pub force_scalar: f32,
    pub parent_yield_scalar: f32,
}

impl StressControl {
    pub const DEFAULT: Self = Self {
        objects_per_frame: 400,
        repeat_delay_min_ms: 100,
        repeat_delay_max_ms: 5000,
        force_scalar: 0.12,
        parent_yield_scalar: 1.4,
    };
    pub const MULTI: Self = Self { force_scalar: 0.1, parent_yield_scalar: 0.5, ..Self::DEFAULT };
    pub const WRECKING_CREW_LOW: Self = Self {
        objects_per_frame: 800,
        repeat_delay_min_ms: 50,
        repeat_delay_max_ms: 700,
        force_scalar: 0.08,
        parent_yield_scalar: 0.5,
    };
    pub const WRECKING_CREW_HIGH: Self =
        Self { repeat_delay_max_ms: 400, force_scalar: 0.2, ..Self::WRECKING_CREW_LOW };
}

/// Gravity constant the stress code multiplies by (f64 at 0x00b4d078).
pub const STRESS_GRAVITY: f32 = 9.8;
/// Lower bound of the lever's direction factor (0x00ad6440 / 0x00b4c860).
pub const LEVER_FLOOR: f32 = 0.25;
/// The stress code gathers at most this many supports per piece.
pub const MAX_SUPPORTS: usize = 256;
/// `FUN_006da810` thresholds (0x00aeeb1c, 0x00aeef00).
pub const BIG_PIECE_EXTENT: f32 = 3.0;
pub const BIG_PIECE_VOLUME: f32 = 10.0;
/// Guard for the overflow chain (the game recurses through its damage callback).
pub const MAX_SPILL_DEPTH: u32 = 8;

#[derive(Clone, Debug)]
pub struct Piece {
    pub pos: Vec3,
    pub bmin: Vec3,
    pub bmax: Vec3,
    pub mass: f32,
    pub flags: u8,
    pub material: usize,
    pub links: Vec<u16>,
}

#[derive(Clone, Copy, Debug)]
pub struct Link {
    pub a: u16,
    pub b: u16,
    pub yield_max: i32,
    pub area: f32,
    pub flags: u8,
}

impl Link {
    pub fn other(&self, piece: usize) -> usize {
        if self.a as usize == piece { self.b as usize } else { self.a as usize }
    }
}

/// Id of the island that is still fixed to the world.
pub const ANCHORED: u32 = 0;

#[derive(Clone, Debug)]
pub struct Structure {
    pub pieces: Vec<Piece>,
    pub links: Vec<Link>,
    /// Remaining link strength (starts at `yield_max`, `FUN_006ece30`).
    pub link_strength: Vec<i32>,
    pub link_broken: Vec<bool>,
    pub hp: Vec<i32>,
    pub destroyed: Vec<bool>,
    /// Wave roots: pieces held by the world.
    pub root: Vec<bool>,
    /// Rigid body each piece belongs to; [`ANCHORED`] is the static building.
    pub island: Vec<u32>,
    next_island: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StressReport {
    pub broken_links: Vec<usize>,
    pub parent_yield_breaks: usize,
    pub cluster_breaks: usize,
    pub waves: usize,
    /// Highest stress / strength ratio of any cluster (diagnostic).
    pub worst_ratio: f32,
}

/// A group of pieces that left an island.
#[derive(Clone, Debug, PartialEq)]
pub struct Split {
    pub from: u32,
    pub to: u32,
    pub pieces: Vec<usize>,
}

#[derive(Clone, Copy, Default)]
struct Load {
    com: Vec3,
    mass: f32,
    stress: f32,
    strength: i64,
}

impl Structure {
    /// `root(i)` decides which pieces the world holds (the game's contact records).
    pub fn new(def: &DestroyableDef, materials: &[MaterialDef], root: impl Fn(usize, &Piece) -> bool) -> Self {
        let pieces: Vec<Piece> = def
            .pieces
            .iter()
            .map(|p| Piece {
                pos: Vec3::from(p.pos),
                bmin: Vec3::from(p.bmin),
                bmax: Vec3::from(p.bmax),
                mass: p.mass,
                flags: p.flags,
                material: p.material,
                links: p.links.clone(),
            })
            .collect();
        let links: Vec<Link> = def
            .links
            .iter()
            .map(|&(a, b, yield_max, area, flags)| Link { a, b, yield_max, area, flags })
            .collect();
        let hp = pieces.iter().map(|p| piece_hit_points(p, materials)).collect();
        let root = pieces.iter().enumerate().map(|(i, p)| root(i, p)).collect();
        let mut structure = Self {
            link_strength: links.iter().map(|l| l.yield_max).collect(),
            link_broken: links.iter().map(|l| l.flags & link_flags::BROKEN != 0).collect(),
            destroyed: vec![false; pieces.len()],
            island: vec![ANCHORED; pieces.len()],
            hp,
            root,
            pieces,
            links,
            next_island: 1,
        };
        structure.anchor_unrooted_groups();
        structure
    }

    /// Groups that no root reaches at load (glass panes behind pre-broken links, parts hung off
    /// the terrain) are held by the world in the game through contact records that are not yet
    /// decoded (`FUN_006d08c0`). Until then each such group is rooted at its lowest pieces, so a
    /// pristine object stays exactly as placed.
    pub fn anchor_unrooted_groups(&mut self) {
        let n = self.pieces.len();
        let mut seen = vec![false; n];
        for start in 0..n {
            if seen[start] {
                continue;
            }
            seen[start] = true;
            let mut group = vec![start];
            let mut i = 0;
            while i < group.len() {
                let p = group[i];
                i += 1;
                let next: Vec<usize> = self.live_links(p).map(|(_, q)| q).collect();
                for q in next {
                    if !seen[q] {
                        seen[q] = true;
                        group.push(q);
                    }
                }
            }
            if group.iter().any(|&p| self.root[p]) {
                continue;
            }
            let floor = group.iter().map(|&p| self.pieces[p].bmin.y).fold(f32::INFINITY, f32::min);
            for &p in &group {
                if self.pieces[p].bmin.y <= floor + 0.15 {
                    self.root[p] = true;
                }
            }
        }
    }

    pub fn live_links(&self, piece: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.pieces[piece].links.iter().filter_map(move |&l| {
            let l = l as usize;
            (!self.link_broken[l]).then(|| (l, self.links[l].other(piece)))
        })
    }

    /// Whether a piece has lost any of its original links (shows its fracture surface).
    pub fn is_cracked(&self, piece: usize) -> bool {
        self.pieces[piece].links.iter().any(|&l| {
            self.link_broken[l as usize] && self.links[l as usize].flags & link_flags::BROKEN == 0
        })
    }

    pub fn break_link(&mut self, link: usize) -> bool {
        !std::mem::replace(&mut self.link_broken[link], true)
    }

    /// One full stress pass over the anchored island (the game spreads the same work over
    /// frames, `Objects_Per_Frame` pieces at a time).
    pub fn stress_pass(&mut self, control: &StressControl) -> StressReport {
        let n = self.pieces.len();
        let mut depth = vec![-1i32; n];
        let mut waves: Vec<Vec<usize>> = vec![Vec::new()];
        for i in 0..n {
            if self.root[i] && self.island[i] == ANCHORED && !self.destroyed[i] {
                depth[i] = 0;
                waves[0].push(i);
            }
        }
        let mut d = 0;
        while d < waves.len() {
            let mut next = Vec::new();
            for &p in &waves[d] {
                for (_, q) in self.live_links(p) {
                    if depth[q] < 0 && self.island[q] == ANCHORED && !self.destroyed[q] {
                        depth[q] = d as i32 + 1;
                        next.push(q);
                    }
                }
            }
            if !next.is_empty() {
                waves.push(next);
            }
            d += 1;
        }

        let mut load = vec![Load::default(); n];
        let mut queued: Vec<usize> = Vec::new();
        let mut report = StressReport { waves: waves.len(), ..Default::default() };
        let force_scale = control.force_scalar * STRESS_GRAVITY;
        for d in (1..waves.len()).rev() {
            for &p in &waves[d] {
                let mut supports: Vec<(usize, usize)> = self
                    .live_links(p)
                    .filter(|&(_, q)| depth[q] == d as i32 - 1)
                    .take(MAX_SUPPORTS)
                    .collect();
                let mut strength: i64 = 0;
                for &(l, _) in &supports {
                    strength = (strength + self.link_strength[l] as i64).min(i32::MAX as i64);
                }
                let piece = &self.pieces[p];
                if !supports.is_empty() && strength < (control.parent_yield_scalar * piece.mass) as i64 {
                    supports.retain(|&(l, _)| {
                        let breakable = self.links[l].flags & link_flags::NO_STRESS_BREAK == 0;
                        if breakable {
                            queued.push(l);
                        }
                        !breakable
                    });
                    report.parent_yield_breaks += 1;
                }
                let mut acc = load[p];
                let total = acc.mass + piece.mass;
                if total > 0.0 {
                    acc.com = (acc.com * acc.mass + piece.pos * piece.mass) / total;
                    acc.mass = total / supports.len().max(1) as f32;
                }
                let force = force_scale * acc.mass;
                for &(l, q) in &supports {
                    if self.link_strength[l] < 0 {
                        continue;
                    }
                    acc.stress += lever(piece, acc.com, &self.pieces[q]) * force;
                    acc.strength += self.link_strength[l] as i64;
                    let below = &mut load[q];
                    let sum = below.mass + acc.mass;
                    if sum > 0.0 {
                        below.com = (below.com * below.mass + acc.com * acc.mass) / sum;
                        below.mass = sum;
                    }
                }
                load[p] = acc;
            }
            // Clusters: pieces of this wave joined by unbroken links.
            let mut seen = vec![false; n];
            for &start in &waves[d] {
                if seen[start] {
                    continue;
                }
                seen[start] = true;
                let mut cluster = vec![start];
                let mut i = 0;
                while i < cluster.len() {
                    let p = cluster[i];
                    i += 1;
                    for (_, q) in self.live_links(p) {
                        if !seen[q] && depth[q] == d as i32 {
                            seen[q] = true;
                            cluster.push(q);
                        }
                    }
                }
                let (mut stress, mut strength) = (0.0f32, 0u32);
                for &p in &cluster {
                    if self.pieces[p].flags & piece_flags::FOUNDATION == 0 {
                        stress += load[p].stress;
                        strength = strength.wrapping_add(load[p].strength as u32);
                    }
                }
                if strength > 0 {
                    report.worst_ratio = report.worst_ratio.max(stress / strength as f32);
                }
                if strength < stress.round().min(u32::MAX as f32) as u32 {
                    report.cluster_breaks += 1;
                    for &p in &cluster {
                        for (l, q) in self.live_links(p) {
                            if depth[q] == d as i32 - 1 && self.links[l].flags & link_flags::NO_STRESS_BREAK == 0 {
                                queued.push(l);
                            }
                        }
                    }
                }
            }
        }
        for l in queued {
            if self.break_link(l) {
                report.broken_links.push(l);
            }
        }
        report
    }

    /// Recomputes rigid groups after links broke. Anchored pieces that can no longer reach a root
    /// and dynamic islands that fell apart get new island ids.
    pub fn resolve_islands(&mut self) -> Vec<Split> {
        let n = self.pieces.len();
        let mut seen = vec![false; n];
        let mut splits = Vec::new();
        let mut groups: Vec<(u32, Vec<usize>, bool)> = Vec::new();
        for start in 0..n {
            if seen[start] {
                continue;
            }
            let island = self.island[start];
            seen[start] = true;
            let mut group = vec![start];
            let mut rooted = self.root[start] && !self.destroyed[start];
            let mut i = 0;
            while i < group.len() {
                let p = group[i];
                i += 1;
                let neighbours: Vec<usize> = self.live_links(p).map(|(_, q)| q).collect();
                for q in neighbours {
                    if !seen[q] && self.island[q] == island {
                        seen[q] = true;
                        rooted |= self.root[q] && !self.destroyed[q];
                        group.push(q);
                    }
                }
            }
            groups.push((island, group, rooted));
        }
        // Per island: anchored groups with a root stay; otherwise the largest group keeps the id.
        let mut keeper: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        for (g, (island, group, rooted)) in groups.iter().enumerate() {
            if *island == ANCHORED {
                continue;
            }
            let _ = rooted;
            let best = keeper.entry(*island).or_insert(g);
            if groups[*best].1.len() < group.len() {
                *best = g;
            }
        }
        for (g, (island, group, rooted)) in groups.into_iter().enumerate() {
            let stays = if island == ANCHORED { rooted } else { keeper.get(&island) == Some(&g) };
            if stays {
                continue;
            }
            let to = self.next_island;
            self.next_island += 1;
            for &p in &group {
                self.island[p] = to;
            }
            splits.push(Split { from: island, to, pieces: group });
        }
        splits
    }

    /// Structural damage `damage` (the game's piece-level units) to every piece whose bounds
    /// reach into the sphere. How a weapon's table value becomes this number is not yet recovered.
    pub fn apply_damage(&mut self, center: Vec3, radius: f32, damage: i32, materials: &[MaterialDef], spill: bool) -> Vec<usize> {
        let hit: Vec<usize> = (0..self.pieces.len())
            .filter(|&i| {
                let p = &self.pieces[i];
                !self.destroyed[i] && center.clamp(p.bmin, p.bmax).distance(center) <= radius
            })
            .collect();
        for &i in &hit {
            self.damage_piece(i, damage, center, materials, spill, 0);
        }
        hit
    }

    /// `FUN_006d4290`: damage one piece at `point`.
    /// - Hit points drop by `damage` (foundation pieces are immune); at zero the piece is destroyed.
    /// - A surviving piece larger than 3 m or 10 m^3 (`FUN_006da810`) takes no link damage.
    /// - Otherwise each breakable link loses `clamp((1 - d / diagonal) * area, 0.01, 1) *
    ///   min(15 * damage, 0x7fffff)` in 8.8 fixed point, d = distance from `point` to the link's
    ///   contact. A destroyed piece's links break outright.
    /// - A link driven below 1 breaks; with `spill`, the overflow times the piece material's
    ///   1/Brittleness damages the neighbour (callback 0x006d5fc0).
    pub fn damage_piece(&mut self, piece: usize, damage: i32, point: Vec3, materials: &[MaterialDef], spill: bool, depth: u32) {
        let p = self.pieces[piece].clone();
        let foundation = p.flags & piece_flags::FOUNDATION != 0;
        if self.hp[piece] >= 1 && !foundation {
            self.hp[piece] = if self.destroyed[piece] { -1 } else { self.hp[piece].saturating_sub(damage) };
        }
        let destroyed = !foundation && self.hp[piece] < 1;
        if destroyed {
            self.destroyed[piece] = true;
        } else {
            let size = p.bmax - p.bmin;
            if size.max_element() > BIG_PIECE_EXTENT || size.x * size.y * size.z > BIG_PIECE_VOLUME {
                return;
            }
        }
        let diagonal = (p.bmax - p.bmin).length().max(1e-3);
        let base = ((damage as i64) * 15).clamp(0, 0x7f_ffff) << 8;
        let brittle_inverse = materials.get(p.material).map_or(1.0, |m| 1.0 / m.brittleness.max(1e-3));
        for &l in &p.links {
            let l = l as usize;
            let link = self.links[l];
            if self.link_broken[l] || link.flags & link_flags::NO_STRESS_BREAK != 0 {
                continue;
            }
            let other = link.other(piece);
            let q = &self.pieces[other];
            let lo = p.bmin.max(q.bmin);
            let hi = p.bmax.min(q.bmax);
            let contact = if lo.cmple(hi).all() { (lo + hi) * 0.5 } else { q.pos.clamp(p.bmin, p.bmax) };
            let f = ((1.0 - point.distance(contact) / diagonal) * link.area).clamp(0.01, 1.0);
            let mut amount = (((f * 256.0) as i64 * base) >> 16) as i32;
            if destroyed && amount < self.link_strength[l] {
                amount = self.link_strength[l];
            }
            self.link_strength[l] -= amount;
            if self.link_strength[l] < 1 {
                self.break_link(l);
                let overflow = (brittle_inverse * -(self.link_strength[l] as f32)).round() as i32;
                if spill && overflow > 0 && depth < MAX_SPILL_DEPTH && !self.destroyed[other] {
                    self.damage_piece(other, overflow, point, materials, spill, depth + 1);
                }
            }
        }
    }

    pub fn destroy_piece(&mut self, piece: usize) {
        self.destroyed[piece] = true;
        let links: Vec<usize> = self.pieces[piece].links.iter().map(|&l| l as usize).collect();
        for l in links {
            self.break_link(l);
        }
    }
}

/// Hit points of a piece (`FUN_006eb140`): footprint area x 256, truncated, times
/// `LinkStrength * 1000 * 256`, shifted right 16. Foundation pieces cannot be destroyed.
pub fn piece_hit_points(piece: &Piece, materials: &[MaterialDef]) -> i32 {
    if piece.flags & piece_flags::FOUNDATION != 0 {
        return i32::MAX;
    }
    let size = piece.bmax - piece.bmin;
    let footprint = (size.x * size.z * 256.0) as i64;
    let strength = materials.get(piece.material).map_or(200_000, |m| m.link_strength);
    ((footprint * (strength << 8)) >> 16).clamp(1, i32::MAX as i64) as i32
}

/// `FUN_006ee420`.
pub fn lever(piece: &Piece, load_centre: Vec3, support: &Piece) -> f32 {
    let lo = piece.bmin.max(support.bmin);
    let hi = piece.bmax.min(support.bmax);
    let contact = if lo.cmple(hi).all() {
        (lo + hi) * 0.5
    } else {
        // Bounds that do not touch: the game looks for a closest point (FUN_009eab20, not yet
        // decoded) and otherwise uses the support's centre.
        (support.bmin + support.bmax) * 0.5
    };
    let d = load_centre - contact;
    let length = d.length();
    if length <= f32::EPSILON {
        return 0.0;
    }
    let up = (d.y / length).max(0.0);
    (1.0 - up * up).max(LEVER_FLOOR) * length
}
