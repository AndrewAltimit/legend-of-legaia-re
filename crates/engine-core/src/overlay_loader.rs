//! Mode-table parallel overlay loaders.
//!
//! PORT: FUN_8003EBE4, FUN_8003EC70
//!
//! ## NOT WIRED
//!
//! Applies to every anchor in this file. The host trait side is finished -
//! `OverlayLoaderHost for ProtCdDmaHost` lives in [`crate::cd_dma`] - what
//! is missing is a caller. The engine has no mode-table overlay-residency
//! model: it resolves PROT entries on demand through the scene host and
//! keeps no `gp+0x924` / `gp+0x934` cache pair, so there is no dispatcher to
//! route a paired parallel load through. Wiring [`load_overlay_a`] /
//! [`load_overlay_b`] needs that residency model first.
//!
//! [`battle_stage_overlay_entry`] is inert for a narrower reason: the engine
//! carries no per-formation stage id, so nothing produces the
//! `_DAT_8007B64A` value it maps. The one battle that pages a stage overlay
//! is primed by the host instead, through `World::prime_battle_tutorial`.
//!
//! Two SCUS-resident wrappers around [`crate::cd_dma::CdDmaHost::prot_one_shot_load`]
//! that the mode-table dispatcher uses to stream the active scene's pair of
//! overlay PROT entries (extraction `param + 0x37F`; retail raw-TOC
//! `param + 0x381`) into RAM. Each loader carries its own
//! cache slot, destination buffer, and sister-cache invalidation so the
//! mode-table machinery can hold two distinct overlays resident in parallel
//! (one read from `*DAT_8001038C`, the other from `*DAT_80010390`).
//!
//! ## Function map
//!
//! | Method            | SCUS function | Role                                                                            |
//! |-------------------|---------------|---------------------------------------------------------------------------------|
//! | [`load_overlay_a`] | FUN_8003EBE4  | Cache-key `gp+0x924`, dst `*DAT_8001038C`. On a fresh load, invalidates `gp+0x934`. |
//! | [`load_overlay_b`] | FUN_8003EC70  | Cache-key `gp+0x934`, dst `*DAT_80010390`. Force-invalidates its own cache when `_DAT_8007B83C == 0x15`. |
//!
//! Both functions:
//!
//! 1. Branch on the dev/retail flag `_DAT_8007B868`. When the flag is
//!    non-zero the dev path stashes `param` in its cache slot and returns
//!    early - the actual streaming load is skipped.
//! 2. Otherwise, compare `param` against the cache slot. A match short-
//!    circuits ("already loaded"); a miss issues an
//!    [`CdDmaHost::prot_one_shot_load`] of extraction PROT entry
//!    `param + 0x37F` (retail raw-TOC `param + 0x381` over the
//!    header-included in-RAM TOC - same entry, see [`OVERLAY_PROT_BASE`])
//!    into the loader's destination buffer with [`LoadFlags::ISSUE`] only
//!    (async; the caller polls completion through the read-wait helpers).
//! 3. Update the cache slots: the loader writes its own slot to `param`,
//!    and FUN_8003EBE4 additionally clears the sister slot (`gp+0x934 = -1`)
//!    so a subsequent FUN_8003EC70 call will re-load.
//!
//! ## Why two parallel loaders?
//!
//! The mode-table parallel overlay loaders are the discovery from the
//! `cd-read-api-stack` work catalogued in the memory map: most retail mode
//! transitions install **two** overlays - a "scene driver" overlay and a
//! "scene resources" overlay - which live at different RAM addresses and
//! cache independently. The two cache slots prevent re-loading the same
//! overlay across mode bounces; the sister-invalidate captures the case
//! where the same PROT entry is staged for the other slot.
//!
//! ## Clean-room boundary
//!
//! No bytes from `SCUS_942.54` live in this crate. The two reference dumps
//! (`ghidra/scripts/funcs/8003ebe4.txt`, `8003ec70.txt`) are the *spec*.
//! Native and offline implementations of [`OverlayLoaderHost`] sit on top
//! of whatever [`CdDmaHost`] impl the platform layer uses.
//!
//! REF: FUN_8003E800, FUN_8003E8A8, FUN_8003EB98

use crate::cd_dma::{CdDmaHost, DestAddr, LoadFlags};

/// Identifies one of the two parallel overlay loader slots.
///
/// Maps to the two retail cache globals + destination buffer pointers:
///
/// | Variant     | Cache slot (gp) | Destination ptr (DAT_) |
/// |-------------|-----------------|------------------------|
/// | [`Self::A`] | `gp+0x924`      | `*DAT_8001038C`        |
/// | [`Self::B`] | `gp+0x934`      | `*DAT_80010390`        |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayCacheSlot {
    /// Loader A (FUN_8003EBE4).
    A,
    /// Loader B (FUN_8003EC70).
    B,
}

/// Sentinel cache value indicating "no overlay resident". Retail uses
/// `-1` (signed 32-bit) when invalidating; we expose the same value so
/// host impls and tests share a single source of truth.
pub const OVERLAY_CACHE_EMPTY: i32 = -1;

/// PROT-index offset added to the caller-supplied `param` to land on
/// the loaded PROT entry, in **extraction index space**. The offset is
/// shared by both loaders.
///
/// Index space: retail computes `raw_idx = param + 0x381` against the
/// in-RAM TOC at `0x801C70F0`, which is raw `PROT.DAT` from byte 0
/// (header included); the per-entry extraction index sits 2 below the
/// raw one, so the loaded entry is `extraction = param + 0x37F` (see
/// `docs/formats/prot.md` § index spaces; capture-pinned, e.g. mode 2
/// loads field 0897 and the Gimard cast loads stager 0903). The engine
/// host chain ([`CdDmaHost::prot_one_shot_load`] →
/// `ProtIndex::entry_start_lba_retail`, whose `toc` array starts at raw
/// dword 2) consumes extraction-space indices, so this constant carries
/// the shift already applied.
pub const OVERLAY_PROT_BASE: i32 = 0x37F;

/// Mode-state word value (`_DAT_8007B83C`) that forces [`load_overlay_b`] to
/// invalidate its cache before checking. Retail compares against `0x15`
/// and clears `gp+0x934` to `-1` on match.
pub const OVERLAY_B_INVALIDATE_STATE: u16 = 0x15;

/// Loader-B parameter base the battle scene loader adds to the battle-stage
/// id before calling [`load_overlay_b`].
///
/// The battle scene loader `FUN_800520F0` reads the stage id byte
/// `_DAT_8007B64A`, and - **only when it is non-zero** (`beq v1, zero` at
/// `0x80052688` skips the whole call) - issues
/// `FUN_8003EC70(stage_id + 0x47, 0)` at `0x800526A0`. So a stage overlay
/// occupies extraction PROT entry `stage_id + BATTLE_STAGE_PARAM_BASE +
/// OVERLAY_PROT_BASE` = `stage_id + 966`, and stage id `0` means *no* stage
/// overlay: the battle draws over the resident field/world backdrop alone.
///
/// This is the `+0x47` computed-parameter site in the SCUS loader census -
/// the only one that can reach extraction entries 967/968, neither of which
/// any constant-parameter call site produces.
pub const BATTLE_STAGE_PARAM_BASE: i32 = 0x47;

/// Extraction PROT entry holding the battle-stage overlay for `stage_id`, or
/// `None` for stage id `0` (no stage overlay - the retail default).
///
/// Pinned across the battle save-state library by reading the stage id at
/// `_DAT_8007B64A` together with loader B's current-id tracker
/// `gp+0x934` (`0x8007BC4C`):
///
/// | Battle | `_DAT_8007B64A` | `0x8007BC4C` | Entry |
/// |---|---|---|---|
/// | Tetsu sparring tutorial (`town01`) | `1` | `0x48` | 967 |
/// | every other catalogued battle | `0` | unchanged | none |
///
/// `SCUS_942.54` writes the byte in exactly three places: two clears, and
/// `FUN_80055B6C`'s `*_DAT_8007BD0C == 0xB5 → 2` per-formation override
/// (`0x8007BD0C` reads `0x4F` = Tetsu's archive id in the tutorial states),
/// which selects entry 968.
///
/// The overlay is battle *code* in slot B, not stage geometry - the backdrop
/// mesh comes from the resident scene bundle
/// (`ProtIndex::battle_stage_entry_for_scene`).
// PORT: FUN_800520F0 (battle-stage overlay dispatch)
pub fn battle_stage_overlay_entry(stage_id: u8) -> Option<u32> {
    if stage_id == 0 {
        return None;
    }
    Some(stage_id as u32 + BATTLE_STAGE_PARAM_BASE as u32 + OVERLAY_PROT_BASE as u32)
}

/// Host hooks for the parallel overlay loaders. Composes the existing
/// [`CdDmaHost`] (for the actual PROT.DAT streaming read) with three
/// per-loader scratchpad globals the retail mode-table uses.
///
/// The trait carries the spec; engine impls supply their re-host of the
/// retail scratchpad. Default impls are no-ops so a minimal host (mode
/// state always zero, both cache slots always empty) compiles.
pub trait OverlayLoaderHost: CdDmaHost {
    /// Read the dev/retail branch discriminator `_DAT_8007B868`. Retail
    /// builds return `0`; debug builds return non-zero (the mode-table
    /// uses the value as the return code in the dev short-circuit path).
    fn dev_branch_flag(&self) -> u32 {
        0
    }

    /// Read the cache slot's currently-resident PROT index (or
    /// [`OVERLAY_CACHE_EMPTY`] if nothing is loaded).
    fn cache_slot(&self, slot: OverlayCacheSlot) -> i32;

    /// Write the cache slot.
    fn set_cache_slot(&mut self, slot: OverlayCacheSlot, value: i32);

    /// Read the destination buffer pointer for the slot. Retail
    /// dereferences `DAT_8001038C` (A) / `DAT_80010390` (B) - both
    /// constants are populated at boot from the per-build mode table.
    fn overlay_dst(&self, slot: OverlayCacheSlot) -> DestAddr;

    /// Read the mode-state word `_DAT_8007B83C` consumed by
    /// [`load_overlay_b`]'s invalidate guard. Default `0` means "never
    /// force-invalidate".
    fn mode_state_word(&self) -> u16 {
        0
    }
}

/// Load overlay A (FUN_8003EBE4).
///
/// PORT: FUN_8003EBE4
///
/// Caches extraction PROT entry `param + 0x37F` in slot A. Behaviour matrix:
///
/// | Branch                                | Effect                                                                                          | Return     |
/// |---------------------------------------|-------------------------------------------------------------------------------------------------|------------|
/// | dev (`dev_branch_flag != 0`)          | Stash `param` in slot A; no PROT load.                                                          | dev flag   |
/// | retail, slot A == `param`             | Already-resident short-circuit; no PROT load.                                                   | `-1`       |
/// | retail, slot A != `param` (fresh load)| `prot_one_shot_load(param + 0x37F, dst_A, ISSUE)`; invalidate slot B (`= -1`); update slot A.   | `param`    |
pub fn load_overlay_a<H: OverlayLoaderHost + ?Sized>(host: &mut H, param: i32) -> i32 {
    let dev = host.dev_branch_flag();
    if dev != 0 {
        // Dev branch: stash and return the dev discriminator. Retail
        // returns `_DAT_8007B868`'s value (e.g. a non-zero mode id).
        host.set_cache_slot(OverlayCacheSlot::A, param);
        return dev as i32;
    }
    if host.cache_slot(OverlayCacheSlot::A) == param {
        return OVERLAY_CACHE_EMPTY;
    }
    let prot_idx = (param + OVERLAY_PROT_BASE) as u16;
    let dst = host.overlay_dst(OverlayCacheSlot::A);
    host.prot_one_shot_load(prot_idx, dst, LoadFlags::ISSUE);
    // Sister-invalidate: FUN_8003EBE4 always clears gp+0x934 (slot B) on
    // a fresh load - the two slots are mutually exclusive caches.
    host.set_cache_slot(OverlayCacheSlot::B, OVERLAY_CACHE_EMPTY);
    host.set_cache_slot(OverlayCacheSlot::A, param);
    param
}

/// Load overlay B (FUN_8003EC70).
///
/// PORT: FUN_8003EC70
///
/// Caches extraction PROT entry `param + 0x37F` in slot B. Mirrors [`load_overlay_a`]
/// except:
///
/// - Uses [`OverlayCacheSlot::B`] for the cache slot and destination.
/// - Force-invalidates slot B when `mode_state_word() == 0x15` (retail's
///   `_DAT_8007B83C == 0x15`) before the cache check.
/// - Does **not** invalidate the sister slot (slot A); the two loaders
///   are asymmetric in this regard.
/// - Returns `-1` (not `param`) in the dev short-circuit branch.
/// - On a cache hit returns the resident slot value; on a fresh load
///   returns `param`.
pub fn load_overlay_b<H: OverlayLoaderHost + ?Sized>(host: &mut H, param: i32) -> i32 {
    let dev = host.dev_branch_flag();
    if dev != 0 {
        // Dev branch: stash and return `-1` (retail's `_li v0,-0x1` in
        // the branch's delay slot).
        host.set_cache_slot(OverlayCacheSlot::B, param);
        return OVERLAY_CACHE_EMPTY;
    }
    if host.mode_state_word() == OVERLAY_B_INVALIDATE_STATE {
        host.set_cache_slot(OverlayCacheSlot::B, OVERLAY_CACHE_EMPTY);
    }
    let resident = host.cache_slot(OverlayCacheSlot::B);
    if resident == param {
        return resident;
    }
    let prot_idx = (param + OVERLAY_PROT_BASE) as u16;
    let dst = host.overlay_dst(OverlayCacheSlot::B);
    host.prot_one_shot_load(prot_idx, dst, LoadFlags::ISSUE);
    host.set_cache_slot(OverlayCacheSlot::B, param);
    param
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cd_dma::{CdDmaHost, DestAddr, LoadFlags, ProtIndex, ReadWaitOutcome};
    use std::cell::RefCell;

    /// Recording host. Tracks every CD-DMA call plus the cache-slot /
    /// destination / mode-state state so tests can assert behaviour.
    struct RecOverlayHost {
        dev_flag: u32,
        mode_state: u16,
        slot_a: i32,
        slot_b: i32,
        dst_a: DestAddr,
        dst_b: DestAddr,
        prot_loads: RefCell<Vec<(ProtIndex, DestAddr, LoadFlags)>>,
    }

    impl RecOverlayHost {
        fn retail() -> Self {
            Self {
                dev_flag: 0,
                mode_state: 0,
                slot_a: OVERLAY_CACHE_EMPTY,
                slot_b: OVERLAY_CACHE_EMPTY,
                dst_a: 0x8010_0000,
                dst_b: 0x8011_0000,
                prot_loads: RefCell::default(),
            }
        }
    }

    impl CdDmaHost for RecOverlayHost {
        fn prot_index_size_lookup(&mut self, _prot_idx: ProtIndex, _set_msf: bool) -> u32 {
            16 // synthetic non-zero LBA count
        }
        fn async_lba_load(&mut self, _dst: DestAddr, _count: u32, _flags: LoadFlags) {}
        fn kick_libcd_read(&mut self) {}
        fn read_wait_poll(&mut self, _gated: bool) -> ReadWaitOutcome {
            ReadWaitOutcome::Ready
        }
        // Override the wrapper so the test sees the exact call shape.
        fn prot_one_shot_load(
            &mut self,
            prot_idx: ProtIndex,
            dst: DestAddr,
            flags: LoadFlags,
        ) -> u32 {
            self.prot_loads.borrow_mut().push((prot_idx, dst, flags));
            16
        }
    }

    impl OverlayLoaderHost for RecOverlayHost {
        fn dev_branch_flag(&self) -> u32 {
            self.dev_flag
        }
        fn cache_slot(&self, slot: OverlayCacheSlot) -> i32 {
            match slot {
                OverlayCacheSlot::A => self.slot_a,
                OverlayCacheSlot::B => self.slot_b,
            }
        }
        fn set_cache_slot(&mut self, slot: OverlayCacheSlot, value: i32) {
            match slot {
                OverlayCacheSlot::A => self.slot_a = value,
                OverlayCacheSlot::B => self.slot_b = value,
            }
        }
        fn overlay_dst(&self, slot: OverlayCacheSlot) -> DestAddr {
            match slot {
                OverlayCacheSlot::A => self.dst_a,
                OverlayCacheSlot::B => self.dst_b,
            }
        }
        fn mode_state_word(&self) -> u16 {
            self.mode_state
        }
    }

    // ---- load_overlay_a (FUN_8003EBE4) ----------------------------------

    #[test]
    fn load_a_dev_branch_stashes_param_and_returns_dev_flag() {
        let mut host = RecOverlayHost::retail();
        host.dev_flag = 0x42;
        let result = load_overlay_a(&mut host, 5);
        assert_eq!(result, 0x42, "dev branch returns the dev flag value");
        assert_eq!(host.slot_a, 5, "param stashed in slot A");
        assert!(
            host.prot_loads.borrow().is_empty(),
            "dev branch skips the PROT load"
        );
    }

    #[test]
    fn load_a_fresh_load_invalidates_sister_slot() {
        let mut host = RecOverlayHost::retail();
        host.slot_b = 99; // pretend slot B has something resident
        let result = load_overlay_a(&mut host, 7);
        assert_eq!(result, 7, "fresh load returns the loaded param");
        assert_eq!(host.slot_a, 7);
        assert_eq!(host.slot_b, OVERLAY_CACHE_EMPTY, "slot B invalidated");
        let loads = host.prot_loads.borrow();
        assert_eq!(loads.len(), 1);
        let (prot_idx, dst, flags) = loads[0];
        assert_eq!(prot_idx, (7 + OVERLAY_PROT_BASE) as u16);
        assert_eq!(dst, 0x8010_0000);
        assert_eq!(flags, LoadFlags::ISSUE);
    }

    #[test]
    fn load_a_cache_hit_returns_minus_one_without_load() {
        let mut host = RecOverlayHost::retail();
        host.slot_a = 7;
        host.slot_b = 99; // sister slot should NOT be invalidated on a hit
        let result = load_overlay_a(&mut host, 7);
        assert_eq!(result, OVERLAY_CACHE_EMPTY);
        assert_eq!(host.slot_a, 7);
        assert_eq!(host.slot_b, 99, "sister slot preserved on cache hit");
        assert!(host.prot_loads.borrow().is_empty());
    }

    // ---- load_overlay_b (FUN_8003EC70) ----------------------------------

    #[test]
    fn load_b_dev_branch_stashes_and_returns_minus_one() {
        let mut host = RecOverlayHost::retail();
        host.dev_flag = 0x99;
        let result = load_overlay_b(&mut host, 11);
        assert_eq!(result, OVERLAY_CACHE_EMPTY, "dev branch always returns -1");
        assert_eq!(host.slot_b, 11);
        assert!(host.prot_loads.borrow().is_empty());
    }

    #[test]
    fn load_b_mode_state_0x15_forces_cache_invalidate() {
        let mut host = RecOverlayHost::retail();
        host.slot_b = 11; // already resident
        host.mode_state = OVERLAY_B_INVALIDATE_STATE;
        let result = load_overlay_b(&mut host, 11);
        // Even though cache key matches the param, mode_state forces an
        // invalidate which makes the subsequent equality check fail.
        assert_eq!(result, 11, "fresh load returns param");
        assert_eq!(host.slot_b, 11);
        assert_eq!(host.prot_loads.borrow().len(), 1, "PROT load was issued");
    }

    #[test]
    fn load_b_fresh_load_does_not_invalidate_slot_a() {
        let mut host = RecOverlayHost::retail();
        host.slot_a = 42; // pretend A has something resident
        let result = load_overlay_b(&mut host, 7);
        assert_eq!(result, 7);
        assert_eq!(host.slot_b, 7);
        assert_eq!(host.slot_a, 42, "slot A preserved (asymmetric with A→B)");
        let loads = host.prot_loads.borrow();
        assert_eq!(loads.len(), 1);
        assert_eq!(loads[0].1, 0x8011_0000, "dst is slot B's buffer");
    }

    #[test]
    fn load_b_cache_hit_returns_resident_value() {
        let mut host = RecOverlayHost::retail();
        host.slot_b = 7;
        let result = load_overlay_b(&mut host, 7);
        assert_eq!(result, 7, "cache hit returns the resident param");
        assert!(host.prot_loads.borrow().is_empty());
    }

    #[test]
    fn prot_base_offset_matches_retail_dump() {
        // The PROT offset literal in both functions is `_addiu a0,a0,0x381`,
        // a RAW in-RAM TOC index (header-included). The engine host chain
        // consumes extraction-space indices, which sit 2 below raw - the
        // constant carries the shift (capture-pinned: mode 2 loads field
        // 0897; the Gimard cast loads stager 0903).
        assert_eq!(OVERLAY_PROT_BASE, 0x381 - 2);
    }
}
