//! Disc-gated: drive the **real** parsed dance step chart (PROT 0980) through
//! the engine dance rules engine ([`legaia_engine_core::dance`]).
//!
//! The chart parser itself is pinned by `legaia-asset`'s `dance_chart_real`;
//! this closes the engine end - that [`DanceGame::from_overlay`] loads the baked
//! chart off the user's disc and a full beat-clock + judge run is driveable on
//! it (no synthetic fixture). No Sony bytes are asserted, only structural facts:
//! the chart loads, a perfectly-timed play-through of the active lane's own
//! chart symbols scores and passes, and the song clock terminates the run.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/PROT.DAT` are absent.

use std::path::PathBuf;

use legaia_asset::static_overlay;
use legaia_engine_core::dance::{DanceDir, DanceGame, Judge};
use legaia_prot::archive::Archive;

fn prot_dat() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for p in ["extracted/PROT.DAT", "../../extracted/PROT.DAT"] {
        let f = PathBuf::from(p);
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

fn dance_overlay() -> Option<Vec<u8>> {
    let prot = prot_dat()?;
    let mut archive = Archive::open(&prot).expect("open PROT.DAT");
    let rec = static_overlay::overlay_map()
        .by_prot_index(legaia_asset::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32)
        .expect("dance overlay in static map");
    let entry = archive
        .entries
        .iter()
        .find(|e| e.index == rec.prot_index)
        .cloned()
        .expect("PROT entry present");
    let mut raw = Vec::new();
    archive.read_entry(&entry, &mut raw).expect("read entry");
    Some(static_overlay::as_loaded(&raw, rec).expect("as-loaded form"))
}

#[test]
fn real_dance_chart_drives_a_scoring_run() {
    let Some(overlay) = dance_overlay() else {
        eprintln!("[skip] dance overlay unavailable (disc-gated)");
        return;
    };

    let mut game = DanceGame::from_overlay(&overlay, false).expect("real chart loads");

    // Auto-play the run frame by frame (the beat clock advances 10 phase units
    // per frame; a beat spans BEAT_PERIOD=281 units, so ~28 frames per beat).
    // On the first frame of each new beat the intra-beat phase is smallest -
    // inside the acceptance window - so that is when a CPU dancer presses the
    // note the chart calls for. This proves the real chart yields judgeable,
    // scoring notes through the engine judge.
    let mut hits = 0usize;
    let mut notes_seen = 0usize;
    let mut last_beat = game.beat_index();
    let mut frames = 0u32;
    while !game.song_over() && frames < 100_000 {
        game.advance(1);
        frames += 1;
        let beat = game.beat_index();
        // Only act once per beat, on the frame the beat index first changes.
        if beat == last_beat {
            continue;
        }
        last_beat = beat;
        // Press exactly what the hit judge expects for this lane + beat (the raw
        // chart cell, `FUN_801d1960`'s source - not the display path's held-
        // sequence substitution), so a well-timed press never misses.
        if let Some(symbol) = game.judged_symbol()
            && symbol != 0
        {
            notes_seen += 1;
            let dir = match symbol {
                1 => DanceDir::A,
                2 => DanceDir::B,
                _ => DanceDir::C,
            };
            match game.judge_press(dir) {
                Judge::Hit { weight } | Judge::Sequence { weight } => {
                    assert!(
                        weight > 0,
                        "an in-window press carries a positive accuracy weight"
                    );
                    hits += 1;
                }
                Judge::Miss => panic!("a required-symbol press inside the window must not miss"),
            }
        }
    }

    assert!(
        notes_seen > 0,
        "the real lane-0 chart must present judgeable notes during the run"
    );
    assert_eq!(
        hits, notes_seen,
        "every well-timed press on a real note scored"
    );
    assert!(game.score() > 0, "an auto-played run scores points");
    assert!(game.song_over(), "the beat clock terminates the run");
    eprintln!(
        "[dance] real-chart run: {hits}/{notes_seen} notes hit, final score {}",
        game.score()
    );
}

/// Mirror the **play-window K-key path** exactly: open the disc as a
/// [`ProtIndex`], read the dance overlay through `entry_bytes_extended`, lift it
/// to loaded form, parse the chart, then drive it through `World::enter_dance` +
/// `World::tick`. This is the load path `start_dance_minigame` uses in the
/// engine binary (which can't be unit-tested through its wgpu window), so this
/// locks that the runtime entry point resolves a real, scoreable chart.
#[test]
fn playwindow_load_path_enters_and_scores_a_dance() {
    use legaia_engine_core::input::PadButton;
    use legaia_engine_core::scene::SceneHost;
    use legaia_engine_core::world::{SceneMode, World};

    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let host = match SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return;
        }
    };
    let rec = static_overlay::overlay_map()
        .by_prot_index(legaia_asset::dance_chart::DANCE_OVERLAY_PROT_INDEX as u32)
        .expect("dance overlay in static map");
    let raw = host
        .index
        .entry_bytes_extended(rec.prot_index)
        .expect("read PROT 0980 (extended)");
    let loaded = static_overlay::as_loaded(&raw, rec).expect("as-loaded form");
    let game = DanceGame::from_overlay(&loaded, false).expect("real chart loads via shell path");

    let mut world = World::new();
    world.mode = SceneMode::Field;
    world.enter_dance(game);
    assert_eq!(world.mode, SceneMode::Dance);

    // Drive several beats, pressing the arrow the chart calls for on each first
    // in-window frame. Proves the wired tick judges real chart data into score.
    let mut pressed_any = false;
    for _ in 0..600 {
        if world.mode != SceneMode::Dance {
            break;
        }
        // Press what the hit judge matches against - the raw chart cell
        // (`judged_symbol`), not the display path's held-sequence substitution.
        // `tick_dance` advances the clock then judges this frame's press, so a
        // press on any in-window frame carrying a note scores.
        let want = world
            .dance
            .as_ref()
            .filter(|g| !g.in_dead_zone())
            .and_then(|g| g.judged_symbol())
            .filter(|s| *s != 0);
        let button = match want {
            Some(1) => Some(PadButton::Left),
            Some(2) => Some(PadButton::Right),
            Some(3) => Some(PadButton::Up),
            _ => None,
        };
        world.set_pad(0);
        if let Some(b) = button {
            world.set_pad(b.mask());
            pressed_any = true;
        }
        let _ = world.tick();
    }
    assert!(pressed_any, "the real chart called for at least one arrow");
    let final_game = world.exit_dance().expect("game still installed");
    // A cooperating play-through banks a non-zero score off the real chart.
    assert!(
        final_game.score() > 0,
        "expected a scoring run off the real dance chart"
    );
}
