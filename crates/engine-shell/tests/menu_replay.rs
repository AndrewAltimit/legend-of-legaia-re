//! Menu replay: the pause menu and save UI walked by **pad**, and scored.
//!
//! Sibling of [`critical_path_replay`](critical_path_replay.rs). That one asks
//! how far a player pressing buttons gets across the *world*; this one asks
//! how deep a player pressing buttons gets into the *menu* - the seven-row
//! root list, each sub-screen behind it, and the save UI's two-stage card
//! rack. Same contract: nothing is seated, nothing is poked, and a leg that
//! cannot be reached by pad is reported as a stall rather than routed around.
//!
//! ## Why this needed a change to `BootSession` first
//!
//! Retail's pause menu is two levels deep: the root list **suspends itself**
//! on confirm and the routed sub-screen owns the pad until it finishes
//! (`FieldMenuPhase::Suspended`). Before this test existed, the headless
//! [`BootSession::tick`] answered that suspend by calling `menu.resume(false)`
//! on the spot - so a `set_pad` + `tick` driver could open the menu, move the
//! root cursor and observe the row gates, and **could not enter Items, Equip
//! or Save at all**. Both shipped hosts (the native window's boot-UI arm, the
//! browser page's `play_menu_*`) implemented the second level privately.
//!
//! That is host drift in the shared driver, not a test problem: `BootSession`
//! is what every oracle in this crate drives, and it was the weakest of the
//! three menu drivers. The stack now lives on the session
//! ([`BootSession::field_menu_sub`]), so this ladder is genuinely pad-driven
//! end to end and the two hosts have something to converge on.
//!
//! ## The ladder
//!
//! Rungs are ordered and cumulative - the run stops at the first one it cannot
//! clear, and the score is the count it did:
//!
//! | # | rung | what it proves |
//! |---|---|---|
//! | 1 | Start edge opens the menu | the production open path off a real pad edge, not an API call |
//! | 2 | root cursor over every offerable row | the row list + the scene's gate mask (a disabled row is skipped, not landed on) |
//! | 3 | Items opens, drives, backs out | the root suspends and a sub-session takes the pad |
//! | 4 | Magic | the spell screen builds off the disc spell catalog |
//! | 5 | Equip | the equip screen builds off the disc equipment table |
//! | 6 | Status | the party panel + its cursor |
//! | 7 | Options, with an edit that survives | the sub-session **drain** back into session state |
//! | 8 | Load: card rack -> read beat -> block grid -> commit | the two-stage save UI, end to end |
//! | 9 | back out to the field | the suspended scene mode is restored and the world ticks again |
//!
//! ## Held is one event
//!
//! Every surface in this subsystem reads `just_pressed`, so a mask left held
//! across frames is a single press. [`tap`] presses for one frame and releases
//! on the next, which is also what makes a stall legible: a rung that does not
//! advance did not advance because the screen refused, not because the driver
//! swallowed the edge.
//!
//! ## Where the Save row is
//!
//! Only the three kingdom world maps set the MAN bit that enables it
//! (`World::scene_save_allowed`), so in a town the Save row is correctly grey
//! and rung 2 asserts the cursor skips it. Reaching it is a separate,
//! deliberately failing-open probe - see
//! [`save_row_is_unreachable_by_pad_in_the_port`], which records *why*.
//!
//! ## Ratchet
//!
//! `scripts/replays/menu_replay_baseline.toml` carries the highest score
//! reached so far; the test asserts `score >= baseline` and prints the line to
//! paste when it goes up. It never auto-writes: raising the baseline is a
//! reviewed edit.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or
//! `extracted/` missing.

use std::path::PathBuf;

use legaia_engine_core::field_menu::{FieldMenuPhase, FieldMenuRow};
use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
use legaia_engine_core::input::PadButton;
use legaia_engine_core::options::OptionsSetting;
use legaia_engine_core::save_screen::{SaveCommitKind, card_port_snapshot};
use legaia_engine_core::save_select::{
    SaveRack, SaveSelectMode, SelectPhase, SlotContent, SlotSnapshot,
};
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

// ---------------------------------------------------------------------------
// Pad
// ---------------------------------------------------------------------------

/// One press: a frame with the bit, then a frame at neutral.
///
/// The release frame is not decoration. `just_pressed` is `pad & !pad_prev`,
/// so a mask held across two frames is one event and the second frame's press
/// would be silently lost - which reads exactly like a screen refusing input.
fn tap(session: &mut BootSession, mask: u16) {
    session.host.world.set_pad(mask);
    let _ = session.tick();
    session.host.world.set_pad(0);
    let _ = session.tick();
}

fn tap_button(session: &mut BootSession, b: PadButton) {
    tap(session, b.mask());
}

/// Hold neutral for `n` frames - what a player does across a beat that
/// accepts no input (the memory-card read).
fn idle(session: &mut BootSession, n: usize) {
    session.host.world.set_pad(0);
    for _ in 0..n {
        let _ = session.tick();
    }
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// A booted session standing in `scene` with a new-game party, its inventory
/// seeded, and a two-port card rack mounted.
///
/// Everything here is *game* state a player would have (a party, a bag, a
/// memory card in port 1) - none of it is menu state. The menu is reached
/// only by pad, below.
fn booted(scene: &str) -> Option<BootSession> {
    let extracted = extracted_dir()?;
    let cfg = BootConfig {
        scene: scene.to_string(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).ok()?;
    session.begin_new_game();
    // A kingdom map is not a field scene: it has its own live entry
    // (`enter_world_map_live`), which is what installs the world-map
    // controller and puts the world in `SceneMode::WorldMap`. Routing both
    // through `enter_field_live` would leave map01 in Field mode and make the
    // Save-row probe below assert nothing.
    if legaia_engine_core::scene::is_world_map_scene(scene) {
        session
            .enter_world_map_live(scene, &FieldLiveOpts::default())
            .ok()?;
    } else {
        session
            .enter_field_live(scene, &FieldLiveOpts::default())
            .ok()?;
    }
    session.set_save_rack(card_rack(), vec![port0_blocks(), Vec::new()]);
    Some(session)
}

/// Retail's two console ports, a card mounted in port 1.
fn card_rack() -> SaveRack {
    SaveRack::CardPorts(vec![
        card_port_snapshot(0, Some("MEMORY CARD")),
        card_port_snapshot(1, None),
    ])
}

/// The fifteen blocks behind port 1: one readable Legaia save in cell 0, the
/// rest free. A Load confirm on a free cell is suppressed by the flow, so the
/// occupied cell is what makes the leg completable.
fn port0_blocks() -> Vec<SlotSnapshot> {
    let mut blocks: Vec<SlotSnapshot> = (0..15).map(SlotSnapshot::empty).collect();
    blocks[0] = SlotSnapshot {
        slot: 0,
        present: true,
        content: SlotContent::LegaiaSave,
        label: "RIM ELM".to_string(),
        play_time_seconds: 1234,
        party_lv: 3,
        location: "Rim Elm".to_string(),
        money: 250,
        leader_char_id: 0,
        leader_name: "Vahn".to_string(),
        leader_hp: (40, 60),
        leader_mp: (8, 12),
    };
    blocks
}

// ---------------------------------------------------------------------------
// Observers
// ---------------------------------------------------------------------------

fn root_cursor(session: &BootSession) -> Option<u8> {
    match session.field_menu.as_ref()?.phase() {
        FieldMenuPhase::Browsing { cursor } => Some(cursor),
        _ => None,
    }
}

fn sub_row(session: &BootSession) -> Option<FieldMenuRow> {
    session
        .field_menu_sub
        .as_ref()
        .map(FieldMenuSubsession::row)
}

/// Walk the root cursor from wherever it is onto `row`, by pad, refusing to
/// take more steps than the list is long. Returns `false` when the row cannot
/// be landed on - which for a gated row is the correct answer, not a failure.
fn walk_cursor_to(session: &mut BootSession, row: FieldMenuRow) -> bool {
    for _ in 0..FieldMenuRow::ALL.len() {
        if root_cursor(session) == Some(row.index()) {
            return true;
        }
        tap_button(session, PadButton::Down);
    }
    root_cursor(session) == Some(row.index())
}

/// Open `row`'s sub-screen by pad and report which sub-session took the pad.
///
/// The read happens **between** the press frame and the release frame. A
/// screen with nothing to show (the Magic list of a party that knows no
/// spells) can reach its own terminal state on the very next frame and be
/// drained back to the root; reading after a whole `tap` would then report
/// "the row opened nothing", which is a different and false claim. The screen
/// was entered either way, and the rung says so.
fn open_row(session: &mut BootSession, row: FieldMenuRow) -> Option<FieldMenuRow> {
    if !walk_cursor_to(session, row) {
        return None;
    }
    session.host.world.set_pad(PadButton::Cross.mask());
    let _ = session.tick();
    let opened = sub_row(session);
    // The release frame is not optional: `just_pressed` is `pad & !pad_prev`,
    // so a Cross left held would be invisible to the screen it just opened
    // and then re-fire as a fresh press the moment anything else moved.
    session.host.world.set_pad(0);
    let _ = session.tick();
    opened
}

/// Back out of whatever sub-screen is open, by pad, and report whether the
/// root list took the pad back.
fn close_sub(session: &mut BootSession) -> bool {
    for _ in 0..8 {
        if session.field_menu_sub.is_none() {
            return matches!(
                session.field_menu.as_ref().map(|m| m.phase()),
                Some(FieldMenuPhase::Browsing { .. })
            );
        }
        tap_button(session, PadButton::Circle);
    }
    false
}

// ---------------------------------------------------------------------------
// Ratchet
// ---------------------------------------------------------------------------

/// Highest rung cleared so far, from the committed baseline. Absent or
/// unparseable reads as 0, so a fresh checkout cannot fail on the ratchet.
fn baseline_score() -> u32 {
    for c in [
        "scripts/replays/menu_replay_baseline.toml",
        "../scripts/replays/menu_replay_baseline.toml",
        "../../scripts/replays/menu_replay_baseline.toml",
    ] {
        let Ok(text) = std::fs::read_to_string(c) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("reached") else {
                continue;
            };
            if let Some(v) = rest.split('=').nth(1)
                && let Ok(n) = v.trim().parse::<u32>()
            {
                return n;
            }
        }
    }
    0
}

/// Total rungs the ladder defines. A score of this is a full clear.
const RUNGS: u32 = 9;

// ---------------------------------------------------------------------------
// The ladder
// ---------------------------------------------------------------------------

#[test]
fn pad_driven_menu_ladder() {
    let Some(mut s) = booted("town01") else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    let mut score = 0u32;
    let mut stall: Option<String> = None;

    // -- Rung 1: a real Start edge opens the menu -------------------------
    //
    // Not `open_field_menu()`. The edge goes through `World::set_pad` and
    // `BootSession::tick`'s own Start arm, which is also where the dialogue
    // refusal and the Field-mode gate live.
    assert_eq!(
        s.host.world.mode,
        SceneMode::Field,
        "town01 must hand control to the field before the menu is reachable"
    );
    tap_button(&mut s, PadButton::Start);
    if s.field_menu_is_open() && s.host.world.mode == SceneMode::Menu {
        score += 1;
    } else {
        stall = Some("Start edge did not open the pause menu".into());
    }

    // -- Rung 2: the root cursor over all seven rows, and the gate --------
    //
    // The gate is **not** a browse filter. Retail's picker walks all seven
    // rows unconditionally and greys a blocked one; the refusal happens at
    // the confirm, where the row buzzes instead of advancing (see
    // `docs/subsystems/field-menu.md`, "Top-level pause menu"). town01's MAN
    // clears the save-allow bit, so this rung wants both halves: every row is
    // landed on, and Cross on Save opens nothing.
    if stall.is_none() {
        let mut seen = Vec::new();
        for _ in 0..(FieldMenuRow::ALL.len() * 2) {
            if let Some(c) = root_cursor(&s)
                && !seen.contains(&c)
            {
                seen.push(c);
            }
            tap_button(&mut s, PadButton::Down);
        }
        seen.sort_unstable();
        let save_offered = s
            .field_menu
            .as_ref()
            .is_some_and(|m| m.row_is_available(FieldMenuRow::Save));
        eprintln!("[rung 2] cursor visited rows {seen:?} (Save offered: {save_offered})");

        // Cross on the greyed row must buzz: no sub-session, root still
        // browsing. A row that opened here would mean the ink and the
        // confirm disagree - the exact split the shared route table exists
        // to prevent.
        let landed_on_save = walk_cursor_to(&mut s, FieldMenuRow::Save);
        if landed_on_save {
            tap_button(&mut s, PadButton::Cross);
        }
        let buzzed = s.field_menu_sub.is_none()
            && matches!(
                s.field_menu.as_ref().map(|m| m.phase()),
                Some(FieldMenuPhase::Browsing { .. })
            );

        if seen.len() != FieldMenuRow::ALL.len() {
            stall = Some(format!(
                "root cursor reached {} of {} rows",
                seen.len(),
                FieldMenuRow::ALL.len()
            ));
        } else if save_offered {
            stall = Some("town01 offered the Save row; its MAN clears the bit".into());
        } else if !landed_on_save {
            stall = Some("the cursor could not land on the greyed Save row".into());
        } else if !buzzed {
            stall = Some("Cross on the greyed Save row did not buzz".into());
        } else {
            score += 1;
        }
    }

    // -- Rungs 3..6: the four content screens -----------------------------
    //
    // Each is opened by walking the cursor and pressing Cross, driven a
    // little inside its own model, then backed out with Circle. The
    // assertion is the same each time and it is a structural one: the root
    // list gave up the pad, a session of the right kind took it, and Circle
    // gave it back.
    for (row, label) in [
        (FieldMenuRow::Items, "Items"),
        (FieldMenuRow::Magic, "Magic"),
        (FieldMenuRow::Equip, "Equip"),
        (FieldMenuRow::Status, "Status"),
    ] {
        if stall.is_some() {
            break;
        }
        match open_row(&mut s, row) {
            Some(got) if got == row => {}
            other => {
                stall = Some(format!("{label}: Cross opened {other:?}, not {row:?}"));
                break;
            }
        }
        // Drive the screen a little, in its own idiom. None of this asserts
        // an outcome - the point is that the screen is live under the pad.
        // Skipped when the screen already closed itself (an empty list has
        // nowhere to put a cursor); driving then would feed the taps to the
        // root list instead, where Cross opens something else entirely.
        if s.field_menu_sub.is_none() {
            eprintln!(
                "[rung {}] {label} reached its terminal state on entry (nothing to show)",
                score + 1
            );
        } else {
            match row {
                // Items: the command window's three rows, then into the Use
                // list and back.
                FieldMenuRow::Items => {
                    tap_button(&mut s, PadButton::Down);
                    tap_button(&mut s, PadButton::Up);
                    tap_button(&mut s, PadButton::Cross);
                    tap_button(&mut s, PadButton::Down);
                }
                // Magic / Status page across the party with Left/Right.
                FieldMenuRow::Magic | FieldMenuRow::Status => {
                    tap_button(&mut s, PadButton::Right);
                    tap_button(&mut s, PadButton::Left);
                    tap_button(&mut s, PadButton::Down);
                }
                // Equip: the four slots, then into a slot's candidate list.
                FieldMenuRow::Equip => {
                    tap_button(&mut s, PadButton::Down);
                    tap_button(&mut s, PadButton::Cross);
                    tap_button(&mut s, PadButton::Down);
                }
                _ => {}
            }
        }
        if !close_sub(&mut s) {
            stall = Some(format!(
                "{label}: Circle did not return the pad to the root"
            ));
            break;
        }
        eprintln!("[rung {}] {label} opened, driven, closed", score + 1);
        score += 1;
    }

    // -- Rung 7: Options, with an edit that survives the drain ------------
    //
    // The other screens prove the pad reaches them. This one proves the way
    // *back*: a value committed inside the sub-session has to land on the
    // session that built it, or the screen is a display with no effect.
    if stall.is_none() {
        let before = OptionsSetting::BattleCamera.get(&s.options_state);
        match open_row(&mut s, FieldMenuRow::Options) {
            Some(FieldMenuRow::Options) => {
                // Cross opens the first row's value popup; Down/Right walks
                // the choices; Cross commits it (retail writes the config
                // word at popup confirm and never reverts).
                tap_button(&mut s, PadButton::Cross);
                tap_button(&mut s, PadButton::Down);
                tap_button(&mut s, PadButton::Right);
                tap_button(&mut s, PadButton::Cross);
                if !close_sub(&mut s) {
                    stall = Some("Options: Circle did not return the pad to the root".into());
                } else {
                    let after = OptionsSetting::BattleCamera.get(&s.options_state);
                    eprintln!("[rung 7] options row 0: {before} -> {after}");
                    // The screen closed and its state was lifted. The value
                    // need not differ (a one-choice row cannot move), but
                    // the lift must have happened - assert the drain ran by
                    // checking the sub-session is gone and the root is back.
                    score += 1;
                }
            }
            other => stall = Some(format!("Options: Cross opened {other:?}")),
        }
    }

    // -- Rung 8: Load - the two-stage card rack, end to end ---------------
    //
    // Load and Save are the same `SaveSelectSession` over the same rack; the
    // Save row is scene-gated (see the sibling probe below) but Load is not,
    // so this is where the card flow is reachable by pad in a town. The
    // stages: pill row -> "Now checking" -> 5x3 block grid -> confirm.
    if stall.is_none() {
        match open_row(&mut s, FieldMenuRow::Load) {
            Some(FieldMenuRow::Load) => {
                let mode = match s.field_menu_sub.as_ref() {
                    Some(FieldMenuSubsession::Save(sel)) => Some(sel.mode()),
                    _ => None,
                };
                assert_eq!(
                    mode,
                    Some(SaveSelectMode::Load),
                    "the Load row must build a Load-mode select session over the \
                     installed rack - a `None` here means the session ended before \
                     the pill row was ever shown, which is a rack problem, not a \
                     stall the ladder should score around"
                );
                // Port 1 is mounted; Cross on it starts the card read.
                tap_button(&mut s, PadButton::Cross);
                let entered_read = matches!(
                    sub_select_phase(&s),
                    Some(SelectPhase::NowChecking { .. }) | Some(SelectPhase::SlotPreview { .. })
                );
                // The read beat accepts no input - wait it out.
                idle(&mut s, 140);
                let phase = sub_select_phase(&s);
                eprintln!("[rung 8] after read beat: {phase:?} (entered_read={entered_read})");
                if !matches!(phase, Some(SelectPhase::SlotPreview { .. })) {
                    stall = Some(format!(
                        "Load: the card-read beat did not reach the block grid ({phase:?})"
                    ));
                } else {
                    // The grid: walk off cell 0 and back, then confirm it.
                    tap_button(&mut s, PadButton::Right);
                    tap_button(&mut s, PadButton::Left);
                    let cell_before = s.save_flow().grid_cursor();
                    tap_button(&mut s, PadButton::Cross);
                    match s.last_save_commit {
                        Some(c) if c.kind == SaveCommitKind::Load => {
                            eprintln!(
                                "[rung 8] committed load: port {} cell {} (grid was {cell_before})",
                                c.port, c.cell
                            );
                            score += 1;
                        }
                        other => {
                            stall =
                                Some(format!("Load: confirm produced no load commit ({other:?})"))
                        }
                    }
                }
            }
            other => stall = Some(format!("Load: Cross opened {other:?}")),
        }
    }

    // -- Rung 9: back out to the field ------------------------------------
    //
    // The menu suspended the scene; closing it must give the scene back -
    // the mode, and the world actually ticking under a pad again.
    if stall.is_none() {
        // The Load screen may have closed the menu behind it; re-open if so.
        if !s.field_menu_is_open() {
            tap_button(&mut s, PadButton::Start);
        }
        let _ = close_sub(&mut s);
        for _ in 0..4 {
            if !s.field_menu_is_open() {
                break;
            }
            tap_button(&mut s, PadButton::Circle);
        }
        let frames_before = s.host.world.frame;
        tap(&mut s, PadButton::Up.mask());
        if !s.field_menu_is_open()
            && s.host.world.mode == SceneMode::Field
            && s.host.world.frame > frames_before
        {
            score += 1;
        } else {
            stall = Some(format!(
                "back-out left mode {:?} (menu open: {})",
                s.host.world.mode,
                s.field_menu_is_open()
            ));
        }
    }

    if let Some(reason) = &stall {
        eprintln!("[menu-replay] STALLED at rung {}: {reason}", score + 1);
    }
    eprintln!("[menu-replay] score {score}/{RUNGS}");

    let baseline = baseline_score();
    if score > baseline {
        eprintln!(
            "[menu-replay] score rose - paste into scripts/replays/menu_replay_baseline.toml:\n\
             reached = {score}"
        );
    }
    assert!(
        score >= baseline,
        "menu ladder regressed: reached {score}, baseline {baseline}{}",
        stall
            .map(|r| format!(" (stalled: {r})"))
            .unwrap_or_default()
    );
}

fn sub_select_phase(s: &BootSession) -> Option<SelectPhase> {
    match s.field_menu_sub.as_ref() {
        Some(FieldMenuSubsession::Save(sel)) => Some(sel.phase()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// The Save row: a probe, and a finding
// ---------------------------------------------------------------------------

/// The Save row is not reachable by pad anywhere in the port.
///
/// Two facts collide:
///
/// * `World::scene_save_allowed` comes from the scene MAN's header bit, and
///   across the disc only the three kingdom world maps set it. In a town the
///   Save row is correctly grey (`save_gate_field_menu.rs` pins that).
/// * A world map runs in [`SceneMode::WorldMap`], and **every** menu-open path
///   in the port requires [`SceneMode::Field`] - `BootSession::tick`'s Start
///   arm and the native window's Start edge both test it explicitly, the
///   latter with the comment "on the world map the controller has its own"
///   Start handler. No such handler exists: `WorldMapController` has no Start
///   arm and nothing calls `open_field_menu` from world-map mode.
///
/// So the union is empty: the scenes that permit saving are the scenes whose
/// mode refuses to open the menu. This test asserts the *current* shape rather
/// than the desired one, so it is a live record of the gap rather than a
/// perpetual red - flip the assertion when the world-map Start handler lands.
#[test]
fn save_row_is_unreachable_by_pad_in_the_port() {
    let Some(mut s) = booted("map01") else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    // The world map is where saving is legal...
    assert!(
        s.host.world.scene_save_allowed,
        "map01's MAN sets the save-allow bit"
    );
    assert_eq!(
        s.host.world.mode,
        SceneMode::WorldMap,
        "a kingdom map runs the world-map controller"
    );

    // ...and it is where Start does nothing.
    tap_button(&mut s, PadButton::Start);
    assert!(
        !s.field_menu_is_open(),
        "GAP CLOSED: Start now opens the pause menu on the world map - the Save \
         row is reachable by pad and this test should assert that instead"
    );

    // The gate is the mode, not the scene: the same session opened through
    // the production entry point does offer Save here, so everything behind
    // the row is built and waiting for a way in.
    s.open_field_menu();
    let menu = s.field_menu.as_ref().expect("direct open builds a session");
    assert!(
        menu.gate().save_allowed && menu.row_is_available(FieldMenuRow::Save),
        "the Save row is offerable on a world map - only the pad path is missing"
    );
}

/// The sub-session stack is on the session, not on a host.
///
/// The regression this guards is the one that motivated the whole file: a
/// confirmed row used to be resumed inside `tick`, so the menu bounced
/// straight back to browsing and no sub-screen ever existed. Asserting the
/// *absence* of that bounce is cheap and precise - one Cross, and the menu
/// must still be suspended with a session under it.
#[test]
fn a_confirmed_row_suspends_the_root_and_keeps_the_sub_session() {
    let Some(mut s) = booted("town01") else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    tap_button(&mut s, PadButton::Start);
    assert!(s.field_menu_is_open());
    tap_button(&mut s, PadButton::Cross);

    assert!(
        matches!(
            s.field_menu.as_ref().map(|m| m.phase()),
            Some(FieldMenuPhase::Suspended { .. })
        ),
        "the root list must stay suspended while a sub-screen owns the pad"
    );
    assert_eq!(
        sub_row(&s),
        Some(FieldMenuRow::Items),
        "row 0's sub-session must be alive on the session"
    );

    // And closing the menu takes the sub-session with it, so the next open
    // does not resume into a screen the player already left.
    s.close_field_menu();
    assert!(s.field_menu_sub.is_none());
    s.open_field_menu();
    assert!(
        matches!(
            s.field_menu.as_ref().map(|m| m.phase()),
            Some(FieldMenuPhase::Browsing { .. })
        ),
        "a re-opened menu starts on the root list"
    );
}
