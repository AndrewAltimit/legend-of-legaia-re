use super::*;

#[test]
fn a_won_round_raises_you_win_split_about_the_centre_with_its_line() {
    let mut c = BakaChrome::default();
    c.raise_result(RoundResult::Win);
    let f = c.step(&ChromeTick::default(), (&[], &[]));
    let words: Vec<(u8, i16, i16, i32)> = f
        .draws
        .iter()
        .map(|d| (d.widget, d.x, d.y, d.brightness))
        .collect();
    assert!(
        words.contains(&(0x08, 0xD0, 0x78, 0x80)),
        "WIN! right of centre"
    );
    assert!(
        words.contains(&(0x07, 0x70, 0x78, 0x80)),
        "YOU left of centre"
    );
    assert_eq!(
        f.xa,
        Some(XaCue {
            clip: 0x20,
            chan: 2,
            dur: 0x45
        })
    );
    // The line starts once; the words hold until cleared.
    let g = c.step(&ChromeTick::default(), (&[], &[]));
    assert_eq!(g.xa, None);
    assert_eq!(g.draws.len(), 2);
    c.clear_result();
    assert!(c.step(&ChromeTick::default(), (&[], &[])).draws.is_empty());
}

/// The result stands for the round-over hold; the next round's banner
/// asked for meanwhile waits and starts when the hold ends.
#[test]
fn the_round_banner_waits_out_the_result_hold() {
    let mut c = BakaChrome::default();
    c.raise_result(RoundResult::Lose);
    c.start_round_banner(ROUND_BANNER_SPRITE);
    assert!(!c.busy(), "the round banner waits for the result");
    for _ in 0..RESULT_HOLD_FRAMES {
        c.step(&ChromeTick::default(), (&[], &[]));
    }
    assert!(c.result_sprites().is_empty(), "the hold is over");
    assert!(c.busy(), "the deferred round banner runs");
}

#[test]
fn draw_and_perfect_stand_alone_at_the_centre_and_lose_splits() {
    assert_eq!(
        result_banner_spawns(RoundResult::Draw),
        vec![(0x0A, 0xA0, 0x78)]
    );
    assert_eq!(
        result_banner_spawns(RoundResult::Perfect),
        vec![(0x11, 0xA0, 0x78)]
    );
    assert_eq!(
        result_banner_spawns(RoundResult::Lose),
        vec![(0x09, 0xD0, 0x78), (0x07, 0x70, 0x78)]
    );
    assert_eq!(result_xa(RoundResult::Lose).chan, 3);
    assert_eq!(result_xa(RoundResult::Draw).chan, 4);
    assert_eq!(result_xa(RoundResult::Perfect).chan, 5);
}

#[test]
fn the_editor_band_is_an_unsigned_window_above_the_round_band() {
    assert!(!editor_band_open(100));
    assert!(!editor_band_open(399));
    assert!(editor_band_open(400));
    assert!(editor_band_open(499));
    assert!(!editor_band_open(500));
    // The unsigned test also rejects everything below the base.
    assert!(!editor_band_open(-1));
}

#[test]
fn outside_the_band_the_editor_retires_and_does_nothing() {
    let mut slots = Vec::new();
    let t = editor_tick(100, 5, 4, 2, 1, true, 8, &[0, 1, 2], &mut slots);
    assert!(t.retired);
    assert_eq!(t.edit, EditorKeyEdit::None);
    assert!(slots.is_empty());
}

#[test]
fn a_moved_frame_cursor_installs_or_removes_by_the_edit_mode() {
    let mut slots = Vec::new();
    let t = editor_tick(400, 0, 0, 2, 1, true, 8, &[0, 1, 2, 3], &mut slots);
    assert_eq!(t.edit, EditorKeyEdit::Install);
    assert_eq!(slots, vec![2]);
    assert_eq!(t.keyframe, Some(2));

    let t = editor_tick(400, 0, 0, 2, 1, false, 8, &[0, 1, 2, 3], &mut slots);
    assert_eq!(t.edit, EditorKeyEdit::Remove);

    // A still cursor edits nothing.
    let t = editor_tick(400, 0, 0, 2, 2, true, 8, &[0, 1, 2, 3], &mut slots);
    assert_eq!(t.edit, EditorKeyEdit::None);
}

#[test]
fn the_editor_wraps_both_cursors() {
    let mut slots = Vec::new();
    let t = editor_tick(
        400,
        EDITOR_ACTION_COUNT,
        0,
        9,
        9,
        false,
        4,
        &[0, 1],
        &mut slots,
    );
    assert_eq!(t.action, 0);
    assert_eq!(t.frame, 0);
}

#[test]
fn impact_pair_mirrors_by_slot_and_lifts_by_the_special_latch() {
    let a = impact_effect_pair(0, (100, 20, 300), (5, 6, 7), true, false);
    assert_eq!(a[0].pos, (105, 26, 307 - IMPACT_Z_LIFT));
    assert_eq!(a[0].rot, (0, 0, 0));
    assert_eq!(a[0].template, IMPACT_TEMPLATE_A[0]);
    assert_eq!(a[1].rot, (0, IMPACT_YAW[0], 0));
    assert_eq!(a[1].template, IMPACT_TEMPLATE_B[0]);

    // A clear facing flag subtracts the keyframe X instead.
    let b = impact_effect_pair(1, (100, 20, 300), (5, 6, 7), false, true);
    assert_eq!(b[0].pos, (95, 26, 307));
    assert_eq!(b[1].rot, (0, IMPACT_YAW[1], 0));
    assert_eq!(IMPACT_YAW[0], -IMPACT_YAW[1]);
}

#[test]
fn impact_keyframe_index_honours_the_reset_argument() {
    assert_eq!(impact_keyframe_index(4, false), 4);
    assert_eq!(impact_keyframe_index(4, true), 0);
}

#[test]
fn sprite_blit_biases_the_source_rect_off_the_table_entry() {
    let c = sprite_blit(0, [0x40, 0x10]).unwrap();
    assert_eq!(c.src_x, 0x10 + BLIT_SRC_X_BASE);
    assert_eq!(c.src_y, 0x10 + BLIT_SRC_Y_BASE);
    assert_eq!(c.size, (6, 0x18));
    // The destination is fixed, not table-driven.
    assert_eq!((c.dst_x, c.dst_y), (0x340, BLIT_DST_Y));
    // Only mode 0 is a defined call.
    assert!(sprite_blit(1, [0x40, 0x10]).is_none());
}

#[test]
fn an_empty_live_mask_retires_the_afterimage() {
    let f = afterimage_pass(0, 0, 1, 0, 1, 1);
    assert!(f.retire);
    assert!(f.passes.is_empty());
}

#[test]
fn afterimage_ghosts_lag_three_frames_each_and_darken() {
    // Both bits live, one frame past the spawn: step 8 at n = 1 advances
    // 16 (one whole frame); the ghosts sit 3 and 6 frames behind it and
    // both are still before the clip start.
    let f = afterimage_pass(0b11, 0, 1, 8, 1, 30);
    assert_eq!(f.cursor, 16);
    assert_eq!(f.passes[0].cursor, 16 - 0x30);
    assert_eq!(f.passes[1].cursor, 16 - 0x60);
    assert_eq!(f.passes[0].cue, AFTERIMAGE_CUE_START);
    assert_eq!(f.passes[1].cue, AFTERIMAGE_CUE_START + AFTERIMAGE_CUE_STEP);
    assert!(f.passes.iter().all(|p| !p.drawn && !p.expired));
}

#[test]
fn afterimage_ghosts_expire_on_the_clip_end() {
    // Cursor far past a one-frame clip: both expire, mask clears.
    let f = afterimage_pass(0b11, 0x400, 0, 0, 1, 1);
    assert!(f.passes.iter().all(|p| p.expired && !p.drawn));
    assert_eq!(f.live_mask, 0);
    assert_eq!(f.cursor, 0x400);
    // Inside the clip a ghost draws.
    let f = afterimage_pass(0b01, 0x100, 0, 0, 1, 0x40);
    assert!(f.passes[0].drawn);
    assert_eq!(f.live_mask, 0b01);
}

#[test]
fn afterimage_spawn_halves_the_special_speed() {
    // `speed * 4 >> 3`, rounded toward zero.
    assert_eq!(AfterimageActor::spawn(1, 13).step, 6);
    assert_eq!(AfterimageActor::spawn(0, -3).step, -1);
    let mut a = AfterimageActor::spawn(0, 8);
    assert_eq!((a.live_mask, a.cursor, a.step), (3, 0, 4));
    // n = 1: the pass doubles the step, so the ghost clip runs at the
    // pre-special rate while the fighter plays at half.
    a.tick(1, Some((1, 30)));
    assert_eq!(a.cursor, 8);
}

#[test]
fn cameo_walks_in_turns_poses_and_walks_out() {
    let p = cameo_pose(0).unwrap();
    assert_eq!(
        (p.x, p.yaw, p.clip, p.blit_index),
        (0x100, 0x400, CAMEO_CLIP_WALK, 0)
    );
    assert_eq!((p.y, p.z), (CAMEO_Y, CAMEO_Z));
    assert_eq!(cameo_pose(0x10).unwrap().x, 0x80);
    let p = cameo_pose(0x30).unwrap();
    assert_eq!((p.x, p.yaw), (0, 0x200));
    let p = cameo_pose(0x40).unwrap();
    assert_eq!(
        (p.yaw, p.clip, p.hold_last_frame, p.blit_index),
        (0, CAMEO_CLIP_POSE, true, 1)
    );
    let p = cameo_pose(0xA0).unwrap();
    assert_eq!(
        (p.yaw, p.clip, p.hold_last_frame),
        (0x200, CAMEO_CLIP_WALK, false)
    );
    let p = cameo_pose(0xC0).unwrap();
    assert_eq!(
        (p.x, p.yaw, p.blit_index, p.retire),
        (-0x80, 0x400, 1, false)
    );
    assert!(cameo_pose(CAMEO_RETIRE_PHASE).unwrap().retire);
    assert!(cameo_pose(-1).is_none());
    assert!(cameo_spawns(0x10) && cameo_spawns(0x110) && !cameo_spawns(0x40));
}

#[test]
fn intro_fires_each_announcer_line_once() {
    let mut st = IntroTitle::default();
    let mut lines = Vec::new();
    for t in 0..200 {
        if let Some(c) = st.frame(t).xa {
            lines.push((c.clip, c.chan, c.dur));
        }
    }
    assert_eq!(lines, vec![(0x20, 0x0E, 0x3F), (0x20, 0x0F, 0x76)]);
    assert_eq!(st.announced, 2);
}

#[test]
fn intro_logo_brightness_ramps_then_holds() {
    let mut st = IntroTitle::default();
    assert!(st.frame(29).draws.is_empty());
    let f = st.frame(30);
    assert_eq!(f.draws[0].brightness, 0);
    let mut st = IntroTitle::default();
    assert_eq!(st.frame(40).draws[0].brightness, 80);
    let mut st = IntroTitle::default();
    // The multiplier clamps at 0x10 once t - 30 reaches 0x11.
    assert_eq!(st.frame(60).draws[0].brightness, 0x80);
    let mut st = IntroTitle::default();
    assert_eq!(st.frame(99).draws[0].brightness, 0x80);
}

#[test]
fn intro_subtitle_shrinks_and_flashes_the_clut() {
    let mut st = IntroTitle::default();
    let f = st.frame(100);
    assert_eq!(f.banner_clut, Some(BANNER_CLUT_FLASH));
    assert_eq!(f.tint, Some(ScreenTint { grey: 0xFF }));
    let sub = f.draws.iter().find(|d| d.widget == 0x22).unwrap();
    assert_eq!(sub.size, (0x10 << 11) + 0x1000);
    let mut st = IntroTitle::default();
    let f = st.frame(116);
    assert_eq!(f.banner_clut, Some(BANNER_CLUT_IDLE));
    let sub = f.draws.iter().find(|d| d.widget == 0x22).unwrap();
    assert_eq!(sub.size, 0x1000);
    assert!(f.tint.is_none());
}

#[test]
fn intro_caption_ramp_walks_four_cells() {
    let cell = |t: i32| {
        let f = IntroTitle { announced: 2 }.frame(t);
        f.draws
            .iter()
            .find(|d| (0x24..=0x27).contains(&d.widget))
            .map(|d| d.widget)
    };
    assert_eq!(cell(140), None);
    assert_eq!(cell(147), Some(0x24));
    assert_eq!(cell(152), Some(0x25));
    assert_eq!(cell(156), Some(0x26));
    assert_eq!(cell(160), Some(0x27));
    assert_eq!(cell(164), None);
}

#[test]
fn intro_tint_fades_white_to_black_over_64_frames() {
    let g = |t: i32| IntroTitle { announced: 2 }.frame(t).tint.unwrap().grey;
    assert_eq!(g(140), 0xFF);
    assert_eq!(g(150), 0xFF - 40);
    assert_eq!(g(204), 0x00);
    assert_eq!(g(400), 0x00);
}

#[test]
fn banner_voice_channel_is_the_round_index() {
    let f = round_banner_frame(0, 2);
    assert_eq!(
        f.xa,
        Some(XaCue {
            clip: 0x1F,
            chan: 2,
            dur: 0x48
        })
    );
    assert!(round_banner_frame(1, 2).xa.is_none());
}

#[test]
fn banner_converges_then_parts() {
    // Slide in: the offset walks 0xB4 -> 0 in 6px steps.
    assert_eq!(round_banner_frame(0, 0).draws[0].x, 0xB4 + 0x90);
    assert_eq!(round_banner_frame(29, 0).draws[0].x, 0xB4 - 29 * 6 + 0x90);
    // Joined pose: level ramps 0x80 -> 0xFF over 30 frames.
    assert_eq!(round_banner_frame(30, 0).draws[0].brightness, 0x80);
    assert_eq!(round_banner_frame(60, 0).draws[0].brightness, 0xFF);
    // Held at the clamp until the part starts.
    assert_eq!(round_banner_frame(89, 0).draws[0].brightness, 0xFF);
    // Parting: level falls from 0xC8, halved for the parted pose.
    assert_eq!(round_banner_frame(90, 0).draws[0].brightness, 0xC8 / 2);
}

#[test]
fn banner_digit_is_round_plus_one() {
    let f = round_banner_frame(0, 3);
    let g = f.draws.iter().find(|d| d.widget == GLYPH_WIDGET).unwrap();
    assert_eq!(g.glyph, Some(4));
    assert_eq!(glyph_u(4), 96);
}

#[test]
fn banner_flags_track_the_parted_pose() {
    assert!(round_banner_flags(0));
    assert!(!round_banner_flags(30));
    assert!(!round_banner_flags(89));
    assert!(round_banner_flags(90));
}

#[test]
fn countdown_walks_its_four_states() {
    let mut st = Countdown::default();
    let f = countdown_frame(&mut st, 1, false, 0, 0, 0);
    assert_eq!(st.state, 2);
    // Both lines cannot fire on the same frame - the second wins the slot,
    // exactly as retail's single call does.
    assert_eq!(f.xa.unwrap().chan, 0x0B);
    assert_eq!(st.timer, COUNTDOWN_TIMER);
    // Below the fade gate nothing decays.
    countdown_frame(&mut st, 4, false, 0x10, 0, 0);
    assert_eq!(st.timer, COUNTDOWN_TIMER);
    for _ in 0..8 {
        countdown_frame(&mut st, 4, false, 0x11, 0, 0);
    }
    // Eight ticks land the timer exactly on zero - the transition needs
    // the ninth, because retail tests the post-decrement value for `< 0`.
    assert_eq!(st.timer, 0);
    assert_eq!(st.state, 2);
    countdown_frame(&mut st, 4, false, 0x11, 0, 0);
    assert_eq!(st.timer, 0);
    assert_eq!(st.state, 3);
}

#[test]
fn countdown_final_round_swaps_line_and_widget() {
    let mut st = Countdown { state: 2, timer: 1 };
    let f = countdown_frame(&mut st, 4, false, 0x20, 0, COUNTDOWN_FINAL_ROUND);
    assert_eq!(f.xa.unwrap().chan, 0x0D);
    assert!(f.draws.iter().any(|d| d.widget == 0x1D));
    let mut st = Countdown { state: 2, timer: 1 };
    let f = countdown_frame(&mut st, 4, false, 0x20, 0, 0);
    assert_eq!(f.xa.unwrap().chan, 0x0C);
    assert!(f.draws.iter().any(|d| d.widget == 0x1C));
}

#[test]
fn countdown_loading_freezes_the_sequence_but_still_draws() {
    let mut st = Countdown::default();
    let f = countdown_frame(&mut st, 4, true, 0x40, 0x40, 0);
    assert_eq!(st.state, 1);
    assert_eq!(f.draws.len(), 2);
    assert_eq!(f.draws[0].brightness, 0x20);
}

#[test]
fn actor_draw_conditions_the_fade_level() {
    let a = ChromeActor {
        x: 10,
        y: 20,
        id: 7,
        size: 0x800,
        fade: 0x100,
    };
    let d = chrome_actor_draw(&a, 0, 0).draw.unwrap();
    assert_eq!(d.brightness, 0x10);
    assert_eq!(d.size, 0x800);
    // Above 0x4000 the level is discarded, not clamped.
    let a = ChromeActor { fade: 0x4001, ..a };
    assert_eq!(chrome_actor_draw(&a, 0, 0).draw.unwrap().brightness, 0);
    // Saturation at 0xFF.
    let a = ChromeActor { fade: 0x4000, ..a };
    assert_eq!(chrome_actor_draw(&a, 0, 0).draw.unwrap().brightness, 0xFF);
}

#[test]
fn actor_draw_modes() {
    let a = ChromeActor {
        x: 1,
        y: 2,
        id: 3,
        size: 0x1000,
        fade: 0x800,
    };
    assert!(!chrome_actor_draw(&a, 0, 1).retire);
    assert!(chrome_actor_draw(&a, 1, 1).retire);
    assert!(!chrome_actor_draw(&a, 1, 0).retire);
    let g = chrome_actor_draw(&a, 2, 0).draw.unwrap();
    assert_eq!(g.widget, GLYPH_WIDGET);
    assert_eq!(g.glyph, Some(3));
    assert_eq!(g.size, 0x1000);
    assert!(chrome_actor_draw(&a, 3, 0).draw.is_none());
}

#[test]
fn anim_slots_reuse_then_append_then_saturate() {
    let mut keys = Vec::new();
    let a = anim_slot_install(&mut keys, 0x30);
    assert_eq!((a.slot, a.reused, a.count), (0, false, 1));
    let b = anim_slot_install(&mut keys, 0x3F);
    // 0x3F >> 4 == 3 == 0x30 >> 4, so it lands back in slot 0.
    assert_eq!((b.slot, b.reused, b.count), (0, true, 1));
    // Key 3 is already resident, so seven fresh keys fill the table.
    for k in 4..11 {
        anim_slot_install(&mut keys, k * 0x10);
    }
    assert_eq!(keys.len(), 8);
    assert!(anim_slot_install(&mut keys, 0x900).full);
    assert_eq!(keys.len(), 8);
}

#[test]
fn anim_slot_delete_compacts_and_reports_the_index() {
    let mut keys = Vec::new();
    for k in 0..4 {
        anim_slot_install(&mut keys, k * 0x10);
    }
    assert_eq!(keys, vec![0, 1, 2, 3]);

    // A miss changes nothing and reports `-1` (here: `None`).
    let miss = anim_slot_delete(&mut keys, 0x90);
    assert_eq!(miss.slot, None);
    assert_eq!(miss.count, 4);
    assert_eq!(keys, vec![0, 1, 2, 3]);

    // The query takes the same `>> 4` the installer used, so 0x1F finds
    // the slot 0x10 installed.
    let hit = anim_slot_delete(&mut keys, 0x1F);
    assert_eq!(hit.slot, Some(1));
    assert_eq!(hit.count, 3);
    // Everything above the hole moved down one place.
    assert_eq!(keys, vec![0, 2, 3]);

    // Deleting the last live slot needs no move at all.
    let tail = anim_slot_delete(&mut keys, 0x30);
    assert_eq!((tail.slot, tail.count), (Some(2), 2));
    assert_eq!(keys, vec![0, 2]);
}

#[test]
fn anim_slot_key_truncates_toward_zero_on_both_sides() {
    // The `+0xF` bias is what makes the arithmetic shift truncate toward
    // zero instead of toward negative infinity.
    assert_eq!(anim_slot_key(0x1F), 1);
    assert_eq!(anim_slot_key(-0x1F), -1);
    assert_eq!(anim_slot_key(-0x20), -2);
    assert_eq!(anim_slot_key(-1), 0);
}

#[test]
fn editor_removes_the_keyframe_when_edit_mode_is_off() {
    let mut slots = vec![0i16, 1, 2];
    let t = editor_tick(
        EDITOR_PHASE_BASE,
        0,
        0,
        1,
        0,
        false,
        8,
        &[0, 0x10, 0x20],
        &mut slots,
    );
    assert_eq!(t.edit, EditorKeyEdit::Remove);
    // frame 1 << EDITOR_FRAME_SHIFT, taken back down by `>> 4`, is key 1.
    assert_eq!(slots, vec![0, 2]);
}

#[test]
fn actor_bind_retires_once_the_match_ends() {
    let b = chrome_actor_bind(0, 1, 3, (3, 4), 0, &[], &[]);
    assert_eq!(b.flags, ACTOR_FLAG_RETIRE);
    assert_eq!(b.frames, None);
}

#[test]
fn actor_bind_picks_bank_and_frame_count() {
    // Offset-table word 1 -> record at byte 0x20; byte +7 of it is the
    // frame count.
    let mut bank = vec![0u8; 64];
    bank[4..8].copy_from_slice(&0x20u32.to_le_bytes());
    bank[0x20 + 7] = 0x1B;
    let b = chrome_actor_bind(0xFF, 0, 3, (3, 4), 1, &bank, &[]);
    assert_eq!(b.frames, Some(0x1B));
    assert_eq!(b.fade, 0);
    assert_eq!(b.flags, 0xFF & !ACTOR_FLAG_TICK);
    assert!(!b.clear_accum);
    // At or above the split it is the other bank, and `+0x74` is cleared.
    // The id masks down to the same slot, which is what makes the split a
    // bank select rather than an index offset.
    let b = chrome_actor_bind(0, 0, 9, (3, 4), ANIM_BANK_SPLIT + 1, &[], &bank);
    assert_eq!(b.frames, Some(0x1B));
    assert_eq!(b.fade, 0x800);
    assert!(b.clear_accum);
}

#[test]
fn actor_hold_sets_one_bit() {
    assert_eq!(chrome_actor_hold(1), 1 | ACTOR_FLAG_HOLD);
}

#[test]
fn the_runner_spawns_a_banner_actor_and_drives_the_timeline() {
    let mut c = BakaChrome::default();
    assert!(!c.busy());
    c.start_round_banner(ROUND_BANNER_SPRITE);
    assert!(c.busy());
    assert_eq!(c.sprites().len(), 1);
    // The spawn goes through the shared screen-centre wrapper.
    assert_eq!(c.sprites()[0].actor.x, 0xA0);
    assert_eq!(c.sprites()[0].actor.y, 0x78);
    assert_eq!(c.sprites()[0].flags & ACTOR_FLAG_HOLD, ACTOR_FLAG_HOLD);

    let tick = ChromeTick::default();
    let f = c.step(&tick, (&[], &[]));
    // Frame 0 fires the banner's announce line and draws the banner's
    // four quads plus the countdown's two.
    assert_eq!(f.xa.map(|x| x.clip), Some(0x1F));
    assert!(f.draws.len() >= 6);
}

#[test]
fn the_runner_retires_its_banner_actors_once_the_match_ends() {
    let mut c = BakaChrome::default();
    c.start_round_banner(ROUND_BANNER_SPRITE);
    let tick = ChromeTick {
        match_phase: 1,
        ..ChromeTick::default()
    };
    c.step(&tick, (&[], &[]));
    assert!(c.sprites().is_empty());
}

#[test]
fn the_banner_timeline_runs_out_and_disarms() {
    let mut c = BakaChrome::default();
    c.start_round_banner(ROUND_BANNER_SPRITE);
    let tick = ChromeTick::default();
    for _ in 0..BANNER_END {
        c.step(&tick, (&[], &[]));
    }
    assert!(!c.busy());
}

#[test]
fn the_intro_card_runs_only_while_armed() {
    let mut plain = BakaChrome::default();
    let tick = ChromeTick::default();
    assert!(plain.step(&tick, (&[], &[])).draws.is_empty());

    let mut c = BakaChrome::with_intro();
    assert!(c.busy());
    // The card is silent before its first range opens.
    assert!(c.step(&tick, (&[], &[])).draws.is_empty());
    for _ in 0..INTRO_LOGO_IN {
        c.step(&tick, (&[], &[]));
    }
    assert!(!c.step(&tick, (&[], &[])).draws.is_empty());
    for _ in 0..INTRO_END {
        c.step(&tick, (&[], &[]));
    }
    assert!(!c.busy());
}

#[test]
fn glyph_u_wraps_like_the_byte_store() {
    assert_eq!(glyph_u(0), 0);
    assert_eq!(glyph_u(1), 24);
    assert_eq!(glyph_u(10), 240);
    assert_eq!(glyph_u(11), 8);
}
