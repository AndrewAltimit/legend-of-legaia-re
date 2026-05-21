//! Disc-gated regression test: decode the monster stat archive (PROT entry
//! `0867_battle_data`) and assert known monster ids decode to the expected
//! name + HP/MP. These values were byte-validated against live battle RAM
//! (Gimard id 10, Killer Bee id 62, Queen Bee id 63 match the actor stats a
//! PCSX-Redux watchpoint captured during the Rim Elm scripted fights).
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.
//!
//! What this catches:
//! - The `(id-1)*0x14000` slot stride or the `[u32 dec_size][LZS]` slot
//!   framing regresses.
//! - The record byte layout (name offset / HP / MP) drifts.
//! - PROT entry 867 stops being the monster archive (e.g. an extractor
//!   change truncates it).

use legaia_asset::monster_archive;
use std::path::PathBuf;

fn entry_867() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT", "../../extracted/PROT"] {
        let f = PathBuf::from(p).join("0867_battle_data.BIN");
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

#[test]
fn known_monster_ids_decode_to_expected_records() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    // (id, name, hp, mp) - the Rim Elm scripted-battle monster set; HP/MP
    // for Gimard/Killer Bee/Queen Bee are byte-exact vs live battle RAM.
    let expected: &[(u16, &str, u16, u16)] = &[
        (4, "Gobu Gobu", 76, 15),
        (7, "Green Slime", 69, 24),
        (10, "Gimard", 99, 20),
        (61, "Hornet", 188, 88),
        (62, "Killer Bee", 288, 288),
        (63, "Queen Bee", 888, 888),
        (79, "Tetsu", 999, 999),
    ];

    for &(id, name, hp, mp) in expected {
        let rec = monster_archive::record(&entry, id)
            .unwrap_or_else(|e| panic!("id {id}: decode error {e:#}"))
            .unwrap_or_else(|| panic!("id {id}: expected a record, got None"));
        assert_eq!(rec.name, name, "id {id} name");
        assert_eq!(rec.hp, hp, "id {id} HP");
        assert_eq!(rec.mp, mp, "id {id} MP");
    }

    // The archive holds 194 fixed slots; a healthy fraction decode to real
    // records (the rest are filler / unused ids - e.g. index 78 "Comm").
    let all = monster_archive::records(&entry).expect("archive walk");
    assert!(
        all.len() > 100,
        "expected >100 populated monster records, got {}",
        all.len()
    );
}

#[test]
fn spell_list_decodes_from_record_offset_array() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    // Gimard (id 10): magic_count 9; the +0x4C offsets resolve to the
    // passive/affinity prefix (ids 0,1,2,4,5,0x0B at cost 0), two offensive
    // castable spells (0x0D @ 28 SP, 0x0F @ 32 SP, both <= its SP stat 60),
    // and the 0x23 ('#') special slot. Spirit (stats[0]) gates casting.
    let gimard = monster_archive::record(&entry, 10).unwrap().unwrap();
    assert_eq!(gimard.magic_count as usize, gimard.spells.len());
    assert_eq!(gimard.magic_count, 9);
    let castable: Vec<(u8, u8)> = gimard
        .spells
        .iter()
        .filter(|s| s.is_castable())
        .map(|s| (s.id, s.sp_cost))
        .collect();
    assert_eq!(castable, vec![(0x0D, 28), (0x0F, 32)]);
    for (id, cost) in &castable {
        assert!(
            (*cost as u16) <= gimard.spirit(),
            "Gimard spell 0x{id:02X} cost {cost} should be <= SP {}",
            gimard.spirit()
        );
    }

    // Every populated record's spell list length matches its declared count,
    // and no offset escaped the block (the parser would have dropped it).
    for r in monster_archive::records(&entry).unwrap() {
        assert_eq!(
            r.magic_count as usize,
            r.spells.len(),
            "id {} magic_count {} != decoded spells {}",
            r.id,
            r.magic_count,
            r.spells.len()
        );
    }
}

#[test]
fn monster_mesh_is_an_embedded_tmd_at_record_plus_4() {
    let Some(entry) = entry_867() else {
        eprintln!("[skip] extracted/PROT/0867_battle_data.BIN or LEGAIA_DISC_BIN missing");
        return;
    };

    // Gimard (id 10): the embedded mesh sits at block +0x7c (= the value of
    // stat record +0x04), parses as a Legaia TMD, and has real geometry.
    let m = monster_archive::mesh(&entry, 10)
        .expect("mesh decode")
        .expect("Gimard has a mesh");
    assert_eq!(m.id, 10);
    assert_eq!(m.tmd_offset, 0x7c);
    let tmd = legaia_tmd::parse(m.tmd_bytes()).expect("Gimard TMD parses");
    let st = tmd.stats();
    assert_eq!(st.total_vertices, 200, "Gimard vertex count");
    assert!(st.total_primitives > 0, "Gimard has primitives");
    // The texture/CLUT pool pointer (+0x08) lands inside the block.
    assert!(
        m.texture_pool_bytes().is_some(),
        "Gimard has a texture pool"
    );

    // Almost every populated stat record carries a parseable mesh; only a
    // handful of slots are empty/filler. Assert the overwhelming majority of
    // the roster has a TMD at +0x04 that the parser walks without error.
    let mut with_mesh = 0usize;
    let mut total = 0usize;
    for id in 1..=monster_archive::slot_count(&entry) as u16 {
        if monster_archive::record(&entry, id).unwrap().is_none() {
            continue; // empty / filler slot
        }
        total += 1;
        if let Some(mesh) = monster_archive::mesh(&entry, id).unwrap()
            && legaia_tmd::parse(mesh.tmd_bytes()).is_ok()
        {
            with_mesh += 1;
        }
    }
    assert!(total > 100, "expected >100 populated records, got {total}");
    assert!(
        with_mesh as f64 / total as f64 > 0.95,
        "expected >95% of populated records to carry a parseable mesh, got {with_mesh}/{total}"
    );
}
