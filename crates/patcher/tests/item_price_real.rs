//! Disc-gated tests for the shop item-price edits + sellable-pool filtering:
//! the chest-found equipment gets the reviewed prices, quest/key items stay at
//! price 0 (and out of the sellable pool), and a shop `Random` pass only ever
//! stocks priced (non-quest) items. Gates on `LEGAIA_DISC_BIN`.

use legaia_asset::item_names;
use legaia_iso::iso9660::read_file_in_image;
use legaia_patcher::apply;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::drops::DropMode;
use legaia_patcher::item_price::{CHEST_EQUIPMENT_PRICES, price_patches, sellable_pool};

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

#[test]
fn chest_equipment_is_priced_and_quest_items_excluded() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&disc, "SCUS_942.54").expect("SCUS");

    // The 13 chest-equipment items ship free (price 0); the patch set covers all.
    for &(id, _) in CHEST_EQUIPMENT_PRICES {
        assert_eq!(
            item_names::item_price(&scus, id),
            Some(0),
            "item 0x{id:02x} should ship at price 0"
        );
    }
    assert_eq!(
        price_patches(&scus).unwrap().len(),
        CHEST_EQUIPMENT_PRICES.len(),
        "every chest-equipment price differs from its target"
    );

    // Apply the price edits; every target lands, and re-applying is a no-op.
    let mut patcher = DiscPatcher::open(disc.clone()).expect("open");
    assert_eq!(
        apply::apply_item_price_edits(&mut patcher).unwrap(),
        CHEST_EQUIPMENT_PRICES.len()
    );
    let scus2 = patcher
        .read_named_file("SCUS_942.54")
        .expect("patched SCUS");
    for &(id, price) in CHEST_EQUIPMENT_PRICES {
        assert_eq!(
            item_names::item_price(&scus2, id),
            Some(price),
            "item 0x{id:02x} priced to {price}"
        );
    }
    assert_eq!(
        apply::apply_item_price_edits(&mut patcher).unwrap(),
        0,
        "idempotent: nothing changes on a second apply"
    );

    // The sellable pool now includes the priced equipment and never a quest item.
    let pool = sellable_pool(&scus2).unwrap();
    for &(id, _) in CHEST_EQUIPMENT_PRICES {
        assert!(
            pool.contains(&id),
            "priced equipment 0x{id:02x} is sellable"
        );
    }
    // Known quest / key / story items (all price 0) must be absent.
    for q in [
        0xad, // Camera Stone
        0xbc, // Fire Droplet
        0x6e, 0x6f, 0x70, // Earth/Water/Light Egg
        0xa4, 0xa5, // Sunrise/Lightning Key
        0x9a, // Mary's Diary
        0x8f, // Fire Book I
        0x01, // Ra-Seru Meta $1 (story form)
    ] {
        assert!(
            !pool.contains(&q),
            "quest/key item 0x{q:02x} must not be sellable"
        );
    }
}

#[test]
fn shop_random_only_stocks_priced_items() {
    let Some(disc) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(disc).expect("open");
    apply::randomize_shops(&mut patcher, 0xBEEF, DropMode::Random).expect("randomize shops");

    // Re-decode the patched shops; every stocked item must be priced > 0 in the
    // (now price-patched) SCUS - i.e. never a quest / free item.
    let scus = patcher.read_named_file("SCUS_942.54").expect("SCUS");
    let shops = apply::current_shops(&patcher).expect("shops");
    let mut checked = 0;
    for s in &shops {
        for &id in &s.items {
            let price = item_names::item_price(&scus, id).unwrap_or(0);
            assert!(
                price > 0,
                "shop {:?} stocks 0-price item 0x{id:02x}",
                s.name
            );
            checked += 1;
        }
    }
    assert!(checked > 100, "checked a meaningful number of shop slots");
}
