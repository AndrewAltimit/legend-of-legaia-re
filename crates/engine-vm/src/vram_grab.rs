//! The framebuffer grab: the render-mode-7 arm of the part tick
//! `FUN_80021DF4` (`0x80022C20..0x80022CB8`).
//!
//! A part whose `+0x5A` is `7` draws nothing. Each tick it projects its own
//! position and copies the screen rectangle around that point out of the
//! frame being drawn into a fixed VRAM rectangle, where a textured mesh can
//! sample it. Read off the disassembly:
//!
//! 1. `jal 0x800172C0` loads the field camera into the GTE; when
//!    `+0x52 & 0x400` is set, `FUN_8003D1A4(0x8007BF10)` replaces it with the
//!    base matrix block (rotation `S_b`, zero translation), so the point is
//!    an eye-space offset locked to the camera - the same bit the render
//!    dispatcher's camera-relative arm keys on.
//! 2. `FUN_8003D368(part + 0x14, sp + 0x28)` - one `RTPS` of the part's
//!    `+0x14 / +0x16 / +0x18`: `SXY2` stored, `IR3` returned.
//! 3. `jal 0x800172C0` again, restoring the field camera.
//! 4. `FUN_800468A4(ot, sx + [+0xCC], sy + [+0xCE], [+0xD4], [+0xD6],
//!    [+0xD0], [+0xD2])`: the ordering-table slot is
//!    `(IR3 + [+0xC4]) >> (byte 0x1F8003A4 + 2)`, the source corner the
//!    projected point plus the offset pair, the extent `+0xD4 / +0xD6`, the
//!    destination `+0xD0 / +0xD2`. The callee links a GP0 `0x80`
//!    VRAM-to-VRAM copy ([`crate::vram_rect_copy`]).
//!
//! Op `0x45` seats the operand block (`0x8002409C..0x800240F8`). Every
//! shipped carrier is a field prescript record that grabs `64 x 64` pixels
//! centred on itself (`-32, -32`) into `(832, 0)`.
//!
//! This module is the VM-side half: [`grab_request`] reads a part's state
//! into a [`GrabRequest`]. The projection runs where the frame's camera is -
//! `legaia_render_kernels::vram_grab`.
//!
//! REF: FUN_80021DF4 (the render-mode-7 arm, `0x80022C20..0x80022CB8`)

use crate::move_vm::ActorState;

/// The render mode (`+0x5A`) of a grab part; op `0x45` is its only binder.
pub const GRAB_RENDER_MODE: i16 = 7;

/// `+0x52` bit that projects the part through the base matrix alone
/// (`andi v0,v0,0x400` at `0x80022C40`).
pub const GRAB_CAMERA_LOCKED: u16 = 0x0400;

/// One tick's grab, before projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrabRequest {
    /// `+0x14 / +0x16 / +0x18` - the projected point.
    pub pos: [i16; 3],
    /// `+0x52 & 0x400`: project through the base matrix, not the camera.
    pub camera_locked: bool,
    /// `+0xC4` - added to `IR3` before the ordering-table shift.
    pub depth_bias: i16,
    /// `+0xCC / +0xCE` - source corner relative to the projected point.
    pub offset: [i16; 2],
    /// `+0xD4 / +0xD6` - copy width and height.
    pub size: [i16; 2],
    /// `+0xD0 / +0xD2` - destination corner in VRAM.
    pub dest: [i16; 2],
}

/// The grab a part issues this tick, or `None` when its `+0x5A` is not
/// [`GRAB_RENDER_MODE`].
pub fn grab_request(st: &ActorState) -> Option<GrabRequest> {
    if st.move_submode != GRAB_RENDER_MODE {
        return None;
    }
    // `+0xAC`-relative window offsets of `+0xC4 .. +0xD6`.
    let w = |off: usize| st.anim_block_u16(off) as i16;
    Some(GrabRequest {
        pos: [st.world_x, st.world_y, st.world_z],
        camera_locked: st.field_52 & GRAB_CAMERA_LOCKED != 0,
        depth_bias: w(0x18),
        offset: [w(0x20), w(0x22)],
        size: [w(0x28), w(0x2A)],
        dest: [w(0x24), w(0x26)],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::move_vm::{MoveHost, StepResult, step};

    struct NullHost;
    impl MoveHost for NullHost {}

    /// `kor5`'s grab record: op `0x45` with depth bias `0x300`, offset
    /// `(-32, -32)`, extent `64 x 64`, destination `(832, 0)`.
    #[test]
    fn op_0x45_seats_the_block_the_arm_reads() {
        let words = [
            0x0045u16, 0x0300, 0xFFE0, 0xFFE0, 0x0040, 0x0040, 0x0340, 0x0000, 0x0008,
        ];
        let mut st = ActorState::new();
        st.world_x = 2368;
        st.world_z = 5120;
        let mut h = NullHost;
        assert!(matches!(step(&mut h, &mut st, &words), StepResult::Advance));
        let r = grab_request(&st).expect("mode 7");
        assert_eq!(r.depth_bias, 0x300);
        assert_eq!(r.offset, [-32, -32]);
        assert_eq!(r.size, [64, 64]);
        assert_eq!(r.dest, [832, 0]);
        assert_eq!(r.pos, [2368, 0, 5120]);
        assert!(!r.camera_locked);
    }

    #[test]
    fn another_mode_issues_no_grab() {
        let st = ActorState::new();
        assert_eq!(grab_request(&st), None);
    }
}
