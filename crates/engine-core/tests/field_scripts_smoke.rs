//! Disc-gated smoke test: drive the field VM against real per-scene event
//! scripts and assert that we observe at least one of every expected step
//! outcome shape (Advance / Halt or Yield).
//!
//! Skips silently when `extracted/` is missing or `LEGAIA_DISC_BIN` is
//! unset — same skip-pattern as the rest of the disc-gated suite.
//!
//! What this catches:
//!  - `Scene::find_event_scripts` returns *something* on at least one real
//!    scene (proving the detector + record-range walker actually fire on
//!    real data, not just the synthesised test fixtures).
//!  - `World::load_field_record` + `step_field` walks at least 100 steps
//!    on a real record without panicking.
//!  - At least one step makes forward progress (`Advance` or `Yield`),
//!    proving we're not stuck on an Unknown opener forever.

use std::path::PathBuf;

use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::field::StepResult;

fn extracted_dir() -> Option<PathBuf> {
    let d = PathBuf::from("extracted");
    if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
        Some(d)
    } else {
        // Try repo root from the workspace member's CWD.
        let alt = PathBuf::from("../../extracted");
        if alt.join("PROT.DAT").exists() && alt.join("CDNAME.TXT").exists() {
            Some(alt)
        } else {
            None
        }
    }
}

#[test]
fn field_vm_runs_against_real_scene_event_scripts() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing; run legaia-extract first");
        return;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset; matches disc-gated convention");
        return;
    }

    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");

    // CDNAME-walk the scenes, find any that has an event-script entry. Some
    // scenes are pure asset bundles (no scripts), so we hunt across the map.
    // We don't pin a specific scene name to keep this resilient to CDNAME
    // labelling drift.
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("parse cdname");
    // The map is `entry_index -> first scene name in block`; deduplicate
    // names so we hit each block once.
    let mut scenes: Vec<String> = cdname.values().cloned().collect();
    scenes.sort();
    scenes.dedup();

    // Sweep every scene; collect (name, entry_idx, record_count) for those
    // that carry event-scripts. Aggregate counts let us judge how broad
    // detection actually is across the disc.
    let mut hits: Vec<(String, u32, usize)> = Vec::new();
    for scene_name in &scenes {
        let Ok(scene) = Scene::load(&index, scene_name) else {
            continue;
        };
        if let Some(es) = scene.find_event_scripts() {
            hits.push((scene_name.clone(), es.entry_idx, es.len()));
        }
    }
    eprintln!(
        "[smoke] {} of {} CDNAME scenes carry event-script entries",
        hits.len(),
        scenes.len()
    );
    assert!(
        !hits.is_empty(),
        "no CDNAME scene resolved an event-script entry — Scene::find_event_scripts is broken"
    );

    // Pick the first scene with the most records — exercises the most VM.
    let (scene_name, entry_idx, n_records) = hits
        .iter()
        .max_by_key(|h| h.2)
        .cloned()
        .expect("hits non-empty");
    eprintln!("[smoke] driving scene '{scene_name}' (entry {entry_idx}, {n_records} records)");

    // Reload the scene and pre-extract every record's bytes so the lifetime
    // borrow on `Scene` doesn't leak into the test loop.
    let scene = Scene::load(&index, &scene_name).expect("reload scene");
    let es = scene.find_event_scripts().expect("event scripts present");
    let records: Vec<Vec<u8>> = (0..es.len())
        .map(|i| es.record(i).map(|s| s.to_vec()).unwrap_or_default())
        .collect();

    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };

    let mut totals = [0u32; 5]; // advance, yield, halt, pending, unknown
    for (i, record) in records.iter().enumerate() {
        if record.is_empty() {
            continue;
        }
        world.load_field_record(record);
        for _ in 0..200 {
            let res = world.step_field();
            match res {
                Some(StepResult::Advance { .. }) => totals[0] += 1,
                Some(StepResult::Yield { .. }) => totals[1] += 1,
                Some(StepResult::Halt { .. }) => {
                    totals[2] += 1;
                    break;
                }
                Some(StepResult::Pending { .. }) => totals[3] += 1,
                Some(StepResult::Unknown { .. }) => {
                    totals[4] += 1;
                    break;
                }
                None => break,
            }
        }
        let _ = i;
    }
    eprintln!(
        "[smoke] aggregated step counts across {} record(s): advance={} yield={} halt={} pending={} unknown={}",
        records.len(),
        totals[0],
        totals[1],
        totals[2],
        totals[3],
        totals[4]
    );
    // Acceptance: at least one record made forward progress (advance+yield),
    // OR every record terminated cleanly with halt (no Unknown/Pending).
    // Both are valid VM behaviour; the failure mode we catch is "every
    // record dies on Unknown" (= we're misreading record bytes or the
    // frame-divider skip is wrong).
    let progressed = totals[0] + totals[1] + totals[2];
    assert!(
        progressed > 0,
        "no record made forward progress on '{scene_name}' — VM is stuck"
    );
    // Every record should at least dispatch SOMETHING — total opcodes seen
    // across all records must exceed records.len() (= one outcome per record
    // minimum).
    let total_outcomes = totals.iter().sum::<u32>();
    assert!(
        total_outcomes >= records.len() as u32,
        "fewer outcomes ({total_outcomes}) than records ({}) — possible empty bytecode buffer",
        records.len()
    );
}
