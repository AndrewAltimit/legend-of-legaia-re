//! `StreamLoadQueue`: the index-based async streaming enqueue (`FUN_8003DDA0`).

use super::*;

// =========================================================================
// StreamLoadQueue - index-based async streaming enqueue (FUN_8003DDA0)
// =========================================================================

/// One entry in the boot-time async streaming load queue built by
/// [`StreamLoadQueue::enqueue`]. Retail stores an 8-byte descriptor at
/// `*(gp+0x1A8) + n*8`: `[u32 prot_idx][u32 byte_offset]`. `gp+0x1A8` holds
/// the table's *pointer* (`lw a0,0x1a8(gp)` at `0x8003DE34`), not the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamLoadDescriptor {
    /// PROT entry index queued (retail `desc[0]`, `gp+0x1A8 + n*8 + 0`).
    pub prot_idx: ProtIndex,
    /// Running byte offset into the accumulated destination buffer at
    /// which this entry's payload will land (retail `desc[1]`, `+4`).
    pub byte_offset: u32,
}

/// Index-based streaming **load queue** - the port of `FUN_8003DDA0`.
///
/// PORT: FUN_8003DDA0
/// REF: FUN_8003EE7C, FUN_8003DE7C, FUN_8003ED04 (XA-control toggles the
/// retail routine brackets each append with; hardware side-band, not
/// modelled here)
///
/// REPLACED-BY: `crate::scene::ProtIndex` whole-entry reads, through
/// `Scene` / `SceneAssets` - each load names its PROT entries and reads each
/// one whole and synchronously, so there is no contiguous multi-entry read
/// plan to accumulate and no destination cursor to advance.
///
/// What the queue computes in retail is a contiguous multi-entry read plan:
/// which entries, and at what offset of the shared destination each payload
/// lands (the running cursor). Both halves are discharged by construction in
/// the port: no read spans entries, and every entry gets its own buffer. This is the same replacement the
/// rest of this module carries ([`CdDmaHost::prot_one_shot_load`] and the
/// [`ProtCdDmaHost`] primitives), and [`ProtCdDmaHost`] - the only
/// non-test implementor of the host that would feed this queue - is
/// constructed only in tests. See `docs/tooling/reach-triage.md`.
///
/// Retail `FUN_8003DDA0(idx)` appends a load descriptor for PROT entry
/// `idx` to the queue and advances a running byte cursor by the entry's
/// byte size, reading the in-RAM TOC at `0x801C70F0` with the standard
/// [PROT TOC math](../../../docs/formats/prot.md):
///
/// ```text
///   start        = TOC[idx + 2]                 // gp+0x8F0 stash
///   next         = TOC[idx + 3]
///   size_sectors = next - start
///   gp+0x90C     = idx                          // last-enqueued index
///   gp+0x8F0     = start
///   n            = gp+0x8BC                      // queued-entry count
///   cursor       = gp+0x984                      // running byte cursor
///   gp+0x91C     = 0x78                          // read-wait timeout
///   desc         = *(gp+0x1A8) + n*8             // table pointer, not inline
///   desc[0]      = idx
///   desc[1]      = cursor
///   gp+0x984     = cursor + (size_sectors << 11) // advance by size*2048
///   gp+0x8BC     = n + 1
/// ```
///
/// The `<< 11` (`* 0x800`) turns a sector count into a byte count, so the
/// cursor tracks the total bytes queued so far - the destination offset the
/// next entry's payload lands at. This is the size-aware sibling of the
/// plain LBA resolver [`CdDmaHost::prot_index_size_lookup`]
/// (`FUN_8003E8A8`): the resolver hands a one-shot loader a single entry's
/// LBA count, whereas this accumulates a *contiguous multi-entry* read plan.
///
/// The TOC is caller-supplied (built from the user's disc at runtime); no
/// Sony bytes live here. `ghidra/scripts/funcs/8003dda0.txt` is the spec.
#[derive(Debug, Clone, Default)]
pub struct StreamLoadQueue {
    /// The descriptor table `gp+0x1A8` points at; length is retail's
    /// queued-entry count `gp+0x8BC`.
    entries: Vec<StreamLoadDescriptor>,
    /// Running byte cursor `gp+0x984` - total bytes queued so far.
    byte_cursor: u32,
    /// Last-enqueued PROT index `gp+0x90C`.
    last_prot_idx: ProtIndex,
    /// Last-resolved start LBA `gp+0x8F0`.
    last_start_lba: Lba,
    /// Read-wait timeout `gp+0x91C`, reset to `0x78` on each enqueue.
    read_wait_timeout: u32,
}

impl StreamLoadQueue {
    /// A fresh queue: empty descriptor table, byte cursor at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a load descriptor for PROT entry `prot_idx`, given its TOC
    /// start LBA and the following entry's start LBA (`next_lba`, so the
    /// entry spans `next_lba - start_lba` sectors). Mirrors `FUN_8003DDA0`
    /// exactly; returns the descriptor just appended.
    ///
    /// Arithmetic is wrapping to match the 32-bit retail registers (LBAs
    /// are small positive values in practice, so this never wraps for real
    /// disc data, but a degenerate `next < start` will not panic).
    pub fn enqueue(
        &mut self,
        prot_idx: ProtIndex,
        start_lba: Lba,
        next_lba: Lba,
    ) -> StreamLoadDescriptor {
        let size_sectors = next_lba.wrapping_sub(start_lba);
        self.last_prot_idx = prot_idx;
        self.last_start_lba = start_lba;
        self.read_wait_timeout = 0x78;
        let desc = StreamLoadDescriptor {
            prot_idx,
            byte_offset: self.byte_cursor,
        };
        self.entries.push(desc);
        self.byte_cursor = self.byte_cursor.wrapping_add(size_sectors.wrapping_shl(11));
        desc
    }

    /// Convenience wrapper resolving `start`/`next` straight out of a raw
    /// in-RAM TOC word slice (`0x801C70F0` head), reading `toc[idx + 2]`
    /// and `toc[idx + 3]` per the PROT TOC math, then calling
    /// [`enqueue`](Self::enqueue). Returns `None` when `idx + 3` is out of
    /// range. The TOC slice is caller-supplied disc data.
    pub fn enqueue_from_toc(
        &mut self,
        prot_idx: ProtIndex,
        toc: &[u32],
    ) -> Option<StreamLoadDescriptor> {
        let i = prot_idx as usize;
        let start = *toc.get(i + 2)?;
        let next = *toc.get(i + 3)?;
        Some(self.enqueue(prot_idx, start, next))
    }

    /// The queued descriptors so far (retail `gp+0x1A8` table).
    pub fn entries(&self) -> &[StreamLoadDescriptor] {
        &self.entries
    }

    /// Queued-entry count (retail `gp+0x8BC`).
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Running byte cursor (retail `gp+0x984`) - total bytes queued, i.e.
    /// the destination offset the next enqueue will land at.
    pub fn byte_cursor(&self) -> u32 {
        self.byte_cursor
    }

    /// Last-enqueued PROT index (retail `gp+0x90C`).
    pub fn last_prot_idx(&self) -> ProtIndex {
        self.last_prot_idx
    }

    /// Last-resolved start LBA (retail `gp+0x8F0`).
    pub fn last_start_lba(&self) -> Lba {
        self.last_start_lba
    }

    /// Read-wait timeout (retail `gp+0x91C`), `0x78` after any enqueue.
    pub fn read_wait_timeout(&self) -> u32 {
        self.read_wait_timeout
    }
}
