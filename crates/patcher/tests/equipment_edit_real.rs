//! Disc-gated round-trip for the manual equipment editor: set the Astral
//! Sword's swing cost to the favored tier, price one weapon differently for
//! two characters, reprice a Ra-Seru arm, a pair of boots' Up kick and two
//! section defaults, and hand the Astral Sword (plus Noa's boots) to
//! everyone; re-decode off the patched image and confirm:
//!
//! - every requested cost reads back from the LZS-recompressed section (the
//!   `+0x04` and the footwear-only `+0x08` record alike), and an untouched
//!   weapon keeps its retail value;
//! - Noa's weapon sections are found (they key under section 3, not 2) and
//!   her weapon hand reads as Right;
//! - the owner mask reads back and the report names the characters whose
//!   files have no section for the item (the fall-through case), with the
//!   default record's cost after this pass's edits;
//! - every changed disc sector stays EDC/ECC-valid, and applying the same
//!   edit set twice is a no-op (idempotent).
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::battle_data_pack;
use legaia_iso::raw::SECTOR_SIZE;
use legaia_patcher::apply::{
    self, CostTarget, EquipOwnerEdit, EquipmentEdits, FallThrough, SwingCostEdit, SwingRecord,
    SwingSection,
};
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::weapon_specialty::{PLAYERS, arm_cost_offset, up_cost_offset};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Swing cost of `item` in character `ci`'s file, or `None` without a section.
fn cost_of(patcher: &DiscPatcher, ci: usize, item: u8) -> Option<u8> {
    record_cost_of(patcher, ci, item, SwingRecord::Primary)
}

fn record_cost_of(patcher: &DiscPatcher, ci: usize, item: u8, rec: SwingRecord) -> Option<u8> {
    let buf = patcher.read_player_file(PLAYERS[ci].entry).ok()?;
    let pack = battle_data_pack::detect(&buf)?;
    let (idx, _) = pack
        .records
        .iter()
        .enumerate()
        .find(|(_, r)| r.id == item as u32)?;
    let dec = battle_data_pack::decode_record(&buf, &pack, idx).ok()?;
    let off = match rec {
        SwingRecord::Primary => arm_cost_offset(&dec.bytes)?,
        SwingRecord::Up => up_cost_offset(&dec.bytes)?,
    };
    Some(dec.bytes[off])
}

fn item(character: usize, id: u8, cost: u8) -> SwingCostEdit {
    SwingCostEdit {
        character,
        target: CostTarget::Item(id),
        record: SwingRecord::Primary,
        cost,
    }
}

fn owner_of(patcher: &DiscPatcher, item: u8) -> Option<u8> {
    let t = apply::read_equipment_table(patcher).ok()??;
    t.rows.iter().find(|r| r.id == item).map(|r| r.mask)
}

fn changed_sectors_valid(orig: &[u8], patched: &[u8]) -> usize {
    assert_eq!(orig.len(), patched.len(), "image must not change size");
    let mut checked = 0;
    for s in 0..orig.len() / SECTOR_SIZE {
        let a = s * SECTOR_SIZE;
        let b = a + SECTOR_SIZE;
        if orig[a..b] != patched[a..b] {
            assert!(
                legaia_iso::write::mode2_form1_sector_is_valid(&patched[a..b]),
                "sector {s} invalid after patch"
            );
            checked += 1;
        }
    }
    checked
}

const ASTRAL: u8 = 0xBA;
const SURVIVAL_CLUB: u8 = 0x2E;
const NAIL_GLOVE: u8 = 0x28;
const META_9: u8 = 0x09;
const TRIUMPH_BOOTS: u8 = 0x5E;
const STEEL_BOOTS: u8 = 0x63;

#[test]
fn equipment_edits_round_trip_off_the_patched_disc() {
    let Some(orig) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(orig.clone()).expect("open disc");

    // Retail baseline the doc tabulates.
    assert_eq!(
        cost_of(&patcher, 0, ASTRAL),
        Some(0x36),
        "Astral Sword is Vahn's 54"
    );
    assert_eq!(
        cost_of(&patcher, 1, ASTRAL),
        None,
        "Noa has no Astral section"
    );
    assert_eq!(
        cost_of(&patcher, 1, SURVIVAL_CLUB),
        Some(0x36),
        "Noa club = far tier"
    );
    assert_eq!(
        cost_of(&patcher, 2, NAIL_GLOVE),
        Some(0x2A),
        "Gala claw = off-class"
    );
    assert_eq!(
        owner_of(&patcher, ASTRAL),
        Some(1),
        "Astral Sword is Vahn-only"
    );

    assert_eq!(
        record_cost_of(&patcher, 0, TRIUMPH_BOOTS, SwingRecord::Up),
        Some(0x1E),
        "boots carry an Up record"
    );
    assert_eq!(
        record_cost_of(&patcher, 0, ASTRAL, SwingRecord::Up),
        None,
        "a weapon section has no Up record"
    );

    let edits = EquipmentEdits {
        costs: vec![
            item(0, ASTRAL, 30),
            item(1, SURVIVAL_CLUB, 42),
            item(2, NAIL_GLOVE, 30),
            item(1, ASTRAL, 30), // no section
            SwingCostEdit {
                character: 1,
                target: CostTarget::Default(SwingSection::Weapon),
                record: SwingRecord::Primary,
                cost: 54,
            }, // Noa's fall-through weapon record
            item(0, META_9, 36), // Vahn's Ra-Seru arm (Right)
            SwingCostEdit {
                character: 0,
                target: CostTarget::Item(TRIUMPH_BOOTS),
                record: SwingRecord::Up,
                cost: 40,
            }, // an Up kick
            SwingCostEdit {
                character: 0,
                target: CostTarget::Item(ASTRAL),
                record: SwingRecord::Up,
                cost: 40,
            }, // no Up record on a weapon
            SwingCostEdit {
                character: 2,
                target: CostTarget::Default(SwingSection::Footwear),
                record: SwingRecord::Up,
                cost: 44,
            }, // Gala's barefoot Up kick
        ],
        owners: vec![
            EquipOwnerEdit {
                item_id: ASTRAL,
                mask: 7,
            },
            EquipOwnerEdit {
                item_id: STEEL_BOOTS,
                mask: 7,
            },
        ],
        // The default-look path: no model carried over, so the fall-through
        // notes below name every new owner.
        skip_model_transplant: true,
        ..Default::default()
    };
    let rep = apply::apply_equipment_edits(&mut patcher, &edits).expect("apply");
    assert_eq!(rep.costs_changed, 7, "{rep:?}");
    assert_eq!(
        rep.costs_no_section,
        vec![
            ("Vahn".to_string(), "0xBA:up".to_string()),
            ("Noa".to_string(), "0xBA".to_string())
        ],
        "reported in character order"
    );
    assert!(rep.costs_skipped_fit.is_empty(), "{rep:?}");
    assert_eq!(rep.owners_changed, 2);
    let ft = |character: &str, item: u8, slot: &'static str, costs: Vec<u8>| FallThrough {
        character: character.to_string(),
        item,
        slot,
        costs,
    };
    assert_eq!(
        rep.owners_without_section,
        vec![
            ft("Noa", ASTRAL, "weapon", vec![54]),
            ft("Gala", ASTRAL, "weapon", vec![0x1E]),
            ft("Vahn", STEEL_BOOTS, "footwear", vec![0x1E, 0x1E]),
            ft("Gala", STEEL_BOOTS, "footwear", vec![0x1E, 44]),
        ],
        "the fall-through warning names every character without a section and the cost they pay"
    );

    // Re-open the patched bytes cold and read everything back.
    let patched = patcher.image().to_vec();
    let re = DiscPatcher::open(patched.clone()).expect("reopen");
    assert_eq!(cost_of(&re, 0, ASTRAL), Some(30));
    assert_eq!(cost_of(&re, 1, SURVIVAL_CLUB), Some(42));
    assert_eq!(cost_of(&re, 2, NAIL_GLOVE), Some(30));
    assert_eq!(
        cost_of(&re, 0, NAIL_GLOVE),
        Some(0x2A),
        "untouched weapon keeps retail"
    );
    assert_eq!(owner_of(&re, ASTRAL), Some(7));
    assert_eq!(cost_of(&re, 0, META_9), Some(36));
    assert_eq!(
        record_cost_of(&re, 0, TRIUMPH_BOOTS, SwingRecord::Up),
        Some(40)
    );
    assert_eq!(
        record_cost_of(&re, 0, TRIUMPH_BOOTS, SwingRecord::Primary),
        Some(0x1E),
        "the Down record of the same boots is untouched"
    );
    let t = apply::read_equipment_table(&re).unwrap().unwrap();
    assert_eq!(
        t.defaults.map(|d| d.weapon),
        [Some(0x1E), Some(54), Some(0x1E)],
        "only Noa's weapon default was repriced"
    );
    assert_eq!(t.defaults.map(|d| d.ra_seru), [Some(0x1E); 3]);
    assert_eq!(
        t.defaults.map(|d| (d.down, d.up)),
        [
            (Some(0x1E), Some(0x1E)),
            (Some(0x1E), Some(0x1E)),
            (Some(0x1E), Some(44))
        ],
        "only Gala's barefoot Up kick was repriced"
    );
    let n = changed_sectors_valid(&orig, &patched);
    assert!(n > 0, "the patch changed something");

    // Idempotent: the same edit set on the patched image writes nothing.
    let mut again = DiscPatcher::open(patched.clone()).expect("reopen");
    let rep2 = apply::apply_equipment_edits(&mut again, &edits).expect("apply twice");
    assert_eq!(rep2.costs_changed, 0);
    assert_eq!(rep2.costs_unchanged, 7);
    assert_eq!(rep2.owners_changed, 0);
    assert_eq!(again.image(), &patched[..]);
}

#[test]
fn equipment_table_lists_noa_weapons_under_her_own_section() {
    let Some(orig) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(orig).expect("open disc");
    let table = apply::read_equipment_table(&patcher)
        .expect("read")
        .expect("table present");
    assert_eq!(
        table.defaults,
        [apply::SectionDefaults {
            weapon: Some(0x1E),
            ra_seru: Some(0x1E),
            down: Some(0x1E),
            up: Some(0x1E)
        }; 3],
        "every section default record swings at the favored tier"
    );
    assert_eq!(
        table.weapon_hand,
        [Some("Left"), Some("Right"), Some("Left")],
        "Noa's weapons price the Right command"
    );
    let rows = &table.rows;
    let glove = rows
        .iter()
        .find(|r| r.id == NAIL_GLOVE)
        .expect("Nail Glove row");
    assert_eq!(glove.slot, "weapon");
    assert_eq!(glove.costs, [Some(0x2A), Some(0x1E), Some(0x2A)]);
    assert_eq!(glove.cmds, [Some("Left"), Some("Right"), Some("Left")]);
    assert!(!glove.ra_seru_arm);
    let astral = rows.iter().find(|r| r.id == ASTRAL).expect("Astral row");
    assert_eq!(astral.costs, [Some(0x36), None, None]);
    let meta = rows.iter().find(|r| r.id == META_9).expect("Meta $9 row");
    assert!(
        meta.ra_seru_arm,
        "a Ra-Seru level sits in the Ra-Seru section"
    );
    assert_eq!(meta.cmds, [Some("Right"), None, None]);
    assert_eq!(meta.costs, [Some(0x1E), None, None]);
    let boots = rows
        .iter()
        .find(|r| r.id == TRIUMPH_BOOTS)
        .expect("boots row");
    assert_eq!(boots.slot, "footwear");
    assert_eq!(boots.cmds, [Some("Down"), None, None]);
    assert_eq!(boots.costs, [Some(0x1E), None, None]);
    assert_eq!(boots.up_costs, [Some(0x1E), None, None]);
    assert!(
        rows.iter().any(|r| r.slot == "body"),
        "non-weapons are listed too"
    );
    assert!(
        rows.iter()
            .filter(|r| r.slot == "body" || r.slot == "head")
            .all(|r| r.costs == [None; 3] && r.up_costs == [None; 3]),
        "body and head carry no command cost"
    );
    assert!(
        rows.iter()
            .filter(|r| r.slot != "footwear")
            .all(|r| r.up_costs == [None; 3]),
        "only footwear carries an Up record"
    );
}

/// Descriptor index of `item` in `ci`'s file, in a held section.
fn weapon_record_section(patcher: &DiscPatcher, ci: usize, item: u8) -> Option<usize> {
    let buf = patcher.read_player_file(PLAYERS[ci].entry).ok()?;
    let pack = battle_data_pack::detect(&buf)?;
    legaia_asset::equip_transplant::find_weapon_record(&pack, item as u32).map(|(_, s)| s)
}

/// Every record of `ci`'s file, decoded, keyed by (section, id).
fn decoded_records(patcher: &DiscPatcher, ci: usize) -> Vec<((usize, u32), Vec<u8>)> {
    let buf = patcher.read_player_file(PLAYERS[ci].entry).unwrap();
    let pack = battle_data_pack::detect(&buf).unwrap();
    let secs = legaia_asset::equip_transplant::record_sections(&pack);
    pack.records
        .iter()
        .zip(&secs)
        .map(|(r, s)| {
            (
                (*s, r.id),
                battle_data_pack::decode_record(&buf, &pack, r.index)
                    .unwrap()
                    .bytes,
            )
        })
        .collect()
}

#[test]
fn astral_sword_model_carries_over_to_noa_by_moving_a_boundary() {
    let Some(orig) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(orig.clone()).expect("open disc");
    let span_before: u64 = PLAYERS
        .iter()
        .map(|p| patcher.entry_footprint(p.entry).unwrap())
        .sum();
    let noa_before = decoded_records(&patcher, 1);
    let gala_before = decoded_records(&patcher, 2);
    let vahn_bytes = patcher.read_entry(PLAYERS[0].entry).unwrap();

    let edits = EquipmentEdits {
        // Noa's Astral Sword cost lands on the transplanted record.
        costs: vec![item(1, ASTRAL, 36)],
        owners: vec![EquipOwnerEdit {
            item_id: ASTRAL,
            mask: 0b011, // Vahn + Noa
        }],
        ..Default::default()
    };
    let rep = apply::apply_equipment_edits(&mut patcher, &edits).expect("apply");
    assert_eq!(
        rep.models_transplanted,
        vec![apply::ModelTransplant {
            character: "Noa".to_string(),
            item: ASTRAL,
            source: "Vahn".to_string(),
            cost: 36,
        }],
        "the note quotes the folded cost, not the donor's 54"
    );
    assert!(
        rep.models_no_room.is_empty() && rep.models_failed.is_empty(),
        "{rep:?}"
    );
    assert!(rep.owners_without_section.is_empty(), "{rep:?}");
    assert_eq!(rep.relayout_sectors, 0);
    assert_eq!(rep.costs_changed, 1, "{rep:?}");
    assert!(rep.costs_no_section.is_empty(), "{rep:?}");
    let delta: i64 = rep.entries_reassigned.iter().map(|(_, d)| d).sum();
    assert_eq!(
        delta, 0,
        "the run keeps its footprint: {:?}",
        rep.entries_reassigned
    );
    assert!(
        rep.entries_reassigned
            .iter()
            .any(|(e, d)| *e == PLAYERS[1].entry && *d > 0),
        "Noa's file grew: {:?}",
        rep.entries_reassigned
    );

    // Cold re-open: the TOC moved, every file still parses, nothing but the
    // new record changed in decoded terms, the donor is untouched.
    let patched = patcher.image().to_vec();
    assert_eq!(patched.len(), orig.len(), "no relayout");
    let re = DiscPatcher::open(patched.clone()).expect("reopen");
    let span_after: u64 = PLAYERS
        .iter()
        .map(|p| re.entry_footprint(p.entry).unwrap())
        .sum();
    assert_eq!(span_before, span_after);
    assert!(
        re.read_entry(PLAYERS[0].entry).unwrap() == vahn_bytes,
        "Vahn's file is untouched"
    );
    assert_eq!(
        weapon_record_section(&re, 1, ASTRAL),
        Some(3),
        "Noa's sword sits in her weapon section"
    );
    assert_eq!(
        cost_of(&re, 1, ASTRAL),
        Some(36),
        "the folded cost edit landed"
    );
    assert_eq!(cost_of(&re, 0, ASTRAL), Some(0x36));
    assert_eq!(owner_of(&re, ASTRAL), Some(0b011));
    let noa_after = decoded_records(&re, 1);
    assert_eq!(noa_after.len(), noa_before.len() + 1);
    let mut rest: Vec<_> = noa_after
        .iter()
        .filter(|(k, _)| *k != (3, ASTRAL as u32))
        .cloned()
        .collect();
    rest.sort_by_key(|(k, _)| *k);
    let mut before = noa_before.clone();
    before.sort_by_key(|(k, _)| *k);
    assert!(
        rest == before,
        "every other Noa record decodes byte-identical"
    );
    let mut gala_after = decoded_records(&re, 2);
    gala_after.sort_by_key(|(k, _)| *k);
    let mut gala_b = gala_before.clone();
    gala_b.sort_by_key(|(k, _)| *k);
    assert!(
        gala_after == gala_b,
        "Gala's re-packed file decodes byte-identical"
    );
    let n = changed_sectors_valid(&orig, &patched);
    assert!(n > 0);
    // The assembled battle mesh carries the sword.
    let buf = re.read_entry(PLAYERS[1].entry).unwrap();
    let pack = battle_data_pack::detect(&buf).unwrap();
    let armed =
        legaia_asset::battle_char_assembly::assemble_character(&buf, &pack, &[0, 0, 0, ASTRAL, 0])
            .unwrap();
    assert_eq!(armed.sections[3].id, ASTRAL as u32);
    let t = apply::read_equipment_table(&re).unwrap().unwrap();
    let astral = t.rows.iter().find(|r| r.id == ASTRAL).unwrap();
    assert_eq!(
        astral.costs,
        [Some(0x36), Some(36), None],
        "the table now prices Noa's sword"
    );

    // Idempotent: a second pass finds the record and moves nothing.
    let mut again = DiscPatcher::open(patched.clone()).expect("reopen");
    let rep2 = apply::apply_equipment_edits(&mut again, &edits).expect("apply twice");
    assert!(rep2.models_transplanted.is_empty(), "{rep2:?}");
    assert!(rep2.entries_reassigned.is_empty(), "{rep2:?}");
    assert_eq!(rep2.costs_unchanged, 1);
    assert!(again.image() == &patched[..]);
}

#[test]
fn every_owner_lands_in_the_dmy_annex_without_a_relayout() {
    let Some(orig) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Two sword records do not fit the three files' pool; both go to the
    // DMY.DAT annex, header in place, and the image keeps its size.
    let mut patcher = DiscPatcher::open(orig.clone()).expect("open disc");
    let free_before = patcher.annex_free_sectors().expect("annex room");
    let noa_head_before = patcher.read_entry_footprint(864).unwrap()[..0x8000].to_vec();
    let edits = EquipmentEdits {
        costs: vec![item(2, ASTRAL, 42)],
        owners: vec![EquipOwnerEdit {
            item_id: ASTRAL,
            mask: 7,
        }],
        ..Default::default()
    };
    let rep = apply::apply_equipment_edits(&mut patcher, &edits).expect("apply");
    assert_eq!(
        rep.models_transplanted
            .iter()
            .map(|t| t.character.as_str())
            .collect::<Vec<_>>(),
        ["Noa", "Gala"],
        "{rep:?}"
    );
    assert!(rep.models_no_room.is_empty(), "{rep:?}");
    assert_eq!(rep.relayout_sectors, 0);
    assert!(
        rep.entries_reassigned.is_empty(),
        "no boundary moved: {rep:?}"
    );
    assert_eq!(
        rep.models_annexed
            .iter()
            .map(|(c, _, _)| c.as_str())
            .collect::<Vec<_>>(),
        ["Noa", "Gala"],
        "{rep:?}"
    );
    let (dmy_lba, dmy_size) =
        legaia_iso::iso9660::find_file_in_image(&orig, "DMY.DAT").expect("DMY.DAT");
    let dmy_end = dmy_lba + dmy_size.div_ceil(2048);
    for (who, lba, sectors) in &rep.models_annexed {
        assert!(
            *lba > dmy_lba && lba + sectors < dmy_end,
            "{who}'s annex [{lba}, +{sectors}) inside DMY.DAT [{dmy_lba}, {dmy_end})"
        );
    }
    let used: u32 = rep.models_annexed.iter().map(|(_, _, s)| s).sum();
    assert_eq!(
        patcher.annex_free_sectors().unwrap(),
        free_before - used,
        "the marker accounts for every annexed sector"
    );
    assert_eq!(owner_of(&patcher, ASTRAL), Some(7));

    let patched = patcher.image().to_vec();
    assert_eq!(patched.len(), orig.len(), "same-size image (still a PPF)");
    assert!(changed_sectors_valid(&orig, &patched) > 0);

    // The entries did not move and the in-place header changed only in the
    // descriptor table (the annexed offsets); everything else is retail.
    let re = DiscPatcher::open(patched.clone()).expect("reopen");
    let noa_head = re.read_entry_footprint(864).unwrap()[..0x8000].to_vec();
    let table = legaia_asset::player_file_annex::chain(&noa_head).expect("annexed chain");
    assert!(table.is_annexed());
    assert_eq!(
        noa_head[..table.table_offset],
        noa_head_before[..table.table_offset]
    );
    assert!(re.player_file_annex(864).unwrap().is_some());
    assert!(re.player_file_annex(865).unwrap().is_some());
    assert!(
        re.player_file_annex(863).unwrap().is_none(),
        "Vahn's file is untouched"
    );

    // Read back through the annex-aware reader: the records are there.
    assert_eq!(weapon_record_section(&re, 1, ASTRAL), Some(3));
    assert_eq!(weapon_record_section(&re, 2, ASTRAL), Some(2));
    assert_eq!(cost_of(&re, 1, ASTRAL), Some(0x36), "donor's price");
    assert_eq!(cost_of(&re, 2, ASTRAL), Some(42), "the cost edit folded in");
    // Every retail record survived the rebuild byte-for-byte.
    let noa_orig = decoded_records(&DiscPatcher::open(orig.clone()).unwrap(), 1);
    let noa_now = decoded_records(&re, 1);
    for (key, bytes) in &noa_orig {
        let now = noa_now.iter().find(|(k, _)| k.1 == key.1 && k.0 == key.0);
        assert!(
            now.is_some_and(|(_, b)| b == bytes),
            "Noa record {key:?} survived"
        );
    }
    // The listing reads the patched disc the same way.
    let t = apply::read_equipment_table(&re).unwrap().unwrap();
    let astral = t.rows.iter().find(|r| r.id == ASTRAL).unwrap();
    assert_eq!(astral.costs, [Some(0x36), Some(0x36), Some(42)]);

    // A second pass over the annexed disc keeps working: a cost edit on
    // the annexed record routes to the annex, and a further transplant
    // allocates past the first.
    let mut again = DiscPatcher::open(patched.clone()).expect("reopen");
    let edits2 = EquipmentEdits {
        costs: vec![item(1, ASTRAL, 30)],
        owners: vec![EquipOwnerEdit {
            item_id: 0x33, // Great Axe, Vahn's
            mask: 7,
        }],
        ..Default::default()
    };
    let rep2 = apply::apply_equipment_edits(&mut again, &edits2).expect("apply again");
    assert_eq!(cost_of(&again, 1, ASTRAL), Some(30), "{rep2:?}");
    assert_eq!(weapon_record_section(&again, 1, 0x33), Some(3), "{rep2:?}");
    assert_eq!(weapon_record_section(&again, 2, 0x33), Some(2), "{rep2:?}");
    assert_eq!(
        weapon_record_section(&again, 1, ASTRAL),
        Some(3),
        "first pass kept"
    );
    assert_eq!(again.image().len(), orig.len());

    // Deterministic.
    let mut p2 = DiscPatcher::open(orig.clone()).expect("open disc");
    apply::apply_equipment_edits(&mut p2, &edits).expect("apply");
    assert!(p2.image() == &patched[..], "byte-deterministic");
}

#[test]
fn relayout_is_the_fallback_when_the_annex_is_full() {
    let Some(orig) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Fill the annex first; without a relayout the models then stay out.
    let mut patcher = DiscPatcher::open(orig.clone()).expect("open disc");
    let free = patcher.annex_free_sectors().unwrap();
    patcher.annex_alloc(free).expect("fill the annex");
    assert_eq!(patcher.annex_free_sectors().unwrap(), 0);
    let filled = patcher.image().to_vec();
    let edits = EquipmentEdits {
        owners: vec![EquipOwnerEdit {
            item_id: ASTRAL,
            mask: 7,
        }],
        ..Default::default()
    };
    let rep = apply::apply_equipment_edits(&mut patcher, &edits).expect("apply");
    assert!(rep.models_transplanted.is_empty(), "{rep:?}");
    assert!(rep.models_annexed.is_empty(), "{rep:?}");
    assert_eq!(
        rep.models_no_room
            .iter()
            .map(|f| f.character.as_str())
            .collect::<Vec<_>>(),
        ["Noa", "Gala"],
        "{rep:?}"
    );
    assert!(rep.entries_reassigned.is_empty());
    assert_eq!(owner_of(&patcher, ASTRAL), Some(7), "the mask still moved");

    // With one, both files grow and the disc gets longer.
    let mut patcher = DiscPatcher::open(filled.clone()).expect("open disc");
    let terra_before = patcher.read_entry(866).unwrap();
    let archive_head = patcher.read_entry(867).unwrap()[..0x4000].to_vec();
    let edits = EquipmentEdits {
        allow_relayout: true,
        ..edits
    };
    let rep = apply::apply_equipment_edits(&mut patcher, &edits).expect("apply");
    assert_eq!(
        rep.models_transplanted
            .iter()
            .map(|t| t.character.as_str())
            .collect::<Vec<_>>(),
        ["Noa", "Gala"],
        "{rep:?}"
    );
    assert!(rep.relayout_sectors > 0);
    assert!(rep.entries_reassigned.is_empty());
    let patched = patcher.image().to_vec();
    assert_eq!(
        patched.len(),
        filled.len() + rep.relayout_sectors as usize * SECTOR_SIZE,
        "the image grew by exactly the relayout"
    );
    let re = DiscPatcher::open(patched).expect("reopen the grown image");
    assert_eq!(weapon_record_section(&re, 1, ASTRAL), Some(3));
    assert_eq!(weapon_record_section(&re, 2, ASTRAL), Some(2));
    assert_eq!(cost_of(&re, 1, ASTRAL), Some(0x36));
    assert_eq!(cost_of(&re, 2, ASTRAL), Some(0x36));
    assert!(
        re.read_entry(866).unwrap() == terra_before,
        "Terra's file rides the shift intact"
    );
    assert!(
        re.read_entry(867).unwrap()[..0x4000] == archive_head[..],
        "so does the monster archive"
    );
}
