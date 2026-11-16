//! The `F12` panel's Tools page: the debug switches that otherwise take a
//! launch flag or an environment variable, and the minigame and game-mode
//! entries that otherwise take a keyboard key, so a controller player and a
//! player with no terminal reach them.
//!
//! Every switch is for this session only, as the browser play page's *Dev
//! menu* box and its battle tools are: nothing here is written to a settings
//! file, so a developer tool cannot come back on by itself on a later start.
//! The browser play page's twins are its dev-menu box, Auto-battle box,
//! *Overworld markers* box, *Retail battle cadence* box, Minigame list,
//! *Effect outlines* box, Party field, Battle BGM field, Learn spell row,
//! *Start a battle here* button, GameShark box (a file here: a window has
//! no text field), *Export session* button and *Copy report* button.

use super::*;
use legaia_engine_save_state::WorldSaveState;
use std::path::PathBuf;

/// One row of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolRow {
    /// The original game's developer menu (`LEGAIA_DEV_MENU`).
    DevMenu,
    /// The window's diagnostic rows (`F1`).
    Diagnostics,
    /// The wide orbit vantage (`F3`).
    DebugCamera,
    /// Party turns resolve themselves (`--no-player-battle`).
    AutoBattle,
    /// The battle pass on retail's cadence (`LEGAIA_BATTLE_FRAME_CADENCE`).
    BattleCadence,
    /// The port's overworld markers (`--world-map-markers`).
    OverworldMarkers,
    /// A wireframe box around every effect sprite (`LEGAIA_DIAG_FX`).
    EffectOutlines,
    /// The present party (`--party`); picked, then applied.
    Party,
    /// The track a fight plays (`--battle-bgm`); picked, then applied.
    BattleBgm,
    /// Teach the lead a spell (`--learn-spell`); picked, then applied.
    LearnSpell,
    /// Read and apply a GameShark code file (`--cheat-file`).
    GameShark,
    /// The memory card in port 2 (`--card`, or a card file dropped on the
    /// window); picked, then inserted.
    MemoryCard,
    /// Write the running game as an `.lgsf` file in the saves folder: the
    /// page's *Export session* button.
    ExportSession,
    /// Put a bug report on the clipboard: the page's *Copy report* button.
    CopyReport,
    /// Arm a fight from the scene's own encounter table (`--battle first`).
    StartBattle,
    /// A press of the window key that enters a minigame or a mode.
    Enter(KeyCode),
}

/// The rows: control, label, one line of help.
pub(super) const TOOL_ROWS: [(ToolRow, &str, &str); 22] = [
    (
        ToolRow::DevMenu,
        "Developer menu",
        "The original game's debug screens over the scene. Square on its list: records.",
    ),
    (
        ToolRow::Diagnostics,
        "Diagnostics",
        "Scene, frame, mesh and camera readouts on the screen. Key: F1.",
    ),
    (
        ToolRow::DebugCamera,
        "Debug camera",
        "A wide orbit view of the field instead of the follow camera. Key: F3.",
    ),
    (
        ToolRow::AutoBattle,
        "Auto-battle",
        "From the next round on, each party turn attacks the first living monster by itself.",
    ),
    (
        ToolRow::BattleCadence,
        "Battle cadence",
        "On: battles advance once a battle frame, as the original does. Off: once a display frame.",
    ),
    (
        ToolRow::OverworldMarkers,
        "Overworld markers",
        "A marker over each place on the world map. Not in the original game.",
    ),
    (
        ToolRow::EffectOutlines,
        "Effect outlines",
        "A box around every effect sprite, so a spawn shows when its art is not loaded.",
    ),
    (
        ToolRow::Party,
        "Party",
        "Left/Right pick, Cross seat: who fights, in battle order. Empty seats join at the start level.",
    ),
    (
        ToolRow::BattleBgm,
        "Battle music",
        "Left/Right pick, Cross use: the track a fight plays from the next fight on.",
    ),
    (
        ToolRow::LearnSpell,
        "Teach the lead",
        "Left/Right pick, Cross teach: adds the Seru spell to the first party member's list.",
    ),
    (
        ToolRow::GameShark,
        "GameShark file",
        "Cross: apply the --cheat-file codes, or legaia-codes.txt in the settings folder.",
    ),
    (
        ToolRow::MemoryCard,
        "Memory card 2",
        "Left/Right pick, Cross insert: a card mounted before, or one in the saves folder.",
    ),
    (
        ToolRow::ExportSession,
        "Export session",
        "Cross: write the running game to an .lgsf file in the saves folder.",
    ),
    (
        ToolRow::CopyReport,
        "Copy report",
        "Cross: copy a bug report: version, system, scene, settings. No game data.",
    ),
    (
        ToolRow::StartBattle,
        "Start a battle here",
        "Cross: the scene's first encounter formation, through the normal transition.",
    ),
    (
        ToolRow::Enter(KeyCode::KeyL),
        "Fishing",
        "Start fishing from this scene. Key: L.",
    ),
    (
        ToolRow::Enter(KeyCode::KeyO),
        "Slot machine",
        "Start the casino slots from this scene. Key: O.",
    ),
    (
        ToolRow::Enter(KeyCode::KeyB),
        "Baka Fighter",
        "Start a Baka Fighter duel from this scene. Key: B.",
    ),
    (
        ToolRow::Enter(KeyCode::KeyM),
        "Muscle Dome",
        "Start the Muscle Dome ladder from this scene. Key: M.",
    ),
    (
        ToolRow::Enter(KeyCode::KeyK),
        "Dance",
        "Start the dance minigame from this scene. Key: K.",
    ),
    (
        ToolRow::Enter(KeyCode::Backquote),
        "Game modes",
        "The port's own modes: the expansion pack's, or the built-in practice set. Key: `.",
    ),
    (
        ToolRow::Enter(KeyCode::Backslash),
        "Trading cards",
        "The expansion pack's card menu: binder, boosters, deck. Needs a pack with cards. Key: \\.",
    ),
];

/// The page's rows.
pub(super) const TOOL_COUNT: usize = TOOL_ROWS.len();

/// How many rows the plate shows at once; the list scrolls under the cursor.
const SHOWN: usize = 13;

/// The party rows' picks, in the `--party` spec form.
const PARTY_PICKS: [&str; 7] = [
    "vahn",
    "vahn,noa",
    "vahn,noa,gala",
    "noa,vahn,gala",
    "gala,noa,vahn",
    "noa",
    "gala",
];

/// A party pick as the row and its note show it: the names as the game
/// spells them (`vahn,noa` reads `Vahn, Noa`), not the `--party` spec.
fn party_label(spec: &str) -> String {
    spec.split(',')
        .map(|name| {
            let mut c = name.trim().chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The spell picks: the player Seru block the Cheats page's Seru grant covers.
const SPELL_PICKS: std::ops::RangeInclusive<u8> = legaia_engine_cheats::cheats::SERU_SPELL_IDS;

/// The most card files the memory-card row offers.
const CARD_PICKS_MAX: usize = 32;
/// The most characters of a card's file name the row shows.
const CARD_LABEL_CHARS: usize = 24;
/// What the memory-card row shows for an empty port.
const NO_CARD: &str = "No card";

/// The card files the memory-card row steps through: the mounted one, then
/// the ones the launcher remembers, then every card file in `folders`, by
/// name. Each file once, only files that are there now, never port 1's own
/// card (mounting it twice would let two ports write one file), at most
/// [`CARD_PICKS_MAX`].
pub(super) fn card_picks(
    mounted: Option<&Path>,
    remembered: &[PathBuf],
    folders: &[PathBuf],
    port_one: Option<&Path>,
) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for dir in folders {
        let Ok(read) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut here: Vec<PathBuf> = read
            .flatten()
            .map(|e| e.path())
            .filter(|p| crate::launcher::is_card_file(p))
            .collect();
        here.sort();
        found.extend(here);
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for p in mounted
        .into_iter()
        .map(Path::to_path_buf)
        .chain(remembered.iter().cloned())
        .chain(found)
    {
        if Some(p.as_path()) != port_one && p.is_file() && !out.contains(&p) {
            out.push(p);
        }
    }
    out.truncate(CARD_PICKS_MAX);
    out
}

/// A card pick as its row shows it: the file's name, cut in the middle when
/// it is long so both its start and its extension stay.
fn card_label(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= CARD_LABEL_CHARS {
        return name;
    }
    let keep = CARD_LABEL_CHARS - 3;
    let head: String = chars[..keep - keep / 2].iter().collect();
    let tail: String = chars[chars.len() - keep / 2..].iter().collect();
    format!("{head}...{tail}")
}

/// The file a Cross on the GameShark row reads when `--cheat-file` named none.
const DEFAULT_CODES_FILE: &str = "legaia-codes.txt";

/// A battle-music pick: the default theme, no swap, or one battle-bank track.
fn bgm_pick(i: usize) -> Option<u16> {
    match i {
        0 => None,
        1 => Some(0),
        i => Some(legaia_engine_system::music_labels::BATTLE_BANK_BGM_IDS[i - 2]),
    }
}
const BGM_PICKS: usize = 2 + legaia_engine_system::music_labels::BATTLE_BANK_BGM_IDS.len();

/// The most session files the export row numbers before it asks for room.
const SESSION_FILES_MAX: u32 = 999;
/// The file the report row writes when no clipboard takes the text.
const REPORT_FILE: &str = "legaia-report.txt";

/// The session file name for `scene` and number `n`: the scene's label
/// reduced to file-safe characters, so a pack scene's name cannot leave the
/// folder.
fn session_file_name(scene: &str, n: u32) -> String {
    let label: String = scene
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(24)
        .collect();
    let label = if label.is_empty() { "game" } else { &label };
    format!("session_{label}_{n:03}.lgsf")
}

/// Write `bytes` as the next unused session file for `scene` in `dir`
/// (created when missing): never over a file that is there.
pub(super) fn write_session_file(dir: &Path, scene: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for n in 1..=SESSION_FILES_MAX {
        let path = dir.join(session_file_name(scene, n));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                use std::io::Write;
                return f
                    .write_all(bytes)
                    .map(|()| path.clone())
                    .map_err(|e| format!("{}: {e}", path.display()));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    Err("The saves folder is full of session files; move some out.".to_string())
}

/// The system clipboard, opened on the report row's first press and kept for
/// the session: on X11 the text lives only as long as its owner does.
#[derive(Default)]
struct HeldClipboard(Option<arboard::Clipboard>);

impl std::fmt::Debug for HeldClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() { "open" } else { "closed" })
    }
}

impl Clone for HeldClipboard {
    /// A copy of the page holds no clipboard; it opens its own.
    fn clone(&self) -> Self {
        Self(None)
    }
}

impl HeldClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), arboard::Error> {
        if self.0.is_none() {
            self.0 = Some(arboard::Clipboard::new()?);
        }
        self.0.as_mut().expect("just set").set_text(text)
    }
}

/// The cursor, each picker's position and the last row's message.
#[derive(Debug, Clone, Default)]
pub(super) struct ToolsPage {
    pub(super) cursor: usize,
    party: usize,
    bgm: usize,
    spell: usize,
    /// The memory-card row's pick: `0` is the empty port, `n` is
    /// `cards[n - 1]`.
    card: usize,
    /// The card files the memory-card row steps through, listed on the
    /// row's first press ([`card_picks`]); `None` until then.
    cards: Option<Vec<PathBuf>>,
    /// What the last press did, or why it did nothing.
    note: Option<String>,
    /// The report row's clipboard.
    clipboard: HeldClipboard,
}

impl ToolsPage {
    /// Move the cursor by `d` rows, wrapping, and drop the last note.
    pub(super) fn move_cursor(&mut self, d: i32) {
        self.cursor = (self.cursor as i32 + d).rem_euclid(TOOL_COUNT as i32) as usize;
        self.note = None;
    }

    /// Left / Right on a picker row; `false` on any other row.
    fn pick(&mut self, row: ToolRow, dir: i8) -> bool {
        let (at, n) = match row {
            ToolRow::Party => (&mut self.party, PARTY_PICKS.len()),
            ToolRow::BattleBgm => (&mut self.bgm, BGM_PICKS),
            ToolRow::LearnSpell => (&mut self.spell, SPELL_PICKS.count()),
            _ => return false,
        };
        *at = (*at as i32 + i32::from(dir)).rem_euclid(n as i32) as usize;
        self.note = None;
        true
    }

    /// The first row the plate shows, keeping the cursor on it.
    fn first_shown(&self) -> usize {
        (self.cursor + 1).saturating_sub(SHOWN)
    }
}

const X0: i32 = 10;
const Y0: i32 = super::settings_panel::Y0;
const WIDTH: i32 = 300;
const ROW_H: i32 = super::settings_panel::ROW_H;
const HELP_H: i32 = 12;
const NOTE_LINES: usize = 2;
const CONTROLS: &str = "Arrows pick  Enter use  Esc close";
/// [`CONTROLS`] on a controller (`settings_panel::pad_controls`).
const PAD_CONTROLS: &str = "D-pad pick  {cross} use  {circle} close";
const FLIP_HINT: &str = "L1/R1 or Tab: keys";
/// What a refused minigame press says.
const REFUSED: &str = "Not from here: stand in a town, a field or on the world map.";

/// A row's value column.
enum ToolValue {
    Switch(bool),
    Pick(String),
    Press(&'static str),
}

impl PlayWindowApp {
    /// What the row's value column shows.
    fn tool_value(&self, row: ToolRow) -> ToolValue {
        let world = &self.session.host.world;
        let page = &self.tools_page;
        ToolValue::Switch(match row {
            ToolRow::DevMenu => self.dev_menu_on,
            ToolRow::Diagnostics => self.diag_rows,
            ToolRow::DebugCamera => self.field_debug_camera,
            ToolRow::AutoBattle => !world.battle.player_driven,
            ToolRow::BattleCadence => world.toggles.battle_sm_frame_cadence,
            ToolRow::OverworldMarkers => world.toggles.overworld_marker_overlay,
            ToolRow::EffectOutlines => self.fx_outline,
            ToolRow::Party => return ToolValue::Pick(party_label(PARTY_PICKS[page.party])),
            ToolRow::BattleBgm => return ToolValue::Pick(bgm_pick_label(page.bgm)),
            ToolRow::LearnSpell => {
                let id = SPELL_PICKS.clone().nth(page.spell).unwrap_or(0x81);
                return ToolValue::Pick(spell_pick_label(world, id));
            }
            ToolRow::GameShark => return ToolValue::Press("Cross: apply"),
            ToolRow::MemoryCard => {
                // Before the first press the row names what port 2 holds.
                let picked = match &page.cards {
                    Some(cards) => page.card.checked_sub(1).and_then(|i| cards.get(i)).cloned(),
                    None => self.card.as_ref().and_then(|c| c.path.clone()),
                };
                return ToolValue::Pick(match picked {
                    Some(path) => card_label(&path),
                    None => match (&page.cards, &self.card) {
                        (None, Some(card)) => card.label.clone(),
                        _ => NO_CARD.to_string(),
                    },
                });
            }
            ToolRow::ExportSession => return ToolValue::Press("Cross: write"),
            ToolRow::CopyReport => return ToolValue::Press("Cross: copy"),
            ToolRow::StartBattle => return ToolValue::Press("Cross: start"),
            ToolRow::Enter(_) => return ToolValue::Press("Cross: start"),
        })
    }

    /// Left / Right on the page: a picker steps, a switch flips, an entry
    /// row does nothing.
    pub(super) fn tools_change(&mut self, dir: i8) {
        let (row, _, _) = TOOL_ROWS[self.tools_page.cursor];
        if row == ToolRow::MemoryCard {
            self.list_cards();
            let page = &mut self.tools_page;
            let n = page.cards.as_ref().map_or(0, Vec::len) as i32 + 1;
            page.card = (page.card as i32 + i32::from(dir)).rem_euclid(n) as usize;
            page.note = (n == 1).then(|| {
                "No card files found. Mount one in the launcher, or put it in the saves folder."
                    .to_string()
            });
            return;
        }
        if self.tools_page.pick(row, dir) {
            return;
        }
        if !matches!(
            row,
            ToolRow::Enter(_)
                | ToolRow::GameShark
                | ToolRow::StartBattle
                | ToolRow::MemoryCard
                | ToolRow::ExportSession
                | ToolRow::CopyReport
        ) {
            self.tools_apply();
        }
    }

    /// Cross / Enter on the focused row.
    pub(super) fn tools_apply(&mut self) {
        let (row, _, _) = TOOL_ROWS[self.tools_page.cursor];
        self.tools_page.note = None;
        match row {
            ToolRow::DevMenu => self.set_dev_menu(!self.dev_menu_on),
            ToolRow::Diagnostics => self.diag_rows ^= true,
            ToolRow::DebugCamera => self.field_debug_camera ^= true,
            ToolRow::AutoBattle => {
                let driven = !self.session.host.world.battle.player_driven;
                self.session.host.world.battle.player_driven = driven;
                self.field_live_opts.player_battle = driven;
            }
            ToolRow::BattleCadence => {
                let toggles = &mut self.session.host.world.toggles;
                let on = !toggles.battle_sm_frame_cadence;
                toggles.battle_sm_frame_cadence = on;
                if let Some(sw) = self.launch_switches.as_mut() {
                    sw.battle_frame_cadence = on;
                }
            }
            ToolRow::OverworldMarkers => {
                if self.retail_session() {
                    self.tools_page.note = Some("Off while Original game only is on.".into());
                    return;
                }
                let toggles = &mut self.session.host.world.toggles;
                let on = !toggles.overworld_marker_overlay;
                toggles.overworld_marker_overlay = on;
                if let Some(sw) = self.launch_switches.as_mut() {
                    sw.world_map_markers = on;
                }
            }
            ToolRow::EffectOutlines => self.fx_outline ^= true,
            ToolRow::Party => {
                let spec = PARTY_PICKS[self.tools_page.party];
                self.tools_page.note = Some(match self.session.seat_party_spec(spec) {
                    Ok(_) => format!("Party: {}.", party_label(spec)),
                    Err(e) => e,
                });
            }
            ToolRow::BattleBgm => {
                let pick = bgm_pick(self.tools_page.bgm);
                self.field_live_opts.battle_bgm = pick;
                let track = self.field_live_opts.to_live_loop_opts().battle_bgm;
                self.session.host.world.set_battle_bgm(track);
                self.tools_page.note = Some(format!(
                    "From the next fight: {}.",
                    bgm_pick_label(self.tools_page.bgm)
                ));
            }
            ToolRow::LearnSpell => {
                let id = SPELL_PICKS
                    .clone()
                    .nth(self.tools_page.spell)
                    .unwrap_or(0x81);
                let world = &mut self.session.host.world;
                let label = spell_pick_label(world, id);
                let lead = world.party.active_party.first().copied().unwrap_or(0) as usize;
                self.tools_page.note = Some(match world.party.roster.members.get_mut(lead) {
                    Some(record) => {
                        legaia_engine_battle::magic_xp::learn_spell_prepend(record, id);
                        format!("Learned {label}.")
                    }
                    None => "No party member to teach yet.".to_string(),
                });
            }
            ToolRow::GameShark => {
                let path = self.cheat_file.clone().or_else(|| {
                    crate::launcher::settings_path()
                        .and_then(|p| p.parent().map(|d| d.join(DEFAULT_CODES_FILE)))
                });
                self.tools_page.note = Some(match path {
                    Some(path) => self.apply_codes_file(&path),
                    None => format!("No settings folder to find {DEFAULT_CODES_FILE} in."),
                });
            }
            ToolRow::MemoryCard => {
                self.list_cards();
                let page = &self.tools_page;
                let pick = page
                    .card
                    .checked_sub(1)
                    .and_then(|i| page.cards.as_ref()?.get(i))
                    .cloned();
                self.tools_page.note = Some(match pick {
                    Some(path) => match self.mount_port_two(&path) {
                        Ok(name) => format!("Memory card 2: {name}. The save screen shows it."),
                        Err(name) => format!("Not a memory card: {name}."),
                    },
                    None => {
                        self.card = None;
                        "Memory card 2 taken out.".to_string()
                    }
                });
            }
            ToolRow::ExportSession => {
                self.tools_page.note = Some(match self.export_session() {
                    Ok(path) => format!(
                        "Wrote {} in the saves folder.",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    Err(e) => e,
                });
            }
            ToolRow::CopyReport => {
                let report = self.session_report();
                self.tools_page.note = Some(self.hand_over_report(&report));
            }
            ToolRow::StartBattle => {
                let world = &mut self.session.host.world;
                if !matches!(world.mode, SceneMode::Field | SceneMode::WorldMap) {
                    self.tools_page.note = Some(REFUSED.into());
                    return;
                }
                if world.force_battle_here().is_some() {
                    self.hotkey_help = false;
                } else {
                    self.tools_page.note =
                        Some("This scene registers no encounter formation to start.".into());
                }
            }
            ToolRow::Enter(key) => {
                if self.minigame_key(key, ElementState::Pressed) {
                    self.hotkey_help = false;
                } else {
                    self.tools_page.note = Some(REFUSED.into());
                }
            }
        }
    }

    /// Write the running game as an `.lgsf` session file in the saves
    /// folder, with the resume trailer the save screen's files carry: the
    /// page's `export_save`, by the same refusal and the same writer.
    pub(super) fn export_session(&mut self) -> Result<PathBuf, String> {
        if !self.session.host.world.host_save_allowed() {
            return Err("A battle is starting - export once it is over.".into());
        }
        let resume = self.session.current_resume();
        let bytes = self
            .session
            .host
            .world
            .save_full()
            .write_with_resume(&resume);
        let dir = self.menu_runtime.save_dir.clone();
        write_session_file(&dir, &resume.scene, &bytes)
    }

    /// What a player pastes into a bug report while the game runs: the
    /// launcher's failure report ([`legaia_app_theme::game_exit::report_text`])
    /// with the running session in place of how the game ended. It names the
    /// scene and the settings, never the disc's bytes or the save's.
    pub(super) fn session_report(&self) -> String {
        use legaia_app_theme::game_exit;
        let world = &self.session.host.world;
        let scene = self.session.current_resume().scene;
        let headline = format!(
            "Copied while playing: scene {}, mode {:?}, frame {}",
            if scene.is_empty() { "(none)" } else { &scene },
            world.mode,
            world.frame
        );
        let (w, h) = self.win.surface_size();
        let log_text = self.session_log_text();
        let mut detail = vec![format!(
            "window {w}x{h}{}",
            if self.win.is_fullscreen() {
                ", fullscreen"
            } else {
                ""
            }
        )];
        // The adapter line is logged once at start, far above the log's end.
        if let Some(gpu) = log_text.as_deref().and_then(|t| {
            t.lines()
                .find(|l| l.contains("wgpu adapter:"))
                .map(game_exit::strip_log_prefix)
        }) {
            detail.push(gpu.to_string());
        }
        detail.push(format!(
            "controller: {}",
            match self.last_pad {
                Some(style) => format!("{style:?}"),
                None => "keyboard last".to_string(),
            }
        ));
        detail.push(format!(
            "not at the original's values: {}",
            match world.toggles.retail_deviations() {
                d if d.is_empty() => "nothing".to_string(),
                d => d.join(", "),
            }
        ));
        if let Some(sw) = self.launch_switches.as_ref() {
            detail.push(format!("switches: {sw:?}"));
        }
        detail.push(format!("options: {:?}", self.options_state));
        if let Some(pack) = world.expansion.pack.as_deref() {
            detail.push(format!("expansion pack: {}", pack.manifest.id));
        }
        if self.netplay.is_some() {
            detail.push("in a multiplayer room".to_string());
        }
        game_exit::report_text(
            "legaia-engine play-window",
            env!("CARGO_PKG_VERSION"),
            &headline,
            Some(&detail.join("\n")),
            log_text.as_deref(),
            game_exit::home_folder().as_deref(),
        )
    }

    /// The log of this run, when the launcher is keeping one: its game log
    /// in the working directory, written to since this window started (an
    /// older file is some earlier run's).
    fn session_log_text(&self) -> Option<String> {
        let path = std::env::current_dir()
            .ok()?
            .join(crate::launcher::GAME_LOG);
        let written = std::fs::metadata(&path).ok()?.modified().ok()?;
        let started = std::time::SystemTime::now().checked_sub(self.started_at.elapsed())?;
        (written >= started)
            .then(|| std::fs::read_to_string(&path).ok())
            .flatten()
    }

    /// Put `report` on the clipboard, or in a file beside the saves when no
    /// clipboard takes it (a scripted capture never touches the player's).
    fn hand_over_report(&mut self, report: &str) -> String {
        if !self.scripted_run() && self.tools_page.clipboard.set_text(report).is_ok() {
            return "Report copied. Paste it into your bug report.".to_string();
        }
        let path = self.menu_runtime.save_dir.join(REPORT_FILE);
        let wrote = std::fs::create_dir_all(&self.menu_runtime.save_dir)
            .and_then(|()| std::fs::write(&path, report));
        match wrote {
            Ok(()) => format!("No clipboard here: wrote {REPORT_FILE} in the saves folder."),
            Err(e) => format!("The report could not be copied or written: {e}"),
        }
    }

    /// List the memory-card row's picks on its first press, with the cursor
    /// on the card port 2 holds now.
    fn list_cards(&mut self) {
        if self.tools_page.cards.is_some() {
            return;
        }
        let mounted = self.card.as_ref().and_then(|c| c.path.clone());
        let port_one = self.port1_card.as_ref().and_then(|c| c.path.clone());
        let remembered: Vec<PathBuf> = crate::launcher::settings_path()
            .and_then(|f| crate::launcher::LauncherSettings::load(&f).ok())
            .map(|s| s.card.into_iter().chain(s.recent_cards).collect())
            .unwrap_or_default();
        // The saves folder: where port 1's own card lives.
        let folders: Vec<PathBuf> = port_one
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .into_iter()
            .collect();
        let cards = card_picks(
            mounted.as_deref(),
            &remembered,
            &folders,
            port_one.as_deref(),
        );
        self.tools_page.card = mounted
            .and_then(|m| cards.iter().position(|p| *p == m))
            .map_or(0, |i| i + 1);
        self.tools_page.cards = Some(cards);
    }

    /// Read a code file and apply it the way the page's GameShark box does.
    fn apply_codes_file(&mut self, path: &Path) -> String {
        use legaia_engine_cheats::cheat_applier::{CheatTextFormat, apply_text};
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => return format!("{}: {e}", path.display()),
        };
        let world = &mut self.session.host.world;
        match apply_text(world, &text, CheatTextFormat::for_path(path, &text), false) {
            Ok(report) => report.summary(),
            Err(e) => format!("Codes not understood: {e}"),
        }
    }

    /// The Tools page's draws, in stage pixels.
    pub(super) fn tools_stage_draws(&self) -> Vec<TextDraw> {
        let heading = self.chrome().heading;
        let key_ink = self.chrome().key;
        let text = self.chrome().text;
        let changed = self.chrome().changed;
        let height = super::settings_panel::plate_height(SHOWN, NOTE_LINES);
        let mut out = Vec::new();
        self.push_scrim(
            &mut out,
            (X0, Y0),
            (WIDTH as u32, height as u32),
            super::settings_panel::PANEL_SCRIM,
        );
        let width = |s: &str| self.font.layout_ascii(s).advance_x as i32;
        let put = |out: &mut Vec<TextDraw>, s: &str, at: (i32, i32), color: [f32; 4]| {
            out.extend(legaia_engine_ui::text_draws_for(
                &self.font.layout_ascii(s),
                at,
                color,
            ));
        };
        let left = X0 + 6;
        let right = X0 + WIDTH - 6;
        put(&mut out, "Tools", (left, Y0 + 1), heading);
        put(
            &mut out,
            FLIP_HINT,
            (right - width(FLIP_HINT), Y0 + 1),
            text,
        );
        let mut y = Y0 + super::settings_panel::HEAD_H;
        let first = self.tools_page.first_shown();
        // The scroll bar on the plate's edge, as the settings page draws it:
        // without it nothing says the minigame rows sit under the last one
        // shown.
        if let Some((thumb_y, thumb_h)) =
            scroll_thumb(SHOWN, TOOL_COUNT, first, SHOWN as i32 * ROW_H)
            && let Some(src) = self.battle_hud_solid_src()
        {
            let x = X0 + WIDTH - 3;
            out.push(TextDraw {
                dst: (x, y, 1, (SHOWN as i32 * ROW_H) as u32),
                src,
                color: self.chrome().rule,
            });
            out.push(TextDraw {
                dst: (x, y + thumb_y, 1, thumb_h as u32),
                src,
                color: text,
            });
        }
        for (i, (row, label, _)) in TOOL_ROWS.iter().enumerate().skip(first).take(SHOWN) {
            let focused = i == self.tools_page.cursor;
            if focused {
                self.push_focus_row(&mut out, (X0, y), (WIDTH, ROW_H));
            }
            put(&mut out, label, (left, y - 2), text);
            let value = self.tool_value(*row);
            let shown = match (&value, focused) {
                (ToolValue::Switch(on), true) => format!("< {} >", if *on { "On" } else { "Off" }),
                (ToolValue::Switch(on), false) => if *on { "On" } else { "Off" }.to_string(),
                (ToolValue::Pick(v), true) => format!("< {v} >"),
                (ToolValue::Pick(v), false) => v.clone(),
                (ToolValue::Press(v), true) => self.panel_words(v),
                (ToolValue::Press(_), false) => String::new(),
            };
            let ink = match (&value, focused) {
                (ToolValue::Switch(true), _) => changed,
                (_, true) => key_ink,
                _ => text,
            };
            put(&mut out, &shown, (right - width(&shown), y - 2), ink);
            y += ROW_H;
        }
        y += super::settings_panel::ROWS_GAP;
        let note = self
            .tools_page
            .note
            .as_deref()
            .unwrap_or(TOOL_ROWS[self.tools_page.cursor].2);
        for line in super::settings_panel::wrap_lines(&self.panel_words(note), WIDTH - 12, &width)
            .iter()
            .take(NOTE_LINES)
        {
            put(&mut out, line, (left, y), text);
            y += HELP_H;
        }
        put(
            &mut out,
            &self.panel_controls(CONTROLS, PAD_CONTROLS),
            (left, super::settings_panel::controls_y(Y0, height)),
            heading,
        );
        out
    }
}

/// The scroll bar's thumb for a list of `total` rows showing `shown` from
/// row `first`, on a track `track_h` pixels tall: its offset down the track
/// and its height. `None` when every row shows.
fn scroll_thumb(shown: usize, total: usize, first: usize, track_h: i32) -> Option<(i32, i32)> {
    if total <= shown || shown == 0 {
        return None;
    }
    let thumb_h = (track_h * shown as i32 / total as i32).max(6);
    let max = (total - shown) as i32;
    let at = (first as i32).min(max);
    Some(((track_h - thumb_h) * at / max, thumb_h))
}

/// A battle-music pick's words.
fn bgm_pick_label(i: usize) -> String {
    match bgm_pick(i) {
        None => "Standard theme".into(),
        Some(0) => "Field track plays on".into(),
        Some(id) => legaia_engine_system::music_labels::label_for_bgm_id(id)
            .map(|l| shorten_label(&l, BGM_LABEL_CHARS))
            .unwrap_or_else(|| format!("Track {id}")),
    }
}

/// The most characters a battle-music pick shows beside its row name on
/// the plate; a longer track label is cut at a word and marked `...`.
const BGM_LABEL_CHARS: usize = 26;

/// `label` cut to at most `max` characters at a word boundary, with `...`
/// standing for the rest; a label that fits is returned whole.
fn shorten_label(label: &str, max: usize) -> String {
    if label.chars().count() <= max {
        return label.to_string();
    }
    let budget = max.saturating_sub(3);
    let mut out = String::new();
    for word in label.split(' ') {
        let next = if out.is_empty() {
            word.to_string()
        } else {
            format!("{out} {word}")
        };
        if next.chars().count() > budget {
            break;
        }
        out = next;
    }
    if out.is_empty() {
        out = label.chars().take(budget).collect();
    }
    format!("{out}...")
}

/// A spell pick's name off the disc's table, else its id.
fn spell_pick_label(world: &legaia_engine_core::world::World, id: u8) -> String {
    world
        .menu
        .text
        .as_ref()
        .and_then(|t| t.spell_names.as_ref())
        .and_then(|n| n.name(id))
        .filter(|s| !s.is_empty())
        .map_or_else(|| format!("Spell {id:#04x}"), str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scroll_thumb_spans_the_track_from_first_row_to_last() {
        let track = SHOWN as i32 * ROW_H;
        // A list that fits draws no bar.
        assert_eq!(scroll_thumb(SHOWN, SHOWN, 0, track), None);
        let (top, h) = scroll_thumb(SHOWN, TOOL_COUNT, 0, track).expect("the page scrolls");
        assert_eq!(top, 0);
        assert!(h >= 6 && h < track);
        // At the last page the thumb ends on the track's end.
        let (bottom, h2) =
            scroll_thumb(SHOWN, TOOL_COUNT, TOOL_COUNT - SHOWN, track).expect("the page scrolls");
        assert_eq!(h2, h);
        assert_eq!(bottom + h, track);
        // Every cursor position's first row keeps the thumb on the track.
        let mut p = ToolsPage::default();
        for _ in 0..TOOL_COUNT {
            let (y, h) = scroll_thumb(SHOWN, TOOL_COUNT, p.first_shown(), track).unwrap();
            assert!(y >= 0 && y + h <= track);
            p.move_cursor(1);
        }
    }

    /// The memory-card row offers the mounted card, the launcher's, then the
    /// saves folder's: each file once, never port 1's own, never a file that
    /// is gone or is not a card container by name.
    #[test]
    fn the_card_row_lists_mounted_then_remembered_then_folder_cards() {
        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str| {
            let p = dir.path().join(name);
            std::fs::write(&p, b"x").unwrap();
            p
        };
        let port_one = file("memcard1.mcr");
        let (b, a) = (file("b.mcd"), file("a.gme"));
        file("notes.txt");
        let other = tempfile::tempdir().unwrap();
        let remembered_card = other.path().join("epsxe000.mcr");
        std::fs::write(&remembered_card, b"x").unwrap();
        let gone = other.path().join("gone.mcr");

        let picks = card_picks(
            Some(&b),
            &[gone, remembered_card.clone(), b.clone(), port_one.clone()],
            &[dir.path().to_path_buf()],
            Some(&port_one),
        );
        assert_eq!(picks, vec![b, remembered_card, a]);
        // Nothing mounted, nothing remembered, no folder: an empty list,
        // which the row shows as the empty port alone.
        assert!(card_picks(None, &[], &[], None).is_empty());
    }

    #[test]
    fn a_long_card_name_keeps_its_start_and_its_extension() {
        assert_eq!(card_label(Path::new("/x/epsxe000.mcr")), "epsxe000.mcr");
        let long = card_label(Path::new("/x/Legend of Legaia (USA) playthrough two.mcr"));
        assert_eq!(long.chars().count(), CARD_LABEL_CHARS);
        assert!(
            long.starts_with("Legend of") && long.ends_with(".mcr"),
            "{long}"
        );
    }

    #[test]
    fn the_cursor_wraps_and_a_move_clears_the_refusal() {
        let mut p = ToolsPage {
            note: Some(REFUSED.into()),
            ..Default::default()
        };
        p.move_cursor(-1);
        assert_eq!(p.cursor, TOOL_COUNT - 1);
        assert_eq!(p.note, None);
        p.move_cursor(1);
        assert_eq!(p.cursor, 0);
    }

    /// The page stays on the stage with the row count it carries.
    #[test]
    fn a_session_file_never_lands_on_another() {
        let dir = tempfile::tempdir().unwrap();
        let saves = dir.path().join("saves");
        let a = write_session_file(&saves, "town01", b"one").unwrap();
        let b = write_session_file(&saves, "town01", b"two").unwrap();
        assert_eq!(a.file_name().unwrap(), "session_town01_001.lgsf");
        assert_eq!(b.file_name().unwrap(), "session_town01_002.lgsf");
        assert_eq!(std::fs::read(&a).unwrap(), b"one");
        assert_eq!(std::fs::read(&b).unwrap(), b"two");
        // A scene label is reduced to file-safe characters; none is "game".
        let c = write_session_file(&saves, "../../etc/x y", b"3").unwrap();
        assert_eq!(c.parent().unwrap(), saves);
        assert_eq!(c.file_name().unwrap(), "session_etcxy_001.lgsf");
        let d = write_session_file(&saves, "", b"4").unwrap();
        assert_eq!(d.file_name().unwrap(), "session_game_001.lgsf");
    }

    #[test]
    fn the_plate_fits_the_stage() {
        let h = super::super::settings_panel::plate_height(SHOWN, NOTE_LINES);
        assert!(Y0 + h <= legaia_engine_ui::BOOT_UI_STAGE_H as i32, "{h}");
    }

    /// A pick steps inside its list and leaves other rows alone.
    #[test]
    fn a_party_pick_reads_as_names() {
        assert_eq!(party_label("vahn"), "Vahn");
        assert_eq!(party_label("noa,vahn,gala"), "Noa, Vahn, Gala");
    }

    #[test]
    fn a_pick_wraps_its_own_list() {
        let mut p = ToolsPage::default();
        assert!(p.pick(ToolRow::Party, -1));
        assert_eq!(p.party, PARTY_PICKS.len() - 1);
        assert!(p.pick(ToolRow::BattleBgm, 1));
        assert_eq!(p.bgm, 1);
        assert_eq!(bgm_pick(p.bgm), Some(0));
        assert!(!p.pick(ToolRow::AutoBattle, 1));
    }

    /// The window of shown rows always holds the cursor.
    #[test]
    fn the_cursor_stays_on_the_plate() {
        let mut p = ToolsPage::default();
        for _ in 0..TOOL_COUNT {
            let first = p.first_shown();
            assert!(p.cursor >= first && p.cursor < first + SHOWN);
            p.move_cursor(1);
        }
    }

    /// A track label too long for the plate is cut at a word, with `...`.
    #[test]
    fn a_long_track_label_is_cut_at_a_word() {
        assert_eq!(shorten_label("M12 - Short", 26), "M12 - Short");
        assert_eq!(
            shorten_label("M47B - Another flying castle escape", 26),
            "M47B - Another flying..."
        );
        assert!(
            shorten_label("M47B - Another flying castle escape", 26)
                .chars()
                .count()
                <= 26
        );
    }

    /// Every help line and the controls line fit the plate, in the disc's
    /// dialog font the window letters them in.
    #[test]
    fn every_note_and_the_controls_fit_the_plate() {
        let font = std::env::var_os("LEGAIA_DISC_BIN")
            .and_then(|p| crate::launcher::disc_font(std::path::Path::new(&p)));
        if let Some(font) = font {
            let width = |s: &str| font.layout_ascii(s).advance_x as i32;
            for (_, label, help) in TOOL_ROWS {
                for table in super::super::settings_panel::widest_word_tables() {
                    let help = super::super::settings_panel::device_words(help, &table);
                    let lines = super::super::settings_panel::wrap_lines(&help, WIDTH - 12, &width);
                    assert!(lines.len() <= NOTE_LINES, "{label}: {lines:?}");
                }
                assert!(width(label) + width("< Off >") <= WIDTH - 12, "{label}");
            }
            for i in 0..BGM_PICKS {
                let v = bgm_pick_label(i);
                assert!(
                    width("Battle music") + width(&format!("< {v} >")) <= WIDTH - 12,
                    "{v}"
                );
            }
            for p in PARTY_PICKS {
                let v = party_label(p);
                assert!(
                    width("Party") + width(&format!("< {v} >")) <= WIDTH - 12,
                    "{v}"
                );
            }
            let lines = super::super::settings_panel::wrap_lines(REFUSED, WIDTH - 12, &width);
            assert!(lines.len() <= NOTE_LINES, "{lines:?}");
            assert!(width(CONTROLS) <= WIDTH - 12, "{}", width(CONTROLS));
            let [a, b] = super::super::settings_panel::WIDEST_PAD_LABELS;
            let pad = super::super::settings_panel::pad_controls(PAD_CONTROLS, a, b);
            assert!(width(&pad) <= WIDTH - 12, "{pad}: {}", width(&pad));
        }
    }
}
