//! Regressions for the two defects that made a default session unplayable:
//! a battle that could never end, and a battle whose HP never left the fight.
//!
//! **1. Battle entry was ungated; battle driving was not.** A field carrier's
//! scripted fight (`3E FF`) and a world-map region encounter both flipped the
//! world into [`SceneMode::Battle`] regardless of
//! [`World::live_gameplay_loop`], while `World::tick`'s Battle arm only drove
//! the full [`World::live_battle_tick`] when that flag was set - otherwise it
//! ran one bare `step_battle` per frame, which applies no damage, arms no
//! turn and never calls `finish_battle`. A default `play-window` session that
//! walked into a scripted fight therefore entered a battle with no exit.
//! Retail has no "loop enabled" concept: `FUN_801E295C` always drives the
//! battle it is in.
//!
//! **2. Post-battle HP / MP was discarded.** `finish_battle` restored the
//! actor table from the pre-battle snapshot, so every fight ended with the
//! party back at the HP it started with - which also made a party wipe
//! unobservable (you lost, and woke up in the field at full health).
//!
//! Disc-free: synthetic party + the vanilla monster/formation tables.

use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};

/// A world seated for a battle against the vanilla Goblin formation (id 1),
/// with the live loop **off** - the default a bare `World::new()` boots with.
fn world_in_a_battle() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.mp = 40;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 60);
    }
    w.load_party(legaia_save::Party::zeroed(3));
    // `load_party` overwrites the mirrors from the (zeroed) records; put the
    // synthetic combat stats back and keep the records in step.
    let mut party = w.roster.clone();
    for rec in party.members.iter_mut() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = 100;
        hms.hp_max = 100;
        hms.mp_cur = 40;
        rec.set_hp_mp_sp(hms);
    }
    w.load_party(party);
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.mode = SceneMode::Field;
    assert!(
        !w.live_gameplay_loop,
        "the default must stay off - that is what this test is about"
    );
    w
}

/// Enter the battle the way an ungated entry path does (a field carrier's
/// scripted `3E FF`, a world-map region encounter), then tick with the
/// default flags. The battle MUST reach a terminal state.
#[test]
fn a_battle_entered_with_default_flags_still_resolves() {
    let mut w = world_in_a_battle();
    assert!(
        w.trigger_scripted_battle(0) || w.trigger_scripted_battle(1),
        "the vanilla formation table should register a scripted row"
    );
    // One ordinary `tick` in Field mode drains the latched scripted battle
    // through `tick_field_carriers` - no test-side glue.
    w.tick();
    assert_eq!(
        w.mode,
        SceneMode::Battle,
        "the scripted-battle path enters battle regardless of the live-loop flag"
    );

    let mut resolved = false;
    for _ in 0..20_000 {
        w.tick();
        if w.mode != SceneMode::Battle {
            resolved = true;
            break;
        }
    }
    assert!(
        resolved,
        "a battle entered with the live loop off must still be driven to a \
         terminal state - otherwise the session is soft-locked in SceneMode::Battle"
    );
    assert!(
        !w.actors[3..].iter().any(|a| a.battle.liveness != 0),
        "the battle resolved by wiping the monsters"
    );
}

/// The battle's HP / MP must survive the return to the field, in BOTH the
/// live actor mirrors and the roster records a save is written from.
#[test]
fn party_hp_and_mp_survive_the_battle() {
    let mut w = world_in_a_battle();
    assert!(w.trigger_scripted_battle(0) || w.trigger_scripted_battle(1));
    w.tick();
    assert_eq!(w.mode, SceneMode::Battle);

    // Take a chunk out of party slot 0 mid-battle and spend some MP, the way
    // a monster turn and a cast would.
    w.actors[0].battle.hp = 37;
    w.actors[0].battle.mp = 11;

    for _ in 0..20_000 {
        w.tick();
        if w.mode != SceneMode::Battle {
            break;
        }
    }
    assert_ne!(w.mode, SceneMode::Battle, "battle must have resolved");

    assert_eq!(
        w.roster.members[0].hp_mp_sp().hp_cur,
        37,
        "post-battle HP must be persisted into the character record"
    );
    assert_eq!(
        w.roster.members[0].hp_mp_sp().mp_cur,
        11,
        "post-battle MP must be persisted into the character record"
    );
    assert_eq!(
        w.actors[0].battle.hp, 37,
        "the restored field actor must carry the post-battle HP, not the \
         pre-battle snapshot's"
    );
    assert_eq!(w.actors[0].battle.mp, 11, "same for MP");
    // Untouched members keep theirs.
    assert_eq!(w.roster.members[1].hp_mp_sp().hp_cur, 100);
}

/// A party wipe must raise `game_over` (the flag both hosts now read) rather
/// than quietly returning a full-HP party to the field.
#[test]
fn a_party_wipe_raises_game_over_and_leaves_the_party_down() {
    let mut w = world_in_a_battle();
    assert!(w.trigger_scripted_battle(0) || w.trigger_scripted_battle(1));
    w.tick();
    assert_eq!(w.mode, SceneMode::Battle);

    for i in 0..3 {
        w.actors[i].battle.hp = 0;
        w.actors[i].battle.liveness = 0;
    }
    for _ in 0..20_000 {
        w.tick();
        if w.mode != SceneMode::Battle {
            break;
        }
    }
    assert_ne!(
        w.mode,
        SceneMode::Battle,
        "the wipe must resolve the battle"
    );
    assert!(w.game_over, "a party wipe raises game over");
    assert_eq!(
        w.roster.members[0].hp_mp_sp().hp_cur,
        0,
        "losing must not hand the player a healed party"
    );
    assert!(w.last_battle_rewards.is_none(), "a wipe grants no loot");

    // `revive_party_full` is what a host's "Retry" row runs; without it the
    // party would re-wipe on the next encounter.
    w.revive_party_full();
    assert_eq!(w.roster.members[0].hp_mp_sp().hp_cur, 100);
    assert_eq!(w.actors[0].battle.hp, 100);
    assert_eq!(w.actors[0].battle.liveness, 1);
}

/// The spoils panel is armed by a victory and ages out - the reader
/// `last_battle_rewards` never had.
#[test]
fn a_victory_arms_the_spoils_panel() {
    let mut w = world_in_a_battle();
    assert!(w.trigger_scripted_battle(0) || w.trigger_scripted_battle(1));
    w.tick();
    for _ in 0..20_000 {
        w.tick();
        if w.mode != SceneMode::Battle {
            break;
        }
    }
    assert_ne!(w.mode, SceneMode::Battle);
    let banner = w
        .battle_spoils_banner()
        .expect("a victory arms the spoils panel");
    assert!(banner.xp > 0 || banner.gold > 0, "the panel shows the loot");

    for _ in 0..World::SPOILS_BANNER_FRAMES + 2 {
        w.tick();
    }
    assert!(
        w.battle_spoils_banner().is_none(),
        "the panel ages out on its own"
    );
}

/// A **cast** must not park the action SM.
///
/// `MagicSustain` (`0x2B`) holds while the caster's `spell_iter`
/// (`actor+0x1FA`) is non-zero, and the SM only ever *sets* that byte -
/// retail's cast-animation system counts it back down, and the port has no
/// such driver. The result was that any battle in which a monster (or a party
/// member) cast a spell stopped dead in `MagicSustain` forever, which is most
/// real encounters. `live_battle_tick` retires it on the frame the state is
/// reached, the same way it retires `ADVANCE_DONE` on the recovery edge.
#[test]
fn a_spell_cast_does_not_park_the_action_sm() {
    use legaia_engine_vm::battle_action::ActionState;

    let mut w = World::new();
    w.mode = SceneMode::Battle;
    w.party_count = 3;
    // A **priced** catalog, so the band's MP debit is a real subtraction and
    // not the zero an unwired `spell_mp_cost` used to hand it. Gimard is 10 MP
    // byte-exact from SCUS.
    w.set_spell_catalog(legaia_engine_core::retail_magic::retail_seru_magic_catalog());
    for i in 0..8 {
        let a = w.spawn_actor(i);
        a.battle.liveness = 1;
        a.battle.hp = 500;
        a.battle.max_hp = 500;
        a.battle.mp = 60;
    }
    // Party slot 0 casts a player Seru spell at monster slot 3.
    w.actors[0].battle.action_category = 2; // Magic
    w.actors[0].battle.active_target = 3;
    w.actors[0].battle.params[0] = 0x81;
    w.battle_ctx.queued_action = 2;
    w.battle_ctx.action_state = ActionState::Begin.as_byte();

    let mut reached_sustain = false;
    let mut left_sustain = false;
    for _ in 0..4_000 {
        w.tick();
        let st = w.battle_ctx.action_state;
        if st == ActionState::MagicSustain.as_byte() {
            reached_sustain = true;
        } else if reached_sustain {
            left_sustain = true;
            break;
        }
    }
    assert!(
        reached_sustain,
        "the Magic band must reach MagicSustain at all"
    );
    assert!(
        left_sustain,
        "MagicSustain must not hold forever - nothing else in the engine \
         ever clears the caster's spell_iter"
    );
    // ...and the cast was actually paid for. `MagicCastBegin` reads the price
    // through `BattleActionHost::spell_mp_cost`, which the engine answers from
    // the same catalog the live cast path charges from; a host wired to a
    // table nothing fills makes this line read 60.
    assert_eq!(
        w.actors[0].battle.mp, 50,
        "Gimard costs 10 MP - the Magic band must debit it, not cast for free"
    );
}
