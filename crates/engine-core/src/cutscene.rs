//! FMV / pre-rendered cutscene helpers.
//!
//! PORT: FUN_801E30E4, FUN_801CF098
//!
//! The retail field VM triggers an FMV via opcode `0x4C 0xE2`
//! (handler at `0x801E30E4` in the cutscene-dialogue overlay). The
//! handler writes the s16 operand to `_DAT_8007BA78` (the FMV index)
//! and pokes the next-game-mode global `_DAT_8007B83C` to `0x1A`
//! (game mode 26 = StrInit). The STR/MDEC overlay (PROT 0970) then
//! runs its master dispatch `FUN_801CEA3C`, which resolves the index
//! to a STR file + frame range and starts MDEC + XA playback.
//!
//! ## Authoritative runtime mapping
//!
//! The FMV dispatch table at `0x801D0A6C` is what the master dispatch
//! `FUN_801CEA3C` indexes (selector `sll v0,v0,0x5` at `0x801CEC9C` -
//! **32-byte** slots) before calling the play loop `FUN_801CF098` on
//! the selected record. The table has 23 slots; the NINE retail slots
//! are `fmv_id 0..=8` and dispatch **every movie on the disc** -
//! `MV3.STR` carries four cutscenes as abutting frame-range segments:
//!
//! | `fmv_id` | path resolved | segment / post-play hand-off (`0x801CE8AC` list) |
//! |---------:|---------------|--------------------------------------------------|
//! | 0  | `\MOV\MV1.STR;1` | intro (also the title attract loop); post-play → mode 22 (card init) |
//! | 1  | `\MOV\MV2.STR;1` | trigger `town01` → return scene `town0b` |
//! | 2  | `\MOV\MV3.STR;1` | segment 1 (frames `1..0xE1`); `garmel` → `map01` |
//! | 3  | `\MOV\MV3.STR;1` | segment 2 (`0xE2..0x1A4`); `deroa`/`chitei2` → `chitei2` |
//! | 4  | `\MOV\MV3.STR;1` | segment 3 (`0x1A5..0x27B`); `dohaty` → `map02` |
//! | 5  | `\MOV\MV3.STR;1` | segment 4 (`0x27C..0x36A`); stays in the current scene |
//! | 6  | `\MOV\MV4.STR;1` | `town0d` → `jou` |
//! | 7  | `\MOV\MV5.STR;1` | `uru` → `uru2` |
//! | 8  | `\MOV\MV6.STR;1` | `jouine` → `town0e` |
//! | 9  | `\MOV\MV1A.STR;1` | dev slot (file not on retail disc) |
//! | 10 | `\DATA\MOV15.STR;1` | dev slot (file not on retail disc) |
//! | 11..=22 | `\DATA\MOV.STR;1` | dev multi-window test slots (file not on retail disc) |
//!
//! (An earlier reading used a 64-byte stride, pairing wrong slot
//! halves and concluding `MV2`/`MV5` were unreferenced and slots
//! `5..=11` were cut paths; the disc bytes and the resident RAM
//! capture both encode the 32-byte stride. That reading is
//! superseded - see `docs/formats/str-fmv-table.md`.)
//!
//! The disc-authoritative decoder is
//! [`legaia_asset::fmv_dispatch::FmvTable`] - call paths that hold the
//! PROT 0970 overlay bytes (e.g. the shell's boot-cutscene player)
//! should prefer `FmvTable::engine_path(fmv_id)`, which also carries
//! the per-slot frame range the four `MV3.STR` segments need. The
//! static map below is the disc-free fallback and mirrors the same
//! nine retail slots.
//!
//! ## Post-play return scenes (`0x801CE8AC`)
//!
//! For mid-game slots (`1..=4`, `6..=8`) the master dispatch copies a
//! CDNAME label from the seven-entry list at `0x801CE8AC` into the
//! next-scene name global `0x80084548` (+ a spawn/door word at
//! `0x80084540`), so each FMV returns to a *specific* field scene
//! rather than the trigger scene. Slot 0 hands off to game mode 22
//! (card init) and slot 5 stays in the current scene. See
//! [`fmv_post_play_return_scene`].
//!
//! See [`docs/subsystems/cutscene.md`](../../../../docs/subsystems/cutscene.md)
//! for the full Ghidra-traced provenance.

/// Retail FMV index → STR filename mapping.
///
/// Returns the path the play loop opens for `fmv_id`. The retail
/// index space is `0..=8` (nine retail slots of the 23-slot dispatch
/// table at `0x801D0A6C`, stride `0x20`; the field-VM op `0x4C 0xE2`
/// permits any s16). Slots `9..=22` reference dev-only paths
/// (`MV1A.STR` / `MOV15.STR` / `MOV.STR`) absent from the retail
/// disc.
///
/// Engines that drain a [`crate::field_events::FieldEvent::FmvTrigger`]
/// event use this helper to resolve the operand to a path that their
/// disc handle can open; ids `2..=5` all resolve to `MV3.STR` (the
/// four frame-range segments - the segment window itself comes from
/// [`legaia_asset::fmv_dispatch::FmvTable`], which is the preferred
/// resolver when disc bytes are available). A `None` result means the
/// slot points at a cut/missing path; engines should treat it as a
/// no-op (or surface a playback error if they want to surface it).
///
/// Static fallback map pinned to the disc-decoded dispatch table
/// (`FUN_801CEA3C`, table `0x801D0A6C`, stride `0x20`; parser
/// `legaia_asset::fmv_dispatch`).
// REF: FUN_801CEA3C
pub fn fmv_index_to_str_filename(fmv_id: i16) -> Option<&'static str> {
    match fmv_id {
        0 => Some("MOV/MV1.STR"),
        1 => Some("MOV/MV2.STR"),
        2..=5 => Some("MOV/MV3.STR"),
        6 => Some("MOV/MV4.STR"),
        7 => Some("MOV/MV5.STR"),
        8 => Some("MOV/MV6.STR"),
        // 9..=22: dev slots (`\MOV\MV1A.STR;1`, `\DATA\MOV15.STR;1`,
        // `\DATA\MOV.STR;1`) - the files don't exist on retail USA.
        // The field VM and debug menu can still write these values;
        // engines should treat them as a no-op.
        _ => None,
    }
}

/// Inverse of [`fmv_index_to_str_filename`]: resolve a `MV*.STR`
/// filename to a retail FMV index that plays it. Case-insensitive on
/// the basename so `mv1.str` and `MOV/MV1.STR` both round-trip.
///
/// Multi-slot files round-trip to the **first** slot that references
/// them (so `MV3.STR` returns `Some(2)` even though slots `3..=5`
/// also play it as later frame-range segments).
///
/// Returns `None` for filenames that no FMV slot references (dev
/// leftovers and arbitrary names).
pub fn str_filename_to_fmv_index(str_filename: &str) -> Option<i16> {
    let trimmed = str_filename
        .rsplit_once('/')
        .map(|(_, name)| name)
        .unwrap_or(str_filename);
    match trimmed.to_ascii_uppercase().as_str() {
        "MV1.STR" => Some(0),
        "MV2.STR" => Some(1),
        "MV3.STR" => Some(2),
        "MV4.STR" => Some(6),
        "MV5.STR" => Some(7),
        "MV6.STR" => Some(8),
        _ => None,
    }
}

/// The post-play return scene the master dispatch (`FUN_801CEA3C`)
/// hands control to after `fmv_id` finishes: a CDNAME label from the
/// seven-entry list at `0x801CE8AC`, written to the next-scene name
/// global `0x80084548` with a spawn/door word at `0x80084540`.
///
/// `None` for the slots without a scene hand-off: `fmv_id 0` (the
/// intro; hands off to game mode 22 / card init), `fmv_id 5` (stays
/// in the current scene) and every dev slot.
pub fn fmv_post_play_return_scene(fmv_id: i16) -> Option<&'static str> {
    match fmv_id {
        1 => Some("town0b"),
        2 => Some("map01"),
        3 => Some("chitei2"),
        4 => Some("map02"),
        6 => Some("jou"),
        7 => Some("uru2"),
        8 => Some("town0e"),
        _ => None,
    }
}

/// Retail's "next game mode = StrInit" constant. The handler at
/// `0x801E30E4` writes this byte to the next-game-mode global
/// (`_DAT_8007B83C`) every time it fires; the main mode dispatcher
/// at `0x80017714` then transitions into mode 26 on the next frame.
pub const STR_INIT_GAME_MODE: u8 = 0x1A;

/// Number of FMV slots in the dispatch table at `0x801D0A6C`
/// (9 retail + 14 dev). Matches
/// [`legaia_asset::fmv_dispatch::FMV_SLOT_COUNT`].
pub const FMV_SLOT_COUNT: usize = 23;

/// PSX-virtual address of the FMV dispatch table base.
pub const FMV_STATE_TABLE_ADDR: u32 = 0x801D_0A6C;

/// Stride (bytes) of one FMV dispatch slot - the `sll v0,v0,0x5`
/// selector at overlay VA `0x801CEC9C` (`FUN_801CEA3C`).
pub const FMV_STATE_SLOT_STRIDE: u32 = 0x20;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmv_index_round_trip_for_first_slots() {
        // Every retail movie file round-trips through the first slot
        // that references it.
        for (idx, name) in [
            (0, "MV1.STR"),
            (1, "MV2.STR"),
            (2, "MV3.STR"),
            (6, "MV4.STR"),
            (7, "MV5.STR"),
            (8, "MV6.STR"),
        ] {
            let path = fmv_index_to_str_filename(idx).expect("retail slot maps");
            let bare = path.rsplit_once('/').map(|(_, n)| n).unwrap_or(path);
            assert_eq!(bare, name);
            assert_eq!(str_filename_to_fmv_index(bare), Some(idx));
            assert_eq!(str_filename_to_fmv_index(path), Some(idx));
        }
    }

    #[test]
    fn fmv_ids_2_to_5_are_mv3_segments() {
        // Slots 2..=5 share the `\MOV\MV3.STR;1` path pointer; the
        // per-slot frame ranges (`+0x08`/`+0x0C`) carve out four
        // abutting segments.
        for id in 2..=5 {
            assert_eq!(fmv_index_to_str_filename(id), Some("MOV/MV3.STR"));
        }
        // The reverse lookup picks the first slot referencing the file.
        assert_eq!(str_filename_to_fmv_index("MV3.STR"), Some(2));
    }

    #[test]
    fn every_retail_movie_is_dispatched() {
        // The corrected 32-byte-stride table dispatches every movie on
        // the disc - the old "MV2/MV5 unreferenced" reading is retired.
        assert_eq!(str_filename_to_fmv_index("MV2.STR"), Some(1));
        assert_eq!(str_filename_to_fmv_index("MV5.STR"), Some(7));
        assert_eq!(fmv_index_to_str_filename(1), Some("MOV/MV2.STR"));
        assert_eq!(fmv_index_to_str_filename(7), Some("MOV/MV5.STR"));
    }

    #[test]
    fn dev_slots_resolve_to_none() {
        // Slots 9..=22 point at \MOV\MV1A.STR / \DATA\MOV15.STR /
        // \DATA\MOV.STR which don't exist on retail.
        // fmv_index_to_str_filename returns None for them - engines
        // should treat as a no-op.
        for i in 9..=22 {
            assert_eq!(fmv_index_to_str_filename(i), None);
        }
    }

    #[test]
    fn fmv_index_out_of_range_returns_none() {
        assert_eq!(fmv_index_to_str_filename(-1), None);
        assert_eq!(fmv_index_to_str_filename(23), None);
        assert_eq!(fmv_index_to_str_filename(i16::MAX), None);
    }

    #[test]
    fn post_play_return_scenes_match_the_0x801ce8ac_list() {
        // The seven mid-game hand-offs, in fmv_id order.
        let expected = [
            (1, "town0b"),
            (2, "map01"),
            (3, "chitei2"),
            (4, "map02"),
            (6, "jou"),
            (7, "uru2"),
            (8, "town0e"),
        ];
        for (id, scene) in expected {
            assert_eq!(fmv_post_play_return_scene(id), Some(scene));
        }
        // Slot 0 hands off to mode 22 (card init), slot 5 stays put,
        // dev slots have no hand-off.
        assert_eq!(fmv_post_play_return_scene(0), None);
        assert_eq!(fmv_post_play_return_scene(5), None);
        assert_eq!(fmv_post_play_return_scene(9), None);
    }

    #[test]
    fn str_filename_unknown_returns_none() {
        assert_eq!(str_filename_to_fmv_index("MV7.STR"), None);
        assert_eq!(str_filename_to_fmv_index("MV1A.STR"), None);
        assert_eq!(str_filename_to_fmv_index("garbage"), None);
        assert_eq!(str_filename_to_fmv_index(""), None);
    }

    #[test]
    fn str_filename_case_insensitive() {
        assert_eq!(str_filename_to_fmv_index("mv1.str"), Some(0));
        assert_eq!(str_filename_to_fmv_index("MOV/mv6.STR"), Some(8));
    }

    #[test]
    fn str_init_game_mode_matches_retail() {
        assert_eq!(STR_INIT_GAME_MODE, 26);
    }

    #[test]
    fn fmv_state_table_constants_are_consistent() {
        // The slot for fmv_id N lives at FMV_STATE_TABLE_ADDR + N * 0x20.
        assert_eq!(FMV_STATE_TABLE_ADDR, 0x801D_0A6C);
        assert_eq!(FMV_STATE_SLOT_STRIDE, 0x20);
        assert_eq!(FMV_SLOT_COUNT, 23);
        assert_eq!(FMV_SLOT_COUNT, legaia_asset::fmv_dispatch::FMV_SLOT_COUNT);
    }
}
