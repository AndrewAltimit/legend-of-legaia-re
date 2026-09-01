//! Print retail floor-sampler heights around a world point of a scene:
//! the center plus 8-direction rings, for diagnosing multi-layer floors
//! (a doorway trigger whose contact tile samples a surface the approach
//! ground does not walk on).
//!
//! Usage: sample_floor_ring <scene> <world_x> <world_z> [ring_psx...]

use legaia_engine_core::glb_export::FloorSampler;
use legaia_engine_core::scene::{ProtIndex, Scene};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: sample_floor_ring <scene> <world_x> <world_z> [ring_psx...]");
        std::process::exit(2);
    }
    let index = ProtIndex::open_extracted(std::path::Path::new("extracted")).expect("index");
    let scene = Scene::load(&index, &args[1]).expect("scene name");
    let floor = FloorSampler::build(&index, &scene);

    let cx: i32 = args[2].parse().expect("world_x");
    let cz: i32 = args[3].parse().expect("world_z");
    let rings: Vec<i32> = if args.len() > 4 {
        args[4..].iter().map(|a| a.parse().expect("ring")).collect()
    } else {
        vec![64, 128, 192]
    };

    println!(
        "center ({cx}, {cz}): floor {} (export-frame y {:.2})",
        floor.height(cx, cz),
        -(floor.height(cx, cz) as f32) / 64.0
    );
    for r in rings {
        print!("ring {r:>4}: ");
        for (dx, dz) in [
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (-1, -1),
        ] {
            let h = floor.height(cx + dx * r, cz + dz * r);
            print!("{:>6.2} ", -(h as f32) / 64.0);
        }
        println!("(export-frame y, N E-ward clockwise)");
    }
}
