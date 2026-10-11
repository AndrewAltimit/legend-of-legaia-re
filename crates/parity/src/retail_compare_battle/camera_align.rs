//! The battle camera's tween at the capture instant: whether it has landed,
//! the tween read out of the context, and its environment hand-off.

use super::*;

/// The tween table `FUN_801D829C` builds (`ctx[+0x118C]`, nine
/// `{u16 step, u16 endpoint}` records: pitch / yaw / roll, the translation
/// trio, the focus trio).
pub(super) const TWEEN_TABLE: u32 = 0x118C;

/// Whether retail's camera sits on its framing: every tweened component with
/// a step reads its endpoint. The yaw is left out - the idle orbit writes it
/// beside the walker. The rotation pair and the translation / focus words
/// compare as the low halfwords the walker steps.
pub(super) fn camera_landed(ram: &[u8], ctx: u32) -> bool {
    let live = [
        game_anchors::u16_at(ram, 0x8007_B790),
        0,
        game_anchors::u16_at(ram, 0x8007_B794),
        game_anchors::u16_at(ram, 0x8008_40B8),
        game_anchors::u16_at(ram, 0x8008_40BC),
        game_anchors::u16_at(ram, 0x8008_40C0),
        game_anchors::u16_at(ram, 0x8008_9118),
        game_anchors::u16_at(ram, 0x8008_911C),
        game_anchors::u16_at(ram, 0x8008_9120),
    ];
    (0..9u32).filter(|&k| k != 1).all(|k| {
        let rec = ctx + TWEEN_TABLE + k * 4;
        let end = game_anchors::u16_at(ram, rec + 2);
        let mask = if k < 3 { 0xFFF } else { 0xFFFF };
        live[k as usize] & mask == end & mask
    })
}

/// Retail's live camera pose and its tween endpoints, in the engine's pose
/// space: the 12-bit angles, the eye translation `0x800840B8/BC/C0`, and the
/// focus `0x80089118/1C/20` un-negated (retail stores it negated, and so do
/// the focus records of the table). The walker steps the low halfwords, so
/// the endpoints are halfwords too.
pub(super) fn retail_cam_tween(
    ram: &[u8],
    ctx: u32,
) -> (
    legaia_engine_battle_vm::battle_cam_script::BattleCamPose,
    legaia_engine_battle_vm::battle_cam_script::BattleCamPose,
) {
    use legaia_engine_battle_vm::battle_cam_script::BattleCamPose;
    let word = |va: u32| game_anchors::u32_at(ram, va) as i32 as f32;
    let live = BattleCamPose {
        pitch: f32::from(game_anchors::u16_at(ram, 0x8007_B790) & 0xFFF),
        yaw: f32::from(game_anchors::u16_at(ram, 0x8007_B792) & 0xFFF),
        tr: [word(0x8008_40B8), word(0x8008_40BC), word(0x8008_40C0)],
        focus: [-word(0x8008_9118), -word(0x8008_911C), -word(0x8008_9120)],
    };
    let end_at = |k: u32| game_anchors::u16_at(ram, ctx + TWEEN_TABLE + k * 4 + 2) as i16;
    let end = BattleCamPose {
        pitch: f32::from(end_at(0) as u16 & 0xFFF),
        yaw: f32::from(end_at(1) as u16 & 0xFFF),
        tr: [3, 4, 5].map(|k| f32::from(end_at(k))),
        focus: [6, 7, 8].map(|k| -f32::from(end_at(k))),
    };
    (live, end)
}
