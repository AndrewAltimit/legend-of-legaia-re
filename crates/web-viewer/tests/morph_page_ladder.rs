//! Ladder for the play page's half of the morph-weight actor: a shipped
//! `4C D8` carrier seats its actor through the scene's own entry script, and
//! the page stages that actor's **blended** mesh through the engine kernel
//! (`World::morph_weight_posed_tmd`) the native redraw poses from too.
//!
//! All three carriers that seat on the disc (`garmel`, `jagaroom`, `juui2`)
//! issue their `4C D8` from `P1[0]`, the scene-entry system script, so an
//! ordinary field entry plus a few ticks reaches the spawner
//! (`FUN_801D77F4`, `World::spawn_morph_weight_actor`) with no fixture: the
//! point of this rung is that the page's own export
//! (`play_dynamic_actor_mesh`) then walks the blend, which no ladder in the
//! union reached before.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset. CI runs without disc
//! data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;
use std::env;

fn loaded_runtime() -> Option<LegaiaRuntime> {
    let disc = env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    Some(rt)
}

/// Enter each carrier, tick until the entry script has seated the morph
/// actor, and stage its blended mesh through the page's export.
#[test]
fn the_entry_script_seats_a_morph_actor_and_the_page_stages_its_blend() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    let mut staged = Vec::new();
    for scene in ["garmel", "jagaroom", "juui2"] {
        if rt.enter_field(scene).is_err() {
            eprintln!("[w6c] {scene}: field entry failed");
            continue;
        }
        let mut slots = Vec::new();
        for _ in 0..240 {
            rt.set_pad(0);
            rt.tick_frame().expect("tick");
            slots = rt.play_morph_weight_slots();
            if !slots.is_empty() {
                break;
            }
        }
        let Some(&slot) = slots.first() else {
            eprintln!("[w6c] {scene}: no morph-weight actor seated in 240 ticks");
            continue;
        };
        // Two stagings a few frames apart: the envelope moves every frame,
        // which is why the page re-stages rather than caching the upload.
        assert!(
            rt.play_dynamic_actor_mesh(slot),
            "{scene}: the seated morph actor stages a drawable mesh"
        );
        for _ in 0..8 {
            rt.set_pad(0);
            rt.tick_frame().expect("tick");
        }
        assert!(
            rt.play_dynamic_actor_mesh(slot),
            "{scene}: still drawable after the ramp moves"
        );
        eprintln!("[w6c] {scene}: morph actor in slot {slot} staged");
        staged.push(scene);
    }
    assert!(
        !staged.is_empty(),
        "at least one shipped 4C D8 carrier must seat and stage a morph actor on the page"
    );
}
