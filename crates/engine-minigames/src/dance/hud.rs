//! HUD widget quads and the HUD driver.
//! Split out of `dance.rs`.

use super::*;

// --------------------------------------------- HUD widget quad + HUD driver

/// One resolved dance HUD quad - the renderer-agnostic form of the 12-word
/// `POLY_GT4` packet the overlay's emitter builds.
///
/// Deliberately **not** [`crate::baka_fighter::HudWidgetQuad`]: the two
/// emitters differ on their edges. Baka's spans `u ..= u + w - 1` inclusive;
/// the dance emitter writes `u + w` and `x + hw` straight out, so its rects
/// are half-open. Sharing one struct would silently pick one convention for
/// both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DanceHudQuad {
    /// GP0 polygon code (`(semi << 1) | 0x3C`).
    pub poly_code: u8,
    /// Quad corners, **half-open**: `x0 .. x1` by `y0 .. y1`.
    pub x0: i16,
    pub y0: i16,
    pub x1: i16,
    pub y1: i16,
    /// Per-corner texture coordinates in vertex order (TL, TR, BL, BR),
    /// half-open the same way.
    pub uv: [(u8, u8); 4],
    /// Brightness-scaled gouraud colours: verts 0/1 take `rgb_top`, verts 2/3
    /// take `rgb_bottom`.
    pub rgb_top: [u8; 3],
    pub rgb_bottom: [u8; 3],
    /// CLUT id, after the mode-2 override.
    pub clut: u16,
    /// Texpage attribute after the ABR fold (`tpage + abr * 0x20`).
    pub tpage_attr: u16,
}

/// Parse the HUD widget table out of an overlay image, pairing each record
/// with its `+0x13` ABR byte.
///
/// `legaia_asset::dance_art::parse_widgets` decodes every field the emitter
/// reads **except** `+0x13`, the semi-transparency rate that folds into the
/// texpage attribute. Until that parser carries it the byte is lifted here off
/// the same committed offsets the parser uses, so there is one source for the
/// table's geometry.
pub fn dance_widgets_with_abr(overlay: &[u8]) -> Vec<(legaia_asset::dance_art::DanceWidget, u8)> {
    use legaia_asset::dance_art::{DANCE_OVERLAY_BASE_VA, WIDGET_STRIDE, WIDGET_TABLE_VA};
    let Ok(widgets) = legaia_asset::dance_art::parse_widgets(overlay) else {
        return Vec::new();
    };
    let base = (WIDGET_TABLE_VA - DANCE_OVERLAY_BASE_VA) as usize;
    widgets
        .into_iter()
        .enumerate()
        .map(|(i, w)| {
            let abr = overlay
                .get(base + i * WIDGET_STRIDE + 0x13)
                .copied()
                .unwrap_or(0);
            (w, abr)
        })
        .collect()
}

/// PROT entry carrying the dance hall's own TIM set - the last slot of the
/// `other7` scene block, an `asset::pack` of 31 TIMs behind a `TIM_LIST` chunk
/// header.
///
/// The HUD's texture page is one of them: the widget table's `tpage` resolves
/// to the 4bpp page at `(512, 0)`, and exactly one member of this pack targets
/// that origin, carrying the CLUT strip at `(0, 500)` the widget CLUT ids
/// index. So the sprite the count-in banner, the score digits, the gauge and
/// the beat track all sample is disc data in this entry, not overlay rodata.
///
/// Retail never has to think about it: the dance **is** `other7`, so the whole
/// pack is resident before the minigame starts. The port suspends whichever
/// scene the player walked in from and runs the dance over it, so the pages
/// this pack would have supplied are the interrupted scene's. Staging is
/// therefore selective - see [`stage_dance_hud_vram`].
pub const DANCE_HUD_ART_PROT_ENTRY: u32 = 1230;

/// One VRAM rect pair the dance HUD samples: `(texture page origin, CLUT
/// origin)`, both in halfword framebuffer coordinates.
///
/// The pair travels together because a widget id names a palette **column**
/// of a strip some other TIM owns whole - staging the page without the strip
/// draws the right shapes in the wrong colours, and there is no error to
/// notice.
pub type DanceHudRect = ((u16, u16), (u16, u16));

/// Widget id bits the emitter takes as the widget **index** (`id & 0x3FF`).
pub const DANCE_WIDGET_ID_MASK: u32 = 0x3FF;
/// Blend mode the emitter takes from the id's upper bits (`id >> 10`).
pub const DANCE_WIDGET_MODE_SHIFT: u32 = 10;
/// CLUT the emitter substitutes when the id's mode field is `2`: palette
/// `0x0F` of the row-500 strip, i.e. VRAM `(240, 500)`.
pub const DANCE_MODE2_CLUT: u16 = 0x7D0F;

/// The MIPS `mult` / `sra` scale idiom: signed multiply, then shift with the
/// `bgez` bias so the round is toward zero.
pub(super) fn mips_scale(value: i32, factor: i32, shift: u32) -> i32 {
    let p = value.wrapping_mul(factor);
    let p = if p < 0 { p + ((1 << shift) - 1) } else { p };
    p >> shift
}

// Wired: [`DanceGame::hud_quads`] / [`DanceGame::number_quads`] /
// [`DanceGame::gauge_readout_quads`] all emit through this, and the play
// window's dance block builds the full quad list per frame
// ([`DanceGame::hud_draw_quads`]). The dance overlay's 4bpp page at `(512, 0)`
// is still not uploaded (its art is staged by the entry path, PROT 1230), so
// the host's quad sink materialises the rects only against a solid atlas
// source - the geometry, gouraud colours and patched `uv` are live every
// frame regardless.
//
// A second, narrower gap is already closed here: the record's `+0x13` ABR byte
// is not decoded by `legaia_asset::dance_art::parse_widgets`, so
// [`dance_widgets_with_abr`] lifts it and the caller passes it in.
/// PORT: FUN_801d2f38 - the dance overlay's textured-quad emitter, the
/// sibling of Baka Fighter's `FUN_801d5ed0`.
///
/// `FUN_801d2f38(x, y, id, brightness, size)` draws one record of the
/// 34-record widget table `DAT_801D46CC`
/// ([`legaia_asset::dance_art::parse_widgets`]) as a quad **centred** on
/// `(x, y)`:
///
/// - the id is two fields. `id & 0x3FF` is the widget index; `id >> 10`
///   (rounded toward zero) is a **blend mode** that overrides the record:
///   mode `0` takes the record's own semi-transparency bit and ABR rate, and
///   any other mode forces semi-transparency on and uses the mode value
///   *itself* as the ABR rate. Mode `2` additionally replaces the CLUT with
///   the fixed [`DANCE_MODE2_CLUT`];
/// - half-extent per axis = `((cell * scale) >> 13) * size >> 12`, both shifts
///   rounding toward zero, so `scale = size = 0x1000` is exactly `cell / 2`;
/// - every colour channel is `channel * brightness >> 8`; verts 0/1 carry
///   `rgb_top`, verts 2/3 `rgb_bottom` - a vertical gradient;
/// - the texpage attribute folds the ABR rate in as `tpage + abr * 0x20`.
///
/// Retail then links the packet into the OT bucket `DAT_801D5154` and forces
/// that slot to `3`, so every draw after the first shares one bucket. That
/// scheduling is host-side; the port returns the quad.
///
/// `abr` is the record's `+0x13` byte. [`legaia_asset::dance_art::DanceWidget`]
/// does not decode it yet, so the caller supplies it.
pub fn dance_hud_widget_quad(
    widget: &legaia_asset::dance_art::DanceWidget,
    abr: u8,
    x: i16,
    y: i16,
    id: u32,
    brightness: i32,
    size: i32,
) -> DanceHudQuad {
    let signed = id as i32;
    let mode = (if signed < 0 { signed + 0x3FF } else { signed }) >> DANCE_WIDGET_MODE_SHIFT;
    let (semi, abr) = if mode == 0 {
        (widget.semi, abr)
    } else {
        (1, mode as u8)
    };
    let scale8 = |c: u8| mips_scale(c as i32, brightness, 8).clamp(0, 0xFF) as u8;
    let half = |cell: u8| mips_scale(size, mips_scale(cell as i32, widget.scale, 13), 12) as i16;
    let hw = half(widget.w);
    let hh = half(widget.h);
    let (u0, v0) = (widget.u, widget.v);
    let (u1, v1) = (
        widget.u.wrapping_add(widget.w),
        widget.v.wrapping_add(widget.h),
    );
    DanceHudQuad {
        poly_code: (semi << 1) | 0x3C,
        x0: x - hw,
        y0: y - hh,
        x1: x + hw,
        y1: y + hh,
        uv: [(u0, v0), (u1, v0), (u0, v1), (u1, v1)],
        rgb_top: widget.rgb_top.map(scale8),
        rgb_bottom: widget.rgb_bottom.map(scale8),
        clut: if mode == 2 {
            DANCE_MODE2_CLUT
        } else {
            widget.clut
        },
        tpage_attr: widget.tpage + abr as u16 * 0x20,
    }
}

/// Which score slot each of the three on-screen score boxes shows, for a mode.
///
/// Wired: [`dance_hud_draws`] permutes its three score readouts through this,
/// and the play window's dance block draws all three boxes (the rivals'
/// scores included) from that list.
/// PORT: FUN_801d231c (`0x801D2320`..`0x801D23AC`) - the HUD driver's slot
/// permutation. The screen's three boxes are fixed; which dancer's score each
/// carries is chosen per mode so the **human** dancer always lands in the
/// centre one. Returns `(centre, left, right)` as indices into
/// `DAT_801D53CC`.
///
/// Retail's default arm (any mode outside `0..=3`) reads its third index out
/// of a register it never writes - an uninitialised read, visible in the
/// disassembly and rendered `unaff_s1` by Ghidra. The mode global is always
/// `0..=3`, so the arm is unreachable; the port returns `None` rather than
/// inventing a value for it.
pub fn dance_score_box_slots(mode: u32) -> Option<(usize, usize, usize)> {
    match mode {
        0 => Some((0, 1, 2)),
        1 => Some((1, 2, 0)),
        2 | 3 => Some((0, 2, 1)),
        _ => None,
    }
}

/// Screen x of the three score boxes' widget-8 frames (`FUN_801d231c`).
pub const DANCE_SCORE_BOX_X: [i16; 3] = [0xA0, 0x40, 0x100];
/// Digit-run origin x paired with each of [`DANCE_SCORE_BOX_X`].
pub const DANCE_SCORE_DIGIT_X: [i16; 3] = [0x40, -0x20, 0xA0];
/// Scanline the whole score strip sits on.
pub const DANCE_SCORE_Y: i16 = 0x14;
/// Widget id of a score-box frame.
pub const DANCE_SCORE_BOX_WIDGET: u32 = 8;
/// Brightness every element of the score strip draws at.
pub const DANCE_HUD_BRIGHTNESS: i32 = 0x80;
/// Groove-gauge readout position for the human dancer, `(x, y)`.
pub const DANCE_GAUGE_XY: (i16, i16) = (0x58, 0xC0);
/// Beat-track anchor for the human dancer, `(x, y)`.
pub const DANCE_TRACK_XY: (i16, i16) = (0x78, 0xC0);
/// Rival gauge / track positions, `(gauge_xy, track_xy)` per rival.
pub const DANCE_RIVAL_XY: [((i16, i16), (i16, i16)); 2] =
    [((0xDC, 0x40), (0xDC, 0xD4)), ((0x50, 0x40), (0x18, 0xD4))];

/// One element of the dance HUD's per-frame draw list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DanceHudDraw {
    /// A dancer's score, drawn as a run of digits from `x` (the emitter's
    /// leading-zero suppression is [`dance_number_digits`]).
    Score {
        slot: usize,
        x: i16,
        y: i16,
        value: u32,
    },
    /// A score-box frame (widget [`DANCE_SCORE_BOX_WIDGET`]).
    ScoreBox { x: i16, y: i16 },
    /// A groove-gauge `Lv.` readout for `slot`.
    Gauge {
        slot: usize,
        x: i16,
        y: i16,
        value: u32,
    },
    /// A beat track for `slot`.
    BeatTrack { slot: usize, x: i16, y: i16 },
}

/// One laid-out row of the dance HUD frame: a string, its 320x240 stage seat,
/// and which of the caller's two pens it takes.
///
/// [`DanceHudDraw`] describes *what* retail's HUD driver emits; this is the
/// resolved presentation of it - digits already suppressed, the `Lv.` label
/// already formed, the rival track already sampled against the chart. Every
/// one of those decisions lived inside the native window's dance block, so
/// the browser play page - running the same `DanceGame` - had no way to draw
/// the frame and showed a plain status line instead.
///
/// A row carries a `String` rather than draws because `legaia-engine-ui` does
/// not depend on this crate: the host lays the string out with its own font
/// and pens, which is three lines, and nothing about *which* string is the
/// host's to decide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DanceHudRow {
    /// The text to draw.
    pub text: String,
    /// Stage-space pen, in retail's 320x240 coordinates.
    pub x: i32,
    pub y: i32,
    /// `true` for the dim pen (box brackets, gauges, rival tracks), `false`
    /// for the bright one (the score readouts).
    pub dim: bool,
}

impl DanceGame {
    /// Whether the rival half of the HUD frame draws: retail's gate
    /// `_DAT_8007B6D0` (`lw v0,-0x4930(v0)` / `beq v0,zero` at
    /// `0x801D24B0..0x801D24B8` in `FUN_801d231c`).
    ///
    /// That word is the **dev counter**, not a versus-mode flag: disc-wide its
    /// only writers are the boot clear (`sw zero,0x3b8(gp)` at `0x80015F64`),
    /// the world-map dev menu's pad ring (`0x801EA00C` / `0x801EA030`, cleared
    /// at `0x801EABF8`, field overlay 0897) and the debug menu's store
    /// (`0x801CED54`, PROT 0971) - no dance-hall script and no mode sets it.
    /// The dance tick reads the same word as a dev switch twice more: raised,
    /// it freezes the camera keyframe track (`0x801CF4F8`) and skips the
    /// song-end test (`0x801D00B0`), so a versus run with it up would never
    /// end. In retail play it is zero in every mode, and the rival gauges and
    /// beat tracks never draw - only the three score boxes do.
    pub fn rival_hud_visible(&self) -> bool {
        false
    }

    /// The HUD frame as laid-out rows - the presentation half of
    /// [`Self::hud_draws`] (`FUN_801d231c`).
    ///
    /// The human dancer's own beat track (`slot == 0`) is skipped: both hosts
    /// already carry the full player track in their own status block, at a
    /// pen of their choosing rather than at retail's anchor.
    pub fn hud_frame_rows(&self, rival_hud: bool) -> Vec<DanceHudRow> {
        let beat = self.beat_index();
        let mut out = Vec::new();
        for d in self.hud_draws(rival_hud) {
            match d {
                DanceHudDraw::Score { x, y, value, .. } => {
                    let text: String = dance_number_digits(value)
                        .iter()
                        .filter_map(|d| d.map(|v| char::from(b'0' + v)))
                        .collect();
                    out.push(DanceHudRow {
                        text,
                        x: x as i32,
                        y: y as i32,
                        dim: false,
                    });
                }
                DanceHudDraw::ScoreBox { x, y } => {
                    // The frame itself is the quad layer's; a bracket marks
                    // its slot in the text layer.
                    out.push(DanceHudRow {
                        text: "[".to_string(),
                        x: x as i32 - 12,
                        y: y as i32,
                        dim: true,
                    });
                }
                DanceHudDraw::Gauge { x, y, value, .. } => {
                    out.push(DanceHudRow {
                        text: format!("Lv.{}", value / GAUGE_STEP),
                        x: x as i32,
                        y: y as i32,
                        dim: true,
                    });
                }
                DanceHudDraw::BeatTrack { slot, x, y } => {
                    if slot == 0 {
                        continue;
                    }
                    if let Some(row) = self.chart_row(self.dancer_lane(slot)) {
                        let text: String = (0..8u32)
                            .map(|i| match row[((beat + i) % row.len() as u32) as usize] {
                                1 => '<',
                                2 => '>',
                                3 => '^',
                                _ => '.',
                            })
                            .collect();
                        out.push(DanceHudRow {
                            text,
                            x: x as i32,
                            y: y as i32,
                            dim: true,
                        });
                    }
                }
            }
        }
        out
    }
}

// Wired: the free-function half of [`DanceGame::hud_draws`], reached through
// it from the play window's dance block every frame (the `rival_hud` gate is
// [`DanceGame::rival_hud_visible`]).
/// PORT: FUN_801d231c - the dance HUD render driver.
///
/// Per frame it draws the three score readouts and their box frames, then the
/// human dancer's groove gauge and beat track, then - **only while the rival
/// HUD flag `_DAT_8007B6D0` is set** - the two rivals' gauges and tracks. Mode
/// `3` (free play) is the single-dancer mode: it draws just the centre box and
/// its digits, skipping both side boxes.
///
/// `scores` and `gauges` are `DAT_801D53CC` / `DAT_801D544C`; `mode` is
/// `DAT_801D514C`.
pub fn dance_hud_draws(
    mode: u32,
    scores: [u32; 3],
    gauges: [u32; 3],
    rival_hud: bool,
) -> Vec<DanceHudDraw> {
    let mut out = Vec::with_capacity(12);
    let Some((centre, left, right)) = dance_score_box_slots(mode) else {
        return out;
    };
    let solo = mode == 3;
    let boxes: &[(usize, usize)] = if solo {
        &[(centre, 0)]
    } else {
        &[(centre, 0), (left, 1), (right, 2)]
    };
    for &(slot, pos) in boxes {
        out.push(DanceHudDraw::Score {
            slot,
            x: DANCE_SCORE_DIGIT_X[pos],
            y: DANCE_SCORE_Y,
            value: scores[slot],
        });
    }
    for &(_, pos) in boxes {
        out.push(DanceHudDraw::ScoreBox {
            x: DANCE_SCORE_BOX_X[pos],
            y: DANCE_SCORE_Y,
        });
    }
    out.push(DanceHudDraw::Gauge {
        slot: 0,
        x: DANCE_GAUGE_XY.0,
        y: DANCE_GAUGE_XY.1,
        value: gauges[0],
    });
    out.push(DanceHudDraw::BeatTrack {
        slot: 0,
        x: DANCE_TRACK_XY.0,
        y: DANCE_TRACK_XY.1,
    });
    if rival_hud {
        for (i, &(gauge_xy, track_xy)) in DANCE_RIVAL_XY.iter().enumerate() {
            out.push(DanceHudDraw::Gauge {
                slot: i + 1,
                x: gauge_xy.0,
                y: gauge_xy.1,
                value: gauges[i + 1],
            });
            out.push(DanceHudDraw::BeatTrack {
                slot: i + 1,
                x: track_xy.0,
                y: track_xy.1,
            });
        }
    }
    out
}
