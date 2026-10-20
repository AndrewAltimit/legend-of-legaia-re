use super::*;

/// The host-facing banner set: each session edge seeds its own timer, the
/// per-frame service produces the animators' draws, and every banner
/// eventually retires itself.
#[test]
fn fishing_banners_seed_service_and_retire() {
    let mut b = FishingBanners::default();
    assert!(b.service_frame(1).is_empty(), "idle set draws nothing");

    // Hook: the splash (two glyphs) and the from-left banner (one) run.
    b.on_hook();
    assert!(b.hook.is_active() && b.splash.is_active());
    assert_eq!(b.service_frame(1).len(), 3);

    // Landed: the reel-in banner cancels the hook banner, as the retail
    // driver tail does.
    b.on_landed();
    assert!(!b.hook.is_active(), "from-left cancelled by from-right");
    assert!(b.reel_in.is_active());

    // Snapped instead: the line-break arm seeds the converging pair and
    // stops the hook banner; the miss banner is not its edge.
    let mut b2 = FishingBanners::default();
    b2.on_hook();
    b2.on_snapped();
    assert!(b2.converge.is_active() && !b2.hook.is_active());
    assert!(!b2.miss.is_active());

    // The recast seeds nothing (the empty reel-in arm stores state 0x32
    // and no timer).
    let mut b3 = FishingBanners::default();
    b3.on_recast();
    assert_eq!(b3, FishingBanners::default());

    // Every timer retires on its own once its animation expires; the
    // splash outlives the banners, so run past both lifetimes.
    for _ in 0..(SPLASH_FRAMES + BANNER_FRAMES) {
        b2.service_frame(1);
    }
    assert!(b2.service_frame(1).is_empty(), "all banners retired");
    assert!(!b2.miss.is_active() && !b2.converge.is_active());
}

#[test]
fn banner_variants_share_the_slide_ramp_and_expire_together() {
    // All four banner animators ride `banner_slide`; the two mirrored ones
    // hold at the same 0xa0 centre as the from-left banner.
    let hold = 0x20;
    assert_eq!(banner_slide(hold), 0xa0);
    let miss = banner_miss_draw(hold).expect("active mid-hold");
    match miss {
        HudDraw::Glyph { id, x, .. } => {
            assert_eq!(id, 0x19);
            assert_eq!(x, 0x140 - 0xa0);
        }
        _ => panic!("miss banner is a glyph draw"),
    }
    let pair = banner_converge_draws(hold).expect("active mid-hold");
    match (pair[0], pair[1]) {
        (HudDraw::Glyph { x: a, id: ia, .. }, HudDraw::Glyph { x: b, id: ib, .. }) => {
            assert_eq!((ia, ib), (0xc, 0xc), "same glyph both sides");
            assert_eq!(a + b, 0x140, "mirrored about the screen centre");
        }
        _ => panic!("converge banner is a glyph pair"),
    }
    // Both expire exactly at the shared lifetime.
    assert!(banner_miss_draw(BANNER_FRAMES - 1).is_some());
    assert!(banner_miss_draw(BANNER_FRAMES).is_none());
    assert!(banner_converge_draws(BANNER_FRAMES).is_none());
}

#[test]
fn digit_field_blanks_leading_zeros_and_right_aligns() {
    let cells = number_digit_cells(0, 100, 50, 42);
    let digits: Vec<i32> = cells.iter().map(|c| c.digit).collect();
    assert_eq!(digits, vec![4, 2], "only significant digits are emitted");
    // Right-aligned in the 8-slot field: '4' lands in slot 6, '2' in 7.
    assert_eq!(cells[0].x, 100 + 6 * DIGIT_PITCH_NARROW);
    assert_eq!(cells[1].x, 100 + 7 * DIGIT_PITCH_NARROW);
    assert!(cells.iter().all(|c| c.y == 50));
}

#[test]
fn digit_field_draws_a_lone_zero() {
    // The seeded last slot is what keeps a zero total visible.
    let cells = number_digit_cells(0, 0, 0, 0);
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0].digit, 0);
    assert_eq!(cells[0].x, 7 * DIGIT_PITCH_NARROW);
}

#[test]
fn digit_field_style_selects_the_slot_pitch() {
    let wide = number_digit_cells(1, 0, 0, 7);
    assert_eq!(wide[0].x, 7 * DIGIT_PITCH_WIDE);
}

#[test]
fn digit_field_fills_every_slot_at_eight_digits() {
    let cells = number_digit_cells(0, 0, 0, 12_345_678);
    let digits: Vec<i32> = cells.iter().map(|c| c.digit).collect();
    assert_eq!(digits, vec![1, 2, 3, 4, 5, 6, 7, 8]);
}

/// The fill is the dome readout's, so the two retail routines' shared half
/// stays shared: every slot the fishing field emits is the slot
/// `decimal_slots` holds, at the index it holds it.
#[test]
fn digit_field_slots_are_the_dome_readouts_slots() {
    for value in [0, 7, 42, 100, 999_999, 12_345_678, 99_999_999] {
        let slots = crate::muscle_dome_hud::decimal_slots(value);
        let cells = number_digit_cells(0, 0, 0, value);
        let expected: Vec<(usize, i32)> = slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.map(|d| (i, d as i32)))
            .collect();
        let got: Vec<(usize, i32)> = cells
            .iter()
            .map(|c| ((c.x / DIGIT_PITCH_NARROW) as usize, c.digit))
            .collect();
        assert_eq!(got, expected, "value {value}");
    }
}

/// Retail draws nothing for a negative value - the fill leaves seven slots
/// blank and puts the negative quotient in the units slot, which the draw
/// loop's `bltz` skips. Not a guard, and not a lone zero.
#[test]
fn digit_field_draws_nothing_for_a_negative_value() {
    assert!(number_digit_cells(0, 0, 0, -1).is_empty());
    assert!(number_digit_cells(0, 0, 0, -12_345).is_empty());
    assert!(number_digit_cells(1, 40, 20, -7).is_empty());
}

#[test]
fn bar_frames_span_their_segments_and_track_the_fill() {
    let segs = 8;
    let h = bar_frame(20, 40, BAR_VALUE_ONE, segs, 0);
    assert_eq!(h.glyphs, [3, 4, 5]);
    // Caps bracket the body along x; y is constant.
    assert_eq!(h.positions[0], (20, 40));
    assert_eq!(h.positions[2], (20 + segs * 8 + 8, 40));
    assert_eq!(h.fill_len, segs * 8, "full value fills every segment");
    assert_eq!(h.fill_brightness, 0xff);

    let v = power_bar_frame(20, 40, BAR_VALUE_ONE / 2, segs);
    assert_eq!(v.glyphs, [0, 1, 2]);
    // The vertical bar brackets along y instead, at a constant x.
    assert_eq!(v.positions[0], (20, 40));
    assert_eq!(v.positions[2], (20, 40 + segs * 8 + 8));
    assert_eq!(v.fill_len, segs * 8 / 2, "half value fills half the bar");
    assert_eq!(v.fill_brightness, 0x7f);
    assert_eq!(v.body_scale, segs << 12);

    // An empty bar still draws its frame, with nothing lit.
    let empty = bar_frame(0, 0, 0, segs, 0);
    assert_eq!((empty.fill_len, empty.fill_brightness), (0, 0));
}

#[test]
fn bar_style_selects_the_fill_ramp_without_moving_geometry() {
    let segs = 8;
    let value = BAR_VALUE_ONE / 2; // brightness byte 0x7f
    let s0 = bar_frame(20, 40, value, segs, 0);
    let s1 = bar_frame(20, 40, value, segs, 1);

    // Style 0 holds the constant red against the ramp...
    assert_eq!(s0.fill_rgb, Some((BAR_FILL_STYLE0_RED, 0x7f, 0)));
    // ...style 1 runs the ramp against its own complement.
    assert_eq!(s1.fill_rgb, Some((0x7f, 0x80, 0)));
    // Blue is zero in both, and the geometry is identical.
    assert_eq!(s0.positions, s1.positions);
    assert_eq!(
        (s0.fill_len, s0.body_scale, s0.glyphs),
        (s1.fill_len, s1.body_scale, s1.glyphs)
    );

    // The complement tracks the ramp across its range.
    let full = bar_frame(0, 0, BAR_VALUE_ONE, segs, 1);
    assert_eq!(full.fill_rgb, Some((0xff, 0x00, 0)));
    let dark = bar_frame(0, 0, 0, segs, 1);
    assert_eq!(dark.fill_rgb, Some((0x00, 0xff, 0)));

    // Any other style jumps the colour stores entirely.
    assert_eq!(bar_frame(0, 0, value, segs, 2).fill_rgb, None);

    // The power bar is permanently the style-0 ramp.
    assert_eq!(
        power_bar_frame(0, 0, value, segs).fill_rgb,
        Some((BAR_FILL_STYLE0_RED, 0x7f, 0))
    );
}

#[test]
fn hud_bar_variants_resolve_through_the_ported_helpers() {
    // The retail HUD uses style 0 for depth and style 1 for tension,
    // so the draw list exercises both ramps.
    let depth = HudDraw::Bar {
        style: 0,
        x: 0x10,
        y: 0x90,
        value: BAR_VALUE_ONE,
        step: 10,
    };
    let tension = HudDraw::Bar {
        style: 1,
        x: 0x10,
        y: 0xb0,
        value: BAR_VALUE_ONE,
        step: 10,
    };
    assert_eq!(
        depth.resolve_bar().unwrap(),
        bar_frame(0x10, 0x90, BAR_VALUE_ONE, 10, 0)
    );
    assert_eq!(
        tension.resolve_bar().unwrap().fill_rgb,
        Some((0xff, 0x00, 0))
    );

    let power = HudDraw::PowerBar {
        x: 0x120,
        y: 0x40,
        power: 0,
        step: 0xc,
    };
    assert_eq!(power.resolve_bar().unwrap().glyphs, [0, 1, 2]);

    // Non-bar variants carry no frame.
    assert!(
        HudDraw::Caption {
            text: HudCaption::LuresLeft,
            x: 0,
            y: 0,
        }
        .resolve_bar()
        .is_none()
    );
}

#[test]
fn persistent_hud_caps_points_and_gates_the_rod_label() {
    let d = persistent_hud_draws(2_000_000, 1234, 2, 7);
    // Point total renders capped; the best-catch row is uncapped input.
    assert!(d.contains(&HudDraw::Number {
        x: 0x32,
        y: 0x16,
        value: HUD_POINT_CAP,
        brightness: HUD_BRIGHTNESS
    }));
    assert!(d.contains(&HudDraw::Number {
        x: 0x32,
        y: 0x08,
        value: 1234,
        brightness: HUD_BRIGHTNESS
    }));
    // Rod index 2 draws its label; the count row shows the lure count.
    assert!(d.contains(&HudDraw::Caption {
        text: HudCaption::RodName(2),
        x: 0x98,
        y: 0x0c
    }));
    assert!(d.contains(&HudDraw::Count {
        value: 7,
        digits: 4,
        x: 0x100,
        y: 0x0c
    }));
    // An out-of-range rod index draws no label but keeps the count row.
    let d = persistent_hud_draws(0, 0, 3, 0);
    assert!(!d.iter().any(|x| matches!(
        x,
        HudDraw::Caption {
            text: HudCaption::RodName(_),
            ..
        }
    )));
    assert!(d.contains(&HudDraw::Caption {
        text: HudCaption::LuresLeft,
        x: 0xf3,
        y: 0x0c
    }));
}

#[test]
fn catch_hud_length_and_percent_arithmetic() {
    // record 812 -> 512 past the strike base -> 512*100 >> 9 = 100 tenths;
    // extent 1024 -> (1024 >> 8) >> 1 = 2 tenths -> 102 = "10.2".
    assert_eq!(length_display(812, 1024), 102);
    assert_eq!(extent_display(1024), 2);
    // Below the strike base the record term clamps to zero.
    assert_eq!(length_display(0, 0), 0);
    assert_eq!(length_display(299, 0), 0);
    // Negative extent clamps to zero (with retail's toward-zero rounding).
    assert_eq!(extent_display(-1), 0);
    assert_eq!(extent_display(-1024), 0);
    // Cast power percent: percent of the 0x1000 meter ceiling.
    assert_eq!(cast_power_percent(0x1000), 100);
    assert_eq!(cast_power_percent(0x800), 50);
    assert_eq!(cast_power_percent(0x20), 0);
    // The HUD splits the length into whole + tenths digits.
    let s = CatchHudState {
        record: 812,
        line_extent: 1024,
        cast_power: 0x800,
        ..Default::default()
    };
    let d = catch_hud_draws(&s);
    assert!(d.contains(&HudDraw::Number {
        x: 0xda,
        y: 0x30,
        value: 10,
        brightness: HUD_BRIGHTNESS
    }));
    assert!(d.contains(&HudDraw::Number {
        x: 0xe8,
        y: 0x30,
        value: 2,
        brightness: HUD_BRIGHTNESS
    }));
    assert!(d.contains(&HudDraw::Number {
        x: 0xe4,
        y: 0xb0,
        value: 50,
        brightness: HUD_BRIGHTNESS
    }));
    assert!(d.contains(&HudDraw::PowerBar {
        x: 0x120,
        y: 0x40,
        power: 0x800,
        step: 0xc
    }));
}

/// `FUN_801D5298`'s rank ladder, at each threshold's two sides.
#[test]
fn catch_rank_glyph_follows_the_strength_ladder() {
    for (strength, rank) in [
        (0, 0x15),
        (0xC8, 0x15),
        (0xC9, 0x13),
        (0x258, 0x13),
        (0x259, 0x12),
        (0x320, 0x12),
        (0x321, 0x11),
        (0x4B0, 0x11),
        (0x4B1, 0x14),
    ] {
        assert_eq!(catch_rank_glyph(strength), rank, "strength {strength:#x}");
    }
}

/// The plate's draws: dark through the `0x180` lead-in, half the lift
/// once it climbs, the points in the large style, and the name rising
/// into view only once its `y` argument drops below `0xF1`.
#[test]
fn catch_result_plate_fades_up_and_the_name_rises() {
    let at = |ramp| catch_result_draws(ramp, 0x300, 1234, 4, 9);
    // Lead-in: every sprite at brightness 0, the name still below.
    let d = at(0x100);
    assert_eq!(d.len(), 5);
    assert!(d.iter().all(|i| match *i {
        HudDraw::Glyph { brightness, .. } | HudDraw::LargeNumber { brightness, .. } =>
            brightness == 0,
        _ => false,
    }));
    assert_eq!(
        d[0],
        HudDraw::Glyph {
            layer: 0,
            id: 0x412,
            x: 0xA0,
            y: 0x78,
            brightness: 0
        }
    );
    assert_eq!(
        d[2],
        HudDraw::LargeNumber {
            x: 0x20,
            y: 0x88,
            value: 1234,
            brightness: 0
        }
    );
    // `y = 0x1A0 - lift` reaches `0xF0` at lift `0xB0`: the first frame
    // the name draws, centred by `13 * 8 / 4 = 26`, 7 px below `y`.
    assert_eq!(at(0x180 + 0xAF).len(), 5);
    let d = at(0x180 + 0xB0);
    assert_eq!(
        d[5],
        HudDraw::Caption {
            text: HudCaption::FishName(4),
            x: 0xA0 - 26,
            y: 0xF0 + 7
        }
    );
    // Fully lifted: brightness `0x80`, name at `0xA0 + 7`.
    let d = at(0x1000);
    assert!(matches!(
        d[3],
        HudDraw::Glyph {
            id: 0x417,
            brightness: 0x80,
            ..
        }
    ));
    assert!(matches!(d[5], HudDraw::Caption { y: 0xA7, .. }));
    // No resolved name, no caption.
    assert_eq!(catch_result_draws(0x1000, 0, 5, 0, 0).len(), 5);
}

#[test]
fn catch_hud_gauge_block_is_gated_on_hook() {
    let mut s = CatchHudState {
        depth: 0x300,
        tension: 0x700,
        ..Default::default()
    };
    // Not hooked: no gauge bars.
    assert!(
        !catch_hud_draws(&s)
            .iter()
            .any(|d| matches!(d, HudDraw::Bar { .. }))
    );
    // Hooked: the depth (style 0) + tension (style 1) bars appear.
    s.gauges_visible = true;
    let d = catch_hud_draws(&s);
    assert!(d.contains(&HudDraw::Bar {
        style: 0,
        x: 0x10,
        y: 0x90,
        value: 0x300,
        step: 10
    }));
    assert!(d.contains(&HudDraw::Bar {
        style: 1,
        x: 0x10,
        y: 0xb0,
        value: 0x700,
        step: 10
    }));
}

fn glyph_x(d: HudDraw) -> i32 {
    match d {
        HudDraw::Glyph { x, .. } => x,
        other => panic!("expected a glyph, got {other:?}"),
    }
}

#[test]
fn banner_slide_ramp_and_lifetime() {
    // From the left: slide in at 8 px/frame, hold at 0xa0, slide off.
    assert_eq!(glyph_x(banner_from_left_draw(1).unwrap()), 8);
    assert_eq!(glyph_x(banner_from_left_draw(0x13).unwrap()), 0x98);
    assert_eq!(glyph_x(banner_from_left_draw(0x14).unwrap()), 0xa0);
    assert_eq!(glyph_x(banner_from_left_draw(0x8b).unwrap()), 0xa0);
    // The slide-off ramp joins continuously at the hold value.
    assert_eq!(glyph_x(banner_from_left_draw(0x8c).unwrap()), 0xa0);
    assert_eq!(glyph_x(banner_from_left_draw(0xc7).unwrap()), 0x278);
    assert!(banner_from_left_draw(0xc8).is_none());
    // From the right: the mirrored trajectory, holding at the same x.
    assert_eq!(glyph_x(banner_from_right_draw(1).unwrap()), 0x140 - 8);
    assert_eq!(glyph_x(banner_from_right_draw(0x20).unwrap()), 0xa0);
    assert!(banner_from_right_draw(0xc8).is_none());
}

#[test]
fn strike_splash_rises_and_fades() {
    // Fade-in at 8/frame, hold at 0x80, fade-out from frame 0x88.
    assert_eq!(splash_brightness(1), 8);
    assert_eq!(splash_brightness(0x10), 0x80);
    assert_eq!(splash_brightness(0x87), 0x80);
    assert_eq!(splash_brightness(0x90), 0x40);
    // The pair rises one pixel every 32 frames from y = 0x50.
    let [a, b] = strike_splash_draws(0x40).unwrap();
    match (a, b) {
        (
            HudDraw::Glyph {
                id: 0x416,
                x: 0xa0,
                y,
                brightness,
                ..
            },
            HudDraw::Glyph {
                id: 0x816,
                y: y2,
                brightness: b2,
                ..
            },
        ) => {
            assert_eq!(y, 0x50 - 2);
            assert_eq!((y, brightness), (y2, b2));
            assert_eq!(brightness, 0x80);
        }
        other => panic!("unexpected splash pair {other:?}"),
    }
    // Expires exactly at the lifetime end (brightness reaches 0 there).
    assert!(strike_splash_draws(SPLASH_FRAMES - 1).is_some());
    assert!(strike_splash_draws(SPLASH_FRAMES).is_none());
}

#[test]
fn banner_timer_advances_while_active_and_resets() {
    let mut t = BannerTimer::default();
    assert!(!t.is_active());
    assert!(t.service(2, banner_from_left_draw).is_none());
    t.start();
    assert!(t.is_active());
    // Each serviced frame draws and advances by the frame step.
    assert!(t.service(2, banner_from_left_draw).is_some());
    assert_eq!(t.0, 3);
    // Run it out: the animator expires and the timer resets to idle.
    let mut frames = 0;
    while t.service(2, banner_from_left_draw).is_some() {
        frames += 1;
        assert!(frames < 1000, "timer failed to expire");
    }
    assert!(!t.is_active());
    // Cancel mirrors the tail's cross-banner zeroing.
    t.start();
    t.cancel();
    assert!(!t.is_active());
}

// --- consumer -----------------------------------------------------------

/// A 16x16 cell per glyph id, so every id in the fishing HUD resolves.
fn test_atlas_glyph(id: u32) -> Option<(u32, u32, u32, u32)> {
    Some(((id & 0xf) * 16, (id >> 4) * 16, 16, 16))
}

fn test_atlas() -> FishingHudAtlas<'static> {
    FishingHudAtlas {
        solid_src: Some((0, 0, 1, 1)),
        glyph_src: &test_atlas_glyph,
        bar_thickness: 8,
        sprites_drawn: false,
    }
}

#[test]
fn consumer_renders_every_persistent_hud_item() {
    let font = legaia_font::synthetic_for_tests();
    let caps = FishingCaptions::placeholder();
    let atlas = test_atlas();
    let items = persistent_hud_draws(1234, 5678, 2, 7);
    let draws = fishing_hud_draws_for(&font, &items, &caps, &atlas, (0, 0));
    assert!(!draws.is_empty(), "the persistent HUD renders quads");

    // Every item kind in the list must contribute at least one quad -
    // this is what fails if a match arm is dropped or stubbed out.
    for item in &items {
        let one = fishing_hud_draws_for(&font, &[*item], &caps, &atlas, (0, 0));
        assert!(!one.is_empty(), "{item:?} produced no draws");
    }
}

#[test]
fn consumer_renders_every_catch_hud_item_including_the_gauges() {
    let font = legaia_font::synthetic_for_tests();
    let caps = FishingCaptions::placeholder();
    let atlas = test_atlas();
    let items = catch_hud_draws(&CatchHudState {
        record: 812,
        line_extent: 1024,
        cast_power: 0x800,
        depth: 0x400,
        tension: 0x600,
        gauges_visible: true,
    });
    for item in &items {
        let one = fishing_hud_draws_for(&font, &[*item], &caps, &atlas, (0, 0));
        assert!(!one.is_empty(), "{item:?} produced no draws");
    }
    // The hooked gauge block puts both bars in the list, so the render
    // carries their fill quads on top of the cap/body/cap frames.
    let fills = fishing_hud_draws_for(&font, &items, &caps, &atlas, (0, 0))
        .into_iter()
        .filter(|d| Some(d.src) == atlas.solid_src)
        .count();
    assert_eq!(fills, 3, "power bar + depth bar + tension bar fills");
}

#[test]
fn consumer_places_bar_fills_on_the_right_axis() {
    let atlas = test_atlas();
    // Horizontal: the fill starts just past the start cap and runs right.
    let h = bar_frame(0x10, 0xb0, BAR_VALUE_ONE / 2, 8, 1);
    let hd = bar_frame_draws(&h, &atlas, (0, 0));
    let hfill = hd.iter().find(|d| Some(d.src) == atlas.solid_src).unwrap();
    assert_eq!(hfill.dst.0, 0x10 + 8, "starts after the start cap");
    assert_eq!(hfill.dst.2, h.fill_len as u32, "length runs along x");
    assert_eq!(hfill.dst.3, atlas.bar_thickness);

    // Vertical: the fill grows upward from the bottom cap.
    let v = power_bar_frame(0x120, 0x40, BAR_VALUE_ONE / 2, 0xc);
    let vd = bar_frame_draws(&v, &atlas, (0, 0));
    let vfill = vd.iter().find(|d| Some(d.src) == atlas.solid_src).unwrap();
    let bottom = v.positions[2].1;
    assert_eq!(vfill.dst.1, bottom - v.fill_len, "top edge is bottom - len");
    assert_eq!(vfill.dst.3, v.fill_len as u32, "length runs along y");
    assert_eq!(vfill.dst.2, atlas.bar_thickness);
}

#[test]
fn consumer_offsets_by_the_origin_and_ramps_the_tint() {
    let font = legaia_font::synthetic_for_tests();
    let caps = FishingCaptions::placeholder();
    let atlas = test_atlas();
    let glyph = HudDraw::Glyph {
        layer: 0,
        id: 7,
        x: 0x20,
        y: 0x78,
        brightness: HUD_BRIGHTNESS,
    };
    let at_origin = fishing_hud_draws_for(&font, &[glyph], &caps, &atlas, (0, 0));
    let moved = fishing_hud_draws_for(&font, &[glyph], &caps, &atlas, (10, 20));
    assert_eq!(moved[0].dst.0 - at_origin[0].dst.0, 10);
    assert_eq!(moved[0].dst.1 - at_origin[0].dst.1, 20);
    // 0x80 is the neutral vertex brightness; the splash's fade-in is
    // linear below it.
    assert_eq!(hud_tint(HUD_BRIGHTNESS)[0], 1.0);
    assert_eq!(hud_tint(HUD_BRIGHTNESS_FULL)[0], 1.0);
    assert_eq!(hud_tint(0x40)[0], 0.5);
    assert_eq!(hud_tint(0)[0], 0.0);
}

#[test]
fn consumer_drops_glyphs_the_host_cannot_place() {
    let font = legaia_font::synthetic_for_tests();
    let caps = FishingCaptions::placeholder();
    let blind = FishingHudAtlas {
        solid_src: Some((0, 0, 1, 1)),
        glyph_src: &|_| None,
        bar_thickness: 8,
        sprites_drawn: false,
    };
    let items = persistent_hud_draws(1234, 5678, 2, 7);
    let draws = fishing_hud_draws_for(&font, &items, &caps, &blind, (0, 0));
    // Glyph rows vanish, but the numbers and captions still render.
    assert!(!draws.is_empty());
    let glyphs_only = [HudDraw::Glyph {
        layer: 1,
        id: 0x1a,
        x: 0,
        y: 0,
        brightness: HUD_BRIGHTNESS,
    }];
    assert!(fishing_hud_draws_for(&font, &glyphs_only, &caps, &blind, (0, 0)).is_empty());
}

#[test]
fn consumer_right_aligns_numbers_and_blank_pads_counts() {
    let font = legaia_font::synthetic_for_tests();
    let caps = FishingCaptions::placeholder();
    let atlas = test_atlas();
    // A Number goes through the ported digit field: 42 occupies the last
    // two of the eight slots, so its leftmost quad sits six pitches in.
    let num = fishing_hud_draws_for(
        &font,
        &[HudDraw::Number {
            x: 0,
            y: 0,
            value: 42,
            brightness: HUD_BRIGHTNESS,
        }],
        &caps,
        &atlas,
        (0, 0),
    );
    assert_eq!(num.len(), 2, "two significant digits, no leading zeros");
    assert_eq!(num[0].dst.0, 6 * DIGIT_PITCH_NARROW);

    // A Count is `FUN_80034B78`'s fixed-width field: 7 in four 8-px
    // cells is three blank cells and a 7 in the last one.
    let cnt = fishing_hud_draws_for(
        &font,
        &[HudDraw::Count {
            value: 7,
            digits: 4,
            x: 0,
            y: 0,
        }],
        &caps,
        &atlas,
        (0, 0),
    );
    assert_eq!(cnt.len(), 1, "leading cells blank, not zeros");
    assert_eq!(cnt[0].dst.0, 3 * 8);
}
