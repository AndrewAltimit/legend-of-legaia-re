//! The arts entry's Rot direction gate (`FUN_801D0748`,
//! `0x801D1E60..0x801D1F7C`) on the live World: a rotted limb's direction is
//! refused with cue `0x23`, every rolled limb counts, and the other
//! directions still enter.

use super::*;
use crate::input::PadButton;
use legaia_engine_vm::status_effects::StatusKind;

fn arts_world() -> World {
    let mut world = World {
        party: crate::world::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.liveness = 1;
    world.open_arts_command_input(0);
    world
}

fn press(world: &mut World, button: PadButton) -> usize {
    world.input.set_pad(0);
    world.input.set_pad(button.mask());
    world.audio.battle_sfx_cues.clear();
    world.tick_battle_arts_input();
    world
        .battle
        .arts_input
        .as_ref()
        .expect("entry open")
        .buffer
        .len()
}

#[test]
fn rotted_legs_refuse_up_and_down_with_the_buzz() {
    let mut world = arts_world();
    world.battle.status_effects.apply(0, StatusKind::Rot);
    world.battle.status_effects.set_rot_limb(0, 2);
    assert_eq!(press(&mut world, PadButton::Up), 0, "Up refused");
    assert_eq!(world.audio.battle_sfx_cues.len(), 1);
    assert_eq!(world.audio.battle_sfx_cues[0].kind, 0x23);
    assert_eq!(press(&mut world, PadButton::Down), 0, "Down refused");
    assert_eq!(press(&mut world, PadButton::Left), 1, "an arm still enters");
    assert!(world.audio.battle_sfx_cues.is_empty());
}

#[test]
fn every_rolled_limb_is_refused() {
    let mut world = arts_world();
    world.battle.status_effects.apply(0, StatusKind::Rot);
    world.battle.status_effects.set_rot_limb(0, 0);
    world.battle.status_effects.set_rot_limb(0, 1);
    assert_eq!(press(&mut world, PadButton::Left), 0);
    assert_eq!(press(&mut world, PadButton::Right), 0);
    assert_eq!(press(&mut world, PadButton::Up), 1, "the legs are sound");
}

fn ring_world() -> World {
    let mut world = arts_world();
    world.battle.arts_input = None;
    world.battle.command = Some(crate::battle_input::BattleCommandSession::new(0, 0));
    world
}

fn ring_press(world: &mut World, button: PadButton) -> crate::battle_input::CommandPhase {
    world.input.set_pad(0);
    world.input.set_pad(button.mask());
    world.audio.battle_sfx_cues.clear();
    world.tick_battle_command();
    world
        .battle
        .command
        .as_ref()
        .map(|s| s.phase.clone())
        .expect("ring still open")
}

/// `0x801D1560..0x801D156C`: all three limbs rotted refuse the Attack arm
/// with the buzz; two limbs still open it.
#[test]
fn a_fully_rotted_body_cannot_open_attack() {
    use crate::battle_input::CommandPhase;
    let mut world = ring_world();
    world.battle.status_effects.apply(0, StatusKind::Rot);
    for limb in 0..3 {
        world.battle.status_effects.set_rot_limb(0, limb);
    }
    assert!(matches!(
        ring_press(&mut world, PadButton::Left),
        CommandPhase::Menu { .. }
    ));
    assert_eq!(world.audio.battle_sfx_cues.len(), 1);

    let mut two = ring_world();
    two.battle.status_effects.apply(0, StatusKind::Rot);
    two.battle.status_effects.set_rot_limb(0, 0);
    two.battle.status_effects.set_rot_limb(0, 1);
    assert!(matches!(
        ring_press(&mut two, PadButton::Left),
        CommandPhase::AttackMode { .. }
    ));
    assert!(two.audio.battle_sfx_cues.is_empty());
}

/// `0x801D1434..0x801D1440`: Curse refuses the Magic arm with the buzz.
#[test]
fn curse_refuses_the_magic_arm() {
    use crate::battle_input::CommandPhase;
    let mut world = ring_world();
    world.battle.status_effects.apply(0, StatusKind::Curse);
    assert!(matches!(
        ring_press(&mut world, PadButton::Right),
        CommandPhase::Menu { .. }
    ));
    assert_eq!(world.audio.battle_sfx_cues.len(), 1);
    assert_eq!(world.audio.battle_sfx_cues[0].kind, 0x23);
}
