//! The party-wide commit confirm (`0x6E`): raised once after the last
//! member's commit, `Begin` plays the round, `Reselect` steps back onto the
//! last member's ring and refunds a committed item. A pad-driven ladder
//! through `World::tick_battle_command`, the seam both hosts tick.
//!
//! These are the cases that read the battle HUD's projections; the ones that
//! read only the `World` stay in engine-core's `world::tests::commit_confirm`.

use legaia_engine_battle_hud::battle_command_chips;
use legaia_engine_battle_menus::battle_flow::BattleFlowState;
use legaia_engine_battle_menus::battle_input::CommandChipPhase;
use legaia_engine_battle_menus::battle_input::{BattleCommandSession, CommandPhase};
use legaia_engine_core::battle_round::PendingPartyAction;
use legaia_engine_core::world::test_drivers::BattleTestDrivers;
use legaia_engine_core::world::*;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::PadButton;

fn party_world(party: u8) -> World {
    let mut world = World::new();
    world.party.party_count = party;
    world.battle.player_driven = true;
    world.toggles.live_gameplay_loop = true;
    world.mode = SceneMode::Battle;
    for i in 0..usize::from(party) + 2 {
        world.actors[i].active = true;
        world.actors[i].battle.liveness = 1;
        world.actors[i].battle.hp = 100;
        world.actors[i].battle.max_hp = 100;
    }
    world
}

fn commit_spirit(world: &mut World, actor: u8) {
    world.battle.command = Some(BattleCommandSession {
        actor,
        party_slot: actor,
        no_escape: false,
        phase: CommandPhase::SpiritGuard,
    });
    world.tick_battle_command();
}

fn press(world: &mut World, button: PadButton) {
    world.set_pad(0);
    world.set_pad(button.mask());
    world.tick_battle_command();
}

fn on_confirm(world: &World) -> bool {
    matches!(
        world.battle.command.as_ref().map(|c| &c.phase),
        Some(CommandPhase::CommitConfirm { .. })
    )
}

#[test]
fn the_screen_waits_for_the_last_member_then_begin_plays_the_round() {
    let mut world = party_world(3);
    commit_spirit(&mut world, 0);
    assert!(!on_confirm(&world), "member 1 still owes a command");
    commit_spirit(&mut world, 1);
    assert!(!on_confirm(&world), "member 2 still owes a command");
    commit_spirit(&mut world, 2);
    assert!(on_confirm(&world), "the last commit raises 0x6E");
    assert_eq!(world.battle.flow, BattleFlowState::CommitBegin);
    let chips = battle_command_chips(&world).expect("the confirm draws");
    assert_eq!(chips.phase, CommandChipPhase::CommitConfirm);
    assert_eq!(chips.chips.len(), 2);
    assert_eq!(chips.cursor, 0, "the highlight opens on Begin");

    // Cross takes the highlighted Begin: the round plays out.
    press(&mut world, PadButton::Cross);
    assert!(world.battle.command.is_none(), "Begin leaves the screen");
    assert_eq!(world.battle.flow, BattleFlowState::Idle);
    // The Spirit turns play through the action SM's spirit band one after
    // another, so the round runs over frames.
    for _ in 0..3000 {
        if (0..3).all(|s| world.battle.ap_gauges[s].spirit_charged) {
            break;
        }
        world.tick();
    }
    for slot in 0..3 {
        assert!(
            world.battle.ap_gauges[slot].spirit_charged,
            "slot {slot} acted"
        );
    }
}

#[test]
fn the_commit_log_lists_every_member_the_cursor_walked_past() {
    use legaia_engine_battle_hud::battle_commit_log;
    use legaia_engine_battle_vm::battle_commit_log as log;
    use legaia_engine_battle_vm::battle_commit_log::CommitLogTarget;
    let mut world = party_world(3);
    world.battle.round_flow.phase = legaia_engine_core::battle_round::RoundPhase::Command;
    assert!(battle_commit_log(&world).is_empty());
    commit_spirit(&mut world, 0);
    let rows = battle_commit_log(&world);
    assert_eq!(rows.len(), 1, "member 0 committed, member 1 entering");
    assert_eq!(rows[0].command_record, log::RECORD_CHIP_SPIRIT);
    assert_eq!(rows[0].target, CommitLogTarget::None);
    world.commit_party_command(1, PendingPartyAction::Attack { target: 3 });
    commit_spirit(&mut world, 2);
    assert!(on_confirm(&world));
    let rows = battle_commit_log(&world);
    assert_eq!(rows.len(), 3, "the 0x6E screen shows the whole party");
    assert_eq!(rows[1].command_record, log::RECORD_CHIP_ATTACK);
    assert!(matches!(rows[1].target, CommitLogTarget::Single(_)));

    // Reselect takes the last member's row down with its commit.
    press(&mut world, PadButton::Circle);
    assert_eq!(battle_commit_log(&world).len(), 2);

    // The round's start removes the log.
    commit_spirit(&mut world, 2);
    press(&mut world, PadButton::Cross);
    assert!(battle_commit_log(&world).is_empty());
}

/// Leaving the ring for the `Auto | Command` prompt launches the log off the
/// left edge over sixteen frames (`FUN_801D388C` step `0x2A`, mode `0`);
/// cancelling back onto the ring slides it home (step `0x2B`, mode `1`).
#[test]
fn the_commit_log_launches_off_screen_for_the_attack_prompt_and_back() {
    use legaia_engine_battle_hud::battle_commit_log;
    use legaia_engine_battle_menus::battle_input::BattleCommand;
    let mut world = party_world(2);
    // One vsync a frame, so each step is one of the glide's sixteen.
    world.clock.frame_step = 1;
    world.battle.round_flow.phase = legaia_engine_core::battle_round::RoundPhase::Command;
    commit_spirit(&mut world, 0);
    let attack = BattleCommand::MENU
        .iter()
        .position(|c| *c == BattleCommand::Attack)
        .unwrap() as u8;
    world.battle.command.as_mut().unwrap().phase = CommandPhase::Menu { cursor: attack };
    assert_eq!(
        battle_commit_log(&world)[0].slide_x,
        0,
        "at rest on the ring"
    );

    press(&mut world, PadButton::Cross);
    assert!(matches!(
        world.battle.command.as_ref().map(|c| &c.phase),
        Some(CommandPhase::AttackMode { .. })
    ));
    let launch = world
        .battle
        .commit_log_launch
        .expect("the prompt launches the log");
    assert!(!launch.inbound);
    world.step_commit_log_launch();
    assert_eq!(battle_commit_log(&world)[0].slide_x, -20);
    for _ in 0..15 {
        world.step_commit_log_launch();
    }
    assert!(
        battle_commit_log(&world).is_empty(),
        "a landed outbound launch has the log off screen"
    );

    press(&mut world, PadButton::Circle);
    let launch = world
        .battle
        .commit_log_launch
        .expect("the cancel brings it back");
    assert!(launch.inbound);
    assert_eq!(battle_commit_log(&world)[0].slide_x, -0x140);
    for _ in 0..16 {
        world.step_commit_log_launch();
    }
    assert!(world.battle.commit_log_launch.is_none());
    assert_eq!(battle_commit_log(&world)[0].slide_x, 0);
}

/// The action seed's record-7 open glides the active-actor bar up from
/// `y = 234` to its rest row `192` over sixteen vsyncs - one `elapsed` unit
/// per engine tick whatever the frame step (`FUN_801D9BBC` adds the step per
/// battle pass, and a pass spans that many vsyncs). A monster's cast on a
/// party member raises it; a party member's attack on a monster does not.
#[test]
fn the_seed_glides_the_readout_bar_in_over_sixteen_ticks() {
    use legaia_engine_battle_hud::{battle_action_plaque_dy, battle_readout_bar_dy};
    use legaia_engine_battle_vm::battle_action::ActionCategory;
    use legaia_engine_battle_vm::battle_cue_group::HUD_CASTER_BANNER;
    let mut world = party_world(1);
    world.clock.frame_step = 2;
    world.battle.command = None;
    world.battle_ctx.active_actor = 1;
    world.battle_ctx.action_state = 0x6E;
    world.actors[1].battle.action_category = ActionCategory::Magic.as_byte();
    world.actors[1].battle.active_target = 0;
    world.note_action_plate_raise(HUD_CASTER_BANNER, 0);
    assert_eq!(battle_readout_bar_dy(&world), 42, "spawned at seat A");
    assert_eq!(battle_action_plaque_dy(&world), -38);
    for _ in 0..10 {
        world.step_action_plate_glides();
    }
    // Ten of sixteen: `234 - 42 * 10 / 16`, the row
    // `nivora_duel_mid_blazing_slash` holds its bar on.
    assert_eq!(battle_readout_bar_dy(&world), 42 - 42 * 10 / 16);
    for _ in 0..6 {
        world.step_action_plate_glides();
    }
    assert_eq!(battle_readout_bar_dy(&world), 0, "landed");

    world.battle_ctx.active_actor = 0;
    world.actors[0].battle.action_category = ActionCategory::Attack.as_byte();
    world.actors[0].battle.active_target = 1;
    world.note_action_plate_raise(HUD_CASTER_BANNER, 0);
    assert!(
        world.battle.readout_bar_glide.is_none(),
        "a monster target raises no bar"
    );
}

/// The retail-compare seat for a capture whose HUD glides had all landed:
/// every plate in flight snaps onto its rest seat.
#[test]
fn landing_the_hud_glides_puts_every_plate_at_rest() {
    use legaia_engine_battle_hud::{battle_action_plaque_dy, battle_readout_bar_dy};
    use legaia_engine_battle_vm::battle_action::ActionCategory;
    use legaia_engine_battle_vm::battle_cue_group::HUD_CASTER_BANNER;
    let mut world = party_world(1);
    world.battle.command = None;
    world.battle_ctx.active_actor = 1;
    world.battle_ctx.action_state = 0x6E;
    world.actors[1].battle.action_category = ActionCategory::Magic.as_byte();
    world.actors[1].battle.active_target = 0;
    world.note_action_plate_raise(HUD_CASTER_BANNER, 0);
    world.step_action_plate_glides();
    assert_ne!(battle_readout_bar_dy(&world), 0);
    world.land_battle_hud_glides();
    assert_eq!(battle_readout_bar_dy(&world), 0);
    assert_eq!(battle_action_plaque_dy(&world), 0);
}

/// The retail-compare seat for a capture whose HUD glides were in flight:
/// a record's target seat names the widget, and the engine's glide takes
/// the record's elapsed.
#[test]
fn seating_a_hud_glide_matches_the_widget_by_its_target_seat() {
    use legaia_engine_battle_hud::{battle_action_plaque_dy, battle_readout_bar_dy};
    use legaia_engine_battle_vm::battle_action::ActionCategory;
    use legaia_engine_battle_vm::battle_cue_group::HUD_CASTER_BANNER;
    let mut world = party_world(1);
    world.battle.command = None;
    world.battle_ctx.active_actor = 1;
    world.battle_ctx.action_state = 0x6E;
    world.actors[1].battle.action_category = ActionCategory::Magic.as_byte();
    world.actors[1].battle.active_target = 0;
    world.note_action_plate_raise(HUD_CASTER_BANNER, 0);
    world.land_battle_hud_glides();
    assert_eq!(battle_readout_bar_dy(&world), 0);
    assert!(world.seat_battle_hud_glide([16, 192], 6));
    assert_eq!(world.battle.readout_bar_glide.unwrap().elapsed, 6);
    assert_ne!(battle_readout_bar_dy(&world), 0, "the bar is back mid-rise");
    assert!(world.seat_battle_hud_glide([16, 12], 0));
    assert_ne!(battle_action_plaque_dy(&world), 0);
    assert!(
        !world.seat_battle_hud_glide([123, 148], 4),
        "a seat no engine glide owns is left alone"
    );
}
