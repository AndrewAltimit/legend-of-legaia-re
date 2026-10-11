use super::*;

#[test]
fn every_gate_condition_is_independently_fatal() {
    assert!(attack_camera_gate(100, CATEGORY_ATTACK, PHASE_ACTIVE, 0));
    assert!(
        !attack_camera_gate(0, CATEGORY_ATTACK, PHASE_ACTIVE, 0),
        "dead target"
    );
    assert!(
        !attack_camera_gate(100, 2, PHASE_ACTIVE, 0),
        "magic, not attack"
    );
    assert!(
        !attack_camera_gate(100, CATEGORY_ATTACK, 0x14, 0),
        "wrong phase"
    );
    assert!(
        !attack_camera_gate(100, CATEGORY_ATTACK, PHASE_ACTIVE, 3),
        "monster slot"
    );
    assert!(!attack_camera_gate(100, CATEGORY_ATTACK, PHASE_ACTIVE, 7));
}

#[test]
fn only_three_participant_ids_have_camera_scripts() {
    assert_eq!(character_arm(1), Some(CharacterArm::One));
    assert_eq!(character_arm(2), Some(CharacterArm::Two));
    assert_eq!(character_arm(3), Some(CharacterArm::Three));
    for id in [0u8, 4, 5, 0xFF] {
        assert_eq!(character_arm(id), None, "id {id}");
    }
}

/// The three characters have three different tables, and the bound is
/// per character: `2` accepts `0x2C`/`0x2D`, which `1` and `3` reject.
#[test]
fn each_character_dispatches_through_its_own_art_table() {
    use CharacterArm::*;
    assert_eq!(ART_JUMP_TABLES[One as usize].len(), 0x11);
    assert_eq!(ART_JUMP_TABLES[Two as usize].len(), 0x14);
    assert_eq!(ART_JUMP_TABLES[Three as usize].len(), 0x11);

    assert_eq!(art_arm(One, 0x1A), Some(0x801D_7308));
    assert_eq!(art_arm(Two, 0x1A), Some(0x801D_76EC));
    assert_eq!(art_arm(Three, 0x1A), Some(0x801D_7B4C));
    // Art 0x1C exists for 1 and 3 but is an epilogue slot for 2.
    assert!(art_arm(One, 0x1C).is_some());
    assert_eq!(art_arm(Two, 0x1C), None);
    assert!(art_arm(Three, 0x1C).is_some());
    // The three ids only character 2 can reach.
    for id in [0x2B, 0x2C, 0x2D] {
        assert_eq!(art_arm(One, id), None, "id {id:#x} past char 1's bound");
        assert_eq!(art_arm(Three, id), None, "id {id:#x} past char 3's bound");
    }
    assert_eq!(art_arm(Two, 0x2C), art_arm(Two, 0x1D), "0x2C shares 0x1D");
    assert_eq!(art_arm(Two, 0x2D), art_arm(Two, 0x1E), "0x2D shares 0x1E");
    assert_eq!(art_arm(Two, 0x2B), None, "an epilogue slot inside range");
}

/// The bias is unsigned, so an id below `0x1A` wraps past every table.
#[test]
fn art_ids_below_the_bias_are_rejected() {
    for c in [CharacterArm::One, CharacterArm::Two, CharacterArm::Three] {
        for id in [0u8, 1, 0x19] {
            assert_eq!(art_arm(c, id), None, "{c:?} id {id:#x}");
        }
    }
}

/// Thirteen distinct arm bodies exist across the three tables, and every
/// one has a track-fold row (possibly empty).
#[test]
fn every_dispatched_arm_has_a_track_map() {
    let mut bodies = std::collections::BTreeSet::new();
    for table in ART_JUMP_TABLES {
        bodies.extend(table.iter().flatten().copied());
    }
    assert_eq!(bodies.len(), 13, "distinct arm bodies");
    for va in &bodies {
        assert!(arm_tracks(*va).is_some(), "arm {va:#010x} has no track map");
    }
    // And the map has no rows for bodies that are not dispatched.
    for arm in ARM_TRACKS {
        assert!(bodies.contains(&arm.entry_va), "{:#010x}", arm.entry_va);
    }
}

/// The track map covers the whole table: every row `0..20` is read by at
/// least one arm. That density is what pins the parser's extent.
#[test]
fn the_track_map_uses_every_row_of_the_table() {
    use legaia_game_tables::battle_attack_camera_table::ATTACK_CAMERA_ROWS;
    let mut used = std::collections::BTreeSet::new();
    for arm in ARM_TRACKS {
        for t in arm.tracks {
            assert!(t.row < ATTACK_CAMERA_ROWS, "row {} out of table", t.row);
            used.insert(t.row);
        }
    }
    assert_eq!(
        used.len(),
        ATTACK_CAMERA_ROWS,
        "unused rows: {:?}",
        (0..ATTACK_CAMERA_ROWS)
            .filter(|r| !used.contains(r))
            .collect::<Vec<_>>()
    );
}

/// A fold lands in the component the arm's `sh` names and wraps at 16
/// bits, and exactly one fold in the whole map subtracts.
#[test]
fn track_folds_hit_the_named_component() {
    let seed = CameraPose::seed([0, 0, 0], 0);
    let fold = |slot, subtract, v| {
        let mut p = seed;
        apply_track(
            &mut p,
            ArmTrack {
                row: 0,
                slot,
                subtract,
                site_va: 0,
            },
            v,
        );
        p
    };
    assert_eq!(fold(PoseSlot::Pitch, false, 0x100).rot[0], 0x100);
    assert_eq!(fold(PoseSlot::Yaw, false, 0x100).rot[1], 0x100);
    assert_eq!(fold(PoseSlot::Dist0, false, 0x100).dist[0], 0x100);
    assert_eq!(
        fold(PoseSlot::Dist2, false, 0x100).dist[2],
        POSE_SEED_ANGLE + 0x100
    );
    assert_eq!(
        fold(PoseSlot::Dist2, true, 0x100).dist[2],
        POSE_SEED_ANGLE - 0x100
    );
    // Halfword wrap, not saturation.
    assert_eq!(fold(PoseSlot::Yaw, false, i16::MAX).rot[1], i16::MAX);
    let mut p = fold(PoseSlot::Yaw, false, i16::MAX);
    apply_track(
        &mut p,
        ArmTrack {
            row: 0,
            slot: PoseSlot::Yaw,
            subtract: false,
            site_va: 0,
        },
        1,
    );
    assert_eq!(p.rot[1], i16::MIN);

    let subtracting: Vec<u32> = ARM_TRACKS
        .iter()
        .flat_map(|a| a.tracks.iter())
        .filter(|t| t.subtract)
        .map(|t| t.site_va)
        .collect();
    assert_eq!(subtracting, vec![0x801D_7934, 0x801D_7B84]);
}

/// The phase cursor selects the row's column, so the two swing phases
/// pull different offsets out of one row.
#[test]
fn arm_phase_reads_the_cursor_column() {
    use legaia_game_tables::battle_attack_camera_table::{
        self as tbl, ATTACK_CAMERA_FILE_OFFSET, ATTACK_CAMERA_LEN,
    };
    let mut buf = vec![0u8; ATTACK_CAMERA_FILE_OFFSET + ATTACK_CAMERA_LEN];
    // Row 3, phase 0 = -256; row 3, phase 1 = 512.
    let o = ATTACK_CAMERA_FILE_OFFSET + 3 * 4;
    buf[o..o + 2].copy_from_slice(&(-256i16).to_le_bytes());
    buf[o + 2..o + 4].copy_from_slice(&512i16.to_le_bytes());
    let table = tbl::parse(&buf).expect("parses");
    assert_eq!(ArmPhase { cursor: 0 }.track_value(&table, 3), Some(-256));
    assert_eq!(ArmPhase { cursor: 1 }.track_value(&table, 3), Some(512));
    assert_eq!(ArmPhase { cursor: 2 }.track_value(&table, 3), None);
}

#[test]
fn pose_seed_negates_the_position_and_the_facing() {
    let p = CameraPose::seed([100, 200, 300], 0x800);
    assert_eq!(p.look_at, [-100, -200, -300]);
    assert_eq!(p.rot, [0, -0x800, 0]);
    assert_eq!(p.dist, [0, POSE_SEED_ANGLE, POSE_SEED_ANGLE]);
}

#[test]
fn pose_seed_negation_wraps_at_the_halfword_boundary() {
    // The actor position is read with `lhu` and negated with `subu`, so a
    // coordinate above 0x7FFF comes back as a positive i16.
    let p = CameraPose::seed([0x8000, 0, 0], 0);
    assert_eq!(p.look_at[0], -0x8000i32 as i16);
    let p = CameraPose::seed([0xFFFF, 0, 0], 0);
    assert_eq!(p.look_at[0], 1);
}

#[test]
fn anim_push_skips_below_the_floor_and_clamps_above_the_cap() {
    assert_eq!(anim_push(0), None);
    assert_eq!(anim_push(ANIM_PUSH_FLOOR - 1), None);
    // At the floor the push is (0x61 - 0x60) << 4 = 0x10.
    assert_eq!(anim_push(ANIM_PUSH_FLOOR), Some(0x10));
    // 0x70 - 0x60 = 0x10, << 4 = 0x100, exactly at the cap and kept.
    assert_eq!(anim_push(0x70), Some(0x100));
    // One frame later the scale overshoots and the clamp bites.
    assert_eq!(anim_push(0x71), Some(ANIM_PUSH_CAP));
    assert_eq!(anim_push(0x7FF), Some(ANIM_PUSH_CAP));
}

#[test]
fn push_split_differs_between_the_two_phase_arms() {
    let base = CameraPose::seed([0, 0, 0], 0);

    let mut a = base;
    apply_anim_push(&mut a, 1, 0x100);
    assert_eq!(a.dist[1], POSE_SEED_ANGLE - 0x100);
    assert_eq!(a.rot[1], 0x80);
    assert_eq!(a.dist[2], POSE_SEED_ANGLE - 0x40);
    assert_eq!(a.dist[0], 0x100);

    let mut b = base;
    apply_anim_push(&mut b, 0, 0x100);
    assert_eq!(b.dist[1], POSE_SEED_ANGLE - 0x100);
    assert_eq!(b.rot[1], -0x100);
    assert_eq!(b.dist[2], POSE_SEED_ANGLE - 0x80);
    assert_eq!(b.dist[0], 0, "the zero-phase arm leaves dist[0] alone");
}

#[test]
fn push_of_zero_still_writes_the_shared_slot() {
    // The store at 0x801D7394 precedes the branch, so it happens even for a
    // push the arms would otherwise ignore.
    let mut p = CameraPose::seed([0, 0, 0], 0);
    apply_anim_push(&mut p, 1, 0);
    assert_eq!(p.dist[1], POSE_SEED_ANGLE);
}

#[test]
fn track_offsets_step_by_two_per_phase_and_four_per_track() {
    let p = ArmPhase { cursor: 0 };
    assert_eq!(p.track_offset(0), 0);
    assert_eq!(p.track_offset(1), 4);
    assert_eq!(p.track_offset(2), 8);
    assert_eq!(p.track_offset(3), 0xC);
    assert_eq!(p.track_offset(5), 0x14);
    let q = ArmPhase { cursor: 3 };
    assert_eq!(q.track_offset(0), 6);
    assert_eq!(q.track_offset(1), 10);
}

#[test]
fn thresholds_are_ordered() {
    assert!(ANIM_THRESHOLDS.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(ANIM_THRESHOLDS[0], ANIM_PUSH_FLOOR);
}

// -- the executable arms ------------------------------------------------

/// A table whose row `r` reads `(r * 16, r * 16 + 1)` - distinguishable
/// per row AND per cursor, so a test can tell which row an arm folded and
/// which column it took.
fn probe_table() -> legaia_game_tables::battle_attack_camera_table::AttackCameraTracks {
    use legaia_game_tables::battle_attack_camera_table as tbl;
    let mut buf = vec![0u8; tbl::ATTACK_CAMERA_FILE_OFFSET + tbl::ATTACK_CAMERA_LEN];
    for r in 0..tbl::ATTACK_CAMERA_ROWS {
        for c in 0..tbl::ATTACK_CAMERA_PHASES {
            let v = (r as i16 * 16 + c as i16).to_le_bytes();
            let o = tbl::ATTACK_CAMERA_FILE_OFFSET + r * 4 + c * 2;
            buf[o] = v[0];
            buf[o + 1] = v[1];
        }
    }
    tbl::parse(&buf).expect("probe table parses")
}

fn probe_actor(character: CharacterArm, art_id: u8) -> AttackCamActor {
    AttackCamActor {
        character,
        art_id,
        arm_select: 0,
        anim_frame: 0,
        pos: [0, 0, 0],
        facing: 0,
    }
}

/// The ramp is a per-display-frame `+8` saturating at `0xC8`; the
/// accumulator takes the same increment and never saturates.
#[test]
fn the_ramps_advance_by_eight_per_frame_and_only_one_saturates() {
    let mut ctx = AttackCamCtx::default();
    ctx.advance(1);
    assert_eq!((ctx.ramp, ctx.accum), (8, 8));
    for _ in 0..100 {
        ctx.advance(1);
    }
    assert_eq!(ctx.ramp, AttackCamCtx::RAMP_CAP, "ramp saturates");
    assert_eq!(ctx.accum, 808, "the accumulator keeps going");
    // A frame step of 2 (a dropped frame) doubles both increments.
    let mut ctx = AttackCamCtx::default();
    ctx.advance(2);
    assert_eq!((ctx.ramp, ctx.accum), (16, 16));
}

/// The latch fires once per crossing, and only for its own expected value.
#[test]
fn the_latch_reset_fires_once_per_threshold_crossing() {
    let mut ctx = AttackCamCtx {
        ramp: 40,
        accum: 400,
        latch: 0,
        phase_cursor: 0,
        death_ramp: 0,
    };
    ctx.latch_reset(1, false);
    assert_eq!(ctx.ramp, 40, "a latch of 0 does not answer to `expect = 1`");
    ctx.latch_reset(0, false);
    assert_eq!((ctx.ramp, ctx.latch, ctx.accum), (0, 1, 400));
    ctx.ramp = 40;
    ctx.latch_reset(0, false);
    assert_eq!(ctx.ramp, 40, "the latch has moved on");
    ctx.latch_reset(1, true);
    assert_eq!((ctx.ramp, ctx.latch, ctx.accum), (0, 2, 0), "clears accum");
}

/// The cursor is a coin flip, so `begin_action` seeds `0` or `1` and
/// wipes the ramps the previous action left behind.
#[test]
fn begin_action_rolls_the_cursor_and_clears_the_ramps() {
    let mut ctx = AttackCamCtx {
        ramp: 200,
        accum: 9999,
        latch: 2,
        phase_cursor: 1,
        death_ramp: 0,
    };
    ctx.begin_action(false);
    assert_eq!(ctx, AttackCamCtx::default());
    ctx.begin_action(true);
    assert_eq!(ctx.phase_cursor, 1);
    assert_eq!((ctx.ramp, ctx.accum, ctx.latch), (0, 0, 0));
}

/// The outer gate is the CALL SITE's, not the routine's - both halves.
#[test]
fn the_outer_gate_rejects_render_mode_two_and_a_targetless_actor() {
    assert!(outer_gate(0, 3));
    assert!(!outer_gate(2, 3), "render mode 2");
    assert!(!outer_gate(0, 8), "no target");
    assert!(!outer_gate(0, 0xFF));
}

/// Every one of the thirteen arm bodies is reachable through the
/// dispatch and produces a framing - the executable counterpart of
/// `every_dispatched_arm_has_a_track_map`.
#[test]
fn every_arm_body_produces_a_framing() {
    let table = probe_table();
    let mut seen = std::collections::BTreeSet::new();
    for (ci, c) in [CharacterArm::One, CharacterArm::Two, CharacterArm::Three]
        .into_iter()
        .enumerate()
    {
        for art in FIRST_ART..=0x2Du8 {
            let Some(va) = art_arm(c, art) else { continue };
            // Walk the animation cursor so every window of a
            // multi-window arm is exercised.
            for anim in [0i16, 0x50, 0x80, 0xB0, 0xF8, 0x120] {
                let mut ctx = AttackCamCtx {
                    phase_cursor: (ci % 2) as u8,
                    ramp: 24,
                    accum: 240,
                    latch: 0,
                    death_ramp: 0,
                };
                let mut a = probe_actor(c, art);
                a.anim_frame = anim;
                if let Some(f) = attack_camera_framing(a, &mut ctx, &table) {
                    assert!(f.duration_frames >= 1);
                    seen.insert(va);
                }
            }
        }
    }
    assert_eq!(seen.len(), 13, "every arm body framed at least once");
}

/// The one arm with a hard cut-off returns `None` past it, which is what
/// leaves case 6's framing standing rather than snapping to a default.
#[test]
fn galas_second_arm_stops_framing_past_its_cutoff() {
    let table = probe_table();
    let mut ctx = AttackCamCtx::default();
    let mut a = probe_actor(CharacterArm::Three, 0x1E);
    assert_eq!(art_arm(CharacterArm::Three, 0x1E), Some(0x801D_81FC));
    a.anim_frame = 0xBF;
    assert!(attack_camera_framing(a, &mut ctx, &table).is_some());
    a.anim_frame = 0xC0;
    assert!(attack_camera_framing(a, &mut ctx, &table).is_none());
}

/// An art id outside the character's own table frames nothing at all.
#[test]
fn an_art_without_an_arm_frames_nothing() {
    let table = probe_table();
    let mut ctx = AttackCamCtx::default();
    for art in [0u8, 0x19, 0x1B, 0x2B, 0xFF] {
        let a = probe_actor(CharacterArm::One, art);
        assert!(
            attack_camera_framing(a, &mut ctx, &table).is_none(),
            "art {art:#x}"
        );
    }
}

/// The cursor picks a COLUMN, and the probe table's odd/even split makes
/// the choice visible in the pose: the two columns of the same row differ
/// by one, so a cursor flip moves the folded component by one.
#[test]
fn the_cursor_selects_which_column_the_arms_fold() {
    let table = probe_table();
    // Character 1, art 0x1C - a single-fold arm (row 7 into the yaw).
    let pose_at = |cursor: u8| {
        let mut ctx = AttackCamCtx {
            phase_cursor: cursor,
            ..Default::default()
        };
        attack_camera_framing(probe_actor(CharacterArm::One, 0x1C), &mut ctx, &table)
            .expect("arm fires")
            .pose
    };
    let a = pose_at(0);
    let b = pose_at(1);
    assert_eq!(b.rot[1] - a.rot[1], 1, "column 1 is column 0 plus one");
    assert_eq!(a.rot[1], 7 * 16, "row 7, column 0");
}

/// The arm's sub-selector genuinely selects: Vahn's `0x1A` arm takes
/// three different bodies for `5`, `0` and `2`.
#[test]
fn the_arm_sub_selector_picks_between_three_bodies() {
    let table = probe_table();
    let pose_for = |s: u8| {
        let mut ctx = AttackCamCtx::default();
        let mut a = probe_actor(CharacterArm::One, 0x1A);
        a.arm_select = s;
        attack_camera_framing(a, &mut ctx, &table)
            .expect("arm fires")
            .pose
    };
    let five = pose_for(5);
    let low = pose_for(0);
    let high = pose_for(2);
    assert_ne!(five, low);
    assert_ne!(low, high);
    assert_ne!(five, high);
    // `s == 5` is the only body that folds row 1 into the yaw.
    assert_eq!(five.rot[1], 16, "row 1, column 0");
    // `s >= 2` is the mid-swing block: pitch `+0xC0`, row 2 into the yaw.
    assert_eq!(high.rot[0], 0xC0);
    assert_eq!(high.rot[1], 2 * 16);
}

/// The animation cursor selects which window of a multi-window arm runs,
/// and the windows are visibly different framings.
#[test]
fn the_animation_cursor_walks_an_arm_through_its_windows() {
    let table = probe_table();
    let pose_at = |anim: i16| {
        let mut ctx = AttackCamCtx::default();
        let mut a = probe_actor(CharacterArm::Three, 0x1C);
        a.anim_frame = anim;
        attack_camera_framing(a, &mut ctx, &table)
            .expect("arm fires")
            .pose
    };
    // Gala's 0x1C arm has four literal windows at 0x40 / 0x70 / 0xA0.
    let w: Vec<CameraPose> = [0x00, 0x40, 0x70, 0xA0]
        .iter()
        .map(|f| pose_at(*f))
        .collect();
    for (i, a) in w.iter().enumerate() {
        for b in &w[i + 1..] {
            assert_ne!(a, b, "window {i} collides with a later one");
        }
    }
    // ... and the seeded TR.y for character 3 is 0x600, not 0x400.
    assert_eq!(w[0].dist[1], 0x600);
    assert_eq!(w[1].dist[1], 0x600 - 0x200);
}

/// The ramp read at the tail of a threshold arm happens AFTER the latch
/// reset, so the frame the threshold is crossed on subtracts nothing.
#[test]
fn a_threshold_crossing_zeroes_the_ramp_before_the_arm_reads_it() {
    let table = probe_table();
    let run = |latch: u8| {
        let mut ctx = AttackCamCtx {
            ramp: 100,
            latch,
            ..Default::default()
        };
        let mut a = probe_actor(CharacterArm::One, 0x1A);
        a.arm_select = 2;
        let p = attack_camera_framing(a, &mut ctx, &table)
            .expect("arm fires")
            .pose;
        (p.dist[2], ctx.ramp)
    };
    // Latch 0: the reset fires, so the `- ramp*2` term is zero.
    assert_eq!(run(0), (POSE_SEED_ANGLE + 3 * 16, 0));
    // Latch 1: no reset, so the ramp is still 100 and the term bites.
    assert_eq!(run(1), (POSE_SEED_ANGLE + 3 * 16 - 200, 100));
}

/// The pose the arms hand back keeps the actor's own position as the
/// (negated) look-at, so the camera orbits whoever is swinging.
#[test]
fn the_framing_orbits_the_acting_actor() {
    let table = probe_table();
    let mut ctx = AttackCamCtx::default();
    let mut a = probe_actor(CharacterArm::Two, 0x1F);
    a.pos = [100, 200, 300];
    a.facing = 0x400;
    let f = attack_camera_framing(a, &mut ctx, &table).expect("arm fires");
    assert_eq!(f.pose.look_at, [-100, -200, -300]);
    // Seeded yaw is the negated facing, then the arm's own folds.
    assert_ne!(f.pose.rot[1], 0);
}

/// Every arm's duration is one of retail's three `a3` operands.
#[test]
fn every_arm_duration_is_one_three_or_six_frames() {
    let table = probe_table();
    for c in [CharacterArm::One, CharacterArm::Two, CharacterArm::Three] {
        for art in FIRST_ART..=0x2Du8 {
            for anim in [0i16, 0x50, 0x80, 0xB0, 0xF8, 0x120] {
                let mut ctx = AttackCamCtx::default();
                let mut a = probe_actor(c, art);
                a.anim_frame = anim;
                if let Some(f) = attack_camera_framing(a, &mut ctx, &table) {
                    assert!(
                        matches!(f.duration_frames, 1 | 3 | 6),
                        "{c:?} art {art:#x} anim {anim:#x} -> {}",
                        f.duration_frames
                    );
                }
            }
        }
    }
}
