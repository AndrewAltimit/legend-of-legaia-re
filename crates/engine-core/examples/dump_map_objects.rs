//! Decode a scene `.MAP`'s object layer the way retail's scene-init object
//! spawner `FUN_8003A55C` does, and print each spawned object with the MAN
//! partition-0 record it binds.
//!
//! Layout (all offsets into the `.MAP` file):
//!   0x0000..0x4000  object descriptors, 0x20 bytes each (index space 0..0x1FF)
//!   0x4000..0x8000  collision / floor grid (0x80 x 0x80 bytes)
//!   0x8000..0x10000 per-tile object-index map (0x80 x 0x80 u16; `& 0x1FF`)
//!   0x10000..       region + kind-1 tile-trigger tables
use std::path::PathBuf;

use legaia_engine_core::field_regions::lookup_tile_trigger;
use legaia_engine_core::scene::{ProtIndex, Scene};

fn main() -> anyhow::Result<()> {
    let extracted = PathBuf::from("extracted");
    let p = ProtIndex::open_extracted(&extracted)?;
    for name in std::env::args().skip(1) {
        let scene = Scene::load(&p, &name)?;
        let idx = scene.field_map_index(&p).expect("map entry");
        let map = p.entry_bytes_lba_footprint(idx)?;
        let (primary, fallback) = scene.field_tile_triggers(&p)?;
        println!("=== {name} (.MAP PROT[{idx}], {} bytes)", map.len());
        for tz in 0..0x80usize {
            for tx in 0..0x80usize {
                let o = 0x8000 + (tz * 0x80 + tx) * 2;
                let raw = u16::from_le_bytes([map[o], map[o + 1]]);
                let oi = (raw & 0x1FF) as usize;
                let d = &map[oi * 0x20..oi * 0x20 + 0x20];
                let flags = i16::from_le_bytes([d[0x12], d[0x13]]);
                if flags & 4 == 0 {
                    continue;
                }
                let dx = d[6] as i8 as i32;
                let dz = d[7] as i8 as i32;
                let (kx, kz) = (tx as i32 + dx, tz as i32 + dz);
                if !(0..0x80).contains(&kx) || !(0..0x80).contains(&kz) {
                    continue;
                }
                let Some(t) = lookup_tile_trigger(&primary, &fallback, kx as u8, kz as u8) else {
                    continue;
                };
                let ox = i16::from_le_bytes([d[0], d[1]]) as i32 + tx as i32 * 0x80 + 0x40;
                let oz = tz as i32 * 0x80 - (i16::from_le_bytes([d[4], d[5]]) as i32 - 0x40);
                // FUN_801CFC40 contact-box centre: object world position plus
                // the descriptor's coarse (*128) + fine (*16) box offsets.
                let fx = d[0x0E] as i8 as i32;
                let fz = d[0x0F] as i8 as i32;
                let cx = ox + dx * 0x80 + fx * 0x10;
                let cz = oz + dz * 0x80 + fz * 0x10;
                let (keyx, keyz) = (kx * 128 + 0x40, kz * 128 + 0x40);
                println!(
                    "  obj#{oi:<3} tile=({tx:>3},{tz:>3}) world=({ox:>6},{oz:>6}) key=({kx:>3},{kz:>3}) -> rec {} gate={} contact=({cx:>6},{cz:>6}) keycentre=({keyx:>6},{keyz:>6}) delta=({:>5},{:>5}) fine=({fx},{fz})",
                    t.record,
                    t.gate,
                    cx - keyx,
                    cz - keyz
                );
            }
        }
    }
    Ok(())
}
