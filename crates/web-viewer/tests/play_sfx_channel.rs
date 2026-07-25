//! Disc-gated oracle for the browser play page's **sound-effect channel**
//! (`LegaiaRuntime::play_sfx_*`).
//!
//! The page had BGM and nothing else. What this pins is the part that can be
//! checked without a speaker:
//!
//! 1. **The descriptor table decodes** off the visitor's own executable, into
//!    the id space `docs/formats/sfx-table.md` documents.
//! 2. **Cues actually produce sound.** `play_sfx_probe_peak` renders a cue
//!    through a throwaway SPU and a fresh upload of the class-2 program bank; a
//!    non-zero peak is the evidence that the descriptor resolves to a real
//!    sample rather than to silence. This is the assertion that would have
//!    caught "the channel is wired and every cue is inaudible".
//! 3. **Provenance is declared.** Every advertised event says whether its cue id
//!    is traced to retail (`disc`) or is a port pick (`site`), so the page can
//!    never quietly claim a sound is the game's.
//!
//! The *live* half (keying a voice in the page's running SPU) needs a
//! `WebAudioOut`, which only exists on wasm32 in a browser - that half is
//! covered by the headless run, not here.
//!
//! No Sony bytes are asserted, only structural facts. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::runtime::LegaiaRuntime;

fn loaded_in_town() -> Option<LegaiaRuntime> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).ok()?;
    rt.enter_field("town01").ok()?;
    Some(rt)
}

/// The descriptor bank installs from the executable at `load_disc`, before any
/// audio device exists - which is what lets the page decide about sound later
/// without re-reading the disc.
#[test]
fn descriptor_bank_installs_from_the_executable() {
    let Some(rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let v: serde_json::Value =
        serde_json::from_str(&rt.play_sfx_state_json()).expect("sfx state json");
    let n = v["descriptors"].as_u64().expect("descriptors");
    assert!(
        n > 0,
        "the SCUS sound-effect table (DAT_8006F198) must decode: {v}"
    );
    assert!(
        n <= 100,
        "the static table is 100 entries (0x00..=0x63); got {n}"
    );
}

/// The measurement that matters: a cue has to make a *noise*. Rendering through
/// a throwaway SPU proves the descriptor's program and sample resolve in the
/// resident class-2 bank, which is exactly what a silent-but-wired channel
/// would fail.
#[test]
fn advertised_cues_render_a_non_silent_buffer() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let events: Vec<serde_json::Value> =
        serde_json::from_str(&rt.play_sfx_events_json()).expect("events json");
    assert!(!events.is_empty(), "the host must advertise some events");

    // A quarter second is far longer than any retail one-shot.
    let window = legaia_engine_audio::SPU_INTERNAL_RATE / 4;
    let mut audible = 0;
    for ev in &events {
        let cue = ev["cue"].as_u64().expect("cue id") as u32;
        let name = ev["event"].as_str().unwrap_or("?");
        let peak = rt.play_sfx_probe_peak(cue, window);
        if peak > 0 {
            audible += 1;
        } else {
            eprintln!("[note] event {name} (cue {cue:#x}) rendered silence on this disc");
        }
    }
    assert!(
        audible > 0,
        "at least one advertised cue must render audible PCM - a channel whose \
         every cue is silent is not a channel"
    );

    // The program bank has to be the class-2 one; a fallback to nothing would
    // make every cue silent for a reason worth surfacing.
    let v: serde_json::Value =
        serde_json::from_str(&rt.play_sfx_state_json()).expect("sfx state json");
    let bank = v["bank_prot"].as_u64().unwrap_or(0);
    assert!(
        bank == 869 || bank == 875,
        "the program bank must be the class-2 SFX bank (869) or its documented \
         alternate (875); got {bank}"
    );
}

/// A nonexistent cue is silent rather than a panic, and an id outside the
/// table's byte space is rejected.
#[test]
fn unknown_cues_are_silent_not_fatal() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert_eq!(rt.play_sfx_probe_peak(0xFF, 1024), 0, "0xFF is unassigned");
    assert_eq!(
        rt.play_sfx_probe_peak(0x1_0000, 1024),
        0,
        "an id past the byte space must be rejected"
    );
    assert!(!rt.play_sfx_event("no_such_event"));
}

/// Every advertised event declares its provenance, and the note explains the
/// choice. This is the honesty contract the page renders.
#[test]
fn every_event_declares_disc_or_site_provenance() {
    let Some(rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let events: Vec<serde_json::Value> =
        serde_json::from_str(&rt.play_sfx_events_json()).expect("events json");
    for ev in &events {
        let src = ev["source"].as_str().unwrap_or("");
        assert!(
            src == "disc" || src == "site",
            "event {ev} must declare source disc or site"
        );
        assert!(
            ev["why"].as_str().is_some_and(|s| !s.is_empty()),
            "event {ev} must carry a provenance note"
        );
    }
    // The footstep row is the one whose cadence is retail and whose cue id is
    // not; it must say so rather than pass itself off as traced.
    let foot = events
        .iter()
        .find(|e| e["event"] == "footstep")
        .expect("footstep event");
    assert_eq!(foot["source"], "site", "the footstep cue id is a port pick");
}

/// The footstep cadence has to be *reachable from the host tick* and has to
/// discriminate: walking produces cues, standing still produces none.
///
/// Assert on `queued`, not `fired`. Off wasm there is no SPU to key a voice
/// into, so `fired` is unconditionally zero here - a "standing still fires
/// nothing" test written against `fired` passes without exercising anything,
/// which is exactly the vacuous shape this file exists to avoid. `queued`
/// counts what the *source* produced and is live on both targets.
#[test]
fn walking_queues_footsteps_and_standing_still_does_not() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let queued = |rt: &LegaiaRuntime| -> u64 {
        let v: serde_json::Value =
            serde_json::from_str(&rt.play_sfx_state_json()).expect("sfx state json");
        v["queued"].as_u64().unwrap_or(0)
    };

    // Idle: the retail gate parks the countdown above `0xB`, so a stationary
    // player is silent however long it stands there.
    rt.set_pad(0);
    for _ in 0..240 {
        rt.tick_frame().expect("tick");
    }
    let idle = queued(&rt);
    assert_eq!(idle, 0, "a stationary player must queue no footstep cue");

    // Walk: hold Up. Four seconds of sim is far more than the cadence's
    // interval, so several steps must mature.
    const UP: u16 = 0x0010;
    rt.set_pad(UP);
    for _ in 0..240 {
        rt.tick_frame().expect("tick");
    }
    let walked = queued(&rt);
    assert!(
        walked > 0,
        "walking must queue footstep cues - the cadence gate has to open for a \
         moving player, or the whole source is inert"
    );

    // Stop again: the count must stop growing, which proves the cues track the
    // walk rather than a free-running timer.
    rt.set_pad(0);
    for _ in 0..240 {
        rt.tick_frame().expect("tick");
    }
    assert_eq!(
        queued(&rt),
        walked,
        "standing still again must queue nothing further"
    );
}
