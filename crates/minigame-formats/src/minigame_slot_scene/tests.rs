use super::*;

fn msg(w: u8, h: u8, fill: u8) -> MarqueeMessage {
    MarqueeMessage {
        u: 0,
        v: 0,
        w,
        h,
        bitmap: vec![fill; w as usize * h as usize],
    }
}

/// The placement blit clips its **destination**, and it clips it unsigned -
/// which is the whole reason a negative row hides a caption instead of
/// wrapping it to the bottom of the matrix.
#[test]
fn the_placement_blit_clips_negative_offsets_away_entirely() {
    let m = msg(4, 4, 7);

    // Fully on-matrix: every texel lands, at `col * stride + row`.
    let mut buf = clear_dots();
    place_message(&mut buf, &m, 2, 3);
    assert_eq!(buf[2 * DOT_STRIDE + 3], 7);
    assert_eq!(buf[5 * DOT_STRIDE + 6], 7);
    assert_eq!(buf.iter().filter(|&&b| b == 7).count(), 16);

    // Pushed two rows above the matrix: the top two source rows are dropped
    // and the rest land from row 0 - a slide-in, not a wrap.
    let mut buf = clear_dots();
    place_message(&mut buf, &m, 0, -2);
    assert_eq!(
        buf.iter().filter(|&&b| b == 7).count(),
        8,
        "two rows clipped"
    );
    assert_eq!(buf[0], 7, "the surviving rows start at row 0");
    assert_eq!(
        buf[DOT_ROWS - 1],
        0,
        "nothing wrapped round to the bottom row"
    );

    // Entirely above the matrix: nothing at all.
    let mut buf = clear_dots();
    place_message(&mut buf, &m, 0, -4);
    assert!(buf.iter().all(|&b| b == 0));

    // Right edge: a message running past the last column loses its tail,
    // which is how the "coin" word clips in retail.
    let mut buf = clear_dots();
    place_message(&mut buf, &msg(8, 1, 3), DOT_COLS as i32 - 3, 0);
    assert_eq!(buf.iter().filter(|&&b| b == 3).count(), 3);
}

/// The two composers are not interchangeable: one scrolls the source through
/// a fixed window, the other places the destination. Same message, same
/// offset, different picture.
#[test]
fn scrolling_and_placing_are_opposite_operations() {
    let m = msg(4, 4, 5);
    // Placed at column 3, the message occupies columns 3..7.
    let mut placed = clear_dots();
    place_message(&mut placed, &m, 3, 0);
    assert_eq!(placed[3 * DOT_STRIDE], 5);
    assert_eq!(placed[0], 0);
    // Scrolled by 3, the message's own column 3 sits at matrix column 0.
    let scrolled = compose_marquee(&m, 3, 0);
    assert_eq!(scrolled[0], 5);
    assert_eq!(scrolled[3 * DOT_STRIDE], 0);
}

/// Leading-zero suppression tests the whole figure at each place, so a
/// four-digit payout prints four digits and a two-digit payout prints two -
/// never a zero-padded field.
#[test]
fn the_payout_caption_suppresses_leading_zeros_only() {
    let caption = |payout: i32| {
        compose_marquee_frame(&MarqueeFrame {
            payout,
            payout_frame: 0xFF,
            ..Default::default()
        })
    };

    // 1000: all four places plus the word.
    let p = caption(1000);
    assert_eq!(p.len(), 5);
    assert_eq!(p[0].msg, MSG_NUMBER_BASE + 1);
    for digit in &p[1..4] {
        assert_eq!(digit.msg, MSG_NUMBER_BASE, "the three trailing zeros print");
    }
    assert_eq!(p[4].msg, MSG_COINS);

    // 7: the units place and the word, nothing else.
    let p = caption(7);
    assert_eq!(p.len(), 2);
    assert_eq!(p[0].msg, MSG_NUMBER_BASE + 7);
    assert_eq!(p[0].col, PAYOUT_DIGIT_COLS[3] as i32);
    assert_eq!(p[1].msg, MSG_COINS);

    // 405: the hundreds, the interior zero, and the units.
    let p = caption(405);
    assert_eq!(p.len(), 4);
    assert_eq!(p[0].msg, MSG_NUMBER_BASE + 4);
    assert_eq!(
        p[1].msg, MSG_NUMBER_BASE,
        "an interior zero is NOT suppressed"
    );
    assert_eq!(p[2].msg, MSG_NUMBER_BASE + 5);

    // A caption needs both a figure and a running clock.
    assert!(caption(0).is_empty());
    assert!(
        compose_marquee_frame(&MarqueeFrame {
            payout: 500,
            payout_frame: 0,
            ..Default::default()
        })
        .is_empty()
    );
}

/// The round pips light one per round still owed, left to right.
#[test]
fn the_round_pips_count_the_rounds_still_owed() {
    let pips = |rounds: i32| {
        compose_marquee_frame(&MarqueeFrame {
            feature_mode: 6,
            reel_state: 1,
            bonus_rounds: rounds,
            ..Default::default()
        })
        .iter()
        .map(|p| p.msg == MSG_ROUND_PIP_ON)
        .collect::<Vec<_>>()
    };
    assert_eq!(pips(3), [true, true, true]);
    assert_eq!(pips(1), [true, false, false]);
    assert_eq!(pips(0), [false, false, false]);
}

/// Outside the bonus feature modes the strip stays dark - the attract legend
/// scrolls there instead, and it is not a placement.
#[test]
fn the_tally_strip_only_draws_in_the_bonus_feature_modes() {
    for mode in 0u8..=6 {
        let out = compose_marquee_frame(&MarqueeFrame {
            feature_mode: mode,
            reel_state: 3,
            ..Default::default()
        });
        assert_eq!(
            !out.is_empty(),
            (4..=6).contains(&mode),
            "feature mode {mode}"
        );
    }
    // ...and only in the reel states 1-4.
    for state in [0i32, 5, 100] {
        assert!(
            compose_marquee_frame(&MarqueeFrame {
                feature_mode: 6,
                reel_state: state,
                ..Default::default()
            })
            .is_empty()
        );
    }
}

#[test]
fn trig_matches_the_scus_tables() {
    // Spot values read out of the SCUS tables at SIN_TABLE_VA / COS_TABLE_VA.
    assert_eq!(sin_4096(0), 0);
    assert_eq!(sin_4096(1024), 4096);
    assert_eq!(sin_4096(128), 799);
    assert_eq!(cos_4096(0), 4096);
    assert_eq!(cos_4096(1024), 0);
    assert_eq!(cos_4096(128), 4017);
}

#[test]
fn trig_truncates_toward_zero_like_the_retail_tables() {
    // The spot values above are agnostic - both roundings land on them.
    // These three are not, and each pins a different edge of the rule.
    //
    // Positive fraction >= 0.5: rounding would say 13 / 19.
    assert_eq!(sin_4096(2), 12);
    assert_eq!(sin_4096(3), 18);
    // The amplitude never saturates off-axis: cos(1) is 4095.995, and only
    // the exact quarter turns reach 0x1000.
    assert_eq!(cos_4096(1), 4095);
    assert_eq!(cos_4096(4), 4095);
    // Negative half: toward zero, not toward minus infinity. sin(2055) is
    // -43.98, so flooring would say -44.
    assert_eq!(sin_4096(2055), -43);
    assert_eq!(sin_4096(3000), -4071);
    // The cosine table is the sine table a quarter turn ahead, entry for
    // entry - so the port's two functions cannot disagree at a boundary.
    for a in 0..ANGLE_FULL {
        assert_eq!(cos_4096(a), sin_4096(a + ANGLE_FULL / 4), "angle {a}");
    }
}

#[test]
fn the_payline_face_is_brightest_and_the_near_half_is_black() {
    // Angle 0x800 carries the symbol on the payline: z = -512, peak shade.
    assert_eq!(reel_z(0x800), -512);
    assert_eq!(reel_shade(reel_z(0x800)), REEL_SHADE_MAX);
    // The near half of the cylinder (z >= 0) shades to black, which is what
    // hides it without a backface cull.
    assert_eq!(reel_shade(reel_z(0)), 0);
}

#[test]
fn the_glass_sits_in_front_of_the_reels() {
    // -z is toward the viewer, so the glass (z = -800) is nearer than the
    // reel's payline face (z = -512) and therefore renders larger.
    assert!(view_scale(GLASS_Z) > view_scale(-512));
}

/// The exporters' vanishing-point form is `project` rearranged, not a
/// second fit: the two agree everywhere in the machine's depth range.
#[test]
fn the_exporter_form_is_the_gte_projection() {
    for &(x, y, z) in &[
        (0, 0, 0),
        (-640, -192, -768),
        (632, -192, -800),
        (-512, 585, 0),
        (256, -640, -800),
    ] {
        let k = PROJ_Z0 / (PROJ_Z0 + z as f32);
        let ex = PROJ_OFX + PROJ_SX0 * k * (x as f32 - PROJ_VANISH.0);
        let ey = PROJ_OFY + PROJ_SX0 / PROJ_ASPECT * k * (y as f32 - PROJ_VANISH.1);
        let (px, py) = project(x, y, z);
        assert!(
            (ex - px).abs() < 1e-3 && (ey - py).abs() < 1e-3,
            "{x},{y},{z}"
        );
    }
    assert_eq!(PROJ_Z0, 8160.0);
    assert_eq!(PROJ_VANISH.0, 240.0);
}

#[test]
fn the_projection_puts_the_reels_where_retail_does() {
    // Each reel's payline face starts at the column the
    // `minigame_slot_machine` display crop has its first lit face pixel
    // on (118 / 221 / 324). The old fitted projection put these three to
    // four columns left; the captured GTE registers land them.
    for (r, want) in [(0usize, 118.0f32), (1, 221.0), (2, 324.0)] {
        let (sx, _) = project(reel_x(r), 0, reel_z(0x800));
        assert!(
            (sx - want).abs() < 1.5,
            "reel {r} left edge projected to {sx}, retail has it at {want}"
        );
    }
}
