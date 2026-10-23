//! The shop windows' label strings, read out of the menu overlay's own
//! string pool (PROT 0899) the way each window renderer loads them.
//!
//! Coordinates only: this module names the VAs the renderers pass to the
//! string primitive `FUN_80036888`; the text is read from the image the user
//! supplied, so no label is committed. A session without the overlay gets
//! empty labels, and the windows then draw no text rather than invented
//! text.
//!
//! Several labels carry a retail **name escape**: `0xC1 n` prints character
//! `n`'s name, `0xC2 n` item `n`'s name (`FUN_80036888` expands both). They
//! are kept as raw bytes ([`ShopLabel`]) and expanded by
//! [`expand_name_escapes`] at draw time, against the live roster and item
//! table.

use crate::pause_screens::MENU_OVERLAY_BASE_VA;

/// Picker rows (window 42, `FUN_801D4868`): Buy / Sell / Quit.
pub const PICKER_LABEL_VAS: [u32; 3] = [0x801C_EB94, 0x801C_EB9C, 0x801C_EBA4];
/// Window 41 (`FUN_801D4C28`): the note for a member already wearing the
/// staged item (`addiu a0,a0,-0x1454` at `0x801D4DA8`).
pub const EQUIPPED_NOTE_VA: u32 = 0x801C_EBAC;
/// Window 41: the note for a member the item's mask excludes
/// (`addiu a0,a0,-0x1448` at `0x801D4EA8`).
pub const CANNOT_EQUIP_NOTE_VA: u32 = 0x801C_EBB8;
/// Window 41's three stat labels, ATK / UDF / LDF order (`-0x1660`,
/// `-0x165C`, `-0x1658` at `0x801D5188` and siblings).
pub const COMPARE_STAT_LABEL_VAS: [u32; 3] = [0x801C_E9A0, 0x801C_E9A4, 0x801C_E9A8];
/// Window 35 (`FUN_801D5510`): the held line's head, `0x801D5560`.
pub const HAVE_HEAD_VA: u32 = 0x801C_EBC8;
/// Window 35: the held line's tail after the count, `0x801D55A0`.
pub const HAVE_TAIL_VA: u32 = 0x801C_EBD0;
/// Window 35: the line drawn instead when none is held, `0x801D55C0`.
pub const NONE_HELD_VA: u32 = 0x801C_EBD4;
/// Window 35: the buy prompt, `0x801D55DC`.
pub const BUY_PROMPT_VA: u32 = 0x801C_EBDC;
/// Window 36 (`FUN_801D56FC`): the bag row, `0x801D5708`.
pub const BAG_ROW_VA: u32 = 0x801C_EBF8;
/// Window 36: one recipient row per character id `0..=2`
/// (`-0x13F8` / `-0x13E8` / `-0x13D8` at `0x801D584C` and siblings), each
/// a sentence that opens with `[0xC1 id]`. Character `3` has no row string.
pub const RECIPIENT_ROW_VAS: [u32; 3] = [0x801C_EC08, 0x801C_EC18, 0x801C_EC28];
/// Window 37 (`FUN_801D5944`): the sell prompt, `0x801D5980`.
pub const SELL_PROMPT_VA: u32 = 0x801C_EC38;
/// Window 39 (`FUN_801D5AE8`): the price row's label.
pub const PRICE_LABEL_VA: u32 = 0x801C_EC54;
/// Window 39: the line that replaces the price row for a `0`-price item.
pub const CANNOT_SELL_VA: u32 = 0x801C_EC5C;
/// Window 31 (`FUN_801DCE20`): the toast heading, opening with `[0xC2 0xFE]`
/// (`addiu a0,a0,-0x15C0` at `0x801DCE30`).
pub const POINT_CARD_HEADING_VA: u32 = 0x801C_EA40;
/// Window 31: the unit after the 8-digit bank (`-0x15B0` at `0x801DCE98`).
pub const POINT_CARD_UNIT_VA: u32 = 0x801C_EA50;

/// First byte of the two-byte name escapes the string primitive expands.
pub const ESCAPE_CHARACTER_NAME: u8 = 0xC1;
/// `0xC2 n`: item `n`'s name.
pub const ESCAPE_ITEM_NAME: u8 = 0xC2;

/// One label's raw bytes: the string from its `@` lead (a width-0 blank
/// glyph retail draws as a one-pixel advance - see
/// [`crate::pause_screens::menu_overlay_string`]) up to its NUL, with
/// every `0xC1..=0xC7` escape kept together with its operand byte (an
/// operand may itself be `0`, which is why a plain C-string read stops
/// short of a label that opens `[0xC1 0x00]`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShopLabel(pub Vec<u8>);

impl ShopLabel {
    /// Read the label at `va` out of a PROT 0899 image; empty when the VA is
    /// outside it.
    pub fn read(overlay: &[u8], va: u32) -> Self {
        // A label a language pack moved is read where the code now finds it.
        let va = legaia_game_tables::menu_string_refs::resolve_va(overlay, va);
        let Some(rest) = va
            .checked_sub(MENU_OVERLAY_BASE_VA)
            .and_then(|o| overlay.get(o as usize..))
        else {
            return Self::default();
        };
        let body = rest;
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(&b) = body.get(i) {
            match b {
                0 => break,
                0xC1..=0xC7 => {
                    out.push(b);
                    if let Some(&op) = body.get(i + 1) {
                        out.push(op);
                    }
                    i += 2;
                }
                _ => {
                    out.push(b);
                    i += 1;
                }
            }
        }
        Self(out)
    }

    /// `true` when nothing was read.
    pub fn is_empty(&self) -> bool {
        self.0.strip_prefix(b"@").unwrap_or(&self.0).is_empty()
    }

    /// The label as text, with its name escapes expanded through `name`
    /// ([`expand_name_escapes`]).
    pub fn text(&self, name: impl Fn(u8, u8) -> Option<String>) -> String {
        expand_name_escapes(&self.0, name)
    }

    /// The label as text for a label with no escapes (an escape expands to
    /// nothing).
    pub fn plain(&self) -> String {
        self.text(|_, _| None)
    }
}

/// Expand a menu string's two-byte name escapes: `name(escape, operand)`
/// supplies the text (`None` drops the escape). Other bytes pass through
/// as Latin-1, the dialog font's byte space; the `0xCE` sprite escape and
/// its operand are dropped, since a `String` cannot carry them to the
/// font's byte layout.
pub fn expand_name_escapes(raw: &[u8], name: impl Fn(u8, u8) -> Option<String>) -> String {
    let mut out = String::new();
    let mut i = 0;
    while let Some(&b) = raw.get(i) {
        match b {
            0 => break,
            0xC1..=0xC7 => {
                if let Some(s) = raw.get(i + 1).and_then(|&op| name(b, op)) {
                    out.push_str(&s);
                }
                i += 2;
            }
            0xCE | 0xCF => i += 2,
            _ => {
                out.push(char::from(b));
                i += 1;
            }
        }
    }
    out
}

/// Byte-preserving sibling of [`expand_name_escapes`] for text that goes to
/// the font's byte layout (`legaia_font::Font::layout`): the name escapes
/// are replaced by the names' bytes, every other byte - the `0xCE` sprite
/// escape and its operand included - is kept.
pub fn expand_name_escapes_bytes(raw: &[u8], name: impl Fn(u8, u8) -> Option<String>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&b) = raw.get(i) {
        match b {
            0 => break,
            0xC1..=0xC7 => {
                if let Some(s) = raw.get(i + 1).and_then(|&op| name(b, op)) {
                    out.extend(
                        s.chars()
                            .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')),
                    );
                }
                i += 2;
            }
            0xCE | 0xCF => {
                out.push(b);
                if let Some(&op) = raw.get(i + 1) {
                    out.push(op);
                }
                i += 2;
            }
            _ => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

/// Every label the shop's windows print, read off one PROT 0899 image.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShopLabels {
    /// Window 42's rows: Buy, Sell, Quit.
    pub picker: [ShopLabel; 3],
    /// Window 41's already-equipped note.
    pub equipped: ShopLabel,
    /// Window 41's cannot-equip note.
    pub cannot_equip: ShopLabel,
    /// Window 41's ATK / UDF / LDF labels.
    pub stat_labels: [ShopLabel; 3],
    /// Window 35's held line: head, tail, and the line for none held.
    pub have_head: ShopLabel,
    pub have_tail: ShopLabel,
    pub none_held: ShopLabel,
    /// Window 35's prompt.
    pub buy_prompt: ShopLabel,
    /// Window 36's bag row.
    pub bag_row: ShopLabel,
    /// Window 36's rows for character ids `0..=2`.
    pub recipient_rows: [ShopLabel; 3],
    /// Window 37's prompt.
    pub sell_prompt: ShopLabel,
    /// Window 39's price label and cannot-sell line.
    pub price: ShopLabel,
    pub cannot_sell: ShopLabel,
    /// Window 31's heading and unit.
    pub point_card_heading: ShopLabel,
    pub point_card_unit: ShopLabel,
}

impl ShopLabels {
    /// Read every label out of a PROT 0899 image.
    pub fn from_menu_overlay(overlay: &[u8]) -> Self {
        let r = |va| ShopLabel::read(overlay, va);
        Self {
            picker: PICKER_LABEL_VAS.map(r),
            equipped: r(EQUIPPED_NOTE_VA),
            cannot_equip: r(CANNOT_EQUIP_NOTE_VA),
            stat_labels: COMPARE_STAT_LABEL_VAS.map(r),
            have_head: r(HAVE_HEAD_VA),
            have_tail: r(HAVE_TAIL_VA),
            none_held: r(NONE_HELD_VA),
            buy_prompt: r(BUY_PROMPT_VA),
            bag_row: r(BAG_ROW_VA),
            recipient_rows: RECIPIENT_ROW_VAS.map(r),
            sell_prompt: r(SELL_PROMPT_VA),
            price: r(PRICE_LABEL_VA),
            cannot_sell: r(CANNOT_SELL_VA),
            point_card_heading: r(POINT_CARD_HEADING_VA),
            point_card_unit: r(POINT_CARD_UNIT_VA),
        }
    }

    /// `true` once a host has installed real disc text.
    pub fn is_installed(&self) -> bool {
        !self.picker[0].is_empty()
    }

    /// Window 36's row for character `char_id`, or `None` for an id with no
    /// row string (retail draws nothing for character `3`).
    pub fn recipient_row(&self, char_id: u8) -> Option<&ShopLabel> {
        self.recipient_rows.get(usize::from(char_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic overlay image with `bytes` at `va`.
    fn image_with(va: u32, bytes: &[u8]) -> Vec<u8> {
        let off = (va - MENU_OVERLAY_BASE_VA) as usize;
        let mut img = vec![0u8; off + bytes.len() + 1];
        img[off..off + bytes.len()].copy_from_slice(bytes);
        img
    }

    #[test]
    fn an_escape_keeps_its_zero_operand() {
        let img = image_with(0x801C_EC08, b"@\xC1\x00 abc\x00");
        let l = ShopLabel::read(&img, 0x801C_EC08);
        assert_eq!(l.0, b"@\xC1\x00 abc");
        let s = l.text(|e, op| (e == 0xC1 && op == 0).then(|| "Name".to_string()));
        assert_eq!(s, "@Name abc");
    }

    #[test]
    fn a_plain_label_reads_to_its_nul() {
        let img = image_with(0x801C_EB94, b"@Abc\x00\x00@Def");
        // The `@` lead is kept: retail draws it as a blank one-pixel glyph.
        assert_eq!(ShopLabel::read(&img, 0x801C_EB94).plain(), "@Abc");
        assert!(ShopLabel::read(&image_with(0x801C_EB94, b"@\x00"), 0x801C_EB94).is_empty());
    }

    #[test]
    fn an_out_of_image_label_is_empty() {
        assert!(ShopLabel::read(&[0u8; 4], 0x801C_EB94).is_empty());
        assert!(!ShopLabels::default().is_installed());
    }

    #[test]
    fn byte_expansion_keeps_sprite_escapes() {
        let raw = b"\xCE\x10 Ab:\xC1\x02/x";
        let out = expand_name_escapes_bytes(raw, |_, op| (op == 2).then(|| "N".to_string()));
        assert_eq!(out, b"\xCE\x10 Ab:N/x");
        assert_eq!(expand_name_escapes(raw, |_, _| None), " Ab:/x");
    }
}
