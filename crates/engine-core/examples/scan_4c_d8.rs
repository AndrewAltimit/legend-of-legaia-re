//! Scan every CDNAME scene's event-script bytecode for the synchronous
//! actor-spawn opcode `0x4C 0xD8`. Used during reverse-engineering to
//! discover whether the opcode appears in on-disc records or only in
//! runtime-projected field-pack bytecode.
//!
//! Run with:
//!   cargo run --release -p legaia-engine-core --example scan_4c_d8

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_core::scene::{ProtIndex, Scene};

fn main() -> anyhow::Result<()> {
    let extracted = PathBuf::from("extracted");
    let p = ProtIndex::open_extracted(&extracted)?;
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT"))?;
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();

    let mut hits: BTreeMap<String, Vec<(usize, usize, Vec<u8>)>> = BTreeMap::new();
    let mut scanned = 0usize;
    let mut with_scripts = 0usize;
    for name in &scene_names {
        let Ok(scene) = Scene::load(&p, name) else {
            continue;
        };
        scanned += 1;
        let Some(scripts) = scene.find_event_scripts() else {
            continue;
        };
        with_scripts += 1;
        for r in 0..scripts.len() {
            let Some(rec) = scripts.record(r) else {
                continue;
            };
            for i in 0..rec.len().saturating_sub(1) {
                if rec[i] == 0x4C && rec[i + 1] == 0xD8 {
                    let end = (i + 9).min(rec.len());
                    hits.entry(name.clone())
                        .or_default()
                        .push((r, i, rec[i..end].to_vec()));
                }
            }
        }
    }

    println!("scanned scenes: {scanned} (with event scripts: {with_scripts})");
    for (name, h) in &hits {
        println!("{} hits in '{}':", h.len(), name);
        for (rec, off, b) in h {
            print!("  record={rec} off=0x{off:X}: ");
            for byte in b {
                print!("{byte:02X} ");
            }
            println!();
        }
    }
    println!("total scenes with 0x4C 0xD8: {}", hits.len());
    Ok(())
}
