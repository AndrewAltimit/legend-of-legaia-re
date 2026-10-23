//! The boot front end, driven headless: publisher logos, the title card, and
//! the title's two ways into the game - New Game (through the opening and
//! the naming prompt to free roam) and Continue (through the card manager's
//! save-select to a library card save).
//!
//! Both play hosts step the same object
//! ([`legaia_engine_session::FrontEnd`]); before it lived in
//! `engine-session` the chain existed only inside the native window's boot
//! UI and the browser page's exports, so no headless run ever crossed it.
//! The soak reaches it two ways:
//!
//! - **Run labels.** `title` starts a run on a cold boot and picks New
//!   Game; `title+continue` (optionally `@<save>` / `@<card>:<save>`) picks
//!   Continue and loads that save off the library card through the retail
//!   save-select. The front-end leg is scripted from the run's state, not
//!   drawn from the policy, so a replay of the run's recorded pads lands in
//!   the same place.
//! - **Deterministic tests.** [`front_end_new_game_reaches_free_roam`] and
//!   [`front_end_continue_lands_a_library_save`] assert each chain end to
//!   end.

use super::*;
use legaia_engine_menus::save_screen::SaveScreenFlow;
use legaia_engine_menus::save_select::{SaveSelectSession, SelectPhase};
use legaia_engine_session::{ContinueScreen, ContinueStep, FrontEnd, FrontEndStep, NoMovieLane};
use legaia_engine_title::title::{TitleOutcome, TitlePhase};
use legaia_save::emu::MountedCard;

const START: u16 = 0x0008;
const UP: u16 = 0x0010;
const RIGHT: u16 = 0x0020;
const DOWN: u16 = 0x0040;
const LEFT: u16 = 0x0080;
const CROSS: u16 = 0x4000;

/// Frames the scripted front-end leg may take (logos + fade + menu, or the
/// save-select's card read and confirm beats). Generous: a bound, not a claim.
const FRONT_END_BUDGET: u32 = 6_000;

/// The front-end runs a full soak carries beside its scene runs.
pub(super) const FRONT_END_RUNS: [&str; 2] = ["title", "title+continue"];

/// Which way a front-end run leaves the title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FrontEndRoute {
    NewGame,
    /// Continue, loading the named save (`None`: the card's first save).
    Continue(Option<String>),
}

/// `"title"` -> New Game, `"title+continue[@save]"` -> Continue; anything
/// else is not a front-end run.
pub(super) fn split_front_end(label: &str) -> Option<FrontEndRoute> {
    let (base, save) = split_save(label);
    match base {
        "title" => Some(FrontEndRoute::NewGame),
        "title+continue" => Some(FrontEndRoute::Continue(save.map(str::to_string))),
        _ => None,
    }
}

/// The card the Continue route mounts in port 1, and the block cell of the
/// save it loads. The library is the harness's own `saves_library`.
pub(super) fn continue_card(save: Option<&str>) -> Result<(MountedCard, u8), String> {
    let lib = saves_library().ok_or_else(|| {
        format!("{NO_SAVE_LIBRARY}: set LEGAIA_SAVES_LIBRARY or check out saves/library")
    })?;
    let (card, save) = match save.and_then(|s| s.split_once(':')) {
        Some((card, save)) => (card.to_string(), Some(save)),
        None => (
            std::env::var("LEGAIA_SOAK_CARD").unwrap_or_else(|_| SOAK_CARD.to_string()),
            save,
        ),
    };
    let path = lib.join("cards").join(&card);
    let mounted = MountedCard::open(&path).map_err(|e| format!("{}: {e:#}", path.display()))?;
    for block in 1..=15u8 {
        let Some(frame) = mounted.dir_frame(block) else {
            continue;
        };
        let name: String = frame[0x0A..0x0A + 20]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();
        let wanted = save.is_none_or(|s| name.ends_with(s));
        if wanted && mounted.save_at(block - 1).is_some() {
            return Ok((mounted, block - 1));
        }
    }
    Err(format!("{card}: no save named {save:?}"))
}

/// The scripted pad for the title: Start through the logos and the Press
/// Start prompt, then the cursor onto `row` and Cross. One edge every four
/// frames, as a player taps.
fn title_pad(fe: &FrontEnd, frame: u32, row: u8) -> u16 {
    if !frame.is_multiple_of(4) {
        return 0;
    }
    match fe.title_session().map(|t| t.phase()) {
        None => START,
        Some(TitlePhase::PressStart { .. }) => START,
        Some(TitlePhase::MainMenu { cursor }) if cursor < row => DOWN,
        Some(TitlePhase::MainMenu { cursor }) if cursor > row => UP,
        Some(TitlePhase::MainMenu { .. }) => CROSS,
        Some(_) => 0,
    }
}

/// Cold-boot the front end (logos first) and step it with the scripted pad
/// until the title resolves. `Ok(frames)` when it resolved to `want`.
pub(super) fn drive_title(
    session: &mut NativeSession,
    want: TitleOutcome,
    any_save: bool,
) -> Result<u32, String> {
    let row = match want {
        TitleOutcome::NewGame => 0,
        TitleOutcome::Continue => 1,
        TitleOutcome::Options => 2,
    };
    let mut fe = FrontEnd::cold_boot(&session.mode_seat, true, any_save);
    let mut opened = false;
    for frame in 0..FRONT_END_BUDGET {
        let pad = title_pad(&fe, frame, row);
        match session.step_front_end(&mut fe, pad, || any_save, &mut NoMovieLane) {
            FrontEndStep::TitleOpened(mode) => {
                opened = true;
                if mode != legaia_engine_mode::mode::GameMode::CardInit {
                    return Err(format!("the logos handed off to {mode:?}, not CARD INIT"));
                }
            }
            FrontEndStep::Picked(o) if o == want => {
                if !opened {
                    return Err("the title resolved without the logos handing off".into());
                }
                return Ok(frame + 1);
            }
            FrontEndStep::Picked(o) => return Err(format!("the title picked {o:?}, not {want:?}")),
            _ => {}
        }
    }
    Err(format!(
        "the title did not resolve in {FRONT_END_BUDGET} frames"
    ))
}

/// The scripted pad for the save-select: Cross onto the card port, then the
/// grid cursor onto `cell` and Cross, then Cross through the confirm.
fn select_pad(session: &SaveSelectSession, flow: &SaveScreenFlow, frame: u32, cell: u8) -> u16 {
    if !frame.is_multiple_of(4) {
        return 0;
    }
    match session.phase() {
        SelectPhase::SlotPreview { .. } => {
            let cur = flow.grid_cursor();
            let (cc, cr, tc, tr) = (cur % 5, cur / 5, cell % 5, cell / 5);
            if cc < tc {
                RIGHT
            } else if cc > tc {
                LEFT
            } else if cr < tr {
                DOWN
            } else if cr > tr {
                UP
            } else {
                CROSS
            }
        }
        // A Yes / No prompt: onto Yes (row 0), then confirm.
        SelectPhase::ConfirmOverwrite { cursor, .. }
        | SelectPhase::ConfirmDelete { cursor, .. }
            if cursor != 0 =>
        {
            UP
        }
        _ => CROSS,
    }
}

/// Continue from the title: the session's one Continue screen
/// ([`ContinueScreen`], both play hosts' owner) over ports whose port 1
/// holds `card`, then the loaded block resumed through
/// `BootSession::resume_save`. `Ok((landing scene, frames))`.
pub(super) fn drive_continue(
    session: &mut NativeSession,
    card: &MountedCard,
    cell: u8,
    opts: &FieldLiveOpts,
) -> Result<(String, u32), String> {
    let ports = [Some(card), None];
    let mut screen = ContinueScreen::open(ports);
    for frame in 0..FRONT_END_BUDGET {
        let pad = select_pad(screen.session(), screen.flow(), frame, cell);
        let load = match screen.step(pad, ports) {
            ContinueStep::Running { .. } => continue,
            ContinueStep::BackedOut { .. } => {
                return Err("the save-select cancelled back to the title".into());
            }
            ContinueStep::Loaded { load, .. } => load,
        };
        if load.port != 0 || load.block != cell + 1 {
            return Err(format!(
                "loaded port {} block {}, wanted port 1 block {}",
                load.port + 1,
                load.block,
                cell + 1
            ));
        }
        let landing = session.resume_save(load.save, &load.scene, opts);
        let scene = landing
            .scene()
            .ok_or_else(|| format!("the resume landed {}", landing.kind()))?
            .to_string();
        return Ok((scene, frame + 1));
    }
    Err(format!(
        "the save-select did not commit in {FRONT_END_BUDGET} frames (phase {:?})",
        screen.session().phase()
    ))
}

/// A front-end run's entry leg, in place of the plain scene entry: drive the
/// title to its outcome, then New Game into the opening or Continue into the
/// save. Returns the scene the run starts in.
pub(super) fn enter_through_front_end(
    session: &mut NativeSession,
    route: &FrontEndRoute,
    opts: &FieldLiveOpts,
) -> Result<String, String> {
    match route {
        FrontEndRoute::NewGame => {
            drive_title(session, TitleOutcome::NewGame, false)?;
            session
                .start_new_game(opts)
                .map(str::to_string)
                .ok_or_else(|| "no opening scene would enter".to_string())
        }
        FrontEndRoute::Continue(save) => {
            let (card, cell) = continue_card(save.as_deref())?;
            drive_title(session, TitleOutcome::Continue, true)?;
            drive_continue(session, &card, cell, opts).map(|(scene, _)| scene)
        }
    }
}

// ---------------------------------------------------------------------------
// Deterministic tests
// ---------------------------------------------------------------------------

fn opts() -> FieldLiveOpts {
    FieldLiveOpts {
        live_loop: true,
        player_battle: true,
        battle_bgm: None,
    }
}

/// The opening's player-facing pad: the naming prompt's retail confirm
/// (Cross opens the Yes / No on No, Up to Yes, Cross commits - the template
/// name), and a Cross every other pair of frames otherwise, as a player
/// mashes through the elder's lines.
fn opening_pad(session: &NativeSession, frame: u64, naming_step: &mut u8) -> u16 {
    if session.host.world.name_entry_active() {
        if !frame.is_multiple_of(4) {
            return 0;
        }
        let pad = [CROSS, UP, CROSS][(*naming_step as usize).min(2)];
        *naming_step = naming_step.saturating_add(1);
        return pad;
    }
    if frame % 4 < 2 { CROSS } else { 0 }
}

/// Cold boot -> logos -> title -> New Game -> the prologue -> `town01`'s
/// establishing sweep -> the naming prompt -> committed -> free roam in
/// `town01`, all through the shared session kernels and the session's own
/// tick (which owns the naming prompt).
#[test]
fn front_end_new_game_reaches_free_roam() {
    let Some(src) = source() else { return };
    install_panic_hook();
    let mut session = open_session(&src);
    let title_frames =
        drive_title(&mut session, TitleOutcome::NewGame, false).expect("title -> New Game");
    let first = session
        .start_new_game(&opts())
        .expect("New Game enters an opening scene");
    eprintln!("[front-end] title resolved in {title_frames} frames; entered '{first}'");
    assert_eq!(
        first,
        legaia_game_tables::new_game::OPENING_CUTSCENE_SCENE,
        "New Game enters the prologue cutscene scene first"
    );

    let mut naming_opened = None;
    let mut naming_done = None;
    let mut free_at = None;
    let mut naming_step = 0u8;
    for frame in 0..30_000u64 {
        let pad = opening_pad(&session, frame, &mut naming_step);
        session.host.world.set_pad(pad);
        let was_naming = session.host.world.name_entry_active();
        session.tick().expect("tick");
        host_drains(&mut session);
        if session.host.world.active_fmv().is_some() {
            session.host.world.finish_cutscene();
            let _ = session.apply_pending_fmv_handoff();
        }
        let w = &session.host.world;
        if naming_opened.is_none() && w.name_entry_active() {
            naming_opened = Some(frame);
        }
        if was_naming && !w.name_entry_active() {
            naming_done = Some(frame);
        }
        if naming_done.is_some()
            && w.active_scene_label == legaia_game_tables::new_game::OPENING_SCENE
            && w.mode == SceneMode::Field
            && !w.cutscene_timeline_active()
            && w.dialog.current.is_none()
            && w.dialog.inline.is_none()
        {
            free_at = Some(frame);
            break;
        }
    }
    let w = &session.host.world;
    eprintln!(
        "[front-end] naming opened {naming_opened:?}, committed {naming_done:?}, free roam {free_at:?}; \
         scene '{}' mode {:?}",
        w.active_scene_label, w.mode
    );
    assert!(
        naming_opened.is_some(),
        "the opening reaches the naming prompt"
    );
    assert!(naming_done.is_some(), "the scripted confirm commits a name");
    assert!(
        !w.party_name(0).is_empty(),
        "the committed name lands in the lead's record"
    );
    assert!(
        free_at.is_some(),
        "the opening hands the controls back in {}",
        legaia_game_tables::new_game::OPENING_SCENE
    );
}

/// Cold boot -> logos -> title (Continue live off the mounted card) ->
/// Continue -> the retail save-select over the library card -> the save's
/// block -> resumed into the save's own scene, which then ticks.
#[test]
fn front_end_continue_lands_a_library_save() {
    let Some(src) = source() else { return };
    let (card, cell) = match continue_card(None) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[skip] front_end_continue: {e}");
            return;
        }
    };
    install_panic_hook();
    let mut session = open_session(&src);
    let title_frames =
        drive_title(&mut session, TitleOutcome::Continue, true).expect("title -> Continue");
    let (expected_sf, expected_resume) = card.save_at(cell).expect("the picked block holds a save");
    let (scene, select_frames) =
        drive_continue(&mut session, &card, cell, &opts()).expect("save-select -> Load");
    eprintln!(
        "[front-end] title {title_frames} frames, save-select {select_frames} frames; \
         block {} resumed into '{scene}' (save names '{}')",
        cell + 1,
        expected_resume.scene
    );
    if !expected_resume.scene.is_empty() {
        assert_eq!(
            scene, expected_resume.scene,
            "a save resumes in its own scene"
        );
    }
    assert_eq!(
        session.host.world.party.money, expected_sf.ext.money,
        "the save's money landed"
    );
    for frame in 0..600u64 {
        session
            .host
            .world
            .set_pad(if frame % 8 == 0 { CROSS } else { 0 });
        session.tick().expect("tick after the resume");
        host_drains(&mut session);
    }
    assert!(
        !matches!(session.host.world.mode, SceneMode::Title),
        "the resumed save keeps running"
    );
}

#[test]
fn front_end_labels_split() {
    assert_eq!(split_front_end("title"), Some(FrontEndRoute::NewGame));
    assert_eq!(
        split_front_end("title+continue"),
        Some(FrontEndRoute::Continue(None))
    );
    assert_eq!(
        split_front_end("title+continue@PRO-14"),
        Some(FrontEndRoute::Continue(Some("PRO-14".into())))
    );
    assert_eq!(split_front_end("town01"), None);
}
