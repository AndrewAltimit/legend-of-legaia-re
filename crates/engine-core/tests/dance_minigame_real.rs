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

/// The **competitors' runs are real**: retail synthesises each CPU dancer's pad
/// word from the same chart (`FUN_801d1820` -> `FUN_801d4040`) and runs it
/// through the same award routine (`FUN_801d1af4`), so their scores climb off
/// the disc's own per-kind bonus row (`DAT_801d41a4`) and they spend their three
/// triangles on the disc's own schedule (`DAT_801d41e4`). Drive a hands-off run
/// on the real overlay tables and assert exactly that.
#[test]
fn real_tables_drive_the_rival_dancers() {
    let Some(overlay) = dance_overlay() else {
        eprintln!("[skip] dance overlay unavailable (disc-gated)");
        return;
    };
    let tables = legaia_asset::dance_chart::parse_tables(&overlay).expect("scoring tables decode");
    // Every kind's bonus row is the retail `k, 2k, 3k` shape - the (lane + 1)
    // scaling is baked into the data, not applied by the code.
    for kind in 0..4 {
        let base = tables.bonus(kind, 0);
        assert!(base > 0, "kind {kind} has a sequence-bonus row");
        assert_eq!(tables.bonus(kind, 1), base * 2);
        assert_eq!(tables.bonus(kind, 2), base * 3);
    }

    let mut game = DanceGame::from_overlay(&overlay, false).expect("real chart + tables load");
    assert_eq!(
        game.dancer_count(),
        3,
        "the qualifier floor is three dancers"
    );
    assert_eq!(game.dancer_kind(0), 0, "slot 0 is Noa (the human)");
    for i in 0..3 {
        assert_eq!(game.dancer_triangles(i), 3, "three groovy moves per song");
    }

    // Hands off the pad: only the CPU auto-feed runs.
    let mut climbed = [0u32; 2];
    while !game.song_over() {
        game.advance(1);
        for (n, c) in climbed.iter_mut().enumerate() {
            *c = game.dancer_score(n + 1);
        }
    }
    assert_eq!(game.score(), 0, "the human never pressed");
    for (n, score) in climbed.iter().enumerate() {
        assert!(*score > 0, "rival {} scored off the real chart", n + 1);
    }
    // Both rivals spent at least one triangle on the disc schedule, and no rival
    // ever exceeded its stock of three.
    let spent: Vec<u32> = (1..3).map(|i| 3 - game.dancer_triangles(i)).collect();
    assert!(
        spent.iter().any(|&s| s > 0),
        "the disc schedule fires a CPU groovy move during the song"
    );
    assert!(spent.iter().all(|&s| s <= 3));
    assert!(
        !game.beating_rivals(),
        "a player who never presses loses the qualifier"
    );
    eprintln!(
        "[dance] hands-off run: rivals {:?} (kinds {:?}), triangles spent {:?}",
        climbed,
        [game.dancer_kind(1), game.dancer_kind(2)],
        spent
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
        // The judged buttons are the retail pad bits `DanceDir::pad_bit`
        // names: symbol 1 = Square (0x80), 2 = Circle (0x20), 3 = Triangle
        // (0x10).
        let button = match want {
            Some(1) => Some(PadButton::Square),
            Some(2) => Some(PadButton::Circle),
            Some(3) => Some(PadButton::Triangle),
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

/// The four modes are four **floors**, not four gradings of one floor.
///
/// `FUN_801d0190` picks both the spawn table and how many of its records to
/// spawn, so the cast size differs per mode: six dancers in free play, one in
/// the how-to demo, three in the two competitive modes. A host that always
/// loads the qualifier's three is wrong in half the modes, and that is exactly
/// what this pins - off the real table, not a fixture.
#[test]
fn each_mode_spawns_its_own_floor() {
    use legaia_engine_core::dance::DanceMode;

    let Some(overlay) = dance_overlay() else {
        eprintln!("[skip] dance overlay unavailable (disc-gated)");
        return;
    };

    let cast = |mode: DanceMode| {
        let g =
            DanceGame::from_overlay_for_mode(&overlay, mode, true).expect("real cast table loads");
        (0..g.dancer_count())
            .map(|i| g.dancer_kind(i))
            .collect::<Vec<_>>()
    };

    let qualifier = cast(DanceMode::Qualifier);
    let finals = cast(DanceMode::Finals);
    let how_to = cast(DanceMode::HowTo);
    let free_play = cast(DanceMode::FreePlay);

    // The sizes are the retail per-mode spawn counts.
    assert_eq!(qualifier.len(), 3, "qualifier floor");
    assert_eq!(finals.len(), 3, "finals floor");
    assert_eq!(how_to.len(), 1, "the how-to demo dances alone");
    assert_eq!(free_play.len(), 6, "free play fills the floor");

    // Every floor is led by the human (kind 0 = Noa).
    for (name, roster) in [
        ("qualifier", &qualifier),
        ("finals", &finals),
        ("how-to", &how_to),
        ("free play", &free_play),
    ] {
        assert_eq!(roster[0], 0, "{name} is led by Noa");
    }

    // The competitive floors are genuinely different rosters, not the same
    // three dancers regraded - the finals swap in a rival the qualifier lacks.
    assert_ne!(
        qualifier, finals,
        "the finals floor is a different cast from the qualifier"
    );

    // The how-to demo is the qualifier table truncated to its first record.
    assert_eq!(how_to[..], qualifier[..1]);

    // Mode 2 forces the short song regardless of the caller's request.
    let demo = DanceGame::from_overlay_for_mode(&overlay, DanceMode::HowTo, true).unwrap();
    assert_eq!(
        demo.song_len(),
        legaia_engine_core::dance::SONG_LEN_SHORT,
        "the how-to demo's song length is fixed short"
    );
}
