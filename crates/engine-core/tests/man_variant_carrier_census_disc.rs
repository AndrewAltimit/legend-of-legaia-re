//! Disc-gated: the streaming **variant MAN carriers** and the story-spine
//! flag writers the carrier-complete SYSTEM-flag census surfaces.
//!
//! Thirteen retail blocks ship a second MAN as the type-3 chunk of a
//! standalone `data_field_streaming` entry - story-state variants with
//! different partition counts and different scripts than the bundle MAN
//! (for the v12-family dungeons `rikuroa` / `dolk2` the streaming carrier
//! is the scene's ONLY MAN; the live script heap at the Mt. Rikuroa Caruban
//! beat byte-matches PROT `0157`'s chunk). A bundle-only census is
//! structurally blind to every script op these carriers hold - which is
//! exactly where the chapter-1 story-spine writers turned out to live:
//!
//!   - `0x142` (Caruban beat / dolk->dolk2 switch): SET by the rikuroa
//!     carrier's P1[10..12] + post-victory record P2[50] (the C1 self-latch
//!     the firehose caught live, `51 42`, `ra 0x801E3598`), re-asserted by
//!     dolk2's carrier P1[0..1], CLEARed by dolk's bundle P1[26].
//!   - `0x482` (Drake mist walls): SET by the `other7` script pool
//!     P1[15]/P1[39], CLEARed by the epilogue variant carriers
//!     (`edbalden` / `eddoman`).
//!   - `0x1BE`: SET by `geremi` P2[0] (Jeremi's arrival one-shot "Meta's
//!     warning", C1 = itself) - surfaced by the partition-2 named-record
//!     header fix, NOT a rikuroa/Zeto flag.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing
//! (CLAUDE.md disc-gated convention).

use std::collections::BTreeSet;
use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{scene_man_carriers, system_flag_census};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_vm::field_disasm::FlagKind;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_index() -> Option<ProtIndex> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    Some(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"))
}

/// The thirteen streaming variant carriers, pinned by `(scene, entry_idx)`.
#[test]
fn thirteen_streaming_variant_carriers_disc_wide() {
    let Some(index) = open_index() else { return };
    let mut found: BTreeSet<(String, u32)> = BTreeSet::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for c in scene_man_carriers(&index, &scene) {
            if c.is_variant() {
                found.insert((name.clone(), c.entry_idx));
            }
        }
    }
    eprintln!("[carriers] {found:?}");
    let want: BTreeSet<(String, u32)> = [
        ("dolk2", 70u32),
        ("rikuroa2", 122),
        ("rikuroa", 157),
        ("rayman", 201),
        ("station", 228),
        ("balden2", 320),
        ("ropeway2", 339),
        ("taiku", 373),
        ("doman", 401),
        ("taiku2", 427),
        ("nilboa2", 648),
        ("edbalden", 792),
        ("eddoman", 817),
    ]
    .into_iter()
    .map(|(s, i)| (s.to_string(), i))
    .collect();
    assert_eq!(
        found, want,
        "streaming variant MAN carrier set (scene, entry_idx)"
    );
}

/// The carrier-complete census surfaces the chapter-1 spine writers.
#[test]
fn spine_flag_writers_surface_in_the_carrier_census() {
    let Some(index) = open_index() else { return };
    let scenes = index.cdname_scene_names();
    let census = system_flag_census(&index, &scenes);

    // 0x142: rikuroa carrier SETs (incl. the P2[50] post-victory one-shot
    // the firehose caught live) + dolk2 carrier re-asserts.
    let s142: BTreeSet<(String, bool, usize, usize)> = census
        .get(&0x142)
        .expect("0x142 sites")
        .iter()
        .filter(|h| h.kind == FlagKind::Set)
        .map(|h| (h.scene_name.clone(), h.variant, h.partition, h.record))
        .collect();
    assert_eq!(
        s142,
        BTreeSet::from([
            ("dolk2".to_string(), true, 1, 0),
            ("dolk2".to_string(), true, 1, 1),
            ("rikuroa".to_string(), true, 1, 10),
            ("rikuroa".to_string(), true, 1, 11),
            ("rikuroa".to_string(), true, 1, 12),
            ("rikuroa".to_string(), true, 2, 50),
        ]),
        "0x142 SET sites (all in streaming variant carriers)"
    );

    // 0x482: set in other6/other7, cleared by the epilogue carriers.
    let s482_set: BTreeSet<(String, usize, usize)> = census
        .get(&0x482)
        .expect("0x482 sites")
        .iter()
        .filter(|h| h.kind == FlagKind::Set)
        .map(|h| (h.scene_name.clone(), h.partition, h.record))
        .collect();
    assert_eq!(
        s482_set,
        BTreeSet::from([("other7".to_string(), 1, 15), ("other7".to_string(), 1, 39),]),
        "0x482 SET sites (other7 script pool)"
    );
    assert!(
        census
            .get(&0x482)
            .unwrap()
            .iter()
            .any(|h| { h.scene_name == "edbalden" && h.variant && h.kind == FlagKind::Clear }),
        "0x482 cleared by the edbalden epilogue variant carrier"
    );

    // 0x1BE: geremi's arrival one-shot (P2[0] self-latch) - the flag the
    // earlier misattributed frame read as a rikuroa/Zeto gate.
    let s1be: BTreeSet<(String, usize, usize, bool)> = census
        .get(&0x1BE)
        .expect("0x1BE sites")
        .iter()
        .filter(|h| h.kind == FlagKind::Set)
        .map(|h| (h.scene_name.clone(), h.partition, h.record, h.variant))
        .collect();
    assert_eq!(
        s1be,
        BTreeSet::from([("geremi".to_string(), 2, 0, false)]),
        "0x1BE single SET: geremi P2[0] (Jeremi arrival one-shot)"
    );
}

/// The rikuroa carrier IS the scene's MAN (v12-family: no bundle MAN), and
/// its post-victory record P2[50] gates on `0x142` itself - the C1
/// self-latching one-shot shape.
#[test]
fn rikuroa_carrier_p2_50_is_the_0x142_self_latch() {
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some(index) = open_index() else { return };
    let scene = Scene::load(&index, "rikuroa").expect("load rikuroa");
    let man = scene
        .field_man_payload(&index)
        .expect("payload")
        .expect("rikuroa MAN resolves");
    let mf = legaia_asset::man_section::parse(&man).expect("parse");
    assert_eq!(mf.header.partition_counts, [13, 29, 64]);
    let (c1, c2) = partition2_record_gates(&mf, &man, 50).expect("P2[50] gates");
    assert_eq!(
        c1,
        vec![0x142],
        "P2[50] C1 blocks respawn once 0x142 is set"
    );
    assert!(c2.is_empty(), "P2[50] has no requires-all gate");
}
