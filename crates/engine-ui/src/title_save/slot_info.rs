//! The save screen's bottom info panel: the focused block's location, play
//! time and one stats column per party member, or a one-line caption when
//! the block holds nothing loadable.
//!
//! Everything here is `FUN_801E08D8`, read off its disassembly
//! (`ghidra/scripts/funcs/overlay_save_ui_select_801e08d8.txt`) and checked
//! against retail framebuffers of the parked panel. Offsets are relative to
//! the panel's slide origin `local_34` (138 parked).

use crate::field_party_hud::{SEPARATOR_SPRITE_DX, SEPARATOR_SPRITE_DY};
use crate::*;

/// Retail PSX framebuffer placement of the slot-info panel (bottom
/// of stage), parked / fully-slid-in. Pinned to FUN_801E08D8's
/// `FUN_801e36c4(0xA0, local_34, 0x11c, 0x40)` call through the
/// messagebox model (centre rect inflated by 8 on every side):
/// footprint `(160 - 142 - 10, local_34 - 2, 284 + 16, 64 + 16)` =
/// `(8, 136, 300, 80)` at the parked `local_34 = 138`. GP0-dump-pinned
/// on the live SlotPreview screen (corner tiles at (8, 136) / (304,
/// 212); left/right edges tiled 3 x 24 tall at x = 8 / 304).
pub const SLOT_INFO_PANEL_POS: (i32, i32) = (8, 136);
pub const SLOT_INFO_PANEL_SIZE: (u32, u32) = (300, 80);
/// Panel-y origin when fully slid-in (= `local_34` at anim_t=0x1000).
pub const SLOT_INFO_PANEL_PARKED_Y: i32 = 138;

/// Title row, relative to `local_34`.
///
/// - `SLOT_INFO_NO_OFFSET`: the block badge - record 3 (`No.`) of the
///   sprite-record table at `(8, local_34 - 8)` and record 2 (the numeral,
///   `u = block << 4` through `DAT_801E5062`) at `(0x1E, local_34 - 8)`, both
///   via `FUN_801E3FF0`.
/// - `SLOT_INFO_OLD_FILENAME_OFFSET`: "Old Filename" (`0x801CF330`), drawn
///   only for a file under the Japanese prefix (`strncmp` against
///   `0x801EF054`), one row above the location.
/// - `SLOT_INFO_LOCATION_OFFSET`: the summary's location string.
/// - `SLOT_INFO_TIME_LABEL_OFFSET`: `Time ` (`0x801CF340`).
/// - `SLOT_INFO_TIME_FIELDS_X` / `SLOT_INFO_TIME_COLONS_X`: hours, minutes and
///   seconds are three two-cell zero-padded numeral fields
///   (`FUN_80034E4C(v, 2, x, y)` at `0xEC` / `0x104` / `0x11C`) with a `:`
///   string (`0x801CF348`) at `0xFC` / `0x114` between them.
pub const SLOT_INFO_NO_OFFSET: (i32, i32) = (8, -8);
pub const SLOT_INFO_OLD_FILENAME_OFFSET: (i32, i32) = (48, 0);
pub const SLOT_INFO_LOCATION_OFFSET: (i32, i32) = (48, 4);
pub const SLOT_INFO_TIME_LABEL_OFFSET: (i32, i32) = (208, 4);
pub const SLOT_INFO_TIME_FIELDS_X: [i32; 3] = [0xEC, 0x104, 0x11C];
pub const SLOT_INFO_TIME_COLONS_X: [i32; 2] = [0xFC, 0x114];
/// Back-compat name for the first time field.
pub const SLOT_INFO_TIME_VALUE_OFFSET: (i32, i32) = (0xEC, 4);

/// Centre-x and y-offset of the panel's single-line caption - what fills
/// the panel instead of a preview when the slot holds no loadable save
/// (retail view modes 2/3/4).
///
/// Retail emits it through the centring wrapper
/// `FUN_801E3EE0(text, 0xA0, local_34 + 0x18)`, which measures the string
/// and hands the raw text emitter `x - width/2` at `y + 7`. So the centre
/// is screen x=160 and the drawn y is `panel_y + 24 + 7` (GP0-dump-pinned:
/// the live "No data" caption glyphs sit at y = 169 = 138 + 31). The `+ 7`
/// is folded in here: unlike every other offset on this panel, which retail
/// passes straight to the raw emitter, this one goes through the wrapper.
pub const SLOT_INFO_CAPTION_CENTER_X: i32 = 160;
pub const SLOT_INFO_CAPTION_Y_OFFSET: i32 = 31;

/// "Able to save."'s picture: the block's save-icon face (record 0, tile =
/// block) at `(0x58, local_34 + 0x1C)` inside the empty-cell frame, record
/// one, at `(0x50, local_34 + 0x14)` - the frame quad's top-left, its 20x20
/// visible box 6 px in.
pub const SLOT_INFO_CAPTION_ICON_OFFSET: (i32, i32) = (0x58, 0x1C);
pub const SLOT_INFO_CAPTION_FRAME_OFFSET: (i32, i32) = (0x50, 0x14);

/// Per-member column: `16 + i * 96` (16 / 112 / 208).
pub const SLOT_INFO_COLUMN_X0: i32 = 0x10;
pub const SLOT_INFO_COLUMN_PITCH: i32 = 0x60;

/// Column-0 element seats. Retail's row base is `s3 = local_34 + 0x14`;
/// every x below is the column base plus the listed offset.
///
/// | Element | Call | Seat |
/// |---|---|---|
/// | face | `FUN_801E3FF0(0, .., x, s3 - 4)`, tile = party id | `(+0, s3 - 4)` |
/// | name | `FUN_80036888(name, 0, 0, x + 0x18, s3)` | `(+24, s3)` |
/// | `LV` / level | `FUN_8002C488(x, s3 + 13, 0xA)` / `FUN_80034B78(lv, 2, x + 0x20, ..)` | `(+0 / +32, s3 + 13)` |
/// | `HP` / cur / `/` / max | `0x3F` / `(cur, 4, x + 0x10)` / glyph 6 at `x + 0x31` / `(max, 4, x + 0x3D)` | `s3 + 26` |
/// | `MP` / cur / `/` / max | `0x40` / `(cur, 3, x + 0x18)` / glyph 6 at `x + 0x31` / `(max, 3, x + 0x45)` | `s3 + 39` |
pub const SLOT_INFO_PORTRAIT_OFFSET: (i32, i32) = (16, 16);
pub const SLOT_INFO_NAME_OFFSET: (i32, i32) = (40, 20);
pub const SLOT_INFO_LV_LABEL_OFFSET: (i32, i32) = (16, 33);
pub const SLOT_INFO_LV_VALUE_OFFSET: (i32, i32) = (48, 33);
pub const SLOT_INFO_HP_LABEL_OFFSET: (i32, i32) = (16, 46);
pub const SLOT_INFO_HP_VALUE_OFFSET: (i32, i32) = (32, 46);
pub const SLOT_INFO_HP_SEP_OFFSET: (i32, i32) = (65, 46);
pub const SLOT_INFO_HP_MAX_OFFSET: (i32, i32) = (77, 46);
pub const SLOT_INFO_MP_LABEL_OFFSET: (i32, i32) = (16, 59);
pub const SLOT_INFO_MP_VALUE_OFFSET: (i32, i32) = (40, 59);
pub const SLOT_INFO_MP_SEP_OFFSET: (i32, i32) = (65, 59);
pub const SLOT_INFO_MP_MAX_OFFSET: (i32, i32) = (85, 59);
/// Numeral field widths, in 8-px cells: the level 2, HP 4, MP 3 (the
/// `FUN_80034B78` count argument). The numerals fill a field from the right.
pub const SLOT_INFO_LV_CELLS: i32 = 2;
pub const SLOT_INFO_HP_CELLS: i32 = 4;
pub const SLOT_INFO_MP_CELLS: i32 = 3;

/// One party member's column on the info panel.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlotInfoMember<'a> {
    /// Roster id: the save-icon tile drawn as the face.
    pub id: u8,
    pub name: &'a str,
    pub level: u8,
    /// Current / maximum, as the save summary stores them.
    pub hp: (i16, i16),
    pub mp: (i16, i16),
}

/// Plain-data view of the focused block for the info-panel renderer, read
/// off the block's save summary.
#[derive(Debug, Clone, Copy)]
pub struct SlotInfoView<'a> {
    /// The 1-based save number the badge prints.
    pub slot_no: u8,
    pub location: &'a str,
    /// Retail's location test passed. A block that fails it prints its badge
    /// and nothing else.
    pub location_valid: bool,
    /// The file carries the Japanese prefix ("Old Filename").
    pub old_filename: bool,
    /// Hours, minutes, seconds (already clamped at `99:59:59`).
    pub play_time: (u32, u32, u32),
    pub members: &'a [SlotInfoMember<'a>],
}

/// The readout tier `FUN_801E08D8` picks into `_DAT_8007B454` for a stat
/// pair, the same law for HP and MP (`0x801E0CF0..0x801E0D50`,
/// `0x801E0D78..0x801E0DD8`): `7` (normal) unless `cur <= max / 2`, then
/// `6`; `9` once `cur <= max / 4`. The divisions round toward zero on the
/// signed halfwords. Both numbers of the pair take the tier; the `/` is
/// drawn after the tier is reset to `7`. Unlike the battle strip there is no
/// K.O. tier.
pub fn slot_info_readout_tier(cur: i16, max: i16) -> u8 {
    let (cur, max) = (i32::from(cur), i32::from(max));
    let mut tier = 7;
    if max / 2 >= cur {
        tier = 6;
    }
    if max / 4 >= cur {
        tier = 9;
    }
    tier
}

fn tier_ink(tier: u8) -> [f32; 4] {
    match tier {
        6 | 9 => gauge_fill_color(tier),
        _ => READOUT_NORMAL,
    }
}

/// Collects one panel's sprites and text against the stage transform.
struct PanelPen<'f> {
    font: Option<&'f legaia_font::Font>,
    origin: (i32, i32),
    scale: i32,
    sprites: Vec<SpriteDraw>,
    texts: Vec<TextDraw>,
}

impl PanelPen<'_> {
    fn sprite(&mut self, src: (u32, u32, u32, u32), x: i32, y: i32, color: [f32; 4]) {
        self.sprites.push(SpriteDraw {
            dst: (
                self.origin.0 + x * self.scale,
                self.origin.1 + y * self.scale,
                src.2 * self.scale as u32,
                src.3 * self.scale as u32,
            ),
            src,
            color,
        });
    }

    fn text(&mut self, s: &str, x: i32, y: i32, color: [f32; 4]) {
        let Some(font) = self.font else { return };
        let layout = font.layout_ascii(s);
        for g in &layout.glyphs {
            self.texts.push(TextDraw {
                dst: (
                    self.origin.0 + (x + g.dst_x) * self.scale,
                    self.origin.1 + (y + g.dst_y) * self.scale,
                    g.width * self.scale as u32,
                    g.height * self.scale as u32,
                ),
                src: (g.atlas_x, g.atlas_y, g.width, g.height),
                color,
            });
        }
    }

    /// A numeral field of `cells` 8-px cells at `x`: the `FUN_80034B78`
    /// right-aligned fill when `zero_pad` is false, `FUN_80034E4C`'s
    /// zero-padded one when true. A negative value prints `0`, as the
    /// subtract loop never runs on one.
    #[allow(clippy::too_many_arguments)]
    fn numerals(
        &mut self,
        digits: Option<crate::BattleChromeRects>,
        value: i32,
        cells: i32,
        zero_pad: bool,
        x: i32,
        y: i32,
        ink: [f32; 4],
    ) {
        let v = value.max(0);
        let s = if zero_pad {
            format!("{v:0width$}", width = cells as usize)
        } else {
            v.to_string()
        };
        let left = x + (cells - s.len() as i32).max(0) * HUD_DIGIT_W;
        for (i, ch) in s.bytes().enumerate() {
            let cx = left + i as i32 * HUD_DIGIT_W;
            match digits.and_then(|b| crate::hud_numeral_cell(&b, u32::from(ch - b'0'), ink)) {
                Some((src, tint)) => self.sprite(src, cx, y, tint),
                None => {
                    let g = [ch];
                    let g = std::str::from_utf8(&g).unwrap_or("0");
                    let adv = self
                        .font
                        .map(|f| f.layout_ascii(g).advance_x as i32)
                        .unwrap_or(HUD_DIGIT_W);
                    self.text(g, cx + (HUD_DIGIT_W - adv) / 2, y, ink);
                }
            }
        }
    }

    fn separator(&mut self, rects: Option<&SaveMenuAtlasRects>, x: i32, y: i32) {
        match rects.and_then(|r| r.battle) {
            Some(b) => self.sprite(
                b.separator,
                x + SEPARATOR_SPRITE_DX,
                y + SEPARATOR_SPRITE_DY,
                numeral_cell_tint(READOUT_NORMAL),
            ),
            None => self.text("/", x, y, READOUT_NORMAL),
        }
    }
}

/// Everything the mode-1 panel draws over its chrome, as one list per
/// pipeline. `font = None` builds only the sprite half; `rects = None` only
/// the text half (every sprite element falls back to a glyph stand-in).
fn slot_info_body(
    font: Option<&legaia_font::Font>,
    rects: Option<&SaveMenuAtlasRects>,
    info: &SlotInfoView<'_>,
    panel_y: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> (Vec<SpriteDraw>, Vec<TextDraw>) {
    let mut pen = PanelPen {
        font,
        origin: stage_origin,
        scale: stage_scale.max(1) as i32,
        sprites: Vec::new(),
        texts: Vec::new(),
    };
    let white = [1.0, 1.0, 1.0, 1.0];
    let text = SAVE_SELECT_TITLE_COLOR;
    let digits = rects.and_then(|r| r.battle);

    if info.old_filename {
        pen.text(
            "Old Filename",
            SLOT_INFO_OLD_FILENAME_OFFSET.0,
            panel_y + SLOT_INFO_OLD_FILENAME_OFFSET.1,
            text,
        );
    }
    if info.location_valid {
        let y = panel_y + SLOT_INFO_LOCATION_OFFSET.1;
        pen.text(info.location, SLOT_INFO_LOCATION_OFFSET.0, y, text);
        pen.text("Time", SLOT_INFO_TIME_LABEL_OFFSET.0, y, text);
        let (h, m, s) = info.play_time;
        for (v, x) in [h, m, s].into_iter().zip(SLOT_INFO_TIME_FIELDS_X) {
            pen.numerals(digits, v as i32, 2, true, x, y, READOUT_NORMAL);
        }
        for x in SLOT_INFO_TIME_COLONS_X {
            pen.text(":", x, y, text);
        }
    }

    // The block badge rides every mode-1 panel, the invalid one too.
    if rects.is_some() {
        pen.sprites.extend(block_badge_draws_for(
            info.slot_no.saturating_sub(1),
            (SLOT_INFO_NO_OFFSET.0, panel_y + SLOT_INFO_NO_OFFSET.1),
            stage_origin,
            stage_scale,
        ));
    } else {
        pen.text(
            &format!("No.{}", info.slot_no),
            SLOT_INFO_NO_OFFSET.0,
            panel_y + SLOT_INFO_NO_OFFSET.1,
            text,
        );
    }
    if !info.location_valid {
        return (pen.sprites, pen.texts);
    }

    for (i, m) in info.members.iter().enumerate() {
        let dx = i as i32 * SLOT_INFO_COLUMN_PITCH;
        let at = |o: (i32, i32)| (o.0 + dx, panel_y + o.1);

        let (x, y) = at(SLOT_INFO_PORTRAIT_OFFSET);
        if let Some(face) =
            rects.and_then(|r| r.save_icons.get(usize::from(m.id)).copied().flatten())
        {
            pen.sprite(face, x, y, white);
        }
        let (x, y) = at(SLOT_INFO_NAME_OFFSET);
        pen.text(m.name, x, y, text);

        let label = |pen: &mut PanelPen<'_>, which: u8, o: (i32, i32)| {
            let (x, y) = at(o);
            match rects {
                Some(r) => {
                    let src = match which {
                        0 => r.label_lv,
                        1 => r.label_hp,
                        _ => r.label_mp,
                    };
                    pen.sprite(src, x, y, white);
                }
                None => pen.text(["LV", "HP", "MP"][usize::from(which)], x, y, text),
            }
        };
        label(&mut pen, 0, SLOT_INFO_LV_LABEL_OFFSET);
        label(&mut pen, 1, SLOT_INFO_HP_LABEL_OFFSET);
        label(&mut pen, 2, SLOT_INFO_MP_LABEL_OFFSET);

        let (x, y) = at(SLOT_INFO_LV_VALUE_OFFSET);
        pen.numerals(
            digits,
            i32::from(m.level),
            SLOT_INFO_LV_CELLS,
            false,
            x,
            y,
            READOUT_NORMAL,
        );

        let hp_ink = tier_ink(slot_info_readout_tier(m.hp.0, m.hp.1));
        let (x, y) = at(SLOT_INFO_HP_VALUE_OFFSET);
        pen.numerals(
            digits,
            i32::from(m.hp.0),
            SLOT_INFO_HP_CELLS,
            false,
            x,
            y,
            hp_ink,
        );
        let (x, y) = at(SLOT_INFO_HP_MAX_OFFSET);
        pen.numerals(
            digits,
            i32::from(m.hp.1),
            SLOT_INFO_HP_CELLS,
            false,
            x,
            y,
            hp_ink,
        );

        let mp_ink = tier_ink(slot_info_readout_tier(m.mp.0, m.mp.1));
        let (x, y) = at(SLOT_INFO_MP_VALUE_OFFSET);
        pen.numerals(
            digits,
            i32::from(m.mp.0),
            SLOT_INFO_MP_CELLS,
            false,
            x,
            y,
            mp_ink,
        );
        let (x, y) = at(SLOT_INFO_MP_MAX_OFFSET);
        pen.numerals(
            digits,
            i32::from(m.mp.1),
            SLOT_INFO_MP_CELLS,
            false,
            x,
            y,
            mp_ink,
        );

        let (x, y) = at(SLOT_INFO_HP_SEP_OFFSET);
        pen.separator(rects, x, y);
        let (x, y) = at(SLOT_INFO_MP_SEP_OFFSET);
        pen.separator(rects, x, y);
    }
    (pen.sprites, pen.texts)
}

/// Build the sprite half of the slot-info panel: the 9-slice messagebox, and
/// over it (when `info` is a readable block) the badge, the faces, the
/// `LV` / `HP` / `MP` label sprites and every numeral and `/` as the 8x12
/// HUD cells retail draws them with.
///
/// `panel_y_offset` is the slide-in delta from the parked y
/// (positive = pushed below parked, used while the panel slides up).
/// Engine callers compute it from `interpolate_anim((0, OFFSCREEN),
/// (0, PARKED), session.info_panel_slide_anim_t()).1 - PARKED`.
///
/// PORT: FUN_801E08D8 (mode 1: `0x801E09CC..0x801E0E5C`)
pub fn slot_info_panel_draws_for(
    rects: &SaveMenuAtlasRects,
    info: Option<&SlotInfoView<'_>>,
    panel_y_offset: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut out = Vec::with_capacity(64);
    let (px, py_base) = SLOT_INFO_PANEL_POS;
    let py = py_base + panel_y_offset;
    let (pw, ph) = SLOT_INFO_PANEL_SIZE;
    nine_slice_panel_into(
        &mut out,
        rects,
        (px, py, pw as i32, ph as i32),
        stage_origin,
        stage_scale,
        false,
    );
    if let Some(info) = info {
        let (sprites, _) = slot_info_body(
            None,
            Some(rects),
            info,
            SLOT_INFO_PANEL_PARKED_Y + panel_y_offset,
            stage_origin,
            stage_scale,
        );
        out.extend(sprites);
    }
    out
}

/// Build the text half of the slot-info panel (location, `Time`, the time
/// colons, names, "Old Filename"). Returns empty when `info` is `None`.
/// `panel_y_offset` matches the value passed to
/// [`slot_info_panel_draws_for`].
///
/// `rects` is the atlas the sprite half draws from: with it, every numeral,
/// label and the badge are that half's sprites and this emits text only;
/// without it, this emits glyph stand-ins for them at the same seats.
pub fn slot_info_panel_text_draws_for(
    font: &legaia_font::Font,
    info: Option<&SlotInfoView<'_>>,
    panel_y_offset: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
    rects: Option<&SaveMenuAtlasRects>,
) -> Vec<TextDraw> {
    let Some(info) = info else { return Vec::new() };
    let (_, texts) = slot_info_body(
        Some(font),
        rects,
        info,
        SLOT_INFO_PANEL_PARKED_Y + panel_y_offset,
        stage_origin,
        stage_scale,
    );
    texts
}

/// Build the [`TextDraw`]s for the info panel's centred caption - the body
/// of the panel when the focused slot holds no loadable save.
///
/// The counterpart to [`slot_info_panel_text_draws_for`]: exactly one of the
/// two fills the panel, mirroring retail, where a single `view_mode` picks
/// between the stats preview and one centred line. Callers get the string
/// from `SlotInfoMode::caption`; pass the same `panel_y_offset` as the
/// chrome.
///
/// PORT: FUN_801E08D8's `FUN_801E3EE0(caption, 0xA0, local_34 + 0x18)` tail.
pub fn slot_info_caption_draws_for(
    font: &legaia_font::Font,
    caption: &str,
    panel_y_offset: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<TextDraw> {
    if caption.is_empty() {
        return Vec::new();
    }
    let scale = stage_scale.max(1);
    let layout = font.layout_ascii(caption);
    // Retail measures the string and draws from `x - width/2`, truncating
    // toward zero exactly as this does.
    let base_x = SLOT_INFO_CAPTION_CENTER_X - layout.advance_x as i32 / 2;
    let base_y = SLOT_INFO_PANEL_PARKED_Y + panel_y_offset + SLOT_INFO_CAPTION_Y_OFFSET;

    layout
        .glyphs
        .iter()
        .map(|g| TextDraw {
            dst: (
                stage_origin.0 + (base_x + g.dst_x) * scale as i32,
                stage_origin.1 + (base_y + g.dst_y) * scale as i32,
                g.width * scale,
                g.height * scale,
            ),
            src: (g.atlas_x, g.atlas_y, g.width, g.height),
            color: SAVE_SELECT_TITLE_COLOR,
        })
        .collect()
}

/// The sprites "Able to save." carries (view mode 3 on a Save): the
/// block's own save-icon face in an empty-cell frame left of the caption,
/// and the block badge. `block` is the 0-based save number. "No data" (the
/// same mode on a Load) and "Not a Legend of Legaia save." carry none.
///
/// PORT: FUN_801E08D8 (`0x801E0EB8..0x801E0F54`)
pub fn slot_info_caption_sprites_for(
    rects: &SaveMenuAtlasRects,
    block: u8,
    panel_y_offset: i32,
    stage_origin: (i32, i32),
    stage_scale: u32,
) -> Vec<SpriteDraw> {
    let mut pen = PanelPen {
        font: None,
        origin: stage_origin,
        scale: stage_scale.max(1) as i32,
        sprites: Vec::new(),
        texts: Vec::new(),
    };
    let py = SLOT_INFO_PANEL_PARKED_Y + panel_y_offset;
    let white = [1.0, 1.0, 1.0, 1.0];
    // The frame first and the face over it: the two records share one
    // ordering-table bucket, where the later link (the frame) draws first -
    // the grid cell's order.
    if let Some(frame) = rects.load_empty_frame {
        pen.sprite(
            frame,
            SLOT_INFO_CAPTION_FRAME_OFFSET.0,
            py + SLOT_INFO_CAPTION_FRAME_OFFSET.1,
            white,
        );
    }
    if let Some(face) = rects.save_icons.get(usize::from(block)).copied().flatten() {
        pen.sprite(
            face,
            SLOT_INFO_CAPTION_ICON_OFFSET.0,
            py + SLOT_INFO_CAPTION_ICON_OFFSET.1,
            white,
        );
    }
    pen.sprites.extend(block_badge_draws_for(
        block,
        (SLOT_INFO_NO_OFFSET.0, py + SLOT_INFO_NO_OFFSET.1),
        stage_origin,
        stage_scale,
    ));
    pen.sprites
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readout_tiers_follow_retails_halving() {
        // Captured on the save screen: 836/1673 gold, 390/1560 orange,
        // 837/1673 normal, 0/339 orange, 144/288 gold, 62/248 orange.
        assert_eq!(slot_info_readout_tier(836, 1673), 6);
        assert_eq!(slot_info_readout_tier(837, 1673), 7);
        assert_eq!(slot_info_readout_tier(390, 1560), 9);
        assert_eq!(slot_info_readout_tier(0, 339), 9);
        assert_eq!(slot_info_readout_tier(144, 288), 6);
        assert_eq!(slot_info_readout_tier(62, 248), 9);
        assert_eq!(slot_info_readout_tier(426, 426), 7);
    }
}
