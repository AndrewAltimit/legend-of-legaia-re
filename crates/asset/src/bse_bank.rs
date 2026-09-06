//! `bse.dat` - the battle half of the sound-effect descriptor table.
//!
//! This module recognises the **runtime SFX descriptor bank**: the rows that
//! back cue ids `>= 0x200`, in exactly the row format the static
//! `SCUS_942.54` table at `DAT_8006F198` uses for ids `< 0x200`
//! ([`crate::sfx_table`]). `bse.dat` is the carrier that named the class; the
//! field-mode occupant of the same role is a scene prescript's record 0.
//!
//! ### Identification (loader-grounded)
//!
//! `FUN_8001FA88` allocates a `0x1800`-byte buffer into `_DAT_8007B8D0`
//! (`jal 0x80017888` with `a1 = 0x1800`) and then fills it down one of two
//! branches on the dev/retail flag `_DAT_8007B8C2`:
//!
//! * **dev** - `a0 = 0x8007B3AC` (`lui a0,0x8008` / `addiu a0,a0,-0x4c54`), the
//!   `"bse.dat"` string in the sound-driver path cluster, passed to the
//!   path opener with `a1` = that buffer.
//! * **retail** - `byindex_sync_loader(0x37A, <same buffer>, 1)`
//!   (`li a0,0x37a` in the branch-delay slot at `0x8001FAD0`).
//!
//! Both branches write the **same destination**, so the dev file name and the
//! retail index name the same asset: raw TOC `0x37A` = **extraction entry 888**
//! (`resolver idx = extraction + 2`). `see ghidra/scripts/funcs/8001fa88.txt`.
//!
//! The size corroborates it. `byindex_sync_loader` resolves through
//! `FUN_8003E8A8`, whose returned sector count `TABLE[idx+3] - TABLE[idx+2]`
//! is the entry's size. Entry 888 is 2 sectors (4096 bytes), which fits the
//! `0x1800`-byte destination. The historical `toc[p+5] - toc[p+3] + 4`
//! expression gives 88 sectors there; loading that many would overrun the
//! buffer 43x over. See
//! [`docs/formats/prot.md`](../../../../docs/formats/prot.md) for why that
//! expression is not an entry's extent.
//!
//! **It is a battle load, not a boot load.** `FUN_8001FA88` has exactly one
//! caller anywhere on the disc - `jal 0x8001fa88` at `0x80051A3C` inside
//! battle init `FUN_800513F0`, itself called once from the battle-scene tick
//! `FUN_80046A20` at `0x80046F74` under the setup-phase gate `ctx[+0x11] == 0`.
//! This module previously said "loaded once at sound-init"; that rested on the
//! routine's shape, not on its caller.
//!
//! ### Layout
//!
//! ```text
//! +0x00   u16  tag            ; 1 in both retail carriers
//! +0x02   u16  body_offset    ; 4 - byte offset of the record table
//! +body   record[]            ; 8 bytes each; the walk ends at a record whose
//!                             ; first four bytes are zero
//! ```
//!
//! ### The bytes past the table are not this bank's
//!
//! Entry 888's table ends at `0x94C` and the 1,716 bytes after it are **PsyQ
//! `VagAtr` tone rows** - the 32-byte per-tone records of a VAB - left in the
//! sector by whatever occupied the disc slot before `bse.dat` overwrote its
//! head. They are byte-identical to entries 886 / 1063 at the *same* file
//! offsets, and every row sits on the `0x824 + k*0x20` grid a VAB laid out
//! from file offset 4 uses. Entry 1062 carries the same shape over entry
//! 1056's tone table. Nothing in those rows indexes this bank.
//!
//! One consequence for [`detect`]: entry 888 has **no authored terminator**.
//! The walk stops because the four bytes at `0x94C` are zero, and those are a
//! residue row's `vibW/vibT/porW/porT` field - structurally zero on every
//! retail tone, so the stop is reliable, but it is a foreign row's field.
//! Entry 1195 does carry a real zero trailer. See
//! `docs/formats/bse-dat.md`.
//!
//! The `+0x02` word is what the loader consumes, and it consumes it as a **byte
//! offset**, not a count: the tail of `FUN_8001FA88` computes
//! `gp[0x678] = base + ((s16)u16@+2 / 2) * 2` (`lhu v1,0x2(a0)`, sign-extend,
//! round toward zero, `>> 1`, `<< 1`) - a round-to-even of the offset. So
//! `gp[0x678]` is the record table's base pointer, and it is the pointer that
//! survives: `_DAT_8007B8D0` is a shared *current-bundle* slot the field
//! loader repoints on every scene load (`FUN_8001F7C0` `0x8001F864`).
//!
//! ### Record columns
//!
//! Each record is 8 bytes. The columns are the [`crate::sfx_table`] columns -
//! not by analogy but because one block of code decodes both tables:
//! `FUN_80016B6C` picks the arm at `0x80016C24` (`slti v0,s0,0x200`), resolves
//! either `0x8006F198 + id*8` or this bank's `record[id - 0x200]`, and falls
//! into the same field reads from `0x80016CB0` on.
//!
//! ```text
//! +0  u8   program    ; VAG / program index          -> FUN_80065034 arg 3
//! +1  u8   tone       ; ADSR-region base, +i per voice
//! +2  u8   level      ; note-level attribute, clusters on 60 (0x3C)
//! +3  u8   flags      ; low 5 bits = voice count; bit 0x20 = sustained
//! +4  u8   category   ; mixer record 0x80091508 + category*12; its +8 is the
//!                     ; VAB slot the cue keys, its +0xB the enable gate
//! +5  u8[3]           ; no runtime reader; zero in every retail row
//! ```
//!
//! The designer's own names, from the debug format string `FUN_80016B6C`
//! prints off `+0..+4`, are `p` / `t` / `l` / `n` / `id`.
//!
//! **Shape-name aliases.** This module used to name the columns for how they
//! behave, while no consumer of `gp[0x678]` had been traced: `a` = `program`,
//! `b` = `tone`, `key` = `level`, `flags` unchanged, and a `u32 v` that is
//! really `category` plus three unused bytes. Those names appear in older
//! notes; the mapping above is what they meant.
//!
//! ### Row index = cue id - `0x200`
//!
//! The battle cue router `FUN_8004FE5C` writes the **category** byte of the row
//! it is about to enqueue, through `gp[0x678]`, from a per-actor byte
//! (`*(0x801C9370[cat] + 0x22C) + 0x80`) or the literal `2`:
//! `sb v1,-0x31c(v0)` at `0x8004FFEC` and `sb v1,0x40c(v0)` at `0x80050084`,
//! both of which reduce to `gp[0x678] + ring_id*8 - 0x1000 + 4` =
//! `record[ring_id - 0x200] + 4`. The debug sound test in overlay 0971
//! confirms it without arithmetic: `0x801CEE44` stores `7` at
//! `0xDC(gp[0x678])` - byte `+4` of row 27 - and enqueues cue `0x21B`
//! (`0x200 + 27`). So the authored category is a **default** and a live cue's
//! VAB slot is chosen by the actor that fired it. Port of the router:
//! `legaia_engine_core::sfx_cue`.
//!
//! ### The two carriers
//!
//! | Extraction | Real extent | Records | Role |
//! |---|---:|---:|---|
//! | 888 | 4096 | 297 | `bse.dat`, the battle occupant (`FUN_8001FA88`, `0x37A`) |
//! | 1195 | 2048 | 7 | one scene block's prescript record 0 - the same format, per scene |
//!
//! Entry 1195 is `other1 + 2`, a prescript slot, and its whole payload is one
//! such bank: the four header bytes read equally as `[u16 tag][u16
//! body_offset = 4]` or `[u16 count = 1][u16 offsets[0] = 4]`, then 7 rows all
//! carrying `category = 2`, then zero fill. It is a scene's bank, not a second
//! `bse.dat` - so the detector matching it is correct, and the class name
//! means the format rather than the file.

/// Byte stride of one record.
pub const RECORD_BYTES: usize = 8;

/// Header word count before the record table.
pub const HEADER_BYTES: usize = 4;

/// Minimum records a buffer must carry to be recognised.
pub const DETECT_MIN_RECORDS: usize = 6;

/// Cue ids at or above this come from a runtime bank; below it they come from
/// the static `SCUS_942.54` table (`slti v0,s0,0x200` at `0x80016C24`).
/// Row index in this bank = `cue_id - CUE_ID_BASE`.
pub const CUE_ID_BASE: u16 = 0x200;

/// A recognised runtime SFX descriptor bank (`bse.dat` or a scene's record 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BseBank {
    /// The `u16` at `+0x00`.
    pub head_word: u16,
    /// The `u16` at `+0x02` - byte offset of the record table.
    pub body_offset: usize,
    /// Records before the all-zero terminator.
    pub records: usize,
}

/// One 8-byte descriptor, decoded. Column names are the designer's
/// (`p` / `t` / `l` / `n` / `id`); see the module docs for the shape-name
/// aliases this type replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BseRecord {
    /// `+0` `p` - program / VAG index.
    pub program: u8,
    /// `+1` `t` - ADSR-region base; voice `i` of a multi-voice cue uses
    /// `tone + i` (`addu a3,a3,s1` at `0x80016D6C`).
    pub tone: u8,
    /// `+2` `l` - note-level voice attribute; clusters on 60 (`0x3C`).
    pub level: u8,
    /// `+3` `n` - low 5 bits are the voice count, bit `0x20` selects the
    /// sustained key-on path.
    pub flags: u8,
    /// `+4` `id` - mixer/VAB category. Rewritten at cue time by the battle
    /// router, so the on-disc value is the authored default.
    pub category: u8,
}

impl BseRecord {
    /// Voices this cue keys on (`flags & 0x1F`, `andi v0,s4,0x1f` at
    /// `0x80016D00`).
    pub fn voice_count(self) -> u8 {
        self.flags & 0x1F
    }

    /// The sustained / continuous key-on path (`flags & 0x20`,
    /// `andi v0,s4,0x20` at `0x80016CF8`).
    pub fn sustained(self) -> bool {
        self.flags & 0x20 != 0
    }
}

fn u16_at(buf: &[u8], off: usize) -> Option<u16> {
    buf.get(off..off + 2)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
}

/// Recognise a runtime SFX descriptor bank.
pub fn detect(buf: &[u8]) -> Option<BseBank> {
    let head_word = u16_at(buf, 0)?;
    let body_offset = u16_at(buf, 2)? as usize;
    // The loader's own pointer arithmetic: the table starts right after the
    // 4-byte header in both carriers, and `head_word` is a small tag.
    if body_offset != HEADER_BYTES || !(1..=64).contains(&head_word) {
        return None;
    }
    let mut records = 0usize;
    loop {
        let at = body_offset + records * RECORD_BYTES;
        let Some(row) = buf.get(at..at + RECORD_BYTES) else {
            break;
        };
        if row[..4] == [0u8; 4] {
            break;
        }
        // `+4` is one category byte followed by three bytes no runtime reader
        // touches; retail leaves them zero in every row of both carriers. The
        // superseded gate spelled this as "the `u32` at `+4` is under 0x100"
        // (`DETECT_MAX_TAIL`), which is the same predicate once `+4` is read as
        // a byte: it can only ever fail on a non-zero trailer.
        if row[5..] != [0u8; 3] {
            return None;
        }
        records += 1;
    }
    (records >= DETECT_MIN_RECORDS).then_some(BseBank {
        head_word,
        body_offset,
        records,
    })
}

/// One record's raw bytes.
pub fn record(buf: &[u8], index: usize) -> Option<&[u8]> {
    let bank = detect(buf)?;
    (index < bank.records).then(|| {
        let at = bank.body_offset + index * RECORD_BYTES;
        &buf[at..at + RECORD_BYTES]
    })
}

/// One record, decoded into its pinned columns.
pub fn record_at(buf: &[u8], index: usize) -> Option<BseRecord> {
    let row = record(buf, index)?;
    Some(BseRecord {
        program: row[0],
        tone: row[1],
        level: row[2],
        flags: row[3],
        category: row[4],
    })
}

/// The descriptor a runtime cue id resolves to: row `cue_id - `[`CUE_ID_BASE`].
/// Returns `None` for a static-table id or one past the bank's end.
pub fn record_for_cue(buf: &[u8], cue_id: u16) -> Option<BseRecord> {
    let index = cue_id.checked_sub(CUE_ID_BASE)?;
    record_at(buf, usize::from(index))
}

/// Every record, decoded.
pub fn records(buf: &[u8]) -> Vec<BseRecord> {
    let Some(bank) = detect(buf) else {
        return Vec::new();
    };
    (0..bank.records)
        .filter_map(|i| record_at(buf, i))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth(rows: &[(u8, u8, u8, u8, u8)]) -> Vec<u8> {
        let mut buf = vec![1u8, 0, 4, 0];
        for &(p, t, l, n, id) in rows {
            buf.extend_from_slice(&[p, t, l, n, id, 0, 0, 0]);
        }
        buf.extend_from_slice(&[0u8; 8]); // terminator
        buf
    }

    #[test]
    fn detects_a_synthetic_bank() {
        let rows: Vec<_> = (0..10u8).map(|i| (0, i, 60 + i, 1, 2u8)).collect();
        let buf = synth(&rows);
        let bank = detect(&buf).expect("bank");
        assert_eq!(bank.head_word, 1);
        assert_eq!(bank.body_offset, 4);
        assert_eq!(bank.records, 10);
        assert_eq!(record(&buf, 0).unwrap()[2], 60);
        assert_eq!(record(&buf, 9).unwrap()[2], 69);
        assert!(record(&buf, 10).is_none());
    }

    #[test]
    fn decodes_the_pinned_columns() {
        let buf = synth(&[
            (5, 1, 61, 0x01, 0),
            (6, 0, 60, 0x22, 3),
            (7, 2, 62, 0x21, 0),
            (8, 0, 60, 0x02, 2),
            (9, 0, 60, 0x01, 0),
            (10, 0, 60, 0x01, 0),
        ]);
        let first = record_at(&buf, 0).expect("row 0");
        assert_eq!(first.program, 5);
        assert_eq!(first.tone, 1);
        assert_eq!(first.level, 61);
        assert_eq!(first.category, 0);
        assert_eq!(first.voice_count(), 1);
        assert!(!first.sustained());

        let second = record_at(&buf, 1).expect("row 1");
        assert_eq!(second.voice_count(), 2);
        assert!(second.sustained());
        assert_eq!(second.category, 3);

        assert_eq!(records(&buf).len(), 6);
    }

    #[test]
    fn cue_ids_index_from_0x200() {
        let rows: Vec<_> = (0..8u8).map(|i| (i, i, 60 + i, 1, 0u8)).collect();
        let buf = synth(&rows);
        // Row 0 is cue 0x200; row 7 is cue 0x207.
        assert_eq!(record_for_cue(&buf, 0x200), record_at(&buf, 0));
        assert_eq!(record_for_cue(&buf, 0x207).unwrap().program, 7);
        // A static-table id resolves to no runtime row at all.
        assert!(record_for_cue(&buf, 0x1FF).is_none());
        // Past the terminator.
        assert!(record_for_cue(&buf, 0x208).is_none());
    }

    #[test]
    fn rejects_a_wrong_body_offset() {
        let rows: Vec<_> = (0..10u8).map(|i| (0, i, 60, 1, 0u8)).collect();
        let mut buf = synth(&rows);
        buf[2] = 8;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_a_nonzero_trailer() {
        let rows: Vec<_> = (0..10u8).map(|i| (0, i, 60, 1, 0u8)).collect();
        let mut buf = synth(&rows);
        // Byte +6 of row 0: the second of the three bytes no reader touches.
        buf[HEADER_BYTES + 6] = 0x12;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_too_few_records() {
        let rows: Vec<_> = (0..3u8).map(|i| (0, i, 60, 1, 0u8)).collect();
        let buf = synth(&rows);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_zeros_and_short_buffers() {
        assert!(detect(&[]).is_none());
        assert!(detect(&vec![0u8; 4096]).is_none());
    }
}
