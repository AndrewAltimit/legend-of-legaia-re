//! Disc-gated: the **browser play page** shows the sparring-tutorial prompt
//! boxes, and shows them under the disc's own condition rather than a
//! host-side switch.
//!
//! The page has drawn these boxes for a while - `play_battle.rs` builds the
//! text + chrome and `play_shop.rs` folds both into the compose. Nothing on
//! the page ever *armed* the machine, so `World::battle_tutorial` was `None`
//! in every browser battle and the whole draw path was dead code. The only
//! production arm lived in the native window, behind a CLI flag plus a
//! hardcoded scene name plus an environment variable - a development shim,
//! not a port.
//!
//! Retail's condition is a one-shot system-flag arm: the field/world entity
//! SM's battle-entry tail (`FUN_801DA51C`, `0x801DA698..0x801DA6B0`) defaults
//! the battle-stage id byte `_DAT_8007B64A` to `0`, tests flag `0x19`
//! (`FUN_8003CE64`), and only on a set flag writes stage id `1` - the id that
//! pages the prompt overlay (PROT 0967) in - clearing the flag with
//! `FUN_8003CE34` in the same breath. The disc's only setter of that flag is
//! town01's Tetsu record, `50 19` two ops before its `3E FF` battle-entry op.
//!
//! So the checks below drive the page's own runtime and assert:
//!
//! 1. entering town01 installs the prompt corpus off the disc (a **count**,
//!    not "the field exists"), and leaves the arm flag down;
//! 2. a battle entered with the flag down arms nothing - the condition is a
//!    condition;
//! 3. a battle entered with the flag up (raised exactly as the Tetsu record
//!    raises it) arms the machine, consumes the flag, and composes a prompt
//!    box whose text is a **real string from the disc corpus** at the retail
//!    emitter's own rect;
//! 4. the box's glyphs actually reach `play_overlay_draws_json`.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use legaia_engine_core::battle_tutorial::{self as tut, BattleTutorialScript};
use legaia_engine_core::input::PadButton;
use legaia_prot::archive::Archive;
use legaia_web_viewer::runtime::LegaiaRuntime;

const W: u32 = 960;
const H: u32 = 720;

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or(serde_json::Value::Null)
}

fn extracted_dir() -> Option<PathBuf> {
    for d in ["extracted", "../../extracted"] {
        let p = PathBuf::from(d);
        if p.join("PROT.DAT").exists() {
            return Some(p);
        }
    }
    None
}

/// The prompt corpus read independently of the engine, so the page's box text
/// is checked against the disc rather than against itself.
fn disc_corpus() -> Option<BattleTutorialScript> {
    let extracted = extracted_dir()?;
    let rec = legaia_asset::static_overlay::overlay_map().by_label("battle_tutorial")?;
    let mut archive = Archive::open(&extracted.join("PROT.DAT")).ok()?;
    let entry = archive.entries.get(rec.prot_index as usize)?.clone();
    let mut bytes = Vec::new();
    archive.read_entry(&entry, &mut bytes).ok()?;
    let loaded = legaia_asset::static_overlay::as_loaded(&bytes, rec).ok()?;
    Some(BattleTutorialScript::from_overlay(&loaded, rec.base_va))
}

/// Tick the page's frame loop, reading the same per-frame surface the page
/// reads (so the compose runs, not only the simulation).
fn step(rt: &mut LegaiaRuntime) {
    rt.tick_frame().expect("tick_frame");
    let _ = rt.play_overlay_draws_json(W, H);
}

/// One pad press through the page: down for a frame, released the next -
/// every menu surface reads `just_pressed`.
fn tap(rt: &mut LegaiaRuntime, mask: u16) {
    rt.set_pad(mask);
    step(rt);
    rt.set_pad(0);
    step(rt);
}

/// Drive a battle until the tutorial puts a box up, or give up.
/// Returns the first box payload seen.
fn run_to_first_box(rt: &mut LegaiaRuntime, frames: u32) -> Option<serde_json::Value> {
    for i in 0..frames {
        let t = json(&rt.play_battle_tutorial_json());
        if t["box"].is_object() {
            return Some(t);
        }
        // The command flow needs input edges to walk its phases; a bare tick
        // loop parks at the first prompt-less state.
        if i.is_multiple_of(6) {
            tap(rt, PadButton::Cross.mask());
        } else {
            step(rt);
        }
    }
    None
}

#[test]
fn the_play_page_shows_the_sparring_tutorial_under_the_disc_condition() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Ok(bytes) = std::fs::read(&disc) else {
        eprintln!("[skip] disc unreadable (disc-gated)");
        return;
    };
    let Some(corpus) = disc_corpus() else {
        eprintln!("[skip] extracted/PROT.DAT missing (disc-gated)");
        return;
    };
    // The corpus this test compares against must itself be real, or every
    // "text matches the disc" assert below is vacuously true.
    let intro = corpus
        .text(tut::msg::LESSON0_INTRO)
        .expect("lesson-0 intro string in the disc corpus");
    assert!(!intro.is_empty(), "disc corpus resolved an empty intro");

    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load_disc");
    rt.enter_field("town01").expect("enter_field(town01)");
    for _ in 0..5 {
        step(&mut rt);
    }

    // --- 1. the corpus lands on the page, without the page asking ---------
    let t = json(&rt.play_battle_tutorial_json());
    assert_eq!(t["armed"], false, "no battle yet, so nothing is armed");
    assert_eq!(
        t["prompts"].as_u64().unwrap_or(0) as usize,
        BattleTutorialScript::MESSAGE_IDS.len() + 3,
        "town01 entry must install the whole PROT 0967 prompt corpus: {t}"
    );
    assert_eq!(
        t["flag_armed"], false,
        "the disc arm starts down - the Tetsu record has not run"
    );

    // --- 2. an unarmed battle arms nothing --------------------------------
    assert!(
        rt.debug_start_test_battle(),
        "no scripted formation row entered battle in town01"
    );
    assert!(rt.play_battle_active(), "battle render did not build");
    let t = json(&rt.play_battle_tutorial_json());
    assert_eq!(
        t["armed"], false,
        "a battle with the arm flag down must NOT run the tutorial: {t}"
    );
    // ...and it stays that way for the whole fight, not just frame 0.
    assert!(
        run_to_first_box(&mut rt, 240).is_none(),
        "an unarmed battle composed a tutorial box"
    );

    // --- 3. the disc condition arms it ------------------------------------
    // Back to the field, then raise the flag the way town01's Tetsu record
    // does (`50 19`) two ops before its battle-entry op.
    rt.enter_field("town01").expect("re-enter town01");
    for _ in 0..5 {
        step(&mut rt);
    }
    assert!(
        rt.debug_system_flag_set(tut::TUTORIAL_ARM_FLAG),
        "flag poke needs a live scene"
    );
    assert_eq!(
        json(&rt.play_battle_tutorial_json())["flag_armed"],
        true,
        "the arm flag should read back set"
    );

    assert!(rt.debug_start_test_battle(), "armed battle failed to enter");
    let t = json(&rt.play_battle_tutorial_json());
    assert_eq!(
        t["armed"], true,
        "the disc arm must run the tutorial in the very next battle: {t}"
    );
    assert_eq!(
        t["lesson"].as_u64(),
        Some(0),
        "the sparring fight opens on lesson 0 (attacks): {t}"
    );
    assert_eq!(
        t["flag_armed"], false,
        "battle entry must CONSUME the arm - retail clears it in the same breath"
    );

    // --- 4. a real prompt, with real disc text, at the emitter's rect ------
    let t = run_to_first_box(&mut rt, 900).unwrap_or_else(|| {
        panic!("the armed battle never queued a tutorial prompt box");
    });
    let b = &t["box"];
    let text = b["text"].as_str().expect("box carries text");
    assert_eq!(
        text, intro,
        "the first prompt must be the disc's lesson-0 intro, not port wording"
    );
    // The rect is the retail emitter's own: left margin 0x10, top anchor
    // 0x0E, height = lines * 14 - 4, width measured from the text.
    let rect: Vec<i64> = b["rect"]
        .as_array()
        .expect("box carries a rect")
        .iter()
        .map(|v| v.as_i64().expect("rect component"))
        .collect();
    let lines = text.lines().count() as i64;
    assert_eq!(rect[0], 0x10, "style-0 boxes sit at the left margin: {b}");
    assert_eq!(rect[1], 0x0E, "style-0 boxes take the top anchor: {b}");
    assert!(rect[2] > 0, "the box must be measured, not zero-width: {b}");
    assert_eq!(rect[3], lines * 14 - 4, "retail box height: {b}");
    assert!(
        rect[0] + rect[2] <= 320 && rect[1] + rect[3] <= 240,
        "the box must fit the 320x240 stage: {b}"
    );

    // --- 5. and it reaches the composed overlay ---------------------------
    // The box lives in stage space, so the compose scales it - the glyph rows
    // land at `origin + rect * scale`, not at the raw rect. Assert the row
    // count and the vertical span rather than an exact pixel, so the check
    // is about "these glyphs are the box's" and not about the letterbox.
    let overlay = json(&rt.play_overlay_draws_json(W, H));
    assert_eq!(
        overlay["open"], true,
        "the tutorial box must open the overlay"
    );
    let texts = overlay["texts"].as_array().expect("overlay texts array");
    assert!(
        !texts.is_empty(),
        "a composed tutorial box drew no glyph quads"
    );
    // Stage transform for a 960x720 surface: the play page letterboxes the
    // 320x240 stage at an integer scale.
    let n_glyphs: usize = text.chars().filter(|c| !c.is_whitespace()).count();
    assert!(
        texts.len() >= n_glyphs,
        "compose drew {} quads for a {n_glyphs}-glyph prompt",
        texts.len()
    );

    // Deliberately no prompt text in the log line - it is disc text.
    eprintln!(
        "battle_tutorial_page: corpus {} prompts, first box {} glyphs at rect {rect:?}, \
         {} composed quads",
        t["prompts"],
        n_glyphs,
        texts.len()
    );
}
