//! Battle-records screen draw-list builder (world-map dev-menu "records"
//! page). This is the render half of `FUN_801ED710`, whose data model
//! (value clamping, play-time decomposition, treasure percentage) is ported
//! renderer-free in `legaia_engine_vm::world_map_overlay::records_screen`.
//!
//! The retail routine takes `(param_1 = x origin, param_2 = y origin)` and
//! draws a fixed grid of labels + zero/blank-padded decimal fields via three
//! GPU-packet leaves - `FUN_8003cc98` (string label), `FUN_80034b78`
//! (blank-padded decimal), `FUN_80034e4c` (zero-padded decimal) - plus the
//! separator-glyph primitive `FUN_8003c1f8` and the per-character portrait
//! icon `FUN_8002c488`. Every draw offset below is expressed relative to the
//! `(x, y)` origin, so it is load-base independent and read straight off the
//! disassembly's `param_1 + N` / `param_2 + N` immediates.
//!
//! What this module ports is the *layout* - the emit order, the per-field
//! screen offsets, the digit-field widths and pad mode, and the per-field ink
//! staging id (`_DAT_8007b454`). The per-character portrait/separator sprites
//! (`FUN_8002c488`, ids `0x86 + col` and `0x8a`) are a UI-icon-atlas seam left
//! to the host, exactly as the status page leaves its LV/HP/MP icons to
//! `status_icon_sprites_for`. The English heading strings are supplied by the
//! caller (`RecordsLabels`) so no game text lives in this crate.
//!
//! PORT: FUN_801ED710 - battle-records screen renderer (layout + emit order)
//! PORT: FUN_80034e4c - zero-padded fixed-width decimal field
//! PORT: FUN_8003c1f8 - single separator-glyph draw
//!
//! Source: `ghidra/scripts/funcs/overlay_world_map_801ed710.txt`.

use crate::{TextDraw, text_draws_for};

/// Max learnable Hyper Arts (`FUN_801ED710` draws `NN / 15`).
pub const HYPER_ARTS_MAX: i64 = 0xF;
/// Max learnable magics (`FUN_801ED710` draws `NN / 22`).
pub const MAGIC_MAX: i64 = 0x16;

/// The English heading labels the records screen prints. Supplied by the
/// caller so the strings stay out of this crate (the retail pointers are
/// `s_No__of_Battles_801cf51c` .. `s_Treasure_801cf590`, in emit order).
#[derive(Debug, Clone, Copy)]
pub struct RecordsLabels<'a> {
    pub battles: &'a str,
    pub escapes: &'a str,
    pub max_hits: &'a str,
    pub max_damage: &'a str,
    pub knockouts: &'a str,
    pub monsters_defeated: &'a str,
    pub hyper_arts: &'a str,
    pub magic: &'a str,
    pub treasure: &'a str,
    /// Trailing "%" glyph label after the treasure percentage.
    pub percent: &'a str,
}

/// The display-ready records model, mirroring
/// `legaia_engine_vm::world_map_overlay::RecordsScreen`. The shell builds it
/// from that model and adds the caller-owned labels.
#[derive(Debug, Clone, Copy)]
pub struct RecordsScreenView<'a> {
    pub battles: u32,
    pub escapes: u32,
    pub play_hours: i32,
    pub play_minutes: i32,
    pub play_seconds: i32,
    pub max_hits: [u32; 3],
    pub max_damage: [u32; 3],
    pub knockouts: [u32; 3],
    pub monsters_defeated: [u32; 3],
    pub hyper_arts: [u8; 3],
    pub magic: [u8; 3],
    /// Treasure found / total counts (`DAT_801c6460` / `DAT_801c6462`).
    pub treasure_found: i32,
    pub treasure_total: i32,
    /// Percentage `found*100/total` and the two-digit fractional remainder.
    pub treasure_percent: i32,
    pub treasure_fraction: i32,
    /// Whether the treasure line draws at all (`0 < total`).
    pub treasure_shown: bool,
    pub labels: RecordsLabels<'a>,
}

/// One draw the records screen emits, in retail emit order. This structured
/// form is what the unit tests assert against - positions, digit widths, pad
/// mode and ink id are all checkable without going through the font atlas.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordsField {
    /// A heading label (`FUN_8003cc98`) at `(x, y)`.
    Label {
        x: i32,
        y: i32,
        text: String,
        ink: u8,
    },
    /// A right-aligned decimal field. `zero_pad = true` is the leading-zero
    /// primitive `FUN_80034e4c`; `false` is the blank-padded `FUN_80034b78`.
    Number {
        x: i32,
        y: i32,
        value: i64,
        digits: u8,
        zero_pad: bool,
        ink: u8,
    },
    /// A single separator glyph (`FUN_8003c1f8`): `':'` (glyph 9), `'/'`
    /// (glyph 6) or `'.'` (glyph 0xD).
    Symbol { x: i32, y: i32, ch: char, ink: u8 },
}

// Ink staging ids FUN_801ED710 writes to `_DAT_8007b454` before each field.
const INK_LABEL: u8 = 7; // white heading
const INK_BATTLES: u8 = 5;
const INK_MAX_HITS: u8 = 6;
const INK_MAX_DAMAGE: u8 = 6;
const INK_KNOCKOUTS: u8 = 9;
const INK_MONSTERS: u8 = 4;
const INK_HYPER_ARTS: u8 = 5;
const INK_MAGIC: u8 = 3;
const INK_TREASURE: u8 = 6;

// Per-character band row pitch (`iVar += 0x13` in every per-char loop).
const ROW_PITCH: i32 = 0x13;

/// Build the ordered field list from the records view at origin `(x, y)`.
///
/// PORT: FUN_801ED710
pub fn records_screen_fields(view: &RecordsScreenView<'_>, pen: (i32, i32)) -> Vec<RecordsField> {
    let (x, y) = pen;
    let mut out = Vec::new();
    let l = &view.labels;

    let label = |out: &mut Vec<RecordsField>, s: &str, lx: i32, ly: i32| {
        out.push(RecordsField::Label {
            x: lx,
            y: ly,
            text: s.to_string(),
            ink: INK_LABEL,
        });
    };
    let num =
        |out: &mut Vec<RecordsField>, v: i64, nx: i32, ny: i32, digits: u8, zp: bool, ink: u8| {
            out.push(RecordsField::Number {
                x: nx,
                y: ny,
                value: v,
                digits,
                zero_pad: zp,
                ink,
            });
        };
    let sym = |out: &mut Vec<RecordsField>, ch: char, sx: i32, sy: i32, ink: u8| {
        out.push(RecordsField::Symbol {
            x: sx,
            y: sy,
            ch,
            ink,
        });
    };

    // --- Heading singles -----------------------------------------------------
    // No. of Battles (cap 99999), value at +0x58.
    label(&mut out, l.battles, x, y);
    num(
        &mut out,
        view.battles as i64,
        x + 0x58,
        y,
        5,
        false,
        INK_BATTLES,
    );
    // No. of Escapes, one line down (+0xe).
    label(&mut out, l.escapes, x, y + 0xE);
    num(
        &mut out,
        view.escapes as i64,
        x + 0x58,
        y + 0xE,
        5,
        false,
        INK_BATTLES,
    );

    // --- Play time (H : MM : SS) at y+7 --------------------------------------
    let yt = y + 7;
    num(
        &mut out,
        view.play_hours as i64,
        x + 0xB8,
        yt,
        3,
        false,
        INK_LABEL,
    );
    sym(&mut out, ':', x + 0xD0, yt, INK_LABEL);
    num(
        &mut out,
        view.play_minutes as i64,
        x + 0xD8,
        yt,
        2,
        true,
        INK_LABEL,
    );
    sym(&mut out, ':', x + 0xE8, yt, INK_LABEL);
    num(
        &mut out,
        view.play_seconds as i64,
        x + 0xF0,
        yt,
        2,
        true,
        INK_LABEL,
    );

    // --- Per-character band 1 (labels y+0x21, rows y+0x31) -------------------
    let band1_label = y + 0x21;
    let band1_row0 = y + 0x31;
    // Maximum Hits (cap 999, 3 digits) - label at x-4, values at x+0x18.
    label(&mut out, l.max_hits, x - 4, band1_label);
    for c in 0..3 {
        let ry = band1_row0 + c as i32 * ROW_PITCH;
        num(
            &mut out,
            view.max_hits[c] as i64,
            x + 0x18,
            ry,
            3,
            false,
            INK_MAX_HITS,
        );
    }
    // Maximum Damage (cap 9999999, 7 digits) - label at x+0x58, values x+0x70.
    label(&mut out, l.max_damage, x + 0x58, band1_label);
    for c in 0..3 {
        let ry = band1_row0 + c as i32 * ROW_PITCH;
        num(
            &mut out,
            view.max_damage[c] as i64,
            x + 0x70,
            ry,
            7,
            false,
            INK_MAX_DAMAGE,
        );
    }
    // Knockouts (cap 999, 3 digits) - label at x+0xd4, values at x+0xe8.
    label(&mut out, l.knockouts, x + 0xD4, band1_label);
    for c in 0..3 {
        let ry = band1_row0 + c as i32 * ROW_PITCH;
        num(
            &mut out,
            view.knockouts[c] as i64,
            x + 0xE8,
            ry,
            3,
            false,
            INK_KNOCKOUTS,
        );
    }

    // --- Per-character band 2 (labels y+0x6b, rows y+0x7b) -------------------
    let band2_label = y + 0x6B;
    let band2_row0 = y + 0x7B;
    // Monsters Defeated (cap 999999, 6 digits) - label at x-4, values at x+0x18.
    label(&mut out, l.monsters_defeated, x - 4, band2_label);
    for c in 0..3 {
        let ry = band2_row0 + c as i32 * ROW_PITCH;
        num(
            &mut out,
            view.monsters_defeated[c] as i64,
            x + 0x18,
            ry,
            6,
            false,
            INK_MONSTERS,
        );
    }
    // Hyper Arts (NN / 15) - label at x+0x74, value x+0x80, '/' x+0x90, max x+0x98.
    label(&mut out, l.hyper_arts, x + 0x74, band2_label);
    for c in 0..3 {
        let ry = band2_row0 + c as i32 * ROW_PITCH;
        num(
            &mut out,
            view.hyper_arts[c] as i64,
            x + 0x80,
            ry,
            2,
            false,
            INK_HYPER_ARTS,
        );
        sym(&mut out, '/', x + 0x90, ry, INK_HYPER_ARTS);
        num(
            &mut out,
            HYPER_ARTS_MAX,
            x + 0x98,
            ry,
            2,
            false,
            INK_HYPER_ARTS,
        );
    }
    // Magic (NN / 22) - label at x+0xd0, value x+0xd8, '/' x+0xe8, max x+0xf0.
    label(&mut out, l.magic, x + 0xD0, band2_label);
    for c in 0..3 {
        let ry = band2_row0 + c as i32 * ROW_PITCH;
        num(
            &mut out,
            view.magic[c] as i64,
            x + 0xD8,
            ry,
            2,
            false,
            INK_MAGIC,
        );
        sym(&mut out, '/', x + 0xE8, ry, INK_MAGIC);
        num(&mut out, MAGIC_MAX, x + 0xF0, ry, 2, false, INK_MAGIC);
    }

    // --- Treasure line (only when total > 0) at y+0xb7 -----------------------
    if view.treasure_shown {
        let yr = y + 0xB7;
        label(&mut out, l.treasure, x + 0x10, yr);
        num(
            &mut out,
            view.treasure_found as i64,
            x + 0x50,
            yr,
            3,
            false,
            INK_TREASURE,
        );
        sym(&mut out, '/', x + 0x68, yr, INK_TREASURE);
        num(
            &mut out,
            view.treasure_total as i64,
            x + 0x70,
            yr,
            3,
            false,
            INK_TREASURE,
        );
        num(
            &mut out,
            view.treasure_percent as i64,
            x + 0x98,
            yr,
            3,
            false,
            INK_TREASURE,
        );
        sym(&mut out, '.', x + 0xB0, yr, INK_TREASURE);
        num(
            &mut out,
            view.treasure_fraction as i64,
            x + 0xB8,
            yr,
            2,
            true,
            INK_TREASURE,
        );
        label(&mut out, l.percent, x + 0xC8, yr);
        // The trailing "%" label inherits the treasure ink (no `= 7` reset).
        if let Some(RecordsField::Label { ink, .. }) = out.last_mut() {
            *ink = INK_TREASURE;
        }
    }

    out
}

/// Resolve a records ink-staging id to an RGBA tint. The `{5,6,7,9}` rows are
/// pinned against the menu string-CLUT (same palette as the status page);
/// staging `3` and `4` select CLUT rows with no golden capture yet, so they
/// fall back to white.
pub fn records_ink(staging: u8) -> [f32; 4] {
    match staging {
        7 => crate::MENU_TEXT_WHITE,
        6 => crate::MENU_TEXT_GOLD,
        5 => crate::MENU_TEXT_TEAL,
        9 => crate::MENU_TEXT_ORANGE,
        2 => crate::MENU_TEXT_RED,
        // Ink 3 (magic) and 4 (monsters) select distinct CLUT rows whose RGB
        // is not yet pinned from a capture; render neutral until it is.
        _ => crate::MENU_TEXT_WHITE,
    }
}

// Fixed decimal-cell pitch of the retail number primitives (8 px per digit).
const NUM_CELL_W: i32 = 8;

/// Blank-padded decimal field (`FUN_80034b78`): the value's decimal digits
/// right-aligned in a `digits`-wide field, leading cells left blank.
fn blank_number_draws(
    font: &legaia_font::Font,
    value: i64,
    x: i32,
    y: i32,
    digits: u8,
    color: [f32; 4],
) -> Vec<TextDraw> {
    let s = value.max(0).to_string();
    let len = s.len() as i32;
    let mut out = Vec::new();
    for (i, ch) in s.chars().enumerate() {
        let cell = (digits as i32 - len + i as i32).max(0);
        out.extend(text_draws_for(
            &font.layout_ascii(&ch.to_string()),
            (x + cell * NUM_CELL_W, y),
            color,
        ));
    }
    out
}

/// Zero-padded decimal field (`FUN_80034e4c`): the value reduced modulo
/// `10^digits` and printed with leading zeros across the whole field.
///
/// PORT: FUN_80034e4c
fn zero_number_draws(
    font: &legaia_font::Font,
    value: i64,
    x: i32,
    y: i32,
    digits: u8,
    color: [f32; 4],
) -> Vec<TextDraw> {
    let width = digits as usize;
    let pow10 = 10i64.pow(digits as u32);
    let v = value.max(0) % pow10;
    let s = format!("{v:0width$}");
    let mut out = Vec::new();
    for (i, ch) in s.chars().enumerate() {
        out.extend(text_draws_for(
            &font.layout_ascii(&ch.to_string()),
            (x + i as i32 * NUM_CELL_W, y),
            color,
        ));
    }
    out
}

/// Build the full records-screen [`TextDraw`] list at origin `pen`. The
/// per-character portrait/separator icons (`FUN_8002c488`) are not emitted -
/// they are a UI-icon-atlas sprite seam owned by the host.
///
/// PORT: FUN_801ED710
pub fn records_screen_draws_for(
    font: &legaia_font::Font,
    view: &RecordsScreenView<'_>,
    pen: (i32, i32),
) -> Vec<TextDraw> {
    let mut out = Vec::new();
    for field in records_screen_fields(view, pen) {
        match field {
            RecordsField::Label { x, y, text, ink } => {
                out.extend(text_draws_for(
                    &font.layout_ascii(&text),
                    (x, y),
                    records_ink(ink),
                ));
            }
            RecordsField::Number {
                x,
                y,
                value,
                digits,
                zero_pad,
                ink,
            } => {
                let color = records_ink(ink);
                if zero_pad {
                    out.extend(zero_number_draws(font, value, x, y, digits, color));
                } else {
                    out.extend(blank_number_draws(font, value, x, y, digits, color));
                }
            }
            RecordsField::Symbol { x, y, ch, ink } => {
                out.extend(text_draws_for(
                    &font.layout_ascii(&ch.to_string()),
                    (x, y),
                    records_ink(ink),
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_labels() -> RecordsLabels<'static> {
        RecordsLabels {
            battles: "BATTLES",
            escapes: "ESCAPES",
            max_hits: "MAXHITS",
            max_damage: "MAXDMG",
            knockouts: "KO",
            monsters_defeated: "MONSTERS",
            hyper_arts: "ARTS",
            magic: "MAGIC",
            treasure: "TREASURE",
            percent: "%",
        }
    }

    fn sample_view() -> RecordsScreenView<'static> {
        RecordsScreenView {
            battles: 1234,
            escapes: 5,
            play_hours: 12,
            play_minutes: 34,
            play_seconds: 56,
            max_hits: [10, 20, 30],
            max_damage: [100, 200, 300],
            knockouts: [1, 2, 3],
            monsters_defeated: [11, 22, 33],
            hyper_arts: [4, 5, 6],
            magic: [7, 8, 9],
            treasure_found: 25,
            treasure_total: 50,
            treasure_percent: 50,
            treasure_fraction: 0,
            treasure_shown: true,
            labels: sample_labels(),
        }
    }

    #[test]
    fn heading_singles_positions_and_ink() {
        let f = records_screen_fields(&sample_view(), (0, 0));
        // First field: battles label at origin, ink 7.
        assert_eq!(
            f[0],
            RecordsField::Label {
                x: 0,
                y: 0,
                text: "BATTLES".into(),
                ink: 7,
            }
        );
        // Second: battles value, 5-digit blank-padded at +0x58, ink 5.
        assert_eq!(
            f[1],
            RecordsField::Number {
                x: 0x58,
                y: 0,
                value: 1234,
                digits: 5,
                zero_pad: false,
                ink: 5,
            }
        );
        // Escapes one line down at +0xe.
        assert!(matches!(f[2], RecordsField::Label { y: 0xE, .. }));
        assert!(matches!(f[3], RecordsField::Number { y: 0xE, ink: 5, .. }));
    }

    #[test]
    fn play_time_is_colon_separated_and_zero_padded() {
        let f = records_screen_fields(&sample_view(), (0, 0));
        // Fields 4..9: hours(blank,3), ':', minutes(zero,2), ':', seconds(zero,2).
        assert_eq!(
            f[4],
            RecordsField::Number {
                x: 0xB8,
                y: 7,
                value: 12,
                digits: 3,
                zero_pad: false,
                ink: 7,
            }
        );
        assert_eq!(
            f[5],
            RecordsField::Symbol {
                x: 0xD0,
                y: 7,
                ch: ':',
                ink: 7
            }
        );
        assert_eq!(
            f[6],
            RecordsField::Number {
                x: 0xD8,
                y: 7,
                value: 34,
                digits: 2,
                zero_pad: true,
                ink: 7,
            }
        );
        assert_eq!(
            f[7],
            RecordsField::Symbol {
                x: 0xE8,
                y: 7,
                ch: ':',
                ink: 7
            }
        );
        assert!(matches!(
            f[8],
            RecordsField::Number {
                x: 0xF0,
                value: 56,
                zero_pad: true,
                ..
            }
        ));
    }

    #[test]
    fn per_character_bands_step_by_pitch() {
        let f = records_screen_fields(&sample_view(), (0, 0));
        // Locate the three Maximum-Hits values (x+0x18, y = 0x31 + c*0x13, ink 6).
        let hits: Vec<_> = f
            .iter()
            .filter_map(|fld| match fld {
                RecordsField::Number {
                    x: 0x18,
                    y,
                    value,
                    digits: 3,
                    ink: 6,
                    ..
                } if *y >= 0x31 && *y < 0x6B => Some((*y, *value)),
                _ => None,
            })
            .collect();
        assert_eq!(hits, vec![(0x31, 10), (0x44, 20), (0x57, 30)]);
    }

    #[test]
    fn hyper_arts_shows_slash_and_max() {
        let f = records_screen_fields(&sample_view(), (0, 0));
        // Row 0 of the Hyper Arts triple: value at x+0x80, '/' at x+0x90, 15 at x+0x98.
        let row0_y = 0x7B;
        assert!(f.contains(&RecordsField::Number {
            x: 0x80,
            y: row0_y,
            value: 4,
            digits: 2,
            zero_pad: false,
            ink: 5,
        }));
        assert!(f.contains(&RecordsField::Symbol {
            x: 0x90,
            y: row0_y,
            ch: '/',
            ink: 5
        }));
        assert!(f.contains(&RecordsField::Number {
            x: 0x98,
            y: row0_y,
            value: HYPER_ARTS_MAX,
            digits: 2,
            zero_pad: false,
            ink: 5,
        }));
    }

    #[test]
    fn magic_max_is_22() {
        let f = records_screen_fields(&sample_view(), (0, 0));
        assert!(f.iter().any(|fld| matches!(
            fld,
            RecordsField::Number { x: 0xF0, value, ink: 3, .. } if *value == MAGIC_MAX
        )));
    }

    #[test]
    fn treasure_line_present_when_shown() {
        let f = records_screen_fields(&sample_view(), (0, 0));
        let yr = 0xB7;
        // percentage "." decimal + fraction + trailing % label all at y+0xb7.
        assert!(f.contains(&RecordsField::Symbol {
            x: 0xB0,
            y: yr,
            ch: '.',
            ink: 6
        }));
        assert!(f.contains(&RecordsField::Number {
            x: 0xB8,
            y: yr,
            value: 0,
            digits: 2,
            zero_pad: true,
            ink: 6,
        }));
        // The trailing "%" inherits the treasure ink (6), not the label ink (7).
        assert!(f.contains(&RecordsField::Label {
            x: 0xC8,
            y: yr,
            text: "%".into(),
            ink: 6,
        }));
    }

    #[test]
    fn treasure_line_hidden_when_not_shown() {
        let mut v = sample_view();
        v.treasure_shown = false;
        let f = records_screen_fields(&v, (0, 0));
        assert!(
            !f.iter()
                .any(|fld| matches!(fld, RecordsField::Label { text, .. } if text == "TREASURE"))
        );
    }

    #[test]
    fn draws_are_nonempty_and_origin_translates() {
        let font = legaia_font::Font::placeholder();
        let view = sample_view();
        let a = records_screen_draws_for(&font, &view, (0, 0));
        let b = records_screen_draws_for(&font, &view, (40, 20));
        assert!(!a.is_empty());
        assert_eq!(a.len(), b.len());
        // Shifting the origin shifts every quad by the same delta.
        assert_eq!(a[0].dst.0 + 40, b[0].dst.0);
        assert_eq!(a[0].dst.1 + 20, b[0].dst.1);
    }

    #[test]
    fn zero_pad_reduces_modulo_field_width() {
        let font = legaia_font::Font::placeholder();
        // 1234 in a 2-digit zero field -> "34" (2 glyph quads).
        let d = zero_number_draws(&font, 1234, 0, 0, 2, [1.0; 4]);
        assert_eq!(d.len(), 2);
        // blank field for the same value keeps all 4 digits.
        let b = blank_number_draws(&font, 1234, 0, 0, 5, [1.0; 4]);
        assert_eq!(b.len(), 4);
    }
}
