//! One-off probe: list a scene's MAN partition-1 actor placements with the
//! cold-flag spawn relocation each record's prologue resolves to.
//!
//! Run with:
//!   cargo run --release -p legaia-engine-core --example dump_placements -- town01

use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{grid_byte_to_world, placement_spawn_relocation};
use legaia_engine_core::scene::{ProtIndex, Scene};

fn main() -> anyhow::Result<()> {
    let extracted = PathBuf::from("extracted");
    let p = ProtIndex::open_extracted(&extracted)?;
    let name = std::env::args().nth(1).expect("scene name");

    let scene = Scene::load(&p, &name)?;
    let man = scene.field_man_payload(&p)?.expect("no MAN");
    let mf = legaia_asset::man_section::parse(&man)?;

    for pl in mf.actor_placements(&man) {
        let reloc = placement_spawn_relocation(&mf, &man, &pl, &|_| false);
        let (rx, rz) = match reloc {
            Some((xe, ze)) => (
                grid_byte_to_world(xe) as f32 / 128.0,
                grid_byte_to_world(ze) as f32 / 128.0,
            ),
            None => (pl.world_x as f32 / 128.0, pl.world_z as f32 / 128.0),
        };
        println!(
            "P1[{:2}] model={:3} anim={:2} special={} header=({:5.2},{:5.2}) cold=({:6.2},{:6.2}){}",
            pl.index,
            pl.model_index,
            pl.anim_id,
            pl.special_model as u8,
            pl.world_x as f32 / 128.0,
            pl.world_z as f32 / 128.0,
            rx,
            rz,
            if reloc.is_some() { "  [reloc]" } else { "" },
        );
    }
    Ok(())
}
