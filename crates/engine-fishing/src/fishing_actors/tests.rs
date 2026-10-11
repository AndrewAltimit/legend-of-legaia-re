use super::*;
use legaia_engine_system::pad::PACK_LEFT;
use legaia_engine_system::pad::PACK_RIGHT;

#[test]
fn a_segment_wholly_behind_the_near_bound_is_rejected() {
    assert_eq!(project_segment([0, 0, 10], [10, 0, 20], 100, 0x100), None);
}

#[test]
fn a_segment_wholly_in_front_projects_both_ends_unclipped() {
    let s = project_segment([0, 0, 1000], [100, 50, 1000], 100, 0x100).unwrap();
    assert_eq!(s.view[0], [0, 0, 1000]);
    assert_eq!(s.view[1], [100, 50, 1000]);
    // Centred x = 0 lands on the screen centre.
    assert_eq!(s.screen[0], SCREEN_CENTRE);
    assert!(s.screen[1].0 > SCREEN_CENTRE.0);
}

#[test]
fn the_near_endpoint_is_pulled_onto_the_bound() {
    // a is behind the bound, b is in front.
    let s = project_segment([0, 0, 50], [400, 0, 450], 100, 0x100).unwrap();
    assert_eq!(s.view[0][2], 100, "clipped to the near bound");
    assert_eq!(s.view[1], [400, 0, 450], "the far end is untouched");
    // The correct crossing sits 1/8 along a->b: x = 50.
    assert_eq!(s.view[0][0], 50);
}

#[test]
fn the_far_arm_uses_the_complementary_parameter() {
    // Mirror of the case above: b is the one behind the bound. The
    // retail arm slides b by 1 - t instead of t, so it does NOT land on
    // the geometric crossing - only its z is forced to the bound.
    let s = project_segment([400, 0, 450], [0, 0, 50], 100, 0x100).unwrap();
    assert_eq!(s.view[0], [400, 0, 450]);
    assert_eq!(s.view[1][2], 100);
    // The geometric crossing is x = 50; retail's complementary
    // parameter puts the endpoint at 350 instead.
    assert_eq!(s.view[1][0], 350);
}

#[test]
fn the_facing_clamp_holds_the_arc() {
    assert_eq!(step_facing(0x800, 0), 0x800);
    assert_eq!(step_facing(0x800, PACK_RIGHT), 0x840);
    assert_eq!(step_facing(0x800, PACK_LEFT), 0x7C0);
    // Both directions in one frame cancel.
    assert_eq!(step_facing(0x800, PACK_LEFT | PACK_RIGHT), 0x800);
    // The clamp catches the ends, and pulls an out-of-range seed in.
    assert_eq!(step_facing(0x700, PACK_LEFT), 0x700);
    assert_eq!(step_facing(0x900, PACK_RIGHT), 0x900);
    assert_eq!(step_facing(0x100, 0), 0x700);
}

#[test]
fn the_camera_negates_the_position_and_wraps_the_yaw() {
    let c = fish_camera(0x200, 0x40, -0x300, 0x800);
    assert_eq!(c.translation, (-0x200, 0, 0x300));
    // facing 0x800 -> (0x800 + 0x800) & 0xFFF = 0 -> yaw 0.
    assert_eq!(c.yaw, 0);
    assert_eq!(c.pitch_term, 0x400 - 6 * 0x40);
    // A quarter turn wraps inside the 12-bit angle space.
    assert_eq!(fish_camera(0, 0, 0, 0).yaw, -0x800);
}

#[test]
fn the_wander_roll_consumes_five_draws_in_order() {
    let draws = [0, 50, 0, 6, 1];
    let mut i = 0;
    let t = roll_wander_target(1000, 2000, || {
        let v = draws[i];
        i += 1;
        v
    });
    assert_eq!(i, 5);
    assert_eq!(t.dwell, 50 + RETARGET_MIN);
    // rand % 6 == 0 -> step 3; rand 6 % 6 == 0 -> step 3 as well.
    assert_eq!(t.z, 2000 + WANDER_Z_BIAS + 3 * 0x20);
    assert_eq!(t.x, 1000 + 3 * 0x50);
    assert_eq!(t.ripple_variant, 1);
}

#[test]
fn debug_tiles_truncate_toward_zero() {
    assert_eq!(debug_tile(0), 0);
    assert_eq!(debug_tile(128), 1);
    assert_eq!(debug_tile(127), 0);
    assert_eq!(debug_tile(-1), 0);
    assert_eq!(debug_tile(-128), -1);
    assert_eq!(debug_tile(-129), -1);
}

#[test]
fn the_debug_readout_needs_both_the_flag_and_the_modifier() {
    assert!(!debug_readout_visible(false, PACK_DEBUG_MODIFIER));
    assert!(!debug_readout_visible(true, 0));
    assert!(debug_readout_visible(true, PACK_DEBUG_MODIFIER));
}

#[test]
fn the_bite_ladder_only_discriminates_at_two_hundred() {
    assert_eq!(bite_interval(201, false), BITE_INTERVAL_NEAR);
    assert_eq!(bite_interval(100_000, false), BITE_INTERVAL_NEAR);
    assert_eq!(bite_interval(200, false), BITE_INTERVAL_PIVOT);
    assert_eq!(bite_interval(199, false), BITE_INTERVAL_FAR);
    assert_eq!(bite_interval(0, false), BITE_INTERVAL_FAR);
    // None of the dead arms' cadences is ever produced.
    for (threshold, cadence) in BITE_LADDER_DEAD_ARMS {
        assert_ne!(bite_interval(threshold, false), cadence);
    }
}

#[test]
fn the_debug_gate_overrides_the_whole_ladder() {
    assert_eq!(bite_interval(0, true), BITE_INTERVAL_DEBUG);
    assert_eq!(bite_interval(5000, true), BITE_INTERVAL_DEBUG);
}

#[test]
fn only_the_far_band_overrides_the_credit() {
    assert_eq!(bite_credit_override(199), Some(BITE_FAR_CREDIT));
    assert_eq!(bite_credit_override(200), None);
    assert_eq!(bite_credit_override(1000), None);
    // The override and the modulus flip on the same comparison.
    for d in 0..400 {
        assert_eq!(
            bite_credit_override(d).is_some(),
            bite_interval(d, false) == BITE_INTERVAL_FAR
        );
    }
}

#[test]
fn the_hit_type_roll_is_skewed_to_band_three() {
    assert_eq!(roll_hit_type(0), 3);
    assert_eq!(roll_hit_type(0x0C00), 3);
    assert_eq!(roll_hit_type(0x0C01), 2);
    assert_eq!(roll_hit_type(0x0E70), 2);
    assert_eq!(roll_hit_type(0x0E71), 1);
    assert_eq!(roll_hit_type(0x0F38), 1);
    assert_eq!(roll_hit_type(0x0F39), 0);
    assert_eq!(roll_hit_type(0x0FFF), 0);
    // The draw is masked, so high bits never change the band.
    assert_eq!(roll_hit_type(0xFFFF_F000), 3);
}

#[test]
fn the_highest_water_class_bit_wins() {
    assert_eq!(water_tile_class(0), None);
    assert_eq!(water_tile_class(0x04), Some((0x1E, 100)));
    assert_eq!(water_tile_class(0x08), Some((0x14, 300)));
    assert_eq!(water_tile_class(0x10), Some((0x14, 500)));
    // Several bits set: the last arm tested wins, as in retail.
    assert_eq!(water_tile_class(0x1C), Some((0x14, 500)));
}

#[test]
fn each_nudge_mask_counts_once() {
    assert_eq!(bite_pad_nudge(0), 0);
    assert_eq!(bite_pad_nudge(0x8000), 1);
    assert_eq!(bite_pad_nudge(0x8000 | 0x2000), 2);
    // 0x40 and 0x80 are one mask, so holding both still counts one.
    assert_eq!(bite_pad_nudge(0x40 | 0x80), 1);
    assert_eq!(bite_pad_nudge(0xA0C0), 3);
}

#[test]
fn the_line_sub_state_has_a_fourth_arm() {
    assert_eq!(LinePhase::from_raw(0), LinePhase::Arm);
    assert_eq!(LinePhase::from_raw(1), LinePhase::Attach);
    assert_eq!(LinePhase::from_raw(2), LinePhase::Track);
    assert_eq!(LinePhase::from_raw(4), LinePhase::Celebrate);
    assert_eq!(LinePhase::from_raw(3), LinePhase::Idle(3));
}

const SCREEN: ClipRect = ClipRect {
    x_min: 0,
    y_min: 0,
    x_max: 0x140,
    y_max: 0xF0,
};

#[test]
fn a_segment_inside_the_window_is_untouched() {
    let (mut p, mut q) = ((10, 20), (300, 200));
    clip_segment_2d(&mut p, &mut q, SCREEN);
    assert_eq!((p, q), ((10, 20), (300, 200)));
}

#[test]
fn a_crossing_endpoint_lands_on_the_bound_at_the_true_intersection() {
    // p is left of x_min = 0; the segment crosses at x = 0, y = 50.
    let (mut p, mut q) = ((-100, 0), (100, 100));
    clip_segment_2d(&mut p, &mut q, SCREEN);
    assert_eq!(p, (0, 50));
    assert_eq!(q, (100, 100));
}

#[test]
fn each_bound_moves_the_endpoint_that_is_outside_it() {
    // q is past x_max; the crossing sits halfway.
    let (mut p, mut q) = ((0x100, 0), (0x180, 0x40));
    clip_segment_2d(&mut p, &mut q, SCREEN);
    assert_eq!(p, (0x100, 0));
    assert_eq!(q, (0x140, 0x20));
}

#[test]
fn a_segment_wholly_outside_one_bound_is_left_alone() {
    // Retail's arm needs one endpoint strictly below the bound and the
    // other strictly above it, so a segment entirely left of x_min is
    // untouched rather than collapsed onto the edge.
    let (mut p, mut q) = ((-200, 10), (-100, 20));
    clip_segment_2d(&mut p, &mut q, SCREEN);
    assert_eq!((p, q), ((-200, 10), (-100, 20)));
}

#[test]
fn the_vertical_arms_swap_the_components() {
    let rect = ClipRect {
        x_min: -1000,
        y_min: 0,
        x_max: 1000,
        y_max: 100,
    };
    let (mut p, mut q) = ((0, -100), (100, 100));
    clip_segment_2d(&mut p, &mut q, rect);
    // p rides up to y_min = 0 (halfway, x = 50); q rides down to y_max.
    assert_eq!(p, (50, 0));
    assert_eq!(q.1, 100);
}

#[test]
fn the_overhead_probe_reads_the_high_nibble_only() {
    let mut grid = vec![0u8; WALK_GRID_PITCH * WALK_GRID_ROWS];
    // x = 64 -> xc = ((64 + 63) >> 6) - 1 = 0; z = 0 -> zc = 0 + 2 = 2.
    // column = 0, row = 1, bit = 1 << (0 + 2*0) = 1.
    let idx = WALK_GRID_PITCH;
    grid[idx] = 0x01; // low nibble only - the field collision's mask
    assert!(!walk_grid_overhead(&grid, 64, 0));
    grid[idx] = 0x10; // the same sub-cell in the high nibble
    assert!(walk_grid_overhead(&grid, 64, 0));
}

#[test]
fn the_overhead_probe_selects_the_sub_cell_by_parity() {
    let mut grid = vec![0u8; WALK_GRID_PITCH * WALK_GRID_ROWS];
    // xc = 1 (x = 128), zc = 3 (z = 64) -> both odd -> bit 8.
    // column = 0, row = 1.
    grid[WALK_GRID_PITCH] = 0x80;
    assert!(walk_grid_overhead(&grid, 128, 64));
    // Same byte, wrong parity pair (xc = 0, zc = 2 -> bit 1).
    assert!(!walk_grid_overhead(&grid, 64, 0));
}

#[test]
fn an_out_of_range_probe_reports_clear_instead_of_panicking() {
    assert!(!walk_grid_overhead(&[], 0, 0));
}

#[test]
fn the_separation_is_a_sub_cell_count() {
    let sqrt = |v: i32| (v as f64).sqrt() as i32;
    // 64 units apart on one axis is exactly one sub-cell.
    assert_eq!(tracked_point_separation((0, 0), (64, 0), sqrt), 1);
    assert_eq!(tracked_point_separation((0, 0), (63, 0), sqrt), 0);
    // The sign of each component is dropped before the square.
    assert_eq!(
        tracked_point_separation((0, 0), (-640, 0), sqrt),
        tracked_point_separation((0, 0), (640, 0), sqrt)
    );
    // A negative normalise result clamps to zero rather than wrapping.
    assert_eq!(tracked_point_separation((0, 0), (100, 100), |_| -1), 0);
}

#[test]
fn the_wander_actor_rolls_on_dwell_expiry_and_holds_its_place() {
    let mut w = FishWander::new(0x400, 0, 0x400);
    // First tick: the dwell is due, so the roll happens immediately.
    let draws = [0u32, 50, 0, 6, 1];
    let mut i = 0;
    let rolled = w
        .tick(0, || {
            let v = draws[i % draws.len()];
            i += 1;
            v
        })
        .expect("first tick re-rolls");
    assert_eq!(rolled.ripple_variant, 1);
    // The dwell now holds; no re-roll, and the actor stays put - the
    // roll names a ripple point, it does not move the angler.
    let (x0, z0) = (w.x, w.z);
    assert!(w.tick(0, || 0).is_none());
    assert_eq!((w.x, w.z), (x0, z0));
    // The facing steps + clamps off the held packed pad.
    let f0 = w.facing;
    w.tick(PACK_RIGHT, || 0);
    assert_eq!(w.facing, f0 + FACING_STEP);
}

#[test]
fn the_line_actor_arms_tracks_and_celebrates() {
    let mut line = LineActorSim::hooked();
    // Arm fires the hook cue and steps to Attach.
    let f = line.tick(1);
    assert_eq!(f.cue, Some(HOOK_CUE));
    assert_eq!(line.raw, 1);
    // Attach copies (host-side) and steps to Track; Track holds.
    line.tick(1);
    assert_eq!(line.raw, 2);
    assert_eq!(line.tick(1), LineActorFrame::default());
    // Landing enters the celebrate arm; the first stage frame fires the
    // celebrate cue + every unlocked burst, and the last hands back.
    line.land(700);
    let mut fired = None;
    let mut done = false;
    for _ in 0..CELEBRATION_STAGE_FRAMES.2 + 2 {
        let f = line.tick(1);
        if f.cue.is_some() {
            assert!(fired.is_none(), "the stage cue fires once");
            fired = Some(f.bursts.len());
        }
        done |= f.done;
    }
    // Score 700 clears the 200 + 600 tiers only.
    assert_eq!(fired, Some(2));
    assert!(done);
}

#[test]
fn celebration_tiers_accumulate_with_the_score() {
    assert_eq!(celebration_bursts(0).count(), 0);
    assert_eq!(celebration_bursts(201).count(), 1);
    assert_eq!(celebration_bursts(601).count(), 2);
    assert_eq!(celebration_bursts(801).count(), 3);
    assert_eq!(celebration_bursts(0x4B1).count(), 4);
    // The cues fire bottom-up, and the top tier is silent.
    let cues: Vec<Option<u8>> = celebration_bursts(2000).map(|b| b.cue).collect();
    assert_eq!(cues, vec![Some(0x25), Some(0x26), Some(0x27), None]);
}

/// A 42-vertex rod whose vertex [`ROD_TIP_VERTEX`] sits at the disc
/// rod's tip, `(0, -138, 0)`, and a one-record bend moving that vertex by
/// `(0, 6, 45)` at full weight - the shape of `other1`'s rods and VDF
/// sub-entry 0, synthesised so no disc bytes are needed.
fn synthetic_rod_mesh() -> RodMesh {
    let mut rest = vec![0u8; 42 * 8];
    let o = ROD_TIP_VERTEX * 8;
    rest[o + 2..o + 4].copy_from_slice(&(-138i16).to_le_bytes());
    let mut bend = Vec::new();
    for w in [1u32, 0, ROD_TIP_VERTEX as u32, 1] {
        bend.extend_from_slice(&w.to_le_bytes());
    }
    for c in [0i16, 6, 45, 0] {
        bend.extend_from_slice(&c.to_le_bytes());
    }
    RodMesh {
        rods: [Some(rest.clone()), Some(rest.clone()), Some(rest)],
        bend: Some(bend),
        ..Default::default()
    }
}

#[test]
fn the_rod_tip_bends_by_the_weighted_vdf_delta() {
    let m = synthetic_rod_mesh();
    assert_eq!(m.tip(0, 0), Some([0, -138, 0]));
    assert_eq!(m.tip(1, 0x1000), Some([0, -132, 45]));
    // `(0x800 * 45) >> 12` = 22: the GPF blend truncates.
    assert_eq!(m.tip(2, 0x800), Some([0, -135, 22]));
    assert_eq!(m.tip(3, 0), None, "three rods");
}

#[test]
fn an_unrotated_rod_projects_its_tip_straight_up_the_centre_line() {
    // R = 6I, TR = 6 * (0, 0x46, 0x64) = (0, 420, 600): the tip lands at
    // view (0, 420 - 828, 600), i.e. screen y = 114 + 220 * -408 / 600.
    let t = rod_tip_screen([0, -138, 0], 0, 0, 0);
    assert_eq!(t.sxy, (160, -36));
    assert_eq!(t.depth, 600);
}

#[test]
fn the_cast_pose_brings_the_tip_into_the_clip_window() {
    let t = rod_tip_screen([0, -138, 0], ROD_BOB_FLOOR, 0, 0);
    let r = LINE_CLIP_RECT;
    assert!(
        (r.x_min..=r.x_max).contains(&t.sxy.0) && (r.y_min..=r.y_max).contains(&t.sxy.1),
        "{t:?}"
    );
}

#[test]
fn the_yaw_swings_the_tip_to_mirrored_sides() {
    // `RotMatrixY(-yaw)` is the last factor, so it acts on the vertex
    // first - about the rod's own axis. A straight rod's tip sits on that
    // axis and does not move; the bend's `z` lean is what the yaw swings.
    let straight = rod_tip_screen([0, -138, 0], ROD_BOB_FLOOR, 0, 0x100);
    assert_eq!(straight, rod_tip_screen([0, -138, 0], ROD_BOB_FLOOR, 0, 0));
    let l = rod_tip_screen([0, -132, 45], ROD_BOB_FLOOR, 0, 0x100);
    let r = rod_tip_screen([0, -132, 45], ROD_BOB_FLOOR, 0, -0x100);
    assert!(l.sxy.0 != 160 && r.sxy.0 != 160);
    assert!((l.sxy.0 - 160).signum() == -(r.sxy.0 - 160).signum());
    assert!(((l.sxy.0 - 160).abs() - (r.sxy.0 - 160).abs()).abs() <= 1);
    assert_eq!(l.sxy.1, r.sxy.1);
}

#[test]
fn the_line_clips_the_fish_end_and_links_at_the_tip_depth() {
    let tip = RodTip {
        sxy: (160, 100),
        depth: 640,
    };
    // The fish is off the left edge: its end rides onto x = 0 along the
    // segment, the rod end stays put.
    let l = fishing_line((-160, 200), tip);
    assert_eq!(l.rod, (160, 100));
    assert_eq!(l.fish, (0, 150));
    assert_eq!((l.fish_rgb, l.rod_rgb), (LINE_FISH_RGB, LINE_ROD_RGB));
    assert_eq!(l.ot, 640 >> 5);
    // An on-screen pair passes through untouched.
    let l = fishing_line((40, 180), tip);
    assert_eq!((l.fish, l.rod), ((40, 180), (160, 100)));
}

/// A four-vertex square in the rod's local `xy` plane with three
/// primitives over it: a quad wound toward the camera, the same quad
/// wound away, and a triangle.
fn square_rod_mesh() -> RodMesh {
    let mut rest = Vec::new();
    for (x, y) in [(-10i16, -10i16), (10, -10), (-10, 10), (10, 10)] {
        for c in [x, y, 0, 0] {
            rest.extend_from_slice(&c.to_le_bytes());
        }
    }
    let red = [[0x80, 0x28, 0x28]; 4];
    RodMesh {
        rods: [Some(rest), None, None],
        bend: None,
        prims: [
            vec![
                RodPrim {
                    verts: [0, 1, 2, 3],
                    quad: true,
                    rgb: red,
                },
                RodPrim {
                    verts: [1, 0, 3, 2],
                    quad: true,
                    rgb: red,
                },
                RodPrim {
                    verts: [0, 1, 2, 2],
                    quad: false,
                    rgb: red,
                },
            ],
            Vec::new(),
            Vec::new(),
        ],
    }
}

#[test]
fn the_rod_model_culls_its_back_faces_and_buckets_by_mean_depth() {
    let pose = RodPose {
        rot_x: 0,
        rot_z: 0,
        yaw: 0,
        weight: 0,
    };
    let faces = rod_faces(&square_rod_mesh(), 0, pose);
    // The away-wound quad fails both NCLIP halves; the other two draw.
    assert_eq!(faces.len(), 2, "{faces:?}");
    assert!(faces[0].quad && !faces[1].quad, "packet order is kept");
    // Every corner is the tip projection of its vertex.
    for (i, v) in [(-10i16, -10i16), (10, -10), (-10, 10), (10, 10)]
        .into_iter()
        .enumerate()
    {
        assert_eq!(faces[0].xy[i], rod_tip_screen([v.0, v.1, 0], 0, 0, 0).sxy);
    }
    // Depth 600 at every corner: AVSZ4 = 4 * 600 * 0x80 >> 12 = 75,
    // AVSZ3 = 3 * 600 * 0xAA >> 12 = 74; both link at `OTZ >> 2` = 18 -
    // the line's `600 >> 5`.
    assert_eq!((faces[0].ot, faces[1].ot), (18, 18));
    assert_eq!(faces[0].ot, 600 >> 5);
    // A missing rod draws nothing.
    assert!(rod_faces(&square_rod_mesh(), 1, pose).is_empty());
}

#[test]
fn the_rod_actor_records_the_pose_it_drew_with() {
    let mesh = synthetic_rod_mesh();
    let mut rod = RodActor::cast();
    assert_eq!(rod.pose, None);
    rod.tick(Some(&mesh), 0, 1);
    let pose = rod.pose.expect("a pose after the first tick");
    // First cast frame: bob -0x40, pitch (0 + 0x1000 / 2) / 16 = 0x80.
    assert_eq!(pose.rot_x, -0x40 + 0x80);
    assert_eq!(pose.weight, ROD_BEND_AT_CAST as i16, "the pre-bleed bend");
    let tip = mesh.tip(0, pose.weight).unwrap();
    assert_eq!(
        rod.tip,
        Some(rod_tip_screen(tip, pose.rot_x, pose.rot_z, pose.yaw))
    );
}

#[test]
fn the_rod_swings_down_on_the_cast_and_retires_after_the_recover() {
    let mesh = synthetic_rod_mesh();
    let mut rod = RodActor::cast();
    let mut frames = 0;
    while rod.swing == RodSwing::Cast {
        rod.tick(Some(&mesh), 0, 1);
        frames += 1;
        assert!(frames < 100);
    }
    // 700 / 0x40 rounds up to eleven frames.
    assert_eq!(frames, 11);
    assert_eq!((rod.swing, rod.bob), (RodSwing::Hold, ROD_BOB_FLOOR));
    assert!(rod.tip.is_some());
    // The cast's seeded bend bleeds off at 0x60 a frame.
    assert_eq!(rod.bend, ROD_BEND_AT_CAST - 11 * 0x60);
    rod.recover();
    let mut frames = 0;
    while !rod.retired() {
        rod.tick(Some(&mesh), 0, 1);
        frames += 1;
        assert!(frames < 100);
    }
    assert_eq!(rod.bob, ROD_BOB_CEILING);
    assert_eq!(frames, 27, "(700 + 0x400) / 0x40 rounds up to 27");
}

#[test]
fn the_lure_tick_bends_lifts_and_rolls_the_rod() {
    let mut rod = RodActor::cast();
    rod.bend = 0;
    // A held reel bends toward 0x1000; a fish on caps it at 0x1800.
    for _ in 0..64 {
        rod.drive(
            RodDrive {
                hooked: false,
                held: 0x80,
            },
            1,
        );
    }
    assert_eq!(rod.bend, 0x1000);
    for _ in 0..64 {
        rod.drive(
            RodDrive {
                hooked: true,
                held: 0,
            },
            1,
        );
    }
    assert_eq!(rod.bend, 0x1800);
    // D-pad down lifts, capped at 0x1000.
    for _ in 0..64 {
        rod.drive(
            RodDrive {
                hooked: false,
                held: 0x4000,
            },
            1,
        );
    }
    assert_eq!(rod.lift, 0x1000);
    // D-pad right rolls toward 0x100 at 0x10 a frame.
    rod.drive(
        RodDrive {
            hooked: false,
            held: 0x2000,
        },
        1,
    );
    assert_eq!((rod.lean_target, rod.lean), (0x100, 0x10));
    for _ in 0..32 {
        rod.drive(
            RodDrive {
                hooked: false,
                held: 0x2000,
            },
            1,
        );
    }
    assert_eq!(rod.lean, 0x100);
    // Released, the target bleeds back and the roll follows it down.
    for _ in 0..200 {
        rod.drive(RodDrive::default(), 1);
    }
    assert_eq!((rod.lean_target, rod.lean), (0, 0));
}
