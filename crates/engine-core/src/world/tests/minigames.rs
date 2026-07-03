use super::*;

// --- Noa dance (rhythm) minigame wiring ------------------------------------

/// A 3-row chart whose beat 0 (every lane) wants symbol 1 (`DanceDir::A` =
/// pad Left), for deterministic judging.
fn dance_test_chart() -> legaia_asset::dance_chart::DanceChart {
    use legaia_asset::dance_chart::{BEATS_PER_ROW, DanceChart};
    let mut rows = Vec::new();
    for _ in 0..3 {
        let mut row = [0u8; BEATS_PER_ROW];
        row[0] = 1; // symbol 1 -> DanceDir::A -> pad Left
        rows.push(row);
    }
    DanceChart { rows }
}

#[test]
fn enter_dance_suspends_mode_and_exit_restores_it() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    let game = crate::dance::DanceGame::new(dance_test_chart(), false);
    world.enter_dance(game);
    assert_eq!(world.mode, SceneMode::Dance);
    assert!(world.dance.is_some());
    // A mid-song abort restores the interrupted mode and yields the game.
    let finished = world.exit_dance();
    assert!(finished.is_some());
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.dance.is_none());
}

#[test]
fn dance_tick_judges_a_correct_press() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_dance(crate::dance::DanceGame::new(dance_test_chart(), false));
    // Rising edge on Left (DanceDir::A) - beat 0 of lane 0 wants symbol 1.
    world.set_pad(0);
    world.set_pad(input::PadButton::Left.mask());
    let _ = world.tick();
    // The press was judged (score advanced, judgement recorded).
    assert!(matches!(
        world.dance_last_judge,
        Some(crate::dance::Judge::Hit { .. }) | Some(crate::dance::Judge::Sequence { .. })
    ));
    assert!(world.dance.as_ref().unwrap().score() > 0);
}

#[test]
fn dance_wrong_direction_misses() {
    let mut world = World::new();
    world.enter_dance(crate::dance::DanceGame::new(dance_test_chart(), false));
    // Beat 0 wants Left; press Right instead -> miss.
    world.set_pad(0);
    world.set_pad(input::PadButton::Right.mask());
    let _ = world.tick();
    assert_eq!(world.dance_last_judge, Some(crate::dance::Judge::Miss));
    assert_eq!(world.dance.as_ref().unwrap().score(), 0);
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
    assert!(world.dance.as_ref().map(|g| g.song_over()).unwrap_or(false));
    let finished = world.exit_dance();
    assert!(finished.is_some());
    assert!(world.dance.is_none());
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
    assert!(world.fishing.is_some());
    let session = world.exit_fishing();
    assert!(session.is_some());
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.fishing.is_none());
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
        world.fishing.as_ref().unwrap().phase(),
        FishingPhase::Casting
    );
    // Confirm (Cross rising edge) locks the cast -> Fighting.
    world.set_pad(0);
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(
        world.fishing.as_ref().unwrap().phase(),
        FishingPhase::Fighting
    );
    // Hold Cross (reel A) until the fight resolves.
    for _ in 0..3000 {
        if world.fishing.as_ref().unwrap().phase() != FishingPhase::Fighting {
            break;
        }
        // Keep Cross held frame to frame (no fresh edge needed for reeling).
        world.set_pad(input::PadButton::Cross.mask());
        let _ = world.tick();
    }
    assert_eq!(world.fishing.as_ref().unwrap().phase(), FishingPhase::Done);
    assert!(world.fishing.as_ref().unwrap().last_outcome().is_some());
}

#[test]
fn fishing_tick_without_session_falls_back_to_return_mode() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    // Force the mode without installing a session (defensive path).
    world.fishing_return_mode = SceneMode::Field;
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
    world.casino_coins = 7;
    world.enter_slot_machine(slot_test_machine(50));
    assert_eq!(world.mode, SceneMode::SlotMachine);
    assert!(world.slot_machine.is_some());
    let machine = world.exit_slot_machine();
    assert!(machine.is_some());
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.slot_machine.is_none());
    // Exit commits the playing balance INTO the bank (the retail state-100
    // assignment `_DAT_800845A4 = DAT_801d4114`), replacing the old value.
    assert_eq!(world.casino_coins, 50);
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
    let m = world.slot_machine.as_ref().unwrap();
    assert_eq!(m.phase(), SlotPhase::Spinning);
    assert_eq!(m.balance(), 50 - m.spin_cost());
    // Run the spin-up timer down into Stopping.
    for _ in 0..SPIN_UP_FRAMES {
        world.set_pad(0);
        let _ = world.tick();
    }
    assert_eq!(
        world.slot_machine.as_ref().unwrap().phase(),
        SlotPhase::Stopping
    );
    // Three fresh Cross edges stop the three reels -> Payout.
    for _ in 0..3 {
        world.set_pad(0);
        let _ = world.tick();
        world.set_pad(input::PadButton::Cross.mask());
        let _ = world.tick();
    }
    let m = world.slot_machine.as_ref().unwrap();
    assert_eq!(m.phase(), SlotPhase::Payout);
    assert_eq!(m.reels_stopped(), crate::slot_machine::REEL_COUNT);
    let result = m.last_result().expect("spin evaluated");
    let before = m.balance();
    // A fresh Cross edge collects the (possibly zero) payout back to Idle.
    world.set_pad(0);
    let _ = world.tick();
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    let m = world.slot_machine.as_ref().unwrap();
    assert_eq!(m.phase(), SlotPhase::Idle);
    assert_eq!(m.balance(), before + result.payout);
}

#[test]
fn slot_machine_spin_accrues_the_net_take() {
    use crate::slot_machine::NET_TAKE_NORMAL_SPIN;
    let mut world = World::new();
    world.enter_slot_machine(slot_test_machine(50));
    assert_eq!(world.slot_machine.as_ref().unwrap().net_take(), 0);
    world.set_pad(0);
    world.set_pad(input::PadButton::Cross.mask());
    let _ = world.tick();
    assert_eq!(
        world.slot_machine.as_ref().unwrap().net_take(),
        NET_TAKE_NORMAL_SPIN
    );
}

#[test]
fn slot_machine_tick_without_session_falls_back_to_return_mode() {
    let mut world = World::new();
    world.slot_return_mode = SceneMode::Field;
    world.mode = SceneMode::SlotMachine;
    let _ = world.tick();
    assert_eq!(world.mode, SceneMode::Field);
}
