//! The battle open holds the first round in every fight: flow `0x00` and
//! `0x0A` run after the entry sweep's park, `0x0B` drains the intro timer
//! under the enemy names, and only its expiry lets the flow reach `0x14`
//! (`FUN_801D0748`, `0x801D0DD0..0x801D0EBC`). Measured on retail:
//! `gaza2_card_battle_init` and a `karisto_sol_pre_encounter` encounter both
//! store `0x0A` / `0x0B` at `+3` / `+6` vsyncs past the park, expire at `+97`
//! and read `0x1E` at `+99`.

use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot};
use legaia_engine_battle_menus::battle_flow::BattleFlowState;
use legaia_engine_battle_menus::battle_open::{BANNER_FRAMES, PLAIN_OPEN_FRAMES};

use super::super::battle::INTRO_LEAD_TICKS;
use super::*;

fn open_world() -> World {
    let mut world = World::new();
    world.toggles.live_gameplay_loop = true;
    world.battle.player_driven = true;
    world.enter_battle_from_formation(&FormationDef::new(7, vec![FormationSlot::new(1)]));
    world.battle.camera = None;
    world
}

/// No sweep in the way: the ticks from the first pass to the prompt are the
/// lead, the hold and the release.
#[test]
fn the_prompt_waits_for_the_lead_and_the_hold() {
    let mut world = open_world();
    assert!(
        world.battle.round_open_pending,
        "the entry's round start waits"
    );
    let hold = u32::from(INTRO_LEAD_TICKS) + u32::from(PLAIN_OPEN_FRAMES);
    for t in 0..hold {
        assert!(world.battle.command.is_none(), "no prompt at tick {t}");
        assert_eq!(world.battle.flow, BattleFlowState::Idle, "tick {t}");
        world.live_battle_tick();
    }
    assert_eq!(world.battle.intro_names_frames, 0);
    world.live_battle_tick();
    assert!(!world.battle.round_open_pending);
    assert_eq!(world.battle.flow, BattleFlowState::TurnPrompt);
    assert!(world.battle.command.is_some(), "Begin | Run is up");
}

/// A later round start owes the open nothing.
#[test]
fn a_later_round_is_not_held() {
    let mut world = open_world();
    for _ in 0..u32::from(INTRO_LEAD_TICKS) + u32::from(PLAIN_OPEN_FRAMES) + 1 {
        world.live_battle_tick();
    }
    assert!(!world.battle.battle_open_held());
    world.battle.command = None;
    world.begin_battle_round();
    assert!(!world.battle.round_open_pending);
    assert!(world.battle.command.is_some());
}

/// The sweep owns the opening first: nothing of the lead runs under it.
#[test]
fn the_lead_waits_out_the_entry_sweep() {
    use legaia_engine_battle_vm::battle_cam_script::{BattleCamPhase, BattleCamera};
    let mut world = open_world();
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    cam.start_entry_sweep();
    world.battle.camera = Some(cam);
    for _ in 0..8 {
        world.live_battle_tick();
    }
    assert_eq!(world.battle.intro_lead_ticks, INTRO_LEAD_TICKS);
    assert_eq!(world.battle.intro_names_frames, PLAIN_OPEN_FRAMES);
}

/// The formation banner is flow `0x0A`'s, so it stands with the names; an
/// ambush takes `0x0B`'s jump to `0xFE` only on the hold's expiry.
#[test]
fn an_ambush_raises_its_banner_after_the_lead_and_strikes_after_the_hold() {
    use legaia_engine_battle_loop::round_flow::RoundPhase;
    let mut world = open_world();
    world.battle_ctx.formation_advantage = 1;
    world.battle.tutorial_boxes.clear();
    world.open_battle_formation();
    world.begin_battle_round();
    assert!(world.battle.round_open_pending);
    assert_eq!(world.battle.intro_names_frames, BANNER_FRAMES);
    assert!(world.battle.tutorial_boxes.is_empty(), "no banner yet");
    for _ in 0..INTRO_LEAD_TICKS {
        world.live_battle_tick();
    }
    assert!(!world.battle.tutorial_boxes.is_empty(), "banner up");
    assert_ne!(world.battle.round_flow.phase, RoundPhase::Execute);
}
