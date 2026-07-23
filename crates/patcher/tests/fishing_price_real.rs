//! Disc-gated tests for the fishing point-exchange price editor
//! (`legaia_patcher::fishing_price` + `apply::set_fishing_price`): the Buma
//! Water Egg row is where the parser says (PROT 972, offset 0x9874, 20000
//! points), a price edit lands as a same-size u32, exactly the targeted
//! `price` fields change, the patched image re-parses, and re-applying is a
//! no-op. Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched
//! image lives only in memory.

use legaia_asset::fishing_exchange;
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::fishing_price::{self, OVERLAY_PROT_INDEX, plan_set_price, price_field_offset};

/// Water Egg item id (SCUS item-name id space).
const WATER_EGG: u32 = 0x6F;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn read_u32(entry: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(entry[off..off + 4].try_into().unwrap())
}

#[test]
fn baseline_buma_water_egg_row_matches_the_parser() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let overlay = patcher
        .read_entry(OVERLAY_PROT_INDEX)
        .expect("read PROT 972");
    let ex = fishing_exchange::parse(&overlay).expect("parse exchange");

    // Buma (page 0) row 0 is the one-time Water Egg at 20000 points.
    let buma = &ex.venues[0];
    let water = buma
        .iter()
        .find(|r| r.item_id == WATER_EGG)
        .expect("Buma sells Water Egg");
    assert!(water.is_one_time(), "Water Egg is a one-time prize");
    assert_eq!(water.price, 20000, "Water Egg costs 20000 points");
    // The parser's row and our offset math agree with the raw bytes.
    let off = price_field_offset(0, water.row);
    assert_eq!(off, 0x9874, "Water Egg price field at 0x9874");
    assert_eq!(read_u32(&overlay, off), 20000);

    // The planner finds it and would reprice it.
    let edits = plan_set_price(&overlay, WATER_EGG, 500).expect("plannable");
    assert!(
        edits
            .iter()
            .any(|e| e.offset == 0x9874 && e.old_price == 20000)
    );
}

#[test]
fn set_price_is_surgical_and_reparses() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    let before = patcher.read_entry(OVERLAY_PROT_INDEX).unwrap();

    let report = apply::set_fishing_price(&mut patcher, WATER_EGG, 1000).expect("apply");
    assert!(
        report
            .edits
            .iter()
            .any(|&(_, _, id, old, new)| id == WATER_EGG && old == 20000 && new == 1000),
        "Water Egg repriced 20000 -> 1000"
    );

    let after = patcher.read_entry(OVERLAY_PROT_INDEX).unwrap();
    // Only the planned `price` fields changed.
    let planned: std::collections::BTreeSet<usize> = plan_set_price(&before, WATER_EGG, 1000)
        .unwrap()
        .iter()
        .map(|e| e.offset)
        .collect();
    let changed: std::collections::BTreeSet<usize> = before
        .iter()
        .zip(after.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i & !3)
        .collect();
    assert_eq!(changed, planned, "only the targeted price words changed");

    // The patched overlay re-parses and reports the new price.
    let ex = fishing_exchange::parse(&after).expect("re-parse");
    let water = ex.venues[0]
        .iter()
        .find(|r| r.item_id == WATER_EGG)
        .unwrap();
    assert_eq!(water.price, 1000);
}

#[test]
fn reapply_is_a_noop_and_absent_item_refused() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");
    apply::set_fishing_price(&mut patcher, WATER_EGG, 750).unwrap();
    // Re-setting to the same value writes nothing.
    let again = apply::set_fishing_price(&mut patcher, WATER_EGG, 750).unwrap();
    assert!(
        again.edits.is_empty(),
        "re-applying the same price is a no-op"
    );

    // An item no fishing prize grants is refused, not silently ignored.
    let overlay = patcher.read_entry(OVERLAY_PROT_INDEX).unwrap();
    assert!(fishing_price::plan_set_price(&overlay, 0x02, 1).is_err());
}
