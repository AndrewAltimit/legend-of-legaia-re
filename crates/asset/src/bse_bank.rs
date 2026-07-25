//! `bse.dat` - the master sound bank loaded once at sound-init.
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
//! ### Layout
//!
//! ```text
//! +0x00   u16  ?              ; 1 in both retail samples
//! +0x02   u16  body_offset    ; 4 - byte offset of the record table
//! +body   record[]            ; 8 bytes each, terminated by an all-zero record
//! ```
//!
//! The `+0x02` word is what the loader consumes, and it consumes it as a **byte
//! offset**, not a count: the tail of `FUN_8001FA88` computes
//! `gp[0x678] = base + ((s16)u16@+2 / 2) * 2` (`lhu v1,0x2(a0)`, sign-extend,
//! round toward zero, `>> 1`, `<< 1`) - a round-to-even of the offset. So
//! `gp[0x678]` is the record table's base pointer.
//!
//! Each record is 8 bytes:
//!
//! ```text
//! +0  u8   a          ; walks 0,1,2,... across the table - a program index
//! +1  u8   b          ; small sub-index within `a`
//! +2  u8   key        ; clusters tightly on 60 (0x3C) and tracks `b`
//! +3  u8   flags      ; low bits 1/2, plus a 0x20 bit
//! +4  u32  v          ; small integer (0 or 2 in both samples)
//! ```
//!
//! **The record field names above are shape, not semantics.** `a`/`b`/`key`
//! are named for how the columns behave, and the obvious reading - a
//! `(program, tone, unity key)` triple, since `0x3C` = 60 is middle C and the
//! sibling [`sfx_table`](crate::sfx_table) descriptors are also 8-byte
//! program/tone records - is a **hypothesis**. No consumer of `gp[0x678]` has
//! been traced, so nothing here asserts what the columns mean. What is pinned
//! is the loader, the destination, the header word's use as a byte offset, and
//! the 8-byte stride.
//!
//! ### The two entries
//!
//! | Extraction | Real extent | Records | Reached by |
//! |---|---:|---:|---|
//! | 888 | 4096 | 297 | `FUN_8001FA88` retail branch (`0x37A`) |
//! | 1195 | 2048 | 7 | nothing in the dump corpus |
//!
//! Entry 1195 is the same format with a 7-record table, and its raw TOC index
//! `0x4AD` appears as a load literal in **no** dumped function - while its
//! block neighbours `0x4B0` / `0x4B1` do (the slot-machine assets). Since the
//! dump corpus is not complete, that is evidence of an unused sibling, not
//! proof; see the handoff.

/// Byte stride of one record.
pub const RECORD_BYTES: usize = 8;

/// Header word count before the record table.
pub const HEADER_BYTES: usize = 4;

/// Minimum records a buffer must carry to be recognised.
pub const DETECT_MIN_RECORDS: usize = 6;

/// Largest value the record's trailing `u32` may take. Both retail samples use
/// only 0 and 2; the gate is deliberately loose and still zero-false-positive.
pub const DETECT_MAX_TAIL: u32 = 0xFF;

/// A recognised `bse.dat`-shaped bank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BseBank {
    /// The `u16` at `+0x00`.
    pub head_word: u16,
    /// The `u16` at `+0x02` - byte offset of the record table.
    pub body_offset: usize,
    /// Records before the all-zero terminator.
    pub records: usize,
}

fn u16_at(buf: &[u8], off: usize) -> Option<u16> {
    buf.get(off..off + 2)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
}

fn u32_at(buf: &[u8], off: usize) -> Option<u32> {
    buf.get(off..off + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

/// Recognise a `bse.dat`-shaped bank.
pub fn detect(buf: &[u8]) -> Option<BseBank> {
    let head_word = u16_at(buf, 0)?;
    let body_offset = u16_at(buf, 2)? as usize;
    // The loader's own pointer arithmetic: the table starts right after the
    // 4-byte header in both samples, and `head_word` is a small tag.
    if body_offset != HEADER_BYTES || !(1..=64).contains(&head_word) {
        return None;
    }
    let mut records = 0usize;
    loop {
        let at = body_offset + records * RECORD_BYTES;
        let Some(quad) = buf.get(at..at + 4) else {
            break;
        };
        if buf.len() < at + RECORD_BYTES {
            break;
        }
        if quad == [0u8; 4] {
            break;
        }
        if u32_at(buf, at + 4)? > DETECT_MAX_TAIL {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn synth(records: &[(u8, u8, u8, u8, u32)]) -> Vec<u8> {
        let mut buf = vec![1u8, 0, 4, 0];
        for &(a, b, k, f, v) in records {
            buf.extend_from_slice(&[a, b, k, f]);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        buf.extend_from_slice(&[0u8; 8]); // terminator
        buf
    }

    #[test]
    fn detects_a_synthetic_bank() {
        let rows: Vec<_> = (0..10u8).map(|i| (0, i, 60 + i, 1, 2u32)).collect();
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
    fn rejects_a_wrong_body_offset() {
        let rows: Vec<_> = (0..10u8).map(|i| (0, i, 60, 1, 0u32)).collect();
        let mut buf = synth(&rows);
        buf[2] = 8;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_a_large_trailing_field() {
        let rows: Vec<_> = (0..10u8).map(|i| (0, i, 60, 1, 0x1234_5678u32)).collect();
        let buf = synth(&rows);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_too_few_records() {
        let rows: Vec<_> = (0..3u8).map(|i| (0, i, 60, 1, 0u32)).collect();
        let buf = synth(&rows);
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_zeros_and_short_buffers() {
        assert!(detect(&[]).is_none());
        assert!(detect(&vec![0u8; 4096]).is_none());
    }
}
