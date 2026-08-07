//! The FMV-trigger -> hand-off ladder: the field-VM `4C E2` trigger plumbing
//! driven with **disc bytes** end to end, through the session runner, into the
//! post-play hand-off map (`FUN_801CEA3C` / `fmv_post_play_handoff`) and out
//! the other side into the hand-off scene.
//!
//! ## What this converts
//!
//! The `801cea3c` reach row: `fmv_post_play_handoff` is the master dispatch's
//! second `switch` (where control goes when a movie ends), and before this
//! ladder its only caller under coverage was the `play` subcommand's `bin/`
//! arm - which no pad ladder reaches, because no ladder run ever fired an FMV.
//! The synthetic halves are already pinned (`field_records.rs` drives a
//! hand-typed `4C E2`; `w1a_fmv_ladder` plays every retail STR); what was
//! missing is the disc-sourced chain: a real scene's own trigger op, executed
//! by the session's field VM, flipping the world into `SceneMode::Cutscene`,
//! and the hand-off resolving where retail says that movie lands.
//!
//! ## Why the runner enters the record AT the trigger op
//!
//! Every one of the eight trigger ops sits `0x16D8..0x7CD3` bytes deep inside
//! a 10-35 KB story-cutscene record (the Mist attack, the Juggernaut reveal,
//! ...). Those records' choreography spins on actor-motion waits (`26 FE FF`
//! self-jumps that poll actor state) and multi-actor beats a headless world
//! never satisfies - the same structural bound
//! `docs/tooling/reach-triage.md` records for pad-only ladders. So the ladder
//! loads the record's own bytes *sliced at the trigger op* into the session's
//! field-VM channel: the op, its literal `fmv_id` operand, and everything
//! downstream (pending trigger -> mode flip -> dispatch slot -> hand-off) are
//! disc-sourced and session-driven; only the preceding choreography is
//! skipped, and that half is not what the row measures.
//!
//! Disc-gated: skip-passes when `LEGAIA_DISC_BIN` is unset or `extracted/`
//! is absent, per the repo convention.

use std::path::PathBuf;
use std::sync::Arc;

use legaia_engine_core::cutscene::{FmvHandoff, fmv_post_play_handoff};
use legaia_engine_core::man_field_scripts::scene_fmv_triggers;
use legaia_engine_core::scene::{ProtIndex, Scene, SceneHost};
use legaia_engine_core::world::SceneMode;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn gate() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let d = extracted_dir();
    if d.is_none() {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    }
    d
}

/// The eight trigger scenes and the `fmv_id` each MAN's script fires
/// (pinned byte-exactly by `scene_fmv_triggers_disc.rs`).
const TRIGGER_SCENES: &[(&str, i16)] = &[
    ("town01", 1),
    ("garmel", 2),
    ("deroa", 3),
    ("chitei2", 3),
    ("dohaty", 4),
    ("town0d", 6),
    ("uru", 7),
    ("jouine", 8),
];

/// The trigger record's bytes sliced at its `4C E2` op (7 encoded bytes:
/// `[4C][E2][lo][hi][3 trailing]`), straight from the scene MAN.
fn trigger_op_bytes(index: &Arc<ProtIndex>, scene_name: &str) -> Option<(Vec<u8>, i16)> {
    let scene = Scene::load(index, scene_name).ok()?;
    let man = scene.field_man_payload(index).ok().flatten()?;
    let mf = legaia_asset::man_section::parse(&man).ok()?;
    let t = *scene_fmv_triggers(&mf, &man).first()?;
    let n1 = mf.header.partition_counts[1].max(0) as usize;
    let mut starts: Vec<usize> = (0..n1)
        .filter_map(|i| mf.actor_placement_record_offset(i, man.len()))
        .collect();
    starts.sort_unstable();
    let start = *starts.get(t.record)?;
    let end = starts.get(t.record + 1).copied().unwrap_or(man.len());
    let op = man.get(start + t.pc..(start + t.pc + 7).min(end))?.to_vec();
    Some((op, t.fmv_id))
}

/// Rung 1 (x8): each trigger scene's own `4C E2` op, executed by the session
/// field VM inside the entered scene, flips the world into the cutscene mode
/// for the pinned movie, and the post-play hand-off resolves where retail's
/// dispatch says that movie lands.
#[test]
fn every_scene_fmv_trigger_reaches_its_retail_handoff() {
    let Some(extracted) = gate() else { return };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"));
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    // fmv_id -> the hand-off retail's dispatch encodes (str-fmv-table.md).
    let expected_handoff = |id: i16| -> FmvHandoff {
        match id {
            1 => FmvHandoff::Field {
                scene: "town0b",
                door: 0x00C,
            },
            2 => FmvHandoff::Field {
                scene: "map01",
                door: 0x055,
            },
            3 => FmvHandoff::Field {
                scene: "chitei2",
                door: 0x2C1,
            },
            4 => FmvHandoff::Field {
                scene: "map02",
                door: 0x0F4,
            },
            6 => FmvHandoff::Field {
                scene: "jou",
                door: 0x276,
            },
            7 => FmvHandoff::Field {
                scene: "uru2",
                door: 0x1BC,
            },
            8 => FmvHandoff::Field {
                scene: "town0e",
                door: 0x2E5,
            },
            other => panic!("no retail scene fires fmv_id {other}"),
        }
    };

    let mut fired = 0usize;
    for &(scene_name, want_id) in TRIGGER_SCENES {
        let Some((op, fmv_id)) = trigger_op_bytes(&index, scene_name) else {
            panic!("[{scene_name}] trigger record not found in the MAN");
        };
        assert_eq!(
            fmv_id, want_id,
            "[{scene_name}] literal fmv_id operand drifted"
        );
        host.enter_field_scene(scene_name, 0)
            .unwrap_or_else(|e| panic!("[{scene_name}] scene entry failed: {e:#}"));

        // The record's own op through the session's field-VM channel.
        host.world.load_field_script(op);
        host.world.set_pad(0);
        let _ = host.world.tick(); // op fires -> pending trigger
        assert_eq!(
            host.world.pending_fmv_trigger,
            Some(want_id),
            "[{scene_name}] the disc op did not record its trigger"
        );
        host.world.set_pad(0);
        let _ = host.world.tick(); // pending consumed -> cutscene mode
        assert_eq!(
            host.world.mode,
            SceneMode::Cutscene,
            "[{scene_name}] trigger did not enter the cutscene mode"
        );
        assert_eq!(host.world.active_fmv(), Some(want_id));
        assert!(
            host.world.active_fmv_str_filename().is_some(),
            "[{scene_name}] retail slot resolves an MV*.STR path"
        );

        // Playback completion (the STR chain itself is w1a_fmv_ladder's
        // rung), then the hand-off - the play host's own shape
        // (`commands/run.rs` apply_fmv_handoff).
        host.world.finish_cutscene();
        assert_eq!(host.world.mode, SceneMode::Field);
        let handoff = fmv_post_play_handoff(want_id);
        assert_eq!(
            handoff,
            expected_handoff(want_id),
            "[{scene_name}] hand-off drifted from the dispatch table"
        );
        fired += 1;
        eprintln!("[fmv-handoff] {scene_name}: fmv {want_id} -> {handoff:?}");
    }
    assert_eq!(fired, TRIGGER_SCENES.len());
}

/// Rung 2: one complete Field -> Cutscene -> hand-off-scene transfer. The
/// deroa clip (`fmv_id 3`, MV3 segment 2) hands off to `chitei2` - a field
/// scene the session can enter - so the ladder finishes the transfer the way
/// the play host does and asserts the world stands in the hand-off scene.
#[test]
fn the_deroa_clip_lands_the_party_in_chitei2() {
    let Some(extracted) = gate() else { return };
    let index = Arc::new(ProtIndex::open_extracted(&extracted).expect("open ProtIndex"));
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    let (op, fmv_id) = trigger_op_bytes(&index, "deroa").expect("deroa trigger record");
    assert_eq!(fmv_id, 3);
    host.enter_field_scene("deroa", 0).expect("enter deroa");
    host.world.load_field_script(op);
    host.world.set_pad(0);
    let _ = host.world.tick();
    let _ = host.world.tick();
    assert_eq!(host.world.mode, SceneMode::Cutscene);
    assert_eq!(host.world.active_fmv(), Some(3));

    host.world.finish_cutscene();
    let FmvHandoff::Field { scene, door } = fmv_post_play_handoff(3) else {
        panic!("fmv 3 must hand off to a field scene");
    };
    assert_eq!((scene, door), ("chitei2", 0x2C1));
    host.enter_field_scene(scene, 0)
        .expect("enter the hand-off scene");
    assert_eq!(host.world.mode, SceneMode::Field);
    eprintln!("[fmv-handoff] deroa -> MV3 seg 2 -> {scene} (door {door:#05x})");
}
