//! Spell-record party broadcast (`FUN_8003053C`).
//!
//! The SCUS-resident entry point a spell cast goes through once its id is
//! known. It reads one 12-byte record out of the static spell-stats table
//! (`DAT_800754C8`, `docs/formats/spell-table.md`) and then branches on a
//! single flag bit to decide **how many targets the record's effect is
//! applied to**:
//!
//! * flag byte `+0x2` bit `0x20` set - the record is applied exactly once,
//!   with target slot `0`, and the applier's own return value is the result.
//! * bit clear - the record is applied once per live party slot, walking the
//!   roster count / id bytes that sit at `+0x454` / `+0x458..` of the
//!   `0x80084140` save block. Slots whose id byte is `>= 3` are skipped, and
//!   the result is "did any application report a hit", not a count.
//!
//! The two record bytes `+0x0` / `+0x1` are passed straight through to the
//! effect applier (`FUN_8003FB10`) as its first two arguments; this module
//! does not model what they mean, only that they are forwarded verbatim.
//!
//! Ported from the disassembly in `ghidra/scripts/funcs/8003053c.txt`; the
//! record geometry is the same one [`crate::retail_magic`] and
//! `legaia_asset::spell_names` read.

/// Stride of one record in the static spell-stats table `DAT_800754C8`.
pub const SPELL_RECORD_STRIDE: usize = 12;

/// Flag bit in record byte `+0x2` that selects the single-application path.
pub const FLAG_SINGLE_APPLY: u8 = 0x20;

/// Roster byte offsets inside the `0x80084140` save block that the
/// multi-target path walks.
pub const ROSTER_COUNT_OFFSET: usize = 0x454;
/// First roster id byte; slot `i` is at `ROSTER_IDS_OFFSET + i`.
pub const ROSTER_IDS_OFFSET: usize = 0x458;

/// Roster id values `>= ROSTER_ID_LIMIT` are skipped by the broadcast loop.
pub const ROSTER_ID_LIMIT: u8 = 3;

/// The three record bytes this dispatcher reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellDispatchRecord {
    /// Record byte `+0x0` - forwarded to the applier unchanged.
    pub arg0: u8,
    /// Record byte `+0x1` - forwarded to the applier unchanged.
    pub arg1: u8,
    /// Record byte `+0x2` - the flag byte tested for [`FLAG_SINGLE_APPLY`].
    pub flags: u8,
}

impl SpellDispatchRecord {
    /// `true` when the record takes the single-application path.
    pub fn is_single_apply(&self) -> bool {
        self.flags & FLAG_SINGLE_APPLY != 0
    }
}

/// Read record `spell_id` out of a raw 12-byte-stride spell-stats table.
///
/// The retail index is `(spell_id & 0xFF) * 12` with no bounds check; this
/// port keeps the masking and returns `None` rather than reading past the
/// end of the caller's slice.
pub fn record_at(table: &[u8], spell_id: u8) -> Option<SpellDispatchRecord> {
    let off = usize::from(spell_id) * SPELL_RECORD_STRIDE;
    let rec = table.get(off..off + 3)?;
    Some(SpellDispatchRecord {
        arg0: rec[0],
        arg1: rec[1],
        flags: rec[2],
    })
}

/// The roster the multi-target path walks: a count byte plus one id byte per
/// slot, as laid out at `0x80084140 + 0x454` / `+ 0x458`.
#[derive(Debug, Clone, Default)]
pub struct BroadcastRoster {
    /// Roster length byte at `+0x454`. `0` short-circuits the whole loop.
    pub count: u8,
    /// Slot id bytes from `+0x458` onward. Only the first `count` are read.
    pub slot_ids: Vec<u8>,
}

impl BroadcastRoster {
    /// Lift the roster out of a save-block slice based at `0x80084140`.
    pub fn from_save_block(block: &[u8]) -> Option<Self> {
        let count = *block.get(ROSTER_COUNT_OFFSET)?;
        let ids = block.get(ROSTER_IDS_OFFSET..ROSTER_IDS_OFFSET + usize::from(count))?;
        Some(Self {
            count,
            slot_ids: ids.to_vec(),
        })
    }
}

/// PORT: FUN_8003053c
///
/// Apply `rec` to one target or to every eligible roster slot.
///
/// `apply(arg0, arg1, target_slot)` stands in for `FUN_8003FB10`; a non-zero
/// return means "the application landed". The return value mirrors retail:
///
/// * single-apply path - the applier's own return value, verbatim;
/// * broadcast path - `1` when at least one application returned non-zero,
///   `0` otherwise (retail computes `sltu v0, zero, hits`, so the count is
///   collapsed to a boolean and is *not* the number of hits).
pub fn broadcast<F>(rec: SpellDispatchRecord, roster: &BroadcastRoster, mut apply: F) -> u32
where
    F: FnMut(u8, u8, u8) -> u32,
{
    if rec.is_single_apply() {
        return apply(rec.arg0, rec.arg1, 0);
    }
    let mut hits = 0u32;
    for i in 0..usize::from(roster.count) {
        let Some(&target) = roster.slot_ids.get(i) else {
            break;
        };
        if target >= ROSTER_ID_LIMIT {
            continue;
        }
        if apply(rec.arg0, rec.arg1, target) != 0 {
            hits += 1;
        }
    }
    u32::from(hits != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_with(id: u8, rec: [u8; 3]) -> Vec<u8> {
        let mut t = vec![0u8; 256 * SPELL_RECORD_STRIDE];
        let off = usize::from(id) * SPELL_RECORD_STRIDE;
        t[off..off + 3].copy_from_slice(&rec);
        t
    }

    #[test]
    fn record_indexing_is_twelve_byte_stride() {
        let t = table_with(0x81, [0x11, 0x22, 0x20]);
        let r = record_at(&t, 0x81).unwrap();
        assert_eq!(
            r,
            SpellDispatchRecord {
                arg0: 0x11,
                arg1: 0x22,
                flags: 0x20
            }
        );
        // Neighbours are untouched - the stride is 12, not 3 or 16.
        assert_eq!(record_at(&t, 0x80).unwrap().arg0, 0);
        assert_eq!(record_at(&t, 0x82).unwrap().arg0, 0);
    }

    #[test]
    fn single_apply_path_forwards_target_zero_and_returns_applier_value() {
        let rec = SpellDispatchRecord {
            arg0: 7,
            arg1: 9,
            flags: FLAG_SINGLE_APPLY,
        };
        let mut seen = Vec::new();
        let out = broadcast(rec, &BroadcastRoster::default(), |a, b, t| {
            seen.push((a, b, t));
            0x2A
        });
        assert_eq!(seen, vec![(7, 9, 0)]);
        // Verbatim, not collapsed to a boolean.
        assert_eq!(out, 0x2A);
    }

    #[test]
    fn single_apply_ignores_the_roster_entirely() {
        let rec = SpellDispatchRecord {
            arg0: 1,
            arg1: 2,
            flags: FLAG_SINGLE_APPLY | 0x01,
        };
        let roster = BroadcastRoster {
            count: 3,
            slot_ids: vec![0, 1, 2],
        };
        let mut calls = 0;
        broadcast(rec, &roster, |_, _, _| {
            calls += 1;
            0
        });
        assert_eq!(calls, 1);
    }

    #[test]
    fn broadcast_skips_ids_at_or_above_the_limit() {
        let rec = SpellDispatchRecord {
            arg0: 3,
            arg1: 4,
            flags: 0,
        };
        let roster = BroadcastRoster {
            count: 4,
            slot_ids: vec![0, 5, 2, 3],
        };
        let mut seen = Vec::new();
        let out = broadcast(rec, &roster, |a, b, t| {
            seen.push((a, b, t));
            0
        });
        assert_eq!(seen, vec![(3, 4, 0), (3, 4, 2)]);
        assert_eq!(out, 0);
    }

    #[test]
    fn broadcast_result_is_a_boolean_not_a_hit_count() {
        let rec = SpellDispatchRecord {
            arg0: 0,
            arg1: 0,
            flags: 0,
        };
        let roster = BroadcastRoster {
            count: 3,
            slot_ids: vec![0, 1, 2],
        };
        assert_eq!(broadcast(rec, &roster, |_, _, _| 1), 1);
        assert_eq!(broadcast(rec, &roster, |_, _, t| u32::from(t == 1)), 1);
        assert_eq!(broadcast(rec, &roster, |_, _, _| 0), 0);
    }

    #[test]
    fn empty_roster_returns_zero_without_applying() {
        let rec = SpellDispatchRecord {
            arg0: 0,
            arg1: 0,
            flags: 0,
        };
        let mut calls = 0;
        let out = broadcast(rec, &BroadcastRoster::default(), |_, _, _| {
            calls += 1;
            1
        });
        assert_eq!((calls, out), (0, 0));
    }

    #[test]
    fn roster_lifts_from_a_save_block_slice() {
        let mut block = vec![0u8; 0x600];
        block[ROSTER_COUNT_OFFSET] = 3;
        block[ROSTER_IDS_OFFSET] = 2;
        block[ROSTER_IDS_OFFSET + 1] = 0;
        block[ROSTER_IDS_OFFSET + 2] = 1;
        let r = BroadcastRoster::from_save_block(&block).unwrap();
        assert_eq!(r.count, 3);
        assert_eq!(r.slot_ids, vec![2, 0, 1]);
    }
}
