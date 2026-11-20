//! Disc-gated: a door's field init holds longer where the destination
//! streams a pack the floor cannot hide.
//!
//! The init reads the scene block's `+4` entry when the asset table carries a
//! `Flag(0x14)` / `Flag(0x0A)` descriptor (`FUN_8002541C`, `0x801D6C18`),
//! before its `0x7E`-vsync floor test. Cycle-stamped on PCSX-Redux, `map01`
//! -> `town01` (`s2_rimelm_town01`) spends `86` vsyncs in that read
//! (town01's `180`-sector pack) and holds mode `2` for `256` vsyncs, where
//! `map02` -> `stone`, with no pack, holds `241`. `doman`'s and `balden2`'s
//! packs are read under the floor.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` or `extracted/` is missing.

use std::path::PathBuf;

use legaia_engine_scene::scene::Scene;
use legaia_engine_scene_host::scene_host::{SceneHost, streamed_pack_read_vsyncs};
use legaia_engine_system::mode_entry_init::door_field_init_hold_vsyncs;

fn extracted_dir() -> Option<PathBuf> {
    ["extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists())
}

#[test]
fn the_door_hold_follows_the_destinations_streamed_pack() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let pack = |name: &str| {
        let scene = Scene::load(&host.index, name).expect(name);
        streamed_pack_read_vsyncs(&host.index, &scene)
    };
    // The measured holds (PCSX-Redux door hops), within two vsyncs.
    for (scene, keeps_music, measured) in [
        ("town01", false, 256),
        ("stone", false, 241),
        ("map02", false, 241),
        ("doman", false, 242),
        ("balden2", true, 184),
    ] {
        let got = door_field_init_hold_vsyncs(keeps_music, pack(scene));
        assert!(
            got.abs_diff(measured) <= 2,
            "{scene}: hold {got}, measured {measured}"
        );
    }
    assert!(pack("town01") > 80, "town01 streams its 180-sector pack");
    assert_eq!(pack("stone"), 0, "stone streams no pack");
}
