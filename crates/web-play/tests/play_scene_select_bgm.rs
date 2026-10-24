//! Disc-gated: every scene the play page's picker lists starts music, picked
//! one after another on one runtime the way a visitor clicks through the
//! list.
//!
//! The page reads `state_json().bgm`: `requested` (the track the field VM
//! last selected), `playing` (the track the director last started) and
//! `sounding` (its sequencer is running). After a picked scene's entry has
//! run, the director must have started a track for this entry and be
//! sounding it, and when the scene selected a track that is the one playing.
//!
//! The scenes this caught were the ones whose entry script **parks** the BGM
//! slot (op `0x35` with `0x1000`, which starts nothing and keeps the previous
//! scene's track): `bylon`, `station` and `retockin` came up silent on a
//! first pick and kept the previously picked scene's music on a later one.
//! A picker entry now plays the track its door's scene brings
//! (`FieldVmState::picker_bgm_inherit`). `son` is the shape that must stay
//! as it is: it starts its own track, then parks over it.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_play::runtime::LegaiaRuntime;

/// The park sentinel's id.
const PARK: u64 = 0x1000;

/// Frames a picked scene's entry gets before it is read.
const ENTRY_FRAMES: u32 = 120;

/// The picker's scenes: `data/gamedata/scenes.toml`'s towns, field areas and
/// overworlds (the page's `GROUPS`), in table order.
fn picker_scenes() -> Vec<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/gamedata/scenes.toml"
    );
    let text = std::fs::read_to_string(path).expect("scenes.toml");
    let mut out = Vec::new();
    let mut id: Option<String> = None;
    for line in text.lines().map(str::trim) {
        if line == "[[scene]]" {
            id = None;
        } else if let Some(v) = line.strip_prefix("id = ") {
            id = Some(v.trim_matches('"').to_string());
        } else if let Some(v) = line.strip_prefix("category = ")
            && matches!(v.trim_matches('"'), "town" | "field" | "world_map")
            && let Some(i) = id.take()
        {
            out.push(i);
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bgm {
    requested: Option<u64>,
    playing: Option<u64>,
    sounding: bool,
}

fn bgm(rt: &LegaiaRuntime) -> Bgm {
    let v: serde_json::Value = serde_json::from_str(&rt.state_json()).unwrap();
    Bgm {
        requested: v["bgm"]["requested"].as_u64(),
        playing: v["bgm"]["playing"].as_u64(),
        sounding: v["bgm"]["sounding"].as_bool().unwrap_or(false),
    }
}

fn runtime() -> Option<LegaiaRuntime> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(std::fs::read(disc).ok()?, String::new())
        .ok()?;
    Some(rt)
}

fn pick(rt: &mut LegaiaRuntime, scene: &str) -> Option<Bgm> {
    rt.enter_field(scene).ok()?;
    for _ in 0..ENTRY_FRAMES {
        let _ = rt.tick_frame();
    }
    Some(bgm(rt))
}

fn starts_its_music(b: Bgm) -> bool {
    b.playing.is_some() && b.sounding && (b.requested == b.playing || b.requested == Some(PARK))
}

#[test]
fn every_picked_scene_starts_music_in_sequence() {
    let Some(mut rt) = runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let scenes = picker_scenes();
    assert!(scenes.len() > 50, "the picker lists the disc's scenes");
    let mut silent = Vec::new();
    let mut picked = 0;
    for scene in &scenes {
        let Some(b) = pick(&mut rt, scene) else {
            continue;
        };
        picked += 1;
        if !starts_its_music(b) {
            silent.push((scene.clone(), b));
        }
    }
    eprintln!("[ok] {picked} scenes picked in sequence");
    assert!(picked > 50);
    assert!(silent.is_empty(), "picked scenes without music: {silent:?}");
}

/// A parking scene picked first on a fresh page: nothing was sounding before
/// it, so only the inherited track can make it sound.
#[test]
fn a_parking_scene_picked_first_sounds() {
    for scene in ["bylon", "station", "retockin"] {
        let Some(mut rt) = runtime() else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
            return;
        };
        let b = pick(&mut rt, scene).expect("enter");
        eprintln!("[ok] {scene}: {b:?}");
        assert!(starts_its_music(b), "{scene}: {b:?}");
        assert_ne!(b.requested, Some(PARK), "{scene}: the park was replaced");
    }
}
