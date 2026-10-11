//! `ProtCdDmaHost`: the concrete offline / WASM `CdDmaHost` over an in-memory `PROT.DAT`.

use super::*;

// =========================================================================
// ProtCdDmaHost - concrete offline/WASM implementation
// =========================================================================

/// PSX main RAM size (2 MiB) used as the synthetic destination buffer.
/// Retail CD-DMA writes target absolute addresses in `0x80000000..0x80200000`;
/// the offline host masks the high bits and writes into a Vec of this size.
pub const SYNTHETIC_MAIN_RAM_BYTES: usize = 0x0020_0000;

/// Bit mask that folds PSX kseg0 / kseg1 / kuseg pointers into the 2 MiB
/// main-RAM window. Retail uses `0x80xxxxxx` (cached) and `0xA0xxxxxx`
/// (uncached) interchangeably for DMA targets; both alias the same physical
/// RAM. The synthetic host accepts either form.
pub const SYNTHETIC_MAIN_RAM_MASK: u32 = 0x001F_FFFF;

/// PSX sector size in bytes (1 LBA = `0x800` bytes).
pub const SECTOR_BYTES: u32 = 0x800;

/// Internal state-machine register for the offline host. Mirrors the
/// `gp[0x980]` register the retail state machine writes:
///
/// - `Idle`  → `0` (no read ever issued, or post-drain snap).
/// - `Busy`  → `1` (kick issued, not yet drained).
/// - `Ready` → `2` (read complete; consumer can use the destination buffer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PollState {
    #[default]
    Idle,
    Busy,
    Ready,
}

/// [`CdDmaHost`] implementation backed by a [`crate::scene::ProtIndex`]
/// and a synthetic 2 MiB main-RAM buffer.
///
/// This is the **"MemoryVfs-backed WASM/offline"** implementation the
/// backlog calls for: the engine port doesn't actually stream sectors
/// from a CD device because `PROT.DAT` is loaded entirely into memory at
/// startup, so the retail asynchronous state machine collapses to
/// synchronous reads. Every [`CdDmaHost::kick_libcd_read`] / matching
/// [`CdDmaHost::async_lba_load`] performs the byte copy immediately and
/// the state machine transitions straight to [`ReadWaitOutcome::Ready`].
///
/// ## Destination buffer model
///
/// The retail SCUS treats [`DestAddr`] as an absolute PSX RAM pointer in
/// the `0x80xxxxxx` window. The offline host carries a private
/// [`SYNTHETIC_MAIN_RAM_BYTES`]-sized `Vec<u8>` and masks incoming `dst`
/// values with [`SYNTHETIC_MAIN_RAM_MASK`] before writing - so callers can
/// pass the same `0x801xxxxx` pointers used in the SCUS dumps and the
/// host transparently routes them into its private buffer.
///
/// Consumers retrieve the loaded bytes via [`Self::main_ram`] (read-only
/// slice over the full buffer) or [`Self::read`] (slice at an arbitrary
/// `(addr, len)`).
///
/// ## Mirrored retail scratchpad
///
/// The retail SCUS keeps the state machine in a fixed `gp+0x8xx..+0x9xx`
/// block of globals. The host carries the same fields as Rust members so
/// the trait's `prot_index_size_lookup` / `async_lba_load` /
/// `kick_libcd_read` / `read_wait_poll` paths can be implemented in a
/// way that mirrors the retail dataflow without leaking into the engine.
///
/// | Retail global   | Field             | Role                                |
/// |-----------------|-------------------|-------------------------------------|
/// | `gp+0x90C`      | `last_prot_idx`   | last-resolved PROT entry index      |
/// | `gp+0x8F0`      | `last_start_lba`  | last-resolved start LBA             |
/// | `gp+0x894`      | `last_dst`        | destination buffer pointer          |
/// | `gp+0x97C`      | `last_count`      | sector count                        |
/// | `gp+0x980`      | `state`           | state-machine register              |
/// | `gp+0x988`      | `read_in_progress`| "read in progress" flag             |
/// | `gp+0x928`      | `error`           | error code                          |
/// | `gp+0x91C`      | `timeout`         | read-wait countdown                 |
/// | `gp+0x95C..+0x95E` | `last_msf`     | per-request BCD MSF (Some when set) |
///
/// The synthetic host always takes the non-libcd (`_DAT_8007B8C2 == 0`) shape
/// of `read_wait_poll`. Retail runs the `!= 0` arm, which drives libcd
/// directly; the offline replacement has no libcd to drive, so it mirrors the
/// callback-completion shape instead. That is a host-substitution difference,
/// not a claim about which arm retail takes.
pub struct ProtCdDmaHost {
    pub(super) prot: std::sync::Arc<crate::scene::ProtIndex>,
    main_ram: Vec<u8>,
    state: PollState,
    last_prot_idx: ProtIndex,
    last_start_lba: Lba,
    last_dst: DestAddr,
    last_count: u32,
    last_msf: Option<(u8, u8, u8)>,
    timeout: u32,
    error: bool,
    read_in_progress: bool,
    // ---- OverlayLoaderHost state (FUN_8003EBE4 / FUN_8003EC70) ----
    /// `gp+0x924` (Loader A cache slot, FUN_8003EBE4).
    overlay_slot_a: i32,
    /// `gp+0x934` (Loader B cache slot, FUN_8003EC70).
    overlay_slot_b: i32,
    /// `*DAT_8001038C` - Loader A destination buffer pointer.
    overlay_dst_a: DestAddr,
    /// `*DAT_80010390` - Loader B destination buffer pointer.
    overlay_dst_b: DestAddr,
    /// `_DAT_8007B83C` - mode-state word consumed by Loader B's
    /// invalidate guard.
    overlay_mode_state: u16,
    /// `_DAT_8007B868` - dev/retail branch discriminator. Retail = 0;
    /// dev builds set this to non-zero (the mode-table uses the value
    /// as the return code in the dev short-circuit path).
    overlay_dev_flag: u32,
}

impl ProtCdDmaHost {
    /// Build a host over `prot`. The synthetic main-RAM buffer is allocated
    /// up-front at [`SYNTHETIC_MAIN_RAM_BYTES`] and zero-initialised.
    pub fn new(prot: std::sync::Arc<crate::scene::ProtIndex>) -> Self {
        Self {
            prot,
            main_ram: vec![0u8; SYNTHETIC_MAIN_RAM_BYTES],
            state: PollState::Idle,
            last_prot_idx: 0,
            last_start_lba: 0,
            last_dst: 0,
            last_count: 0,
            last_msf: None,
            timeout: 0,
            error: false,
            read_in_progress: false,
            overlay_slot_a: crate::overlay_loader::OVERLAY_CACHE_EMPTY,
            overlay_slot_b: crate::overlay_loader::OVERLAY_CACHE_EMPTY,
            overlay_dst_a: 0,
            overlay_dst_b: 0,
            overlay_mode_state: 0,
            overlay_dev_flag: 0,
        }
    }

    /// Configure the overlay-loader destination buffers. Retail's
    /// `*DAT_8001038C` (Loader A) and `*DAT_80010390` (Loader B) are
    /// populated at boot from the per-build mode table; engines call this
    /// once after [`Self::new`] to wire the addresses.
    pub fn set_overlay_destinations(&mut self, dst_a: DestAddr, dst_b: DestAddr) {
        self.overlay_dst_a = dst_a;
        self.overlay_dst_b = dst_b;
    }

    /// Set the mode-state word read by Loader B's invalidate guard
    /// ([`crate::overlay_loader::load_overlay_b`]). Mirrors `_DAT_8007B83C`.
    pub fn set_overlay_mode_state(&mut self, value: u16) {
        self.overlay_mode_state = value;
    }

    /// Set the dev/retail branch discriminator. Mirrors `_DAT_8007B868`.
    /// Retail = 0; dev builds set non-zero so the overlay loaders
    /// short-circuit.
    pub fn set_overlay_dev_flag(&mut self, value: u32) {
        self.overlay_dev_flag = value;
    }

    /// Read the current state of an overlay cache slot.
    pub fn overlay_slot(&self, slot: crate::overlay_loader::OverlayCacheSlot) -> i32 {
        match slot {
            crate::overlay_loader::OverlayCacheSlot::A => self.overlay_slot_a,
            crate::overlay_loader::OverlayCacheSlot::B => self.overlay_slot_b,
        }
    }

    /// Read-only view over the synthetic main-RAM buffer.
    pub fn main_ram(&self) -> &[u8] {
        &self.main_ram
    }

    /// Read `len` bytes from the synthetic main-RAM buffer at `addr` (a
    /// retail PSX pointer; the high bits are folded via
    /// [`SYNTHETIC_MAIN_RAM_MASK`]). Returns `None` if the slice would
    /// run past the buffer end.
    pub fn read(&self, addr: DestAddr, len: usize) -> Option<&[u8]> {
        let start = (addr & SYNTHETIC_MAIN_RAM_MASK) as usize;
        let end = start.checked_add(len)?;
        self.main_ram.get(start..end)
    }

    /// Last PROT index touched by [`CdDmaHost::prot_index_size_lookup`].
    /// Mirrors the `gp+0x90C` retail global.
    pub fn last_prot_idx(&self) -> ProtIndex {
        self.last_prot_idx
    }

    /// Last start LBA stashed by [`CdDmaHost::prot_index_size_lookup`].
    /// Mirrors the `gp+0x8F0` retail global.
    pub fn last_start_lba(&self) -> Lba {
        self.last_start_lba
    }

    /// Last destination buffer addr written by
    /// [`CdDmaHost::async_lba_load`]. Mirrors the `gp+0x894` retail global.
    pub fn last_dst(&self) -> DestAddr {
        self.last_dst
    }

    /// Last sector count written by [`CdDmaHost::async_lba_load`].
    /// Mirrors the `gp+0x97C` retail global.
    pub fn last_count(&self) -> u32 {
        self.last_count
    }

    /// Last BCD MSF stashed when [`CdDmaHost::prot_index_size_lookup`] was
    /// called with `set_msf = true`. Mirrors `gp+0x95C..+0x95E`.
    /// `None` when no `set_msf` call has happened yet.
    pub fn last_msf(&self) -> Option<(u8, u8, u8)> {
        self.last_msf
    }

    /// Synthesised libcd-equivalent read. Copies
    /// `last_count * SECTOR_BYTES` bytes out of `PROT.DAT` (via
    /// [`crate::scene::ProtIndex::prot_dat_raw_bytes`]) into
    /// `main_ram[last_dst..]`. Sets `state = Ready` and clears
    /// `read_in_progress`. Idempotent if no kick has been queued.
    fn perform_synchronous_read(&mut self) -> Result<(), String> {
        if !self.read_in_progress {
            return Ok(());
        }
        let byte_offset = (self.last_start_lba as u64) * (SECTOR_BYTES as u64);
        let len = (self.last_count as usize)
            .checked_mul(SECTOR_BYTES as usize)
            .ok_or_else(|| "lba count overflow".to_string())?;
        let bytes = self
            .prot
            .prot_dat_raw_bytes(byte_offset, len)
            .map_err(|e| format!("read PROT.DAT @ 0x{byte_offset:x} +{len}: {e}"))?;
        let dst_start = (self.last_dst & SYNTHETIC_MAIN_RAM_MASK) as usize;
        let dst_end = dst_start
            .checked_add(len)
            .ok_or_else(|| "dst overflow".to_string())?;
        if dst_end > self.main_ram.len() {
            return Err(format!(
                "dst 0x{:x}..0x{:x} past main-RAM end (0x{:x})",
                dst_start,
                dst_end,
                self.main_ram.len()
            ));
        }
        self.main_ram[dst_start..dst_end].copy_from_slice(&bytes);
        self.state = PollState::Ready;
        self.read_in_progress = false;
        Ok(())
    }
}

impl CdDmaHost for ProtCdDmaHost {
    /// Mirrors `FUN_8003e8a8(prot_idx, set_msf)`. Stashes the start LBA
    /// and PROT index, optionally computes BCD MSF, returns the entry's
    /// sector count via the retail `toc[idx+3] - toc[idx+2]` formula.
    /// Out-of-range indices return zero (matches retail's `subu` wrap
    /// on a TOC overread, which would yield garbage rather than panic).
    ///
    /// PORT: FUN_8003E8A8
    ///
    /// REPLACED-BY: `crate::scene::ProtIndex` - same replacement as every
    /// method in this block, see [`CdDmaHost::prot_one_shot_load`].
    /// `ProtCdDmaHost` is constructed only under `#[cfg(test)]` and in the
    /// disc-gated `cd_dma_real_prot` oracle.
    fn prot_index_size_lookup(&mut self, prot_idx: ProtIndex, set_msf: bool) -> u32 {
        let count = self.prot.entry_lba_count_retail(prot_idx).unwrap_or(0);
        let start_lba = self.prot.entry_start_lba_retail(prot_idx).unwrap_or(0);
        self.last_prot_idx = prot_idx;
        self.last_start_lba = start_lba;
        if set_msf {
            self.last_msf = Some(lba_to_bcd_msf(start_lba));
        }
        count
    }

    /// Mirrors `FUN_8003e800(dst, count, flags)`. The offline path
    /// collapses asynchrony: a stale `read_in_progress` is drained
    /// immediately, `error` is cleared, then [`LoadFlags::ISSUE`] queues
    /// the kick and [`LoadFlags::BLOCK`] is satisfied without a real
    /// poll loop (the synchronous copy in `Self::perform_synchronous_read`
    /// already left the state machine in `PollState::Ready`).
    ///
    /// PORT: FUN_8003E800
    ///
    /// REPLACED-BY: `crate::scene::ProtIndex` - same replacement as every
    /// method in this block, see [`CdDmaHost::prot_one_shot_load`]. The retail
    /// routine is reached from the whole loader surface on the disc; the
    /// port's own loaders read the entry instead of queueing a transfer.
    fn async_lba_load(&mut self, dst: DestAddr, count: u32, flags: LoadFlags) {
        if self.read_in_progress {
            // Drain any stale read first - retail's FUN_8003e800 calls
            // FUN_8003de7c(0) when read_in_progress is set.
            let _ = self.read_wait_poll(false);
        }
        if self.error {
            self.error = false;
        }
        if flags.issue() {
            self.last_count = count;
            self.last_dst = dst;
            self.kick_libcd_read();
        }
        if flags.block() {
            let _ = self.read_wait_poll(false);
        }
        // Refresh the read-wait countdown to the retail magic value 0x78.
        self.timeout = 0x78;
    }

    /// Mirrors `FUN_8003f128`. The offline host doesn't dispatch to libcd;
    /// the synchronous PROT.DAT read happens here, and the state machine
    /// snaps directly from `Idle`/`Busy` into `Ready` once the copy is
    /// done. Errors from the underlying [`crate::scene::ProtIndex`] read
    /// surface as `error = true` and `state = Idle`.
    ///
    /// PORT: FUN_8003F128
    ///
    /// REPLACED-BY: `crate::scene::ProtIndex` - same replacement as every
    /// method in this block, see [`CdDmaHost::prot_one_shot_load`]. Its only
    /// in-workspace caller is [`Self::async_lba_load`] above, which is
    /// replaced for the same reason.
    fn kick_libcd_read(&mut self) {
        self.read_in_progress = true;
        self.state = PollState::Busy;
        if let Err(_msg) = self.perform_synchronous_read() {
            self.error = true;
            self.state = PollState::Idle;
            self.read_in_progress = false;
        }
    }

    /// Mirrors `FUN_8003de7c(gated)`. The retail return convention
    /// gates on `read_in_progress` (`gp+0x988`):
    ///
    /// - `gated = true`  (per-frame poll): if `read_in_progress == 0`,
    ///   return `Ready` immediately (matches the
    ///   `lw v0,0x988(gp); beq v0,zero,...; clear v0` early-out at
    ///   `0x8003df70..0x8003df7c`). Otherwise the synthetic kick is
    ///   already done, but if a previous error stuck the state in
    ///   `Busy` we surface `InProgress`.
    /// - `gated = false` (drain stale read): snap `read_in_progress`
    ///   off, return `Ready` unconditionally.
    ///
    /// The offline host never returns [`ReadWaitOutcome::Idle`]: that
    /// return is reserved for the dev-arm state machine gated on
    /// `_DAT_8007BA70` / `_DAT_8007B8C2`, neither of which exist in
    /// the offline replacement.
    ///
    /// PORT: FUN_8003DE7C
    ///
    /// REPLACED-BY: `crate::scene::ProtIndex` - same replacement as every
    /// method in this block, see [`CdDmaHost::prot_one_shot_load`]. This is
    /// the widest-reaching of the five on the disc - the reference sweep puts
    /// `FUN_8003DE7C` at 127 `jal` sites across `SCUS_942.54` and eleven
    /// overlay images - so what is replaced is a heavily-used routine, not
    /// dead code: a synchronous read has nothing left to wait for.
    fn read_wait_poll(&mut self, gated: bool) -> ReadWaitOutcome {
        if !gated {
            self.read_in_progress = false;
            self.state = PollState::Idle;
            return ReadWaitOutcome::Ready;
        }
        if self.timeout > 0 {
            self.timeout -= 1;
        }
        if !self.read_in_progress {
            return ReadWaitOutcome::Ready;
        }
        match self.state {
            PollState::Ready | PollState::Idle => ReadWaitOutcome::Ready,
            PollState::Busy => ReadWaitOutcome::InProgress,
        }
    }
}

/// Wires [`ProtCdDmaHost`] as the concrete offline implementation of
/// [`crate::overlay_loader::OverlayLoaderHost`]. Cache slots, destinations
/// and the dev/mode-state words live as inline fields on the host;
/// configure them via [`ProtCdDmaHost::set_overlay_destinations`],
/// [`ProtCdDmaHost::set_overlay_mode_state`], and
/// [`ProtCdDmaHost::set_overlay_dev_flag`].
impl crate::overlay_loader::OverlayLoaderHost for ProtCdDmaHost {
    fn dev_branch_flag(&self) -> u32 {
        self.overlay_dev_flag
    }
    fn cache_slot(&self, slot: crate::overlay_loader::OverlayCacheSlot) -> i32 {
        self.overlay_slot(slot)
    }
    fn set_cache_slot(&mut self, slot: crate::overlay_loader::OverlayCacheSlot, value: i32) {
        match slot {
            crate::overlay_loader::OverlayCacheSlot::A => self.overlay_slot_a = value,
            crate::overlay_loader::OverlayCacheSlot::B => self.overlay_slot_b = value,
        }
    }
    fn overlay_dst(&self, slot: crate::overlay_loader::OverlayCacheSlot) -> DestAddr {
        match slot {
            crate::overlay_loader::OverlayCacheSlot::A => self.overlay_dst_a,
            crate::overlay_loader::OverlayCacheSlot::B => self.overlay_dst_b,
        }
    }
    fn mode_state_word(&self) -> u16 {
        self.overlay_mode_state
    }
}
