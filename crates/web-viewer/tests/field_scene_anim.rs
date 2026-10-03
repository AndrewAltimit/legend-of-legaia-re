//! Disc-gated: the browser field-scene animation runner
//! (`field_scene::build_field_scene_anim` + `build_field_scene_live` /
//! `tick_field_scene_vsync`) animates end-to-end against the real disc,
//! exactly as the site page drives it (init after assembly, one vsync per
//! elapsed retail vsync, VRAM re-upload on change), and animates what the
//! play page animates.
//!
//!  - `jou`: no walker table, but the live scene's ambient move-VM tree (the
//!    pulsating-flesh palette cyclers) rewrites VRAM texels.
//!  - `garmel`: a 1-entry walker table (water shimmer) whose `MoveImage`
//!    fires change the dest CLUT cell.
//!
//! Skipped (passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::scene::ProtIndex;
use legaia_web_viewer::disc::{extract_cdname_txt, extract_prot_dat};
use legaia_web_viewer::field_scene::{
    build_field_scene, build_field_scene_anim, build_field_scene_live, tick_field_scene_vsync,
};
use std::env;
use std::fs;
use std::sync::Arc;

fn index() -> Option<ProtIndex> {
    let disc_path = env::var_os("LEGAIA_DISC_BIN")?;
    let disc = fs::read(&disc_path).expect("disc image");
    let prot = extract_prot_dat(&disc).expect("PROT.DAT extraction");
    let cdname = extract_cdname_txt(&disc).expect("CDNAME.TXT extraction");
    Some(ProtIndex::from_bytes(prot, Some(&cdname)).expect("ProtIndex"))
}

#[test]
fn jou_and_garmel_animate_in_the_viewer_or_skip() {
    let Some(index) = index() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let index = Arc::new(index);

    // jou: the live scene's ambient move-VM tree only.
    let mut pack = build_field_scene(&index, "jou").expect("build jou");
    pack.anim = build_field_scene_anim(&index, &mut pack);
    assert!(pack.anim.is_none(), "jou has no walker table");
    build_field_scene_live(index.clone(), &mut pack);
    let ambient = pack
        .live
        .as_ref()
        .expect("jou runs live")
        .host
        .world
        .ambient
        .fx
        .len();
    assert!(ambient >= 20, "jou ambient fan-out ({ambient})");
    let before = pack.res.vram.as_bytes().to_vec();
    let mut wrote = false;
    for _ in 0..16 {
        wrote |= tick_field_scene_vsync(&mut pack);
    }
    assert!(wrote, "jou ambient tick reports VRAM changes");
    assert_ne!(
        before,
        pack.res.vram.as_bytes(),
        "jou VRAM texels actually changed"
    );

    // garmel: 1-entry walker table.
    let mut pack = build_field_scene(&index, "garmel").expect("build garmel");
    pack.anim = build_field_scene_anim(&index, &mut pack);
    let walkers = pack.anim.as_ref().expect("garmel walker").walker_entries();
    assert_eq!(walkers, 1, "garmel walker entries");
    build_field_scene_live(index.clone(), &mut pack);
    let mut wrote = false;
    for _ in 0..32 {
        wrote |= tick_field_scene_vsync(&mut pack);
    }
    assert!(wrote, "garmel walker fires MoveImage copies");
}

/// Host-drift gate for the map viewer: the viewer and the play page run one
/// scene side by side, and every animated quantity the play page reads off
/// its world - the floor-wave offsets of the terrain + placed draws, the
/// ground under the live ladder, each placed prop's pose key, and the field
/// VRAM the CLUT walker / ambient tree / scripted effects write - comes out
/// identical tick for tick. A viewer that bakes any of them (or runs a
/// second animation path) diverges here.
///
/// - `concnow`: the entry script installs a new ladder and keeps it moving.
/// - `jouina`: the travelling ground wave.
/// - `town01`: the windmill's clip.
/// - `jou`: the ambient palette cyclers in VRAM.
#[test]
fn the_map_viewer_animates_what_the_play_page_animates_or_skip() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let bytes = fs::read(&disc_path).expect("disc image");
    let index = Arc::new(index().expect("index"));
    for (scene, ticks) in [
        ("concnow", 240),
        ("jouina", 120),
        ("town01", 100),
        ("jou", 64),
    ] {
        let mut rt = legaia_web_viewer::runtime::LegaiaRuntime::new();
        rt.load_disc(bytes.clone(), String::new())
            .expect("load disc");
        rt.enter_field(scene).expect("enter");

        let mut pack = build_field_scene(&index, scene).expect("build");
        pack.anim = build_field_scene_anim(&index, &mut pack);
        build_field_scene_live(index.clone(), &mut pack);
        assert!(pack.live.is_some(), "{scene}: the viewer runs it live");

        // The two hosts' VRAM differs before any tick (the play page also
        // carries the party / effect pages the map has no use for), so the
        // comparand is what the animation WROTE: the same texels, to the
        // same values.
        let (v0, p0) = (pack.res.vram.as_bytes().to_vec(), rt.field_vram_bytes());
        let mut moved = (false, 0usize, false);
        for t in 0..ticks {
            rt.tick_frame().expect("tick");
            tick_field_scene_vsync(&mut pack);
            let (wv, wp) = (pack.floor_wave_offsets(), rt.field_floor_wave_offsets());
            assert_eq!(wv, wp, "{scene} t{t}: floor-wave offsets");
            moved.0 |= wv.iter().any(|&o| o != 0.0);
            let (gv, gp) = (
                pack.ground_live_positions(),
                rt.field_ground_live_positions(),
            );
            assert_eq!(gv, gp, "{scene} t{t}: ground under the live ladder");
            moved.1 += usize::from(!gv.is_empty());
            let (fv, fp) = (pack.placement_frames(), rt.field_placement_frames());
            assert_eq!(fv, fp, "{scene} t{t}: placed-prop pose keys");
            moved.2 |= fv.iter().any(|&k| k > 0);
        }
        let (v1, p1) = (pack.res.vram.as_bytes(), rt.field_vram_bytes());
        let mut written = 0usize;
        for i in 0..v1.len() {
            let (vw, pw) = (v0[i] != v1[i], p0[i] != p1[i]);
            assert!(
                vw == pw && (!vw || v1[i] == p1[i]),
                "{scene}: VRAM byte {i} (row {}, x {}) after {ticks} ticks: \
                 viewer {:#04x}->{:#04x}, play {:#04x}->{:#04x}",
                i / 2048,
                (i % 2048) / 2,
                v0[i],
                v1[i],
                p0[i],
                p1[i]
            );
            written += usize::from(vw);
        }
        if scene == "jou" {
            assert!(written > 0, "jou's palette cyclers write VRAM");
        }
        eprintln!(
            "[ran] {scene}: wave={} ground={} props={}",
            moved.0, moved.1, moved.2
        );
        match scene {
            "concnow" => assert!(moved.0 && moved.1 > 1, "concnow's ladder moves"),
            "jouina" => assert!(moved.1 > 1, "jouina's ground pulses"),
            "town01" => assert!(moved.2, "town01's windmill turns"),
            _ => {}
        }
    }
}

/// The **play** page's path: `LegaiaRuntime::tick_frame` drains the same two
/// mechanisms against the live scene host's VRAM and raises the dirty flag
/// the page re-uploads on (`field_vram_take_dirty`). Before this wiring the
/// play page uploaded VRAM once at scene entry and jou never pulsed.
#[test]
fn play_runtime_animates_field_vram_or_skip() {
    let Some(disc_path) = env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let bytes = fs::read(&disc_path).expect("disc image");

    // jou: the scene host spawns the ambient tree at entry; ticking the
    // runtime must rewrite VRAM texels and report them dirty.
    let mut rt = legaia_web_viewer::runtime::LegaiaRuntime::new();
    rt.load_disc(bytes.clone(), String::new())
        .expect("load disc");
    rt.enter_field("jou").expect("enter jou");
    let before = rt.field_vram_bytes();
    assert!(!before.is_empty(), "jou scene VRAM present");
    let mut dirty = false;
    for _ in 0..32 {
        rt.tick_frame().expect("tick");
        dirty |= rt.field_vram_take_dirty();
    }
    assert!(dirty, "jou play ticks raise the VRAM dirty flag");
    assert_ne!(
        before,
        rt.field_vram_bytes(),
        "jou play VRAM texels actually changed"
    );

    // garmel: the walker table must be parked + ticked by the runtime too
    // (the ambient tree is jou-specific; shimmer covers the walker family).
    let mut rt = legaia_web_viewer::runtime::LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load disc");
    rt.enter_field("garmel").expect("enter garmel");
    let before = rt.field_vram_bytes();
    let mut dirty = false;
    for _ in 0..64 {
        rt.tick_frame().expect("tick");
        dirty |= rt.field_vram_take_dirty();
    }
    assert!(dirty, "garmel walker fires through the play runtime");
    assert_ne!(
        before,
        rt.field_vram_bytes(),
        "garmel play VRAM texels actually changed"
    );
}
