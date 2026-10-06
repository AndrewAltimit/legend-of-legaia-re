use super::*;
use legaia_asset::dance_chart::{DANCE_BONUS_LANES, DANCE_SCHEDULE_SLOTS, DANCE_SKILL_ROWS};

/// A 3-row chart with a known step layout for judging.
fn chart() -> DanceChart {
    let mut rows = Vec::new();
    for lane in 0..3u8 {
        let mut row = [0u8; BEATS_PER_ROW];
        // Beat 0 of every lane wants symbol 1 (DanceDir::A).
        row[0] = 1;
        // Beat 1 wants symbol 2 in lane 0, symbol 1 elsewhere.
        row[1] = if lane == 0 { 2 } else { 1 };
        rows.push(row);
    }
    DanceChart { rows }
}

/// Retail-shaped scoring tables: `k, 2k, 3k` bonus rows and a triangle
/// schedule that fires the CPU dancers' first groovy move early.
fn tables() -> DanceScoreTables {
    let mut bonus = Vec::new();
    let mut schedule = Vec::new();
    for k in 0..DANCE_SKILL_ROWS {
        let base = (17 - 3 * k) as i32;
        let mut row = [0i32; DANCE_BONUS_LANES];
        for (lane, cell) in row.iter_mut().enumerate().take(3) {
            *cell = base * (lane as i32 + 1);
        }
        bonus.push(row);
        let mut s = [1000i32; DANCE_SCHEDULE_SLOTS];
        if k > 0 {
            s[0] = 1; // spend the first triangle after one banked combo slot
            s[1] = 2;
        }
        schedule.push(s);
    }
    DanceScoreTables { bonus, schedule }
}

fn game() -> DanceGame {
    DanceGame::with_tables(chart(), tables(), &QUALIFIER_KINDS, false)
}

#[test]
fn constants_match_the_re() {
    assert_eq!(BEAT_PERIOD, 0x119);
    assert_eq!(BEAT_WINDOW, 0xd2);
    assert_eq!(BEAT_PHASE_WRAP, 0x2320);
    // The phase wrap is exactly one chart row of beats.
    assert_eq!(BEAT_PHASE_WRAP, BEAT_PERIOD * BEATS_PER_ROW as u32);
    assert_eq!((MULT_ORDINARY, MULT_COMBO, MULT_FINALE), (3, 25, 34));
    assert_eq!((SCORE_MAX, GAUGE_MAX, GAUGE_STEP), (999, 2999, 1000));
    assert_eq!((SEQUENCE_GAUGE_STEP, TRIANGLE_STOCK), (250, 3));
    assert_eq!(TRIANGLE_FEEDBACK_WINDOW, 0x3c);
    assert_eq!(WIN_THRESHOLD_SOLO, 300);
}

#[test]
fn symbol_pad_bit_map() {
    assert_eq!(DanceDir::A.pad_bit(), 0x80);
    assert_eq!(DanceDir::B.pad_bit(), 0x20);
    assert_eq!(DanceDir::A.symbol(), 1);
    assert_eq!(DanceDir::C.pad_bit(), 0x10);
    assert!(DanceDir::C.is_triangle());
    assert!(!DanceDir::A.is_triangle());
}

#[test]
fn accuracy_weight_peaks_on_beat_and_decays_to_edge() {
    let mut g = game();
    assert_eq!(g.accuracy_weight(), ACCURACY_MAX);
    g.phase = BEAT_WINDOW;
    assert_eq!(g.accuracy_weight(), 0);
    assert!(!g.in_dead_zone());
    g.phase = BEAT_WINDOW + 1;
    assert!(g.in_dead_zone());
    assert_eq!(g.accuracy_weight(), 0);
}

#[test]
fn beat_clock_wraps_and_ends_song() {
    let mut g = game();
    g.advance(1);
    assert_eq!(g.phase, PHASE_PER_DELTA);
    assert_eq!(g.beat_index(), 0);
    for _ in 0..2000 {
        g.advance(1);
    }
    assert!(g.song_over());
    assert!(g.phase < BEAT_PHASE_WRAP);
}

#[test]
fn dead_zone_press_misses_but_never_lowers_the_gauge() {
    let mut g = game();
    g.dancers[0].gauge = 1500;
    g.phase = BEAT_WINDOW + 5; // dead zone
    assert_eq!(g.press(DanceDir::A), DanceEvent::Miss);
    // Retail's award routine has no gauge-drop path: a miss only bumps the
    // miss counter (and the sad-face pose).
    assert_eq!(g.gauge(), 1500);
    assert_eq!(g.dancers[0].misses, 1);
}

#[test]
fn a_closed_chain_scores_the_kinds_bonus_a_bare_hit_does_not() {
    // Lane 0: a single matched note closes the chain (cursor + 1 == 1).
    let mut g = game();
    assert_eq!(g.judged_symbol(), Some(1));
    // Kind 0's lane-0 bonus is 17; the human's award is accuracy-weighted
    // (`base/2 + (base * w) >> 13`), so a dead-on press banks 8 + 8 = 16.
    assert!(matches!(
        g.press(DanceDir::A),
        DanceEvent::Sequence { points, .. } if points == 16
    ));
    assert_eq!(g.score(), 16);
    assert_eq!(g.gauge(), SEQUENCE_GAUGE_STEP);

    // Lane 1 needs two matched notes: the first is a bare Hit worth nothing.
    let mut g = game();
    g.dancers[0].gauge = 1000; // lane 1
    assert!(matches!(g.press(DanceDir::A), DanceEvent::Hit { .. }));
    assert_eq!(g.score(), 0);
    // Advance to beat 1 (lane 1 wants symbol 1 again) and close the chain.
    g.phase = BEAT_PERIOD;
    g.dancers[0].latch = 0;
    g.dancers[0].latch_timer = 0;
    assert!(matches!(
        g.press(DanceDir::A),
        DanceEvent::Sequence { points, .. } if points == 34 // 17 * lane(1)+1
    ));
    assert_eq!(g.score(), 34);
}

#[test]
fn wrong_direction_misses_and_the_press_is_latched() {
    let mut g = game();
    assert_eq!(g.press(DanceDir::B), DanceEvent::Miss);
    assert_eq!(g.score(), 0);
    // A judged press latches the dancer: an immediate re-press is ignored
    // (retail is playing the miss-reaction clip).
    assert_eq!(g.press(DanceDir::A), DanceEvent::Ignored);
}

// ---------------------------------------------------------- triangles

#[test]
fn triangle_stock_is_three_and_runs_out() {
    let mut g = game();
    assert_eq!(g.triangles(), 3);
    for n in 0..3 {
        // Free the dancer from the previous spend's spin + latch.
        g.dancers[0].spin_turns = 0;
        g.dancers[0].latch = 0;
        g.dancers[0].last_beat = None;
        assert!(matches!(
            g.press(DanceDir::C),
            DanceEvent::Groovy { left, .. } if left == 2 - n
        ));
    }
    assert_eq!(g.triangles(), 0);
    g.dancers[0].spin_turns = 0;
    g.dancers[0].latch = 0;
    g.dancers[0].last_beat = None;
    assert_eq!(g.press(DanceDir::C), DanceEvent::NoCharge);
    assert_eq!(g.triangles(), 0);
}

#[test]
fn triangle_on_the_combo_slot_multiplies_and_promotes_the_lane() {
    // Off the combo slot: the wildcard is worth only (lane + 1) * 3.
    let mut g = game();
    assert!(!g.on_combo_slot());
    assert!(matches!(
        g.press(DanceDir::C),
        DanceEvent::Groovy { landed: false, points, .. } if points == MULT_ORDINARY
    ));
    assert_eq!(g.gauge(), 0, "an off-beat spend does not fill the gauge");

    // On the 4-beat combo slot: (lane + 1) * 25, plus a full gauge step.
    let mut g = game();
    g.phase = 3 * BEAT_PERIOD;
    assert!(g.on_combo_slot());
    assert!(matches!(
        g.press(DanceDir::C),
        DanceEvent::Groovy { landed: true, points, .. } if points == MULT_COMBO
    ));
    assert_eq!(g.score(), MULT_COMBO);
    assert_eq!(g.gauge(), GAUGE_STEP);
    assert_eq!(g.lane(), 1, "the landed triangle promoted the lane");

    // In the finale (song over, countdown / wipe running - states 0xB /
    // 0xC) the landed triangle pays (lane + 1) * 0x22 and raises the
    // finale flag; a chart-only run has no finale and keeps 0x19.
    let mut g = game();
    g.finish_programs = vec![(0x17, vec![0xFFFF, 0, 0x0008])];
    g.song_timer = g.song_len;
    assert!(g.in_finale());
    g.phase = 3 * BEAT_PERIOD;
    assert!(matches!(
        g.press(DanceDir::C),
        DanceEvent::Groovy { landed: true, points, .. } if points == MULT_FINALE
    ));
    assert!(g.finale_landed(0));
    let mut g = game();
    g.song_timer = g.song_len;
    assert!(!g.in_finale(), "no countdown programs, no finale window");

    // Spent at the end of a long combo (lane 2) it is worth 3 x 25 = 75.
    let mut g = game();
    g.dancers[0].gauge = 2000; // lane 2 - the combo the player built
    g.phase = 3 * BEAT_PERIOD;
    assert!(matches!(
        g.press(DanceDir::C),
        DanceEvent::Groovy { landed: true, points, .. } if points == 3 * MULT_COMBO
    ));
}

#[test]
fn a_spent_triangle_locks_input_out_for_the_groovy_move() {
    // Spent at the end of a long combo (lane 2): 3 turns at 0xC0 units per
    // frame = 64 frames - the retail groovy-move window.
    let mut g = game();
    g.dancers[0].gauge = 2000;
    g.phase = 3 * BEAT_PERIOD;
    let DanceEvent::Groovy { lock, .. } = g.press(DanceDir::C) else {
        panic!("triangle spent");
    };
    assert_eq!(
        lock,
        3 * SPIN_TURN_UNITS / (SPIN_RATE_BASE + 2 * SPIN_RATE_PER_LANE)
    );
    assert_eq!(lock, 64);
    assert!(g.in_groovy_move());
    assert_eq!(g.groovy_lock(), lock);
    // Every press inside the window is ignored - no score, no miss.
    let before = g.score();
    for f in 0..lock {
        assert_eq!(
            g.press(DanceDir::A),
            DanceEvent::Ignored,
            "input is disrupted for the whole groovy move (frame {f})"
        );
        assert_eq!(g.score(), before);
        g.advance(1);
    }
    // ...and it ends: the dancer is judged again.
    assert!(!g.in_groovy_move());
    assert_eq!(g.groovy_lock(), 0);
    assert_eq!(g.dancers[0].misses, 0, "ignored presses are not misses");
}

#[test]
fn triangle_arms_the_feedback_window() {
    let mut g = game();
    assert_eq!(g.triangle_feedback(), None);
    g.phase = 3 * BEAT_PERIOD;
    let _ = g.press(DanceDir::C);
    assert_eq!(g.triangle_feedback(), Some(true), "it landed on the slot");
    for _ in 0..TRIANGLE_FEEDBACK_WINDOW {
        g.advance(1);
    }
    assert_eq!(g.triangle_feedback(), None);
}

// ------------------------------------------------------------- rivals

#[test]
fn rival_scores_advance_over_the_song() {
    let mut g = game();
    assert_eq!(g.dancer_count(), 3);
    assert_eq!(
        (g.dancer_kind(0), g.dancer_kind(1), g.dancer_kind(2)),
        (0, 2, 3)
    );
    assert_eq!((g.dancer_score(1), g.dancer_score(2)), (0, 0));
    let mut last = [0u32; 2];
    let mut climbs = 0;
    for _ in 0..1500 {
        g.advance(1);
        let now = [g.dancer_score(1), g.dancer_score(2)];
        if now[0] > last[0] && now[1] > last[1] {
            climbs += 1;
        }
        assert!(now[0] >= last[0] && now[1] >= last[1], "scores never fall");
        last = now;
    }
    assert!(g.dancer_score(1) > 0, "rival 1 scored off the auto-feed");
    assert!(g.dancer_score(2) > 0, "rival 2 scored off the auto-feed");
    assert!(
        climbs > 1,
        "the rival scores advance repeatedly over the song"
    );
    // The human never touched the pad, so the rivals are ahead.
    assert_eq!(g.score(), 0);
    assert!(!g.beating_rivals());
    // A rival's kind picks its bonus row: kind 2 out-scores kind 3.
    assert!(
        g.dancer_score(1) >= g.dancer_score(2),
        "the stronger kind's bonus row scores at least as fast"
    );
}

#[test]
fn rivals_spend_their_triangles_on_the_disc_schedule() {
    let mut g = game();
    // The fixture schedule fires kind 2/3's first triangle after one banked
    // combo slot, so both rivals spend one within the first bars.
    for _ in 0..600 {
        g.advance(1);
    }
    assert!(g.dancer_triangles(1) < TRIANGLE_STOCK);
    assert!(g.dancer_triangles(2) < TRIANGLE_STOCK);
    // Never more than the stock, ever.
    for _ in 0..2000 {
        g.advance(1);
    }
    assert!(g.dancer_triangles(1) <= TRIANGLE_STOCK);
}

#[test]
fn gauge_promotes_lane_and_score_clamps() {
    let mut g = game();
    g.dancers[0].gauge = 1500;
    assert_eq!(g.lane(), 1);
    g.dancers[0].gauge = GAUGE_MAX;
    assert_eq!(g.lane(), 2);
    g.dancers[0].score = SCORE_MAX - 1;
    g.dancers[0].gauge = 0;
    let _ = g.press(DanceDir::A);
    assert_eq!(g.score(), SCORE_MAX);
}

#[test]
fn required_symbol_holds_the_triangle_on_the_fourth_beat() {
    let mut g = game();
    g.phase = 3 * BEAT_PERIOD;
    assert_eq!(g.required_symbol(), Some(3));
    g.phase = 3 * BEAT_PERIOD + BEAT_WINDOW + 1;
    assert_eq!(g.required_symbol(), None);
}

#[test]
fn pass_threshold_and_versus_grade() {
    let mut g = game();
    assert!(!g.passed());
    g.dancers[0].score = WIN_THRESHOLD_SOLO;
    assert!(g.passed());
    g.dancers[1].score = WIN_THRESHOLD_SOLO;
    assert!(g.beating_rivals(), "a tie goes to the human");
    g.dancers[2].score = WIN_THRESHOLD_SOLO + 1;
    assert!(!g.beating_rivals());
}

/// State 1 tests `0x133`, `0x134`, `0x135`, `0x428` in that order and each
/// hit overwrites the mode, so the last flag set wins; none keeps the
/// entry's qualifier.
#[test]
fn state_one_maps_the_story_flags_to_the_floor() {
    let only = |f: u16| move |x: u16| x == f;
    assert_eq!(dance_mode_from_flags(|_| false), DanceMode::Qualifier);
    assert_eq!(dance_mode_from_flags(only(0x133)), DanceMode::HowTo);
    assert_eq!(dance_mode_from_flags(only(0x134)), DanceMode::Qualifier);
    assert_eq!(dance_mode_from_flags(only(0x135)), DanceMode::Finals);
    assert_eq!(dance_mode_from_flags(only(0x428)), DanceMode::FreePlay);
    assert_eq!(
        dance_mode_from_flags(|f| f == 0x133 || f == 0x135),
        DanceMode::Finals
    );
}

/// The results state's grade, per mode (`0x801CFE80..0x801CFF14`): the
/// qualifier reads slot 2, the finals slot 1, a tie keeps the pass flag, the
/// how-to demo clears at `0x12D` and up, free play never clears.
#[test]
fn results_grade_clears_the_pass_flag_per_mode() {
    let mut g = game();
    g.mode = DanceMode::Qualifier;
    g.dancers[0].score = 100;
    g.dancers[1].score = 500;
    g.dancers[2].score = 100;
    assert!(
        !g.results_clear_win_flag(),
        "slot 1 is not the qualifier's rival"
    );
    g.dancers[2].score = 101;
    assert!(g.results_clear_win_flag());
    g.mode = DanceMode::Finals;
    assert!(g.results_clear_win_flag());
    g.dancers[1].score = 100;
    assert!(!g.results_clear_win_flag(), "a tie keeps the flag");
    g.mode = DanceMode::HowTo;
    g.dancers[0].score = 0x12C;
    assert!(!g.results_clear_win_flag());
    g.dancers[0].score = 0x12D;
    assert!(g.results_clear_win_flag());
    g.mode = DanceMode::FreePlay;
    assert!(!g.results_clear_win_flag());
}

#[test]
fn legacy_judge_wrapper_folds_the_events() {
    let mut g = game();
    assert!(matches!(g.judge_press(DanceDir::A), Judge::Sequence { .. }));
    let mut g = game();
    assert_eq!(g.judge_press(DanceDir::B), Judge::Miss);
    let mut g = game();
    g.phase = 3 * BEAT_PERIOD;
    assert!(matches!(g.judge_press(DanceDir::C), Judge::Sequence { .. }));
    // Mid-groovy-move presses fold to Miss but apply no penalty.
    assert_eq!(g.judge_press(DanceDir::A), Judge::Miss);
    assert_eq!(g.dancers[0].misses, 0);
}

#[test]
fn step_mark_spawns_at_the_grid_cell() {
    let s = step_mark_effect_spawn(20, 15, 7);
    assert_eq!((s.x, s.y), (160, 120));
    assert_eq!((s.scale, s.sprite_id), (0x1000, 7));
}

#[test]
fn score_glyph_u_steps_eight_texels_per_thousand() {
    assert_eq!(score_thousands_glyph_u(0), -0x30);
    assert_eq!(score_thousands_glyph_u(999), -0x30);
    assert_eq!(score_thousands_glyph_u(1000), -0x28);
    assert_eq!(score_thousands_glyph_u(6000), 0);
    assert_eq!(score_thousands_glyph_u(9999), 0x18);
}

// ---------------------------------------------------------- HUD kernels

#[test]
fn number_split_suppresses_leading_zeros_and_zero_draws_nothing() {
    // Right-aligned, leading blanks are None; the drawn digits are the value.
    assert_eq!(
        dance_number_digits(1234),
        [None, None, None, None, Some(1), Some(2), Some(3), Some(4)]
    );
    assert_eq!(
        dance_number_digits(50),
        [None, None, None, None, None, None, Some(5), Some(0)]
    );
    // The units slot is seeded with `0` before the fill (`0x801D3358`), so a
    // zero value draws one `0`.
    assert_eq!(
        dance_number_digits(0),
        [None, None, None, None, None, None, None, Some(0)]
    );
    // 10^7 - 1 fills the low seven slots (the eighth place is still zero).
    assert_eq!(
        dance_number_digits(9_999_999),
        [
            None,
            Some(9),
            Some(9),
            Some(9),
            Some(9),
            Some(9),
            Some(9),
            Some(9)
        ]
    );
}

#[test]
fn digit_glyph_u_steps_match_the_two_widget_styles() {
    assert_eq!(dance_score_digit_u(0), 0x00);
    assert_eq!(dance_score_digit_u(9), 0x90);
    assert_eq!(dance_level_digit_u(0), 0x40);
    assert_eq!(dance_level_digit_u(9), 0x88);
}

#[test]
fn beat_track_mask_widens_with_the_level() {
    assert_eq!(dance_beat_level_mask(0), 3);
    assert_eq!(dance_beat_level_mask(1), 7);
    assert_eq!(dance_beat_level_mask(2), 7);
}

#[test]
fn combo_window_flashes_on_the_masked_slot_inside_the_window() {
    // Level 0 masks to 4: beat 3 on the beat flashes, past 0x46 does not.
    assert!(dance_combo_window_bright(3, 0, 0));
    assert!(dance_combo_window_bright(3, 0, 0x45));
    assert!(!dance_combo_window_bright(3, 0, 0x46));
    assert!(!dance_combo_window_bright(2, 0, 0));
    // Level 1 masks to 8: `beat & 7 == 3` flashes every 8th beat (3, 11, ...),
    // so beat 3 is a combo slot but beat 7 - a level-0 slot - no longer is.
    assert!(dance_combo_window_bright(3, 1, 0));
    assert!(dance_combo_window_bright(11, 1, 0));
    assert!(!dance_combo_window_bright(7, 1, 0));
    assert!(!dance_combo_window_bright(5, 1, 0));
}

#[test]
fn beat_track_note_scrolls_one_cell_per_beat() {
    // On the beat (frac 0): note i sits at base + i*16 - 9.
    assert_eq!(dance_beat_track_note_x(120, 0, 0), 120 - 9);
    assert_eq!(dance_beat_track_note_x(120, 1, 0), 120 + 16 - 9);
    // Across a full beat the fraction subtracts a further ~16 texels: note 1
    // has scrolled almost onto note 0's on-beat slot.
    let edge = dance_beat_track_note_x(120, 1, BEAT_PERIOD - 1);
    assert!(edge < dance_beat_track_note_x(120, 1, 0));
    assert!(edge <= dance_beat_track_note_x(120, 0, 0) + 1);
}

#[test]
fn hit_sting_keys_two_voices_per_random_pick() {
    for r in 0..STING_RANDOM_VARIANTS {
        let [a, b] = dance_hit_sting_voices(r);
        assert_eq!((a.voice, b.voice), (0x12, 0x13));
        assert_eq!((a.tone, b.tone), ((2 * r) as i16, (2 * r + 1) as i16));
        assert_eq!((a.note, b.note), (0x3c + r as i16, 0x3c + r as i16));
        // Both voices carry the two arguments the earlier port dropped:
        // `li a1,0x2` (level) and `li a2,0x1` (program). The program is
        // what makes the browser page's `tones[1]` bank lookup the right
        // one rather than a guess.
        assert_eq!((a.level, b.level), (STING_LEVEL, STING_LEVEL));
        assert_eq!((a.program, b.program), (STING_PROGRAM, STING_PROGRAM));
    }
}

/// The groovy-move tiers key a sting the random space never reaches, so
/// the kernel has to answer for it too: `FUN_801d1af4` reaches
/// `FUN_801d3d78` from four sites and three of them pass a literal `5`.
#[test]
fn the_tier_sting_is_outside_the_random_space() {
    let [a, b] = dance_hit_sting_voices(STING_TIER_VARIANT);
    assert_eq!((a.tone, b.tone), (0xa, 0xb));
    assert_eq!((a.note, b.note), (0x41, 0x41));
    assert_eq!((a.voice, b.voice), (0x12, 0x13));
    // Same primitive, same two dropped-then-restored arguments.
    assert_eq!((a.level, b.level), (STING_LEVEL, STING_LEVEL));
    assert_eq!((a.program, b.program), (STING_PROGRAM, STING_PROGRAM));
    // No random pick can produce it, which is why a `0..3` enumeration is
    // short one sting rather than merely unlucky.
    for r in 0..STING_RANDOM_VARIANTS {
        assert_ne!(dance_hit_sting_voices(r)[0].tone, a.tone);
    }
}

#[test]
fn good_banner_places_banner_centre_and_stars_symmetric() {
    let s = good_banner_spawn(0x0abc);
    assert_eq!(s.weight, 0x0abc);
    assert_eq!(s.banner.sprite_id, 0xb);
    assert_eq!((s.stars[0].sprite_id, s.stars[1].sprite_id), (0x16, 0x16));
    // The two stars flank the banner symmetrically (0x38 either side, then
    // the shared <<3 spawn convention).
    let (bx, lx, rx) = (s.banner.x, s.stars[0].x, s.stars[1].x);
    assert_eq!(rx - bx, bx - lx);
    assert_eq!(bx, 0xa0 << 3);
}

#[test]
fn face_rig_remaps_only_the_qualifier_cast() {
    // Qualifier: dancer -> kind (2 -> 3, 1 -> 2), so rig id == dancer kind.
    assert_eq!(dance_face_rig(DanceMode::Qualifier, 0), Some(0));
    assert_eq!(dance_face_rig(DanceMode::Qualifier, 1), Some(2));
    assert_eq!(dance_face_rig(DanceMode::Qualifier, 2), Some(3));
    // Other modes stamp the dancer index straight through.
    assert_eq!(dance_face_rig(DanceMode::Finals, 1), Some(1));
    assert_eq!(dance_face_rig(DanceMode::HowTo, 3), Some(3));
    // Dancers past the fourth are not stamped.
    assert_eq!(dance_face_rig(DanceMode::FreePlay, 4), None);
}

#[test]
fn fade_weight_collapses_past_the_window_instead_of_saturating() {
    assert_eq!(sprite_part_fade_weight(0), 0);
    assert_eq!(sprite_part_fade_weight(0x10), 1);
    // 0x4000 >> 4 = 0x400, clamped to 0xff.
    assert_eq!(sprite_part_fade_weight(0x4000), 0xFF);
    // One past the window is zero, not 0xff.
    assert_eq!(sprite_part_fade_weight(0x4001), 0);
    assert_eq!(sprite_part_fade_weight(0xFFFF), 0);
}

#[test]
fn dancer_emit_modes_differ_in_rounding_and_flags() {
    assert_eq!(sprite_part_emit(0, 0, 0, 0), SpritePartEmit::CopyTemplate);
    assert_eq!(sprite_part_emit(1, 0, 0, 0), SpritePartEmit::SetTemplateZ);
    assert_eq!(
        sprite_part_emit(2, 16, -1, 0x20),
        SpritePartEmit::Shadowed {
            x: 2,
            // -1 rounds toward zero before the shift.
            y: 0,
            flags: [0x420, 0x820],
        }
    );
    // Mode 3 does not scale at all.
    assert_eq!(
        sprite_part_emit(3, 16, -1, 0x20),
        SpritePartEmit::Plain {
            x: 16,
            y: -1,
            flags: 0x20
        }
    );
    assert_eq!(
        sprite_part_emit(4, 16, -1, 0x0A),
        SpritePartEmit::Marker {
            x: 16,
            y: -1,
            clut_byte: 0xA0
        }
    );
    assert_eq!(sprite_part_emit(5, 0, 0, 0), SpritePartEmit::None);
}

#[test]
fn scene_teardown_restores_the_caller_and_forces_a_bgm_reload() {
    let s = dance_scene_stage();
    // The teardown restores what the init saved - it does not write a
    // dance-side scene literal. `0x801D518C` is BSS in the static PROT
    // 0980 image, and the only overlay carrying `other1` is the fishing
    // one; the dance's own venue is `other7` at block base 0x4CC.
    assert!(s.restores_caller_scene);
    assert!(s.restores_scene_block_base);
    assert_eq!(DANCE_SCENE_BLOCK_BASE, 0x4CC);
    assert_eq!(
        legaia_asset::dance_cast::DANCE_SCENE_NAME,
        "other7",
        "the dance venue's scene name lives in the asset crate"
    );
    assert!(s.clear_pad_latch);
    // The impossible-value write is the BGM swap's force-reload idiom.
    assert_eq!(s.bgm_force_reload, -1);
}

#[test]
fn the_entry_and_the_teardown_agree_on_the_block_base() {
    let e = dance_scene_entry();
    // The teardown restores `_DAT_80084540` from the slot the entry wrote,
    // so the two have to name the same value or the BGM resolver indexes
    // a different block after the dance than before it.
    assert_eq!(e.scene_block_base, DANCE_SCENE_BLOCK_BASE);
    assert!(dance_scene_stage().restores_scene_block_base);
    // The field file the entry loads is the venue block itself; the second
    // stream is the audio bank, five entries along.
    assert_eq!(e.stream_ids.0, u32::from(DANCE_SCENE_BLOCK_BASE));
    assert_eq!(e.stream_ids.1, 0x4d1);
}

#[test]
fn the_entry_stages_the_qualifier_floor() {
    let e = dance_scene_entry();
    // Three per-dancer slots cleared, and the qualifier cast is the same
    // size - the overlay stages one floor whichever mode runs later.
    assert_eq!(e.cleared_dancer_slots, QUALIFIER_KINDS.len());
    assert_eq!(e.cleared_dancer_slots, 3);
    // Slot 0 is stamped at pose 1, every later call at pose 0; the mode
    // global is up (finals, no remap) for the first batch of three and
    // down (qualifier) for the second.
    assert_eq!(e.face_stamps[0], (0, 1));
    assert!(e.face_stamps[1..].iter().all(|&(_, pose)| pose == 0));
    assert_eq!(e.face_stamp_mode, [1, 1, 1, 0, 0]);
    // Slots 1 and 2 are stamped twice, slot 0 once - five calls, three
    // slots, and the repeat is what makes the count odd.
    assert_eq!(e.face_stamps.len(), 5);
    for slot in 0..e.cleared_dancer_slots as u8 {
        assert!(e.face_stamps.iter().any(|&(s, _)| s == slot));
    }
}

#[test]
fn the_dance_view_window_is_centred_where_the_fields_is_offset() {
    let e = dance_scene_entry();
    let (x0, z0, x1, z1) = e.view_window;
    // Symmetric about the camera on both axes.
    assert_eq!(x1, -x0);
    assert_eq!(z1, -z0);
    assert_eq!(i32::from(x1 - x0), 16);
    assert_eq!(i32::from(z1 - z0), 20);

    // The field's default is offset instead - further ahead than behind
    // and further left than right - so the two are not the same box even
    // though both are deeper than wide.
    let f = crate::mode_entry_init::FIELD_DEFAULT_VIEW_WINDOW;
    assert_ne!(f.2, -f.0);
    assert_ne!(f.3, -f.1);
    assert_ne!(e.view_window, f);
    // And the dance floor is the larger box on both axes.
    assert!(x1 - x0 > f.2 - f.0);
    assert!(z1 - z0 > f.3 - f.1);

    // Y is above the origin: the dancer's spawn height is negative.
    assert!(e.dancer_spawn.1 < 0);
}

#[test]
fn countin_banner_slides_holds_then_fades() {
    // Slide-in: two half-bright halves flying in from 0xb4 toward centre.
    let s0 = dance_countin_banner_envelope(0);
    assert!(!s0.hold);
    assert_eq!(s0.x_offset, 0xb4);
    assert_eq!(s0.brightness, 0x80 / 2);
    let s29 = dance_countin_banner_envelope(29);
    assert_eq!(s29.x_offset, 0xb4 - 6 * 29);
    assert!(!s29.hold);

    // Hold: single opaque centred banner, brightness ramps from 0x80 and
    // clamps at 0xff; the intro cue fires on entry (frame 0x1e).
    let h = dance_countin_banner_envelope(0x1e);
    assert!(h.hold);
    assert_eq!(h.x_offset, 0);
    assert_eq!(h.brightness, 0x80);
    assert_eq!(COUNTIN_INTRO_CUE, 0x200);
    // Deep into the hold the ramp saturates at full brightness.
    assert_eq!(dance_countin_banner_envelope(0x59).brightness, 0xff);

    // Slide-out: two halves again, flying back out as brightness fades.
    let o = dance_countin_banner_envelope(0x5a);
    assert!(!o.hold);
    assert_eq!(o.x_offset, 0);
    assert_eq!(o.brightness, 200 / 2);
    let o_late = dance_countin_banner_envelope(0x5a + 30);
    assert!(o_late.x_offset > o.x_offset);
    assert!(o_late.brightness < o.brightness);
}

/// Retail runs the banner animator once every three vsyncs with its own
/// counter advancing by three, so the halves jump 18 px per visible step.
/// A per-vsync port slid them 6 - same destination, three times the
/// sampling rate, and a visibly smoother slide than retail's.
#[test]
fn the_countin_animator_runs_once_every_three_vsyncs() {
    let mut ci = CountIn::new();
    let first = ci.step().banner.unwrap();
    // Two more vsyncs return the SAME envelope - the animator has not run.
    assert_eq!(ci.step().banner, Some(first));
    assert_eq!(ci.step().banner, Some(first));
    // The fourth vsync is the animator's next run, three counter units on.
    let second = ci.step().banner.unwrap();
    assert_ne!(second, first);
    assert_eq!(
        first.x_offset - second.x_offset,
        6 * COUNTIN_ANIM_STEP,
        "one visible step is 18 px"
    );

    // The whole count-in - READY to its 0x6F exit, GO! in and out - lasts
    // COUNTIN_TOTAL_VSYNCS: 38 + 11 + 11 animator runs of three vsyncs, with
    // `done` on the first vsync of the run after.
    assert_eq!(COUNTIN_RUNS, 38 + 11 + 11);
    let mut ci = CountIn::new();
    let mut vsyncs = 1;
    while !ci.step().done {
        vsyncs += 1;
        assert!(vsyncs < 1000, "count-in never finished");
    }
    assert_eq!(vsyncs, COUNTIN_TOTAL_VSYNCS);
}

/// `FUN_801cf470` states 3 -> 4 -> 5: the READY banner is cut at counter
/// `0x6F` (before its slide-out ends), then `GO!` fades in to `0x3C * 2` and
/// back out, and the run-start cue `0x201` fires once during the fade-in -
/// after the READY hold's `0x200`.
#[test]
fn the_countin_leaves_ready_at_0x6f_then_fades_go_with_the_start_cue() {
    let mut ci = CountIn::new();
    let mut last_ready = None;
    let mut go_peak = 0;
    let mut cues = Vec::new();
    let mut go_frames = 0;
    loop {
        let s = ci.step();
        if let Some(c) = s.cue {
            cues.push(c);
        }
        if s.banner.is_some() {
            last_ready = Some(ci.frame() - COUNTIN_ANIM_STEP);
            assert!(s.go.is_none(), "READY and GO! never draw together");
        }
        if let Some(g) = s.go {
            go_frames += 1;
            go_peak = go_peak.max(g);
        }
        if s.done {
            break;
        }
    }
    // The last READY run drew counter 111 = 0x6F, short of the 0x78 end.
    assert_eq!(last_ready, Some(COUNTIN_READY_EXIT));
    const { assert!(COUNTIN_READY_EXIT < COUNTIN_END_FRAME) };
    assert_eq!(go_peak, COUNTIN_GO_FULL * 2);
    assert_eq!(go_frames, 22 * 3, "11 runs in, 11 runs out");
    assert_eq!(cues, vec![COUNTIN_INTRO_CUE, COUNTIN_START_CUE]);
}

#[test]
fn clip_driver_gate_fires_on_spin_or_flag() {
    // A spinning dancer (groovy-move turns left) drives its clip.
    assert!(dance_clip_driver_gate(1, 0));
    // The 0x1000 flag bit alone drives it too.
    assert!(dance_clip_driver_gate(0, 0x1000));
    assert!(dance_clip_driver_gate(0, 0x1234));
    // Neither: idle, no clip drive.
    assert!(!dance_clip_driver_gate(0, 0));
    assert!(!dance_clip_driver_gate(-1, 0x2000));
}

fn probe_widget() -> legaia_asset::dance_art::DanceWidget {
    legaia_asset::dance_art::DanceWidget {
        scale: 0x1000,
        tpage: 0x0008,
        clut: 0x7D08,
        u: 0x10,
        v: 0x20,
        w: 0x20,
        h: 0x10,
        rgb_top: [0x80, 0x40, 0x20],
        rgb_bottom: [0x40, 0x20, 0x10],
        semi: 0,
    }
}

#[test]
fn widget_quad_is_centred_and_half_open() {
    let w = probe_widget();
    let q = dance_hud_widget_quad(&w, 0, 100, 50, 0, 0x100, 0x1000);
    // 1:1 scale + 1:1 size means half-extent is exactly cell/2.
    assert_eq!((q.x0, q.x1), (100 - 0x10, 100 + 0x10));
    assert_eq!((q.y0, q.y1), (50 - 8, 50 + 8));
    // Half-open UVs: `u + w`, not `u + w - 1` like the Baka emitter's.
    assert_eq!(q.uv[0], (0x10, 0x20));
    assert_eq!(q.uv[3], (0x30, 0x30));
    assert_eq!(q.poly_code, 0x3C);
    assert_eq!(q.tpage_attr, 0x0008);
    // Brightness 0x100 passes the tints through unchanged.
    assert_eq!(q.rgb_top, w.rgb_top);
    assert_eq!(q.rgb_bottom, w.rgb_bottom);
    // Half brightness halves every channel.
    let dim = dance_hud_widget_quad(&w, 0, 0, 0, 0, 0x80, 0x1000);
    assert_eq!(dim.rgb_top, [0x40, 0x20, 0x10]);
}

#[test]
fn the_widget_ids_upper_bits_override_the_records_blend() {
    let w = probe_widget();
    // Mode 0 takes the record's own semi bit and the caller's abr byte.
    let m0 = dance_hud_widget_quad(&w, 1, 0, 0, 8, 0x100, 0x1000);
    assert_eq!(m0.poly_code, 0x3C);
    assert_eq!(m0.tpage_attr, 0x0008 + 0x20);
    // Any other mode forces semi-transparency on and *is* the abr rate.
    let m1 = dance_hud_widget_quad(&w, 0, 0, 0, (1 << 10) | 8, 0x100, 0x1000);
    assert_eq!(m1.poly_code, 0x3E);
    assert_eq!(m1.tpage_attr, 0x0008 + 0x20);
    assert_eq!(m1.clut, w.clut);
    // Mode 2 additionally replaces the CLUT with the fixed override.
    let m2 = dance_hud_widget_quad(&w, 0, 0, 0, (2 << 10) | 8, 0x100, 0x1000);
    assert_eq!(m2.clut, DANCE_MODE2_CLUT);
    assert_eq!(m2.tpage_attr, 0x0008 + 0x40);
    // The index field is masked, so the mode bits never leak into it.
    assert_eq!(DANCE_WIDGET_ID_MASK & ((2 << 10) | 8), 8);
}

#[test]
fn the_human_always_lands_in_the_centre_score_box() {
    // Whichever mode, slot 0 (the human) is in the centre box except in
    // the finals, where the mode global rotates the trio.
    assert_eq!(dance_score_box_slots(0), Some((0, 1, 2)));
    assert_eq!(dance_score_box_slots(1), Some((1, 2, 0)));
    assert_eq!(dance_score_box_slots(2), Some((0, 2, 1)));
    assert_eq!(dance_score_box_slots(3), Some((0, 2, 1)));
    // The unreachable default arm reads an unwritten register in retail;
    // the port refuses rather than inventing a slot.
    assert_eq!(dance_score_box_slots(4), None);
}

#[test]
fn hud_driver_skips_the_side_boxes_in_free_play() {
    let scores = [111, 222, 333];
    let gauges = [1500, 500, 2500];
    let versus = dance_hud_draws(0, scores, gauges, false);
    assert_eq!(
        versus
            .iter()
            .filter(|d| matches!(d, DanceHudDraw::ScoreBox { .. }))
            .count(),
        3
    );
    let solo = dance_hud_draws(3, scores, gauges, false);
    assert_eq!(
        solo.iter()
            .filter(|d| matches!(d, DanceHudDraw::ScoreBox { .. }))
            .count(),
        1
    );
    // Only the centre box, and it carries slot 0's score.
    assert_eq!(
        solo[0],
        DanceHudDraw::Score {
            slot: 0,
            x: DANCE_SCORE_DIGIT_X[0],
            y: DANCE_SCORE_Y,
            value: 111
        }
    );
}

#[test]
fn the_rival_hud_rows_are_gated_off_by_default() {
    let off = dance_hud_draws(0, [0; 3], [0; 3], false);
    let on = dance_hud_draws(0, [0; 3], [0; 3], true);
    let tracks = |v: &[DanceHudDraw]| {
        v.iter()
            .filter(|d| matches!(d, DanceHudDraw::BeatTrack { .. }))
            .count()
    };
    assert_eq!(tracks(&off), 1, "only the human's track without the flag");
    assert_eq!(tracks(&on), 3);
    // The rivals' rows sit at the traced off-centre positions.
    assert!(on.contains(&DanceHudDraw::BeatTrack {
        slot: 1,
        x: 0xDC,
        y: 0xD4
    }));
    assert!(on.contains(&DanceHudDraw::BeatTrack {
        slot: 2,
        x: 0x18,
        y: 0xD4
    }));
}

/// The HUD frame's **presentation** is the engine's, not a host's: which
/// rows exist, at which 320x240 seats, in which pen. It used to live
/// inside the native window's dance block, so the browser play page drew
/// a plain status line and no frame at all.
#[test]
fn the_hud_frame_resolves_its_own_rows() {
    let mut g = DanceGame::new(chart(), false);
    // The rival gate is the dev counter `_DAT_8007B6D0`, zero in every
    // retail run - a versus mode does not raise it (this asserted the
    // opposite while the hosts stood in for it with a mode test).
    for mode in [
        DanceMode::Qualifier,
        DanceMode::Finals,
        DanceMode::HowTo,
        DanceMode::FreePlay,
    ] {
        g.mode = mode;
        assert!(!g.rival_hud_visible(), "{mode:?}");
    }
    g.mode = DanceMode::Qualifier;
    let solo = g.hud_frame_rows(false);
    let versus = g.hud_frame_rows(true);
    assert!(
        versus.len() > solo.len(),
        "the rival flag adds the rivals' gauges and tracks"
    );
    // Every seat the emitter names survives into a row, and the score
    // readouts take the bright pen while the chrome takes the dim one.
    let gauge = solo
        .iter()
        .find(|r| r.text.starts_with("Lv."))
        .expect("the human's groove gauge is always in the frame");
    assert_eq!(
        (gauge.x, gauge.y),
        (DANCE_GAUGE_XY.0 as i32, DANCE_GAUGE_XY.1 as i32)
    );
    assert!(gauge.dim);
    assert!(
        solo.iter().any(|r| !r.dim),
        "a score readout takes the bright pen"
    );
    // The human's own beat track is the host's row, not the frame's: the
    // frame carries the rivals' only.
    let rival_tracks = versus
        .iter()
        .filter(|r| r.text.len() == 8 && r.text.chars().all(|c| "<>^.".contains(c)))
        .count();
    assert_eq!(rival_tracks, 2);
    assert!(
        !solo
            .iter()
            .any(|r| r.text.len() == 8 && r.text.chars().all(|c| "<>^.".contains(c)))
    );
}

#[test]
fn a_running_game_lays_its_own_hud_out() {
    let mut g = DanceGame::new(chart(), false);
    assert_eq!(g.mode(), DanceMode::Qualifier);
    let draws = g.hud_draws(false);
    assert!(draws.contains(&DanceHudDraw::Gauge {
        slot: 0,
        x: DANCE_GAUGE_XY.0,
        y: DANCE_GAUGE_XY.1,
        value: 0
    }));
    // With no overlay image behind it there is no widget table to resolve.
    assert!(g.hud_quads(false).is_empty());
    // The score readout tracks the run: land a step, then re-read the HUD.
    g.press(DanceDir::A);
    let scored = g.hud_draws(false);
    assert_eq!(
        scored[0],
        DanceHudDraw::Score {
            slot: 0,
            x: DANCE_SCORE_DIGIT_X[0],
            y: DANCE_SCORE_Y,
            value: g.score()
        }
    );
}

// ------------------------------------------------- dancer actor records

#[test]
fn a_run_spawns_one_actor_per_floor_slot() {
    let g = game();
    assert_eq!(g.dancer_actors().len(), g.dancer_count());
    // The pool is populated by the constructor, not by a test: every slot
    // already carries the retail spawn scale.
    for a in g.dancer_actors() {
        assert_eq!(a.scale, crate::minigame_actor::SPAWN_SCALE);
    }
    assert_eq!(g.dancer_clip_frames().len(), g.dancer_count());
    // Nothing enters the sprite-part pool until something scores.
    assert!(g.sprite_parts().is_empty());
}

#[test]
fn the_groovy_spin_raises_the_clip_drive_flag() {
    let mut g = game();
    // No clip is bound on a chart-only run (the ids are overlay data), so
    // the gate is false until the flag arm fires.
    assert!(g.dancer_clip_frames().iter().all(|f| !f.clip_driver));
    // A triangle throws the human into the groovy move, which is the
    // `0x1000` arm: `FUN_801d4098` runs the clip driver regardless.
    let ev = g.press(DanceDir::C);
    assert!(matches!(ev, DanceEvent::Groovy { .. }), "{ev:?}");
    assert!(g.in_groovy_move());
    let f = g.dancer_clip_frames();
    assert!(f[0].clip_driver, "the spinning dancer must drive its clip");
    assert_eq!(
        g.dancer_actors()[0].flags & crate::minigame_actor::FLAG_DRIVE_CLIP,
        crate::minigame_actor::FLAG_DRIVE_CLIP
    );
    // The rivals are not spinning, so their gate stays down.
    assert!(f[1..].iter().all(|s| !s.clip_driver));
}

// ---------------------------------------------------------- sprite parts

#[test]
fn a_closed_chain_spawns_the_banner_parts_and_they_emit() {
    let mut g = game();
    assert!(g.sprite_parts().is_empty());
    // Lane 0 closes its chain on the first matched note.
    let ev = g.press(DanceDir::A);
    assert!(matches!(ev, DanceEvent::Sequence { .. }), "{ev:?}");
    // The banner + its two stars, spawned by the rules engine itself.
    assert_eq!(g.sprite_parts().len(), 3);
    let frames = g.sprite_part_emits();
    assert_eq!(frames.len(), 3);
    // The banner sits at screen centre: `0xa0 << 3` spawned, `>> 3` back
    // out again by the emit dispatch.
    match frames[0].emit {
        SpritePartEmit::Shadowed { x, y, flags } => {
            assert_eq!((x, y), (0xa0, 0x90));
            assert_eq!(flags, [0xb | 0x400, 0xb | 0x800]);
        }
        other => panic!("a part takes the shadowed arm, got {other:?}"),
    }
    // A fresh part spawns at the top of the ramp, so the prologue's `>> 4`
    // + clamp puts it at full weight; `advance` decays it from there. The
    // clamp is why the hold runs long and the fade is the tail: the weight
    // only leaves 0xFF once `+0x78` drops below `0xFF << 4`.
    assert!(frames.iter().all(|f| f.fade == 0xFF), "{frames:?}");
    let hold = (u32::from(crate::minigame_actor::BEAT_FADE_CEILING) - 0xFF0) / PART_AGE_STEP;
    g.advance(hold);
    assert!(g.sprite_part_emits().iter().all(|f| f.fade == 0xFF));
    g.advance(4);
    let mid = g.sprite_part_emits();
    assert!(
        mid.iter().all(|f| f.fade > 0 && f.fade < 0xFF),
        "the fade weight must track the part's age, got {mid:?}"
    );
}

#[test]
fn sprite_parts_retire_when_the_fade_runs_out() {
    let mut g = game();
    g.press(DanceDir::A);
    assert_eq!(g.sprite_parts().len(), 3);
    let frames_to_zero = u32::from(crate::minigame_actor::BEAT_FADE_CEILING) / PART_AGE_STEP;
    for _ in 0..frames_to_zero {
        g.advance(1);
    }
    assert!(g.sprite_parts().is_empty(), "{:?}", g.sprite_parts());
}

#[test]
fn the_fade_prologue_collapses_above_the_ceiling() {
    // The one arm the port's own driving never reaches, kept covered on
    // the kernel: retail compares `+0x78` against 0x4000 as a signed 32-bit
    // value, so a value past it goes to zero outright, not to a saturated
    // 0xFF.
    assert_eq!(sprite_part_fade_weight(0x4000), 0xFF);
    assert_eq!(sprite_part_fade_weight(0x4001), 0);
    assert_eq!(sprite_part_fade_weight(0xFFFF), 0);
    assert_eq!(sprite_part_fade_weight(0x100), 0x10);
    assert_eq!(sprite_part_fade_weight(0), 0);
}

#[test]
fn the_emit_dispatch_rounds_a_negative_pair_toward_zero() {
    let mut g = game();
    g.press(DanceDir::A);
    // Park a part on a negative component so the round-toward-zero shift
    // is exercised through the live record.
    g.parts.actors_mut()[0].pos = [-1, 0x40, 0];
    match g.sprite_part_emits()[0].emit {
        SpritePartEmit::Shadowed { x, y, .. } => assert_eq!((x, y), (0, 8)),
        other => panic!("a part takes the shadowed arm, got {other:?}"),
    }
}

// ---------------------------------------------------------- move clip length

/// Five kinds whose clips carry distinct ids: idle `10 + k`, dance `20 + k`,
/// move pair `p` = `100 + p` (pair 1 in the party bank).
fn synthetic_kinds() -> Vec<legaia_asset::dance_cast::DanceKind> {
    use legaia_asset::dance_cast::{DanceClip, DanceKind, MOVE_PAIRS};
    let clip = |anim_id: u16, party_bank: bool| DanceClip {
        anim_id,
        party_bank,
        rate: 16,
    };
    (0..5u16)
        .map(|k| DanceKind {
            model: k,
            home: [0; 3],
            idle: clip(10 + k, false),
            dance: clip(20 + k, false),
            alt: clip(0, false),
            moves: (0..MOVE_PAIRS as u16)
                .map(|p| clip(100 + p, p == 1))
                .collect(),
        })
        .collect()
}

#[test]
fn a_judge_move_holds_the_dancer_until_its_clip_ends() {
    let mut g = game();
    g.kinds = synthetic_kinds();
    // The miss reaction (circle, pair 1) plays 40 ticks.
    let mut ticks = std::collections::HashMap::new();
    ticks.insert((101u16, 16u16), 40u32);
    g.clip_ticks = Some(ticks);
    g.advance(1);
    assert_eq!(g.dancers[0].clip, 20, "the dance loop is bound in play");

    assert_eq!(g.press(DanceDir::B), DanceEvent::Miss);
    assert_eq!(g.dancers[0].clip, 101, "the miss reaction is bound");
    // Well past the note latch (15 at 2 a frame), the reaction still plays
    // and the dancer is still not judged - retail's award routine only runs
    // while a standing loop is bound.
    for _ in 0..20 {
        g.advance(1);
    }
    assert_eq!(g.dancers[0].latch, 0, "the note latch has long expired");
    assert_eq!(g.dancers[0].clip, 101);
    assert!(g.dancers[0].locked(u32::MAX));
    // The move's own end flag rebinds the loop.
    for _ in 0..20 {
        g.advance(1);
    }
    assert_eq!(g.dancers[0].clip, 20, "the loop is back at the move's end");
    assert!(!g.dancers[0].locked(u32::MAX));
}

#[test]
fn without_clip_lengths_the_note_latch_times_the_move() {
    let mut g = game();
    g.kinds = synthetic_kinds();
    g.advance(1);
    assert_eq!(g.press(DanceDir::B), DanceEvent::Miss);
    assert_eq!(g.dancers[0].clip, 101);
    for _ in 0..8 {
        g.advance(1);
    }
    assert_eq!(g.dancers[0].clip, 20, "the latch fallback rebinds the loop");
}

/// The HUD quad frame keeps retail's emission order: every score digit run
/// before every box frame. One ordering-table bucket holds the whole HUD and
/// `AddPrim` prepends, so whatever is emitted first draws last - the digits
/// over the boxes' opaque interiors. With the frames emitted first the boxes
/// covered every score on both hosts.
#[test]
fn hud_digits_are_emitted_before_their_score_boxes() {
    use legaia_asset::dance_art::DanceWidget;
    let mut g = game();
    // A synthetic widget table: each record's cell row `v` is its own index,
    // so a quad's source widget reads straight off its UVs.
    g.widgets = (0..=0x21u8)
        .map(|i| {
            (
                DanceWidget {
                    scale: 0x1000,
                    tpage: 0x0008,
                    clut: 0x7D00,
                    u: 0,
                    v: i,
                    w: 16,
                    h: 16,
                    rgb_top: [0x80; 3],
                    rgb_bottom: [0x80; 3],
                    semi: 0,
                },
                0,
            )
        })
        .collect();
    for (i, d) in g.dancers.iter_mut().enumerate() {
        d.score = 40 + i as u32;
    }
    let quads = g.hud_draw_quads(false);
    let src = |q: &DanceHudQuad| q.uv[0].1;
    let digit_at: Vec<usize> = (0..quads.len()).filter(|&i| src(&quads[i]) == 1).collect();
    let box_at: Vec<usize> = (0..quads.len())
        .filter(|&i| u32::from(src(&quads[i])) == DANCE_SCORE_BOX_WIDGET)
        .collect();
    assert_eq!(digit_at.len(), 6, "two digits per score, three scores");
    assert_eq!(box_at.len(), 3, "one frame per score box");
    assert!(
        digit_at.iter().max() < box_at.iter().min(),
        "every digit run precedes every box frame: digits {digit_at:?}, boxes {box_at:?}"
    );
}
