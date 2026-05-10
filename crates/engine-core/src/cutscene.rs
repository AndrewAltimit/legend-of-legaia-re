//! FMV / pre-rendered cutscene helpers.
//!
//! The retail field VM triggers an FMV via opcode `0x4C 0xE2`
//! (handler at `0x801E30E4` in the cutscene-dialogue overlay). The
//! handler writes the s16 operand to `_DAT_8007BA78` (FMV index used
//! by the runtime FMV-state table at `0x801D0A6C`) and pokes the
//! next-game-mode global `_DAT_8007B83C` to `0x1A` (game mode 26 =
//! StrInit). The str_fmv overlay then resolves the index to a STR
//! file and starts MDEC + XA playback.
//!
//! The runtime FMV-state table at `0x801D0A6C` is populated from the
//! compact MV-file table at `0x801CAE40` (parsed by
//! [`legaia_asset::str_fmv_table`]). On retail USA the disc carries
//! six numbered movie files (`MV1.STR..MV6.STR`); the table indexes
//! into them in order.
//!
//! See [`docs/subsystems/cutscene.md`](../../../../docs/subsystems/cutscene.md)
//! for the full Ghidra-traced provenance.

/// Retail FMV index → MV filename mapping.
///
/// The runtime table at `0x801D0A6C` (str_fmv overlay) is populated
/// from the compact MV-file table at `0x801CAE40`; both tables ship
/// the same six entries in the same order. Engines that drain a
/// [`crate::field_events::FieldEvent::FmvTrigger`] event use this
/// helper to resolve the operand to a `MOV/MVn.STR` path that their
/// disc handle can open.
///
/// Returns `None` for indices outside `0..=5` - the field VM permits
/// out-of-range writes (the handler is unconditional), but the str_fmv
/// overlay's lookup at `0x801D0A6C` is only valid for 6 slots. Engines
/// that see an out-of-range index should treat it as a script-side
/// sentinel (e.g. clear a pending FMV) rather than a playback request.
pub fn fmv_index_to_str_filename(fmv_id: i16) -> Option<&'static str> {
    match fmv_id {
        0 => Some("MOV/MV1.STR"),
        1 => Some("MOV/MV2.STR"),
        2 => Some("MOV/MV3.STR"),
        3 => Some("MOV/MV4.STR"),
        4 => Some("MOV/MV5.STR"),
        5 => Some("MOV/MV6.STR"),
        _ => None,
    }
}

/// Inverse of [`fmv_index_to_str_filename`]: resolve a `MV*.STR`
/// filename to its retail FMV index. Case-insensitive on the
/// basename so `mv1.str` and `MOV/MV1.STR` both round-trip.
///
/// Returns `None` for filenames that don't match any of the six
/// numbered FMV entries.
pub fn str_filename_to_fmv_index(str_filename: &str) -> Option<i16> {
    let trimmed = str_filename
        .rsplit_once('/')
        .map(|(_, name)| name)
        .unwrap_or(str_filename);
    match trimmed.to_ascii_uppercase().as_str() {
        "MV1.STR" => Some(0),
        "MV2.STR" => Some(1),
        "MV3.STR" => Some(2),
        "MV4.STR" => Some(3),
        "MV5.STR" => Some(4),
        "MV6.STR" => Some(5),
        _ => None,
    }
}

/// Retail's "next game mode = StrInit" constant. The handler at
/// `0x801E30E4` writes this byte to the next-game-mode global
/// (`_DAT_8007B83C`) every time it fires; the main mode dispatcher
/// at `0x80017714` then transitions into mode 26 on the next frame.
pub const STR_INIT_GAME_MODE: u8 = 0x1A;

/// Number of FMV slots in the runtime FMV-state table at `0x801D0A6C`
/// (mirrors the compact MV-file table at `0x801CAE40`).
pub const FMV_SLOT_COUNT: usize = 6;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmv_index_round_trip_pins_six_files() {
        for i in 0..=5 {
            let path = fmv_index_to_str_filename(i).expect("index 0..=5 maps");
            let bare = path.rsplit_once('/').map(|(_, n)| n).unwrap_or(path);
            assert_eq!(str_filename_to_fmv_index(bare), Some(i));
            assert_eq!(str_filename_to_fmv_index(path), Some(i));
        }
    }

    #[test]
    fn fmv_index_out_of_range_returns_none() {
        assert_eq!(fmv_index_to_str_filename(-1), None);
        assert_eq!(fmv_index_to_str_filename(6), None);
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
        assert_eq!(str_filename_to_fmv_index("MOV/mv6.STR"), Some(5));
    }

    #[test]
    fn str_init_game_mode_matches_retail() {
        assert_eq!(STR_INIT_GAME_MODE, 26);
    }
}
