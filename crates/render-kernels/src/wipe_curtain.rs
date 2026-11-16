//! The screen wipe's curtain: what the field draws over itself while the
//! pause menu's brightness accumulator `_DAT_8007B440` ramps.
//!
//! The field-overlay session `FUN_801ED308` hands the accumulator to the
//! SCUS emitter `FUN_8003479C` on every pass, and the emitter does not fade
//! the frame evenly. It closes two soft-edged curtains from the top and the
//! bottom of the screen toward the middle:
//!
//! - level `0` draws nothing (`beq s4,zero` at `0x800347B8`);
//! - from `0xF2` up the level clamps to `0xF2` and one opaque black
//!   `POLY_F4` covers the screen, `y -4 .. 0xE0` (`0x800347C0..0x80034810`);
//! - from `0x81` up two opaque black bars stand above the top band and below
//!   the bottom band: `y -4 .. level - 0x84` and `0x160 - level .. 0xE0`
//!   (`0x80034818..0x800348A8`);
//! - always, two semi-transparent `POLY_G4` bands `0x80` lines tall: the top
//!   one from `level - 0x84` (white) to `level - 4` (black), the bottom one
//!   from `0xE0 - level` (black) to `0x160 - level` (white)
//!   (`0x800348F0..0x80034994`), bracketed by two draw-mode packets,
//!   texpage `0x1E` linked first and `0x5E` last.
//!
//! Every packet links into ordering-table entry `1` (`lw a0,0xE0(0x1F800314)`
//! then `addiu a0,a0,4` ahead of each `FUN_8003D2C4`), so the GPU walks them
//! in reverse: the `0x5E` mode (ABR `2`, `B - F`) first, then the two bands
//! subtracting their colour from the frame - full white is black, black is
//! no change - then the `0x1E` mode and the opaque bars. A band therefore
//! darkens the field from nothing at its inner edge to black at its outer
//! one, and the visible strip of field narrows toward the screen's middle
//! as the level rises. The town01 capture (`autorun_pause_wipe_step.lua`)
//! shows exactly that strip at level `140`.
//!
//! [`curtain_packets`] is the emit order the routine follows, which the
//! retail kernel differential compares packet for packet;
//! [`curtain_prims`] is the same packets as screen primitives, which both
//! play hosts draw through `engine-screens`' screen layers.
//!
//! PORT: FUN_8003479C

use crate::screen_prim::{FlatQuad, ScreenPrim};

/// The level the emitter clamps to, and at which it covers the screen.
pub const CURTAIN_FULL: i32 = 0xF2;
/// From this level up the opaque bars stand beyond the bands.
pub const CURTAIN_BARS_FROM: i32 = 0x81;
/// Screen width the emitter authors (`0x140`).
pub const CURTAIN_W: i16 = 0x140;
/// Screen bottom the emitter authors (`0xE0`).
pub const CURTAIN_RETAIL_H: i16 = 0xE0;
/// The top edge every top quad starts from (`-4`, the draw offset's bias).
pub const CURTAIN_TOP: i16 = -4;
/// Draw-mode texpage linked ahead of the bands (`0x1E`, ABR `0`).
pub const CURTAIN_MODE_RESET: u16 = 0x1E;
/// Draw-mode texpage the bands draw under (`0x5E`, ABR `2`: `B - F`).
pub const CURTAIN_MODE_SUBTRACT: u16 = 0x5E;
/// Ordering-table entry every packet links into.
pub const CURTAIN_OT: u32 = 1;

/// One packet of the emitter, in the order it builds and links them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurtainPacket {
    /// `FUN_8003C510`: an opaque black `POLY_F4` spanning the full width
    /// between two lines.
    Bar { y0: i16, y1: i16 },
    /// `FUN_80059010`: a draw-mode packet with this texpage.
    DrawMode { tpage: u16 },
    /// `FUN_8003C43C`: a semi-transparent `POLY_G4` spanning the full width,
    /// its top edge in `top` and its bottom edge in `bottom` (`0x00BBGGRR`).
    Band {
        y0: i16,
        y1: i16,
        top: u32,
        bottom: u32,
    },
}

/// The packets the emitter builds for `level`, in link order.
pub fn curtain_packets(level: i32) -> Vec<CurtainPacket> {
    if level == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(7);
    let mut l = level;
    if l >= CURTAIN_FULL {
        l = CURTAIN_FULL;
        out.push(CurtainPacket::Bar {
            y0: CURTAIN_TOP,
            y1: CURTAIN_RETAIL_H,
        });
    }
    // The emitter stores each coordinate with `sh`, so a level outside the
    // ramp's range wraps the way the halfword does.
    let y = |v: i32| v as i16;
    if l >= CURTAIN_BARS_FROM {
        out.push(CurtainPacket::Bar {
            y0: CURTAIN_TOP,
            y1: y(l - 0x84),
        });
        out.push(CurtainPacket::Bar {
            y0: y(0x160 - l),
            y1: CURTAIN_RETAIL_H,
        });
    }
    out.push(CurtainPacket::DrawMode {
        tpage: CURTAIN_MODE_RESET,
    });
    out.push(CurtainPacket::Band {
        y0: y(l - 0x84),
        y1: y(l - 4),
        top: 0x00FF_FFFF,
        bottom: 0,
    });
    out.push(CurtainPacket::Band {
        y0: y(0xE0 - l),
        y1: y(0x160 - l),
        top: 0,
        bottom: 0x00FF_FFFF,
    });
    out.push(CurtainPacket::DrawMode {
        tpage: CURTAIN_MODE_SUBTRACT,
    });
    out
}

/// The curtain for `level` as screen primitives, in link order at
/// [`CURTAIN_OT`] (the ordering-table walk draws them last-linked first).
///
/// `stage_h` is the stage bottom: a bar retail ends on its `0xE0` literal
/// runs on to the engine's taller stage, as the shutter bars do
/// ([`crate::screen_prim::cinematic_bar_prims`]), so no line of field shows
/// under a closed curtain. The draw-mode packets carry no pixels; the
/// subtractive blend they select travels on each band.
pub fn curtain_prims(level: i32, stage_h: i16) -> Vec<ScreenPrim> {
    let bottom = stage_h.max(CURTAIN_RETAIL_H);
    let quad = |y0: i16, y1: i16| [(0, y0), (CURTAIN_W, y0), (0, y1), (CURTAIN_W, y1)];
    let rgba = |c: u32| [(c & 0xFF) as u8, (c >> 8) as u8, (c >> 16) as u8, 0xFF];
    curtain_packets(level)
        .into_iter()
        .filter_map(|p| match p {
            CurtainPacket::Bar { y0, y1 } => {
                let y1 = if y1 == CURTAIN_RETAIL_H { bottom } else { y1 };
                Some(ScreenPrim::Flat(FlatQuad {
                    xy: quad(y0, y1),
                    color: [0, 0, 0, 0xFF],
                    gouraud: None,
                    semi_transparent: false,
                    abr_mode: 0,
                    ot_index: CURTAIN_OT,
                    depth: None,
                }))
            }
            CurtainPacket::DrawMode { .. } => None,
            CurtainPacket::Band {
                y0,
                y1,
                top,
                bottom,
            } => {
                let (t, b) = (rgba(top), rgba(bottom));
                Some(ScreenPrim::Flat(FlatQuad {
                    xy: quad(y0, y1),
                    color: t,
                    gouraud: Some([t, t, b, b]),
                    semi_transparent: true,
                    abr_mode: 2,
                    ot_index: CURTAIN_OT,
                    depth: None,
                }))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_zero_draws_nothing() {
        assert!(curtain_packets(0).is_empty());
        assert!(curtain_prims(0, 240).is_empty());
    }

    /// Below `0x81` only the two bands, bracketed by the draw modes.
    #[test]
    fn a_low_level_is_two_bands() {
        let p = curtain_packets(20);
        assert_eq!(
            p,
            vec![
                CurtainPacket::DrawMode { tpage: 0x1E },
                CurtainPacket::Band {
                    y0: 20 - 0x84,
                    y1: 16,
                    top: 0xFF_FFFF,
                    bottom: 0
                },
                CurtainPacket::Band {
                    y0: 0xE0 - 20,
                    y1: 0x160 - 20,
                    top: 0,
                    bottom: 0xFF_FFFF
                },
                CurtainPacket::DrawMode { tpage: 0x5E },
            ]
        );
    }

    /// At `140` the bars stand outside bands that overlap in the middle:
    /// the strip of field the town01 capture shows.
    #[test]
    fn a_high_level_adds_the_bars() {
        let p = curtain_packets(140);
        assert_eq!(p.len(), 6);
        assert_eq!(p[0], CurtainPacket::Bar { y0: -4, y1: 8 });
        assert_eq!(p[1], CurtainPacket::Bar { y0: 212, y1: 0xE0 });
    }

    /// From `0xF2` up the level clamps and a full-screen bar leads.
    #[test]
    fn a_full_level_clamps_and_covers() {
        let p = curtain_packets(300);
        assert_eq!(p[0], CurtainPacket::Bar { y0: -4, y1: 0xE0 });
        assert_eq!(
            p[1],
            CurtainPacket::Bar {
                y0: -4,
                y1: 0xF2 - 0x84
            }
        );
        assert_eq!(curtain_packets(0xF2), p);
    }

    /// The prims: bars opaque and stretched to the stage, bands subtractive
    /// gouraud, all at OT `1`.
    #[test]
    fn the_prims_carry_the_blend_and_the_stage() {
        let prims = curtain_prims(0xF2, 240);
        assert_eq!(prims.len(), 5);
        let ScreenPrim::Flat(full) = prims[0] else {
            panic!("flat")
        };
        assert_eq!(full.xy[3], (0x140, 240));
        assert!(!full.semi_transparent);
        let ScreenPrim::Flat(band) = prims[3] else {
            panic!("flat")
        };
        assert!(band.semi_transparent);
        assert_eq!(band.abr_mode, 2);
        assert_eq!(
            band.gouraud,
            Some([
                [0xFF, 0xFF, 0xFF, 0xFF],
                [0xFF, 0xFF, 0xFF, 0xFF],
                [0, 0, 0, 0xFF],
                [0, 0, 0, 0xFF]
            ])
        );
        assert!(prims.iter().all(|p| p.ot_index() == CURTAIN_OT));
    }
}
