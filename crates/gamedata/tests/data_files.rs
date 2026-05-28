//! Cross-validation invariants for the curated game-data tables.

use legaia_gamedata::{ArtKind, Character, Database, ShopEntryCategory, SpellFamily};

#[test]
fn arts_action_constants_match_legaia_art_tables() {
    let db = Database::load();
    for art in db.arts() {
        let Some(ac) = art.action_constant else {
            continue;
        };
        let action = legaia_art::ActionConstant::from_byte(ac)
            .unwrap_or_else(|| panic!("invalid action constant 0x{ac:02X}"));
        let expected = legaia_art::tables::art_name(art.character.to_art_character(), action)
            .unwrap_or_else(|| {
                panic!(
                    "no canonical art name for {:?} action 0x{:02X}",
                    art.character, ac
                )
            });
        assert!(
            names_match(expected, &art.name),
            "art {:?} action 0x{:02X}: gamedata name {:?} vs legaia_art canonical {:?}",
            art.character,
            ac,
            art.name,
            expected,
        );
    }
}

/// `legaia_art::tables` uses canonical names like `"Tornado Flame (Hyper)"`,
/// but a gamedata entry may shorten that to `"Tornado Flame"` if the kind is
/// already disambiguated by `kind = "hyper"`. Treat `(Hyper)` / `(Miracle)`
/// as equivalent for the comparison.
fn names_match(canonical: &str, gamedata: &str) -> bool {
    if canonical == gamedata {
        return true;
    }
    let strip = |s: &str| -> String {
        s.replace(" (Hyper)", "")
            .replace(" (Miracle)", "")
            .replace(" 1", "")
            .replace(" 2", "")
            .replace(" 3", "")
            .trim()
            .to_string()
    };
    strip(canonical) == strip(gamedata)
}

#[test]
fn art_directions_round_trip_through_token_mapping() {
    let db = Database::load();
    for art in db.arts() {
        for d in &art.directions {
            assert!(
                (1..=4).contains(d),
                "art {:?} direction byte {d} out of 1..=4",
                art.name
            );
        }
        assert_eq!(
            art.directions.len(),
            art.command.len(),
            "art {:?} direction/command length mismatch",
            art.name
        );
    }
}

#[test]
fn ap_costs_align_with_kind() {
    let db = Database::load();
    for art in db.arts() {
        match art.kind {
            ArtKind::Regular => assert!(art.ap <= 36, "regular art {} AP {}", art.name, art.ap),
            ArtKind::Hyper => assert!(
                art.ap >= 30 && art.ap <= 70,
                "hyper art {} AP {}",
                art.name,
                art.ap
            ),
            ArtKind::Super => assert!(
                art.ap >= 48 && art.ap <= 72,
                "super art {} AP {}",
                art.name,
                art.ap
            ),
            ArtKind::Miracle => assert_eq!(art.ap, 99, "miracle {} AP {}", art.name, art.ap),
        }
    }
}

#[test]
fn miracle_arts_one_per_character() {
    let db = Database::load();
    for chr in [Character::Vahn, Character::Noa, Character::Gala] {
        let count = db
            .arts_for(chr)
            .filter(|a| a.kind == ArtKind::Miracle)
            .count();
        assert_eq!(count, 1, "{chr:?} should have exactly 1 miracle art");
    }
}

#[test]
fn magic_split_seru_vs_ra_seru() {
    let db = Database::load();
    let seru = db
        .spells()
        .iter()
        .filter(|s| s.family == SpellFamily::Seru)
        .count();
    let ra_seru = db
        .spells()
        .iter()
        .filter(|s| s.family == SpellFamily::RaSeru)
        .count();
    assert_eq!(seru, 21);
    assert_eq!(ra_seru, 8);
}

#[test]
fn magic_elements_canonical() {
    let db = Database::load();
    let allowed = [
        "fire", "water", "earth", "wind", "thunder", "light", "dark", "evil",
    ];
    for sp in db.spells() {
        assert!(
            allowed.contains(&sp.element.as_str()),
            "spell {:?} has unexpected element {:?}",
            sp.name,
            sp.element
        );
    }
}

#[test]
fn shop_inventory_keys_resolve() {
    let db = Database::load();
    for shop in db.shops() {
        for key in shop.inventory.iter().chain(shop.featured.iter()) {
            assert!(
                db.resolve_key(key).is_some(),
                "shop {:?} (town {:?}) references unknown item key {:?}",
                shop.name
                    .as_deref()
                    .or(shop.merchant.as_deref())
                    .unwrap_or("?"),
                shop.town,
                key,
            );
        }
    }
}

#[test]
fn casino_and_fishing_item_keys_resolve() {
    let db = Database::load();
    for prize in db.slot_prizes() {
        assert!(
            db.resolve_key(&prize.item).is_some(),
            "slot prize {:?} item {:?} not found",
            prize.location,
            prize.item
        );
    }
    for prize in db.fishing_prizes() {
        assert!(
            db.resolve_key(&prize.item).is_some(),
            "fishing prize at {:?} item {:?} not found",
            prize.location,
            prize.item
        );
    }
    for course in db.muscle_dome() {
        if let Some(ref bonus) = course.reward_first_clear {
            assert!(
                db.resolve_key(bonus).is_some(),
                "muscle dome {:?} bonus {:?} not found",
                course.name,
                bonus
            );
        }
    }
}

#[test]
fn enemy_drop_and_steal_keys_resolve() {
    let db = Database::load();
    for enemy in db.enemies() {
        if let Some(ref k) = enemy.drop {
            assert!(
                db.resolve_key(k).is_some(),
                "enemy {:?} drop key {:?} not found",
                enemy.name,
                k
            );
        }
        if let Some(ref k) = enemy.steal {
            assert!(
                db.resolve_key(k).is_some(),
                "enemy {:?} steal key {:?} not found",
                enemy.name,
                k
            );
        }
    }
}

#[test]
fn enemy_stat_coverage_is_universal() {
    // Meth962 ingest landed stats for every enemy; the test gates that we
    // don't regress that coverage as new entries get added.
    let db = Database::load();
    let mut missing: Vec<&str> = Vec::new();
    for enemy in db.enemies() {
        if enemy.hp.is_none() || enemy.exp.is_none() || enemy.atk.is_none() {
            missing.push(&enemy.name);
        }
    }
    assert!(
        missing.is_empty(),
        "enemies without hp/exp/atk: {:?}",
        missing
    );
}

#[test]
fn enemy_stat_ranges_are_plausible() {
    // Ground sanity: HP/MP/AGL within Meth962's observed bounds. Lapis has
    // the highest stats in the corpus; the upper bounds here are well
    // beyond his to flag obvious paste errors without false positives.
    let db = Database::load();
    for enemy in db.enemies() {
        if let Some(hp) = enemy.hp {
            assert!(hp <= 70_000, "{:?} hp out of range: {}", enemy.name, hp);
        }
        if let Some(mp) = enemy.mp {
            assert!(mp <= 5_000, "{:?} mp out of range: {}", enemy.name, mp);
        }
        if let Some(agl) = enemy.agl {
            assert!(agl <= 1_000, "{:?} agl out of range: {}", enemy.name, agl);
        }
    }
}

#[test]
fn weapons_have_consistent_user() {
    let db = Database::load();
    for w in db.weapons() {
        assert!(
            ["Vahn", "Noa", "Gala"].contains(&w.equip_best.as_str()),
            "weapon {:?} equip_best {:?}",
            w.name,
            w.equip_best
        );
        for other in &w.equip_others {
            assert!(
                ["Vahn", "Noa", "Gala"].contains(&other.as_str()),
                "weapon {:?} equip_others {:?}",
                w.name,
                other
            );
        }
    }
}

#[test]
fn armor_equip_field_is_canonical() {
    let db = Database::load();
    for a in db.armor() {
        assert!(
            ["Vahn", "Noa", "Gala", "None"].contains(&a.equip.as_str()),
            "armor {:?} equip {:?}",
            a.name,
            a.equip
        );
        assert!(
            ["armor", "helmet", "shoes"].contains(&a.slot.as_str()),
            "armor {:?} slot {:?}",
            a.name,
            a.slot
        );
    }
}

#[test]
fn item_categories_canonical() {
    let db = Database::load();
    let allowed = [
        "consumable",
        "permanent_stat",
        "key",
        "art_book",
        "fishing_lure",
    ];
    for it in db.items() {
        assert!(
            allowed.contains(&it.category.as_str()),
            "item {:?} category {:?}",
            it.name,
            it.category
        );
    }
}

#[test]
fn known_lookups_smoke() {
    let db = Database::load();
    // Vahn 0x1B = Vahn's Craze = miracle = 99 AP
    let craze = db.find_art_by_name("Vahn's Craze").unwrap();
    assert_eq!(craze.ap, 99);
    assert_eq!(craze.kind, ArtKind::Miracle);

    // Spell elements
    assert_eq!(db.spell_by_name("Vera").unwrap().element, "light");
    assert_eq!(db.spell_by_name("Gimard").unwrap().element, "fire");

    // Shop resolve
    let entries = db.shop_inventory("Sol", "Items Shop II (Bakery)").unwrap();
    let life_ring = entries.iter().find(|e| e.key == "life_ring").unwrap();
    assert_eq!(life_ring.category, ShopEntryCategory::Accessory);
    assert_eq!(life_ring.price, Some(9500));
}
