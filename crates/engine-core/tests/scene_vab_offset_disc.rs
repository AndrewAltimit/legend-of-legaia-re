//! Disc-gated oracle for the offset [`SceneHost::scene_vab_bytes`] hands out.
//!
//! A scene's VAB-bearing PROT entry is a `scene_vab_stream`: a DATA_FIELD
//! chunk stream whose chunk 0 carries the bank's header part. So the entry
//! starts with a 4-byte chunk header and the `pBAV` magic sits at `+4`. No
//! retail PROT entry begins with that magic, which makes parsing the entry at
//! offset 0 a guaranteed error rather than a slightly-wrong bank - the scene's
//! instruments simply never reach the SPU, and the only symptom is silence.
//!
//! The test pins both halves so the offset cannot quietly go back to 0:
//!
//!   1. every scene the host reports a VAB entry for parses at the offset the
//!      accessor returns, and
//!   2. the same buffer does **not** parse at offset 0.
//!
//! Skip-pass (CLAUDE.md disc-gated convention) when `extracted/` is absent.

use std::path::PathBuf;

use legaia_engine_core::scene::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn scene_vab_bytes_reports_a_parseable_offset() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - scene VAB offset oracle skipped");
        return;
    };

    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    let mut scene_names: Vec<String> = cdname.values().cloned().collect();
    scene_names.sort();
    scene_names.dedup();
    assert!(!scene_names.is_empty(), "no CDNAME scene names resolved");

    let mut with_vab = 0usize;
    let mut fails_at_zero = 0usize;
    let mut carriers: Vec<String> = Vec::new();

    for name in &scene_names {
        if host.load_scene(name).is_err() {
            continue;
        }
        let Ok(Some((bytes, vab_off))) = host.scene_vab_bytes() else {
            continue;
        };
        with_vab += 1;
        carriers.push(format!("{name}@{vab_off:#x}"));
        assert!(
            legaia_vab::parse(&bytes, vab_off).is_ok(),
            "scene {name}: VAB does not parse at the reported offset {vab_off:#x}"
        );
        if legaia_vab::parse(&bytes, 0).is_err() {
            fails_at_zero += 1;
        }
    }

    eprintln!(
        "[ok] scene VAB offset: {with_vab} of {} scene name(s) carry a VAB entry, \
         all parse at the reported offset, {fails_at_zero} fail at offset 0; \
         carriers: {}",
        scene_names.len(),
        carriers.join(" ")
    );
    assert!(
        with_vab > 0,
        "no scene surfaced a VAB entry - the sweep is vacuous"
    );
    // The whole reason the accessor returns the offset: offset 0 is the chunk
    // header word, never the bank. If this stops holding, the container shape
    // changed and every caller needs re-checking.
    assert_eq!(
        fails_at_zero, with_vab,
        "some scene VAB entry parses at offset 0 - the stream shape changed"
    );
}
