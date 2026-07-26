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
use legaia_engine_core::field_submode_screen::{SUBMODE_ACCEPT_MASK, slot_for_op49_sub_op};
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

#[test]
fn the_coin_counter_draws_a_panel_from_inside_the_frame_loop() {
    let mut w = field_world();
    w.money = 50_000;
    w.open_coin_counter();
    assert!(
        tick_until(&mut w, 16, |w| !w.submode_screen.draws().is_empty()),
        "the panel-window painter produced no draws in a live frame loop"
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
fn the_three_dedicated_sub_ops_still_own_their_own_paths() {
    // Sub-0 (inline gold shop), sub-3 (name entry) and sub-5 (tile board) each
    // have a host path already; routing them through the submode screen would
    // arm two parks at once.
    for s in [0u8, 3, 5] {
        assert_eq!(slot_for_op49_sub_op(s), None);
    }
    for s in [1u8, 2, 4, 6, 7, 8, 9, 0xA, 0xB, 0xC, 0xD] {
        assert_eq!(slot_for_op49_sub_op(s), Some(slot::CLOSE_TICK));
    }
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
