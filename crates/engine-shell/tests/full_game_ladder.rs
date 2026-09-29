//! Full-game ladder: how far through the **whole** game - New Game to the
//! ending credits - the port gets, as one ordered, game-denominated score.
//!
//! The other progression instruments each cover a slice. The pad-driven
//! [`critical_path_replay`](critical_path_replay.rs) stops in chapter 1;
//! `chapter1_frontier_ladder` (engine-core) measures chapter-1 scene
//! *breadth*; the chapter-2 / chapter-3 spine oracles seat the player at a
//! gate rather than drive them there. None of them answers "from a cold New
//! Game, where does the port stop, and why".
//!
//! ## The spine
//!
//! `scripts/replays/full_game_spine.toml` lists the main-story milestones in
//! order. Each names the scene it happens in and a **retail anchor** - a save
//! on one of the library's memory cards, or a catalogued save state - whose
//! SC save window (RAM `0x80084140..0x80086140`, the block a memory card
//! stores) records the party, bag, gold, story flags and position a real
//! playthrough had there. Part A reads every anchor, checks its own scene
//! label against the spine, re-runs the ordering test the file's header
//! describes, and prints the disc route between consecutive milestones over
//! the decoded `0x3F` scene-change graph.
//!
//! ## Segments and tiers
//!
//! One pad run cannot cross the game yet, so the ladder is **segmented**:
//! segment `i` seeds the engine at milestone `i` from that milestone's anchor
//! and asks how far toward milestone `i + 1` it gets. Each segment clears a
//! tier, cumulatively:
//!
//! | tier | name | what it proves |
//! |---|---|---|
//! | 1 | `loads` | the seeded save lands in its scene (Field / WorldMap) |
//! | 2 | `enters` | the entry script settles: control comes back, or the script leaves the scene itself |
//! | 3 | `progresses` | the next milestone is reached by **seated** traversal - the player is placed on each door's walk-on tile and the engine's own dispatch fires it |
//! | 4 | `pad` | the next milestone is reached with pad input only |
//!
//! Segment 0 is seeded by the New Game path itself (`BootSession::start_new_game`),
//! so it measures the cold opening rather than a save.
//!
//! The headline is **how many milestones a cold New Game reaches
//! contiguously** at the `progresses` and `pad` tiers: the count of leading
//! segments that each cleared the tier.
//!
//! ## Stall diagnostics
//!
//! A tier that does not clear prints why, one level down: the script
//! `(pc, opcode)` the pad holder parked on, the hop whose door never
//! fired (and whether the scene has a walk-on door to that destination at
//! all), the tile a pad walk stalled on, a panic message, and - for every
//! segment that stops short - the story flags the next anchor carries that
//! the engine never set, with the disc sites that set each one (the
//! disc-wide system-flag census).
//!
//! ## What it cannot measure
//!
//! The seated tier places the player; it proves the doors, scripts and
//! scene graph, not locomotion. The pad tier's fighter is the engine's own
//! auto-resolve (`player_battle` off), not a command-menu player. Neither
//! tier talks to NPCs, opens menus or buys anything, so a story beat that
//! waits on a conversation reads as a stall at that beat - which is the
//! finding, not a ladder defect. The route follows `0x3F` named scene
//! changes only; a `0x3E` door warp or an FMV hand-off between two
//! milestones is a missing edge.
//!
//! ## Ratchet
//!
//! `scripts/replays/full_game_baseline.toml` holds the per-segment tiers and
//! the two headline counts; each is asserted `>=`. Raising them is a
//! reviewed edit - the test prints the block to paste.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset, or
//! the extracted tree / save library missing. The extracted tree is found at
//! `$LEGAIA_EXTRACTED_DIR`, then `extracted/` up the tree; the library at
//! `$LEGAIA_SAVES_LIBRARY`, then `saves/library/`.
//!
//! `LEGAIA_FGL_ONLY=<id>[,<id>...]` runs only the segments that END at those
//! milestones (the baseline is then not asserted); `LEGAIA_FGL_NO_PAD=1`
//! skips the pad tier.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use legaia_engine_core::field_regions::TileTrigger;
use legaia_engine_core::input::PadButton;
use legaia_engine_core::man_field_scripts::{
    overworld_portal_sites, scene_destinations, scene_man_carriers, system_flag_census,
};
use legaia_engine_core::scene::{FmvHandoffOutcome, ProtIndex, Scene, SceneTickEvent};
use legaia_engine_core::world::{SceneMode, WorldMapEntityConfig};
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};
use legaia_engine_vm::field_disasm::FlagKind;
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn extracted_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from)
        && d.join("PROT.DAT").exists()
    {
        return Some(d);
    }
    [
        repo_root().join("extracted"),
        PathBuf::from("extracted"),
        PathBuf::from("../extracted"),
        PathBuf::from("../../extracted"),
    ]
    .into_iter()
    .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

fn library_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("LEGAIA_SAVES_LIBRARY").map(PathBuf::from)
        && d.is_dir()
    {
        return Some(d);
    }
    [
        repo_root().join("saves/library"),
        PathBuf::from("saves/library"),
        PathBuf::from("../saves/library"),
        PathBuf::from("../../saves/library"),
    ]
    .into_iter()
    .find(|d| d.is_dir())
}

/// Everything a disc-gated run needs, or `None` (with the skip printed).
struct Inputs {
    extracted: PathBuf,
    library: PathBuf,
}

fn inputs() -> Option<Inputs> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted tree missing (set LEGAIA_EXTRACTED_DIR)");
        return None;
    };
    let Some(library) = library_dir() else {
        eprintln!("[skip] save library missing (set LEGAIA_SAVES_LIBRARY)");
        return None;
    };
    if std::env::var_os("LEGAIA_SCUS").is_none() {
        // SAFETY: set once, before any save-state read, from the test thread.
        unsafe { std::env::set_var("LEGAIA_SCUS", extracted.join("SCUS_942.54")) };
    }
    Some(Inputs { extracted, library })
}

// ---------------------------------------------------------------------------
// The spine file
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SpineFile {
    milestone: Vec<Milestone>,
}

#[derive(Debug, Clone, Deserialize)]
struct Milestone {
    id: String,
    scene: String,
    #[serde(default)]
    seed: Option<String>,
    reach: String,
    #[serde(default)]
    reach_flags: Vec<u16>,
    /// Story waypoints between the previous milestone and this one: scenes
    /// the retail run passed through to play a beat the route's shortest
    /// path skips. Visited in order, each with its beats pass played.
    #[serde(default)]
    via: Vec<String>,
    #[serde(default)]
    anchor: Option<AnchorRef>,
    /// `false` for an anchor from outside the main playthrough (a debug
    /// credits run): it anchors the scene but not the order.
    #[serde(default = "yes")]
    order_check: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AnchorRef {
    Card { card: String, save: String },
    State { label: String, fingerprint: String },
}

impl AnchorRef {
    fn describe(&self) -> String {
        match self {
            Self::Card { card, save } => format!("card {card} {save}"),
            Self::State { label, .. } => format!("state {label}"),
        }
    }
}

fn load_spine() -> Vec<Milestone> {
    let path = repo_root().join("scripts/replays/full_game_spine.toml");
    let text = std::fs::read_to_string(&path).expect("read full_game_spine.toml");
    let file: SpineFile = toml::from_str(&text).expect("parse full_game_spine.toml");
    file.milestone
}

// ---------------------------------------------------------------------------
// Anchors: the SC save window of a retail save
// ---------------------------------------------------------------------------

/// RAM base of the SC save window (`block_offset = ram - 0x80084140`).
const SC_RAM_BASE: u32 = 0x8008_4140;
const SC_LEN: usize = 0x2000;
/// SC offset of the play-time counter (`0x80084570`).
const SC_PLAY_TIME: usize = 0x430;
/// SC offset of the system-flag bank (`0x80085758`) and its full extent up
/// to the item window at `0x80085958`.
const SC_SYSTEM_FLAGS: usize = 0x1618;
const SC_SYSTEM_FLAGS_LEN: usize = 0x200;
/// How much of that bank `legaia_save`'s story window reaches - the whole
/// bank; part A fails on an anchor flag past it.
const SAVE_WINDOW_SYSTEM_BYTES: usize = legaia_save::card::RETAIL_STORY_FLAGS_OFFSET
    + legaia_save::card::RETAIL_STORY_FLAGS_SIZE
    - SC_SYSTEM_FLAGS;

struct Anchor {
    sc: Vec<u8>,
    scene: String,
    /// Where to seat the player when the save's own position snapshot is not
    /// it: the live position of a field-run state. `None` lets the resume
    /// path seat the party from the snapshot, as a card load does.
    seat: Option<(i16, i16)>,
    play_time: u32,
    flags: BTreeSet<u16>,
}

fn flags_of(sc: &[u8]) -> BTreeSet<u16> {
    let mut out = BTreeSet::new();
    for (i, b) in sc[SC_SYSTEM_FLAGS..SC_SYSTEM_FLAGS + SC_SYSTEM_FLAGS_LEN]
        .iter()
        .enumerate()
    {
        for k in 0..8 {
            if b & (0x80 >> k) != 0 {
                out.insert((i * 8 + k) as u16);
            }
        }
    }
    out
}

fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| (0x20..0x7F).contains(&b))
        .map(|&b| b as char)
        .collect()
}

fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn load_anchor(lib: &Path, a: &AnchorRef) -> Result<Anchor, String> {
    match a {
        AnchorRef::Card { card, save } => {
            let path = lib.join("cards").join(card);
            let mounted =
                legaia_save::emu::MountedCard::open(&path).map_err(|e| format!("{e:#}"))?;
            for block in 1..=15u8 {
                let Some(frame) = mounted.dir_frame(block) else {
                    continue;
                };
                let name = ascii(&frame[0x0A..0x0A + 20]);
                if !name.ends_with(save.as_str()) {
                    continue;
                }
                let Some(sc) = mounted.sc_block(block) else {
                    continue;
                };
                let sc = sc.to_vec();
                return Ok(Anchor {
                    scene: ascii(&sc[0x408..0x410]),
                    seat: None,
                    play_time: i32_at(&sc, SC_PLAY_TIME) as u32,
                    flags: flags_of(&sc),
                    sc,
                });
            }
            Err(format!("{card}: no save named {save}"))
        }
        AnchorRef::State { label, fingerprint } => {
            let med = lib.join("mednafen").join(format!("{fingerprint}.mcr"));
            let pcsx = lib.join("pcsx-redux").join(format!("{fingerprint}.sstate"));
            let ram: Vec<u8> = if med.exists() {
                let st =
                    legaia_mednafen::SaveState::from_path(&med).map_err(|e| format!("{e:#}"))?;
                st.main_ram().map_err(|e| format!("{e:#}"))?.to_vec()
            } else if pcsx.exists() {
                let st = legaia_pcsxr::SaveState::from_path(&pcsx).map_err(|e| format!("{e:#}"))?;
                st.main_ram().to_vec()
            } else {
                return Err(format!(
                    "{label}: fingerprint {fingerprint} not in the library"
                ));
            };
            let base = (SC_RAM_BASE & 0x1F_FFFF) as usize;
            let sc = ram[base..base + SC_LEN].to_vec();
            let scene = legaia_mednafen::game_anchors::scene_name(&ram);
            let mode = legaia_mednafen::game_anchors::game_mode(&ram);
            let seat = if mode == 0x03 {
                legaia_mednafen::game_anchors::player_pos(&ram)
            } else {
                None
            };
            Ok(Anchor {
                scene,
                seat,
                play_time: i32_at(&sc, SC_PLAY_TIME) as u32,
                flags: flags_of(&sc),
                sc,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// The disc scene graph
// ---------------------------------------------------------------------------

/// The disc's scene graph: every CDNAME scene's decoded `0x3F` destinations
/// over every MAN carrier the scene has (bundle + streaming variants), plus
/// the FMV hand-off edges - a record that triggers movie `id` (`4C E2 id`)
/// leaves for the scene retail's post-play dispatch (`FUN_801CEA3C`) names.
///
/// Each edge also records whether a **walk-on** band reaches it (a gate-1
/// `.MAP` trigger whose partition-2 record carries the `0x3F` or the FMV, in
/// the MAN the live host loads). Routes prefer those: an edge only a talk or
/// scripted record carries costs [`SCRIPTED_EDGE_COST`] hops.
struct DiscGraph {
    edges: BTreeMap<String, BTreeSet<String>>,
    /// `(from, to)` pairs some `0x3F` names.
    doors: BTreeSet<(String, String)>,
    /// `(from, to)` -> the `(partition, record)` sites whose FMV makes the hop.
    fmv: BTreeMap<(String, String), BTreeSet<(usize, usize)>>,
    /// `(from, to)` pairs a walk-on band reaches.
    walk_on: BTreeSet<(String, String)>,
}

/// Route cost of an edge no walk-on band reaches.
const SCRIPTED_EDGE_COST: usize = 6;

impl DiscGraph {
    fn build(index: &ProtIndex) -> Self {
        use legaia_asset::field_disasm::{find_fmv_triggers, partition_record_span};
        use legaia_engine_core::cutscene::{FmvHandoff, fmv_post_play_handoff};
        let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut doors = BTreeSet::new();
        let mut fmv: BTreeMap<(String, String), BTreeSet<(usize, usize)>> = BTreeMap::new();
        let mut walk_on = BTreeSet::new();
        for name in index.cdname_scene_names() {
            let Ok(scene) = Scene::load(index, &name) else {
                continue;
            };
            let set = edges.entry(name.clone()).or_default();
            for c in scene_man_carriers(index, &scene) {
                let man = &c.payload;
                let Ok(mf) = legaia_asset::man_section::parse(man) else {
                    continue;
                };
                for d in scene_destinations(&mf, man) {
                    if d.scene_name != name {
                        set.insert(d.scene_name.clone());
                        doors.insert((name.clone(), d.scene_name));
                    }
                }
                for partition in 1..3 {
                    let n = mf.partitions.get(partition).map_or(0, Vec::len);
                    for rec in 0..n {
                        let Some((start, pc0, len)) =
                            partition_record_span(&mf, man, partition, rec)
                        else {
                            continue;
                        };
                        for (_, id) in find_fmv_triggers(&man[start + pc0..start + len]) {
                            if let FmvHandoff::Field { scene: dest, .. } = fmv_post_play_handoff(id)
                                && dest != name
                            {
                                set.insert(dest.to_string());
                                fmv.entry((name.clone(), dest.to_string()))
                                    .or_default()
                                    .insert((partition, rec));
                            }
                        }
                    }
                }
            }
            // Walk-on reachability, in the MAN the live host loads.
            if let Ok(Some(man)) = scene.field_man_payload(index)
                && let Ok(mf) = legaia_asset::man_section::parse(&man)
                && let Ok((p, f)) = scene.field_tile_triggers(index)
            {
                let triggers: Vec<TileTrigger> = p.into_iter().chain(f).collect();
                for s in overworld_portal_sites(&mf, &man, &triggers) {
                    walk_on.insert((name.clone(), s.scene_name.clone()));
                    if let Some(c) = &s.conditional {
                        walk_on.insert((name.clone(), c.scene_name.clone()));
                    }
                }
                let g1: BTreeSet<usize> = triggers
                    .iter()
                    .filter(|t| t.gate == 1)
                    .map(|t| usize::from(t.record))
                    .collect();
                for ((from, to), recs) in &fmv {
                    if from == &name && recs.iter().any(|&(p, r)| p == 2 && g1.contains(&r)) {
                        walk_on.insert((from.clone(), to.clone()));
                    }
                }
            }
        }
        Self {
            edges,
            doors,
            fmv,
            walk_on,
        }
    }

    fn cost(&self, from: &str, to: &str) -> usize {
        if self.walk_on.contains(&(from.to_string(), to.to_string())) {
            1
        } else {
            SCRIPTED_EDGE_COST
        }
    }

    /// Render a route: `>` a walk-on door, `~fmv~>` an FMV hand-off, `=>` a
    /// `0x3F` no walk-on band reaches (a talk, touch or scripted record).
    fn show(&self, route: &[String]) -> String {
        let mut s = route[0].clone();
        for w in route.windows(2) {
            let key = (w[0].clone(), w[1].clone());
            let arrow = if self.walk_on.contains(&key) {
                if self.doors.contains(&key) {
                    ">"
                } else {
                    "~fmv~>"
                }
            } else if self.doors.contains(&key) {
                "=>"
            } else {
                "~fmv=>"
            };
            s.push_str(arrow);
            s.push_str(&w[1]);
        }
        s
    }

    /// Cheapest scene route `from -> to` (walk-on edges first), both ends
    /// included.
    fn route(&self, from: &str, to: &str) -> Option<Vec<String>> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        if from == to {
            return Some(vec![from.to_string()]);
        }
        let mut dist: HashMap<String, usize> = HashMap::new();
        let mut prev: HashMap<String, String> = HashMap::new();
        let mut heap = BinaryHeap::new();
        dist.insert(from.to_string(), 0);
        heap.push(Reverse((0usize, from.to_string())));
        while let Some(Reverse((d, cur))) = heap.pop() {
            if cur == to {
                let mut path = vec![cur.clone()];
                let mut s = cur;
                while let Some(p) = prev.get(&s) {
                    path.push(p.clone());
                    s = p.clone();
                }
                path.reverse();
                return Some(path);
            }
            if dist.get(&cur).is_some_and(|&best| d > best) {
                continue;
            }
            for n in self.edges.get(&cur).into_iter().flatten() {
                let nd = d + self.cost(&cur, n);
                if dist.get(n).is_none_or(|&best| nd < best) {
                    dist.insert(n.clone(), nd);
                    prev.insert(n.clone(), cur.clone());
                    heap.push(Reverse((nd, n.clone())));
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Session + seeding
// ---------------------------------------------------------------------------

fn open_session(extracted: &Path) -> BootSession {
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = BootSession::open(extracted, &cfg).expect("open boot session");
    // Both play hosts run field dialogue through the inline-script field-VM
    // runner by default (`play-window`'s `--simple-dialogue` and the browser
    // play page's runtime both key on this toggle), so a talk executes its
    // record's flag writes, branches and scene changes. `BootSession` leaves
    // it off; without it a conversation only types its first text segment.
    session.host.world.toggles.use_vm_dialogue = true;
    session
}

/// Live-loop options per tier. Both tiers make battles player-driven and
/// fight them with the pad ([`fight_pad`]); the seated tier runs with the
/// random-encounter roll off (it measures doors and scripts, not
/// encounters), the pad tier arms it.
fn live_opts(pad: bool) -> FieldLiveOpts {
    FieldLiveOpts {
        live_loop: pad,
        player_battle: true,
        battle_bgm: None,
    }
}

/// Seed `session` at milestone `m`. `Ok(scene)` when the landing entered the
/// milestone's scene.
fn seed(
    session: &mut BootSession,
    m: &Milestone,
    anchor: Option<&Anchor>,
    opts: &FieldLiveOpts,
) -> Result<String, String> {
    if m.seed.as_deref() == Some("new_game") {
        return session
            .start_new_game(opts)
            .map(str::to_string)
            .ok_or_else(|| "start_new_game entered nothing".to_string());
    }
    let a = anchor.ok_or("no anchor to seed from")?;
    let sf =
        legaia_save::SaveFile::from_retail_sc_block(&a.sc, legaia_save::RETAIL_SC_PARTY_RECORDS)
            .map_err(|e| format!("lift SC block: {e:#}"))?;
    // Flags go in before the entry so the entry script's first slice and the
    // scene's gated spawns see the saved story state, then the host's own
    // resume path runs (which applies the save again over the landing, and
    // seats the party at the save's position unless a live one is armed).
    session.host.world.load_full(sf.clone());
    if let Some((x, z)) = a.seat {
        session.host.set_entry_seat(x, z);
    }
    let landing = session.resume_save(sf, &m.scene, opts);
    if !landing.entered_scene() {
        return Err(format!("resume landed {landing:?}"));
    }
    Ok(landing.scene().unwrap_or_default().to_string())
}

fn scene_name(session: &BootSession) -> String {
    session
        .host
        .scene
        .as_ref()
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

fn walking(session: &BootSession) -> bool {
    matches!(
        session.host.world.mode,
        SceneMode::Field | SceneMode::WorldMap
    )
}

fn player_xz(session: &BootSession) -> (i16, i16) {
    let w = &session.host.world;
    let slot = w.player_actor_slot.unwrap_or(0) as usize;
    let ms = &w.actors[slot].move_state;
    (ms.world_x, ms.world_z)
}

fn tile_of(x: i16, z: i16) -> (i16, i16) {
    ((x - 0x40) >> 7, (z - 0x40) >> 7)
}

/// Where the context holding the pad came to rest, as `(pc, opcode)`: the
/// modal timeline's, else the first live helper record's, else the entry
/// script's (the frontier ladder's `park_site`).
fn park_site(session: &BootSession) -> String {
    let w = &session.host.world;
    let at = |who: &str, bc: &[u8], pc: usize| match bc.get(pc) {
        Some(op) => format!("{who} pc=0x{pc:04X} op=0x{op:02X}"),
        None => format!("{who} pc=0x{pc:04X} (past end {})", bc.len()),
    };
    if let Some(tl) = w.cutscene.timeline.as_ref() {
        return at("timeline", &tl.bytecode, tl.pc);
    }
    if let Some(id) = w.dialog.inline.as_ref() {
        return at("talk", &id.bytecode, id.pc);
    }
    if let Some(h) = w.field_vm.helper_contexts.first() {
        return at("helper", &h.bytecode, h.pc);
    }
    at("entry", &w.field_bytecode, w.field_pc)
}

fn holder(session: &BootSession) -> &'static str {
    let w = &session.host.world;
    if w.cutscene_timeline_active() {
        "cutscene timeline"
    } else if w.dialogue_owns_input() {
        "dialogue"
    } else if !w.field_vm.helper_contexts.is_empty() {
        "a spawned record"
    } else if !w.field_vm.pending_record_spawns.is_empty() {
        "a queued spawn"
    } else {
        "nothing"
    }
}

fn released(session: &BootSession) -> bool {
    let w = &session.host.world;
    walking(session)
        && !w.cutscene_timeline_active()
        && !w.dialogue_owns_input()
        && w.field_vm.helper_contexts.is_empty()
        && w.field_vm.pending_record_spawns.is_empty()
        && w.active_fmv().is_none()
}

// ---------------------------------------------------------------------------
// Tick drivers
// ---------------------------------------------------------------------------

/// Ticks an entry script gets to hand control back.
const SETTLE_TICKS: usize = 3_600;
/// Ticks a door record gets to reach its `0x3F` after the step, and the deep
/// budget a record still running at the end of it gets.
const EXIT_TICKS: usize = 2_400;
const DEEP_EXIT_TICKS: usize = 24_000;
/// Consecutive idle ticks that end a post-step wait.
const EXIT_IDLE_TICKS: usize = 4;
/// A battle's tick budget under the pad fighter.
const BATTLE_TICKS: usize = 30_000;
/// The most a scripted sequence may run while its park site keeps moving.
const SCRIPT_CEILING: usize = 60_000;
/// Hops a segment may take before it is called lost.
const MAX_HOPS: usize = 16;
/// Door sites tried per hop.
const MAX_SITES: usize = 12;

/// What one tick-run ended on.
#[derive(Debug)]
enum Run {
    /// A scene change landed (directly, or via an FMV hand-off).
    Entered(String),
    /// Control came back (nothing is running).
    Released,
    /// The budget ran out with something still holding the pad.
    Parked(String),
    /// A battle ended in a wipe or never resolved.
    Battle(String),
    /// The host returned an error.
    Error(String),
}

/// What a player presses while a script holds the frame: Cross on a
/// press-2-release-14 duty cycle (pages are edge-triggered), except on the
/// naming prompt's Yes/No confirm, which opens on No - there the hand moves
/// Up to Yes first, or the prompt drops back to editing forever.
fn script_pad(session: &BootSession, f: usize) -> u16 {
    use legaia_engine_core::name_entry::NameEntryState;
    if let Some(pad) = held_pad_poll(session) {
        return pad;
    }
    if f % 16 >= 2 {
        return 0;
    }
    if let Some(pad) = picker_pad(session) {
        return pad;
    }
    if let Some(pad) = flag_window_pad(session) {
        return pad;
    }
    if let Some(ne) = session.host.world.party.name_entry.as_ref()
        && ne.state == NameEntryState::Confirm
        && !ne.confirm_yes
    {
        return PadButton::Up.mask();
    }
    PadButton::Cross.mask()
}

/// The d-pad a record is polling for: a cutscene timeline (or spawned
/// record) sitting on op `42 01 <i>`, the held-pad compare against the
/// compass table (`0x801F28D0`: Down, Down+Left, Left, Left+Up, Up,
/// Up+Right, Right, Right+Down). Rim Elm's "stand here and press" beats
/// (`town01` P2[12..14], `town0e` P2[0..4]) re-run every tick the player
/// stands on the tile (the `2E 13` re-poll bit) and take their action only
/// while the direction is held.
fn held_pad_poll(session: &BootSession) -> Option<u16> {
    let w = &session.host.world;
    let (bc, pc) = if let Some(tl) = w.cutscene.timeline.as_ref() {
        (&tl.bytecode, tl.pc)
    } else {
        let h = w.field_vm.helper_contexts.first()?;
        (&h.bytecode, h.pc)
    };
    // The record re-spawns every tick, so between ticks it usually sits on
    // the `2E 13` that raises the re-poll bit, one op ahead of the poll.
    let pc = if bc.get(pc..pc + 2) == Some(&[0x2E, 0x13]) {
        pc + 2
    } else {
        pc
    };
    if bc.get(pc) != Some(&0x42) || bc.get(pc + 1) != Some(&0x01) {
        return None;
    }
    let (d, l, u, r) = (
        PadButton::Down.mask(),
        PadButton::Left.mask(),
        PadButton::Up.mask(),
        PadButton::Right.mask(),
    );
    [d, d | l, l, l | u, u, u | r, r, r | d]
        .get(usize::from(*bc.get(pc + 2)?))
        .copied()
}

/// A conversation picker: the talk record's address and the picker's offset
/// in it.
type PickerKey = (usize, usize);

thread_local! {
    /// Visits per conversation picker, keyed by the talk record and the
    /// picker's offset in it, and the picker open on the last pad read.
    static PICKS: std::cell::RefCell<(HashMap<PickerKey, usize>, Option<PickerKey>)> =
        std::cell::RefCell::new((HashMap::new(), None));
}

/// The pad on a conversation picker (the inline runner's option menu).
/// Option 0 is often "tell me again" - a branch that jumps back to the same
/// speech - so a hand that always confirms the default loops forever. The
/// hand instead takes option `k` on the picker's `k`-th opening (0, then 1,
/// ...), which walks a menu to its story branch or its exit.
fn picker_pad(session: &BootSession) -> Option<u16> {
    let open = session.host.world.dialog.inline.as_ref().and_then(|id| {
        let panel = id.panel.as_ref()?;
        let pk = panel.picker()?;
        panel.menu_active().then_some((
            (std::sync::Arc::as_ptr(&id.bytecode) as usize, pk.open),
            pk.n.max(1),
            panel.picker_cursor(),
            panel.picker_takes_input(),
        ))
    });
    let Some((key, n, cursor, takes)) = open else {
        PICKS.with(|p| p.borrow_mut().1 = None);
        return None;
    };
    let visit = PICKS.with(|p| {
        let mut p = p.borrow_mut();
        if p.1 != Some(key) {
            p.1 = Some(key);
            *p.0.entry(key).or_insert(0) += 1;
        }
        p.0[&key]
    });
    if !takes {
        return Some(0);
    }
    let want = (visit - 1) % n;
    Some(if cursor < want {
        PadButton::Down.mask()
    } else if cursor > want {
        PadButton::Up.mask()
    } else {
        PadButton::Cross.mask()
    })
}

thread_local! {
    /// The scene the current hop is heading for, read by
    /// [`flag_window_pad`].
    static HOP_DEST: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The pad on an op-`49 04` flag-window picker (slot `0x23`, the Uru Mais
/// warp pads of `kor` / `kor3` / `kor4`). The picker opens on the pad the
/// party stands on and commits the highlighted row as the one set flag of
/// `base..base+count`; the record then branches on that flag to one scene
/// change per row. Confirming the default re-enters the pad's own floor, so
/// the hand reads the record's branch table, finds the row whose branch
/// names the hop's destination, moves the highlight there and confirms.
/// `None` when no flag window is up (or no row leads to the destination).
fn flag_window_pad(session: &BootSession) -> Option<u16> {
    use legaia_asset::field_disasm::{InsnInfo, decode, scene_change_name};
    use legaia_engine_core::field_submode_flag_window::{
        FLAG_WINDOW_SLOT, descriptor_from_operand,
    };
    let w = &session.host.world;
    let screen = &w.field_vm.submode_screen;
    if !screen.open || screen.actor.state != FLAG_WINDOW_SLOT {
        return None;
    }
    let dest = HOP_DEST.with(|d| d.borrow().clone())?;
    let desc = descriptor_from_operand(&screen.op49_operand);
    let base = desc.base_flag;
    let count = i32::from(desc.count);
    let bc = &w.cutscene.timeline.as_ref()?.bytecode;
    // Each row's branch: the TEST of `base + row`, then the first scene
    // change its target reaches.
    let mut want = None;
    let mut pc = 0usize;
    while pc < bc.len() && want.is_none() {
        let Ok(insn) = decode(bc, pc) else {
            pc += 1;
            continue;
        };
        if let InsnInfo::SystemFlag {
            idx,
            target: Some(t),
            ..
        } = insn.info
            && (base..base + count).contains(&i32::from(idx))
        {
            let mut q = t;
            for _ in 0..64 {
                let Ok(i) = decode(bc, q) else { break };
                if let Some(name) = scene_change_name(bc, &i) {
                    if name == dest {
                        want = Some(i32::from(idx) - base);
                    }
                    break;
                }
                if i.size == 0 {
                    break;
                }
                q += i.size;
            }
        }
        pc += insn.size.max(1);
    }
    let want = want?;
    // The rows are drawn flipped (`flag_window_row_flip`): the highest flag
    // sits on the top row, so Up raises the selection. A row outside the
    // window's visible band cannot be reached from this pad.
    let sel = screen.flag_window.selection;
    let first = i32::from(desc.first_visible);
    if !(first..first + i32::from(desc.rows)).contains(&want) {
        return None;
    }
    Some(if sel < want {
        PadButton::Up.mask()
    } else if sel > want {
        PadButton::Down.mask()
    } else {
        PadButton::Cross.mask()
    })
}

/// Tick with Cross pulsed on a human duty cycle (edge-triggered pages advance
/// on a press), completing FMVs and auto-resolving battles, until a scene
/// change, release, or the budget.
///
/// `stop_on_release` ends the run as soon as nothing holds the pad (after
/// `EXIT_IDLE_TICKS` idle ticks); otherwise only a scene change ends it.
fn run(session: &mut BootSession, budget: usize, stop_on_release: bool) -> Run {
    let mut idle = 0usize;
    for f in 0..budget {
        if session.host.world.mode == SceneMode::Battle {
            if let Some(r) = drain_battle(session) {
                return r;
            }
            continue;
        }
        let pad = script_pad(session, f);
        // The naming prompt (the opening's op-0x49) takes this pad's edge
        // inside `BootSession::tick`, as it does in both play hosts.
        session.host.world.set_pad(pad);
        match session.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Run::Entered(name),
            Ok(_) => {}
            Err(e) => return Run::Error(format!("{e:#}")),
        }
        if session.host.world.active_fmv().is_some() {
            session.host.world.finish_cutscene();
            if let Some(FmvHandoffOutcome::Entered { scene, .. }) =
                session.apply_pending_fmv_handoff()
            {
                return Run::Entered(scene.to_string());
            }
        }
        if f >= 2 && released(session) {
            idle += 1;
            if stop_on_release && idle >= EXIT_IDLE_TICKS {
                session.host.world.set_pad(0);
                return Run::Released;
            }
        } else {
            idle = 0;
        }
    }
    session.host.world.set_pad(0);
    if released(session) {
        Run::Released
    } else {
        Run::Parked(format!("{} at {}", holder(session), park_site(session)))
    }
}

/// [`run`] until a scene change or release, for as long as the pad holder's
/// park site keeps moving: a window of [`SETTLE_TICKS`] is re-granted
/// whenever the site changed across the last one, up to `ceiling` ticks in
/// all. A long cutscene is not a stall; a script that sits on one
/// instruction for a whole window is.
fn run_while_moving(session: &mut BootSession, ceiling: usize) -> Run {
    let mut spent = 0usize;
    loop {
        let before = park_site(session);
        let r = run(session, SETTLE_TICKS, true);
        spent += SETTLE_TICKS;
        match r {
            Run::Parked(p) => {
                if park_site(session) == before || spent >= ceiling {
                    return Run::Parked(p);
                }
            }
            other => return other,
        }
    }
}

/// The pad mask a player presses this frame in a battle: Begin, Attack,
/// Auto, confirm the target, confirm a message box, back out of a bag. The
/// command-ring choices `critical_path_replay`'s fighter makes, minus its
/// healing policy - every one a pad press, no engine call.
fn fight_pad(session: &BootSession) -> u16 {
    use legaia_engine_core::battle_input::CommandPhase;
    use legaia_engine_core::battle_tutorial::TutorialLesson;
    use legaia_engine_core::inventory_use::InventoryUseState;
    let w = &session.host.world;
    if !w.battle.tutorial_boxes.is_empty() {
        return PadButton::Cross.mask();
    }
    // A tutorial battle (Tetsu's spar) validates each commit against the
    // lesson it is teaching and rewinds any other command to the ring, so
    // the hand picks the command the lesson names: the ring's up arm for
    // Items (and uses the first item on the first target), its down arm for
    // Spirit. Attacks and Hyper Arts both commit through Attack.
    let lesson = w.battle.tutorial.as_ref().map(|t| t.lesson());
    if let Some(menu) = w.battle.item_menu.as_ref() {
        return match (lesson, &menu.state) {
            (Some(TutorialLesson::Items), InventoryUseState::Browsing { .. })
                if !menu.filtered_items.is_empty() =>
            {
                PadButton::Cross.mask()
            }
            (Some(TutorialLesson::Items), InventoryUseState::TargetSelect { .. }) => {
                PadButton::Cross.mask()
            }
            _ => PadButton::Circle.mask(),
        };
    }
    if let Some(cmd) = w.battle.command.as_ref() {
        return match &cmd.phase {
            CommandPhase::Menu { .. } if lesson == Some(TutorialLesson::Items) => {
                PadButton::Up.mask()
            }
            CommandPhase::Menu { .. } if lesson == Some(TutorialLesson::Spirit) => {
                PadButton::Down.mask()
            }
            CommandPhase::RoundPrompt { .. }
            | CommandPhase::Menu { .. }
            | CommandPhase::AttackMode { .. }
            | CommandPhase::CommitConfirm { .. } => PadButton::Left.mask(),
            CommandPhase::Targeting { .. } => PadButton::Cross.mask(),
            _ => 0,
        };
    }
    // Between command sessions (intro, victory banner, results): page on.
    PadButton::Cross.mask()
}

/// One line of battle state for a stall report.
fn battle_snapshot(session: &BootSession) -> String {
    let w = &session.host.world;
    let n = w.party.party_count.clamp(1, 3) as usize;
    let hp = |i: usize| format!("{}/{}", w.actors[i].battle.hp, w.actors[i].battle.max_hp);
    let party: Vec<String> = (0..n).map(hp).collect();
    let mobs: Vec<String> = (3..w.actors.len())
        .filter(|&i| w.actors[i].battle.max_hp > 0)
        .map(hp)
        .collect();
    let formation = w.battle.active_formation.as_ref().map_or_else(
        || "F-".to_string(),
        |f| {
            let ids: Vec<String> = f.slots.iter().map(|s| s.monster_id.to_string()).collect();
            format!("F{}[{}]", f.formation_id, ids.join(","))
        },
    );
    let phase = w.battle.command.as_ref().map_or_else(
        || "no command session".to_string(),
        |c| format!("{:?}", c.phase),
    );
    let phase: String = phase.chars().take(60).collect();
    let clip = |s: String| s.chars().take(90).collect::<String>();
    let sm = w.battle_ctx.action_state;
    let sm_name = legaia_engine_vm::battle_action::ActionState::from_byte(sm)
        .map_or_else(|| "?".to_string(), |s| format!("{s:?}"));
    format!(
        "{formation} party[{}] mobs[{}] action SM ctx[7]=0x{sm:02X} {sm_name} actor {} boxes {} phase {phase}; driven {} end {:?} victory {} flow {} round {}",
        party.join(" "),
        mobs.join(" "),
        w.battle_ctx.active_actor,
        w.battle.tutorial_boxes.len(),
        w.battle.player_driven,
        w.battle.end,
        w.battle.victory.is_some(),
        clip(format!("{:?}", w.battle.flow)),
        clip(format!("{:?}", w.battle.round_flow)),
    )
}

/// Fight a battle with the pad ([`fight_pad`]). `None` when it ended and a
/// walking mode came back.
fn drain_battle(session: &mut BootSession) -> Option<Run> {
    let mut prev = 0u16;
    for _ in 0..BATTLE_TICKS {
        // A press is an edge: alternate the wanted mask with neutral.
        let want = fight_pad(session);
        let pad = if prev == 0 { want } else { 0 };
        prev = pad;
        session.host.world.set_pad(pad);
        if let Err(e) = session.tick() {
            return Some(Run::Error(format!("{e:#}")));
        }
        let w = &session.host.world;
        if w.game_over_hold || w.game_over {
            return Some(Run::Battle(format!(
                "party wiped: {}",
                battle_snapshot(session)
            )));
        }
        if w.mode != SceneMode::Battle {
            return None;
        }
    }
    Some(Run::Battle(format!(
        "battle unresolved after {BATTLE_TICKS} ticks: {}",
        battle_snapshot(session)
    )))
}

// ---------------------------------------------------------------------------
// Doors
// ---------------------------------------------------------------------------

/// One way out of the current scene toward `dest`.
#[derive(Debug, Clone)]
struct Door {
    tile: (u8, u8),
}

/// The doors toward `dest` from the loaded scene, and - when there are none -
/// why, in terms of the disc structures.
fn doors_to(session: &BootSession, graph: &DiscGraph, dest: &str) -> Result<Vec<Door>, String> {
    let w = &session.host.world;
    if session.host.world.mode == SceneMode::WorldMap {
        let doors: Vec<Door> = w
            .world_map
            .entity_configs
            .iter()
            .zip(w.world_map.entity_positions.iter())
            .filter_map(|(cfg, &(x, z))| match cfg {
                WorldMapEntityConfig::OverworldPortal { scene_name, .. } if scene_name == dest => {
                    Some(Door {
                        tile: ((x >> 7) as u8, (z >> 7) as u8),
                    })
                }
                _ => None,
            })
            .collect();
        if doors.is_empty() {
            let installed: BTreeSet<String> = w
                .world_map
                .entity_configs
                .iter()
                .filter_map(|c| match c {
                    WorldMapEntityConfig::OverworldPortal { scene_name, .. } => {
                        Some(scene_name.clone())
                    }
                    _ => None,
                })
                .collect();
            return Err(format!(
                "no overworld portal to {dest} installed (portals: {installed:?}); {}",
                site_gate_report(session, dest)
            ));
        }
        return Ok(doors);
    }
    let name = scene_name(session);
    let index = &session.host.index;
    let scene = Scene::load(index, &name).map_err(|e| format!("Scene::load: {e:#}"))?;
    let man = scene
        .field_man_payload(index)
        .map_err(|e| format!("MAN: {e:#}"))?
        .ok_or("no MAN")?;
    let mf = legaia_asset::man_section::parse(&man).map_err(|e| format!("parse MAN: {e:#}"))?;
    let (primary, fallback) = scene
        .field_tile_triggers(index)
        .map_err(|e| format!(".MAP triggers: {e:#}"))?;
    let triggers: Vec<TileTrigger> = primary.into_iter().chain(fallback).collect();
    let sites = overworld_portal_sites(&mf, &man, &triggers);
    let doors: Vec<Door> = sites
        .iter()
        .filter(|s| {
            s.scene_name == dest || s.conditional.as_ref().is_some_and(|c| c.scene_name == dest)
        })
        .map(|s| Door {
            tile: (s.overworld_x, s.overworld_z),
        })
        .collect();
    // An FMV hop: the walk-on tiles whose partition-2 record triggers the
    // movie that hands off to `dest`.
    let fmv_doors: Vec<Door> = graph
        .fmv
        .get(&(name.clone(), dest.to_string()))
        .map(|recs| {
            triggers
                .iter()
                .filter(|t| t.gate == 1 && recs.contains(&(2, usize::from(t.record))))
                .map(|t| Door {
                    tile: (t.tile_x, t.tile_z),
                })
                .collect()
        })
        .unwrap_or_default();
    if doors.is_empty() && !fmv_doors.is_empty() {
        return Ok(fmv_doors);
    }
    if doors.is_empty() {
        if let Some(recs) = graph.fmv.get(&(name.clone(), dest.to_string())) {
            return Err(format!(
                "{dest} is reached by an FMV hand-off from record(s) {recs:?} (partition, record), none on a walk-on band"
            ));
        }
        let walk_on: BTreeSet<&str> = sites.iter().map(|s| s.scene_name.as_str()).collect();
        let listed = scene_destinations(&mf, &man)
            .iter()
            .any(|d| d.scene_name == dest);
        return Err(format!(
            "no walk-on door to {dest} (walk-on doors lead to {walk_on:?}; {dest} {} in the MAN's 0x3F set - {})",
            if listed { "is" } else { "is not" },
            if listed {
                "reached by a partition-1 / talk / scripted record, not a .MAP band"
            } else {
                "the loaded MAN carrier does not name it"
            }
        ));
    }
    Ok(doors)
}

/// Seat one tile off `tile` and onto it, so the walk-on dispatch sees a
/// genuine tile change.
fn step_onto(session: &mut BootSession, tile: (u8, u8)) {
    session.host.world.set_pad(0);
    // The approach tile must itself be inert: a kind-1 band there spawns its
    // own record, and a kind-0 teleport there arms a warp whose landing, a
    // few dozen frames later, carries the player off the tile under test
    // (the warp in flight also owns the dispatcher, so the step never fires).
    let claimed = claimed_tiles(session);
    let off = [(-1i16, 0i16), (1, 0), (0, -1), (0, 1)]
        .iter()
        .map(|&(dx, dz)| (i16::from(tile.0) + dx, i16::from(tile.1) + dz))
        .filter(|&(x, z)| (0..128).contains(&x) && (0..128).contains(&z))
        .map(|(x, z)| (x as u8, z as u8))
        .find(|t| !claimed.contains(t))
        .unwrap_or(if tile.0 > 0 {
            (tile.0 - 1, tile.1)
        } else {
            (tile.0 + 1, tile.1)
        });
    session.host.world.seat_player_at_tile(off.0, off.1);
    let _ = session.tick();
    session.host.world.seat_player_at_tile(tile.0, tile.1);
}

/// Tiles of the loaded field scene that carry a `.MAP` trigger of either
/// kind (kind-1 record bands, kind-0 intra-scene teleports).
fn claimed_tiles(session: &BootSession) -> HashSet<(u8, u8)> {
    let mut out = HashSet::new();
    if session.host.world.mode != SceneMode::Field {
        return out;
    }
    let index = &session.host.index;
    let Ok(scene) = Scene::load(index, &scene_name(session)) else {
        return out;
    };
    if let Ok((p, f)) = scene.field_tile_triggers(index) {
        out.extend(p.iter().chain(f.iter()).map(|t| (t.tile_x, t.tile_z)));
    }
    if let Ok((p, f)) = scene.field_intra_scene_teleports(index) {
        out.extend(p.iter().chain(f.iter()).map(|t| (t.tile_x, t.tile_z)));
    }
    out
}

/// Try `doors` by seating; `Ok(entered)` for the first that changed scene.
fn seated_hop(session: &mut BootSession, graph: &DiscGraph, dest: &str) -> Result<String, String> {
    let doors = match doors_to(session, graph, dest) {
        Ok(d) => d,
        Err(why) => return talk_hop(session, graph, dest).ok_or(why)?,
    };
    HOP_DEST.with(|d| *d.borrow_mut() = Some(dest.to_string()));
    let mut tried = Vec::new();
    for d in doors.iter().take(MAX_SITES) {
        step_onto(session, d.tile);
        let mut r = run(session, EXIT_TICKS, true);
        if matches!(r, Run::Parked(_)) {
            r = run_while_moving(session, DEEP_EXIT_TICKS);
        }
        match r {
            Run::Entered(s) => return Ok(s),
            other => tried.push(format!("{:?}: {other:?}", d.tile)),
        }
    }
    Err(format!(
        "{} door(s) to {dest}, none fired: {}; {}",
        doors.len(),
        tried.join("; "),
        site_gate_report(session, dest)
    ))
}

/// A hop no walk-on band carries, taken by talking: the talk-NPC placements
/// whose own partition-1 record names `dest` in a `0x3F`, directly or through
/// the partition-2 records it spawns (`0x44`, followed a few levels), or
/// spawns the record whose FMV hands off to `dest` (`town01`'s P1[40] spawns
/// P2[25], the mist-night movie). `None` when no such talk exists;
/// `Some(Err)` when talks ran and none left the scene.
fn talk_hop(
    session: &mut BootSession,
    graph: &DiscGraph,
    dest: &str,
) -> Option<Result<String, String>> {
    use legaia_asset::field_disasm::{InsnInfo, LinearWalker, scene_change_name};
    use legaia_engine_core::man_field_scripts::partition_record_span;
    let (mf, man, _) = scene_man_and_triggers(session)?;
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let fmv: BTreeSet<usize> = graph
        .fmv
        .get(&(scene_name(session), dest.to_string()))
        .map(|r| r.iter().filter(|(p, _)| *p == 2).map(|&(_, r)| r).collect())
        .unwrap_or_default();
    // Does the record (partition, index) reach `dest` within `depth` spawns?
    fn reaches(
        mf: &legaia_asset::man_section::ManFile,
        man: &[u8],
        part: usize,
        rec: usize,
        dest: &str,
        base: usize,
        depth: usize,
    ) -> bool {
        let Some((start, pc0, len)) = partition_record_span(mf, man, part, rec) else {
            return false;
        };
        let body = &man[start..start + len];
        for insn in LinearWalker::new(body, pc0).flatten() {
            match insn.info {
                InsnInfo::SceneChange { .. }
                    if scene_change_name(body, &insn).as_deref() == Some(dest) =>
                {
                    return true;
                }
                InsnInfo::SpawnRecord { global_index } if depth > 0 => {
                    if let Some(r2) = usize::from(global_index).checked_sub(base)
                        && (r2 != rec || part != 2)
                        && reaches(mf, man, 2, r2, dest, base, depth - 1)
                    {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }
    let w = &session.host.world;
    let slots: Vec<u8> = w
        .npcs
        .positions
        .keys()
        .copied()
        .filter(|s| w.npcs.dialog.contains_key(s) || w.npcs.dialog_prologue.contains_key(s))
        .filter(|&s| {
            reaches(&mf, &man, 1, usize::from(s), dest, n0 + n1, 3)
                || spawned_p2(&mf, &man, 1, usize::from(s), n0 + n1, 3)
                    .iter()
                    .any(|r| fmv.contains(r))
        })
        .collect();
    if slots.is_empty() {
        return None;
    }
    let mut tried = Vec::new();
    for slot in slots {
        match talk_to(session, slot) {
            Run::Entered(s) => return Some(Ok(s)),
            other => tried.push(format!("talk P1[{slot}]: {other:?}")),
        }
    }
    Some(Err(format!(
        "no walk-on door to {dest}; talks that lead there did not leave: {}",
        tried.join("; ")
    )))
}

/// The loaded scene's MAN, parsed, and its `.MAP` triggers.
fn scene_man_and_triggers(
    session: &BootSession,
) -> Option<(
    legaia_asset::man_section::ManFile,
    Vec<u8>,
    Vec<TileTrigger>,
)> {
    let name = scene_name(session);
    let index = &session.host.index;
    let scene = Scene::load(index, &name).ok()?;
    let man = scene.field_man_payload(index).ok()??;
    let mf = legaia_asset::man_section::parse(&man).ok()?;
    let (p, f) = scene.field_tile_triggers(index).ok()?;
    Some((mf, man, p.into_iter().chain(f).collect()))
}

/// For every walk-on site whose record names `dest`: the record, its
/// partition-2 C1 / C2 story gates (retail `FUN_8003BDE0`) and which of them
/// the live flag bank fails - the one-level-down reason a door stays shut.
fn site_gate_report(session: &BootSession, dest: &str) -> String {
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return "scene MAN / triggers unavailable".into();
    };
    let w = &session.host.world;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for s in overworld_portal_sites(&mf, &man, &triggers) {
        let names_dest =
            s.scene_name == dest || s.conditional.as_ref().is_some_and(|c| c.scene_name == dest);
        if !names_dest || !seen.insert(s.record) {
            continue;
        }
        let cond = s.conditional.as_ref().map_or(String::new(), |c| {
            format!(
                " (dest switches to {} when 0x{:03X} {})",
                c.scene_name,
                c.flag,
                if w.system_flag_test(c.flag) {
                    "SET"
                } else {
                    "clear"
                }
            )
        });
        match partition2_record_gates(&mf, &man, usize::from(s.record)) {
            Some((c1, c2)) => {
                let bad1: Vec<String> = c1
                    .iter()
                    .filter(|&&f| w.system_flag_test(f))
                    .map(|f| format!("0x{f:03X} set"))
                    .collect();
                let bad2: Vec<String> = c2
                    .iter()
                    .filter(|&&f| !w.system_flag_test(f))
                    .map(|f| format!("0x{f:03X} clear"))
                    .collect();
                let verdict = if bad1.is_empty() && bad2.is_empty() {
                    "gates pass".to_string()
                } else {
                    format!("gates FAIL: {}", [bad1, bad2].concat().join(", "))
                };
                out.push(format!("P2[{}] {verdict}{cond}", s.record));
            }
            None => out.push(format!("P2[{}] gates undecodable{cond}", s.record)),
        }
    }
    if out.is_empty() {
        format!("no walk-on record in this scene names {dest}")
    } else {
        format!("sites: {}", out.join(" | "))
    }
}

// ---------------------------------------------------------------------------
// Pad walking (a condensed critical_path_replay follower)
// ---------------------------------------------------------------------------

const SUBCELL: i16 = 32;
const TILE: i16 = 128;
const PAD_LEG_FRAMES: u32 = 12_000;
const PAD_STALL_FRAMES: u32 = 300;
const MAX_PLAN_NODES: usize = 300_000;
/// Tiles short of a door the lattice may end before a pad hop is called
/// unwalkable rather than attempted.
const DOOR_APPROACH_SLACK: i32 = 6;
type Cell = (i16, i16);

fn cell_of(x: i16, z: i16) -> Cell {
    ((x + SUBCELL / 2) / SUBCELL, (z + SUBCELL / 2) / SUBCELL)
}
fn cell_center(c: Cell) -> (i16, i16) {
    (c.0 * SUBCELL, c.1 * SUBCELL)
}
fn tile_center(t: (i16, i16)) -> (i16, i16) {
    (t.0 * TILE + 0x40, t.1 * TILE + 0x40)
}
fn dispatch_tile(x: i16, z: i16) -> (i32, i32) {
    (i32::from(x) >> 7, i32::from(z) >> 7)
}
const STEPS: [((i16, i16), usize); 4] = [((0, -1), 0), ((-1, 0), 1), ((0, 1), 2), ((1, 0), 3)];

fn pad_for_step(session: &BootSession, dwx: i16, dwz: i16) -> u16 {
    let mut pad = 0u16;
    let (sx, sy) = if session.host.world.mode == SceneMode::WorldMap {
        let az = session
            .host
            .world
            .world_map
            .ctrl
            .as_ref()
            .map_or(0, |c| c.azimuth);
        let (dx, dz) = (f32::from(dwx), f32::from(dwz));
        let len = (dx * dx + dz * dz).sqrt();
        if len == 0.0 {
            return 0;
        }
        let theta = (az as f32) / 4096.0 * std::f32::consts::TAU;
        let (sin, cos) = theta.sin_cos();
        let (dx, dz) = (dx / len, dz / len);
        let sx = dx * cos + dz * sin;
        let sy = -dx * sin + dz * cos;
        const T: f32 = 0.382_683_43;
        (
            if sx > T {
                1
            } else if sx < -T {
                -1
            } else {
                0
            },
            if sy > T {
                1
            } else if sy < -T {
                -1
            } else {
                0
            },
        )
    } else {
        let az = session.host.world.locomotion.camera_azimuth;
        let quadrant = (u32::from(az).wrapping_add(512) / 1024) & 3;
        match quadrant {
            0 => (dwx, dwz),
            1 => (-dwz, dwx),
            2 => (-dwx, -dwz),
            _ => (dwz, -dwx),
        }
    };
    if sy > 0 {
        pad |= PadButton::Up.mask();
    } else if sy < 0 {
        pad |= PadButton::Down.mask();
    }
    if sx > 0 {
        pad |= PadButton::Right.mask();
    } else if sx < 0 {
        pad |= PadButton::Left.mask();
    }
    pad
}

/// BFS over the collision lattice toward `goal`, never entering an `avoid`
/// dispatch tile (retail fires on a tile change, so occupying one is safe).
fn plan_path(
    session: &BootSession,
    from: Cell,
    goal: (i16, i16),
    avoid: &HashSet<(i32, i32)>,
) -> Option<Vec<Cell>> {
    let w = &session.host.world;
    let gw = tile_center(goal);
    let gc = cell_of(gw.0, gw.1);
    let score = |c: Cell| (c.0 - gc.0).abs() + (c.1 - gc.1).abs();
    let mut seen: HashMap<Cell, Cell> = HashMap::new();
    let mut q = VecDeque::from([from]);
    seen.insert(from, from);
    let mut best = from;
    while let Some(cur) = q.pop_front() {
        if seen.len() > MAX_PLAN_NODES {
            break;
        }
        if score(cur) < score(best) {
            best = cur;
        }
        if cur == gc {
            break;
        }
        let (cx, cz) = cell_center(cur);
        for ((dx, dz), dir) in STEPS {
            let next = (cur.0 + dx, cur.1 + dz);
            if next.0 < 0 || next.1 < 0 || seen.contains_key(&next) {
                continue;
            }
            if w.field_dir_blocked(cx, cz, dir) || w.field_actor_dir_blocked(cx, cz, dir) {
                continue;
            }
            let (nx, nz) = cell_center(next);
            let (nt, ct) = (dispatch_tile(nx, nz), dispatch_tile(cx, cz));
            if nt != ct && avoid.contains(&nt) {
                continue;
            }
            seen.insert(next, cur);
            q.push_back(next);
        }
    }
    if best == from {
        return None;
    }
    let mut path = vec![best];
    let mut s = best;
    while s != from {
        s = seen[&s];
        if s != from {
            path.push(s);
        }
    }
    path.reverse();
    Some(path)
}

/// Every door tile in the scene that does NOT lead to `dest` - stepping on one
/// would leave for the wrong scene.
fn hazards(session: &BootSession, dest: &str) -> HashSet<(i32, i32)> {
    let w = &session.host.world;
    let mut out = HashSet::new();
    if w.mode == SceneMode::WorldMap {
        for (cfg, &(x, z)) in w
            .world_map
            .entity_configs
            .iter()
            .zip(w.world_map.entity_positions.iter())
        {
            if let WorldMapEntityConfig::OverworldPortal { scene_name, .. } = cfg
                && scene_name != dest
            {
                out.insert((i32::from(x) >> 7, i32::from(z) >> 7));
            }
        }
        return out;
    }
    let name = scene_name(session);
    let index = &session.host.index;
    if let Ok(scene) = Scene::load(index, &name)
        && let Ok(Some(man)) = scene.field_man_payload(index)
        && let Ok(mf) = legaia_asset::man_section::parse(&man)
        && let Ok((p, f)) = scene.field_tile_triggers(index)
    {
        let triggers: Vec<TileTrigger> = p.into_iter().chain(f).collect();
        for s in overworld_portal_sites(&mf, &man, &triggers) {
            if s.scene_name != dest {
                out.insert((i32::from(s.overworld_x), i32::from(s.overworld_z)));
            }
        }
    }
    out
}

/// Walk to a door toward `dest` with the pad only. `Ok(entered)` on a scene
/// change (which may not be `dest`; the caller checks).
fn pad_hop(session: &mut BootSession, graph: &DiscGraph, dest: &str) -> Result<String, String> {
    let doors = doors_to(session, graph, dest)?;
    let avoid = hazards(session, dest);
    let (sx, sz) = player_xz(session);
    let start = cell_of(sx, sz);
    // The door the lattice gets closest to.
    let goal = doors
        .iter()
        .map(|d| (i16::from(d.tile.0), i16::from(d.tile.1)))
        .min_by_key(|&g| {
            plan_path(session, start, g, &avoid)
                .and_then(|p| p.last().copied())
                .map_or(i32::MAX, |c| {
                    let t = tile_of(cell_center(c).0, cell_center(c).1);
                    i32::from((t.0 - g.0).abs() + (t.1 - g.1).abs())
                })
        })
        .expect("doors_to is non-empty");
    let dist = |a: (i16, i16)| i32::from((a.0 - goal.0).abs() + (a.1 - goal.1).abs());
    // A door the collision lattice cannot get near is a different finding
    // from a walk that stalls on the way: the scene is split into walk
    // components (map01's north / south halves meet only through `suimon`),
    // and this planner does not route through a crossing scene. A short gap
    // is not that: a door tile reads as a wall from inside, so the lattice
    // routinely ends a few tiles short and the follower presses on at the
    // band (the critical-path ladder's rule).
    if let Some(end) = plan_path(session, start, goal, &avoid).and_then(|p| p.last().copied()) {
        let t = tile_of(cell_center(end).0, cell_center(end).1);
        if dist(t) > DOOR_APPROACH_SLACK {
            return Err(format!(
                "no walkable path: the start's walk component ends {} tiles short of door {goal:?} (closest tile {t:?})",
                dist(t)
            ));
        }
    }
    let mut best = dist(tile_of(sx, sz));
    let mut since = 0u32;
    let mut planned_from = start;
    let mut path = plan_path(session, start, goal, &avoid).unwrap_or_default();
    let walking_mode = session.host.world.mode;
    for _ in 0..PAD_LEG_FRAMES {
        if session.host.world.mode == SceneMode::Battle {
            if let Some(r) = drain_battle(session) {
                return Err(format!("battle on the walk to {dest}: {r:?}"));
            }
            since = 0;
            continue;
        }
        if session.host.world.mode != walking_mode {
            return Err(format!("mode changed to {:?}", session.host.world.mode));
        }
        let (wx, wz) = player_xz(session);
        let cell = cell_of(wx, wz);
        if cell != planned_from {
            path = plan_path(session, cell, goal, &avoid).unwrap_or_default();
            planned_from = cell;
        }
        let (tx, tz) = match path.first() {
            Some(&c) => cell_center(c),
            None => tile_center(goal),
        };
        let pad = pad_for_step(session, (tx - wx).signum(), (tz - wz).signum());
        session.host.world.set_pad(pad);
        match session.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Ok(name),
            Ok(_) => {}
            Err(e) => return Err(format!("tick: {e:#}")),
        }
        if session.host.world.mode == SceneMode::Battle {
            continue;
        }
        let w = &session.host.world;
        if w.cutscene_timeline_active() || w.dialogue_owns_input() || w.active_fmv().is_some() {
            match run(session, DEEP_EXIT_TICKS, true) {
                Run::Entered(s) => return Ok(s),
                Run::Released => {}
                other => return Err(format!("scripted sequence on the walk: {other:?}")),
            }
        }
        let (px, pz) = player_xz(session);
        let d = dist(tile_of(px, pz));
        if d < best {
            best = d;
            since = 0;
        } else {
            since += 1;
            if since >= PAD_STALL_FRAMES {
                return Err(format!(
                    "pad walk stalled at tile {:?} (world ({px},{pz})), {d} tiles short of door {goal:?}",
                    tile_of(px, pz)
                ));
            }
        }
    }
    Err(format!("pad walk to {goal:?} ran out of frames"))
}

// ---------------------------------------------------------------------------
// Segments
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    None = 0,
    Loads = 1,
    Enters = 2,
    Progresses = 3,
    Pad = 4,
}

impl Tier {
    fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Loads => "loads",
            Self::Enters => "enters",
            Self::Progresses => "progresses",
            Self::Pad => "pad",
        }
    }
}

struct SegmentReport {
    key: String,
    from: String,
    to: String,
    route: String,
    tier: Tier,
    /// Why the first tier that did not clear failed.
    stall: Option<String>,
    /// Story flags the next anchor carries that the engine had not set where
    /// the seated run stopped.
    missing_flags: Vec<u16>,
}

/// Is `target` reached in the session's current state?
fn reached(session: &BootSession, target: &Milestone) -> bool {
    if scene_name(session) != target.scene || !walking(session) {
        return false;
    }
    match target.reach.as_str() {
        "control" => released(session),
        "flags" => target
            .reach_flags
            .iter()
            .all(|&f| session.host.world.system_flag_test(f)),
        _ => true,
    }
}

/// Walk-on beat records tried per scene visit.
const MAX_BEATS: usize = 120;

/// The **beats** pass of the seated tier: in the loaded field scene, approach
/// every boss stager whose park gate is clear (the touch dispatch runs its
/// placement record) and step onto every gate-1 walk-on tile whose
/// partition-2 record the live flags let spawn and which is not a door. This
/// is how the seated tier plays a story beat a player triggers by walking
/// somewhere; beats that wait on a conversation, a menu or an item stay
/// unplayed. `Ok(Some(scene))` when a beat left the scene.
fn play_beats(session: &mut BootSession, log: &mut Vec<String>) -> Result<Option<String>, String> {
    use legaia_engine_core::man_field_scripts::{boss_stager_placements, partition2_record_gates};
    if session.host.world.mode != SceneMode::Field {
        return Ok(None);
    }
    let name = scene_name(session);
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return Ok(None);
    };
    let before: BTreeSet<u16> = flags_of_world(session);
    let mut ran = 0usize;
    let finish = |session: &BootSession, log: &mut Vec<String>, ran: usize| {
        let gained = flags_of_world(session).difference(&before).count();
        log.push(format!("{name}: {ran} beat(s) run, +{gained} flag(s)"));
    };
    for p in boss_stager_placements(&mf, &man) {
        if p.park_gate_flag
            .is_some_and(|f| session.host.world.system_flag_test(f))
        {
            continue;
        }
        let at = match (p.station_world, p.spawn_parked) {
            (Some(st), _) => st,
            (None, false) => p.spawn_world,
            (None, true) => continue,
        };
        let (tx, tz) = tile_of(at.0, at.1);
        session
            .host
            .world
            .seat_player_at_tile(tx.clamp(0, 127) as u8, tz.clamp(0, 127) as u8);
        ran += 1;
        match run_while_moving(session, DEEP_EXIT_TICKS) {
            Run::Entered(s) => {
                finish(session, log, ran);
                return Ok(Some(s));
            }
            Run::Battle(b) => return Err(format!("boss stager P1[{}]: {b}", p.placement_index)),
            Run::Error(e) => return Err(e),
            Run::Released | Run::Parked(_) => {}
        }
    }
    let doors: BTreeSet<u8> = overworld_portal_sites(&mf, &man, &triggers)
        .iter()
        .map(|s| s.record)
        .collect();
    // A tile reaches a record only when no earlier entry claims it: the
    // dispatch takes the first primary-then-fallback match
    // (`FUN_801D5630`), so a fallback band shadowed by a primary entry on
    // the same tile never fires its own record.
    let mut first_tile: BTreeMap<u8, (u8, u8)> = BTreeMap::new();
    for t in triggers.iter().filter(|t| t.gate == 1) {
        let owner = triggers
            .iter()
            .find(|u| (u.tile_x, u.tile_z) == (t.tile_x, t.tile_z));
        if owner.is_some_and(|u| u.record == t.record && u.gate == 1) {
            first_tile.entry(t.record).or_insert((t.tile_x, t.tile_z));
        }
    }
    // Talk and walk-on beats unlock each other (a conversation sets the flag
    // a walk-on record's C2 gate needs, and a walk-on cutscene sets the flag
    // a conversation branches on), so both passes repeat while a round still
    // gains flags. Each beat plays at most once per scene visit.
    let mut walked: BTreeSet<u8> = BTreeSet::new();
    let overreach = overreaching_records(&mf, &man, 2);
    for _round in 0..BEAT_ROUNDS {
        let round_start = flags_of_world(session);
        // A talk is re-tried each round while its flag stays clear: its
        // record may branch on a flag the previous round's beats set.
        for slot in talk_beats(session, &mf, &man) {
            if ran >= MAX_BEATS {
                continue;
            }
            ran += 1;
            let f0 = flags_of_world(session);
            let r = talk_to(session, slot);
            trace_beat(session, &f0, || format!("{name} talk P1[{slot}] -> {r:?}"));
            match r {
                Run::Entered(s) => {
                    finish(session, log, ran);
                    return Ok(Some(s));
                }
                Run::Battle(b) => return Err(format!("talk P1[{slot}]: {b}")),
                Run::Error(e) => return Err(e),
                Run::Released | Run::Parked(_) => {}
            }
        }
        for (&rec, &tile) in &first_tile {
            if doors.contains(&rec)
                || walked.contains(&rec)
                || overreach.contains(&usize::from(rec))
                || ran >= MAX_BEATS
            {
                continue;
            }
            let pass = partition2_record_gates(&mf, &man, usize::from(rec))
                .is_some_and(|(c1, c2)| session.host.world.p2_record_gates_pass(&c1, &c2));
            if !pass {
                continue;
            }
            walked.insert(rec);
            step_onto(session, tile);
            ran += 1;
            let f0 = flags_of_world(session);
            let r = run_while_moving(session, DEEP_EXIT_TICKS);
            trace_beat(session, &f0, || {
                format!("{name} walk P2[{rec}] at {tile:?} -> {r:?}")
            });
            match r {
                Run::Entered(s) => {
                    finish(session, log, ran);
                    return Ok(Some(s));
                }
                Run::Battle(b) => return Err(format!("beat P2[{rec}]: {b}")),
                Run::Error(e) => return Err(e),
                Run::Released | Run::Parked(_) => {}
            }
        }
        if flags_of_world(session) == round_start {
            break;
        }
    }
    finish(session, log, ran);
    Ok(None)
}

/// With `LEGAIA_FGL_TRACE` set, print one line per played beat: what ran,
/// how it ended, and the flags it set.
fn trace_beat(session: &BootSession, before: &BTreeSet<u16>, what: impl FnOnce() -> String) {
    if std::env::var_os("LEGAIA_FGL_TRACE").is_none() {
        return;
    }
    let gained: Vec<String> = flags_of_world(session)
        .difference(before)
        .map(|f| format!("0x{f:03X}"))
        .collect();
    eprintln!("    [beat] {} +{gained:?}", what());
}

thread_local! {
    /// The system flags the segment's next anchor carries, when it has one:
    /// what the retail run had set by the time it got there.
    static NEXT_ANCHOR_FLAGS: std::cell::RefCell<Option<BTreeSet<u16>>> =
        const { std::cell::RefCell::new(None) };
}

/// Does the next anchor carry `flag`? `true` when there is no anchor to ask.
fn next_anchor_has(flag: u16) -> bool {
    NEXT_ANCHOR_FLAGS.with(|a| a.borrow().as_ref().is_none_or(|f| f.contains(&flag)))
}

/// The records of `partition` that SET (cleanly, outside a debug picker) a
/// **latch** the next anchor does not carry - a flag some partition-2
/// record of this scene lists in its C1 gate, so setting it shuts that
/// record. The retail run had not played such a beat by the next milestone,
/// and playing it overreaches: `conc2` P2[12] latches `0x3E1`, the C1 gate
/// of the `juui1` hand-off P2[20] the story takes. Flags no gate reads are
/// left alone - a long cutscene sets and clears many a scratch flag the
/// anchor never shows.
fn overreaching_records(
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    partition: usize,
) -> BTreeSet<usize> {
    use legaia_engine_core::man_field_scripts::{
        FlagBank, partition2_record_gates, walk_partition_gflag_sites,
    };
    let n2 = mf.partitions.get(2).map_or(0, Vec::len);
    let latches: BTreeSet<u16> = (0..n2)
        .filter_map(|r| partition2_record_gates(mf, man, r))
        .flat_map(|(c1, _)| c1)
        .collect();
    walk_partition_gflag_sites(mf, man, partition)
        .into_iter()
        .filter(|s| {
            s.bank == FlagBank::System
                && s.kind == FlagKind::Set
                && s.clean
                && !s.text_alias
                && !s.debug_menu
                && latches.contains(&s.flag)
                && !next_anchor_has(s.flag)
        })
        .map(|s| s.record)
        .collect()
}

/// Rounds of the talk + walk-on beat passes per scene visit.
const BEAT_ROUNDS: usize = 3;

/// The **story talks** of the loaded field scene: the talk-NPC placements
/// (partition-1 record index = the slot the interact probe addresses) whose
/// own record carries a clean, non-debug SET of a system flag the live bank
/// still has clear. A conversation that writes no story flag is scenery and
/// is not played.
fn talk_beats(
    session: &BootSession,
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
) -> Vec<u8> {
    use legaia_engine_core::man_field_scripts::{FlagBank, walk_partition_gflag_sites};
    let w = &session.host.world;
    let wanted = |s: &legaia_engine_core::man_field_scripts::GFlagSite| {
        s.bank == FlagBank::System
            && s.kind == FlagKind::Set
            && s.clean
            && !s.text_alias
            && !s.debug_menu
            && !w.system_flag_test(s.flag)
            && next_anchor_has(s.flag)
    };
    let setters1: BTreeSet<usize> = walk_partition_gflag_sites(mf, man, 1)
        .iter()
        .filter(|s| wanted(s))
        .map(|s| s.record)
        .collect();
    let setters2: BTreeSet<usize> = walk_partition_gflag_sites(mf, man, 2)
        .iter()
        .filter(|s| wanted(s))
        .map(|s| s.record)
        .collect();
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let mut slots: Vec<u8> = w
        .npcs
        .positions
        .keys()
        .copied()
        .filter(|s| w.npcs.dialog.contains_key(s) || w.npcs.dialog_prologue.contains_key(s))
        .filter(|&s| {
            // The talk's own record writes a wanted flag, or a partition-2
            // record it spawns does (`station`'s ticket seller spawns the
            // P2[19] departure that latches `0x36B`).
            setters1.contains(&usize::from(s))
                || spawned_p2(mf, man, 1, usize::from(s), n0 + n1, 3)
                    .iter()
                    .any(|r| setters2.contains(r))
        })
        .collect();
    slots.sort_unstable();
    slots
}

/// The partition-2 records `(part, rec)` spawns through op `0x44`
/// (`SPAWN_RECORD`, global index `base + r`), followed `depth` levels.
fn spawned_p2(
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    part: usize,
    rec: usize,
    base: usize,
    depth: usize,
) -> BTreeSet<usize> {
    use legaia_asset::field_disasm::{InsnInfo, LinearWalker};
    use legaia_engine_core::man_field_scripts::partition_record_span;
    let mut out = BTreeSet::new();
    let Some((start, pc0, len)) = partition_record_span(mf, man, part, rec) else {
        return out;
    };
    let body = &man[start..start + len];
    for insn in LinearWalker::new(body, pc0).flatten() {
        if let InsnInfo::SpawnRecord { global_index } = insn.info
            && let Some(r2) = usize::from(global_index).checked_sub(base)
            && r2 < mf.partitions.get(2).map_or(0, Vec::len)
            && out.insert(r2)
            && depth > 1
        {
            out.extend(spawned_p2(mf, man, 2, r2, base, depth - 1));
        }
    }
    out
}

/// Talk to the NPC in placement `slot` as a player does: stand on a tile
/// next to it, face it so the retail interact probe (64 units ahead, the
/// NPC's 72-unit box) lands on it, press Cross, and page the conversation.
/// A talk record often branches on where the player stands (a `0x4D`
/// box test on the player), so each side is tried in turn until one gains a
/// flag. The player goes back where it stood afterwards, so the next press
/// does not re-open the same talk.
fn talk_to(session: &mut BootSession, slot: u8) -> Run {
    let Some(&(nx, nz)) = session.host.world.npcs.positions.get(&slot) else {
        return Run::Released;
    };
    let (bx, bz) = player_xz(session);
    let (tx, tz) = tile_of(nx, nz);
    let claimed = claimed_tiles(session);
    let mut last = Run::Released;
    for (dx, dz) in [(-1i16, 0i16), (1, 0), (0, -1), (0, 1)] {
        let (sx, sz) = (tx + dx, tz + dz);
        if !(0..128).contains(&sx)
            || !(0..128).contains(&sz)
            || claimed.contains(&(sx as u8, sz as u8))
        {
            continue;
        }
        session.host.world.set_pad(0);
        session.host.world.seat_player_at_tile(sx as u8, sz as u8);
        // The one compass sector whose probe point lands in the NPC's box.
        let facing = (0..8u8).find(|&d| {
            session.host.world.face_player_sector(d);
            session.host.world.field_interact_probe_slot() == Some(slot)
        });
        let Some(_) = facing else {
            continue;
        };
        let before = flags_of_world(session);
        session.host.world.set_pad(PadButton::Cross.mask());
        match session.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Run::Entered(name),
            Ok(_) => {}
            Err(e) => return Run::Error(format!("{e:#}")),
        }
        // Page the conversation to its end, then step back off the NPC
        // before anything else runs: a player facing the NPC with the
        // confirm button pulsing re-opens the same talk the frame it ends,
        // and that restart is not what retail runs next - a record the
        // talk spawned is (`town01` P1[40] spawns the P2[25] mist night).
        let mut ended = false;
        for f in 0..DEEP_EXIT_TICKS {
            if session.host.world.mode == SceneMode::Battle {
                if let Some(r) = drain_battle(session) {
                    return r;
                }
                continue;
            }
            let w = &session.host.world;
            if f >= 2 && w.dialog.inline.is_none() && !w.dialogue_owns_input() {
                ended = true;
                break;
            }
            let pad = script_pad(session, f);
            session.host.world.set_pad(pad);
            match session.tick() {
                Ok(SceneTickEvent::SceneEntered { name }) => return Run::Entered(name),
                Ok(_) => {}
                Err(e) => return Run::Error(format!("{e:#}")),
            }
        }
        if !ended {
            return Run::Parked(format!("{} at {}", holder(session), park_site(session)));
        }
        if walking(session) {
            let tile = tile_of(bx, bz);
            session
                .host
                .world
                .seat_player_at_tile(tile.0.clamp(0, 127) as u8, tile.1.clamp(0, 127) as u8);
        }
        let r = run_while_moving(session, DEEP_EXIT_TICKS);
        if !matches!(r, Run::Released) || !walking(session) {
            return r;
        }
        let gained = flags_of_world(session) != before;
        last = r;
        if gained {
            break;
        }
    }
    if walking(session) {
        let tile = tile_of(bx, bz);
        session
            .host
            .world
            .seat_player_at_tile(tile.0.clamp(0, 127) as u8, tile.1.clamp(0, 127) as u8);
    }
    last
}

fn flags_of_world(session: &BootSession) -> BTreeSet<u16> {
    let mut out = BTreeSet::new();
    for (i, b) in session.host.world.flags.system_flags.iter().enumerate() {
        for k in 0..8 {
            if b & (0x80 >> k) != 0 {
                out.insert((i * 8 + k) as u16);
            }
        }
    }
    out
}

/// Drive from the seeded state toward `target`, hop by hop, by seated
/// traversal (with the beats pass) or by the pad. `Ok(())` when reached.
fn traverse(
    session: &mut BootSession,
    graph: &DiscGraph,
    target: &Milestone,
    pad: bool,
    trail: &mut Vec<String>,
) -> Result<(), String> {
    let mut beaten: BTreeSet<String> = BTreeSet::new();
    // Waypoints only steer the seated tier: the pad tier plays no beats.
    let mut via = 0usize;
    let vias: &[String] = if pad { &[] } else { &target.via };
    for _ in 0..MAX_HOPS + vias.len() * 4 {
        // Let whatever the last landing started finish first.
        match run_while_moving(session, SCRIPT_CEILING) {
            Run::Entered(s) => {
                trail.push(format!("{s}(scripted)"));
                continue;
            }
            Run::Released => {}
            Run::Parked(p) => {
                if via >= vias.len() && reached(session, target) {
                    return Ok(());
                }
                return Err(format!("in {}: parked - {p}", scene_name(session)));
            }
            other => return Err(format!("in {}: {other:?}", scene_name(session))),
        }
        if via >= vias.len() && reached(session, target) {
            return Ok(());
        }
        let cur = scene_name(session);
        // A waypoint reached: play its beats once, then head for the next.
        if let Some(w) = vias.get(via)
            && *w == cur
        {
            via += 1;
            let mut log = Vec::new();
            let left = play_beats(session, &mut log)
                .map_err(|b| format!("in {cur} (via), playing beats: {b}"))?;
            trail.push(format!("[via {}]", log.join("; ")));
            if let Some(s) = left {
                trail.push(format!("{s}(beat)"));
            }
            continue;
        }
        let goal = vias.get(via).unwrap_or(&target.scene).clone();
        // In the target scene with its beat unplayed, or stuck on a door:
        // play the scene's walkable beats once, then look again.
        let stuck_here = cur == target.scene && via >= vias.len();
        let hop = if stuck_here {
            Err(String::new())
        } else {
            let Some(route) = graph.route(&cur, &goal) else {
                return Err(format!("no scene route from {cur} to {goal}"));
            };
            let next = route[1].clone();
            let hop = if pad {
                pad_hop(session, graph, &next)
            } else {
                seated_hop(session, graph, &next)
            };
            hop.map_err(|e| {
                let (x, z) = player_xz(session);
                format!("hop {cur} -> {next} at tile {:?}: {e}", tile_of(x, z))
            })
        };
        match hop {
            Ok(entered) => trail.push(entered),
            Err(e) => {
                if !pad && beaten.insert(cur.clone()) {
                    let mut log = Vec::new();
                    let left = play_beats(session, &mut log)
                        .map_err(|b| format!("in {cur}, playing beats: {b}"))?;
                    trail.push(format!("[{}]", log.join("; ")));
                    if let Some(s) = left {
                        trail.push(format!("{s}(beat)"));
                    }
                    continue;
                }
                if stuck_here {
                    let missing: Vec<String> = target
                        .reach_flags
                        .iter()
                        .filter(|&&f| !session.host.world.system_flag_test(f))
                        .map(|f| format!("0x{f:03X}"))
                        .collect();
                    return Err(format!(
                        "in {cur} with control, but reach flag(s) {} never set",
                        missing.join(",")
                    ));
                }
                return Err(e);
            }
        }
    }
    Err(format!("gave up after {MAX_HOPS} hops"))
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic".into()
    }
}

/// Run segment `from -> to`.
fn run_segment(
    inp: &Inputs,
    graph: &DiscGraph,
    from: &Milestone,
    from_anchor: Option<&Anchor>,
    to: &Milestone,
    to_anchor: Option<&Anchor>,
    with_pad: bool,
) -> SegmentReport {
    let route = graph
        .route(&from.scene, &to.scene)
        .map_or_else(|| "(no route)".to_string(), |r| graph.show(&r));
    let mut rep = SegmentReport {
        key: to.id.clone(),
        from: from.id.clone(),
        to: to.id.clone(),
        route,
        tier: Tier::None,
        stall: None,
        missing_flags: Vec::new(),
    };

    NEXT_ANCHOR_FLAGS.with(|a| *a.borrow_mut() = to_anchor.map(|a| a.flags.clone()));
    // -- seated pass: loads, enters, progresses -----------------------------
    let seated = catch_unwind(AssertUnwindSafe(|| {
        let mut session = open_session(&inp.extracted);
        let opts = live_opts(false);
        let mut tier = Tier::None;
        let landed = match seed(&mut session, from, from_anchor, &opts) {
            Ok(s) => s,
            Err(e) => return (tier, Some(format!("seed: {e}")), None),
        };
        if landed != from.scene || !walking(&session) {
            return (
                tier,
                Some(format!(
                    "seed landed {landed} in {:?}, milestone scene is {}",
                    session.host.world.mode, from.scene
                )),
                None,
            );
        }
        tier = Tier::Loads;
        let mut trail = vec![landed];
        match run_while_moving(&mut session, SCRIPT_CEILING) {
            Run::Released => tier = Tier::Enters,
            Run::Entered(s) => {
                trail.push(format!("{s}(scripted)"));
                tier = Tier::Enters;
            }
            Run::Parked(p) => {
                // A parked entry script can still be the segment's success
                // (a milestone reached mid-cutscene), but the tier stops here.
                let flags = session.host.world.flags.system_flags.clone();
                return (
                    tier,
                    Some(format!("entry script never released: {p}")),
                    Some(flags),
                );
            }
            other => {
                let flags = session.host.world.flags.system_flags.clone();
                return (tier, Some(format!("entry: {other:?}")), Some(flags));
            }
        }
        let res = traverse(&mut session, graph, to, false, &mut trail);
        let flags = session.host.world.flags.system_flags.clone();
        match res {
            Ok(()) => (Tier::Progresses, None, Some(flags)),
            Err(e) => (
                tier,
                Some(format!("{e} [trail {}]", trail.join(">"))),
                Some(flags),
            ),
        }
    }));
    let (tier, stall, flags_after) = match seated {
        Ok(r) => r,
        Err(p) => (
            Tier::None,
            Some(format!("PANIC: {}", panic_text(&*p))),
            None,
        ),
    };
    rep.tier = tier;
    rep.stall = stall;
    if let (Some(bank), Some(next)) = (flags_after, to_anchor)
        && rep.tier < Tier::Progresses
    {
        let have = |f: u16| {
            bank.get((f >> 3) as usize)
                .is_some_and(|b| b & (0x80 >> (f & 7)) != 0)
        };
        let prev: BTreeSet<u16> = from_anchor.map(|a| a.flags.clone()).unwrap_or_default();
        rep.missing_flags = next
            .flags
            .iter()
            .copied()
            .filter(|&f| !have(f) && !prev.contains(&f))
            .collect();
    }

    // -- pad pass ---------------------------------------------------------------
    if with_pad && rep.tier >= Tier::Progresses {
        let padded = catch_unwind(AssertUnwindSafe(|| {
            let mut session = open_session(&inp.extracted);
            let opts = live_opts(true);
            seed(&mut session, from, from_anchor, &opts)?;
            let mut trail = vec![scene_name(&session)];
            traverse(&mut session, graph, to, true, &mut trail)
                .map_err(|e| format!("{e} [trail {}]", trail.join(">")))
        }));
        match padded {
            Ok(Ok(())) => rep.tier = Tier::Pad,
            Ok(Err(e)) => rep.stall = Some(format!("pad: {e}")),
            Err(p) => rep.stall = Some(format!("pad PANIC: {}", panic_text(&*p))),
        }
    }
    rep
}

// ---------------------------------------------------------------------------
// Baseline
// ---------------------------------------------------------------------------

fn baseline_path() -> PathBuf {
    repo_root().join("scripts/replays/full_game_baseline.toml")
}

#[derive(Debug, Default, Deserialize)]
struct Baseline {
    #[serde(default)]
    headline: BTreeMap<String, usize>,
    #[serde(default)]
    segments: BTreeMap<String, usize>,
}

fn read_baseline() -> Baseline {
    std::fs::read_to_string(baseline_path())
        .ok()
        .and_then(|t| toml::from_str(&t).ok())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Part A: the spine is anchored, ordered and routed
// ---------------------------------------------------------------------------

/// Every anchor loads, sits in the scene the spine names, and the anchors
/// fall in story order; prints the disc route for each segment and how many
/// of each anchor's flags lie past the save bridge's story window.
#[test]
fn part_a_spine_is_anchored_ordered_and_routed() {
    let Some(inp) = inputs() else { return };
    let spine = load_spine();
    let index = ProtIndex::open_extracted(&inp.extracted).expect("open ProtIndex");
    let graph = DiscGraph::build(&index);
    eprintln!(
        "[spine] {} milestones; disc graph: {} scenes, {} 0x3F edges",
        spine.len(),
        graph.edges.len(),
        graph.edges.values().map(BTreeSet::len).sum::<usize>()
    );

    let mut anchors: Vec<Option<Anchor>> = Vec::new();
    let mut bad = Vec::new();
    // A waypoint must be a scene the disc graph knows.
    for m in &spine {
        for v in m.via.iter().filter(|v| !graph.edges.contains_key(*v)) {
            bad.push(format!("{}: via scene {v} is not in the disc graph", m.id));
        }
    }
    for m in &spine {
        let a = m.anchor.as_ref().map(|r| (r, load_anchor(&inp.library, r)));
        match a {
            Some((r, Ok(a))) => {
                let tail = a
                    .flags
                    .iter()
                    .filter(|&&f| usize::from(f >> 3) >= SAVE_WINDOW_SYSTEM_BYTES)
                    .count();
                if tail > 0 {
                    bad.push(format!(
                        "{}: {tail} flag(s) lie past the save lift's story window",
                        m.id
                    ));
                }
                eprintln!(
                    "[anchor] {:24} {:8} {:44} scene={:8} t={:>8} flags={:4} past-window={tail}",
                    m.id,
                    m.scene,
                    r.describe(),
                    a.scene,
                    a.play_time,
                    a.flags.len()
                );
                if a.scene != m.scene {
                    bad.push(format!(
                        "{}: anchor is in {}, spine says {}",
                        m.id, a.scene, m.scene
                    ));
                }
                anchors.push(Some(a));
            }
            Some((r, Err(e))) => {
                eprintln!("[anchor] {:24} {} - UNREADABLE: {e}", m.id, r.describe());
                bad.push(format!("{}: {e}", m.id));
                anchors.push(None);
            }
            None => {
                eprintln!("[anchor] {:24} {:8} (no anchor)", m.id, m.scene);
                anchors.push(None);
            }
        }
    }

    // Order: each anchor's flag set is (nearly) a superset of its
    // predecessor's. Counted, not asserted per pair: separate sessions differ
    // by a few optional flags.
    let present: Vec<(&Milestone, &Anchor)> = spine
        .iter()
        .zip(anchors.iter())
        .filter(|(m, _)| m.order_check)
        .filter_map(|(m, a)| a.as_ref().map(|a| (m, a)))
        .collect();
    let mut inversions = Vec::new();
    for w in present.windows(2) {
        let (pm, pa) = w[0];
        let (nm, na) = w[1];
        let lost = pa.flags.difference(&na.flags).count();
        let gained = na.flags.difference(&pa.flags).count();
        let route = graph.route(&pm.scene, &nm.scene).map_or_else(
            || "NO ROUTE".to_string(),
            |r| format!("{} hop(s) {}", r.len() - 1, graph.show(&r)),
        );
        eprintln!(
            "[segment] {:24} -> {:24} +{gained:<3} -{lost:<3} {route}",
            pm.id, nm.id
        );
        if lost > gained {
            inversions.push(format!("{} -> {} (+{gained} -{lost})", pm.id, nm.id));
        }
    }
    // Reach-flag candidates for the same-scene ("flags") milestones: the flags
    // the anchor gained over its predecessor that a clean disc SET site in
    // the milestone's own scene writes.
    let census = system_flag_census(&index, index.cdname_scene_names());
    for (i, m) in spine.iter().enumerate() {
        if m.reach != "flags" || i == 0 {
            continue;
        }
        let (Some(prev), Some(cur)) = (&anchors[i - 1], &anchors[i]) else {
            continue;
        };
        let cands: Vec<String> = cur
            .flags
            .difference(&prev.flags)
            .filter(|f| {
                census.get(f).is_some_and(|sites| {
                    sites
                        .iter()
                        .any(|s| s.kind == FlagKind::Set && s.clean && s.scene_name == m.scene)
                })
            })
            .map(|f| {
                let sites: Vec<String> = census[f]
                    .iter()
                    .filter(|s| s.kind == FlagKind::Set && s.clean && s.scene_name == m.scene)
                    .map(|s| format!("P{}[{}]", s.partition, s.record))
                    .collect();
                format!("0x{f:03X}({})", sites.join("/"))
            })
            .collect();
        eprintln!(
            "[reach] {:24} reach_flags {:?} - gained flags SET in {}: {}",
            m.id,
            m.reach_flags,
            m.scene,
            cands.join(" ")
        );
        for f in &m.reach_flags {
            if !cur.flags.contains(f) {
                bad.push(format!(
                    "{}: reach flag 0x{f:03X} is clear in its own anchor",
                    m.id
                ));
            }
        }
    }

    // Routes for the pairs the order check skipped (an off-order anchor).
    for w in spine.windows(2) {
        if w[0].order_check && w[1].order_check {
            continue;
        }
        let route = graph.route(&w[0].scene, &w[1].scene).map_or_else(
            || "NO ROUTE".to_string(),
            |r| format!("{} hop(s) {}", r.len() - 1, graph.show(&r)),
        );
        eprintln!(
            "[segment] {:24} -> {:24} (unordered) {route}",
            w[0].id, w[1].id
        );
    }
    assert!(
        bad.is_empty(),
        "spine anchors disagree with the spine: {bad:#?}"
    );
    assert!(
        inversions.is_empty(),
        "spine order contradicts the anchors' flag sets: {inversions:#?}"
    );
}

// ---------------------------------------------------------------------------
// Part B: the ladder
// ---------------------------------------------------------------------------

#[test]
fn part_b_full_game_ladder() {
    let Some(inp) = inputs() else { return };
    let spine = load_spine();
    let index = ProtIndex::open_extracted(&inp.extracted).expect("open ProtIndex");
    let graph = DiscGraph::build(&index);
    let census = system_flag_census(&index, index.cdname_scene_names());
    let anchors: Vec<Option<Anchor>> = spine
        .iter()
        .map(|m| {
            m.anchor
                .as_ref()
                .and_then(|r| load_anchor(&inp.library, r).ok())
        })
        .collect();
    let only: Option<BTreeSet<String>> = std::env::var("LEGAIA_FGL_ONLY")
        .ok()
        .map(|s| s.split(',').map(str::to_string).collect());
    let with_pad = std::env::var_os("LEGAIA_FGL_NO_PAD").is_none();

    let mut reports: Vec<SegmentReport> = Vec::new();
    for i in 0..spine.len() - 1 {
        let (from, to) = (&spine[i], &spine[i + 1]);
        if let Some(only) = &only
            && !only.contains(&to.id)
        {
            continue;
        }
        let t0 = std::time::Instant::now();
        let rep = run_segment(
            &inp,
            &graph,
            from,
            anchors[i].as_ref(),
            to,
            anchors[i + 1].as_ref(),
            with_pad,
        );
        eprintln!(
            "[seg {i:2}] {:24} -> {:24} {:10} ({:.1}s){}",
            rep.from,
            rep.to,
            rep.tier.name(),
            t0.elapsed().as_secs_f32(),
            rep.stall
                .as_ref()
                .map_or(String::new(), |s| format!("\n          stall: {s}"))
        );
        reports.push(rep);
    }

    // -- table ---------------------------------------------------------------
    eprintln!();
    eprintln!("== full-game ladder ==");
    eprintln!("{:>3} {:24} {:24} {:10} route", "#", "from", "to", "tier");
    for (i, r) in reports.iter().enumerate() {
        eprintln!(
            "{i:>3} {:24} {:24} {:10} {}",
            r.from,
            r.to,
            r.tier.name(),
            r.route
        );
    }
    eprintln!();
    eprintln!("== stalls ==");
    for r in &reports {
        if let Some(s) = &r.stall {
            eprintln!("- {} -> {}: {s}", r.from, r.to);
        }
        if !r.missing_flags.is_empty() {
            let shown: Vec<String> = r
                .missing_flags
                .iter()
                .take(6)
                .map(|f| {
                    let setters: Vec<String> = census
                        .get(f)
                        .into_iter()
                        .flatten()
                        .filter(|s| s.kind == FlagKind::Set && s.clean)
                        .take(3)
                        .map(|s| format!("{} P{}[{}]", s.scene_name, s.partition, s.record))
                        .collect();
                    format!(
                        "0x{f:03X} (set by {})",
                        if setters.is_empty() {
                            "no clean disc SET site".to_string()
                        } else {
                            setters.join(" / ")
                        }
                    )
                })
                .collect();
            eprintln!(
                "    {} flag(s) the next anchor has and the engine never set; first: {}",
                r.missing_flags.len(),
                shown.join(", ")
            );
        }
    }

    // -- headline ------------------------------------------------------------
    let contiguous = |t: Tier| reports.iter().take_while(|r| r.tier >= t).count();
    let full = only.is_none();
    let head_progress = contiguous(Tier::Progresses);
    let head_pad = contiguous(Tier::Pad);
    let tier_sum: usize = reports.iter().map(|r| r.tier as usize).sum();
    let denom = reports.len();
    eprintln!();
    eprintln!(
        "[headline] cold New Game reaches {head_progress}/{denom} milestones contiguously at `progresses`, {head_pad}/{denom} at `pad`; tier sum {tier_sum}/{}",
        denom * Tier::Pad as usize
    );
    let per_tier = |t: Tier| reports.iter().filter(|r| r.tier >= t).count();
    eprintln!(
        "[headline] segments clearing loads {} / enters {} / progresses {} / pad {} of {denom}",
        per_tier(Tier::Loads),
        per_tier(Tier::Enters),
        per_tier(Tier::Progresses),
        per_tier(Tier::Pad)
    );

    eprintln!();
    eprintln!("# paste into scripts/replays/full_game_baseline.toml when a score rises:");
    eprintln!("[headline]");
    eprintln!("contiguous_progresses = {head_progress}");
    eprintln!("contiguous_pad = {head_pad}");
    eprintln!("tier_sum = {tier_sum}");
    eprintln!("[segments]");
    for r in &reports {
        eprintln!("{} = {}", r.key, r.tier as usize);
    }

    if !full {
        eprintln!("[note] LEGAIA_FGL_ONLY set: baseline not asserted");
        return;
    }
    let base = read_baseline();
    let mut regressions = Vec::new();
    for (k, want) in &base.segments {
        let got = reports
            .iter()
            .find(|r| &r.key == k)
            .map_or(0, |r| r.tier as usize);
        if got < *want {
            regressions.push(format!("segment {k}: tier {got} < baseline {want}"));
        }
    }
    for (k, got) in [
        ("contiguous_progresses", head_progress),
        ("contiguous_pad", head_pad),
        ("tier_sum", tier_sum),
    ] {
        let want = base.headline.get(k).copied().unwrap_or(0);
        if got < want {
            regressions.push(format!("{k}: {got} < baseline {want}"));
        }
    }
    assert!(
        regressions.is_empty(),
        "full-game ladder regressed: {regressions:#?}"
    );
}
