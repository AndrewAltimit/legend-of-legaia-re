//! Disc-gated structural oracle for the per-tile field-region ports
//! (`crates/engine-core/src/field_regions.rs` - FUN_80017FBC /
//! FUN_800180EC / FUN_801DBA20).
//!
//! Walks every CDNAME scene and asserts the retail data matches the shapes
//! the ports assume:
//!
//! - the `.MAP` `+0x10000` region-table header (body offset `s16` at `+0xE`,
//!   count `s16` at `+0x10`) stays in-bounds and its 8-byte records carry
//!   `[x0, z0, x1, z1, type, 0, 0, 0]` (the three pad bytes pin the stride
//!   the retail scan reads from `DAT_8007B31B`);
//! - the MAN section-3 zone table's count byte times the 18-byte stride
//!   fits the section body (`FUN_801DBA20`'s `pbVar7 += 0x12` walk).
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::field_regions::{REGION_RECORD_STRIDE, RegionTable, ZONE_RECORD_STRIDE};
use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn region_and_zone_tables_match_port_shapes_across_corpus() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();

    let mut scenes_with_regions = 0usize;
    let mut total_region_records = 0usize;
    let mut padded_records = 0usize;
    let mut scenes_with_zones = 0usize;
    let mut total_zone_records = 0usize;

    for scene_name in &scene_names {
        if host.load_scene(scene_name).is_err() {
            continue;
        }
        let index = host.index.clone();
        let Some(scene) = host.scene.as_ref() else {
            continue;
        };

        if let Ok(Some(block)) = scene.field_map_region_block(&index)
            && let Some(table) = RegionTable::parse(&block)
            && table.count() > 0
        {
            scenes_with_regions += 1;
            // Header bounds: every record fits the block.
            let body = u16::from_le_bytes([block[0xE], block[0xF]]) as usize;
            let end = body + table.count() * REGION_RECORD_STRIDE;
            assert!(
                end <= block.len(),
                "{scene_name}: region table runs past the .MAP block ({end} > {})",
                block.len()
            );
            for i in 0..table.count() {
                let off = body + i * REGION_RECORD_STRIDE;
                let rec = &block[off..off + REGION_RECORD_STRIDE];
                total_region_records += 1;
                assert!(
                    rec[4] < 32,
                    "{scene_name}: record {i} type byte {} exceeds the 32-bit mask",
                    rec[4]
                );
                if rec[5..8] == [0, 0, 0] {
                    padded_records += 1;
                }
            }
        }

        if let Ok(Some(zone)) = scene.field_zone_table(&index)
            && let Some(&count) = zone.first()
            && count > 0
        {
            scenes_with_zones += 1;
            total_zone_records += count as usize;
            assert!(
                count as usize * ZONE_RECORD_STRIDE < zone.len(),
                "{scene_name}: zone table declares {count} records but the section body \
                 is only {} bytes",
                zone.len()
            );
        }
    }

    // Coverage: the corpus actually exercises both tables.
    assert!(
        scenes_with_regions > 10,
        "expected >10 scenes with region tables, got {scenes_with_regions}"
    );
    assert!(
        scenes_with_zones > 10,
        "expected >10 scenes with zone tables, got {scenes_with_zones}"
    );
    // Stride pin: the 8-byte record's tail pad is zero corpus-wide (the
    // structural evidence behind `REGION_RECORD_STRIDE = 8`).
    assert_eq!(
        padded_records, total_region_records,
        "non-zero region-record pad bytes - stride assumption violated"
    );

    eprintln!(
        "[field_regions] {scenes_with_regions} scenes / {total_region_records} region records; \
         {scenes_with_zones} scenes / {total_zone_records} zone records"
    );
}
