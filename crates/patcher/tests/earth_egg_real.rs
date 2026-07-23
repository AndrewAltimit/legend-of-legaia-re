//! Disc-gated oracle for the Earth Egg coin-threshold editor
//! (`legaia_patcher::earth_egg` + `apply::set_earth_egg_price`).
//!
//! Asserts against a real disc that:
//! - the exchange is located in the `koin1` scene bundle (retail PROT entry
//!   543), with the retail shape: coins-required 100000, gate threshold 99999,
//!   debit 100000, prize item 0x6E, and a `GIVE_ITEM 0x39 0x6E` present;
//! - a price edit lands: the patched image re-decodes to the new coins-required,
//!   gate = price - 1, debit = price;
//! - the edit is **surgical in the decompressed MAN** - exactly the four
//!   threshold-half bytes and the three coin-debit bytes change, nothing else
//!   (the give, the flag latch, the skip-delta, and every neighbouring
//!   descriptor are untouched);
//! - the recompressed MAN fits the zero-slack footprint and the touched
//!   PROT.DAT sector stays EDC/ECC-valid;
//! - re-applying the same value is a no-op, and out-of-range prices are refused;
//! - the whole patch is byte-deterministic across two runs.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. The patched image lives
//! only in memory - no Sony bytes are committed.

use std::collections::BTreeSet;

use legaia_iso::iso9660::find_file_in_image;
use legaia_iso::raw::{SECTOR_SIZE, USER_DATA_OFFSET, USER_DATA_SIZE};
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::earth_egg::{
    self, EARTH_EGG_ITEM_ID, EarthEggExchange, MAX_PRICE, RETAIL_PRICE,
};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// Locate the exchange by scanning entries (returns the located view).
fn locate(patcher: &DiscPatcher) -> EarthEggExchange {
    for idx in 0..patcher.entry_count() {
        let entry = patcher.read_entry(idx).unwrap();
        if let Some(e) = EarthEggExchange::locate(&entry, idx) {
            return e;
        }
    }
    panic!("Earth Egg exchange not found on disc");
}

#[test]
fn baseline_matches_retail_shape() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let e = locate(&patcher);
    assert_eq!(e.entry_idx, 543, "koin1 scene bundle is PROT entry 543");
    assert_eq!(e.threshold, 99_999, "retail gate threshold");
    assert_eq!(e.debit, RETAIL_PRICE, "retail coin debit");
    assert_eq!(e.price(), RETAIL_PRICE, "retail coins required");
    // The Earth Egg give is present in the same MAN.
    assert!(
        e.decoded.windows(2).any(|w| w == [0x39, EARTH_EGG_ITEM_ID]),
        "GIVE_ITEM Earth Egg present"
    );
    // The read-only info view agrees.
    let info = e.info();
    assert_eq!(info.item_id, EARTH_EGG_ITEM_ID);
    assert_eq!(info.price, RETAIL_PRICE);
    assert_eq!(apply::current_earth_egg(&patcher).unwrap().unwrap(), info);
}

#[test]
fn price_edit_is_surgical_and_reparses() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");

    // Capture the pre-edit decoded MAN + the exact edit offsets.
    let before = locate(&patcher);
    let entry_idx = before.entry_idx;
    let man_before = before.decoded.clone();
    // The only bytes that may move: the threshold's two u16 halves (op+3/+4 and
    // op+7/+8, straddling the untouched skip-delta at op+5/+6) and the debit's
    // 3-byte u24 (op+2/+3/+4). A given (old,new) pair changes a subset of these
    // (a high byte can stay put), so the surgical invariant is "nothing outside
    // this candidate set moved".
    let candidate_offsets: BTreeSet<usize> = [
        before.compare_off + 3,
        before.compare_off + 4,
        before.compare_off + 7,
        before.compare_off + 8,
        before.debit_off + 2,
        before.debit_off + 3,
        before.debit_off + 4,
    ]
    .into_iter()
    .collect();

    const NEW: u32 = 250_000;
    let report = apply::set_earth_egg_price(&mut patcher, NEW).expect("apply");
    assert!(report.changed);
    assert_eq!(report.entry_idx, entry_idx);
    assert_eq!(report.old_price, RETAIL_PRICE);
    assert_eq!(report.new_price, NEW);

    // Re-decode the patched entry and check the new shape round-trips.
    let after_entry = patcher.read_entry(entry_idx).unwrap();
    let after = EarthEggExchange::locate(&after_entry, entry_idx).expect("re-locate");
    assert_eq!(after.price(), NEW);
    assert_eq!(after.threshold, NEW - 1, "gate = price - 1");
    assert_eq!(after.debit, NEW, "debit = price");
    let man_after = after.decoded.clone();

    // Surgical in the decompressed MAN: exactly the seven targeted bytes moved.
    assert_eq!(man_before.len(), man_after.len(), "MAN size unchanged");
    let changed: BTreeSet<usize> = man_before
        .iter()
        .zip(man_after.iter())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| i)
        .collect();
    assert!(
        !changed.is_empty() && changed.is_subset(&candidate_offsets),
        "only the threshold halves + debit u24 may change in the decoded MAN \
         (changed={changed:?}, candidates={candidate_offsets:?})"
    );
    // The low half of the threshold and the debit's low byte always move for a
    // distinct price, so the edit is provably non-vacuous.
    assert!(changed.contains(&(before.compare_off + 3)));
    assert!(changed.contains(&(before.debit_off + 2)));
    // The give, the skip-delta, and everything else are byte-identical.
    assert!(
        after
            .decoded
            .windows(2)
            .any(|w| w == [0x39, EARTH_EGG_ITEM_ID]),
        "Earth Egg give still present"
    );
    assert_eq!(
        &man_before[before.compare_off + 5..before.compare_off + 7],
        &man_after[after.compare_off + 5..after.compare_off + 7],
        "compare skip-delta preserved"
    );

    // Neighbouring descriptors in the same entry are untouched (zero-slack
    // footprint: an overflow would corrupt the next asset).
    let orig_entry = {
        let p = DiscPatcher::open(load_disc().unwrap()).unwrap();
        p.read_entry(entry_idx).unwrap()
    };
    let man_stream_start = before.man_offset;
    let man_stream_end = man_stream_start + before.compressed_budget;
    for i in 0..orig_entry.len().min(after_entry.len()) {
        if (man_stream_start..man_stream_end).contains(&i) {
            continue; // the MAN's own compressed region legitimately re-packs
        }
        assert_eq!(
            orig_entry[i], after_entry[i],
            "byte 0x{i:X} outside the MAN stream changed"
        );
    }

    // The patched scene's PROT.DAT sector stays EDC/ECC-valid.
    let img = patcher.image();
    let (prot_lba, prot_size) = find_file_in_image(img, "PROT.DAT").unwrap();
    let psectors = (prot_size as usize).div_ceil(USER_DATA_SIZE);
    let mut payload = Vec::with_capacity(psectors * USER_DATA_SIZE);
    for i in 0..psectors {
        let b = (prot_lba as usize + i) * SECTOR_SIZE + USER_DATA_OFFSET;
        payload.extend_from_slice(&img[b..b + USER_DATA_SIZE]);
    }
    payload.truncate(prot_size as usize);
    let archive = legaia_prot::archive::Archive::from_bytes(payload).unwrap();
    let lba = archive.entries[entry_idx].start_lba;
    let sb = (prot_lba as u64 + lba as u64) as usize * SECTOR_SIZE;
    assert!(
        legaia_iso::write::mode2_form1_sector_is_valid(&img[sb..sb + SECTOR_SIZE]),
        "patched Earth Egg scene sector must be EDC/ECC-valid"
    );
}

#[test]
fn reapply_is_noop_and_bounds_are_enforced() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open disc");

    let r1 = apply::set_earth_egg_price(&mut patcher, 5_000).expect("apply");
    assert!(r1.changed && r1.new_price == 5_000);
    // Re-setting to the same value rewrites nothing.
    let r2 = apply::set_earth_egg_price(&mut patcher, 5_000).expect("apply again");
    assert!(!r2.changed, "re-applying the same price is a no-op");
    assert_eq!(r2.new_price, 5_000);

    // Out-of-range prices are refused, not silently clamped.
    assert!(apply::set_earth_egg_price(&mut patcher, 0).is_err());
    assert!(apply::set_earth_egg_price(&mut patcher, MAX_PRICE + 1).is_err());

    // Plan-level refusal on a non-exchange entry (SCUS-style bytes, no bundle).
    assert!(earth_egg::plan_set_price(&[7, 0, 0, 0], 0, 100).is_err());
}

#[test]
fn patch_is_deterministic() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut a = DiscPatcher::open(disc.clone()).unwrap();
    let mut b = DiscPatcher::open(disc).unwrap();
    apply::set_earth_egg_price(&mut a, 333_333).unwrap();
    apply::set_earth_egg_price(&mut b, 333_333).unwrap();
    assert_eq!(
        a.image(),
        b.image(),
        "same input + value => identical image"
    );
}
