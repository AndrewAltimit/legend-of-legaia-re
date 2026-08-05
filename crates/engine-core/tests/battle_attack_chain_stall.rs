//! Regression: the strike-pacing gate must always be able to retire, and an
//! action must not inherit the previous action's parameter stream.
//!
//! Two defects composed into a hard soft-lock that a real disc encounter hits
//! on its own:
//!
//! 1. The monster-AI picker writes the chosen spell id into the actor's
//!    action-parameter stream (`params[0]`, retail `+0x1DF`) *before* the cast
//!    is folded. A cast that cannot fold - no catalog entry for that id, which
//!    is the default on any host without a spell catalog - falls back to a
//!    physical strike, and nothing cleared the stream, so `attack_chain` then
//!    walked the spell id as if it were a swing-anim byte.
//! 2. `attack_chain` sets `ADVANCE_DONE` when it stages a byte and holds until
//!    the animation system retires it. The engine's anim commit retires it for
//!    a clip-less swing, but only past its `queued_anim == current_anim`
//!    early-out - so a staged byte equal to the actor's current anim id could
//!    never be retired, and the SM parked at `AttackChain` (`0x1E`) forever.
//!
//! Disc-free; the fight below reproduces the park with plain synthetic stats.

use legaia_engine_core::monster_catalog::{
    FormationDef, FormationSlot, FormationTable, MonsterCatalog, MonsterDef,
};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::battle_action::{ActionState, ActorFlags};

fn world_vs_caster() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    let mut party = w.roster.clone();
    for rec in party.members.iter_mut() {
        let mut hms = rec.hp_mp_sp();
        hms.hp_cur = 200;
        hms.hp_max = 200;
        hms.mp_cur = 20;
        rec.set_hp_mp_sp(hms);
    }
    w.load_party(party);
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 200;
        w.actors[i].battle.max_hp = 200;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 40);
        w.set_battle_defense(i as u8, 12);
    }

    let mut cat = MonsterCatalog::new();
    let mut def = MonsterDef::new(7, "Caster", 300, 30);
    def.udf = 24;
    def.ldf = 24;
    def.mp = 60;
    // Spell ids with no entry in the (empty) spell catalog: the picker rolls
    // one, `cast_spell_on_slots` refuses it, and the turn falls back to a
    // physical strike carrying the id in its parameter stream.
    // One id, so consecutive fallback turns stage the *same* stale byte -
    // which is what drives the actor's anim id pair to the converged state
    // the commit's early-out cannot leave.
    def.magic_attacks = vec![0x50];
    cat.insert(def);
    let mut table = FormationTable::new();
    table.insert(FormationDef::new(1, vec![FormationSlot::new(7)]));
    w.set_formation_table(table, cat);
    w.mode = SceneMode::Field;
    assert!(w.trigger_scripted_battle(1));
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

/// End-to-end: a monster carrying the leftovers of a folded-away cast - a
/// stale strike-script byte plus the anim id the previous fallback swing
/// committed - must still fight to a finish.
///
/// The seed below is the state the real chain produces: the scripted-AI arm of
/// `pick_monster_action` writes the spell id into `params[0]` before
/// `take_monster_turn` discovers the cast cannot fold, and the first fallback
/// swing leaves `current_anim` holding that same id.
#[test]
fn a_monster_whose_cast_cannot_fold_does_not_park_the_attack_chain() {
    let mut w = world_vs_caster();
    {
        let a = &mut w.actors[3].battle;
        a.params[0] = 0x50;
        a.current_anim = 0x50;
    }
    let mut resolved = false;
    for _ in 0..80_000 {
        w.tick();
        if w.mode != SceneMode::Battle {
            resolved = true;
            break;
        }
    }
    assert!(
        resolved,
        "the battle parked at state 0x{:02x} (actor {}) - the strike-pacing \
         gate never retired",
        w.battle_ctx.action_state, w.battle_ctx.active_actor
    );
}

/// The narrow shape, stated directly: a staged strike byte that equals the
/// actor's current anim id must still retire the pacing flag.
#[test]
fn a_staged_byte_matching_the_current_anim_still_retires_the_pacing_flag() {
    let mut w = world_vs_caster();
    // Park the monster mid-chain in exactly the state the anim commit's
    // early-out cannot leave: staged == current, flag set, no clip in flight.
    let slot = 3usize;
    w.battle_ctx.active_actor = slot as u8;
    w.battle_ctx.action_state = ActionState::AttackChain.as_byte();
    {
        let a = &mut w.actors[slot].battle;
        a.params[0] = 0x50;
        a.strike_index = 1;
        a.queued_anim = 0x50;
        a.current_anim = 0x50;
        a.flag_bits.set(ActorFlags::ADVANCE_DONE);
    }
    for _ in 0..200 {
        w.tick();
        if w.battle_ctx.action_state != ActionState::AttackChain.as_byte() {
            return;
        }
    }
    panic!("the attack chain never left 0x1E");
}

/// And the source of the stale byte: arming a physical strike must start from
/// an empty parameter stream, the way retail re-seeds it per action.
#[test]
fn arming_a_physical_strike_clears_the_previous_actions_parameter_stream() {
    let mut w = world_vs_caster();
    let slot = 3usize;
    w.actors[slot].battle.params[0] = 0x50;
    w.actors[slot].battle.params[1] = 0x51;
    w.actors[slot].battle.strike_index = 2;
    // Drive turns until the monster acts again; whatever the picker chose, a
    // physical arm must have wiped the stream.
    for _ in 0..20_000 {
        w.tick();
        if w.mode != SceneMode::Battle {
            break;
        }
        let a = &w.actors[slot].battle;
        if a.params[0] == 0 && a.params[1] == 0 {
            return;
        }
    }
    panic!(
        "the stale parameter stream survived: {:?}",
        &w.actors[slot].battle.params[..4]
    );
}
