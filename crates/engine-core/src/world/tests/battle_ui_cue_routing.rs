//! Command-surface blips on the live World, disc-free: the cross-button
//! entry's highlight walk and the sparring tutorial's wrong-lesson rewind.

use super::*;
use legaia_engine_battle::battle_tutorial::TutorialLesson;
use legaia_engine_battle_menus::battle_input::{BattleCommandSession, CommandPhase};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::PadButton;
use legaia_engine_system::option_values::BattleCommandOpt;
use legaia_engine_system::sound_state::SfxRingOp;

fn ring_world() -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.battle.player_driven = true;
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.liveness = 1;
    world.battle.command = Some(BattleCommandSession::new(0, 0));
    world
}

fn press(world: &mut World, button: PadButton) -> Vec<SfxRingOp> {
    world.input.set_pad(0);
    world.input.set_pad(button.mask());
    world.audio.sfx_ring_ops.clear();
    world.tick_battle_command();
    std::mem::take(&mut world.audio.sfx_ring_ops)
}

/// `FUN_8004FCC8(0x22)` on a highlight move rings `0x21`; a press on the lit
/// arm is silent; the confirm commits the lit arm with `0x21` -> `0x20`.
#[test]
fn the_cross_button_ring_steps_its_highlight_with_the_cursor_cue() {
    let mut world = ring_world();
    world.toggles.battle_command = BattleCommandOpt::CrossButton;
    assert_eq!(press(&mut world, PadButton::Left), vec![], "Left is lit");
    assert_eq!(
        press(&mut world, PadButton::Up),
        vec![SfxRingOp::Push(0x21)]
    );
    assert!(matches!(
        world.battle.command.as_ref().map(|s| &s.phase),
        Some(CommandPhase::Menu { .. })
    ));
    assert_eq!(
        press(&mut world, PadButton::Cross),
        vec![SfxRingOp::Push(0x20)]
    );
    assert!(world.battle.item_menu.is_some(), "the lit Item arm opened");

    // The directional entry commits on the press itself.
    let mut direct = ring_world();
    assert_eq!(
        press(&mut direct, PadButton::Up),
        vec![SfxRingOp::Push(0x20)]
    );
    assert!(direct.battle.item_menu.is_some());
}

/// Retail's tutorial hook (overlay 967, `FUN_801F6B70`) runs on the flow
/// state the press already committed, and neither it nor its rewind
/// `FUN_801F7628` calls a cue routine: a wrong-lesson pick sounds the
/// commit cue and nothing else.
#[test]
fn a_rejected_tutorial_pick_sounds_only_its_commit() {
    let mut world = ring_world();
    world.arm_battle_tutorial();
    world.battle.tutorial.as_mut().expect("armed").lesson = TutorialLesson::Attacks.raw();
    // The Item arm during the attack lesson: committed, then rewound.
    assert_eq!(
        press(&mut world, PadButton::Up),
        vec![SfxRingOp::Push(0x20)]
    );
    assert!(
        world.battle.item_menu.is_none(),
        "the hook rewound the pick"
    );
    assert!(world.battle.command.is_some(), "the ring is back up");
}
