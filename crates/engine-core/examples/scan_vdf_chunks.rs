//! Scan every CDNAME scene for asset-type-0x07 (VDF / set_mime) chunks
//! inside its entries. Prints `(scene, entry_idx, chunk_offset, size,
//! first_bytes)` for each hit. Used to confirm VDF data is reachable from
//! a scene without overlay capture.

use std::path::PathBuf;

use legaia_asset::AssetType;
use legaia_engine_core::scene::{ProtIndex, Scene};

fn main() -> anyhow::Result<()> {
    let extracted = PathBuf::from("extracted");
    let p = ProtIndex::open_extracted(&extracted)?;
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT"))?;
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();

    let mut total_hits = 0usize;
    let mut scenes_with_vdf = 0usize;
    for name in &names {
        let Ok(scene) = Scene::load(&p, name) else {
            continue;
        };
        let mut scene_hit = false;
        for entry in &scene.entries {
            let Ok(report) = legaia_asset::parse_streaming(&entry.bytes, 4096) else {
                continue;
            };
            for c in &report.chunks {
                if matches!(AssetType::from_byte(c.type_byte), AssetType::Vdf) {
                    total_hits += 1;
                    if !scene_hit {
                        scenes_with_vdf += 1;
                        scene_hit = true;
                    }
                    let body_off = c.header_offset + 4;
                    let end = (body_off + 16).min(entry.bytes.len());
                    let prefix: Vec<String> = entry.bytes[body_off..end]
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect();
                    println!(
                        "{name:24} entry={:4} hdr=0x{:X} size={} bytes: {}",
                        entry.idx,
                        c.header_offset,
                        c.size,
                        prefix.join(" ")
                    );
                }
            }
        }
    }
    println!("\nVDF chunk hits: {total_hits} across {scenes_with_vdf} scenes");
    Ok(())
}
