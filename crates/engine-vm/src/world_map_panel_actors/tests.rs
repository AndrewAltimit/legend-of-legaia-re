use super::*;

fn pad(held: u32, pressed: u32) -> CursorPad {
    CursorPad {
        held,
        pressed,
        action_a_mask: 0x20,
        action_b_mask: 0x40,
    }
}

// --- the shared exit --------------------------------------------------

/// The four stores, and the fact that `scene[+0x3E]` is not one of them:
/// the exit arms write `+0x2E`, and only the separate `case 5` arm clears
/// `+0x3E`. Writing the sentinel into the wrong halfword is the mistake
/// this pins.
#[test]
fn exit_writes_2e_and_leaves_3e_alone() {
    let (mut scene_2e, mut scene_40) = (0i16, 0u16);
    let scene_3e = 7i16;
    let (mut ctx_50, mut ctx_54) = (0x11u16, 5i16);
    ActorExit {
        saved_handler: 0x11,
        next_handler: 0x29,
    }
    .apply(&mut scene_2e, &mut scene_40, &mut ctx_50, &mut ctx_54);
    assert_eq!(scene_2e, ActorExit::SCENE_SLOT_CLEAR);
    assert_eq!(scene_3e, 7, "the exit never touches +0x3E");
    assert_eq!(ctx_50, 0x29, "+0x50 takes the arm's next handler id");
    assert_eq!(ctx_54, 0, "+0x54 is zeroed");
}

/// `scene[+0x40]` takes the actor's *old* `+0x50`, not the new one.
#[test]
fn exit_saves_the_old_handler_into_scene_40() {
    let (mut scene_2e, mut scene_40) = (0i16, 0u16);
    let (mut ctx_50, mut ctx_54) = (0x11u16, 0i16);
    ActorExit {
        saved_handler: 0x11,
        next_handler: 0x2B,
    }
    .apply(&mut scene_2e, &mut scene_40, &mut ctx_50, &mut ctx_54);
    assert_eq!(scene_40, 0x11);
    assert_eq!(ctx_50, 0x2B);
}

// --- FUN_801ED308 -----------------------------------------------------

#[test]
fn fade_flash_case0_arms_and_ramps_in_one_frame() {
    let (phase, level, _, out) = fade_flash_tick(
        0,
        FadeFlashInput {
            frame_delta: 1,
            level: 999,
            ..Default::default()
        },
    );
    assert_eq!(phase, 1);
    assert_eq!(level, BRIGHTNESS_STEP, "case 0 zeroes then case 1 ramps");
    assert_eq!(out, vec![FadeFlashEffect::ApplyBrightness(BRIGHTNESS_STEP)]);
}

#[test]
fn fade_flash_captures_the_tint_at_the_biased_threshold() {
    // 0x82 + 0x70 == 0xF2, still below the bound.
    let (phase, _, _, out) = fade_flash_tick(
        1,
        FadeFlashInput {
            frame_delta: 0,
            level: 0x82,
            ..Default::default()
        },
    );
    assert_eq!(phase, 1);
    assert_eq!(out, vec![FadeFlashEffect::ApplyBrightness(0x82)]);

    let (phase, _, _, out) = fade_flash_tick(
        1,
        FadeFlashInput {
            frame_delta: 0,
            level: 0x83,
            ..Default::default()
        },
    );
    assert_eq!(phase, 2);
    assert_eq!(out[0], FadeFlashEffect::CaptureAndClearTint);
}

#[test]
fn fade_flash_saturating_hold_runs_the_restore_arm() {
    let (phase, level, _, out) = fade_flash_tick(
        2,
        FadeFlashInput {
            frame_delta: 1,
            level: 0,
            flash_counter: FLASH_COUNTER_RESTORE,
            ..Default::default()
        },
    );
    assert_eq!(phase, 4, "case 2 falls into case 3's body");
    assert_eq!(level, BRIGHTNESS_MAX);
    assert_eq!(out[0], FadeFlashEffect::RestoreTint);
}

#[test]
fn fade_flash_rampdown_returns_to_counter_minus_one() {
    let (phase, level, counter, _) = fade_flash_tick(
        4,
        FadeFlashInput {
            frame_delta: 100,
            level: 1,
            flash_counter: 7,
            ..Default::default()
        },
    );
    assert_eq!(phase, 6);
    assert_eq!(level, 0);
    assert_eq!(counter, 0);
}

#[test]
fn fade_flash_terminal_arms_pick_different_handlers() {
    let inp = FadeFlashInput {
        handler_id: 0x11,
        ..Default::default()
    };
    let (_, _, _, six) = fade_flash_tick(6, inp);
    let (_, _, _, seven) = fade_flash_tick(7, inp);
    assert_eq!(
        six,
        vec![FadeFlashEffect::Exit(ActorExit {
            saved_handler: 0x11,
            next_handler: 0x29
        })]
    );
    assert_eq!(
        seven,
        vec![FadeFlashEffect::Exit(ActorExit {
            saved_handler: 0x11,
            next_handler: 0x2B
        })]
    );
}

#[test]
fn fade_flash_out_of_range_phase_does_nothing() {
    let (phase, _, _, out) = fade_flash_tick(8, FadeFlashInput::default());
    assert_eq!(phase, 8);
    assert!(out.is_empty());
}

// --- FUN_801ED590 -----------------------------------------------------

#[test]
fn sub_list_open_halves_the_scale_and_zeroes_the_cursor() {
    let (phase, cursor, out) = sub_list_tick(
        0,
        SubListInput {
            cursor: 5,
            ..Default::default()
        },
    );
    assert_eq!((phase, cursor), (1, 0));
    assert_eq!(out[0], SubListEffect::ScaleAudioLevel { shift_right: true });
    assert_eq!(out[1], SubListEffect::RunPanelScript(SUBLIST_OPEN_SCRIPT));
    assert_eq!(*out.last().unwrap(), SubListEffect::TickTextActors);
}

#[test]
fn sub_list_confirm_phase_is_cursor_plus_two() {
    for cursor in 0..2 {
        let (phase, _, out) = sub_list_tick(
            1,
            SubListInput {
                cursor,
                pad: pad(0x20, 0),
                ..Default::default()
            },
        );
        assert_eq!(phase, (cursor + 2) as i16);
        assert_eq!(out[0], SubListEffect::PlaySfx(SUBLIST_CONFIRM_SFX));
    }
}

#[test]
fn sub_list_cancel_always_closes() {
    let (phase, _, _) = sub_list_tick(
        1,
        SubListInput {
            cursor: 1,
            pad: pad(0x40, 0),
            ..Default::default()
        },
    );
    assert_eq!(phase, 2);
}

#[test]
fn sub_list_ignores_the_pad_while_locked() {
    let (phase, cursor, out) = sub_list_tick(
        1,
        SubListInput {
            input_locked: true,
            cursor: 1,
            pad: pad(0x20, 0),
            ..Default::default()
        },
    );
    assert_eq!((phase, cursor), (1, 1));
    assert_eq!(out, vec![SubListEffect::TickTextActors]);
}

#[test]
fn sub_list_close_arm_doubles_back_and_exits() {
    let (phase, _, out) = sub_list_tick(
        2,
        SubListInput {
            handler_id: 7,
            ..Default::default()
        },
    );
    assert_eq!(phase, 0);
    assert!(out.contains(&SubListEffect::ClearWindowDescriptor));
    assert!(out.contains(&SubListEffect::Exit(ActorExit {
        saved_handler: 7,
        next_handler: SUBLIST_NEXT_HANDLER
    })));
}

// --- FUN_801EDF00 -----------------------------------------------------

#[test]
fn soft_reset_slide_walks_down_to_the_rest_value() {
    let (mut phase, mut slide, _) = soft_reset_tick(0, SoftResetInput::default());
    assert_eq!((phase, slide), (1, SOFT_RESET_SLIDE_START));
    for _ in 0..(SOFT_RESET_SLIDE_START - SOFT_RESET_SLIDE_REST) {
        let (p, s, _) = soft_reset_tick(
            phase,
            SoftResetInput {
                slide,
                ..Default::default()
            },
        );
        phase = p;
        slide = s;
    }
    assert_eq!((phase, slide), (1, SOFT_RESET_SLIDE_REST));
}

#[test]
fn soft_reset_pad_is_only_sampled_at_rest() {
    let (phase, _, out) = soft_reset_tick(
        1,
        SoftResetInput {
            slide: 0x40,
            pad: SOFT_RESET_PAD_MASK,
            ..Default::default()
        },
    );
    assert_eq!(phase, 1);
    assert!(
        !out.iter()
            .any(|e| matches!(e, SoftResetEffect::WhiteFade { .. }))
    );

    let (phase, slide, out) = soft_reset_tick(
        1,
        SoftResetInput {
            slide: SOFT_RESET_SLIDE_REST,
            pad: SOFT_RESET_PAD_MASK,
            ..Default::default()
        },
    );
    assert_eq!((phase, slide), (2, 0));
    assert!(out.contains(&SoftResetEffect::WhiteFade {
        frames: SOFT_RESET_FADE_FRAMES
    }));
}

#[test]
fn soft_reset_reloads_on_reaching_the_bound_not_one_before() {
    let (_, _, out) = soft_reset_tick(
        2,
        SoftResetInput {
            frame_delta: 1,
            slide: SOFT_RESET_RELOAD_AT - 2,
            ..Default::default()
        },
    );
    assert!(!out.contains(&SoftResetEffect::ReloadExecutable));
    let (_, slide, out) = soft_reset_tick(
        2,
        SoftResetInput {
            frame_delta: 1,
            slide: SOFT_RESET_RELOAD_AT - 1,
            ..Default::default()
        },
    );
    assert_eq!(slide, SOFT_RESET_RELOAD_AT);
    assert!(out.contains(&SoftResetEffect::ReloadExecutable));
}

// --- FUN_801EE5D4 -----------------------------------------------------

#[test]
fn fill_fade_case0_posts_and_falls_into_the_hold() {
    let (phase, timer, out) = fill_fade_tick(
        0,
        FillFadeInput {
            frame_delta: 3,
            timer: 999,
            ..Default::default()
        },
    );
    assert_eq!(phase, 1);
    assert_eq!(timer, 3, "the hold arm ticks in the same frame");
    assert_eq!(out[0], FillFadeEffect::RunPanelScript(FILL_FADE_SCRIPT));
    assert_eq!(out[1], FillFadeEffect::PostFillPrim);
}

#[test]
fn fill_fade_hold_respects_the_input_lock() {
    let (phase, _, _) = fill_fade_tick(
        1,
        FillFadeInput {
            timer: FILL_FADE_HOLD_FRAMES,
            input_locked: true,
            ..Default::default()
        },
    );
    assert_eq!(phase, 1);
}

#[test]
fn fill_fade_load_gate_holds_phase_two() {
    let (phase, _, _) = fill_fade_tick(
        2,
        FillFadeInput {
            load_pending: true,
            ..Default::default()
        },
    );
    assert_eq!(phase, 2);
    let (phase, timer, out) = fill_fade_tick(
        2,
        FillFadeInput {
            timer: 9,
            ..Default::default()
        },
    );
    assert_eq!((phase, timer), (3, 0));
    assert!(out.contains(&FillFadeEffect::QueueDmaAndRestoreTint));
}

#[test]
fn fill_fade_every_phase_ticks_the_text_actors() {
    for phase in 1..5 {
        let (_, _, out) = fill_fade_tick(phase, FillFadeInput::default());
        assert!(
            out.contains(&FillFadeEffect::TickTextActors),
            "phase {phase}"
        );
    }
}

// --- FUN_801EE90C -----------------------------------------------------

#[test]
fn text_box_case0_jumps_to_the_fade_block() {
    let (phase, _, _, out) = text_box_tick(0, TextBoxInput::default());
    assert_eq!(phase, TEXT_BOX_FADE_PHASE);
    assert!(
        !out.contains(&TextBoxEffect::TickTextActors),
        "phase 10 is not below the tick bound"
    );
}

#[test]
fn text_box_confirm_on_row_zero_restores_the_party() {
    let (phase, _, _, out) = text_box_tick(
        1,
        TextBoxInput {
            cursor: 0,
            pad: pad(0x20, 0),
            ..Default::default()
        },
    );
    assert_eq!(phase, 2);
    assert!(out.contains(&TextBoxEffect::RestoreParty));
    assert!(out.contains(&TextBoxEffect::RunPanelScript(TEXT_BOX_CONFIRM_SCRIPT)));
}

#[test]
fn text_box_confirm_on_row_one_declines() {
    let (phase, _, _, out) = text_box_tick(
        1,
        TextBoxInput {
            cursor: 1,
            pad: pad(0x20, 0),
            ..Default::default()
        },
    );
    assert_eq!(phase, 3);
    assert!(out.contains(&TextBoxEffect::RunPanelScript(TEXT_BOX_DECLINE_SCRIPT)));
    assert!(!out.contains(&TextBoxEffect::RestoreParty));
}

#[test]
fn text_box_dismiss_needs_both_the_unlock_and_the_press() {
    let base = TextBoxInput {
        handler_id: 3,
        ..Default::default()
    };
    assert_eq!(text_box_tick(2, base).0, 2);
    let (phase, _, _, out) = text_box_tick(
        2,
        TextBoxInput {
            dismiss_pressed: true,
            ..base
        },
    );
    assert_eq!(phase, 0);
    assert!(out.iter().any(|e| matches!(e, TextBoxEffect::Exit(_))));
    assert_eq!(
        text_box_tick(
            2,
            TextBoxInput {
                dismiss_pressed: true,
                input_locked: true,
                ..base
            }
        )
        .0,
        2
    );
}

#[test]
fn text_box_idle_phases_only_tick_text_actors() {
    for phase in 4..10 {
        let (p, _, _, out) = text_box_tick(phase, TextBoxInput::default());
        assert_eq!(p, phase);
        assert_eq!(out, vec![TextBoxEffect::TickTextActors], "phase {phase}");
    }
}

#[test]
fn text_box_fade_block_posts_the_prim_and_resets_the_timer() {
    // Phase 10 zeroes the timer that phase 11's body then ticks, so a
    // large incoming timer must NOT let it skip a frame.
    let (phase, _, timer, out) = text_box_tick(
        10,
        TextBoxInput {
            fade: FillFadeInput {
                timer: 999,
                frame_delta: 1,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    assert_eq!((phase, timer), (11, 1));
    assert_eq!(out, vec![TextBoxEffect::Fade(FillFadeEffect::PostFillPrim)]);
}

#[test]
fn text_box_fade_hold_ignores_the_input_lock() {
    // FUN_801EE5D4's hold arm gates on `_DAT_8007BB80`; the dispatcher's
    // copy does not, and that difference is the reason the two blocks are
    // written out separately.
    let (phase, _, _, _) = text_box_tick(
        11,
        TextBoxInput {
            input_locked: true,
            fade: FillFadeInput {
                timer: FILL_FADE_HOLD_FRAMES,
                input_locked: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    assert_eq!(phase, 12);
}

#[test]
fn text_box_fade_block_walks_to_the_terminal_case() {
    let held = FillFadeInput {
        timer: FILL_FADE_HOLD_FRAMES,
        ..Default::default()
    };
    for (from, to) in [(11i16, 12i16), (12, 13), (13, 14)] {
        let (phase, _, _, _) = text_box_tick(
            from,
            TextBoxInput {
                fade: held,
                ..Default::default()
            },
        );
        assert_eq!(phase, to, "phase {from}");
    }
    let (_, _, _, out) = text_box_tick(14, TextBoxInput::default());
    assert_eq!(out, vec![TextBoxEffect::ClearSceneField3E]);
}

// --- FUN_801EF014 -----------------------------------------------------

fn fw_desc() -> FlagWindowDescriptor {
    FlagWindowDescriptor {
        count: 8,
        first_visible: 0,
        rows: 4,
        base_flag: 0x138,
    }
}

#[test]
fn flag_window_panel_grows_from_a_fixed_bottom_edge() {
    for rows in 1u8..=8 {
        let (y, h) = flag_window_panel_geometry(rows);
        assert_eq!(
            y + h,
            FLAG_WINDOW_PANEL_ROWS * 16 + FLAG_WINDOW_PANEL_Y_BASE
        );
    }
}

#[test]
fn flag_window_row_flip_is_an_involution() {
    let d = fw_desc();
    for v in -4..12 {
        assert_eq!(flag_window_row_flip(flag_window_row_flip(v, d), d), v);
    }
}

#[test]
fn flag_window_scan_falls_back_to_first_visible() {
    let d = FlagWindowDescriptor {
        first_visible: 3,
        ..fw_desc()
    };
    let (phase, sel, remembered, out) = flag_window_tick(
        0,
        FlagWindowInput {
            desc: d,
            ..Default::default()
        },
        |_| false,
    );
    assert_eq!(phase, 1);
    assert_eq!(remembered, i32::from(d.count));
    assert_eq!(sel, i32::from(d.first_visible));
    assert!(out.contains(&FlagWindowEffect::ClearRange {
        base: d.base_flag,
        count: d.count
    }));
}

#[test]
fn flag_window_scan_finds_the_first_set_flag() {
    let d = fw_desc();
    let (_, sel, remembered, _) = flag_window_tick(
        0,
        FlagWindowInput {
            desc: d,
            ..Default::default()
        },
        |f| f == d.base_flag + 5,
    );
    assert_eq!((sel, remembered), (5, 5));
}

#[test]
fn flag_window_confirm_on_the_remembered_row_sets_nothing() {
    let d = fw_desc();
    let (phase, sel, _, out) = flag_window_tick(
        1,
        FlagWindowInput {
            desc: d,
            selection: 2,
            remembered: 2,
            pad: pad(0x20, 0),
            ..Default::default()
        },
        |_| false,
    );
    assert_eq!(phase, 3);
    assert_eq!(sel, FLAG_WINDOW_CANCEL_SENTINEL);
    assert!(
        !out.iter()
            .any(|e| matches!(e, FlagWindowEffect::SetFlag(_)))
    );
}

#[test]
fn flag_window_confirm_on_a_new_row_commits_the_flag() {
    let d = fw_desc();
    let (phase, _, _, out) = flag_window_tick(
        1,
        FlagWindowInput {
            desc: d,
            selection: 2,
            remembered: 0,
            pad: pad(0x20, 0),
            ..Default::default()
        },
        |_| false,
    );
    assert_eq!(phase, 2);
    assert!(out.contains(&FlagWindowEffect::SetFlag(d.base_flag + 2)));
}

#[test]
fn flag_window_cursor_moves_against_the_inverted_row_space() {
    let d = fw_desc();
    // Screen row for selection 2 is 4 - (2 - 0) - 1 = 1; pressing Down
    // moves the screen row to 2, which is selection 1.
    let (phase, sel, _, _) = flag_window_tick(
        1,
        FlagWindowInput {
            desc: d,
            selection: 2,
            remembered: 9,
            pad: pad(0, crate::world_map_panel::PAD_DOWN),
            ..Default::default()
        },
        |_| false,
    );
    assert_eq!(phase, 1);
    assert_eq!(sel, 1);
}

#[test]
fn flag_window_cancel_writes_the_sentinel() {
    let d = fw_desc();
    let (phase, sel, _, _) = flag_window_tick(
        1,
        FlagWindowInput {
            desc: d,
            selection: 2,
            pad: pad(0x40, 0),
            ..Default::default()
        },
        |_| false,
    );
    assert_eq!((phase, sel), (3, FLAG_WINDOW_CANCEL_SENTINEL));
}

// --- FUN_801D0D38 -----------------------------------------------------

#[test]
fn hud_is_suppressed_by_each_gate_in_turn() {
    let base = HudInput {
        player_stationary: true,
        timer: 1,
        ..Default::default()
    };
    for gated in [
        HudInput {
            hud_disabled: true,
            ..base
        },
        HudInput {
            view_mode: 2,
            ..base
        },
        HudInput {
            pad: HUD_SUPPRESS_PAD_MASK,
            ..base
        },
        HudInput {
            scratch_suppress: true,
            ..base
        },
    ] {
        assert_eq!(field_hud_tick(gated).1, HudDecision::Suppressed);
    }
}

#[test]
fn hud_rearm_picks_the_view_mode_idle() {
    assert_eq!(
        field_hud_tick(HudInput {
            rearm: true,
            view_mode: 0,
            ..Default::default()
        })
        .1,
        HudDecision::Rearmed {
            timer: HUD_IDLE_FRAMES_NEAR
        }
    );
    assert_eq!(
        field_hud_tick(HudInput {
            rearm: true,
            view_mode: 1,
            short_idle: true,
            ..Default::default()
        })
        .1,
        HudDecision::Rearmed {
            timer: HUD_IDLE_FRAMES_FAR_SHORT
        }
    );
}

#[test]
fn hud_movement_rearms_ignoring_the_short_idle_marker() {
    let d = field_hud_tick(HudInput {
        view_mode: 1,
        short_idle: true,
        player_stationary: false,
        ..Default::default()
    })
    .1;
    assert_eq!(
        d,
        HudDecision::Rearmed {
            timer: HUD_IDLE_FRAMES_FAR
        }
    );
}

#[test]
fn hud_draws_once_the_countdown_expires() {
    let (timer, d) = field_hud_tick(HudInput {
        player_stationary: true,
        timer: 1,
        timer_delta: 1,
        projected_y: Some(0x40),
        ..Default::default()
    });
    assert_eq!(timer, 0);
    assert_eq!(d, HudDecision::Draw { y: HUD_Y_TOP });
}

#[test]
fn hud_dodges_a_player_near_the_screen_top() {
    let (_, d) = field_hud_tick(HudInput {
        player_stationary: true,
        timer: 0,
        timer_delta: 0,
        projected_y: Some(HUD_DODGE_THRESHOLD - 1),
        ..Default::default()
    });
    assert_eq!(d, HudDecision::Draw { y: HUD_Y_BOTTOM });
    let (_, d) = field_hud_tick(HudInput {
        player_stationary: true,
        timer: 0,
        timer_delta: 0,
        projected_y: None,
        ..Default::default()
    });
    assert_eq!(d, HudDecision::Draw { y: HUD_Y_BOTTOM });
}
