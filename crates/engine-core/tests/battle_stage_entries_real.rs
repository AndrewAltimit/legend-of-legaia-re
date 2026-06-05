//! Disc-gated: a scene's battle-stage backdrop entries resolve from its CDNAME
//! block. The overworld `map01` battle is fought inside the `scene_tmd_stream`
//! half-dome at PROT 88 (with 89/90 as texture variants) — pinned from the
//! fingerprinted `overworld_battle_bg_angle_*` captures.
use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for d in ["extracted", "../../extracted"] {
        let p = PathBuf::from(d);
        if p.join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists() {
            return Some(p);
        }
    }
    None
}

#[test]
fn map01_battle_stage_is_prot_88() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ or LEGAIA_DISC_BIN missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let stages = host.index.battle_stage_entries("map01");
    eprintln!("map01 battle-stage entries: {stages:?}");
    assert!(
        stages.contains(&88),
        "map01 stage should include PROT 88, got {stages:?}"
    );
    // The leading TMD of the stage entry parses as a real multi-object dome.
    let bytes = host.index.entry_bytes(88).expect("read PROT 88");
    let s = legaia_asset::scene_tmd_stream::detect(&bytes).expect("88 is a scene_tmd_stream");
    let tmd = legaia_tmd::parse(&bytes[s.tmd_range()]).expect("parse dome TMD");
    assert_eq!(tmd.objects.len(), 4, "map01 stage dome has 4 objects");
}
