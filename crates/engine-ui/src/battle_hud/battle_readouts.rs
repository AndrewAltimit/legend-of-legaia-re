//! The combo cluster and the floating value readouts.

use super::*;

pub fn apply_alpha(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [
        color[0],
        color[1],
        color[2],
        color[3] * alpha.clamp(0.0, 1.0),
    ]
}

/// One laid-out digit of a floating battle value readout.
///
/// Hosts fill these from `engine-battle-vm::battle_value_readout::value_cells`, which
/// is where the geometry is pinned. The type is a mirror of that kernel's
/// output shape, not an import - a deliberate seam, not a crate-graph
/// constraint (`engine-ui` depends on `legaia-engine-vm` and could import
/// it). Fields are **stage pixels** on the retail 320x240 stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueCellView {
    /// The decimal digit the cell shows.
    pub digit: u8,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Colour the retail numeral art reads as - a hot gold, the top of the sheet's
/// own vertical ramp. Only the **fallback** uses it: retail's quads carry the
/// neutral colour word `0x808080` and take their colour from the texels, so a
/// host drawing the real cells must not tint them.
pub const VALUE_READOUT_FALLBACK_COLOR: [f32; 4] = [1.0, 0.78, 0.24, 1.0];

/// One label of the combo counter cluster, as the fallback builder draws
/// it: the word and its stage seat (`engine-battle-vm::battle_value_readout::
/// ComboLabel`, minus the texel rect a VRAM-sampling host uses instead).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComboLabelView<'a> {
    pub word: &'a str,
    pub x: i32,
    pub y: i32,
}

/// Fallback draw list for the combo counter cluster - `N HIT` / `TOTAL x`
/// or `DAMAGE x` - for a host without the battle effect atlas resident.
///
/// The **layout is retail's** (`engine-battle-vm::battle_value_readout::
/// combo_cluster`: the label seats, the right-aligned 16-px value cells,
/// the 24-px hit count, and the anchor's 16-frame glide in from the right)
/// and only the letterforms differ: the sheet's `HIT` / `TOTAL` / `DAMAGE`
/// word cells become dialog-font text on the same seats, in the numeral
/// art's own gold, and the digits go through
/// [`battle_value_readout_draws_for`]. A host that can sample VRAM draws the
/// real cells off texture page `0x27` / CLUT `0x7703` and skips this.
pub fn battle_combo_cluster_draws_for(
    font: &legaia_font::Font,
    labels: &[ComboLabelView<'_>],
    cells: &[ValueCellView],
    origin: (i32, i32),
    scale: u32,
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for l in labels {
        if l.word.is_empty() {
            continue;
        }
        let layout = font.layout_ascii(l.word);
        // The word cells are 16 rows tall; the font's 12-row glyphs sit two
        // rows down inside them.
        let mut draws = text_draws_for(&layout, (l.x, l.y + 2), VALUE_READOUT_FALLBACK_COLOR);
        scale_stage_text_draws(&mut draws, origin, scale);
        out.extend(draws);
    }
    out.extend(battle_value_readout_draws_for(
        font,
        cells,
        VALUE_READOUT_FALLBACK_COLOR,
        origin,
        scale,
    ));
    out
}

/// Fallback draw list for a floating value readout: the dialog font's digits
/// scaled into the pinned cells.
///
/// This is what a host without the battle effect atlas resident draws. The
/// **layout is retail's** - same cells, same pitch, same pop and rise - and
/// only the letterforms differ, the same bargain the HUD numerals already
/// strike. A host that can sample VRAM should draw the real 24x24 cells off
/// texture page `0x27` / CLUT `0x7703` instead and skip this entirely.
///
/// `origin` / `scale` are the stage-to-surface transform the caller already
/// uses for the rest of the battle chrome.
pub fn battle_value_readout_draws_for(
    font: &legaia_font::Font,
    cells: &[ValueCellView],
    color: [f32; 4],
    origin: (i32, i32),
    scale: u32,
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for c in cells {
        if c.w == 0 || c.h == 0 {
            continue;
        }
        let glyph = [b'0' + (c.digit % 10)];
        let s = match std::str::from_utf8(&glyph) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let layout = font.layout_ascii(s);
        for d in text_draws_for(&layout, (0, 0), color) {
            let (dx, dy, dw, dh) = d.dst;
            // Scale the glyph's own quad into the cell: the font atlas has no
            // 24-px digits, so each source rect is stretched rather than
            // blitted. The scale is taken from the glyph RECT (`dw` / `dh`),
            // not from the layout advance. The advance is the *proportional*
            // pen step - 5 px for `1`, 7 for `3` - while the atlas cell the
            // quad samples is a fixed 14x15, so dividing by it inflated every
            // digit by `rect / advance` (~2x-3x) and, because the advance is
            // per glyph, drew the digits of one number at DIFFERENT sizes.
            // Retail's readout is a run of uniform 24x24 cells, so the quad
            // is the cell.
            let sx = c.w as f32 / (dw.max(1) as f32);
            let sy = c.h as f32 / (dh.max(1) as f32);
            out.push(TextDraw {
                dst: (
                    origin.0 + (c.x * scale as i32) + (dx as f32 * sx) as i32 * scale as i32,
                    origin.1 + (c.y * scale as i32) + (dy as f32 * sy) as i32 * scale as i32,
                    ((dw as f32 * sx) as u32).max(1) * scale,
                    ((dh as f32 * sy) as u32).max(1) * scale,
                ),
                src: d.src,
                color: d.color,
            });
        }
    }
    out
}
