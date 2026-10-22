//! The menu SM's pre-dispatch gate under the round prompt's open glide:
//! `FUN_801D0748` runs the glide stepper ahead of its state switch and
//! dispatches nothing while it returns non-zero (`0x801D0B2C..0x801D0BA4`),
//! parking one press made near the end in `ctx[+0x884]`
//! (`0x801D0B5C..0x801D0B98`) for the first dispatched pass
//! (`0x801D0C1C..0x801D0C58`).
//!
//! These are the cases that read the battle HUD's projections; the ones that
//! read only the `World` stay in engine-core's `world::tests::round_prompt_gate`.

use legaia_engine_battle_hud::battle_chip_dpad_visible;
use legaia_engine_battle_menus::battle_input::{BattleCommandSession, CommandPhase};
use legaia_engine_core::world::test_drivers::BattleTestDrivers;
use legaia_engine_core::world::*;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::PadButton;

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

/// A press while the chips are still far from their seats is neither taken
/// nor parked: the return is `6` or more.
#[test]
fn an_early_press_is_dropped() {
    let mut world = prompt_world();
    tick(&mut world, 0);
    tick(&mut world, PadButton::Left.mask());
    assert!(on_prompt(&world), "no state dispatched under the glide");
    assert!(world.command_pad_buffer().is_none());
    for _ in 0..30 {
        tick(&mut world, 0);
    }
    assert!(on_prompt(&world), "the dropped press never arrives");
    assert!(battle_chip_dpad_visible(&world));
}

/// The prompt takes nothing for the sixteen passes of the glide, the
/// landing pass included; a press on the pass after is live.
#[test]
fn the_first_live_pass_is_the_one_after_the_landing() {
    let mut world = prompt_world();
    for _ in 0..15 {
        tick(&mut world, 0);
    }
    assert_eq!(elapsed(&world), 15);
    // Pass 16 lands the glide and still exits: no glyph, no dispatch.
    tick(&mut world, 0);
    assert_eq!(elapsed(&world), 16);
    assert_eq!(world.battle.widget_glide_return, World::GLIDE_BLOCKING_FLAG);
    assert!(!battle_chip_dpad_visible(&world));
    assert!(on_prompt(&world));
    // Pass 17 dispatches the state.
    tick(&mut world, PadButton::Left.mask());
    assert!(!on_prompt(&world), "Begin taken on the first live pass");
    // ...and the ring it opens is under its own glide: no glyph again.
    assert!(!battle_chip_dpad_visible(&world));
}
