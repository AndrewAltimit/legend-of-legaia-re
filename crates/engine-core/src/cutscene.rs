//! FMV / pre-rendered cutscene helpers.
//!
//! PORT: FUN_801E30E4, FUN_801CF098
//!
//! The retail field VM triggers an FMV via opcode `0x4C 0xE2`
//! (handler at `0x801E30E4` in the cutscene-dialogue overlay). The
//! handler writes the s16 operand to `_DAT_8007BA78` (FMV index used
//! by the runtime FMV-state table at `0x801D0A6C`) and pokes the
//! next-game-mode global `_DAT_8007B83C` to `0x1A` (game mode 26 =
//! StrInit). The str_fmv overlay then resolves the index to a STR
//! file and starts MDEC + XA playback.
//!
//! ## Authoritative runtime mapping
//!
//! The runtime FMV-state table at `0x801D0A6C` is what the play loop
//! at `FUN_801CF098` actually reads (offset `+0x00` of each 64-byte
//! slot is the path-string pointer that the disc reader opens). The
//! table has **at least 12 slots** in the retail USA build; index `N`
//! is computed as `_DAT_8007BA78 * 64`.
//!
//! Six of the twelve slots reference real `MV*.STR` files via the
//! path string table at `0x801CE810`. `MV2.STR` and `MV5.STR` are
//! **NOT** referenced by any slot — they're disc-resident files that
//! the runtime never plays:
//!
//! | `fmv_id` | path resolved | notes |
//! |---------:|---------------|-------|
//! | 0  | `\MOV\MV1.STR;1` | intro logo (also fired by title-screen attract loop) |
//! | 1  | `\MOV\MV3.STR;1` | first segment / start sector ≈ 1 |
//! | 2  | `\MOV\MV3.STR;1` | second segment / start sector offset `+0x1A5` |
//! | 3  | `\MOV\MV4.STR;1` | |
//! | 4  | `\MOV\MV6.STR;1` | |
//! | 5  | `\DATA\MOV15.STR;1` | dev-only path (file not on retail disc) |
//! | 6..=11 | `\DATA\MOV.STR;1` | dev-only path (file not on retail disc) |
//!
//! The slot pointers were lifted from the str_fmv overlay's data
//! section captured in a prior corpus rotation; the latest corpus
//! pins the trigger-side state (`_DAT_8007BA78` + game mode) for
//! `fmv_id ∈ 0..=8` via the `cutscene_trigger_corpus` capture
//! observation, but does not have the FMV overlay loaded so the
//! runtime mapping is derived statically.
//!
//! ## Compact table at `0x801CAE40` is **not** the play-engine source
//!
//! [`legaia_asset::str_fmv_table`] parses a 6-entry compact table at
//! `0x801CAE40` whose entries are labelled `MV1.STR;1` .. `MV6.STR;1`.
//! Empirical check: the BCD-MSF + size fields in that table do not
//! match the disc layout for the same names (entry `[0]` "MV1.STR" has
//! a size + LBA that points at disc `MV2.STR`; entry `[5]` "MV6.STR"
//! points at `XA15.XA`). The compact table is a separate dev/init
//! lookup, not the FMV play engine's table — the play loop reads from
//! `0x801D0A6C`, not `0x801CAE40`.
//!
//! See [`docs/subsystems/cutscene.md`](../../../../docs/subsystems/cutscene.md)
//! for the full Ghidra-traced provenance.

/// Retail FMV index → STR filename mapping.
///
/// Returns the path the play loop opens for `fmv_id`. The retail
/// index space is `0..=8` (the runtime FMV-state table has at least
/// 9 valid slots; the field-VM op `0x4C 0xE2` permits any s16). Six
/// of the twelve slots reference real `MV*.STR` files; the rest
/// reference dev-only paths that don't exist on the retail disc.
///
/// Engines that drain a [`crate::field_events::FieldEvent::FmvTrigger`]
/// event use this helper to resolve the operand to a path that their
/// disc handle can open. A `None` result means the slot points at a
/// cut/missing path; engines should treat it as a no-op (or surface a
/// playback error if they want to surface it).
pub fn fmv_index_to_str_filename(fmv_id: i16) -> Option<&'static str> {
    match fmv_id {
        0 => Some("MOV/MV1.STR"),
        1 => Some("MOV/MV3.STR"),
        2 => Some("MOV/MV3.STR"),
        3 => Some("MOV/MV4.STR"),
        4 => Some("MOV/MV6.STR"),
        // 5..=11: cut paths (`\DATA\MOV15.STR;1`, `\DATA\MOV.STR;1`)
        // — disc files don't exist on retail USA. The field VM and
        // debug menu can still write these values; engines should
        // treat them as a no-op.
        _ => None,
    }
}

/// Inverse of [`fmv_index_to_str_filename`]: resolve a `MV*.STR`
/// filename to a retail FMV index that plays it. Case-insensitive on
/// the basename so `mv1.str` and `MOV/MV1.STR` both round-trip.
///
/// Multi-slot files round-trip to the **first** slot that references
/// them (so `MV3.STR` returns `Some(1)` even though slot `2` also
/// plays it as a second segment).
///
/// Returns `None` for filenames that no FMV slot references — this
/// includes `MV2.STR` and `MV5.STR` (disc-resident but unused by the
/// FMV runtime), plus arbitrary names.
pub fn str_filename_to_fmv_index(str_filename: &str) -> Option<i16> {
    let trimmed = str_filename
        .rsplit_once('/')
        .map(|(_, name)| name)
        .unwrap_or(str_filename);
    match trimmed.to_ascii_uppercase().as_str() {
        "MV1.STR" => Some(0),
        "MV3.STR" => Some(1),
        "MV4.STR" => Some(3),
        "MV6.STR" => Some(4),
        // MV2.STR and MV5.STR are disc-resident but never reached by
        // the runtime — no slot in the FMV-state table points at them.
        _ => None,
    }
}

/// Retail's "next game mode = StrInit" constant. The handler at
/// `0x801E30E4` writes this byte to the next-game-mode global
/// (`_DAT_8007B83C`) every time it fires; the main mode dispatcher
/// at `0x80017714` then transitions into mode 26 on the next frame.
pub const STR_INIT_GAME_MODE: u8 = 0x1A;

/// Number of FMV slots in the runtime FMV-state table at `0x801D0A6C`.
/// The slot stride is 64 bytes; `_DAT_8007BA78 * 64 + 0x801D0A6C`
/// addresses the slot for the given `fmv_id`. Twelve slots fit before
/// the table is followed by other overlay data.
pub const FMV_SLOT_COUNT: usize = 12;

/// PSX-virtual address of the runtime FMV-state table base.
pub const FMV_STATE_TABLE_ADDR: u32 = 0x801D_0A6C;

/// Stride (bytes) of one FMV-state slot.
pub const FMV_STATE_SLOT_STRIDE: u32 = 64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmv_index_round_trip_for_unique_slots() {
        // The four unique-slot mappings (slot[2] is a second segment
        // of MV3 and round-trips to the first MV3 slot, which is 1).
        for (idx, name) in [
            (0, "MV1.STR"),
            (1, "MV3.STR"),
            (3, "MV4.STR"),
            (4, "MV6.STR"),
        ] {
            let path = fmv_index_to_str_filename(idx).expect("unique slot maps");
            let bare = path.rsplit_once('/').map(|(_, n)| n).unwrap_or(path);
            assert_eq!(bare, name);
            assert_eq!(str_filename_to_fmv_index(bare), Some(idx));
            assert_eq!(str_filename_to_fmv_index(path), Some(idx));
        }
    }

    #[test]
    fn fmv_id_2_is_second_segment_of_mv3() {
        // Slot 2's path pointer matches slot 1's (both `\MOV\MV3.STR;1`),
        // but the slot's start-sector offset (`+0x08`) differs.
        assert_eq!(fmv_index_to_str_filename(2), Some("MOV/MV3.STR"));
        // The reverse lookup picks the first slot referencing the file.
        assert_eq!(str_filename_to_fmv_index("MV3.STR"), Some(1));
    }

    #[test]
    fn fmv_id_2_and_5_disc_files_have_no_active_slot() {
        // MV2 and MV5 exist on the retail disc but no FMV slot points
        // at them — they're never reached by the play loop.
        assert_eq!(str_filename_to_fmv_index("MV2.STR"), None);
        assert_eq!(str_filename_to_fmv_index("MV5.STR"), None);
        assert_eq!(str_filename_to_fmv_index("MOV/MV2.STR"), None);
    }

    #[test]
    fn fmv_id_5_through_11_resolve_to_cut_paths() {
        // Slots 5..=11 point at \DATA\MOV15.STR / \DATA\MOV.STR which
        // don't exist on retail. fmv_index_to_str_filename returns
        // None for them — engines should treat as a no-op.
        for i in 5..=11 {
            assert_eq!(fmv_index_to_str_filename(i), None);
        }
    }

    #[test]
    fn fmv_index_out_of_range_returns_none() {
        assert_eq!(fmv_index_to_str_filename(-1), None);
        assert_eq!(fmv_index_to_str_filename(12), None);
        assert_eq!(fmv_index_to_str_filename(i16::MAX), None);
    }

    #[test]
    fn str_filename_unknown_returns_none() {
        assert_eq!(str_filename_to_fmv_index("MV7.STR"), None);
        assert_eq!(str_filename_to_fmv_index("garbage"), None);
        assert_eq!(str_filename_to_fmv_index(""), None);
    }

    #[test]
    fn str_filename_case_insensitive() {
        assert_eq!(str_filename_to_fmv_index("mv1.str"), Some(0));
        assert_eq!(str_filename_to_fmv_index("MOV/mv6.STR"), Some(4));
    }

    #[test]
    fn str_init_game_mode_matches_retail() {
        assert_eq!(STR_INIT_GAME_MODE, 26);
    }

    #[test]
    fn fmv_state_table_constants_are_consistent() {
        // The slot for fmv_id N lives at FMV_STATE_TABLE_ADDR + N * 64.
        assert_eq!(FMV_STATE_TABLE_ADDR, 0x801D_0A6C);
        assert_eq!(FMV_STATE_SLOT_STRIDE, 64);
        assert_eq!(FMV_SLOT_COUNT, 12);
    }
}
