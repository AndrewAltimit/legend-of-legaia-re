//! The art and magic pages' full-battle view relaunches its fight on every
//! selection (`window.__playLaunch` -> `startPendingCast`, site/_content/play.html):
//! the page enters `map01` over the running battle, which abandons it
//! (`World::abandon_live_battle`), and forces the next one. The battle track
//! must start again on every relaunch, however many there are.
//!
//! The defect this pins: the abandon dropped the battle swap but left
//! `World::audio.current_bgm` naming the battle track, so the next fight's
//! swap (`World::swap_to_battle_bgm`) read the track as already sounding and
//! only re-attached a score the scene load had stopped - every relaunch after
//! the first fought in silence.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset. CI runs without disc data.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_audio::AudioSink;
use legaia_web_play::runtime::LegaiaRuntime;

/// The play page's cap on the off-screen opening.
const CAP: u32 = 1800;

enum Action {
    Art(&'static str, &'static str),
    Cast(u8),
}

fn disc_runtime() -> Option<LegaiaRuntime> {
    let path = std::env::var_os("LEGAIA_DISC_BIN")?;
    let disc = std::fs::read(path).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(disc, String::new()).ok()?;
    Some(rt)
}

/// `PlayView.enter` + `startPendingCast`, the skip included.
fn launch(rt: &mut LegaiaRuntime, action: &Action) {
    let _ = rt.play_abandon_opening_chain();
    rt.enter_field("map01").expect("enter map01");
    let _ = rt.cheat_apply("level", 99.0, "");
    match action {
        Action::Art(char, arrows) => {
            let reason = rt.set_active_party_spec(char);
            assert!(reason.is_empty(), "party spec {char}: {reason}");
            let _ = rt.cheat_apply("arts", 0.0, "");
            assert!(rt.debug_seed_inflight_art(0, arrows, 3));
        }
        Action::Cast(spell) => {
            let lead: Vec<u8> = serde_json::from_str(&rt.active_party_json()).unwrap_or_default();
            let lead = lead.first().copied().unwrap_or(0);
            let reason = rt.set_active_party_spec(&lead.to_string());
            assert!(reason.is_empty(), "party spec {lead}: {reason}");
            assert!(rt.debug_seed_inflight_cast(0, *spell, 3));
        }
    }
    assert!(rt.debug_seed_formation(0));
    assert!(rt.debug_force_battle(-1), "no formation on map01");
    let ticks = rt.debug_run_to_inflight(CAP);
    assert!(ticks > 0 && !rt.debug_inflight_pending());
}

#[test]
fn every_showcase_relaunch_plays_the_battle_track() {
    let Some(mut rt) = disc_runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let battle = legaia_engine_system::music_labels::BATTLE_THEME_1_BGM_ID;
    let cases = [
        Action::Cast(0x81),
        Action::Cast(0x82),
        Action::Cast(0x9E),
        Action::Art("vahn", "434"),
        Action::Cast(0x83),
        Action::Art("noa", "232"),
        Action::Cast(0x84),
    ];
    for (i, action) in cases.iter().enumerate() {
        launch(&mut rt, action);
        let sink = rt.scene_host.director().expect("director").audio().clone();
        let mut loud = 0.0;
        for _ in 0..240 {
            rt.frame_ticks(1, 0, 0).expect("tick");
            loud += sink.render_video_frame().mean_abs();
        }
        let d = rt.scene_host.director().expect("director");
        let progress = d.audio().sequencer_progress();
        eprintln!(
            "[relaunch {i}] last_started {:?} playing {} progress {progress:?} mean |pcm| {:.1}",
            d.last_started,
            d.is_playing(),
            loud / 240.0
        );
        assert_eq!(
            d.last_started,
            Some(battle),
            "relaunch {i}: the battle track did not start"
        );
        assert!(
            d.is_playing() && progress.is_some_and(|p| p.tick > 0 && !p.finished),
            "relaunch {i}: the battle track is not advancing ({progress:?})"
        );
    }
}
