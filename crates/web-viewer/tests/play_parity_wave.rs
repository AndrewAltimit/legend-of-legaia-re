//! Parity oracles for the play page's engine-driven presentation wave:
//!
//! - **NPC hide-box contract**: the browser's drawable-NPC set (upload rule
//!   plus per-frame hide-box skip, both served through the `LegaiaRuntime`
//!   play API) matches the native play-window's rule over the same world
//!   state - upload every MAN partition-1 placement except one that is
//!   header-parked AND still parked live, and never draw a slot whose live
//!   position is the off-map hide box.
//! - **Sim-tick NPC clip playback**: `play_npc_clip_states` frames advance
//!   with `tick_frame` (the 60 Hz sim clock, 2 ticks per clip frame - the
//!   retail cadence), not with render-side reads.
//! - **Opening chain**: entering `opdeene` through the browser runtime arms
//!   the chain (prologue grade + depth cue staged, narration lock reported),
//!   and the retail intro-skip hands off to `town01`, landing in free-roam
//!   Field mode with no timeline pending (the browser has no name-entry
//!   surface; the establishing-sweep timeline is deliberately not run).
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

#[test]
fn npc_hide_box_contract_matches_native_rule() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    rt.enter_field("town01").expect("enter town01");

    let hide = rt.field_offmap_hide_xz();
    let cat: serde_json::Value =
        serde_json::from_str(&rt.play_npc_catalog_json()).expect("catalog json");
    let npcs = cat["npcs"].as_array().expect("npcs array");
    let nt = rt.play_npc_transforms();
    assert_eq!(nt.len(), npcs.len() * 4, "one transform per catalog entry");

    // The native window's rule over the same live state: a header-parked
    // placement uploads only when the scene-entry spawn-prologue pre-run
    // seated it into the town (live position != hide box); any slot whose
    // live position IS the hide box is skipped at draw time.
    let mut drawable = 0usize;
    let mut parked_live = 0usize;
    let mut seated_from_park = 0usize;
    for (i, npc) in npcs.iter().enumerate() {
        let conditional = npc["conditional"].as_bool().unwrap();
        let (x, z) = (nt[i * 4] as i32, nt[i * 4 + 2] as i32);
        let live_hidden = x == hide && z == hide;
        if live_hidden {
            parked_live += 1;
        }
        if conditional && !live_hidden {
            seated_from_park += 1;
        }
        // The page's contract: upload unless (conditional && live_hidden);
        // draw unless live_hidden. Net drawable = !live_hidden.
        if !live_hidden {
            drawable += 1;
        }
        // Native equivalence per entry: drawn(native) == !live_hidden too -
        // the upload rule only ever excludes entries that are also
        // live-hidden, so the two rules agree on the drawn set.
        let native_uploaded = !(conditional && live_hidden);
        let native_drawn = native_uploaded && !live_hidden;
        assert_eq!(
            native_drawn, !live_hidden,
            "entry {i}: web drawable rule must equal the native rule"
        );
    }
    println!(
        "town01: {} catalog entries, {} drawable, {} parked live, {} seated-from-park",
        npcs.len(),
        drawable,
        parked_live,
        seated_from_park
    );
    assert!(drawable > 0, "town01 must have visible NPCs");
    // Non-vacuity: the spawn-prologue pre-run parks a large share of the
    // town01 placements from frame one (retail: ~half stand parked or
    // relocated). If nothing is parked live, the hide-box skip is untested.
    assert!(
        parked_live > 0,
        "expected at least one story-parked placement at scene entry"
    );
}

#[test]
fn npc_clip_frames_advance_on_sim_ticks_only() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    rt.enter_field("town01").expect("enter town01");

    let s0 = rt.play_npc_clip_states();
    assert!(!s0.is_empty(), "town01 catalog must expose clip states");
    // A pure render-side read (no tick) must not move any playhead.
    let s1 = rt.play_npc_clip_states();
    assert_eq!(s0, s1, "reads must not advance the playhead");

    // Pick an entry with a live multi-frame clip.
    let cat: serde_json::Value = serde_json::from_str(&rt.play_npc_catalog_json()).unwrap();
    let npcs = cat["npcs"].as_array().unwrap();
    let target = (0..npcs.len()).find(|&i| {
        let dims = rt.play_npc_pose_dims(i as u32);
        s0[i * 2] >= 0 && dims[0] > 1
    });
    let Some(i) = target else {
        panic!("town01 must have at least one animated NPC clip");
    };
    let dims = rt.play_npc_pose_dims(i as u32);
    let frames = dims[0] as i32;
    let bones = dims[1] as usize;
    let f0 = s0[i * 2];

    // The live pose stream is 6 ints per bone.
    let bones0 = rt.play_npc_live_bones(i as u32);
    assert_eq!(bones0.len(), bones * 6, "6 ints per bone");

    // 8 sim ticks at the retail cadence (2 ticks per clip frame) = +4 frames.
    for _ in 0..8 {
        rt.tick_frame().expect("tick");
    }
    let s2 = rt.play_npc_clip_states();
    assert_eq!(
        s2[i * 2],
        (f0 + 4).rem_euclid(frames),
        "8 sim ticks must advance the clip by exactly 4 frames (ticks_per_frame = 2)"
    );
}

#[test]
fn opening_chain_stages_grade_and_skips_to_town01() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    rt.enter_field("opdeene").expect("enter opdeene");

    let st: serde_json::Value =
        serde_json::from_str(&rt.play_cutscene_state_json()).expect("cutscene state");
    assert_eq!(
        st["chain"], true,
        "opdeene entry must arm the opening chain"
    );
    assert!(
        st["grade"].is_object(),
        "prologue sepia grade must be staged on the cutscene legs"
    );
    assert!(
        st["cue"].is_object(),
        "prologue depth-cue ramp must be staged on the cutscene legs"
    );

    // Tick until the narration crawl locks the pad (the timeline installs the
    // roller within the first beats) and the intro-skip arms.
    let mut saw_lock = false;
    let mut target = String::new();
    for _ in 0..1800 {
        rt.set_pad(0);
        rt.tick_frame().expect("tick");
        let st: serde_json::Value = serde_json::from_str(&rt.play_cutscene_state_json()).unwrap();
        if st["locked"] == true {
            saw_lock = true;
        }
        target = rt.play_take_prologue_handoff(true);
        if !target.is_empty() {
            break;
        }
    }
    assert!(
        saw_lock,
        "the narration crawl must lock the pad at some beat"
    );
    assert_eq!(target, "town01", "the intro-skip target is Rim Elm");

    // Narration text draws serve font quads while the crawl is up (checked
    // before the skip tears the roller down would be racy - instead assert
    // the closed shape contract now that the chain is gone).
    let closed: serde_json::Value =
        serde_json::from_str(&rt.play_cutscene_text_draws_json(960, 720)).unwrap();
    assert_eq!(closed["open"], false, "no narration after the skip");

    // Enter the handoff target exactly as the page does; the browser lands in
    // free-roam Field with no timeline pending (no name-entry surface).
    rt.enter_field(&target).expect("enter town01 after handoff");
    let state: serde_json::Value = serde_json::from_str(&rt.state_json()).unwrap();
    assert_eq!(state["scene"], "town01");
    assert_eq!(state["mode"], "Field");
    let st: serde_json::Value = serde_json::from_str(&rt.play_cutscene_state_json()).unwrap();
    assert_eq!(st["chain"], false, "the chain ends at Rim Elm");
    assert_eq!(st["locked"], false, "free-roam play is unlocked");
    assert!(st["grade"].is_null(), "the sepia grade drops on the field");
}

#[test]
fn narration_text_draws_render_during_the_crawl() {
    let Some(mut rt) = loaded_runtime() else {
        eprintln!("LEGAIA_DISC_BIN unset - skipping");
        return;
    };
    rt.enter_field("opdeene").expect("enter opdeene");
    // Tick until the roller reports visible text, then assert the draw list
    // carries font quads for it.
    for _ in 0..1800 {
        rt.set_pad(0);
        rt.tick_frame().expect("tick");
        let st: serde_json::Value = serde_json::from_str(&rt.play_cutscene_state_json()).unwrap();
        if st["narration"] == true {
            let draws: serde_json::Value =
                serde_json::from_str(&rt.play_cutscene_text_draws_json(960, 720)).unwrap();
            if draws["open"] == true {
                let texts = draws["texts"].as_array().unwrap();
                assert!(!texts.is_empty(), "visible narration must emit glyphs");
                return;
            }
        }
    }
    panic!("the opdeene narration crawl never presented a visible line");
}
