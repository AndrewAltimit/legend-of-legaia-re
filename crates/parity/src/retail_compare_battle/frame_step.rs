//! Retail's frame-step word and display lag at the capture instant.

use super::*;

/// Retail's per-frame duration history (`0x80084098`, sixteen halfwords in
/// hsync units, `gp = 0x8007B318`): the frame driver at `0x80017098` stores
/// each frame's duration there (clamped to `0x2BC`) and derives the frame
/// step from the largest of the sixteen.
pub(super) const FRAME_HISTORY: u32 = 0x8008_4098;
/// A forced frame step (`gp+0x5D8`); non-zero skips the history.
pub(super) const FORCED_STEP: u32 = 0x8007_B8F0;
/// The frame-rate mode word (`gp+0x4CE`); only mode `0x10` steps adaptively,
/// every other mode stores step `1`.
pub(super) const STEP_MODE: u32 = 0x8007_B7E6;
/// The step floor (`0x8007B9D8`, read at `0x80017178`).
pub(super) const STEP_FLOOR: u32 = 0x8007_B9D8;

/// The adaptive frame step `*(0x1F800393)` - vsyncs per game frame - rebuilt
/// from main RAM, since the scratchpad byte itself is not in a main-RAM
/// image. The thresholds are the frame driver's (`0x80017108..0x80017198`):
/// under `0xF1` hsyncs step `1`, under `0x1FF` step `2`, under `0x2D1` step
/// `3`, else `4`, then raised to the floor.
pub fn frame_step(ram: &[u8]) -> u8 {
    let forced = game_anchors::u32_at(ram, FORCED_STEP);
    if forced != 0 {
        return forced as u8;
    }
    if game_anchors::i16_at(ram, STEP_MODE) != 0x10 {
        return 1;
    }
    let longest = (0..16)
        .map(|i| game_anchors::i16_at(ram, FRAME_HISTORY + i * 2))
        .max()
        .unwrap_or(0);
    let step = if longest < 0xF1 {
        1
    } else if longest < 0x1FF {
        2
    } else if longest < 0x2D1 {
        3
    } else {
        4
    };
    let floor = game_anchors::u32_at(ram, STEP_FLOOR).min(4) as u8;
    step.max(floor)
}

/// The frame driver's write cursor into [`FRAME_HISTORY`] (`gp+0x440`): the
/// pass stores its duration at `cursor & 0xF` and leaves `cursor + 1`
/// (`0x80017044..0x800170CC`), so the newest entry sits one below it.
pub(super) const FRAME_HISTORY_CURSOR: u32 = 0x8007_B758;
/// The clamp the history stores a long frame as (`li s0,0x2bc` at
/// `0x800170A8`, for a duration of `0x2D0` or more).
const FRAME_HISTORY_CLAMP: i16 = 0x2BC;

/// The frame steps of the capture's last two passes: `[0]` the pass the RAM
/// holds (the table being built), `[1]` the one before it (the older table).
///
/// [`frame_step`] reads the step off the history's largest entry, which is
/// the rule for every frame but a long one: the history stores a duration of
/// `0x2D0` hsyncs or more as `0x2BC`, while the step is taken off the
/// **raw** duration (`slti v0,a2,0x2d0` / `move s0,a2` at
/// `0x800170F8..0x80017104`), which lands past `0x2D1` on step `4`. So an
/// entry reading the clamp is a step-`4` pass whatever the rest of the
/// history says. The pass before reads its own entry the same way, against
/// the history less the newest entry (the one it overwrote is gone).
/// `delilas_gi_spirit_artifact`'s history reads `0x2CF` at most, so
/// [`frame_step`] says `3`; its newest entry is the clamp, and the older
/// ordering table's ground grid stands one step-`4` pass behind the RAM's
/// camera, not one step-`3` pass.
pub fn pass_steps(ram: &[u8]) -> [u8; 2] {
    let forced = game_anchors::u32_at(ram, FORCED_STEP);
    if forced != 0 {
        return [forced as u8; 2];
    }
    if game_anchors::i16_at(ram, STEP_MODE) != 0x10 {
        return [1; 2];
    }
    let cursor = game_anchors::u32_at(ram, FRAME_HISTORY_CURSOR);
    let entry = |back: u32| {
        game_anchors::i16_at(ram, FRAME_HISTORY + (cursor.wrapping_sub(back) & 0xF) * 2)
    };
    let floor = game_anchors::u32_at(ram, STEP_FLOOR).min(4) as u8;
    let step_for = |back: u32| {
        let longest = (0..16u32)
            .filter(|&k| k >= back)
            .map(|k| entry(k + 1))
            .max()
            .unwrap_or(0);
        let step = if entry(back + 1) == FRAME_HISTORY_CLAMP {
            4
        } else if longest < 0xF1 {
            1
        } else if longest < 0x1FF {
            2
        } else if longest < 0x2D1 {
            3
        } else {
            4
        };
        step.max(floor)
    };
    [step_for(0), step_for(1)]
}

/// How many passes the displayed frame lags the RAM a capture holds: the
/// double buffer's two ([`display_lag_vsyncs`]).
pub const DISPLAY_LAG_PASSES: u16 = 2;

/// The steps of the two passes the displayed frame lags by, newest first:
/// [`pass_steps`] where they differ from [`frame_step`] and the older
/// ordering table's battle floor confirms the newest
/// (`camera_pool::older_table_step`), `frame_step` for both otherwise.
///
/// The history alone cannot settle a clamped entry: `0x2BC` is also a raw
/// duration of exactly `700` hsyncs, stored unclamped and stepped `3`. The
/// floor can, wherever the camera moved on the newest pass.
pub fn display_pass_steps(ram: &[u8]) -> [u8; 2] {
    let fs = frame_step(ram);
    let steps = pass_steps(ram);
    if steps == [fs; 2] {
        return steps;
    }
    let ctx = game_anchors::u32_at(ram, BATTLE_CTX);
    if in_ram(ctx) && older_table_step(ram, ctx) == Some(steps[0]) {
        steps
    } else {
        [fs; 2]
    }
}

/// How far the displayed frame lags the RAM a capture holds: **two** game
/// frames. Retail double-buffers - the CPU builds frame `N` while the GPU
/// draws `N - 1` and the display scans out `N - 2` - so the VRAM display
/// area a capture's frame is cropped from shows the state two ticks before
/// the RAM's. The fade actor steps `*(0x1F800393)` vsyncs a tick
/// (`FUN_80020C14`, `lbu v0,0x393(v0)` at `0x80020C34`), so the flash
/// visible in the frame is `2 * step` vsyncs younger than the block says.
/// Measured on the summon captures: the two packet pools each hold the
/// flash's full-screen `POLY_F4` - one at the block's value (the frame
/// being built), the other one step behind - and the displayed frame is one
/// step older still. A flash-in block at `178` (packets `178` / `153`)
/// shows `123` on screen, one at `255` shows `206`, one at `102` shows
/// `49`, and a block `6` vsyncs past its delay on a step-`3` frame shows no
/// flash at all.
///
/// The two passes need not share a step ([`display_pass_steps`]), but this
/// lag stays two of [`frame_step`]: taken as the confirmed passes' sum, the
/// part seeds it drives split the captures it reaches
/// (`delilas_gi_spirit_streaks_v2` `0.956` to `0.988`,
/// `delilas_gi_spirit_distortion_v3` `0.889` to `0.864`,
/// `gimard_burning_attack` `0.817` to `0.808`). The camera walk-back, the
/// idle clip cursors and the glide seats take the per-pass steps.
pub fn display_lag_vsyncs(ram: &[u8]) -> u16 {
    DISPLAY_LAG_PASSES * u16::from(frame_step(ram))
}
