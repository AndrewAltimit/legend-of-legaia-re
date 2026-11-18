//! The command flow's held window (`FUN_801D0748`, `s2 = _DAT_8007B874 |
//! _DAT_8007B938` at `0x801D0B0C..0x801D0B20`): a button held across the pad
//! pump's 32-vsync window reads as pressed on every pass, so a held direction
//! repeats in the arts entry and walks on through the ring.

use super::*;
use legaia_engine_battle_menus::battle_input::{BattleCommandSession, CommandPhase};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::PadButton;

fn battle_world() -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.liveness = 1;
    world
}

/// One vsync of a held button: the pump's tick, then the surface.
fn hold_tick(world: &mut World, button: PadButton, surface: fn(&mut World)) {
    world.clock.display_frames += 1;
    world.input.set_pad(button.mask());
    surface(world);
}

fn entered(world: &World) -> usize {
    world
        .battle
        .arts_input
        .as_ref()
        .expect("entry open")
        .buffer
        .len()
}

#[test]
fn a_held_direction_records_once_a_pass_after_the_window() {
    let mut world = battle_world();
    world.open_arts_command_input(0);
    world.input.set_pad(0);
    let step = world.battle.frame_clock.span();
    let mut counts = Vec::new();
    for _ in 0..48 {
        hold_tick(&mut world, PadButton::Left, World::tick_battle_arts_input);
        counts.push(entered(&world));
    }
    // The press itself records one.
    assert_eq!(counts[0], 1);
    // Nothing more until the window has seen the button on every slot.
    assert!(counts[..30].iter().all(|&n| n == 1), "{counts:?}");
    // Then one a battle frame, never on two ticks of the same frame, until
    // the pool is spent; the held direction keeps the entry open.
    assert_eq!(counts[47], 3, "{counts:?}");
    assert!(matches!(
        world.battle.arts_input.as_ref().map(|s| &s.phase),
        Some(legaia_engine_battle::arts_command_input::ArtsInputPhase::Entering)
    ));
    let grew: Vec<usize> = (1..counts.len())
        .filter(|&i| counts[i] > counts[i - 1])
        .collect();
    for w in grew.windows(2) {
        assert!(w[1] - w[0] >= step as usize, "{grew:?} on step {step}");
    }
}

#[test]
fn a_released_button_stops_the_repeat() {
    let mut world = battle_world();
    world.open_arts_command_input(0);
    for _ in 0..33 {
        hold_tick(&mut world, PadButton::Left, World::tick_battle_arts_input);
    }
    world.clock.display_frames += 1;
    world.input.set_pad(0);
    world.tick_battle_arts_input();
    let after_release = entered(&world);
    for _ in 0..12 {
        world.clock.display_frames += 1;
        world.input.set_pad(0);
        world.tick_battle_arts_input();
    }
    assert_eq!(entered(&world), after_release);
}

#[test]
fn a_held_left_walks_the_round_prompt_on_to_the_attack_mode() {
    let mut world = battle_world();
    let mut session = BattleCommandSession::new(0, 0);
    session.phase = CommandPhase::RoundPrompt { cursor: 0 };
    world.battle.command = Some(session);
    world.input.set_pad(0);
    let phase = |w: &World| w.battle.command.as_ref().map(|s| s.phase.clone());
    hold_tick(&mut world, PadButton::Left, World::tick_battle_command);
    // The press takes Begin and opens the ring.
    assert!(matches!(phase(&world), Some(CommandPhase::Menu { .. })));
    // One neutral pump and 31 held ones: the window is one slot short.
    for _ in 0..30 {
        hold_tick(&mut world, PadButton::Left, World::tick_battle_command);
    }
    assert!(matches!(phase(&world), Some(CommandPhase::Menu { .. })));
    for _ in 0..4 {
        hold_tick(&mut world, PadButton::Left, World::tick_battle_command);
    }
    // The window then reads Left on the ring: its Attack arm opens the
    // Auto | Command prompt, as on retail (`0x28 -> 0x78`).
    assert!(
        !matches!(phase(&world), Some(CommandPhase::Menu { .. })),
        "{:?}",
        phase(&world)
    );
}
