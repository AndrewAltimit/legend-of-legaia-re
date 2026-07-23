//! Live-path coverage for the battle wiring this lane landed.
//!
//! Three retail routines that previously had no engine caller now run from the
//! live battle loop, and each is exercised here through `World` alone:
//!
//! - `FUN_8003FB10` - the action validator. The battle target rows are built
//!   from its per-slot validity byte (`World::battle_target_rows`), so a
//!   downed monster is unselectable *because the validator says so*.
//! - `FUN_801DA6B4` - the target-select cursor tint, stamped while the picker
//!   walks the monster row and cleared when it closes.
//! - `FUN_801E295C` case `0xFF` - the end-of-round handler: the round counter
//!   bump (`World::advance_battle_mode`, the multi-phase boss gate) and the
//!   status-`0x400` waker `FUN_801F45A4`.
//!
//! Disc-free: synthetic party + monsters. Runs in CI unconditionally.

use legaia_engine_core::input::{InputState, PadButton};
use legaia_engine_core::world::{Actor, SceneMode, World};
use legaia_engine_vm::battle_action::{
    ActionState, CURSOR_COLOR_BRIGHT, CURSOR_COLOR_DIM, CURSOR_FLAG_DIMMED, CURSOR_FLAG_SELECTED,
};

/// 3 party + `monsters` monster slots, all live, player-driven battle mode.
fn battle_world(monsters: usize) -> World {
    let mut w = World::new();
    while w.actors.len() < 3 + monsters {
        w.actors.push(Actor::default());
    }
    w.party_count = 3;
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 200;
        w.actors[i].battle.max_hp = 200;
        w.actors[i].battle.liveness = 1;
        w.set_battle_attack(i as u8, 10);
    }
    for i in 3..3 + monsters {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 400;
        w.actors[i].battle.max_hp = 400;
        w.actors[i].battle.liveness = 1;
    }
    w.mode = SceneMode::Battle;
    w.live_gameplay_loop = true;
    w.battle_player_driven = true;
    // Park the SM where the live loop re-arms, so the next tick hands the
    // turn to the player and opens the command session.
    w.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
    w
}

/// Tick until the live loop opens a player command session (it does so on the
/// first party turn), returning the acting party slot. Panics if it never does.
fn open_command_menu(w: &mut World) -> usize {
    for _ in 0..600 {
        if let Some(s) = w.battle_command.as_ref() {
            return s.actor as usize;
        }
        w.tick();
    }
    panic!("the live loop never opened a command session");
}

#[test]
fn validator_backed_target_rows_skip_a_downed_monster_slot() {
    // Monster slot 4 is seated but down. The target rows the picker opens on
    // are built from the validator's arm-0x05 validity byte, so slot 4 is not
    // a candidate and the cursor steps straight over it.
    let mut w = battle_world(3);
    w.actors[4].battle.hp = 0;
    w.actors[4].battle.liveness = 0;
    let acting = open_command_menu(&mut w);

    // Cross selects Attack and opens the cursor on the first valid slot.
    w.set_pad(InputState::mask_of([PadButton::Cross]));
    w.tick();
    assert!(w.battle_command.is_some(), "target cursor open");
    assert_eq!(
        w.actors[acting].battle.active_target, 3,
        "cursor opens on the first slot the validity byte marks selectable"
    );

    // Right moves to the next *valid* slot: 5, not the downed 4.
    w.set_pad(InputState::mask_of([PadButton::Right]));
    w.tick();
    assert_eq!(
        w.actors[acting].battle.active_target, 5,
        "the downed slot fails the validator arm, so the cursor skips it"
    );
}

#[test]
fn target_cursor_tint_is_stamped_while_picking_and_cleared_on_confirm() {
    let mut w = battle_world(3);
    let acting = open_command_menu(&mut w);

    // Cross #1: Attack selected, target cursor opens on the first live
    // monster. FUN_801DA6B4's "on" arm has run.
    w.set_pad(InputState::mask_of([PadButton::Cross]));
    w.tick();
    assert!(w.battle_command.is_some(), "picker still open");
    assert_eq!(w.actors[3].battle.render_flag, CURSOR_FLAG_SELECTED);
    assert_eq!(w.actors[3].battle.render_color, CURSOR_COLOR_BRIGHT);
    for slot in 4..6 {
        assert_eq!(
            w.actors[slot].battle.render_flag, CURSOR_FLAG_DIMMED,
            "slot {slot} dimmed"
        );
        assert_eq!(w.actors[slot].battle.render_color, CURSOR_COLOR_DIM);
    }
    // The pointed-at slot is mirrored onto the acting actor's `+0x1DD`, which
    // is where retail's picker keeps it and where the tint reads it back.
    assert_eq!(w.actors[acting].battle.active_target, 3);

    // Walk the cursor right: the tint follows.
    w.set_pad(InputState::mask_of([PadButton::Right]));
    w.tick();
    assert_eq!(w.actors[4].battle.render_flag, CURSOR_FLAG_SELECTED);
    assert_eq!(w.actors[3].battle.render_flag, CURSOR_FLAG_DIMMED);

    // Cross #2 confirms: the cursor closes and the clear arm runs.
    w.set_pad(InputState::mask_of([PadButton::Cross]));
    w.tick();
    for slot in 3..6 {
        assert_eq!(
            w.actors[slot].battle.render_flag, 0,
            "slot {slot} tint cleared"
        );
        assert_eq!(w.actors[slot].battle.render_scale, 0);
    }
}

/// Drive an auto-resolving battle far enough to cross several round
/// boundaries, returning the world.
fn run_rounds(mut w: World, frames: usize) -> World {
    w.battle_player_driven = false;
    w.battle_ctx.queued_action = 3;
    w.battle_ctx.action_state = ActionState::Begin.as_byte();
    for i in 0..6 {
        w.battle_speed[i] = 20 + i as u16;
    }
    for _ in 0..frames {
        w.tick();
    }
    w
}

#[test]
fn round_boundary_bumps_the_battle_mode_counter() {
    let w = battle_world(3);
    assert_eq!(w.battle_mode(), 0, "a fresh battle opens on mode 0");
    let w = run_rounds(w, 4000);
    assert!(
        w.battle_mode() > 0,
        "the end-of-round handler must have bumped ctx[+0x28A] at least once"
    );
}

#[test]
fn status_0x400_waker_clears_the_bit_on_a_live_actor_only() {
    // A down actor keeps the bit: retail's sweep gates on `+0x14C != 0`.
    let mut w = battle_world(3);
    w.actors[5].battle.field_flags |= 0x400;
    w.actors[5].battle.hp = 0;
    w.actors[5].battle.liveness = 0;
    let w = run_rounds(w, 4000);
    assert_eq!(
        w.actors[5].battle.field_flags & 0x400,
        0x400,
        "a downed actor is skipped, so its bit survives"
    );

    // A live actor's bit is rolled off within a handful of round boundaries
    // (1-in-8 per boundary).
    let mut w = battle_world(3);
    w.actors[0].battle.field_flags |= 0x400;
    let w = run_rounds(w, 8000);
    assert_eq!(
        w.actors[0].battle.field_flags & 0x400,
        0,
        "the waker clears the latent 0x400 status on a live actor"
    );
}
