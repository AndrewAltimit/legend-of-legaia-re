//! 512 KB SPU RAM model + libspu-shaped DMA transfer engine.
//!
//! The PSX SPU has 512 KB of dedicated RAM. ADPCM sample blocks live there
//! and voices read by 8-byte-aligned address. The CPU side talks to it via:
//!
//! 1. **Address pointer** — `_spu_t` in retail (a global current-write addr,
//!    unit = 8 bytes). Set by `SpuSetTransferStartAddr`.
//! 2. **Direction flag** — `_spu_a` (read vs write). Flipped by
//!    `SpuSetTransferMode`.
//! 3. **Body** — `SpuWrite(buf, len)` queues bytes from CPU RAM to SPU RAM
//!    starting at the current pointer; `SpuTransfer()` (`_SpuTransfer`)
//!    actually drains the queue, advancing the pointer in 8-byte units.
//!
//! Real hardware does this asynchronously by DMA, but a clean-room model can
//! drain synchronously since the playback layer reads SPU RAM by address
//! during voice ticks anyway.
//!
//! All addresses here are **byte** offsets into the 512 KB region. The
//! original code stores them in 8-byte units; converters live on the public
//! API.

/// SPU RAM size in bytes.
pub const SPU_RAM_BYTES: usize = 512 * 1024;
/// SPU RAM size in 8-byte units (the unit `_spu_t` is stored in).
pub const SPU_RAM_UNITS_8: usize = SPU_RAM_BYTES / 8;

/// Direction of the active transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransferDirection {
    /// Reads from CPU into SPU RAM (the common case: load a sample bank).
    #[default]
    CpuToSpu,
    /// Reads from SPU RAM back to CPU (used for envelope readback).
    SpuToCpu,
}

/// SPU RAM + transfer-engine state.
#[derive(Debug, Clone)]
pub struct SpuRam {
    bytes: Vec<u8>,
    /// Current write/read pointer, in BYTES (libspu-API addresses are 8-byte
    /// units; we convert at the boundary).
    transfer_addr: usize,
    direction: TransferDirection,
}

impl Default for SpuRam {
    fn default() -> Self {
        Self::new()
    }
}

impl SpuRam {
    pub fn new() -> Self {
        Self {
            bytes: vec![0; SPU_RAM_BYTES],
            transfer_addr: 0,
            direction: TransferDirection::CpuToSpu,
        }
    }

    /// Set the transfer pointer in **8-byte units**, matching libspu's
    /// `SpuSetTransferStartAddr`. Returns the byte address.
    pub fn set_transfer_start_units_8(&mut self, units: u32) -> u32 {
        let bytes = (units as usize) * 8;
        self.transfer_addr = bytes.min(SPU_RAM_BYTES);
        self.transfer_addr as u32
    }

    /// Set the transfer pointer in bytes directly.
    pub fn set_transfer_start_bytes(&mut self, addr: u32) {
        self.transfer_addr = (addr as usize).min(SPU_RAM_BYTES);
    }

    /// Get the current transfer pointer in bytes.
    pub fn transfer_addr(&self) -> u32 {
        self.transfer_addr as u32
    }

    /// Set the transfer direction (libspu `SpuSetTransferMode`).
    pub fn set_direction(&mut self, dir: TransferDirection) {
        self.direction = dir;
    }

    pub fn direction(&self) -> TransferDirection {
        self.direction
    }

    /// libspu-style synchronous write: copy `data` into SPU RAM at the
    /// current transfer pointer, then advance the pointer. Returns the
    /// number of bytes actually written (clipped to RAM end).
    ///
    /// Real hardware is asynchronous — the queue flush happens on the next
    /// `SpuTransfer()`. The clean-room model collapses queue+drain since we
    /// want the bytes visible to subsequent decoder ticks immediately.
    pub fn write(&mut self, data: &[u8]) -> usize {
        if self.direction != TransferDirection::CpuToSpu {
            return 0;
        }
        let avail = SPU_RAM_BYTES.saturating_sub(self.transfer_addr);
        let n = data.len().min(avail);
        self.bytes[self.transfer_addr..self.transfer_addr + n].copy_from_slice(&data[..n]);
        self.transfer_addr += n;
        n
    }

    /// libspu-style synchronous read: copy `dst.len()` bytes from SPU RAM
    /// at the current transfer pointer into `dst`. Returns bytes read.
    pub fn read(&mut self, dst: &mut [u8]) -> usize {
        if self.direction != TransferDirection::SpuToCpu {
            return 0;
        }
        let avail = SPU_RAM_BYTES.saturating_sub(self.transfer_addr);
        let n = dst.len().min(avail);
        dst[..n].copy_from_slice(&self.bytes[self.transfer_addr..self.transfer_addr + n]);
        self.transfer_addr += n;
        n
    }

    /// Direct read by address — used by the voice playback layer to fetch
    /// the next ADPCM block. Bypasses the transfer pointer/direction and
    /// is only intended for the SPU itself.
    pub fn slice(&self, addr: u32, len: u32) -> &[u8] {
        let start = (addr as usize).min(SPU_RAM_BYTES);
        let end = (start + len as usize).min(SPU_RAM_BYTES);
        &self.bytes[start..end]
    }

    /// Direct write by address — used by `SsSpuMalloc` / staging code that
    /// wants to drop a whole VAB body in at a known offset, no queue.
    pub fn write_at(&mut self, addr: u32, data: &[u8]) -> usize {
        let start = (addr as usize).min(SPU_RAM_BYTES);
        let n = data.len().min(SPU_RAM_BYTES - start);
        self.bytes[start..start + n].copy_from_slice(&data[..n]);
        n
    }
}

/// libspu-shaped allocator over the SPU RAM, modeling
/// `SsSpuMalloc` / `SpuFree` / the compactor (`_spu_a` direction flips
/// during compaction). Stores allocated regions as a sorted free-list.
///
/// Implementation: simple first-fit free-list. Real hardware is identical
/// in shape (Sony's allocator is also first-fit) but uses a linked-list
/// header in SPU RAM itself; we keep our metadata CPU-side because it's
/// cleaner to reason about in tests.
#[derive(Debug, Clone)]
pub struct SpuAllocator {
    /// Sorted-by-address list of free regions: (start_byte, len_bytes).
    free: Vec<(u32, u32)>,
}

impl SpuAllocator {
    /// Build an allocator over the address range `[start, start+size)`.
    /// `start` should be at least `0x1000` to avoid the SPU's reverb area.
    pub fn new(start: u32, size: u32) -> Self {
        Self {
            free: vec![(start, size)],
        }
    }

    /// Allocate `size` bytes. Returns the start address of the allocation,
    /// or `None` if no free region can fit.
    pub fn alloc(&mut self, size: u32) -> Option<u32> {
        if size == 0 {
            return None;
        }
        // Round up to 16-byte alignment (one ADPCM block).
        let size = size.div_ceil(16) * 16;
        for i in 0..self.free.len() {
            let (start, len) = self.free[i];
            if len >= size {
                let allocated = start;
                let new_len = len - size;
                let new_start = start + size;
                if new_len == 0 {
                    self.free.remove(i);
                } else {
                    self.free[i] = (new_start, new_len);
                }
                return Some(allocated);
            }
        }
        None
    }

    /// Free a previously-allocated region. Coalesces with neighbours.
    pub fn free(&mut self, addr: u32, size: u32) {
        let size = size.div_ceil(16) * 16;
        let mut entry = (addr, size);
        // Insert at the right position, then coalesce both sides.
        let pos = self.free.partition_point(|&(s, _)| s < addr);
        // Try to merge with predecessor.
        if pos > 0 {
            let (ps, pl) = self.free[pos - 1];
            if ps + pl == entry.0 {
                entry = (ps, pl + entry.1);
                self.free.remove(pos - 1);
            }
        }
        // Try to merge with successor (note: index may have shifted).
        let pos = self.free.partition_point(|&(s, _)| s < entry.0);
        if pos < self.free.len() {
            let (ns, nl) = self.free[pos];
            if entry.0 + entry.1 == ns {
                entry = (entry.0, entry.1 + nl);
                self.free.remove(pos);
            }
        }
        let pos = self.free.partition_point(|&(s, _)| s < entry.0);
        self.free.insert(pos, entry);
    }

    /// Total free bytes (across all regions).
    pub fn total_free(&self) -> u32 {
        self.free.iter().map(|&(_, l)| l).sum()
    }

    /// Number of free regions (pre-coalesce inspection helper).
    pub fn region_count(&self) -> usize {
        self.free.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_advances_pointer_and_lands_at_addr() {
        let mut ram = SpuRam::new();
        ram.set_transfer_start_units_8(0x100); // byte addr 0x800
        let n = ram.write(&[1, 2, 3, 4]);
        assert_eq!(n, 4);
        assert_eq!(ram.transfer_addr(), 0x800 + 4);
        assert_eq!(ram.slice(0x800, 4), &[1, 2, 3, 4]);
    }

    #[test]
    fn write_in_wrong_direction_no_op() {
        let mut ram = SpuRam::new();
        ram.set_direction(TransferDirection::SpuToCpu);
        let n = ram.write(&[1, 2, 3, 4]);
        assert_eq!(n, 0);
    }

    #[test]
    fn write_at_end_clips() {
        let mut ram = SpuRam::new();
        ram.set_transfer_start_bytes(SPU_RAM_BYTES as u32 - 2);
        let n = ram.write(&[9, 9, 9, 9]);
        assert_eq!(n, 2);
    }

    #[test]
    fn alloc_first_fit_then_free_coalesce() {
        let mut a = SpuAllocator::new(0x1000, 0x10000);
        let p1 = a.alloc(64).unwrap();
        let p2 = a.alloc(128).unwrap();
        let p3 = a.alloc(64).unwrap();
        assert_eq!(p1, 0x1000);
        assert_eq!(p2, p1 + 64);
        assert_eq!(p3, p2 + 128);
        let initial_free = a.total_free();
        a.free(p2, 128);
        // freeing the middle region: should still be one fragmented region count >= 2
        assert!(a.region_count() >= 2);
        assert_eq!(a.total_free(), initial_free + 128);
        a.free(p1, 64);
        a.free(p3, 64);
        // Full coalesce -> back to single region.
        assert_eq!(a.region_count(), 1);
        assert_eq!(a.total_free(), 0x10000);
    }

    #[test]
    fn alloc_returns_none_when_too_big() {
        let mut a = SpuAllocator::new(0, 1024);
        assert!(a.alloc(2048).is_none());
        // Existing free region untouched.
        assert_eq!(a.total_free(), 1024);
    }

    #[test]
    fn alloc_aligns_up_to_16() {
        let mut a = SpuAllocator::new(0, 64);
        // 7 byte request rounds to 16.
        let _ = a.alloc(7).unwrap();
        assert_eq!(a.total_free(), 48);
    }
}
