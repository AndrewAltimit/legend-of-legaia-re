//! A Seru **captured in a live fight and banked at battle teardown** - the
//! production route to `World::resolve_captures` and, through it, to the
//! record-side learn commit `magic_xp::learn_spell_prepend` (`FUN_801E92DC`).
//!
//! That address is the last row of the reach page's Seru-capture gate, and its
//! coverage side was already satisfied for the wrong reason: `battle_depth_replay`
//! calls `learn_spell_prepend` from its own test body to seat a caster, so the
//! function executed while the route that calls it in play - battle teardown -
//! ran in no ladder at all. A row whose only executor is a ladder's setup code
//! is entered and undriven at the same time; this file drives it.
//!
//! The chain is the live player path end to end: pad -> field encounter ->
//! round prompt -> command ring (Magic arm) -> the capture spell -> target ->
//! `World::cast_spell_on_slots` -> `SpellOutcome::CaptureRoll` ->
//! `World::resolve_capture` (the missing-HP-scaled roll, which downs the
//! monster and logs its id) -> the wipe -> `World::finish_battle` ->
//! `World::resolve_captures` -> `seru_learning::record_capture` ->
//! `magic_xp::learn_spell_prepend` on the character record, plus the capture
//! banner the hosts render.
//!
//! **Two seeds, and neither is the gate's behaviour.** The roll is bounded by
//! the target's missing-HP fraction, so the monsters are left at 1 HP - a
//! weakened Seru is retail's own precondition and the ladder re-casts until
//! the roll takes rather than forcing it. And the capture-points total is
//! seeded just under the Seru's learn threshold through `SeruCaptureLog::restore_row`,
//! which is the shape a resumed save has: one capture's worth of points is
//! what a single fight can bank, so without prior progress no one fight can
//! ever cross a threshold and the learn edge is unreachable by construction.
//!
//! Disc-free: the vanilla formation table (formation 11 = two Killer Bees,
//! monster id 7), the vanilla monster catalog (7 -> Seru 1) and the vanilla
//! Seru registry and spell catalog (spell `0x40` "Reseal" is the
//! `SpellEffect::Capture` row). Runs in CI unconditionally.

use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::seru_learning::SeruRegistry;
use legaia_engine_core::spells::SpellCatalog;
use legaia_engine_core::world::{Actor, SceneMode, World};

/// Formation 11 of the vanilla table: two Killer Bees (monster id 7).
const FORMATION_KILLER_BEE_PAIR: u16 = 11;
/// The Seru monster id 7 carries, and the spell it teaches.
const SERU_ID: u16 = 0x0001;
const SERU_SPELL: u8 = 0x20;
/// The vanilla capture spell: `SpellEffect::Capture { hit_pct: 60 }`.
const RESEAL: u8 = 0x40;
/// Points already banked when the fight starts. The Seru is worth 25 a
/// capture against a threshold of 100, so this is the resumed save a player
/// who has caught three of them arrives with.
const BANKED_POINTS: u16 = 75;

fn build_world() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 3;
    // Zeroed records first: retail's member walk hands no command ring to a
    // member with no HP.
    w.load_party(legaia_save::Party::zeroed(3));
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 200;
        w.actors[i].battle.max_hp = 200;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 60);
    }
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.set_spell_catalog(SpellCatalog::vanilla());
    w.set_seru_registry(SeruRegistry::vanilla());

    // Only the caster knows the capture spell.
    {
        let rec = &mut w.party.roster.members[0];
        let mut list = rec.spell_list();
        list.count = 1;
        list.ids[0] = RESEAL;
        list.levels[0] = 1;
        rec.set_spell_list(list);
    }
    // The banked progress a resumed save carries, for every party member the
    // Seru's learnable mask covers.
    for slot in 0..3u8 {
        w.seru
            .log
            .restore_row(slot, SERU_ID, BANKED_POINTS, 3, false, None);
    }

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.locomotion.camera_azimuth = 0;

    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("w1g_seru_capture_ladder");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(FORMATION_KILLER_BEE_PAIR, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.toggles.live_gameplay_loop = true;
    w.battle.player_driven = true;
    w
}

fn enter_battle(w: &mut World) {
    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..6000 {
        w.set_pad(up);
        let _ = w.tick();
        if w.mode == SceneMode::Battle {
            return;
        }
    }
    panic!("no encounter triggered in 6000 field ticks");
}

fn press(w: &mut World, b: PadButton) {
    w.set_pad(InputState::mask_of([b]));
    let _ = w.tick();
    w.set_pad(0);
    let _ = w.tick();
}

/// Tick until the command session reopens, or the battle ends.
fn wait_for_prompt(w: &mut World) -> bool {
    for _ in 0..0x400 {
        if w.battle.command.is_some() {
            return true;
        }
        if w.mode != SceneMode::Battle {
            return false;
        }
        w.set_pad(0);
        let _ = w.tick();
    }
    false
}

/// Leave every live monster on 1 HP out of a wide max, so the capture roll's
/// missing-HP scaling is near its `hit_pct` ceiling. Retail's capture is
/// reliable only on a weakened Seru; this is that state, not a forced roll.
fn weaken_monsters(w: &mut World) {
    let first = w.party.party_count as usize;
    for s in first..w.actors.len() {
        if w.actors[s].battle.max_hp == 0 {
            continue;
        }
        w.actors[s].battle.max_hp = 100;
        if w.actors[s].battle.liveness != 0 {
            w.actors[s].battle.hp = 1;
        }
    }
}

#[test]
fn a_captured_seru_banks_at_teardown_and_commits_the_learn_to_the_record() {
    let mut w = build_world();
    enter_battle(&mut w);

    // Post-entry re-seat: MP for the casts, and a fight that cannot end by
    // the monsters fleeing (retail's own scripted no-escape flag).
    for i in 0..3 {
        w.actors[i].battle.max_hp = 200;
        w.actors[i].battle.hp = 200;
        w.actors[i].battle.liveness = 1;
        w.actors[i].battle.mp = 99;
    }
    w.battle.magic[0] = 40;
    w.battle.no_escape = true;
    weaken_monsters(&mut w);

    assert!(
        w.party.roster.members[0].spell_list().count == 1,
        "the caster starts with the capture spell alone"
    );
    assert!(
        !w.seru.log.has_learned(0, SERU_ID),
        "nothing is learned before the fight"
    );

    let mut casts = 0usize;
    let mut captured_mid_battle = false;
    for _ in 0..24 {
        if w.mode != SceneMode::Battle {
            break;
        }
        weaken_monsters(&mut w);
        if !wait_for_prompt(&mut w) {
            break;
        }
        if matches!(
            w.battle.command.as_ref().map(|s| &s.phase),
            Some(legaia_engine_core::battle_input::CommandPhase::RoundPrompt { .. })
        ) {
            press(&mut w, PadButton::Cross); // Begin -> the command ring
        }
        if w.battle_ctx.active_actor != 0 {
            // Only slot 0 knows Reseal; the others Spirit so the round turns
            // over without killing the target the capture needs alive.
            press(&mut w, PadButton::Down);
            continue;
        }
        press(&mut w, PadButton::Right); // ring: Magic arm
        assert!(
            w.battle.spell_menu.is_some(),
            "the Magic arm should open the spell submenu (cast {casts})"
        );
        press(&mut w, PadButton::Cross); // spell row 0 (Reseal) -> target
        press(&mut w, PadButton::Cross); // target confirm -> the commit
        casts += 1;
        // The last commit raises the party's Begin | Reselect (`0x6E`);
        // Cross takes its highlighted Begin.
        for _ in 0..4 {
            match w.battle.command.as_ref().map(|c| &c.phase) {
                None => break,
                Some(legaia_engine_core::battle_input::CommandPhase::CommitConfirm { .. }) => {
                    press(&mut w, PadButton::Cross)
                }
                Some(_) => press(&mut w, PadButton::Down),
            }
        }
        // Run the cast band out so the capture roll folds.
        for _ in 0..0x400 {
            if w.casting.pending_cast.is_none() || w.mode != SceneMode::Battle {
                break;
            }
            w.set_pad(0);
            let _ = w.tick();
        }
        if !w.seru.battle_captures.is_empty() {
            captured_mid_battle = true;
        }
    }

    assert!(
        captured_mid_battle || w.mode != SceneMode::Battle,
        "the ladder never landed a capture roll in {casts} casts"
    );

    // Run the teardown out: the wipe ends the battle and `finish_battle`
    // drains `battle_captures` into `resolve_captures`.
    for _ in 0..0x800 {
        if w.mode != SceneMode::Battle {
            break;
        }
        w.set_pad(0);
        let _ = w.tick();
    }
    assert_ne!(w.mode, SceneMode::Battle, "the battle never ended");
    assert!(
        w.seru.battle_captures.is_empty(),
        "teardown always drains the capture list"
    );

    // The bank: the seeded 75 plus this capture's 25 crosses the threshold.
    assert!(
        w.seru.log.has_learned(0, SERU_ID),
        "the capture crossed the Seru's learn threshold"
    );
    assert!(
        w.seru.log.learned_spells(0).contains(&SERU_SPELL),
        "the read-model list carries the learned spell"
    );

    // `FUN_801E92DC`: the record-side commit. The learned id is *prepended*,
    // so it takes slot 0 and pushes the capture spell down one - and the
    // parallel XP word for the new entry starts at zero.
    let rec = &w.party.roster.members[0];
    let list = rec.spell_list();
    assert_eq!(list.count, 2, "the record's spell count was bumped");
    assert_eq!(list.ids[0], SERU_SPELL, "the learned id is prepended");
    assert_eq!(list.levels[0], 1, "a learned spell starts at level 1");
    assert_eq!(
        list.ids[1], RESEAL,
        "the spell the caster already knew is pushed down one slot"
    );
    assert_eq!(
        legaia_engine_core::magic_xp::spell_xp(rec, 0usize),
        0,
        "the new entry's XP word starts at zero"
    );

    // Every party member the Seru's mask covers banked the same capture.
    for slot in 1..3u8 {
        assert!(
            w.seru.log.has_learned(slot, SERU_ID),
            "capture points bank against every eligible character (slot {slot})"
        );
    }

    // The host-facing banner the capture raises, and the outcome list under
    // it.
    let banner = w
        .party
        .current_capture_banner
        .as_ref()
        .expect("the capture staged its banner");
    assert_eq!(banner.seru_name(), "Spark");
    assert!(!banner.learns().is_empty());
    let outcomes = w.drain_last_capture_outcomes();
    assert!(
        outcomes.iter().any(|o| o.accepted && !o.learns.is_empty()),
        "an accepted outcome carrying learn events"
    );
}
