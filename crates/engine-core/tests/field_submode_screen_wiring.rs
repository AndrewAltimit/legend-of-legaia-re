//! The op-`0x49` submode screen, driven through `World::tick` rather than
//! through its own entry point.
//!
//! `World::tick` is what the play-window host calls every frame
//! (`legaia-engine play-window` -> `impl ApplicationHandler::window_event` ->
//! `redraw` -> `BootSession::tick` -> `World::tick`), so a change these tests
//! see is a change a running session sees. Calling
//! `World::tick_submode_screen` directly would prove only that the kernel
//! computes - the gap this suite exists to close is whether the frame loop
//! reaches it at all.

use legaia_engine_core::actor_handler::ActorHandler;
use legaia_engine_core::field_submode_screen::{
    Op49ParkOwner, SUBMODE_ACCEPT_MASK, slot_for_op49_sub_op,
};
use legaia_engine_core::world::World;
use legaia_engine_vm::baka_hub_actors::{GOLD_PER_COIN, PICK_ACCEPT, slot};

/// A world with the submode driver actor the MAN loader spawns.
fn field_world() -> World {
    let mut w = World::new();
    w.man_load_actor_reset();
    assert!(
        w.find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_some(),
        "every MAN load spawns the op-0x49 submode driver (FUN_801D9C3C)"
    );
    w
}

/// Tick until the closure says stop, or give up after `limit` frames.
fn tick_until(w: &mut World, limit: usize, mut done: impl FnMut(&World) -> bool) -> bool {
    for _ in 0..limit {
        w.tick();
        if done(w) {
            return true;
        }
    }
    false
}

#[test]
fn world_tick_runs_the_submode_dispatcher() {
    let mut w = field_world();
    w.money = 50_000;
    w.open_coin_counter();

    // One `World::tick` must reach `tick_handler_actors` -> the dispatcher.
    // The actor cadence gates on `actor_vsync_accum`, so allow a few ticks.
    assert!(
        tick_until(&mut w, 16, |w| !w.submode_screen.frame.actions.is_empty()),
        "the frame loop never reached the dispatcher"
    );
}

/// The painter follows the **descriptor the state machine installs**, not a
/// window the caller pinned. The counter's entry arm installs
/// `PANEL_COIN_IDLE`, whose record `10` painter (`FUN_801E6F70`) lives outside
/// this module, so nothing draws until the accept installs
/// `PANEL_COIN_CONFIRM` - record `11`, the three-line panel.
#[test]
fn the_coin_counter_draws_the_panel_its_own_descriptor_installs() {
    let mut w = field_world();
    w.money = 50_000;
    w.open_coin_counter();
    assert!(tick_until(&mut w, 16, |w| w.submode_screen.actor.sub == 1));
    assert_eq!(
        w.submode_screen.installed_windows,
        vec![0, 10, 10],
        "the entry arm installs the coin counter's own idle record"
    );

    w.submode_screen.counter.set_entered(3);
    w.input.set_pad(SUBMODE_ACCEPT_MASK as u16);
    assert!(tick_until(&mut w, 16, |w| w.submode_screen.actor.sub == 2));
    assert_eq!(
        w.submode_screen.installed_windows,
        vec![legaia_engine_vm::baka_hub_actors::window::THREE_LINE]
    );
    assert!(
        !w.submode_screen.draws().is_empty(),
        "the installed panel's painter produced no draws in a live frame loop"
    );
}

#[test]
fn buying_coins_through_the_frame_loop_moves_gold_into_the_coin_bank() {
    let mut w = field_world();
    w.money = 5_000;
    w.casino_coins = 7;
    w.open_coin_counter();

    // Frame 1: the counter seeds itself and opens the entry panel.
    assert!(tick_until(&mut w, 16, |w| w.submode_screen.actor.sub == 1));
    w.submode_screen.counter.set_entered(12);

    // Accept -> the Yes/No panel.
    w.input.set_pad(SUBMODE_ACCEPT_MASK as u16);
    assert!(
        tick_until(&mut w, 16, |w| w.submode_screen.actor.sub == 2),
        "the accept edge never reached the counter"
    );

    // Pick Yes and accept -> the commit state, then the commit itself.
    w.input.set_pad(0);
    w.submode_screen.counter.yes_no = 0;
    w.submode_screen.picker_result = PICK_ACCEPT;
    assert!(tick_until(&mut w, 16, |w| w.submode_screen.actor.sub >= 3));
    w.submode_screen.picker_result = 0;
    assert!(tick_until(&mut w, 16, |w| w.casino_coins != 7));

    // The regression this pins: the credit is coins, the debit is gold, and
    // the two never swap. (A sibling minigame tally once paid a coin prize
    // into gold.)
    assert_eq!(w.casino_coins, 19, "12 coins credited to the casino bank");
    assert_eq!(
        w.money,
        5_000 - 12 * GOLD_PER_COIN,
        "gold paid 100 per coin, and nothing else touched it"
    );
}

#[test]
fn an_unaffordable_amount_never_reaches_the_bank() {
    let mut w = field_world();
    w.money = 250; // two coins' worth
    w.casino_coins = 0;
    w.open_coin_counter();
    assert!(tick_until(&mut w, 16, |w| w.submode_screen.actor.sub == 1));
    w.submode_screen.counter.set_entered(9_999);

    w.input.set_pad(SUBMODE_ACCEPT_MASK as u16);
    for _ in 0..16 {
        w.tick();
    }
    assert_eq!(w.casino_coins, 0, "the refusal buzz banks nothing");
    assert_eq!(w.money, 250);
    assert_ne!(
        w.submode_screen.actor.sub, 2,
        "an over-budget accept must not open the confirm panel"
    );
}

#[test]
fn a_screen_closes_itself_and_reports_done() {
    let mut w = field_world();
    // Slot 0 - what a freshly spawned driver actor carries.
    w.open_field_submode_screen(slot::CLOSE_TICK, None);
    assert!(
        tick_until(&mut w, 32, |w| w.submode_screen.is_done()),
        "the close tick never handed the frame back"
    );
    assert!(!w.submode_screen.is_open());
    // Retail retires the driver node; the engine's kill-bit sweep frees it.
    assert!(
        w.find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_none(),
        "the retired driver left the pool"
    );
}

#[test]
fn the_four_dedicated_sub_ops_still_own_their_own_paths() {
    // Sub-0 (inline gold shop), sub-3 (name entry), sub-5 (tile board) and
    // sub-7 (casino prize exchange) each have a host path already; routing
    // them through the submode screen would arm two parks at once.
    for s in [0u8, 3, 5, 7] {
        assert_eq!(slot_for_op49_sub_op(s), None);
    }
    // Sub-`0xD` opens nothing either, for a different reason: its row in
    // retail's table is `-1` AND nothing else opens a screen for it, so the
    // park has to stand as a menu-entry context. Opening the close tick for
    // it - what this list used to assert - retires within a few frames and
    // takes the context with it, which is why the kind-0xD notice panel and
    // ready check were unreachable.
    // See `field_submode_screen::OP49_PARK_PRESERVING_SUB_OPS`.
    assert_eq!(slot_for_op49_sub_op(0x0D), None);
    // Every other sub-op takes the handler retail's own table names
    // (`0x801F33A4`); the one remaining row that names no handler (`1`)
    // falls back to the slot a freshly spawned driver carries, because
    // retail routes it into a driver that does hand back.
    for (s, want) in [
        (1u8, slot::CLOSE_TICK),
        (2, 0x21),
        (4, 0x23),
        (6, slot::COIN_COUNTER),
        (8, slot::START_MENU),
        (9, slot::PROMPT),
        (0xA, 0x31),
        (0xB, slot::SUBMENU),
        (0xC, 0x33),
    ] {
        assert_eq!(slot_for_op49_sub_op(s), Some(want), "sub-op {s:#x}");
    }
}

/// Retail's op-`0x49` Idle arm calls the allocator on **every** arm
/// (`jal 0x80020de0` at `0x801E09A0`, unconditional for any `sub_op < 0xE`),
/// so a scene gets a fresh driver per screen. The port only had the one
/// `man_load_actor_reset` spawns, and the dispatcher retires it on hand-back -
/// so the second screen of a scene had no dispatcher and never handed back,
/// leaving the park Armed for the rest of the scene.
#[test]
fn a_second_screen_gets_its_own_driver_and_still_hands_back() {
    let mut w = field_world();
    w.open_field_submode_screen(slot::CLOSE_TICK, None);
    assert!(tick_until(&mut w, 32, |w| w.submode_screen.is_done()));
    assert!(
        w.find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_none(),
        "the first screen's hand-back retired the driver"
    );

    // The arm re-allocates, exactly as the retail Idle arm does.
    w.open_field_submode_screen(slot::CLOSE_TICK, None);
    assert!(
        w.find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_some(),
        "arming a screen with no live driver must spawn one"
    );
    assert!(
        tick_until(&mut w, 32, |w| w.submode_screen.is_done()),
        "the second screen never handed back - the park would stay Armed forever"
    );
}

/// The park is per-context. Retail's `_DAT_8007B450` is one global, but the
/// port steps the field script, the per-actor channels and the spawned
/// partition-2 records inside one `World::tick` - and the town01 opening's
/// name-entry hand-off is a *dedicated* sub-op that bypasses the global. A
/// screen armed by the field script answering the cutscene timeline's own
/// op-`0x49` is what swallows that beat.
#[test]
fn a_field_script_park_never_answers_the_cutscene_timeline() {
    let mut w = field_world();
    // The per-tick field script arms a default submode park.
    w.in_spawned_record_slice = false;
    w.in_cutscene_timeline = false;
    assert_eq!(w.op49_park_owner(), Op49ParkOwner::FieldScript);
    w.open_field_submode_screen(slot::CLOSE_TICK, None);
    assert!(w.submode_screen.is_open());

    // Same frame, the modal timeline steps its own op-0x49. It must read Idle
    // (so `op49_invoke_setup` runs), not the field script's Armed.
    w.in_spawned_record_slice = true;
    w.in_cutscene_timeline = true;
    assert_eq!(w.op49_park_owner(), Op49ParkOwner::CutsceneTimeline);
    assert!(
        !w.submode_screen
            .is_open_for(Op49ParkOwner::CutsceneTimeline)
    );
    assert!(
        !w.submode_screen
            .is_done_for(Op49ParkOwner::CutsceneTimeline)
    );
    // ... and the field script still owns its own park.
    assert!(w.submode_screen.is_open_for(Op49ParkOwner::FieldScript));

    // A helper context is a third owner, distinct from both.
    w.in_spawned_record_slice = true;
    w.in_cutscene_timeline = false;
    assert_eq!(w.op49_park_owner(), Op49ParkOwner::HelperContext);
    assert!(!w.submode_screen.is_open_for(Op49ParkOwner::HelperContext));
}

#[test]
fn an_idle_world_pays_nothing_for_the_new_pass() {
    // The dispatcher must be inert with no screen up: no draws, no actions,
    // and no change to the money model.
    let mut w = field_world();
    w.money = 1_234;
    w.casino_coins = 5;
    for _ in 0..32 {
        w.tick();
    }
    assert!(w.submode_screen.frame.actions.is_empty());
    assert!(w.submode_screen.draws().is_empty());
    assert_eq!(w.money, 1_234);
    assert_eq!(w.casino_coins, 5);
    assert!(
        w.find_actor_by_handler(ActorHandler::SubmodeDriver)
            .is_some()
    );
}

/// The coin counter's Yes/No confirm is pad-driven end to end: no test (or
/// host) pre-loads `picker_result` - the accept edge alone commits the buy.
///
/// The regression this pins: `picker_result` had no production writer, so
/// once the player accepted an amount the counter parked at state 2 forever
/// with the field script still Armed - a softlock at every koin1 / balden
/// coin cabinet.
#[test]
fn the_coin_confirm_is_pad_driven_without_a_picker_feed() {
    let mut w = field_world();
    w.money = 5_000;
    w.casino_coins = 7;
    w.open_coin_counter();
    assert!(tick_until(&mut w, 16, |w| w.submode_screen.actor.sub == 1));
    w.submode_screen.counter.set_entered(12);

    // Accept -> the Yes/No panel (cursor seeded to No).
    w.input.set_pad(SUBMODE_ACCEPT_MASK as u16);
    assert!(tick_until(&mut w, 16, |w| w.submode_screen.actor.sub == 2));
    assert_eq!(w.submode_screen.counter.yes_no, 1, "seeded to No");

    // A direction edge toggles onto Yes; release the pad in between so each
    // press is a fresh edge.
    w.input.set_pad(0);
    w.tick();
    w.input.set_pad(legaia_engine_core::dev_menu::PACK_UP);
    assert!(tick_until(&mut w, 16, |w| w.submode_screen.counter.yes_no == 0));

    // Accept on Yes -> commit: coins in, gold out, screen hands back.
    w.input.set_pad(0);
    w.tick();
    w.input.set_pad(SUBMODE_ACCEPT_MASK as u16);
    assert!(
        tick_until(&mut w, 32, |w| w.casino_coins != 7),
        "the pad-driven Yes never committed - the state-2 softlock is back"
    );
    assert_eq!(w.casino_coins, 19);
    assert_eq!(w.money, 5_000 - 12 * GOLD_PER_COIN);
    w.input.set_pad(0);
    assert!(
        tick_until(&mut w, 64, |w| w.submode_screen.is_done()),
        "the counter never handed back after the commit"
    );
}
