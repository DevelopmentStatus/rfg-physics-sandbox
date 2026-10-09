//! Support-removal experiment on a converted chunk, without rigid bodies:
//! destroys ground-floor pieces in order and reports what the game's stress rules do.
//! Usage: cargo run -p rfg_destruction --example demolish -- <chunk> [steps]

use rfg_destruction::data::piece_flags;
use rfg_destruction::{ANCHORED, ChunkFile, MaterialDef, StressControl, Structure};

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().expect("chunk name");
    let steps: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);
    let damage: i32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(150_000);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../LocalData/Assets/chunks");
    let materials: Vec<MaterialDef> =
        serde_json::from_str(&std::fs::read_to_string(dir.join("materials.json")).unwrap()).unwrap();
    let chunk: ChunkFile =
        serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{name}.json"))).unwrap()).unwrap();
    let def = chunk.destroyables.iter().max_by_key(|d| d.pieces.len()).unwrap();
    let base = def.pieces.iter().map(|p| p.bmin[1]).fold(f32::INFINITY, f32::min);
    let mut s = Structure::new(def, &materials, |_, p| p.bmin.y < base + 0.15);
    let total_mass: f32 = s.pieces.iter().map(|p| p.mass).sum();
    // Ground-floor supports: pieces standing on the roots, sorted along X.
    // Support ring: non-root, non-foundation pieces linked directly to a root piece.
    let mut supports: Vec<usize> = (0..s.pieces.len())
        .filter(|&i| {
            !s.root[i]
                && s.pieces[i].flags & piece_flags::FOUNDATION == 0
                && s.live_links(i).any(|(_, q)| s.root[q])
        })
        .collect();
    supports.sort_by(|&a, &b| s.pieces[a].pos.x.total_cmp(&s.pieces[b].pos.x));
    println!("{name}/{}: {} pieces, {:.0} t, {} ground-floor supports", def.name, s.pieces.len(), total_mass / 1000.0, supports.len());
    let per_step = (supports.len() / steps.max(1)).max(1);
    for (step, chunk) in supports.chunks(per_step).enumerate() {
        for &p in chunk {
            // A hammer-like hit at the piece centre (damage in the game's piece units).
            let c = (s.pieces[p].bmin + s.pieces[p].bmax) * 0.5;
            s.apply_damage(c, 0.4, damage, &materials, true);
        }
        s.resolve_islands();
        // The game repeats passes until nothing more breaks (100 ms apart while active).
        let mut passes = 0;
        let mut broken = 0;
        let mut worst = 0.0f32;
        loop {
            let r = s.stress_pass(&StressControl::DEFAULT);
            passes += 1;
            worst = worst.max(r.worst_ratio);
            if r.broken_links.is_empty() || passes > 50 {
                break;
            }
            broken += r.broken_links.len();
            s.resolve_islands();
        }
        let fallen: f32 = (0..s.pieces.len()).filter(|&i| s.island[i] != ANCHORED).map(|i| s.pieces[i].mass).sum();
        println!(
            "step {step:2}: removed {:3} supports, stress broke {broken:4} links in {passes:2} passes (worst ratio {worst:.3}), {:5.1}% of mass detached",
            ((step + 1) * per_step).min(supports.len()),
            100.0 * fallen / total_mass
        );
        if fallen / total_mass > 0.9 {
            break;
        }
    }
}
