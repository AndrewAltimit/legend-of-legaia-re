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
//! (`801f3990`/`FUN_801dd0ac` also read `+0` as the half `>> 1` and full `>> 0`
//! values for the same move, so `+0` is the base power used at full / half /
//! quarter scale.)
//!
//! ## `param_1` is a *mapped* index, not the raw move id
//!
//! The kernel's `param_1` is not the battle move id directly — it is looked up
//! through a 128-byte **id → index map** immediately before the table at
//! [`MOVE_ID_INDEX_MAP_VA`] (`0x801F4E63`, raw-entry offset
//! [`MOVE_ID_INDEX_MAP_FILE_OFFSET`]). The setup site passes
//! `param_1 = map[actor[+0x1df]]` (`FUN_801dd0ac(*(byte*)(actor+0x1df) +
//! 0x801F4E63, …)` in `overlay_battle_action_801e09f8`). The map covers move ids
//! `0x00..=0x7F` and resolves the ids `0x04..=0x74` to power indices `0x01..=0x2b`
//! (a `0x00` entry = the unused record 0, `0xFF` = a no-record sentinel). So the
//! full resolution is `power_table[map[move_id]]` — see [`index_for_move_id`] /
//! [`record_for_move_id`]. The map is static (byte-identical across the same two
//! battle save states) and sits exactly `0x80` bytes before the 8-byte-record
//! table at `0x801F4EE3`.
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
//! The clean 26-byte-record structure holds for [`MOVE_POWER_TABLE_LEN`] entries
//! (indices `0..=43`; index 0 is an all-zero/unused slot); past it the region
//! transitions to other battle-overlay data (a float/transform table, then the
//! `data\battle\summon.DAT` / `readef.DAT` filename strings).
//!
//! Decoded record fields (each code-traced to a battle-action reader):
//! - `+0x00` `i16` **power** — the roll modulus (`FUN_801dd0ac` / `801f3990`).
//! - `+0x04` `u16` — copied to a per-actor counter at `ctx+0x6c6` that the action
//!   SM decrements (`FUN_801dea50` seeds it, `801e09f8` ticks it): the move's
//!   timing window. Exposed as [`MoveRecord::counter_init`].
//! - `+0x0d` `u8` — a **sound / voice cue id** the SM hands to the cue dispatcher
//!   `FUN_8004fcc8` (`801e09f8`). Exposed as [`MoveRecord::sound_cue_id`].
//!
//! Still open: the secondary u16 at `+0x02`, the `+0x08` flag halfword
//! (`0x0120`/`0x0020`/…), the small `+0x0a`/`+0x0b` field (the SM's most-read,
//! 8×), the `+0x0c` category byte (`C`/`E`/`G`/`0x00`), and the `+0x0e`/`+0x12`/
//! `+0x16` fields the SM reads — meanings TBD. See
//! `docs/reference/open-rev-eng-threads.md`.

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

/// Runtime VA of the id → power-index map (`0x80` bytes immediately before the
/// power table). `FUN_801dd0ac`'s `param_1` = `map[actor[+0x1df]]`.
pub const MOVE_ID_INDEX_MAP_VA: u32 = 0x801F_4E63;

/// Raw-entry file offset of the id → index map within PROT 0898
/// (= [`MOVE_POWER_TABLE_FILE_OFFSET`] − `0xF9`).
pub const MOVE_ID_INDEX_MAP_FILE_OFFSET: usize = MOVE_POWER_TABLE_FILE_OFFSET - 0xF9;

/// Length of the id → index map: move ids `0x00..=0x7F`.
pub const MOVE_ID_INDEX_MAP_LEN: usize = 0x80;

/// Map byte meaning "this move id has no power record" (the kernel never indexes
/// the table with it).
pub const MOVE_ID_INDEX_NONE: u8 = 0xFF;

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

    /// `+0x04` `u16` — the move's timing-window counter the action SM seeds at
    /// `ctx+0x6c6` and decrements (`FUN_801dea50` → `801e09f8`).
    pub fn counter_init(&self) -> u16 {
        u16::from_le_bytes([self.raw[4], self.raw[5]])
    }

    /// `+0x0d` `u8` — the move's sound / voice cue id, handed to the cue
    /// dispatcher `FUN_8004fcc8` by the action SM (`801e09f8`).
    pub fn sound_cue_id(&self) -> u8 {
        self.raw[0x0d]
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

/// Read the 128-byte id → power-index map out of the raw PROT 0898 entry. Each
/// byte `map[move_id]` is the power-table index the kernel uses for that battle
/// move id (`actor[+0x1df]`); [`MOVE_ID_INDEX_NONE`] (`0xFF`) and `0` mean "no
/// power record". Returns `None` if the slice is too short or the structural
/// guard fails (`map[4] == 1`, the first mapped id).
pub fn parse_id_index_map(battle_overlay_0898: &[u8]) -> Option<[u8; MOVE_ID_INDEX_MAP_LEN]> {
    let end = MOVE_ID_INDEX_MAP_FILE_OFFSET + MOVE_ID_INDEX_MAP_LEN;
    if end > battle_overlay_0898.len() {
        return None;
    }
    let mut map = [0u8; MOVE_ID_INDEX_MAP_LEN];
    map.copy_from_slice(&battle_overlay_0898[MOVE_ID_INDEX_MAP_FILE_OFFSET..end]);
    // Guard: move id 4 is the first mapped move (-> power index 1).
    if map[4] != 1 {
        return None;
    }
    Some(map)
}

/// Resolve a battle move id (`actor[+0x1df]`) to its power-table index via the
/// id → index map. Returns `None` for ids out of the map range or mapped to a
/// "no record" sentinel (`0` or `0xFF`).
pub fn index_for_move_id(map: &[u8; MOVE_ID_INDEX_MAP_LEN], move_id: u8) -> Option<u8> {
    let idx = *map.get(move_id as usize)?;
    if idx == 0 || idx == MOVE_ID_INDEX_NONE {
        None
    } else {
        Some(idx)
    }
}

/// Resolve a battle move id straight to its [`MoveRecord`] (map lookup + table
/// index). `table` is the [`parse`] output, `map` the [`parse_id_index_map`]
/// output.
pub fn record_for_move_id<'a>(
    table: &'a [MoveRecord],
    map: &[u8; MOVE_ID_INDEX_MAP_LEN],
    move_id: u8,
) -> Option<&'a MoveRecord> {
    let idx = index_for_move_id(map, move_id)? as usize;
    table.get(idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_shift_matches_kernel() {
        // 0x02ee = 750 -> >>2 = 187 (the kernel's roll modulus base).
        let mut raw = [0u8; MOVE_POWER_RECORD_STRIDE];
        raw[4] = 0xc8; // +0x04 counter_init = 0x00c8
        raw[0x0d] = 0x4b; // +0x0d sound cue = 0x4b
        let r = MoveRecord {
            index: 1,
            power_raw: 0x02ee,
            raw,
        };
        assert_eq!(r.power(), 187);
        assert_eq!(r.counter_init(), 0x00c8);
        assert_eq!(r.sound_cue_id(), 0x4b);
        // Sign preserved (arithmetic shift).
        let n = MoveRecord {
            index: 0,
            power_raw: -32,
            raw: [0; MOVE_POWER_RECORD_STRIDE],
        };
        assert_eq!(n.power(), -8);
    }

    #[test]
    fn id_index_map_resolves_move_ids() {
        // Synthetic 0898-shaped buffer: map at its offset, full table after it.
        let mut buf = vec![
            0u8;
            MOVE_POWER_TABLE_FILE_OFFSET
                + MOVE_POWER_RECORD_STRIDE * MOVE_POWER_TABLE_LEN
        ];
        // map[4] = 1 (the guard + first mapped move), map[5] = 2, map[0x10] = 0xff.
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 4] = 1;
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 5] = 2;
        buf[MOVE_ID_INDEX_MAP_FILE_OFFSET + 0x10] = MOVE_ID_INDEX_NONE;
        // table record 1 power 0x02ee, record 2 power 0x09c4.
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE] = 0xee;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE + 1] = 0x02;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * 2] = 0xc4;
        buf[MOVE_POWER_TABLE_FILE_OFFSET + MOVE_POWER_RECORD_STRIDE * 2 + 1] = 0x09;

        let map = parse_id_index_map(&buf).expect("map parses");
        let table = parse(&buf).expect("table parses");
        assert_eq!(index_for_move_id(&map, 4), Some(1));
        assert_eq!(index_for_move_id(&map, 5), Some(2));
        assert_eq!(index_for_move_id(&map, 0), None); // map[0] == 0 -> no record
        assert_eq!(index_for_move_id(&map, 0x10), None); // 0xff sentinel
        assert_eq!(
            record_for_move_id(&table, &map, 4).map(|r| r.power()),
            Some(187)
        );
        assert_eq!(
            record_for_move_id(&table, &map, 5).map(|r| r.power()),
            Some(625)
        );
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
