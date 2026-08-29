//! Disc-gated round-trip for the manual equipment editor: set the Astral
//! Sword's swing cost to the favored tier, price one weapon differently for
//! two characters, and hand the Astral Sword to everyone; re-decode off the
//! patched image and confirm:
//!
//! - every requested swing cost reads back from the LZS-recompressed section,
//!   and an untouched weapon keeps its retail value;
//! - Noa's weapon sections are found (they key under section 3, not 2);
//! - the owner mask reads back and the report names the two characters whose
//!   files have no Astral Sword section (the fall-through case);
//! - every changed disc sector stays EDC/ECC-valid, and applying the same
//!   edit set twice is a no-op (idempotent).
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::battle_data_pack;
use legaia_iso::raw::SECTOR_SIZE;
use legaia_patcher::apply::{self, EquipOwnerEdit, EquipmentEdits, SwingCostEdit};
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::weapon_specialty::{PLAYERS, arm_cost_offset};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Swing cost of `item` in character `ci`'s file, or `None` without a section.
fn cost_of(patcher: &DiscPatcher, ci: usize, item: u8) -> Option<u8> {
    let buf = patcher.read_entry(PLAYERS[ci].entry).ok()?;
    let pack = battle_data_pack::detect(&buf)?;
    let (idx, _) = pack
        .records
        .iter()
        .enumerate()
        .find(|(_, r)| r.id == item as u32)?;
    let dec = battle_data_pack::decode_record(&buf, &pack, idx).ok()?;
    let off = arm_cost_offset(&dec.bytes)?;
    Some(dec.bytes[off])
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

    let edits = EquipmentEdits {
        costs: vec![
            SwingCostEdit {
                character: 0,
                item_id: ASTRAL,
                cost: 30,
            },
            SwingCostEdit {
                character: 1,
                item_id: SURVIVAL_CLUB,
                cost: 42,
            },
            SwingCostEdit {
                character: 2,
                item_id: NAIL_GLOVE,
                cost: 30,
            },
            SwingCostEdit {
                character: 1,
                item_id: ASTRAL,
                cost: 30,
            }, // no section
            SwingCostEdit {
                character: 1,
                item_id: apply::DEFAULT_WEAPON,
                cost: 54,
            }, // Noa's fall-through record
        ],
        owners: vec![EquipOwnerEdit {
            item_id: ASTRAL,
            mask: 7,
        }],
    };
    let rep = apply::apply_equipment_edits(&mut patcher, &edits).expect("apply");
    assert_eq!(rep.costs_changed, 4, "{rep:?}");
    assert_eq!(rep.costs_no_section, vec![("Noa".to_string(), ASTRAL)]);
    assert!(rep.costs_skipped_fit.is_empty(), "{rep:?}");
    assert_eq!(rep.owners_changed, 1);
    assert_eq!(
        rep.owners_without_section,
        vec![
            ("Noa".to_string(), ASTRAL, 54),
            ("Gala".to_string(), ASTRAL, 0x1E)
        ],
        "the fall-through warning names both characters without a section and the cost they pay"
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
    assert_eq!(
        apply::read_equipment_table(&re)
            .unwrap()
            .unwrap()
            .default_costs,
        [Some(0x1E), Some(54), Some(0x1E)],
        "only Noa's default record was repriced"
    );
    let n = changed_sectors_valid(&orig, &patched);
    assert!(n > 0, "the patch changed something");

    // Idempotent: the same edit set on the patched image writes nothing.
    let mut again = DiscPatcher::open(patched.clone()).expect("reopen");
    let rep2 = apply::apply_equipment_edits(&mut again, &edits).expect("apply twice");
    assert_eq!(rep2.costs_changed, 0);
    assert_eq!(rep2.costs_unchanged, 4);
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
        table.default_costs,
        [Some(0x1E); 3],
        "every default weapon record swings at the favored tier"
    );
    let rows = &table.rows;
    let glove = rows
        .iter()
        .find(|r| r.id == NAIL_GLOVE)
        .expect("Nail Glove row");
    assert_eq!(glove.slot, "weapon");
    assert_eq!(glove.costs, [Some(0x2A), Some(0x1E), Some(0x2A)]);
    let astral = rows.iter().find(|r| r.id == ASTRAL).expect("Astral row");
    assert_eq!(astral.costs, [Some(0x36), None, None]);
    assert!(
        rows.iter().any(|r| r.slot == "body"),
        "non-weapons are listed too"
    );
    assert!(
        rows.iter()
            .filter(|r| r.slot != "weapon")
            .all(|r| r.costs == [None; 3]),
        "only weapons carry a swing cost"
    );
}
