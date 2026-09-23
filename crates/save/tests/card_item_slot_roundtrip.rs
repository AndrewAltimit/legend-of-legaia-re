//! A real memory card's **item slot array** survives the lift, the engine
//! save format and the write-back, slot for slot.
//!
//! Slot order is not cosmetic: one retail consumer indexes the bag by slot
//! (PROT 0941's Steal is a rejection sampler over the physical array), so a
//! save path that compacts the array - or that stops at the 72-slot
//! consumable display page - changes what a steal can take and silently
//! drops every item a played-through bag holds above slot 71.
//!
//! Keys on `~/.mednafen/sav` like `real_card_roundtrip` (a memory card is not
//! disc data, so no `LEGAIA_DISC_BIN` gate); skips and passes when no usable
//! card exists.

use legaia_save::SaveFile;
use legaia_save::retail_inventory::ITEM_SLOTS_TOTAL;
use std::path::PathBuf;

/// Same card-discovery rule as `real_card_roundtrip`: the first sorted
/// candidate that holds an active save block.
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
            .and_then(|b| legaia_save::card::parse_card(&b).ok())
            .is_some_and(|blocks| !blocks.is_empty())
    })
}

/// An SC block's bytes beside its item region decoded to `(id, count)` pairs.
type BlockWithSlots = (Vec<u8>, Vec<(u8, u8)>);

/// The first SC block on the card, with its raw item region beside it.
fn card_block() -> Option<BlockWithSlots> {
    let path = locate_card()?;
    let bytes = std::fs::read(&path).ok()?;
    let blocks = legaia_save::card::parse_card(&bytes).ok()?;
    let sc = legaia_save::card::read_block(&bytes, blocks.first()?.block)?.to_vec();
    let raw = legaia_save::card::read_retail_item_window(&sc)?;
    let slots: Vec<(u8, u8)> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| (c[0], c[1]))
        .collect();
    eprintln!(
        "[card-slots] {} occupied {} of {ITEM_SLOTS_TOTAL}, highest occupied slot {}",
        path.display(),
        slots.iter().filter(|(id, _)| *id != 0).count(),
        slots
            .iter()
            .rposition(|(id, _)| *id != 0)
            .map_or(-1, |i| i as i32),
    );
    Some((sc, slots))
}

#[test]
fn the_lift_keeps_the_whole_array_not_the_display_page() {
    let Some((sc, raw_slots)) = card_block() else {
        eprintln!("[skip] no usable Legaia memory-card image at ~/.mednafen/sav/");
        return;
    };
    let sf = SaveFile::from_retail_sc_block(&sc, 4).expect("lift the SC block");
    assert_eq!(
        sf.ext.item_slots, raw_slots,
        "the lifted array is the block's bytes, slot for slot"
    );
    assert_eq!(sf.ext.item_slots.len(), ITEM_SLOTS_TOTAL);

    // Non-vacuity: a bag that fits inside the 72-slot display page would not
    // distinguish the widened lift from the old one.
    let highest = raw_slots
        .iter()
        .rposition(|(id, _)| *id != 0)
        .expect("the card's bag holds something");
    assert!(
        highest >= 72,
        "this card's bag stops at slot {highest}, inside the display page - it cannot \
         tell the widened lift from the 72-slot one"
    );

    // The compact view is the same multiset with the holes removed.
    let occupied: Vec<(u8, u8)> = raw_slots
        .iter()
        .copied()
        .filter(|&(id, count)| !(id == 0 && count == 0))
        .collect();
    assert_eq!(sf.ext.inventory, occupied);
}

#[test]
fn the_engine_save_format_round_trips_the_slot_array() {
    let Some((sc, raw_slots)) = card_block() else {
        eprintln!("[skip] no usable Legaia memory-card image at ~/.mednafen/sav/");
        return;
    };
    let sf = SaveFile::from_retail_sc_block(&sc, 4).expect("lift the SC block");
    let bytes = sf.write();
    let back = SaveFile::parse(&bytes).expect("parse the LGSF file back");
    assert_eq!(
        back.ext.item_slots, raw_slots,
        "the LGX6 block carried every slot, holes included"
    );
    assert_eq!(back.ext.inventory, sf.ext.inventory);
}

#[test]
fn the_write_back_reproduces_the_block_bytes() {
    let Some((sc, raw_slots)) = card_block() else {
        eprintln!("[skip] no usable Legaia memory-card image at ~/.mednafen/sav/");
        return;
    };
    let sf = SaveFile::from_retail_sc_block(&sc, 4).expect("lift the SC block");
    let mut fresh = vec![0u8; sc.len()];
    sf.write_into_retail_sc_block(&mut fresh)
        .expect("compose an SC block");
    let written = legaia_save::card::read_retail_item_window(&fresh).expect("item window");
    let round: Vec<(u8, u8)> = written
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| (c[0], c[1]))
        .collect();
    assert_eq!(
        round, raw_slots,
        "the composed block's item region is the card's own, slot for slot"
    );
}
