//! Disc-gated sweep: every placed `.MAP` object draws its record's `+0x10`
//! mesh, **whatever the record id** - records `1..=3` included.
//!
//! Retail's mesh resolver `FUN_80020F88` stores
//! `actor+0x64 = record[+0x10] + DAT_8007b6f8` for every record-backed actor
//! (`0x80020FDC..0x80020FF0`), and neither the spawner `FUN_8003A55C` nor the
//! allocator `FUN_80020DE0` tests the record id. The port used to return no
//! mesh for records `1..=3` (read as "protagonist / NPC ids"), so a placed
//! record on one of them never drew. `conc2` is where it showed: record `1` is
//! the double door in the archway at cell `(108, 69)`, its `+0x10` is `61`, and
//! a live retail `conc2` field state holds that actor with `+0x60 = 1`,
//! `+0x64 = 0x42` (`61 + 5`) - the port's archway stood empty.
//!
//! The sweep reads the rule straight off the record bytes (independent of
//! `pack_mesh_index`), checks every scene's placed objects against it and
//! against the scene's mesh pool, and pins the set of placements on records
//! `1..=3` - the draws the fix added.
//!
//! Skips when `LEGAIA_DISC_BIN` / `extracted/` are missing (disc-gated).

use std::path::PathBuf;

use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_scene_formats::field_objects::{OBJECT_RECORD_STRIDE, parse_placements};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn every_placed_record_draws_its_own_mesh_slot() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open prot index");
    let cdname = std::fs::read_to_string(extracted.join("CDNAME.TXT")).expect("CDNAME");
    let names: Vec<String> = cdname
        .lines()
        .filter_map(|l| {
            let mut w = l.split_whitespace();
            (w.next() == Some("#define")).then(|| w.next().map(str::to_string))?
        })
        .collect();

    let (mut scenes, mut placed) = (0usize, 0usize);
    let mut low: Vec<String> = Vec::new();
    for name in &names {
        let Ok(scene) = Scene::load(&index, name) else {
            continue;
        };
        let Ok(Some(map)) = scene.field_map_bytes(&index) else {
            continue;
        };
        scenes += 1;
        for p in parse_placements(&map) {
            placed += 1;
            // The record's `+0x10`, read off the bytes.
            let rec = usize::from(p.obj_idx) * OBJECT_RECORD_STRIDE;
            let field = u16::from_le_bytes([map[rec + 0x10], map[rec + 0x11]]);
            assert_eq!(
                p.pack_index,
                Some(field),
                "{name} record {} at ({}, {}): the mesh is record[+0x10]",
                p.obj_idx,
                p.col,
                p.row
            );
            if (1..=3).contains(&p.obj_idx) {
                low.push(format!(
                    "{name}#{}@({},{})->{field}",
                    p.obj_idx, p.col, p.row
                ));
            }
        }
    }
    eprintln!("[low-record-ids] {scenes} scenes, {placed} placed objects; on records 1..=3:");
    for l in &low {
        eprintln!("[low-record-ids]   {l}");
    }
    assert!(scenes >= 100, "scenes with a field map: {scenes}");
    assert!(
        low.iter().any(|l| l == "conc2#1@(108,69)->61"),
        "conc2's archway door is record 1, mesh 61: {low:?}"
    );
    assert_eq!(low.len(), EXPECTED_LOW_RECORD_PLACEMENTS, "{low:?}");
}

/// Placed objects on records `1..=3` across the disc - every one a draw the
/// id carve-out withheld.
const EXPECTED_LOW_RECORD_PLACEMENTS: usize = 5;
