//! Regressions for the two turn-order defects at battle entry.
//!
//! Both fail against the pre-fix engine:
//!
//! - **Round 1 ignored initiative.** Battle setup hand-armed slot 0 and the
//!   seeder consumed slot 0's key, so slot 0 opened every battle in the game
//!   regardless of SPD - the party's fastest member could not lead, no monster
//!   could ever act first, and a rolled back attack could not cash in.
//! - **Party `battle_speed` dropped equipment SPD.** The aggregator resolved it
//!   into `BattleStats::spd` and the caller threw it away.

use legaia_engine_core::monster_catalog::{
    FormationDef, FormationSlot, FormationTable, MonsterCatalog, MonsterDef,
};
use legaia_engine_core::world::{Actor, SceneMode, World};

/// A three-member party seated in battle against one monster from `catalog`,
/// with each party slot's SPD set to `party_spd`.
fn battle_with(monster: MonsterDef, party_spd: u16) -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 200;
        w.actors[i].battle.max_hp = 200;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 40);
        w.battle_speed[i] = party_spd;
    }
    let mut catalog = MonsterCatalog::new();
    let id = monster.id;
    catalog.insert(monster);
    let mut table = FormationTable::new();
    table.insert(FormationDef::new(1, vec![FormationSlot::new(id)]));
    w.set_formation_table(table, catalog);
    w.mode = SceneMode::Field;
    assert!(w.trigger_scripted_battle(1), "formation row 1 registers");
    // The scripted entry runs the field-to-battle intro transition
    // (132 display frames) before the mode flips.
    for _ in 0..200 {
        if w.mode == SceneMode::Battle {
            break;
        }
        w.tick();
    }
    assert_eq!(w.mode, SceneMode::Battle);
    w
}

/// Round 1's opener is the highest initiative key, not slot 0.
///
/// Retail's next-actor selector `FUN_801DABA4` picks the max key from the
/// battle's first turn onward. Here the monster is far faster than the whole
/// party, so it must open.
#[test]
fn round_one_opens_on_the_fastest_combatant_not_slot_zero() {
    let mut speedy = MonsterDef::new(1, "Speedy", 400, 20);
    speedy.speed = 400;
    let w = battle_with(speedy, 5);
    assert_eq!(
        w.battle_ctx.active_actor, w.party_count,
        "the fast monster must take the opening turn; slot 0 opening again \
         means the seeder is consuming its key",
    );
}

/// The mirror: a party far faster than the monster still opens on a party slot,
/// so the fix is a real max-key pick and not a blanket hand-off to the enemy.
#[test]
fn a_fast_party_still_opens_against_a_slow_monster() {
    let mut sluggish = MonsterDef::new(1, "Sluggish", 400, 20);
    sluggish.speed = 3;
    let w = battle_with(sluggish, 300);
    assert!(
        w.battle_ctx.active_actor < w.party_count,
        "a party 100x the monster's SPD must open, got slot {}",
        w.battle_ctx.active_actor
    );
}

/// A battle with no SPD anywhere keeps the round-robin fallback and still opens
/// on slot 0. Without the `any_battle_speed` guard the initiative pick would
/// fall through to `next_living_combatant(0)` and silently hand the opening
/// turn to slot 1.
#[test]
fn a_battle_without_speed_still_opens_on_slot_zero() {
    let w = battle_with(MonsterDef::new(1, "Mob", 100, 10), 0);
    assert!(
        w.battle_speed.iter().all(|&s| s == 0),
        "this setup leaves every SPD at 0"
    );
    assert_eq!(w.battle_ctx.active_actor, 0);
}

/// The seeder leaves **every** living slot armed - consuming slot 0's key is
/// exactly what let slot 0 open every battle.
#[test]
fn setup_seeding_consumes_no_initiative_key() {
    let mut fast = MonsterDef::new(1, "Fast", 400, 20);
    fast.speed = 90;
    let w = battle_with(fast, 40);
    // Three party slots plus one monster; the opener's key is consumed by the
    // pick itself, so exactly one living slot may be at zero.
    let armed = (0..4)
        .filter(|&i| w.actors[i].battle.liveness != 0 && w.actors[i].battle.init_key > 0)
        .count();
    assert_eq!(
        armed, 3,
        "seeding must arm all four and the opener's pick consume exactly one"
    );
}

/// The party's resolved SPD - base plus the equipment table's footwear bonus -
/// reaches `battle_speed`.
///
/// `seed_party_battle_stats` computed `base_spd`, folded it through the
/// aggregator into `BattleStats::spd`, and then never wrote it anywhere. The
/// slot kept the raw `live_stats().spd` that `load_party` seeded, so every SPD
/// point a party member's gear granted was invisible to turn order, the
/// formation-advantage roll and the escape roll - all three of which read
/// `battle_speed`.
#[test]
fn party_battle_speed_keeps_the_resolved_spd() {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 1;
    let mut party = legaia_save::Party::zeroed(1);
    let rec = &mut party.members[0];
    let mut live = rec.live_stats();
    live.atk = 30; // non-zero, so the seeder does not skip the slot
    live.spd = 12;
    live.agl = 20;
    live.udf = 10;
    live.ldf = 10;
    rec.set_live_stats(live);
    w.load_party(party);
    w.actors[0].battle.liveness = 1;

    // Raw record SPD lands first, via `load_party`.
    assert_eq!(w.battle_speed[0], 12);

    // Now resolve through the aggregator. With an empty equipment table the
    // resolved SPD equals the base, so this alone would not distinguish a
    // working write from no write - poison the slot first so only a real write
    // can restore it.
    w.battle_speed[0] = 0;
    w.seed_party_battle_stats();
    assert_eq!(
        w.battle_speed[0], 12,
        "the resolved SPD must be written back to battle_speed"
    );
}
