//! Disc-gated: a kingdom overworld's VRAM holds its slot-0 atlas CLUTs word
//! for word, transparent zeros included.
//!
//! The kingdom bundle's slot-0 pack is a known DMA list retail uploads in
//! order over the boot-resident rows, and `LoadImage` replaces every word of
//! a CLUT block. The scene build's merge-zeros CLUT pass (kept for the field
//! sweep's over-collected TIMs) used to leave a boot-resident `init_data`
//! word under a kingdom CLUT's entry 0: on `map03` the row-484 slot at
//! `x = 240`, the tree-base quads' palette, kept an opaque near-black where
//! retail's VRAM holds `0x0000`, and every tree stood on a dark square.
//!
//! Skips (and passes) without `extracted/`.

use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::scene_assembly::assemble_field_scene;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

#[test]
fn kingdom_atlas_cluts_overwrite_the_boot_rows() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let mut zero_entries = 0usize;
    for name in ["map01", "map02", "map03"] {
        let a = assemble_field_scene(&index, name).expect("assemble overworld");
        let scene = Scene::load(&index, name).expect("load overworld");
        // The first kingdom bundle in the block is the one the build takes.
        let tims = scene
            .entries
            .iter()
            .find_map(|e| {
                let slot0 = legaia_asset::kingdom_bundle::decode_slot(&e.bytes, 0).ok()?;
                let members = legaia_asset::pack::extract_pack(&slot0).ok()?;
                Some(members.iter().map(|m| m.to_vec()).collect::<Vec<_>>())
            })
            .expect("a kingdom bundle with a slot-0 atlas");
        // Last write wins: the expected word at each CLUT cell is the last
        // atlas member's that covers it.
        let mut expect = std::collections::BTreeMap::new();
        for t in tims.iter().filter_map(|t| legaia_tim::parse(t).ok()) {
            let Some(c) = t.clut else { continue };
            let run = usize::from(c.w) * usize::from(c.h);
            for (k, &w) in c.entries.iter().take(run).enumerate() {
                expect.insert((usize::from(c.fb_x) + k, usize::from(c.fb_y)), w);
            }
        }
        for (&(x, y), &w) in &expect {
            assert_eq!(
                a.res.vram.pixel(x, y),
                w,
                "{name}: CLUT word at ({x}, {y}) is not the kingdom atlas's"
            );
            zero_entries += usize::from(w == 0);
        }
        eprintln!("[ran] {name}: {} kingdom CLUT words match", expect.len());
    }
    assert!(zero_entries > 0, "the check covers transparent entries");
}
