//! The hand-off between two actors of a battle round, measured on the town01
//! sparring fight on retail: every turn's `0x50` re-picks and raises the
//! side-band stage, whose read sets `0x51`'s length (`76` vsyncs at frame
//! step `2`); a turn with no action still runs the whole Done band; and the
//! round boundary spends retail's passes before the next round starts.

use super::*;
use legaia_engine_battle_vm::battle_action::{ActionState, sideband_turn_seed_vsyncs};
use legaia_engine_battle_vm::status_effects::StatusKind;

/// A lone fighter against one monster, driven with no pad.
fn auto_fight() -> World {
    let mut world = World::new();
    world.enter_battle(1, 1);
    world.toggles.live_gameplay_loop = true;
    world.battle.player_driven = false;
    world.battle.speed[0] = 10;
    world.battle.speed[1] = 10;
    world.seed_battle_initiative();
    world
}

/// Tick until `want` holds, within a budget.
fn tick_until(world: &mut World, budget: u32, want: impl Fn(&World) -> bool) -> u32 {
    for n in 0..budget {
        if want(world) {
            return n;
        }
        world.tick();
    }
    panic!("the wait for the state never ended");
}

fn in_done_fade_down(world: &World) -> bool {
    world.battle_ctx.action_state == ActionState::DoneFadeDown.as_byte()
}

#[test]
fn the_re_pick_at_cleanup_raises_the_stage_for_every_turn() {
    let mut world = auto_fight();
    // The opening turn's fade-down.
    tick_until(&mut world, 900, in_done_fade_down);
    tick_until(&mut world, 900, |w| !in_done_fade_down(w));
    // The second turn is a pick in the round's execution band: the stage
    // is up as its Done band starts, and it stands for the read's time.
    tick_until(&mut world, 900, in_done_fade_down);
    assert_ne!(world.battle_ctx.menu_open, 0, "the stage is up in 0x51");
    assert_eq!(
        world.casting.sideband_read_left,
        Some(sideband_turn_seed_vsyncs(world.battle_ctx.summon_frame_idx)),
        "the pick seeds the read's vsyncs on the pass that ran `0x50`"
    );
}

#[test]
fn the_side_band_stage_falls_during_the_fade_down_and_stops_holding_it() {
    let mut world = auto_fight();
    tick_until(&mut world, 900, in_done_fade_down);
    tick_until(&mut world, 900, |w| !in_done_fade_down(w));
    tick_until(&mut world, 900, in_done_fade_down);
    let mut dwell = 0u32;
    while in_done_fade_down(&world) {
        world.tick();
        dwell += 1;
        assert!(dwell < 400, "the Done band must not park");
    }
    assert_eq!(
        world.battle_ctx.menu_open, 0,
        "the stage is down at the exit"
    );
    assert!(
        dwell >= u32::from(sideband_turn_seed_vsyncs(0)),
        "the countdown floor held the band for the read ({dwell} ticks)"
    );
}

#[test]
fn a_blocked_actors_turn_runs_the_done_band_instead_of_being_skipped() {
    let mut world = auto_fight();
    world
        .battle
        .status_effects
        .apply_with_duration(1, StatusKind::Sleep, 255);
    let mut saw_slot1_done = false;
    for _ in 0..1800 {
        if !world.battle.status_effects.has(1, StatusKind::Sleep) {
            world
                .battle
                .status_effects
                .apply_with_duration(1, StatusKind::Sleep, 255);
        }
        world.tick();
        if world.battle_ctx.active_actor == 1 && in_done_fade_down(&world) {
            saw_slot1_done = true;
            break;
        }
    }
    assert!(
        saw_slot1_done,
        "the sleeping monster's turn reached the Done band (category 0)"
    );
    assert_eq!(world.actors[1].battle.action_category, 0);
}

#[test]
fn a_finished_round_waits_three_passes_before_the_next_round_starts() {
    let mut world = auto_fight();
    tick_until(&mut world, 3000, |w| w.battle.round_flow.boundary_hold > 0);
    let round_before = world.battle_mode();
    assert_eq!(world.battle.round_flow.boundary_hold, 3);
    // The band's own closing glides have landed by the `0x14` pass, so it
    // dispatches on its first pass: three passes, none of them gated.
    let mut ticks = 0u32;
    while world.battle.round_flow.boundary_hold > 0 {
        world.tick();
        ticks += 1;
        assert!(ticks < 200, "the boundary must end");
    }
    // Retail at the battle's step of `2`: `0x51` exits, then `0x5A`, `0xFF`
    // and the `0x14` dispatch, six vsyncs (`party_basic_attack_vs_gobu_gobu`,
    // `autorun_round_boundary_hold.lua`).
    assert_eq!(ticks, 6, "three passes of two vsyncs");
    assert_eq!(
        world.battle_mode(),
        round_before + 1,
        "the 0xFF round bump lands when the boundary's passes are over"
    );
}

/// The boundary's `0x14` pass dispatches only once the menu SM's glide gate
/// opens: a glide still in flight as the pass before it ended - the Done
/// band's closing ones, at a round's end - holds the round start a pass.
#[test]
fn a_closing_glide_still_live_holds_the_next_round_start() {
    use legaia_engine_battle_loop::battle_state::ActionHudClose;
    use legaia_engine_battle_vm::battle_commit_log::LogLaunch;
    let mut world = auto_fight();
    tick_until(&mut world, 3000, |w| w.battle.round_flow.boundary_hold > 0);
    // Count the passes the boundary takes with a closing glide that lands
    // only after the `0xFF` pass.
    let mut glide = LogLaunch::new(true);
    glide.elapsed = 0;
    world.battle.action_hud_close = Some(ActionHudClose {
        glide,
        plaque: None,
        bar: None,
        panels: false,
        spirit: None,
    });
    let mut ticks = 0u32;
    let mut held_at_one = 0u32;
    while world.battle.round_flow.boundary_hold > 0 {
        if world.battle.round_flow.boundary_hold == 1 {
            held_at_one += 1;
        }
        world.tick();
        ticks += 1;
        assert!(ticks < 200, "the boundary must end");
    }
    assert!(
        world
            .battle
            .action_hud_close
            .is_some_and(|c| c.glide.settled()),
        "the round started after the closing glide landed"
    );
    assert!(
        held_at_one > 2,
        "the 0x14 pass waited for the glide ({held_at_one} ticks)"
    );
}

/// A capture drive crosses the command band in a few ticks, so it lands the
/// round-start pick's read there; outside the band the read stands.
#[test]
fn a_capture_drive_lands_the_read_only_in_the_command_band() {
    use legaia_engine_battle_loop::round_flow::RoundPhase;
    let mut world = World::new();
    world.battle_ctx.menu_open = 1;
    world.casting.sideband_read_left = Some(64);

    world.battle.round_flow.phase = RoundPhase::Execute;
    world.land_command_band_sideband_read();
    assert_eq!(
        world.battle_ctx.menu_open, 1,
        "the execute band keeps its read"
    );

    world.battle.round_flow.phase = RoundPhase::Command;
    world.land_command_band_sideband_read();
    assert_eq!(world.battle_ctx.menu_open, 0);
    assert_eq!(world.casting.sideband_read_left, None);
}
