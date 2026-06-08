//! Disc-gated cross-table integrity sweep over the scene-embedded id spaces.
//!
//! Companion to `legaia-asset`'s `monster_cross_table_integrity` (which ties the
//! monster archive's drop / magic / steal ids back to the item + spell tables).
//! Where that one covers the *static* roster tables, this one covers the ids the
//! field-VM scripts embed inline in every scene MAN — the same populations the
//! randomizer rewrites — and asserts each resolves in the table it indexes:
//!
//!   * **chest grant id  -> item-name table**   every `GIVE_ITEM` (op `0x39`)
//!     hands out a real, named item.
//!   * **shop stock id   -> item price slot**    every town-merchant record is a
//!     run of sellable (`SCUS_942.54` price `> 0`) items followed by an optional
//!     trailing run of unsellable template ids — the record `count` over-counts
//!     the real stock by this padding (see below). The guard pins that clean
//!     partition: the stock never interleaves priced + unpriced ids, and the
//!     padding stays within the observed bound.
//!   * **door dest name  -> CDNAME scene**       every `0x3F` named-warp's inline
//!     destination resolves to a declared scene block in `CDNAME.TXT`.
//!
//! ## Shop-record padding (found by this sweep)
//!
//! The op-`0x49` shop record's `count` byte counts the leading purchasable stock
//! **plus** a trailing run of unsellable, price-`0` template ids (commonly the
//! "Ra-Seru Meta $N" placeholders `0x01/0x02/0x03`, or a lone `0x03`); the real
//! shop UI stops at the sellable run. The original record doc was pinned from the
//! Rim Elm Variety Store, which happens to have a tail-less ten-item list, so the
//! padding never showed. Across the whole disc every shop partitions cleanly —
//! a priced prefix then an unpriced tail, never interleaved — and the priced
//! prefix matches the curated walkthrough stock (e.g. "Market" = 7 items, not the
//! decoded 10). That clean partition is exactly what this guard asserts.
//!
//! The shape is the cheap, reusable guard the backlog calls for: decode side A
//! (the scene-embedded ids), decode side B (the indexed table), assert every
//! cross-reference resolves, with a non-vacuous floor so a parser that silently
//! returns nothing can't pass. A layout drift in any of the scene scanners or
//! the SCUS tables surfaces here as a dangling reference, not as a wrong item /
//! a warp to nowhere in-game.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::item_names::{self, ItemNameTable};
use legaia_iso::iso9660::read_file_in_image;
use legaia_prot::cdname;
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn scus(image: &[u8]) -> Vec<u8> {
    read_file_in_image(image, "SCUS_942.54").expect("SCUS_942.54 present on disc")
}

#[test]
fn every_chest_grant_id_names_a_real_item() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let items = ItemNameTable::from_scus(&scus(&disc)).expect("parse item-name table");

    let chests = apply::current_chests(&patcher).expect("enumerate chest sites");
    assert!(chests.len() > 30, "too few chest sites: {}", chests.len());

    let mut dangling: Vec<String> = Vec::new();
    for c in &chests {
        // Every chest grants a concrete item id; there is no no-grant sentinel
        // in this population (an empty chest is simply not a give-item site).
        if items.name(c.item).is_none_or(|s| s.is_empty()) {
            dangling.push(format!(
                "chest in entry {} @ man+0x{:X} grants unnamed id 0x{:02X}",
                c.entry_idx, c.man_offset, c.item
            ));
        }
    }
    assert!(
        dangling.is_empty(),
        "chest grant ids that don't name a real item (parser drift?):\n{}",
        dangling.join("\n")
    );
}

#[test]
fn every_shop_stock_partitions_into_priced_then_padding() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let scus = scus(&disc);
    let priced = |id: u8| item_names::price_slot(&scus, id).is_some_and(|(_, price)| price > 0);

    // Scan structurally (no mask) so each record exposes its FULL declared list,
    // padding included — the consumer scanners trim the padding, so this checks
    // the raw on-disc partition the trim relies on.
    let mut shops = 0usize;
    let mut ids_checked = 0usize;
    let mut sellable_total = 0usize;
    let mut bad: Vec<String> = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher.read_entry(idx).expect("read PROT entry");
        let Some(loc) = legaia_asset::shop_stock::locate(&entry, None) else {
            continue;
        };
        for r in &loc.records {
            shops += 1;
            let items: Vec<u8> = r.id_offsets.iter().map(|&o| loc.decoded[o]).collect();
            ids_checked += items.len();
            // The sellable stock is the leading priced run; the tail (if any) is
            // the unsellable template-id padding the count over-counts.
            let stock = items.iter().take_while(|&&id| priced(id)).count();
            let tail = &items[stock..];
            sellable_total += stock;

            // Every shop must sell at least one real item.
            if stock == 0 {
                bad.push(format!(
                    "shop {:?} has no sellable stock: {items:02X?}",
                    r.name
                ));
            }
            // The tail must be ENTIRELY unsellable — a priced id after an
            // unpriced one means the partition isn't clean (record drift).
            if let Some(&id) = tail.iter().find(|&&id| priced(id)) {
                bad.push(format!(
                    "shop {:?} interleaves priced id 0x{id:02X} into the padding tail: {items:02X?}",
                    r.name
                ));
            }
            // The padding stays within the observed bound (≤3); a longer tail
            // would mean a real item was misread as padding.
            if tail.len() > 3 {
                bad.push(format!(
                    "shop {:?} padding tail too long ({}): {items:02X?}",
                    r.name,
                    tail.len()
                ));
            }
        }
    }
    assert!(shops > 20, "too few shops found: {shops}");
    eprintln!(
        "[xtable] shops={shops} ids={ids_checked} sellable={sellable_total} padding={}",
        ids_checked - sellable_total
    );
    assert!(ids_checked > 30, "too few shop ids checked: {ids_checked}");
    // Non-vacuous: the sweep actually saw the padding it's characterising.
    assert!(
        ids_checked > sellable_total,
        "expected some shops to carry the unsellable padding tail"
    );
    assert!(
        bad.is_empty(),
        "shop stock didn't partition into priced-then-padding (parser drift?):\n{}",
        bad.join("\n")
    );
}

#[test]
fn every_door_destination_resolves_to_a_real_scene() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let cd = apply::cdname_map(&patcher);
    assert!(!cd.is_empty(), "CDNAME.TXT parsed");

    let doors = apply::current_doors(&patcher).expect("enumerate doors");
    assert!(doors.len() >= 120, "too few doors: {}", doors.len());

    let mut dangling: Vec<String> = Vec::new();
    for d in &doors {
        // The inline destination name a 0x3F op carries must be a scene block
        // declared in CDNAME.TXT — i.e. a real, loadable scene. A name that
        // doesn't resolve is a warp to nowhere (or a mis-decoded record).
        if cdname::block_range_for_name(&cd, &d.dest_scene).is_none() {
            dangling.push(format!(
                "door in {} @ man+0x{:X} warps to undeclared scene {:?}",
                d.home_scene, d.op_pc, d.dest_scene
            ));
        }
    }
    assert!(
        dangling.is_empty(),
        "door destinations that don't resolve to a CDNAME scene (parser drift?):\n{}",
        dangling.join("\n")
    );
}
