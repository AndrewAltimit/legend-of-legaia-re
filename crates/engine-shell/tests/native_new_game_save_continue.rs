//! A New Game saved and continued through the native play window's own frame
//! path (`HeadlessPlayWindow`), with a quit and relaunch between the two:
//!
//! cold boot -> logos -> title -> New Game -> the prologue -> `town01`'s
//! naming prompt (retail's Yes / No confirm) -> free roam -> Save -> the
//! window dropped -> a fresh window over the same save directory -> the
//! title's Continue -> port 1 block 1.
//!
//! The save is made twice, each in its own save directory: at the `town01`
//! save point (the action button on it opens the save screen through its
//! `49 01` record), and through the pause menu's Save row with
//! `WorldToggles::save_anywhere` raised. After the Continue the party, the
//! scene, the gold, the lead's name and the play clock must be the ones the
//! save was made with - a New Game party is Vahn alone.
//!
//! Every frame is a pad word handed to `display_frame`, except the save
//! point's seat: the player is stood in front of it and faced at it, as the
//! soak harness's talk visits do, and the press itself is the pad's.
//! `LEGAIA_NGSC_PAD_OUT=<dir>` writes each leg's pad stream as a
//! `play-window --pad-script` (`<leg>.pad-script`), so the save-anywhere leg
//! can be replayed on a real window.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use std::path::{Path, PathBuf};

use legaia_engine_menus::save_select::SelectPhase;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_engine_system::input::PadButton;
use legaia_engine_title::title::TitlePhase;

const START: u16 = 0x0008;
/// The pause menu's root rows: Items, Magic, Equip, Status, Options, Load,
/// Save.
const SAVE_ROW: u8 = 6;

fn mask(b: PadButton) -> u16 {
    b.mask()
}

/// The pads a leg pressed, one word per display frame (one sim tick each).
#[derive(Default)]
struct PadLog(Vec<u16>);

impl PadLog {
    fn frame(&mut self, w: &mut HeadlessPlayWindow, pad: u16) {
        self.0.push(pad);
        w.display_frame(pad, 1);
    }

    /// The `--pad-script` spelling: `TICK:BUTTON` per pressed button per
    /// frame. The window numbers its first sim tick 1.
    fn pad_script(&self) -> String {
        let names = [
            (PadButton::Up, "Up"),
            (PadButton::Down, "Down"),
            (PadButton::Left, "Left"),
            (PadButton::Right, "Right"),
            (PadButton::Cross, "Cross"),
            (PadButton::Circle, "Circle"),
            (PadButton::Start, "Start"),
        ];
        let mut out = Vec::new();
        for (i, &p) in self.0.iter().enumerate() {
            for (b, n) in names {
                if p & b.mask() != 0 {
                    out.push(format!("{}:{n}", i + 1));
                }
            }
        }
        out.join(",")
    }
}

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("legaia-ngsc-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch save dir");
    dir
}

/// Title: Start through the logos and the prompt, the cursor onto `row`,
/// Cross. One edge every eight frames.
fn title_pad(w: &HeadlessPlayWindow, frame: u32, row: u8) -> u16 {
    if frame % 8 >= 2 {
        return 0;
    }
    match w
        .front_end()
        .and_then(|fe| fe.title_session())
        .map(|t| t.phase())
    {
        Some(TitlePhase::MainMenu { cursor }) if cursor < row => mask(PadButton::Down),
        Some(TitlePhase::MainMenu { cursor }) if cursor > row => mask(PadButton::Up),
        Some(TitlePhase::MainMenu { .. }) => mask(PadButton::Cross),
        _ => START,
    }
}

/// The save screen's pad onto block 1: Cross onto the port and the grid's
/// first cell (where the cursor opens), Up onto a Yes / No's Yes.
fn select_pad(phase: &SelectPhase, frame: u32) -> u16 {
    if frame % 8 >= 2 {
        return 0;
    }
    match phase {
        SelectPhase::ConfirmOverwrite { cursor, .. }
        | SelectPhase::ConfirmDelete { cursor, .. }
            if *cursor != 0 =>
        {
            mask(PadButton::Up)
        }
        _ => mask(PadButton::Cross),
    }
}

const SEEDED_GOLD: i32 = 12_345;
const SEEDED_PLAY_SECONDS: u32 = 3_725;

/// The interact probe's eight compass points
/// (`legaia_engine_core::world::config::FIELD_FACING_PROBES`): sector ->
/// `(dx, dz)`, probe point `(x + dx, z - dz)`.
const FACING_PROBES: [(i16, i16); 8] = [
    (0, 64),
    (-64, 64),
    (-64, 0),
    (-64, -64),
    (0, -64),
    (64, -64),
    (64, 0),
    (64, 64),
];

/// Stand the player in front of the scene's save point - the text-free
/// placement whose interaction section presses `49 01` - facing it, on open
/// floor where the action button's probe answers with it. Returns its slot.
fn seat_at_save_point(w: &mut HeadlessPlayWindow) -> Option<u8> {
    let world = &mut w.session_mut().host.world;
    let slot = world.npcs.dialog_prologue.iter().find_map(|(&slot, rec)| {
        let text_free = rec.first_segment >= rec.body.len();
        let presses_save = rec
            .body
            .get(rec.entry_pc..)
            .is_some_and(|b| b.windows(2).any(|p| p == [0x49, 0x01]));
        (text_free && presses_save).then_some(slot)
    })?;
    let (ax, az) = *world.npcs.positions.get(&slot)?;
    let player = world.player_actor_slot? as usize;
    for q in [8i16, 7, 6, 4] {
        for sector in [4usize, 0, 2, 6, 1, 3, 5, 7] {
            let (dx, dz) = FACING_PROBES[sector];
            let (sx, sz) = (ax - dx * q / 4, az + dz * q / 4);
            if world.field_walk_component_size(sx, sz) < 48 {
                continue;
            }
            let sy = world.sample_field_floor_height(sx as i32, sz as i32) as i16;
            let m = &mut world.actors[player].move_state;
            m.world_x = sx;
            m.world_y = sy;
            m.world_z = sz;
            m.render_26 = (((sector as i16) << 9) + 0x800) & 0x0FFF;
            if world.field_interact_probe_slot() == Some(slot) {
                return Some(slot);
            }
        }
    }
    None
}

/// What a free-roam frame in the opening town looks like.
fn free_roam(w: &HeadlessPlayWindow) -> bool {
    let world = &w.session().host.world;
    world.active_scene_label == legaia_game_tables::new_game::OPENING_SCENE
        && world.mode == SceneMode::Field
        && !world.cutscene_timeline_active()
        && world.dialog.current.is_none()
        && world.dialog.inline.is_none()
        && !world.name_entry_active()
        && !world.script_context_engages_player()
        && !w.session().field_menu_is_open()
}

/// Cold boot to free roam in `town01` through a New Game, by pad alone.
fn new_game_to_free_roam(w: &mut HeadlessPlayWindow, log: &mut PadLog) {
    let mut frame = 0u32;
    while w.boot_ui_active() {
        assert!(frame < 6000, "the title never resolved");
        let pad = title_pad(w, frame, 0);
        log.frame(w, pad);
        frame += 1;
    }
    // The prologue and the opening: Cross mashed through the lines, the
    // naming prompt's retail confirm (Cross opens Yes / No on No, Up, Cross).
    let mut naming_step = 0u8;
    let mut named = false;
    let mut settled = 0u32;
    for f in 0..40_000u32 {
        let world = &w.session().host.world;
        let pad = if world.name_entry_active() {
            if f % 8 >= 2 {
                0
            } else {
                let p = [PadButton::Cross, PadButton::Up, PadButton::Cross]
                    [(naming_step as usize).min(2)];
                naming_step = naming_step.saturating_add(1);
                named = true;
                mask(p)
            }
        } else if named && free_roam(w) {
            // Hold still a while: the controls are handed back.
            settled += 1;
            if settled > 60 {
                return;
            }
            0
        } else {
            settled = 0;
            if f % 4 < 2 { mask(PadButton::Cross) } else { 0 }
        };
        log.frame(w, pad);
    }
    panic!(
        "the opening never reached free roam (scene '{}', mode {:?}, named {named})",
        w.session().host.world.active_scene_label,
        w.session().host.world.mode
    );
}

/// Drive an open save screen onto port 1 block 1 and through the Yes, until
/// the menu has closed again (Circle backs out of a root left standing).
fn save_into_block_one(w: &mut HeadlessPlayWindow, log: &mut PadLog, card: &Path) {
    for f in 0..6000u32 {
        let phase = w.save_select_phase();
        if std::env::var_os("LEGAIA_NGSC_TRACE").is_some() && f % 30 == 0 {
            eprintln!(
                "[ngsc] f{f} save {phase:?} menu {} {:?}",
                w.session().field_menu_is_open(),
                w.session().field_menu.as_ref().map(|m| m.phase())
            );
        }
        let saved = card.is_file();
        let pad = match (&phase, w.session().field_menu_is_open()) {
            (Some(p), _) => select_pad(p, f),
            (None, true) if saved && f % 8 < 2 => mask(PadButton::Circle),
            (None, true) => 0,
            (None, false) if saved => return,
            (None, false) => 0,
        };
        log.frame(w, pad);
    }
    panic!("the save screen did not write {} and close", card.display());
}

#[derive(Debug, PartialEq)]
struct Saved {
    scene: String,
    party_count: u8,
    roster: usize,
    money: i32,
    name: String,
    play_seconds: u32,
}

fn snapshot(w: &HeadlessPlayWindow) -> Saved {
    let world = &w.session().host.world;
    Saved {
        scene: world.active_scene_label.clone(),
        party_count: world.party.party_count,
        roster: world.party.roster.members.len(),
        money: world.party.money,
        name: world.party_name(0).to_string(),
        play_seconds: world.clock.play_time_seconds,
    }
}

/// Relaunch over `save_dir` and Continue onto port 1 block 1.
fn relaunch_and_continue(disc: &Path, save_dir: &Path) -> HeadlessPlayWindow {
    let mut w = HeadlessPlayWindow::open_boot_ui_in(disc, "town01", None, save_dir)
        .expect("reopen the boot-UI window");
    let mut frame = 0u32;
    let mut saw_continue = false;
    while w.boot_ui_active() {
        assert!(frame < 8000, "the Continue never landed");
        let tap = frame % 8 < 2;
        let pad = if let Some(c) = w.continue_screen() {
            saw_continue = true;
            match c.session().phase() {
                _ if !tap => 0,
                p => select_pad(&p, 0),
            }
        } else {
            title_pad(&w, frame, 1)
        };
        w.display_frame(pad, 1);
        frame += 1;
    }
    assert!(saw_continue, "the title's Continue row was live and opened");
    w
}

fn run_leg(disc: &Path, leg: &str, save_anywhere: bool) {
    let save_dir = scratch_dir(leg);
    let card = save_dir.join("memcard1.mcr");
    let mut log = PadLog::default();
    let mut w = HeadlessPlayWindow::open_boot_ui_in(disc, "town01", None, &save_dir)
        .expect("open the boot-UI window");
    new_game_to_free_roam(&mut w, &mut log);
    eprintln!(
        "[ngsc:{leg}] free roam in '{}' after {} frames",
        w.session().host.world.active_scene_label,
        log.0.len()
    );
    if save_anywhere {
        w.session_mut().host.world.toggles.save_anywhere = true;
        // Start, then the root cursor down to Save (the last row), Cross.
        let mut f = 0u32;
        while w.save_select_phase().is_none() {
            assert!(
                f < 2000,
                "the pause menu's Save row never opened: open {} cursor {:?} phase {:?} save {:?}",
                w.session().field_menu_is_open(),
                w.session().field_menu.as_ref().map(|m| m.cursor()),
                w.session().field_menu.as_ref().map(|m| m.phase()),
                w.save_select_phase()
            );
            let pad = if f % 8 >= 2 {
                0
            } else if !w.session().field_menu_is_open() {
                START
            } else if w
                .session()
                .field_menu
                .as_ref()
                .is_some_and(|m| m.cursor() < SAVE_ROW)
            {
                mask(PadButton::Down)
            } else {
                mask(PadButton::Cross)
            };
            log.frame(&mut w, pad);
            f += 1;
        }
    } else {
        let slot = seat_at_save_point(&mut w).expect("town01 has a save point a seat faces");
        eprintln!("[ngsc:{leg}] seated at the save point (placement slot {slot})");
        // The action button: the save point's `49 01` opens the save screen.
        let mut f = 0u32;
        while w.save_select_phase().is_none() {
            assert!(
                f < 600,
                "the save point's press never opened the save screen (menu open {})",
                w.session().field_menu_is_open()
            );
            let pad = if f % 8 < 2 { mask(PadButton::Cross) } else { 0 };
            log.frame(&mut w, pad);
            f += 1;
        }
    }
    // Marks the save carries, so the Continue's are its and not a New
    // Game's: gold off the starting 500, and a play clock (the engine's clock
    // follows wall time, which a headless run barely spends).
    w.session_mut().host.world.party.money = SEEDED_GOLD;
    w.session_mut().host.world.clock.play_time_seconds = SEEDED_PLAY_SECONDS;
    save_into_block_one(&mut w, &mut log, &card);
    let before = snapshot(&w);
    eprintln!(
        "[ngsc:{leg}] saved after {} frames: {before:?}",
        log.0.len()
    );
    if let Some(dir) = std::env::var_os("LEGAIA_NGSC_PAD_OUT") {
        let path = Path::new(&dir).join(format!("{leg}.pad-script"));
        std::fs::write(&path, log.pad_script()).expect("write the pad script");
        eprintln!("[ngsc:{leg}] pad script -> {}", path.display());
    }
    drop(w);

    let w = relaunch_and_continue(disc, &save_dir);
    let after = snapshot(&w);
    eprintln!("[ngsc:{leg}] continued: {after:?}");
    assert_eq!(after, before, "the Continue lands the game the Save wrote");
    assert_eq!(
        (after.money, after.play_seconds),
        (SEEDED_GOLD, SEEDED_PLAY_SECONDS),
        "the gold and the play clock are the save's"
    );
    assert_eq!((after.party_count, after.roster), (1, 1), "Vahn alone");
    assert!(
        w.session().host.world.party.active_party.len() <= 1,
        "no stale composition: {:?}",
        w.session().host.world.party.active_party
    );
    let _ = std::fs::remove_dir_all(&save_dir);
}

#[test]
fn new_game_save_point_quit_continue() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    run_leg(Path::new(&disc), "save-point", false);
}

#[test]
fn new_game_save_anywhere_quit_continue() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    run_leg(Path::new(&disc), "save-anywhere", true);
}
