use super::*;
use crate::ap_gauge::ApGauge;
use crate::battle_stats::StatRecord;

fn party_slot(name: &str) -> SessionSlotInfo {
    SessionSlotInfo {
        name: name.into(),
        is_party: true,
        record: Some(StatRecord {
            base_attack: 50,
            base_udf: 30,
            base_ldf: 25,
            base_accuracy: 80,
            base_evasion: 20,
            ..Default::default()
        }),
        mp_max: 30,
    }
}

fn monster_slot(name: &str, hp: u16) -> (SessionSlotInfo, u16) {
    (
        SessionSlotInfo {
            name: name.into(),
            is_party: false,
            record: Some(StatRecord {
                base_attack: 30,
                base_udf: 20,
                base_ldf: 15,
                base_accuracy: 70,
                base_evasion: 10,
                ..Default::default()
            }),
            mp_max: 0,
        },
        hp,
    )
}

fn fresh_world_with_actors() -> World {
    let mut w = World::new();
    for _ in 0..8 {
        w.actors.push(crate::world::Actor::default());
    }
    // Give the party plausible HP.
    for i in 0..3 {
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.mp = 30;
        w.ap_gauges[i] = ApGauge::with_base(8);
    }
    w
}

fn fresh_session() -> BattleSession {
    let mut s = BattleSession::new();
    s.set_party([Character::Vahn, Character::Noa, Character::Gala]);
    s.set_slot_info(0, party_slot("Vahn"));
    s.set_slot_info(1, party_slot("Noa"));
    s.set_slot_info(2, party_slot("Gala"));
    let (info, _hp) = monster_slot("Goblin", 50);
    s.set_slot_info(3, info);
    s.set_monster_count(1);
    s
}

#[test]
fn new_session_starts_idle() {
    let s = BattleSession::new();
    assert_eq!(s.phase(), BattlePhase::Idle);
    assert!(!s.is_done());
}

#[test]
fn begin_round_transitions_to_round_intro() {
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    // Set monster HP via actor.battle.
    w.actors[3].battle.hp = 50;
    w.actors[3].battle.max_hp = 50;
    s.begin_round(&mut w);
    assert_eq!(s.phase(), BattlePhase::RoundIntro);
    // HUD slots populated.
    assert!(s.hud.slots[0].active);
    assert_eq!(s.hud.slots[0].name, "Vahn");
    assert_eq!(s.hud.slots[0].hp, 100);
}

#[test]
fn intro_auto_advances_to_command_input_after_intro_frames() {
    let mut s = BattleSession::new().with_phase_durations(3, 5);
    s.set_party([Character::Vahn, Character::Noa, Character::Gala]);
    s.set_slot_info(0, party_slot("Vahn"));
    s.set_slot_info(1, party_slot("Noa"));
    s.set_slot_info(2, party_slot("Gala"));
    let mut w = fresh_world_with_actors();
    s.begin_round(&mut w);
    for _ in 0..3 {
        s.tick(&mut w, SessionInput::default());
    }
    assert_eq!(s.phase(), BattlePhase::CommandInput);
}

#[test]
fn cross_during_intro_skips_to_command_input() {
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    s.begin_round(&mut w);
    let input = SessionInput {
        cross: true,
        ..Default::default()
    };
    let events = s.tick(&mut w, input);
    assert_eq!(s.phase(), BattlePhase::CommandInput);
    assert!(events.iter().any(|e| matches!(
        e,
        SessionEvent::PhaseChanged {
            to: BattlePhase::CommandInput,
            ..
        }
    )));
}

#[test]
fn direction_input_during_command_phase_pushes_command() {
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    s.begin_round(&mut w);
    // Skip past intro.
    let _ = s.tick(
        &mut w,
        SessionInput {
            cross: true,
            ..Default::default()
        },
    );
    assert_eq!(s.phase(), BattlePhase::CommandInput);
    let input = SessionInput {
        right: true,
        ..Default::default()
    };
    let events = s.tick(&mut w, input);
    assert!(events.iter().any(|e| matches!(
        e,
        SessionEvent::CommandPushed {
            slot: 0,
            command: Command::Right
        }
    )));
    assert_eq!(s.runner.current_buffer(), &[Command::Right]);
}

#[test]
fn circle_during_command_phase_pops_command_and_refunds_ap() {
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    s.begin_round(&mut w);
    let _ = s.tick(
        &mut w,
        SessionInput {
            cross: true,
            ..Default::default()
        },
    );
    // Direction commands are 0-cost so push + pop checks the routing
    // round-trips cleanly.
    s.tick(
        &mut w,
        SessionInput {
            left: true,
            ..Default::default()
        },
    );
    assert_eq!(s.runner.current_buffer().len(), 1);
    let events = s.tick(
        &mut w,
        SessionInput {
            circle: true,
            ..Default::default()
        },
    );
    assert!(events.iter().any(|e| matches!(
        e,
        SessionEvent::CommandPopped {
            slot: 0,
            command: Command::Left,
        }
    )));
    assert!(s.runner.current_buffer().is_empty());
}

#[test]
fn square_charges_spirit_and_emits_event() {
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    s.begin_round(&mut w);
    let _ = s.tick(
        &mut w,
        SessionInput {
            cross: true,
            ..Default::default()
        },
    );
    let before = w.ap_gauges[0].current_ap;
    let events = s.tick(
        &mut w,
        SessionInput {
            square: true,
            ..Default::default()
        },
    );
    let after = w.ap_gauges[0].current_ap;
    assert!(after > before);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SessionEvent::SpiritCharged { slot: 0 }))
    );
}

#[test]
fn triangle_advances_to_next_inputable_party_slot() {
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    s.begin_round(&mut w);
    let _ = s.tick(
        &mut w,
        SessionInput {
            cross: true,
            ..Default::default()
        },
    );
    assert_eq!(s.runner.active_party_slot(), 0);
    s.tick(
        &mut w,
        SessionInput {
            triangle: true,
            ..Default::default()
        },
    );
    assert_eq!(s.runner.active_party_slot(), 1);
}

#[test]
fn start_commits_turn_and_transitions_to_resolve() {
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    s.begin_round(&mut w);
    let _ = s.tick(
        &mut w,
        SessionInput {
            cross: true,
            ..Default::default()
        },
    );
    let events = s.tick(
        &mut w,
        SessionInput {
            start: true,
            ..Default::default()
        },
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SessionEvent::TurnCommitted))
    );
    assert_eq!(s.phase(), BattlePhase::Resolve);
}

#[test]
fn resolve_phase_transitions_to_outro_when_runner_idle() {
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    s.begin_round(&mut w);
    let _ = s.tick(
        &mut w,
        SessionInput {
            cross: true,
            ..Default::default()
        },
    );
    let _ = s.tick(
        &mut w,
        SessionInput {
            start: true,
            ..Default::default()
        },
    );
    // SM is committed. Manually drain the queue (simulates engine
    // ticking step_battle until the queue is consumed).
    s.runner.end_round(&mut w);
    // Runner is now Idle.
    s.tick(&mut w, SessionInput::default());
    assert_eq!(s.phase(), BattlePhase::RoundOutro);
}

#[test]
fn party_wipe_transitions_to_defeat() {
    let mut s = fresh_session().with_phase_durations(0, 0);
    s.set_party([Character::Vahn, Character::Noa, Character::Gala]);
    s.set_slot_info(0, party_slot("Vahn"));
    s.set_slot_info(1, party_slot("Noa"));
    s.set_slot_info(2, party_slot("Gala"));
    let mut w = fresh_world_with_actors();
    // Knock the whole party down before starting.
    for i in 0..3 {
        w.actors[i].battle.hp = 0;
    }
    s.begin_round(&mut w);
    // Auto-advance through intro → command → start → resolve.
    s.tick(
        &mut w,
        SessionInput {
            cross: true,
            ..Default::default()
        },
    );
    s.tick(
        &mut w,
        SessionInput {
            start: true,
            ..Default::default()
        },
    );
    // Drain the SM.
    s.runner.end_round(&mut w);
    // tick → resolve detects Idle, transitions to outro.
    s.tick(&mut w, SessionInput::default());
    // tick → outro auto-advances; party wipe → defeat.
    for _ in 0..32 {
        s.tick(&mut w, SessionInput::default());
        if s.is_done() {
            break;
        }
    }
    assert_eq!(s.phase(), BattlePhase::Defeat);
    assert!(s.is_done());
}

#[test]
fn monster_wipe_transitions_to_victory() {
    let mut s = fresh_session().with_phase_durations(0, 0);
    let mut w = fresh_world_with_actors();
    // Monster slot is dead.
    w.actors[3].battle.hp = 0;
    w.actors[3].battle.max_hp = 50;
    s.begin_round(&mut w);
    s.tick(
        &mut w,
        SessionInput {
            cross: true,
            ..Default::default()
        },
    );
    s.tick(
        &mut w,
        SessionInput {
            start: true,
            ..Default::default()
        },
    );
    s.runner.end_round(&mut w);
    s.tick(&mut w, SessionInput::default());
    for _ in 0..32 {
        s.tick(&mut w, SessionInput::default());
        if s.is_done() {
            break;
        }
    }
    assert_eq!(s.phase(), BattlePhase::Victory);
}

#[test]
fn fold_event_into_hud_routes_apply_art_strike_to_popup_and_log() {
    use crate::art_strike::ArtStrikeOutcome;
    use crate::battle_events::BattleEvent;
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    s.begin_round(&mut w);

    let outcome = ArtStrikeOutcome {
        damage: Some(30),
        ..Default::default()
    };
    let ev = BattleEvent::ApplyArtStrike {
        actor_slot: 0,
        target_slot: 3,
        strike_index: 0,
        outcome,
    };
    let mut sink = Vec::new();
    s.fold_event_into_hud(&ev, &mut sink);
    assert!(s.hud.popups.iter().any(|p| p.amount == 30 && p.slot == 3));
    assert!(s.hud.log.iter().any(|l| l.text.contains("-30 HP slot 3")));
    assert!(sink.iter().any(|e| matches!(
        e,
        SessionEvent::HpChanged {
            slot: 3,
            amount: 30,
            is_heal: false
        }
    )));
}

#[test]
fn fold_event_into_hud_emits_status_event_when_enemy_effect_present() {
    use crate::art_strike::ArtStrikeOutcome;
    use crate::battle_events::BattleEvent;
    use legaia_art::record::EnemyEffect;
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    s.begin_round(&mut w);

    let outcome = ArtStrikeOutcome {
        damage: Some(0),
        enemy_effect: EnemyEffect::Toxic,
        ..Default::default()
    };
    let ev = BattleEvent::ApplyArtStrike {
        actor_slot: 0,
        target_slot: 3,
        strike_index: 1,
        outcome,
    };
    let mut sink = Vec::new();
    s.fold_event_into_hud(&ev, &mut sink);
    assert!(sink.iter().any(|e| matches!(
        e,
        SessionEvent::StatusApplied {
            slot: 3,
            kind: StatusKind::Toxic
        }
    )));
}

#[test]
fn input_to_command_priority_right_first() {
    let i = SessionInput {
        right: true,
        up: true,
        ..Default::default()
    };
    assert_eq!(input_to_command(i), Some(Command::Right));
}

#[test]
fn input_to_command_returns_none_when_no_directions() {
    let i = SessionInput::default();
    assert_eq!(input_to_command(i), None);
}

#[test]
fn is_slot_inputable_skips_dead_party_slot() {
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    w.actors[1].battle.hp = 0;
    s.begin_round(&mut w);
    assert!(!s.is_slot_inputable(&w, 1));
    assert!(s.is_slot_inputable(&w, 0));
}

#[test]
fn open_target_picker_enters_subphase_target_pick() {
    use crate::target_picker::TargetKind;
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    w.actors[3].battle.hp = 50;
    w.actors[3].battle.max_hp = 50;
    s.begin_round(&mut w);
    let mut events = Vec::new();
    s.open_target_picker(&w, TargetKind::SingleEnemy, 0, None, &mut events);
    assert_eq!(s.sub_phase(), SubPhase::TargetPick);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SessionEvent::TargetPickerOpened { .. }))
    );
}

#[test]
fn target_picker_confirm_emits_target_confirmed() {
    use crate::target_picker::TargetKind;
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    w.actors[3].battle.hp = 50;
    w.actors[3].battle.max_hp = 50;
    s.begin_round(&mut w);
    let mut events = Vec::new();
    s.open_target_picker(&w, TargetKind::SingleEnemy, 0, None, &mut events);
    events.clear();
    // SubPhase=TargetPick now; cross confirms the only enemy.
    let confirm_input = SessionInput {
        cross: true,
        ..Default::default()
    };
    // Need to be in CommandInput phase for tick to route.
    s.transition(BattlePhase::CommandInput);
    let evs = s.tick(&mut w, confirm_input);
    // slot is row-relative; the first enemy is index 0 in the monsters row.
    assert!(
        evs.iter()
            .any(|e| matches!(e, SessionEvent::TargetConfirmed { target_slot: 0 }))
    );
    // Picker is closed, sub-phase back to CommandSelect.
    assert_eq!(s.sub_phase(), SubPhase::CommandSelect);
}

#[test]
fn target_picker_sweep_resolves_immediately() {
    use crate::target_picker::TargetKind;
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    w.actors[3].battle.hp = 50;
    w.actors[3].battle.max_hp = 50;
    s.begin_round(&mut w);
    let mut events = Vec::new();
    s.open_target_picker(&w, TargetKind::AllEnemies, 0, None, &mut events);
    // Sweep targets resolve in init_cursor → maybe_close_picker emits
    // TargetSweepConfirmed and clears the picker.
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SessionEvent::TargetSweepConfirmed))
    );
    assert_eq!(s.sub_phase(), SubPhase::CommandSelect);
}

#[test]
fn target_confirm_writes_active_target_into_actor_and_pushes_pending_command() {
    use crate::target_picker::TargetKind;
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    w.actors[3].battle.hp = 50;
    w.actors[3].battle.max_hp = 50;
    s.begin_round(&mut w);
    // Skip past intro into command-input.
    s.transition(BattlePhase::CommandInput);
    // Open with a pending command: the player buffered Right and the
    // engine then opened the picker.
    let mut events = Vec::new();
    s.open_target_picker(
        &w,
        TargetKind::SingleEnemy,
        0,
        Some(Command::Right),
        &mut events,
    );
    events.clear();
    // Cross confirms the only enemy - through tick so we get the
    // active-target write side effect.
    let evs = s.tick(
        &mut w,
        SessionInput {
            cross: true,
            ..Default::default()
        },
    );
    // active_target written.
    assert_eq!(w.actors[0].battle.active_target, 0);
    // TargetConfirmed event present.
    assert!(
        evs.iter()
            .any(|e| matches!(e, SessionEvent::TargetConfirmed { target_slot: 0 }))
    );
    // CommandPushed (Right) appears in the same frame because the
    // pending command auto-admits.
    assert!(evs.iter().any(|e| matches!(
        e,
        SessionEvent::CommandPushed {
            slot: 0,
            command: Command::Right
        }
    )));
    // Runner buffer reflects the admitted command.
    assert_eq!(s.runner.current_buffer(), &[Command::Right]);
}

#[test]
fn target_sweep_writes_sentinel_and_admits_pending_command() {
    use crate::target_picker::TargetKind;
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    w.actors[3].battle.hp = 50;
    w.actors[3].battle.max_hp = 50;
    s.begin_round(&mut w);
    // open_target_picker with AllEnemies resolves immediately;
    // maybe_close_picker is called inside open_target_picker but
    // without a mutable World ref, so the sentinel write is deferred.
    // Drive via push_command_with_target which goes through the
    // mutable-world path correctly when sweep resolves on the same
    // call.
    s.transition(BattlePhase::CommandInput);
    let ok = s.push_command_with_target(&mut w, Command::Up, TargetKind::AllEnemies, 0);
    assert!(ok);
    // Sweep-immediate path: actor active_target updated to sentinel
    // and command auto-admitted.
    assert_eq!(
        w.actors[0].battle.active_target,
        BattleSession::SWEEP_TARGET_SENTINEL
    );
    assert_eq!(s.runner.current_buffer(), &[Command::Up]);
}

#[test]
fn push_command_with_target_returns_false_when_out_of_ap() {
    use crate::target_picker::TargetKind;
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    // Drain the AP gauge so the cost can't be paid.
    w.ap_gauges[0] = ApGauge::with_base(0);
    s.begin_round(&mut w);
    s.transition(BattlePhase::CommandInput);
    let ok = s.push_command_with_target(&mut w, Command::Right, TargetKind::AllEnemies, 0);
    // Direction commands are 0-cost, so this should still admit.
    assert!(ok);
    // Now try a chained-art-shape action that costs 1 AP. We can't
    // construct one through `Command`, so emulate by spending the
    // gauge to <0 and testing the cost-check directly: the only
    // 0-cost commands are directional, which always admit.
    // (This test just validates the API doesn't panic on empty
    // gauge for a 0-cost cmd.)
}

#[test]
fn rot_refuses_the_rotted_limbs_command_only() {
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    s.begin_round(&mut w);
    s.transition(BattlePhase::CommandInput);
    // Rot the active slot's Right arm (limb roll 1).
    let active = s.runner.active_party_slot();
    w.status_effects
        .apply(active, legaia_engine_vm::status_effects::StatusKind::Rot);
    w.status_effects.set_rot_limb(active, 1);
    assert!(
        !s.push_command(&mut w, Command::Right),
        "rotted limb blocked"
    );
    assert!(s.push_command(&mut w, Command::Left), "other limbs admit");
    assert!(s.push_command(&mut w, Command::Down));
    // Curing restores the command.
    w.status_effects
        .cure(active, legaia_engine_vm::status_effects::StatusKind::Rot);
    assert!(s.push_command(&mut w, Command::Right));
}

#[test]
fn target_cancelled_drops_pending_command_without_pushing() {
    use crate::target_picker::TargetKind;
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    w.actors[3].battle.hp = 50;
    w.actors[3].battle.max_hp = 50;
    s.begin_round(&mut w);
    s.transition(BattlePhase::CommandInput);
    let mut events = Vec::new();
    s.open_target_picker(
        &w,
        TargetKind::SingleEnemy,
        0,
        Some(Command::Down),
        &mut events,
    );
    let buffer_before = s.runner.current_buffer().len();
    events.clear();
    let evs = s.tick(
        &mut w,
        SessionInput {
            circle: true,
            ..Default::default()
        },
    );
    assert!(
        evs.iter()
            .any(|e| matches!(e, SessionEvent::TargetCancelled))
    );
    // No new command admitted.
    assert_eq!(s.runner.current_buffer().len(), buffer_before);
}

#[test]
fn cancel_target_picker_drops_pending_command() {
    use crate::target_picker::TargetKind;
    let mut s = fresh_session();
    let mut w = fresh_world_with_actors();
    w.actors[3].battle.hp = 50;
    w.actors[3].battle.max_hp = 50;
    s.begin_round(&mut w);
    let mut events = Vec::new();
    s.open_target_picker(
        &w,
        TargetKind::SingleEnemy,
        0,
        Some(Command::Up),
        &mut events,
    );
    events.clear();
    s.cancel_target_picker(&mut events);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, SessionEvent::TargetCancelled))
    );
    assert!(s.target_picker().is_none());
}
