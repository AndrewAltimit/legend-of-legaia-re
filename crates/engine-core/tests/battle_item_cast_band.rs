//! The battle **Item command runs the action SM's Item band** - the ladder for
//! the reach-triage "spirit-cast" rows.
//!
//! Retail commits a battle item as a category-1 action: `FUN_801E295C`'s seed
//! routes it through `item_seed_band` into the `0x3C..0x40` cast states -
//! `SpiritPreArm` (stamp the effect-descriptor pair, raise the `0x4C` label),
//! `SpiritWait` (the once-per-cast audio cue, `FUN_801F3990`),
//! `SpiritFire`/`SpiritFireDamage` (the applier call + the cue-group
//! expansion, `FUN_801E22C8`), `SpiritPostDamage`. The two SummonFlute ids
//! (`0x98`/`0x99`) reroute to the summon band instead
//! (`SummonInvoke..SummonDone`, whose return arm is the queued-magic guard
//! `FUN_801F3C34`).
//!
//! Before this wire the live loop parked an item use straight at
//! `EndOfAction`, so none of those states ever ran in a playthrough - the
//! defect this test would have caught. The ladder is pad-only: it walks into
//! a battle, opens the command ring, picks Item, uses a Healing Leaf, and
//! asserts the band actually executed (state trace through the world's own
//! host) with the item's simulation effects intact.
//!
//! Disc-free: synthetic party + the vanilla monster/formation tables. Runs in
//! CI unconditionally.

use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::battle_action::{ActionCategory, ActionState};

/// Build a field-ready world with the player-driven battle flag set - the
/// same shape as `battle_player_driven.rs`.
fn build_world() -> World {
    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 60);
    }
    w.load_party(legaia_save::Party::zeroed(3));
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());

    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    w.field_camera_azimuth = 0;

    use legaia_engine_core::encounter::{
        EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
    };
    let mut table = EncounterTable::new("item_cast_band_test");
    table.set_trigger_rate(0xFF);
    table.push(EncounterEntry::new(1, 1));
    let mut session = EncounterSession::new(EncounterTracker::new(table));
    session.transition_frames = 2;
    session.grace_frames = 2;
    w.set_encounter_session(Some(session));

    w.mode = SceneMode::Field;
    w.live_gameplay_loop = true;
    w.battle_player_driven = true;
    w
}

/// Walk until the world flips into battle mode.
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

/// Press one button for a tick, then release for a tick (so `just_pressed`
/// edges fire exactly once), recording every action-SM state seen.
fn press(w: &mut World, b: PadButton, trace: &mut Vec<u8>) {
    w.set_pad(InputState::mask_of([b]));
    let _ = w.tick();
    trace.push(w.battle_ctx.action_state);
    w.set_pad(0);
    let _ = w.tick();
    trace.push(w.battle_ctx.action_state);
}

/// Tick with no input, recording states.
fn idle_ticks(w: &mut World, n: usize, trace: &mut Vec<u8>) {
    for _ in 0..n {
        w.set_pad(0);
        let _ = w.tick();
        trace.push(w.battle_ctx.action_state);
    }
}

#[test]
fn item_use_runs_the_sm_item_band_to_the_cast_states() {
    let mut w = build_world();
    // Two copies so the "one copy consumed" assertion is non-vacuous. The
    // catalog defaults empty (disc-installed at boot); the clean-room vanilla
    // catalog carries the Healing Leaf entry the menu filter needs.
    w.set_item_catalog(legaia_engine_core::items::ItemCatalog::vanilla());
    w.inventory.insert(0x77, 2);

    enter_battle(&mut w);
    // Battle entry reseeds party stats from the (zeroed) roster records -
    // re-seat live battle stats post-entry so the party rows exist, with a
    // hurt member so the Healing Leaf has a deficit to close and passes the
    // retail menu-usability gate (an item is only offered when a target would
    // benefit).
    for i in 0..3 {
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.liveness = 1;
    }
    w.actors[0].battle.hp = 40;
    let mut trace: Vec<u8> = Vec::new();

    // Round prompt (`Begin | Run`): Cross takes Begin -> the command ring.
    press(&mut w, PadButton::Cross, &mut trace);
    assert!(
        w.battle_command.is_some(),
        "command session should be open on the party turn"
    );
    // Ring seat order is up/left/right/down = Item/Attack/Magic/Spirit; the
    // Up press itself commits the Item arm (retail state 0x28 dispatch).
    press(&mut w, PadButton::Up, &mut trace);
    assert!(
        w.battle_item_menu.is_some(),
        "the Item arm should hand off to the inventory submenu"
    );

    // Inventory submenu: Cross on the Healing Leaf row -> target select;
    // Cross on the first target row (slot 0) -> the use commits.
    press(&mut w, PadButton::Cross, &mut trace);
    press(&mut w, PadButton::Cross, &mut trace);

    // The commit must ARM the SM, not park it: category 1 with the item id
    // staged as the action parameter.
    assert_eq!(
        w.actors[w.battle_ctx.active_actor as usize]
            .battle
            .action_category,
        ActionCategory::Item.as_byte(),
        "the committed item use arms a category-1 action"
    );
    assert_eq!(
        w.actors[w.battle_ctx.active_actor as usize].battle.params[0], 0x77,
        "the staged action parameter is the item id"
    );

    // Simulation effects landed at commit (the fold precedes the band).
    assert_eq!(w.actors[0].battle.hp, 100, "Healing Leaf healed the target");
    assert_eq!(
        w.inventory.get(&0x77).copied(),
        Some(1),
        "exactly one copy consumed"
    );

    // Let the band run. SpiritPostDamage alone holds 0x80 frames.
    idle_ticks(&mut w, 0x140, &mut trace);

    // The whole Item band executed through the world's own battle host: the
    // seed states and every Spirit-band cast state.
    for state in [
        ActionState::Begin,
        ActionState::SpiritPreArm,
        ActionState::SpiritFireDamage,
        ActionState::SpiritPostDamage,
    ] {
        assert!(
            trace.contains(&state.as_byte()),
            "action SM never reached {state:?}; trace = {trace:02X?}"
        );
    }
    // The band's HUD label (`0x4C`, the cast-name element retail raises at
    // `SpiritPreArm` and re-raises at `SpiritFire`) reached the event queue.
    let saw_cast_label = w.pending_battle_events.iter().any(|e| {
        matches!(
            e,
            legaia_engine_core::battle_events::BattleEvent::UiElement {
                effect_id: 0x4C,
                ..
            }
        )
    });
    assert!(saw_cast_label, "the 0x4C cast label was raised by the band");

    // The turn cycled: the SM came back around to another party command (the
    // command session reopened) - i.e. the band did not park the battle.
    let mut reopened = false;
    for _ in 0..0x200 {
        w.set_pad(0);
        let _ = w.tick();
        if w.battle_command.is_some() {
            reopened = true;
            break;
        }
    }
    assert!(reopened, "the battle parked after the item band");
}

#[test]
fn summon_flute_item_reroutes_to_the_summon_band_and_completes() {
    let mut w = build_world();
    enter_battle(&mut w);

    // Wait for the party command session, then commit a SummonFlute use as
    // the live loop's item arm now does (the flute ids are not in the
    // clean-room item catalog's usable set, so the menu path is not the
    // vehicle here - the arming contract is).
    let mut opened = false;
    for _ in 0..0x40 {
        w.set_pad(0);
        let _ = w.tick();
        if w.battle_command.is_some() {
            opened = true;
            break;
        }
    }
    assert!(opened, "no command session opened");
    let actor = w.battle_command.as_ref().unwrap().actor;
    w.battle_command = None;
    if let Some(a) = w.actors.get_mut(actor as usize) {
        // Fresh action stream (the live loop's own arming sites clear it the
        // same way before staging params[0]).
        a.battle.params = Default::default();
        a.battle.strike_index = 0;
        a.battle.active_target = w.party_count; // first monster slot
        a.battle.action_category = ActionCategory::Item.as_byte();
        a.battle.params[0] = 0x98; // SummonFlute
    }
    w.battle_ctx.active_actor = actor;
    w.battle_ctx.queued_action = ActionCategory::Item.as_byte();
    w.battle_ctx.action_state = ActionState::Begin.as_byte();

    // The flute must traverse the summon band (item_seed_band's `0x98`
    // reroute: sub_route = 9, id -= 2) and come out the other side - the
    // settle glue's job. SummonSustain alone holds 0x78 frames.
    let mut trace: Vec<u8> = Vec::new();
    idle_ticks(&mut w, 0x200, &mut trace);
    for state in [
        ActionState::MagicCastBegin,
        ActionState::SummonInvoke,
        ActionState::SummonSustain,
        ActionState::SummonReturn,
        ActionState::SummonDone,
    ] {
        assert!(
            trace.contains(&state.as_byte()),
            "summon band never reached {state:?}; trace = {trace:02X?}"
        );
    }
    assert_eq!(
        w.actors[actor as usize].battle.params[0], 0x96,
        "item_seed_band stages the flute as summon id (0x98 - 2)"
    );
    // No park: the SM reached EndOfAction and the menu_open latch was
    // released, so the next combatant gets armed.
    assert!(
        trace.contains(&ActionState::EndOfAction.as_byte()),
        "the flute turn never cycled; trace = {trace:02X?}"
    );
    assert_eq!(
        w.battle_ctx.menu_open, 0,
        "the summon band's menu_open latch must be released"
    );
}
