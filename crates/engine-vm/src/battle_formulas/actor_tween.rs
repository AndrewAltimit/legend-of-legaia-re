//! Battle-actor packed-channel tween kernel.
//!
//! A pure integer helper the battle-overlay actor state machine
//! (`FUN_80050120`) drives every frame to ease a three-channel packed value
//! toward a target. Each channel is a 10-bit field of one `u32`
//! (`bits 0..=9`, `10..=19`, `20..=29`); the state machine uses it for the
//! per-actor tint/flash tween that fades back to the neutral `0x80,0x80,0x80`
//! word (packed `0x2008_0200`, the value `FUN_80050120` tests for "arrived").
//! See `docs/subsystems/battle.md` § Additional SCUS battle-band helpers.
//!
//! It is a genuine closed-form arithmetic kernel with no hardware or table
//! dependency, so it ports clean-room even though its consumer is presentation.
//!
//! # NOT WIRED
//!
//! The tween is one step of `FUN_80050120`, the per-actor tint state machine,
//! and that machine's state is what is missing: the per-actor step-scale byte
//! and the "tween is running" flag both live on the actor struct outside the
//! range `BattleActor` models, so nothing can supply `step_scale` or decide
//! which slots are mid-tween on a given frame. The word it eases,
//! `actor[+0x4]`, *is* on the port (`BattleActor::render_color`, which the
//! target-select cursor `FUN_801DA6B4` stamps), so this is the second half of
//! that pair - it becomes wirable as soon as the tint SM's own state lands.

/// One channel stepped toward `target` by at most `max_delta`, clamping exactly
/// on the target without overshoot. Signed so a channel may pass through zero
/// mid-step (retail does the near-target compare with a signed `slt`).
///
/// PORT: FUN_80050f30 (per-axis core)
pub fn approach_channel_clamped(cur: i32, target: i32, max_delta: i32) -> i32 {
    use core::cmp::Ordering::*;
    match cur.cmp(&target) {
        Equal => cur,
        Less => (cur + max_delta).min(target),
        Greater => (cur - max_delta).max(target),
    }
}

/// Ease a 3×10-bit packed value one frame toward the per-channel target.
///
/// `target_x/y/z` are the retail 8-bit target bytes, each widened `<< 2` into
/// its 10-bit channel (so a target byte `0x80` becomes channel `0x200`). The
/// per-frame max step is `step_scale * frame_scalar * 8`, where `frame_scalar`
/// is the scratchpad frame-time byte `DAT_1f800393` (~2 at runtime), passed
/// explicitly to keep the kernel pure. Only channels that differ from their
/// target are rewritten, byte-for-byte as retail (which is why the top two bits
/// survive an unchanged Z channel but are cleared when Z is rewritten).
///
/// PORT: FUN_80050f30
pub fn packed3_approach_target(
    packed: u32,
    target_x: u8,
    target_y: u8,
    target_z: u8,
    step_scale: u8,
    frame_scalar: u8,
) -> u32 {
    let max_delta = (step_scale as i32) * (frame_scalar as i32) * 8;
    let (tx, ty, tz) = (
        (target_x as i32) << 2,
        (target_y as i32) << 2,
        (target_z as i32) << 2,
    );
    let mut p = packed;

    let cx = (p & 0x3ff) as i32;
    if cx != tx {
        p = (p & 0xffff_fc00) | (approach_channel_clamped(cx, tx, max_delta) as u32 & 0x3ff);
    }
    let cy = ((p >> 10) & 0x3ff) as i32;
    if cy != ty {
        p = (p & 0xfff0_03ff)
            | ((approach_channel_clamped(cy, ty, max_delta) as u32 & 0x3ff) << 10);
    }
    let cz = ((p >> 20) & 0x3ff) as i32;
    if cz != tz {
        p = (p & 0x000f_ffff)
            | ((approach_channel_clamped(cz, tz, max_delta) as u32 & 0x3ff) << 20);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(x: u32, y: u32, z: u32) -> u32 {
        x | (y << 10) | (z << 20)
    }

    #[test]
    fn per_channel_step_up_down_and_clamp() {
        // step up, no overshoot
        assert_eq!(approach_channel_clamped(0x100, 0x200, 16), 0x110);
        // step down, no overshoot
        assert_eq!(approach_channel_clamped(0x300, 0x200, 16), 0x2f0);
        // already at target
        assert_eq!(approach_channel_clamped(0x200, 0x200, 16), 0x200);
        // would overshoot upward -> clamp to target
        assert_eq!(approach_channel_clamped(0x1f8, 0x200, 16), 0x200);
        // would overshoot downward -> clamp to target
        assert_eq!(approach_channel_clamped(0x205, 0x200, 16), 0x200);
    }

    #[test]
    fn packed_eases_each_channel_independently() {
        // frame_scalar 2, step_scale 1 -> max_delta = 16
        let input = pack(0x100, 0x300, 0x1fe);
        let out = packed3_approach_target(input, 0x80, 0x80, 0x80, 1, 2);
        // X: 0x100 -> +16 = 0x110 ; Y: 0x300 -> -16 = 0x2f0 ; Z: 0x1fe -> +16 clamps to 0x200
        assert_eq!(out, pack(0x110, 0x2f0, 0x200));
    }

    #[test]
    fn arrival_word_is_the_neutral_tint() {
        // Once every channel reaches the 0x80 target the packed word is the
        // exact value FUN_80050120 tests for "arrived".
        let arrived = packed3_approach_target(pack(0x200, 0x200, 0x200), 0x80, 0x80, 0x80, 1, 2);
        assert_eq!(arrived, 0x2008_0200);
    }

    #[test]
    fn max_delta_scales_with_frame_and_step() {
        // step_scale 3, frame_scalar 2 -> max_delta = 48
        let out = packed3_approach_target(pack(0, 0, 0), 0x80, 0x80, 0x80, 3, 2);
        assert_eq!(out, pack(48, 48, 48));
    }
}
