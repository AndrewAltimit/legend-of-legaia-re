//! Disc-gated cross-validation of the curated casino / Muscle Dome prize lists
//! against the real prize-exchange table in the menu/save/shop overlay's data
//! segment (PROT entry 0899, stored raw).
//!
//! Unlike a town gold merchant (inline in a scene's field-VM script), the coin
//! prize counter is a static table at `DAT_801e4518` - file offset
//! `0x15D00` of PROT 0899 - decoded by the randomizer's canonical reader
//! `legaia_patcher::casino::CasinoExchange`. Each prize is an 8-byte record
//! `[u16 item_id][u16 story_gate][u32 coin_price]`; blocks are `0x60` bytes
//! (12 records), terminated by the first `item_id == 0`. The retail US disc
//! holds four blocks: block 1 is the Vidna casino counter, block 0 the Sol
//! Tower Muscle Dome counter (its high-value prizes story-gated via the `+2`
//! word), and blocks 2/3 are short pre-progression states (a single cheap
//! healing item each - the counter shows these before progression unlocks the
//! full list). To avoid a dev-dependency cycle (`legaia-patcher` already depends
//! on `legaia-gamedata`) the 8-byte record walk is reproduced inline here; the
//! constants mirror `legaia_patcher::casino`.
//!
//! Every curated prize joins a disc record byte-exact on (item name, coin
//! price) - across both the Vidna and Sol lists - with one documented
//! exception: **Earth Egg @ 100000 coins**, the Muscle Paradise "Chicken King"
//! easter egg, which is a separate hidden exchange and is NOT in the four-block
//! prize table. The exception is asserted explicitly so it can't go vacuous.
//!
//! Skips silently when `extracted/SCUS_942.54` or the PROT 0899 entry is
//! missing.

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_asset::item_names::ItemNameTable;
use legaia_gamedata::Database;

/// Mirror of `legaia_patcher::casino` constants (kept inline to avoid a
/// dev-dependency cycle; see the module doc).
const CASINO_TABLE_OFFSET: usize = 0x15D00;
const BLOCK_SIZE: usize = 0x60;
const RECORD_SIZE: usize = 8;
const RECORDS_PER_BLOCK: usize = BLOCK_SIZE / RECORD_SIZE;
const CASINO_BLOCK_COUNT: usize = 4;

fn workspace() -> Option<PathBuf> {
    Some(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .parent()?
            .to_path_buf(),
    )
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// One decoded prize record (item id + coin price; the story-gate word is read
/// past but not needed for the curated join).
struct Prize {
    item_id: u8,
    price: u32,
}

/// Decode the leading `item_id > 0` records of each of the four prize blocks.
fn decode_blocks(buf: &[u8]) -> Vec<Vec<Prize>> {
    let mut blocks = Vec::new();
    for b in 0..CASINO_BLOCK_COUNT {
        let block_off = CASINO_TABLE_OFFSET + b * BLOCK_SIZE;
        let mut recs = Vec::new();
        for r in 0..RECORDS_PER_BLOCK {
            let o = block_off + r * RECORD_SIZE;
            let item_id = u16::from_le_bytes([buf[o], buf[o + 1]]);
            if item_id == 0 {
                break;
            }
            let price = u32::from_le_bytes([buf[o + 4], buf[o + 5], buf[o + 6], buf[o + 7]]);
            recs.push(Prize {
                // ids are in the shared 256-entry item-name space
                item_id: item_id as u8,
                price,
            });
        }
        blocks.push(recs);
    }
    blocks
}

#[test]
fn curated_casino_prizes_match_the_disc() {
    let Some(ws) = workspace() else { return };
    let Ok(scus) = std::fs::read(ws.join("extracted").join("SCUS_942.54")) else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let prot0899 = ws.join("extracted").join("PROT").join("0899_xxx_dat.BIN");
    let Ok(bytes) = std::fs::read(&prot0899) else {
        eprintln!("[skip] extracted/PROT/0899_xxx_dat.BIN missing");
        return;
    };

    let names = ItemNameTable::from_scus(&scus).expect("parse item-name table");
    let blocks = decode_blocks(&bytes);
    assert_eq!(
        blocks.len(),
        CASINO_BLOCK_COUNT,
        "expected four prize blocks"
    );

    // Blocks 0 and 1 are the full Sol / Vidna prize lists; blocks 2+ are the
    // short pre-progression states (a single cheap item) - not curated prizes.
    let full = &blocks[0..2];
    let early = &blocks[2..];
    assert!(
        full.iter().all(|b| b.len() >= 10),
        "the two full prize lists should each carry 10+ prizes"
    );
    assert!(
        early.iter().all(|b| b.len() <= 2),
        "pre-progression blocks should be short"
    );

    // Disc prize population: (normalized item name, coin price) over the two
    // full lists. A set handles the items that legitimately repeat across the
    // Vidna and Sol counters (e.g. Fury Boost @ 150).
    let disc: BTreeSet<(String, u32)> = full
        .iter()
        .flatten()
        .map(|p| (norm(names.name(p.item_id).unwrap_or("")), p.price))
        .collect();

    let db = Database::load();
    let earth_egg = norm("Earth Egg");

    // Forward: every curated prize joins the disc table, except the Earth Egg
    // easter egg, which is a separate hidden exchange (asserted below).
    let mut matched = 0usize;
    let mut earth_egg_seen = false;
    let mut unmatched: Vec<String> = Vec::new();
    for sp in db.slot_prizes() {
        let Some(resolved) = db.resolve_key(&sp.item) else {
            unmatched.push(format!("{} (unresolved key {})", sp.item, sp.item));
            continue;
        };
        let name = norm(resolved.name);
        if disc.contains(&(name.clone(), sp.cost_coins)) {
            matched += 1;
        } else if name == earth_egg && sp.cost_coins == 100_000 {
            earth_egg_seen = true;
        } else {
            unmatched.push(format!(
                "{} / {} @ {} coins",
                sp.location, resolved.name, sp.cost_coins
            ));
        }
    }
    assert!(
        unmatched.is_empty(),
        "curated casino prizes with no matching disc record (disc is authoritative): {unmatched:#?}"
    );
    assert!(
        earth_egg_seen,
        "Earth Egg easter-egg exception went vacuous - curated table no longer lists it @ 100000"
    );

    // Reverse: every disc prize in the two full lists is a curated prize, so
    // the table carries nothing the chart omits.
    let curated: BTreeSet<(String, u32)> = db
        .slot_prizes()
        .iter()
        .filter_map(|sp| {
            db.resolve_key(&sp.item)
                .map(|r| (norm(r.name), sp.cost_coins))
        })
        .collect();
    let mut disc_only: Vec<String> = Vec::new();
    for (name, price) in &disc {
        if !curated.contains(&(name.clone(), *price)) {
            disc_only.push(format!("{name} @ {price}"));
        }
    }
    assert!(
        disc_only.is_empty(),
        "disc prize records absent from the curated chart: {disc_only:#?}"
    );

    assert!(
        matched >= 20,
        "expected 20+ curated↔disc prize matches, got {matched}"
    );
}
