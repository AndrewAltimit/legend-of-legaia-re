use super::*;

#[test]
fn compass_lut_is_retail_space() {
    assert_eq!(heading_lut_retail(0), 0x000);
    assert_eq!(heading_lut_retail(2), 0x400);
    assert_eq!(heading_lut_retail(7), 0xE00);
    // Ops index it `& 7`.
    assert_eq!(heading_lut_retail(0x8F), heading_lut_retail(7));
}

#[test]
fn op04_ramps_over_its_budget_and_snaps_exactly() {
    // `[04, lut=2 increasing, 8 frames]`: 0x000 -> 0x400 in 8 ticks.
    let code = [0x04u8, 0x02, 0x08];
    let mut vm = AmbientMotion::new(1, 0x000);
    let mut headings = Vec::new();
    for _ in 0..8 {
        assert_eq!(vm.tick(&code, 1), AmbientTick::Yield);
        headings.push(vm.heading);
    }
    // Eight equal steps of 0x400/8 = 0x80.
    assert_eq!(
        headings,
        vec![0x080, 0x100, 0x180, 0x200, 0x280, 0x300, 0x380, 0x400]
    );
    // The ninth tick is the terminal snap, which does NOT consume the
    // frame: PC has advanced past the op.
    vm.tick(&code, 1);
    assert_eq!(vm.heading, 0x400);
    assert_eq!(vm.pc, 3);
    assert_eq!(vm.cursor, 0);
}

#[test]
fn op04_cursor_is_unit_per_tick_not_frame_scaled() {
    // A frame scalar of 4 must not shorten the leg: retail's `0x04`
    // cursor is `addiu a0, a0, 1`.
    let code = [0x04u8, 0x02, 0x08];
    let mut vm = AmbientMotion::new(1, 0x000);
    for _ in 0..8 {
        assert_eq!(vm.tick(&code, 4), AmbientTick::Yield);
    }
    assert_eq!(vm.pc, 0, "still mid-leg after 8 ticks at speed 4");
    vm.tick(&code, 4);
    assert_eq!(vm.pc, 3);
}

#[test]
fn op04_decreasing_holds_raw_wrapped_headings() {
    // 0x100 -> 0x000 decreasing is a 0x100 arc; but force the LONG way
    // by aiming at 0x200 decreasing: arc = (0x100 - 0x200) mod 0x1000
    // = 0xF00, so the heading walks down through zero and holds 0xFxxx
    // raw until the snap.
    let code = [0x04u8, 0x81, 0x06]; // lut 1 = 0x200, decreasing, 6 frames
    let mut vm = AmbientMotion::new(1, 0x100);
    let mut raw = Vec::new();
    for _ in 0..6 {
        vm.tick(&code, 1);
        raw.push(vm.heading);
    }
    assert!(
        raw.iter().any(|h| *h > 0x0FFF),
        "a decreasing ramp through zero must hold raw >0xFFF headings: {raw:04X?}"
    );
    // The raw hold is invisible to a renderer: masking recovers the
    // heading the ramp is actually pointing at.
    assert!(
        raw.iter().any(|h| *h != *h & 0x0FFF),
        "at least one tick's raw value differs from its rendered form"
    );
    vm.tick(&code, 1);
    assert_eq!(vm.heading, 0x200, "terminal snap lands back in range");
}

#[test]
fn op04_has_no_shortest_arc_choice() {
    // 0x000 -> 0x200 with the decreasing bit set takes the 0xE00 long
    // way, not the 0x200 short one.
    let code = [0x04u8, 0x81, 0x0E];
    let mut vm = AmbientMotion::new(1, 0x000);
    vm.tick(&code, 1);
    // First step is 0xE00 / 14 = 0x100 *downward*.
    assert_eq!(vm.heading, 0x000u16.wrapping_sub(0x100));
}

#[test]
fn op0d_pre_unwraps_then_tweens_to_the_compass_point() {
    // Increasing from 0xE00 to 0x200: retail parks the start at
    // 0xE00 - 0x1000 = 0xFE00 (raw) so the lerp runs upward through the
    // wrap.
    let code = [0x0Du8, 0x01, 0x10, 0x00]; // lut 1 = 0x200, increasing, 16 frames
    let mut vm = AmbientMotion::new(1, 0x0E00);
    vm.tick(&code, 1);
    assert!(vm.ramps.is_driving(RAMP_DEST_HEADING));
    // Start value is the pre-unwrapped one.
    let mut seen_out_of_range = false;
    for _ in 0..40 {
        if vm.heading > 0x0FFF {
            seen_out_of_range = true;
        }
        vm.tick(&code, 1);
        if usize::from(vm.pc) >= code.len() {
            break;
        }
    }
    assert!(
        seen_out_of_range,
        "the tween runs on the pre-unwrap value, which is out of range"
    );
    assert_eq!(vm.heading, 0x200, "endpoint is the compass entry verbatim");
    assert_eq!(vm.ramps.active(), 0, "slot freed on retirement");
}

#[test]
fn op0d_cursor_is_frame_scaled_and_retires_with_its_ramp() {
    // Duration 32 at speed 4 -> 8 scheduler ticks; the op retires one
    // tick later (its terminal arm re-reads the cursor).
    let code = [0x0Du8, 0x02, 0x20, 0x00];
    let mut vm = AmbientMotion::new(1, 0x000);
    let mut ticks = 0;
    for _ in 0..64 {
        vm.tick(&code, 4);
        ticks += 1;
        if usize::from(vm.pc) >= code.len() {
            break;
        }
    }
    assert_eq!(ticks, 9, "32/4 = 8 stepping ticks + the terminal one");
    assert_eq!(vm.heading, 0x400);
}

#[test]
fn op0d_zero_duration_is_an_instant_compass_write() {
    let code = [0x0Du8, 0x03, 0x00, 0x00];
    let mut vm = AmbientMotion::new(1, 0x000);
    vm.tick(&code, 1);
    assert_eq!(vm.heading, heading_lut_retail(3));
    assert_eq!(vm.pc, 4);
}

#[test]
fn op05_wait_consumes_frames_at_the_frame_scalar() {
    let code = [0x05u8, 0x08, 0x01]; // wait 8, then op 0x01 restart
    let mut vm = AmbientMotion::new(1, 0);
    for _ in 0..4 {
        assert_eq!(vm.tick(&code, 2), AmbientTick::Yield);
    }
    assert_eq!(vm.pc, 2, "8 frames at scalar 2 = 4 ticks");
}

/// The `0x05` arm's touch-wake head (`0x8003882C`): a posted contact
/// naming this actor ends the wait on the frame it arrives, instead of
/// after the authored countdown.
#[test]
fn op05_wait_is_cut_short_by_a_posted_touch() {
    let code = [0x05u8, 0x60, 0x01]; // wait 0x60 ticks, then restart
    let mut vm = AmbientMotion::new(7, 0);
    vm.default_move = [0x03, 0x00]; // op 0x17 installed a default move
    for _ in 0..4 {
        assert_eq!(vm.tick(&code, 1), AmbientTick::Yield);
    }
    assert_eq!(vm.pc, 0, "still parked in the wait");

    vm.pending_touch = Some(7); // the collision probe posted this actor
    vm.tick(&code, 1);
    assert_eq!(vm.pc, 2, "the touch expires the wait this frame");
    assert_eq!(vm.cursor, 0);
    assert_eq!(vm.pending_touch, None, "retail stores the 0xFF sentinel");
}

/// The head's three gates, each of which leaves the countdown running.
#[test]
fn a_touch_that_fails_a_gate_leaves_the_wait_alone() {
    let code = [0x05u8, 0x60, 0x01];
    // Gate 2: the post names a different actor.
    let mut other = AmbientMotion::new(7, 0);
    other.default_move = [0x03, 0x00];
    other.pending_touch = Some(9);
    other.tick(&code, 1);
    assert_eq!(other.pc, 0, "a post for another actor is not ours");
    assert_eq!(other.pending_touch, Some(9), "and is not consumed");

    // Gate 3: no default move installed - the `0x801C6470` sentinel.
    let mut unset = AmbientMotion::new(7, 0);
    assert_eq!(unset.default_move[0], DEFAULT_MOVE_UNSET);
    unset.pending_touch = Some(7);
    unset.tick(&code, 1);
    assert_eq!(unset.pc, 0, "a sentinel record never wakes on a bump");

    // Gate 1: an empty mailbox is the ordinary case.
    let mut idle = AmbientMotion::new(7, 0);
    idle.default_move = [0x03, 0x00];
    idle.tick(&code, 1);
    assert_eq!(idle.pc, 0);
}

/// The producer's guard and the consumer's third gate read the same
/// `0x801C6470` byte, so a suppressed post can never wake a wait.
#[test]
fn the_post_guard_and_the_wake_gate_agree_on_the_arena_byte() {
    let mut arena = vec![crate::motion_vm::TOUCH_POST_SUPPRESS_CLASS; 8 * 4];
    arena[3 * 4] = 0x03; // slot 3 has a default move; slot 5 does not
    assert_eq!(crate::motion_vm::post_touch(&arena, 3), Some(3));
    assert_eq!(crate::motion_vm::post_touch(&arena, 5), None);

    let code = [0x05u8, 0x60, 0x01];
    let mut vm = AmbientMotion::new(3, 0);
    vm.default_move = [arena[3 * 4], 0];
    vm.pending_touch = crate::motion_vm::post_touch(&arena, 3);
    vm.tick(&code, 1);
    assert_eq!(vm.pc, 2);
}

#[test]
fn op01_restarts_the_variant_without_consuming_the_tick() {
    // wait(2) then restart: the restart must loop straight back into
    // the wait in the same tick, so the stream never stalls.
    let code = [0x05u8, 0x02, 0x01];
    let mut vm = AmbientMotion::new(1, 0);
    for _ in 0..6 {
        assert_eq!(vm.tick(&code, 1), AmbientTick::Yield);
    }
    assert!(usize::from(vm.pc) < code.len());
}

#[test]
fn unmodelled_ops_are_stepped_over_by_width() {
    // 0x07 SET flag (width 3) then a facing ramp: the facing channel
    // must reach the ramp.
    let code = [0x07u8, 0x10, 0x00, 0x04, 0x02, 0x04];
    let mut vm = AmbientMotion::new(1, 0x000);
    vm.tick(&code, 1);
    assert_eq!(vm.pc, 3, "stepped past the flag op into the ramp");
    assert_ne!(vm.heading, 0x000);
}

/// `b1` high nibble = heading-LUT index, low nibble = pace; `b2 & 0x3F`
/// = tiles.
fn dir_step(op: u8, lut: u8, bits: u8, tiles: u8) -> [u8; 3] {
    [op, (lut << 4) | (bits & 0x0F), tiles & 0x3F]
}

#[test]
fn dir_step_walks_one_tile_per_operand_tile_at_any_pace() {
    // The distance a leg covers is `(b2 & 0x3F) * 0x80` whatever the
    // pace selector is - `budget * step` is pace-invariant.
    for bits in 0..4u8 {
        // LUT 4 = +Z.
        let code = dir_step(0x03, 4, bits, 3);
        let mut vm = AmbientMotion::new(1, 0).with_position(1000, 2000);
        let mut ticks = 0;
        while usize::from(vm.pc) < code.len() && ticks < 10_000 {
            vm.tick(&code, 1);
            ticks += 1;
        }
        assert_eq!(vm.x, 1000, "pure +Z leg leaves X alone");
        assert_eq!(vm.z, 2000 + 3 * 0x80, "bits={bits}: three tiles of +Z");
        assert_eq!(ticks, (3u32 << (u32::from(bits) + 2)) as usize);
        assert_eq!(vm.heading, 0x800, "0x03 snaps the facing to LUT[4]");
    }
}

#[test]
fn dir_step_0x19_moves_without_touching_the_facing() {
    let code = dir_step(0x19, 6, 0, 1); // LUT 6 = +X
    let mut vm = AmbientMotion::new(1, 0x123).with_position(0, 0);
    while usize::from(vm.pc) < code.len() {
        vm.tick(&code, 1);
    }
    assert_eq!((vm.x, vm.z), (0x80, 0));
    assert_eq!(vm.heading, 0x123, "0x19 leaves the heading standing");
}

#[test]
fn dir_step_0x20_halves_the_budget() {
    let code = dir_step(0x20, 4, 0, 2);
    let mut vm = AmbientMotion::new(1, 0).with_position(0, 0);
    while usize::from(vm.pc) < code.len() {
        vm.tick(&code, 1);
    }
    assert_eq!(vm.z, 0x80, "two tiles authored, one tile walked");
}

#[test]
fn dir_step_cursor_is_unit_per_tick_not_frame_scaled() {
    // The `0x04` asymmetry applies to the walk ops too: the leg is the
    // same number of ticks at any frame scalar, so a cadence change
    // cannot change how far a leg travels.
    let code = dir_step(0x03, 4, 1, 2);
    for speed in [1u8, 2, 4] {
        let mut vm = AmbientMotion::new(1, 0).with_position(0, 0);
        let mut ticks = 0;
        while usize::from(vm.pc) < code.len() && ticks < 1000 {
            vm.tick(&code, speed);
            ticks += 1;
        }
        assert_eq!(ticks, 2 << 3, "speed={speed}");
        assert_eq!(vm.z, 2 * 0x80, "speed={speed}");
    }
}

#[test]
fn dir_step_blocked_holds_position_and_pc() {
    let code = dir_step(0x03, 4, 0, 4);
    let mut vm = AmbientMotion::new(1, 0).with_position(500, 500);
    for _ in 0..50 {
        assert_eq!(vm.tick_with(&code, 1, &AlwaysBlocks), AmbientTick::Yield);
    }
    assert_eq!((vm.x, vm.z), (500, 500), "a blocked step never commits");
    assert_eq!(vm.pc, 0, "and never retires the op");
    assert_eq!(vm.cursor & 0xFF, 0, "nor burns budget");
}

/// `[18, min_x, min_z, max_x, max_z]` over a tile-space box, pace `bits`
/// scattered over the four high bits.
fn wander(min_x: u8, min_z: u8, max_x: u8, max_z: u8, bits: u8) -> [u8; 5] {
    [
        0x18,
        min_x | ((bits & 8) << 4),
        min_z | ((bits & 4) << 5),
        max_x | ((bits & 2) << 6),
        max_z | ((bits & 1) << 7),
    ]
}

#[test]
fn wander_bits_round_trip_the_scatter() {
    for bits in 0..16u8 {
        let b = wander(0x10, 0x11, 0x20, 0x21, bits);
        let got = ((b[1] & 0x80) >> 4) | ((b[2] & 0x80) >> 5) | ((b[3] & 0x80) >> 6) | (b[4] >> 7);
        assert_eq!(got, bits);
    }
}

#[test]
fn wander_moves_the_actor_and_stays_inside_its_box() {
    let code = wander(0x10, 0x10, 0x18, 0x18, 3);
    let (min, max) = (0x10 * 0x80 + 0x40, 0x18 * 0x80 + 0x40);
    let mut vm = AmbientMotion::new(1, 0).with_position(0x14 * 0x80 + 0x40, 0x14 * 0x80 + 0x40);
    let start = (vm.x, vm.z);
    let mut moved_ticks = 0;
    for _ in 0..4000 {
        // Retail loops the stream; the op retires often, so re-arm it.
        if usize::from(vm.pc) >= code.len() {
            vm.pc = 0;
        }
        vm.tick(&code, 1);
        if vm.moved {
            moved_ticks += 1;
        }
        assert!(
            i32::from(vm.x) >= min - 0x40 && i32::from(vm.x) <= max + 0x40,
            "X {} escaped the box",
            vm.x
        );
        assert!(
            i32::from(vm.z) >= min - 0x40 && i32::from(vm.z) <= max + 0x40,
            "Z {} escaped the box",
            vm.z
        );
    }
    assert!(moved_ticks > 100, "the villager actually wanders");
    assert_ne!((vm.x, vm.z), start);
}

#[test]
fn wander_outside_its_box_retires_without_moving() {
    let code = wander(0x10, 0x10, 0x18, 0x18, 3);
    let mut vm = AmbientMotion::new(1, 0).with_position(0, 0);
    vm.tick(&code, 1);
    assert_eq!((vm.x, vm.z), (0, 0));
    assert_eq!(vm.pc, 5, "op skipped entirely");
}

#[test]
fn wander_turn_takes_the_shortest_arc() {
    // The wander's turn phase differs from `0x04`/`0x0D`: it picks the
    // short way round rather than an authored direction. Facing +Z
    // (0x800), a turn to -Z (0x000) may go either way, so use a
    // three-eighths case: from LUT 6 (+X, 0xC00) to LUT 0 (-Z, 0x000)
    // the short arc is increasing through the 0x1000 wrap.
    let mut vm = AmbientMotion::new(1, 0xC00).with_position(0, 0);
    vm.wander_dir = 0;
    vm.wander_phase = 1;
    // A live turn phase always carries a non-zero cursor (retail's phase
    // 1 writes 1 into it); a zero cursor would re-arm the entry guard.
    vm.cursor = 1;
    let code = wander(0x00, 0x00, 0x7F, 0x7F, 3);
    let mut seen = Vec::new();
    for _ in 0..64 {
        vm.tick(&code, 1);
        seen.push(vm.heading);
        if vm.wander_phase != 1 {
            break;
        }
    }
    assert_eq!(*seen.last().unwrap(), 0x000, "lands on the compass point");
    // Increasing through the wrap: every sample is >= 0xC00 until it
    // wraps to a small value, never dipping through 0x800.
    assert!(
        seen.iter().all(|&h| h >= 0xC00 || h <= 0x100),
        "took the long way round: {seen:04X?}"
    );
}

#[test]
fn wander_masks_the_heading_every_tick() {
    // Unlike `0x04` / `0x0D`, no raw out-of-range heading is ever held.
    let code = wander(0x00, 0x00, 0x7F, 0x7F, 0);
    let mut vm = AmbientMotion::new(7, 0xC00).with_position(0x40 * 0x80, 0x40 * 0x80);
    for _ in 0..2000 {
        if usize::from(vm.pc) >= code.len() {
            vm.pc = 0;
        }
        vm.tick(&code, 1);
        assert!(vm.heading <= 0x0FFF, "raw heading {:#X} held", vm.heading);
    }
}

#[test]
fn wander_walk_segment_is_half_a_tile_at_any_pace() {
    for bits in 0..4u8 {
        let mut vm = AmbientMotion::new(1, 0x800).with_position(0x40 * 0x80, 0x40 * 0x80);
        vm.wander_dir = 4; // +Z, already faced
        vm.wander_phase = 1;
        vm.cursor = 1;
        let code = wander(0x00, 0x00, 0x7F, 0x7F, bits);
        let z0 = vm.z;
        // One tick to clear the (no-op) turn phase, then the segment.
        for _ in 0..(2u32 << bits) + 1 {
            vm.tick(&code, 1);
        }
        assert_eq!(i32::from(vm.z) - i32::from(z0), 0x40, "bits={bits}");
    }
}

#[test]
fn op17_installs_the_default_move_without_consuming_the_tick() {
    // `0x17` then a `0x05` wait: both run in the same tick.
    let code = [0x17u8, 0x0A, 0x0B, 0x05, 0x04];
    let mut vm = AmbientMotion::new(1, 0);
    vm.tick(&code, 1);
    assert_eq!(vm.default_move, [0x0A, 0x0B]);
    assert_eq!(vm.pc, 3, "stepped into the wait");
}

#[test]
fn walk_ops_restamp_the_requested_move() {
    let code = [0x17u8, 0x0A, 0x0B, 0x03, 0x40, 0x01];
    let mut vm = AmbientMotion::new(1, 0).with_position(0, 0);
    vm.tick(&code, 1);
    assert_eq!(
        vm.requested_move,
        Some(0x0B),
        "the step prologue stamps the record's anim byte"
    );
}

#[test]
fn op04_stamps_the_anim_byte_while_turning_and_the_move_byte_on_exit() {
    // `0x17` installs [move 0x0A, anim 0x0B]; `0x04` turns over 2 frames.
    let code = [0x17u8, 0x0A, 0x0B, 0x04, 0x02, 0x02, 0x05, 0x40];
    let mut vm = AmbientMotion::new(1, 0);
    vm.tick(&code, 1);
    assert_eq!(vm.requested_move, Some(0x0B), "stepping: anim byte");
    vm.tick(&code, 1);
    assert_eq!(vm.requested_move, Some(0x0B));
    vm.tick(&code, 1); // terminal: snap, epilogue restamp, fall through
    assert_eq!(vm.requested_move, Some(0x0A), "exit: move byte");
    // No record installed: neither restamp fires.
    let mut bare = AmbientMotion::new(1, 0);
    let code = [0x04u8, 0x02, 0x01, 0x05, 0x40];
    bare.tick(&code, 1);
    bare.tick(&code, 1);
    assert_eq!(bare.requested_move, None);
}

#[test]
fn scheduler_lerp_matches_the_retail_form() {
    // value = end + (start - end) * remaining / total, truncating.
    assert_eq!(lerp_remaining(0, 100, 100, 100), 0);
    assert_eq!(lerp_remaining(0, 100, 50, 100), 50);
    assert_eq!(lerp_remaining(0, 100, 1, 100), 99);
    assert_eq!(lerp_remaining(0, 100, 0, 100), 100);
    // Truncation is toward zero, so a descending ramp rounds up.
    assert_eq!(lerp_remaining(100, 0, 33, 100), 33);
}

#[test]
fn scheduler_overflows_instead_of_evicting() {
    let mut s = RampScheduler::new();
    let r = Ramp {
        dest: 1,
        owner: 1,
        start: 0,
        end: 1,
        total: 100,
        remaining: 100,
        kind: RampKind::U16,
    };
    for _ in 0..RAMP_SLOTS {
        assert!(s.install(r).is_some());
    }
    assert!(s.install(r).is_none());
    assert_eq!(s.overflow, 1);
    assert_eq!(s.active(), RAMP_SLOTS);
}

#[test]
fn pool_reset_frees_every_slot_and_the_overflow_counter() {
    let mut s = RampScheduler::new();
    let r = Ramp {
        dest: 1,
        owner: 7,
        start: 0,
        end: 1,
        total: 100,
        remaining: 100,
        kind: RampKind::U16,
    };
    for _ in 0..RAMP_SLOTS {
        s.install(r);
    }
    assert!(s.install(r).is_none());
    assert_eq!((s.active(), s.overflow), (RAMP_SLOTS, 1));

    s.reset_pool();
    assert_eq!((s.active(), s.overflow), (0, 0));
    // Slot 0 stays the list header, so the pool is still 63 wide - not 64.
    for _ in 0..RAMP_SLOTS {
        assert!(s.install(r).is_some());
    }
    assert!(s.install(r).is_none());
}

fn zone_ramp_fixture() -> ZoneRamp {
    ZoneRamp {
        start: 0,
        end: 0x100,
        x_lo: -100,
        x_hi: 100,
        z_lo: 0,
        z_hi: 0x40,
        kind: 2,
    }
}

#[test]
fn zone_ramp_interpolates_on_the_player_z_inside_the_box() {
    let r = zone_ramp_fixture();
    assert_eq!(
        zone_ramp_tick(&r, 0, 0, false, false),
        ZoneRampTick::Write {
            value: 0,
            width: ZoneRampWidth::U16
        }
    );
    assert_eq!(
        zone_ramp_tick(&r, 0, 0x20, false, false),
        ZoneRampTick::Write {
            value: 0x80,
            width: ZoneRampWidth::U16
        }
    );
    assert_eq!(
        zone_ramp_tick(&r, 0, 0x40, false, false),
        ZoneRampTick::Write {
            value: 0x100,
            width: ZoneRampWidth::U16
        }
    );
    // X is a gate only - it never moves the value.
    assert_eq!(
        zone_ramp_tick(&r, 99, 0x20, false, false),
        zone_ramp_tick(&r, -99, 0x20, false, false)
    );
}

#[test]
fn zone_ramp_gates_reject_before_any_arithmetic() {
    let r = zone_ramp_fixture();
    // Both global gates, then each AABB edge - all four `bnez` exits.
    assert_eq!(zone_ramp_tick(&r, 0, 0x20, true, false), ZoneRampTick::Idle);
    assert_eq!(zone_ramp_tick(&r, 0, 0x20, false, true), ZoneRampTick::Idle);
    assert_eq!(
        zone_ramp_tick(&r, -101, 0x20, false, false),
        ZoneRampTick::Idle
    );
    assert_eq!(
        zone_ramp_tick(&r, 101, 0x20, false, false),
        ZoneRampTick::Idle
    );
    assert_eq!(zone_ramp_tick(&r, 0, -1, false, false), ZoneRampTick::Idle);
    assert_eq!(
        zone_ramp_tick(&r, 0, 0x41, false, false),
        ZoneRampTick::Idle
    );
}

#[test]
fn rgb_ramp_scales_green_and_blue_before_dividing() {
    // start 0, end 10 on every lane, one third remaining.
    let start = 0;
    let end = 0x000A_0A0A;
    let v = rgb_lerp(start, end, 1, 3, false);
    assert_eq!(v & 0xFF, 7, "red: 10 + trunc(-10/3)");
    assert_eq!((v >> 8) & 0xFF, 6, "green: floor of the scaled lerp");
    assert_eq!((v >> 16) & 0xFF, 6, "blue: same as green");
    assert_eq!(v >> 24, 0);
    // Increasing lanes agree with the plain lerp.
    let up = rgb_lerp(0x000A_0A0A, 0, 1, 3, false);
    assert_eq!(up, 0x0003_0303);
    assert_eq!(rgb_lerp(1, end, 0, 3, true), end);
}

#[test]
fn zone_ramp_width_ladder_sends_kind_3_to_sw_not_to_rgb() {
    let mut r = zone_ramp_fixture();
    for (kind, want) in [
        (1, ZoneRampWidth::U8),
        (2, ZoneRampWidth::U16),
        (3, ZoneRampWidth::U32),
        (4, ZoneRampWidth::U32),
    ] {
        r.kind = kind;
        assert_eq!(
            zone_ramp_tick(&r, 0, 0x40, false, false),
            ZoneRampTick::Write {
                value: 0x100,
                width: want
            },
            "kind {kind}"
        );
    }
    // Everything else takes the `+0x10 |= 8` arm.
    for kind in [0, 5, -1, 0x10] {
        r.kind = kind;
        assert_eq!(
            zone_ramp_tick(&r, 0, 0x40, false, false),
            ZoneRampTick::Retire,
            "kind {kind}"
        );
    }
}

#[test]
fn zone_ramp_truncates_toward_zero_and_reports_the_zero_span() {
    let mut r = zone_ramp_fixture();
    r.end = 100;
    r.z_hi = 100;
    // 100 * 33 / 100 = 33 exactly; 100 * 1 / 3 truncates to 33 as well.
    assert_eq!(
        zone_ramp_tick(&r, 0, 33, false, false),
        ZoneRampTick::Write {
            value: 33,
            width: ZoneRampWidth::U16
        }
    );
    // A descending window truncates toward zero, not toward -inf.
    r.start = 0;
    r.end = -100;
    assert_eq!(
        zone_ramp_tick(&r, 0, 33, false, false),
        ZoneRampTick::Write {
            value: -33,
            width: ZoneRampWidth::U16
        }
    );

    // A degenerate window is retail's divide-by-zero trap.
    r.z_lo = 5;
    r.z_hi = 5;
    assert_eq!(
        zone_ramp_tick(&r, 0, 5, false, false),
        ZoneRampTick::DivideByZero
    );
}

#[test]
fn facing_sites_walks_a_mixed_stream() {
    // 0x17 default-move (3), 0x04 ramp (3), 0x05 wait (2), 0x0D (4).
    let code = [
        0x17u8, 0x01, 0x02, 0x04, 0x83, 0x18, 0x05, 0x10, 0x0D, 0x05, 0x40, 0x00,
    ];
    let sites = facing_sites(&code);
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[0].op, 0x04);
    assert_eq!(sites[0].lut_index, 3);
    assert!(sites[0].decreasing);
    assert_eq!(sites[0].duration, 0x18);
    assert_eq!(sites[1].op, 0x0D);
    assert_eq!(sites[1].lut_index, 5);
    assert!(!sites[1].decreasing);
    assert_eq!(sites[1].duration, 0x40);
    assert_eq!(sites[1].target(), 0xA00);
}

#[test]
fn replay_lands_every_synthetic_site_on_its_compass_point() {
    for lut in 0..8u8 {
        for dec in [false, true] {
            for dur in [1i32, 5, 24, 0x7F] {
                let site = AmbientFacingSite {
                    offset: 0,
                    op: 0x04,
                    lut_index: lut,
                    decreasing: dec,
                    duration: dur,
                };
                let trace = replay_site(&site, 0x321, 1);
                assert_eq!(
                    *trace.last().unwrap(),
                    site.target(),
                    "lut {lut} dec {dec} dur {dur}"
                );
            }
        }
    }
}
