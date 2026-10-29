//! Expansion battle waves on a disc-free world: a waved formation seats each
//! wave into the freed monster seats of the same battle, raises its banner,
//! and ends in one victory that pays every wave's spoils; a retail fight
//! with a waved pack active plays exactly as without one.

use std::sync::Arc;

use super::*;
use legaia_engine_battle::monster_catalog::{
    FormationDef, FormationSlot, MonsterCatalog, MonsterDef,
};
use legaia_engine_mode::mode::SceneMode;
use legaia_expansion_format::{ExpansionPack, PackFiles};

const PACK_TOML: &str = r#"
id = "wavepack"
name = "Wave Pack"
version = "0.1.0"
"#;

/// Three waves: one Dire Wolf; then a Dire Wolf and a Rat together; then two
/// Rats one at a time, never more than one standing.
const BATTLE_TOML: &str = r#"
[monsters.dire_wolf]
base = { kind = "monster", id = 9 }
name = "Dire Wolf"
exp = 1000
gold = 400

[formations.gauntlet]
label = "Gauntlet"
monsters = ["dire_wolf"]

[[formations.gauntlet.waves]]
monsters = ["dire_wolf", { kind = "monster", id = 10 }]

[[formations.gauntlet.waves]]
monsters = [{ kind = "monster", id = 10 }]
arrival = "refill"
count = 2
max_standing = 1

[formations.single]
monsters = ["dire_wolf"]
"#;

fn pack() -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert("pack.toml".into(), PACK_TOML.as_bytes().to_vec());
    files.insert("battle.toml".into(), BATTLE_TOML.as_bytes().to_vec());
    Arc::new(ExpansionPack::from_files(&files).expect("synthetic pack parses"))
}

/// One capable party member against a catalog holding a Wolf (9) and a Rat
/// (10), with retail's formation row `1` (one Wolf) registered.
fn world() -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Field;
    world.actors[0].active = true;
    world.actors[0].battle.hp = 900;
    world.actors[0].battle.max_hp = 900;
    world.actors[0].battle.liveness = 1;
    world.battle.set_battle_attack(0, 80);
    let mut cat = MonsterCatalog::new();
    let mut wolf = MonsterDef::new(9, "Wolf", 35, 12);
    wolf.exp = 20;
    wolf.gold = 40;
    cat.insert(wolf);
    let mut rat = MonsterDef::new(10, "Rat", 20, 6);
    rat.exp = 30;
    rat.gold = 50;
    cat.insert(rat);
    world.set_monster_catalog(cat);
    world
        .tables
        .formation_table
        .insert(FormationDef::new(1, vec![FormationSlot::new(9)]));
    world.rng_state = 0x0BAD_5EED;
    world
}

/// What one fight shows, frame by frame.
#[derive(Default)]
struct Watch {
    /// `(actor slot, monster id)` each time a monster seat comes to life.
    arrivals: Vec<(usize, u16)>,
    banners: Vec<String>,
    victories: usize,
}

fn fight(w: &mut World) -> Watch {
    for _ in 0..400 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle, "the fight starts");
    let mut watch = Watch::default();
    let mut alive = [false; 8];
    let mut in_victory = false;
    for _ in 0..30_000 {
        for (s, was) in alive.iter_mut().enumerate().skip(1) {
            let a = &w.actors[s];
            let now = a.active && a.battle.liveness != 0 && a.battle_monster_id.is_some();
            if now && !*was {
                watch.arrivals.push((s, a.battle_monster_id.unwrap()));
            }
            *was = now;
        }
        if let Some(b) = &w.battle.message_banner
            && b.text.starts_with("Wave")
            && watch.banners.last() != Some(&b.text)
        {
            watch.banners.push(b.text.clone());
        }
        let v = w.battle.victory.is_some();
        if v && !in_victory {
            watch.victories += 1;
        }
        in_victory = v;
        w.tick();
        if w.mode != SceneMode::Battle {
            return watch;
        }
    }
    panic!("the fight never ended");
}

#[test]
fn waves_seat_in_the_same_battle_and_pay_one_summed_victory() {
    let mut w = world();
    w.activate_expansion(pack()).expect("pack activates");
    assert!(w.start_expansion_battle("gauntlet"));
    let money = w.party.money;
    let watch = fight(&mut w);
    assert_eq!(watch.victories, 1, "one results screen");
    // Wave 1's wolf, wave 2's pair, then wave 3's two rats, one at a time.
    let ids: Vec<u16> = watch.arrivals.iter().map(|&(_, id)| id).collect();
    assert_eq!(ids, [9, 9, 10, 10, 10], "{:?}", watch.arrivals);
    assert_eq!(watch.banners, ["Wave 2 / 3", "Wave 3 / 3"]);
    // Every wave's EXP and gold, summed once.
    let rewards = w.battle.last_rewards.clone().expect("the party wins");
    assert_eq!(rewards.xp, 1000 + 1000 + 30 * 3);
    let acc = [400u16, 400, 50, 50, 50]
        .iter()
        .map(|&g| legaia_engine_battle_vm::battle_formulas::victory_gold_per_monster(g))
        .sum();
    let gold = legaia_engine_battle_vm::battle_formulas::victory_gold_finalize(acc, false);
    assert_eq!(rewards.gold, gold);
    assert_eq!(w.party.money, money + gold as i32);
    // The scene's Wolf is back and the run is gone.
    assert_eq!(w.tables.monster_catalog.get(9).unwrap().name, "Wolf");
    assert!(w.expansion.battle.shadowed.is_empty());
    assert!(w.expansion.battle.waves.is_none());
    assert!(w.battle.active_formation.is_none());
}

#[test]
fn a_wave_seat_takes_the_catalog_stats_of_its_monster() {
    let mut w = world();
    w.activate_expansion(pack()).unwrap();
    assert!(w.start_expansion_battle("gauntlet"));
    for _ in 0..400 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    // Down the first wave by hand and let the wipe scan run.
    w.actors[1].battle.hp = 0;
    w.actors[1].battle.liveness = 0;
    assert!(w.expansion_seat_wave(legaia_engine_expansion::battle_waves::Moment::Wipe));
    assert_eq!(w.take_expansion_reseated_monsters(), [1, 2]);
    let rat = &w.actors[2];
    assert_eq!(rat.battle_monster_id, Some(10));
    assert_eq!((rat.battle.hp, rat.battle.max_hp), (20, 20));
    assert!(rat.active && rat.battle.liveness == 1);
    let wolf = &w.actors[1];
    assert_eq!(wolf.battle_monster_id, Some(9));
    assert_eq!(wolf.battle.hp, 35);
    // The two seats of the new layout.
    let seat = |k| legaia_engine_battle::battle_seats::monster_seat(2, k, false);
    assert_eq!(w.actors[2].move_state.world_x, seat(1).x);
    assert_eq!(w.actors[1].move_state.world_z, seat(0).z);
    // The fight's formation now spans both waves.
    let ids: Vec<u16> = w
        .battle
        .active_formation
        .as_ref()
        .unwrap()
        .slots
        .iter()
        .map(|s| s.monster_id)
        .collect();
    assert_eq!(ids, [9, 9, 10]);
    // With monsters standing, neither moment seats anything more.
    assert!(!w.expansion_seat_wave(legaia_engine_expansion::battle_waves::Moment::Wipe));
}

#[test]
fn a_single_wave_formation_holds_no_wave_run() {
    let mut w = world();
    w.activate_expansion(pack()).unwrap();
    assert!(w.start_expansion_battle("single"));
    for _ in 0..400 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert!(w.expansion.battle.waves.is_none());
    let watch = fight(&mut w);
    assert_eq!(watch.victories, 1);
    assert!(watch.banners.is_empty());
    assert_eq!(w.battle.last_rewards.as_ref().unwrap().xp, 1000);
}

/// What a retail fight leaves behind, for comparing worlds.
fn retail_fight(w: &mut World) -> (u32, u8, i32, u32) {
    assert!(w.force_encounter(1));
    let watch = fight(w);
    assert!(watch.banners.is_empty());
    (
        w.party.level_up_tracker.xp[0],
        w.party.level_up_tracker.level[0],
        w.party.money,
        w.rng_state,
    )
}

#[test]
fn a_retail_fight_beside_a_waved_pack_plays_as_without_one() {
    let mut plain = world();
    let retail = retail_fight(&mut plain);
    let mut active = world();
    active.activate_expansion(pack()).unwrap();
    assert_eq!(retail_fight(&mut active), retail);
}

/// Two rats, then one more arriving into the first freed seat. The member
/// whose strike downs the second rat ends its action on a wipe the refill
/// answers: the action SM must idle at `EndOfAction` for the turn cycling
/// (the next pick by initiative, or the next round), never step on to
/// `PreActionWait`, which re-seeds that member's spent action at the corpse
/// it just killed - an approach (`0x19`) that never comes within reach.
#[test]
fn a_refill_on_the_wipe_hands_the_turn_back_to_the_turn_cycling() {
    use legaia_engine_battle_vm::battle_action::ActionState;
    let battle = r#"
[formations.refill]
monsters = [{ kind = "monster", id = 10 }, { kind = "monster", id = 10 }]

[[formations.refill.waves]]
monsters = [{ kind = "monster", id = 10 }]
arrival = "refill"
count = 2
max_standing = 1
"#;
    let mut files = PackFiles::new();
    files.insert("pack.toml".into(), PACK_TOML.as_bytes().to_vec());
    files.insert("battle.toml".into(), battle.as_bytes().to_vec());
    let pack = Arc::new(ExpansionPack::from_files(&files).unwrap());
    let mut w = world();
    w.activate_expansion(pack).unwrap();
    assert!(w.start_expansion_battle("refill"));
    for _ in 0..400 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);
    let end = ActionState::EndOfAction.as_byte();
    let wait = ActionState::PreActionWait.as_byte();
    let mut prev = w.battle_ctx.action_state;
    let mut reseats = 0;
    for tick in 0..30_000 {
        w.tick();
        if w.mode != SceneMode::Battle {
            assert!(reseats >= 2, "both refills arrived ({reseats})");
            return;
        }
        reseats += w.take_expansion_reseated_monsters().len();
        let now = w.battle_ctx.action_state;
        assert!(
            !(prev == end && now == wait),
            "tick {tick}: EndOfAction stepped to PreActionWait (a stale re-seed)"
        );
        prev = now;
    }
    panic!("the fight never ended");
}
