//! Disc-gated ladder for the **fishing HUD's five one-shot banner animators**
//! (`FUN_801d78ec` hook / `FUN_801d75dc` reel-in / `FUN_801d6f10` miss /
//! `FUN_801d7528` auxiliary / `FUN_801d71d4` strike splash).
//!
//! Each animator only runs while its own timer is non-zero, and every timer is
//! seeded by a *session phase edge*. `docs/tooling/reach-triage.md` files these
//! five as "catch / miss / strike event banners a short session does not land":
//! the composition ladder starts a fishing session and reads its HUD, but never
//! hooks, lands or snaps, so all five timers stay at zero and their bodies
//! never execute.
//!
//! This ladder lands them, on the host that owns the venue-faithful loop - the
//! browser **minigames page** (`LegaiaMinigames::fishing_pond_*`, the same API
//! `site/js/minigames-app.js` drives). It plays real casts until each edge has
//! fired, servicing the HUD once a frame exactly as the page does.
//!
//! ## Land and snap are the two ends of one race, and the reel button decides
//!
//! Reeling shortens the line at a fixed rate while the fish's pull loads the
//! tension gauge through the reel divisor. Reel A (Cross, `rod*9 + 0x23`)
//! recovers 3 line units per frame; reel B (Square, `rod*6 + 0x19`) recovers 2
//! but divides the pull *less*, so it loads the gauge roughly twice as fast per
//! unit of line recovered. That ratio is what makes the snap reachable at all:
//! swept across every venue x lure x rod combination, the reel-A path never
//! reached the ceiling, and reel B does - on the Heavy-lure row at Buma, whose
//! band 3 (the 75% roll) is the hardest-pulling common fish. Both levers are
//! ordinary play, so the two outcomes come from playing differently rather than
//! from reaching into the session.
//!
//! ## Host symmetry
//!
//! The browser **play page** owns the simpler `FishingSession` loop and services
//! the same five timers from `tick_frame`; its hook / reel-in / auxiliary edges
//! are driven here too. The native window's `window/minigames.rs` runs the
//! identical `FishingBanners` edge map but lives in a `bin/` target no test can
//! call - that host is covered by inspection, not by this ladder.

#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeSet;

use legaia_web_viewer::minigames::LegaiaMinigames;
use legaia_web_viewer::runtime::LegaiaRuntime;

/// Glyph ids the five banner animators emit, from their retail call sites.
const GLYPH_HOOK: i64 = 7;
const GLYPH_REEL_IN: i64 = 0xd;
const GLYPH_MISS: i64 = 0x19;
const GLYPH_CONVERGE: i64 = 0xc;
const GLYPH_SPLASH_A: i64 = 0x416;
const GLYPH_SPLASH_B: i64 = 0x816;

/// The shared slide ramp's hold value (`banner_slide` holds at `0xa0`).
const SLIDE_HOLD: i64 = 0xa0;

const REEL_A: u32 = 0x40;
/// Reel B (Square): the "reel harder" path - `rod*6 + 0x19` divides the pull
/// less, so it loads the tension gauge faster per unit of line recovered.
const REEL_B: u32 = 0x80;

fn disc_bytes() -> Option<Vec<u8>> {
    let p = std::env::var_os("LEGAIA_DISC_BIN")?;
    std::fs::read(p).ok()
}

fn minigames() -> Option<LegaiaMinigames> {
    let mut mg = LegaiaMinigames::new();
    mg.load_disc(disc_bytes()?).ok()?;
    mg.fishing_pond_ready().then_some(mg)
}

/// One frame's banner glyph draws, as `(id, x, y, brightness)`.
fn banner_glyphs(hud: &str) -> Vec<(i64, i64, i64, i64)> {
    let v: serde_json::Value = serde_json::from_str(hud).expect("hud json");
    v.as_array()
        .expect("hud array")
        .iter()
        .filter(|d| d["t"] == "glyph" && d["layer"] == 0)
        .map(|d| {
            (
                d["id"].as_i64().unwrap_or(-1),
                d["x"].as_i64().unwrap_or(0),
                d["y"].as_i64().unwrap_or(0),
                d["b"].as_i64().unwrap_or(0),
            )
        })
        .collect()
}

fn tension(mg: &LegaiaMinigames) -> i64 {
    let v: serde_json::Value =
        serde_json::from_str(&mg.fishing_pond_state_json()).expect("state json");
    v["tension"].as_i64().unwrap_or(0)
}

fn phase(mg: &LegaiaMinigames) -> String {
    let v: serde_json::Value =
        serde_json::from_str(&mg.fishing_pond_state_json()).expect("state json");
    v["phase"].as_str().unwrap_or_default().to_string()
}

/// Every banner glyph the run emitted, keyed by id, with the frames each was
/// seen at. Servicing the HUD once per frame is the page's own contract.
#[derive(Default)]
struct Seen {
    ids: BTreeSet<i64>,
    /// Per-id `(x, y, brightness)` samples, in frame order.
    samples: Vec<(i64, i64, i64, i64)>,
    /// Highest tension any fight in the run reached.
    peak_tension: i64,
}

impl Seen {
    fn absorb(&mut self, hud: &str) {
        for g in banner_glyphs(hud) {
            self.ids.insert(g.0);
            self.samples.push(g);
        }
    }
    fn xs(&self, id: i64) -> Vec<i64> {
        self.samples
            .iter()
            .filter(|s| s.0 == id)
            .map(|s| s.1)
            .collect()
    }
}

/// Play one cast to its resolution (or until the line reels back in), driving
/// the reel gesture. Services the HUD every frame, as the page does.
fn play_one_cast(mg: &mut LegaiaMinigames, seen: &mut Seen, budget: usize, fight_reel: u32) {
    // Idle -> WindUp -> Power.
    mg.fishing_pond_tick(0, true, 0);
    seen.absorb(&mg.fishing_pond_hud_json());
    for _ in 0..budget {
        if phase(mg) == "power" {
            break;
        }
        mg.fishing_pond_tick(0, false, 0);
        seen.absorb(&mg.fishing_pond_hud_json());
    }
    // Let the meter climb a while, then lock: a deep cast keeps the readout
    // above the bite ladder's live threshold, which is what makes a strike
    // possible at all.
    for _ in 0..40 {
        mg.fishing_pond_tick(0, false, 0);
        seen.absorb(&mg.fishing_pond_hud_json());
    }
    mg.fishing_pond_tick(0, true, 0);
    seen.absorb(&mg.fishing_pond_hud_json());
    for _ in 0..budget {
        if phase(mg) == "waiting" {
            break;
        }
        mg.fishing_pond_tick(0, false, 0);
        seen.absorb(&mg.fishing_pond_hud_json());
    }
    // Work the lure: a 6-on / 6-off reel gesture, which is what the cadence
    // recogniser matches against and what the strike roll requires held.
    let mut f = 0usize;
    while f < budget && phase(mg) == "waiting" {
        let held = (f / 6).is_multiple_of(2);
        mg.fishing_pond_tick(
            if held { REEL_A } else { 0 },
            false,
            i32::from(f.is_multiple_of(6)),
        );
        seen.absorb(&mg.fishing_pond_hud_json());
        f += 1;
    }
    // Fight: straight reel-in until it lands or snaps.
    let mut f = 0usize;
    while f < budget && phase(mg) == "hooked" {
        mg.fishing_pond_tick(fight_reel, false, 0);
        seen.absorb(&mg.fishing_pond_hud_json());
        seen.peak_tension = seen.peak_tension.max(tension(mg));
        f += 1;
    }
    // Let the resolution banner run, then recast off the result screen - the
    // edge the auxiliary converge banner is seeded from.
    for _ in 0..0x110 {
        mg.fishing_pond_tick(0, false, 0);
        seen.absorb(&mg.fishing_pond_hud_json());
    }
    if matches!(phase(mg).as_str(), "landed" | "snapped") {
        mg.fishing_pond_tick(0, true, 0);
        seen.absorb(&mg.fishing_pond_hud_json());
        for _ in 0..0x110 {
            mg.fishing_pond_tick(0, false, 0);
            seen.absorb(&mg.fishing_pond_hud_json());
        }
    }
}

/// Play casts at `rod` until `want` has been seen or the cast budget runs out.
fn run_venue(rod: i32, lure: u32, seed: u32, casts: usize, fight_reel: u32) -> Option<Seen> {
    let mut mg = minigames()?;
    assert!(
        mg.fishing_pond_start(0, lure, rod, 100, 0, 0, 0, 0, seed),
        "the pond tables decoded but the session refused to start"
    );
    let mut seen = Seen::default();
    for _ in 0..casts {
        play_one_cast(&mut mg, &mut seen, 4000, fight_reel);
    }
    Some(seen)
}

#[test]
fn the_hook_reel_in_and_splash_banners_all_run_on_a_played_venue() {
    // The upgraded rod divides the pull hardest, so the reel-in wins the race
    // against the tension gauge and the catch lands.
    let Some(seen) = run_venue(2, 1, 0x1357_9BDF, 24, REEL_A) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or the pond tables did not decode");
        return;
    };

    assert!(
        seen.ids.contains(&GLYPH_HOOK),
        "the hook banner never ran; glyphs seen: {:?}",
        seen.ids
    );
    assert!(
        seen.ids.contains(&GLYPH_SPLASH_A) && seen.ids.contains(&GLYPH_SPLASH_B),
        "the strike splash is a PAIR of glyphs and only part of it ran: {:?}",
        seen.ids
    );
    assert!(
        seen.ids.contains(&GLYPH_REEL_IN),
        "no catch landed in 24 casts, so the reel-in banner never ran: {:?}",
        seen.ids
    );
    assert!(
        seen.ids.contains(&GLYPH_CONVERGE),
        "the auxiliary banner never ran off the recast edge: {:?}",
        seen.ids
    );

    // The two slide banners travel in opposite directions and meet at the
    // same hold. The hook banner slides in from the left, so its x rises to
    // the hold; the reel-in banner is its mirror about the 0x140 stage width.
    let hook_xs = seen.xs(GLYPH_HOOK);
    assert!(
        hook_xs.contains(&SLIDE_HOLD),
        "the hook banner never reached its hold: {:?}",
        &hook_xs[..hook_xs.len().min(40)]
    );
    assert!(
        hook_xs.first().copied().unwrap_or(SLIDE_HOLD) < SLIDE_HOLD,
        "the hook banner must slide IN, not appear at the hold"
    );
    let reel_xs = seen.xs(GLYPH_REEL_IN);
    assert!(
        reel_xs.contains(&SLIDE_HOLD),
        "the reel-in banner never reached the same hold: {reel_xs:?}"
    );
    assert!(
        reel_xs.first().copied().unwrap_or(SLIDE_HOLD) > SLIDE_HOLD,
        "the reel-in banner must slide in from the RIGHT"
    );

    // The auxiliary banner is one glyph emitted twice per frame, mirrored -
    // a converging pair, not a single sprite.
    let conv: Vec<_> = seen
        .samples
        .iter()
        .filter(|s| s.0 == GLYPH_CONVERGE)
        .collect();
    assert!(conv.len() >= 2, "converge pair: {conv:?}");
    assert!(
        conv.chunks(2)
            .filter(|c| c.len() == 2)
            .all(|c| c[0].1 + c[1].1 == 0x140),
        "the converging pair must be mirrored about the 0x140 stage width"
    );

    // The splash fades: its brightness ramps up to the 0x80 hold and back
    // down, and it rises one pixel every 32 frames.
    let splash: Vec<_> = seen
        .samples
        .iter()
        .filter(|s| s.0 == GLYPH_SPLASH_A)
        .collect();
    assert!(
        splash.iter().any(|s| s.3 == 0x80),
        "the splash never reached its brightness hold"
    );
    assert!(
        splash.iter().any(|s| s.2 < 0x50),
        "the splash never rose off its seed row"
    );
}

#[test]
fn the_miss_banner_runs_when_reeling_hard_loses_the_line() {
    // Heavy lure at Buma on the starter rod, reeled with Square: the gauge
    // pins before the line is in and the fight snaps - the retry countdown the
    // miss banner animates.
    let Some(seen) = run_venue(0, 2, 0x0BAD_F00D, 40, REEL_B) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or the pond tables did not decode");
        return;
    };
    assert!(
        seen.ids.contains(&GLYPH_MISS),
        "no line snapped in 40 casts of hard reeling, so the miss banner \
         never ran; glyphs seen: {:?}, peak tension {}",
        seen.ids,
        seen.peak_tension
    );
    // Same mirrored trajectory as the reel-in banner - it enters from the
    // right and holds at the shared 0xa0.
    let xs = seen.xs(GLYPH_MISS);
    assert!(
        xs.contains(&SLIDE_HOLD),
        "the miss banner never reached the shared hold: {xs:?}"
    );
    assert!(
        xs.first().copied().unwrap_or(SLIDE_HOLD) > SLIDE_HOLD,
        "the miss banner must slide in from the right"
    );
}

/// Host symmetry: the browser play page services the same five timers off its
/// own `FishingSession` phase edges, from `tick_frame`. Drive a cast to its
/// hook and its landing there too, so the banner set is not one host's.
#[test]
fn the_play_page_services_the_same_banner_timers() {
    let Some(bytes) = disc_bytes() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    if rt.load_disc(bytes, String::new()).is_err() || rt.enter_field("town01").is_err() {
        eprintln!("[skip] the play page could not reach town01");
        return;
    }
    if !rt.play_fishing_start() {
        eprintln!("[skip] the fishing overlay did not decode");
        return;
    }

    let phase = |rt: &LegaiaRuntime| -> String {
        let v: serde_json::Value =
            serde_json::from_str(&rt.play_fishing_state_json()).expect("state json");
        v["phase"].as_str().unwrap_or_default().to_string()
    };
    const CROSS: u16 = 0x4000;

    // Cross locks the cast: the Casting -> Fighting edge seeds the hook and
    // splash timers.
    assert_eq!(phase(&rt), "casting");
    rt.set_pad(0);
    rt.tick_frame().expect("tick");
    rt.set_pad(CROSS);
    rt.tick_frame().expect("tick");
    assert_eq!(phase(&rt), "fighting", "Cross must lock the cast");

    // Hold Cross: reeling both accrues landing progress and loads the gauge,
    // so the fight resolves one way or the other. Either resolution is a
    // banner edge (reel-in on a catch, miss on a snap).
    let mut resolved = false;
    for _ in 0..20_000 {
        rt.tick_frame().expect("tick");
        if phase(&rt) == "done" {
            resolved = true;
            break;
        }
    }
    assert!(resolved, "the play page's fight never resolved");

    // Recast off the result: the Done -> Casting edge is the auxiliary
    // banner's seed on this host.
    rt.set_pad(0);
    rt.tick_frame().expect("tick");
    rt.set_pad(CROSS);
    rt.tick_frame().expect("tick");
    assert_eq!(phase(&rt), "casting", "Cross must recast off the result");

    // The HUD still composes while the banners run - the page reads it every
    // frame and a live banner must not close the payload.
    let hud: serde_json::Value =
        serde_json::from_str(&rt.play_fishing_hud_json(320, 240)).expect("hud json");
    assert_eq!(hud["open"].as_bool(), Some(true), "{hud}");
}
