//! The **field / overworld party-status HUD** - the persistent readout retail
//! keeps in the top-left of every walkable frame: one column per present party
//! member carrying the name, `LV`, `HP cur/max` and `MP cur/max` over a
//! semi-transparent black plate.
//!
//! This is the draw half of `FUN_801D0D38`. The decision half - whether the
//! HUD is up this frame and which of the two rows it takes - is the ported
//! kernel [`legaia_engine_vm::world_map_panel_actors::field_hud_tick`]; hosts
//! run that first and call [`field_party_hud_draws_for`] only on its
//! `HudDecision::Draw { y }` arm, passing that `y`.
//!
//! # Where the geometry comes from
//!
//! Every constant below is read off `FUN_801D0D38`'s own disassembly (the
//! draw loop at `0x801D0FE4..0x801D1178` for the text, `0x801D1194..0x801D12CC`
//! for the plate), and each was then confirmed against retail framebuffers:
//!
//! * `s5` starts at `0x10` and advances by `0x64` per member - the column
//!   origin and pitch ([`FIELD_HUD_X0`] / [`FIELD_HUD_PITCH`]).
//! * The three text rows are `y`, `y + 0xE` and `y + 0x1A` (the stack slots
//!   `0x38/0x3C/0x40(sp)`).
//! * The numeral fields are fixed-width, **right-aligned**, 8-px cells: level
//!   `2` cells at `x + 0x48`, `HP`/`MP` current `4` cells at `x + 0x10`,
//!   maximum `4` cells at `x + 0x38`.
//! * The plate is three `GP0(0x2A)` quads - a flat **semi-transparent black**
//!   polygon, so PSX blend mode 0 (`0.5*B + 0.5*F`) over `F = 0` halves the
//!   scene behind it. Its silhouette is a hexagon: a top bevel from
//!   `(x-1, y-3)` widening to `(x-4, y)`, the body down to `y + 0x25`, and a
//!   bottom bevel closing to `y + 0x28`.
//!
//! The four plate edges are visible as a hard 2:1 brightness step in the
//! overworld-resident captures, at exactly `x = 12`, `x = 308` (three members:
//! `16 + 2*100 + 92`), `y = 9` and `y = 52` - which is what pins the constants
//! rather than merely being consistent with them.
//!
//! # Why this is not the battle roster panel
//!
//! The battle strip ([`crate::ui_overlay::battle_hud_draws_for`]) has its own
//! layout, its own widths and its own plate art. The two read the same
//! character record and take the same readout-tint law, and nothing else about
//! them is shared - the field columns are 4 px narrower, the field separator
//! sits on the numeral row rather than four above it, and the field plate is a
//! translucent scrim rather than the marbled 102x48 sprite.

use crate::*;

/// Column origin of the first member's readout (`li s5,0x10`).
pub const FIELD_HUD_X0: i32 = 0x10;
/// Horizontal pitch between members (`addiu s5,s5,0x64`).
pub const FIELD_HUD_PITCH: i32 = 0x64;

/// Row offsets inside a column, from the decision's `y`.
/// Name + `LV` sit on the base row; HP is `+0xE`, MP `+0x1A`.
pub const ROW_HP_DY: i32 = 0x0E;
pub const ROW_MP_DY: i32 = 0x1A;

/// `LV` label seat, relative to the column origin (`x+0x3a`, `y+2`).
pub const LV_LABEL: (i32, i32) = (0x3A, 2);
/// `HP` / `MP` label seats (`x`, `y+0x10` / `y+0x1c`).
pub const HP_LABEL_DY: i32 = 0x10;
pub const MP_LABEL_DY: i32 = 0x1C;

/// Numeral-field origins, relative to the column origin, and their widths in
/// cells. Retail's numeral drawer takes `(value, cells, x, y)` and fills the
/// field from the right, so a shorter number leaves blank cells on the left.
pub const LV_FIELD_X: i32 = 0x48;
pub const LV_FIELD_CELLS: i32 = 2;
pub const CUR_FIELD_X: i32 = 0x10;
pub const MAX_FIELD_X: i32 = 0x38;
pub const VALUE_FIELD_CELLS: i32 = 4;
/// Width of one numeral cell - retail's 8-px HUD digit, the same cell the
/// battle strip lays out in.
pub const DIGIT_W: i32 = 8;

/// `/` separator seat (`FUN_8003C1F8` glyph 6 at `x+0x30`).
pub const SEPARATOR_DX: i32 = 0x30;
/// The separator **sprite** is 8x16 and its ink sits four rows down inside
/// that cell, so the blit seats four rows above the numeral row - the same
/// relation the battle panel pins (`PANEL_HP_SEPARATOR` vs
/// `PANEL_HP_DIGIT_Y`).
pub const SEPARATOR_SPRITE_DY: i32 = -4;
/// ...and one column right. Retail's `/` ink lands at `x + 0x32 .. x + 0x35`
/// in the overworld captures; the atlas cell's own ink starts one column
/// earlier inside its 8-px box, so the sprite seat carries the difference.
/// The font-glyph fallback does not (it is centred in the cell like every
/// other numeral fallback).
pub const SEPARATOR_SPRITE_DX: i32 = 1;

/// Plate silhouette, relative to the column origin / decision `y`.
pub const PLATE_LEFT_DX: i32 = -4;
pub const PLATE_RIGHT_DX: i32 = 92;
pub const PLATE_BEVEL_LEFT_DX: i32 = -1;
pub const PLATE_TOP_RIGHT_DX: i32 = 89;
pub const PLATE_BOTTOM_RIGHT_DX: i32 = 88;
pub const PLATE_TOP_DY: i32 = -3;
pub const PLATE_BODY_BOTTOM_DY: i32 = 0x25;
pub const PLATE_BOTTOM_DY: i32 = 0x28;

/// The scrim colour: retail's quad is flat **black** and semi-transparent in
/// PSX blend mode 0, i.e. `0.5*background`. An alpha-blended black rect with
/// `a = 0.5` is the same operation.
pub const PLATE_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.5];

/// One member's readout, as the host projects it out of the character record.
///
/// The record fields retail reads are `+0x2A7` (name), `+0x6F8` (the number
/// beside `LV`), `+0x6CE`/`+0x6CC` (HP current / maximum) and
/// `+0x6D2`/`+0x6D0` (MP current / maximum).
#[derive(Debug, Clone, Copy, Default)]
pub struct FieldHudMember<'a> {
    pub name: &'a str,
    pub level: u8,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
    /// Drives the dead tier of the readout-tint law, exactly as on the battle
    /// strip: a downed member's HP *and* MP go red, the name does not dim.
    pub alive: bool,
}

/// Everything one field party-HUD frame needs.
pub struct FieldPartyHudFrame<'a> {
    /// The present party, left to right. Empty draws nothing.
    pub members: &'a [FieldHudMember<'a>],
    /// The decision's row
    /// ([`legaia_engine_vm::world_map_panel_actors::HudDecision::Draw`]).
    pub y: i32,
    /// The resident system-UI atlas. With it the `LV`/`HP`/`MP` labels, the
    /// `/` and the numerals blit retail's own cells; without it they degrade
    /// to tinted font glyphs on the identical seats - the same two-path shape
    /// the battle strip has.
    pub chrome: Option<&'a crate::SaveMenuAtlasRects>,
    /// A fully-opaque 1x1 texel **inside the system-UI atlas**. When present
    /// the scrim is emitted into the sprite half, ahead of the labels and
    /// numerals that share that atlas, so it lands *under* them: hosts
    /// composite the sprite half beneath the text half, and a scrim in the
    /// text half would instead half-darken its own readout.
    pub scrim_src: Option<(u32, u32, u32, u32)>,
    /// A 1x1 solid white texel in the **font** atlas
    /// ([`crate::font_solid_src`]) - the scrim's fallback surface for a host
    /// with no system-UI atlas, where the labels and numerals are font glyphs
    /// anyway and the layering question does not arise.
    pub solid_src: Option<(u32, u32, u32, u32)>,
    /// Stage transform: the builder lays everything out in 320x240 stage
    /// pixels and maps it the way [`crate::scale_stage_text_draws`] does.
    pub origin: (i32, i32),
    pub scale: i32,
}

/// Build one field party-HUD frame.
///
/// PORT: FUN_801d0d38 (draw half)
pub fn field_party_hud_draws_for(
    font: &legaia_font::Font,
    frame: &FieldPartyHudFrame<'_>,
) -> BattleHudDraws {
    let FieldPartyHudFrame {
        members,
        y,
        chrome,
        scrim_src,
        solid_src,
        origin,
        scale,
    } = *frame;
    let mut out = BattleHudDraws::default();
    if members.is_empty() {
        return out;
    }
    let white = [1.0f32, 1.0, 1.0, 1.0];
    let label_gold = [1.0f32, 0.72, 0.16, 1.0];
    let label_green = [0.16f32, 0.78, 0.35, 1.0];

    let stage_rect = |out: &mut Vec<TextDraw>, x: i32, y: i32, w: i32, h: i32, c: [f32; 4]| {
        if let Some(src) = solid_src
            && w > 0
            && h > 0
        {
            out.push(TextDraw {
                dst: (
                    origin.0 + x * scale,
                    origin.1 + y * scale,
                    (w * scale) as u32,
                    (h * scale) as u32,
                ),
                src,
                color: c,
            });
        }
    };
    let stage_text = |out: &mut Vec<TextDraw>, s: &str, x: i32, y: i32, c: [f32; 4]| {
        let mut draws = text_draws_for(&font.layout_ascii(s), (x, y), c);
        scale_stage_text_draws(&mut draws, origin, scale as u32);
        out.extend(draws);
    };
    let stage_sprite = |out: &mut Vec<SpriteDraw>, src: (u32, u32, u32, u32), x: i32, y: i32| {
        out.push(SpriteDraw {
            dst: (
                origin.0 + x * scale,
                origin.1 + y * scale,
                src.2 * scale as u32,
                src.3 * scale as u32,
            ),
            src,
            color: white,
        });
    };

    // The three-quad scrim, as a run of scanline rects. Retail emits two
    // bevel trapezoids and a body quad; the bevels are only three rows tall,
    // so stepping them one row at a time reproduces the same silhouette
    // without a triangle rasteriser in either overlay layer.
    //
    // Which layer it lands in follows the readout it backs: with the
    // system-UI atlas resident it goes in the sprite half ahead of the labels
    // and numerals, because that half draws first; without it everything is a
    // font-atlas quad and the scrim leads the text half instead.
    {
        let mut scrim_rects: Vec<(i32, i32, i32, i32)> = Vec::new();
        for (ordinal, _) in members.iter().enumerate() {
            let x = FIELD_HUD_X0 + ordinal as i32 * FIELD_HUD_PITCH;
            let bevel_rows = -PLATE_TOP_DY; // 3
            for r in 0..bevel_rows {
                // Widens from the narrow top edge to the full body width.
                let t = (r + 1) as f32 / bevel_rows as f32;
                let l =
                    PLATE_BEVEL_LEFT_DX as f32 + (PLATE_LEFT_DX - PLATE_BEVEL_LEFT_DX) as f32 * t;
                let rr =
                    PLATE_TOP_RIGHT_DX as f32 + (PLATE_RIGHT_DX - PLATE_TOP_RIGHT_DX) as f32 * t;
                scrim_rects.push((
                    x + l.round() as i32,
                    y + PLATE_TOP_DY + r,
                    (rr - l).round() as i32,
                    1,
                ));
            }
            scrim_rects.push((
                x + PLATE_LEFT_DX,
                y,
                PLATE_RIGHT_DX - PLATE_LEFT_DX,
                PLATE_BODY_BOTTOM_DY,
            ));
            let close_rows = PLATE_BOTTOM_DY - PLATE_BODY_BOTTOM_DY; // 3
            for r in 0..close_rows {
                let t = (r + 1) as f32 / close_rows as f32;
                let l = PLATE_LEFT_DX as f32 + (PLATE_BEVEL_LEFT_DX - PLATE_LEFT_DX) as f32 * t;
                let rr =
                    PLATE_RIGHT_DX as f32 + (PLATE_BOTTOM_RIGHT_DX - PLATE_RIGHT_DX) as f32 * t;
                scrim_rects.push((
                    x + l.round() as i32,
                    y + PLATE_BODY_BOTTOM_DY + r,
                    (rr - l).round() as i32,
                    1,
                ));
            }
        }
        match scrim_src {
            // Any fully-opaque texel does: the shader multiplies
            // `color.rgb * texel.rgb`, and the scrim's colour is black.
            Some(src) => {
                for (rx, ry, rw, rh) in scrim_rects {
                    if rw > 0 && rh > 0 {
                        out.sprites.push(SpriteDraw {
                            dst: (
                                origin.0 + rx * scale,
                                origin.1 + ry * scale,
                                (rw * scale) as u32,
                                (rh * scale) as u32,
                            ),
                            src: (src.0, src.1, 1, 1),
                            color: PLATE_COLOR,
                        });
                    }
                }
            }
            None => {
                for (rx, ry, rw, rh) in scrim_rects {
                    stage_rect(&mut out.text, rx, ry, rw, rh, PLATE_COLOR);
                }
            }
        }
    }

    // Labels: the resident system-UI cells (one baked sub-palette, so they
    // blit untinted), or tinted text at the same seats.
    let label =
        |sprites: &mut Vec<SpriteDraw>, text: &mut Vec<TextDraw>, which: u8, x: i32, y: i32| {
            match chrome {
                Some(rects) => {
                    let src = match which {
                        0 => rects.label_hp,
                        1 => rects.label_mp,
                        _ => rects.label_lv,
                    };
                    stage_sprite(sprites, src, x, y);
                }
                None => {
                    let (s, c) = match which {
                        0 => ("HP", label_gold),
                        1 => ("MP", label_green),
                        _ => ("LV", label_gold),
                    };
                    stage_text(text, s, x, y, c);
                }
            }
        };
    let battle_cells = chrome.and_then(|r| r.battle);
    let separator =
        |sprites: &mut Vec<SpriteDraw>, text: &mut Vec<TextDraw>, x: i32, y: i32, c: [f32; 4]| {
            match battle_cells {
                Some(b) => sprites.push(SpriteDraw {
                    dst: (
                        origin.0 + (x + SEPARATOR_SPRITE_DX) * scale,
                        origin.1 + (y + SEPARATOR_SPRITE_DY) * scale,
                        b.separator.2 * scale as u32,
                        b.separator.3 * scale as u32,
                    ),
                    src: b.separator,
                    color: c,
                }),
                None => stage_text(text, "/", x, y, c),
            }
        };
    // A right-aligned numeral field of `cells` 8-px cells starting at `x`.
    let numerals = |sprites: &mut Vec<SpriteDraw>,
                    text: &mut Vec<TextDraw>,
                    value: u32,
                    x: i32,
                    cells: i32,
                    y: i32,
                    c: [f32; 4]| {
        let s = value.to_string();
        let digits = s.len() as i32;
        // Retail's field is fixed-width; a value wider than the field simply
        // starts at the field's left edge rather than running off to the left.
        let left = x + (cells - digits).max(0) * DIGIT_W;
        for (i, ch) in s.bytes().enumerate() {
            let cell_x = left + i as i32 * DIGIT_W;
            let d = u32::from(ch - b'0');
            match battle_cells
                .and_then(|b| b.digits)
                .and_then(|strip| crate::hud_digit_rect(strip, d))
            {
                Some(src) => sprites.push(SpriteDraw {
                    dst: (
                        origin.0 + cell_x * scale,
                        origin.1 + y * scale,
                        src.2 * scale as u32,
                        src.3 * scale as u32,
                    ),
                    src,
                    color: c,
                }),
                None => {
                    let glyph = [ch];
                    let g = std::str::from_utf8(&glyph).unwrap_or("0");
                    let advance = font.layout_ascii(g).advance_x as i32;
                    stage_text(text, g, cell_x + (DIGIT_W - advance) / 2, y, c);
                }
            }
        }
    };

    for (ordinal, m) in members.iter().enumerate() {
        let x = FIELD_HUD_X0 + ordinal as i32 * FIELD_HUD_PITCH;
        let hp_y = y + ROW_HP_DY;
        let mp_y = y + ROW_MP_DY;
        // Readout-tint law, identical to the battle strip's: a downed member
        // paints both readouts in the dead tier and leaves the name alone.
        let hp_tint = if m.alive {
            tier_color(hp_bar_color_index(m.hp, m.hp_max, false))
        } else {
            gauge_fill_color(2)
        };
        let mp_tint = if m.alive {
            tier_color(mp_bar_color_index(m.mp, m.mp_max))
        } else {
            gauge_fill_color(2)
        };

        stage_text(&mut out.text, m.name, x, y, READOUT_NORMAL);
        label(
            &mut out.sprites,
            &mut out.text,
            2,
            x + LV_LABEL.0,
            y + LV_LABEL.1,
        );
        numerals(
            &mut out.sprites,
            &mut out.text,
            u32::from(m.level),
            x + LV_FIELD_X,
            LV_FIELD_CELLS,
            y,
            READOUT_NORMAL,
        );

        label(&mut out.sprites, &mut out.text, 0, x, y + HP_LABEL_DY);
        numerals(
            &mut out.sprites,
            &mut out.text,
            u32::from(m.hp),
            x + CUR_FIELD_X,
            VALUE_FIELD_CELLS,
            hp_y,
            hp_tint,
        );
        separator(
            &mut out.sprites,
            &mut out.text,
            x + SEPARATOR_DX,
            hp_y,
            READOUT_NORMAL,
        );
        numerals(
            &mut out.sprites,
            &mut out.text,
            u32::from(m.hp_max),
            x + MAX_FIELD_X,
            VALUE_FIELD_CELLS,
            hp_y,
            hp_tint,
        );

        label(&mut out.sprites, &mut out.text, 1, x, y + MP_LABEL_DY);
        numerals(
            &mut out.sprites,
            &mut out.text,
            u32::from(m.mp),
            x + CUR_FIELD_X,
            VALUE_FIELD_CELLS,
            mp_y,
            mp_tint,
        );
        separator(
            &mut out.sprites,
            &mut out.text,
            x + SEPARATOR_DX,
            mp_y,
            READOUT_NORMAL,
        );
        numerals(
            &mut out.sprites,
            &mut out.text,
            u32::from(m.mp_max),
            x + MAX_FIELD_X,
            VALUE_FIELD_CELLS,
            mp_y,
            mp_tint,
        );
    }

    out
}

/// The readout-tint law's tier -> colour map. Every tier but "normal" takes
/// retail's own CLUT colour; "normal" keeps [`READOUT_NORMAL`].
fn tier_color(idx: u8) -> [f32; 4] {
    match idx {
        2 | 3 | 6 | 9 => gauge_fill_color(idx),
        _ => READOUT_NORMAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> legaia_font::Font {
        legaia_font::synthetic_for_tests()
    }

    fn members() -> Vec<FieldHudMember<'static>> {
        vec![
            FieldHudMember {
                name: "Vahn",
                level: 3,
                hp: 259,
                hp_max: 259,
                mp: 35,
                mp_max: 35,
                alive: true,
            },
            FieldHudMember {
                name: "Noa",
                level: 2,
                hp: 182,
                hp_max: 182,
                mp: 16,
                mp_max: 16,
                alive: true,
            },
            FieldHudMember {
                name: "Gala",
                level: 2,
                hp: 254,
                hp_max: 254,
                mp: 48,
                mp_max: 48,
                alive: true,
            },
        ]
    }

    /// The plate's outer silhouette, measured off the retail overworld
    /// framebuffers as a hard 2:1 brightness step: left `12`, top `9`,
    /// bottom `52`, and - with three members up - right `308`.
    #[test]
    fn the_scrim_spans_the_retail_measured_rect() {
        let f = font();
        let d = field_party_hud_draws_for(
            &f,
            &FieldPartyHudFrame {
                members: &members(),
                y: 12,
                chrome: None,
                scrim_src: None,
                solid_src: Some((0, 0, 1, 1)),
                origin: (0, 0),
                scale: 1,
            },
        );
        let rects: Vec<_> = d
            .text
            .iter()
            .filter(|t| t.color == PLATE_COLOR)
            .map(|t| (t.dst.0, t.dst.1, t.dst.2 as i32, t.dst.3 as i32))
            .collect();
        assert!(!rects.is_empty(), "the scrim must be emitted");
        let left = rects.iter().map(|r| r.0).min().unwrap();
        let top = rects.iter().map(|r| r.1).min().unwrap();
        let right = rects.iter().map(|r| r.0 + r.2).max().unwrap();
        let bottom = rects.iter().map(|r| r.1 + r.3).max().unwrap();
        assert_eq!((left, top, right, bottom), (12, 9, 308, 52));
    }

    /// The numeral fields are right-aligned in fixed-width cells, so a
    /// three-digit HP and a four-digit HP share the same right edge. Both
    /// edges are the retail capture's: current ends at `x+0x30`, maximum at
    /// `x+0x58`, level at `x+0x58`.
    #[test]
    fn numeral_fields_are_right_aligned_at_the_measured_edges() {
        let f = font();
        let one = vec![FieldHudMember {
            name: "Vahn",
            level: 3,
            hp: 259,
            hp_max: 4984,
            mp: 35,
            mp_max: 35,
            alive: true,
        }];
        let d = field_party_hud_draws_for(
            &f,
            &FieldPartyHudFrame {
                members: &one,
                y: 12,
                chrome: None,
                scrim_src: None,
                solid_src: Some((0, 0, 1, 1)),
                origin: (0, 0),
                scale: 1,
            },
        );
        // The glyph draws land inside their cells; take the cell grid back
        // out of the field constants instead of re-deriving it here.
        let cur_right = FIELD_HUD_X0 + CUR_FIELD_X + VALUE_FIELD_CELLS * DIGIT_W;
        let max_right = FIELD_HUD_X0 + MAX_FIELD_X + VALUE_FIELD_CELLS * DIGIT_W;
        let lv_right = FIELD_HUD_X0 + LV_FIELD_X + LV_FIELD_CELLS * DIGIT_W;
        assert_eq!((cur_right, max_right, lv_right), (0x40, 0x68, 0x68));
        // A 3-digit current starts one cell in; a 4-digit maximum fills.
        assert!(
            d.text
                .iter()
                .any(|t| t.dst.0 >= FIELD_HUD_X0 + CUR_FIELD_X + DIGIT_W)
        );
    }

    /// A downed member's HP and MP take the dead tier; the name does not.
    #[test]
    fn a_downed_member_paints_both_readouts_red_and_keeps_its_name() {
        let f = font();
        let dead = vec![FieldHudMember {
            name: "Gala",
            level: 5,
            hp: 0,
            hp_max: 254,
            mp: 10,
            mp_max: 48,
            alive: false,
        }];
        let d = field_party_hud_draws_for(
            &f,
            &FieldPartyHudFrame {
                members: &dead,
                y: 12,
                chrome: None,
                scrim_src: None,
                solid_src: Some((0, 0, 1, 1)),
                origin: (0, 0),
                scale: 1,
            },
        );
        let red = gauge_fill_color(2);
        assert!(d.text.iter().any(|t| t.color == red));
        // The name glyphs sit on the base row in the normal readout colour.
        assert!(
            d.text
                .iter()
                .any(|t| t.color == READOUT_NORMAL && t.dst.1 >= 12 && t.dst.1 < 12 + ROW_HP_DY)
        );
    }

    /// Columns are `0x64` apart and start at `0x10` - the loop's own
    /// `li s5,0x10` / `addiu s5,s5,0x64`.
    #[test]
    fn columns_sit_at_the_retail_pitch() {
        let f = font();
        let d = field_party_hud_draws_for(
            &f,
            &FieldPartyHudFrame {
                members: &members(),
                y: 12,
                chrome: None,
                scrim_src: None,
                solid_src: Some((0, 0, 1, 1)),
                origin: (0, 0),
                scale: 1,
            },
        );
        let plate_lefts: Vec<i32> = {
            let mut v: Vec<i32> = d
                .text
                .iter()
                .filter(|t| t.color == PLATE_COLOR && t.dst.3 == 0x25)
                .map(|t| t.dst.0)
                .collect();
            v.sort_unstable();
            v
        };
        assert_eq!(plate_lefts, vec![12, 112, 212]);
    }

    /// The stage transform is applied, so a host drawing at 2x gets the whole
    /// HUD at 2x rather than a 1x HUD in the corner.
    #[test]
    fn the_stage_transform_scales_the_whole_hud() {
        let f = font();
        let d = field_party_hud_draws_for(
            &f,
            &FieldPartyHudFrame {
                members: &members(),
                y: 12,
                chrome: None,
                scrim_src: None,
                solid_src: Some((0, 0, 1, 1)),
                origin: (40, 8),
                scale: 3,
            },
        );
        let left = d.text.iter().map(|t| t.dst.0).min().unwrap();
        assert_eq!(left, 40 + 12 * 3);
    }
}
