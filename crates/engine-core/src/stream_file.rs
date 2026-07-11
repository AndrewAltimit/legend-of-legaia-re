//! Dual-mode stream-file API - the side-band "file handle" layer the battle
//! streaming machinery uses to pull sector ranges out of `PROT.DAT` mid-play.
//!
//! PORT: FUN_800558FC
//! PORT: FUN_80055A5C
//! PORT: FUN_800559EC
//! PORT: FUN_80055AC8
//! PORT: FUN_8003E964
//!
//! ## Retail shape (per the dumps - `ghidra/scripts/funcs/800558fc.txt`,
//! `80055a5c.txt`, `800559ec.txt`, `80055ac8.txt`, `8003e964.txt`)
//!
//! Four SCUS shims form a classic `open`/`seek`/`read`/`close` file API over
//! two branches selected by the halfword `_DAT_8007B8C2`:
//!
//! - `_DAT_8007B8C2 == 0`: the dev-host path (`FUN_800608F0` /
//!   `FUN_80060920` / `FUN_80060944` / `FUN_80060910`) that opens the
//!   `data\battle\...` path string. On the retail build these are **trap
//!   stubs** and the flag is non-zero in every capture, so this branch is a
//!   debug-build referent only. Not ported.
//! - `_DAT_8007B8C2 != 0` (retail-effective): the path string is **ignored**
//!   and the API operates on the raw in-RAM PROT TOC at `0x801C70F0`
//!   (header-included copy of PROT.DAT's first sectors - raw index =
//!   extraction index + 2, see `docs/formats/cdname.md#numbering-space`).
//!
//! Retail-effective semantics, function by function:
//!
//! | Shim | Retail body |
//! |---|---|
//! | open  `FUN_800558FC(path, _, _, raw_idx)` | `FUN_8003E8A8(raw_idx, 1)`: stop any in-flight CD op, `start = toc_word[raw_idx+2]`, `next = toc_word[raw_idx+3]`, current MSF `0x8007BC5C` = PROT base MSF (`0x8007BC50`) + `start`, copy current -> saved-base MSF (`gp+0x95C`). The resolver returns `next - start` (size in sectors) but the shim clobbers `v0` with caller-saved `s0`, so the retail **return value is garbage** - callers treat the handle as opaque/meaningless. |
//! | seek  `FUN_80055A5C(fd, byte_off, whence)` | `FUN_8003E964(byte_off >> 11, whence & 0xFF)` - byte offset floors to whole 2048-byte sectors (sub-sector remainder silently dropped). |
//! | read  `FUN_800559EC(fd, dst, byte_len)` | `FUN_8003E800(dst, byte_len >> 11, 1)` - reads `floor(byte_len / 2048)` sectors at the current MSF (a `byte_len < 0x800` reads **zero** sectors). `fd` is unused. On completion the read-wait poll `FUN_8003DE7C` advances the current MSF by the sector count (`msf = lba_to_msf(msf_to_lba(msf) + gp[0x97C])` at `0x8003E034..0x8003E060`), so sequential reads walk forward. |
//! | close `FUN_80055AC8()` | **No-op** beyond the shared preamble - no handle state is released; the current/saved MSF cells survive close. |
//!
//! The seek helper `FUN_8003E964(sector_off, whence)`:
//!
//! ```text
//!   if whence == 0:                        // seek from file base
//!       cur_msf(0x8007BC5C) = saved_base_msf(gp+0x95C..0x95E)
//!   cur_msf = lba_to_msf(msf_to_lba(cur_msf) + sector_off)
//! ```
//!
//! i.e. `whence == 0` is SEEK_SET relative to the `FUN_8003E8A8`-saved entry
//! base and `whence != 0` is SEEK_CUR. There is **no EOF clamp** anywhere in
//! the chain - seeking or reading past the entry end just walks into the
//! following PROT.DAT sectors (the extraction pipeline's "over-read window").
//!
//! All four shims share a preamble over the deferred-op scratch: a pending
//! token at `_DAT_8007BD08` is parked into the retry slot `_DAT_8007BD38`
//! with a `0xB4`-tick timer at `_DAT_8007BD44`, then the pending cell is
//! cleared. The port mirrors that as [`StreamFileHost::park_pending_op`].
//!
//! ## Port model
//!
//! Retail has exactly **one implicit global cursor** (the MSF cell pair) -
//! there are no real handles. [`StreamFileHost`] models that: `open_raw` /
//! `open_extraction` re-aim the single cursor, `seek`/`read` move it, and
//! `close` is the retail no-op. Sector positions are kept PROT.DAT-relative
//! (the retail disc-absolute MSF = PROT base MSF + these values; the base
//! constant cancels out of every seek/read delta).
//!
//! Consumers (not wired here):
//! REF: FUN_801F17F8
//! REF: FUN_80052770
//!
//! The underlying primitives are already ported in [`crate::cd_dma`]
//! (`FUN_8003E8A8` size lookup / `FUN_8003E800` async load); this module
//! layers the file-API cursor semantics that `cd_dma`'s offline host does
//! not express (position-relative reads).

use std::sync::Arc;

use anyhow::{Context, Result, bail};

use crate::scene::ProtIndex;

/// PSX CD sector payload size (Mode 2 Form 1 = 2048 bytes).
pub const SECTOR_BYTES: u32 = 0x800;

/// `log2(SECTOR_BYTES)` - the `srl reg, 0xB` both the seek and read shims
/// apply to their byte arguments.
pub const SECTOR_SHIFT: u32 = 11;

/// Raw in-RAM PROT TOC index space (what `FUN_800558FC`'s fourth argument
/// carries; = extraction index + 2).
pub type RawTocIndex = u16;

/// Byte count / offset -> whole-sector count, flooring - the exact
/// `srl a1, 0xB` conversion in `FUN_80055A5C` (seek) and `FUN_800559EC`
/// (read). Sub-sector remainders are silently discarded: a 0x7FF-byte seek
/// does not move, a 0x7FF-byte read reads nothing.
pub const fn bytes_to_sectors_floor(bytes: u32) -> u32 {
    bytes >> SECTOR_SHIFT
}

/// Seek origin for [`StreamFileHost::seek`]. Mirrors the `whence` byte the
/// retail seek shim forwards to `FUN_8003E964` (`param_3 & 0xFF`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekWhence {
    /// `whence == 0`: rewind the cursor to the saved entry base first
    /// (the `gp+0x95C -> 0x8007BC5C` MSF copy), then advance.
    FromBase,
    /// `whence != 0`: advance from the current cursor position.
    /// The player-file loader `FUN_80052770` uses this mode (`li a2, 0x1`).
    FromCurrent,
}

impl SeekWhence {
    /// Decode the retail `whence` byte (`0` = base, non-zero = current).
    pub fn from_retail(byte: u8) -> Self {
        if byte == 0 {
            Self::FromBase
        } else {
            Self::FromCurrent
        }
    }
}

/// The single-cursor stream-file host. See the module docs for the retail
/// mapping. One instance models the SCUS gp-scratch cell block; `open`
/// re-aims the cursor rather than allocating a handle.
pub struct StreamFileHost {
    prot: Arc<ProtIndex>,
    /// Saved entry-base sector (PROT.DAT-relative). Mirrors the
    /// `gp+0x95C..0x95E` saved-base MSF that `FUN_8003E8A8` stashes and
    /// `FUN_8003E964`'s `whence == 0` branch restores.
    base_sector: Option<u32>,
    /// Current cursor sector (PROT.DAT-relative). Mirrors the MSF cell at
    /// `0x8007BC5C` (the one the libcd kick `FUN_8003F128` converts into
    /// the read start LBA).
    pos_sector: u32,
    /// Size in sectors of the opened entry (`toc[idx+3] - toc[idx+2]`, the
    /// `FUN_8003E8A8` return). Retail computes this and throws it away at
    /// the open shim; kept here for callers that want an EOF reference.
    /// Never used as a clamp - retail has none.
    size_sectors: u32,
    /// Deferred-op scratch mirroring `_DAT_8007BD08` (pending) /
    /// `_DAT_8007BD38` (parked) / `_DAT_8007BD44` (retry timer).
    pending_op: u32,
    parked_op: u32,
    parked_timer: u32,
}

impl StreamFileHost {
    /// Build a host over a shared PROT index. The cursor starts unopened;
    /// [`Self::seek`]/[`Self::read`] before an open fail (retail would walk
    /// from whatever stale MSF the cells held - surfacing an error is the
    /// clean-room tightening).
    pub fn new(prot: Arc<ProtIndex>) -> Self {
        Self {
            prot,
            base_sector: None,
            pos_sector: 0,
            size_sectors: 0,
            pending_op: 0,
            parked_op: 0,
            parked_timer: 0,
        }
    }

    /// Shared shim preamble: park any pending deferred-op token into the
    /// retry slot with the retail `0xB4`-tick timer, then clear the pending
    /// cell. All four retail shims run this before branching.
    fn park_pending_op(&mut self) {
        if self.pending_op != 0 {
            self.parked_op = self.pending_op;
            self.parked_timer = 0xB4;
        }
        self.pending_op = 0;
    }

    /// Stage a deferred-op token (test/observability hook for the
    /// `_DAT_8007BD08` cell the preamble consumes).
    pub fn set_pending_op(&mut self, token: u32) {
        self.pending_op = token;
    }

    /// The parked token + remaining retry timer, if any (mirrors
    /// `_DAT_8007BD38` / `_DAT_8007BD44`).
    pub fn parked_op(&self) -> Option<(u32, u32)> {
        (self.parked_timer != 0).then_some((self.parked_op, self.parked_timer))
    }

    /// Open by **raw in-RAM TOC index** - the retail-effective branch of
    /// `FUN_800558FC(path, _, _, raw_idx)`. The dev path string is ignored
    /// exactly as retail ignores it. Positions the cursor at the entry base
    /// and saves that base for `SeekWhence::FromBase`.
    ///
    /// Returns the entry size in sectors (`toc[raw+2] - toc[raw+1]`
    /// neighbour delta = the `FUN_8003E8A8` return). NB retail *computes*
    /// this but the shim's actual return register is uninitialized
    /// (caller-saved `s0` leaks through `move v0, s0`); no retail consumer
    /// reads it, so surfacing the resolver's value is strictly more useful
    /// and diverges from nothing observable.
    pub fn open_raw(&mut self, raw_idx: RawTocIndex) -> Result<u32> {
        self.park_pending_op();
        let extraction = raw_idx
            .checked_sub(2)
            .with_context(|| format!("raw TOC index {raw_idx} has no extraction sibling"))?;
        self.open_extraction(extraction)
    }

    /// Open by **extraction-space** entry index (= raw - 2). Same cursor
    /// semantics as [`Self::open_raw`].
    pub fn open_extraction(&mut self, extraction_idx: u16) -> Result<u32> {
        self.park_pending_op();
        // FUN_8003E8A8: start = toc_word[raw+2], next = toc_word[raw+3]
        // over the header-included in-RAM copy; ProtIndex's accessors
        // consume extraction-space indices over the header-stripped TOC -
        // the same words (see overlay_loader::OVERLAY_PROT_BASE).
        let start = self
            .prot
            .entry_start_lba_retail(extraction_idx)
            .with_context(|| format!("PROT TOC has no entry {extraction_idx}"))?;
        let count = self
            .prot
            .entry_lba_count_retail(extraction_idx)
            .with_context(|| format!("PROT TOC has no size pair for {extraction_idx}"))?;
        self.base_sector = Some(start);
        self.pos_sector = start;
        self.size_sectors = count;
        Ok(count)
    }

    /// Seek shim `FUN_80055A5C(fd, byte_off, whence)`: floor the byte
    /// offset to sectors, then apply [`Self::seek_sectors`]. The `fd`
    /// argument does not exist in the port (retail ignores it too).
    pub fn seek(&mut self, byte_offset: u32, whence: SeekWhence) -> Result<u32> {
        self.park_pending_op();
        self.seek_sectors(bytes_to_sectors_floor(byte_offset), whence)
    }

    /// Seek helper `FUN_8003E964(sector_off, whence)`: `whence == 0`
    /// restores the cursor to the saved entry base, then (both modes) the
    /// cursor advances by `sector_off` sectors. Returns the new cursor
    /// sector (PROT.DAT-relative).
    pub fn seek_sectors(&mut self, sector_offset: u32, whence: SeekWhence) -> Result<u32> {
        let base = self.opened_base()?;
        if whence == SeekWhence::FromBase {
            self.pos_sector = base;
        }
        // Retail is `CdIntToPos(CdPosToInt(cur) + off)` - unclamped
        // wrapping-free addition in LBA space. Sector counts here are tiny
        // vs u32, but keep the wrapping semantic explicit.
        self.pos_sector = self.pos_sector.wrapping_add(sector_offset);
        Ok(self.pos_sector)
    }

    /// Read shim `FUN_800559EC(fd, dst, byte_len)`: reads
    /// `floor(dst.len() / 2048)` whole sectors from the current cursor into
    /// `dst`, then advances the cursor by that sector count (the
    /// `FUN_8003DE7C` completion advance). Returns the byte count actually
    /// read - `dst.len() & !0x7FF`, so a sub-sector buffer reads **zero**
    /// bytes, exactly like the retail `srl a1, 0xB` truncation.
    ///
    /// The retail chain is asynchronous (kick + per-frame poll); the port
    /// collapses it to a synchronous copy like
    /// [`crate::cd_dma::ProtCdDmaHost`] does. No EOF clamp against the
    /// opened entry - reads past the entry end walk into the neighbouring
    /// PROT.DAT sectors as on retail; only running off the end of
    /// `PROT.DAT` itself errors.
    pub fn read(&mut self, dst: &mut [u8]) -> Result<usize> {
        self.park_pending_op();
        self.opened_base()?;
        let len_u32 = u32::try_from(dst.len()).context("read length exceeds u32")?;
        let sectors = bytes_to_sectors_floor(len_u32);
        if sectors == 0 {
            return Ok(0);
        }
        let byte_len = (sectors as usize) * SECTOR_BYTES as usize;
        let byte_offset = (self.pos_sector as u64) * SECTOR_BYTES as u64;
        let bytes = self
            .prot
            .prot_dat_raw_bytes(byte_offset, byte_len)
            .with_context(|| {
                format!(
                    "stream read {sectors} sectors at PROT.DAT sector {}",
                    self.pos_sector
                )
            })?;
        dst[..byte_len].copy_from_slice(&bytes);
        self.pos_sector = self.pos_sector.wrapping_add(sectors);
        Ok(byte_len)
    }

    /// Close shim `FUN_80055AC8()`: on the retail branch this is a pure
    /// no-op beyond the shared preamble - no handle is released and the
    /// cursor cells keep their values (a subsequent seek/read would still
    /// work on retail). The port mirrors that: the cursor stays open.
    pub fn close(&mut self) {
        self.park_pending_op();
        // Retail-effective branch: nothing else. The dev branch's
        // FUN_80060910 host-file close has no retail counterpart.
    }

    /// Current cursor sector (PROT.DAT-relative). Mirrors the MSF cell at
    /// `0x8007BC5C` minus the PROT base.
    pub fn position_sector(&self) -> u32 {
        self.pos_sector
    }

    /// Current cursor byte offset into PROT.DAT.
    pub fn position_bytes(&self) -> u64 {
        (self.pos_sector as u64) * SECTOR_BYTES as u64
    }

    /// Saved entry-base sector of the last open, if any.
    pub fn base_sector(&self) -> Option<u32> {
        self.base_sector
    }

    /// Sector count of the opened entry (the `FUN_8003E8A8` return retail
    /// discards at the open shim). Informational only - never a clamp.
    pub fn size_sectors(&self) -> u32 {
        self.size_sectors
    }

    fn opened_base(&self) -> Result<u32> {
        match self.base_sector {
            Some(b) => Ok(b),
            None => bail!("stream-file cursor used before open"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal synthetic PROT.DAT: header + 3 entries so the retail
    /// neighbour-delta size formula has room. Layout (sectors):
    ///
    /// - entry 0 (extraction): start 1, 4 sectors of `0xAA`
    /// - entry 1: start 5, 2 sectors of `0xBB`
    /// - entry 2: start 7, 2 sectors of `0xCC`
    ///
    /// Same shape as the `cd_dma` synthetic image, extended one entry.
    fn build_synthetic_prot_dat() -> Vec<u8> {
        const TOTAL_BYTES: usize = 0x4800; // 9 sectors
        let mut buf = vec![0u8; TOTAL_BYTES];
        buf[0..4].copy_from_slice(&0u32.to_le_bytes());
        buf[4..8].copy_from_slice(&3u32.to_le_bytes()); // file_num - 1 = 3
        // toc[0] = header_sectors = 1; entries at toc[2..]:
        //   starts 1, 5, 7, next 9; tail words keep the archive parser's
        //   indexed-size formula (toc[p+5]-toc[p+3]+4) in range.
        const TOC: [u32; 8] = [1, 0, 1, 5, 7, 9, 8, 7];
        for (i, v) in TOC.iter().enumerate() {
            let off = 0x08 + i * 4;
            buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
        }
        for b in &mut buf[0x0800..0x2800] {
            *b = 0xAA;
        }
        for b in &mut buf[0x2800..0x3800] {
            *b = 0xBB;
        }
        for b in &mut buf[0x3800..0x4800] {
            *b = 0xCC;
        }
        buf
    }

    fn make_host() -> StreamFileHost {
        let prot =
            ProtIndex::from_bytes(build_synthetic_prot_dat(), None).expect("synthetic PROT.DAT");
        StreamFileHost::new(Arc::new(prot))
    }

    #[test]
    fn byte_to_sector_floor_matches_the_srl_11() {
        assert_eq!(bytes_to_sectors_floor(0), 0);
        assert_eq!(bytes_to_sectors_floor(0x7FF), 0);
        assert_eq!(bytes_to_sectors_floor(0x800), 1);
        assert_eq!(bytes_to_sectors_floor(0xFFF), 1);
        assert_eq!(bytes_to_sectors_floor(0x10800), 33); // one summon/readef slot
        assert_eq!(bytes_to_sectors_floor(u32::MAX), u32::MAX >> 11);
    }

    #[test]
    fn open_raw_positions_at_entry_base_and_returns_sector_count() {
        let mut h = make_host();
        // Raw index 2 = extraction entry 0 (raw = extraction + 2).
        let n = h.open_raw(2).expect("open raw 2");
        assert_eq!(n, 4, "toc neighbour delta for entry 0");
        assert_eq!(h.base_sector(), Some(1));
        assert_eq!(h.position_sector(), 1);
        assert_eq!(h.size_sectors(), 4);
        // Raw 3 = extraction 1.
        let n = h.open_raw(3).expect("open raw 3");
        assert_eq!(n, 2);
        assert_eq!(h.position_sector(), 5);
    }

    #[test]
    fn open_rejects_raw_indices_without_extraction_sibling() {
        let mut h = make_host();
        assert!(h.open_raw(0).is_err());
        assert!(h.open_raw(1).is_err());
        assert!(h.open_extraction(999).is_err());
    }

    #[test]
    fn use_before_open_errors() {
        let mut h = make_host();
        assert!(h.seek(0x800, SeekWhence::FromBase).is_err());
        let mut buf = [0u8; 0x800];
        assert!(h.read(&mut buf).is_err());
    }

    #[test]
    fn seek_from_base_rewinds_then_advances() {
        let mut h = make_host();
        h.open_raw(2).unwrap();
        // Move somewhere else first.
        h.seek(0x1000, SeekWhence::FromCurrent).unwrap();
        assert_eq!(h.position_sector(), 3);
        // FromBase = restore saved base, then add.
        let pos = h.seek(0x800, SeekWhence::FromBase).unwrap();
        assert_eq!(pos, 2, "base 1 + 1 sector");
    }

    #[test]
    fn seek_floors_sub_sector_offsets() {
        let mut h = make_host();
        h.open_raw(2).unwrap();
        // 0x7FF bytes floors to 0 sectors - the cursor must not move.
        h.seek(0x7FF, SeekWhence::FromCurrent).unwrap();
        assert_eq!(h.position_sector(), 1);
        // 0xFFF floors to 1 sector.
        h.seek(0xFFF, SeekWhence::FromCurrent).unwrap();
        assert_eq!(h.position_sector(), 2);
        // FromBase with a sub-sector offset = plain rewind to base.
        h.seek(0x123, SeekWhence::FromBase).unwrap();
        assert_eq!(h.position_sector(), 1);
    }

    #[test]
    fn retail_whence_byte_decodes() {
        assert_eq!(SeekWhence::from_retail(0), SeekWhence::FromBase);
        assert_eq!(SeekWhence::from_retail(1), SeekWhence::FromCurrent);
        assert_eq!(SeekWhence::from_retail(0xFF), SeekWhence::FromCurrent);
    }

    #[test]
    fn read_copies_sectors_and_advances_cursor() {
        let mut h = make_host();
        h.open_raw(2).unwrap();
        let mut buf = vec![0u8; 0x1000];
        let n = h.read(&mut buf).expect("read 2 sectors");
        assert_eq!(n, 0x1000);
        assert!(buf.iter().all(|&b| b == 0xAA));
        assert_eq!(h.position_sector(), 3, "cursor advanced by 2 sectors");
        // Sequential read continues where the last one ended (the
        // FUN_8003DE7C completion advance).
        let mut buf2 = vec![0u8; 0x800];
        h.read(&mut buf2).unwrap();
        assert!(buf2.iter().all(|&b| b == 0xAA), "4th sector of entry 0");
        assert_eq!(h.position_sector(), 4);
    }

    #[test]
    fn sub_sector_read_reads_nothing() {
        let mut h = make_host();
        h.open_raw(2).unwrap();
        let mut buf = vec![0x5Au8; 0x7FF];
        let n = h.read(&mut buf).expect("sub-sector read is a no-op");
        assert_eq!(n, 0);
        assert!(buf.iter().all(|&b| b == 0x5A), "buffer untouched");
        assert_eq!(h.position_sector(), 1, "cursor did not move");
    }

    #[test]
    fn non_multiple_read_floors_and_leaves_the_tail_untouched() {
        let mut h = make_host();
        h.open_raw(2).unwrap();
        let mut buf = vec![0x5Au8; 0xC00]; // 1.5 sectors -> 1 sector read
        let n = h.read(&mut buf).unwrap();
        assert_eq!(n, 0x800);
        assert!(buf[..0x800].iter().all(|&b| b == 0xAA));
        assert!(buf[0x800..].iter().all(|&b| b == 0x5A), "tail untouched");
    }

    #[test]
    fn reads_cross_entry_boundaries_without_clamping() {
        // Retail has no EOF clamp: reading past entry 0's 4 sectors walks
        // into entry 1's bytes (the extraction over-read window).
        let mut h = make_host();
        h.open_raw(2).unwrap();
        h.seek(3 * 0x800, SeekWhence::FromBase).unwrap();
        let mut buf = vec![0u8; 0x1000];
        h.read(&mut buf).unwrap();
        assert!(buf[..0x800].iter().all(|&b| b == 0xAA), "entry 0 tail");
        assert!(buf[0x800..].iter().all(|&b| b == 0xBB), "entry 1 head");
    }

    #[test]
    fn read_past_prot_dat_end_errors() {
        let mut h = make_host();
        h.open_raw(4).unwrap(); // extraction 2: start 7, 2 sectors
        h.seek(2 * 0x800, SeekWhence::FromBase).unwrap(); // sector 9 = EOF
        let mut buf = vec![0u8; 0x800];
        assert!(h.read(&mut buf).is_err());
    }

    #[test]
    fn close_is_a_retail_noop_cursor_survives() {
        let mut h = make_host();
        h.open_raw(2).unwrap();
        h.seek(0x800, SeekWhence::FromBase).unwrap();
        h.close();
        // Retail keeps the MSF cells; a post-close read still works.
        assert_eq!(h.position_sector(), 2);
        let mut buf = vec![0u8; 0x800];
        assert_eq!(h.read(&mut buf).unwrap(), 0x800);
        assert!(buf.iter().all(|&b| b == 0xAA));
    }

    #[test]
    fn pending_op_parks_with_the_retail_timer() {
        let mut h = make_host();
        h.set_pending_op(0x1234);
        assert_eq!(h.parked_op(), None);
        h.close(); // any shim runs the preamble
        assert_eq!(h.parked_op(), Some((0x1234, 0xB4)));
        // A second shim call with no pending token leaves the park alone.
        h.close();
        assert_eq!(h.parked_op(), Some((0x1234, 0xB4)));
    }

    #[test]
    fn player_loader_shape_open_then_immediate_read() {
        // FUN_80052770 opens `data\battle\PLAYERn` (raw char+0x360+1..) and
        // reads 0x8000 bytes with no intervening seek - open must leave the
        // cursor at the entry base.
        let mut h = make_host();
        h.open_raw(3).unwrap(); // extraction 1: start 5, 0xBB
        let mut buf = vec![0u8; 0x800];
        h.read(&mut buf).unwrap();
        assert!(buf.iter().all(|&b| b == 0xBB));
    }
}
