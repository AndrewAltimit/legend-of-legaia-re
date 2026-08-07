//! Card-inventory ladder: run the retail item-window **accessor family**
//! (`FUN_8004313C` / `FUN_80042F4C` / `FUN_80042310` / `FUN_80043048` /
//! `FUN_800423E0` / `FUN_800421D4`) over a **real** memory card's SC block.
//!
//! The family's preservation host is `save-tool items`, which asks the same
//! questions of the same data; this ladder is the `#[test]`-shaped copy of
//! that session so the replay-coverage union can see it run. The model is
//! read-only over the card - every mutation happens to the in-memory
//! [`RetailInventory`], never to the `.mcr` - which is exactly the read-only
//! stance the `normalize` / `add` tags disclose.
//!
//! Keys on `~/.mednafen/sav` like `real_card_roundtrip` (a memory card is
//! not disc data, so no `LEGAIA_DISC_BIN` gate); skips and passes when no
//! usable card exists.

use std::path::PathBuf;

use legaia_save::retail_inventory::{
    AddOutcome, ITEM_WINDOW_BASE, ItemWindow, RetailInventory, STACK_CAP, oob_target,
};

/// Same card-discovery rule as `real_card_roundtrip`: first sorted candidate
/// that actually holds an active save block.
fn locate_card() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home).join(".mednafen/sav");
    if !dir.exists() {
        return None;
    }
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| {
                    let name = n.to_string_lossy();
                    name.contains("Legaia") && name.ends_with(".0.mcr")
                })
                .unwrap_or(false)
        })
        .collect();
    candidates.sort();
    candidates.into_iter().find(|p| {
        std::fs::read(p)
            .ok()
            .and_then(|b| legaia_save::parse_card(&b).ok())
            .is_some_and(|saves| !saves.is_empty())
    })
}

/// Lift the first active save's item window: the selector's real inputs
/// (member count at `SC+0x454`, half byte at `SC+0x458`) plus the raw slot
/// array at `SC+0x1818`.
fn real_window() -> Option<(ItemWindow, RetailInventory)> {
    let card_path = locate_card()?;
    let bytes = std::fs::read(&card_path).ok()?;
    let saves = legaia_save::parse_card(&bytes).ok()?;
    let sc = legaia_save::read_block(&bytes, saves.first()?.block)?;
    // The full 256-slot accessor window (`SC+0x1818..+0x1A18`), not the
    // 72-slot general page - the distinction the ACE analysis re-opened.
    let raw = legaia_save::card::read_retail_item_window(sc)?;

    let members = sc.get(0x454).copied().unwrap_or(0);
    let high_half = sc.get(0x458).copied().unwrap_or(0) != 0;
    // Story flag 20 gates only the solo-member arm; a saved game has a party,
    // so the arm the real inputs take is the `>= 2` row. Both polarities are
    // fed so the selector's flag test runs rather than being sliced away.
    let window = ItemWindow::select(members, false, high_half)
        .or_else(|| ItemWindow::select(members, true, high_half))?;

    let (lo, hi) = window.bounds();
    let slots: Vec<(u8, u8)> = raw
        .chunks_exact(2)
        .skip(lo)
        .take(hi - lo)
        .map(|c| (c[0], c[1]))
        .collect();
    let base = ITEM_WINDOW_BASE + (lo as u32) * 2;
    eprintln!(
        "[w2c-card] {} members={} window={window:?} occupied={}",
        card_path.display(),
        members,
        slots.iter().filter(|(id, _)| *id != 0).count()
    );
    Some((window, RetailInventory::from_slots(base, slots)))
}

/// The whole accessor family, over the real bag, asserted at the model's own
/// laws: find agrees with the slots, consume leaves a hole, consume-by-slot
/// echoes its no-ops, normalize merges-then-squeezes without losing an item,
/// and add merges before it places.
#[test]
fn the_retail_accessor_family_runs_over_a_real_bag() {
    let Some((window, mut inv)) = real_window() else {
        eprintln!("[skip] no usable Legaia memory-card image at ~/.mednafen/sav/");
        return;
    };

    // The selector's own outputs, pinned against the model.
    assert_eq!(
        oob_target(ITEM_WINDOW_BASE, window.bounds().1),
        inv.oob_target(),
        "the window's one-past-the-end OOB target"
    );

    // A saved game holds items; a bag with none would make every arm below
    // vacuous, so say so instead of passing silently.
    let (held_id, held_count) = *inv
        .slots()
        .iter()
        .find(|(id, count)| *id != 0 && *count > 0)
        .expect("a real save's bag holds at least one item");

    // find-count-by-id (FUN_80042F4C) reads the same byte the slot holds.
    assert_eq!(inv.find_count(held_id), held_count);
    assert_eq!(
        inv.find_count(0),
        0,
        "id 0 is the empty sentinel, never found"
    );

    // consume-by-id (FUN_80042310): drain the stack; the freed slot is a
    // HOLE (id zeroed in place), not a compaction.
    let slot = inv.find_slot(held_id).expect("the held id has a slot");
    assert!(inv.consume(held_id, held_count));
    assert_eq!(
        inv.slots()[slot],
        (0, 0),
        "a drained slot is a hole in place"
    );
    assert_eq!(inv.find_count(held_id), 0);

    // consume-by-slot (FUN_80043048): the no-op arms echo the third argument.
    assert_eq!(
        inv.consume_slot(slot as i16, 1, 0xEE),
        0xEE,
        "consuming from the hole echoes"
    );
    assert_eq!(
        inv.consume_slot(window.len() as i16, 1, 0xEE),
        0xEE,
        "an out-of-window index echoes"
    );
    if let Some(other) = inv
        .slots()
        .iter()
        .position(|(id, count)| *id != 0 && *count > 1)
    {
        let before = inv.slots()[other].1;
        let left = inv.consume_slot(other as i16, 1, 0xEE);
        assert_eq!(left, before - 1, "the occupied arm decrements in place");
    }

    // normalize (FUN_800423E0): merge duplicate stacks then squeeze holes.
    // The law asserted is conservation - the per-id totals survive (capped),
    // holes disappear, and occupancy packs to the front.
    let mut totals_before = std::collections::BTreeMap::<u8, u32>::new();
    for &(id, count) in inv.slots() {
        if id != 0 {
            *totals_before.entry(id).or_default() += u32::from(count);
        }
    }
    inv.normalize();
    let occupied = inv.slots().iter().take_while(|(id, _)| *id != 0).count();
    assert!(
        inv.slots()[occupied..].iter().all(|&(id, _)| id == 0),
        "normalize must pack every live id to the front"
    );
    let mut totals_after = std::collections::BTreeMap::<u8, u32>::new();
    for &(id, count) in inv.slots() {
        if id != 0 {
            *totals_after.entry(id).or_default() += u32::from(count);
        }
    }
    for (id, before) in &totals_before {
        let after = totals_after.get(id).copied().unwrap_or(0);
        assert_eq!(
            after,
            (*before).min(u32::from(STACK_CAP)),
            "normalize lost or invented item {id:#04x}"
        );
    }

    // add (FUN_800421D4): MERGE pass first - re-granting the consumed id
    // lands on its (normalized) stack or the first hole, never past the cap.
    let outcome = inv.add(held_id, 2);
    match outcome {
        AddOutcome::Merged { slot, new_count } => {
            assert_eq!(inv.slots()[slot].0, held_id);
            assert!(new_count <= STACK_CAP);
        }
        AddOutcome::Placed { slot } => {
            assert_eq!(inv.slots()[slot], (held_id, 2));
        }
        AddOutcome::OobIdWrite { .. } => {
            panic!("a bag with holes must never take the full-bag OOB exit")
        }
    }
    assert!(inv.find_count(held_id) >= 2);
}

/// The full-bag OOB add primitive, surfaced as data on a **synthetically
/// filled copy** of the real window: the id byte would land one slot past the
/// window, and the model performs no write.
///
/// The synthetic fill is the point, not a shortcut: `oob_reachability`
/// records that no normal-play path fills a retail window, so the only honest
/// way to execute the exit is to construct the state it needs.
#[test]
fn a_full_window_surfaces_the_oob_id_store_without_writing() {
    let Some((_, inv)) = real_window() else {
        eprintln!("[skip] no usable Legaia memory-card image at ~/.mednafen/sav/");
        return;
    };

    // Fill every slot with a live id (duplicates are fine - what matters is
    // that no slot is `id == 0`); keep the real base so the would-be OOB
    // address is the retail one.
    let n = inv.window_slots();
    let full: Vec<(u8, u8)> = (0..n).map(|i| (((i % 200) + 1) as u8, 1)).collect();
    let mut full_inv = RetailInventory::from_slots(inv.base(), full);

    // An id not present anywhere: the merge pass misses, the free-slot scan
    // exhausts the window, and the add reports the OOB store as data.
    let probe_id = 0xFA;
    assert_eq!(full_inv.find_slot(probe_id), None);
    let before = full_inv.slots().to_vec();
    match full_inv.add(probe_id, 1) {
        AddOutcome::OobIdWrite {
            oob_target: target,
            written_id,
        } => {
            assert_eq!(target, inv.oob_target(), "the retail landing address");
            assert_eq!(written_id, probe_id);
        }
        other => panic!("a full window must take the OOB exit, got {other:?}"),
    }
    assert_eq!(
        full_inv.slots(),
        &before[..],
        "the model surfaces the primitive without performing the write"
    );
}
