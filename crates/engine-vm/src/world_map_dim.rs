//! World-map top-view screen-dim pass, ported clean-room from
//! `FUN_801E75DC`.
//!
//! PORT: FUN_801E75DC
//!
//! ## Which image this lives in
//!
//! `FUN_801E75DC` is resident in the **field overlay, PROT 0897**
//! (`extracted/overlays/overlay_field_0897.bin`, base `0x801CE818`, file
//! offset `0x18DC4`). The historical `overlay_world_map_*` capture dumps are
//! byte-identical to that image at the same VAs - the world map is a
//! 0897-hosted *mode*, not an overlay of its own, the same relationship
//! `functions.md` already records for the move-VM overlay extension. The
//! bytes were resolved against the extracted image rather than taken from a
//! dump, so neither the `+0xE818` nor the `+0x5818` mis-base cluster
//! (`docs/tooling/dump-corpus-integrity.md`) applies here.
//!
//! ## Reachability
//!
//! Retail-reachable, but only behind the **top-view debug path**. The single
//! call site is in the world-map controller `FUN_801E76D4`, gated by two
//! consecutive branches (`0x801E7794..0x801E77B8`):
//!
//! ```text
//! lbu v0, 0x2b94(v0)   ; DAT_801F2B94 - view mode
//! beqz v0, ...         ; 0 = normal walk -> skip the whole top-view block
//! lbu v0, 0x2b95(s0)   ; DAT_801F2B95 - top-view animation flags
//! andi v0, v0, 1
//! beqz v0, 0x801e77c0  ; bit 0 clear -> skip
//! jal  0x801e75dc
//! ```
//!
//! so `view_mode != 0 && (anim_flags & 1) != 0`. Entering top view at all
//! additionally needs the debug flag `_DAT_8007B98C`, which retail leaves
//! clear - see [`engine_core::world_map::WorldMapController`].
//!
//! ## What it draws
//!
//! Three primitives, in OT order, all posted through `AddPrim`
//! (`FUN_8003D2C4`) into the OT at `*(0x1F800314 + 0xE0) + 8`, allocated off
//! the scratchpad prim-pool cursor at `0x1F800314 + 0x8C`:
//!
//! | # | Packet | Bytes | Retail construction |
//! |---|---|---|---|
//! | 0 | `DR_MODE` | 12 | `SetDrawMode(p, dfe=0, dtd=0, tpage=0x1E, tw=NULL)` (`FUN_80059010`) |
//! | 1 | `POLY_F4` | 24 | tag `0x05000000`, GP0 word `0x2A808080` with the three colour bytes then overwritten to zero (`sb zero, 4/5/6`) -> `0x2A000000` |
//! | 2 | `DR_MODE` | 12 | `SetDrawMode(p, dfe=0, dtd=1, tpage=0x1E, tw=NULL)` |
//!
//! GP0 command `0x2A` is a flat, untextured, **semi-transparent** quad, and
//! the colour is black. `tpage = 0x1E` selects semi-transparency mode
//! `ABR = (0x1E >> 5) & 3 = 0`, i.e. `0.5 * back + 0.5 * front`. With a black
//! front, that halves whatever is already in the framebuffer: the pass is a
//! **50% screen darken**, drawn behind the top-view debug panels - not an
//! "animation step".
//!
//! The two `DR_MODE` packets bracket it because the blend quad must not be
//! dithered: dither goes off (`dtd = 0`) before the quad and back on
//! (`dtd = 1`) after, leaving the mode word restored for whatever draws next.
//!
//! ## Vertex geometry
//!
//! The four vertices are literal constants (`0x801E764C..0x801E7674`), stored
//! as `i16` pairs:
//!
//! ```text
//! (0, -4)  (320, -4)
//! (0, 224) (320, 224)
//! ```
//!
//! A full 320x224 NTSC draw area, started four scanlines above the top edge
//! so the band the horizon emitter also starts at (`y = i - 4`) is covered.

/// Semi-transparency mode carried by the `tpage` word both `DR_MODE`
/// packets set (`0x1E`): `ABR = (tpage >> 5) & 3 = 0` = `0.5*B + 0.5*F`.
pub const DIM_TPAGE: u16 = 0x1E;

/// GP0 command byte of the blend quad: flat, untextured, 4-vertex,
/// semi-transparent (`0x20 | ABE | QUAD`).
pub const DIM_GP0_CMD: u8 = 0x2A;

/// One `DR_MODE` packet (`SetDrawMode`), as retail fills it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawModePacket {
    /// `dfe` - drawing to the display area enabled. Zero in both packets.
    pub draw_on_display: bool,
    /// `dtd` - dither enable. `false` before the quad, `true` after.
    pub dither: bool,
    /// `tpage` word. [`DIM_TPAGE`] in both packets.
    pub tpage: u16,
}

/// The full-screen semi-transparent blend quad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DimQuad {
    /// GP0 command byte. [`DIM_GP0_CMD`].
    pub cmd: u8,
    /// Flat colour, after the three `sb zero` stores. Always black.
    pub color: (u8, u8, u8),
    /// The four `(x, y)` vertices in retail packet order: top-left,
    /// top-right, bottom-left, bottom-right.
    pub verts: [(i16, i16); 4],
}

/// One frame's screen-dim pass: the three primitives `FUN_801E75DC` posts,
/// in OT order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenDimPass {
    /// Packet 0 - dither off.
    pub mode_before: DrawModePacket,
    /// Packet 1 - the blend quad.
    pub quad: DimQuad,
    /// Packet 2 - dither back on.
    pub mode_after: DrawModePacket,
}

impl ScreenDimPass {
    /// Total bytes this pass allocates from the scratchpad prim pool
    /// (`0x1F800314 + 0x8C`): `12 + 24 + 12`. Retail advances the cursor by
    /// exactly this much across the three allocations.
    pub const POOL_BYTES: u32 = 12 + 24 + 12;
}

/// Build the screen-dim pass.
///
/// Retail takes no arguments and reads no state - every field is a literal
/// in the function body - so this is a pure constructor. Kept as a function
/// rather than a constant to keep the one-call-site-per-frame shape of the
/// original and to give the port tag a body to sit on.
// PORT: FUN_801E75DC
pub fn emit_screen_dim() -> ScreenDimPass {
    ScreenDimPass {
        // 0x801E75EC..0x801E7614: a1 = 0 (dfe), a2 = 0 (dtd), a3 = 0x1E.
        mode_before: DrawModePacket {
            draw_on_display: false,
            dither: false,
            tpage: DIM_TPAGE,
        },
        quad: DimQuad {
            // 0x801E7628 loads 0x2A808080; 0x801E7674..0x801E767C then
            // zero the low three bytes, leaving the command and a black
            // colour.
            cmd: DIM_GP0_CMD,
            color: (0, 0, 0),
            // 0x801E764C..0x801E7670, stored as `sh` at packet +0x08..+0x16.
            verts: [(0, -4), (320, -4), (0, 224), (320, 224)],
        },
        // 0x801E768C..0x801E76A8: a1 = 0 (dfe), a2 = 1 (dtd), a3 = 0x1E.
        mode_after: DrawModePacket {
            draw_on_display: false,
            dither: true,
            tpage: DIM_TPAGE,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The literal packet fields, read straight off the disassembly of
    /// `overlay_field_0897.bin` at `0x801E75DC`.
    #[test]
    fn packet_fields_match_retail_literals() {
        let p = emit_screen_dim();
        assert_eq!(p.quad.cmd, 0x2A);
        assert_eq!(p.quad.color, (0, 0, 0));
        assert_eq!(p.quad.verts, [(0, -4), (320, -4), (0, 224), (320, 224)]);
        assert_eq!(p.mode_before.tpage, 0x1E);
        assert_eq!(p.mode_after.tpage, 0x1E);
    }

    /// The whole point of the two `DR_MODE` packets: dither is off across
    /// the blend quad and restored after it.
    #[test]
    fn dither_is_bracketed_off_then_on() {
        let p = emit_screen_dim();
        assert!(!p.mode_before.dither);
        assert!(p.mode_after.dither);
        assert!(!p.mode_before.draw_on_display);
        assert!(!p.mode_after.draw_on_display);
    }

    /// 320x224 starting four scanlines above the top edge.
    #[test]
    fn quad_covers_the_full_ntsc_draw_area() {
        let v = emit_screen_dim().quad.verts;
        let xs: Vec<i16> = v.iter().map(|p| p.0).collect();
        let ys: Vec<i16> = v.iter().map(|p| p.1).collect();
        assert_eq!(*xs.iter().min().unwrap(), 0);
        assert_eq!(*xs.iter().max().unwrap(), 320);
        assert_eq!(*ys.iter().min().unwrap(), -4);
        assert_eq!(*ys.iter().max().unwrap(), 224);
    }

    #[test]
    fn pool_footprint_is_three_packets() {
        assert_eq!(ScreenDimPass::POOL_BYTES, 48);
    }
}
