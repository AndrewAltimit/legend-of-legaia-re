//! Disc-gated decode of the STR FMV dispatch table out of the real STR/MDEC
//! overlay (PROT 0970), pinning the five retail `fmv_id -> MVn.STR` entries +
//! their frame ranges.
//!
//! The overlay loads verbatim from its PROT entry (`form = raw` in the
//! static-overlay map), so the raw entry bytes are byte-identical to the
//! as-loaded overlay and the table at VA `0x801D0A6C` decodes directly.
//!
//! Skips + passes when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.

use legaia_asset::fmv_dispatch::FmvTable;
use std::path::PathBuf;

fn str_overlay() -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for dir in ["extracted/PROT", "../../extracted/PROT"] {
        let d = PathBuf::from(dir);
        if !d.is_dir() {
            continue;
        }
        // PROT 0970 - the STR/MDEC cutscene overlay.
        let entry = std::fs::read_dir(&d)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.starts_with("0970_"))
            })?;
        return std::fs::read(entry).ok();
    }
    None
}

#[test]
fn retail_fmv_dispatch_table_decodes_from_disc() {
    let Some(overlay) = str_overlay() else {
        eprintln!("[skip] extracted/PROT/0970 or LEGAIA_DISC_BIN missing");
        return;
    };
    let table = FmvTable::from_str_overlay(&overlay).expect("decode FMV dispatch table");

    // All 12 slots decode (5 retail + dev placeholders).
    assert_eq!(table.entries.len(), 12, "full FMV slot table");

    // The five retail FMVs: file + (start, end) frame range. fmv 1 + 2 are two
    // distinct cutscenes carved out of the single MV3.STR by frame range; fmv 2
    // is the one that seeks in to frame 0x1a5.
    let want: &[(i16, &str, u32, u32)] = &[
        (0, "MOV/MV1.STR", 1, 0x53a),
        (1, "MOV/MV3.STR", 1, 0xe1),
        (2, "MOV/MV3.STR", 0x1a5, 0x27b),
        (3, "MOV/MV4.STR", 1, 0x152),
        (4, "MOV/MV6.STR", 1, 0x297),
    ];
    for &(id, path, start, end) in want {
        let e = table
            .entry(id)
            .unwrap_or_else(|| panic!("fmv {id} present"));
        assert_eq!(
            table.engine_path(id).as_deref(),
            Some(path),
            "fmv {id} path"
        );
        assert_eq!(e.start_frame, start, "fmv {id} start frame");
        assert_eq!(e.end_frame, end, "fmv {id} end frame");
        assert_eq!((e.width, e.height), (320, 240), "fmv {id} dims");
        assert!(e.on_retail_disc(), "fmv {id} is a retail movie");
    }

    // The dev slots (5..) reference MOV15.STR / MOV.STR, which aren't on the
    // released disc, so engine_path() declines them.
    for id in 5..12i16 {
        assert!(
            !table.entry(id).unwrap().on_retail_disc(),
            "dev slot {id} is not a retail movie"
        );
        assert_eq!(
            table.engine_path(id),
            None,
            "dev slot {id} has no retail path"
        );
    }

    // Cross-check the disc decode against the engine's resolver: every retail
    // slot agrees, so the engine mapping is faithfully disc-sourced.
    for &(id, path, ..) in want {
        assert_eq!(
            legaia_engine_core_cutscene_path(id).as_deref(),
            Some(path),
            "fmv {id} matches the engine resolver"
        );
    }
}

/// Mirror of `legaia_engine_core::cutscene::fmv_index_to_str_filename` (kept
/// local so `legaia-asset` doesn't depend on the engine crate).
fn legaia_engine_core_cutscene_path(fmv_id: i16) -> Option<String> {
    Some(
        match fmv_id {
            0 => "MOV/MV1.STR",
            1 | 2 => "MOV/MV3.STR",
            3 => "MOV/MV4.STR",
            4 => "MOV/MV6.STR",
            _ => return None,
        }
        .to_string(),
    )
}
