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

use legaia_web_play::runtime::LegaiaRuntime;
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

/// A VDF morph the page holds back while nothing can see it lands on the mesh
/// it would have drawn all along, and the reach the page judges "nothing can
/// see it" by bounds how far the morph really moved. `jouina` runs its env
/// morphs every tick.
#[test]
fn a_held_morph_builds_the_mesh_the_page_would_have_drawn() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    // The page's every-frame path, keeping the last positions per slot, and the
    // reach of the state they came from.
    let run = |rt: &mut LegaiaRuntime, build_each_frame: bool| {
        rt.enter_field("jouina").expect("enter jouina");
        let mut held = std::collections::BTreeSet::new();
        let mut last = std::collections::BTreeMap::new();
        for _ in 0..240 {
            rt.set_pad(0);
            rt.tick_frame().expect("tick");
            for slot in rt.field_morph_slots() {
                held.insert(slot);
                // Holding reads the deltas whether or not the mesh is built.
                let reach = rt.field_morph_reach(slot);
                if build_each_frame {
                    last.insert(slot, (rt.field_morph_positions(slot), reach));
                }
            }
        }
        for &slot in &held {
            if !build_each_frame {
                let reach = rt.field_morph_reach(slot);
                last.insert(slot, (rt.field_morph_positions(slot), reach));
            }
        }
        last
    };
    let eager = run(&mut rt, true);
    let late = run(&mut rt, false);
    assert!(!eager.is_empty(), "jouina's env morphs must run");
    assert_eq!(
        eager.keys().collect::<Vec<_>>(),
        late.keys().collect::<Vec<_>>()
    );
    for (slot, (pos, reach)) in &eager {
        let (held_pos, held_reach) = &late[slot];
        assert_eq!(pos, held_pos, "slot {slot}: the held build differs");
        assert_eq!(reach, held_reach, "slot {slot}: the held reach differs");
    }
    // The bound itself: every coordinate of the built mesh stays within the
    // reach of its rest place, past the coplanar resolve's half-unit nudges.
    rt.enter_field("jouina").expect("enter jouina");
    let mut compared = 0;
    for (slot, (pos, reach)) in &eager {
        rt.field_mesh(*slot).expect("the slot builds");
        let rest = rt.field_mesh_positions();
        assert_eq!(
            rest.len(),
            pos.len(),
            "slot {slot}: the stream changed size"
        );
        let moved = pos
            .iter()
            .zip(&rest)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(
            moved <= reach + 8.0,
            "slot {slot}: moved {moved}, reach {reach}"
        );
        compared += usize::from(moved > 0.0);
    }
    assert!(compared > 0, "no slot moved: the bound was not tested");
}
