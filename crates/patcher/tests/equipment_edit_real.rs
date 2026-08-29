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
    let buf = patcher.read_entry(PLAYERS[ci].entry).ok()?;
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
