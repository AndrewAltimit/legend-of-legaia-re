//! The party cast trigger's **per-spell anim-pair lists** - two tables in the
//! battle overlay (PROT 0898) that `FUN_801DBF9C` reads for a spell id below
//! `0x25` when a party member's pre-cast wait expires.
//!
//! ## Layout
//!
//! * an **index** of one byte per spell id at `0x801F4E63 + id` (the reader
//!   forms `0x801F4E64 + id` and loads at `-1`, `0x801DBFAC..0x801DBFB4`);
//! * a **record** table at `0x801F4EDC`, `8` bytes per index
//!   (`sll v1,v1,3` at `0x801DBFC0`): `(anim, effect)` byte pairs, the list
//!   ending on an `anim` of `0xFF` (`0x801DBFD0` / `0x801DC02C`).
//!
//! The reader copies the pairs into the caster's action-parameter stream from
//! `+0x1E0` on (`+0x1DF + 1 + 2k` and `+0x1DF + 2 + 2k`) and closes it with
//! `0xFF` one past the last pair (`0x801DC048..0x801DC060`). Record `0` is the
//! empty list, so an id whose index byte is `0` stages nothing.
//!
//! A third table in the same image belongs to the **monster** pick
//! `FUN_801E9FD4`, which stages a cast's opening camera shot at `+0x1E1`,
//! behind its clip: case `7` for an id below `0x25`, otherwise the byte at
//! `0x801F66D8 + id - 0x25` (`0x801EA548..0x801EA574`). The byte is a case of
//! the cast-effect driver `FUN_801DC0A0`
//! (`legaia_engine_vm::battle_cam_script::spell_cam_case`).
//!
//! The tables are disc data read off the user's own image; nothing here
//! carries their bytes. Consumers: `legaia_engine_core`'s
//! `spell_anim_trigger` and the monster cast staging.

/// Overlay VA of the per-id index (`id` `0` reads one byte before the base the
/// reader forms).
pub const INDEX_VA: u32 = 0x801F_4E63;
/// Overlay VA of the 8-byte pair records.
pub const RECORD_VA: u32 = 0x801F_4EDC;
/// Bytes per record.
pub const RECORD_STRIDE: usize = 8;
/// First spell id the trigger's other (summon) arm takes (`sltiu a1,0x25`).
pub const LIST_ID_END: u8 = 0x25;
/// The list terminator.
pub const END: u8 = 0xFF;
/// Overlay VA of the monster casts' opening-shot table, indexed `id - 0x25`
/// (`addiu v0,v0,0x66d8` / `lbu v0,-0x25(v1)` at `0x801EA560..0x801EA568`).
pub const OPENING_SHOT_VA: u32 = 0x801F_66D8;
/// Spell ids the opening-shot table covers: `0x25` up to the player Seru
/// block, the ids a monster pick stages (`0x25..0x81`).
pub const OPENING_SHOT_IDS: std::ops::Range<u8> = LIST_ID_END..0x81;

/// The pair lists for spell ids `0..0x25`, read off one battle-overlay image.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpellAnimPairs {
    lists: Vec<Vec<(u8, u8)>>,
    opening_shots: Vec<u8>,
}

impl SpellAnimPairs {
    /// Parse from PROT 0898 as loaded at `base_va`. An image too short to
    /// hold the tables yields the empty set.
    pub fn parse(bytes: &[u8], base_va: u32) -> Self {
        let at = |va: u32| va.checked_sub(base_va).map(|o| o as usize);
        let (Some(index), Some(records)) = (at(INDEX_VA), at(RECORD_VA)) else {
            return Self::default();
        };
        let mut lists = Vec::with_capacity(usize::from(LIST_ID_END));
        for id in 0..usize::from(LIST_ID_END) {
            let Some(&rec) = bytes.get(index + id) else {
                return Self::default();
            };
            let start = records + usize::from(rec) * RECORD_STRIDE;
            let mut pairs = Vec::new();
            let mut p = start;
            // The reader has no bound but the terminator; the port stops at
            // the record's own eight bytes.
            while p + 1 < start + RECORD_STRIDE {
                match (bytes.get(p), bytes.get(p + 1)) {
                    (Some(&a), Some(&e)) if a != END => pairs.push((a, e)),
                    _ => break,
                }
                p += 2;
            }
            lists.push(pairs);
        }
        let opening_shots = at(OPENING_SHOT_VA)
            .and_then(|o| bytes.get(o..o + OPENING_SHOT_IDS.len()))
            .map(<[u8]>::to_vec)
            .unwrap_or_default();
        Self {
            lists,
            opening_shots,
        }
    }

    /// The opening camera shot the monster pick stages at `+0x1E1` for
    /// `spell_id`: `7` below `0x25`, the table's byte above. `None` past the
    /// table or when it was not read.
    pub fn opening_shot(&self, spell_id: u8) -> Option<u8> {
        if spell_id < LIST_ID_END {
            return Some(7);
        }
        self.opening_shots
            .get(usize::from(spell_id - LIST_ID_END))
            .copied()
    }

    /// The `(anim, effect)` pairs for `spell_id`, or `None` for an id the
    /// summon arm takes or a table that was not read.
    pub fn pairs(&self, spell_id: u8) -> Option<&[(u8, u8)]> {
        self.lists.get(usize::from(spell_id)).map(Vec::as_slice)
    }

    /// `true` when nothing was read.
    pub fn is_empty(&self) -> bool {
        self.lists.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_then_record_then_terminator() {
        let base = 0x801C_E818;
        let mut b = vec![0u8; (RECORD_VA - base) as usize + 0x100];
        let idx = (INDEX_VA - base) as usize;
        let rec = (RECORD_VA - base) as usize;
        b[idx + 5] = 2; // id 5 -> record 2
        b[rec..rec + 2].copy_from_slice(&[END, 0]); // record 0: empty
        b[rec + 16..rec + 22].copy_from_slice(&[0x20, 0x00, 0x21, 0x10, END, 0]);
        let t = SpellAnimPairs::parse(&b, base);
        assert_eq!(t.pairs(5), Some(&[(0x20, 0x00), (0x21, 0x10)][..]));
        assert_eq!(t.pairs(1), Some(&[][..]));
        assert_eq!(t.pairs(LIST_ID_END), None);
        assert!(SpellAnimPairs::parse(&b[..16], base).is_empty());
    }

    #[test]
    fn opening_shot_is_seven_below_the_table_and_the_byte_above() {
        let base = 0x801C_E818;
        let mut b = vec![0u8; (OPENING_SHOT_VA - base) as usize + 0x60];
        let o = (OPENING_SHOT_VA - base) as usize;
        b[o + (0x27 - 0x25)] = 0x07;
        b[o + (0x2A - 0x25)] = 0x0C;
        let t = SpellAnimPairs::parse(&b, base);
        assert_eq!(t.opening_shot(0x10), Some(7));
        assert_eq!(t.opening_shot(0x27), Some(7));
        assert_eq!(t.opening_shot(0x2A), Some(0x0C));
        assert_eq!(t.opening_shot(0x81), None);
        // An image too short for the table answers nothing above `0x25`.
        let short = SpellAnimPairs::parse(&b[..o], base);
        assert_eq!(short.opening_shot(0x27), None);
    }
}
