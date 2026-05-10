//! Verify the WASM viewer's in-memory disc walker can extract PROT.DAT from
//! a real Mode2/2352 disc image. Skipped (passes) when LEGAIA_DISC_BIN is unset
//! - same gating pattern as `crates/iso/tests/disc_pipeline.rs`.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::disc::{extract_prot_dat, is_mode2_2352_disc, parse_prot_toc};
use std::env;
use std::fs;

#[test]
fn extract_prot_dat_from_real_disc() {
    let Some(path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping disc extraction test");
        return;
    };
    let bytes = fs::read(&path).expect("disc image");

    assert!(is_mode2_2352_disc(&bytes), "not a Mode2/2352 disc");

    let prot = extract_prot_dat(&bytes).expect("PROT.DAT extraction");
    assert!(prot.len() > 1024 * 1024, "PROT.DAT is suspiciously small");

    let entries = parse_prot_toc(&prot).expect("PROT TOC parse");
    assert!(
        entries.len() > 1000,
        "expected > 1000 PROT entries, got {}",
        entries.len()
    );

    // Sanity: every entry's bytes are within the extracted PROT buffer.
    for e in &entries {
        let end = e.byte_offset + e.size_bytes;
        assert!(
            end as usize <= prot.len(),
            "entry {} ({}..{}) extends past PROT.DAT len {}",
            e.index,
            e.byte_offset,
            end,
            prot.len()
        );
    }

    eprintln!(
        "[ok] extracted {} bytes of PROT.DAT and {} entries from disc",
        prot.len(),
        entries.len()
    );

    // Count entries that have a parseable TMD via the scene_tmd_stream
    // detector - should be the ~148 scene_tmd_stream entries documented.
    let mut tmd_count = 0;
    let mut total_tris: usize = 0;
    let mut total_verts: usize = 0;
    for e in &entries {
        let off = e.byte_offset as usize;
        let end = (e.byte_offset + e.size_bytes) as usize;
        if end > prot.len() {
            continue;
        }
        let buf = &prot[off..end];
        if let Some(s) = legaia_asset::scene_tmd_stream::detect(buf)
            && let Ok(tmd) = legaia_tmd::parse(&buf[s.tmd_range()])
        {
            let mesh = legaia_tmd::mesh::tmd_to_mesh(&tmd, &buf[s.tmd_range()]);
            total_tris += mesh.triangle_count();
            total_verts += mesh.vertex_count();
            tmd_count += 1;
        }
    }
    eprintln!(
        "[ok] {} entries hold parseable scene_tmd_stream TMDs ({} verts, {} tris total)",
        tmd_count, total_verts, total_tris
    );
    assert!(
        tmd_count >= 100,
        "expected ≥100 scene_tmd_stream entries, got {}",
        tmd_count
    );

    // For the first scene_tmd_stream entry we find with a textured prim,
    // verify the textured-mesh attribute arrays end up consistent.
    let mut found_textured = false;
    for e in &entries {
        let off = e.byte_offset as usize;
        let end = (e.byte_offset + e.size_bytes) as usize;
        if end > prot.len() {
            continue;
        }
        let buf = &prot[off..end];
        if let Some(s) = legaia_asset::scene_tmd_stream::detect(buf)
            && let Ok(tmd) = legaia_tmd::parse(&buf[s.tmd_range()])
        {
            let mesh = legaia_tmd::mesh::tmd_to_vram_mesh(&tmd, &buf[s.tmd_range()]);
            if mesh.indices.is_empty() {
                continue;
            }
            assert_eq!(mesh.positions.len(), mesh.uvs.len());
            assert_eq!(mesh.positions.len(), mesh.cba_tsb.len());
            assert_eq!(mesh.indices.len() % 3, 0);
            eprintln!(
                "[ok] PROT {}: {} textured tris, {} verts",
                e.index,
                mesh.triangle_count(),
                mesh.positions.len()
            );
            found_textured = true;
            break;
        }
    }
    assert!(
        found_textured,
        "no scene_tmd_stream with textured prims found"
    );
}
