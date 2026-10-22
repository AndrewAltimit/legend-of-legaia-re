//! The browser play page's Snowdrift Cave naming prompt stands over Noa's
//! body, not Vahn's.
//!
//! `cave01`'s entry script swaps the party to Noa alone on its first tick
//! (`3D 00 3D 01 3D 02 3C 01`), after the page built the player's rig from the
//! lead it entered with. The page rebuilds the rig when
//! `World::take_player_rig_change` fires, which it now does on a lead change;
//! before, the prompt for Noa's name was drawn over Vahn's mesh. The world
//! half is pinned by `engine-core`'s `cave01_naming_lead_mesh_disc`; this is
//! the page's own rig.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_play::runtime::LegaiaRuntime;
use std::env;

#[test]
fn the_cave01_naming_prompt_stands_over_noas_mesh() {
    let Ok(disc) = env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc image");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load_disc");
    // The story flag the scene before raises for Noa's introduction.
    assert!(rt.set_system_flag(0x156, true));
    rt.enter_field("cave01").expect("enter cave01");
    assert_eq!(
        rt.active_party_json(),
        "[0,1,2]",
        "the picker party, Vahn leading"
    );
    let vahn_mesh = rt.player_mesh_indices();
    assert!(!vahn_mesh.is_empty());
    let mut ticks = 0;
    while !rt.name_entry_is_active() && ticks < 2000 {
        rt.tick_frame().expect("tick");
        ticks += 1;
    }
    assert!(rt.name_entry_is_active(), "name entry never opened");
    let state: serde_json::Value =
        serde_json::from_str(&rt.name_entry_state_json()).expect("name entry json");
    assert_eq!(state["default"], "Noa", "the prompt names Noa");
    assert_eq!(rt.active_party_json(), "[1]");
    let mesh = rt.player_mesh_indices();
    assert!(!mesh.is_empty(), "the page still has a player rig");
    assert_ne!(mesh, vahn_mesh, "the rig was rebuilt off Vahn's mesh");
}

/// The page's own upload follows the rig. It uploads the player's geometry
/// (indices, UVs, texture words) at scene build and only re-poses positions
/// per frame, so a mid-scene rig rebuild must reach it through
/// `FrameTicksReport::player_rig_rebuilt`. Driven one sim tick per display
/// frame - the GPU page's cadence, under which the entry tick's scene
/// rebuild and the party swap land in different frames; at several ticks a
/// frame the rebuild happened to cover the swap, which hid the defect under
/// a slow renderer.
#[test]
fn the_page_reuploads_the_rig_the_cave01_party_swap_rebuilds() {
    let Ok(disc) = env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc image");
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load_disc");
    assert!(rt.set_system_flag(0x156, true));
    rt.enter_field("cave01").expect("enter cave01");
    // What the page holds on the GPU: the scene build's upload.
    let vahn_mesh = rt.player_mesh_indices();
    let mut uploaded = vahn_mesh.clone();
    let mut rebuilt_frames = 0;
    let mut frames = 0;
    while !rt.name_entry_is_active() && frames < 2000 {
        let r = rt.frame_ticks(1, 0, 0).expect("frame");
        // The page's follow-up (`play-view-frame.js`): a scene rebuild, or
        // the rig's own re-upload.
        if !r.scenes.is_empty() || r.player_rig_rebuilt {
            uploaded = rt.player_mesh_indices();
        }
        rebuilt_frames += usize::from(r.player_rig_rebuilt);
        frames += 1;
    }
    assert!(rt.name_entry_is_active(), "name entry never opened");
    assert!(rebuilt_frames > 0, "the party swap reported a rig rebuild");
    let live = rt.player_mesh_indices();
    assert_ne!(live, vahn_mesh, "the engine's rig is Noa's");
    assert_eq!(
        uploaded, live,
        "the geometry the page uploaded is the rig it poses (not Vahn's index buffer)"
    );
}
