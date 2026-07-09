//! Disc-gated regression test for [`befect_cluster::extract`] against the
//! real `befect_data` cluster - retail extraction entries 870..874, the four
//! dev-named battle-effect files (`etim.dat` / `etmd.dat` / `vdf.dat` /
//! `efect.dat`) the battle scene loader `FUN_800520F0` pulls sequentially
//! (see `docs/formats/effect.md` § the befect_data map).
//!
//! The cluster's per-entry PROT extraction over-reads (the entries overlap on
//! disc), so this asserts the footprint-bounded, classified shape. The
//! entry/atlas/script counts and VRAM targets are stable disc invariants.
//!
//! Skips silently when `LEGAIA_DISC_BIN` is unset or when `PROT.DAT` /
//! `CDNAME.TXT` aren't on disk.

use std::path::PathBuf;

use legaia_asset::befect_cluster::{self, Component};
use legaia_prot::archive::Archive;

fn extracted() -> Option<(PathBuf, PathBuf)> {
    for base in ["extracted", "../../extracted"] {
        let prot = PathBuf::from(base).join("PROT.DAT");
        let cdname = PathBuf::from(base).join("CDNAME.TXT");
        if prot.is_file() && cdname.is_file() {
            return Some((prot, cdname));
        }
    }
    None
}

#[test]
fn befect_cluster_extracts_clean_classified_parts() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some((prot, cdname_path)) = extracted() else {
        eprintln!("[skip] extracted/PROT.DAT or CDNAME.TXT missing");
        return;
    };

    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let names = legaia_prot::cdname::parse(&cdname_path).expect("parse CDNAME");
    let cluster = befect_cluster::extract(&mut archive, &names).expect("extract befect cluster");

    assert_eq!(
        cluster.first_index, 870,
        "retail befect_data block starts at extraction entry 870 (raw define 872)"
    );
    assert_eq!(cluster.parts.len(), 4, "etim / etmd / vdf / efect");

    // Part 0: entry 870 - `etim.dat`, the effect texture pages: three 64x256
    // 4bpp TIMs targeting VRAM (320,0)/(384,0)/(448,0), CLUTs at rows
    // 474..=476 (byte-verified against live battle VRAM captures; see
    // docs/formats/effect.md).
    assert_eq!(cluster.parts[0].prot_index, 870);
    assert_eq!(cluster.parts[0].lzs_section, None);
    match &cluster.parts[0].component {
        Component::TimImages { tims } => {
            assert_eq!(tims.len(), 3, "etim.dat carries three texture pages");
            type Target = (u16, u16, Option<(u16, u16)>);
            let targets: Vec<Target> = tims.iter().map(|t| (t.fb_x, t.fb_y, t.clut_fb)).collect();
            assert_eq!(
                targets,
                vec![
                    (320, 0, Some((0, 474))),
                    (384, 0, Some((0, 475))),
                    (448, 0, Some((0, 476))),
                ],
                "etim.dat VRAM targets"
            );
            assert!(
                tims.iter()
                    .all(|t| t.bpp == 4 && t.w_hw == 64 && t.h == 256)
            );
        }
        other => panic!("part 0 expected TimImages, got {other:?}"),
    }

    // Part 1: entry 871 - `etmd.dat`, the effect 3D model pack.
    assert_eq!(cluster.parts[1].prot_index, 871);
    match &cluster.parts[1].component {
        Component::TmdPack { count } => assert_eq!(*count, 30, "etmd.dat effect models"),
        other => panic!("part 1 expected TmdPack, got {other:?}"),
    }

    // Part 2: entry 872 - `vdf.dat`, a 32-entry geometry/billboard offset pack.
    assert_eq!(cluster.parts[2].prot_index, 872);
    match &cluster.parts[2].component {
        Component::OffsetPack { count } => assert_eq!(*count, 32),
        other => panic!("part 2 expected OffsetPack, got {other:?}"),
    }

    // Part 3: entry 873 - the `efect.dat` 2-pack, footprint-bounded to 0x2000
    // (not bleeding into the neighbouring player_data entry).
    assert_eq!(cluster.parts[3].prot_index, 873);
    match &cluster.parts[3].component {
        Component::EffectScript2Pack {
            atlas_entries,
            anim_batches,
            scripts,
        } => {
            assert_eq!(*atlas_entries, 144);
            assert_eq!(*anim_batches, 14);
            assert_eq!(*scripts, 33);
        }
        other => panic!("part 3 expected EffectScript2Pack, got {other:?}"),
    }
    assert_eq!(
        cluster.parts[3].len, 0x2000,
        "efect.dat true footprint is 0x2000"
    );
}
