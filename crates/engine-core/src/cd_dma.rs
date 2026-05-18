//! CD-DMA streaming reader host-trait abstractions.
//!
//! PORT: FUN_8003DE7C, FUN_8003E800, FUN_8003E8A8, FUN_8003EB98, FUN_8003F128
//!
//! Five SCUS-resident helpers around the CD streaming reader. They
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
//! | `_DAT_8007B8C2` | retail/dev branch selector (0 = retail ISO9660 path,  |
//! |                 |  != 0 = debug PROT-index path)                        |
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

/// PROT entry index (0..1234 in retail).
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
/// - `2` -> the retail `Idle` (no read pending; gated by
///   `_DAT_8007BA70 == 0` and `_DAT_8007B8C2 == 0`).
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
    /// - retail (0): the state machine doesn't touch libcd; the read
    ///   completion is signalled by the per-IRQ callback chain set
    ///   up via `FUN_8005E788`.
    /// - debug (!= 0): the state machine drives libcd directly via
    ///   `FUN_8003F2B8` + `FUN_8005FB84` (VSync poll).
    fn read_wait_poll(&mut self, gated: bool) -> ReadWaitOutcome;
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
}
