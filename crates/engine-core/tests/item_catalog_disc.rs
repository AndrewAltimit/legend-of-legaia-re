//! Disc-gated: the default consumable [`ItemCatalog`] is keyed by **real**
//! retail item ids - every entry's id resolves to its entry name in the
//! `SCUS_942.54` item table ([`legaia_asset::item_names`]).
//!
//! This is the guard against the prior bug where the catalog used fabricated
//! sequential ids (`0x01..`) that collide with the table's internal
//! `Ra-Seru Meta $N` placeholders, so a live granted id (e.g. Healing Leaf
//! `0x77`) never matched a catalog effect. Skips without `LEGAIA_DISC_BIN`.

use legaia_asset::item_effect::{ItemEffectTable, RestoreAmount};
use legaia_engine_core::Vfs;
use legaia_engine_core::items::{ItemCatalog, ItemEffect, ItemOutcome};
use legaia_engine_core::world::World;
use std::path::PathBuf;

#[test]
fn vanilla_catalog_ids_match_the_real_item_table() {
    let Some(path) = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return;
    }
    let scus = legaia_engine_core::DiscVfs::open(&path)
        .expect("open disc")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let names =
        legaia_asset::item_names::ItemNameTable::from_scus(&scus).expect("item table parses");

    let catalog = ItemCatalog::vanilla();
    assert!(catalog.len() >= 10, "catalog is populated");
    for entry in catalog.iter() {
        let real = names
            .name(entry.id)
            .unwrap_or_else(|| panic!("catalog id {:#04x} names no real item", entry.id));
        assert_eq!(
            real, entry.name,
            "catalog id {:#04x} should be {:?}, the item table says {:?}",
            entry.id, entry.name, real
        );
    }

    // Anchor: the real Healing Leaf is id 0x77 (not the old fabricated 0x01).
    assert_eq!(names.name(0x77), Some("Healing Leaf"));
    assert!(
        catalog.get(0x77).is_some(),
        "the catalog keys Healing Leaf at its real id 0x77"
    );
    assert!(
        catalog.get(0x01).is_none(),
        "0x01 (Ra-Seru Meta $1 placeholder) is not a consumable in the catalog"
    );
}

/// The curated HP/MP restore amounts are byte-confirmed against the on-disc
/// heal-amount table (`0x8007655C`) the apply handler `FUN_800402F4` reads -
/// so the engine's numbers are disc-faithful, not just walkthrough-sourced.
/// Skips without `LEGAIA_DISC_BIN`.
#[test]
fn curated_heal_amounts_match_the_disc_heal_table() {
    let Some(path) = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return;
    }
    let scus = legaia_engine_core::DiscVfs::open(&path)
        .expect("open disc")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let table = ItemEffectTable::from_scus(&scus).expect("item-effect table parses");

    let catalog = ItemCatalog::vanilla();
    // For every flat-heal catalog entry, the curated amount must equal what the
    // disc apply handler would restore for that item id.
    let mut checked = 0;
    for entry in catalog.iter() {
        match (entry.effect, table.restore_amount(entry.id)) {
            (ItemEffect::Heal { amount }, Some(RestoreAmount::Hp(disc))) => {
                assert_eq!(
                    amount, disc,
                    "{:#04x} {:?}: curated HP heal {} != disc {}",
                    entry.id, entry.name, amount, disc
                );
                checked += 1;
            }
            (ItemEffect::HealMp { amount }, Some(RestoreAmount::Mp(disc))) => {
                assert_eq!(
                    amount, disc,
                    "{:#04x} {:?}: curated MP heal {} != disc {}",
                    entry.id, entry.name, amount, disc
                );
                checked += 1;
            }
            // HealAll is the disc's tier-2 full restore (9999); other effect
            // shapes (cure/revive/buff/spirit/escape) aren't flat amounts.
            (ItemEffect::HealAll, Some(RestoreAmount::Hp(disc))) => {
                assert_eq!(
                    disc, 9999,
                    "{:#04x} HealAll should be the 9999 tier",
                    entry.id
                );
                checked += 1;
            }
            _ => {}
        }
    }
    assert!(checked >= 5, "cross-checked too few heal items ({checked})");

    // Spot anchors keyed by real id.
    assert_eq!(table.restore_amount(0x77), Some(RestoreAmount::Hp(200))); // Healing Leaf
    assert_eq!(table.restore_amount(0x7C), Some(RestoreAmount::Mp(50))); // Magic Leaf
}

/// Installing the on-disc item-effect table seeds the permanent stat-up *Water*
/// line into the catalog (it is absent on disc-free builds), and using one
/// actually raises the target's stat by the handler's decoded amount. The
/// items the engine catalog previously omitted for lack of a buff taxonomy.
/// Skips without `LEGAIA_DISC_BIN`.
#[test]
fn water_line_stat_up_items_seed_and_apply_from_disc() {
    let Some(path) = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return;
    }
    let scus = legaia_engine_core::DiscVfs::open(&path)
        .expect("open disc")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let table = ItemEffectTable::from_scus(&scus).expect("item-effect table parses");

    // A disc-free vanilla catalog does NOT offer the Water line (no no-ops).
    assert!(
        ItemCatalog::vanilla().get(0x82).is_none(),
        "Life Water is omitted until the disc table is installed"
    );

    // One-member party with known stats to watch the raise land.
    let mut party = legaia_save::Party::zeroed(1);
    let mut hms = party.members[0].hp_mp_sp();
    hms.hp_cur = 50;
    hms.hp_max = 100;
    party.members[0].set_hp_mp_sp(hms);
    let mut ls = party.members[0].live_stats();
    ls.atk = 20;
    ls.spd = 10;
    ls.int = 12;
    party.members[0].set_live_stats(ls);

    let mut world = World::new();
    world.load_party(party);
    world.set_item_effects(table); // seeds the Water line onto the catalog

    // Life Water is now offered: field-only, not battle.
    let life = world.item_catalog.get(0x82).expect("Life Water seeded");
    assert_eq!(life.name, "Life Water");
    assert!(life.usable_in_field && !life.usable_in_battle);

    // Life Water (tier 0): Max HP +16, current HP refilled by the gain.
    let outcome = world.use_item(0x82, 0);
    assert_eq!(outcome, ItemOutcome::StatsRaised { count: 1 });
    assert_eq!(world.roster.members[0].hp_mp_sp().hp_max, 116);
    assert_eq!(world.actors[0].battle.max_hp, 116);

    // Power Water (tier 1): ATK +4.
    assert_eq!(
        world.use_item(0x83, 0),
        ItemOutcome::StatsRaised { count: 1 }
    );
    assert_eq!(world.roster.members[0].live_stats().atk, 24);

    // Swift Water (tier 3): SPD +4; Wisdom Water (tier 4): INT +4.
    world.use_item(0x85, 0);
    world.use_item(0x86, 0);
    assert_eq!(world.roster.members[0].live_stats().spd, 14);
    assert_eq!(world.roster.members[0].live_stats().int, 16);

    // Honey (tier 6, all stats): Defence expands to both facets, so the seven
    // record changes become eight individual raises.
    assert_eq!(
        world.use_item(0x65, 0),
        ItemOutcome::StatsRaised { count: 8 }
    );
}

/// Installing the disc table also seeds the one-battle stat-buff Elixirs (class
/// 7, battle-only); using one ramps the target's battle-actor stat scalar by
/// ×6/5 for the battle through the shared buff path (the same machinery as buff
/// spells). Skips without `LEGAIA_DISC_BIN`.
#[test]
fn elixir_battle_buffs_seed_and_ramp_from_disc() {
    let Some(path) = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return;
    }
    let scus = legaia_engine_core::DiscVfs::open(&path)
        .expect("open disc")
        .read("SCUS_942.54")
        .expect("SCUS_942.54 present");
    let table = ItemEffectTable::from_scus(&scus).expect("item-effect table parses");

    // Disc-free vanilla never offers the battle-only buff Elixirs.
    assert!(
        ItemCatalog::vanilla().get(0x8B).is_none(),
        "Power Elixir is omitted until the disc table is installed"
    );

    let mut world = World::new();
    world.set_item_effects(table); // seeds the Elixirs onto the catalog
    world.set_battle_attack(0, 100);
    world.set_battle_defense(0, 50);

    // Power Elixir is battle-only and ramps ATK ×6/5: 100 -> 120.
    let pe = world.item_catalog.get(0x8B).expect("Power Elixir seeded");
    assert_eq!(pe.name, "Power Elixir");
    assert!(pe.usable_in_battle && !pe.usable_in_field);
    assert_eq!(world.use_item(0x8B, 0), ItemOutcome::Buffed { count: 1 });
    assert_eq!(world.battle_attack[0], 120);
    assert_eq!(world.battle_buffs.len(), 1);

    // Shield Elixir ramps DEF ×6/5: 50 -> 60.
    assert_eq!(world.use_item(0x8C, 0), ItemOutcome::Buffed { count: 1 });
    assert_eq!(world.battle_defense[0], 60);

    // Wonder Elixir buffs all four (SPD/DEF/ATK/AGL). ATK + DEF refresh (revert
    // the prior delta, re-ramp from base, no compounding), SPD + AGL are new but
    // have no live scalar; the buff list ends with four distinct (slot, stat)
    // trackers.
    assert_eq!(world.use_item(0x8E, 0), ItemOutcome::Buffed { count: 4 });
    assert_eq!(
        world.battle_attack[0], 120,
        "ATK refreshed from base, not compounded"
    );
    assert_eq!(world.battle_defense[0], 60);
    assert_eq!(world.battle_buffs.len(), 4);
}
