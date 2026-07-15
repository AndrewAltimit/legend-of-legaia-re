//! Audit the `0x3F` named-scene-change transition graph: for each scene,
//! join its `.MAP` gate-1 walk-on tile triggers to the partition-2 records
//! they spawn, and report every destination those records reach.
//!
//! Run with:
//!   cargo run --release -p legaia-engine-core --example scan_scene_transitions -- town01 map01
//! With no args, sweeps every CDNAME scene.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_core::scene::{ProtIndex, Scene};

fn main() -> anyhow::Result<()> {
    let extracted = PathBuf::from("extracted");
    let p = ProtIndex::open_extracted(&extracted)?;
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT"))?;
    let mut all: Vec<String> = cdname.values().cloned().collect();
    all.sort();
    all.dedup();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let want: Vec<String> = if args.is_empty() { all } else { args };

    for name in &want {
        let Ok(scene) = Scene::load(&p, name) else {
            println!("{name}: (no scene)");
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(&p) else {
            println!("{name}: (no MAN)");
            continue;
        };
        let Ok(mf) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        let (primary, fallback) = scene.field_tile_triggers(&p).unwrap_or_default();
        let mut triggers = primary.clone();
        triggers.extend(fallback.clone());

        let dests = legaia_engine_core::man_field_scripts::scene_destinations(&mf, &man);
        let sites =
            legaia_engine_core::man_field_scripts::overworld_portal_sites(&mf, &man, &triggers);
        if dests.is_empty() && sites.is_empty() {
            continue;
        }
        println!("\n=== {name} ===");
        println!("  all 0x3F destinations ({}):", dests.len());
        for d in &dests {
            println!(
                "    -> {:<10} index={:<4} entry=({:#04x},{:#04x}) tile=({},{})",
                d.scene_name,
                d.index,
                d.entry_x,
                d.entry_z,
                d.entry_x & 0x7F,
                d.entry_z & 0x7F
            );
        }
        // Group the gate-1 triggers by the record they spawn, so multi-tile
        // exit bands show as one row.
        let mut by_record: BTreeMap<u8, Vec<(u8, u8)>> = BTreeMap::new();
        for t in triggers.iter().filter(|t| t.gate == 1) {
            by_record
                .entry(t.record)
                .or_default()
                .push((t.tile_x, t.tile_z));
        }
        println!("  gate-1 walk-on triggers -> P2 record:");
        for (rec, tiles) in &by_record {
            let site = sites.iter().find(|s| s.record == *rec);
            match site {
                Some(s) => println!(
                    "    tiles {:?} -> P2[{rec}] -> {} idx={} entry=({},{}) dir={}{}",
                    tiles,
                    s.scene_name,
                    s.index,
                    s.entry_x & 0x7F,
                    s.entry_z & 0x7F,
                    s.dir,
                    s.conditional
                        .as_ref()
                        .map(|c| format!(
                            "  [flag {:#x} SET -> {} ({},{})]",
                            c.flag,
                            c.scene_name,
                            c.entry_x & 0x7F,
                            c.entry_z & 0x7F
                        ))
                        .unwrap_or_default()
                ),
                None => println!("    tiles {:?} -> P2[{rec}] (beat record, no 0x3F)", tiles),
            }
        }
    }
    Ok(())
}
