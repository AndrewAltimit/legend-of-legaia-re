//! Contracts the play window's minigame wiring depends on.
//!
//! The host call sites live in the `legaia-engine` binary (the winit window),
//! which no integration test can link against, so these pin the engine-side
//! halves those call sites rely on: the Muscle Dome round time meter's
//! per-frame advance, the dance HUD's judged-vs-displayed combo-slot split, and
//! the slot machine's session-start strip build. It also pins the two facts
//! that keep the mode-24 minigame door warp deliberately unwired.

use legaia_engine_core::world::{SceneMode, World};

// --- mode-24 minigame door warp: why it stays unwired ----------------------

/// The evidence behind leaving `World::minigame_return_warp` uncalled: nothing
/// in the port fills the accumulator its commit half banks, so the commit would
/// be an add of zero however it were called.
///
/// Retail *does* have a producer - the Baka Fighter end-of-match tally drains
/// into `_DAT_80084440`, which `FUN_80026018` then adds into the casino coin
/// bank `_DAT_800845A4`. This port pays that drain into party gold
/// (`World::money`) instead, which is a different retail word (`0x8008459C`).
/// Redirecting it is the prerequisite, and it lives in `World::tick_baka_fighter`
/// / `World::exit_baka_fighter`.
///
/// So this test is a tripwire: the day the accumulator gains a producer it
/// fails, and whoever made that change has to land the warp's two call sites in
/// the same breath rather than leaving the prize in a stage nothing drains.
#[test]
fn no_minigame_exit_credits_the_mode24_winnings_accumulator() {
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.casino_coins = 60;

    world.enter_slot_machine(slot_machine_for_test());
    assert_eq!(world.mode, SceneMode::SlotMachine);
    let machine = world.exit_slot_machine().expect("session returned");
    // The slot overlay's own state-100 commit is an assignment into the bank...
    assert_eq!(world.casino_coins, machine.balance().max(0) as u32);
    // ...and the mode-24 accumulator is untouched by it.
    assert_eq!(
        world.minigame_winnings, 0,
        "nothing in the engine feeds the mode-24 winnings accumulator"
    );
    // The suspend/restore contract, not a warp, is what carries the mode back.
    assert_eq!(world.mode, SceneMode::Field);
}

/// The engine enters minigames by *suspending* the current mode, so a warp that
/// forces `SceneMode::Field` on exit would be wrong for any entry that did not
/// come from the field. Pin the suspend contract that makes that true.
#[test]
fn minigame_entry_suspends_and_restores_the_interrupted_mode() {
    let mut world = World::new();
    world.mode = SceneMode::Battle;
    world.enter_slot_machine(slot_machine_for_test());
    assert_eq!(world.mode, SceneMode::SlotMachine);
    world.exit_slot_machine();
    assert_eq!(world.mode, SceneMode::Battle, "suspend/restore, not a warp");
}

fn slot_machine_for_test() -> legaia_engine_core::slot_machine::SlotMachine {
    use legaia_asset::slot_payout::SlotPayoutTable;
    let payouts = SlotPayoutTable {
        payouts: [0, 2, 3, 4, 5, 6, 8, 10, 15, 20],
    };
    legaia_engine_core::slot_machine::SlotMachine::new(payouts, 0x1234_5678, 60)
}

// --- slot machine: both strips are built at session start -----------------

/// `SlotMachine::new` runs every reel through `build_reel`, so the display strip
/// is a two-of-each permutation of the ten symbols - not a flat or sequential
/// strip. This is what the reels show.
#[test]
fn a_new_machine_seeds_its_display_strips_from_the_permuted_symbol_strips() {
    use legaia_engine_core::slot_machine::{REEL_COUNT, STRIP_LEN, SYMBOL_COUNT};
    let m = slot_machine_for_test();
    for reel in 0..REEL_COUNT {
        let strip = m.strips()[reel];
        assert_eq!(strip.len(), STRIP_LEN);
        let mut counts = [0usize; SYMBOL_COUNT];
        for &s in &strip {
            counts[s as usize] += 1;
        }
        assert_eq!(counts, [2; SYMBOL_COUNT], "reel {reel} is a permutation");
        assert!(
            strip.windows(2).any(|w| w[0] != w[1]),
            "reel {reel} is shuffled, not flat"
        );
    }
}

// --- Muscle Dome round time meter -----------------------------------------

#[test]
fn the_time_meter_climbs_through_selection_and_drains_once_it_ends() {
    use legaia_engine_core::muscle_dome::{
        MuscleCard, MuscleDomeSession, MusclePhase, TIME_METER_MAX,
    };
    let hand = || {
        [0u8, 1, 2, 3].map(|i| MuscleCard {
            command_id: 0x0C + i,
            cost: 1,
        })
    };
    let mut s = MuscleDomeSession::new(hand(), hand(), [100, 100], [500, 500], 1);
    assert_eq!(s.phase(), MusclePhase::Select);
    assert_eq!(s.time_meter(), 0);

    // Empty bar first, then a full climb over TIME_METER_MAX ticks.
    let empty_y = s.time_meter_bar_y();
    for _ in 0..TIME_METER_MAX {
        s.tick_time_meter(1);
    }
    assert_eq!(s.time_meter(), TIME_METER_MAX, "clamped at a full bar");
    let full_y = s.time_meter_bar_y();
    assert!(full_y > empty_y, "the bar rises as the meter fills");

    // Leaving the selection phase drains it again.
    s.end_selection();
    assert_ne!(s.phase(), MusclePhase::Select);
    for _ in 0..TIME_METER_MAX {
        s.tick_time_meter(1);
    }
    assert_eq!(s.time_meter(), 0, "drains outside the selection phase");
    assert_eq!(s.time_meter_bar_y(), empty_y);
}

// --- dance: the judged cell is not the displayed cell ----------------------

/// The beat track's flash and the judge's combo slot are two different tests,
/// and the HUD reads the first while the scoring path reads the second. Pin the
/// split so a future edit cannot quietly collapse them: the track's mask widens
/// with the dancer's level, and its flash window is far narrower than the
/// judge's acceptance window.
#[test]
fn the_displayed_combo_slot_is_not_the_judged_one() {
    use legaia_engine_core::dance::{
        BEAT_WINDOW, COMBO_FLASH_WINDOW, dance_beat_level_mask, dance_combo_window_bright,
    };
    assert_eq!(dance_beat_level_mask(0), 3);
    assert_eq!(
        dance_beat_level_mask(1),
        7,
        "a promoted lane widens the mask"
    );

    // Beat 7 is a track combo slot at level 0 but not at level 1.
    assert!(dance_combo_window_bright(7, 0, 0));
    assert!(!dance_combo_window_bright(7, 1, 0));
    // Beat 3 is a combo slot at both levels.
    assert!(dance_combo_window_bright(3, 0, 0));
    assert!(dance_combo_window_bright(3, 1, 0));

    // The flash window is strictly inside the judge's acceptance window: a
    // phase past the flash edge is still judged, just not lit.
    const { assert!(COMBO_FLASH_WINDOW < BEAT_WINDOW) };
    assert!(!dance_combo_window_bright(3, 0, COMBO_FLASH_WINDOW));
}

/// The HUD score readout goes through the retail decimal split, so leading
/// zeros are blank slots and zero draws nothing.
#[test]
fn the_dance_score_readout_blanks_leading_zeros() {
    use legaia_engine_core::dance::dance_number_digits;
    assert_eq!(dance_number_digits(0), [None; 8]);
    let d = dance_number_digits(407);
    assert_eq!(&d[..5], &[None; 5]);
    assert_eq!(&d[5..], &[Some(4), Some(0), Some(7)]);
}

/// The scrolling note row slides one 16-px cell per beat: note `i` sits a cell
/// right of note `i - 1`, and the whole row drifts left as the intra-beat
/// fraction grows.
#[test]
fn the_beat_track_notes_scroll_one_cell_per_beat() {
    use legaia_engine_core::dance::{BEAT_PERIOD, dance_beat_track_note_x};
    let base = 60;
    assert_eq!(
        dance_beat_track_note_x(base, 1, 0) - dance_beat_track_note_x(base, 0, 0),
        16
    );
    assert!(
        dance_beat_track_note_x(base, 0, BEAT_PERIOD - 1) < dance_beat_track_note_x(base, 0, 0),
        "the row drifts left within a beat"
    );
}

// --- casino: the coin-exchange counter the slot entry point buys through ---

/// The play window tops a thin coin bank up by running a purchase through the
/// ported counter (`coin_exchange_quote`, `FUN_801E6F70`) instead of conjuring
/// a stake, so this pins the contract that host encodes: the entry field is
/// eight single-digit cells stored **units first**, coins are a flat
/// `COIN_PRICE_GOLD` each, and the sale is refused - with nothing debited -
/// when either the gold or the stock gate fails.
#[test]
fn the_coin_counter_prices_the_slot_entry_stake() {
    use legaia_engine_core::slot_machine::{COIN_PRICE_GOLD, coin_exchange_quote};
    // The host lays 100 out as [0, 0, 1, 0, 0, 0, 0, 0] - units first.
    let digits = [0u8, 0, 1, 0, 0, 0, 0, 0];
    let cost = 100 * COIN_PRICE_GOLD;
    let q = coin_exchange_quote(&digits, cost, i32::MAX);
    assert_eq!(q.coins, 100);
    assert_eq!(q.cost, cost);
    assert!(q.is_valid(), "exactly enough gold buys the stake");
    // One gold short: refused, and the host keeps the party's money.
    assert!(!coin_exchange_quote(&digits, cost - 1, i32::MAX).is_valid());
    // Stock is the other gate, independent of gold.
    let thin = coin_exchange_quote(&digits, cost, 99);
    assert!(thin.affordable && !thin.in_stock && !thin.is_valid());
}
