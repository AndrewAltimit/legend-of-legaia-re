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
pub fn display_lag_vsyncs(ram: &[u8]) -> u16 {
    2 * u16::from(frame_step(ram))
}
