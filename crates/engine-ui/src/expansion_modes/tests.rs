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
        has(&s, "Choose a card") || has(&s, "Rival is thinking"),
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

/// A table NPC's name longer than any plate: every name box must cut it.
const LONG: &str = "The Warden's Second, Keeper of the Mist Lantern";

/// `id` with long names in every seat but the player's.
fn long_named(id: &str) -> ActiveMode {
    let mut m = build(id);
    m.seat_names = (0..8)
        .map(|i| {
            if i == 0 {
                String::new()
            } else {
                format!("{LONG} {i}")
            }
        })
        .collect();
    m
}

/// The in-world HUD of `id` at frames across a session, with the player
/// pressing Cross every few frames so the game moves through its phases
/// (bets and folds, card picks, strokes, laps), and a long banner and log
/// line up.
fn in_world_frames(id: &str) -> Vec<Canvas> {
    in_world_frames_named(id, true)
}

/// [`in_world_frames`], with the long names, banner and log line or with
/// the mode's own short ones.
fn in_world_frames_named(id: &str, long: bool) -> Vec<Canvas> {
    let mut m = if long { long_named(id) } else { build(id) };
    let log = [if long {
        format!("{LONG} raises the pot by 4900 chips")
    } else {
        "Seat 2 raises to 200".to_string()
    }];
    let banner = if long {
        format!("{LONG} wins 12000 - Full house")
    } else {
        "Seat 3 wins 450 - Two pair".to_string()
    };
    let mut out = Vec::new();
    for f in 0..1500u32 {
        // Cross on a beat, Right now and then (the poker menu's raise, the
        // duel's next card, the golf aim).
        let pad = match f % 23 {
            0 => 0x4000,
            7 => 0x2000,
            _ => 0,
        };
        m.tick(&[pad]);
        if f % 50 == 0 {
            for b in [None, Some(banner.as_str())] {
                out.push(compose(&ModeScreen::InWorld {
                    mode: &m,
                    log: &log,
                    quit_prompt: false,
                    banner: b,
                    outro: false,
                }));
            }
        }
    }
    out
}

const IN_WORLD_MODES: [&str; 4] = [
    "builtin-poker",
    "builtin-duel",
    "builtin-golf",
    "builtin-race",
];

/// Every in-world HUD, at every frame sampled: each prim inside the
/// stage, each label held to a box inside the stage, and no two labels on
/// one row sharing pixels - so nothing runs off the screen at any host
/// size (the hosts scale the 320x240 stage as one) and no stack runs into
/// its bet, no name into its score.
#[test]
fn in_world_huds_keep_every_box_on_the_stage_and_apart() {
    for id in IN_WORLD_MODES {
        let frames = in_world_frames(id);
        // Not vacuous: every mode's HUD draws labels and a long name.
        assert!(
            frames
                .iter()
                .any(|c| c.labels.iter().any(|l| l.text.contains("Warden"))),
            "{id}: no long name on its HUD"
        );
        for c in frames {
            on_stage(&c.prims);
            for p in &c.prims {
                for (x, y) in p.corners() {
                    assert!(
                        (0..=STAGE_W as i16).contains(&x) && (0..=STAGE_H as i16).contains(&y),
                        "{id}: prim corner ({x}, {y}) off the stage"
                    );
                }
            }
            for l in &c.labels {
                let Some((a, b)) = l.span() else {
                    panic!("{id}: `{}` has no box", l.text);
                };
                assert!(
                    a >= 0 && b <= STAGE_W && l.y >= 0 && l.y + LINE <= STAGE_H,
                    "{id}: `{}` box {a}..{b} at row {} leaves the stage",
                    l.text,
                    l.y
                );
            }
            for (i, l) in c.labels.iter().enumerate() {
                for k in &c.labels[i + 1..] {
                    if (l.y - k.y).abs() >= LINE - 2 {
                        continue;
                    }
                    let ((a0, a1), (b0, b1)) = (l.span().unwrap(), k.span().unwrap());
                    // A banner's box sits over the HUD on purpose only when
                    // it does not share a row with a label under it.
                    assert!(
                        a1 <= b0 || b1 <= a0,
                        "{id}: `{}` ({a0}..{a1}) and `{}` ({b0}..{b1}) share row {}",
                        l.text,
                        k.text,
                        l.y
                    );
                }
            }
        }
    }
}

/// Every label's glyphs land inside its box once set in a font: a long
/// name is cut and marked `..`, a short one drawn whole. The synthetic
/// font's glyphs run wider than the dialog font's, so a fit here is a fit
/// on the disc font too.
#[test]
fn in_world_hud_text_fits_its_box_in_the_font() {
    let font = legaia_font::synthetic_for_tests();
    let mut cut = 0;
    for id in IN_WORLD_MODES {
        for c in in_world_frames(id) {
            for l in &c.labels {
                let (a, b) = l.span().unwrap();
                let draws = labels_to_draws(&font, std::slice::from_ref(l));
                // A glyph quad is its whole atlas cell; its pen is what the
                // box holds (the advance is checked below).
                for d in &draws {
                    let x = d.dst.0;
                    assert!(
                        x >= a - 1 && x < b,
                        "{id}: `{}` glyph pen {x} outside {a}..{b}",
                        l.text
                    );
                }
                let shown = fit_text(&font, &l.text, b - a);
                assert!(
                    font.layout_ascii(&shown).advance_x as i32 <= b - a,
                    "{id}: `{shown}` wider than {}",
                    b - a
                );
                if shown != l.text {
                    assert!(shown.ends_with(".."), "{id}: `{shown}`");
                    cut += 1;
                }
            }
        }
    }
    assert!(cut > 0, "the long names were never cut");
}

#[test]
fn fit_text_keeps_what_fits_and_marks_the_cut() {
    let font = legaia_font::synthetic_for_tests();
    let w = |s: &str| font.layout_ascii(s).advance_x as i32;
    assert_eq!(fit_text(&font, "Odile", 200), "Odile");
    let cut = fit_text(&font, "The Warden's Second", 60);
    assert!(cut.ends_with("..") && w(&cut) <= 60, "{cut}");
    assert!(cut.len() > 2, "{cut}");
    // No room even for the mark: nothing.
    assert_eq!(fit_text(&font, "Brann", 2), "");
}

/// The poker plates set a seat's stack and its street bet on separate rows,
/// so "4900" and "Bet 100" never meet.
#[test]
fn a_poker_plate_keeps_the_stack_and_bet_on_their_own_rows() {
    let mut m = long_named("builtin-poker");
    let mut seen = false;
    for _ in 0..2000 {
        m.tick(&[0]);
        let ModeView::Poker(v) = m.view() else {
            unreachable!()
        };
        if !v
            .seats
            .iter()
            .skip(1)
            .any(|s| s.street_bet > 0 && !s.folded)
        {
            continue;
        }
        let c = compose(&ModeScreen::InWorld {
            mode: &m,
            log: &[],
            quit_prompt: false,
            banner: None,
            outro: false,
        });
        let bet = c
            .labels
            .iter()
            .find(|l| l.text.starts_with("Bet ") && l.y < 60)
            .expect("an opponent's bet row");
        assert!(
            !c.labels
                .iter()
                .any(|l| l.y == bet.y && l.text.chars().all(|ch| ch.is_ascii_digit())),
            "a stack shares the bet's row"
        );
        assert!(!c.labels.iter().any(|l| l.text.starts_with('+')));
        seen = true;
        break;
    }
    assert!(seen, "no opponent ever bet");
}

/// With the dialog font off the extracted disc data (skipped without it):
/// at the modes' own names nothing the HUDs say is cut - the prompts,
/// the control hints, a card's four strengths and the stacks all fit their
/// boxes whole; only a name longer than its plate is ever shortened.
#[test]
fn in_world_hud_text_reads_whole_in_the_dialog_font() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    let Ok(font) = legaia_font::Font::load_from_extracted(&root) else {
        eprintln!("skip: no extracted dialog font");
        return;
    };
    for id in IN_WORLD_MODES {
        for c in in_world_frames_named(id, false) {
            for l in &c.labels {
                let (a, b) = l.span().unwrap();
                let shown = fit_text(&font, &l.text, b - a);
                assert_eq!(shown, l.text, "{id}: `{}` cut to fit {}px", l.text, b - a);
            }
        }
    }
}
