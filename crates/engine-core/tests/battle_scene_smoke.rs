//! Smoke test: drive the battle-action state machine via `World::tick` for
//! many frames and assert the SM makes forward progress without hitting
//! `UnknownState`. No disc data needed — this exercises the engine
//! integration layer end-to-end.

use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::battle_action::{ActionState, BattleEndCause, StepOutcome};

fn build_world(queued_action: u8) -> World {
    let mut world = World {
        mode: SceneMode::Battle,
        party_count: 3,
        ..World::default()
    };
    // 3 party + 5 monsters, all alive.
    for i in 0..8 {
        let actor = world.spawn_actor(i);
        actor.battle.liveness = 1;
    }
    world.battle_ctx.queued_action = queued_action;
    world.battle_ctx.action_state = ActionState::Begin.as_byte();
    world
}

#[test]
fn battle_world_ticks_many_frames_without_unknown_state() {
    let mut world = build_world(3); // queued action = 3 (Attack)
    let mut transitions = 0u32;
    let mut stays = 0u32;
    let mut completes = 0u32;
    let mut unknowns = 0u32;
    for _ in 0..500 {
        let outcome = world.tick();
        match outcome {
            Some(StepOutcome::Transition { .. }) => transitions += 1,
            Some(StepOutcome::Stay) => stays += 1,
            Some(StepOutcome::BattleComplete) => completes += 1,
            Some(StepOutcome::UnknownState { .. }) => unknowns += 1,
            None => {}
        }
    }
    assert_eq!(unknowns, 0, "battle SM hit UnknownState");
    assert!(
        transitions > 0,
        "battle SM made zero transitions in 500 frames"
    );
    let _ = (stays, completes);
}

#[test]
fn battle_complete_fires_on_party_wipe() {
    let mut world = build_world(3);
    // Kill the party.
    for i in 0..3 {
        world.actors[i].battle.liveness = 0;
    }
    // Force the SM to the EndOfAction state (which routes through the
    // party-liveness check).
    world.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
    let mut last_cause: Option<BattleEndCause> = None;
    for _ in 0..100 {
        let outcome = world.tick();
        if let Some(StepOutcome::BattleComplete) = outcome {
            last_cause = world.battle_end;
            break;
        }
    }
    assert_eq!(last_cause, Some(BattleEndCause::PartyWipe));
}

#[test]
fn battle_world_handles_every_queued_action_id() {
    // queued_action 0..=5 should each let the SM make some progress
    // without unknowns or panics, even with otherwise-default state.
    for qa in 0..=5u8 {
        let mut world = build_world(qa);
        let mut unknowns = 0u32;
        for _ in 0..100 {
            if let Some(StepOutcome::UnknownState { .. }) = world.tick() {
                unknowns += 1;
            }
        }
        assert_eq!(
            unknowns, 0,
            "battle SM hit UnknownState with queued_action={qa}"
        );
    }
}

/// Drives a full attack action turn and verifies that BattleEvent
/// emissions reach the world's queue.
///
/// The state machine routes through Begin → PreActionWait → … →
/// EndOfAction. Along the way the host gets called for poses, UI
/// elements, damage, etc. We just want to confirm at least one battle
/// event gets pushed onto pending_battle_events during a normal turn.
#[test]
fn battle_turn_emits_events_into_pending_queue() {
    use legaia_engine_core::battle_events::BattleEvent;

    let mut world = build_world(3);
    let mut event_count = 0usize;
    let mut event_kinds: Vec<&'static str> = Vec::new();

    for _ in 0..500 {
        let _ = world.tick();
        for ev in world.drain_battle_events() {
            event_count += 1;
            let kind = match ev {
                BattleEvent::Pose { .. } => "Pose",
                BattleEvent::UiElement { .. } => "UiElement",
                BattleEvent::CameraBounds => "CameraBounds",
                BattleEvent::PartySetup { .. } => "PartySetup",
                BattleEvent::MonsterSetup { .. } => "MonsterSetup",
                BattleEvent::RecomputeBattleOrder => "RecomputeBattleOrder",
                BattleEvent::LoadCaptureArchive { .. } => "LoadCaptureArchive",
                BattleEvent::SpellAnimTrigger { .. } => "SpellAnimTrigger",
                BattleEvent::SpellAnimSustain { .. } => "SpellAnimSustain",
                BattleEvent::ApplyDamage { .. } => "ApplyDamage",
                BattleEvent::ScreenShake { .. } => "ScreenShake",
                BattleEvent::RampBrightness { .. } => "RampBrightness",
                BattleEvent::BattleEnd { .. } => "BattleEnd",
            };
            if !event_kinds.contains(&kind) {
                event_kinds.push(kind);
            }
        }
    }

    assert!(
        event_count > 0,
        "expected battle events to be emitted, got {event_count}"
    );
    eprintln!("[smoke] {event_count} events across kinds {event_kinds:?}");
}

/// Damage formula primitive sanity-check via the world-side helper.
#[test]
fn basic_damage_is_a_function_of_atk_minus_def() {
    use legaia_engine_core::battle_events::basic_damage;
    let weak = basic_damage(20, 50, 0);
    let strong = basic_damage(200, 50, 0);
    assert!(strong > weak);
    // 1-damage floor.
    assert_eq!(basic_damage(0, 999, 0), 1);
}
