//! Battle-overlay per-move **power / parameter** table (runtime VA `0x801F4F5C`).
//!
//! The battle-action damage kernel `FUN_801dd0ac` (dumped at
//! `ghidra/scripts/funcs/overlay_battle_action_801dd0ac.txt`) has two branches,
//! selected by its attacker-slot argument `param_2`:
//!
//! - **summon branch** (`param_2 == 7`): the magnitude is derived from
//!   caster/summon battle state, not a static table (see
//!   [`crate::summon_overlay`] and `docs/formats/spell-table.md`).
//! - **non-summon branch** (`param_2 != 7`, the **arts / physical** path): the
//!   attacker roll's modulus is read from a fixed-stride table based at
//!   `0x801F4F5C`, indexed by the move-type byte `param_1`:
//!
//! ```text
//! 801dd19c  lui   a1,0x801f
//! 801dd1a0  addiu a1,a1,0x4f5c       ; a1 = table base 0x801F4F5C
//! 801dd1a4  andi  a0,s5,0xff         ; a0 = param_1 (move-type byte)
//! 801dd1a8..b8                       ; v1 = a0*26  (sll/addu chain: a0<<1+a0
//!                                    ;   <<2 +a0 <<1 = 26*a0)
//! 801dd1bc  addu  v1,v1,a1           ; v1 = &table[a0]
//! 801dd1c0  lhu   a1,0x0(v1)         ; a1 = u16 at record +0
//! 801dd1c8  sll   a1,a1,0x10
//! 801dd1cc  sra   v1,a1,0x12         ; v1 = (i16)power >> 2   (the roll modulus)
//! ```
//!
//! So each record is **26 bytes** and its first field (`+0`, signed 16-bit) is
//! the move's **power**, which the kernel uses as `rand % (((i16)power >> 2) + 1)`.
//!
//! ## Provenance — static overlay data, pinned on disc
//!
//! The table is **static** (loaded with the battle-action overlay image, not
//! built per-battle): the `0x801F4F5C..0x801F69D8` window is byte-identical
//! between two unrelated battle save states (a full-party Gobu Gobu fight and
//! the Tetsu-tutorial command menu). Its bytes live in **PROT entry 0898** (the
//! battle-action overlay, `overlay_0898` / `overlay_battle_action`) at raw-entry
//! file offset [`MOVE_POWER_TABLE_FILE_OFFSET`] — pinned by byte-matching the
//! raw PROT 0898 entry against the in-RAM table at VA `0x801F4F5C` (both the
//! table window and the `FUN_801dd0ac` code body map with one consistent base).
//!
//! ## Extent + open fields
//!
//! The clean 26-byte-record structure holds for roughly [`MOVE_POWER_TABLE_LEN`]
//! entries (move ids `0..` — id 0 is an all-zero/unused slot); past it the
//! region transitions to other battle-overlay data (a float/transform table,
//! then the `data\battle\summon.DAT` / `readef.DAT` filename strings). Only the
//! `+0` power field is decoded here — the remaining per-record halfwords (a
//! secondary u16 at `+2`, a flag halfword at `+8` (`0x0120`/`0x0020`/…), a small
//! flag at `+10`, a two-byte code at `+12` (an ASCII-range category/level pair,
//! e.g. `C`+`K`/`L`/`M` for the three lead records, `E`+`K`, or `0x00`+id), and
//! several trailing words) plus the precise `param_1` → move-id mapping are an
//! open battle-action thread; see `docs/reference/open-rev-eng-threads.md`.

/// CDNAME / PROT index of the battle-action overlay holding the table.
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize = 898;

/// Runtime virtual address the table is loaded to (the `FUN_801dd0ac` base).
pub const MOVE_POWER_TABLE_VA: u32 = 0x801F_4F5C;

/// Raw-entry file offset of the table within PROT 0898. Empirically pinned by
/// byte-matching the entry against the in-RAM table at [`MOVE_POWER_TABLE_VA`].
pub const MOVE_POWER_TABLE_FILE_OFFSET: usize = 0x26744;

/// Per-record stride (the `26*param_1` index math in `FUN_801dd0ac`).
pub const MOVE_POWER_RECORD_STRIDE: usize = 26;

/// Observed clean record count before the region transitions to other overlay
/// data. The intended count is a judgement (the structure degrades rather than
/// ending on an explicit sentinel); callers wanting only confirmed entries
/// should treat trailing empties as unused move ids.
pub const MOVE_POWER_TABLE_LEN: usize = 44;

/// One 26-byte move record. Only the `+0` power field is interpreted; the raw
/// bytes are retained for forward reference as the remaining fields are decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoveRecord {
    /// Move id = the record's index into the table (`param_1`).
    pub index: usize,
    /// The `+0` signed-16-bit field (`lhu` then sign-extended by the kernel).
    pub power_raw: i16,
    /// The full 26-byte record.
    pub raw: [u8; MOVE_POWER_RECORD_STRIDE],
}

impl MoveRecord {
    /// The roll-modulus base `FUN_801dd0ac` derives from `+0`: `(i16)power >> 2`
    /// (arithmetic shift — preserves sign).
    pub fn power(&self) -> i32 {
        (self.power_raw as i32) >> 2
    }

    /// `true` when the whole record is zero (an unused move-id slot).
    pub fn is_empty(&self) -> bool {
        self.raw.iter().all(|&b| b == 0)
    }
}

/// Parse `count` records from `bytes` starting at `offset`. Returns `None` when
/// the slice doesn't fit or the structural guard fails (record 0 must be the
/// all-zero unused slot and at least the first few real records must be
/// populated — a cheap check that the pinned offset still lands on the table).
pub fn parse_at(bytes: &[u8], offset: usize, count: usize) -> Option<Vec<MoveRecord>> {
    let end = offset.checked_add(count.checked_mul(MOVE_POWER_RECORD_STRIDE)?)?;
    if end > bytes.len() {
        return None;
    }
    let mut records = Vec::with_capacity(count);
    for i in 0..count {
        let base = offset + i * MOVE_POWER_RECORD_STRIDE;
        let mut raw = [0u8; MOVE_POWER_RECORD_STRIDE];
        raw.copy_from_slice(&bytes[base..base + MOVE_POWER_RECORD_STRIDE]);
        let power_raw = i16::from_le_bytes([raw[0], raw[1]]);
        records.push(MoveRecord {
            index: i,
            power_raw,
            raw,
        });
    }
    // Structural guard: id 0 is the unused all-zero slot; the table proper must
    // carry several populated records right after it.
    if !records[0].is_empty() {
        return None;
    }
    let populated = records
        .iter()
        .skip(1)
        .take(4)
        .filter(|r| !r.is_empty())
        .count();
    if populated == 0 {
        return None;
    }
    Some(records)
}

/// Parse the table out of the raw PROT 0898 (battle-action overlay) entry bytes
/// at the pinned offset + length.
pub fn parse(battle_overlay_0898: &[u8]) -> Option<Vec<MoveRecord>> {
    parse_at(
        battle_overlay_0898,
        MOVE_POWER_TABLE_FILE_OFFSET,
        MOVE_POWER_TABLE_LEN,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_shift_matches_kernel() {
        // 0x02ee = 750 -> >>2 = 187 (the kernel's roll modulus base).
        let r = MoveRecord {
            index: 1,
            power_raw: 0x02ee,
            raw: [0; MOVE_POWER_RECORD_STRIDE],
        };
        assert_eq!(r.power(), 187);
        // Sign preserved (arithmetic shift).
        let n = MoveRecord {
            index: 0,
            power_raw: -32,
            raw: [0; MOVE_POWER_RECORD_STRIDE],
        };
        assert_eq!(n.power(), -8);
    }

    #[test]
    fn parse_at_reads_stride_and_guards() {
        // Synthetic: record 0 empty, record 1 has power_raw 0x02ee.
        let mut buf = vec![0u8; 16 + MOVE_POWER_RECORD_STRIDE * 3];
        let off = 16;
        // record 1 (index 1) +0 = 0x02ee
        buf[off + MOVE_POWER_RECORD_STRIDE] = 0xee;
        buf[off + MOVE_POWER_RECORD_STRIDE + 1] = 0x02;
        let recs = parse_at(&buf, off, 3).expect("parses");
        assert_eq!(recs.len(), 3);
        assert!(recs[0].is_empty());
        assert_eq!(recs[1].power_raw, 0x02ee);
        assert_eq!(recs[1].power(), 187);

        // Guard: a non-empty record-0 (offset lands off the table) -> None.
        assert!(parse_at(&buf, off + 1, 3).is_none());
        // Guard: slice too short -> None.
        assert!(parse_at(&buf, buf.len() - 4, 2).is_none());
    }
}
