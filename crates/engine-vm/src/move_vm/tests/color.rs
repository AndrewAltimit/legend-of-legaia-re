//! RGB / HSV conversion and the HSV ramp sub-ops.

use super::*;

// --- HSV helpers + ext sub-op 0x1F / 0x20 -----------------------------

#[test]
fn rgb_to_hsv_pure_red_round_trip() {
    let (h, s, v) = rgb_to_hsv(0xFF, 0, 0);
    assert_eq!(h, 0, "pure red has H = 0");
    assert!(s > 0xF0, "pure red is fully saturated, got {s:#x}");
    assert_eq!(v, 0xFF, "pure red has V = 0xFF");
}

#[test]
fn rgb_to_hsv_pure_green_lands_in_segment_2() {
    let (h, _s, _v) = rgb_to_hsv(0, 0xFF, 0);
    // Green = 120 deg = 0x78 in this encoding.
    assert_eq!(h, 0x78);
}

#[test]
fn rgb_to_hsv_pure_blue_lands_in_segment_4() {
    let (h, _s, _v) = rgb_to_hsv(0, 0, 0xFF);
    // Blue = 240 deg = 0xF0.
    assert_eq!(h, 0xF0);
}

#[test]
fn rgb_to_hsv_zero_returns_zero() {
    assert_eq!(rgb_to_hsv(0, 0, 0), (0, 0, 0));
}

#[test]
fn hsv_to_rgb_segment_dispatch_matches_each_arm() {
    // V = 0xFF, S = 0xFF - picks the segment based on H.
    // Segment 0 (H=0): (V, t, p) - pure red.
    let (r, g, b) = hsv_to_rgb(0, 0xFF, 0xFF);
    assert!(r >= 0xF0 && g <= 1 && b <= 1, "segment 0 ≈ pure red");
    // Segment 2 (H=0x78=120 deg): green.
    let (r, g, b) = hsv_to_rgb(0x78, 0xFF, 0xFF);
    assert!(r <= 1 && g >= 0xF0 && b <= 1, "segment 2 ≈ pure green");
    // Segment 4 (H=0xF0=240 deg): blue.
    let (r, g, b) = hsv_to_rgb(0xF0, 0xFF, 0xFF);
    assert!(r <= 1 && g <= 1 && b >= 0xF0, "segment 4 ≈ pure blue");
}

#[test]
fn hsv_to_rgb_zero_saturation_returns_grey() {
    let (r, g, b) = hsv_to_rgb(0x55, 0, 0x80);
    assert_eq!((r, g, b), (0x80, 0x80, 0x80));
}

#[test]
fn op2f_subop_1f_rotates_hue_on_keyframe_desc_lo() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // Pre-set actor[+0xa0..+0xa3] = packed pure-red RGB.
    state.keyframe_desc[0] = 0x00FF; // R=0xFF, G=0
    state.keyframe_desc[1] = 0x0000; // B=0
    // Sub-op 0x1F: delta H = 0x78 (= 120 deg), delta S = 0, delta V = 0.
    // Should rotate red → green.
    let bc = program(&[0x2F, 0x1F, 0x78, 0, 0]);
    step(&mut host, &mut state, &bc);
    let r = state.keyframe_desc[0] & 0xFF;
    let g = (state.keyframe_desc[0] >> 8) & 0xFF;
    let b = state.keyframe_desc[1] & 0xFF;
    assert!(
        g > r,
        "hue-rotated by 120 deg should make G dominant ({r:#x},{g:#x},{b:#x})"
    );
    assert!(
        g > b,
        "hue-rotated by 120 deg should make G dominate B ({r:#x},{g:#x},{b:#x})"
    );
    // FUN_8001a6c8 caps at 0xF8.
    assert!(g <= 0xF8);
    // Size 5 - opcode + sub-op + the three H/S/V delta words (`li s2, 0x5`
    // at 0x801D3F60).
    assert_eq!(state.pc, 5);
}

#[test]
fn op2f_subop_20_targets_keyframe_desc_hi_pair() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    // Pre-set actor[+0xa4..+0xa7] = packed pure-blue.
    state.keyframe_desc[2] = 0x0000;
    state.keyframe_desc[3] = 0x00FF; // B=0xFF
    // Sub-op 0x20: delta H = 0x78 (= 120 deg). Blue → red.
    let bc = program(&[0x2F, 0x20, 0x78, 0, 0]);
    step(&mut host, &mut state, &bc);
    let r = state.keyframe_desc[2] & 0xFF;
    let g = (state.keyframe_desc[2] >> 8) & 0xFF;
    let b = state.keyframe_desc[3] & 0xFF;
    assert!(r > g);
    assert!(r > b);
    // 0x1F slot must be untouched.
    assert_eq!(state.keyframe_desc[0], 0);
    assert_eq!(state.keyframe_desc[1], 0);
}

#[test]
fn op2f_subop_1f_value_decrement_dims_color() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.keyframe_desc[0] = 0x80FF; // R=0xFF, G=0x80
    state.keyframe_desc[1] = 0x0040; // B=0x40
    let v_before = (state.keyframe_desc[0] & 0xFF).max((state.keyframe_desc[0] >> 8) & 0xFF);
    // Delta H = 0, delta S = 0, delta V = -0x40 (use signed).
    let bc = program(&[0x2F, 0x1F, 0, 0, (-0x40i16) as u16]);
    step(&mut host, &mut state, &bc);
    let r_after = state.keyframe_desc[0] & 0xFF;
    let g_after = (state.keyframe_desc[0] >> 8) & 0xFF;
    let v_after = r_after.max(g_after);
    assert!(
        v_after < v_before,
        "lowering V should reduce the dominant channel ({v_before:#x} -> {v_after:#x})"
    );
}

/// 0x1F / 0x20 are ordinary 5-halfword instructions whose three operands
/// are the H/S/V deltas - not size-1 arms that re-read their own operands
/// as a fresh opcode. `li s2, 0x5` at `0x801D3F60` is on the single path
/// through the shared arm.
#[test]
fn op2f_hsv_ramp_sub_ops_consume_their_three_delta_operands() {
    for sub_op in [0x1Fu16, 0x20] {
        let mut host = TestHost::default();
        let mut state = ActorState::new();
        step(&mut host, &mut state, &ext_program(sub_op));
        assert_eq!(state.pc, 5, "sub-op 0x{sub_op:02X}");
    }
}

/// The HSV re-pack is a full 32-bit `sw` (`sw v1,0x0(s0)` at 0x801D3F84),
/// so the packed word's top byte is cleared rather than preserved.
#[test]
fn op2f_hsv_ramp_repack_clears_the_top_byte() {
    let mut host = TestHost::default();
    let mut state = ActorState::new();
    state.keyframe_desc = [0x0000, 0xAB00, 0, 0];
    // Zero deltas: the colour round-trips, but the store still rewrites the
    // whole word.
    step(&mut host, &mut state, &ext_program(0x1F));
    assert_eq!(
        state.keyframe_desc[1] & 0xFF00,
        0,
        "the `sw` clears bits 24..31 of the packed RGB word"
    );
}
