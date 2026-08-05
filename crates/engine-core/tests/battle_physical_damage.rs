//! Regression: a physical swing must run the retail melee roll pair, not a
//! flat `attack - defense`.
//!
//! The port used to resolve every basic strike with
//! `art_strike_damage_default(attack, defense, 16)` - `attack - defense`,
//! floored at **1**. Retail's melee routine `FUN_801EC3E4` rolls the attacker's
//! ATK against the defender's UDF/LDF and, when the attack roll fails to clear
//! the guard roll, *rewrites* the attack roll to `guard + 3/4` of itself rather
//! than flooring it. The difference is not cosmetic: real disc enemies carry
//! defence values well above a party member's attack for most of the game, so
//! under the old model an ordinary fight resolved at one point of damage per
//! swing.
//!
//! The disc-gated half measures that against the real monster archive; the
//! disc-free half pins the same property on synthetic stats and skips nothing.

use std::path::PathBuf;

use legaia_engine_core::monster_catalog::{
    FormationDef, FormationSlot, FormationTable, MonsterCatalog, MonsterDef,
    catalog_from_monster_archive, monster_def_from_record,
};
use legaia_engine_core::world::{Actor, SceneMode, World};

/// A three-member party at fresh-game-shaped stats, seated in a battle
/// against `formation` and auto-resolving (no command menu).
fn world_against(table: FormationTable, catalog: MonsterCatalog, atk: u16, row: u8) -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    let mut party = w.roster.clone();
    for rec in party.members.iter_mut() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = 180;
        hms.hp_max = 180;
        rec.set_hp_mp_sp(hms);
    }
    w.load_party(party);
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 180;
        w.actors[i].battle.max_hp = 180;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, atk);
        w.set_battle_defense(i as u8, 16);
    }
    w.set_formation_table(table, catalog);
    w.mode = SceneMode::Field;
    assert!(
        w.trigger_scripted_battle(row),
        "formation row {row} registers"
    );
    // The scripted entry runs the field-to-battle intro transition (132
    // display frames) before the mode flips.
    for _ in 0..200 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);
    w
}

/// Drive the battle and report `(turns_observed, damage_of_the_first_landed
/// party swing, resolved)`.
fn run_to_resolution(w: &mut World, max_frames: usize) -> (u32, u16, bool) {
    let mut first_hit = 0u16;
    let mut swings = 0u32;
    for _ in 0..max_frames {
        w.tick();
        for fx in w.drain_battle_hit_fx() {
            if !fx.is_heal && fx.target_slot >= w.party_count {
                swings += 1;
                if first_hit == 0 {
                    first_hit = fx.amount;
                }
            }
        }
        if w.mode != SceneMode::Battle {
            return (swings, first_hit, true);
        }
    }
    (swings, first_hit, false)
}

/// Disc-free: an attacker whose ATK is *under* the defender's defence still
/// lands a real hit, and the fight terminates in a sane number of swings.
#[test]
fn an_outmatched_attacker_still_lands_scaling_damage() {
    let mut cat = MonsterCatalog::new();
    // The shape of a real early-game enemy after the battle-load defence
    // boost: modest HP, defence well above a starting party member's attack.
    let mut def = MonsterDef::new(1, "Armoured", 82, 34);
    def.udf = 48;
    def.ldf = 56;
    def.exp = 50;
    def.gold = 44;
    cat.insert(def);
    let mut table = FormationTable::new();
    table.insert(FormationDef::new(1, vec![FormationSlot::new(1)]));

    let mut w = world_against(table, cat, 24, 1);
    let (swings, first_hit, resolved) = run_to_resolution(&mut w, 60_000);
    assert!(resolved, "the battle must resolve");
    assert!(
        first_hit >= 3,
        "the melee rewrite floors the underdog swing above the old 1-damage \
         chip; got {first_hit}"
    );
    // 82 HP at the old one-point chip needed 82 landed swings. The rewrite
    // brings that inside a handful of rounds.
    assert!(
        swings <= 40,
        "an 82 HP enemy should not take {swings} swings to fell"
    );
}

/// The other half of the same defect: melee used to be gated on the
/// selector-9 accuracy roll, which retail's melee routine never performs. With
/// the engine's own stat seeding - a party slot's accuracy/evasion from AGL
/// (~100 at level one), a monster's from its record INT (~12 in the opening
/// bestiary) - that gate let enemies land roughly one swing in nine.
#[test]
fn a_monster_swing_is_not_whiffed_by_the_accuracy_gate() {
    let mut cat = MonsterCatalog::new();
    let mut def = MonsterDef::new(1, "Biter", 4000, 60);
    def.udf = 20;
    def.ldf = 20;
    // Real early-bestiary shape: INT (= the engine's accuracy/evasion seed) is
    // an order of magnitude under a party member's AGL.
    def.accuracy = 12;
    def.evasion = 12;
    cat.insert(def);
    let mut table = FormationTable::new();
    table.insert(FormationDef::new(1, vec![FormationSlot::new(1)]));

    let mut w = world_against(table, cat, 24, 1);
    for slot in 0..3usize {
        w.battle_accuracy[slot] = 100;
        w.battle_evasion[slot] = 100;
    }
    let mut monster_swings = 0u32;
    for _ in 0..40_000 {
        w.tick();
        for fx in w.drain_battle_hit_fx() {
            if !fx.is_heal && fx.target_slot < w.party_count {
                monster_swings += 1;
            }
        }
        if w.mode != SceneMode::Battle {
            break;
        }
    }
    assert!(
        monster_swings >= 8,
        "the enemy landed only {monster_swings} swings across the fight - the \
         accuracy gate is back"
    );
}

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() {
            return Some(d);
        }
    }
    None
}

/// Disc-gated: the same property against the **real** monster archive. Picks
/// the lightest enemies on the disc (the ones a starting party meets) and
/// requires each fight to resolve in a plausible number of swings.
#[test]
fn a_starting_party_can_fell_a_real_early_enemy() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut archive =
        legaia_prot::archive::Archive::open(&extracted.join("PROT.DAT")).expect("open PROT.DAT");
    let entry = archive.entries[867].clone();
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 867");

    // The lowest-HP real records - the ones the opening areas field.
    let ids: Vec<u16> = (1..=40u16)
        .filter(|&id| {
            legaia_asset::monster_archive::record(&bytes, id)
                .ok()
                .flatten()
                .map(|r| monster_def_from_record(&r))
                .is_some_and(|d| d.hp > 0 && d.hp <= 100)
        })
        .collect();
    assert!(!ids.is_empty(), "the archive carries low-HP early records");

    let catalog = catalog_from_monster_archive(&bytes, &ids);
    for &id in ids.iter().take(4) {
        let def = catalog.get(id).expect("catalog entry").clone();
        let mut table = FormationTable::new();
        table.insert(FormationDef::new(1, vec![FormationSlot::new(id)]));
        // Vahn's new-game ATK is 24 (`0x80078C4C` starting-party template);
        // the starting weapon adds to it, so 24 is the floor of the range.
        let mut w = world_against(table, catalog.clone(), 24, 1);
        let (swings, first_hit, resolved) = run_to_resolution(&mut w, 200_000);
        eprintln!(
            "[melee] {} hp={} udf={} ldf={} -> first hit {first_hit}, {swings} swings, resolved={resolved}",
            def.name, def.hp, def.udf, def.ldf
        );
        assert!(resolved, "{} fight must resolve", def.name);
        assert!(
            first_hit >= 3,
            "{}: a starting swing chipped for {first_hit}",
            def.name
        );
        assert!(
            swings <= 60,
            "{}: {swings} swings to fell {} HP reads as the old 1-damage model",
            def.name,
            def.hp
        );
    }
}
