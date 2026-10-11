use super::*;

fn globals() -> TransitionGlobals {
    TransitionGlobals {
        battle_id: 4,
        total_duration: 200,
        ..Default::default()
    }
}

#[test]
fn phase_zero_fires_the_cue_and_advances() {
    let mut e = TransitionEntity::default();
    let g = TransitionGlobals {
        bgm_voice_id: 0x12,
        audio_level: 4,
        ..globals()
    };
    let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
    assert_eq!(e.phase, 1);
    assert_eq!(
        tick.effects,
        vec![
            TransitionEffect::SetAudioCue {
                cue: AUDIO_CUE_FLAGGED
            },
            TransitionEffect::ReapplyBgmLevel {
                voice_id: 0x12,
                // 4 << 15 == 0x20000; >> 16 == 2 - the retail halve into
                // libsnd's 0..0x7F volume range.
                level: 2
            }
        ]
    );
}

#[test]
fn phase_zero_sentinel_id_arms_the_cue_then_advances_without_playing() {
    let mut e = TransitionEntity::default();
    let g = TransitionGlobals {
        battle_id: -1,
        battle_flags: 0x80,
        ..globals()
    };
    let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
    assert_eq!(e.phase, 1);
    assert_eq!(
        tick.effects,
        vec![
            TransitionEffect::SetAudioCue {
                cue: AUDIO_CUE_PLAIN
            },
            TransitionEffect::SetAudioCue {
                cue: AUDIO_CUE_FLAGGED
            },
        ],
        "the delay-slot store lands, then the flagged one overwrites it"
    );
}

#[test]
fn phase_zero_other_negative_id_does_nothing_but_advance() {
    let mut e = TransitionEntity::default();
    let g = TransitionGlobals {
        battle_id: -5,
        ..globals()
    };
    let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
    assert_eq!(e.phase, 1);
    assert!(tick.effects.is_empty(), "blez arm skips both stores");
}

#[test]
fn phase_one_holds_until_assembly_reports_0x80() {
    let mut e = TransitionEntity {
        phase: 1,
        ..Default::default()
    };
    let tick = tick_transition(&mut e, &globals(), &TransitionResponses::default());
    assert_eq!(e.phase, 1, "holds");
    assert_eq!(tick.effects, vec![TransitionEffect::AssembleBattleMeshes]);

    let g = TransitionGlobals {
        assembly_state: 0x80,
        ..globals()
    };
    let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
    assert_eq!(e.phase, 2);
    assert_eq!(
        tick.effects,
        vec![
            TransitionEffect::AssembleBattleMeshes,
            TransitionEffect::WaitForLoad
        ]
    );
}

#[test]
fn phase_two_bundle_id_arms() {
    let mut e = TransitionEntity {
        phase: 2,
        ..Default::default()
    };
    let tick = tick_transition(&mut e, &globals(), &TransitionResponses::default());
    assert!(tick.effects.contains(&TransitionEffect::LoadBattleBundle {
        bundle_id: 0x36F + 4,
        flags: BATTLE_BUNDLE_FLAGS,
    }));

    // battle_id 0 with the alt flag set picks 0x370.
    let mut e = TransitionEntity {
        phase: 2,
        ..Default::default()
    };
    let g = TransitionGlobals {
        battle_id: 0,
        alt_default_bundle: 1,
        ..globals()
    };
    let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
    assert!(tick.effects.contains(&TransitionEffect::LoadBattleBundle {
        bundle_id: 0x370,
        flags: BATTLE_BUNDLE_FLAGS,
    }));

    // A negative id loads the BGM but no bundle.
    let mut e = TransitionEntity {
        phase: 2,
        ..Default::default()
    };
    let g = TransitionGlobals {
        battle_id: -3,
        ..globals()
    };
    let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
    assert_eq!(tick.effects.len(), 1);
    assert_eq!(e.phase, 3);
}

#[test]
fn wait_phases_hold_while_busy() {
    for phase in [3i16, 6] {
        let mut e = TransitionEntity {
            phase,
            ..Default::default()
        };
        tick_transition(&mut e, &globals(), &TransitionResponses { load_busy: 1 });
        assert_eq!(e.phase, phase);
        tick_transition(&mut e, &globals(), &TransitionResponses::default());
        assert_eq!(e.phase, phase + 1);
    }
}

#[test]
fn phase_seven_sets_ready_bit_and_does_not_advance() {
    let mut e = TransitionEntity {
        phase: 7,
        ..Default::default()
    };
    tick_transition(&mut e, &globals(), &TransitionResponses::default());
    assert_eq!(e.phase, 7);
    assert_eq!(e.ready, 2);
}

#[test]
fn out_of_range_phase_runs_no_body() {
    let mut e = TransitionEntity {
        phase: 9,
        ..Default::default()
    };
    let tick = tick_transition(&mut e, &globals(), &TransitionResponses::default());
    assert_eq!(e.phase, 9);
    assert!(tick.effects.is_empty());
}

#[test]
fn ready_bit_zero_raised_near_the_end_of_the_spin() {
    let g = TransitionGlobals {
        total_duration: 100,
        ..globals()
    };
    let mut e = TransitionEntity {
        phase: 4,
        elapsed: 70,
        ..Default::default()
    };
    tick_transition(&mut e, &g, &TransitionResponses::default());
    assert_eq!(e.ready, 0, "100 - 0x1E == 70, not < 70");

    e.elapsed = 71;
    tick_transition(&mut e, &g, &TransitionResponses::default());
    assert_eq!(e.ready, 1);
}

/// The entity state the handoff tail needs: rested at phase 7 (ready bit
/// 1) with the clock past the duration (ready bit 0).
fn at_completion(total: i32, elapsed: i16) -> TransitionEntity {
    assert!(i32::from(elapsed) > total, "test setup must clear the gate");
    TransitionEntity {
        phase: 7,
        elapsed,
        ready: 3,
    }
}

#[test]
fn the_handoff_needs_both_ready_bits_not_just_either() {
    let g = TransitionGlobals {
        total_duration: 100,
        ..globals()
    };
    // Bit 1 without bit 0: rested at phase 7, but the spin clock has not
    // passed `total - 0x1E`, so the post-switch never raises bit 0.
    let mut e = TransitionEntity {
        phase: 7,
        elapsed: 50,
        ..Default::default()
    };
    let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
    assert_eq!(e.ready, 2);
    assert!(!tick.entered_battle_mode);

    // Bit 0 without bit 1: the clock is spent but the machine is still
    // walking the phases, so phase 7's arm has not run.
    let mut e = TransitionEntity {
        phase: 4,
        elapsed: 200,
        ..Default::default()
    };
    let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
    assert_eq!(e.ready, 1);
    assert!(!tick.entered_battle_mode);

    // Both: the tail reads `ready` *after* the switch and the post-switch
    // OR, so a phase-7 tick with a spent clock raises both bits and
    // completes inside the same tick rather than on the next one.
    let mut e = TransitionEntity {
        phase: 7,
        elapsed: 200,
        ..Default::default()
    };
    let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
    assert_eq!(e.ready, 3);
    assert!(tick.entered_battle_mode);
}

#[test]
fn the_mode_write_lands_even_for_a_negative_battle_id() {
    // The store is in the delay slot of the id test, so only the audio
    // handoff behind it is skipped.
    let g = TransitionGlobals {
        total_duration: 100,
        battle_id: -1,
        ..globals()
    };
    let mut e = at_completion(100, 200);
    let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
    assert!(tick.entered_battle_mode);
    assert!(
        !tick
            .effects
            .iter()
            .any(|f| matches!(f, TransitionEffect::StartBattleBgmVoice)),
        "a negative id skips the whole audio handoff"
    );
}

#[test]
fn the_bgm_swap_runs_in_retail_order() {
    let g = TransitionGlobals {
        total_duration: 100,
        battle_id: 4,
        bundle_handle: 0xDEAD_BEEF,
        ..globals()
    };
    let mut e = at_completion(100, 200);
    let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
    assert_eq!(
        tick.effects,
        vec![
            TransitionEffect::StopFieldBgmVoice,
            TransitionEffect::ReleaseFieldBgmSlot,
            TransitionEffect::InstallBattleSequence {
                bundle_handle: 0xDEAD_BEEF
            },
            TransitionEffect::StartBattleBgmVoice,
        ],
        "stop the field voice, release its slot, install the battle SEQ, start it"
    );
}

#[test]
fn the_field_voice_stops_six_frames_before_the_handoff() {
    let g = TransitionGlobals {
        total_duration: 100,
        battle_id: 0,
        ..globals()
    };
    // 100 - 6 == 94: not `< 94`, so nothing yet.
    let mut e = TransitionEntity {
        phase: 7,
        elapsed: 94,
        ready: 3,
    };
    let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
    assert!(tick.effects.is_empty());
    // One frame later the stop fires, and the mode write still does not.
    let mut e = TransitionEntity {
        phase: 7,
        elapsed: 95,
        ready: 3,
    };
    let tick = tick_transition(&mut e, &g, &TransitionResponses::default());
    assert_eq!(tick.effects, vec![TransitionEffect::StopFieldBgmVoice]);
    assert!(!tick.entered_battle_mode);
}

#[test]
fn one_formation_resets_the_battle_id_and_the_others_do_not() {
    let mk = |slot0: u8| {
        let g = TransitionGlobals {
            total_duration: 100,
            battle_id: 4,
            formation_slot0: slot0,
            ..globals()
        };
        let mut e = at_completion(100, 200);
        tick_transition(&mut e, &g, &TransitionResponses::default()).cleared_battle_id
    };
    assert!(mk(FORMATION_RESET_BATTLE_ID));
    assert!(!mk(FORMATION_RESET_BATTLE_ID.wrapping_sub(1)));
    assert!(!mk(0));
}

fn desc() -> IntroQuadDesc {
    IntroQuadDesc {
        size_q12: 0x1000,
        tpage: 0x0020,
        clut: 0x1234,
        u0: 8,
        v0: 16,
        w: 32,
        h: 64,
        top: [0xFF, 0x80, 0x00],
        abr: 1,
        bottom: [0x00, 0x40, 0xFF],
        tpage_step: 3,
    }
}

fn req(anchor: IntroQuadAnchor, x: i16, y: i16, key: i32, intensity: i32) -> IntroQuadRequest {
    IntroQuadRequest {
        anchor,
        x,
        y,
        key,
        intensity,
        scale_x: 0x1000,
        scale_y: 0x1000,
        ot_depth: 3,
    }
}

#[test]
fn quad_top_left_anchor_and_uvs() {
    let table = [desc()];
    let q = build_intro_quad(
        &IntroQuadRequest {
            ot_depth: 5,
            ..req(IntroQuadAnchor::TopLeft, 10, 20, 0, 0x100)
        },
        &table,
    )
    .unwrap();
    assert_eq!((q.verts[0].x, q.verts[0].y), (10, 20));
    assert_eq!((q.verts[3].x, q.verts[3].y), (42, 84));
    assert_eq!((q.verts[0].u, q.verts[0].v), (8, 16));
    assert_eq!((q.verts[1].u, q.verts[1].v), (40, 16));
    assert_eq!((q.verts[2].u, q.verts[2].v), (8, 80));
    assert_eq!((q.verts[3].u, q.verts[3].v), (40, 80));
    assert_eq!(q.ot_depth, 5);
}

#[test]
fn quad_centre_anchor_halves_the_extents() {
    let table = [desc()];
    let q = build_intro_quad(&req(IntroQuadAnchor::Centre, 100, 100, 0, 0x100), &table).unwrap();
    assert_eq!((q.verts[0].x, q.verts[0].y), (84, 68));
    assert_eq!((q.verts[3].x, q.verts[3].y), (116, 132));
}

#[test]
fn quad_gradient_is_top_then_bottom() {
    let table = [desc()];
    let q = build_intro_quad(&req(IntroQuadAnchor::TopLeft, 0, 0, 0, 0x80), &table).unwrap();
    // Half intensity.
    assert_eq!(q.verts[0].rgb, [0x7F, 0x40, 0x00]);
    assert_eq!(q.verts[1].rgb, q.verts[0].rgb);
    assert_eq!(q.verts[2].rgb, [0x00, 0x20, 0x7F]);
    assert_eq!(q.verts[3].rgb, q.verts[2].rgb);
}

#[test]
fn mode_zero_uses_descriptor_abr_and_step() {
    let table = [desc()];
    let q = build_intro_quad(&req(IntroQuadAnchor::TopLeft, 0, 0, 0, 0x100), &table).unwrap();
    assert_eq!(q.code, (1 << 1) | POLY_GT4_CODE);
    assert_eq!(q.tpage, 0x20 + (3 << 5));
    assert_eq!(q.clut, 0x1234);
}

#[test]
fn non_zero_mode_forces_abr_one_and_uses_mode_as_step() {
    let table = [desc()];
    // mode 2: abr forced to 1, step 2, CLUT replaced.
    let q = build_intro_quad(&req(IntroQuadAnchor::TopLeft, 0, 0, 2 << 10, 0x100), &table).unwrap();
    assert_eq!(q.code, (1 << 1) | POLY_GT4_CODE);
    assert_eq!(q.tpage, 0x20 + (2 << 5));
    assert_eq!(q.clut, INTRO_QUAD_MODE2_CLUT);
}

#[test]
fn desc_parse_round_trips_field_offsets() {
    let mut raw = [0u8; INTRO_QUAD_DESC_STRIDE];
    raw[0..4].copy_from_slice(&0x1000i32.to_le_bytes());
    raw[4..6].copy_from_slice(&0x0020u16.to_le_bytes());
    raw[6..8].copy_from_slice(&0x1234u16.to_le_bytes());
    raw[8] = 8;
    raw[9] = 16;
    raw[0x0a] = 32;
    raw[0x0b] = 64;
    raw[0x0c..0x0f].copy_from_slice(&[0xFF, 0x80, 0x00]);
    raw[0x0f] = 1;
    raw[0x10..0x13].copy_from_slice(&[0x00, 0x40, 0xFF]);
    raw[0x13] = 3;
    assert_eq!(IntroQuadDesc::parse(&raw), Some(desc()));
    assert_eq!(IntroQuadDesc::parse(&raw[..8]), None);
}
