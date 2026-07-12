//! Disc-gated decode of the STR FMV dispatch table out of the real STR/MDEC
//! overlay (PROT 0970), pinning the nine retail `fmv_id -> MVn.STR` entries +
//! their frame ranges.
//!
//! The overlay loads verbatim from its PROT entry (`form = raw` in the
//! static-overlay map), so the raw entry bytes are byte-identical to the
//! as-loaded overlay and the table at VA `0x801D0A6C` decodes directly at the
//! selector's 32-byte stride (`sll v0,v0,0x5` at overlay VA `0x801CEC9C`).
//!
//! Skips + passes when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.

use legaia_asset::fmv_dispatch::{FMV_SLOT_COUNT, FmvTable};
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

    // All 23 slots decode (9 retail + 14 dev placeholders).
    assert_eq!(table.entries.len(), FMV_SLOT_COUNT, "full FMV slot table");

    // The nine retail FMVs: file + (start, end) frame range. Every one of the
    // six on-disc movies is dispatched; MV3.STR carries four distinct
    // cutscenes carved out by frame range.
    let want: &[(i16, &str, u32, u32)] = &[
        (0, "MOV/MV1.STR", 1, 0x53a),
        (1, "MOV/MV2.STR", 1, 0xf4),
        (2, "MOV/MV3.STR", 1, 0xe1),
        (3, "MOV/MV3.STR", 0xe2, 0x1a4),
        (4, "MOV/MV3.STR", 0x1a5, 0x27b),
        (5, "MOV/MV3.STR", 0x27c, 0x36a),
        (6, "MOV/MV4.STR", 1, 0x152),
        (7, "MOV/MV5.STR", 1, 0x288),
        (8, "MOV/MV6.STR", 1, 0x297),
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
        assert_ne!(e.scale_flag, 0, "fmv {id} is 24-bit color");
        assert!(e.on_retail_disc(), "fmv {id} is a retail movie");
    }

    // The dev slots (9..) reference MV1A.STR / MOV15.STR / MOV.STR, which
    // aren't on the released disc, so engine_path() declines them.
    for id in 9..FMV_SLOT_COUNT as i16 {
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

    // Retail frame ranges partition each movie contiguously: MV3's four
    // segments abut (1..0xe1 | 0xe2..0x1a4 | 0x1a5..0x27b | 0x27c..0x36a).
    for pair in [(2i16, 3i16), (3, 4), (4, 5)] {
        let (a, b) = (table.entry(pair.0).unwrap(), table.entry(pair.1).unwrap());
        assert_eq!(a.end_frame + 1, b.start_frame, "MV3 segments abut");
    }
}
