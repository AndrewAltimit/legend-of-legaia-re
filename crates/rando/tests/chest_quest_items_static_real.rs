//! Disc-gated oracle for the chest randomizer's quest-item protection.
//!
//! The default keep-static set is now derived from the disc: every named,
//! unsellable (price-0) item except the chest-found equipment, unioned with the
//! curated tool list (see `items::default_static_chest_items`). This pins that
//! the door keys, garden tools, letters, books, and other story items are all
//! covered, and proves the randomizer honors it end to end: under `Random`
//! mode, no quest item is moved out of its chest and none is dropped into a
//! chest that didn't already hold it. Skips without `LEGAIA_DISC_BIN`.

use legaia_iso::iso9660::read_file_in_image;
use legaia_rando::apply;
use legaia_rando::chest::SceneChests;
use legaia_rando::disc::DiscPatcher;
use legaia_rando::drops::DropMode;

fn load_disc() -> Option<Vec<u8>> {
    let p = std::path::PathBuf::from(std::env::var_os("LEGAIA_DISC_BIN")?);
    p.is_file().then(|| std::fs::read(&p).ok()).flatten()
}

/// (scene idx, site offsets, current items) for every scene with chest sites.
fn snapshot(patcher: &DiscPatcher) -> Vec<(usize, Vec<usize>, Vec<u8>)> {
    let mut out = Vec::new();
    for idx in 0..patcher.entry_count() {
        let Ok(entry) = patcher.read_entry(idx) else {
            continue;
        };
        if let Some(sc) = SceneChests::locate(&entry, idx) {
            out.push((idx, sc.sites.clone(), sc.current_items()));
        }
    }
    out
}

/// Named quest / key / story items the player must keep predictable - the door
/// keys, garden-quest tools, letters, diaries, and one-off story items. Every
/// one must land in the disc-derived default keep-static set.
const QUEST_ITEMS: &[(u8, &str)] = &[
    (0x6a, "Zalan's Letter"),
    (0x9a, "Mary's Diary"),
    (0x9b, "Soren Secrets"),
    (0xa4, "Sunrise Key"),
    (0xa5, "Lightning Key"),
    (0xa6, "Star Key"),
    (0xa7, "Mountain Key"),
    (0xa8, "Water Key"),
    (0xa9, "Fertilizer"),
    (0xaa, "Weed Hammer"),
    (0xb0, "Spring Salts"),
    (0xb3, "Letona Key"),
    (0xb4, "West Ratayu Key"),
    (0xb7, "Genesis Seedling"),
    (0xb8, "Soren Flute"),
    (0xbb, "Music Score"),
    (0xbd, "Ruins Key"),
    (0xbe, "TimeSpace Bomb"),
    (0xbf, "Evil Seru Key"),
];

#[test]
fn default_keep_static_covers_every_quest_item() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let scus = read_file_in_image(&original, "SCUS_942.54").expect("SCUS on disc");
    let keep_static = legaia_rando::items::default_static_chest_items(&scus);

    for &(id, name) in QUEST_ITEMS {
        assert!(
            keep_static.contains(&id),
            "quest item {name} (0x{id:02x}) must be in the default keep-static set"
        );
    }
    // The chest-found equipment (real gear shipping price-0) must stay
    // randomizable, i.e. NOT static.
    for &(id, _) in legaia_rando::item_price::CHEST_EQUIPMENT_PRICES {
        assert!(
            !keep_static.contains(&id),
            "chest-found equipment 0x{id:02x} must remain randomizable"
        );
    }
    // Buyable items are NOT protected: only genuinely unsellable quest items are.
    // The Silver Compass (0xf3) is a shop-tradeable accessory (lowers the
    // battle-start ambush rate; price > 0), so it must stay randomizable.
    assert!(
        legaia_asset::item_names::item_price(&scus, 0xf3).is_some_and(|p| p > 0),
        "Silver Compass (0xf3) is expected to be a buyable item on this disc"
    );
    assert!(
        !keep_static.contains(&0xf3),
        "the buyable Silver Compass (0xf3) must remain randomizable"
    );
}

#[test]
fn random_fill_never_moves_or_places_a_quest_item() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seed = 0xBADF00D_u64;
    let scus = read_file_in_image(&original, "SCUS_942.54").expect("SCUS on disc");
    let keep_static = legaia_rando::items::default_static_chest_items(&scus);
    let pool = legaia_rando::items::valid_item_pool(&scus).expect("item pool");

    let before = snapshot(&DiscPatcher::open(original.clone()).unwrap());

    // Where does each quest item currently sit? (scene idx, site offset).
    let quest_sites = |snap: &[(usize, Vec<usize>, Vec<u8>)]| {
        let mut v: Vec<(usize, usize, u8)> = Vec::new();
        for (idx, offs, items) in snap {
            for (o, it) in offs.iter().zip(items) {
                if keep_static.contains(it) {
                    v.push((*idx, *o, *it));
                }
            }
        }
        v.sort_unstable();
        v
    };
    let before_quest = quest_sites(&before);

    let mut patcher = DiscPatcher::open(original.clone()).unwrap();
    apply::randomize_chests(&mut patcher, &pool, seed, DropMode::Random, &keep_static)
        .expect("randomize");
    let after = snapshot(&patcher);
    let after_quest = quest_sites(&after);

    // Every quest item occupies exactly the same chest site before and after:
    // none moved, vanished, or appeared anywhere new.
    assert_eq!(
        before_quest, after_quest,
        "a quest item moved or was placed by the Random chest fill"
    );

    eprintln!(
        "random chest fill: {} quest-item chest(s) unchanged; {} keep-static id(s)",
        before_quest.len(),
        keep_static.len()
    );
}
