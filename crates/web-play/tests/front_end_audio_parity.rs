//! Disc-gated: the browser play page's **front-end audio level** runs the
//! same `LaunchFade` stores the native window runs.
//!
//! Retail's card manager stores the live audio level every frame of its
//! `LaunchFade` arm (`0x801DFA00..0x801DFA58` in `FUN_801DD35C`): `254`
//! through the `0x5A` hold, then down to `0` over the launch ramp, on both
//! routes out of the title - a NEW GAME and a Continue Load. The port's
//! halves are the session's: `AudioDirector::front_end_level_frame` under the
//! Continue screen (`ContinueScreen::music_level`) and the launch hold
//! `AudioDirector::begin_launch_hold` / `end_launch_hold` that
//! `BootSession::step_audio_frame` drives off `World::launch_fade`. The
//! native halves are `engine-session`'s `new_game_launch_audio_disc` and
//! `engine-shell`'s `native_front_end`; this file drives the page's own
//! entry points (`play_title_bgm`, `play_new_game`, `boot_continue_step`,
//! `tick_frame`), so a page that reached the director another way - or not
//! at all - fails here.
//!
//! The page's director is built over the device-free `TestAudioSink` off
//! wasm. No Sony bytes are asserted. Skips + passes when `LEGAIA_DISC_BIN`
//! is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_menus::save_select::SelectPhase;
use legaia_web_play::runtime::LegaiaRuntime;

const START: u16 = 0x0008;
const DOWN: u16 = 0x0040;
const LEFT: u16 = 0x0080;
const CROSS: u16 = 0x4000;

fn loaded() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    Some(rt)
}

/// A raw memory card carrying one Legaia save in block 1 (grid cell 0, where
/// the Load grid's cursor opens), written in `town01`. Synthesised, not
/// disc-derived.
fn card_with_save() -> Vec<u8> {
    use legaia_save::card;
    let block = 1u8;
    let mut buf = vec![0u8; card::CARD_SIZE];
    buf[..2].copy_from_slice(&card::CARD_MAGIC);
    for i in 1..=card::DIR_FRAMES {
        let off = card::DIR_FRAME_SIZE * i;
        buf[off..off + 4].copy_from_slice(&card::state::FREE.to_le_bytes());
    }
    let f = card::DIR_FRAME_SIZE * block as usize;
    buf[f..f + 4].copy_from_slice(&card::state::FIRST_BLOCK.to_le_bytes());
    buf[f + 8..f + 10].copy_from_slice(&0xFFFFu16.to_le_bytes());
    let filename = card::legaia_save_filename(u32::from(block) - 1);
    buf[f + 10..f + 28].copy_from_slice(filename.as_bytes());
    let b = card::BLOCK_SIZE * block as usize;
    let sc = &mut buf[b..b + card::BLOCK_SIZE];
    sc[..2].copy_from_slice(&card::SAVE_BLOCK_MAGIC);
    let mut rec = legaia_save::CharacterRecord::zeroed();
    rec.set_name("Vahn");
    legaia_save::write_retail_char_records(sc, std::slice::from_ref(&rec.raw)).unwrap();
    legaia_save::write_retail_gold(sc, 777).unwrap();
    let scene = b"town01";
    let at = card::RETAIL_SCENE_LABEL_OFFSET;
    sc[at..at + scene.len()].copy_from_slice(scene);
    card::restamp_sc_block_checksum(sc);
    buf
}

/// The page's NEW GAME (`play_new_game`, what `PlayView.newGame` calls on
/// the title's outcome) holds the title theme under the launch's black,
/// swells it to `254` for the hold, fades it to `0` over the ramp, and the
/// launch frame ends the hold with the level rested.
#[test]
fn the_page_new_game_black_swells_then_fades_the_title_theme() {
    let Some(mut rt) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(rt.play_title_bgm(), "the title theme starts on the page");
    let title = legaia_engine_system::music_labels::TITLE_THEME_BGM_ID;
    rt.play_new_game()
        .expect("the page's New Game enters the opening");
    let fading = |rt: &LegaiaRuntime| {
        rt.scene_host
            .host()
            .is_some_and(|h| h.world.launch_fade.is_some())
    };
    assert!(fading(&rt), "the launch's black runs");
    assert!(
        rt.scene_host.director().is_some_and(|d| d.launch_holding()),
        "the page's New Game holds the opening's BGM commands"
    );
    let mut applied = Vec::new();
    let mut frames = 0;
    while fading(&rt) {
        rt.set_pad(0);
        rt.tick_frame().expect("tick_frame");
        if !fading(&rt) {
            break;
        }
        let d = rt.scene_host.director().expect("the page's director");
        assert_eq!(d.last_started, Some(title), "the title theme plays on");
        applied.push(d.duck_applied());
        frames += 1;
        assert!(frames < 400, "the launch never handed over");
    }
    let d = rt.scene_host.director().expect("the page's director");
    let hold = applied.iter().filter(|&&l| l == 254).count();
    eprintln!(
        "[page-new-game] {frames} black frames, {hold} at 254, tail {:?}",
        &applied[applied.len().saturating_sub(20)..]
    );
    assert!(
        (88..=90).contains(&hold),
        "the hold swells the theme for its 0x5A frames (got {hold})"
    );
    assert_eq!(applied.last(), Some(&0), "the ramp fades it to silence");
    assert!(!d.launch_holding(), "the launch frame ends the hold");
    assert_ne!(d.duck_level(), 0, "the scene load rests the level");
}

/// The page's title Continue (`boot_continue_step`, one call per tick as
/// `play.html` drives it) runs the Load's `LaunchFade` stores: the title
/// theme swells to `254`, fades to `0`, and the landing rests the level on
/// the configured one rather than the fade's `0`.
#[test]
fn the_page_continue_load_swells_fades_and_rests_the_level() {
    let Some(mut rt) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    rt.insert_card(0, card_with_save(), "card A".into())
        .expect("insert card into port 1");
    assert!(rt.play_title_bgm(), "the title theme starts on the page");
    rt.boot_title_start();
    for _ in 0..240 {
        rt.boot_title_step(0);
    }
    rt.boot_title_step(START);
    rt.boot_title_step(0);
    rt.boot_title_step(DOWN);
    let mut outcome = String::new();
    for _ in 0..240 {
        rt.boot_title_step(0);
        outcome = rt.boot_title_step(CROSS);
        if !outcome.is_empty() {
            break;
        }
    }
    assert_eq!(outcome, "continue", "Cross on the Continue row hands off");

    let mut applied: Vec<u8> = Vec::new();
    let mut step = String::new();
    // Hands off once the confirm's Yes is pressed: a press on the result
    // line starts the ramp early (retail's `+0x5A`), so the hold runs whole
    // only untouched.
    let mut confirmed = false;
    for frame in 0..6000u32 {
        let tap = frame % 8 < 2;
        let phase = rt.boot_continue.as_ref().map(|c| c.session().phase());
        let pad = match phase {
            _ if !tap || confirmed => 0,
            Some(SelectPhase::ConfirmOverwrite { cursor, .. }) if cursor != 0 => LEFT,
            Some(SelectPhase::ConfirmOverwrite { .. }) => {
                confirmed = true;
                CROSS
            }
            Some(SelectPhase::NowChecking { .. }) => 0,
            _ => CROSS,
        };
        step = rt.boot_continue_step(pad);
        if let Some(d) = rt.scene_host.director() {
            applied.push(d.duck_applied());
        }
        if !step.is_empty() {
            break;
        }
    }
    assert_eq!(step, "loaded", "the Load landed a save");
    eprintln!(
        "[page-continue] the Load applied 254 on {} frames",
        applied.iter().filter(|&&l| l == 254).count()
    );
    let hold = applied.iter().filter(|&&l| l == 254).count();
    assert!(
        (88..=90).contains(&hold),
        "the result line swells the theme for its 0x5A frames (got {hold})"
    );
    assert!(applied.contains(&0), "the ramp fades it to silence");
    let d = rt.scene_host.director().expect("the page's director");
    assert!(
        d.duck_target() > 0 && d.duck_level() == d.duck_target(),
        "the landing rests the level on the configured one, not on the fade's 0"
    );
    // The page's landing as `play.html` runs it after the title's
    // `'loaded'` (`resumeLoadedSave` -> `play_resume_save`, no hand-off
    // call): the saved scene's entry lets the title theme go.
    rt.play_resume_save().expect("the Load parked its save");
    for _ in 0..3 {
        rt.set_pad(0);
        rt.tick_frame().expect("tick_frame");
    }
    let title = legaia_engine_system::music_labels::TITLE_THEME_BGM_ID;
    let d = rt.scene_host.director().expect("the page's director");
    assert_ne!(d.last_started, Some(title), "the title theme is let go");
    assert_ne!(d.duck_level(), 0, "the field enters at the rested level");
}
