//! Disc-gated: drive the **real** parsed slot-machine payout table (PROT 0975)
//! through the engine slot-machine rules engine
//! ([`legaia_engine_core::slot_machine`]).
//!
//! The payout-table parser itself is pinned by `legaia-asset`'s
//! `slot_payout_real`; this closes the engine end - the play-window load path
//! (`SceneHost::open_disc` -> `entry_bytes_extended(975)` ->
//! `static_overlay::as_loaded` -> `parse`) resolves a real table, and a
//! `SlotMachine` session spins, stops, evaluates, and commits its balance
//! into the world's casino coin bank. No Sony bytes are asserted, only
//! structural facts. Skips + passes when `LEGAIA_DISC_BIN` is absent.

use legaia_asset::slot_payout::BONUS_VALUE_BASE;
use legaia_asset::static_overlay;
use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;
use legaia_engine_core::slot_machine::{
    BONUS_SPIN_UP_FRAMES, REEL_COUNT, SPIN_UP_FRAMES, SlotMachine, SlotPhase,
};
use legaia_engine_core::world::{SceneMode, World};

#[test]
fn playwindow_load_path_spins_the_real_payout_table() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return;
        }
    };
    let rec = static_overlay::overlay_map()
        .by_prot_index(legaia_asset::slot_payout::SLOT_OVERLAY_PROT_INDEX as u32)
        .expect("slot overlay in static map");
    let raw = host
        .index
        .entry_bytes_extended(rec.prot_index)
        .expect("read PROT 0975 (extended)");
    let loaded = static_overlay::as_loaded(&raw, rec).expect("as-loaded form");
    let payouts = legaia_asset::slot_payout::parse(&loaded).expect("real payout table parses");

    // Drive the session through the World exactly like play-window's O key.
    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.casino_coins = 200;
    world.enter_slot_machine(SlotMachine::new(payouts.clone(), 0xC0FFEE, 200));
    assert_eq!(world.mode, SceneMode::SlotMachine);

    // Play a handful of spins through the pad; every evaluated spin must
    // account coins exactly (bet debited, collect credits the evaluated
    // payout, normal wins read the real table). This seed happens to hit a
    // jackpot line and play the bonus round out, so the bonus-round invariants
    // below are exercised too - `bonus_spins_seen` keeps that non-vacuous.
    let mut bonus_spins_seen = 0;
    for spin in 0..8 {
        let m = world.slot_machine.as_ref().unwrap();
        if !m.can_spin() {
            break;
        }
        let before = m.balance();
        // Every spin charges the flat cost (3 coins, 1 during a feature).
        let cost = m.spin_cost();
        // A bonus spin - and the one straight after a bonus round - runs long:
        // the display strip is refilled one row per frame, so the reels have to
        // travel before the numerals (or the symbols coming back) reach the
        // payline. Retail forces `DAT_801d3c90 = 0x18` on both edges, so the
        // spin-up is mode-dependent; ask the machine rather than assume it.
        let bonus_spin = m.in_bonus_round();
        let armed = m.next_spin_up_frames();
        world.set_pad(0);
        world.set_pad(PadButton::Cross.mask());
        let _ = world.tick();
        assert_eq!(
            world.slot_machine.as_ref().unwrap().phase(),
            SlotPhase::Spinning
        );
        assert_eq!(
            world.slot_machine.as_ref().unwrap().balance(),
            before - cost
        );
        let mut waited = 0;
        let limit = SPIN_UP_FRAMES + BONUS_SPIN_UP_FRAMES + 8;
        while world.slot_machine.as_ref().unwrap().phase() == SlotPhase::Spinning && waited < limit
        {
            world.set_pad(0);
            let _ = world.tick();
            waited += 1;
        }
        assert_eq!(
            world.slot_machine.as_ref().unwrap().phase(),
            SlotPhase::Stopping
        );
        assert_eq!(
            waited, armed,
            "spin {spin}: the spin-up runs {armed} frames (bonus spin: {bonus_spin})"
        );
        assert!(
            armed == SPIN_UP_FRAMES || armed == SPIN_UP_FRAMES + BONUS_SPIN_UP_FRAMES,
            "spin {spin}: the spin-up is one of the two retail lengths"
        );
        for _ in 0..REEL_COUNT {
            world.set_pad(0);
            let _ = world.tick();
            world.set_pad(PadButton::Cross.mask());
            let _ = world.tick();
        }
        let m = world.slot_machine.as_ref().unwrap();
        assert_eq!(m.phase(), SlotPhase::Payout);
        let result = m.last_result().expect("spin evaluated");
        if let (Some(sym), false) = (result.symbol, result.bonus_spin) {
            // A normal-line win pays exactly the real table's byte.
            assert_eq!(
                result.payout,
                payouts.payout(sym).unwrap_or(0) as i32,
                "spin {spin}: normal win pays the disc table value"
            );
        }
        if result.bonus_spin {
            // A bonus spin: the reels really did rotate onto the numeral strip,
            // the marquee tally is complete, and its product IS the payout - the
            // two are the same latch, so this cannot drift.
            bonus_spins_seen += 1;
            for reel in 0..REEL_COUNT {
                let v = m.payline_symbol(reel);
                assert!(
                    v >= BONUS_VALUE_BASE,
                    "spin {spin}: reel {reel} pays on a bonus value, got {v:#x}"
                );
            }
            let tally = m.tally();
            assert!(m.tally_complete(), "spin {spin}: all three columns claimed");
            assert!(
                tally.iter().all(|&n| (1..=10).contains(&n)),
                "spin {spin}: three numerals 1..=10, got {tally:?}"
            );
            assert_eq!(
                m.tally_product() as i32,
                result.payout,
                "spin {spin}: the payout is the product of the claimed tally {tally:?}"
            );
            assert_eq!(result.line, Some(1), "spin {spin}: the centre line pays");
            assert!(
                (1..=1000).contains(&result.payout),
                "spin {spin}: bounded 1..=1000"
            );
        }
        let before_collect = m.balance();
        world.set_pad(0);
        let _ = world.tick();
        world.set_pad(PadButton::Cross.mask());
        let _ = world.tick();
        let m = world.slot_machine.as_ref().unwrap();
        assert_eq!(m.phase(), SlotPhase::Idle);
        assert_eq!(m.balance(), before_collect + result.payout);
        eprintln!(
            "[slots] spin {spin}: symbol {:?} payout {} balance {}",
            result.symbol,
            result.payout,
            m.balance()
        );
    }

    // The bonus-round assertions above are not vacuous: this seed plays one.
    assert!(
        bonus_spins_seen >= 1,
        "the seeded run reaches the bonus round (saw {bonus_spins_seen} bonus spins)"
    );

    // Cash out: the world bank is ASSIGNED the final playing balance.
    let final_balance = world.slot_machine.as_ref().unwrap().balance();
    let m = world.exit_slot_machine().expect("machine installed");
    assert_eq!(m.balance(), final_balance);
    assert_eq!(world.casino_coins as i32, final_balance);
    assert_eq!(world.mode, SceneMode::Field);
}
