//! On-screen width limits for translated text, per display context.
//!
//! Byte room is not screen room: a translated line that fits its slot can
//! still run past the box it is drawn in. Each [`TextLimit`] is the pen
//! distance, in pixels, from where retail starts drawing a string to the
//! next thing on its row (a number column, a box edge), measured with
//! [`crate::Font::measure`] at the surface's `glyph_pad`. Only contexts whose
//! numbers come from the disassembly or a capture are listed; the derivation
//! of each lives in `docs/formats/dialog-font.md` (section "Line width and
//! wrapping") and `docs/subsystems/field-menu.md`.
//!
//! No retail text surface wraps: every line break is a `0x7C` byte or a new
//! `0x1F` dialog line written by the author. A too-long line is drawn in
//! full, past its box.

use crate::measure::TextMeasure;

/// One display context's width budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextLimit {
    /// Stable identifier for the context.
    pub context: &'static str,
    /// Largest pen advance, in pixels, that stays clear of the next column
    /// or the box edge.
    pub max_px: u16,
    /// Rows the surface shows at once, when it has a fixed count.
    pub max_lines: Option<u8>,
    /// Whether retail breaks an over-long line by itself. `false` for every
    /// context found so far.
    pub wraps: bool,
    /// `DAT_800740E8` for this surface: the extra pixel per glyph the field
    /// dialog pager adds. Measure with the same value.
    pub glyph_pad: u8,
    /// Where the number comes from.
    pub provenance: &'static str,
}

impl TextLimit {
    /// True when every line of `m` fits in [`Self::max_px`] and, for a
    /// fixed-row surface, the line count fits [`Self::max_lines`].
    pub fn fits(&self, m: &TextMeasure) -> bool {
        m.max_px <= u32::from(self.max_px)
            && self.max_lines.is_none_or(|n| m.lines() <= usize::from(n))
    }

    /// Pixels by which the widest line of `m` overruns the budget.
    pub fn overflow_px(&self, m: &TextMeasure) -> u32 {
        m.max_px.saturating_sub(u32::from(self.max_px))
    }
}

/// Look a context up by its identifier.
pub fn limit_for(context: &str) -> Option<&'static TextLimit> {
    TEXT_LIMITS.iter().find(|l| l.context == context)
}

/// Every pinned context. Field-dialog rows are separate `0x1F` lines, so
/// `max_lines` there counts rows per box, not `0x7C` breaks.
pub const TEXT_LIMITS: &[TextLimit] = &[
    TextLimit {
        context: "field_dialog_row",
        max_px: 244,
        max_lines: Some(3),
        wraps: false,
        glyph_pad: 1,
        provenance: "FUN_801D84D0: rows drawn at the box x (ctx+0x12) with \
                     DAT_800740E8 = 1 (0x801D97D8); box centre rect 0xF4 wide \
                     (0x801D99CC); 3 rows (_DAT_801F2740 = 3 at 0x801D90F0). \
                     Pad capture-confirmed: v0_1_tetsu_dialogue_accept glyph \
                     pitch = width + 2",
    },
    TextLimit {
        context: "field_dialog_row_beside_page_hand",
        max_px: 228,
        max_lines: Some(2),
        wraps: false,
        glyph_pad: 1,
        provenance: "FUN_801D84D0: page-advance hand at absolute x 0x10A (0x801D9834; box \
                     x 0x26 + 0xE4), y box_y + rows*0xF - 0x13, 16 px tall - it \
                     covers the right end of the page's last two rows while the \
                     pager waits for confirm",
    },
    TextLimit {
        context: "field_dialog_option",
        max_px: 228,
        max_lines: Some(4),
        wraps: false,
        glyph_pad: 1,
        provenance: "FUN_801D84D0 picker: labels at box x + 0x10 \
                     (0x801D9B6C) with DAT_800740E8 = 1 (0x801D9B78) in a \
                     0xF4-wide box; N <= 4 options",
    },
    TextLimit {
        context: "party_name",
        max_px: 56,
        max_lines: Some(1),
        wraps: false,
        glyph_pad: 0,
        provenance: "FUN_801F03F0 (field overlay 0897): a typed glyph is kept \
                     only while FUN_80035F04(name) < 0x39 (0x801F064C..0x801F0654); \
                     name field record +0x2A7, 9 bytes",
    },
    TextLimit {
        context: "item_list_name",
        max_px: 104,
        max_lines: Some(1),
        wraps: false,
        glyph_pad: 0,
        provenance: "FUN_80032A44 bag row: name at WX+0xC (0x8003316C), count \
                     as a 3-cell 8-px field from WX+0x6C (0x8003317C); counts \
                     cap at 99, so the tens cell at WX+0x74 is the first ink",
    },
    TextLimit {
        context: "shop_buy_name",
        max_px: 104,
        max_lines: Some(1),
        wraps: false,
        glyph_pad: 0,
        provenance: "FUN_80032A44 class 0x3000/0xA000 row (built by \
                     FUN_80030628 case 0x0B): name at WX+0x18 (0x800335A8), \
                     5-cell price field from WX+0x80 (0x800335B0)",
    },
    TextLimit {
        context: "item_info_name",
        max_px: 124,
        max_lines: Some(1),
        wraps: false,
        glyph_pad: 0,
        provenance: "FUN_801D0F1C draws the name at WX (0x801D0F8C); \
                     FUN_801DCB60 draws the 2-digit count at WX+0x7C (0x801DCBD4)",
    },
    TextLimit {
        context: "status_magic_name",
        max_px: 104,
        max_lines: Some(1),
        wraps: false,
        glyph_pad: 0,
        provenance: "FUN_801D33D8 magic page: spell name at s7 (0x801D42EC), \
                     level string at s7+0x68 (0x801D430C)",
    },
    TextLimit {
        context: "status_moves_name",
        max_px: 114,
        max_lines: Some(1),
        wraps: false,
        glyph_pad: 0,
        provenance: "FUN_801D33D8 moves page: art name at s7 (0x801D44F4), \
                     3-cell AP field at s7+0x72 (0x801D4538)",
    },
    TextLimit {
        context: "battle_message_line",
        max_px: 288,
        max_lines: None,
        wraps: false,
        glyph_pad: 0,
        provenance: "battle message banner / formation line: pen (16,12), \
                     box 288 wide (FUN_801D9D3C immediates at the 0x801DA234 \
                     arm; placement record 67); capture: 'Ambushed!' spawned \
                     288 wide",
    },
    TextLimit {
        context: "battle_intro_enemy_label",
        max_px: 308,
        max_lines: Some(1),
        wraps: false,
        glyph_pad: 0,
        provenance: "FUN_801D9D3C clamps each label to 6 <= x <= 0x13A - width; \
                     labels share one row, so several groups need far less",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::MeasureOptions;
    use crate::synthetic_for_tests;

    #[test]
    fn contexts_are_unique_and_none_wrap() {
        for (i, a) in TEXT_LIMITS.iter().enumerate() {
            assert!(!a.wraps, "{}", a.context);
            assert!(a.max_px > 0);
            for b in &TEXT_LIMITS[i + 1..] {
                assert_ne!(a.context, b.context);
            }
        }
    }

    #[test]
    fn dialog_contexts_carry_the_pager_pad() {
        for l in TEXT_LIMITS {
            let want = u8::from(l.context.starts_with("field_dialog"));
            assert_eq!(l.glyph_pad, want, "{}", l.context);
        }
    }

    #[test]
    fn fits_checks_width_and_rows() {
        let f = synthetic_for_tests();
        let l = limit_for("field_dialog_row").unwrap();
        let opts = MeasureOptions::dialog();
        assert!(l.fits(&f.measure(b"short", &opts)));
        let long = vec![b'm'; 80];
        let m = f.measure(&long, &opts);
        assert!(!l.fits(&m));
        assert_eq!(l.overflow_px(&m), m.max_px - 244);
        assert!(!l.fits(&f.measure(b"a|b|c|d", &opts)));
        assert!(l.fits(&f.measure(b"a|b|c", &opts)));
    }

    #[test]
    fn unknown_context_is_none() {
        assert!(limit_for("no_such_context").is_none());
    }
}
