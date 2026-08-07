//! The pause menu freezes the world, because retail's CARD mode runs no
//! master frame driver.
//!
//! Retail reaches the in-field pause menu through the CARD mode pair
//! (`_DAT_8007B83C = 0x17` in every menu-open capture in the save library).
//! Mode 23's per-frame handler `FUN_80025F74` is the same eight-instruction
//! skeleton as the other two, with one substitution: where they call the
//! master frame driver `FUN_80016444`, it calls `FUN_80017978`. That body is
//! 18 instructions (`0x80017978..0x800179BC`) with three `jal`s - the debug
//! mode-advance chord, the CARD actor's own `+0x0C` handler, and the dev
//! readout HUD - and **no `jal 0x80016444` among them**.
//!
//! `FUN_80016444` is what runs the five `FUN_8002519C` actor tick passes. So
//! for as long as the menu owns the frame, retail advances no actor, no move
//! VM, no animation and no effect. The player-denominated statement of that:
//! a timed section does not tick down while you read a menu, and a walking NPC
//! does not keep walking behind it.
//!
//! Synthetic world, no disc gate, no Sony bytes.

use legaia_engine_core::mode::{GameMode, runs_master_frame_driver};
use legaia_engine_core::world::{SceneMode, World};

/// The mode-table law the gate reads. Exactly one shipped per-frame mode does
/// not run the master driver, and it is the one the pause menu runs under.
#[test]
fn card_mode_is_the_only_mode_that_runs_no_master_frame_driver() {
    assert!(!runs_master_frame_driver(GameMode::CardMode));
    for i in 0..28usize {
        let m = GameMode::from_index(i).unwrap();
        if m == GameMode::CardMode {
            continue;
        }
        assert!(
            runs_master_frame_driver(m),
            "mode {i} ({m:?}) unexpectedly skips FUN_80016444"
        );
    }
    // And the pause menu's scene mode is the one that maps onto it.
    assert_eq!(
        GameMode::for_scene_mode(SceneMode::Menu),
        Some(GameMode::CardMode)
    );
}

/// A scripted countdown (`0x4C 0xD3`) advances zero frames while the pause
/// menu is open. This is the player-visible half: a collapsing-dungeon clock
/// must not drain while the player is reading their inventory.
#[test]
fn a_timed_section_advances_zero_frames_under_the_pause_menu() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // ab = packed flag word (high half expiry, low half below-threshold).
    world.schedule_timed_flags(0x04C7_0123, 600, 100);
    for _ in 0..10 {
        world.tick();
    }
    let running = world.escape_timer.remaining;
    assert!(running < 600, "the countdown has to be live to be frozen");

    world.mode = SceneMode::Menu;
    for _ in 0..120 {
        world.tick();
    }
    assert_eq!(
        world.escape_timer.remaining, running,
        "the countdown drained while the pause menu owned the frame"
    );

    // Non-vacuity: it resumes the moment the menu closes.
    world.mode = SceneMode::Field;
    for _ in 0..10 {
        world.tick();
    }
    assert!(world.escape_timer.remaining < running, "and it resumes");
}

/// The actor pool does not advance under the menu. `wait_timer` is the
/// cheapest witness of the move-VM pass: `tick_move_vms` decrements it
/// unconditionally before its own gate, so any frame that reaches the pass at
/// all moves it.
#[test]
fn the_actor_pool_does_not_advance_under_the_pause_menu() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.spawn_actor(0);
    world.actors[0].move_state.wait_timer = 10_000;
    // Move-VM HALT (`0x08`) - the actor never leaves the VM, so the only thing
    // that moves is the pre-tick decrement.
    world.set_move_bytecode(0, Some(vec![0x0008]));

    for _ in 0..5 {
        world.tick();
    }
    let running = world.actors[0].move_state.wait_timer;
    assert!(running < 10_000, "the pool has to be live to be frozen");

    world.mode = SceneMode::Menu;
    for _ in 0..60 {
        world.tick();
    }
    assert_eq!(
        world.actors[0].move_state.wait_timer, running,
        "the actor pool advanced while the pause menu owned the frame"
    );

    // Non-vacuity: the freeze is the menu's, not a dead pool.
    world.mode = SceneMode::Field;
    for _ in 0..5 {
        world.tick();
    }
    assert!(
        world.actors[0].move_state.wait_timer < running,
        "the pool has to resume when the menu closes"
    );
}

/// Battle is not a menu. The gate keys on the mode table, so every other mode
/// keeps its master driver - a regression here would freeze the game.
#[test]
fn every_other_mode_keeps_ticking_its_actor_pool() {
    for mode in [
        SceneMode::Field,
        SceneMode::Battle,
        SceneMode::Cutscene,
        SceneMode::WorldMap,
        SceneMode::Title,
    ] {
        let mut world = World::new();
        world.mode = mode;
        world.spawn_actor(0);
        world.actors[0].move_state.wait_timer = 10_000;
        world.set_move_bytecode(0, Some(vec![0x0008]));
        for _ in 0..5 {
            world.tick();
        }
        assert!(
            world.actors[0].move_state.wait_timer < 10_000,
            "{mode:?} stopped advancing its actor pool"
        );
    }
}
