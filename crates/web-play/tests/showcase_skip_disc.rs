//! The art and magic pages' showcase start (`debug_run_to_inflight`): a
//! seeded art or cast runs its fight's opening off screen and the page's first
//! drawn frame is the caster's turn with the action under way.
//!
//! What it pins:
//!
//! 1. **Same state as the played-out opening.** One runtime skips, a second
//!    ticks the same launch one owner-path tick at a time until the seed
//!    dispatches; both land on the same tick count with the same battle
//!    camera and the same body cursors. The skip draws nothing and sounds
//!    nothing, and changes nothing else.
//! 2. **No opening sound left keyed.** The browser mixes on its own turns,
//!    none of which run inside the skip, so a voice the opening keyed would
//!    sound over the action's first frame: the skip leaves no SFX-range voice
//!    audible that the played-out opening's last tick did not key.
//! 3. **A cast's round is the cast.** The lead casts alone, every monster's
//!    HP is floored as the art seed floors it, and a level-99 spell that
//!    would win the fight leaves it standing at the next prompt instead.
//! 4. **A relaunch mid-summon** does not wait on the abandoned line.
//! 5. **Every character and the casts start.** Each of a sample of arts per
//!    character, Seru casts and a Ra-Seru summon dispatches inside the cap,
//!    from a neutral formation, and its first seconds tick without a trap.
//!
//! The launch is the play page's `startPendingCast` (site/_content/play.html).
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

/// `startPendingCast`, minus the skip.
fn launch(rt: &mut LegaiaRuntime, action: &Action) {
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
            // The lead alone, as the page seats it.
            let lead: Vec<u8> = serde_json::from_str(&rt.active_party_json()).unwrap_or_default();
            let lead = lead.first().copied().unwrap_or(0);
            let reason = rt.set_active_party_spec(&lead.to_string());
            assert!(reason.is_empty(), "party spec {lead}: {reason}");
            assert!(rt.debug_seed_inflight_cast(0, *spell, 3));
        }
    }
    assert!(rt.debug_seed_formation(0));
    assert!(rt.debug_force_battle(-1), "no formation on map01");
}

fn state(rt: &LegaiaRuntime) -> String {
    format!(
        "{} {} {:?}",
        rt.scene_mode(),
        rt.play_battle_camera_json(),
        rt.play_battle_actor_cursor()
    )
}

/// SFX-range voices (`7..=23`) that are keyed and audible.
fn audible_sfx_voices(rt: &LegaiaRuntime) -> Vec<usize> {
    let Some(d) = rt.scene_host.director() else {
        return Vec::new();
    };
    d.audio().with_spu(|spu| {
        (7..=23)
            .filter(|&v| {
                let voice = &spu.voices[v];
                !voice.is_off() && (voice.vol_left != 0 || voice.vol_right != 0)
            })
            .collect()
    })
}

#[test]
fn showcase_skip_lands_on_the_played_out_turn() {
    let (Some(mut skip), Some(mut play)) = (disc_runtime(), disc_runtime()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let action = Action::Art("vahn", "434");
    launch(&mut skip, &action);
    let ticks = skip.debug_run_to_inflight(CAP);
    assert!(ticks > 0 && ticks < CAP, "skip took {ticks} ticks");
    assert!(!skip.debug_inflight_pending());
    assert!(skip.play_battle_active());

    launch(&mut play, &action);
    let mut played = 0;
    let mut keyed_on_last = Vec::new();
    while play.debug_inflight_pending() && played < CAP {
        let before = audible_sfx_voices(&play);
        play.frame_ticks(1, 0, 0).expect("tick");
        played += 1;
        keyed_on_last = audible_sfx_voices(&play)
            .into_iter()
            .filter(|v| !before.contains(v))
            .collect();
    }
    assert_eq!(
        ticks, played,
        "the skip and the played-out opening disagree"
    );
    assert_eq!(state(&skip), state(&play));

    let left = audible_sfx_voices(&skip);
    let stray: Vec<_> = left.iter().filter(|v| !keyed_on_last.contains(v)).collect();
    eprintln!(
        "[showcase] {ticks} ticks; SFX voices audible after the skip {left:?}, \
         after the played-out opening {:?}",
        audible_sfx_voices(&play)
    );
    assert!(
        stray.is_empty(),
        "the skip left opening SFX voices {stray:?} keyed over the action"
    );
}

#[test]
fn showcase_skip_starts_every_character_and_cast() {
    let Some(mut rt) = disc_runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let cases = [
        Action::Art("vahn", "434"),       // Somersault
        Action::Art("vahn", "2231"),      // Fire Blow (Hyper)
        Action::Art("vahn", "231414231"), // Vahn's Craze (Miracle)
        Action::Art("noa", "232"),        // Sonic Javelin
        Action::Art("noa", "1122"),       // Frost Breath (Hyper)
        Action::Art("noa", "224434312"),  // Love You (Super)
        Action::Art("gala", "433"),       // Ironhead
        Action::Art("gala", "22111"),     // Explosive Fist (Hyper)
        Action::Art("gala", "223434311"), // Biron Rage (Miracle)
        Action::Cast(0x81),               // Gimard
        Action::Cast(0x8A),               // a Seru cast
        Action::Cast(0x9E),               // a Ra-Seru summon
    ];
    for action in &cases {
        let label = match action {
            Action::Art(c, a) => format!("{c} {a}"),
            Action::Cast(s) => format!("cast {s:#04x}"),
        };
        launch(&mut rt, action);
        let ticks = rt.debug_run_to_inflight(CAP);
        assert!(
            ticks > 0 && !rt.debug_inflight_pending(),
            "{label}: the seed did not dispatch inside {CAP} ticks ({ticks})"
        );
        assert!(rt.play_battle_active(), "{label}: not in battle");
        for _ in 0..240 {
            rt.frame_ticks(1, 0, 0).expect("tick");
        }
        eprintln!("[showcase] {label}: dispatched after {ticks} ticks");
    }
}

#[test]
fn showcase_cast_round_is_the_cast_and_the_fight_stands() {
    let Some(mut rt) = disc_runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // Gimard kills map01's first formation outright at level 99 without the
    // floor; Theeder hits the whole enemy row.
    for spell in [0x81u8, 0x82] {
        launch(&mut rt, &Action::Cast(spell));
        assert!(rt.debug_run_to_inflight(CAP) > 0);
        let w = &rt.scene_host.host().expect("host").world;
        let party = usize::from(w.party.party_count);
        assert_eq!(party, 1, "cast {spell:#04x}: the lead is not alone");
        let mut prompt = false;
        for _ in 0..3000 {
            rt.frame_ticks(1, 0, 0).expect("tick");
            let w = &rt.scene_host.host().expect("host").world;
            assert!(
                rt.play_battle_active(),
                "cast {spell:#04x}: the fight ended under the showcase"
            );
            if w.battle.flow == legaia_engine_battle_menus::battle_flow::BattleFlowState::TurnPrompt
            {
                prompt = true;
                break;
            }
        }
        assert!(
            prompt,
            "cast {spell:#04x}: the round never came back to the prompt"
        );
        eprintln!("[showcase] cast {spell:#04x}: the fight stands at the next prompt");
    }
}

/// Ozma's line runs on long after the page's next click; the relaunch
/// abandons that fight mid-cast. Its CD-XA drive word used to ride into the
/// next fight and hold that cast in `QueuedFromMenu` until the old line would
/// have ended (about 1270 ticks for a Gimard clicked 1500 ticks into Ozma).
#[test]
fn showcase_relaunch_mid_summon_casts_without_waiting_on_the_old_line() {
    let Some(mut rt) = disc_runtime() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    launch(&mut rt, &Action::Cast(0xA0));
    assert!(rt.debug_run_to_inflight(CAP) > 0);
    for _ in 0..1500 {
        rt.frame_ticks(1, 0, 0).expect("tick");
    }
    assert!(
        rt.scene_host
            .host()
            .expect("host")
            .world
            .cast_module_resident(),
        "Ozma's cast is no longer running at the relaunch: the case is vacuous"
    );
    launch(&mut rt, &Action::Cast(0x81));
    assert!(rt.debug_run_to_inflight(CAP) > 0);
    let mut resident = None;
    for i in 0..600 {
        rt.frame_ticks(1, 0, 0).expect("tick");
        if rt
            .scene_host
            .host()
            .expect("host")
            .world
            .cast_module_resident()
        {
            resident = Some(i);
            break;
        }
    }
    eprintln!("[showcase] Gimard after Ozma: module resident at tick {resident:?}");
    assert!(
        resident.is_some_and(|t| t < 200),
        "Gimard's module waited {resident:?} ticks behind Ozma's abandoned line"
    );
}
