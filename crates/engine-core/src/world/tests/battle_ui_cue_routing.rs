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
    world.battle.arm_battle_tutorial();
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

/// The arts entry sounds the commit cue for every arrow it takes
/// (`0x801D1F80..0x801D1F98` into the tail `0x801D20D4`), the one the gauge
/// cannot pay included: HUD step `0xB` returns ahead of its append and the
/// tail still runs.
#[test]
fn an_entered_arts_arrow_sounds_the_commit_cue() {
    let mut world = ring_world();
    world.battle.command = None;
    world.open_arts_command_input(0);
    let arrow = |world: &mut World, button: PadButton| {
        world.input.set_pad(0);
        world.input.set_pad(button.mask());
        world.audio.sfx_ring_ops.clear();
        world.tick_battle_arts_input();
        std::mem::take(&mut world.audio.sfx_ring_ops)
    };
    assert_eq!(
        arrow(&mut world, PadButton::Left),
        vec![SfxRingOp::Push(0x20)]
    );
    let entry = world.battle.arts_input.as_mut().expect("entry open");
    assert_eq!(entry.buffer.len(), 1);
    // A gauge that cannot pay the next arrow: nothing is appended, the cue
    // still sounds.
    entry.pool = 0;
    assert_eq!(
        arrow(&mut world, PadButton::Up),
        vec![SfxRingOp::Push(0x20)]
    );
    let entry = world.battle.arts_input.as_ref().expect("entry open");
    assert_eq!(entry.buffer.len(), 1, "the unpaid arrow is not entered");
    // A frame with no press is silent.
    world.input.set_pad(0);
    world.audio.sfx_ring_ops.clear();
    world.tick_battle_arts_input();
    assert!(world.audio.sfx_ring_ops.is_empty());
}

/// The ring's Magic arm tests the member's Ra-Seru marker first
/// (`ctx[+0x25F + member]`, `beq v0,zero,0x801D2968` at `0x801D1420`): a
/// member with none is refused with cue `0x23` and the ring stays up.
#[test]
fn the_magic_arm_buzzes_for_a_member_with_no_raseru() {
    let mut world = ring_world();
    world.party.roster.members = vec![legaia_save::CharacterRecord::zeroed()];
    assert_eq!(
        press(&mut world, PadButton::Right),
        vec![SfxRingOp::Push(0x22)]
    );
    assert!(world.battle.spell_menu.is_none(), "no window opened");
    assert!(matches!(
        world.battle.command.as_ref().map(|s| &s.phase),
        Some(CommandPhase::Menu { .. })
    ));
    // The cross-button entry refuses the lit arm the same way.
    world.toggles.battle_command = BattleCommandOpt::CrossButton;
    press(&mut world, PadButton::Right);
    assert_eq!(
        press(&mut world, PadButton::Cross),
        vec![SfxRingOp::Push(0x22)]
    );
    assert!(world.battle.spell_menu.is_none());

    // A member who knows a spell keeps the arm with no Ra-Seru worn: a
    // state retail cannot hold and the port's Seru cheat can.
    let mut cheat = ring_world();
    let mut record = legaia_save::CharacterRecord::zeroed();
    legaia_engine_battle::magic_xp::learn_spell_prepend(&mut record, 0x81);
    cheat.party.roster.members = vec![record];
    assert_eq!(
        press(&mut cheat, PadButton::Right),
        vec![SfxRingOp::Push(0x20)]
    );
}

/// The Item arm's one refusal is the special-battle word's `0x100`
/// (`0x801D1370..0x801D137C`), answered through the same tail.
#[test]
fn the_item_arm_buzzes_under_the_item_forbidden_word() {
    let mut world = ring_world();
    world.battle.special_word = 0x100;
    assert_eq!(
        press(&mut world, PadButton::Up),
        vec![SfxRingOp::Push(0x22)]
    );
    assert!(world.battle.item_menu.is_none(), "no window opened");
    world.battle.special_word = 0;
    assert_eq!(
        press(&mut world, PadButton::Up),
        vec![SfxRingOp::Push(0x20)]
    );
    assert!(world.battle.item_menu.is_some());
}

/// The magic window's hand is the list kernel's (`FUN_80032A44`): Up / Down
/// always queue the step cue and wrap inside the eight-row page, Left /
/// Right flip the page and sound only when it flipped.
#[test]
fn the_spell_list_steps_as_the_list_kernel_does() {
    use legaia_engine_battle::battle_magic::{BattleSpellSession, SpellPhase};
    use legaia_engine_battle::spells::{SpellCatalog, SpellDef};

    let spell_world = |count: u8| {
        let mut world = ring_world();
        world.battle.command = None;
        let mut catalog = SpellCatalog::new();
        let ids: Vec<u8> = (1..=count).collect();
        for &id in &ids {
            catalog.insert(SpellDef {
                id,
                name: format!("Spell {id}"),
                ..SpellDef::default()
            });
        }
        world.battle.spell_menu = Some(BattleSpellSession::new(0, 0, &ids, &catalog, 99, 0));
        world.tables.spell_catalog = catalog;
        world
    };
    let step = |world: &mut World, button: PadButton| {
        world.input.set_pad(0);
        world.input.set_pad(button.mask());
        world.audio.sfx_ring_ops.clear();
        world.tick_battle_spell_menu();
        let cursor = match world.battle.spell_menu.as_ref().map(|m| &m.phase) {
            Some(SpellPhase::Select { cursor }) => *cursor,
            other => panic!("list not up: {other:?}"),
        };
        (cursor, std::mem::take(&mut world.audio.sfx_ring_ops))
    };
    let blip = vec![SfxRingOp::Push(0x21)];

    // One row: the hand has nowhere to go and the step still sounds.
    let mut one = spell_world(1);
    assert_eq!(step(&mut one, PadButton::Down), (0, blip.clone()));
    assert_eq!(step(&mut one, PadButton::Up), (0, blip.clone()));
    assert_eq!(step(&mut one, PadButton::Right), (0, vec![]));
    assert_eq!(step(&mut one, PadButton::Left), (0, vec![]));

    // Ten rows: two pages.
    let mut two = spell_world(10);
    assert_eq!(
        step(&mut two, PadButton::Up),
        (7, blip.clone()),
        "page wrap"
    );
    assert_eq!(step(&mut two, PadButton::Down), (0, blip.clone()));
    assert_eq!(step(&mut two, PadButton::Left), (0, vec![]), "first page");
    assert_eq!(step(&mut two, PadButton::Right), (8, blip.clone()));
    assert_eq!(step(&mut two, PadButton::Right), (8, vec![]), "last page");
    assert_eq!(step(&mut two, PadButton::Down), (9, blip.clone()));
    assert_eq!(
        step(&mut two, PadButton::Down),
        (8, blip.clone()),
        "short page"
    );
    assert_eq!(step(&mut two, PadButton::Left), (0, blip));
    let menu = two.battle.spell_menu.as_ref().expect("list up");
    assert_eq!(menu.browse_pad_toward(9), Some(PadButton::Right.mask()));
    assert_eq!(menu.browse_pad_toward(3), Some(PadButton::Down.mask()));
    assert_eq!(menu.browse_pad_toward(0), None);
}
