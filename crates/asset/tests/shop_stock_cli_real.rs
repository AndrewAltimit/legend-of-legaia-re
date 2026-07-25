//! Disc-gated oracle for what `asset shop-stock` reports.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.
//!
//! The subcommand is thin wiring over `legaia_asset::shop_stock` +
//! `legaia_asset::item_names`, so this test drives the same library calls in
//! the same order rather than shelling out - what it pins is the *join*, which
//! is the part a reader has to get right:
//!
//! - the scene label comes from `block_for_extraction_index` (the CDNAME
//!   `N - 2` shift), not from the extraction filename;
//! - the id list is decoded with a **name** mask, so the unsellable tail
//!   survives into the output instead of being rejected at scan time;
//! - `sellable_count` is computed separately with the **price** predicate, so
//!   "decodes N, sells M" is a real distinction and M <= N always;
//! - the tail is only ever a suffix - no shop interleaves priced and unpriced
//!   ids, which is what makes a prefix length the right answer at all.

use legaia_asset::{item_names, shop_stock};
use legaia_prot::{archive::Archive, cdname};
use std::path::PathBuf;

fn extracted_root() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT.DAT").is_file())
}

struct Found {
    entry: u32,
    scene: String,
    shop: String,
    decoded: usize,
    sellable: usize,
}

fn collect() -> Option<Vec<Found>> {
    let root = extracted_root()?;
    let scus = std::fs::read(root.join("SCUS_942.54")).ok()?;
    let names = item_names::ItemNameTable::from_scus(&scus)?;
    let named_mask: [bool; 256] = std::array::from_fn(|id| names.name(id as u8).is_some());
    let price_of = |id: u8| item_names::item_price(&scus, id).unwrap_or(0);
    let map = cdname::parse(&root.join("CDNAME.TXT")).ok()?;

    let mut archive = Archive::open(&root.join("PROT.DAT")).ok()?;
    let entries = archive.entries.clone();
    let mut buf = Vec::new();
    let mut out = Vec::new();
    for entry in &entries {
        if archive.read_entry(entry, &mut buf).is_err() {
            continue;
        }
        let Some(loc) = shop_stock::locate(&buf, Some(&named_mask)) else {
            continue;
        };
        for rec in &loc.records {
            let ids: Vec<u8> = rec
                .id_offsets
                .iter()
                .filter_map(|&o| loc.decoded.get(o).copied())
                .collect();
            let sellable = rec.sellable_count(&loc.decoded, |id| price_of(id) > 0);

            // The prefix claim, re-asserted per shop: everything before
            // `sellable` is priced and everything after is not. If this ever
            // fails, "sellable_count" is the wrong shape for that shop and the
            // listing would silently drop real stock.
            for (i, &id) in ids.iter().enumerate() {
                let priced = price_of(id) > 0;
                assert_eq!(
                    priced,
                    i < sellable,
                    "entry {} shop {:?}: id {:#04x} at {} breaks the priced-prefix \
                     partition (sellable={} of {})",
                    entry.index,
                    rec.name,
                    id,
                    i,
                    sellable,
                    ids.len()
                );
            }
            out.push(Found {
                entry: entry.index,
                scene: cdname::block_for_extraction_index(&map, entry.index)
                    .unwrap_or("")
                    .to_string(),
                shop: rec.name.clone(),
                decoded: ids.len(),
                sellable,
            });
        }
    }
    Some(out)
}

#[test]
fn shop_stock_joins_scene_names_and_prices() {
    let Some(shops) = collect() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    assert!(
        shops.len() > 20,
        "expected the disc's full shop set, found {}",
        shops.len()
    );
    for s in &shops {
        assert!(!s.shop.is_empty(), "entry {}: empty shop name", s.entry);
        assert!(
            s.sellable <= s.decoded,
            "entry {}: sells {} of {} decoded",
            s.entry,
            s.sellable,
            s.decoded
        );
        assert!(s.sellable > 0, "entry {}: shop with no stock", s.entry);
        // Every shop sits inside a named CDNAME block; an empty label would
        // mean the +2 resolution had fallen off the map.
        assert!(!s.scene.is_empty(), "entry {}: no CDNAME block", s.entry);
    }
}

#[test]
fn the_unsellable_tail_exists_and_is_reported() {
    let Some(shops) = collect() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    // The whole reason the command prints two numbers: a good share of shops
    // over-count. If this stops being true the distinction is untested, not
    // absent, and the doc line promising it would be stale.
    let padded = shops.iter().filter(|s| s.sellable < s.decoded).count();
    assert!(
        padded > 5,
        "expected many shops to carry an unsellable tail, found {padded}"
    );
    // The module docs' worked example, kept honest: one shop named "Market"
    // decodes 10 ids and sells 7.
    let market = shops
        .iter()
        .find(|s| s.shop == "Market")
        .expect("a shop named \"Market\"");
    assert_eq!((market.decoded, market.sellable), (10, 7));
}

#[test]
fn the_confirm_picker_vendor_is_found() {
    let Some(shops) = collect() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    // Biron Monastery's Corey sits behind a dialogue confirm-picker whose
    // option-jump table desyncs a linear disassembler before it reaches the
    // op `0x49`. Finding it is the evidence that the scan is a byte scan and
    // has not been "improved" into an opcode walk.
    let corey = shops.iter().find(|s| s.shop == "Corey").expect(
        "Biron Monastery's Corey vendor - if this is missing, the site \
                 scan has become an opcode walk",
    );
    assert_eq!(corey.scene, "bylon");
    assert!(corey.sellable > 0);
}
