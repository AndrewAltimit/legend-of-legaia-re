//! One-off investigation sweep: for every mednafen scenario in the library,
//! read the live field buffer (scratchpad `_DAT_1f8003ec`) and classify its
//! `+0x4000` collision grid AND its `+0x8000` object-index grid against
//! candidate on-disc `.MAP` base grids (town01 = PROT 0010, town0c = PROT
//! 0028). Answers which base map a session's field buffer actually holds -
//! the cold-vs-variant `.MAP` streaming question - and cross-validates the
//! object-placement source (`Scene::field_object_placements`) the same way
//! the collision grid was pinned.
//!
//! Run: `cargo run -p legaia-engine-shell --release --example field_grid_census`
//! Requires `extracted/PROT.DAT`, `scripts/scenarios.toml`, `saves/library`.

use legaia_engine_core::capture_observations::field_pack_intra_transition::read_pool_slot_name;
use legaia_mednafen::{SaveState, ScenarioManifest};
use std::path::PathBuf;

fn main() {
    let prot = std::fs::read("extracted/PROT.DAT").expect("extracted/PROT.DAT");
    // (label, PROT entry byte offset) of candidate field .MAP entries:
    // for each scene the `define-2` (preceding-cluster) candidate vs the
    // first in-block FIELD_MAP_LEN candidate.
    let bases = [
        ("town01/0010", 0x0015_9800usize),
        ("town0c/0028", 0x0034_2000usize),
        ("keik/0109", 0x007E_C000usize),
        ("keik/0118", 0x0084_5800usize),
        ("dolk/0058", 0x004D_B800usize),
        ("dolk/0066", 0x0056_5000usize),
        ("koin3/0559", 0x024B_B000usize),
        ("koin3/0568", 0x0253_6800usize),
    ];
    // Per candidate: (collision grid +0x4000..0x8000, object grid +0x8000..0x10000).
    let grids: Vec<(&str, &[u8], &[u8])> = bases
        .iter()
        .map(|&(n, off)| {
            (
                n,
                &prot[off + 0x4000..off + 0x8000],
                &prot[off + 0x8000..off + 0x10000],
            )
        })
        .collect();

    let manifest = ScenarioManifest::from_path("scripts/scenarios.toml").expect("manifest");
    let lib = PathBuf::from("saves/library");
    for scn in &manifest.scenarios {
        let Ok(p) = manifest.mednafen_save_path(scn, Some(&lib)) else {
            continue;
        };
        if !p.exists() {
            continue;
        }
        let Ok(state) = SaveState::from_path(&p) else {
            continue;
        };
        let (Ok(ram), Ok(scratch)) = (state.main_ram(), state.scratch_ram()) else {
            continue;
        };
        let scene = read_pool_slot_name(ram, 0).unwrap_or_else(|| "?".into());
        let fb = u32::from_le_bytes(scratch[0x3EC..0x3F0].try_into().unwrap());
        if fb & 0xFF00_0000 != 0x8000_0000 {
            println!(
                "{:38} scene {:8} field buffer ptr invalid (0x{fb:08X})",
                scn.label, scene
            );
            continue;
        }
        let base_off = (fb & 0x1F_FFFF) as usize;
        if base_off + 0x10000 > ram.len() {
            continue;
        }
        let live_coll = &ram[base_off + 0x4000..base_off + 0x8000];
        let live_obj = &ram[base_off + 0x8000..base_off + 0x10000];
        let mut line = format!("{:38} scene {:8}", scn.label, scene);
        for (name, coll, obj) in &grids {
            let dc = live_coll.iter().zip(*coll).filter(|(a, b)| a != b).count();
            let dobj = live_obj.iter().zip(*obj).filter(|(a, b)| a != b).count();
            line.push_str(&format!("  vs {name}: c{dc:5} o{dobj:5}"));
        }
        println!("{line}");
    }
}
