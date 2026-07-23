//! CD-DMA streaming reader host-trait abstractions.
//!
//! PORT: FUN_8003DE7C, FUN_8003E800, FUN_8003E8A8, FUN_8003EB98, FUN_8003F128
//! PORT: FUN_8005EA84
//!
//! SCUS-resident helpers around the CD streaming reader. They
//! sit one layer above the libcd primitives at `0x8005Dxxx` and one
//! layer below the per-format loaders ([`scene_resources`],
//! [`battle_session`], etc.) The trait exposed here lets the engine
//! layer plug in either:
//!
//! - a synthetic `MemoryVfs`-backed implementation for offline /
//!   WASM targets (where there is no CD device),
//! - a libcd-backed implementation for native targets that actually
//!   stream sectors from a real PSX disc image.
//!
//! ## SCUS state map
//!
//! The retail helpers all read & write a small block of `gp+0x8xx..+0x9xx`
//! scratchpad globals plus the PROT TOC at `0x801C70F0`:
//!
//! | Addr            | Role                                                  |
//! |-----------------|-------------------------------------------------------|
//! | `gp+0x894`      | destination buffer pointer (input to libcd kick)      |
//! | `gp+0x97C`      | sector count (input to libcd kick)                    |
//! | `gp+0x8F0`      | last-resolved PROT start LBA                          |
//! | `gp+0x90C`      | last-resolved PROT entry index                        |
//! | `gp+0x91C`      | read-wait countdown timer (set to `0x78` per request) |
//! | `gp+0x928`      | error code (non-zero = error pending)                 |
//! | `gp+0x980`      | state machine register (1/2/6 = busy/ready/snap)      |
//! | `gp+0x988`      | "read in progress" flag                               |
//! | `gp+0x95C..+0x95E` | per-request BCD MSF (computed by FUN_8003E8A8)     |
//! | `gp+0x958`      | libcd CdSync timeout (set to `0xB4` by FUN_8003F128)  |
//! | `gp+0x8B8`      | MSF + sector-count target                             |
//! | `gp+0x8C8`      | start MSF (BCD)                                       |
//! | `_DAT_8007B8C2` | retail/dev branch selector (0 = dev host-trap path,   |
//! |                 |  != 0 = retail PROT-index path; retail boots at 1)    |
//!
//! Engines port these as opaque host state - the trait abstracts the
//! call surface so re-hosting the gp scratchpad is invisible to
//! consumers.
//!
//! ## Function map
//!
//! | Method                     | SCUS function | Role                                                                 |
//! |----------------------------|---------------|----------------------------------------------------------------------|
//! | [`prot_index_size_lookup`] | FUN_8003E8A8  | Look up PROT entry size; stash start LBA + (optional) BCD MSF.      |
//! | [`async_lba_load`]         | FUN_8003E800  | Queue async sector read into `dst`. Flags gate libcd kick + block.  |
//! | [`prot_one_shot_load`]     | FUN_8003EB98  | 3-arg wrapper: `size_lookup` + `async_lba_load`. Returns LBA count. |
//! | [`kick_libcd_read`]        | FUN_8003F128  | Issue the libcd async request. Called from `async_lba_load`.        |
//! | [`read_wait_poll`]         | FUN_8003DE7C  | Per-frame read-wait poll. Drives the dual-mode state machine.       |
//!
//! ## Clean-room boundary
//!
//! No bytes from `SCUS_942.54` live in this crate. The five reference
//! dumps (`ghidra/scripts/funcs/8003de7c.txt`, `8003e800.txt`,
//! `8003e8a8.txt`, `8003eb98.txt`, `8003f128.txt`) are the *spec*.
//! Native implementations of this trait wrap the libcd-equivalent
//! API the host platform exposes; WASM / offline implementations
//! synthesise the calls against an in-memory disc image.
//!
//! REF: FUN_8003F2B8, FUN_8005C42C, FUN_8005C328, FUN_8005BE8C
//! REF: FUN_8005BECC, FUN_8005C034, FUN_8005FB84, FUN_8003ED04
//! REF: FUN_8003EE7C, FUN_8005BEE4, FUN_8005E788
//! REF: FUN_8003EBE4, FUN_8003EC70

/// PROT entry index (0..=1232 in retail).
pub type ProtIndex = u16;

/// Logical LBA (PSX 75-sector-per-second clock).
pub type Lba = u32;

/// Destination buffer address (RAM offset for the read). Engines
/// re-host this however they want - the retail value is an absolute
/// `0x800xxxxx` RAM pointer.
pub type DestAddr = u32;

/// Flags passed to [`CdDmaHost::async_lba_load`] and
/// [`CdDmaHost::prot_one_shot_load`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LoadFlags(u32);

impl LoadFlags {
    /// `bit 0` - issue the libcd async request now (FUN_8003F128).
    pub const ISSUE: Self = Self(0x1);
    /// `bit 1` - block on completion (call read_wait_poll until ready).
    pub const BLOCK: Self = Self(0x2);
    /// Combined "issue + block" - the common one-shot synchronous shape.
    pub const SYNCHRONOUS: Self = Self(0x3);

    pub const fn new(bits: u32) -> Self {
        Self(bits)
    }
    pub const fn bits(self) -> u32 {
        self.0
    }
    pub const fn issue(self) -> bool {
        self.0 & 0x1 != 0
    }
    pub const fn block(self) -> bool {
        self.0 & 0x2 != 0
    }
}

impl std::ops::BitOr for LoadFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Outcome of [`CdDmaHost::read_wait_poll`].
///
/// Mirrors the retail return convention of `FUN_8003DE7C`:
/// - `0` -> `Ready` (read complete, consumer can use the buffer).
/// - `1` -> `InProgress` (still streaming sectors).
/// - `2` -> `Idle` (no read pending; gated by `_DAT_8007BA70 == 0`
///   and `_DAT_8007B8C2 == 0`, i.e. only reachable on the dev arm).
///
/// Engines treat `Ready` as the green light for consuming the
/// destination buffer; `InProgress` is the wait-state; `Idle` is the
/// "no read was ever issued" case (typically the boot path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadWaitOutcome {
    /// Read complete - consumer can use the destination buffer.
    Ready,
    /// Read still streaming sectors. Caller should yield and re-poll
    /// next frame.
    InProgress,
    /// No read pending.
    Idle,
}

impl ReadWaitOutcome {
    /// Match the retail return: `0` = Ready, `1` = InProgress,
    /// `2` = Idle (the `iVar2 == 2` branch in FUN_8003DE7C).
    pub fn from_retail(v: u32) -> Self {
        match v {
            0 => Self::Ready,
            2 => Self::Idle,
            _ => Self::InProgress,
        }
    }
    /// Inverse of [`from_retail`].
    pub fn to_retail(self) -> u32 {
        match self {
            Self::Ready => 0,
            Self::InProgress => 1,
            Self::Idle => 2,
        }
    }
}

/// Engine-side hooks the streaming reader needs. Implementations
/// live in the platform layer (native libcd-backed, WASM in-memory,
/// test mock).
pub trait CdDmaHost {
    /// Look up the PROT entry's size and stash its start LBA + (when
    /// `set_msf` is true) the start MSF in the gp scratchpad.
    /// Mirrors `FUN_8003E8A8(prot_idx, set_msf)`:
    ///
    /// ```text
    ///   start_lba = PROT_TOC[prot_idx + 2]   // 0x801C70F0
    ///   next_lba  = PROT_TOC[prot_idx + 3]
    ///   gp[0x90C] = prot_idx
    ///   gp[0x8F0] = start_lba
    ///   if set_msf:
    ///       msf = LBA_to_BCD_MSF(start_lba)
    ///       gp[0x95C..0x95E] = msf
    ///   return next_lba - start_lba          // LBA count for the entry
    /// ```
    ///
    /// Engines that re-host the PROT TOC supply the LBA count
    /// however they like; the trait method's contract is
    /// "return the in-LBAs size".
    fn prot_index_size_lookup(&mut self, prot_idx: ProtIndex, set_msf: bool) -> u32;

    /// Queue an async sector read of `count` LBAs into `dst`.
    /// Mirrors `FUN_8003E800(dst, count, flags)`:
    ///
    /// ```text
    ///   if read_in_progress:
    ///       read_wait_poll(0)        // drain any stale read
    ///   if error_pending:
    ///       clear_error_state()      // FUN_8003ED04
    ///   if flags & 0x1:               // ISSUE
    ///       gp[0x97C] = count
    ///       gp[0x894] = dst
    ///       kick_libcd_read()
    ///   if flags & 0x2:               // BLOCK
    ///       read_wait_poll(0)
    ///   gp[0x91C] = 0x78              // refresh read-wait timeout
    /// ```
    fn async_lba_load(&mut self, dst: DestAddr, count: u32, flags: LoadFlags);

    /// One-shot PROT-by-index load. Combines
    /// [`prot_index_size_lookup`] + [`async_lba_load`]. Mirrors
    /// `FUN_8003EB98(prot_idx, dst, flags)`:
    ///
    /// ```text
    ///   count = prot_index_size_lookup(prot_idx, flags & 0x1)
    ///   async_lba_load(dst, count, flags)
    ///   return count
    /// ```
    ///
    /// Default implementation forwards to the two trait methods, so
    /// hosts that override the primitives get this for free.
    fn prot_one_shot_load(&mut self, prot_idx: ProtIndex, dst: DestAddr, flags: LoadFlags) -> u32 {
        let count = self.prot_index_size_lookup(prot_idx, flags.issue());
        self.async_lba_load(dst, count, flags);
        count
    }

    /// Issue the actual libcd async request. Mirrors
    /// `FUN_8003F128()`:
    ///
    /// ```text
    ///   libcd_init()                  // FUN_8005BEE4
    ///   libcd_set_callback(null)      // FUN_8005BECC
    ///   gp[0x988] = 1                 // read_in_progress = true
    ///   gp[0x940] = gp[0x894]         // mirror dst into libcd globals
    ///   gp[0x968] = gp[0x97C]         // mirror count
    ///   gp[0x8C8] = LBA_to_BCD(gp[0x95C..0x95E])
    ///   gp[0x8B8] = gp[0x8C8] + count
    ///   gp[0x980] = 1                 // state = busy
    ///   if CdSync(1) == 2:            // FUN_8005BE8C
    ///       gp[0x980] = 2             // state = ready
    ///       CdControlF(2, msf)        // FUN_8005C034
    ///   else:
    ///       gp[0x958] = 0xB4          // libcd timeout
    ///   libcd_set_callback(streaming) // FUN_8005BECC(&LAB_8003daa8)
    /// ```
    ///
    /// Internal helper; typically called from [`async_lba_load`]
    /// when [`LoadFlags::ISSUE`] is set. Exposed on the trait so
    /// engines can re-issue a kick after a timeout.
    fn kick_libcd_read(&mut self);

    /// Per-frame read-wait poll. Mirrors `FUN_8003DE7C(gated)`:
    ///
    /// - `gated = true`: the per-frame "is the read done?" probe.
    ///   Decrements `gp[0x91C]` (`DAT_1F800393` if the dev branch is
    ///   active), checks the state machine, returns `Ready` /
    ///   `InProgress` based on `gp[0x980]`.
    /// - `gated = false`: the "drain stale read" entry. Spins until
    ///   `gp[0x980] == 6` (snap-to-idle), clearing
    ///   `gp[0x988]`/`gp[0x980]` along the way. Always returns
    ///   `Ready` after draining.
    ///
    /// Dual-mode branching on `_DAT_8007B8C2`:
    /// - dev (0): the state machine doesn't touch libcd; the read
    ///   completion is signalled by the per-IRQ callback chain set
    ///   up via `FUN_8005E788`.
    /// - retail (!= 0, the value retail boots with): the state machine
    ///   drives libcd directly via `FUN_8003F2B8` + `FUN_8005FB84`
    ///   (VSync poll).
    fn read_wait_poll(&mut self, gated: bool) -> ReadWaitOutcome;
}

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
    prot: std::sync::Arc<crate::scene::ProtIndex>,
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

// =========================================================================
// Streaming-read completion sync - FUN_8005EA84
// =========================================================================

/// Overall streaming-read timeout in vsyncs (`0x4B0` = 1200 frames = 20
/// seconds NTSC). Measured from the read-start timestamp `DAT_800796E0`;
/// once exceeded, [`stream_read_sync`] reports [`STREAM_SYNC_TIMED_OUT`].
pub const STREAM_SYNC_TIMEOUT_VSYNCS: i32 = 0x4B0;

/// Per-sector stall window in vsyncs (`0x3C` = 60 frames = 1 second NTSC).
/// Measured from the last-sector-IRQ timestamp `DAT_800796DC`; once
/// exceeded, [`stream_read_sync`] restarts the streaming read.
pub const STREAM_SYNC_STALL_VSYNCS: i32 = 0x3C;

/// Return value of [`stream_read_sync`] when the overall timeout expired
/// (the `li s0,-0x1` delay-slot default at `0x8005EAC8` that survives to
/// the return when the timeout branch is taken).
pub const STREAM_SYNC_TIMED_OUT: i32 = -1;

/// Host surface for the streaming-read completion sync
/// ([`stream_read_sync`], the port of `FUN_8005EA84`). Maps 1:1 onto the
/// retail globals + callees the function touches:
///
/// | Method                    | Retail                                        |
/// |---------------------------|-----------------------------------------------|
/// | `vsync_now`               | `FUN_8005FB84(-1)` (VSync counter read)       |
/// | `read_start_vsync`        | `DAT_800796E0` (read-start timestamp)         |
/// | `last_sector_vsync`       | `DAT_800796DC` (last-sector-IRQ timestamp)    |
/// | `sectors_remaining`       | `DAT_800796D8` (negative = error state)       |
/// | `total_sector_count`      | `DAT_800796C4` (request's full sector count)  |
/// | `restart_streaming_read`  | `FUN_8005E788(1)` (re-arm the IRQ chain)      |
/// | `deliver_status`          | `FUN_8005BEAC(1, result)` (CdSync-shape poll) |
pub trait StreamReadSyncHost {
    /// Current vsync counter (`FUN_8005FB84(-1)` - absolute count, no wait).
    fn vsync_now(&mut self) -> i32;
    /// Vsync timestamp at which the streaming read started (`DAT_800796E0`).
    fn read_start_vsync(&self) -> i32;
    /// Vsync timestamp of the most recent sector-IRQ (`DAT_800796DC`).
    fn last_sector_vsync(&self) -> i32;
    /// Sectors still outstanding (`DAT_800796D8`). Negative marks the
    /// error state the IRQ callback leaves behind on a failed read.
    fn sectors_remaining(&self) -> i32;
    /// Full sector count of the current request (`DAT_800796C4`).
    fn total_sector_count(&self) -> i32;
    /// Restart the streaming read (`FUN_8005E788(1)`): re-copies the
    /// source globals and re-registers the per-IRQ callback.
    fn restart_streaming_read(&mut self);
    /// Deliver the drive status (`FUN_8005BEAC(1, result)`, which forwards
    /// both register arguments into the CdSync-shaped poller
    /// `FUN_8005CCB4`). `result` receives the status bytes when the host
    /// has them; retail callers pass a small stack buffer.
    fn deliver_status(&mut self, result: Option<&mut [u8]>);
}

/// PORT: FUN_8005EA84
///
/// Streaming-read completion sync - the libcd-layer `CdReadSync`-shaped
/// poll/block primitive above the
/// per-IRQ streaming reader (`FUN_8005E574` / `FUN_8005E788`). Two modes,
/// selected by `poll_once` (retail `a0`):
///
/// - `poll_once = true` (retail `a0 = 1`): one health-check pass, then
///   return. Used by the boot TOC loader `FUN_8003E4E8` and the path
///   loader `FUN_8003D3C4`, which loop at the call site.
/// - `poll_once = false` (retail `a0 = 0`): block until the read
///   completes (`sectors_remaining` reaches 0) or the overall timeout
///   expires. Used by the sync LBA reader `FUN_8005E4D4`.
///
/// Each pass (loop head `0x8005EAAC`):
///
/// 1. Default the report to [`STREAM_SYNC_TIMED_OUT`].
/// 2. If `vsync_now() > read_start + 0x4B0`, the whole read timed out -
///    keep the `-1` and fall through to the exit check.
/// 3. Otherwise, a negative `sectors_remaining` (IRQ-side error state)
///    or a stalled transfer (`last_sector + 0x3C < vsync_now()`, i.e.
///    more than 1 s without a sector IRQ) restarts the stream via
///    `restart_streaming_read` and reports the full
///    `total_sector_count`; a healthy transfer reports
///    `sectors_remaining` as-is.
/// 4. Exit when `poll_once` is set, or when the report is `<= 0`
///    (complete, or timed out).
///
/// On exit the drive status is delivered into `result` via
/// `deliver_status` (retail `FUN_8005BEAC(1, result)`), and the last
/// report is returned: sectors remaining, `0` on completion, or
/// [`STREAM_SYNC_TIMED_OUT`].
///
/// NOT WIRED: nothing implements [`StreamReadSyncHost`]. The engine reads
/// PROT entries and named files synchronously out of an in-memory disc image
/// through `legaia_iso`, so there is no IRQ-driven sector chain to wait on -
/// no outstanding-sector counter, no per-sector IRQ timestamp, and no vsync
/// counter to time the two watchdogs against. Every value the trait asks for
/// would have to be invented. Wiring it needs an asynchronous sector reader
/// behind the asset loader, which is the same prerequisite the streaming
/// half of this module is waiting on.
pub fn stream_read_sync(
    host: &mut impl StreamReadSyncHost,
    poll_once: bool,
    result: Option<&mut [u8]>,
) -> i32 {
    let mut report;
    loop {
        // Delay-slot default at 0x8005EAC8: s0 = -1 (timed out).
        report = STREAM_SYNC_TIMED_OUT;
        let now = host.vsync_now();
        if now <= host.read_start_vsync() + STREAM_SYNC_TIMEOUT_VSYNCS {
            let stalled = if host.sectors_remaining() < 0 {
                // bltz at 0x8005EAD4: IRQ-side error state.
                true
            } else {
                // Second vsync read at 0x8005EADC: per-sector stall check.
                let now = host.vsync_now();
                host.last_sector_vsync() + STREAM_SYNC_STALL_VSYNCS < now
            };
            if stalled {
                host.restart_streaming_read();
                report = host.total_sector_count();
            } else {
                // Fresh re-read at 0x8005EB10 - the IRQ chain may have
                // delivered sectors between the two loads.
                report = host.sectors_remaining();
            }
        }
        // Exit check at 0x8005EB14: poll mode returns after one pass;
        // block mode loops while the report stays positive.
        if poll_once || report <= 0 {
            break;
        }
    }
    host.deliver_status(result);
    report
}

/// LBA → BCD `(minutes, seconds, frames)` triple. Mirrors the retail
/// helper at `FUN_8005c42c` + `FUN_8005c328` chain used inside
/// `FUN_8003e8a8` to materialise the per-request MSF into
/// `gp+0x95C..+0x95E`. The 2-second pregap offset is folded in (the
/// PSX clock starts the data area at MSF `00:02:00`, i.e. LBA 0 →
/// `(0, 2, 0)`).
fn lba_to_bcd_msf(lba: u32) -> (u8, u8, u8) {
    let lba = lba.wrapping_add(150);
    let mins = lba / (60 * 75);
    let secs = (lba / 75) % 60;
    let frames = lba % 75;
    (
        bin_to_bcd(mins as u8),
        bin_to_bcd(secs as u8),
        bin_to_bcd(frames as u8),
    )
}

fn bin_to_bcd(v: u8) -> u8 {
    ((v / 10) << 4) | (v % 10)
}

// =========================================================================
// StreamLoadQueue - index-based async streaming enqueue (FUN_8003DDA0)
// =========================================================================

/// One entry in the boot-time async streaming load queue built by
/// [`StreamLoadQueue::enqueue`]. Retail stores an 8-byte descriptor at
/// `gp+0x1A8 + n*8`: `[u32 prot_idx][u32 byte_offset]`.
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
///   desc         = gp+0x1A8 + n*8
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
    /// The `gp+0x1A8` descriptor table; length is retail's queued-entry
    /// count `gp+0x8BC`.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic host that fakes a 4-sector LBA range per PROT
    /// entry. Used by the smoke tests.
    #[derive(Default)]
    struct FakeCdHost {
        calls: Vec<String>,
        sizes: std::collections::HashMap<ProtIndex, u32>,
        ready: bool,
    }

    impl CdDmaHost for FakeCdHost {
        fn prot_index_size_lookup(&mut self, prot_idx: ProtIndex, set_msf: bool) -> u32 {
            self.calls
                .push(format!("size_lookup({prot_idx}, {set_msf})"));
            self.sizes.get(&prot_idx).copied().unwrap_or(4)
        }
        fn async_lba_load(&mut self, dst: DestAddr, count: u32, flags: LoadFlags) {
            self.calls.push(format!(
                "async_load({dst:#x}, {count}, {:#x})",
                flags.bits()
            ));
            if flags.issue() {
                self.kick_libcd_read();
            }
        }
        fn kick_libcd_read(&mut self) {
            self.calls.push("kick".into());
            self.ready = true;
        }
        fn read_wait_poll(&mut self, gated: bool) -> ReadWaitOutcome {
            self.calls.push(format!("wait_poll({gated})"));
            if self.ready {
                ReadWaitOutcome::Ready
            } else {
                ReadWaitOutcome::InProgress
            }
        }
    }

    #[test]
    fn load_flags_bit_decode_round_trips() {
        assert!(LoadFlags::ISSUE.issue());
        assert!(!LoadFlags::ISSUE.block());
        assert!(LoadFlags::BLOCK.block());
        assert!(!LoadFlags::BLOCK.issue());
        assert!(LoadFlags::SYNCHRONOUS.issue());
        assert!(LoadFlags::SYNCHRONOUS.block());
        // Bit-or composes correctly.
        let f = LoadFlags::ISSUE | LoadFlags::BLOCK;
        assert!(f.issue());
        assert!(f.block());
    }

    #[test]
    fn read_wait_outcome_round_trips_retail_codes() {
        for code in 0..=2 {
            let r = ReadWaitOutcome::from_retail(code);
            assert_eq!(r.to_retail(), code);
        }
        // Unknown codes fold into InProgress (the retail catch-all).
        assert_eq!(
            ReadWaitOutcome::from_retail(99),
            ReadWaitOutcome::InProgress
        );
        assert_eq!(ReadWaitOutcome::InProgress.to_retail(), 1);
    }

    #[test]
    fn one_shot_default_chains_size_lookup_and_async_load() {
        let mut h = FakeCdHost::default();
        let n = h.prot_one_shot_load(42, 0x80100000, LoadFlags::SYNCHRONOUS);
        // Default LBA count from the fake = 4.
        assert_eq!(n, 4);
        assert_eq!(
            h.calls,
            vec![
                "size_lookup(42, true)".to_string(),
                "async_load(0x80100000, 4, 0x3)".to_string(),
                "kick".to_string(),
            ]
        );
    }

    #[test]
    fn one_shot_without_issue_skips_kick() {
        let mut h = FakeCdHost::default();
        // Empty flags = neither ISSUE nor BLOCK.
        let n = h.prot_one_shot_load(7, 0x80200000, LoadFlags::default());
        assert_eq!(n, 4);
        assert_eq!(
            h.calls,
            vec![
                "size_lookup(7, false)".to_string(),
                "async_load(0x80200000, 4, 0x0)".to_string(),
            ]
        );
    }

    #[test]
    fn poll_reports_ready_after_kick() {
        let mut h = FakeCdHost::default();
        assert_eq!(h.read_wait_poll(true), ReadWaitOutcome::InProgress);
        h.kick_libcd_read();
        assert_eq!(h.read_wait_poll(true), ReadWaitOutcome::Ready);
    }

    #[test]
    fn host_records_size_override() {
        let mut h = FakeCdHost::default();
        h.sizes.insert(100, 17);
        assert_eq!(h.prot_index_size_lookup(100, false), 17);
        assert_eq!(h.prot_index_size_lookup(101, false), 4); // default
    }

    // -- ProtCdDmaHost tests -------------------------------------------

    /// Build a minimal valid PROT.DAT byte image with two entries:
    ///
    /// - entry 0: `start_lba = 1`, retail LBA count = 4 (footprint).
    /// - entry 1: `start_lba = 5`, retail LBA count = 2.
    ///
    /// Entry 0's 4 sectors are filled with `0xAA`, entry 1's 2 sectors
    /// with `0xBB`, so a successful read can be detected by checking
    /// the destination buffer's first byte.
    fn build_synthetic_prot_dat() -> Vec<u8> {
        const TOTAL_BYTES: usize = 0x3800;
        let mut buf = vec![0u8; TOTAL_BYTES];
        // Header head at offset 0: [unused u32][file_num_minus_1 u32].
        // `header_sectors` aliases `toc[0]` in the on-disc layout (the
        // `detect_header` reader pulls bytes 8..12, and the TOC slice
        // starts at byte 8). We set header_sectors = 1 via toc[0] below.
        buf[0..4].copy_from_slice(&0u32.to_le_bytes());
        buf[4..8].copy_from_slice(&2u32.to_le_bytes()); // file_num - 1 = 2 (file_num = 3)
        // TOC starts at file offset 0x08. We write 7 dwords:
        //   toc[0] = 1                       (= header_sectors)
        //   toc[2] = 1, toc[3] = 5,
        //   toc[4] = 7, toc[5] = 6, toc[6] = 5
        // Retail-formula counts: toc[3]-toc[2]=4, toc[4]-toc[3]=2.
        // Archive-parser indexed sizes: toc[5]-toc[3]+4=5, toc[6]-toc[4]+4=2.
        const TOC: [u32; 7] = [1, 0, 1, 5, 7, 6, 5];
        for (i, v) in TOC.iter().enumerate() {
            let off = 0x08 + i * 4;
            buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
        }
        // Entry 0 body: LBA 1..5 (4 sectors), filled with 0xAA.
        for b in &mut buf[0x0800..0x2800] {
            *b = 0xAA;
        }
        // Entry 1 body: LBA 5..7 (2 sectors), filled with 0xBB.
        for b in &mut buf[0x2800..0x3800] {
            *b = 0xBB;
        }
        buf
    }

    fn make_synthetic_host() -> ProtCdDmaHost {
        let bytes = build_synthetic_prot_dat();
        let prot =
            crate::scene::ProtIndex::from_bytes(bytes, None).expect("parse synthetic PROT.DAT");
        ProtCdDmaHost::new(std::sync::Arc::new(prot))
    }

    #[test]
    fn synthetic_prot_dat_yields_two_entries() {
        let host = make_synthetic_host();
        assert_eq!(host.prot.entry_count(), 2);
        // Retail formula matches what we crafted.
        assert_eq!(host.prot.entry_start_lba_retail(0), Some(1));
        assert_eq!(host.prot.entry_lba_count_retail(0), Some(4));
        assert_eq!(host.prot.entry_start_lba_retail(1), Some(5));
        assert_eq!(host.prot.entry_lba_count_retail(1), Some(2));
    }

    #[test]
    fn prot_size_lookup_stashes_start_lba_and_returns_count() {
        let mut host = make_synthetic_host();
        let n = host.prot_index_size_lookup(0, false);
        assert_eq!(n, 4);
        assert_eq!(host.last_prot_idx(), 0);
        assert_eq!(host.last_start_lba(), 1);
        assert!(host.last_msf().is_none(), "set_msf=false must not set MSF");
        let n = host.prot_index_size_lookup(1, true);
        assert_eq!(n, 2);
        assert_eq!(host.last_start_lba(), 5);
        // BCD MSF: (5+150) sectors = 155 sectors = 0:02:05 + 2-sec pregap.
        // Actually 155/75 = 2 seconds 5 frames. mins=0, secs=2, frames=5.
        assert_eq!(host.last_msf(), Some((0x00, 0x02, 0x05)));
    }

    #[test]
    fn prot_size_lookup_out_of_range_returns_zero() {
        let mut host = make_synthetic_host();
        // Way past the last valid TOC entry - retail formula wraps,
        // but our None-on-out-of-range fallback yields zero.
        let n = host.prot_index_size_lookup(u16::MAX, false);
        assert_eq!(n, 0);
    }

    #[test]
    fn one_shot_load_copies_entry_bytes_into_main_ram() {
        let mut host = make_synthetic_host();
        let dst: DestAddr = 0x8010_0000;
        let n = host.prot_one_shot_load(0, dst, LoadFlags::SYNCHRONOUS);
        assert_eq!(n, 4, "retail LBA count for entry 0");
        // The 4 sectors (0x2000 bytes) of entry 0 are 0xAA.
        let slice = host.read(dst, 0x2000).expect("read back");
        assert!(slice.iter().all(|&b| b == 0xAA), "entry 0 bytes mismatch");
        // Just past the read window the buffer is still zero.
        let tail = host.read(dst + 0x2000, 1).unwrap();
        assert_eq!(tail, &[0u8]);
    }

    #[test]
    fn one_shot_load_for_entry_1_copies_at_a_different_offset() {
        let mut host = make_synthetic_host();
        let dst: DestAddr = 0x8014_0000;
        let n = host.prot_one_shot_load(1, dst, LoadFlags::SYNCHRONOUS);
        assert_eq!(n, 2);
        let slice = host.read(dst, 0x1000).expect("read back");
        assert!(slice.iter().all(|&b| b == 0xBB), "entry 1 bytes mismatch");
    }

    #[test]
    fn poll_after_synchronous_kick_reports_ready() {
        let mut host = make_synthetic_host();
        // Boot state: no kick issued => poll returns Ready immediately
        // (matches retail FUN_8003de7c's `read_in_progress == 0` early-out).
        assert_eq!(host.read_wait_poll(true), ReadWaitOutcome::Ready);
        // The synchronous one-shot load completes inline. Poll still Ready.
        host.prot_one_shot_load(0, 0x8010_0000, LoadFlags::SYNCHRONOUS);
        assert_eq!(host.read_wait_poll(true), ReadWaitOutcome::Ready);
        // Drain returns Ready unconditionally and the post-drain poll is
        // still Ready (state is back to the boot configuration).
        assert_eq!(host.read_wait_poll(false), ReadWaitOutcome::Ready);
        assert_eq!(host.read_wait_poll(true), ReadWaitOutcome::Ready);
    }

    #[test]
    fn issue_without_block_still_drives_state_to_ready() {
        // The offline host's "issue" path performs the copy synchronously,
        // so the BLOCK bit doesn't gate anything in practice - the buffer
        // is already populated after the kick.
        let mut host = make_synthetic_host();
        host.prot_index_size_lookup(0, false);
        host.async_lba_load(0x8010_0000, 4, LoadFlags::ISSUE);
        assert_eq!(host.read_wait_poll(true), ReadWaitOutcome::Ready);
        assert_eq!(host.last_count(), 4);
        assert_eq!(host.last_dst(), 0x8010_0000);
    }

    #[test]
    fn high_psx_pointers_fold_into_synthetic_main_ram() {
        // Retail uses both kseg0 (0x80xxxxxx) and kseg1 (0xA0xxxxxx) for
        // DMA targets; both should alias the same offset.
        let mut host = make_synthetic_host();
        host.prot_one_shot_load(0, 0xA010_0000, LoadFlags::SYNCHRONOUS);
        let folded = host.read(0x8010_0000, 0x2000).unwrap();
        assert!(folded.iter().all(|&b| b == 0xAA));
    }

    #[test]
    fn overlay_loader_a_drives_prot_cd_dma_host_end_to_end() {
        use crate::overlay_loader::{
            OVERLAY_CACHE_EMPTY, OVERLAY_PROT_BASE, OverlayCacheSlot, load_overlay_a,
        };
        let mut host = make_synthetic_host();
        host.set_overlay_destinations(0x8010_0000, 0x8011_0000);
        // The synthetic PROT only has 2 entries; the overlay loaders' real
        // call site uses param values whose `+ OVERLAY_PROT_BASE` resolves
        // to a real overlay PROT index. For the smoke test we accept the
        // synthetic PROT's "garbage size" return - the wiring is what we
        // verify.
        let param = -(OVERLAY_PROT_BASE); // → prot_idx 0
        let result = load_overlay_a(&mut host, param);
        assert_eq!(result, param, "fresh load returns param");
        assert_eq!(host.overlay_slot(OverlayCacheSlot::A), param);
        assert_eq!(
            host.overlay_slot(OverlayCacheSlot::B),
            OVERLAY_CACHE_EMPTY,
            "sister slot invalidated"
        );
        assert_eq!(host.last_prot_idx(), 0, "PROT 0 was looked up");
        assert_eq!(host.last_dst(), 0x8010_0000);
    }

    #[test]
    fn overlay_loader_b_drives_prot_cd_dma_host_end_to_end() {
        use crate::overlay_loader::{OVERLAY_PROT_BASE, OverlayCacheSlot, load_overlay_b};
        let mut host = make_synthetic_host();
        host.set_overlay_destinations(0x8010_0000, 0x8011_0000);
        let param = -(OVERLAY_PROT_BASE); // → prot_idx 0
        let result = load_overlay_b(&mut host, param);
        assert_eq!(result, param);
        assert_eq!(host.overlay_slot(OverlayCacheSlot::B), param);
        assert_eq!(host.last_dst(), 0x8011_0000, "uses slot B's dst buffer");
    }

    #[test]
    fn overlay_loader_dev_branch_short_circuits_real_host() {
        use crate::overlay_loader::{OverlayCacheSlot, load_overlay_a};
        let mut host = make_synthetic_host();
        host.set_overlay_dev_flag(0x42);
        // Dev branch: stash and return the flag value. No PROT load.
        let last_dst_before = host.last_dst();
        let result = load_overlay_a(&mut host, 100);
        assert_eq!(result, 0x42);
        assert_eq!(host.overlay_slot(OverlayCacheSlot::A), 100);
        assert_eq!(
            host.last_dst(),
            last_dst_before,
            "dev branch must not trigger a CD-DMA read"
        );
    }

    // -- stream_read_sync (FUN_8005EA84) tests -------------------------

    /// Scripted host for the streaming-read sync: the vsync counter
    /// advances by one per read, and `sectors_remaining` counts down by
    /// one per vsync read past `complete_at` (simulating the IRQ chain
    /// delivering sectors while the CPU polls).
    struct FakeSyncHost {
        vsync: i32,
        start_vsync: i32,
        last_sector_vsync: i32,
        sectors: i32,
        total: i32,
        restarts: u32,
        delivered: u32,
        /// When `Some(n)`, each `vsync_now` call past `n` decrements
        /// `sectors` (floor 0) and refreshes `last_sector_vsync`.
        drain_after: Option<i32>,
    }

    impl FakeSyncHost {
        fn healthy(sectors: i32) -> Self {
            Self {
                vsync: 100,
                start_vsync: 100,
                last_sector_vsync: 100,
                sectors,
                total: sectors,
                restarts: 0,
                delivered: 0,
                drain_after: None,
            }
        }
    }

    impl StreamReadSyncHost for FakeSyncHost {
        fn vsync_now(&mut self) -> i32 {
            self.vsync += 1;
            if let Some(n) = self.drain_after
                && self.vsync > n
                && self.sectors > 0
            {
                self.sectors -= 1;
                self.last_sector_vsync = self.vsync;
            }
            self.vsync
        }
        fn read_start_vsync(&self) -> i32 {
            self.start_vsync
        }
        fn last_sector_vsync(&self) -> i32 {
            self.last_sector_vsync
        }
        fn sectors_remaining(&self) -> i32 {
            self.sectors
        }
        fn total_sector_count(&self) -> i32 {
            self.total
        }
        fn restart_streaming_read(&mut self) {
            self.restarts += 1;
            self.sectors = self.total;
            self.last_sector_vsync = self.vsync;
        }
        fn deliver_status(&mut self, result: Option<&mut [u8]>) {
            self.delivered += 1;
            if let Some(buf) = result
                && let Some(b) = buf.first_mut()
            {
                *b = 0x02; // "ready" status marker for the test
            }
        }
    }

    #[test]
    fn stream_sync_poll_reports_remaining_without_restart() {
        let mut h = FakeSyncHost::healthy(5);
        let n = stream_read_sync(&mut h, true, None);
        assert_eq!(n, 5, "healthy in-flight read reports sectors remaining");
        assert_eq!(h.restarts, 0);
        assert_eq!(h.delivered, 1, "status delivered exactly once on exit");
    }

    #[test]
    fn stream_sync_poll_reports_zero_when_complete() {
        let mut h = FakeSyncHost::healthy(0);
        assert_eq!(stream_read_sync(&mut h, true, None), 0);
        assert_eq!(h.restarts, 0);
    }

    #[test]
    fn stream_sync_block_loops_until_drained() {
        let mut h = FakeSyncHost::healthy(3);
        h.drain_after = Some(100); // sectors arrive from the first poll on
        let n = stream_read_sync(&mut h, false, None);
        assert_eq!(n, 0, "block mode returns only once the read completes");
        assert_eq!(h.restarts, 0);
        assert_eq!(h.delivered, 1);
    }

    #[test]
    fn stream_sync_overall_timeout_reports_minus_one() {
        let mut h = FakeSyncHost::healthy(4);
        // Jump the clock past start + 0x4B0: the next vsync read exceeds
        // the overall window, so even block mode exits with -1.
        h.vsync = h.start_vsync + STREAM_SYNC_TIMEOUT_VSYNCS;
        assert_eq!(stream_read_sync(&mut h, false, None), STREAM_SYNC_TIMED_OUT);
        assert_eq!(h.restarts, 0, "timeout path does not restart the read");
        assert_eq!(h.delivered, 1, "status still delivered on the way out");
    }

    #[test]
    fn stream_sync_error_state_restarts_and_reports_total() {
        let mut h = FakeSyncHost::healthy(6);
        h.sectors = -1; // IRQ-side error state (DAT_800796D8 < 0)
        let n = stream_read_sync(&mut h, true, None);
        assert_eq!(n, 6, "restart reports the request's full sector count");
        assert_eq!(h.restarts, 1);
    }

    #[test]
    fn stream_sync_stall_restarts_and_reports_total() {
        let mut h = FakeSyncHost::healthy(2);
        // Last sector arrived long ago: > 0x3C vsyncs before "now".
        h.last_sector_vsync = h.vsync - STREAM_SYNC_STALL_VSYNCS - 2;
        let n = stream_read_sync(&mut h, true, None);
        assert_eq!(n, 2, "total == remaining here; restart path taken");
        assert_eq!(h.restarts, 1, "stalled transfer is re-armed");
    }

    #[test]
    fn stream_sync_fresh_transfer_is_not_stalled() {
        let mut h = FakeSyncHost::healthy(2);
        // Exactly at the stall boundary: last + 0x3C == now is NOT a
        // stall (retail slt is strict less-than).
        h.last_sector_vsync = h.vsync + 2 - STREAM_SYNC_STALL_VSYNCS;
        let n = stream_read_sync(&mut h, true, None);
        assert_eq!(n, 2);
        assert_eq!(h.restarts, 0, "boundary case must not restart");
    }

    #[test]
    fn stream_sync_writes_status_into_result_buffer() {
        let mut h = FakeSyncHost::healthy(0);
        let mut status = [0u8; 8];
        stream_read_sync(&mut h, true, Some(&mut status));
        assert_eq!(status[0], 0x02, "deliver_status saw the result buffer");
    }

    #[test]
    fn lba_to_bcd_msf_round_trips_known_landmarks() {
        // LBA 0 is the PSX-pregap entry: MSF 00:02:00.
        assert_eq!(lba_to_bcd_msf(0), (0x00, 0x02, 0x00));
        // LBA 75 = 1 second past the pregap: MSF 00:03:00.
        assert_eq!(lba_to_bcd_msf(75), (0x00, 0x03, 0x00));
        // LBA 60*75 = 1 minute past pregap: MSF 01:02:00.
        assert_eq!(lba_to_bcd_msf(60 * 75), (0x01, 0x02, 0x00));
    }

    // ---- StreamLoadQueue (FUN_8003DDA0) ----

    #[test]
    fn stream_queue_first_enqueue_lands_at_offset_zero() {
        let mut q = StreamLoadQueue::new();
        // Entry 897 spans 4 sectors (start 1000, next 1004).
        let d = q.enqueue(897, 1000, 1004);
        assert_eq!(
            d,
            StreamLoadDescriptor {
                prot_idx: 897,
                byte_offset: 0
            }
        );
        assert_eq!(q.count(), 1);
        assert_eq!(q.last_prot_idx(), 897);
        assert_eq!(q.last_start_lba(), 1000);
        assert_eq!(q.read_wait_timeout(), 0x78);
        // 4 sectors * 0x800 = 0x2000 bytes queued.
        assert_eq!(q.byte_cursor(), 4 * 0x800);
    }

    #[test]
    fn stream_queue_accumulates_byte_cursor_across_entries() {
        let mut q = StreamLoadQueue::new();
        q.enqueue(10, 0, 3); // 3 sectors -> +0x1800
        let d2 = q.enqueue(11, 100, 105); // 5 sectors -> +0x2800
        // Second entry's payload lands right after the first (0x1800).
        assert_eq!(d2.byte_offset, 3 * 0x800);
        assert_eq!(q.byte_cursor(), (3 + 5) * 0x800);
        assert_eq!(q.count(), 2);
        assert_eq!(
            q.entries()[0],
            StreamLoadDescriptor {
                prot_idx: 10,
                byte_offset: 0
            }
        );
        // last_* track the most recent enqueue only.
        assert_eq!(q.last_prot_idx(), 11);
        assert_eq!(q.last_start_lba(), 100);
    }

    #[test]
    fn stream_queue_from_toc_reads_idx_plus_2_and_3() {
        // Raw in-RAM TOC head: word[idx+2] = start, word[idx+3] = next.
        // Lay out so entry idx=1 -> start=toc[3], next=toc[4].
        let toc: Vec<u32> = vec![0, 0, 0, 500, 512, 600];
        let mut q = StreamLoadQueue::new();
        let d = q.enqueue_from_toc(1, &toc).expect("idx+3 in range");
        assert_eq!(d.prot_idx, 1);
        assert_eq!(q.last_start_lba(), 500);
        // 512 - 500 = 12 sectors * 0x800.
        assert_eq!(q.byte_cursor(), 12 * 0x800);
    }

    #[test]
    fn stream_queue_from_toc_out_of_range_returns_none() {
        let toc: Vec<u32> = vec![0, 0, 500];
        let mut q = StreamLoadQueue::new();
        // idx=0 needs toc[2] and toc[3]; toc[3] is missing.
        assert!(q.enqueue_from_toc(0, &toc).is_none());
        assert_eq!(q.count(), 0, "a failed lookup must not mutate the queue");
    }

    #[test]
    fn stream_queue_degenerate_size_does_not_panic() {
        let mut q = StreamLoadQueue::new();
        // next < start: retail computes a wrapping size; port must not panic.
        let d = q.enqueue(5, 200, 100);
        assert_eq!(d.byte_offset, 0);
        assert_eq!(q.count(), 1);
    }
}
