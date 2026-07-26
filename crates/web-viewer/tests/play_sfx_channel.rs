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

/// Rendering through a throwaway SPU proves each **pinned retail** cue id's
/// program and sample resolve in the resident class-2 bank.
///
/// Read what this does and does not claim. It probes `cue` - the id retail
/// fires - not `fires`, which is what this host enqueues and is currently
/// `null` for every menu row. So a pass here means "the id resolves to a real
/// sample in the bank the page stages", which is worth pinning: it is how the
/// class-2 bank was confirmed to be the right bank for this id range (program 0
/// there is a one-VAG-per-semitone SFX key map whose single-note windows line
/// up with these descriptors' notes). It does **not** mean the page plays them,
/// nor that the rendering is pitched correctly - that is the open question in
/// `play_sfx::CUE_MENU_CURSOR`, and it is why these cues are withheld.
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
        // Retail's id and what the host plays are separate fields, and both
        // must be present: reporting only `cue` would claim a sound the host
        // withholds, reporting only `fires` would hide a pinned fact.
        assert!(
            ev["cue"].as_u64().is_some(),
            "event {ev} must report retail's cue id"
        );
        assert!(
            ev.get("fires").is_some(),
            "event {ev} must report what the host fires (null when withheld)"
        );
        // A fired cue must be retail's own id, never a stand-in sample.
        if let Some(f) = ev["fires"].as_u64() {
            assert_eq!(
                f,
                ev["cue"].as_u64().unwrap(),
                "event {ev}: a fired cue must be retail's id"
            );
        }
    }
    // The three pause-menu rows are traced ring writes, not port picks. This
    // is the corrected claim: they come from the SCUS list kernel FUN_80032A44,
    // not from the Baka Fighter overlay they were previously attributed to.
    for name in ["menu_cursor", "menu_confirm", "menu_cancel"] {
        let ev = events
            .iter()
            .find(|e| e["event"] == name)
            .unwrap_or_else(|| panic!("{name} must be advertised"));
        assert_eq!(ev["source"], "disc", "{name} is a traced ring write");
    }
    // The footstep must NOT appear here. Its cadence is the retail kernel but
    // its cue id is unpinned, and an id resolves through the descriptor table
    // to a *program index* whose sample differs per resident bank - so a
    // guessed id is arbitrary, not approximate. It played an impact sample in
    // the field. This row's absence is the fix; advertising an id the host no
    // longer fires would be worse than either firing it or omitting it.
    assert!(
        events.iter().all(|e| e["event"] != "footstep"),
        "the footstep must stay out of the advertised cue list while its id is \
         unpinned - see CUE_FOOTSTEP"
    );
}

/// The menu firing site has to stay *wired* while its cues are withheld.
///
/// Assert on `menu_cue_requests`, not `queued` or `fired`, for the same reason
/// the footstep test below asserts on `cadence_steps`: a withheld row never
/// reaches the scheduler, so `queued` cannot tell "the page asked and the host
/// declined" from "the page never asked" - and that is precisely the failure
/// that would make this fix indistinguishable from deleting the calls. When the
/// pitch path is pinned and the cues flip to `Some`, `queued` starts tracking
/// this counter and the test still holds.
#[test]
fn a_withheld_menu_cue_is_requested_counted_and_silent() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let state = |rt: &LegaiaRuntime| -> (u64, u64, u64) {
        let v: serde_json::Value =
            serde_json::from_str(&rt.play_sfx_state_json()).expect("sfx state json");
        (
            v["menu_cue_requests"].as_u64().unwrap_or(0),
            v["queued"].as_u64().unwrap_or(0),
            v["fired"].as_u64().unwrap_or(0),
        )
    };

    let (r0, q0, f0) = state(&rt);
    assert_eq!(r0, 0, "no cue requested before the page fires one");

    // Exactly what site/js/play-app.js does on a pad edge in the pause menu.
    for name in ["menu_cursor", "menu_confirm", "menu_cancel"] {
        assert!(
            !rt.play_sfx_event(name),
            "{name} is withheld, so firing it must report that nothing sounded"
        );
    }
    let (r1, q1, f1) = state(&rt);
    assert_eq!(r1, r0 + 3, "each menu cue request must be counted");
    assert_eq!(q1, q0, "a withheld cue must not reach the scheduler");
    assert_eq!(f1, f0, "a withheld cue must not key a voice");

    // An unknown event is still rejected without counting - the counter tracks
    // real wiring, not every string the page passes in.
    assert!(!rt.play_sfx_event("no_such_event"));
    assert_eq!(state(&rt).0, r1, "an unknown event is not a cue request");
}

/// The footstep cadence has to be *reachable from the host tick* and has to
/// discriminate: walking steps the cadence, standing still does not.
///
/// Assert on `cadence_steps`, not `queued` or `fired`. `CUE_FOOTSTEP` is
/// `None` while retail's cue id is unpinned, so no cue is enqueued and no
/// voice is keyed - but the ported kernel still runs, and this is the only
/// counter that can tell a wired-but-silent cadence from an unwired one. That
/// distinction is not academic: this cadence once ran every frame and fired
/// zero steps over 274 units of walking, because it was fed the wrong speed
/// quantity, while every unit test in the kernel passed.
///
/// `fired` would be worse still: off wasm there is no SPU to key a voice into,
/// so it is unconditionally zero here, and a "standing still fires nothing"
/// assertion written against it passes without exercising anything - exactly
/// the vacuous shape this file exists to avoid.
#[test]
fn walking_steps_the_cadence_and_standing_still_does_not() {
    let Some(mut rt) = loaded_in_town() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let steps = |rt: &LegaiaRuntime| -> u64 {
        let v: serde_json::Value =
            serde_json::from_str(&rt.play_sfx_state_json()).expect("sfx state json");
        v["cadence_steps"].as_u64().unwrap_or(0)
    };

    // Idle: the retail gate parks the countdown above `0xB`, so a stationary
    // player is silent however long it stands there.
    rt.set_pad(0);
    for _ in 0..240 {
        rt.tick_frame().expect("tick");
    }
    let idle = steps(&rt);
    assert_eq!(idle, 0, "a stationary player must not step the cadence");

    // Walk: hold Up. Four seconds of sim is far more than the cadence's
    // interval, so several steps must mature.
    const UP: u16 = 0x0010;
    rt.set_pad(UP);
    for _ in 0..240 {
        rt.tick_frame().expect("tick");
    }
    let walked = steps(&rt);
    assert!(
        walked > 0,
        "walking must step the cadence - the retail gate has to open for a \
         moving player, or the whole source is inert"
    );

    // Stop again: the count must stop growing, which proves the cues track the
    // walk rather than a free-running timer.
    rt.set_pad(0);
    for _ in 0..240 {
        rt.tick_frame().expect("tick");
    }
    assert_eq!(
        steps(&rt),
        walked,
        "standing still again must step the cadence no further"
    );
}
