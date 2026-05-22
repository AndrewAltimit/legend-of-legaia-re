//! Disc-gated regression test for [`befect_cluster::extract`] against the
//! real `befect_data` cluster (PROT entries 872..876).
//!
//! The cluster's per-entry PROT extraction over-reads (the entries overlap on
//! disc), so this asserts the footprint-bounded, LZS-expanded, classified
//! shape: a geometry pack, the `efect.dat` 2-pack, an LZS container of three
//! sub-files (effect-model TMDs / a pack / effect-texture TIMs), and a raw
//! page blob. The entry/atlas/script counts are stable disc invariants.
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

    assert_eq!(cluster.first_index, 872, "befect_data starts at PROT 872");
    assert_eq!(
        cluster.parts.len(),
        6,
        "4 entries, with entry 874 split into 3 LZS sections"
    );

    // Part 0: entry 872 - a 32-entry geometry/billboard offset pack.
    match &cluster.parts[0].component {
        Component::OffsetPack { count } => assert_eq!(*count, 32),
        other => panic!("part 0 expected OffsetPack, got {other:?}"),
    }
    assert_eq!(cluster.parts[0].prot_index, 872);
    assert_eq!(cluster.parts[0].lzs_section, None);

    // Part 1: entry 873 - the efect.dat 2-pack, footprint-bounded to ~0x2000
    // (no longer bleeding into entry 874).
    match &cluster.parts[1].component {
        Component::EffectScript2Pack {
            atlas_entries,
            anim_batches,
            scripts,
        } => {
            assert_eq!(*atlas_entries, 144);
            assert_eq!(*anim_batches, 14);
            assert_eq!(*scripts, 33);
        }
        other => panic!("part 1 expected EffectScript2Pack, got {other:?}"),
    }
    assert_eq!(
        cluster.parts[1].len, 0x2000,
        "efect.dat true footprint is 0x2000"
    );

    // Part 2: entry 874 LZS section 0 - the effect 3D models.
    assert_eq!(cluster.parts[2].prot_index, 874);
    assert_eq!(cluster.parts[2].lzs_section, Some(0));
    match &cluster.parts[2].component {
        Component::TmdPack { count } => assert_eq!(*count, 5),
        other => panic!("part 2 expected TmdPack, got {other:?}"),
    }

    // Part 3: entry 874 LZS section 1 - a generic offset pack.
    assert_eq!(cluster.parts[3].lzs_section, Some(1));
    assert!(matches!(
        cluster.parts[3].component,
        Component::OffsetPack { .. }
    ));

    // Part 4: entry 874 LZS section 2 - `etim.dat`, the effect texel source.
    // CLUTs in the high VRAM rows 473..478; the pixel blocks byte-match a live
    // battle VRAM dump captured mid-cast (verified out-of-band).
    assert_eq!(cluster.parts[4].lzs_section, Some(2));
    match &cluster.parts[4].component {
        Component::TimImages { tims } => {
            assert!(
                tims.len() >= 8,
                "expected >=8 effect-texture TIMs, got {}",
                tims.len()
            );
            assert!(
                tims.iter()
                    .all(|t| t.clut_fb.map(|(_, y)| y >= 473).unwrap_or(false)),
                "effect-texture CLUTs live in the high VRAM rows"
            );
        }
        other => panic!("part 4 expected TimImages, got {other:?}"),
    }

    // Part 5: entry 875 - a raw 0x20000 page blob (2D-billboard texel
    // candidate; upload mechanism not yet pinned).
    assert_eq!(cluster.parts[5].prot_index, 875);
    assert_eq!(cluster.parts[5].len, 0x20000);
    assert!(matches!(cluster.parts[5].component, Component::Raw));
}
