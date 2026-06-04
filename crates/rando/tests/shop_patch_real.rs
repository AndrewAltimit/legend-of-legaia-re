//! Disc-gated tests for the town-merchant + casino shop randomizers.
//!
//! Gates on `LEGAIA_DISC_BIN`; skips+passes when unset. Patched images live only
//! in memory.

use legaia_iso::iso9660::read_file_in_image;
use legaia_rando::apply;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;
use std::collections::BTreeMap;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

fn item_names(image: &[u8]) -> Option<legaia_asset::item_names::ItemNameTable> {
    read_file_in_image(image, "SCUS_942.54")
        .and_then(|scus| legaia_asset::item_names::ItemNameTable::from_scus(&scus))
}

#[test]
fn town_shops_enumerate_cleanly() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let shops = apply::current_shops(&patcher).expect("enumerate shops");
    let names = item_names(&disc);

    assert!(!shops.is_empty(), "the disc has town shops");
    for s in &shops {
        // Every shop name is printable + non-trivial, every item id is named.
        assert!(s.name.len() >= 2, "shop name too short: {:?}", s.name);
        assert!(
            s.name.chars().all(|c| c.is_ascii_graphic() || c == ' '),
            "shop name not printable: {:?}",
            s.name
        );
        assert!(!s.items.is_empty(), "shop {:?} sells nothing", s.name);
        if let Some(t) = &names {
            for &id in &s.items {
                assert!(
                    t.name(id).is_some(),
                    "shop {:?} sells unnamed id {id}",
                    s.name
                );
            }
        }
    }

    // The Rim Elm "Variety Store" must be present with its 10 known items
    // (pinned from a live capture; order-independent multiset check).
    let variety = shops
        .iter()
        .find(|s| s.name.starts_with("Variety"))
        .expect("Rim Elm Variety Store present");
    let mut got = variety.items.clone();
    got.sort_unstable();
    let mut want = vec![0x22, 0x34, 0x59, 0xd6, 0x77, 0x7e, 0x88, 0x43, 0xc7, 0xc8];
    want.sort_unstable();
    assert_eq!(got, want, "Variety Store sells its known 10 items");
}

#[test]
fn shop_shuffle_preserves_multiset_and_round_trips() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let before = apply::current_shops(&DiscPatcher::open(disc.clone()).unwrap()).unwrap();
    let multiset = |listings: &[apply::ShopListing]| {
        let mut m: BTreeMap<u8, usize> = BTreeMap::new();
        for s in listings {
            for &id in &s.items {
                *m.entry(id).or_default() += 1;
            }
        }
        m
    };

    let seed = 0x5409u64;
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let report = apply::randomize_shops(&mut patcher, seed, DropMode::Shuffle).unwrap();
    assert!(report.slots_total > 0);
    assert_eq!(patcher.image().len(), disc.len(), "image size unchanged");

    // Re-enumerate off the patched image and confirm the global item multiset is
    // preserved over the written shops (skipped scenes keep their items, which
    // also leaves the global multiset intact).
    let after = apply::current_shops(&patcher).expect("re-enumerate patched shops");
    assert_eq!(
        multiset(&before),
        multiset(&after),
        "shuffle preserves the global shop-item multiset"
    );
    // Per-shop item counts and names are unchanged (only ids move).
    assert_eq!(before.len(), after.len(), "same number of shops");
    for (b, a) in before.iter().zip(&after) {
        assert_eq!(b.name, a.name, "shop name preserved");
        assert_eq!(b.items.len(), a.items.len(), "shop item count preserved");
    }

    // Deterministic for a fixed seed.
    let mut p2 = DiscPatcher::open(disc).expect("reopen");
    apply::randomize_shops(&mut p2, seed, DropMode::Shuffle).unwrap();
    assert_eq!(
        p2.image(),
        patcher.image(),
        "fixed seed is byte-deterministic"
    );
}

#[test]
fn casino_shuffle_preserves_prizes_and_round_trips() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(disc.clone()).expect("open disc");
    let before = apply::current_casino(&patcher)
        .expect("read casino")
        .expect("casino table present");
    assert!(before.active_slot_count() > 0, "casino has prizes");

    let seed = 0xCA5107u64;
    let mut p = DiscPatcher::open(disc.clone()).expect("open disc");
    let changed = apply::randomize_casino(&mut p, seed, DropMode::Shuffle).unwrap();
    assert_eq!(p.image().len(), disc.len(), "image size unchanged");

    let after = apply::current_casino(&p).unwrap().unwrap();
    let bag = |ex: &legaia_rando::casino::CasinoExchange| {
        let mut v: Vec<(u16, u32)> = ex
            .blocks
            .iter()
            .flatten()
            .map(|r| (r.item_id, r.price))
            .collect();
        v.sort_unstable();
        v
    };
    assert_eq!(
        bag(&before),
        bag(&after),
        "casino shuffle preserves the (item, price) prize multiset"
    );
    // Block counts unchanged.
    let bc = |ex: &legaia_rando::casino::CasinoExchange| {
        ex.blocks.iter().map(|b| b.len()).collect::<Vec<_>>()
    };
    assert_eq!(bc(&before), bc(&after), "prize counts per block preserved");
    assert!(changed <= before.active_slot_count());

    // Deterministic.
    let mut p2 = DiscPatcher::open(disc).expect("reopen");
    apply::randomize_casino(&mut p2, seed, DropMode::Shuffle).unwrap();
    assert_eq!(p2.image(), p.image(), "fixed seed is byte-deterministic");
}
