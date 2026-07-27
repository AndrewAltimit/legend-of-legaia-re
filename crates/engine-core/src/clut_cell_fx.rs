//! CLUT-cell HSV cycler - the move-VM-driven palette animation behind the
//! field scenes' "pulsating" textures (jou's Juggernaut flesh, lightning
//! brightness spikes).
//!
//! PORT: FUN_80019d50
//!
//! ## Retail chain
//!
//! A `0x4000` render-mode part (`actor[+0x5A] = 3`) whose move program runs
//! op `0x2C` (`KEY_BUFFER_ALLOC [x, y, w, h]`) captures the VRAM rect
//! `(x, y, w, h)` into a per-actor buffer (descriptor init `FUN_8005842C`;
//! `w >= 0x11` heap-allocates, smaller cells use the inline `+0xAC` buffer)
//! and arms the frame gate `+0x9C = 1`. From then on the actor render tail
//! (`FUN_80021DF4`, mode-3 arm) each frame:
//!
//! 1. integrates the HSV offset triple: `+0x90/92/94 +=
//!    (+0x96/98/9A * DAT_1F800393 * DAT_1F80037D) >> 6` (the tween source /
//!    scale registers, repurposed as **H / S / V adds** and their
//!    velocities - ops `0x2B`/`0x2E`/`0x2D` steer them);
//! 2. ramps the white-blend amount `+0x68 += (+0x6A * dt) >> 6`, clamped to
//!    `0x100`;
//! 3. once `+0x9C > 1`, calls `FUN_80019D50(mode = +0x9E, white = +0x68,
//!    h = +0x90, s = +0x92, v = +0x94, captured_texels, descriptor)` -
//!    which rewrites every captured 15-bit texel through an RGB→HSV→RGB
//!    round trip with the adds applied, optionally blends toward
//!    white/invert (`mode == 1`: `c += (255 - 2c) * white >> 8`), repacks
//!    with the STP bit preserved (zero texels stay zero), and emits a
//!    `LoadImage` packet of the result onto the captured rect
//!    (`FUN_800583C8` enqueue);
//! 4. advances `+0x9C` (clamped at 1000).
//!
//! jou's ambient stager tree is the worked example
//! (`docs/subsystems/field-ambient-fx.md`): fifteen spawned instances of one
//! record tile CLUT row 502 in 16-halfword cells (a self-modifying
//! `ext 0x1E` steps the captured `x` per spawn), idle at zero adds, and jump
//! their S/V adds on the shared lightning flag - palette-space pulses with
//! no vertex ever moving.
//!
//! ## Engine split
//!
//! The **integrator** ([`mode3_integrate`]) mutates the move-VM
//! [`ActorState`] exactly like the retail tail; the **writer**
//! ([`apply_hsv_cell`]) is a pure texel kernel. The renderer owns VRAM:
//! it captures the rect's texels the first time a part's
//! [`ClutCellFx`] surfaces (`World::active_clut_cell_fx`), applies
//! [`apply_hsv_cell`] per frame, and uploads the result - the engine-side
//! mirror of the per-frame `LoadImage` packet.
//!
//! REF: FUN_80021DF4, FUN_8005842C, FUN_800583C8

use legaia_engine_vm::move_vm::{ActorState, hsv_to_rgb, rgb_to_hsv};

/// The retail startup value of the game-speed multiplier `DAT_1F80037D`
/// (see `MoveHost::keyframe_curve_multiplier`).
pub const SPEED_SCALAR: u32 = 0x10;

/// One live CLUT-cell effect, snapshot from a mode-3 part's actor state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClutCellFx {
    /// Captured VRAM rect `(x, y, w, h)` in halfwords (op `0x2C` operands).
    pub rect: (u16, u16, u16, u16),
    /// Hue add, degrees `0..0x168` space (`actor[+0x90]`).
    pub h_add: i16,
    /// Saturation add (`actor[+0x92]`).
    pub s_add: i16,
    /// Value/brightness add (`actor[+0x94]`).
    pub v_add: i16,
    /// Post-HSV blend mode (`actor[+0x9E]`; `1` = white/invert blend).
    pub mode: i16,
    /// White-blend amount `0..=0x100` (`actor[+0x68]`).
    pub white: i16,
}

impl ClutCellFx {
    /// `true` when this frame's write would reproduce the captured texels
    /// exactly (all adds zero and no white blend) - lets a renderer skip
    /// the rewrite for idle cells.
    pub fn is_identity(&self) -> bool {
        self.h_add == 0 && self.s_add == 0 && self.v_add == 0 && (self.mode != 1 || self.white == 0)
    }
}

/// The `FUN_80021DF4` mode-3 per-frame integrator: advance the H/S/V adds by
/// their velocities, ramp the white-blend amount, and advance the frame gate.
/// Returns the effect snapshot to write this frame (`None` until the gate has
/// seen one armed frame, mirroring `+0x9C > 1`).
///
/// `frame_step` is the adaptive per-mode step `DAT_1F800393` (2 in towns, 3
/// on the overworld); the speed scalar is pinned at [`SPEED_SCALAR`].
pub fn mode3_integrate(state: &mut ActorState, frame_step: u8) -> Option<ClutCellFx> {
    let dt = u32::from(frame_step) * SPEED_SCALAR;
    let step = |v: i16| -> i16 { ((i32::from(v) * dt as i32) >> 6) as i16 };
    state.tween_src_x = state.tween_src_x.wrapping_add(step(state.tween_scale_x));
    state.tween_src_y = state.tween_src_y.wrapping_add(step(state.tween_scale_y));
    state.tween_src_z = state.tween_src_z.wrapping_add(step(state.tween_scale_z));
    let ramped = i32::from(state.field_68) + ((i32::from(state.field_6a) * dt as i32) >> 6);
    // The retail clamp is on the sign-extended 16-bit value: `0x100 <
    // (short)v` caps at 0x100 (no lower clamp - negative ramps wrap).
    state.field_68 = if ramped > 0x100 { 0x100 } else { ramped as i16 };
    if state.field_9c == 0 {
        return None;
    }
    let emit = state.field_9c > 1;
    state.field_9c = (state.field_9c + 1).min(1000);
    emit.then_some(ClutCellFx {
        rect: (
            state.keyframe_desc[0],
            state.keyframe_desc[1],
            state.keyframe_desc[2],
            state.keyframe_desc[3],
        ),
        h_add: state.tween_src_x,
        s_add: state.tween_src_y,
        v_add: state.tween_src_z,
        mode: state.field_9e as i16,
        white: state.field_68,
    })
}

/// The `FUN_80019D50` texel kernel: rewrite `src` (15-bit BGR555 halfwords,
/// the captured cell) through the HSV shift + optional white blend. Zero
/// texels stay zero and every texel's STP bit is preserved.
pub fn apply_hsv_cell(src: &[u16], fx: &ClutCellFx) -> Vec<u16> {
    src.iter()
        .map(|&texel| {
            if texel == 0 {
                return 0;
            }
            let stp = texel & 0x8000;
            let r = i32::from(texel & 0x1F) << 3;
            let g = (i32::from(texel) >> 2) & 0xF8;
            let b = (i32::from(texel) >> 7) & 0xF8;
            let (h, s, v) = rgb_to_hsv(r, g, b);
            let h = (h + i32::from(fx.h_add)).rem_euclid(0x168);
            let s = (s + i32::from(fx.s_add)).clamp(0, 0xFF);
            let v = (v + i32::from(fx.v_add)).clamp(0, 0xFF);
            let (nr, ng, nb) = hsv_to_rgb(h, s, v);
            // FUN_8001A6C8 caps each channel at 0xF8 before the caller's
            // blend (the same clamp ext sub-ops 0x1F/0x20 mirror).
            let (mut nr, mut ng, mut nb) = (nr.min(0xF8), ng.min(0xF8), nb.min(0xF8));
            if fx.mode == 1 {
                // White/invert blend: c += (255 - 2c) * white >> 8. At
                // white = 0x100 this inverts; at 0x80 it converges to grey.
                let blend =
                    |c: i32| -> i32 { c + ((((0xFF & !c) - c) * i32::from(fx.white)) >> 8) };
                nr = blend(nr);
                ng = blend(ng);
                nb = blend(nb);
            }
            ((nr >> 3) | ((ng >> 3) << 5) | ((nb >> 3) << 10)) as u16 | stp
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(h: i16, s: i16, v: i16, mode: i16, white: i16) -> ClutCellFx {
        ClutCellFx {
            rect: (0, 502, 16, 1),
            h_add: h,
            s_add: s,
            v_add: v,
            mode,
            white,
        }
    }

    #[test]
    fn zero_adds_round_trip_preserves_zero_and_stp() {
        // Zero texels stay zero (transparent CLUT slots untouched).
        let out = apply_hsv_cell(&[0, 0x8000 | 0x7FFF], &fx(0, 0, 0, 0, 0));
        assert_eq!(out[0], 0);
        assert_eq!(out[1] & 0x8000, 0x8000, "STP preserved");
    }

    #[test]
    fn v_add_darkens_toward_black() {
        // A mid-grey texel with V-add -255 collapses to (near) black.
        let grey = 0x10 | (0x10 << 5) | (0x10 << 10);
        let out = apply_hsv_cell(&[grey], &fx(0, 0, -255, 0, 0));
        assert_eq!(out[0], 0x8000 & out[0], "fully darkened");
        // And a positive V-add brightens.
        let bright = apply_hsv_cell(&[grey], &fx(0, 0, 100, 0, 0))[0];
        assert!((bright & 0x1F) > (grey as u16 & 0x1F));
    }

    #[test]
    fn white_blend_full_inverts_channels() {
        // mode 1, white 0x100: c += (255 - 2c) → 255 - c (invert).
        let dark_red = 0x08u16; // r = 0x40
        let out = apply_hsv_cell(&[dark_red], &fx(0, 0, 0, 1, 0x100));
        let r = out[0] & 0x1F;
        assert!(r >= 0x17, "dark red inverts to bright: r5 = {r:#x}");
    }

    #[test]
    fn integrator_gates_on_9c_and_ramps_adds() {
        let mut st = ActorState::new();
        // Not armed: integrates but never emits.
        st.tween_scale_z = -0x100;
        assert!(mode3_integrate(&mut st, 2).is_none());
        assert_eq!(st.tween_src_z, -(0x100 * 2 * 0x10 >> 6) as i16);
        // Arm the gate the way op 0x2C does.
        st.field_9c = 1;
        st.keyframe_desc = [0x70, 0x1F8, 0x10, 1];
        // First armed frame: 9c 1 → 2, no emit yet.
        assert!(mode3_integrate(&mut st, 2).is_none());
        // Second armed frame emits with the integrated adds.
        let fx = mode3_integrate(&mut st, 2).expect("emits once 9c > 1");
        assert_eq!(fx.rect, (0x70, 0x1F8, 0x10, 1));
        assert_eq!(fx.v_add, st.tween_src_z);
        // The white ramp clamps at 0x100.
        st.field_6a = 0x7FFF;
        mode3_integrate(&mut st, 2);
        assert_eq!(st.field_68, 0x100);
    }

    #[test]
    fn identity_snapshot_skips() {
        assert!(fx(0, 0, 0, 0, 0).is_identity());
        assert!(
            fx(0, 0, 0, 0, 0x100).is_identity(),
            "white idle without mode 1"
        );
        assert!(!fx(0, 0, -1, 0, 0).is_identity());
        assert!(!fx(0, 0, 0, 1, 1).is_identity());
    }
}
