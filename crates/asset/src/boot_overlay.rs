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
/// `resident_param` is the loader's current-id tracker for slot B; pass `None`
/// when nothing is resident.
///
// PORT: FUN_80025ba0
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

/// Where the effect-data loader sources its bytes for a given build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectDataSource {
    /// Retail: open the effect file through the ISO9660 filesystem.
    IsoFile,
    /// Debug: resolve [`EFFECT_DATA_EXTRACTION_INDEX`] through the PROT TOC.
    ProtEntry(u32),
}

/// Pick the effect-data source for a build.
///
/// The routine is keyed on the dev/retail flag: a **retail** image opens the
/// effect file by name through the ISO9660 filesystem, zero-fills the tail up to
/// the next 2 KB boundary, and records the padded length; a **debug** image
/// resolves the same content through the PROT TOC instead. Both branches land
/// the bytes at [`EFFECT_DATA_BUFFER_OFFSET`] past the streaming-buffer base.
///
// PORT: FUN_8003e360
pub fn effect_data_source(debug_build: bool) -> EffectDataSource {
    if debug_build {
        EffectDataSource::ProtEntry(EFFECT_DATA_EXTRACTION_INDEX)
    } else {
        EffectDataSource::IsoFile
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

/// Where the CARD-mode init sources its TIM pack for a given build.
///
/// Retail opens the pack by dev path through the path-based resolver, which maps
/// the name onto the same entry the debug branch resolves by index; the returned
/// [`CARD_TIM_EXTRACTION_INDEX`] is the entry either branch ends up reading. The
/// loaded bundle is then walked as an `asset::pack` and each TIM uploaded to
/// VRAM.
///
// PORT: FUN_8002574c
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
// PORT: FUN_8001eef0
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
        assert_eq!(effect_data_source(true), EffectDataSource::ProtEntry(979));
        assert_eq!(effect_data_source(false), EffectDataSource::IsoFile);
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
