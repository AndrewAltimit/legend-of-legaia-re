//! `cutscenes.toml`: scripted scenes.
//!
//! A cutscene is a list of **steps** a small deterministic sequencer runs
//! one frame at a time: walk an actor along tiles, turn it, play a clip,
//! move the camera, fade the screen, say a dialogue line and branch on its
//! answer, raise flags, hand out items, and end on a fight. Steps run in
//! order on the **main track**; a step with `wait = false` starts and lets
//! the next one run at once, and a named **track** runs a second list beside
//! the main one (`start_track` / `join`), so a camera sweep and a walk can
//! play together.
//!
//! ```toml
//! [cutscenes.tower_intro]
//! title = "The Tower Wakes"
//! map = "rebirth_tower_1f"
//!
//! [[cutscenes.tower_intro.steps]]
//! do = "fade"
//! to = "black"
//!
//! [[cutscenes.tower_intro.steps]]
//! do = "camera"
//! focus = { tile = [7, 2] }
//! pitch = -20
//! frames = 0
//!
//! [[cutscenes.tower_intro.steps]]
//! do = "fade"
//! to = "clear"
//! frames = 40
//!
//! [[cutscenes.tower_intro.steps]]
//! do = "move"
//! actor = "warden"
//! path = [[7, 8], [7, 9]]
//!
//! [[cutscenes.tower_intro.steps]]
//! do = "say"
//! line = "warden_greeting"
//! on_choice = ["accept", "refuse"]
//!
//! [[cutscenes.tower_intro.steps]]
//! do = "label"
//! name = "accept"
//! ```
//!
//! The steps are the modder-facing layer: readable, and checked as a whole
//! by the validator (every actor, line, flag, label and track it names
//! exists; no step is unreachable; no loop spins without a wait). The engine
//! runs them on its own sequencer (`legaia_engine_expansion::cutscene`), not
//! on the retail field VM: a pack carries no field-VM bytecode, so it can
//! never write a retail story flag or reach a retail script's state.
//!
//! What starts one: a map trigger (`kind = "cutscene"`), a map's
//! `enter_cutscenes` (on arrival), a pack NPC's `cutscene` (talking to it),
//! and a quest's `start_cutscene` / `complete_cutscene`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::quest::Prerequisites;
use crate::refs::DiscRef;

/// The `cutscenes.toml` section.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CutsceneSection {
    /// Every cutscene, keyed by its pack-unique id (`[cutscenes.<id>]`).
    pub cutscenes: BTreeMap<String, CutsceneDef>,
}

/// One cutscene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CutsceneDef {
    /// Display title, for the workbench and the journal of what was seen.
    pub title: String,
    /// The pack map the cutscene plays on. When set, its actors are that
    /// map's NPCs and its tiles that map's local tiles, the validator checks
    /// them against it, and the engine refuses to start it anywhere else.
    /// `None` plays on whatever field scene is running: tiles are then the
    /// scene's field-grid tiles and the actors are the player and retail
    /// placements (`retail:<slot>`).
    pub map: Option<String>,
    /// Whether the Start button skips the cutscene. A cutscene is skippable
    /// only once it has been seen to the end (or skipped) before.
    pub skippable: bool,
    /// Whether the player is locked for the whole cutscene (no walk, no
    /// talk, no menu, no encounters). A `lock` / `unlock` step changes it
    /// midway.
    pub lock_player: bool,
    /// Cinematic bars over the top and bottom of the screen while it runs.
    pub letterbox: bool,
    /// The main track.
    pub steps: Vec<Step>,
    /// Named tracks a `start_track` step runs beside the main one.
    pub tracks: BTreeMap<String, TrackDef>,
}

impl Default for CutsceneDef {
    fn default() -> Self {
        Self {
            title: String::new(),
            map: None,
            skippable: true,
            lock_player: true,
            letterbox: false,
            steps: Vec::new(),
            tracks: BTreeMap::new(),
        }
    }
}

impl CutsceneDef {
    /// Every step of the main track and every named track.
    pub fn all_steps(&self) -> impl Iterator<Item = &Step> {
        self.steps
            .iter()
            .chain(self.tracks.values().flat_map(|t| t.steps.iter()))
    }
}

/// A named track.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackDef {
    pub steps: Vec<Step>,
}

/// The pack flag raised once a cutscene has been seen to the end or
/// skipped: `cutscene.<id>`. It makes the cutscene skippable from then on
/// and keeps a map's `enter_cutscenes` from playing it again.
pub fn seen_flag(id: &str) -> String {
    format!("cutscene.{id}")
}

/// Who a step moves: `"player"` (the party leader), the id of a pack NPC on
/// the cutscene's map, or `"retail:<slot>"` for a retail scene's placement
/// `slot` (a cutscene with no `map`).
pub type ActorId = String;

/// The player's actor id.
pub const PLAYER: &str = "player";

/// The placement slot a `retail:<slot>` actor id names.
pub fn retail_slot(actor: &str) -> Option<u8> {
    actor.strip_prefix("retail:")?.parse().ok()
}

/// Where a camera step looks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CameraFocus {
    /// `{ tile = [c, r] }`: the centre of a tile.
    Tile { tile: [u16; 2] },
    /// `{ actor = "id" }`: an actor, followed while it moves.
    Actor { actor: ActorId },
}

/// The screen level a fade ends at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FadeTo {
    /// Fully black.
    #[default]
    Black,
    /// Fully white.
    White,
    /// No fade: the scene shows.
    Clear,
}

/// How a tint mixes onto the frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TintMode {
    /// The colour is added (a glow).
    #[default]
    Add,
    /// The colour is subtracted (a shade: `[0, 80, 160]` warms the frame).
    Subtract,
}

fn yes() -> bool {
    true
}

fn one_u32() -> u32 {
    1
}

fn one_i64() -> i64 {
    1
}

fn default_music_fade() -> u16 {
    30
}

/// One step. The `do` key selects the variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Step {
    /// Wait `frames` display frames (60 a second).
    Wait { frames: u32 },
    /// Show a `dialog.toml` line and wait for it to close. `on_choice`
    /// names a label for each of the line's options in order (`""` goes on
    /// to the next step); a line with choices and no `on_choice` goes on
    /// whatever is picked. The choice's own effects (flags, `next`, a fight)
    /// apply as they do for any pack line.
    Say {
        line: String,
        #[serde(default)]
        on_choice: Vec<String>,
    },
    /// Walk an actor to `to`, or along `path` (tile by tile, in order). The
    /// player walks at `speed` world units a frame (`8` walking, `12` with
    /// `run`); an NPC at its own glide speed unless `speed` is given. `clip`
    /// is the clip the walk plays (an NPC's `walk_clip` when unset).
    Move {
        actor: ActorId,
        #[serde(default)]
        to: Option<[u16; 2]>,
        #[serde(default)]
        path: Vec<[u16; 2]>,
        #[serde(default)]
        speed: Option<u16>,
        #[serde(default)]
        run: bool,
        #[serde(default)]
        clip: Option<u8>,
        #[serde(default = "yes")]
        wait: bool,
    },
    /// Turn an actor to compass sector `facing` (`0..8`, the spawn table),
    /// or toward another actor.
    Face {
        actor: ActorId,
        #[serde(default)]
        facing: Option<u8>,
        #[serde(default)]
        toward: Option<ActorId>,
    },
    /// Play clip `clip` on an actor (an NPC's clip id; the player's clip
    /// base `0..7` of its bank). With `frames`, wait that long and put the
    /// idle clip back.
    Clip {
        actor: ActorId,
        clip: u8,
        #[serde(default)]
        frames: Option<u32>,
    },
    /// Put an actor on `tile` at once, facing `facing` when given.
    Teleport {
        actor: ActorId,
        tile: [u16; 2],
        #[serde(default)]
        facing: Option<u8>,
    },
    /// Show a pack NPC hidden by `hide` (or absent because its `present`
    /// gate is shut), at `tile` when given.
    Show {
        npc: String,
        #[serde(default)]
        tile: Option<[u16; 2]>,
    },
    /// Hide a pack NPC: it neither draws, talks nor blocks.
    Hide { npc: String },
    /// Move the camera over `frames` (`0` cuts). Each of `focus`, `yaw`,
    /// `pitch` and `zoom` is optional and keeps its current value when left
    /// out; all are measured from the follow camera's own shot - `yaw` and
    /// `pitch` in degrees added to it, `zoom` a factor on its distance
    /// (`> 1` pulls back).
    Camera {
        #[serde(default)]
        focus: Option<CameraFocus>,
        #[serde(default)]
        yaw: Option<f32>,
        #[serde(default)]
        pitch: Option<f32>,
        #[serde(default)]
        zoom: Option<f32>,
        #[serde(default)]
        frames: u32,
        #[serde(default = "yes")]
        wait: bool,
    },
    /// Ease the camera back to the follow camera over `frames`.
    CameraReset {
        #[serde(default)]
        frames: u32,
        #[serde(default = "yes")]
        wait: bool,
    },
    /// Shake the camera, `amplitude` world units at the start, dying out
    /// over `frames`.
    Shake { amplitude: u16, frames: u32 },
    /// Fade the screen to black, white or clear over `frames`.
    Fade {
        to: FadeTo,
        #[serde(default)]
        frames: u32,
        #[serde(default = "yes")]
        wait: bool,
    },
    /// A flash of `color` that dies out over `frames`.
    Flash { color: [u8; 3], frames: u32 },
    /// A title across the lower screen - a boss's name, a place - in the
    /// place-name banner's balloon: the first page of `dialog.toml` line
    /// `line`, its rows joined. It does not wait.
    Banner { line: String },
    /// Hold a colour over the frame until changed; `color = [0, 0, 0]`
    /// clears it.
    Tint {
        color: [u8; 3],
        #[serde(default)]
        frames: u32,
        #[serde(default)]
        mode: TintMode,
    },
    /// Raise a pack flag.
    SetFlag { flag: String },
    /// Clear a pack flag.
    ClearFlag { flag: String },
    /// Add `add` (default 1) to a pack counter.
    Counter {
        counter: String,
        #[serde(default = "one_i64")]
        add: i64,
    },
    /// A place a `goto`, a `branch` or an `on_choice` can jump to.
    Label { name: String },
    /// Jump to a label.
    Goto { label: String },
    /// Jump to `goto` when `requires` holds (and `counter` is at least
    /// `at_least`, when named), else to `otherwise` (the next step when
    /// unset).
    Branch {
        #[serde(default)]
        requires: Prerequisites,
        #[serde(default)]
        counter: Option<String>,
        #[serde(default)]
        at_least: i64,
        goto: String,
        #[serde(default)]
        otherwise: Option<String>,
    },
    /// Start a named track beside this one.
    StartTrack { track: String },
    /// Wait for a named track to finish.
    Join { track: String },
    /// Lock the player.
    Lock,
    /// Unlock the player: it walks while the cutscene goes on.
    Unlock,
    /// End the cutscene and fight a `battle.toml` formation.
    Battle { formation: String },
    /// End the cutscene and fight a `battle.toml` boss.
    Boss { boss: String },
    /// Give `count` of a retail item.
    GiveItem {
        item: u8,
        #[serde(default = "one_u32")]
        count: u32,
    },
    /// Take up to `count` of a retail item.
    TakeItem {
        item: u8,
        #[serde(default = "one_u32")]
        count: u32,
    },
    /// Give (or with a negative amount, take) gold.
    Gold { amount: i64 },
    /// Give a `cards.toml` card.
    GiveCard { card: String },
    /// Start a `quests.toml` quest.
    StartQuest { quest: String },
    /// Play a sound effect by its `DAT_8006F198` table id
    /// (`docs/formats/sfx-table.md`).
    Sfx { id: u16 },
    /// Switch the music to a sound-test track ([`DiscRef::Bgm`]).
    Bgm { bgm: DiscRef },
    /// Play a retail cue or a pack sound (`audio.toml`,
    /// `docs/formats/expansion-pack-audio.md`).
    Sound { sound: crate::audio::SoundRef },
    /// Cross-fade to a retail cue, a pack track or silence over `fade`
    /// frames, held over the map's music zones until a `music_release`.
    Music {
        music: crate::audio::MusicRef,
        #[serde(default = "default_music_fade")]
        fade: u16,
    },
    /// Hand the music back to the map and its zones.
    MusicRelease,
    /// End the cutscene here.
    End,
}

/// A camera zoom factor's range.
pub const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.25..=4.0;
/// A camera pitch offset's range, in degrees.
pub const PITCH_RANGE: std::ops::RangeInclusive<f32> = -60.0..=60.0;
/// The longest a single step's frame count may be (ten minutes).
pub const MAX_FRAMES: u32 = 36_000;
/// The largest shake amplitude, world units.
pub const MAX_SHAKE: u16 = 64;
/// How many steps the sequencer runs on one track in one frame before it
/// gives up on the cutscene (a loop that never waits).
pub const STEP_BUDGET: usize = 512;

impl Step {
    /// The step's `do` key.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Wait { .. } => "wait",
            Self::Say { .. } => "say",
            Self::Move { .. } => "move",
            Self::Face { .. } => "face",
            Self::Clip { .. } => "clip",
            Self::Teleport { .. } => "teleport",
            Self::Show { .. } => "show",
            Self::Hide { .. } => "hide",
            Self::Camera { .. } => "camera",
            Self::CameraReset { .. } => "camera_reset",
            Self::Shake { .. } => "shake",
            Self::Fade { .. } => "fade",
            Self::Flash { .. } => "flash",
            Self::Banner { .. } => "banner",
            Self::Tint { .. } => "tint",
            Self::SetFlag { .. } => "set_flag",
            Self::ClearFlag { .. } => "clear_flag",
            Self::Counter { .. } => "counter",
            Self::Label { .. } => "label",
            Self::Goto { .. } => "goto",
            Self::Branch { .. } => "branch",
            Self::StartTrack { .. } => "start_track",
            Self::Join { .. } => "join",
            Self::Lock => "lock",
            Self::Unlock => "unlock",
            Self::Battle { .. } => "battle",
            Self::Boss { .. } => "boss",
            Self::GiveItem { .. } => "give_item",
            Self::TakeItem { .. } => "take_item",
            Self::Gold { .. } => "gold",
            Self::GiveCard { .. } => "give_card",
            Self::StartQuest { .. } => "start_quest",
            Self::Sfx { .. } => "sfx",
            Self::Bgm { .. } => "bgm",
            Self::Sound { .. } => "sound",
            Self::Music { .. } => "music",
            Self::MusicRelease => "music_release",
            Self::End => "end",
        }
    }

    /// Whether the step can hold its track for a frame or more: the steps
    /// that make a loop through them safe.
    pub fn may_wait(&self) -> bool {
        match self {
            Self::Wait { frames } => *frames > 0,
            Self::Say { .. } | Self::Join { .. } => true,
            Self::Clip { frames, .. } => frames.is_some_and(|f| f > 0),
            Self::Move { wait, .. } => *wait,
            Self::Camera { wait, frames, .. } | Self::CameraReset { wait, frames } => {
                *wait && *frames > 0
            }
            Self::Fade { wait, frames, .. } => *wait && *frames > 0,
            _ => false,
        }
    }

    /// Whether control never falls through to the next step.
    pub fn ends_flow(&self) -> bool {
        matches!(
            self,
            Self::Goto { .. } | Self::End | Self::Battle { .. } | Self::Boss { .. }
        )
    }

    /// The labels the step can jump to.
    pub fn targets(&self) -> Vec<&str> {
        match self {
            Self::Goto { label } => vec![label.as_str()],
            Self::Branch {
                goto, otherwise, ..
            } => std::iter::once(goto.as_str())
                .chain(otherwise.as_deref())
                .collect(),
            Self::Say { on_choice, .. } => on_choice
                .iter()
                .map(String::as_str)
                .filter(|l| !l.is_empty())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The actors the step names.
    pub fn actors(&self) -> Vec<&str> {
        match self {
            Self::Move { actor, .. } | Self::Clip { actor, .. } | Self::Teleport { actor, .. } => {
                vec![actor.as_str()]
            }
            Self::Face { actor, toward, .. } => std::iter::once(actor.as_str())
                .chain(toward.as_deref())
                .collect(),
            Self::Camera {
                focus: Some(CameraFocus::Actor { actor }),
                ..
            } => vec![actor.as_str()],
            Self::Show { npc, .. } | Self::Hide { npc } => vec![npc.as_str()],
            _ => Vec::new(),
        }
    }

    /// The tiles the step names.
    pub fn tiles(&self) -> Vec<[u16; 2]> {
        match self {
            Self::Move { to, path, .. } => to.iter().chain(path).copied().collect(),
            Self::Teleport { tile, .. } => vec![*tile],
            Self::Show { tile, .. } => tile.iter().copied().collect(),
            Self::Camera {
                focus: Some(CameraFocus::Tile { tile }),
                ..
            } => vec![*tile],
            _ => Vec::new(),
        }
    }

    /// Each step's waypoints, `to` then `path`.
    pub fn waypoints(&self) -> Vec<[u16; 2]> {
        match self {
            Self::Move { to, path, .. } => path.iter().chain(to.iter()).copied().collect(),
            _ => Vec::new(),
        }
    }
}

impl CutsceneSection {
    /// The cutscene `id`.
    pub fn get(&self, id: &str) -> Option<&CutsceneDef> {
        self.cutscenes.get(id)
    }
}

/// The labels a step list declares, `name -> index`. A repeated name keeps
/// the first.
pub fn labels(steps: &[Step]) -> BTreeMap<&str, usize> {
    let mut out = BTreeMap::new();
    for (i, s) in steps.iter().enumerate() {
        if let Step::Label { name } = s {
            out.entry(name.as_str()).or_insert(i);
        }
    }
    out
}

/// The steps of `steps` no flow reaches from the first one, by index.
pub fn unreachable_steps(steps: &[Step]) -> Vec<usize> {
    let labels = labels(steps);
    let mut seen = vec![false; steps.len()];
    let mut todo = vec![0usize];
    while let Some(i) = todo.pop() {
        if i >= steps.len() || seen[i] {
            continue;
        }
        seen[i] = true;
        let s = &steps[i];
        for t in s.targets() {
            if let Some(&j) = labels.get(t) {
                todo.push(j);
            }
        }
        if !s.ends_flow() {
            todo.push(i + 1);
        }
    }
    seen.iter()
        .enumerate()
        .filter(|(_, s)| !**s)
        .map(|(i, _)| i)
        .collect()
}

/// A loop no frame ever ends: a step index from which the flow returns to
/// itself through steps none of which can wait ([`Step::may_wait`]).
pub fn spinning_loop(steps: &[Step]) -> Option<usize> {
    let labels = labels(steps);
    for start in 0..steps.len() {
        // Follow every non-waiting path from `start`; a return to `start`
        // is a spin.
        let mut seen = BTreeSet::new();
        let mut todo = vec![start];
        while let Some(i) = todo.pop() {
            let Some(s) = steps.get(i) else { continue };
            if s.may_wait() || !seen.insert(i) {
                continue;
            }
            let mut next: Vec<usize> = s
                .targets()
                .iter()
                .filter_map(|t| labels.get(t).copied())
                .collect();
            if !s.ends_flow() {
                next.push(i + 1);
            }
            for j in next {
                if j == start {
                    return Some(start);
                }
                todo.push(j);
            }
        }
    }
    None
}

impl crate::validate::SectionRules for CutsceneSection {
    fn declare(&self, cx: &mut crate::validate::Checker<'_>) {
        for id in self.cutscenes.keys() {
            cx.declare("cutscene", id, &format!("cutscenes.{id}"));
        }
    }

    fn check(&self, cx: &mut crate::validate::Checker<'_>) {
        for (id, c) in &self.cutscenes {
            check_cutscene(cx, id, c);
        }
    }
}

/// What a cutscene's map offers its steps: NPC ids and the grid size.
struct MapView {
    npcs: BTreeSet<String>,
    size: Option<(usize, usize)>,
}

fn check_cutscene(cx: &mut crate::validate::Checker<'_>, id: &str, c: &CutsceneDef) {
    let at = format!("cutscenes.{id}");
    let pack = cx.pack();
    let map = match &c.map {
        Some(m) => match pack.maps.get(m) {
            Some(def) => Some(MapView {
                npcs: def.npcs.iter().map(|n| n.id.clone()).collect(),
                size: def.tile_grid().ok().map(|g| (g.width, g.height)),
            }),
            None => {
                cx.error(&format!("{at}.map"), format!("no map {m:?} in this pack"));
                None
            }
        },
        None => None,
    };
    if c.steps.is_empty() {
        cx.warning(&format!("{at}.steps"), "the cutscene has no steps");
    }
    let tracks: Vec<(String, &[Step])> = std::iter::once((format!("{at}.steps"), &c.steps[..]))
        .chain(
            c.tracks
                .iter()
                .map(|(t, d)| (format!("{at}.tracks.{t}.steps"), &d.steps[..])),
        )
        .collect();
    let mut started = BTreeSet::new();
    for (track_at, steps) in &tracks {
        check_steps(cx, c, map.as_ref(), track_at, steps, &mut started);
    }
    for t in c.tracks.keys() {
        if !started.contains(t.as_str()) {
            cx.warning(
                &format!("{at}.tracks.{t}"),
                "no start_track step starts this track",
            );
        }
    }
}

fn check_steps(
    cx: &mut crate::validate::Checker<'_>,
    c: &CutsceneDef,
    map: Option<&MapView>,
    at: &str,
    steps: &[Step],
    started: &mut BTreeSet<String>,
) {
    let labels = labels(steps);
    let mut names = BTreeSet::new();
    for (i, s) in steps.iter().enumerate() {
        let sat = format!("{at}[{i}]");
        if let Step::Label { name } = s
            && !names.insert(name.as_str())
        {
            cx.error(&sat, format!("label {name:?} is defined twice"));
        }
        for t in s.targets() {
            if !labels.contains_key(t) {
                cx.error(
                    &sat,
                    format!("no label {t:?} on this track (a jump stays on its own track)"),
                );
            }
        }
        for a in s.actors() {
            check_actor(
                cx,
                map,
                &sat,
                a,
                matches!(s, Step::Show { .. } | Step::Hide { .. }),
            );
        }
        if let Some(m) = map
            && let Some((w, h)) = m.size
        {
            for [col, row] in s.tiles() {
                if usize::from(col) >= w || usize::from(row) >= h {
                    cx.error(
                        &sat,
                        format!("tile ({col}, {row}) is off the {w} x {h} map"),
                    );
                }
            }
        }
        check_step(cx, c, &sat, s, started);
    }
    for i in unreachable_steps(steps) {
        // A lone trailing label is harmless; any other step is dead.
        if !matches!(steps[i], Step::Label { .. }) {
            cx.error(
                &format!("{at}[{i}]"),
                format!("the `{}` step is unreachable", steps[i].kind()),
            );
        }
    }
    if let Some(i) = spinning_loop(steps) {
        cx.error(
            &format!("{at}[{i}]"),
            "a loop through here never waits a frame; put a wait, a say or a blocking move in it",
        );
    }
}

fn check_actor(
    cx: &mut crate::validate::Checker<'_>,
    map: Option<&MapView>,
    at: &str,
    actor: &str,
    npc_only: bool,
) {
    if actor == PLAYER {
        if npc_only {
            cx.error(at, "only a pack NPC is shown or hidden");
        }
        return;
    }
    if let Some(slot) = retail_slot(actor) {
        if map.is_some() {
            cx.error(
                at,
                format!("{actor:?} is a retail placement; a cutscene on a pack map moves its NPCs"),
            );
        } else if npc_only {
            cx.error(at, "only a pack NPC is shown or hidden");
        } else if slot == 0 || slot >= 0xF0 {
            cx.error(at, format!("placement slot {slot} is not an NPC (1..240)"));
        }
        return;
    }
    match map {
        Some(m) if m.npcs.contains(actor) => {}
        Some(_) => cx.error(at, format!("the cutscene's map has no NPC {actor:?}")),
        None => cx.error(
            at,
            format!(
                "actor {actor:?} needs the cutscene's `map` (it names a pack NPC), or is \
                 `player` / `retail:<slot>`"
            ),
        ),
    }
}

fn check_frames(cx: &mut crate::validate::Checker<'_>, at: &str, frames: u32) {
    if frames > MAX_FRAMES {
        cx.error(
            at,
            format!("{frames} frames is over the {MAX_FRAMES}-frame limit"),
        );
    }
}

fn check_step(
    cx: &mut crate::validate::Checker<'_>,
    c: &CutsceneDef,
    at: &str,
    s: &Step,
    started: &mut BTreeSet<String>,
) {
    let pack = cx.pack();
    match s {
        Step::Wait { frames } => check_frames(cx, at, *frames),
        Step::Say { line, on_choice } => {
            cx.reference("line", line, &format!("{at}.line"));
            if let Some(l) = pack.dialog.lines.get(line) {
                if !on_choice.is_empty() && l.choices.is_empty() {
                    cx.error(
                        &format!("{at}.on_choice"),
                        format!("line {line:?} has no choices to branch on"),
                    );
                } else if on_choice.len() > l.choices.len() {
                    cx.error(
                        &format!("{at}.on_choice"),
                        format!(
                            "{} labels for the {} options of line {line:?}",
                            on_choice.len(),
                            l.choices.len()
                        ),
                    );
                }
            }
        }
        Step::Move {
            to, path, speed, ..
        } => {
            if to.is_none() && path.is_empty() {
                cx.error(at, "a move names `to` or a `path`");
            }
            if speed.is_some_and(|v| v == 0 || v > 64) {
                cx.error(
                    &format!("{at}.speed"),
                    "a speed is 1..=64 world units a frame",
                );
            }
        }
        Step::Face { facing, toward, .. } => {
            if facing.is_none() == toward.is_none() {
                cx.error(at, "a face step names one of `facing` or `toward`");
            }
            if facing.is_some_and(|f| f >= 8) {
                cx.error(&format!("{at}.facing"), "a facing is a compass sector 0..8");
            }
        }
        Step::Teleport { facing, .. } => {
            if facing.is_some_and(|f| f >= 8) {
                cx.error(&format!("{at}.facing"), "a facing is a compass sector 0..8");
            }
        }
        Step::Clip { frames, .. } => {
            if let Some(f) = frames {
                check_frames(cx, at, *f);
            }
        }
        Step::Camera {
            pitch,
            zoom,
            frames,
            ..
        } => {
            check_frames(cx, at, *frames);
            if zoom.is_some_and(|z| !ZOOM_RANGE.contains(&z)) {
                cx.error(
                    &format!("{at}.zoom"),
                    format!("zoom is {}..={}", ZOOM_RANGE.start(), ZOOM_RANGE.end()),
                );
            }
            if pitch.is_some_and(|p| !PITCH_RANGE.contains(&p)) {
                cx.error(
                    &format!("{at}.pitch"),
                    format!(
                        "pitch is {}..={} degrees",
                        PITCH_RANGE.start(),
                        PITCH_RANGE.end()
                    ),
                );
            }
        }
        Step::CameraReset { frames, .. } | Step::Fade { frames, .. } => {
            check_frames(cx, at, *frames)
        }
        Step::Flash { frames, .. } | Step::Tint { frames, .. } => check_frames(cx, at, *frames),
        Step::Banner { line } => {
            cx.reference("line", line, &format!("{at}.line"));
            if let Some(l) = pack.dialog.lines.get(line) {
                let t = crate::boss_script::banner_text(l);
                if t.chars().count() > crate::boss_script::BANNER_WARN_CHARS {
                    cx.warning(
                        &format!("{at}.line"),
                        format!(
                            "line {line:?} reads {} characters as a banner; it holds about {}",
                            t.chars().count(),
                            crate::boss_script::BANNER_WARN_CHARS
                        ),
                    );
                }
            }
        }
        Step::Shake { amplitude, frames } => {
            check_frames(cx, at, *frames);
            if *amplitude > MAX_SHAKE {
                cx.error(
                    &format!("{at}.amplitude"),
                    format!("a shake is at most {MAX_SHAKE} world units"),
                );
            }
        }
        Step::SetFlag { flag } | Step::ClearFlag { flag } | Step::Counter { counter: flag, .. } => {
            if flag.trim().is_empty() {
                cx.error(at, "an empty pack flag or counter id");
            }
        }
        Step::Label { name } => {
            if name.trim().is_empty() {
                cx.error(at, "a label has a name");
            }
        }
        Step::Goto { .. } | Step::End | Step::Lock | Step::Unlock => {}
        Step::Show { .. } | Step::Hide { .. } => {}
        Step::Branch {
            requires, counter, ..
        } => {
            requires.check(cx, &format!("{at}.requires"));
            if counter.as_deref().is_some_and(|c| c.trim().is_empty()) {
                cx.error(&format!("{at}.counter"), "an empty pack counter id");
            }
        }
        Step::StartTrack { track } | Step::Join { track } => {
            if !c.tracks.contains_key(track) {
                cx.error(&format!("{at}.track"), format!("no track {track:?}"));
            }
            if matches!(s, Step::StartTrack { .. }) {
                started.insert(track.clone());
            }
        }
        Step::Battle { formation } => {
            cx.reference("formation", formation, &format!("{at}.formation"))
        }
        Step::Boss { boss } => cx.reference("boss", boss, &format!("{at}.boss")),
        Step::GiveItem { count, .. } | Step::TakeItem { count, .. } => {
            if *count == 0 || *count > 99 {
                cx.error(&format!("{at}.count"), "a count is 1..=99");
            }
        }
        Step::Gold { .. } => {}
        Step::GiveCard { card } => {
            if pack.cards.card(card).is_none() {
                cx.error(
                    &format!("{at}.card"),
                    format!("no cards.toml card {card:?}"),
                );
            }
        }
        Step::StartQuest { quest } => cx.reference("quest", quest, &format!("{at}.quest")),
        Step::Sfx { .. } | Step::MusicRelease => {}
        Step::Sound { sound } => crate::audio::check_sound(cx, &format!("{at}.sound"), sound),
        Step::Music { music, fade } => {
            crate::audio::check_music(cx, &format!("{at}.music"), music);
            if *fade > crate::audio::MAX_FADE_FRAMES {
                cx.error(
                    &format!("{at}.fade"),
                    format!(
                        "a fade runs at most {} frames",
                        crate::audio::MAX_FADE_FRAMES
                    ),
                );
            }
        }
        Step::Bgm { bgm } => {
            if matches!(bgm, DiscRef::Bgm { .. }) {
                cx.disc_ref(&format!("{at}.bgm"), bgm);
            } else {
                cx.error(&format!("{at}.bgm"), format!("{bgm} is not a bgm ref"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> CutsceneSection {
        toml::from_str(text).expect("cutscenes parse")
    }

    const SCENE: &str = r#"
[cutscenes.intro]
title = "Intro"
map = "m"
letterbox = true

[[cutscenes.intro.steps]]
do = "fade"
to = "black"

[[cutscenes.intro.steps]]
do = "start_track"
track = "pan"

[[cutscenes.intro.steps]]
do = "move"
actor = "guide"
path = [[1, 1], [2, 1]]
wait = false

[[cutscenes.intro.steps]]
do = "say"
line = "offer"
on_choice = ["yes", ""]

[[cutscenes.intro.steps]]
do = "goto"
label = "out"

[[cutscenes.intro.steps]]
do = "label"
name = "yes"

[[cutscenes.intro.steps]]
do = "set_flag"
flag = "agreed"

[[cutscenes.intro.steps]]
do = "label"
name = "out"

[[cutscenes.intro.steps]]
do = "join"
track = "pan"

[cutscenes.intro.tracks.pan]
steps = [
  { do = "camera", focus = { tile = [3, 3] }, pitch = -10.0, zoom = 1.5, frames = 90 },
  { do = "camera_reset", frames = 30 },
]
"#;

    #[test]
    fn a_cutscene_parses_with_tracks_and_defaults() {
        let s = parse(SCENE);
        let c = s.get("intro").unwrap();
        assert!(c.skippable && c.lock_player && c.letterbox);
        assert_eq!(c.steps.len(), 9);
        assert_eq!(
            c.steps[0],
            Step::Fade {
                to: FadeTo::Black,
                frames: 0,
                wait: true
            }
        );
        match &c.steps[2] {
            Step::Move {
                path, wait, run, ..
            } => {
                assert_eq!(path, &[[1, 1], [2, 1]]);
                assert!(!wait && !run);
            }
            other => panic!("{other:?}"),
        }
        let pan = &c.tracks["pan"].steps;
        assert!(matches!(
            &pan[0],
            Step::Camera { focus: Some(CameraFocus::Tile { tile: [3, 3] }), zoom: Some(z), .. } if *z == 1.5
        ));
        // Round trip.
        let text = toml::to_string(&s).unwrap();
        assert_eq!(parse(&text), s);
    }

    #[test]
    fn unreachable_steps_are_the_ones_past_a_jump_without_a_label() {
        let s = parse(SCENE);
        assert!(unreachable_steps(&s.get("intro").unwrap().steps).is_empty());
        let steps = vec![
            Step::Goto { label: "a".into() },
            Step::SetFlag {
                flag: "dead".into(),
            },
            Step::Label { name: "a".into() },
            Step::End,
            Step::Wait { frames: 1 },
        ];
        assert_eq!(unreachable_steps(&steps), vec![1, 4]);
    }

    #[test]
    fn a_loop_without_a_wait_spins() {
        let spin = vec![
            Step::Label { name: "top".into() },
            Step::Counter {
                counter: "n".into(),
                add: 1,
            },
            Step::Goto {
                label: "top".into(),
            },
        ];
        assert_eq!(spinning_loop(&spin), Some(0));
        let mut ok = spin.clone();
        ok.insert(1, Step::Wait { frames: 1 });
        assert_eq!(spinning_loop(&ok), None);
    }

    fn pack_with(cutscenes: &str) -> crate::ExpansionPack {
        let mut files = crate::PackFiles::new();
        files.insert(
            "pack.toml".into(),
            b"id = \"t\"\nname = \"T\"\nversion = \"1.0.0\"\nschema = 1\ndisc = \"SCUS-94254\"\n"
                .to_vec(),
        );
        files.insert(
            "dialog.toml".into(),
            br#"
[lines.offer]
text = "Go?"
choices = [{ label = "Yes" }, { label = "No" }]
[lines.plain]
text = "Hello."
"#
            .to_vec(),
        );
        files.insert(
            "maps/m.toml".into(),
            br#"
[tiles]
rows = ["....", "...."]
[tiles.legend]
"." = {}
[[npcs]]
id = "guide"
model = { kind = "npc", scene = "town01", index = 3 }
tile = [0, 0]
"#
            .to_vec(),
        );
        files.insert("cutscenes.toml".into(), cutscenes.as_bytes().to_vec());
        crate::ExpansionPack::from_files(&files).expect("pack parses")
    }

    fn errors(pack: &crate::ExpansionPack) -> Vec<String> {
        crate::validate::validate(pack)
            .into_iter()
            .filter(|i| {
                i.severity == crate::validate::Severity::Error && i.path == "cutscenes.toml"
            })
            .map(|i| format!("{} {}", i.field, i.message))
            .collect()
    }

    #[test]
    fn a_sound_cutscene_validates_clean() {
        let pack = pack_with(
            r#"
[cutscenes.c]
map = "m"
steps = [
  { do = "move", actor = "guide", to = [3, 1] },
  { do = "face", actor = "player", toward = "guide" },
  { do = "say", line = "offer", on_choice = ["yes", ""] },
  { do = "end" },
  { do = "label", name = "yes" },
  { do = "set_flag", flag = "agreed" },
]
"#,
        );
        assert_eq!(errors(&pack), Vec::<String>::new());
    }

    #[test]
    fn the_validator_names_every_broken_reference() {
        let pack = pack_with(
            r#"
[cutscenes.c]
map = "m"
steps = [
  { do = "move", actor = "ghost", to = [9, 9] },
  { do = "say", line = "plain", on_choice = ["x"] },
  { do = "say", line = "missing" },
  { do = "goto", label = "nowhere" },
  { do = "set_flag", flag = "dead" },
  { do = "start_track", track = "none" },
  { do = "camera", zoom = 9.0 },
  { do = "face", actor = "player" },
]
"#,
        );
        let e = errors(&pack).join("\n");
        for want in [
            "no NPC \"ghost\"",
            "off the 4 x 2 map",
            "has no choices",
            "line \"missing\" is not defined",
            "no label \"nowhere\"",
            "unreachable",
            "no track \"none\"",
            "zoom is",
            "one of `facing` or `toward`",
        ] {
            assert!(e.contains(want), "missing {want:?} in:\n{e}");
        }
    }

    #[test]
    fn a_cutscene_without_a_map_takes_only_the_player_and_retail_slots() {
        let pack = pack_with(
            r#"
[cutscenes.c]
steps = [
  { do = "move", actor = "retail:4", to = [30, 40] },
  { do = "move", actor = "guide", to = [1, 1] },
  { do = "hide", npc = "retail:4" },
]
"#,
        );
        let e = errors(&pack).join("\n");
        assert!(e.contains("needs the cutscene's `map`"), "{e}");
        assert!(e.contains("only a pack NPC is shown or hidden"), "{e}");
        assert_eq!(errors(&pack).len(), 2, "{e}");
    }

    #[test]
    fn hooks_name_cutscenes_that_exist() {
        let mut pack = pack_with("[cutscenes.c]\nsteps = [{ do = \"wait\", frames = 1 }]\n");
        let m = pack.maps.get_mut("m").unwrap();
        m.enter_cutscenes.push(crate::map::EnterCutscene {
            cutscene: "nope".into(),
            ..Default::default()
        });
        m.npcs[0].cutscene = Some("c".into());
        let issues = crate::validate::validate(&pack);
        assert!(
            issues
                .iter()
                .any(|i| i.path == "maps/m.toml" && i.message.contains("cutscene \"nope\"")),
            "{issues:?}"
        );
        assert!(
            !issues.iter().any(|i| i.field.contains("npcs[0].cutscene")),
            "{issues:?}"
        );
    }

    #[test]
    fn retail_actor_ids_name_a_slot() {
        assert_eq!(retail_slot("retail:12"), Some(12));
        assert_eq!(retail_slot("retail:x"), None);
        assert_eq!(retail_slot("guide"), None);
        assert_eq!(seen_flag("intro"), "cutscene.intro");
    }
}
