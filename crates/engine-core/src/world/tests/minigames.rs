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
    for _ in 0..crate::dance::COUNTIN_TOTAL_VSYNCS {
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
    // The intro cue fires once, on the hold-segment entry, stored straight
    // into ring slot 0 as `FUN_801d2d98` does - not through the cue
    // dispatcher, which would classify `0x200` as a CD-XA voice.
    let intro = crate::world::SfxRingOp::WriteSlot(
        crate::dance::STAGE_CUE_RING_SLOT,
        crate::dance::COUNTIN_INTRO_CUE as i16,
    );
    let mut ops = world.take_sfx_ring_ops();
    for _ in 0..crate::dance::COUNTIN_TOTAL_VSYNCS {
        world.set_pad(0);
        let _ = world.tick();
        ops.extend(world.take_sfx_ring_ops());
    }
    assert_eq!(
        ops.iter().filter(|o| **o == intro).count(),
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

/// Entering a run is the overlay's state 1: the one-shot mode requests are
/// consumed, the free-play flag stands, and the pass flag is raised.
#[test]
fn enter_dance_consumes_the_mode_request_and_raises_the_pass_flag() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    for f in [0x133, 0x134, 0x135, 0x428] {
        world.system_flag_set(f);
    }
    world.enter_dance(crate::dance::DanceGame::new(dance_test_chart(), false));
    for f in [0x133, 0x134, 0x135] {
        assert!(!world.system_flag_test(f), "flag {f:#x} consumed");
    }
    assert!(world.system_flag_test(0x428));
    assert!(world.system_flag_test(crate::dance::WIN_FLAG));
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

fn fishing_test_species(
    index: usize,
    pull_factor: i32,
) -> legaia_asset::fishing_species::FishingSpecies {
    legaia_asset::fishing_species::FishingSpecies {
        index,
        name_ptr_va: 0,
        score_value: 10_000,
        pull_factor,
        dart_factor: 60,
        sink_factor: 4,
        depth_gate: 1024,
        roll_cutoff_a: 200,
        roll_cutoff_b: 512,
        roll_cutoff_c: 90,
        strike_gate: 400,
    }
}

/// Synthetic overlay tables: ten species, every lure row spawning species
/// 3 / 5 so any band hooks something, and no cadence templates (the slow,
/// held-reel strike path).
fn fishing_test_tables(pull_factor: i32) -> crate::fishing::FishingTables {
    let mut page = vec![[0u32; 8]; 8];
    for row in page.iter_mut().take(3) {
        *row = [3, 5, 3, 5, 5, 0, 0, 0];
    }
    crate::fishing::FishingTables {
        species: (0..10)
            .map(|i| fishing_test_species(i, pull_factor))
            .collect(),
        spawn: [page.clone(), page],
        cadence: Vec::new(),
    }
}

fn fishing_phase(world: &World) -> crate::fishing::PondPhase {
    world.minigames.fishing.as_ref().unwrap().phase()
}

/// One frame with `mask` held; a button's edge is its first held frame.
fn fishing_frame(world: &mut World, mask: u16) {
    world.set_pad(mask);
    let _ = world.tick();
}

/// Circle edge -> wind-up -> power sweep -> Circle lock -> flight -> the lure
/// lands in the pre-hook loop.
fn fishing_cast(world: &mut World) {
    use crate::fishing::{FLIGHT_FRAMES, PondPhase, WINDUP_FRAMES};
    let circle = input::PadButton::Circle.mask();
    fishing_frame(world, 0);
    fishing_frame(world, circle);
    for _ in 0..WINDUP_FRAMES + 24 {
        fishing_frame(world, 0);
    }
    assert_eq!(fishing_phase(world), PondPhase::Power);
    fishing_frame(world, circle);
    assert_eq!(fishing_phase(world), PondPhase::Flight);
    for _ in 0..FLIGHT_FRAMES {
        fishing_frame(world, 0);
    }
    assert_eq!(fishing_phase(world), PondPhase::Waiting);
}

/// Hold Cross (reel A) through the pre-hook loop, re-casting whenever the
/// empty line is fully reeled in, until a fish strikes. Returns the frame's
/// events on the hook frame.
fn fishing_hold_until_hooked(world: &mut World) -> Vec<crate::fishing::PondEvent> {
    use crate::fishing::PondPhase;
    for _ in 0..40_000 {
        fishing_frame(world, input::PadButton::Cross.mask());
        match fishing_phase(world) {
            PondPhase::Hooked => return world.minigames.fishing_events.clone(),
            PondPhase::Idle => fishing_cast(world),
            _ => {}
        }
    }
    panic!("no strike over the held-reel budget");
}

#[test]
fn enter_fishing_suspends_mode_and_exit_restores_it() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_fishing_session(&fishing_test_tables(64), 0, None);
    assert_eq!(world.mode, SceneMode::Fishing);
    assert!(world.minigames.fishing.is_some());
    let session = world.exit_fishing();
    assert!(session.is_some());
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.minigames.fishing.is_none());
}

/// The session opens from the persistent save-block words on the world and
/// banks every one of them back on exit - the migration that made the
/// world's `fishing_*` cells and the session's fields one state.
#[test]
fn a_session_seeds_from_and_banks_back_the_persistent_words() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.minigames.fishing_points = 1234;
    world.minigames.fishing_best_points = 99;
    world.minigames.fishing_best_fish = 4;
    world.minigames.fishing_casts = 60;
    world.minigames.fishing_prizes_purchased = 0b100;
    // The lure gate re-points the lure index at one the party holds: only
    // the Heavy Lure (0x9f) is in the bag.
    world.minigames.fishing_lure = 0;
    world.party.inventory.insert(0x9f, 3);
    world.enter_fishing_session(&fishing_test_tables(64), 1, None);
    {
        let s = world.minigames.fishing.as_ref().unwrap();
        assert_eq!(s.record.points, 1234);
        assert_eq!((s.record.best_points, s.record.best_fish), (99, 4));
        assert_eq!(s.casts, 60);
        assert_eq!(s.purchased_mask, 0b100);
        assert_eq!(s.lure, 2, "the lure gate re-pointed at the owned lure");
        assert_eq!(s.venue, 1);
    }
    assert_eq!(
        world.minigames.fishing_lure, 2,
        "written back, as retail does"
    );
    fishing_cast(&mut world);
    world.exit_fishing();
    assert_eq!(world.minigames.fishing_casts, 61, "the landing banked back");
    assert_eq!(world.minigames.fishing_points, 1234);
}

#[test]
fn fishing_casts_hooks_and_reels_to_a_resolution() {
    use crate::fishing::{PondEvent, PondPhase};
    let mut world = World::new();
    world.enter_fishing_session(&fishing_test_tables(64), 0, None);
    for _ in 0..3 {
        fishing_frame(&mut world, 0);
    }
    assert_eq!(fishing_phase(&world), PondPhase::Idle);
    fishing_cast(&mut world);
    let events = fishing_hold_until_hooked(&mut world);
    assert!(events.iter().any(|e| matches!(e, PondEvent::Hooked(_))));
    // Reel while tension is safe, rest when it climbs, until it resolves.
    for _ in 0..40_000 {
        if fishing_phase(&world) != PondPhase::Hooked {
            break;
        }
        let t = world.minigames.fishing.as_ref().unwrap().tension();
        let mask = if t < 0x800 {
            input::PadButton::Cross.mask()
        } else {
            0
        };
        fishing_frame(&mut world, mask);
    }
    assert!(matches!(
        fishing_phase(&world),
        PondPhase::Landed | PondPhase::Snapped
    ));
    // Circle dismisses the result back to the shore, raising the recast event.
    fishing_frame(&mut world, 0);
    fishing_frame(&mut world, input::PadButton::Circle.mask());
    assert_eq!(fishing_phase(&world), PondPhase::Idle);
    assert!(world.minigames.fishing_events.contains(&PondEvent::Recast));
}

/// The hook cue and the celebration cues come off the **session's own
/// events**, on the world's shared cue queue, so every host that ticks the
/// world hears them. They used to live on `LineActorSim`, which only the
/// native window drives, so the strike and the catch were silent in both
/// browsers.
#[test]
fn the_strike_and_catch_edges_queue_their_cues_on_the_world() {
    use crate::fishing::PondPhase;
    use crate::fishing_actors::{CELEBRATE_CUE, HOOK_CUE};
    let mut world = World::new();
    world.enter_fishing_session(&fishing_test_tables(16), 0, None);
    let _ = world.drain_minigame_sfx_cues();
    fishing_cast(&mut world);
    assert!(
        world.drain_minigame_sfx_cues().is_empty(),
        "casting frames raise no cue"
    );
    fishing_hold_until_hooked(&mut world);
    assert!(
        world
            .drain_minigame_sfx_cues()
            .contains(&u16::from(HOOK_CUE)),
        "the strike queues the hook cue"
    );
    // A weak fish: holding reel A the whole fight never pins tension.
    for _ in 0..40_000 {
        if fishing_phase(&world) != PondPhase::Hooked {
            break;
        }
        let t = world.minigames.fishing.as_ref().unwrap().tension();
        let mask = if t < 0x800 {
            input::PadButton::Cross.mask()
        } else {
            0
        };
        fishing_frame(&mut world, mask);
    }
    assert_eq!(fishing_phase(&world), PondPhase::Landed);
    let cues = world.drain_minigame_sfx_cues();
    assert!(
        cues.contains(&u16::from(CELEBRATE_CUE)),
        "a landed fish queues the celebration cue, got {cues:?}"
    );
}

/// The reel buttons are the retail packed-pad bits decoded by
/// `ReelInput::from_pad_mask`: `0x40` Cross = reel A, `0x80` Square = reel B,
/// both held = reel A. Circle (`0x20`) is the cast input, not a reel.
///
/// Tension after the hook frame plus one fight frame differs by which reel
/// was held, because the two divisors differ (`rod*9 + 0x23` / `rod*6 + 0x19`).
#[test]
fn fishing_reel_buttons_are_cross_and_square_with_cross_winning() {
    use crate::fishing::PondPhase;
    let run = |mask: u16| -> i32 {
        let mut world = World::new();
        world.enter_fishing_session(&fishing_test_tables(4000), 0, None);
        fishing_cast(&mut world);
        fishing_hold_until_hooked(&mut world);
        assert_eq!(fishing_phase(&world), PondPhase::Hooked);
        let before = world.minigames.fishing.as_ref().unwrap().tension();
        // Same BiosRand stream from here in every run, so the pull matches.
        fishing_frame(&mut world, mask);
        world.minigames.fishing.as_ref().unwrap().tension() - before
    };
    let reel_a = run(input::PadButton::Cross.mask());
    let reel_b = run(input::PadButton::Square.mask());
    let both = run(input::PadButton::Cross.mask() | input::PadButton::Square.mask());
    let circle = run(input::PadButton::Circle.mask());
    assert_ne!(reel_a, reel_b, "the two divisors must be distinguishable");
    assert_eq!(both, reel_a, "Cross wins - the retail decoder's priority");
    assert!(circle <= 0, "Circle is not a reel: the gauge bleeds off");
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
    // Each reel has its own stop button (Square / Cross / Circle -> reels
    // 0 / 1 / 2): a repeated Cross stops reel 1 alone.
    for _ in 0..3 {
        world.set_pad(0);
        let _ = world.tick();
        world.set_pad(input::PadButton::Cross.mask());
        let _ = world.tick();
    }
    assert_eq!(
        world
            .minigames
            .slot_machine
            .as_ref()
            .unwrap()
            .reels_stopped(),
        1,
        "Cross is reel 1's button only"
    );
    // One frame with Square + Circle edges stops the other two together.
    world.set_pad(0);
    let _ = world.tick();
    world.set_pad(input::PadButton::Square.mask() | input::PadButton::Circle.mask());
    let _ = world.tick();
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

/// The cash-out submenu through the World pad path: Triangle opens it, Down
/// picks the quit row, Cross takes it, and the leave fade commits the
/// balance into the coin bank and drops back to the interrupted mode - with
/// the submenu's cues in ring slot 0.
#[test]
fn slot_machine_cash_out_menu_quits_through_the_pad() {
    use crate::slot_machine::{CUE_MENU_CONFIRM, CUE_MENU_CURSOR, SlotPhase};
    let mut world = World::new();
    let before = world.mode;
    world.enter_slot_machine(slot_test_machine(41));
    let press = |world: &mut World, b: input::PadButton| {
        world.set_pad(0);
        let _ = world.tick();
        world.set_pad(b.mask());
        let _ = world.tick();
    };
    world.audio.sfx_ring_ops.clear();
    press(&mut world, input::PadButton::Triangle);
    let m = world.minigames.slot_machine.as_ref().unwrap();
    assert_eq!(m.phase(), SlotPhase::Menu);
    press(&mut world, input::PadButton::Down);
    press(&mut world, input::PadButton::Cross);
    let ring = std::mem::take(&mut world.audio.sfx_ring_ops);
    use crate::world::SfxRingOp::WriteSlot;
    assert!(ring.contains(&WriteSlot(0, CUE_MENU_CONFIRM)));
    assert!(ring.contains(&WriteSlot(0, CUE_MENU_CURSOR)));
    assert_eq!(
        world.minigames.slot_machine.as_ref().unwrap().phase(),
        SlotPhase::Leaving,
        "Cross never spun the reels on the menu"
    );
    for _ in 0..20 {
        world.set_pad(0);
        let _ = world.tick();
    }
    assert!(world.minigames.slot_machine.is_none(), "the machine left");
    assert_eq!(world.mode, before);
    assert_eq!(world.minigames.casino_coins, 41);
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

/// Every minigame exit path closes a door-entered round trip through one
/// kernel, `close_minigame_round_trip` - the Start escape, the native hotkeys
/// and the page's fishing button. Those last two used to call the bare
/// `exit_*`, which left the scene backup armed and the winnings unbanked.
#[test]
fn close_minigame_round_trip_closes_a_door_entry_and_nothing_else() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.active_scene_label = "sioro".to_string();
    world.minigames.casino_coins = 100;
    world.arm_minigame_warp();
    world.active_scene_label = "minigame".to_string();
    world.mode = SceneMode::Fishing;
    world.minigames.winnings = 40;
    world.close_minigame_round_trip();
    assert_eq!(world.active_scene_label, "sioro");
    assert_eq!(world.minigames.casino_coins, 140);
    assert_eq!(world.mode, SceneMode::Field);
    assert!(world.minigames.scene_backup.is_none());

    // A launcher-opened session armed nothing: the call leaves it alone.
    world.mode = SceneMode::Fishing;
    world.minigames.winnings = 7;
    world.close_minigame_round_trip();
    assert_eq!(world.mode, SceneMode::Fishing);
    assert_eq!(world.minigames.casino_coins, 140);
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

/// Four minigame modes clear the frame to black over whatever colour the
/// suspended field's `4C 13` left; fishing and the field keep that colour.
#[test]
fn minigame_frames_clear_to_black_over_the_suspended_field_colour() {
    let mut world = World::new();
    world.presentation.clear_rgb = [0x3C, 0x28, 0x14];
    world.mode = SceneMode::Field;
    assert_eq!(world.frame_clear_rgb(), [0x3C, 0x28, 0x14]);
    for mode in [
        SceneMode::SlotMachine,
        SceneMode::BakaFighter,
        SceneMode::MuscleDome,
        SceneMode::Dance,
    ] {
        world.mode = mode;
        assert_eq!(world.frame_clear_rgb(), [0; 3], "{mode:?}");
    }
    world.mode = SceneMode::Fishing;
    assert_eq!(world.frame_clear_rgb(), [0x3C, 0x28, 0x14]);
}

/// A dance miss is a direct store of `0x210` into ring slot 3, and a closed
/// chain keys the two sting voices on the queue both hosts drain - in the
/// dance's VAB (`a1 = 2`), program 1, at a note in the random band.
#[test]
fn dance_award_sounds_reach_the_ring_and_the_voice_queue() {
    use crate::dance::{AWARD_CUE_RING_SLOT, AWARD_MISS_CUE, DanceAwardSound};
    let mut world = World::new();
    world.route_dance_award_sounds(&[DanceAwardSound::Cue(AWARD_MISS_CUE)]);
    assert_eq!(
        world.take_sfx_ring_ops(),
        vec![crate::world::SfxRingOp::WriteSlot(
            AWARD_CUE_RING_SLOT,
            AWARD_MISS_CUE as i16
        )]
    );
    world.route_dance_award_sounds(&[DanceAwardSound::Sting { r: 0, random: true }]);
    let keys = world.take_sfx_voice_keys();
    assert_eq!(keys.len(), 2);
    assert_eq!(
        keys.iter().map(|k| k.voice).collect::<Vec<_>>(),
        vec![0x12, 0x13]
    );
    for k in &keys {
        assert_eq!((k.vab_program_tone.0, k.vab_program_tone.1), (2, 1));
        assert!((0x3C..0x3F).contains(&k.note_and_fine.0));
    }
}

/// The lure landing stores cue `0x204` into ring slot 2, once per cast, on
/// the frame the persistent cast counter moves (`FUN_801d26cc` `0x801D2950`).
#[test]
fn the_lure_landing_stores_its_cue_into_ring_slot_two() {
    let mut world = World::new();
    world.enter_fishing_session(&fishing_test_tables(64), 0, None);
    for _ in 0..3 {
        fishing_frame(&mut world, 0);
    }
    let _ = world.take_sfx_ring_ops();
    let mut ops = Vec::new();
    let circle = input::PadButton::Circle.mask();
    fishing_frame(&mut world, 0);
    fishing_frame(&mut world, circle);
    ops.extend(world.take_sfx_ring_ops());
    for _ in 0..crate::fishing::WINDUP_FRAMES + 24 {
        fishing_frame(&mut world, 0);
        ops.extend(world.take_sfx_ring_ops());
    }
    fishing_frame(&mut world, circle);
    ops.extend(world.take_sfx_ring_ops());
    let landing = crate::world::SfxRingOp::WriteSlot(2, 0x204);
    assert!(!ops.contains(&landing), "nothing before the lure flies");
    for _ in 0..crate::fishing::FLIGHT_FRAMES {
        fishing_frame(&mut world, 0);
        ops.extend(world.take_sfx_ring_ops());
    }
    assert_eq!(fishing_phase(&world), crate::fishing::PondPhase::Waiting);
    assert_eq!(ops.iter().filter(|o| **o == landing).count(), 1);
}

/// A hooked rod held past its bend cap creaks: cue `0x201` into ring slot 1,
/// first on the frame the cap is crossed, then at most once per re-armed
/// countdown (`rand() % 200 + 60` vsyncs).
#[test]
fn a_hooked_rod_bent_past_its_cap_creaks() {
    let mut world = World::new();
    world.enter_fishing_session(&fishing_test_tables(64), 0, None);
    for _ in 0..3 {
        fishing_frame(&mut world, 0);
    }
    fishing_cast(&mut world);
    let _ = fishing_hold_until_hooked(&mut world);
    let _ = world.take_sfx_ring_ops();
    let creak = crate::world::SfxRingOp::WriteSlot(1, 0x201);
    let cross = input::PadButton::Cross.mask();
    let mut frames_with_creak = Vec::new();
    for f in 0..120 {
        if fishing_phase(&world) != crate::fishing::PondPhase::Hooked {
            break;
        }
        fishing_frame(&mut world, cross);
        if world.take_sfx_ring_ops().contains(&creak) {
            frames_with_creak.push(f);
        }
    }
    assert!(!frames_with_creak.is_empty(), "the rod creaks under load");
    for w in frames_with_creak.windows(2) {
        assert!(
            w[1] - w[0] >= 60,
            "re-armed at least 60 vsyncs out: {frames_with_creak:?}"
        );
    }
}
