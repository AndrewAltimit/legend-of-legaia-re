//! A prop record run does not survive a scene change.
//!
//! The touched object is an actor of the departing scene's list, and the
//! dialog SM context stepping its record (`FUN_80039B7C` on the actor
//! `FUN_801D5B5C` engaged) goes with that list when the next scene loads.
//! The engine keeps the run on `World::dialog.inline`; carried across, it
//! parks on the record's `2D 08` end-latch spin, which no actor of the new
//! scene answers, and the pad stays with it for good. The soak met it as a
//! Door of Wind lift landing in `map01` while a `chitei2` cupboard stood open
//! (`softlock|map01|dialogue`).
//!
//! Disc-gated: skips and passes without `LEGAIA_DISC_BIN`.

use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn disc_path() -> Option<PathBuf> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    path.is_file().then_some(path)
}

/// The searchable cupboard's anim id (its bind names clip 2).
const CUPBOARD_ANIM: u8 = 2;

/// The player's movement-disabled bit (`+0x10 & 0x80000`).
const ENGAGED: u32 = 0x0008_0000;

#[test]
fn an_open_cupboard_run_is_dropped_by_the_next_scene_load() {
    let Some(path) = disc_path() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or not a file");
        return;
    };
    let mut host = SceneHost::open_disc(&path).expect("open disc");
    host.enter_field_scene("town01", 0).expect("enter town01");

    // Open one of Rim Elm's cupboards (the bind naming clip 2) and step its
    // record into the door swing.
    let mut anchors: Vec<(u8, u8)> = host
        .world
        .props
        .bank
        .props
        .iter()
        .filter(|(_, p)| p.anim.anim_id == CUPBOARD_ANIM)
        .map(|(&a, _)| a)
        .collect();
    anchors.sort_unstable();
    let started = anchors
        .into_iter()
        .find(|&a| host.world.start_prop_interaction(a))
        .expect("town01 carries a cupboard whose record starts");
    let _ = host.world.tick();
    assert!(
        host.world
            .dialog
            .inline
            .as_ref()
            .is_some_and(|id| id.prop_anchor == Some(started)),
        "the prop run is in flight before the scene change"
    );

    host.enter_field_scene("town0b", 0).expect("enter town0b");
    assert!(
        host.world.dialog.inline.is_none(),
        "the departing scene's prop run went with its actor list"
    );
    assert!(
        !host.world.dialogue_owns_input(),
        "nothing holds the pad in the new scene"
    );
    let slot = host.world.player_actor_slot.expect("a player is seated") as usize;
    assert_eq!(
        host.world.actors[slot].move_state.flags & ENGAGED,
        0,
        "the engaged bit the touch raised is released"
    );
}
