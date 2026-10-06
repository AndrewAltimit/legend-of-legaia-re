use super::*;

fn species(index: usize, score_value: i32, strike_gate: i32) -> FishingSpecies {
    FishingSpecies {
        index,
        name_ptr_va: 0,
        score_value,
        pull_factor: 250,
        dart_factor: 60,
        sink_factor: 4,
        depth_gate: 1024,
        roll_cutoff_a: 200,
        roll_cutoff_b: 512,
        roll_cutoff_c: 90,
        strike_gate,
    }
}

/// `is_available` folds three refusals together; `is_latched` asks only
/// about the one-time bit. Reading availability as the latch is what
/// printed "sold" beside every unaffordable one-time prize on a fresh
/// save.
#[test]
fn engine_pad_input_counts_retail_pad_nudges() {
    use crate::input::PadButton as B;
    let m = |bs: &[B]| bs.iter().fold(0u16, |a, b| a | b.mask());
    // Cross + Square pressed together: ONE nudge (the reel pair is one
    // mask at `0x801D3458`), both reel bits held.
    let i = PondInput::from_engine_pad(m(&[B::Cross, B::Square]), 0);
    assert_eq!(i.edge_bonus, 1);
    assert_eq!(i.reel_mask, REEL_A_PAD_BIT | REEL_B_PAD_BIT);
    // Left + Right + a reel: three.
    let i = PondInput::from_engine_pad(m(&[B::Left, B::Right, B::Cross]), 0);
    assert_eq!(i.edge_bonus, 3);
    // The cast press is an edge but not a nudge.
    let i = PondInput::from_engine_pad(m(&[B::Circle]), 0);
    assert!(i.cast_edge);
    assert_eq!(i.edge_bonus, 0);
    // A held (not newly pressed) button nudges nothing.
    let held = m(&[B::Left, B::Cross]);
    let i = PondInput::from_engine_pad(held, held);
    assert_eq!(i.edge_bonus, 0);
    // The held word carries the held D-pad side too - the rod's roll
    // reads it - in the packed layout.
    assert_eq!(i.reel_mask, REEL_A_PAD_BIT | ROD_PAD_LEFT);
}

#[test]
fn the_one_time_latch_is_not_the_same_question_as_availability() {
    let rows = vec![
        PrizeRow {
            row: 0,
            limit: 1,
            price: 500,
            item_id: 0x70,
            name: None,
        },
        PrizeRow {
            row: 1,
            limit: 99,
            price: 100,
            item_id: 0x71,
            name: None,
        },
    ];
    let ex = PrizeExchange {
        venue: 0,
        rows,
        cursor: 0,
    };
    // Broke, nothing bought: row 0 is unavailable and NOT latched.
    assert!(!ex.is_available(0, 0, 0, 0));
    assert!(!ex.is_latched(0, 0));
    // Rich, nothing bought: available, still not latched.
    assert!(ex.is_available(0, 10_000, 0, 0));
    assert!(!ex.is_latched(0, 0));
    // Bought: latched, and unavailable however rich the player is.
    let mask = 1u32 << ex.purchase_bit(0);
    assert!(ex.is_latched(0, mask));
    assert!(!ex.is_available(0, 10_000, 0, mask));
    // Its neighbour's bit is untouched.
    assert!(!ex.is_latched(1, mask));
}

/// State `1` compares the backed-up departure-scene id against the two
/// overworld `#define`s and leaves the variant alone for anything else.
#[test]
fn the_departure_scene_picks_the_venue() {
    assert_eq!(venue_for_departure_scene(VENUE_SCENE_VIDNA, 0), 1);
    assert_eq!(venue_for_departure_scene(VENUE_SCENE_BUMA, 1), 0);
    assert_eq!(venue_for_departure_scene(1195, 0), 0);
    assert_eq!(venue_for_departure_scene(1195, 1), 1);
}

/// A session built from the tables and the persistent words hands the
/// same words back, and its catch HUD is dark at the shore.
#[test]
fn a_session_round_trips_its_persistent_words() {
    let tables = FishingTables {
        species: (0..10).map(|i| species(i, 1000, 400)).collect(),
        spawn: [vec![[0u32; 8]; 8], vec![[0u32; 8]; 8]],
        cadence: templates(),
    };
    let persist = FishingPersist {
        lure: 2,
        rod: 1,
        casts: 77,
        record: FishingRecord {
            points: 50,
            best_points: 9,
            best_fish: 3,
        },
        purchased_mask: 0x101,
    };
    let s = PondSession::from_tables(&tables, 1, persist, 7);
    assert_eq!(s.venue, 1);
    assert_eq!(s.persist(), persist);
    assert!(!s.catch_hud().visible, "no catch HUD before a cast");
    let (line, hint) = s.status_rows("S", "Z", "X");
    assert!(line.contains("S = cast"), "{line}");
    assert!(hint.contains('Z') && hint.contains('X'), "{hint}");
}

#[test]
fn reel_decoder_matches_retail_three_way_branch() {
    // Cross (0x40) -> reel A.
    assert_eq!(ReelInput::from_pad_mask(0x40), ReelInput::ReelA);
    // Square (0x80) without Cross -> reel B.
    assert_eq!(ReelInput::from_pad_mask(0x80), ReelInput::ReelB);
    // Both reel buttons held: Cross takes priority (not a blend).
    assert_eq!(ReelInput::from_pad_mask(0xC0), ReelInput::ReelA);
    // Neither reel button -> idle, even with other buttons down.
    assert_eq!(ReelInput::from_pad_mask(0), ReelInput::Idle);
    assert_eq!(ReelInput::from_pad_mask(0x20), ReelInput::Idle); // Circle = cast
    assert_eq!(ReelInput::from_pad_mask(0x100), ReelInput::Idle);
    // Exhaustive cross-check of the low byte against the retail formula
    // `(m & 0x40) ? 1 : ((m >> 6) & 2)`.
    for m in 0u32..0x1_0000 {
        let want = if m & 0x40 != 0 {
            ReelInput::ReelA
        } else if (m >> 6) & 2 != 0 {
            ReelInput::ReelB
        } else {
            ReelInput::Idle
        };
        assert_eq!(ReelInput::from_pad_mask(m), want, "mask {m:#x}");
    }
}

#[test]
fn rod_gate_rejects_an_empty_tacklebox() {
    let mut idx = 0;
    assert!(!select_owned_rod(&mut idx, |_| 0));
    assert_eq!(idx, 0, "index untouched when nothing is owned");
}

#[test]
fn rod_gate_repoints_the_index_at_the_next_owned_lure() {
    // Only the third lure (0x9f) is held; a selection sitting on the first
    // must walk forward to it.
    let mut idx = 0;
    assert!(select_owned_rod(&mut idx, |id| i32::from(id == 0x9f)));
    assert_eq!(idx, 2);
    // Already on an owned kind: no movement.
    let mut idx = 2;
    assert!(select_owned_rod(&mut idx, |id| i32::from(id == 0x9f)));
    assert_eq!(idx, 2);
}

#[test]
fn rod_gate_wraps_past_the_last_kind() {
    // Only the first lure is held, selection parked on the last -> wraps.
    let mut idx = 2;
    assert!(select_owned_rod(&mut idx, |id| i32::from(id == 0x9d)));
    assert_eq!(idx, 0);
}

#[test]
fn cast_power_oscillates_within_bounds_and_locks() {
    let mut c = CastPower::new();
    assert_eq!(c.value(), CAST_POWER_SEED);
    // Sweep up to the ceiling and confirm it bounces back down.
    for _ in 0..200 {
        c.advance(0x40);
    }
    assert!(c.value() >= CAST_POWER_MIN && c.value() <= CAST_POWER_MAX);
    let locked = c.lock();
    assert!(c.is_locked());
    assert_eq!(locked, c.value());
    // Locked meter no longer moves.
    c.advance(0x40);
    assert_eq!(c.value(), locked);
}

#[test]
fn cast_power_bounces_at_ceiling() {
    let mut c = CastPower::new();
    // Big step jumps straight to the ceiling and flips direction.
    c.advance(CAST_POWER_MAX);
    assert_eq!(c.value(), CAST_POWER_MAX);
    c.advance(0x40);
    assert!(
        c.value() < CAST_POWER_MAX,
        "direction flipped downward at ceiling"
    );
}

#[test]
fn tension_rises_on_reel_and_bleeds_when_idle() {
    let mut g = TensionGauge::new(0);
    // Reel button A with a base pull raises tension (rod_stat 0 -> div 0x23).
    g.apply_reel(ReelInput::ReelA, 0x1000, 1);
    let after_reel = g.tension();
    assert!(after_reel > 0, "reeling raised tension");
    // Idle bleeds it off by (0*0x40 + 0x4a) * 1 = 0x4a per frame.
    g.apply_reel(ReelInput::Idle, 0, 1);
    assert_eq!(g.tension(), (after_reel - REEL_RELEASE_ADD).max(0));
}

#[test]
fn tension_clamps_at_bounds() {
    let mut g = TensionGauge::new(0);
    // Huge reel spike pins at the ceiling.
    g.apply_reel(ReelInput::ReelA, i32::MAX / 2, 1);
    assert_eq!(g.tension(), TENSION_MAX);
    assert!(g.at_max());
    // Idle can't drive below zero.
    for _ in 0..1000 {
        g.apply_reel(ReelInput::Idle, 0, 1);
    }
    assert_eq!(g.tension(), TENSION_MIN);
}

#[test]
fn rod_stat_softens_the_reel_spike() {
    let mut weak = TensionGauge::new(0);
    let mut strong = TensionGauge::new(10);
    weak.apply_reel(ReelInput::ReelA, 0x1000, 1);
    strong.apply_reel(ReelInput::ReelA, 0x1000, 1);
    assert!(
        strong.tension() < weak.tension(),
        "a higher rod stat divides the tension spike down"
    );
}

#[test]
fn record_credit_caps_and_tracks_best() {
    let mut r = FishingRecord::default();
    r.credit(3, 100);
    assert_eq!(r.points, 100);
    assert_eq!((r.best_points, r.best_fish), (100, 3));
    // A smaller catch adds points but doesn't beat the best.
    r.credit(1, 40);
    assert_eq!(r.points, 140);
    assert_eq!((r.best_points, r.best_fish), (100, 3));
    // A bigger catch takes the best.
    r.credit(7, 250);
    assert_eq!((r.best_points, r.best_fish), (250, 7));
    // Points cap at 999999.
    r.credit(0, FISH_POINTS_CAP);
    assert_eq!(r.points, FISH_POINTS_CAP);
}

fn exchange() -> PrizeExchange {
    // Shaped like a venue page: a one-time top prize + repeatables.
    let rows = [
        (1u32, 20_000u32, 0x6Fu32),
        (1, 6_500, 0xE5),
        (99, 200, 0x98),
    ];
    let rows: Vec<_> = rows
        .iter()
        .enumerate()
        .map(
            |(row, &(limit, price, item_id))| legaia_asset::fishing_exchange::ExchangeRow {
                row,
                limit,
                price,
                item_id,
            },
        )
        .collect();
    PrizeExchange::from_asset(1, &rows, None)
}

#[test]
fn exchange_row0_hidden_until_strictly_affordable() {
    let ex = exchange();
    assert_eq!(ex.first_visible(19_999), 1);
    assert_eq!(ex.first_visible(20_000), 1); // strict less-than
    assert_eq!(ex.first_visible(20_001), 0);
}

#[test]
fn exchange_availability_gates() {
    let ex = exchange();
    // Affordable + unowned + unlatched = available.
    assert!(ex.is_available(1, 6_500, 0, 0));
    // Unaffordable.
    assert!(!ex.is_available(1, 6_499, 0, 0));
    // Inventory pinned at the 99 cap.
    assert!(!ex.is_available(2, 1_000, 99, 0));
    // One-time bit latched (venue 1 -> bits 8..).
    let latched = 1 << ex.purchase_bit(1);
    assert!(!ex.is_available(1, 6_500, 0, latched));
    assert_eq!(ex.purchase_bit(1), 9);
}

#[test]
fn exchange_max_qty_and_buy() {
    let ex = exchange();
    // Repeatable row: min(points/price, limit - owned).
    assert_eq!(ex.max_qty(2, 1_000, 0, 0), 5);
    assert_eq!(ex.max_qty(2, 1_000_000, 90, 0), 9);
    // One-time row not yet latched treats owned as 0.
    assert_eq!(ex.max_qty(1, 6_500, 1, 0), 1);
    let p = ex.buy(2, 3, 1_000, 0, 0).expect("buys");
    assert_eq!(
        (p.item_id, p.qty, p.cost, p.latched_bit),
        (0x98, 3, 600, None)
    );
    // One-time buy latches its venue-offset bit.
    let p = ex.buy(1, 1, 6_500, 0, 0).expect("buys");
    assert_eq!(p.latched_bit, Some(9));
    // Over-quantity and unavailable rows refuse.
    assert!(ex.buy(2, 6, 1_000, 0, 0).is_none());
    assert!(ex.buy(1, 1, 6_500, 0, 1 << 9).is_none());
}

// -- help_panel_layout (overlay_fishing 0x801D72A0) ----------------

#[test]
fn help_panel_page0_has_14_lines_at_13px_pitch() {
    let l = help_panel_layout(0x20, 0x18, false);
    assert_eq!(l.lines.len(), 14);
    assert_eq!(
        l.lines[0],
        HelpPanelLine {
            string_index: 0,
            x: 0x20,
            y: 0x18
        }
    );
    assert_eq!(l.lines[13].y, 0x18 + 13 * 13);
    assert_eq!(l.footer, (0xE0, 0xCA));
    assert_eq!(l.frame, (0x20, 0x18, 0x119, 0xC3));
}

#[test]
fn help_panel_page1_has_15_lines() {
    let l = help_panel_layout(0, 0, true);
    assert_eq!(l.lines.len(), 15);
    assert_eq!(l.lines[14].y, 13 * 14);
}

// -- FishingMenu (overlay_fishing 0x801D0474) ----------------------

#[test]
fn fishing_menu_cursor_wraps_by_snapping() {
    let mut m = FishingMenu::default();
    // Up from row 0: cursor goes -1, snap to 4.
    let t = m.tick(0x1000, true);
    assert_eq!(m.cursor, 4);
    assert_eq!(t.sfx, Some(0x21));
    // Down from row 4: cursor goes 5, snap to 0.
    m.tick(0x4000, true);
    assert_eq!(m.cursor, 0);
    assert_eq!(m.cursor_pos(), (0x5B, 0x58));
}

#[test]
fn fishing_menu_confirm_maps_rows_to_states() {
    for (row, want) in FISHING_MENU_ROW_STATES.iter().enumerate() {
        let mut m = FishingMenu { cursor: row as i32 };
        let t = m.tick(0x40, true);
        assert_eq!(t.next_state, Some(*want), "row {row}");
        assert_eq!(t.sfx, Some(0x20));
        assert_eq!(t.snapshot_points, row == 2 || row == 3, "row {row}");
        assert_eq!(t.leave_venue, row == 4, "row {row}");
    }
}

#[test]
fn fishing_menu_cancel_and_non_interactive() {
    let mut m = FishingMenu { cursor: 2 };
    let t = m.tick(0x20, true);
    assert_eq!(t.next_state, Some(0x0A));
    assert_eq!(t.sfx, Some(0x37));
    // Non-interactive: pad ignored entirely.
    let mut m = FishingMenu { cursor: 2 };
    let t = m.tick(0xFFFF, false);
    assert_eq!(t.next_state, None);
    assert_eq!(t.sfx, None);
    assert_eq!(m.cursor, 2);
}

// Inventory helper for the rod/lure select tests: `owned` lists the item
// ids the player holds (count 1 each).
fn inv(owned: &[u32]) -> impl FnMut(u32) -> i32 + '_ {
    move |id| owned.contains(&id) as i32
}

#[test]
fn rod_lure_select_equips_owned_lure() {
    // Cursor on lure row 1; the paired lure item 0x9e is owned. Accept
    // (Cross 0x40) equips lure index 1 with the confirm SFX.
    let mut s = RodLureSelect { cursor: 1 };
    let t = s.tick(0x40, 0, true, inv(&[0x9e, 0xa0]));
    assert_eq!(t.equip_lure, Some(1));
    assert_eq!(t.equip_rod, None);
    assert_eq!(t.sfx, Some(0x20));
}

#[test]
fn rod_lure_select_refuses_unowned_lure() {
    // Lure row 2 whose item 0x9f is not owned: accept refuses (SFX 0x22),
    // nothing equipped.
    let mut s = RodLureSelect { cursor: 2 };
    let t = s.tick(0x40, 0, true, inv(&[0x9d, 0xa0]));
    assert_eq!(t.equip_lure, None);
    assert_eq!(t.sfx, Some(0x22));
}

#[test]
fn rod_lure_select_walks_owned_rods() {
    // Player owns rod slots 0 and 2 (item 0xa0, 0xa2), so two rod rows show
    // at cursor 3 and 4. Rod row 4 (cursor-3 = 1) is the *second* owned rod
    // = slot 2, skipping the unowned slot 1.
    let mut s = RodLureSelect { cursor: 4 };
    let t = s.tick(0x40, 0, true, inv(&[0xa0, 0xa2]));
    assert_eq!(t.equip_rod, Some(2));
    assert_eq!(t.equip_lure, None);
    assert_eq!(t.sfx, Some(0x20));
    // Rod row 3 (cursor-3 = 0) is the first owned rod = slot 0.
    let mut s = RodLureSelect { cursor: 3 };
    let t = s.tick(0x40, 0, true, inv(&[0xa0, 0xa2]));
    assert_eq!(t.equip_rod, Some(0));
}

#[test]
fn rod_lure_select_cursor_wraps_against_owned_rods() {
    // Two owned rods -> max cursor = owned_rods + 2 = 4. Moving down past it
    // snaps to 0; moving up below 0 snaps to 4.
    let mut s = RodLureSelect { cursor: 4 };
    let t = s.tick(0, 0x4000, true, inv(&[0xa0, 0xa2])); // down
    assert_eq!(t.sfx, Some(0x21));
    assert_eq!(s.cursor, 0);
    let mut s = RodLureSelect { cursor: 0 };
    s.tick(0, 0x1000, true, inv(&[0xa0, 0xa2])); // up
    assert_eq!(s.cursor, 4);
}

#[test]
fn rod_lure_select_cancel_and_non_interactive() {
    // Cancel (Circle 0x20) leaves with the cancel SFX.
    let mut s = RodLureSelect { cursor: 1 };
    let t = s.tick(0x20, 0, true, inv(&[0x9d]));
    assert!(t.leave);
    assert_eq!(t.sfx, Some(0x37));
    // Non-interactive: no pad acted, but the snap-wrap still runs. An
    // out-of-range cursor with one owned rod (max 3) snaps to 0.
    let mut s = RodLureSelect { cursor: 9 };
    let t = s.tick(0xFFFF, 0xFFFF, false, inv(&[0xa0]));
    assert_eq!(t.sfx, None);
    assert!(!t.leave);
    assert_eq!(s.cursor, 0);
}

// --- retail species selection ------------------------------------------

use legaia_asset::fishing_species::{CadenceStep, CadenceTemplate};

#[test]
fn band_roll_matches_the_cutoff_table() {
    assert_eq!(band_roll(0), 3);
    assert_eq!(band_roll(0xc00), 3);
    assert_eq!(band_roll(0xc01), 2);
    assert_eq!(band_roll(0xe70), 2);
    assert_eq!(band_roll(0xe71), 1);
    assert_eq!(band_roll(0xf38), 1);
    assert_eq!(band_roll(0xf39), 0);
    assert_eq!(band_roll(0xfff), 0);
}

/// What the strike ladder is *for*: a shallow cast cannot bite. Retail
/// gets that from the far band replacing the credit base with `-100`
/// against a `2000` modulus, not from a length test.
#[test]
fn a_shallow_cast_can_never_strike() {
    let mut b = BandCheck::default();
    let mut rng = BiosRand::new(0xC0FFEE);
    for readout in [0, 50, 99, 100, 150, 199] {
        for _ in 0..5000 {
            assert!(
                !b.tick(&mut rng, None, readout + 300, readout, 3, 0, true, 1),
                "readout {readout} struck"
            );
        }
    }
}

/// And the other half: a cadence match arms the bite. The credit base
/// jumps from `countdown + 2` to the `0x40` hold length against the same
/// `1000` modulus, which is the ~30x swing the doc describes.
#[test]
fn a_cadence_match_arms_the_bite() {
    let readout = 1000;
    let count = |cadence: Option<usize>| {
        let mut b = BandCheck::default();
        let mut rng = BiosRand::new(7);
        let mut n = 0;
        for _ in 0..4000 {
            // Re-arm every frame so the hold does not decay away.
            b.countdown = 0;
            if b.tick(&mut rng, cadence, readout + 300, readout, 0, 0, true, 1) {
                n += 1;
            }
        }
        n
    };
    let bare = count(None);
    let matched = count(Some(2));
    assert!(
        matched > bare * 10,
        "cadence {matched} vs bare {bare} - the credit override is inert"
    );
}

#[test]
fn band4_gate_conditions() {
    // Buma: > 50 casts, even, Normal Lure, third rod, band 0, then 1/16.
    let mut hits = 0;
    for seed in 0..64u32 {
        let mut rng = BiosRand::new(seed);
        if band4_gate(0, 1, 2, 0, 52, &mut rng) {
            hits += 1;
        }
    }
    assert!(hits > 0, "1/16 roll never fired over 64 seeds");
    // Any failed precondition short-circuits without advancing the rng.
    let mut rng = BiosRand::new(7);
    let before = rng;
    assert!(!band4_gate(0, 1, 2, 0, 51, &mut rng)); // odd counter
    assert!(!band4_gate(0, 1, 2, 0, 40, &mut rng)); // even but under the 0x32 threshold
    assert!(!band4_gate(0, 0, 2, 0, 52, &mut rng)); // wrong lure
    assert!(!band4_gate(0, 1, 1, 0, 52, &mut rng)); // wrong rod
    assert!(!band4_gate(0, 1, 2, 1, 52, &mut rng)); // wrong band
    assert_eq!(rng, before, "short-circuit must not advance the rng");
    // Vidna: Heavy Lure, no cast-count threshold, 1/4.
    let mut hits = 0;
    for seed in 0..16u32 {
        let mut rng = BiosRand::new(seed);
        if band4_gate(1, 2, 2, 0, 0, &mut rng) {
            hits += 1;
        }
    }
    assert!(hits > 0, "1/4 roll never fired over 16 seeds");
}

fn templates() -> Vec<CadenceTemplate> {
    // The disc's four shapes (doc table; durations as on the USA disc).
    let t = |steps: &[(i32, u8)], window: i32| CadenceTemplate {
        history_window: window,
        steps: steps
            .iter()
            .map(|&(duration, button)| CadenceStep { duration, button })
            .collect(),
    };
    vec![
        t(&[(40, 0), (25, 1), (40, 0), (15, 2)], 0x8c),
        t(&[(15, 2), (25, 1), (0, 0)], 0x8c),
        t(&[(15, 2), (40, 0), (15, 2)], 0x82),
        t(&[(25, 1), (40, 0), (25, 1)], 0x96),
    ]
}

/// Drive the recogniser through `seq` = [(button, frames)] and return the
/// first match.
fn drive(c: &mut ReelCadence, seq: &[(u8, i32)]) -> Option<usize> {
    for &(b, frames) in seq {
        for _ in 0..frames {
            if let Some(t) = c.feed(b, 1) {
                return Some(t);
            }
        }
    }
    None
}

#[test]
fn cadence_recogniser_matches_template_3() {
    // Cross 25, idle 40, Cross 25 - the natural pump rhythm.
    let mut c = ReelCadence::new(templates());
    let got = drive(&mut c, &[(0, 30), (1, 25), (0, 40), (1, 25)]);
    assert_eq!(got, Some(3));
}

#[test]
fn cadence_recogniser_matches_template_0_with_tolerance() {
    // idle 40, Cross 25, idle 40, Square 15 - +-10 slop on each step.
    let mut c = ReelCadence::new(templates());
    let got = drive(&mut c, &[(0, 45), (1, 20), (0, 35), (2, 18)]);
    assert_eq!(got, Some(0));
}

#[test]
fn cadence_recogniser_rejects_out_of_tolerance_holds() {
    let mut c = ReelCadence::new(templates());
    // Cross held far too long between the idles: no template fits.
    let got = drive(&mut c, &[(0, 40), (1, 60), (0, 40)]);
    assert_eq!(got, None);
}

#[test]
fn cadence_match_resets_the_ring() {
    let mut c = ReelCadence::new(templates());
    assert_eq!(
        drive(&mut c, &[(0, 30), (1, 25), (0, 40), (1, 25)]),
        Some(3)
    );
    // Immediately after the reset the same tail can't re-match.
    assert_eq!(c.feed(1, 1), None);
}

#[test]
fn band_check_holds_a_matched_band_and_boosts_credit() {
    let mut b = BandCheck::default();
    let mut rng = BiosRand::new(1);
    // A cadence match stores the template id as the band and arms the
    // countdown + splash.
    b.tick(&mut rng, Some(0), 1000, 700, 0, 0, false, 1);
    assert_eq!(b.band, 0);
    assert!(b.splash);
    assert_eq!(b.countdown, BAND_HOLD_FRAMES);
    // While held, unmatched frames keep the band (countdown decays).
    b.tick(&mut rng, None, 1000, 700, 0, 0, false, 1);
    assert_eq!(b.band, 0);
    assert!(!b.splash);
    assert_eq!(b.countdown, BAND_HOLD_FRAMES - 1);
}

#[test]
fn band_check_strike_requires_reel_and_readout() {
    let mut rng = BiosRand::new(2);
    let mut b = BandCheck::default();
    // Readout below the floor: no strike regardless of credit.
    for _ in 0..200 {
        assert!(!b.tick(&mut rng, Some(0), 1000, 150, 5, 0, true, 1));
    }
    // Reel not held: no strike.
    let mut b = BandCheck::default();
    for _ in 0..200 {
        assert!(!b.tick(&mut rng, Some(0), 1000, 700, 5, 0, false, 1));
    }
}

#[test]
fn spawn_lookup_uses_lure_row_and_band_column() {
    let mut table = vec![[0u32; 8]; 8];
    table[1] = [5, 5, 3, 5, 9, 0, 0, 0];
    assert_eq!(spawn_species(&table, 1, 0), Some(5));
    assert_eq!(spawn_species(&table, 1, 4), Some(9));
    assert_eq!(spawn_species(&table, 9, 0), None);
    table[2][0] = 99; // out-of-table species id
    assert_eq!(spawn_species(&table, 2, 0), None);
}

fn pond() -> PondSession {
    let species: Vec<FishingSpecies> = (0..10)
        .map(|i| species(i, 1000 * (i as i32 + 1), 400))
        .collect();
    let mut spawn = vec![[0u32; 8]; 8];
    spawn[0] = [3, 3, 5, 5, 0, 0, 0, 0];
    spawn[1] = [5, 5, 3, 5, 9, 0, 0, 0];
    spawn[2] = [7, 1, 4, 2, 0, 0, 0, 0];
    PondSession::new(
        species,
        spawn,
        templates(),
        0,
        1,
        2,
        60,
        FishingRecord::default(),
        0,
        0xC0FFEE,
    )
}

#[test]
fn pond_session_full_loop_hooks_fights_and_lands() {
    let mut p = pond();
    assert_eq!(p.phase(), PondPhase::Idle);
    // Cast press -> wind-up -> power.
    p.tick(
        PondInput {
            cast_edge: true,
            ..Default::default()
        },
        1,
        0x80,
    );
    for _ in 0..WINDUP_FRAMES {
        p.tick(PondInput::default(), 1, 0x80);
    }
    assert_eq!(p.phase(), PondPhase::Power);
    // Sweep to a deep cast, then lock.
    for _ in 0..24 {
        p.tick(PondInput::default(), 1, 0x80);
    }
    p.tick(
        PondInput {
            cast_edge: true,
            ..Default::default()
        },
        1,
        0x80,
    );
    assert_eq!(p.phase(), PondPhase::Flight);
    let casts_before = p.casts;
    for _ in 0..FLIGHT_FRAMES {
        p.tick(PondInput::default(), 1, 0x80);
    }
    assert_eq!(p.phase(), PondPhase::Waiting);
    assert_eq!(p.casts, casts_before + 1, "landing increments the counter");
    assert!(p.line_record() > BAND_CHECK_MIN_RECORD);

    // Hold reel A until a strike hooks a fish (bounded). A bare held reel
    // is the *slow* hook path: the strike roll is `rand % 1000 < 2` for a
    // deep cast, so this needs several casts' worth of frames. The fast
    // path is a reel cadence, which replaces the credit base with the
    // `0x40` hold length - see `a_cadence_match_arms_the_bite` below.
    let mut hooked = false;
    for _ in 0..8000 {
        p.tick(
            PondInput {
                reel_mask: 0x40,
                ..Default::default()
            },
            1,
            0x80,
        );
        if p.phase() == PondPhase::Hooked {
            hooked = true;
            break;
        }
        if p.phase() == PondPhase::Idle {
            // Fully reeled in without a strike: cast again.
            p.tick(
                PondInput {
                    cast_edge: true,
                    ..Default::default()
                },
                1,
                0x80,
            );
            for _ in 0..WINDUP_FRAMES + 40 {
                p.tick(PondInput::default(), 1, 0x80);
            }
            p.tick(
                PondInput {
                    cast_edge: true,
                    ..Default::default()
                },
                1,
                0x80,
            );
            for _ in 0..FLIGHT_FRAMES {
                p.tick(PondInput::default(), 1, 0x80);
            }
        }
    }
    assert!(hooked, "no strike over 8000 held-reel frames");
    let events = p.take_events();
    assert!(
        events.iter().any(|e| matches!(e, PondEvent::Hooked(_))),
        "{events:?}"
    );
    let id = p.hooked().expect("species").index;
    // The hooked species came from the lure row of the spawn table.
    assert!([5usize, 3, 9].contains(&id), "id {id} not in lure-1 row");

    // Fight: alternate reeling and resting so tension never pins, until
    // the fish lands.
    let mut landed = false;
    for i in 0..20000 {
        let reel = if p.tension() < 0x800 { 0x40 } else { 0 };
        p.tick(
            PondInput {
                reel_mask: reel,
                ..Default::default()
            },
            1,
            0x80,
        );
        match p.phase() {
            PondPhase::Landed => {
                landed = true;
                break;
            }
            PondPhase::Snapped => panic!("line snapped under the safe reel policy at {i}"),
            _ => {}
        }
    }
    assert!(landed, "fight never resolved");
    assert!(p.record.points > 0);
    assert!(p.last_award() > 0);
    let events = p.take_events();
    assert!(events.iter().any(|e| matches!(e, PondEvent::Landed(_))));
    // The result plate: on the landing frame, counter 0, the award and the
    // species the fight hooked; it climbs `4` per frame step and holds at
    // `0x1000` (`FUN_801D5298`).
    let r = p.catch_result().expect("a landed catch shows its plate");
    assert_eq!((r.ramp, r.points, r.species), (0, p.last_award(), id));
    p.tick(PondInput::default(), 3, 0x80);
    assert_eq!(p.catch_result().unwrap().ramp, 12);
    for _ in 0..0x500 {
        p.tick(PondInput::default(), 1, 0x80);
    }
    assert_eq!(p.catch_result().unwrap().ramp, 0x1000);
    // Recast returns to the shore.
    p.tick(
        PondInput {
            cast_edge: true,
            ..Default::default()
        },
        1,
        0x80,
    );
    assert_eq!(p.phase(), PondPhase::Idle);
}

#[test]
fn pond_session_is_deterministic_for_a_seed() {
    let run = || {
        let mut p = pond();
        let mut log = Vec::new();
        for i in 0..4000u32 {
            let input = PondInput {
                reel_mask: if i % 90 < 45 { 0x40 } else { 0 },
                cast_edge: i % 200 == 0,
                ..Default::default()
            };
            p.tick(input, 1, 0x80);
            log.push((p.phase() as u8 as u32, p.tension(), p.line_record()));
        }
        (log, p.record.points, p.casts)
    };
    assert_eq!(run(), run());
}

#[test]
fn pond_snaps_when_tension_pins() {
    let mut p = pond();
    // Cast deep.
    p.tick(
        PondInput {
            cast_edge: true,
            ..Default::default()
        },
        1,
        0x80,
    );
    for _ in 0..WINDUP_FRAMES + 30 {
        p.tick(PondInput::default(), 1, 0x80);
    }
    p.tick(
        PondInput {
            cast_edge: true,
            ..Default::default()
        },
        1,
        0x80,
    );
    for _ in 0..FLIGHT_FRAMES {
        p.tick(PondInput::default(), 1, 0x80);
    }
    // Hold reel forever: the session must terminate (a weak fish lands
    // before tension pins; a strong pull snaps the line; an empty reel-in
    // returns to Idle) - it must never wedge in the fight.
    let mut resolved = None;
    for _ in 0..30000 {
        p.tick(
            PondInput {
                reel_mask: 0x40,
                ..Default::default()
            },
            1,
            0x80,
        );
        if let ph @ (PondPhase::Snapped | PondPhase::Landed | PondPhase::Idle) = p.phase() {
            resolved = Some(ph);
            break;
        }
    }
    assert!(resolved.is_some(), "held-reel session never resolved");
    // And the snap edge itself is exercised directly by the gauge: a
    // strong pull with the reel held pins the ceiling.
    let mut g = TensionGauge::new(2);
    for _ in 0..0x1000 {
        g.apply_reel(ReelInput::ReelA, 4000, 1);
    }
    assert!(g.at_max());
}

#[test]
fn entry_rod_scan_keeps_a_held_rod_and_falls_back_to_zero() {
    // The saved rod is held: the scan keeps it and probes nothing else.
    let mut probes = Vec::new();
    assert_eq!(
        entry_rod_index(2, |id| {
            probes.push(id);
            i32::from(id == rod_item_id(2))
        }),
        2
    );
    assert_eq!(probes, vec![rod_item_id(2)]);

    // Saved rod not held: step forward, wrapping past the last kind.
    assert_eq!(entry_rod_index(2, |id| i32::from(id == rod_item_id(0))), 0);
    assert_eq!(entry_rod_index(1, |id| i32::from(id == rod_item_id(2))), 2);

    // Nothing held at all lands on rod 0, and stops after the probe cap
    // rather than spinning.
    let mut n = 0;
    assert_eq!(
        entry_rod_index(1, |_| {
            n += 1;
            0
        }),
        0
    );
    assert_eq!(n, ENTRY_ROD_PROBES);

    // A stale out-of-range index still terminates on the same cap.
    assert_eq!(entry_rod_index(9, |_| 0), 0);
}

#[test]
fn the_rod_family_is_not_the_lure_family() {
    // Two persistent indices, two item bands - reading one gate as the
    // other ties the reel divisors to the lure the player happens to hold.
    for k in 0..ROD_KINDS {
        assert_eq!(rod_item_id(k), 0xa0 + k);
        assert_eq!(lure_item_id(k), 0x9d + k);
        assert_ne!(rod_item_id(k), lure_item_id(k));
    }
}

#[test]
fn the_floor_lut_is_one_tier_per_nibble_at_a_fixed_step() {
    assert_eq!(FISHING_FLOOR_LUT[0], 0);
    for n in 1..FISHING_FLOOR_LUT.len() {
        assert_eq!(FISHING_FLOOR_LUT[n] - FISHING_FLOOR_LUT[n - 1], 0x20);
    }
    // The LUT is indexed by a map cell's low nibble, so it must cover
    // every value a nibble can take.
    assert_eq!(FISHING_FLOOR_LUT.len(), 16);
}
