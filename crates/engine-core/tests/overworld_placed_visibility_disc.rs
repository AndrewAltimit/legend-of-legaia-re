//! Story-hidden overworld landmarks: the walk `.MAP`'s placed-flag records
//! spawn through the same `FUN_8003A55C` bind + prologue pre-run as a town's
//! placed objects, so a record whose prologue parks its actor at the off-map
//! hide box draws nothing. map01's record 414 (the dome in the sea south of
//! Rim Elm's gate) and record 349 (the golden bridge's second stamp) both
//! open with `23 7F 7F`; a retail capture at the gate
//! (`overworld_into_town_man_load`) holds both actors at `(0x3FC0, 0x3FC0)`.
//! map02 / map03 park nothing on a cold entry.
//!
//! Disc-gated: skips (and passes) without `LEGAIA_DISC_BIN`.

use legaia_engine_core::field_env;
use legaia_engine_core::scene::SceneHost;

/// `(kingdom, .MAP records the cold entry must drop from the landmark layer)`.
const CASES: &[(&str, &[u16])] = &[("map01", &[414, 349]), ("map02", &[]), ("map03", &[])];

#[test]
fn overworld_landmarks_parked_by_their_prologue_are_not_drawn() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN").ok() else {
        eprintln!("skip: LEGAIA_DISC_BIN unset");
        return;
    };
    let path = std::path::PathBuf::from(disc);
    if !path.exists() {
        eprintln!("skip: LEGAIA_DISC_BIN missing");
        return;
    }
    for &(name, dropped) in CASES {
        let mut host = SceneHost::open_disc(&path).expect("open disc");
        host.enter_field_scene(name, 0).expect("enter kingdom");
        let scene = host.scene.as_ref().expect("scene");
        let all = scene
            .walk_object_placements(&host.index)
            .expect("read")
            .expect("walk .MAP");
        let binds = scene
            .field_object_binds(&host.index)
            .expect("read")
            .expect("kingdom binds");
        // The live world (both play hosts) and the static assembly's scratch
        // world agree on what the prologues hide.
        let live = host.world.hidden_object_records();
        let staged = field_env::story_hidden_records_for_scene(scene, &host.index);
        for hidden in [&live, &staged] {
            let mut kept = all.clone();
            field_env::retain_visible_landmark_placements(&mut kept, &binds, hidden);
            let mut gone: Vec<u16> = all
                .iter()
                .filter(|p| !kept.iter().any(|k| k.col == p.col && k.row == p.row))
                .map(|p| p.obj_idx)
                .collect();
            gone.sort_unstable();
            let mut want = dropped.to_vec();
            want.sort_unstable();
            assert_eq!(
                gone, want,
                "{name}: dropped landmark records (hidden {hidden:?})"
            );
            assert!(
                !kept.is_empty(),
                "{name}: the filter emptied the landmark layer"
            );
        }
        eprintln!("[ran] {name}: {} landmarks, dropped {dropped:?}", all.len());
    }
}
