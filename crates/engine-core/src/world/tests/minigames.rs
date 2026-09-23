use super::*;

// --- Noa dance (rhythm) minigame wiring ------------------------------------

/// A 3-row chart whose beat 0 (every lane) wants symbol 1 (`DanceDir::A` =
/// pad Square), for deterministic judging.
fn dance_test_chart() -> legaia_asset::dance_chart::DanceChart {
    use legaia_asset::dance_chart::{BEATS_PER_ROW, DanceChart};
    let mut rows = Vec::new();
    for _ in 0..3 {
        let mut row = [0u8; BEATS_PER_ROW];
        row[0] = 1; // symbol 1 -> DanceDir::A -> pad Square
        rows.push(row);
    }
    DanceChart { rows }
}

/// Play the pre-song **count-in** out on neutral pad frames, so a judging
/// test starts on the first frame the beat clock actually runs.
///
/// `World::enter_dance` arms `minigames.dance_countin` (retail's
/// `FUN_801cf470` below-10 states) and the dance tick holds
/// `DanceGame::advance` off until it clears, so a test that pressed on the
/// entry frame would be pressing into the banner.
fn run_dance_countin(world: &mut World) {
    for _ in 0..crate::dance::COUNTIN_END_FRAME {
        world.set_pad(0);
        let _ = world.tick();
    }
    assert!(
        world.minigames.dance_countin.is_none(),
        "the count-in did not clear in its own frame budget"
    );
}

/// The count-in owns the frames before the song: the beat clock does not
/// advance, no press is judged, and the banner envelope is published for the
/// host to draw. Both hosts read this one phase, which is what gives the
/// **door-warp** entry a count-in at all.
#[test]
fn enter_dance_counts_in_before_the_beat_clock_runs() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_dance(crate::dance::DanceGame::new(dance_test_chart(), false));
    assert!(world.minigames.dance_countin.is_some());
    // A judged button during the count-in scores nothing.
    world.set_pad(0);
    world.set_pad(input::PadButton::Square.mask());
    let _ = world.tick();
    assert_eq!(world.minigames.dance_last_judge, None);
    assert_eq!(world.minigames.dance.as_ref().unwrap().song_timer(), 0);
    assert!(world.minigames.dance_countin_banner.is_some());
    // The banner and the status readout are mutually exclusive, and one
    // predicate says so for every host.
    assert!(!world.minigames.dance_status_visible());
    // The intro cue fires once, on the hold-segment entry.
    let mut cues = world.drain_minigame_sfx_cues();
    for _ in 0..crate::dance::COUNTIN_END_FRAME {
        world.set_pad(0);
        let _ = world.tick();
        cues.extend(world.drain_minigame_sfx_cues());
    }
    assert_eq!(
        cues.iter()
            .filter(|c| **c == crate::dance::COUNTIN_INTRO_CUE)
            .count(),
        1,
        "the count-in intro cue is once-only"
    );
    assert!(world.minigames.dance_countin.is_none());
    assert!(world.minigames.dance_countin_banner.is_none());
    assert!(world.minigames.dance_status_visible());
    // And the song started: the chart loop is queued as an op-0x35 start, the
    // same event both hosts' BGM directors consume.
    assert!(world.audio.minigame_bgm_active);
    assert!(
        world
            .pending_field_events
            .iter()
            .any(|e| matches!(e, crate::field_events::FieldEvent::Bgm { sub_op: 1, .. }))
    );
}

#[test]
fn enter_dance_suspends_mode_and_exit_restores_it() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    let game = crate::dance::DanceGame::new(dance_test_chart(), false);
    world.enter_dance(game);
    assert_eq!(world.mode, SceneMode::Dance);
    assert!(world.minigames.dance.is_some());
    // A mid-song abort restores the interrupted mode and yields the game.
    let finished = world.exit_dance();
    assert!(finished.is_some());
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.minigames.dance.is_none());
}

/// Both hosts poll `exit_dance` from their frame path on every non-`Dance`
/// frame, so the poll has to be inert when no run is installed. It was not:
/// the teardown ran the dance stager's PAD-LATCH CLEAR every frame, which
/// forces `pad_prev = pad` and hides every edge from whatever the host reads
/// after the poll (on the play page, the developer menu).
#[test]
fn exit_dance_without_a_run_does_not_eat_the_frame_s_pad_edges() {
    use crate::input::PadButton;
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.set_pad(0);
    world.set_pad(PadButton::Down.mask());
    assert!(
        world.input.just_pressed(PadButton::Down),
        "the fixture itself has to present an edge"
    );
    assert!(world.exit_dance().is_none(), "no run to tear down");
    assert!(
        world.input.just_pressed(PadButton::Down),
        "the no-run poll must leave the frame's pad edges alone"
    );
}

/// The real teardown still performs the stager's clear: the press that leaves
/// the hall must not carry into the restored field mode.
#[test]
fn exit_dance_with_a_run_still_clears_the_pad_latch() {
    use crate::input::PadButton;
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_dance(crate::dance::DanceGame::new(dance_test_chart(), false));
    world.set_pad(0);
    world.set_pad(PadButton::Cross.mask());
    assert!(world.input.just_pressed(PadButton::Cross));
    assert!(world.exit_dance().is_some());
    assert!(
        !world.input.just_pressed(PadButton::Cross),
        "the hall's exit press must not be delivered to the field"
    );
}

#[test]
fn dance_tick_judges_a_correct_press() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_dance(crate::dance::DanceGame::new(dance_test_chart(), false));
    run_dance_countin(&mut world);
    // Rising edge on Square (DanceDir::A) - beat 0 of lane 0 wants symbol 1.
    world.set_pad(0);
    world.set_pad(input::PadButton::Square.mask());
    let _ = world.tick();
    // The press was judged (the chain closed, the groove gauge advanced). The
    // score itself is the dancer kind's disc-resident bonus row, which a chart
    // fixture with no overlay tables leaves at zero - `dance_minigame_real`
    // covers the scoring end on the real tables.
    assert!(matches!(
        world.minigames.dance_last_judge,
        Some(crate::dance::Judge::Hit { .. }) | Some(crate::dance::Judge::Sequence { .. })
    ));
    assert!(world.minigames.dance.as_ref().unwrap().gauge() > 0);
}

#[test]
fn dance_wrong_direction_misses() {
    let mut world = World::new();
    world.enter_dance(crate::dance::DanceGame::new(dance_test_chart(), false));
    run_dance_countin(&mut world);
    // Beat 0 wants Square; press Circle instead -> miss.
    world.set_pad(0);
    world.set_pad(input::PadButton::Circle.mask());
    let _ = world.tick();
    assert_eq!(
        world.minigames.dance_last_judge,
        Some(crate::dance::Judge::Miss)
    );
    assert_eq!(world.minigames.dance.as_ref().unwrap().score(), 0);
}

/// The judge reads the retail packed-pad word, so the three judged buttons are
/// the face triple `DanceDir::pad_bit` names (Square / Circle / Triangle) and a
/// dpad press is not a dance input at all.
#[test]
fn dance_judges_the_retail_pad_bits_not_the_dpad() {
    use crate::dance::DanceDir;
    // The bit map itself, straight off `FUN_801d4040`.
    assert_eq!(DanceDir::A.pad_bit(), 0x80);
    assert_eq!(DanceDir::B.pad_bit(), 0x20);
    assert_eq!(DanceDir::C.pad_bit(), 0x10);
    let mut world = World::new();
    world.enter_dance(crate::dance::DanceGame::new(dance_test_chart(), false));
    run_dance_countin(&mut world);
    // Dpad Left used to be direction A; it is not a judged bit.
    world.set_pad(0);
    world.set_pad(input::PadButton::Left.mask());
    let _ = world.tick();
    assert_eq!(
        world.minigames.dance_last_judge, None,
        "the dpad is not judged"
    );
    // Square is - and it is the direction beat 0 asks for.
    world.set_pad(0);
    world.set_pad(input::PadButton::Square.mask());
    let _ = world.tick();
    assert!(matches!(
        world.minigames.dance_last_judge,
        Some(crate::dance::Judge::Hit { .. }) | Some(crate::dance::Judge::Sequence { .. })
    ));
}

#[test]
fn dance_song_end_auto_restores_mode() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_dance(crate::dance::DanceGame::new(dance_test_chart(), false));
    // Run enough neutral-pad frames to exhaust the short song. tick_dance
    // advances the beat clock 10 phase units/frame; the short song ends at
    // SONG_LEN_SHORT (0x41dc) so a few thousand frames guarantees the timeout.
    for _ in 0..3000 {
        if world.mode != SceneMode::Dance {
            break;
        }
        world.set_pad(0);
        let _ = world.tick();
    }
    // The song timed out: mode restored, but the game is still installed for
    // the host to read the final score until it calls exit_dance.
    assert_eq!(world.mode, SceneMode::Field);
    assert!(
        world
            .minigames
            .dance
            .as_ref()
            .map(|g| g.song_over())
            .unwrap_or(false)
    );
    let finished = world.exit_dance();
    assert!(finished.is_some());
    assert!(world.minigames.dance.is_none());
}

// --- Fishing minigame wiring -----------------------------------------------

fn fishing_test_session() -> crate::fishing::FishingSession {
    use legaia_asset::fishing_species::FishingSpecies;
    let mk = |index: usize, strike_gate: i32| FishingSpecies {
        index,
        name_ptr_va: 0,
        score_value: 10_000,
        pull_factor: 64,
        dart_factor: 60,
        sink_factor: 4,
        depth_gate: 1024,
        roll_cutoff_a: 200,
        roll_cutoff_b: 512,
        roll_cutoff_c: 90,
        strike_gate,
    };
    // Small strike gates so a reeled fight lands quickly in-test.
    crate::fishing::FishingSession::new(
        vec![mk(0, 8), mk(1, 8), mk(2, 8)],
        8,
        crate::fishing::FishingRecord::default(),
    )
}

#[test]
fn enter_fishing_suspends_mode_and_exit_restores_it() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_fishing(fishing_test_session());
    assert_eq!(world.mode, SceneMode::Fishing);
    assert!(world.minigames.fishing.is_some());
    let session = world.exit_fishing();
    assert!(session.is_some());
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.minigames.fishing.is_none());
}

#[test]
fn fishing_casts_locks_and_reels_to_a_resolution() {
    use crate::fishing::FishingPhase;
    let mut world = World::new();
    world.enter_fishing(fishing_test_session());
    // A few casting frames oscillate the meter.
    for _ in 0..3 {
        world.set_pad(0);
        let _ = world.tick();
    }
    assert_eq!(
        world.minigames.fishing.as_ref().unwrap().phase(),
        FishingPhase::Casting
    );
    // Confirm (Cross rising edge) locks the cast -> Fighting.
    world.set_pad(0);
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(
        world.minigames.fishing.as_ref().unwrap().phase(),
        FishingPhase::Fighting
    );
    // Hold Cross (reel A) until the fight resolves.
    for _ in 0..3000 {
        if world.minigames.fishing.as_ref().unwrap().phase() != FishingPhase::Fighting {
            break;
        }
        // Keep Cross held frame to frame (no fresh edge needed for reeling).
        world.set_pad(input::PadButton::Cross.mask());
        let _ = world.tick();
    }
    assert_eq!(
        world.minigames.fishing.as_ref().unwrap().phase(),
        FishingPhase::Done
    );
    assert!(
        world
            .minigames
            .fishing
            .as_ref()
            .unwrap()
            .last_outcome()
            .is_some()
    );
}

/// The hook cue and the celebration cues come off the **session's own phase
/// edges**, on the world's shared cue queue, so every host that ticks the
/// world hears them. They used to live on `LineActorSim`, which only the
/// native window drives, so the strike and the catch were silent in both
/// browsers - the same shape the strike splash had before it moved here.
#[test]
fn the_strike_and_catch_edges_queue_their_cues_on_the_world() {
    use crate::fishing::FishingPhase;
    use crate::fishing_actors::{CELEBRATE_CUE, HOOK_CUE};
    let mut world = World::new();
    world.enter_fishing(fishing_test_session());
    // Drain whatever entry queued so the assertions below read this edge.
    let _ = world.drain_minigame_sfx_cues();
    for _ in 0..3 {
        world.set_pad(0);
        let _ = world.tick();
    }
    assert!(
        world.drain_minigame_sfx_cues().is_empty(),
        "casting frames raise no cue"
    );
    // The lock edge is the strike: splash + hook cue, one producer.
    world.set_pad(0);
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(
        world.minigames.fishing.as_ref().unwrap().phase(),
        FishingPhase::Fighting
    );
    assert!(
        world
            .drain_minigame_sfx_cues()
            .contains(&u16::from(HOOK_CUE)),
        "the strike edge queues the hook cue"
    );
    // Reel it in; the catch edge queues the celebration cue.
    for _ in 0..3000 {
        if world.minigames.fishing.as_ref().unwrap().phase() != FishingPhase::Fighting {
            break;
        }
        world.set_pad(input::PadButton::Cross.mask());
        let _ = world.tick();
    }
    assert_eq!(
        world.minigames.fishing.as_ref().unwrap().phase(),
        FishingPhase::Done
    );
    let cues = world.drain_minigame_sfx_cues();
    assert!(
        cues.contains(&u16::from(CELEBRATE_CUE)),
        "a landed fish queues the celebration cue, got {cues:?}"
    );
}

/// The reel buttons are the retail packed-pad bits decoded by
/// `ReelInput::from_pad_mask`: `0x40` Cross = reel A, `0x80` Square = reel B,
/// both held = reel A. Circle (`0x20`) is the cast/hook input, not a reel.
///
/// The two reels are told apart by their divisors (`rod*9 + 0x23` for A,
/// `rod*6 + 0x19` for B), so one frame of each leaves a different tension.
#[test]
fn fishing_reel_buttons_are_cross_and_square_with_cross_winning() {
    use legaia_asset::fishing_species::FishingSpecies;
    // A fish that pulls hard enough for one frame of reeling to move the
    // gauge through the integer divisors at rod stat 8.
    let strong = |index: usize| FishingSpecies {
        index,
        name_ptr_va: 0,
        score_value: 10_000,
        pull_factor: 4000,
        dart_factor: 60,
        sink_factor: 4,
        depth_gate: 1024,
        roll_cutoff_a: 200,
        roll_cutoff_b: 512,
        roll_cutoff_c: 90,
        strike_gate: 1000,
    };
    let one_frame = |mask: u16| -> i32 {
        let mut world = World::new();
        world.enter_fishing(crate::fishing::FishingSession::new(
            (0..3).map(strong).collect(),
            8,
            crate::fishing::FishingRecord::default(),
        ));
        world.set_pad(0);
        world.set_pad(input::PadButton::Cross.mask());
        let _ = world.tick(); // locks the cast -> Fighting
        world.set_pad(mask);
        let _ = world.tick();
        world
            .minigames
            .fishing
            .as_ref()
            .and_then(|s| s.fight())
            .map(|f| f.tension())
            .unwrap_or(-1)
    };
    // base_pull = 4000/8 = 500; reel A divisor 8*9+0x23 = 107, reel B 8*6+0x19 = 73.
    let reel_a = one_frame(input::PadButton::Cross.mask());
    let reel_b = one_frame(input::PadButton::Square.mask());
    assert_eq!(reel_a, 500 / 107);
    assert_eq!(reel_b, 500 / 73);
    assert_ne!(reel_a, reel_b, "the two divisors must be distinguishable");
    // Both held resolves to reel A - the retail decoder's priority, not a blend.
    assert_eq!(
        one_frame(input::PadButton::Cross.mask() | input::PadButton::Square.mask()),
        reel_a
    );
    // Circle is the cast/hook input: idle, so the gauge bleeds off (clamped at 0).
    assert_eq!(one_frame(input::PadButton::Circle.mask()), 0);
}

#[test]
fn fishing_tick_without_session_falls_back_to_return_mode() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // Force the mode without installing a session (defensive path).
    world.minigames.fishing_return_mode = SceneMode::Field;
    world.mode = SceneMode::Fishing;
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Field);
}

fn slot_test_machine(balance: i32) -> crate::slot_machine::SlotMachine {
    use legaia_asset::slot_payout::SlotPayoutTable;
    // Synthetic payout table: symbol id i pays (i+1)*2 coins.
    let mut payouts = [0u8; legaia_asset::slot_payout::SLOT_SYMBOL_COUNT];
    for (i, p) in payouts.iter_mut().enumerate() {
        *p = ((i + 1) * 2) as u8;
    }
    crate::slot_machine::SlotMachine::new(SlotPayoutTable { payouts }, 0xC0FFEE, balance)
}

#[test]
fn enter_slot_machine_suspends_mode_and_exit_commits_the_bank() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.minigames.casino_coins = 7;
    world.enter_slot_machine(slot_test_machine(50));
    assert_eq!(world.mode, SceneMode::SlotMachine);
    assert!(world.minigames.slot_machine.is_some());
    let machine = world.exit_slot_machine();
    assert!(machine.is_some());
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.minigames.slot_machine.is_none());
    // Exit commits the playing balance INTO the bank (the retail state-100
    // assignment `_DAT_800845A4 = DAT_801d4114`), replacing the old value.
    assert_eq!(world.minigames.casino_coins, 50);
}

#[test]
fn slot_machine_spins_stops_and_collects_through_the_pad() {
    use crate::slot_machine::{SPIN_UP_FRAMES, SlotPhase};
    let mut world = World::new();
    world.enter_slot_machine(slot_test_machine(50));
    // Confirm (Cross rising edge) charges the bet and starts the spin.
    world.set_pad(0);
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    let m = world.minigames.slot_machine.as_ref().unwrap();
    assert_eq!(m.phase(), SlotPhase::Spinning);
    assert_eq!(m.balance(), 50 - m.spin_cost());
    // Run the spin-up timer down into Stopping.
    for _ in 0..SPIN_UP_FRAMES {
        world.set_pad(0);
        let _ = world.tick();
    }
    assert_eq!(
        world.minigames.slot_machine.as_ref().unwrap().phase(),
        SlotPhase::Stopping
    );
    // Three fresh Cross edges stop the three reels -> Payout.
    for _ in 0..3 {
        world.set_pad(0);
        let _ = world.tick();
        world.set_pad(input::PadButton::Cross.mask());
        let _ = world.tick();
    }
    let m = world.minigames.slot_machine.as_ref().unwrap();
    assert_eq!(m.phase(), SlotPhase::Payout);
    assert_eq!(m.reels_stopped(), crate::slot_machine::REEL_COUNT);
    let result = m.last_result().expect("spin evaluated");
    let before = m.balance();
    // A fresh Cross edge collects the (possibly zero) payout back to Idle.
    world.set_pad(0);
    let _ = world.tick();
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    let m = world.minigames.slot_machine.as_ref().unwrap();
    assert_eq!(m.phase(), SlotPhase::Idle);
    assert_eq!(m.balance(), before + result.payout);
}

#[test]
fn slot_machine_spin_accrues_the_net_take() {
    use crate::slot_machine::NET_TAKE_NORMAL_SPIN;
    let mut world = World::new();
    world.enter_slot_machine(slot_test_machine(50));
    assert_eq!(world.minigames.slot_machine.as_ref().unwrap().net_take(), 0);
    world.set_pad(0);
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(
        world.minigames.slot_machine.as_ref().unwrap().net_take(),
        NET_TAKE_NORMAL_SPIN
    );
}

#[test]
fn slot_machine_tick_without_session_falls_back_to_return_mode() {
    let mut world = World::new();
    world.minigames.slot_return_mode = SceneMode::Field;
    world.mode = SceneMode::SlotMachine;
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Field);
}

// --- Mode-24 minigame door-warp round trip (FUN_80025980 / FUN_80026018) ---

#[test]
fn minigame_return_warp_restores_scene_and_commits_winnings() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.active_scene_label = "sioro".to_string();
    world.minigames.casino_coins = 100;

    // 0x3E warp arm + mode-24 OTHER-INIT: name backed up, accumulator zeroed.
    world.minigames.winnings = 55; // stale value from a previous session
    world.arm_minigame_warp();
    assert_eq!(world.minigames.scene_backup.as_deref(), Some("sioro"));
    assert_eq!(world.minigames.winnings, 0);

    // The minigame overlay runs: scene state clobbered, winnings accumulate.
    world.active_scene_label = "minigame".to_string();
    world.mode = SceneMode::SlotMachine;
    world.minigames.winnings = 250;

    // FUN_80026018: name restored, `_DAT_800845A4 += _DAT_80084440`, mode 2.
    world.minigame_return_warp();
    assert_eq!(world.active_scene_label, "sioro");
    assert_eq!(world.minigames.casino_coins, 350);
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.minigames.scene_backup.is_none(), "backup consumed");
}

#[test]
fn minigame_return_warp_coin_bank_saturates_at_retail_cap() {
    let mut world = World::new();
    world.active_scene_label = "sioro".to_string();
    world.arm_minigame_warp();
    world.minigames.casino_coins = 9_999_000;
    world.minigames.winnings = 5_000;
    world.minigame_return_warp();
    assert_eq!(
        world.minigames.casino_coins, 9_999_999,
        "clamped to the retail cap"
    );
}

#[test]
fn minigame_return_warp_without_arm_keeps_scene_but_still_commits() {
    let mut world = World::new();
    world.active_scene_label = "town01".to_string();
    world.minigames.casino_coins = 1;
    world.minigames.winnings = 2;
    world.minigame_return_warp();
    // Retail's coin add is unconditional; only the restore needs the backup.
    assert_eq!(world.minigames.casino_coins, 3);
    assert_eq!(world.active_scene_label, "town01");
}

/// The dance stager's pad-latch clear, applied on entry and on teardown.
///
/// Retail zeroes `_DAT_8007B880` inside `FUN_801D414C`, so the confirm press
/// that opens the hall cannot also be judged as the run's first note (and the
/// press that leaves cannot leak into the restored field mode).
#[test]
fn entering_the_dance_drops_the_confirm_press_edge() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // Square is newly pressed on the frame the dance opens - exactly the shape
    // of a script confirm that launches the minigame.
    world.set_pad(0);
    world.set_pad(input::PadButton::Square.mask());
    world.enter_dance(crate::dance::DanceGame::new(dance_test_chart(), false));
    let _ = world.tick();
    assert_eq!(
        world.minigames.dance_last_judge, None,
        "the opening press was consumed as a note"
    );
    // Past the count-in the button is still *held*, so releasing and pressing
    // it again scores. (The latch clear is what this pins; the count-in in
    // between is `enter_dance_counts_in_before_the_beat_clock_runs`.)
    run_dance_countin(&mut world);
    world.set_pad(0);
    world.set_pad(input::PadButton::Square.mask());
    let _ = world.tick();
    assert!(
        world.minigames.dance_last_judge.is_some(),
        "later presses still judge"
    );
}

#[test]
fn leaving_the_dance_drops_the_press_edge_too() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_dance(crate::dance::DanceGame::new(dance_test_chart(), false));
    world.set_pad(0);
    world.set_pad(input::PadButton::Circle.mask());
    let _ = world.exit_dance();
    assert!(!world.input.just_pressed(input::PadButton::Circle));
    assert!(world.input.pressed(input::PadButton::Circle), "still held");
}
