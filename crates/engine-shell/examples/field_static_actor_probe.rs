//! Diagnostic sweep for the STATIC arm of the actor-collision probe
//! (`FUN_801cf9f4` / `FUN_801cfc40` result bit `4`): for every catalogued
//! mednafen field capture, walk the live active-actor table
//! (`DAT_801c93c8`, count `_DAT_8007b6b8`) and, for each static-class entry
//! (`flags & 0x1020000 == 0`), decode the retail anchor formula against the
//! live field buffer's object-record table:
//!
//! ```text
//! off_x = (i8)rec[+0x6] * 0x80 + (i8)rec[+0xE] * 0x10
//! off_z = (i8)rec[+0x7] * 0x80 + (i8)rec[+0xF] * 0x10
//! if actor[+0x52] & 8 { off_x -= (i16)rec[+0x0]; off_z += (i16)rec[+0x4] }
//! box centre = actor[+0x14/+0x18] + (off_x, off_z), half-extent 0x40+0x10
//! ```
//!
//! and print the live actor position, the record-derived offset, the
//! resulting box centre, and the matching engine placement
//! (`Scene::field_object_placements`, matched by `obj_idx == actor[+0x60]`)
//! so the engine's prop-collision source can be pinned against retail.
//!
//! Run: `cargo run -p legaia-engine-shell --release --example field_static_actor_probe`
//! Requires `extracted/`, `scripts/scenarios.toml`, `saves/library`.

use legaia_engine_core::capture_observations::field_pack_intra_transition::read_pool_slot_name;
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_mednafen::{SaveState, ScenarioManifest};
use std::path::PathBuf;

fn rd32(ram: &[u8], va: u32) -> u32 {
    let off = (va & 0x1F_FFFF) as usize;
    u32::from_le_bytes(ram[off..off + 4].try_into().unwrap())
}

fn rd16(ram: &[u8], va: u32) -> u16 {
    let off = (va & 0x1F_FFFF) as usize;
    u16::from_le_bytes(ram[off..off + 2].try_into().unwrap())
}

fn main() {
    let extracted = PathBuf::from("extracted");
    let index = ProtIndex::open_extracted(&extracted).expect("ProtIndex");
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
        let count = rd32(ram, 0x8007_B6B8);
        if count == 0 || count > 0x20 {
            continue;
        }
        let fb = u32::from_le_bytes(scratch[0x3EC..0x3F0].try_into().unwrap());
        if fb & 0xFF00_0000 != 0x8000_0000 {
            continue;
        }
        let fb_off = (fb & 0x1F_FFFF) as usize;
        let scene_name = read_pool_slot_name(ram, 0).unwrap_or_else(|| "?".into());
        let placements = Scene::load(&index, &scene_name)
            .ok()
            .and_then(|scene| scene.field_object_placements(&index).ok().flatten());

        println!("{:38} scene {:8} actors={count}", scn.label, scene_name);
        for i in 0..count {
            let ptr = rd32(ram, 0x801C_93C8 + i * 4);
            if ptr & 0xFF00_0000 != 0x8000_0000 {
                continue;
            }
            let flags = rd32(ram, ptr + 0x10);
            let x = rd16(ram, ptr + 0x14) as i16;
            let z = rd16(ram, ptr + 0x18) as i16;
            let idx60 = rd16(ram, ptr + 0x60) as usize;
            let f52 = rd16(ram, ptr + 0x52);
            if flags & 0x0102_0000 != 0 {
                println!("  [{i}] MOVING flags={flags:08X} pos=({x},{z})");
                continue;
            }
            let rec_off = fb_off + idx60 * 0x20;
            let Some(rec) = ram.get(rec_off..rec_off + 0x20) else {
                continue;
            };
            let mut off_x = (rec[0x6] as i8) as i32 * 0x80 + (rec[0xE] as i8) as i32 * 0x10;
            let mut off_z = (rec[0x7] as i8) as i32 * 0x80 + (rec[0xF] as i8) as i32 * 0x10;
            if f52 & 8 != 0 {
                off_x -= i16::from_le_bytes([rec[0x0], rec[0x1]]) as i32;
                off_z += i16::from_le_bytes([rec[0x4], rec[0x5]]) as i32;
            }
            let (cx, cz) = (x as i32 + off_x, z as i32 + off_z);
            let plc = placements
                .as_deref()
                .and_then(|ps| ps.iter().find(|p| p.obj_idx as usize == idx60));
            let plc_str = match plc {
                Some(p) => format!(
                    "placement obj {} tile ({},{}) world ({},{}) flags={:04X}",
                    p.obj_idx, p.col, p.row, p.world_x, p.world_z, p.flags
                ),
                None => "NO matching placement".into(),
            };
            println!(
                "  [{i}] STATIC flags={flags:08X} rec={idx60} +0x52={f52:04X} \
                 pos=({x},{z}) off=({off_x},{off_z}) centre=({cx},{cz}) | {plc_str}"
            );
        }
    }
}
