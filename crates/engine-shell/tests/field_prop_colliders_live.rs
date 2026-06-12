//! Disc + save-library gated: the engine's static **prop collider** source
//! (`Scene::field_object_placements` `collider_x`/`collider_z` — the
//! static-entity arm of the actor-collision probe, retail `FUN_801cf9f4`
//! result bit `4`) matches the live static collision actors of real retail
//! sessions.
//!
//! For each catalogued capture whose active-actor table (`DAT_801c93c8`,
//! count `_DAT_8007b6b8`) holds static-class entries (`flags+0x10 &
//! 0x1020000 == 0`), the live actor is matched to the engine placement by
//! its object-record index (`actor+0x60`) and the test asserts:
//!
//! - the live actor position (`+0x14`/`+0x18`) equals the placement's spawn
//!   world position (the `FUN_8003A55C` spawn formula), and
//! - the live record-derived box centre (live position + the retail
//!   footprint-offset formula over the **live** field-buffer record bytes,
//!   including the `+0x52 & 8` correction) equals the placement's
//!   `collider_x`/`collider_z` (computed from **disc** bytes) — pinning the
//!   engine's prop-collision source end to end.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` /
//! `scripts/scenarios.toml` / `saves/library` (CI runs without Sony bytes).

use std::path::PathBuf;

use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_mednafen::{SaveState, ScenarioManifest};

/// Catalogued captures whose field buffer + actor table belong to the named
/// field scene and hold at least one static collision actor.
const CAPTURES: &[(&str, &str)] = &[
    ("v0_1_pre_battle_tetsu", "town01"),
    ("vahn_walks_out", "town01"),
    ("mei_house_inside", "town0c"),
    ("minigame_dance_noa", "koin3"),
];

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn manifest_path() -> Option<PathBuf> {
    for c in [
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ] {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn library_dir() -> Option<PathBuf> {
    for c in ["saves/library", "../saves/library", "../../saves/library"] {
        let d = PathBuf::from(c);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

fn rd32(ram: &[u8], va: u32) -> u32 {
    let off = (va & 0x1F_FFFF) as usize;
    u32::from_le_bytes(ram[off..off + 4].try_into().unwrap())
}

fn rd16(ram: &[u8], va: u32) -> u16 {
    let off = (va & 0x1F_FFFF) as usize;
    u16::from_le_bytes(ram[off..off + 2].try_into().unwrap())
}

#[test]
fn field_prop_colliders_match_live_static_actors() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let (Some(manifest_path), Some(library)) = (manifest_path(), library_dir()) else {
        eprintln!("[skip] scenarios manifest / saves library missing");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse manifest");

    let prot = std::fs::read(extracted.join("PROT.DAT")).expect("read PROT.DAT");
    let cdname = std::fs::read_to_string(extracted.join("CDNAME.TXT")).expect("read CDNAME.TXT");
    let index = ProtIndex::from_bytes(prot, Some(&cdname)).expect("build ProtIndex");

    let mut checked_actors = 0;
    for &(label, scene_name) in CAPTURES {
        let Some(scn) = manifest.scenarios.iter().find(|s| s.label == label) else {
            continue;
        };
        let Some(save_path) = manifest.library_save_path(scn, library.as_path()) else {
            continue;
        };
        if !save_path.exists() {
            continue;
        }
        let state = SaveState::from_path(&save_path).expect("parse save state");
        let ram = state.main_ram().expect("main RAM");
        let scratch = state.scratch_ram().expect("scratch RAM");
        let fb = u32::from_le_bytes(scratch[0x3EC..0x3F0].try_into().unwrap());
        assert_eq!(fb & 0xFF00_0000, 0x8000_0000, "{label}: field buffer VA");
        let fb_off = (fb & 0x1F_FFFF) as usize;

        let scene = Scene::load(&index, scene_name).expect("load scene");
        let placements = scene
            .field_object_placements(&index)
            .expect("placements read")
            .expect("scene has a field .MAP");

        let count = rd32(ram, 0x8007_B6B8);
        assert!((1..=0x20).contains(&count), "{label}: live actor count");
        for i in 0..count {
            let ptr = rd32(ram, 0x801C_93C8 + i * 4);
            assert_eq!(ptr & 0xFF00_0000, 0x8000_0000, "{label}: actor ptr");
            let flags = rd32(ram, ptr + 0x10);
            if flags & 0x0102_0000 != 0 {
                continue; // moving-class (NPC) entry — the other arm's oracle
            }
            let live_x = rd16(ram, ptr + 0x14) as i16 as i32;
            let live_z = rd16(ram, ptr + 0x18) as i16 as i32;
            let rec_idx = rd16(ram, ptr + 0x60) as usize;
            let f52 = rd16(ram, ptr + 0x52);

            // The retail footprint offset over the LIVE record bytes.
            let rec_off = fb_off + rec_idx * 0x20;
            let rec = &ram[rec_off..rec_off + 0x20];
            let mut off_x = (rec[0x6] as i8) as i32 * 0x80 + (rec[0xE] as i8) as i32 * 0x10;
            let mut off_z = (rec[0x7] as i8) as i32 * 0x80 + (rec[0xF] as i8) as i32 * 0x10;
            if f52 & 8 != 0 {
                off_x -= i16::from_le_bytes([rec[0x0], rec[0x1]]) as i32;
                off_z += i16::from_le_bytes([rec[0x4], rec[0x5]]) as i32;
            }
            let (live_cx, live_cz) = (live_x + off_x, live_z + off_z);

            let p = placements
                .iter()
                .find(|p| {
                    p.obj_idx as usize == rec_idx && p.world_x == live_x && p.world_z == live_z
                })
                .unwrap_or_else(|| {
                    panic!(
                        "{label}: live static actor (record {rec_idx}, pos \
                         ({live_x},{live_z})) has a matching engine placement"
                    )
                });
            assert_eq!(
                (p.collider_x, p.collider_z),
                (live_cx, live_cz),
                "{label}: record {rec_idx} disc-derived collider centre == live-derived"
            );
            // The live record's `+0x52 & 8` correction class comes from the
            // record flag bit the disc bytes carry.
            assert_eq!(
                (p.flags & 0x8 != 0),
                (f52 & 8 != 0),
                "{label}: record {rec_idx} correction flag mirrors actor +0x52 bit 8"
            );
            checked_actors += 1;
            eprintln!(
                "[prop-live] {label} record {rec_idx} pos ({live_x},{live_z}) \
                 centre ({live_cx},{live_cz}) OK"
            );
        }
    }
    assert!(
        checked_actors >= 3,
        "at least three live static actors validated, got {checked_actors}"
    );
}
