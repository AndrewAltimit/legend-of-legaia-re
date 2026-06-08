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

/// For every scene shop on `patcher`'s image, the unsellable template-id padding
/// tail each record's `count` carries past its sellable stock: keyed by
/// `(entry_idx, count_off)` so a record matches across a re-pack, valued by the
/// decoded padding bytes (positions `stock..count`). The shop randomizer must
/// leave these untouched — it only rewrites the leading sellable run.
///
/// The price table is read from the SAME image: `randomize_shops` (Random mode)
/// also prices the chest-found equipment, so judging "sellable" with a stale
/// price table would mis-split the partition for an image whose prices moved.
fn shop_padding_tails(patcher: &DiscPatcher) -> BTreeMap<(usize, usize), Vec<u8>> {
    let scus = read_file_in_image(patcher.image(), "SCUS_942.54").expect("SCUS present");
    let priced =
        |id: u8| legaia_asset::item_names::price_slot(&scus, id).is_some_and(|(_, p)| p > 0);
    let mut out = BTreeMap::new();
    for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        // Structural scan (no mask) exposes the FULL declared list incl. padding.
        let Some(loc) = legaia_asset::shop_stock::locate(&entry, None) else {
            continue;
        };
        for r in &loc.records {
            let ids: Vec<u8> = r.id_offsets.iter().map(|&o| loc.decoded[o]).collect();
            let stock = ids.iter().take_while(|&&id| priced(id)).count();
            out.insert((idx, r.count_off), ids[stock..].to_vec());
        }
    }
    out
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

/// The shop randomizer rewrites only the leading sellable stock of each record,
/// never the trailing unsellable template-id padding the `count` over-counts.
/// Decode every shop's padding tail before and after a randomization and assert
/// it is byte-identical — a regression that let the randomizer shuffle the
/// padding back into the stock (the pre-fix behaviour) would change a tail.
#[test]
fn shop_randomization_leaves_the_padding_tail_untouched() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let before = shop_padding_tails(&DiscPatcher::open(disc.clone()).unwrap());
    // The disc genuinely carries padding (most shops do) — otherwise the test is
    // vacuous.
    assert!(
        before.values().any(|t| !t.is_empty()),
        "expected some shops to carry an unsellable padding tail"
    );

    for mode in [DropMode::Shuffle, DropMode::Random] {
        let mut patcher = DiscPatcher::open(disc.clone()).expect("open disc");
        apply::randomize_shops(&mut patcher, 0x5EED, mode).expect("randomize shops");
        let after = shop_padding_tails(&patcher);
        assert_eq!(
            before, after,
            "{mode:?} changed a shop's unsellable padding tail"
        );
    }
}
