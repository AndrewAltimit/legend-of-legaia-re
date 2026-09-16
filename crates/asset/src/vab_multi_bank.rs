//! Multi-bank VAB archive - `monster.snd`, PROT extraction 891.
//!
//! One entry holding 206 independent sound banks, addressed by a sector index
//! table in its own first sector. The per-monster SE bank is selected by the
//! monster's id, so the archive is streamed a bank at a time rather than
//! loaded whole (it is 5.7 MB).
//!
//! ### The index table, from its reader
//!
//! `FUN_8003E104(bank, slot, dest)` is the consumer, and every field of the
//! head comes out of its address arithmetic (`see
//! ghidra/scripts/funcs/8003e104.txt`):
//!
//! ```text
//! 8003e110  lui   v0,0x801d
//! 8003e118  addiu s2,v0,-0x7680   ; s2 = 0x801C8980, the resident table
//! 8003e130  lw    a2,0x4(s2)      ; a2 = count
//! 8003e138  sltu  v0,s1,a2        ; bank < count, else "ERR monster over"
//! 8003e198  addiu v1,s1,0x3       ; &s2[bank + 3]
//! 8003e1a4  addiu v0,s1,0x2       ; &s2[bank + 2]
//! 8003e1c8  lw    v1,0x0(v1)      ; end   = table[bank + 1]
//! 8003e1cc  lw    s0,0x0(v0)      ; start = table[bank]
//! 8003e1dc  subu  s1,v1,s0        ; sectors = end - start
//! 8003e25c  sll   s1,s1,0xb       ; byte length  = sectors * 0x800
//! 8003e28c  sll   s4,s0,0xb       ; byte offset  = start   * 0x800
//! ```
//!
//! So the head is `[u32 reserved][u32 count][u32 start_sector[count + 1]]`:
//! the reader indexes the table from word 2 and reads **`count + 1`** words,
//! because bank `i` is bounded by its successor's start. The final word is the
//! archive's own sector count, exactly the way a PROT TOC entry's size is the
//! gap to the next entry ([`docs/formats/prot.md`](../../../../docs/formats/prot.md)).
//! Sectors are relative to the entry's own start, which the CD path confirms:
//! `FUN_8003E104` takes the entry's position (`li v0,0x37d` -> `gp+0x90C`,
//! raw TOC `0x37D` = extraction 891), converts it with the `CdPosToInt` /
//! `CdIntToPos` pair at `0x8003E1D8` / `0x8003E1E8` and advances it by `start`.
//!
//! The table is resident because the boot image stages it: PROT 0895 reads one
//! sector of entry `0x37D` and copies `0x400` bytes of it to `0x801C8980`
//! (`lui a0,0x801d` / `addiu a0,a0,-0x7680` / `jal 0x8001a8b0` with
//! `a2 = 0x400` at `0x801CEF74`..`0x801CEF84`, image
//! `extracted/overlays/overlay_boot_init_pak_0895.bin`). 1024 bytes covers the
//! whole `8 + 4 * (206 + 1)` = 836-byte table.
//!
//! ### A bank is a two-chunk DATA_FIELD stream, not a bare VAB
//!
//! Each bank starts on its sector and is a [`crate::data_field`]-shaped chunk
//! stream, terminated by a zero word and padded to the sector boundary:
//!
//! | Chunk | Header | Payload |
//! |---|---|---|
//! | 0 | `(0x00 << 24) \| header_len` | the VAB's header part: `VabHdr`, the 128-slot program table, the tone rows, the 256-slot VAG size table |
//! | 1 | `(0x01 << 24) \| body_len` | the VAG bodies |
//!
//! `header_len + body_len == VabHdr.fsize` in all 206 banks, and
//! `header_len == 0x20 + 0x800 + 0x200 * programs + 0x200` - the PsyQ header
//! part. The two halves are therefore **not contiguous**: the chunk-1 header
//! sits between the VAG size table and the first VAG body, which is why
//! `legaia_vab::parse(buf, bank + 4)` places every VAG body four bytes early.
//! Use [`Bank::body_offset`] for the bodies.
//!
//! Everything from the end of chunk 1 to the bank's declared end is slack the
//! container's own size math covers: the stream terminator, then whatever the
//! builder's sector buffer happened to hold. That is not padding in the
//! zero-fill sense - banks 61 and 62 carry bank 60's bytes from the same
//! buffer offset - and nothing reads past `fsize`.

use legaia_bytes::u32_le;

/// The archive's addressing unit. A bank always starts on a sector.
pub const SECTOR: usize = 0x800;

/// The extraction index of the one retail member of this class.
pub const MONSTER_SND_PROT_INDEX: usize = 891;

const VAB_MAGIC: &[u8; 4] = &[0x70, 0x42, 0x41, 0x56]; // 'pBAV' LE = VABp

/// One bank's extent, resolved to bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bank {
    pub index: usize,
    /// `table[index]` - the bank's first sector, relative to the entry start.
    pub start_sector: u32,
    /// `table[index + 1]` - the first sector of the next bank.
    pub end_sector: u32,
    /// Chunk-0 payload length: the VAB's header part.
    pub header_len: usize,
    /// Chunk-1 payload length: the VAG bodies.
    pub body_len: usize,
    /// `VabHdr.fsize`, which equals `header_len + body_len`.
    pub fsize: u32,
    pub programs: u16,
    pub vags: u16,
}

impl Bank {
    /// Byte offset of the bank's chunk 0 header.
    pub fn offset(&self) -> usize {
        self.start_sector as usize * SECTOR
    }
    /// Byte length of the bank's whole sector span, slack included.
    pub fn span(&self) -> usize {
        (self.end_sector - self.start_sector) as usize * SECTOR
    }
    /// Byte offset of the `pBAV` magic.
    pub fn vab_offset(&self) -> usize {
        self.offset() + 4
    }
    /// Byte offset of the chunk-1 header that separates the header part from
    /// the bodies.
    pub fn body_chunk_offset(&self) -> usize {
        self.vab_offset() + self.header_len
    }
    /// Byte offset of the first VAG body.
    pub fn body_offset(&self) -> usize {
        self.body_chunk_offset() + 4
    }
    /// One past the last byte the two chunks cover.
    pub fn content_end(&self) -> usize {
        self.body_offset() + self.body_len
    }
}

#[derive(Debug, Clone)]
pub struct VabMultiBank {
    pub count: usize,
    /// One entry per bank the index table resolves inside the buffer.
    pub banks: Vec<Bank>,
}

impl VabMultiBank {
    /// Byte extent of the index table itself, header word included.
    pub fn table_end(&self) -> usize {
        8 + 4 * (self.count + 1)
    }
}

/// Read the bank at `offset`, or `None` if it is not a two-chunk VAB stream.
fn read_bank(buf: &[u8], index: usize, start: u32, end: u32) -> Option<Bank> {
    let off = (start as usize).checked_mul(SECTOR)?;
    let span = (end.checked_sub(start)? as usize).checked_mul(SECTOR)?;
    if off.checked_add(span)? > buf.len() {
        return None;
    }
    let bank = buf.get(off..off + span)?;
    let chunk0 = u32_le(bank, 0)?;
    if chunk0 >> 24 != 0 {
        return None;
    }
    let header_len = (chunk0 & 0x00FF_FFFF) as usize;
    if bank.get(4..8)? != VAB_MAGIC {
        return None;
    }
    let fsize = u32_le(bank, 0x10)?;
    let programs = u16::from_le_bytes([*bank.get(0x16)?, *bank.get(0x17)?]);
    let vags = u16::from_le_bytes([*bank.get(0x1A)?, *bank.get(0x1B)?]);
    // The PsyQ header part: VabHdr, 128 program slots, 16 tone rows per
    // program, and the 256-slot VAG size table.
    let expect = 0x20 + 0x800 + 0x200 * programs as usize + 0x200;
    if header_len != expect {
        return None;
    }
    let chunk1 = u32_le(bank, 4 + header_len)?;
    if chunk1 >> 24 != 1 {
        return None;
    }
    let body_len = (chunk1 & 0x00FF_FFFF) as usize;
    if header_len + body_len != fsize as usize {
        return None;
    }
    if 8 + header_len + body_len > span {
        return None;
    }
    Some(Bank {
        index,
        start_sector: start,
        end_sector: end,
        header_len,
        body_len,
        fsize,
        programs,
        vags,
    })
}

pub fn detect(buf: &[u8]) -> Option<VabMultiBank> {
    if buf.len() < 12 {
        return None;
    }
    let reserved = u32_le(buf, 0)?;
    if reserved != 0 {
        return None;
    }
    let count = u32_le(buf, 4)? as usize;
    if !(4..=1024).contains(&count) {
        return None;
    }
    let header_end = 8usize.checked_add(count.checked_mul(4)?)?;
    if header_end > buf.len() {
        return None;
    }
    // Check that the first sector contains VABp magic at sector*0x800+4
    let first_sector = u32_le(buf, 8)? as usize;
    if first_sector == 0 {
        return None;
    }
    let vab_pos = first_sector.checked_mul(SECTOR)?.checked_add(4)?;
    if vab_pos + 4 > buf.len() {
        return None;
    }
    if &buf[vab_pos..vab_pos + 4] != VAB_MAGIC {
        return None;
    }

    // The reader indexes one past the last bank, so the table carries
    // `count + 1` words; where the successor word is missing (a truncated
    // buffer, or a synthetic one) the buffer's own sector count stands in, the
    // same way the archive's own terminator does.
    let sectors_in_buf = (buf.len() / SECTOR) as u32;
    let bound = |i: usize| -> Option<u32> {
        match u32_le(buf, 8 + 4 * i) {
            Some(v) if i <= count => Some(v),
            _ => None,
        }
    };
    let mut banks = Vec::new();
    for i in 0..count {
        let Some(start) = bound(i) else { break };
        let end = bound(i + 1).unwrap_or(sectors_in_buf);
        if end <= start {
            break;
        }
        match read_bank(buf, i, start, end) {
            Some(b) => banks.push(b),
            None => break,
        }
    }
    Some(VabMultiBank { count, banks })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic archive: `n` banks, each one sector, each a valid
    /// two-chunk stream around a one-program VAB.
    fn make_archive(n: usize) -> Vec<u8> {
        let header_len = 0x20 + 0x800 + 0x200 + 0x200; // one program
        let body_len = 0x10;
        // One bank needs 4 + header_len + 4 + body_len bytes; round to sectors.
        let bank_bytes: usize = 4 + header_len + 4 + body_len;
        let bank_sectors = bank_bytes.div_ceil(SECTOR);
        let total_sectors = 1 + n * bank_sectors;
        let mut buf = vec![0u8; total_sectors * SECTOR];
        buf[4..8].copy_from_slice(&(n as u32).to_le_bytes());
        for i in 0..=n {
            let sector = 1 + i * bank_sectors;
            buf[8 + 4 * i..12 + 4 * i].copy_from_slice(&(sector as u32).to_le_bytes());
        }
        for i in 0..n {
            let off = (1 + i * bank_sectors) * SECTOR;
            buf[off..off + 4].copy_from_slice(&(header_len as u32).to_le_bytes());
            buf[off + 4..off + 8].copy_from_slice(VAB_MAGIC);
            let fsize = (header_len + body_len) as u32;
            buf[off + 0x10..off + 0x14].copy_from_slice(&fsize.to_le_bytes());
            buf[off + 0x16..off + 0x18].copy_from_slice(&1u16.to_le_bytes());
            buf[off + 0x1A..off + 0x1C].copy_from_slice(&1u16.to_le_bytes());
            let c1 = off + 4 + header_len;
            buf[c1..c1 + 4].copy_from_slice(&(0x0100_0000 | body_len as u32).to_le_bytes());
        }
        buf
    }

    #[test]
    fn detects_and_resolves_every_bank() {
        let buf = make_archive(6);
        let r = detect(&buf).expect("synthetic archive detects");
        assert_eq!(r.count, 6);
        assert_eq!(r.banks.len(), 6);
        assert_eq!(r.table_end(), 8 + 4 * 7);
        for b in &r.banks {
            assert_eq!(&buf[b.vab_offset()..b.vab_offset() + 4], VAB_MAGIC);
            assert_eq!(b.header_len + b.body_len, b.fsize as usize);
            assert!(b.content_end() <= b.offset() + b.span());
        }
    }

    #[test]
    fn the_last_bank_is_bounded_by_the_terminator_word() {
        // `detect`'s count window starts at 4, so 4 is the smallest archive
        // this can be asked about.
        let buf = make_archive(4);
        let r = detect(&buf).unwrap();
        let last = *r.banks.last().unwrap();
        // The word after the last bank's start is the archive's own sector
        // count, so the last bank's span ends at the buffer end.
        assert_eq!(last.end_sector as usize * SECTOR, buf.len());
    }

    #[test]
    fn rejects_nonzero_reserved() {
        let mut buf = make_archive(4);
        buf[0] = 1;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_zero_first_sector() {
        let mut buf = make_archive(4);
        buf[8..12].copy_from_slice(&0u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_count_too_small() {
        let mut buf = make_archive(4);
        buf[4..8].copy_from_slice(&2u32.to_le_bytes());
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn rejects_wrong_magic() {
        let mut buf = make_archive(4);
        let vab_pos = SECTOR + 4;
        buf[vab_pos] = 0xFF;
        assert!(detect(&buf).is_none());
    }

    #[test]
    fn a_bank_whose_chunk1_header_is_missing_stops_the_walk() {
        let mut buf = make_archive(4);
        // Break bank 1's chunk-1 type byte; the walk keeps bank 0 only.
        let header_len = 0x20 + 0x800 + 0x200 + 0x200;
        let bank_bytes: usize = 4 + header_len + 4 + 0x10;
        let bank_sectors = bank_bytes.div_ceil(SECTOR);
        let c1 = (1 + bank_sectors) * SECTOR + 4 + header_len + 3;
        buf[c1] = 0x09;
        let r = detect(&buf).unwrap();
        assert_eq!(r.count, 4, "the header's count word is unchanged");
        assert_eq!(r.banks.len(), 1, "the bank walk stops at the broken bank");
    }
}
