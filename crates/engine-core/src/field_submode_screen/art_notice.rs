//! The **art-learned notice** a field script raises with op `49 09`: the
//! prompt screen (slot `0x28`, `FUN_801F1FDC`) installs descriptor
//! `PANEL_PROMPT`, whose window record `9` is painted by `FUN_801E733C`.
//!
//! The painter draws two field-overlay strings (`0x801F2B5C`, `0x801F2B6C`),
//! stacked `0x0F` apart at the window origin (`FUN_8003CD00(str, x, y)`).
//! Before drawing it patches the operand of the first `0xC1` token in the
//! first string to the parked payload's `+2` byte (the character), and the
//! operand of the first `0xC5` token in the second to `+3 + (+2) * 0x40`
//! (the art, keyed the way the dialog kernel's `0xC5` arm splits it). The
//! token search is `FUN_8003CBF8`, which steps over each `0xCx` operand.
//! The strings are disc text, read by the host; nothing here carries them.
//!
//! Drake Castle's book of Biron arts (`dolk2` P1[49]) and every master who
//! teaches a Hyper Art follow their `4C C7` grant with this beat. Without a
//! painter for record `9` the screen drew nothing and the confirm that
//! closes it looked like skipped text (issue #603).
//!
//! REF: FUN_801E733C, FUN_8003CBF8, FUN_801F1FDC

use super::*;

/// The two strings `FUN_801E733C` draws, by their field-overlay VA.
pub const ART_NOTICE_STRING_VAS: [u32; 2] = [0x801F_2B5C, 0x801F_2B6C];

/// Window record `9`'s geometry words (`0x801F2B98 + 9 * 0x1C`, `+0x08` ..
/// `+0x0E`): `(x, y, w, h)` = `(0x20, 0x60, 0x100, 0x1A)`.
pub const ART_NOTICE_CONTENT_RECT: (i32, i32, i32, i32) = (0x20, 0x60, 0x100, 0x1A);

/// The second string's offset below the first (`addiu a2,s5,0xf` at
/// `0x801E73F0`).
pub const ART_NOTICE_LINE_PITCH: i32 = 0x0F;

/// The caller-drawn frame's pad past the content rect on every side (the
/// window-system convention the Incense notice uses).
pub const ART_NOTICE_FRAME_PAD: i32 = 8;

/// The frame rect `(x, y, w, h)` a host draws the notice chrome at.
pub fn art_notice_frame_rect() -> (i32, i32, i32, i32) {
    let (x, y, w, h) = ART_NOTICE_CONTENT_RECT;
    (
        x - ART_NOTICE_FRAME_PAD,
        y - ART_NOTICE_FRAME_PAD,
        w + 2 * ART_NOTICE_FRAME_PAD,
        h + 2 * ART_NOTICE_FRAME_PAD,
    )
}

/// The two lines' pens `(x, y)` in 320x240 stage pixels.
pub fn art_notice_pens() -> [(i32, i32); 2] {
    let (x, y, _, _) = ART_NOTICE_CONTENT_RECT;
    [(x, y), (x, y + ART_NOTICE_LINE_PITCH)]
}

/// `FUN_8003CBF8(s, token, 1)` plus the painter's store: the operand of the
/// first `token` in `s` becomes `value`. The walk steps over each `0xCx`
/// pair's operand, as the routine's `andi v0,v0,0xf0` test does. A string
/// without the token is left alone.
pub fn patch_token_operand(s: &mut [u8], token: u8, value: u8) {
    let mut i = 0;
    while i < s.len() && s[i] != 0 {
        if s[i] == token {
            if let Some(op) = s.get_mut(i + 1) {
                *op = value;
            }
            return;
        }
        i += if s[i] & 0xF0 == 0xC0 { 2 } else { 1 };
    }
}

/// A dialog-encoded string up to its terminator, read the way the text
/// kernel walks it: a `0xC0..=0xCF` byte and its operand are one token, so a
/// zero operand (`0xC1 0x00`, the first character's name) is not the end.
/// `FUN_8003CBF8` and the draw both step tokens this way.
pub fn token_cstr(bytes: &[u8]) -> Vec<u8> {
    let mut i = 0;
    while i < bytes.len() && bytes[i] != 0 {
        i += if bytes[i] & 0xF0 == 0xC0 { 2 } else { 1 };
    }
    bytes[..i.min(bytes.len())].to_vec()
}

impl World {
    /// `(character, art)` while the art-learned notice is on screen: the
    /// prompt screen is up with window record `9` installed. The pair is the
    /// parked operand's `+2` / `+3` bytes, which is what the painter reads
    /// through `_DAT_8007B450`.
    pub fn art_notice_shown(&self) -> Option<(u8, u8)> {
        let s = &self.field_vm.submode_screen;
        (s.open
            && s.actor.state == slot::PROMPT
            && s.installed_windows.contains(&hub::window::ART_NOTICE))
        .then(|| (s.op49_operand[2], s.op49_operand[3]))
    }

    /// The notice's two lines, with the painter's operand patches applied and
    /// the `0xC1` / `0xC5` tokens expanded through the live party names and
    /// the arts-name table. `read` returns the field-overlay bytes from a VA
    /// onward (to the image's end); each string is cut at its terminator by
    /// [`token_cstr`], since both strings carry a zero operand until the
    /// painter patches it. `None` while no notice is shown.
    pub fn art_notice_lines(&self, read: impl Fn(u32) -> Vec<u8>) -> Option<[Vec<u8>; 2]> {
        let (character, art) = self.art_notice_shown()?;
        let mut first = token_cstr(&read(ART_NOTICE_STRING_VAS[0]));
        let mut second = token_cstr(&read(ART_NOTICE_STRING_VAS[1]));
        patch_token_operand(&mut first, 0xC1, character);
        patch_token_operand(
            &mut second,
            0xC5,
            art.wrapping_add(character.wrapping_mul(0x40)),
        );
        Some([
            self.expand_notice_tokens(&first),
            self.expand_notice_tokens(&second),
        ])
    }

    /// Expand the `0xC1` (party name; `0x63` the leader) and `0xC5` (arts
    /// name) tokens. Other pairs - the `0xCF` ink escapes - pass through.
    fn expand_notice_tokens(&self, raw: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(raw.len() + 16);
        let mut i = 0;
        while i < raw.len() && raw[i] != 0 {
            let b = raw[i];
            if b & 0xF0 == 0xC0 {
                let op = raw.get(i + 1).copied().unwrap_or(0);
                match b {
                    0xC1 => {
                        let who = if op == 0x63 {
                            usize::from(self.field_leader())
                        } else {
                            usize::from(op)
                        };
                        out.extend_from_slice(self.party_name(who).as_bytes());
                    }
                    0xC5 => out.extend(self.arts_name_token(op).unwrap_or_default()),
                    _ => out.extend_from_slice(&[b, op]),
                }
                i += 2;
                continue;
            }
            out.push(b);
            i += 1;
        }
        out
    }
}
