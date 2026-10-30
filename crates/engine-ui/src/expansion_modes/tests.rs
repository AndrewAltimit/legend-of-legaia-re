//! The mode screens' compositions: every kind draws, and the key labels a
//! player reads are present.

use legaia_engine_modes::active::{ActiveMode, BuildOptions, ModeKind, builtin_sections, entries};
use legaia_engine_modes::session::Placement;

use super::*;

fn build(id: &str) -> ActiveMode {
    let (modes, cards) = builtin_sections();
    ActiveMode::build(modes, cards, id, 5, &BuildOptions::default()).unwrap()
}

fn strings(mode: &ActiveMode, quit: bool) -> Vec<String> {
    mode_screen_strings(&ModeScreen::Playing {
        mode,
        log: &["Hand 1".to_string()],
        quit_prompt: quit,
    })
}

fn has(s: &[String], needle: &str) -> bool {
    s.iter().any(|l| l.contains(needle))
}

/// Every prim lies on the stage (a little slack for outlines).
fn on_stage(prims: &[ScreenPrim]) {
    for p in prims {
        for (x, y) in p.corners() {
            assert!((-2..=STAGE_W as i16 + 2).contains(&x), "x {x}");
            assert!((-2..=STAGE_H as i16 + 2).contains(&y), "y {y}");
        }
    }
}

#[test]
fn picker_lists_every_instance_with_its_kind() {
    let (modes, _) = builtin_sections();
    let list = entries(modes);
    let screen = ModeScreen::Picker {
        heading: "Practice modes",
        entries: &list,
        cursor: 2,
        error: None,
    };
    let s = mode_screen_strings(&screen);
    for e in &list {
        assert!(has(&s, &e.name), "{} missing", e.name);
        assert!(has(&s, e.kind.label()));
    }
    // Backdrop, list panel, the cursor row's two bars.
    assert!(mode_screen_prims(&screen).len() > 6);
}

#[test]
fn poker_table_shows_the_menu_board_and_seats() {
    let mut m = build("builtin-poker");
    // Deal and let the AI act until the player is on the clock.
    for _ in 0..600 {
        if let ModeView::Poker(v) = m.view()
            && v.to_act == Some(0)
        {
            break;
        }
        m.tick(&[0]);
    }
    let ModeView::Poker(v) = m.view() else {
        unreachable!()
    };
    assert_eq!(v.to_act, Some(0), "the player gets a turn");
    let s = strings(&m, false);
    for needle in ["Fold", "Raise", "All in", "Pot", "Seat 2", "Seat 3", "You "] {
        assert!(has(&s, needle), "`{needle}` missing from {s:?}");
    }
    assert!(has(&s, "Check") || has(&s, "Call"));
    let prims = mode_screen_prims(&ModeScreen::Playing {
        mode: &m,
        log: &[],
        quit_prompt: false,
    });
    on_stage(&prims);
    // Two hole cards and five board slots at least.
    assert!(prims.len() > 40, "{}", prims.len());
}

#[test]
fn duel_board_shows_both_hands_and_the_score() {
    let m = build("builtin-duel");
    let s = strings(&m, false);
    for needle in ["You", "Rival", "5"] {
        assert!(has(&s, needle), "`{needle}` missing from {s:?}");
    }
    assert!(
        has(&s, "Your turn") || has(&s, "Rival is thinking"),
        "{s:?}"
    );
    let prims = mode_screen_prims(&ModeScreen::Playing {
        mode: &m,
        log: &[],
        quit_prompt: false,
    });
    on_stage(&prims);
    // Every hand card is a face from the duel's card page.
    let faces = prims
        .iter()
        .filter(|p| {
            matches!(p, legaia_render_kernels::screen_prim::ScreenPrim::Textured(q)
                if q.tpage == legaia_engine_modes::cards::table_art::tpage())
        })
        .count();
    assert_eq!(faces, 10);
}

#[test]
fn golf_course_draws_its_tiles_and_scorecard() {
    let m = build("builtin-golf");
    let s = strings(&m, false);
    for needle in ["Hole 1/2", "Par 2", "Power", "Total", "You"] {
        assert!(has(&s, needle), "`{needle}` missing from {s:?}");
    }
    let prims = mode_screen_prims(&ModeScreen::Playing {
        mode: &m,
        log: &[],
        quit_prompt: false,
    });
    on_stage(&prims);
    // The first hole is 12 x 3 tiles: every tile is a rect.
    assert!(prims.len() >= 36);
}

#[test]
fn race_and_hunt_draw_their_debug_views() {
    let mut race = build("builtin-race");
    // Cross (`0x4000` in the raw pad layout) holds the gas.
    race.tick(&[0x4000]);
    let s = strings(&race, false);
    assert!(has(&s, "Lap 1/2") && has(&s, "Place"), "{s:?}");
    let hunt = build("builtin-prop-hunt");
    let s = strings(&hunt, false);
    assert!(has(&s, "Round 1") && has(&s, "Score"), "{s:?}");
    for m in [&race, &hunt] {
        on_stage(&mode_screen_prims(&ModeScreen::Playing {
            mode: m,
            log: &[],
            quit_prompt: false,
        }));
    }
}

#[test]
fn quit_prompt_and_results_read() {
    let m = build("builtin-golf");
    let s = strings(&m, true);
    assert!(has(&s, "Leave this game?"));
    let rows = [
        Placement {
            seat: 1,
            place: 1,
            score: 5,
        },
        Placement {
            seat: 0,
            place: 2,
            score: 7,
        },
    ];
    let granted = ["+40 coins".to_string()];
    let s = mode_screen_strings(&ModeScreen::Results {
        name: "Practice Green",
        kind: ModeKind::Golf,
        rows: &rows,
        granted: &granted,
        completed: true,
        local_seat: 0,
        seat_names: &[],
        over_world: false,
        holding: false,
        banner: None,
        names: &[],
    });
    for needle in ["Game over", "1st", "2nd", "7 strokes", "+40 coins"] {
        assert!(has(&s, needle), "`{needle}` missing from {s:?}");
    }
}

#[test]
fn texts_lay_out_through_the_font() {
    let m = build("builtin-duel");
    let font = legaia_font::synthetic_for_tests();
    let screen = ModeScreen::Playing {
        mode: &m,
        log: &[],
        quit_prompt: false,
    };
    let draws = mode_screen_texts(&font, &screen);
    assert!(!draws.is_empty());
    // The synthetic font's glyphs run wider than the disc font's, so only
    // the rows are checked against the stage.
    for d in &draws {
        assert!(d.dst.1 >= 0 && d.dst.1 < STAGE_H, "{:?}", d.dst);
    }
}

#[test]
fn a_networked_player_reads_their_own_seat_as_you() {
    let mut m = build("builtin-poker");
    m.local_seat = 1;
    let s = mode_screen_strings(&ModeScreen::Playing {
        mode: &m,
        log: &[],
        quit_prompt: false,
    });
    // Seat 1 is "You" along the near edge, so seat 0 sits on the far rail.
    assert!(has(&s, "Seat 1"), "{s:?}");
    assert!(!has(&s, "Seat 2"), "{s:?}");
}
