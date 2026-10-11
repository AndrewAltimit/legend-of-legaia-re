use super::*;

/// Host that always returns a fixed 8-byte record.
struct FixedRecord {
    bytes: [u8; 8],
}
impl MoveBufferHost for FixedRecord {
    fn resolve_record(&self, _actor_status_flags: u32, _requested_id: i16) -> Option<&[u8]> {
        Some(&self.bytes)
    }
}

/// Host that always returns `None`.
struct NoRecord;
impl MoveBufferHost for NoRecord {
    fn resolve_record(&self, _: u32, _: i16) -> Option<&[u8]> {
        None
    }
}

fn record(flag: u8, frame_count: u16, divisor: u8) -> [u8; 8] {
    let fc = frame_count.to_le_bytes();
    [0, flag, fc[0], fc[1], 0, 0, divisor, 0]
}

#[test]
fn frozen_envelope_does_nothing() {
    let mut s = MoveBufferState {
        env_flags: FROZEN,
        bone_count: 4,
        ..Default::default()
    };
    s.lanes[0] = 5;
    let before = s.clone();
    envelope_tick(&mut s, 1);
    assert_eq!(s, before);
}

#[test]
fn init_primes_lanes_to_peak() {
    let mut s = MoveBufferState {
        env_flags: INIT,
        bone_count: 4,
        ..Default::default()
    };
    envelope_tick(&mut s, 1);
    assert_eq!(s.lanes[..4], [MoveBufferState::PEAK; 4]);
    // Every lane bit set + finishing sign-bit set.
    assert_eq!(s.done_mask, 0xF | DONE_MASK_FINISHING);
    // INIT cleared.
    assert_eq!(s.env_flags & INIT, 0);
}

#[test]
fn ramp_velocities_are_per_lane_not_one_global_rate() {
    // Retail reads the up-ramp velocity at `0xb8(a1)` with
    // `a1 = actor + lane*2`, and move-VM op 0x0A authors one value
    // per morph slot. Give lane 0 and lane 1 different rates and
    // check each lane ramps at its own.
    let mut up = [0i16; MAX_BONES];
    up[0] = 0xC00;
    up[1] = 0x100;
    let mut s = MoveBufferState {
        bone_count: 2,
        up_velocity: up,
        ..Default::default()
    };
    envelope_tick(&mut s, 1);
    assert_eq!(s.lanes[0], 0xC00);
    assert_eq!(s.lanes[1], 0);
    // Lane 0 peaks, cascading to lane 1 in the same pass - lane 1
    // must move by its own 0x100, not by lane 0's 0x800.
    envelope_tick(&mut s, 1);
    assert_eq!(s.lanes[0], MoveBufferState::PEAK);
    assert_eq!(s.lanes[1], 0x100);
}

#[test]
fn down_ramp_velocity_is_per_lane_too() {
    let mut down = [0i16; MAX_BONES];
    down[0] = 0x400;
    down[1] = 0x900;
    let mut s = MoveBufferState {
        bone_count: 2,
        env_flags: INIT,
        down_velocity: down,
        ..Default::default()
    };
    // INIT primes both lanes to peak and arms the finishing bit;
    // the down-ramp then drains the top lane first.
    envelope_tick(&mut s, 1);
    assert_eq!(s.lanes[1], MoveBufferState::PEAK - 0x900);
    assert_eq!(s.lanes[0], MoveBufferState::PEAK);
}

#[test]
fn up_ramp_cascades_lane_0_first() {
    let mut s = MoveBufferState {
        bone_count: 3,
        up_velocity: [0xC00; MAX_BONES],
        ..Default::default()
    };
    // Frame 1: lane 0 moves; lane 1 / 2 wait for lane 0 to peak.
    envelope_tick(&mut s, 1);
    assert_eq!(s.lanes[0], 0xC00);
    assert_eq!(s.lanes[1], 0);
    assert_eq!(s.lanes[2], 0);
    assert_eq!(s.done_mask, 0);
    // Frame 2: lane 0 overshoots to 0x1800 > 0x1000 -> clamp +
    // set done bit. Same-frame cascade: lane 1 sees done bit and
    // steps in this iter.
    envelope_tick(&mut s, 1);
    assert_eq!(s.lanes[0], MoveBufferState::PEAK);
    assert!(s.done_mask & 0x1 != 0);
    assert_eq!(s.lanes[1], 0xC00);
    // Frame 3: lane 1 peaks (clamp), cascade to lane 2.
    envelope_tick(&mut s, 1);
    assert_eq!(s.lanes[1], MoveBufferState::PEAK);
    assert!(s.done_mask & 0x2 != 0);
    assert_eq!(s.lanes[2], 0xC00);
}

#[test]
fn up_ramp_full_completion_arms_finishing_bit() {
    let mut s = MoveBufferState {
        bone_count: 2,
        up_velocity: [0x1000; MAX_BONES],
        ..Default::default()
    };
    // 4 frames is enough to peak both lanes (2 frames each
    // because the cascade waits one frame between lanes).
    for _ in 0..6 {
        envelope_tick(&mut s, 1);
    }
    assert_eq!(s.lanes[0], MoveBufferState::PEAK);
    assert_eq!(s.lanes[1], MoveBufferState::PEAK);
    assert!(s.done_mask & 0x3 == 0x3);
    assert!(s.done_mask & DONE_MASK_FINISHING != 0);
}

#[test]
fn hold_flag_skips_down_ramp() {
    let mut s = MoveBufferState {
        bone_count: 1,
        up_velocity: [0x1000; MAX_BONES],
        env_flags: HOLD,
        ..Default::default()
    };
    for _ in 0..4 {
        envelope_tick(&mut s, 1);
    }
    // Lane peaked. HOLD path: down-ramp suppressed.
    assert_eq!(s.lanes[0], MoveBufferState::PEAK);
    // HOLD's completion path OR's in ENVELOPE_ADVANCE rather than
    // arming the finishing sign-bit.
    assert!(s.env_flags & ENVELOPE_ADVANCE != 0);
}

#[test]
fn down_ramp_drains_lane_signals_advance() {
    let mut s = MoveBufferState {
        bone_count: 1,
        down_velocity: [0x400; MAX_BONES],
        // Lane already at peak + done_mask peaked + finishing.
        done_mask: 0x1 | DONE_MASK_FINISHING,
        ..Default::default()
    };
    s.lanes[0] = MoveBufferState::PEAK;
    // Drain four frames at 0x400 / frame.
    for _ in 0..5 {
        envelope_tick(&mut s, 1);
    }
    assert_eq!(s.lanes[0], 0);
    // Lane bit cleared.
    assert!(s.done_mask & 0x1 == 0);
    // ENVELOPE_ADVANCE OR'd in (LANE0_SNAP_DOWN was clear).
    assert!(s.env_flags & ENVELOPE_ADVANCE != 0);
}

#[test]
fn down_ramp_lane0_snap_down_clears_finishing_bit() {
    let mut s = MoveBufferState {
        bone_count: 1,
        down_velocity: [0x400; MAX_BONES],
        done_mask: 0x1 | DONE_MASK_FINISHING,
        env_flags: LANE0_SNAP_DOWN,
        ..Default::default()
    };
    s.lanes[0] = MoveBufferState::PEAK;
    for _ in 0..5 {
        envelope_tick(&mut s, 1);
    }
    assert_eq!(s.lanes[0], 0);
    // LANE0_SNAP_DOWN's distinctive effect: FINISHING gets
    // cleared when lane 0 wraps below zero (vs. the default
    // branch which OR's in ENVELOPE_ADVANCE without touching
    // FINISHING).
    assert!(s.done_mask & DONE_MASK_FINISHING == 0);
}

#[test]
fn down_ramp_default_branch_keeps_finishing_bit() {
    // Mirror of `down_ramp_lane0_snap_down_clears_finishing_bit`
    // with LANE0_SNAP_DOWN clear. The wrap-below-zero handler
    // takes the OR-ENVELOPE_ADVANCE branch and FINISHING stays
    // set (it's the early-exit's job to clear it, not the
    // wrap-handler).
    let mut s = MoveBufferState {
        bone_count: 1,
        down_velocity: [0x400; MAX_BONES],
        done_mask: 0x1 | DONE_MASK_FINISHING,
        env_flags: 0,
        ..Default::default()
    };
    s.lanes[0] = MoveBufferState::PEAK;
    for _ in 0..5 {
        envelope_tick(&mut s, 1);
    }
    assert_eq!(s.lanes[0], 0);
    // FINISHING NOT cleared by the wrap-handler in this branch.
    assert!(s.done_mask & DONE_MASK_FINISHING != 0);
}

#[test]
fn cursor_new_id_latches_and_arms_move_vm() {
    let host = FixedRecord {
        bytes: record(0, 8, 1),
    };
    let mut s = MoveBufferState {
        cursor_requested: 7,
        cursor_active: 0,
        phase_rate: 8,
        ..Default::default()
    };
    cursor_advance(&mut s, &host, 1);
    assert_eq!(s.cursor_active, 7);
    assert_eq!(s.move_vm_kick, 1);
    assert!(s.record_bound);
    // First-frame step: phase advanced by phase_rate * frame_delta.
    assert_eq!(s.phase, 8);
}

#[test]
fn cursor_zero_or_negative_id_is_idle() {
    let host = FixedRecord {
        bytes: record(0, 8, 1),
    };
    let mut s = MoveBufferState::default();
    // cursor_requested = 0 by default.
    cursor_advance(&mut s, &host, 1);
    assert!(!s.record_bound);
    assert_eq!(s.move_vm_kick, 0);

    s.cursor_requested = -3;
    cursor_advance(&mut s, &host, 1);
    assert!(!s.record_bound);
}

#[test]
fn cursor_envelope_gate_runs_when_flag_set() {
    let host = NoRecord;
    let mut s = MoveBufferState {
        status_flags: STATUS_FLAG_ENVELOPE_ACTIVE,
        env_flags: INIT,
        bone_count: 2,
        ..Default::default()
    };
    cursor_advance(&mut s, &host, 1);
    // Envelope tick ran (INIT cleared, lanes primed).
    assert!(s.env_flags & INIT == 0);
    assert_eq!(s.lanes[..2], [MoveBufferState::PEAK; 2]);
}

#[test]
fn cursor_pause_flag_freezes_phase() {
    let host = FixedRecord {
        bytes: record(0, 16, 1),
    };
    let mut s = MoveBufferState {
        cursor_requested: 4,
        cursor_active: 4,
        phase: 32,
        phase_rate: 8,
        env_flags: PAUSE,
        ..Default::default()
    };
    cursor_advance(&mut s, &host, 1);
    // No phase advance under PAUSE.
    assert_eq!(s.phase, 32);
}

#[test]
fn cursor_overshoot_no_loop_snaps_to_zero() {
    let host = FixedRecord {
        bytes: record(0, 4, 1),
    };
    // frame_count=4 -> max = 4*16 - 1 = 63.
    let mut s = MoveBufferState {
        cursor_requested: 1,
        cursor_active: 1,
        phase: 60,
        phase_rate: 8,
        env_flags: 0,
        ..Default::default()
    };
    cursor_advance(&mut s, &host, 1);
    assert_eq!(s.phase, 0);
    assert!(s.env_flags & CLAMPED != 0);
}

#[test]
fn cursor_overshoot_clamp_ends_holds_at_max() {
    let host = FixedRecord {
        bytes: record(0, 4, 1),
    };
    let mut s = MoveBufferState {
        cursor_requested: 1,
        cursor_active: 1,
        phase: 60,
        phase_rate: 8,
        env_flags: CLAMP_ENDS,
        ..Default::default()
    };
    cursor_advance(&mut s, &host, 1);
    // 4*16 - 1 = 63.
    assert_eq!(s.phase, 63);
    assert!(s.env_flags & CLAMPED != 0);
}

#[test]
fn cursor_undershoot_without_clamp_wraps_forward() {
    let host = FixedRecord {
        bytes: record(0, 4, 1),
    };
    let mut s = MoveBufferState {
        cursor_requested: 1,
        cursor_active: 1,
        phase: 4,
        phase_rate: 8,
        env_flags: REVERSE,
        ..Default::default()
    };
    cursor_advance(&mut s, &host, 1);
    // After step: 4 - 8 = -4; CLAMP_ENDS clear -> wrap by +64 -> 60.
    assert_eq!(s.phase, 60);
    assert!(s.env_flags & CLAMPED != 0);
}

#[test]
fn cursor_start_request_seeds_phase_from_reverse_flag() {
    let host = FixedRecord {
        bytes: record(0, 4, 1),
    };
    // Reverse: phase initialised to (count - 1) * 16 = 48.
    let mut s = MoveBufferState {
        cursor_requested: 5,
        cursor_active: 5,
        phase: 99,
        phase_rate: 8,
        env_flags: START_REQUEST | REVERSE | CLAMP_ENDS,
        ..Default::default()
    };
    cursor_advance(&mut s, &host, 1);
    // START_REQUEST -> seed phase to 48, clear START_REQUEST,
    // then REVERSE step (-8): final = 40.
    assert_eq!(s.phase, 40);
    assert!(s.env_flags & START_REQUEST == 0);
}

#[test]
fn cursor_record_flag_recomputes_phase_rate_from_divisor() {
    // Flag bit 0 set; divisor = 3. New rate = (phase_rate*2 + 2) / 3.
    let host = FixedRecord {
        bytes: record(0x1, 16, 3),
    };
    let mut s = MoveBufferState {
        cursor_requested: 2,
        cursor_active: 2,
        phase: 10,
        phase_rate: 9,
        ..Default::default()
    };
    cursor_advance(&mut s, &host, 1);
    // (9*2 + 3 - 1) / 3 = 20 / 3 = 6 is the step, and phase_rate is not
    // written back (retail keeps it in a register).
    assert_eq!(s.phase_rate, 9);
    // Phase advanced by the derived rate.
    assert_eq!(s.phase, 16);
}

#[test]
fn cursor_no_record_keeps_state() {
    let host = NoRecord;
    let mut s = MoveBufferState {
        cursor_requested: 4,
        cursor_active: 0,
        phase_rate: 8,
        ..Default::default()
    };
    cursor_advance(&mut s, &host, 1);
    // Resolver said None -> no latch.
    assert_eq!(s.cursor_active, 0);
    assert_eq!(s.move_vm_kick, 0);
    assert!(!s.record_bound);
}
