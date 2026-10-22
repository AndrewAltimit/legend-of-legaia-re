//! The menu SM's pre-dispatch gate under the round prompt's open glide:
//! `FUN_801D0748` runs the glide stepper ahead of its state switch and
//! dispatches nothing while it returns non-zero (`0x801D0B2C..0x801D0BA4`),
//! parking one press made near the end in `ctx[+0x884]`
//! (`0x801D0B5C..0x801D0B98`) for the first dispatched pass
//! (`0x801D0C1C..0x801D0C58`).

use super::*;
use legaia_engine_battle_menus::battle_input::{BattleCommandSession, CommandPhase};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::PadButton;
use legaia_engine_system::option_values::BattleCommandOpt;

/// A one-member fight parked on a freshly opened round prompt.
fn prompt_world() -> World {
    let mut world = World::new();
    world.party.party_count = 1;
    world.battle.player_driven = true;
    world.toggles.live_gameplay_loop = true;
    world.mode = SceneMode::Battle;
    for i in 0..3 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    world.battle.command = Some(BattleCommandSession::new_round_open(0, 0, false));
    world.start_round_prompt_glide();
    world
}

/// One battle tick with `mask` as the pad word (`0` releases).
fn tick(world: &mut World, mask: u16) {
    world.set_pad(mask);
    world.live_battle_tick();
}

fn on_prompt(world: &World) -> bool {
    matches!(
        world.battle.command.as_ref().map(|c| &c.phase),
        Some(CommandPhase::RoundPrompt { .. })
    )
}

fn elapsed(world: &World) -> u8 {
    world.battle.round_prompt_glide.expect("glide").elapsed
}

/// The stepper's return across a sixteen-vsync glide: `total - elapsed + 1`
/// while in flight, the record's flag on the pass it lands, `0` after.
#[test]
fn the_stepper_return_counts_down_then_reports_the_landing_pass() {
    let mut world = prompt_world();
    let mut seen = Vec::new();
    for _ in 0..18 {
        world.step_action_plate_glides();
        seen.push(world.battle.widget_glide_return);
    }
    let mut want: Vec<u8> = (2..=16).rev().collect();
    want.extend([World::GLIDE_BLOCKING_FLAG, 0, 0]);
    assert_eq!(seen, want);
}

/// A press made while the return is under `6` is parked and handed to the
/// first dispatched pass, which itself reads no press.
#[test]
fn a_late_press_is_parked_for_the_first_dispatched_pass() {
    let mut world = prompt_world();
    // Eleven passes: the return after pass 11 is `6`, not yet under it.
    for _ in 0..10 {
        tick(&mut world, 0);
    }
    tick(&mut world, PadButton::Right.mask());
    assert_eq!(world.battle.widget_glide_return, 6);
    assert!(world.battle.command_pad_buffer.is_none(), "6 is not < 6");
    tick(&mut world, 0);
    // Pass 13: return `4`.
    tick(&mut world, PadButton::Left.mask());
    assert_eq!(world.battle.widget_glide_return, 4);
    let parked = world.battle.command_pad_buffer.expect("parked");
    assert!(parked.left && !parked.right);
    assert!(on_prompt(&world), "parked, not dispatched");
    // A second press does not replace the first (`ctx[+0x884] != 0`).
    tick(&mut world, 0);
    tick(&mut world, PadButton::Right.mask());
    assert!(world.battle.command_pad_buffer.expect("still parked").left);
    tick(&mut world, 0);
    assert_eq!(elapsed(&world), 16);
    assert!(on_prompt(&world));
    // The first dispatched pass has no press of its own: it takes Left.
    tick(&mut world, 0);
    assert!(!on_prompt(&world), "the parked Begin lands");
    assert!(world.battle.command_pad_buffer.is_none());
}

/// A press on the first dispatched pass wins over the parked one, and the
/// park is emptied either way.
#[test]
fn a_live_press_outranks_the_parked_one() {
    let mut world = prompt_world();
    for _ in 0..13 {
        tick(&mut world, 0);
    }
    // Circle on the prompt does nothing; it is what gets parked.
    tick(&mut world, PadButton::Circle.mask());
    assert!(world.battle.command_pad_buffer.expect("parked").circle);
    tick(&mut world, 0);
    tick(&mut world, 0);
    assert_eq!(elapsed(&world), 16);
    tick(&mut world, PadButton::Left.mask());
    assert!(!on_prompt(&world), "the live Left is the pass's word");
    assert!(world.battle.command_pad_buffer.is_none());
}

/// Only the directional-buttons entry parks a press
/// (`_DAT_800846C8 == 0`, `0x801D0B84..0x801D0B90`).
#[test]
fn the_cross_button_entry_parks_nothing() {
    let mut world = prompt_world();
    world.toggles.battle_command = BattleCommandOpt::CrossButton;
    for _ in 0..13 {
        tick(&mut world, 0);
    }
    tick(&mut world, PadButton::Left.mask());
    assert!(world.battle.command_pad_buffer.is_none());
}

/// With a stage module paged in, the first zero-return pass is spent
/// clearing `ctx[+0x6B2]` (`0x801D0BA8..0x801D0BCC`). Driven at the gate:
/// a staged fight's side-band pass owns the battle tick.
#[test]
fn a_staged_battle_spends_one_more_pass() {
    for (stage_id, extra) in [(0u8, false), (2, true)] {
        let mut world = prompt_world();
        world.battle.stage_id = stage_id;
        for _ in 0..16 {
            world.step_action_plate_glides();
            assert!(world.battle_command_glide_gate());
        }
        assert_eq!(elapsed(&world), 16);
        world.step_action_plate_glides();
        assert_eq!(world.battle_command_glide_gate(), extra, "stage {stage_id}");
        assert_eq!(world.battle.widget_glide_latch, 0);
        world.step_action_plate_glides();
        assert!(!world.battle_command_glide_gate());
    }
}

/// `Begin` onto the ring runs sub-draw step 1, whose glides hold the pad
/// the same way: the ring's arms take no press until they have landed.
#[test]
fn the_ring_reads_no_pad_until_its_arms_land() {
    let mut world = prompt_world();
    for _ in 0..16 {
        tick(&mut world, 0);
    }
    tick(&mut world, PadButton::Left.mask());
    assert!(!on_prompt(&world), "Begin taken");
    assert!(world.battle.ring_open_glide.is_some_and(|g| !g.settled()));
    assert!(world.battle_command_pad_held());
    // Down is the ring's Spirit arm, a direct commit - once the ring is
    // live. Under the glide it is dropped.
    tick(&mut world, 0);
    tick(&mut world, PadButton::Down.mask());
    assert!(
        matches!(
            world.battle.command.as_ref().map(|c| &c.phase),
            Some(CommandPhase::Menu { .. })
        ),
        "no arm taken under the glide"
    );
    assert!(!world.battle.round_flow.committed(0), "no commit either");
}
