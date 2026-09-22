//! Disc-gated: data-segment structures read off their consumers.
//!
//! * [`legaia_asset::switch_tables::find`] finds `switch` jump tables from the
//!   dispatch idiom alone. On PROT 0898 it must reproduce every row
//!   [`legaia_asset::battle_jump_tables`] pins by hand - the instrument's own
//!   positive control - plus the two tables just above that head.
//! * [`legaia_asset::field_probe_tables::check`] re-derives the field
//!   overlay's three probe-table bases from their `lui` pairs, and the rows
//!   carry the footprints `docs/subsystems/field-locomotion.md` records.
//!
//! Skips + passes without `extracted/PROT`.

use legaia_asset::{battle_jump_tables, field_probe_tables, switch_tables};
use std::path::PathBuf;

fn entry(idx: u32) -> Option<Vec<u8>> {
    let prefix = format!("{idx:04}_");
    for c in [
        "extracted/PROT",
        "../extracted/PROT",
        "../../extracted/PROT",
    ] {
        let Ok(rd) = std::fs::read_dir(PathBuf::from(c)) else {
            continue;
        };
        let mut hits: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&prefix))
            })
            .collect();
        hits.sort();
        if let Some(p) = hits.into_iter().next() {
            return std::fs::read(p).ok();
        }
    }
    eprintln!("[skip] extracted/PROT/{prefix}* missing");
    None
}

#[test]
fn the_generic_finder_reproduces_every_pinned_battle_table() {
    let Some(img) = entry(898) else { return };
    let found = switch_tables::find(&img, battle_jump_tables::OVERLAY_BASE_VA, img.len());
    for t in &battle_jump_tables::JUMP_TABLES {
        let hit = found
            .iter()
            .find(|f| f.va == t.va)
            .unwrap_or_else(|| panic!("no table found at {:#010x}", t.va));
        assert_eq!((hit.arms, hit.jr), (t.arms, t.jr), "table {:#010x}", t.va);
    }
    // Two more, just above the pinned head and below the first real function
    // (`0x801CFA48`): a nine-arm table after the head's closing zero word, and
    // a seven-arm one after the Seru side-effect message pool. Both sit inside
    // the extent the `FUN_801CF5D0` dump claims - a dump over pointer words
    // and strings, not a function.
    let extra: Vec<_> = found
        .iter()
        .filter(|f| !battle_jump_tables::JUMP_TABLES.iter().any(|t| t.va == f.va))
        .map(|f| (f.va, f.arms, f.jr))
        .collect();
    assert_eq!(
        extra,
        vec![(0x801C_F614, 9, 0x801F_3AC0), (0x801C_FA2C, 7, 0x801F_3EB4)]
    );
    eprintln!("[switch-tables] 0898: {} tables", found.len());
}

#[test]
fn the_field_probe_tables_are_formed_where_documented() {
    let Some(img) = entry(field_probe_tables::OVERLAY_PROT_INDEX) else {
        return;
    };
    let errs = field_probe_tables::check(&img);
    assert!(errs.is_empty(), "{errs:#?}");
    let actor = field_probe_tables::read(
        &img,
        field_probe_tables::ACTOR_PROBE_VA,
        field_probe_tables::ACTOR_PROBE_ROWS,
    )
    .expect("actor rows");
    // Row 0 (Z-): three points 64 ahead, +-32 lateral.
    assert_eq!(&actor[0][..3], &[(-32, 64), (0, 64), (32, 64)]);
    let wall = field_probe_tables::read(
        &img,
        field_probe_tables::WALL_PROBE_VA,
        field_probe_tables::WALL_PROBE_ROWS,
    )
    .expect("wall rows");
    // Row 3 (X+): the wall edge at x+48, +-16 in Z.
    assert_eq!(&wall[3][..3], &[(48, -16), (48, 0), (48, 16)]);
    let facing = field_probe_tables::read(
        &img,
        field_probe_tables::FACING_PROBE_VA,
        field_probe_tables::FACING_PROBE_ROWS,
    )
    .expect("facing rows");
    // Eight compass points, every one at radius 64 on each non-zero axis.
    assert!(
        facing
            .iter()
            .flatten()
            .all(|&(dx, dz)| dx.abs() % 64 == 0 && dz.abs() % 64 == 0 && (dx, dz) != (0, 0))
    );
}
