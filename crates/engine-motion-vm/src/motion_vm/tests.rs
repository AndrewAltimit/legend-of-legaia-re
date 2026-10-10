use super::*;

fn st(x: i16, z: i16, speed: u16) -> MotionState {
    MotionState {
        world_x: x,
        world_z: z,
        speed,
        ..Default::default()
    }
}

fn tgt(x: i16, y: i16, z: i16) -> MotionTarget {
    MotionTarget { x, y, z, id: 0 }
}

fn inputs() -> FieldActorInputs {
    FieldActorInputs {
        lifetime: 1,
        flags: 0,
        field_8e: 0,
        path_target_present: false,
        scripted_present: false,
        ambient_gate: false,
        frame_step: 0,
        scene_guard_clear: true,
        global_suppress: false,
    }
}

#[test]
fn rotate_toward_clamped_limits_the_turn() {
    // Delta within rate: lands exactly on target.
    assert_eq!(rotate_toward_clamped(100, 130, 50), 130);
    // Delta exceeds +rate: clamped.
    assert_eq!(rotate_toward_clamped(100, 400, 50), 150);
    // Delta below -rate: clamped the other way.
    assert_eq!(rotate_toward_clamped(400, 100, 50), 350);
    // Zero rate freezes the value.
    assert_eq!(rotate_toward_clamped(400, 100, 0), 400);
}

#[test]
fn height_arm_holds_when_dead_or_frozen() {
    // lifetime < 0: no height arm at all.
    let plan = field_actor_plan(FieldActorInputs {
        lifetime: -1,
        flags: 0x2000_0000,
        ..inputs()
    });
    assert!(!plan.pre_update);
    assert_eq!(plan.height, FieldActorHeight::Hold);
    // flags & 2 freezes the height even for a live actor.
    let plan = field_actor_plan(FieldActorInputs {
        flags: 0x2000_0002,
        ..inputs()
    });
    assert!(plan.pre_update);
    assert_eq!(plan.height, FieldActorHeight::Hold);
}

#[test]
fn height_arm_selects_by_flag_priority() {
    // 0x20000000 overrides Y with -(+0x8E).
    assert_eq!(
        field_actor_plan(FieldActorInputs {
            flags: 0x2000_0000,
            field_8e: 7,
            ..inputs()
        })
        .height,
        FieldActorHeight::Mirror(-7)
    );
    // No target/ambient bits and gate off -> Hold.
    assert_eq!(field_actor_plan(inputs()).height, FieldActorHeight::Hold);
    // Ambient gate alone re-enables the floor-snap arm.
    assert_eq!(
        field_actor_plan(FieldActorInputs {
            ambient_gate: true,
            ..inputs()
        })
        .height,
        FieldActorHeight::SnapToFloor
    );
    // A target bit (0x200) selects the floor snap without the ambient gate.
    assert_eq!(
        field_actor_plan(FieldActorInputs {
            flags: 0x200,
            ..inputs()
        })
        .height,
        FieldActorHeight::SnapToFloor
    );
    // 0x2000 alone does not pass the 0x20200 target gate: still Hold.
    assert_eq!(
        field_actor_plan(FieldActorInputs {
            flags: 0x2000,
            ..inputs()
        })
        .height,
        FieldActorHeight::Hold
    );
    // 0x2000 with a target bit (0x200) switches to the clamped ramp;
    // rate = frame_step * 6.
    assert_eq!(
        field_actor_plan(FieldActorInputs {
            flags: 0x2000 | 0x200,
            frame_step: 4,
            ..inputs()
        })
        .height,
        FieldActorHeight::GlideToFloor { rate: 24 }
    );
}

#[test]
fn dispatch_gates_on_flags_and_presence() {
    let plan = field_actor_plan(FieldActorInputs {
        flags: 0x100 | 0x400 | 0x1000,
        path_target_present: true,
        scripted_present: true,
        ..inputs()
    });
    assert!(plan.dispatch.run_spline);
    assert!(plan.dispatch.run_pursue);
    assert!(plan.dispatch.run_scripted);
    assert!(plan.dispatch.run_move_table);
    // Spline needs the path target present, not just the flag.
    let plan = field_actor_plan(FieldActorInputs {
        flags: 0x100,
        path_target_present: false,
        ..inputs()
    });
    assert!(!plan.dispatch.run_spline);
    // flags & 8 suppresses the scripted VM even with bytecode present.
    let plan = field_actor_plan(FieldActorInputs {
        flags: 8,
        scripted_present: true,
        ..inputs()
    });
    assert!(!plan.dispatch.run_scripted);
    // A held scene bracket guard also suppresses it.
    let plan = field_actor_plan(FieldActorInputs {
        scripted_present: true,
        scene_guard_clear: false,
        ..inputs()
    });
    assert!(!plan.dispatch.run_scripted);
    // move-table runs on lifetime > 0 even with no flags.
    assert!(field_actor_plan(inputs()).dispatch.run_move_table);
}

#[test]
fn global_suppress_clears_the_whole_dispatch() {
    let plan = field_actor_plan(FieldActorInputs {
        flags: 0x100 | 0x400 | 0x1000,
        path_target_present: true,
        scripted_present: true,
        global_suppress: true,
        ..inputs()
    });
    assert_eq!(plan.dispatch, FieldMotionDispatch::default());
    // The facing arm is not part of the suppress gate - it still runs.
    assert_eq!(plan.height, FieldActorHeight::Hold);
}

#[test]
fn step_compass_walk_moves_along_the_table_direction_for_its_budget() {
    // [0x37, b0, b1]: direction 5 (+Z +X), sel = 0 (div 4), 2 units of
    // budget -> total 8 speed units; speed 3 per tick, rate 0x80.
    let mut s = st(0, 0, 3);
    let bc = [0x37, 0x05, 0x02];
    // Tick 1: spend 3 -> step 0x80*3/4 = 96 on both axes.
    assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Yield);
    assert_eq!((s.world_x, s.world_z, s.op_accum), (96, 96, 3));
    assert_eq!(s.pc, 0);
    // Tick 2: spend 3 more (6).
    assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Yield);
    // Tick 3: only 2 remain -> step 0x80*2/4 = 64, leg done.
    assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Done);
    assert_eq!((s.world_x, s.world_z), (96 * 2 + 64, 96 * 2 + 64));
    assert_eq!((s.op_accum, s.pc), (0, 3));
}

#[test]
fn step_compass_walk_slow_rate_and_negative_axes() {
    // 0x41 at rate 0x40, direction 1 (-Z -X), sel from body1 >> 6 = 1
    // (div 8), one unit -> total 8.
    let mut s = st(100, 100, 8);
    let bc = [0x41, 0x01, 0x41];
    assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Done);
    // step = 0x40 * 8 / 8 = 0x40.
    assert_eq!((s.world_x, s.world_z), (100 - 0x40, 100 - 0x40));
}

#[test]
fn compass_axis_signs_cover_the_eight_directions() {
    // Each entry names exactly one sign per moving axis.
    for bits in COMPASS_AXIS_SIGNS {
        assert!(bits & 3 != 3 && bits & 0xC != 0xC && bits != 0);
    }
}

#[test]
fn step_move_toward_target_walks_diagonally() {
    let mut s = st(0, 0, 3);
    let t = tgt(5, 0, 5);
    let bc = [0x47];
    // First step: x += 3, z += 3 - yields, PC stays on op.
    assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
    assert_eq!((s.world_x, s.world_z), (3, 3));
    assert_eq!(s.pc, 0);
    // Second step: x += 2 (clamped), z += 2 -> arrives.
    assert_eq!(step(&mut s, t, &bc), StepResult::Done);
    assert_eq!((s.world_x, s.world_z), (5, 5));
    assert_eq!(s.pc, 1);
}

#[test]
fn step_hold_never_completes() {
    // 0x43's jump-table slot is the epilogue with the done flag clear:
    // retail parks on it (the port used to advance past it).
    let mut s = st(2, 3, 1);
    let bc = [0x43];
    for _ in 0..3 {
        assert_eq!(step(&mut s, tgt(99, 99, 99), &bc), StepResult::Yield);
    }
    assert_eq!((s.world_x, s.world_z), (2, 3));
    assert_eq!(s.pc, 0);
}

#[test]
fn step_rotate_to_angle_ccw() {
    let mut s = MotionState {
        yaw: 0x000,
        speed: 4,
        op_accum: 0,
        pc: 0,
        ..Default::default()
    };
    // body0 = 0x06 (LUT index 6 = +X = engine yaw 0x400),
    // body1 = 0x10 (budget = 16, force bit clear -> increasing).
    let bc = [0x38, 0x06, 0x10];
    // Total arc = 0x400, budget = 16, speed = 4. Retail recomputes the
    // step from the *live* arc each tick, so the increment stays 0x100.
    // First step: moves 0x400 * 4 / 16 = 0x100
    assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Yield);
    assert_eq!(s.yaw, 0x0100);
    assert_eq!(s.op_accum, 4);
    // Second step: moves another 0x100
    assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Yield);
    assert_eq!(s.yaw, 0x0200);
    // Third
    assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Yield);
    assert_eq!(s.yaw, 0x0300);
    // Fourth: remaining=0, snap to target and Done
    assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Done);
    assert_eq!(s.yaw, 0x0400);
    assert_eq!(s.op_accum, 0);
    assert_eq!(s.pc, 3);
}

#[test]
fn step_rotate_to_angle_shortest_path_ccw() {
    let mut s = MotionState {
        yaw: 0x000,
        speed: 8,
        op_accum: 0,
        pc: 0,
        ..Default::default()
    };
    // LUT index 0 = -Z = engine yaw 0x800; the two arcs tie at 0x800 and
    // retail's `arc > 0x800` test breaks the tie toward increasing.
    let bc = [0x38, 0x80, 0x10];
    assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Yield);
    assert_eq!(s.yaw, 0x0400);
}

#[test]
fn step_rotate_to_angle_shortest_path_cw() {
    let mut s = MotionState {
        yaw: 0x000,
        speed: 16,
        op_accum: 0,
        pc: 0,
        ..Default::default()
    };
    // body0 = 0x83: shortest-path bit set, LUT index 3 -> engine yaw 0xE00.
    // The increasing arc is 0xE00 > 0x800, so retail rotates decreasing
    // instead: arc = 0x200, step = 0x200 * 16 / 32 = 0x100, and the raw
    // write-back wraps below zero to 0xFF00 (== 0xF00 mod 0x1000) - the
    // pre-unwrap hold retail's `+0x26` shows on a wrap-crossing ramp.
    let bc = [0x38, 0x83, 0x20];
    assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Yield);
    assert_eq!(s.yaw, 0xFF00);
    assert_eq!(s.yaw & 0x0FFF, 0x0F00, "renderers mask to the same angle");
}

/// A decreasing ramp that crosses the wrap holds **raw** u16 headings
/// outside `0..0xFFF` on every mid-ramp tick and only the terminal snap
/// lands back in range - the retail `+0x26` behaviour (runtime-pinned on
/// the Mei dinner-beat rotate legs, which run `0xFFxx` values live). A
/// per-tick `& 0xFFF` would keep the same angle mod 0x1000 but diverge
/// from the traced raw values.
#[test]
fn rotate_ramp_holds_raw_headings_across_the_wrap() {
    // From yaw 0, rotate DECREASING (body1 bit 7) to LUT index 0
    // (engine 0x800), budget 12, speed 2 - the retail cadence of the
    // traced ramps. arc = (0 - 0x800) mod 0x1000 = 0x800.
    let bc = [0x38, 0x00, 0x8C];
    let mut s = MotionState {
        yaw: 0,
        speed: 2,
        ..Default::default()
    };
    let mut raws = Vec::new();
    loop {
        let r = step(&mut s, tgt(0, 0, 0), &bc);
        assert!(s.yaw_written, "every ramp tick writes the heading");
        raws.push(s.yaw);
        if r == StepResult::Done {
            break;
        }
    }
    let (last, mid) = raws.split_last().unwrap();
    assert!(!mid.is_empty(), "ramp has mid-ramp ticks");
    for &y in mid {
        assert!(
            y > 0x0FFF,
            "mid-ramp heading {y:#06X} should hold the raw wrapped value"
        );
    }
    // First tick: 0x800 * 2 / 12 = 0x155, written as 0 - 0x155 = 0xFEAB.
    assert_eq!(mid[0], 0xFEAB);
    assert_eq!(*last, 0x800, "terminal snap lands exactly on the target");
}

/// The walk snap is **once per leg** (plus once per step-direction
/// change), not a rewrite on every moving frame: `yaw_written` fires on
/// the first moving frame and on the dominant-axis → diagonal cut only,
/// so an interleaved writer's pose survives a straight leg.
#[test]
fn walk_leg_writes_heading_once_per_direction() {
    let mut s = st(0, 0, 4);
    let t = tgt(100, 0, 20);
    let bc = [0x47];
    // Frame 1: leg start - the write.
    assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
    assert!(s.yaw_written, "leg start writes the walk facing");
    assert_eq!(s.yaw, 0x400, "+X compass");
    // Frames 2..20: X still dominates - same direction, no rewrite.
    for i in 0..19 {
        assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
        assert!(!s.yaw_written, "straight-leg frame {i} must not rewrite");
        assert_eq!(s.yaw, 0x400, "held compass heading");
    }
    // Deltas equal: the diagonal cut is a direction change - one write.
    assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
    assert!(s.yaw_written, "direction change writes again");
    assert_eq!(s.yaw, 0x200, "+X +Z diagonal");
    // Next leg re-writes even in the same direction: leg state clears.
    while step(&mut s, t, &bc) != StepResult::Done {}
    assert_eq!(s.walk_facing, None, "leg completion clears the leg state");
    s.pc = 0;
    let t2 = tgt(200, 0, 120);
    assert_eq!(step(&mut s, t2, &bc), StepResult::Yield);
    assert!(s.yaw_written, "a new leg writes at its start");
}

/// The traced per-op ramp rates come from `arc / budget` - the law's
/// per-tick floor-divide sequence, at the retail speed scalar 2, matches
/// the recomp trace's increments exactly (170 170 170 171 ... for arc
/// 0x600 over 18 frames, the Mei beat's first turn: rate 85/frame).
#[test]
fn rotate_ramp_is_linear_at_the_op_budget_rate() {
    // Increasing to LUT index 6 (engine 0x400 from a 0xE00 start:
    // arc (0x400 - 0xE00) mod 0x1000 = 0x600), budget 18, speed 2.
    let bc = [0x38, 0x06, 0x12];
    let mut s = MotionState {
        yaw: 0xE00,
        speed: 2,
        ..Default::default()
    };
    let mut incs = Vec::new();
    let mut prev = s.yaw;
    loop {
        let r = step(&mut s, tgt(0, 0, 0), &bc);
        if r == StepResult::Done {
            break;
        }
        incs.push(s.yaw.wrapping_sub(prev));
        prev = s.yaw;
    }
    assert_eq!(
        incs,
        vec![170, 170, 170, 171, 171, 171, 171, 171],
        "per-tick increments follow arc*speed/remaining with floor"
    );
    assert_eq!(s.yaw, 0x400, "terminal compass snap");
}

#[test]
fn step_face_target_computes_bearing() {
    let mut s = MotionState {
        world_x: 0,
        world_z: 0,
        yaw: 0,
        speed: 100,
        op_accum: 0,
        pc: 0,
        ..Default::default()
    };
    // 0x4C, body0=0x85, body1=0x10, body2=0x00, body3=0xF8
    // target at (100, 0, 0) → atan2(dx=100, dz=0) = π/2 = 0x400
    let bc = [0x4C, 0x85, 0x10, 0x00, 0xF8];
    let t = MotionTarget {
        x: 100,
        y: 0,
        z: 0,
        id: 0,
    };
    let r = step(&mut s, t, &bc);
    // With high speed and small budget, should complete in one step
    assert_eq!(r, StepResult::Done);
    assert_eq!(s.yaw, 0x0400);
}

#[test]
fn step_face_target_invalid_submode_is_done() {
    let mut s = st(0, 0, 1);
    let bc = [0x4C, 0x00, 0x01, 0x00, 0xF8];
    assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Done);
    assert_eq!(s.pc, 5);
}

#[test]
fn step_unknown_opcode_terminates() {
    let mut s = st(0, 0, 1);
    let bc = [0x10];
    assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Done);
}

#[test]
fn step_at_buffer_end_is_immediate_done() {
    let mut s = MotionState {
        pc: 5,
        ..Default::default()
    };
    let bc = [0x47];
    assert_eq!(step(&mut s, tgt(0, 0, 0), &bc), StepResult::Done);
}

#[test]
fn target_selector_decodes_player_linked_and_id_forms() {
    assert_eq!(target_selector(&[0x47], 0, 3), TargetSelector::Own);
    assert_eq!(target_selector(&[0xC7, 0xF8], 0, 3), TargetSelector::Player);
    assert_eq!(
        target_selector(&[0xC7, 0xFB], 0, 3),
        TargetSelector::WorldMapEntity
    );
    assert_eq!(target_selector(&[0xC7, 0x05], 0, 3), TargetSelector::Id(5));
    // The own-id compare runs first, even for the special ids.
    assert_eq!(target_selector(&[0xC7, 0x05], 0, 5), TargetSelector::Own);
    assert_eq!(target_selector(&[0xC7, 0xF8], 0, 0xF8), TargetSelector::Own);
}

#[test]
fn target_select_bit_skips_target_byte() {
    let mut s = st(0, 0, 5);
    let t = tgt(2, 0, 0);
    // 0x47 + high bit -> consumes a target byte. With speed > distance,
    // a single step lands on target -> Done -> PC moves past op + target.
    let bc = [0x47 | 0x80, 0xFB];
    assert_eq!(step(&mut s, t, &bc), StepResult::Done);
    assert_eq!(s.pc, 2, "PC should advance past op byte + target byte");
}

/// Validate a complete NPC-patrol-style script: move toward a waypoint,
/// then face it. This covers the full opcode sequence a field-scene NPC
/// would execute during one patrol leg, verifying that:
///
/// - `MoveTowardTarget` (0x47) yields while approaching and completes
///   when the actor reaches the waypoint.
/// - `FaceTarget` (0x4C, sub-mode 0x85) yields while rotating and
///   snaps to the computed bearing when the budget runs out.
/// - PC advances correctly through both opcodes in sequence.
///
/// Algorithm verified against `FUN_8003774C` in SCUS_942.54
/// (`ghidra/scripts/funcs/8003774c.txt`).
#[test]
fn patrol_leg_move_then_face_sequence() {
    // Actor starts at (0, 0). Waypoint at (6, 0). Speed 3 per frame.
    let mut s = MotionState {
        world_x: 0,
        world_z: 0,
        speed: 3,
        yaw: 0,
        op_accum: 0,
        pc: 0,
        ..Default::default()
    };
    let waypoint = tgt(6, 0, 0);

    // Script: 0x47 MoveTowardTarget (1 byte), then 0x4C FaceTarget sub-mode 0x85.
    // FaceTarget body: [body0=0x85, body1=budget_lo=8, body2=budget_hi=0, body3=0xF8].
    let bc = [
        0x47, // op: MoveTowardTarget
        0x4C, // op: FaceTarget
        0x85, // body0: sub-mode (0x85 = valid rotate yaw)
        0x08, // body1: budget low byte = 8
        0x00, // body2: budget high byte = 0
        0xF8, // body3: target id (self)
    ];

    // Frame 1: x += 3, z += 0. Still short of target (need 6). Yield.
    assert_eq!(step(&mut s, waypoint, &bc), StepResult::Yield);
    assert_eq!(s.world_x, 3);
    assert_eq!(s.pc, 0, "PC stays on op while yielding");

    // Frame 2: x += 3. Arrives at x=6. Done. PC advances to next op.
    assert_eq!(step(&mut s, waypoint, &bc), StepResult::Done);
    assert_eq!(s.world_x, 6);
    assert_eq!(s.pc, 1, "PC moved past MoveTowardTarget op");

    // Now FaceTarget: waypoint is (6, 0, 0), actor is at (6, 0, 0).
    // dx = 6 - 6 = 0, dz = 0 - 0 = 0. atan2(0, 0) = 0. target_yaw = 0.
    // Budget = 8, speed = 3. remaining = 8 - 0 = 8 > 3 → Yield.
    // Step: ccw_dist = 0, cw_dist = 0 → clockwise=false, delta=0, step=0.
    assert_eq!(step(&mut s, waypoint, &bc), StepResult::Yield);
    assert_eq!(s.pc, 1, "PC stays on FaceTarget while yielding");

    // Frame 4: op_accum = 3, remaining = 5 > 3 → Yield.
    assert_eq!(step(&mut s, waypoint, &bc), StepResult::Yield);

    // Frame 5: op_accum = 6, remaining = 2 <= 3 → Done. Snap to bearing.
    assert_eq!(step(&mut s, waypoint, &bc), StepResult::Done);
    assert_eq!(s.yaw, 0, "yaw snapped to computed bearing");
    assert_eq!(s.op_accum, 0, "accumulator reset on Done");
    assert_eq!(s.pc, 6, "PC advanced past FaceTarget body (4 bytes)");
}

/// The eight compass entries of the retail heading LUT `DAT_80073F04`,
/// expressed in the engine's `0` = +Z space. Indices past the eight real
/// direction entries have no heading.
#[test]
fn heading_lut_covers_the_compass_and_stops_at_eight() {
    assert_eq!(heading_lut_engine(0), Some(0x800), "index 0 = -Z");
    assert_eq!(heading_lut_engine(2), Some(0xC00), "index 2 = -X");
    assert_eq!(heading_lut_engine(4), Some(0x000), "index 4 = +Z");
    assert_eq!(heading_lut_engine(6), Some(0x400), "index 6 = +X");
    // 45 degrees per index, in order, all the way round.
    for i in 0..8u8 {
        let expect = (u16::from(i) * 0x200 + 0x800) & 0x0FFF;
        assert_eq!(heading_lut_engine(i), Some(expect), "index {i}");
    }
    for i in 8..16u8 {
        assert_eq!(heading_lut_engine(i), None, "index {i} is not a direction");
    }
}

/// The walk-direction-implied facing table retail derives from the step's
/// axis signs (`0x80037D4C`). Cardinal steps land on the even indices,
/// diagonals on the odd ones between them.
#[test]
fn walk_facing_index_matches_the_retail_sign_table() {
    // (dx, dz) -> LUT index.
    let cases: [(i32, i32, u8); 8] = [
        (0, -1, 0),  // -Z
        (-1, -1, 1), // -X -Z
        (-1, 0, 2),  // -X
        (-1, 1, 3),  // -X +Z
        (0, 1, 4),   // +Z
        (1, 1, 5),   // +X +Z
        (1, 0, 6),   // +X
        (1, -1, 7),  // +X -Z
    ];
    for (dx, dz, idx) in cases {
        assert_eq!(walk_facing_index(dx, dz), Some(idx), "({dx}, {dz})");
        // Magnitude never matters - only the signs.
        assert_eq!(
            walk_facing_index(dx * 97, dz * 3),
            Some(idx),
            "scaled ({dx}, {dz})"
        );
    }
    assert_eq!(
        walk_facing_index(0, 0),
        None,
        "a standing actor keeps its facing"
    );
}

/// A walking actor's facing is a **snap**, not a ramp: the leg's first
/// moving frame writes one of the eight compass headings outright.
#[test]
fn move_toward_target_snaps_facing_to_the_compass() {
    for (tx, tz, want) in [
        (100i16, 0i16, 0x400u16), // +X
        (-100, 0, 0xC00),         // -X
        (0, 100, 0x000),          // +Z
        (0, -100, 0x800),         // -Z
    ] {
        let mut s = st(0, 0, 4);
        s.yaw = 0x111; // an angle the compass can never produce
        let bc = [0x47];
        assert_eq!(step(&mut s, tgt(tx, 0, tz), &bc), StepResult::Yield);
        assert_eq!(s.yaw, want, "walking toward ({tx}, {tz})");
    }
}

/// Retail closes the **dominant axis first** and only cuts the diagonal
/// once the two remaining deltas are equal - so a walking NPC holds a
/// cardinal facing for most of a leg and turns to a diagonal late,
/// instead of moving (and facing) diagonally from frame one.
#[test]
fn move_toward_target_closes_dominant_axis_first() {
    let mut s = st(0, 0, 4);
    let t = tgt(100, 0, 20);
    let bc = [0x47];
    // 20 steps of 4 close X from 0 to 80, leaving |dx| == |dz| == 20.
    for i in 0..20 {
        assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
        assert_eq!(s.world_z, 0, "Z is untouched while X dominates (step {i})");
        assert_eq!(s.yaw, 0x400, "facing +X while closing X (step {i})");
    }
    assert_eq!((s.world_x, s.world_z), (80, 0));
    // Deltas are now equal: the leg cuts the diagonal and the facing turns.
    assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
    assert_eq!(
        (s.world_x, s.world_z),
        (84, 4),
        "both axes advance together"
    );
    assert_eq!(s.yaw, 0x200, "facing the +X +Z diagonal");
}

/// A non-zero approach mode masks the open axes at the full step
/// (`and a3,v1,a1` at `0x80037C48`): mode `3` walks the diagonal from the
/// first frame and ends the leg on the longer axis alone, facing along
/// it - `kor5` P2[5]'s `C7 15 12 29 33` return walk lands its cast facing
/// `-X`, not the `-X +Z` diagonal the default law would leave.
#[test]
fn move_toward_target_approach_mode_masks_the_axes() {
    let mut s = st(0, 0, 4);
    s.approach = 3;
    let t = tgt(100, 0, 20);
    let bc = [0x47];
    for _ in 0..5 {
        assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
    }
    assert_eq!((s.world_x, s.world_z), (20, 20), "diagonal from frame one");
    assert_eq!(s.yaw, 0x200, "facing the +X +Z diagonal");
    assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
    assert_eq!((s.world_x, s.world_z), (24, 20), "Z closed, X alone");
    assert_eq!(s.yaw, 0x400, "facing +X for the rest of the leg");
    // Mode 2 closes Z first.
    let mut s = st(0, 0, 4);
    s.approach = 2;
    assert_eq!(step(&mut s, t, &bc), StepResult::Yield);
    assert_eq!((s.world_x, s.world_z), (0, 4));
}

/// Validate that RotateToAngle reaches its target yaw monotonically and
/// stops exactly at the table entry, regardless of budget pacing.
#[test]
fn rotate_to_angle_reaches_target_monotonically() {
    // LUT index 5 -> engine yaw 0x200. Budget = 20 frames, force bit
    // clear, so the ramp increases from 0x000 to 0x200 and lands exactly
    // on the table entry via the terminal-frame snap.
    let bc = [0x38, 0x05, 0x14]; // op, body0 = index 5, body1 = budget 20
    let mut s = MotionState {
        yaw: 0x000,
        speed: 4,
        op_accum: 0,
        pc: 0,
        ..Default::default()
    };
    let mut prev_yaw = s.yaw as i32;
    let mut steps = 0usize;
    loop {
        let r = step(&mut s, tgt(0, 0, 0), &bc);
        steps += 1;
        // Yaw should be moving CCW (increasing) toward 0x200.
        let cur = s.yaw as i32;
        // Allow the last snap (which sets exactly 0x200) to break monotonicity
        // only on Done.
        if r == StepResult::Yield {
            assert!(
                cur >= prev_yaw,
                "yaw should advance CCW on each Yield step: {} → {}",
                prev_yaw,
                cur
            );
        }
        prev_yaw = cur;
        if r == StepResult::Done {
            break;
        }
        assert!(steps < 50, "should converge in fewer than 50 steps");
    }
    assert_eq!(s.yaw, 0x0200, "final yaw must equal table entry");
    assert_eq!(s.op_accum, 0, "accumulator must reset on Done");
    assert_eq!(s.pc, 3, "PC advanced past 3-byte op");
}

/// Four bind records, stride 4. Record 2 carries the suppress class.
fn bind_records() -> Vec<u8> {
    let mut t = vec![0u8; 4 * BIND_RECORD_STRIDE];
    t[0] = 0x01;
    t[BIND_RECORD_STRIDE] = 0x00;
    t[2 * BIND_RECORD_STRIDE] = TOUCH_POST_SUPPRESS_CLASS;
    t[3 * BIND_RECORD_STRIDE] = 0xFF;
    t
}

#[test]
fn touch_post_stores_index_for_ordinary_records() {
    let t = bind_records();
    assert_eq!(post_touch(&t, 0), Some(0));
    // A zero class byte is ordinary - only 0x8C suppresses.
    assert_eq!(post_touch(&t, 1), Some(1));
    assert_eq!(post_touch(&t, 3), Some(3));
}

#[test]
fn touch_post_suppressed_by_class_8c() {
    let t = bind_records();
    assert_eq!(
        post_touch(&t, 2),
        None,
        "class 0x8C must leave the mailbox untouched"
    );
}

#[test]
fn touch_post_reads_class_at_record_stride() {
    // Only the first byte of each 4-byte record is the class; a 0x8C
    // sitting in a later byte of record 0 must not suppress it.
    let mut t = vec![0u8; 2 * BIND_RECORD_STRIDE];
    t[1] = TOUCH_POST_SUPPRESS_CLASS;
    t[2] = TOUCH_POST_SUPPRESS_CLASS;
    t[3] = TOUCH_POST_SUPPRESS_CLASS;
    assert_eq!(post_touch(&t, 0), Some(0));
}

#[test]
fn touch_post_out_of_range_does_not_post() {
    let t = bind_records();
    assert_eq!(post_touch(&t, 4), None);
    assert_eq!(post_touch(&t, usize::MAX), None);
}
