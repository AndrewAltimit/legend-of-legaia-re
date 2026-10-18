//! Sub-screen `0x15` (the record screen): list source per step, list length, frame shape and row swap.

// ---------------------------------------------------------------------------
// Sub-screen 0x15 - the per-character list screen (`FUN_801DA2A0`)
// ---------------------------------------------------------------------------

/// Character-record offsets sub-screen `0x15` addresses, expressed relative
/// to the record base `0x80084708`.
///
/// The disassembly indexes from `0x80084140` instead, so every printed
/// displacement here is `0x5C8` higher than the constant below - the record
/// array starts `0x5C8` into that block
/// (see [`save-record.md`](../../../docs/formats/save-record.md)).
mod sub15_record {
    /// `0x6BC` printed - the 64-slot accessory-passive ("Goods") ability
    /// bitfield, the same field
    /// `legaia_engine_battle::accessory_passives` indexes.
    pub const ABILITY_BITS: usize = 0x0F4;
    /// `0x5D0` printed - a word array the row swap exchanges, 4-byte stride.
    pub const ROW_WORDS: usize = 0x008;
    /// `0x704` printed - the learned-spell count.
    pub const SPELL_COUNT: usize = 0x13C;
    /// `0x705` printed - the learned-spell id list, immediately after its
    /// count.
    pub const SPELL_LIST: usize = 0x13D;
    /// `0x729` printed - the per-spell byte the swap keeps in step with
    /// [`SPELL_LIST`].
    pub const SPELL_AUX: usize = 0x161;
    /// `0x74D` printed - the third list's count byte.
    pub const THIRD_COUNT: usize = 0x185;
    /// `0x75E` printed - the eight-byte equip array; the Ra-Seru slot index
    /// comes from the `0x8007B424 + char*2` halfword table.
    pub const EQUIP: usize = 0x196;
}

pub use sub15_record::{
    ABILITY_BITS as SUB15_ABILITY_BITS, EQUIP as SUB15_EQUIP, ROW_WORDS as SUB15_ROW_WORDS,
    SPELL_AUX as SUB15_SPELL_AUX, SPELL_COUNT as SUB15_SPELL_COUNT, SPELL_LIST as SUB15_SPELL_LIST,
    THIRD_COUNT as SUB15_THIRD_COUNT,
};

/// Width of the ability bitfield sub-screen `0x15` counts over: `0x40`
/// bits, walked one at a time as `word[bit >> 5] & (1 << (bit & 0x1F))`.
pub const SUB15_ABILITY_BIT_COUNT: usize = 0x40;

/// Which per-character list sub-screen `0x15` is showing.
///
/// The screen is one body serving three lists, and the *step counter*
/// selects which - not a separate screen id. Steps `2`/`5`, `3`/`6` and
/// `4`/`7` are the settle / running pair for each list, which is why the
/// resolver below maps both members of a pair to the same source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sub15ListSource {
    /// Steps `2` / `5`: the row count is the **population count** of the
    /// 64-slot ability bitfield.
    Abilities,
    /// Steps `3` / `6`: the row count is the learned-spell count, and it
    /// is gated on the character's Ra-Seru equip slot being filled - an
    /// empty slot reports zero rows rather than an empty list.
    Magic,
    /// Steps `4` / `7`: the row count is the single byte at `+0x185`.
    Third,
    /// Any other step: no list.
    None,
}

/// Step -> list source, the three-way branch at `0x801DA538..0x801DA64C`.
///
/// PORT: FUN_801DA2A0 (see `ghidra/scripts/funcs/overlay_save_ui_801da2a0.txt`)
///
/// Wired: [`crate::list_order::ListOrderSession`] is the page, opened over
/// the Magic screen's spell rows on both hosts, and the step it opens at is
/// what this resolves. The three sources stay one body, as retail has them:
/// the session asks this which list it is showing and only the spell list's
/// step carries the exchange arm.
pub fn sub15_list_source(step: u8) -> Sub15ListSource {
    match step {
        2 | 5 => Sub15ListSource::Abilities,
        3 | 6 => Sub15ListSource::Magic,
        4 | 7 => Sub15ListSource::Third,
        _ => Sub15ListSource::None,
    }
}

/// Row count for one frame of sub-screen `0x15`.
///
/// `record` is the character's `0x414`-byte record and `raseru_slot` is the
/// index the `0x8007B424 + char*2` halfword table supplies for that
/// character. A short `record` yields `0` rather than panicking.
///
/// The Magic gate is the load-bearing part: retail reads
/// `record[0x196 + raseru_slot]` **first** and only reads the spell count
/// when that byte is non-zero, so a character with spells but no Ra-Seru
/// equipped reports an empty list and takes the reject arm below - the same
/// gate the Magic caster picker applies (`FUN_801D9110`).
///
/// PORT: FUN_801DA2A0 (`0x801DA550..0x801DA64C`)
///
/// Wired through `legaia_engine_menu_runtime::field_menu_dispatch::apply_list_order_outcome`,
/// which re-derives the live length from the record before it replays the
/// page's exchanges - the record is the authority on how long the list is,
/// and the Ra-Seru gate is why a caster with spells can still report none.
pub fn sub15_list_len(step: u8, record: &[u8], raseru_slot: usize) -> u8 {
    let byte = |off: usize| record.get(off).copied().unwrap_or(0);
    match sub15_list_source(step) {
        Sub15ListSource::Abilities => {
            let mut n = 0u8;
            for bit in 0..SUB15_ABILITY_BIT_COUNT {
                let word_off = sub15_record::ABILITY_BITS + (bit >> 5) * 4;
                let word = record
                    .get(word_off..word_off + 4)
                    .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
                    .unwrap_or(0);
                if word & (1 << (bit & 0x1F)) != 0 {
                    n = n.saturating_add(1);
                }
            }
            n
        }
        Sub15ListSource::Magic => {
            if byte(sub15_record::EQUIP + raseru_slot) == 0 {
                0
            } else {
                byte(sub15_record::SPELL_COUNT)
            }
        }
        Sub15ListSource::Third => byte(sub15_record::THIRD_COUNT),
        Sub15ListSource::None => 0,
    }
}

/// What one frame of sub-screen `0x15`'s list half does once the row count
/// is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sub15Frame {
    /// `len == 0`: retail buzzes `0x23`, drops the step back to `1` (the
    /// character picker) and masks both cursors back to their index bits.
    Reject,
    /// `step < 5`: the settle frame. Retail adds **three** to the step,
    /// which is what turns steps `2`/`3`/`4` into their running twins
    /// `5`/`6`/`7`, and runs nothing else this frame.
    Settle { next_step: u8 },
    /// `step >= 5`: run the row picker over `len` rows.
    Browse { rows: u8 },
}

/// One frame of sub-screen `0x15` after the row count, the arm chain at
/// `0x801DA650..0x801DA6D0`.
///
/// Two details the shape depends on. The reject test runs **before** the
/// settle test, so an empty list is rejected on the settle frame rather
/// than one frame later. And the row picker is driven with mode `0`
/// (`FUN_801D688C(&_DAT_8007BB98, len, 0)`) - the only clamping picker in
/// this overlay; every other call site in the save UI passes `1` and wraps.
///
/// PORT: FUN_801DA2A0 (`0x801DA650..0x801DA6D0`)
///
/// Wired: [`crate::list_order::ListOrderSession::open`] runs this before it
/// opens the page, so an empty list takes the reject arm and no page opens
/// rather than a page opening empty.
pub fn sub15_frame(step: u8, len: u8) -> Sub15Frame {
    if len == 0 {
        return Sub15Frame::Reject;
    }
    if step < 5 {
        return Sub15Frame::Settle {
            next_step: step + 3,
        };
    }
    Sub15Frame::Browse { rows: len }
}

/// The row swap the screen commits: exchange rows `a` and `b` of the
/// character's list across all three parallel arrays at once - the id byte
/// at `+0x13D`, its companion byte at `+0x161`, and the word at `+0x08`.
///
/// Retail performs the three exchanges through a scratch byte each, in that
/// order, and raises flag `0x1000` on the second cursor as it commits. A
/// row index outside the record is skipped rather than clamped.
///
/// PORT: FUN_801DA2A0 (`0x801DA768..0x801DA844`)
///
/// Wired: the page ([`crate::list_order::ListOrderSession`]) records each
/// exchange it makes and
/// `legaia_engine_menu_runtime::field_menu_dispatch::apply_list_order_outcome` replays them
/// here against the character's own bytes when the page closes. The page
/// permutes a copy so a cancelled visit changes nothing; this is the only
/// thing that touches the record.
///
/// What the permutation is worth: `build_spell_session` fills each caster's
/// rows from `member.spell_list()`, which reads exactly the `+0x13D` /
/// `+0x161` pair this swaps, **in record order**, so an exchange here is an
/// exchange on the Magic screen - `spell_swap_permutes_the_magic_screen_order`
/// in `field_menu_subsession_e2e` pins that.
pub fn sub15_swap_rows(record: &mut [u8], a: usize, b: usize) {
    if a == b {
        return;
    }
    for base in [sub15_record::SPELL_LIST, sub15_record::SPELL_AUX] {
        let (ia, ib) = (base + a, base + b);
        if ia < record.len() && ib < record.len() {
            record.swap(ia, ib);
        }
    }
    let (wa, wb) = (
        sub15_record::ROW_WORDS + a * 4,
        sub15_record::ROW_WORDS + b * 4,
    );
    if wa + 4 <= record.len() && wb + 4 <= record.len() {
        for k in 0..4 {
            record.swap(wa + k, wb + k);
        }
    }
}
