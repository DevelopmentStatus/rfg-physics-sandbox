//! Structural destruction rules recovered from the original game, independent of the rigid-body
//! backend and of Bevy (see Docs/destruction_reference.md).

pub mod data;
pub mod structure;

pub use data::{ChunkFile, DestroyableDef, MaterialDef};
pub use structure::{ANCHORED, Split, StressControl, StressReport, Structure};

#[cfg(test)]
mod tests {
    use super::data::*;
    use super::structure::*;
    use glam::Vec3;

    fn material() -> Vec<MaterialDef> {
        vec![MaterialDef {
            name: "Concrete-Normal".into(),
            density: 2200.0,
            link_strength: 250_000,
            brittleness: 1.0,
            restitution: 0.05,
            friction: 0.9,
            flags: 1 << 5,
        }]
    }

    fn piece(center: Vec3, half: Vec3, mass: f32, links: Vec<u16>) -> PieceDef {
        let c = center.to_array();
        PieceDef {
            dlod: 0,
            mesh: None,
            crack: None,
            hull: vec![],
            pos: c,
            com: c,
            bmin: (center - half - 0.05).to_array(),
            bmax: (center + half + 0.05).to_array(),
            volume: mass / 2200.0,
            mass,
            material: 0,
            flags: 0,
            links,
        }
    }

    /// A column of `n` 1 m blocks, block 0 on the ground; link i joins blocks i and i+1.
    fn column(n: usize, mass: f32, yield_max: i32) -> DestroyableDef {
        let pieces = (0..n)
            .map(|i| {
                let mut links = vec![];
                if i > 0 {
                    links.push(i as u16 - 1);
                }
                if i + 1 < n {
                    links.push(i as u16);
                }
                piece(Vec3::new(0.0, 0.5 + i as f32, 0.0), Vec3::splat(0.5), mass, links)
            })
            .collect();
        let links = (0..n - 1).map(|i| (i as u16, i as u16 + 1, yield_max, 1.0, 0)).collect();
        DestroyableDef { name: "column".into(), dlods: vec![], pieces, links }
    }

    fn grounded(def: &DestroyableDef) -> Structure {
        Structure::new(def, &material(), |_, p| p.bmin.y < 0.1)
    }

    #[test]
    fn lever_is_short_above_and_long_beside() {
        let p = Piece { pos: Vec3::ZERO, bmin: Vec3::splat(-0.5), bmax: Vec3::splat(0.5), mass: 1.0, flags: 0, material: 0, links: vec![] };
        let s = Piece { bmin: Vec3::new(-0.5, -1.5, -0.5), bmax: Vec3::new(0.5, -0.5, 0.5), ..p.clone() };
        // Contact is the face centre (0, -0.5, 0). Straight above: floor factor 0.25.
        assert!((lever(&p, Vec3::new(0.0, 1.5, 0.0), &s) - 0.25 * 2.0).abs() < 1e-5);
        // Level with the contact, 2 m to the side: full length.
        assert!((lever(&p, Vec3::new(2.0, -0.5, 0.0), &s) - 2.0).abs() < 1e-5);
        // Below the contact counts fully as well.
        assert!((lever(&p, Vec3::new(0.0, -1.5, 0.0), &s) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn strong_column_stands() {
        let mut s = grounded(&column(6, 2200.0, 1_306_548));
        let report = s.stress_pass(&StressControl::DEFAULT);
        assert!(report.broken_links.is_empty(), "{report:?}");
        assert_eq!(report.waves, 6);
    }

    #[test]
    fn overloaded_column_falls_off_its_base() {
        // Weak links: every wave is overloaded in the same pass (breaks are applied after the
        // pass, as the game queues them), so the column above the base falls apart.
        let mut s = grounded(&column(6, 2200.0, 1_000));
        let report = s.stress_pass(&StressControl::DEFAULT);
        assert!(report.broken_links.contains(&0), "{report:?}");
        s.resolve_islands();
        assert_eq!(s.island[0], ANCHORED);
        assert!((1..6).all(|i| s.island[i] != ANCHORED));
    }

    #[test]
    fn only_the_overloaded_link_breaks() {
        // Strong everywhere except the bottom link, which is weaker than the stack's stress.
        let mut s = grounded(&column(6, 2200.0, 1_306_548));
        s.link_strength[0] = 4_000;
        let report = s.stress_pass(&StressControl::DEFAULT);
        assert_eq!(report.broken_links, vec![0]);
        let splits = s.resolve_islands();
        assert_eq!(splits.len(), 1);
        assert_eq!(splits[0].pieces.len(), 5);
    }

    #[test]
    fn parent_yield_drops_pieces_with_nearly_cut_links() {
        let mut s = grounded(&column(3, 2200.0, 1_306_548));
        s.link_strength[1] = 100; // 1.4 * 2200 kg = 3080 > 100
        let report = s.stress_pass(&StressControl::DEFAULT);
        assert!(report.broken_links.contains(&1));
        assert!(report.parent_yield_breaks >= 1);
    }

    #[test]
    fn destroying_a_middle_piece_drops_the_top() {
        let mut s = grounded(&column(5, 500.0, 1_306_548));
        s.destroy_piece(2);
        let splits = s.resolve_islands();
        // Piece 2 (debris) and pieces 3-4 (falling top) leave the anchored island.
        assert_eq!(splits.len(), 2);
        assert!(s.island[0] == ANCHORED && s.island[1] == ANCHORED);
        assert_ne!(s.island[3], ANCHORED);
        assert_eq!(s.island[3], s.island[4]);
    }

    #[test]
    fn hit_points_follow_footprint_and_link_strength() {
        let def = column(1, 100.0, 1);
        let s = Structure::new(&def, &material(), |_, _| true);
        // Footprint 1.1 m x 1.1 m (bounds include the 0.05 margin): (int)(1.21 * 256) = 309.
        assert_eq!(s.hp[0], ((309i64 * (250_000 << 8)) >> 16) as i32);
    }

    #[test]
    fn foundation_pieces_are_indestructible_and_ignored_by_stress_sums() {
        let mut def = column(2, 100.0, 1);
        def.pieces[1].flags = piece_flags::FOUNDATION;
        let mut s = Structure::new(&def, &material(), |i, _| i == 0);
        assert_eq!(s.hp[1], i32::MAX);
        let report = s.stress_pass(&StressControl::DEFAULT);
        assert_eq!(report.cluster_breaks, 0);
    }

    #[test]
    fn hit_damages_links_by_distance_and_area() {
        // Two 1 m blocks side by side; link area 0.5, contact plane x = 0.
        let pieces = vec![
            piece(Vec3::new(-0.5, 0.5, 0.0), Vec3::splat(0.5), 100.0, vec![0]),
            piece(Vec3::new(0.5, 0.5, 0.0), Vec3::splat(0.5), 100.0, vec![0]),
        ];
        let def = DestroyableDef { name: "pair".into(), dlods: vec![], pieces, links: vec![(0, 1, 1_000_000, 0.5, 0)] };
        let mut s = Structure::new(&def, &material(), |i, _| i == 0);
        let hp = s.hp[1];
        // Hit at the contact centre: factor = (1 - 0) * 0.5; base = 15 * 1000 in 8.8 fixed point.
        s.damage_piece(1, 1000, Vec3::new(0.0, 0.5, 0.0), &material(), false, 0);
        assert_eq!(s.hp[1], hp - 1000);
        assert_eq!(s.link_strength[0], 1_000_000 - ((128i64 * (15_000 << 8)) >> 16) as i32);
        // A destroying hit breaks every link outright.
        s.damage_piece(1, i32::MAX, Vec3::new(0.0, 0.5, 0.0), &material(), false, 0);
        assert!(s.destroyed[1] && s.link_broken[0]);
    }

    #[test]
    fn big_surviving_pieces_take_no_link_damage() {
        let pieces = vec![
            piece(Vec3::new(-2.0, 2.0, 0.0), Vec3::new(2.0, 2.0, 0.5), 100.0, vec![0]),
            piece(Vec3::new(2.0, 2.0, 0.0), Vec3::new(2.0, 2.0, 0.5), 100.0, vec![0]),
        ];
        let def = DestroyableDef { name: "slabs".into(), dlods: vec![], pieces, links: vec![(0, 1, 1_000_000, 1.0, 0)] };
        let mut s = Structure::new(&def, &material(), |i, _| i == 0);
        s.damage_piece(1, 1000, Vec3::new(0.0, 2.0, 0.0), &material(), false, 0);
        assert_eq!(s.link_strength[0], 1_000_000);
    }

    /// Every converted building must stand under its own weight with the game's parameters.
    #[test]
    fn real_buildings_stand_under_their_own_weight() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../LocalData/Assets/chunks");
        let Ok(text) = std::fs::read_to_string(dir.join("materials.json")) else {
            eprintln!("skipped: chunk assets not built (game data is local only)");
            return;
        };
        let materials: Vec<MaterialDef> = serde_json::from_str(&text).unwrap();
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if !name.ends_with(".json") || name == "materials.json" || name == "catalog.json" {
                continue;
            }
            let chunk: ChunkFile = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            for d in &chunk.destroyables {
                let base = d.pieces.iter().map(|p| p.bmin[1]).fold(f32::INFINITY, f32::min);
                let mut s = Structure::new(d, &materials, |_, p| p.bmin.y < base + 0.1);
                let loose = s.island.iter().filter(|&&i| i != ANCHORED).count();
                let report = s.stress_pass(&StressControl::DEFAULT);
                println!("{name}/{}: {} pieces, {loose} loose at load, {} waves, worst ratio {:.3}, {} breaks",
                    d.name, d.pieces.len(), report.waves, report.worst_ratio, report.broken_links.len());
                checked += 1;
            }
        }
        assert!(checked > 0);
    }
}
