//! Boot-time overlay + side-band asset resolution.
//!
//! The SCUS-resident boot path never touches asset *bytes* directly: it turns a
//! small loader parameter into a `PROT.DAT` TOC index, hands that to the LBA
//! resolver, and DMAs the entry into one of two overlay windows. This module
//! ports the index arithmetic and the handful of loader call sites whose choice
//! of entry is a real decision rather than a constant, so the engine can resolve
//! the same entries off a user's disc.
//!
//! ## Index spaces
//!
//! Two index spaces coexist and differ by two, which is the single most common
//! source of mis-attribution here:
//!
//! - **Raw TOC index** - what the runtime resolver (`FUN_8003E8A8`) takes. It
//!   indexes the in-RAM copy of `PROT.DAT` from byte 0, reading `toc[idx + 2]`.
//! - **Extraction index** - what `crates/prot` and `extracted/PROT/NNNN_*.BIN`
//!   use; entry `p`'s start comes from file word `p + 4`.
//!
//! So `extraction = raw - 2` ([`RAW_TO_EXTRACTION`]). The two overlay loaders
//! both add [`OVERLAY_PARAM_BIAS`] to their parameter before resolving, which
//! nets out to `param + 0x37F` in extraction space
//! ([`overlay_param_to_extraction`]).
//!
//! **CDNAME filename labels are not content attribution.** Names inherit forward
//! from each `#define`, so an extracted file's label can belong to a neighbour.
//! Every entry named here is pinned by its loader-call constant and confirmed
//! against the entry's own magic bytes or embedded strings - see
//! [`docs/formats/cdname.md`](../../../docs/formats/cdname.md) and the
//! disc-gated tests in `tests/boot_overlay_disc.rs`.
//!
//! ## `FUN_8003E6BC` is not a name resolver
//!
//! Several loaders here pick between two branches on `_DAT_8007B8C2`, and the
//! branch that does *not* use a PROT index calls `FUN_8003E6BC`. Ghidra labels
//! that `path_opener` and annotates it "dev path -> PROT index via CDNAME map".
//! **That annotation is false.** Its body is a host-PC open (`FUN_800608F0`,
//! entirely `break 0x103` - the SN/PsyQ debug-station trap), then lseek / read /
//! close over the same link, then a zero-fill of the tail. There is no CDNAME
//! lookup and no TOC resolution in it, and it is not ISO9660 either - the real
//! ISO path is `FUN_8003D3C4`, which goes through the CD stack.
//!
//! So the two branches are a **disc-index load** versus a **host-PC file read**,
//! not two routes to the same entry. Any doc or comment claiming the non-index
//! branch resolves a name onto the same PROT entry is repeating the bad
//! annotation.
//!
//! Which polarity ships is genuinely **open**: `0x8007B8C2` is BSS-resident
//! (the executable's image ends at `0x8007B800`) and a byte-level scan of
//! `SCUS_942.54` for stores with the matching immediate finds 40 loads and
//! **zero** stores - a scan that does not rely on Ghidra's xref manager, so the
//! LUI+ADDIU trap does not explain it away. Taken at face value that means the
//! flag is `0` on hardware and these loaders take the host branch, which cannot
//! succeed with no host attached. Something has to give; until it is pinned,
//! this module names branches by **mechanism**, never by `retail` / `debug`.
//!
//! See [`docs/subsystems/boot.md`](../../../docs/subsystems/boot.md) for the
//! mode state machine these loaders serve.

/// Difference between the raw in-RAM TOC index space and the extraction index
/// space: `extraction = raw - RAW_TO_EXTRACTION`.
pub const RAW_TO_EXTRACTION: u32 = 2;

/// Constant both overlay loaders add to their parameter before calling the LBA
/// resolver (`FUN_8003E8A8(param + 0x381)`).
pub const OVERLAY_PARAM_BIAS: u32 = 0x381;

/// Resolve an overlay-loader parameter to a raw in-RAM TOC index.
///
// REF: FUN_8003ebe4
pub fn overlay_param_to_raw(param: u32) -> u32 {
    param + OVERLAY_PARAM_BIAS
}

/// Resolve an overlay-loader parameter to an **extraction** PROT index
/// (`param + 0x37F`) - the index space `crates/prot` and `extracted/PROT/` use.
///
// REF: FUN_8003ebe4
pub fn overlay_param_to_extraction(param: u32) -> u32 {
    overlay_param_to_raw(param) - RAW_TO_EXTRACTION
}

/// Convert a raw in-RAM TOC index (as passed to `FUN_8003E8A8`) into an
/// extraction PROT index.
///
// REF: FUN_8003e8a8
pub fn raw_to_extraction(raw: u32) -> Option<u32> {
    raw.checked_sub(RAW_TO_EXTRACTION)
}

// ---------------------------------------------------------------------------
// Slot-B default overlay (`FUN_80025BA0`)
// ---------------------------------------------------------------------------

/// Slot-B overlay-loader parameter chosen when the summon-render flag is clear.
pub const SLOT_B_DEFAULT_PARAM: u32 = 5;

/// Slot-B overlay-loader parameter chosen when the summon-render flag is set.
pub const SLOT_B_ALT_PARAM: u32 = 6;

/// Which overlay the slot-B default loader installs, and whether it needs to
/// load at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotBChoice {
    /// Loader parameter (`5` or `6`).
    pub param: u32,
    /// Extraction PROT index the parameter resolves to (`900` or `901`).
    pub extraction_index: u32,
    /// `false` when the wanted overlay is already resident, so retail skips the
    /// load entirely.
    pub needs_load: bool,
}

/// Pick the slot-B default overlay.
///
/// Retail mirrors the summon-render flag into a work word, then - unless a
/// suppression word is set for this frame - loads parameter `6` when the flag is
/// set and `5` otherwise, skipping the load when that overlay is already the
/// resident one. The suppression word is cleared unconditionally on the way out,
/// so it only ever suppresses a single frame.
///
/// `resident_param` is the word this routine compares against
/// (`80025bec lw v1,-0x43b4(v1)` = **`0x8007BC4C`**); pass `None` when it holds
/// no matching id. It is **not** the overlay loader's own resident tracker -
/// `FUN_8003EC70` keeps that at `gp+0x934` (`8003ecb4` / `8003ecec`). The two
/// are independent skip-checks against different globals, and what maintains
/// `0x8007BC4C` is not yet pinned; do not treat this as the loader's tracker
/// when wiring it.
///
/// Also unmodelled: `FUN_8003EC70` short-circuits entirely when
/// `_DAT_8007B868 != 0` (`8003ec88`), so a caller can request a load that the
/// loader then declines.
///
// PORT: FUN_80025ba0
// NOT WIRED: no caller outside the disc-gated test.
pub fn slot_b_default_overlay(
    summon_render_flag: bool,
    suppressed: bool,
    resident_param: Option<u32>,
) -> Option<SlotBChoice> {
    if suppressed {
        return None;
    }
    let param = if summon_render_flag {
        SLOT_B_ALT_PARAM
    } else {
        SLOT_B_DEFAULT_PARAM
    };
    Some(SlotBChoice {
        param,
        extraction_index: overlay_param_to_extraction(param),
        needs_load: resident_param != Some(param),
    })
}

// ---------------------------------------------------------------------------
// Effect-data side-band load (`FUN_8003E360`)
// ---------------------------------------------------------------------------

/// Raw TOC index the debug branch of the effect-data loader resolves.
pub const EFFECT_DATA_RAW_INDEX: u32 = 0x3D5;

/// Extraction PROT index holding the effect module (`0x3D5 - 2 = 979`).
///
/// Pinned by the loader constant and confirmed by the entry's own strings
/// (`efect init`, `battle bgm %d`) - the CDNAME-inherited filename label on this
/// entry does not name it.
pub const EFFECT_DATA_EXTRACTION_INDEX: u32 = EFFECT_DATA_RAW_INDEX - RAW_TO_EXTRACTION;

/// Byte offset from the streaming-buffer base that the effect-data loader
/// targets in both of its branches.
pub const EFFECT_DATA_BUFFER_OFFSET: u32 = 0x59400;

/// Where the effect-data loader sources its bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectDataSource {
    /// Flag clear: open a **host-PC** file over the SN/PsyQ debug-station link.
    ///
    /// Not ISO9660 and not the disc at all. The branch calls `FUN_800608F0`,
    /// whose entire body is `break 0x103` - the debug-station host trap - and
    /// then its lseek / read / close siblings (`FUN_80060920`, `FUN_80060944`,
    /// `FUN_80060910`). The path operand it opens is a literal PC drive letter.
    /// On hardware with no host attached the open returns `-1`, the routine
    /// bumps the failure counter at `0x8007B86E` and loads nothing.
    HostFile,
    /// Flag set: resolve [`EFFECT_DATA_EXTRACTION_INDEX`] through the PROT TOC.
    ProtEntry(u32),
}

/// Pick the effect-data source from the `_DAT_8007B8C2` flag.
///
/// `8003e37c bne v0,zero,0x8003e49c` branches on the flag: **non-zero** takes
/// the PROT-TOC branch (`8003e4a4 li a0,0x3d5`), **zero** falls through to the
/// host-PC quartet described on [`EffectDataSource::HostFile`]. Both branches
/// target [`EFFECT_DATA_BUFFER_OFFSET`] past the streaming-buffer base.
///
/// The parameter is the raw flag, deliberately not a `retail`/`debug` boolean:
/// which polarity is the shipped configuration is an **open question**, and
/// naming it either way here would bake in an unverified claim. The mechanism
/// above is certain; the build identity is not. See the module docs.
///
/// The Ghidra dump's hand-written header comment on this function
/// (`retail ISO9660 vs debug PROT TOC`) is wrong on both halves - neither
/// branch touches ISO9660 - and is not evidence for anything.
///
// PORT: FUN_8003e360
// NOT WIRED: no caller outside this module's tests; the engine does not yet
// load the effect-data side band.
pub fn effect_data_source(dev_flag_set: bool) -> EffectDataSource {
    if dev_flag_set {
        EffectDataSource::ProtEntry(EFFECT_DATA_EXTRACTION_INDEX)
    } else {
        EffectDataSource::HostFile
    }
}

// ---------------------------------------------------------------------------
// Memory-card / CARD-mode TIM pack (`FUN_8002574C`)
// ---------------------------------------------------------------------------

/// Raw TOC index the CARD-mode init loads its TIM pack from.
pub const CARD_TIM_RAW_INDEX: u32 = 0x37E;

/// Extraction PROT index of the CARD-mode TIM pack (`0x37E - 2 = 892`).
///
/// The entry is an [`crate::pack`] bundle of PSX TIMs. Its CDNAME-inherited
/// filename label names a neighbouring block, so the label is not the
/// attribution - the loader constant plus the entry's pack header and TIM magic
/// are (see `tests/boot_overlay_disc.rs`).
pub const CARD_TIM_EXTRACTION_INDEX: u32 = CARD_TIM_RAW_INDEX - RAW_TO_EXTRACTION;

/// Scratch-buffer size the CARD-mode init allocates for the TIM pack.
pub const CARD_TIM_BUFFER_LEN: u32 = 0x19000;

/// Extraction index of the TIM pack the CARD-mode init loads by constant.
///
/// `800257e4 bne v1,zero,0x80025804` branches on the same `_DAT_8007B8C2` flag
/// as [`effect_data_source`]: **non-zero** reaches `li a0,0x37e` and loads the
/// entry by index. The **zero** branch opens a literal host-PC path through the
/// same `break 0x103` quartet, so it is not "retail by dev path through the
/// path-based resolver" - `FUN_8003E6BC` performs no name resolution at all
/// (see the module docs). The two branches are therefore *not* two routes to
/// one entry, and only the by-index branch has an extraction index to return.
///
/// The by-index branch is additionally gated on `gp+0x7E8 == 1`
/// (`800257c0`); the other path draws two rects and loads no pack. Neither
/// that gate nor the host branch is modelled here - this function only reports
/// the constant.
///
/// The loaded bundle is walked as an [`crate::pack`] and each TIM uploaded to
/// VRAM.
///
// PORT: FUN_8002574c
// NOT WIRED: no caller outside the disc-gated test; nothing in the engine
// loads the CARD-mode TIM pack yet.
pub fn card_tim_pack_extraction_index() -> u32 {
    CARD_TIM_EXTRACTION_INDEX
}

// ---------------------------------------------------------------------------
// Sector-count rounding (`FUN_8001EEF0`)
// ---------------------------------------------------------------------------

/// Bytes per `PROT.DAT` sector.
pub const SECTOR_BYTES: i32 = 0x800;

/// Round a byte length up to whole sectors the way the scene-bundle loader does.
///
/// The routine returns its byte result as a sector count with an arithmetic
/// shift: it adds `0x7FF` and shifts right by 11, but when that sum is negative
/// it adds a further `0x7FF` first. That second bias makes the shift round
/// *toward zero* for negative inputs instead of the floor an arithmetic shift
/// would otherwise give - so an error return stays an error rather than becoming
/// `-1` worth of sectors.
///
/// **Scope: this is the six-instruction arithmetic tail only**
/// (`8001f030`..`8001f050`), not the whole of `FUN_8001EEF0`. The function's
/// dual-mode dispatch, its `0x20` bias, the `FUN_80020310` sizing path and the
/// unconditional `DAT_8007B730 = 0` clear at `8001f02c` are all unmodelled.
///
/// The negative branch only triggers below `-0x7FF`, so small negative inputs
/// still go through the positive path.
///
/// Note `engine-core::stream_file` has its own `bytes_to_sectors_floor`, which
/// is plain floor division with no round-up and no signed path. The two
/// disagree; reconcile them before wiring either into a shared path.
///
// PORT: FUN_8001eef0 (arithmetic tail only)
// NOT WIRED: no caller anywhere in the tree.
pub fn bytes_to_sectors(bytes: i32) -> i32 {
    let biased = bytes.wrapping_add(SECTOR_BYTES - 1);
    let biased = if biased < 0 {
        bytes.wrapping_add(2 * SECTOR_BYTES - 2)
    } else {
        biased
    };
    biased >> 11
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_params_land_in_extraction_space() {
        // The mode-table loader constants documented in docs/subsystems/boot.md.
        assert_eq!(overlay_param_to_extraction(2), 897); // field / town
        assert_eq!(overlay_param_to_extraction(3), 898); // battle
        assert_eq!(overlay_param_to_extraction(4), 899); // menu / memory card
        assert_eq!(overlay_param_to_extraction(7), 902); // game over
        assert_eq!(overlay_param_to_extraction(0x4B), 970); // cutscene / STR
        assert_eq!(overlay_param_to_extraction(0x4C), 971); // debug menu
        assert_eq!(overlay_param_to_extraction(0x54), 979); // effect test
    }

    #[test]
    fn raw_and_param_spaces_agree() {
        assert_eq!(overlay_param_to_raw(2), 899);
        assert_eq!(raw_to_extraction(overlay_param_to_raw(2)), Some(897));
        assert_eq!(raw_to_extraction(1), None);
    }

    #[test]
    fn slot_b_picks_by_flag_and_skips_resident() {
        let clear = slot_b_default_overlay(false, false, None).unwrap();
        assert_eq!((clear.param, clear.extraction_index), (5, 900));
        assert!(clear.needs_load);

        let set = slot_b_default_overlay(true, false, None).unwrap();
        assert_eq!((set.param, set.extraction_index), (6, 901));

        // Already resident -> retail skips the load but still reports the choice.
        let resident = slot_b_default_overlay(true, false, Some(6)).unwrap();
        assert!(!resident.needs_load);

        // The one-frame suppression word wins outright.
        assert!(slot_b_default_overlay(true, true, None).is_none());
    }

    #[test]
    fn side_band_indices_match_their_loader_constants() {
        assert_eq!(EFFECT_DATA_EXTRACTION_INDEX, 979);
        assert_eq!(CARD_TIM_EXTRACTION_INDEX, 892);
        // Flag non-zero -> PROT TOC by index; flag zero -> host-PC file over
        // the debug-station link (NOT ISO9660, and not the disc).
        assert_eq!(effect_data_source(true), EffectDataSource::ProtEntry(979));
        assert_eq!(effect_data_source(false), EffectDataSource::HostFile);
        assert_eq!(card_tim_pack_extraction_index(), 892);
    }

    #[test]
    fn sector_rounding_rounds_up_and_keeps_errors_negative() {
        assert_eq!(bytes_to_sectors(0), 0);
        assert_eq!(bytes_to_sectors(1), 1);
        assert_eq!(bytes_to_sectors(0x800), 1);
        assert_eq!(bytes_to_sectors(0x801), 2);
        assert_eq!(bytes_to_sectors(0x1000), 2);
        // Negative inputs round toward zero, so a -1 error stays 0 sectors
        // rather than becoming a bogus -1.
        assert_eq!(bytes_to_sectors(-1), 0);
        assert_eq!(bytes_to_sectors(-0x800), 0);
    }
}
