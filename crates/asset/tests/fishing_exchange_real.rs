//! Disc-gated reproducibility for the fishing point-exchange + spawn tables.
//!
//! Re-extract the fishing overlay (PROT 0972) from the user's `PROT.DAT`,
//! decode the two per-venue exchange tables + spawn tables out of it, and
//! assert the structural invariants that pin the layouts (no Sony bytes
//! asserted - the literal prices / item ids stay on the user's disc):
//!
//! * both venue pages decode 6 rows each;
//! * every `limit` is 1 (one-time) or 99 (repeatable), every price is positive
//!   and below the 999999 display cap, in whole-hundreds like the walkthrough
//!   prize lists;
//! * every granted item id resolves to a named row of the SCUS item table;
//! * row 0 carries its page's maximum price (the affordability-gate top
//!   prize);
//! * spawn tables: only rod rows 0..3 are populated, every species id is in
//!   the species table's range, and each populated rod row has a nonzero
//!   band-0 pick.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/` files are absent.

use std::path::PathBuf;

use legaia_asset::fishing_exchange::{self as exch, EXCHANGE_ROWS};
use legaia_asset::fishing_species::{self as fish, SPECIES_COUNT};
use legaia_asset::item_names::ItemNameTable;
use legaia_asset::static_overlay;
use legaia_prot::archive::Archive;

fn extracted_file(name: &str) -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for dir in ["extracted", "../../extracted"] {
        let f = PathBuf::from(dir).join(name);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

fn fishing_overlay() -> Option<Vec<u8>> {
    let prot = extracted_file("PROT.DAT")?;
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let rec = static_overlay::overlay_map()
        .by_prot_index(fish::FISHING_OVERLAY_PROT_INDEX as u32)
        .expect("fishing overlay in static map");
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == rec.prot_index)
        .cloned()
        .expect("PROT entry present");
    let mut raw = Vec::new();
    archive.read_entry(&entry, &mut raw).expect("read entry");
    Some(static_overlay::as_loaded(&raw, rec).expect("as-loaded form"))
}

#[test]
fn exchange_tables_reproduce_and_are_well_formed() {
    let Some(overlay) = fishing_overlay() else {
        eprintln!("[skip] LEGAIA_DISC_BIN or extracted/PROT.DAT missing");
        return;
    };

    let ex = exch::parse(&overlay).expect("exchange tables parse");
    let names = extracted_file("SCUS_942.54")
        .map(|p| std::fs::read(p).expect("read SCUS"))
        .and_then(|scus| ItemNameTable::from_scus(&scus));

    for (page, rows) in ex.venues.iter().enumerate() {
        assert_eq!(rows.len(), EXCHANGE_ROWS, "page {page} row count");
        let mut one_time = 0;
        let mut repeatable = 0;
        for r in rows {
            assert!(
                r.limit == 1 || r.limit == 99,
                "page {page} row {} limit {} not 1/99",
                r.row,
                r.limit
            );
            if r.is_one_time() {
                one_time += 1;
            } else {
                repeatable += 1;
            }
            assert!(
                r.price > 0 && r.price < 1_000_000,
                "page {page} row {} price {} out of range",
                r.row,
                r.price
            );
            assert_eq!(
                r.price % 100,
                0,
                "page {page} row {} price {} not whole hundreds",
                r.row,
                r.price
            );
            assert!(
                r.item_id > 0 && r.item_id < 0x100,
                "page {page} row {} item id {:#x} not a u8 item id",
                r.row,
                r.item_id
            );
            if let Some(t) = &names {
                assert!(
                    t.name(r.item_id as u8).is_some(),
                    "page {page} row {} item id {:#x} unnamed in the SCUS table",
                    r.row,
                    r.item_id
                );
            }
        }
        // Each venue mixes one-time prizes with repeatable stock.
        assert!(one_time >= 1, "page {page} has no one-time prize");
        assert!(repeatable >= 1, "page {page} has no repeatable row");
        // Row 0 is the affordability-gated top prize.
        let max_price = rows.iter().map(|r| r.price).max().unwrap();
        assert_eq!(
            rows[0].price, max_price,
            "page {page} row 0 is not the top prize"
        );
        assert!(rows[0].is_one_time(), "page {page} top prize repeats");
    }

    // The two venues sell distinct top prizes from one shared bit space.
    assert_ne!(
        ex.venues[0][0].item_id, ex.venues[1][0].item_id,
        "venue top prizes alias"
    );

    // Spawn tables: rod rows 0..3 populated, ids within the species table.
    let spawns = fish::parse_spawn_tables(&overlay).expect("spawn tables parse");
    for (page, table) in spawns.iter().enumerate() {
        assert_eq!(table.len(), fish::SPAWN_RODS);
        for (rod, row) in table.iter().enumerate() {
            for (band, &id) in row.iter().enumerate() {
                assert!(
                    (id as usize) < SPECIES_COUNT,
                    "page {page} rod {rod} band {band} species {id} out of range"
                );
                if rod >= 3 {
                    assert_eq!(id, 0, "page {page} rod {rod} should be padding");
                }
            }
            if rod < 3 {
                assert_ne!(row[0], u32::MAX, "unreachable");
                assert!(
                    row.iter().any(|&id| id != 0),
                    "page {page} rod {rod} row empty"
                );
            }
        }
    }
}
