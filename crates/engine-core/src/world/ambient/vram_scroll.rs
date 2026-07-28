//! Render-mode 4 - the cyclic VRAM-rect scroller.
//!
//! PORT: FUN_80021DF4 (the mode-4 arm, `0x80022CB8..0x80022EE0`)
//! REF: FUN_80023070, FUN_8005842C, FUN_80058490, FUN_800583C8
//!
//! ## Retail chain
//!
//! Move-VM op `0x1E` seats the mode: `+0x5A = 4`, then its seven operands
//! land in `+0xC4` (period reload), `+0xCC` / `+0xCE` (per-period horizontal
//! and vertical step) and the VRAM rect `+0xD0..+0xD6` (`x, y, w, h`), in
//! that order (`FUN_80023070` `0x80023694..0x800236F0`). The countdown slot
//! `+0xC6` is **not** seated, so a fresh part fires on its first tick.
//!
//! The actor render tail then runs, per game tick
//! (`ghidra/scripts/funcs/80021df4.txt`):
//!
//! ```text
//! 80022cb8  lh v1,0x5a(s5)   ; render mode
//! 80022cc0  bne v1,0x4,...   ; whole arm is mode-4 only
//! 80022ccc  clear a0         ; dx = 0
//! 80022cec  _move s3,a0      ; dy = 0   (branch delay slot)
//! 80022cd0  lbu v1,0x7f(s1)  ; DAT_1F800393 - the frame step, alone:
//! 80022cdc  subu v0,v0,v1    ; +0xC6 -= frame_step     NO 0x10 speed scalar
//! 80022ce4  sll v0,v0,0x10   ; test the stored halfword's sign bit
//! 80022ce8  bgez v0,...      ; not underflowed -> both arms skipped
//! 80022cf0  lhu v0,0xc4(s5)  ; underflowed: reload the period and
//! 80022cf4  lh a0,0xcc(s5)   ; take this tick's steps
//! 80022cf8  lh s3,0xce(s5)
//! ```
//!
//! Each non-zero step then runs the same three-call strip rotate, horizontal
//! first (`0x80022D08..`), vertical second (`0x80022DF8..`), against the
//! **live** VRAM rect. Horizontal, with `sw = (i16)+0xCC * frame_step`:
//!
//! 1. `FUN_8005842C` (`StoreImage`) captures `(x, y, sw, h)` into a scratch
//!    buffer bump-allocated off `0x1F8003A0` at `((sw*h*2) + 3) / 4 * 4`
//!    bytes;
//! 2. `FUN_80058490` (`MoveImage`) copies `(x + sw, y, w - sw, h)` to
//!    `(x, y)` - the remainder slides over the captured strip;
//! 3. `FUN_800583C8` (`LoadImage`) writes the strip back at
//!    `(x + w - sw, y, sw, h)` - the far edge.
//!
//! Net: a cyclic left rotation of the rect by `sw` halfwords. The vertical
//! arm is the transpose (`sh = (i16)+0xCE * frame_step`, capture the top
//! strip, slide the remainder up, re-insert at `y + h - sh`) - a cyclic up
//! rotation. Both together rotate on both axes in one tick.
//!
//! So mode 4 animates **texels in place**: an authored VRAM rect (a
//! waterfall column, a scrolling texture band) is rotated under whatever
//! meshes sample it, with no vertex, UV or CLUT touched.
//!
//! ## Engine split
//!
//! [`mode4_integrate`] mutates the move-VM [`ActorState`] exactly like the
//! retail countdown/reload and returns the tick's strip widths;
//! [`rotate_rect`] is the pure texel kernel. `World::step_ambient_fx` owns
//! VRAM and applies the two together, the same contract the mode-3 sibling
//! ([`crate::clut_cell_fx`]) uses - except that this one is **destructive**:
//! the rect is rotated in place, so it is applied once per game tick rather
//! than recomputed per frame from a cached capture.

use legaia_engine_vm::move_vm::ActorState;

/// `+0xC4` - the period reload, in [`ActorState::anim_block`] byte offsets
/// (the window starts at actor `+0xAC`).
const OFF_PERIOD: usize = 0xC4 - 0xAC;
/// `+0xC6` - the live countdown.
const OFF_COUNTDOWN: usize = 0xC6 - 0xAC;
/// `+0xCC` - the horizontal step per fired period.
const OFF_H_STEP: usize = 0xCC - 0xAC;
/// `+0xCE` - the vertical step per fired period.
const OFF_V_STEP: usize = 0xCE - 0xAC;
/// `+0xD0..+0xD6` - the scrolled VRAM rect (`x, y, w, h`).
const OFF_RECT: usize = 0xD0 - 0xAC;

/// Retail's `+0x5A` render-mode marker for the scroller (move-VM op `0x1E`).
pub const RENDER_MODE_SCROLL: i16 = 4;

/// A live scroller's seat as reported by
/// [`World::active_ambient_scroll_rects`](crate::world::World::active_ambient_scroll_rects):
/// the VRAM rect `(x, y, w, h)` it animates, then the authored per-period
/// horizontal and vertical steps (`+0xCC`, `+0xCE`).
pub type ScrollSeat = ((u16, u16, u16, u16), i16, i16);

/// One fired tick of a mode-4 part: the rect to rotate and by how much.
///
/// `strip_w` / `strip_h` are already multiplied by the frame step, i.e. the
/// halfword counts retail hands to `StoreImage` / `LoadImage`. A zero on
/// either axis means retail skipped that arm (`beq a0,zero` / `beq s3,zero`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VramScrollFx {
    /// The seated VRAM rect `(x, y, w, h)` in halfwords (`+0xD0..+0xD6`).
    pub rect: (u16, u16, u16, u16),
    /// Halfwords to rotate left this tick (`(i16)+0xCC * frame_step`).
    pub strip_w: u16,
    /// Rows to rotate up this tick (`(i16)+0xCE * frame_step`).
    pub strip_h: u16,
}

impl VramScrollFx {
    /// `true` when this tick would leave the rect's texels untouched.
    pub fn is_identity(&self) -> bool {
        self.strip_w == 0 && self.strip_h == 0
    }
}

/// The `FUN_80021DF4` mode-4 per-tick integrator: drain `+0xC6` by the frame
/// step and, on the tick it underflows, reload the period and return this
/// tick's strip widths. `None` on every tick the period has not elapsed, and
/// on a fired tick whose steps are both zero.
///
/// `frame_step` is the adaptive per-mode step `DAT_1F800393` (2 in towns, 3
/// on the overworld). Unlike the mode-3 arm the scroller does **not** fold in
/// the `DAT_1F80037D` speed scalar - the disassembly reads `0x7f(s1)` alone.
///
/// An axis whose strip does not fit the rect (`<= 0`, or `>= w`/`>= h`) is
/// dropped: the retail descriptor arithmetic (`w - sw` as a `u16` store)
/// wraps there and would read outside the rect. No authored record in the
/// retail corpus reaches that case.
pub fn mode4_integrate(state: &mut ActorState, frame_step: u8) -> Option<VramScrollFx> {
    let step = u16::from(frame_step);
    let next = state.anim_block_u16(OFF_COUNTDOWN).wrapping_sub(step);
    state.anim_block_u16_set(OFF_COUNTDOWN, next);
    // `sll v0,0x10; bgez` - the branch tests the sign bit of the stored
    // halfword, so the arm fires exactly on the tick the counter wraps.
    if (next as i16) >= 0 {
        return None;
    }
    state.anim_block_u16_set(OFF_COUNTDOWN, state.anim_block_u16(OFF_PERIOD));

    let rect = (
        state.anim_block_u16(OFF_RECT),
        state.anim_block_u16(OFF_RECT + 2),
        state.anim_block_u16(OFF_RECT + 4),
        state.anim_block_u16(OFF_RECT + 6),
    );
    let fit = |raw: u16, extent: u16| -> u16 {
        let strip = i32::from(raw as i16) * i32::from(step);
        if strip > 0 && strip < i32::from(extent) {
            strip as u16
        } else {
            0
        }
    };
    let fx = VramScrollFx {
        rect,
        strip_w: fit(state.anim_block_u16(OFF_H_STEP), rect.2),
        strip_h: fit(state.anim_block_u16(OFF_V_STEP), rect.3),
    };
    (!fx.is_identity()).then_some(fx)
}

/// The texel kernel: rotate a `w x h` halfword rect left by `strip_w` and up
/// by `strip_h`, in that order (retail runs the horizontal arm first).
/// `texels` is the rect in row-major order, `w * h` halfwords.
pub fn rotate_rect(texels: &[u16], w: usize, h: usize, strip_w: usize, strip_h: usize) -> Vec<u16> {
    if w == 0 || h == 0 || texels.len() < w * h {
        return texels.to_vec();
    }
    let mut out = texels[..w * h].to_vec();
    if strip_w > 0 && strip_w < w {
        for row in 0..h {
            out[row * w..(row + 1) * w].rotate_left(strip_w);
        }
    }
    if strip_h > 0 && strip_h < h {
        out.rotate_left(strip_h * w);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seat a part the way move-VM op `0x1E` does.
    fn seat(state: &mut ActorState, period: u16, dx: i16, dy: i16, rect: (u16, u16, u16, u16)) {
        state.move_submode = RENDER_MODE_SCROLL;
        state.anim_block_u16_set(OFF_PERIOD, period);
        state.anim_block_u16_set(OFF_H_STEP, dx as u16);
        state.anim_block_u16_set(OFF_V_STEP, dy as u16);
        state.anim_block_u16_set(OFF_RECT, rect.0);
        state.anim_block_u16_set(OFF_RECT + 2, rect.1);
        state.anim_block_u16_set(OFF_RECT + 4, rect.2);
        state.anim_block_u16_set(OFF_RECT + 6, rect.3);
    }

    #[test]
    fn fires_on_first_tick_because_the_countdown_is_never_seated() {
        // Op 0x1E writes +0xC4/+0xCC.., never +0xC6, so a fresh part's
        // counter starts at zero and underflows immediately.
        let mut st = ActorState::new();
        seat(&mut st, 8, 0, 1, (0x220, 0x80, 0x0E, 0x80));
        let fx = mode4_integrate(&mut st, 2).expect("fires on the first tick");
        assert_eq!(fx.rect, (0x220, 0x80, 0x0E, 0x80));
        assert_eq!((fx.strip_w, fx.strip_h), (0, 2), "dy * frame_step");
        // Reloaded to +0xC4 = 8: four quiet ticks at step 2, then a fire.
        for tick in 0..4 {
            assert!(
                mode4_integrate(&mut st, 2).is_none(),
                "quiet tick {tick} inside the period"
            );
        }
        assert!(mode4_integrate(&mut st, 2).is_some(), "period elapsed");
    }

    #[test]
    fn period_zero_fires_every_tick_and_scales_with_the_frame_step() {
        let mut st = ActorState::new();
        seat(&mut st, 0, 3, 0, (0, 0, 0x40, 4));
        for _ in 0..4 {
            let fx = mode4_integrate(&mut st, 3).expect("period 0 fires every tick");
            assert_eq!((fx.strip_w, fx.strip_h), (9, 0));
        }
        // The overworld frame step widens the same authored step.
        let mut town = ActorState::new();
        seat(&mut town, 0, 3, 0, (0, 0, 0x40, 4));
        assert_eq!(mode4_integrate(&mut town, 2).unwrap().strip_w, 6);
    }

    #[test]
    fn both_axes_ride_the_same_period() {
        let mut st = ActorState::new();
        seat(&mut st, 1, 2, 5, (0x10, 0x20, 0x20, 0x40));
        let fx = mode4_integrate(&mut st, 2).expect("fires");
        assert_eq!((fx.strip_w, fx.strip_h), (4, 10));
        assert!(!fx.is_identity());
    }

    #[test]
    fn zero_and_out_of_range_steps_drop_their_axis() {
        // Both steps zero: retail leaves a0/s3 at zero and skips both arms.
        let mut st = ActorState::new();
        seat(&mut st, 0, 0, 0, (0, 0, 0x20, 0x20));
        assert!(mode4_integrate(&mut st, 2).is_none());
        // A strip wider than the rect would wrap retail's `w - sw` store.
        let mut wide = ActorState::new();
        seat(&mut wide, 0, 0x40, 1, (0, 0, 0x20, 0x20));
        let fx = mode4_integrate(&mut wide, 2).expect("the vertical axis still fits");
        assert_eq!((fx.strip_w, fx.strip_h), (0, 2));
        // A negative step multiplies to a negative strip - also dropped.
        let mut neg = ActorState::new();
        seat(&mut neg, 0, -1, 0, (0, 0, 0x20, 0x20));
        assert!(mode4_integrate(&mut neg, 2).is_none());
    }

    #[test]
    fn rotate_rect_is_cyclic_left_then_up() {
        // 4x3 rect of distinct texels.
        let src: Vec<u16> = (0..12u16).collect();
        let left = rotate_rect(&src, 4, 3, 1, 0);
        assert_eq!(left, vec![1, 2, 3, 0, 5, 6, 7, 4, 9, 10, 11, 8]);
        let up = rotate_rect(&src, 4, 3, 0, 1);
        assert_eq!(up, vec![4, 5, 6, 7, 8, 9, 10, 11, 0, 1, 2, 3]);
        // Both: horizontal first, then vertical (the retail order).
        let both = rotate_rect(&src, 4, 3, 1, 1);
        assert_eq!(both, rotate_rect(&left, 4, 3, 0, 1));
        // A full lap over w (or h) is the identity, and nothing is lost.
        let mut lap = src.clone();
        for _ in 0..4 {
            lap = rotate_rect(&lap, 4, 3, 1, 0);
        }
        assert_eq!(lap, src, "four single-column rotations restore the rect");
    }

    #[test]
    fn rotate_rect_tolerates_degenerate_shapes() {
        assert!(rotate_rect(&[], 0, 0, 1, 1).is_empty());
        // A strip that spans the whole extent is a no-op, not a panic.
        let src: Vec<u16> = (0..4u16).collect();
        assert_eq!(rotate_rect(&src, 2, 2, 2, 2), src);
        // Short buffers are returned untouched rather than indexed past.
        assert_eq!(rotate_rect(&src, 4, 4, 1, 0), src);
    }
}
