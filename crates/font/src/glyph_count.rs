//! Typewriter **glyph count** of an encoded string - retail `FUN_80036044`
//! (`see ghidra/scripts/funcs/80036044.txt`).
//!
//! Not a width and not a wrap: the routine counts the units a typewriter
//! reveal steps through. The field dialog pager `FUN_801D84D0` calls it on the
//! row being typed (`jal 0x80036044` at `0x801D8A6C`, argument the row pointer
//! `_DAT_801F3540[_DAT_801F3530]`) and compares the result against its reveal
//! counter `_DAT_801F2748`: below the count the row keeps typing, at or above
//! it the row is finished, and a row shorter than `0x22` units then holds for
//! `(0x22 - count) * 4` (`_DAT_801F275C`, drained by `32 * DAT_1F800393` per
//! pager call). `FUN_8003CC98` (draw one string, return its count) and
//! `FUN_8003CD00` are the other callers.
//!
//! ## The walk, read off the disassembly
//!
//! 1. A first byte below `0x1F` returns `0` (`sltiu v0,v0,0x1f` at
//!    `0x80036058`).
//! 2. A **pre-walk** sizes the loop: `units` starts at `1` for the first byte,
//!    then, when the second byte is non-zero, walks from the second byte to the
//!    first `0x00`, adding one per byte and one more for each byte whose high
//!    nibble is `0xC` (whose operand it then skips). The first byte is never
//!    tested for the escape nibble. So `units` is the string's byte length up
//!    to its `NUL`.
//! 3. The **main loop** runs `units` times - it decrements the counter once per
//!    iteration (`addiu t2,t2,-1` in the delay slot at `0x800360F0`) however
//!    many bytes the iteration consumes - and per byte:
//!    - `0xCF` (colour) consumes two bytes and counts nothing;
//!    - `0xCE` (escape) consumes two bytes and counts one;
//!    - `0xC0..=0xC7` (the `(b + 0x40) & 0xFF < 8` gate) consume two bytes
//!      and count the substituted string (below);
//!    - anything else counts one and consumes one.
//!
//!    Because a two-byte unit is one iteration but two bytes of the pre-walk's
//!    length, the loop **runs past the `NUL`** by one byte per two-byte unit,
//!    counting what it finds there. The count of a string with `k` escapes is
//!    therefore `k` higher than its glyphs whenever the bytes after the `NUL`
//!    are plain; this port reads them from `text` (zero past its end), so a
//!    caller that passes the row with the rest of its buffer gets retail's
//!    count.
//!
//! ## Substitutions
//!
//! The jump table at `0x80010EA0` (seven words, `0xC1..=0xC7`) resolves the
//! token's string, and the count adds that string's length up to its first
//! byte below `0x1F`, two per `0xC0..=0xCF` pair:
//!
//! | Token | String |
//! |---|---|
//! | `0xC1 X` | party name at record `+0x2A7` (`0x800849AF + X * 0x414`); `X = 0x63` reads `X` from `DAT_80084597` |
//! | `0xC2 X` / `0xC4 X` | item name `*(0x8007436C + X * 0xC)` |
//! | `0xC3 X` | spell name `*(0x800754D0 + X * 0xC)` |
//! | `0xC5 X` | arts name: the `0x80075EC4` table (`0x14`-byte records), searched for `[X >> 6, X & 0x3F]` |
//! | `0xC7 X` | the inline 8-byte name at `0x80073F24 + X * 8` |
//! | `0xC0`, `0xC6` | no string - counts nothing (`0xC6`'s table word is the no-string exit `0x80036410`) |
//!
//! Then a **nested** pass walks the substituted string to its `NUL` and, for
//! each `0xC1 Y` inside it, adds that party name's length too. Only `0xC1`
//! nests.
//!
//! The strings are runtime state, so they come through the same
//! [`Expander`](crate::measure::Expander) hook [`crate::Font::measure`] uses:
//! `(op, arg)` in, the bytes retail would read out. `0xC1 0x63` and the `0xC5`
//! table search are the hook's to resolve. A token the hook cannot resolve is
//! reported in [`GlyphCount::unresolved`] and counts nothing.

use crate::measure::Expander;

/// Result of [`typewriter_glyph_count`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GlyphCount {
    /// The count `FUN_80036044` returns.
    pub count: u32,
    /// Substitution tokens `(op, arg)` the hook could not resolve. When
    /// non-empty, [`Self::count`] is a lower bound.
    pub unresolved: Vec<(u8, u8)>,
}

/// `(b & 0xF0) == 0xC0` - the two-byte opcode nibble every walk here tests.
fn two_byte(b: u8) -> bool {
    b & 0xF0 == 0xC0
}

/// Length of a substituted string the way the arms count it: bytes up to the
/// first one below `0x1F`, two per `0xC0..=0xCF` pair.
fn string_units(s: &[u8]) -> u32 {
    let at = |k: usize| s.get(k).copied().unwrap_or(0);
    let (mut k, mut n) = (0usize, 0u32);
    while at(k) >= 0x1F {
        if two_byte(at(k)) {
            k += 1;
            n += 1;
        }
        k += 1;
        n += 1;
    }
    n
}

/// The typewriter glyph count of `text` - see the module docs.
///
/// PORT: FUN_80036044
///
/// NOT WIRED: its consumer is the field dialog pager's row gate
/// (`0x801D8A6C..0x801D8AA8`: count vs the reveal counter `_DAT_801F2748`, then
/// the short-row hold `_DAT_801F275C`), and the engine's pager is not built
/// on that counter. `legaia_engine_core::dialog::OwnedDialogPanel` types one
/// MES event per tick and ends a row on its terminator byte, so it has no
/// reveal count to compare this against and no hold to size from it; routing
/// the count in means re-basing the panel's pacing on retail's counter
/// (`_DAT_801F2758` accumulating `DAT_1F800393` against the speed word
/// `_DAT_801F2754`, at most three units a call), which moves every dialogue
/// frame the replay oracles pin.
pub fn typewriter_glyph_count(text: &[u8], expand: Option<&Expander<'_>>) -> GlyphCount {
    let at = |k: usize| text.get(k).copied().unwrap_or(0);
    let mut out = GlyphCount::default();
    if at(0) < 0x1F {
        return out;
    }
    // Pre-walk (`0x80036064..0x800360A8`).
    let mut units: i32 = 1;
    if at(1) != 0 {
        let mut a = 1usize;
        loop {
            if two_byte(at(a)) {
                units += 1;
                a += 1;
            }
            a += 1;
            units += 1;
            if at(a) == 0 {
                break;
            }
        }
    }
    // Main loop (`0x800360E0..0x80036500`).
    let mut i = 0usize;
    loop {
        let b = at(i);
        units -= 1;
        if b == 0xCF {
            i += 2;
        } else if b == 0xCE {
            out.count += 1;
            i += 2;
        } else if b.wrapping_add(0x40) < 8 {
            let arg = at(i + 1);
            if matches!(b, 0xC1..=0xC5 | 0xC7) {
                match expand.and_then(|f| f(b, arg)) {
                    None => out.unresolved.push((b, arg)),
                    Some(sub) => {
                        out.count += string_units(&sub);
                        // The nested pass (`0x80036418..0x800364EC`): only a
                        // `0xC1` inside the spliced string adds again.
                        let sat = |k: usize| sub.get(k).copied().unwrap_or(0);
                        let mut j = 0usize;
                        while sat(j) != 0 {
                            if sat(j) == 0xC1 {
                                let inner = sat(j + 1);
                                match expand.and_then(|f| f(0xC1, inner)) {
                                    Some(name) => out.count += string_units(&name),
                                    None => out.unresolved.push((0xC1, inner)),
                                }
                                j += 1;
                            }
                            j += 1;
                        }
                    }
                }
            }
            i += 2;
        } else {
            out.count += 1;
            i += 1;
        }
        if units <= 0 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(text: &[u8]) -> u32 {
        typewriter_glyph_count(text, None).count
    }

    #[test]
    fn a_plain_row_counts_every_byte_to_its_nul_lead_included() {
        // The pager passes the row pointer, lead byte and all.
        assert_eq!(count(b"\x1FHello\0"), 6);
        assert_eq!(count(b"A\0"), 1);
    }

    #[test]
    fn a_leading_control_byte_counts_nothing() {
        assert_eq!(count(b"\x00abc"), 0);
        assert_eq!(count(b"\x1Eabc\0"), 0);
    }

    #[test]
    fn a_two_byte_unit_runs_the_loop_one_byte_past_the_nul() {
        // `A`, `0xCE 05`, `B`: three glyphs, four bytes, four iterations - the
        // fourth reads the NUL and counts it.
        assert_eq!(count(b"A\xCE\x05B\0"), 4);
        // Two escapes overrun two bytes: the second extra iteration reads
        // the byte *after* the NUL, whatever it is.
        assert_eq!(count(b"\xCE\x01\xCE\x02\0X"), 4);
        // Colour changes count nothing themselves but still overrun.
        assert_eq!(count(b"A\xCF\x02BC\0"), 4);
    }

    #[test]
    fn substitutions_count_the_spliced_string_and_nested_party_names() {
        let names = |op: u8, arg: u8| -> Option<Vec<u8>> {
            match (op, arg) {
                (0xC1, 0) => Some(b"Vahn\0".to_vec()),
                (0xC2, 7) => Some(b"Door of Wind\0".to_vec()),
                // A spliced string carrying a nested party name.
                (0xC7, 1) => Some(b"to \xC1\x00!\0".to_vec()),
                _ => None,
            }
        };
        let hook: &Expander<'_> = &names;
        // `0xC1 00` = "Vahn" (4). The pre-walk reads the zero operand as the
        // NUL, so the loop runs once and does not overrun.
        assert_eq!(typewriter_glyph_count(b"\xC1\x00\0", Some(hook)).count, 4);
        // Item name (12) + `!` (1) + overrun (1).
        assert_eq!(
            typewriter_glyph_count(b"\xC2\x07!\0", Some(hook)).count,
            12 + 1 + 1
        );
        // "to " (3) + the nested pair counted as two units + "!" (1), then
        // the nested pass adds "Vahn" (4), plus the overrun.
        assert_eq!(
            typewriter_glyph_count(b"\xC7\x01\0", Some(hook)).count,
            3 + 2 + 1 + 4 + 1
        );
        // No arm for 0xC0 / 0xC6: nothing counted, nothing reported.
        let r = typewriter_glyph_count(b"\xC6\x01\xC0\x02\0", Some(hook));
        assert_eq!(r.unresolved, Vec::new());
        // An unresolved token is reported.
        let r = typewriter_glyph_count(b"\xC3\x09\0", Some(hook));
        assert_eq!(r.unresolved, vec![(0xC3, 0x09)]);
    }

    #[test]
    fn the_first_byte_is_not_tested_for_the_escape_nibble_by_the_pre_walk() {
        // `0xCE 41 42`: the pre-walk sizes it as three units (the operand is
        // walked as a plain byte), the main loop consumes the escape as two.
        assert_eq!(count(b"\xCE\x41\x42\0"), 1 + 1 + 1);
    }
}
