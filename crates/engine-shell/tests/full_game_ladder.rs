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
//! scene graph, not locomotion. Both tiers fight through the command ring
//! with the pad: heal or revive from the item window, cast the strongest
//! affordable damage spell, else enter an arts string through `Command` (a
//! sparring tutorial's lesson command in a tutorial). The
//! seated tier talks to the NPCs whose records reach a flag the next anchor
//! carries or a destination the route needs, and the pad tier plays the same
//! beats by walking to them; neither tier buys or equips (the pad tier opens
//! the pause menu only to heal or burn an Incense), so a story beat that waits on one reads as
//! a stall at that beat. The route follows `0x3F` scene changes and FMV hand-offs; a
//! transport an entry script spawns on arrival is a missing edge (see
//! `docs/tooling/full-game-ladder.md`).
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
use legaia_engine_core::world::{SceneMode, WorldMapEntityConfig, world_map_camera_relative_bits};
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
    /// `false` for an anchor that names the scene but is not a save of the
    /// playthrough at that point (a door-tile poke from an earlier state):
    /// the segment after it seeds from the nearest earlier milestone whose
    /// anchor does seed, so it starts from the story state the run had.
    #[serde(default = "yes")]
    seeds_next: bool,
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
/// SC offset of the pause menu's Field Move word (`0x800846CC`, Walk `0` /
/// Run non-zero). The run selector XORs it with the held run button
/// (`docs/subsystems/field-locomotion.md`), and it is inside the window a
/// card load restores, so the anchor's player walks or runs by it.
const SC_FIELD_MOVE: usize = 0x58C;
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
    /// `(from, to)` -> every entry tile a `0x3F` from `from` lands on in
    /// `to`: which side of `to` a round trip through `from` delivers to.
    landings: BTreeMap<(String, String), BTreeSet<(u8, u8)>>,
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
        let mut landings: BTreeMap<(String, String), BTreeSet<(u8, u8)>> = BTreeMap::new();
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
                // Landings come from the partition-2 door records only - the
                // records a walk-on band or a scripted beat runs, which are
                // the exits the pad hand leaves a crossing by. A
                // partition-1 talk record's `0x3F` is a conversation's own
                // exit on its own story gate: `dolk2` P1[47]'s lands on
                // `map01` `(65, 50)`, which made `dolk2` read as the way to
                // `vell`'s side when neither of its gate bands goes there
                // (retail's playthrough drains `suimon` instead: `0x27B` is
                // set and P1[47]'s other write, `0x17D`, is not, in every
                // card save from `PRO-01` on). `scene_destinations` keeps one
                // entry per destination; a scene with two exits to one map
                // (a town's two gates) has two landings.
                for s in legaia_asset::man_edit::scene_change_sites(man) {
                    if s.partition == 2 && s.name != name && set.contains(&s.name) {
                        landings
                            .entry((name.clone(), s.name.clone()))
                            .or_default()
                            .insert((s.entry_x & 0x7F, s.entry_z & 0x7F));
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
            landings,
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
        self.route_avoiding(from, to, &BTreeSet::new())
    }

    /// [`Self::route`] without the `dead` edges.
    fn route_avoiding(
        &self,
        from: &str,
        to: &str,
        dead: &BTreeSet<(String, String)>,
    ) -> Option<Vec<String>> {
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
                if dead.contains(&(cur.clone(), n.clone())) {
                    continue;
                }
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
    // The save's Field Move option. The engine keeps it as a host setting
    // rather than save data, so the seed carries it the way the card load
    // carries it in retail: a timed script (`jouind`'s two-switch gate gives
    // 50 vsyncs of free walk between the switches) is paced for the player
    // the anchor recorded, who had Run on.
    session.host.world.locomotion.run_default = i32_at(&a.sc, SC_FIELD_MOVE) != 0;
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
    if w.scene_transition_hold.is_some() {
        "a scene change in flight"
    } else if w.cutscene_timeline_active() {
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

/// The partition-2 record the modal cutscene timeline is running, matched
/// by its bytes against the scene's records.
fn timeline_p2_record(session: &BootSession) -> Option<u8> {
    let tl = session.host.world.cutscene.timeline.as_ref()?;
    let (mf, man, _) = scene_man_and_triggers(session)?;
    let n2 = mf.partitions.get(2)?.len();
    (0..n2).find_map(|i| {
        let (start, _, len) =
            legaia_engine_core::man_field_scripts::partition_record_span(&mf, &man, 2, i)?;
        (len == tl.bytecode.len() && man.get(start..start + len)? == tl.bytecode.as_slice())
            .then_some(i as u8)
    })
}

fn released(session: &BootSession) -> bool {
    let w = &session.host.world;
    // A scene change parked behind the streaming actor's countdown has not
    // released anything: the door is mid-flight.
    walking(session)
        && w.scene_transition_hold.is_none()
        && !w.cutscene_timeline_active()
        && !w.dialogue_owns_input()
        && w.field_vm.helper_contexts.is_empty()
        && w.field_vm.pending_record_spawns.is_empty()
        && w.active_fmv().is_none()
}

/// Fight what a beat committed. A record that ends on `3E FF` (`chitei2`
/// P2[13], the Jette fight) is gone before its battle's intro runs, so the
/// beat reads as released while the fight is still to come - and what the
/// story does next hangs on the post-battle return (the scene system script
/// re-runs and spawns the next record). Anything but a released run, or a
/// release with no fight committed, passes through.
///
/// A beat that reaches the milestone it is played for stops there, fight or
/// no fight: `chitei2` P2[11] sets the `0x470` its milestone waits on, then
/// stages a battle that belongs to the next stretch.
fn fight_committed(session: &mut BootSession, r: Run) -> Run {
    if !matches!(r, Run::Released) || !session.host.world.field_scripts_held_for_battle() {
        return r;
    }
    let reached_here =
        BEAT_TARGET.with(|t| t.borrow().as_ref().is_some_and(|m| reached(session, m)));
    if reached_here {
        return r;
    }
    for _ in 0..SETTLE_TICKS {
        if session.host.world.mode == SceneMode::Battle {
            if let Some(r) = drain_battle(session) {
                return r;
            }
            return run_while_moving(session, DEEP_EXIT_TICKS);
        }
        session.host.world.set_pad(0);
        match session.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Run::Entered(name),
            Ok(_) => {}
            Err(e) => return Run::Error(format!("{e:#}")),
        }
    }
    r
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
/// A battle's tick budget under the pad fighter. A boss the fighter wears
/// down with summons, arts and heals runs well past 30 000 ticks.
const BATTLE_TICKS: usize = 60_000;
/// Past [`BATTLE_TICKS`] a fight goes on while the foes are still losing HP:
/// it ends unresolved only after this long without a foe's HP dropping, or at
/// [`BATTLE_TICKS_CAP`]. A boss whose specials land (Cort's Guilty Cross in
/// `chitei2`) costs the party turns on heals, and a fight it is winning
/// outlasts the flat budget - a player keeps fighting it.
const BATTLE_STALL_TICKS: usize = 36_000;
const BATTLE_TICKS_CAP: usize = 600_000;
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
    // Nothing holds the frame: a Cross here is a talk, not a page turn. A
    // door seat beside an NPC (`rayman`'s gate guard, re-seated beside the
    // `tunnelb` door once `0x1FC` is up) otherwise opened his talk on the
    // frame the walk-on band was due.
    if released(session) {
        return 0;
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

/// The pads a script's leading held-pad poll chain passes, when the pad
/// holder sits on one: consecutive op-`0x42` mode-1 tests (`42 01 <i> ..`),
/// each against one compass point (`0x801F28D0`).
fn poll_chain_pads(session: &BootSession) -> Option<Vec<u16>> {
    let w = &session.host.world;
    let (bc, mut pc) = if let Some(tl) = w.cutscene.timeline.as_ref() {
        (&tl.bytecode, tl.pc)
    } else {
        let h = w.field_vm.helper_contexts.first()?;
        (&h.bytecode, h.pc)
    };
    let (d, l, u, r) = (
        PadButton::Down.mask(),
        PadButton::Left.mask(),
        PadButton::Up.mask(),
        PadButton::Right.mask(),
    );
    let compass = [d, d | l, l, l | u, u, u | r, r, r | d];
    let mut pads = Vec::new();
    while bc.get(pc) == Some(&0x42) && bc.get(pc + 1) == Some(&0x01) {
        pads.push(*compass.get(usize::from(*bc.get(pc + 2)?))?);
        pc += 5;
    }
    (!pads.is_empty()).then_some(pads)
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
    let w = &session.host.world;
    let menu = |bytecode: &std::sync::Arc<Vec<u8>>,
                panel: &legaia_engine_core::dialog::OwnedDialogPanel| {
        let pk = panel.picker()?;
        // Keyed on the record's bytes, not its allocation: each talk
        // re-installs the record in a fresh buffer, so a pointer key made
        // every conversation the picker's first opening and the hand never
        // got past option 0 (`tunnelc` P1[4], Xain: only option 1 raises
        // the `0x325` the fight waits on).
        let id = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            bytecode.as_slice().hash(&mut h);
            h.finish() as usize
        };
        panel.menu_active().then_some((
            (id, pk.open),
            pk.n.max(1),
            panel.picker_cursor(),
            panel.picker_takes_input(),
        ))
    };
    // A talk's box, else a script's: the modal timeline's, else the first
    // spawned record holding one (the pad goes to the same box the engine
    // routes it to). A cutscene picker that always takes its default can
    // loop a record forever (`town0d` P2[31], the song rehearsal).
    let open = w
        .dialog
        .inline
        .as_ref()
        .and_then(|id| menu(&id.bytecode, id.panel.as_ref()?))
        .or_else(|| {
            // The FIRST box, picker or not: a later record's picker the
            // engine does not route to must not steer the pad (`dolk2`'s
            // market beat stacks three spawned records' boxes; pressing Down
            // for the third's picker left the first's page unturned).
            let tl = w
                .cutscene
                .timeline
                .iter()
                .filter(|t| t.dialog.is_some())
                .chain(
                    w.field_vm
                        .helper_contexts
                        .iter()
                        .filter(|t| t.dialog.is_some()),
                )
                .next()?;
            menu(&tl.bytecode, tl.dialog.as_ref()?)
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
    let want = split_stay_option(session)
        .or_else(|| beat_progress_option(session))
        .unwrap_or((visit - 1) % n);
    Some(if cursor < want {
        PadButton::Down.mask()
    } else if cursor > want {
        PadButton::Up.mask()
    } else {
        PadButton::Cross.mask()
    })
}

thread_local! {
    /// The partition-2 record of the walk-on beat [`play_beats`] is walking
    /// onto, while it does.
    static PLAYING_BEAT: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}

/// On the walk-on beat being played, the option of its own picker that
/// carries the story on: the first whose branch spawns a record (`0x44`)
/// and changes no scene before its first `0x21`. `nilboa` P2[20] ("We have
/// to jump down to where Koru is! Are you ready?") spawns the Delilas
/// family's challenge P2[21] only on "I'm ready", and the band is walked
/// once per visit, so the rotation's first answer, "Now wait a second!",
/// left the chain - and the party's way off the north half - unplayed.
fn beat_progress_option(session: &BootSession) -> Option<usize> {
    let beat = PLAYING_BEAT.with(std::cell::Cell::get)?;
    if timeline_p2_record(session) != Some(beat) {
        return None;
    }
    let (count, targets, bc) = open_picker(session)?;
    // The branch runs to its first `0x21`: "I'm ready" plays some eighty
    // instructions of the jump down before its `44 63`.
    let spawns = |mut at: usize| {
        for _ in 0..512 {
            let Ok(i) = legaia_asset::field_disasm::decode(&bc, at) else {
                return false;
            };
            match bc[at] & 0x7F {
                0x44 => return true,
                0x3F | 0x21 => return false,
                _ => {}
            }
            at = match i.info {
                legaia_asset::field_disasm::InsnInfo::JmpRel { target, .. } => target,
                _ => at + i.size.max(1),
            };
        }
        false
    };
    (0..count.min(4)).find(|&k| spawns(targets[k]))
}

/// The picker the modal timeline (or the first spawned record with a box)
/// has open: option count, branch targets and the record's bytes.
fn open_picker(session: &BootSession) -> Option<(usize, [usize; 4], Vec<u8>)> {
    use legaia_asset::field_disasm::{InsnInfo, decode};
    let w = &session.host.world;
    let tl = w
        .cutscene
        .timeline
        .iter()
        .filter(|t| t.dialog.is_some())
        .chain(
            w.field_vm
                .helper_contexts
                .iter()
                .filter(|t| t.dialog.is_some()),
        )
        .next()?;
    let bc = tl.bytecode.as_slice();
    let mut pc = tl.pc;
    for _ in 0..16 {
        let Ok(i) = decode(bc, pc) else { break };
        if let InsnInfo::Picker { count, targets, .. } = i.info {
            return Some((count as usize, targets, bc.to_vec()));
        }
        pc += i.size.max(1);
    }
    None
}

/// While a three-actor talk holds the party split (`43 02`, `nilboa`'s Three
/// Tunnels), the option of a script's picker that keeps the split: the
/// first whose branch spawns no record (`0x44`) and changes no scene
/// (`0x3F`) before its first `0x21`. The tunnel mouths (`nilboa` P2[4] /
/// P2[5] / P2[6]) ask "go back outside?" and their Yes spawns P2[28], which
/// regroups the party and clears the members' progress flags; their No
/// steps the walker back. `None` outside a split, or when no option stays.
fn split_stay_option(session: &BootSession) -> Option<usize> {
    use legaia_asset::field_disasm::{InsnInfo, decode};
    let w = &session.host.world;
    w.dialog.three_actor_talk.as_ref()?;
    let tl = w
        .cutscene
        .timeline
        .iter()
        .filter(|t| t.dialog.is_some())
        .chain(
            w.field_vm
                .helper_contexts
                .iter()
                .filter(|t| t.dialog.is_some()),
        )
        .next()?;
    let bc = tl.bytecode.as_slice();
    // The box's text runs from the record's PC up to the picker it opens.
    let mut pc = tl.pc;
    let mut picker = None;
    for _ in 0..16 {
        let Ok(i) = decode(bc, pc) else { break };
        if let InsnInfo::Picker { count, targets, .. } = i.info {
            picker = Some((count as usize, targets));
            break;
        }
        pc += i.size.max(1);
    }
    let (count, targets) = picker?;
    let stays = |mut at: usize| {
        for _ in 0..48 {
            let Ok(i) = decode(bc, at) else { return true };
            match bc[at] & 0x7F {
                0x44 | 0x3F => return false,
                0x21 => return true,
                _ => {}
            }
            at = match i.info {
                InsnInfo::JmpRel { target, .. } => target,
                _ => at + i.size.max(1),
            };
        }
        true
    };
    (0..count.min(4)).find(|&k| stays(targets[k]))
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

/// The scenes a partition-2 record reaches through an op-`49 04` floor list
/// (empty for any other record). The record is followed from its start with
/// only the flags it sets itself raised, to the list it parks on - a pad
/// first raises its own floor and one of two list variants (`kor3` P2[9]
/// sets `0x136`, the full eight-floor list; P2[10] sets `0x137`, the
/// four-row list from row 4 that leaves out `kor`) - and a row counts only
/// inside that list's visible band. Each row's scene is the first named
/// scene change behind the record's test of `base + row`.
fn flag_window_destinations(
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    record: usize,
) -> BTreeSet<String> {
    use legaia_asset::field_disasm::{FlagKind, InsnInfo, decode, scene_change_name};
    use legaia_engine_core::field_submode_flag_window::descriptor_from_operand;
    use legaia_engine_core::man_field_scripts::partition_record_span;
    let mut out = BTreeSet::new();
    let Some((start, pc0, len)) = partition_record_span(mf, man, 2, record) else {
        return out;
    };
    let body = &man[start..start + len];
    let mut raised: BTreeSet<u16> = BTreeSet::new();
    let mut pc = pc0;
    let mut list = None;
    for _ in 0..512 {
        let Ok(i) = decode(body, pc) else { break };
        match i.info {
            InsnInfo::StateResume { sub_op: 4, .. } => {
                list = Some(descriptor_from_operand(
                    body.get(pc + 1..pc + 7).unwrap_or(&[]),
                ));
                break;
            }
            InsnInfo::SystemFlag {
                kind: FlagKind::Set,
                idx,
                ..
            } => {
                raised.insert(idx);
            }
            InsnInfo::SystemFlag {
                kind: FlagKind::Clear,
                idx,
                ..
            } => {
                raised.remove(&idx);
            }
            InsnInfo::SystemFlag {
                kind: FlagKind::Test,
                idx,
                target: Some(t),
                ..
            } if raised.contains(&idx) => {
                pc = t;
                continue;
            }
            InsnInfo::JmpRel { target, .. } => {
                pc = target;
                continue;
            }
            InsnInfo::SceneChange { .. } => break,
            _ => {}
        }
        if i.size == 0 {
            break;
        }
        pc += i.size;
    }
    let Some(desc) = list else {
        return out;
    };
    let (base, first) = (desc.base_flag, i32::from(desc.first_visible));
    let visible = first..first + i32::from(desc.rows);
    let mut pc = 0usize;
    while pc < body.len() {
        let Ok(insn) = decode(body, pc) else {
            pc += 1;
            continue;
        };
        if let InsnInfo::SystemFlag {
            kind: FlagKind::Test,
            idx,
            target: Some(t),
            ..
        } = insn.info
            && visible.contains(&(i32::from(idx) - base))
        {
            let mut q = t;
            for _ in 0..64 {
                let Ok(i) = decode(body, q) else { break };
                if let Some(name) = scene_change_name(body, &i) {
                    out.insert(name.to_ascii_lowercase());
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
    out
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
        if let Err(e) = pad_budget(session) {
            return Run::Parked(e);
        }
        if session.host.world.mode == SceneMode::Battle {
            if let Some(r) = drain_battle(session) {
                return r;
            }
            continue;
        }
        // A save point or a ready check presses the menu button itself
        // (`World::scripted_menu_open_pending`), and `BootSession::tick` opens
        // the menu; answer it the way a player who is not saving does.
        let pad = match scripted_menu_pad(session, f) {
            Some(p) => p,
            None => script_pad(session, f),
        };
        // The naming prompt (the opening's op-0x49) takes this pad's edge
        // inside `BootSession::tick`, as it does in both play hosts.
        session.host.world.set_pad(pad);
        let before = player_xz(session);
        let site = std::env::var_os("LEGAIA_FGL_POS_TRACE").map(|_| park_site(session));
        let r = session.tick();
        if let Some(site) = site {
            let after = player_xz(session);
            let now = park_site(session);
            if before != after || now != site {
                eprintln!("      [pos] {before:?} -> {after:?} at {site} -> {now}");
            }
        }
        match r {
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

/// The pad that closes a pause menu a script opened: back out of the save
/// screen, dismiss the notice panel, and answer the ready check Yes. Pressed
/// on alternate frames, because the menu reads edges. `None` while no menu is
/// up.
fn scripted_menu_pad(session: &BootSession, f: usize) -> Option<u16> {
    use legaia_engine_core::field_menu::FieldMenuPhase;
    let menu = session.field_menu.as_ref()?;
    let button = if session.field_menu_sub.is_some() {
        PadButton::Circle
    } else {
        match menu.phase() {
            FieldMenuPhase::Notice => PadButton::Cross,
            FieldMenuPhase::ReadyConfirm { cursor: 0, .. } => PadButton::Cross,
            FieldMenuPhase::ReadyConfirm { .. } => PadButton::Left,
            _ => PadButton::Circle,
        }
    };
    Some(if f.is_multiple_of(2) {
        button.mask()
    } else {
        0
    })
}

/// Close a pause menu a script opened mid-walk (a save point the route
/// brushed), answering it with [`scripted_menu_pad`]. Returns whether one was
/// up.
fn close_scripted_menu(session: &mut BootSession) -> bool {
    if session.field_menu.is_none() {
        return false;
    }
    for g in 0..600 {
        let Some(pad) = scripted_menu_pad(session, g) else {
            break;
        };
        session.host.world.set_pad(pad);
        let _ = session.tick();
    }
    session.host.world.set_pad(0);
    true
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

thread_local! {
    /// Set while [`pad_hop`] drains a battle that interrupted its walk - a
    /// random encounter on a travel leg, which [`fight_pad`] flees.
    static FLEE_ENCOUNTERS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// `(actor, party HP sum)` pairs whose item window held nothing worth
    /// using: the fighter does not reopen it until someone's HP moves.
    static NO_ITEM: std::cell::RefCell<HashSet<(u8, u32)>> = Default::default();
    /// Actors whose Magic window held no affordable damage spell this battle.
    static NO_MAGIC: std::cell::RefCell<HashSet<u8>> = Default::default();
    /// The foe round a solo duel last chose Spirit in
    /// ([`duel_wants_spirit`]): the round after it swings the gauge it built.
    static DUEL_SPIRIT_ROUND: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Percent of max HP below which the fighter heals a member.
const HEAL_BELOW_PCT: u32 = 45;
/// [`HEAL_BELOW_PCT`] against a boss ([`boss_fight`]).
const BOSS_HEAL_BELOW_PCT: u32 = 60;

/// A fight that forbids running against a foe of eight times the party's
/// largest HP or more (`dohaty`'s 17200 to a 1256-HP Gala).
fn boss_fight(w: &legaia_engine_core::world::World) -> bool {
    if !w.battle.no_escape {
        return false;
    }
    let n = w.party.party_count.clamp(1, 3) as usize;
    let ours = (0..n)
        .map(|i| u32::from(w.actors[i].battle.max_hp))
        .max()
        .unwrap_or(1)
        .max(1);
    (n..w.actors.len()).any(|i| {
        let a = &w.actors[i].battle;
        a.hp > 0 && u32::from(a.max_hp) >= ours * 8
    })
}

/// The heal threshold for this fight. Against a boss a member tops up below
/// three fifths: its party-wide hit is unseen until it lands, and the
/// `dohaty` fight's first one (832 / 913 / 671) dropped two members sitting
/// at 54% and 53% of their HP, above the ordinary threshold.
fn heal_below_pct(w: &legaia_engine_core::world::World) -> u32 {
    if boss_fight(w) {
        BOSS_HEAL_BELOW_PCT
    } else {
        HEAL_BELOW_PCT
    }
}

thread_local! {
    /// The largest HP loss one member took between two of the party's
    /// command windows this battle. A member is healed while it could not
    /// survive another such stretch - the threshold a player reads off the
    /// last bad round, not a fixed fraction. A round, not a hit: a fast foe
    /// acts twice before the party's next input (Lu Delilas's swing then her
    /// Plasma Strike), and a cast lands its flurry and its burst as separate
    /// HP writes.
    static BIGGEST_HIT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Per member, the HP lost since the party's last command window.
    static ROUND_LOSS: std::cell::Cell<[u32; 3]> = const { std::cell::Cell::new([0; 3]) };
    /// Per acting slot, the ally its last committed item was aimed at, so a
    /// later member of the same round counts that heal as already coming.
    static ITEM_TARGET: std::cell::RefCell<[Option<u8>; 3]> = const { std::cell::RefCell::new([None; 3]) };
    /// The party's total HP loss in each finished round of this battle, in
    /// order - the record [`big_round_due`] reads a foe's cadence off.
    static ROUND_HISTORY: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The largest per-member loss of a finished round in which two or more
    /// members lost HP: the size of the foe's party-wide hit (Van Saryu's
    /// Earthquake). See [`wanted_item`]'s backup heal.
    static AOE_HIT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Whether a foe has shown a charge latch this battle ([`foe_charged`]).
    static CHARGE_SEEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the coming round is the foe's heavy one, read off its cadence the
/// way a player does: a boss that has so far hit hard only every other round
/// (Rogue: Element Change, then a party-wide Wind / Thunder / Flame, from the
/// round counter's parity in its picker arm) and whose last round was the
/// quiet one is about to hit again. The last four rounds must read heavy,
/// quiet, heavy, quiet, with no two heavy rounds back to back anywhere: a foe
/// on a longer cycle (a lone Vahn's `nilboa` fight hits hard every third
/// round) is not this pattern, and guarding its quiet rounds only costs the
/// party its attacks.
fn big_round_due() -> bool {
    ROUND_HISTORY.with(|h| {
        let h = h.borrow();
        let peak = h.iter().copied().max().unwrap_or(0);
        if peak == 0 || h.len() < 4 {
            return false;
        }
        // A third of the peak, not half: a heavy round the party guarded
        // through lands halved and still has to read as heavy.
        let big = |x: u32| x * 3 >= peak;
        let back_to_back = h.windows(2).any(|w| big(w[0]) && big(w[1]));
        let last: Vec<bool> = h[h.len() - 4..].iter().map(|&x| big(x)).collect();
        !back_to_back && last == [true, false, true, false]
    })
}

/// Whether `actor` should take the Spirit stance this round: the foe's heavy
/// round is due ([`big_round_due`]), its hit is a large share of the
/// member's HP, and the guard's halving is what lets the member live through
/// it. Spirit is up from the commit for the whole round
/// (`World::battle.guarding`), so it covers a foe that acts first.
fn wants_guard(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    let Some(a) = w.actors.get(usize::from(actor)) else {
        return false;
    };
    // The HP the member will hold once the round's committed heals land.
    let Some(&hp) = projected_hp(w).get(usize::from(actor)) else {
        return false;
    };
    let max = u32::from(a.battle.max_hp);
    let threat = BIGGEST_HIT.with(std::cell::Cell::get);
    // A foe winding up shows it: a capture-class charge body (Xain's Bull
    // Charge, PROT 0953 arm 0) sets its caster's ability latch
    // (`0x801C8FE0 + (seat - 3 + 1) * 4`) and deals nothing, and the next
    // cast through the same body is the party-wide punch. A player who sees
    // the charge guards the turn it lands, before any heavy round is on
    // record.
    let charged = foe_charged(w);
    // The charge round itself is quiet; a latch the foe holds while it
    // keeps hitting (an AI ability cooldown such as ids `0x97` / `0x98`
    // re-arm every turn) is not a wind-up, and guarding through it starves
    // the fight (`nilboa`'s lone-Vahn F30).
    let quiet_last = ROUND_HISTORY.with(|h| h.borrow().last() == Some(&0));
    if hp > 0 && charged && quiet_last && u32::from(a.battle.hp) * 4 >= max {
        return true;
    }
    hp > 0 && threat * 5 >= max * 2 && hp > threat / 2 + threat / 8 && big_round_due()
}

/// Whether `actor` should take the Spirit stance against a party-wide hitter
/// (Van Saryu's Earthquake): in a fight that forbids running, a member whose
/// HP the foe's party-wide hit ([`AOE_HIT`]) would take to zero, but whom the
/// stance's halving lets live through it, guards rather than attacks. A
/// member below even the halved hit is the heal arm's; the guard is for the
/// band between, where it turns a death into a scratch whichever way the
/// round's turn order falls.
fn wants_aoe_guard(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    // A foe that telegraphs its party-wide hit (a charge latch) is guarded on
    // the tell ([`wants_guard`]); guarding on HP alone against it spends the
    // rounds between charges in the stance, and Xain outlasted the battle
    // cap that way with every member alive.
    if !w.battle.no_escape || CHARGE_SEEN.with(std::cell::Cell::get) {
        return false;
    }
    let aoe = AOE_HIT.with(std::cell::Cell::get);
    let Some(&hp) = projected_hp(w).get(usize::from(actor)) else {
        return false;
    };
    // Only against a hit that takes two fifths of the member or more (an
    // Earthquake is over half of a level-20 Vahn). Against lighter
    // party-wide hits the stance only costs the swings: Rogue (`rugi`
    // P2[38]) and the `chitei2` P2[13] fight both ran out the battle cap
    // with the party healthy.
    let max = u32::from(w.actors[usize::from(actor)].battle.max_hp);
    // Nor on the round after a heavy one from a foe that has never hit hard
    // twice running: Rogue's party-wide hit comes every other round, the
    // round after it is quiet, and guarding through it as well (every member
    // sits inside one unguarded hit) left nobody swinging - the cadence arm
    // ([`wants_guard`]) covers its heavy rounds.
    let quiet_next = ROUND_HISTORY.with(|h| {
        let h = h.borrow();
        let peak = h.iter().copied().max().unwrap_or(0);
        let big = |x: u32| peak > 0 && x * 3 >= peak;
        h.last().is_some_and(|&l| big(l)) && !h.windows(2).any(|p| big(p[0]) && big(p[1]))
    });
    // And never at nine tenths of full or above: a hit that drops even a
    // healthy member is the heal arm's to answer after it lands, and guarding
    // against it at full strength guards every round (`chitei2` P2[13]: all
    // three in the stance at ~1600/1700 HP until the battle cap).
    aoe > 0
        && !quiet_next
        && aoe * 5 >= max * 2
        && hp * 10 < max * 9
        && hp > aoe * 5 / 8
        && hp <= aoe + aoe / 8
}

/// [`wants_aoe_guard`], taken ahead of a heal: the member is the one in
/// danger and no ally is worse off (down, or below even the halved hit). The
/// stance is up from the commit, so it covers a foe that acts first; a heal
/// on itself lands only on the member's own turn, which the round's drawn
/// order may put behind the Earthquake that drops it.
fn guard_before_heal(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    if !wants_aoe_guard(w, actor) {
        return false;
    }
    let aoe = AOE_HIT.with(std::cell::Cell::get);
    projected_hp(w)
        .iter()
        .enumerate()
        .all(|(i, &h)| i == usize::from(actor) || h > aoe * 5 / 8)
}

/// Whether the coming round may bring Evil Seru Magic (`0xAD`), the
/// party-wide flurry and burst of monster `0xB4` (the `chitei2` P2[13]
/// fight): its pick arm (`FUN_801E9FD4`, ported as `monster_ai::decide`)
/// rolls the cast one time in three, but only once its Mystic Shield is
/// down (`_DAT_8007BD84` clear - the damage halve a player sees lift), on
/// the odd values of the per-round battle-mode counter, and while it holds
/// the 255 MP the cast spends. A lv30 party takes about 1900 a member from
/// it unguarded - more than any member holds - and about half that in the
/// Spirit stance, so a player who has watched the shield drop guards the
/// alternate rounds the cast can come on and acts on the others.
fn evil_magic_due(w: &legaia_engine_core::world::World) -> bool {
    const EVIL_MAGIC_CASTER: u16 = 0xB4;
    const EVIL_MAGIC_MP: u16 = 0xFF;
    let ai = &w.battle.monster_ai_state;
    if ai.flag_bd84 != 0 || ai.mode_flags & 1 == 0 {
        return false;
    }
    let n = w.party.party_count.clamp(1, 3) as usize;
    (n..w.actors.len()).any(|i| {
        let a = &w.actors[i];
        a.battle.hp > 0
            && a.battle_monster_id == Some(EVIL_MAGIC_CASTER)
            && a.battle.mp >= EVIL_MAGIC_MP
            && u32::from(a.battle.hp) <= u32::from(a.battle.max_hp) / 2
    })
}

/// [`evil_magic_due`] for one member: it guards when the stance lets it
/// live through the cast (about half of what it does unguarded), and is left
/// to the heal arm when even the guarded hit would drop it.
fn wants_evil_magic_guard(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    if !evil_magic_due(w) {
        return false;
    }
    let Some(a) = w.actors.get(usize::from(actor)) else {
        return false;
    };
    let hp = projected_hp(w)
        .get(usize::from(actor))
        .copied()
        .unwrap_or(0);
    hp > 0 && hp * 100 >= u32::from(a.battle.max_hp) * 55
}

/// Whether the foes' last round took half or more of the HP the party
/// still holds: healing cannot keep pace with that, and fleeing can.
fn losing_race(w: &legaia_engine_core::world::World) -> bool {
    let last = ROUND_HISTORY.with(|h| h.borrow().last().copied().unwrap_or(0));
    last > 0 && last * 2 >= party_hp_key(w)
}

/// Whether a living foe holds its ability latch - the wind-up of a charge
/// body (Xain's Bull Charge) before the party-wide hit it sets up. See
/// [`wants_guard`].
fn foe_charged(w: &legaia_engine_core::world::World) -> bool {
    let n = w.party.party_count.clamp(1, 3) as usize;
    (n..w.actors.len()).any(|i| {
        w.actors[i].battle.hp > 0
            && w.battle
                .monster_ai_state
                .dat
                .get(i - n + 4)
                .is_some_and(|&v| v != 0)
    })
}

/// A **solo duel**: one party member against a no-escape fight whose foe
/// outweighs it several times over (the Nivora duels - `nilboa`'s Che
/// Delilas against Gala, 12000 HP to 1446). Plain swings barely scratch such
/// a foe (the underdog floor) and its three real swings a round take half the
/// member's HP, so the fight is won the way a player wins it: Arts at a full
/// gauge, Spirit between them.
fn solo_duel(w: &legaia_engine_core::world::World) -> bool {
    let n = w.party.party_count.clamp(1, 3) as usize;
    if n != 1 || !w.battle.no_escape {
        return false;
    }
    let ours = u32::from(w.actors[0].battle.max_hp).max(1);
    (n..w.actors.len()).any(|i| {
        let a = &w.actors[i].battle;
        a.hp > 0 && u32::from(a.max_hp) >= ours * 4
    })
}

/// The duel's Spirit turn. The fighter alternates: a Spirit round, then an
/// Arts round on the gauge it extended - the stance triples the member's
/// guard for the foe's swings and halves its specials, so the rounds the foe
/// spends against it cost little, and a Delilas special (Che's Megaton
/// Press, Lu's Plasma Strike) lands on a guarded member far more often than
/// not. Below five eighths of its HP the member holds the stance round after
/// round until the heal arm ahead of this one has topped it up.
fn duel_wants_spirit(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    if !solo_duel(w) {
        return false;
    }
    let a = &w.actors[usize::from(actor)].battle;
    // Held round after round below five eighths, or while one of the foe's
    // heaviest rounds so far (a Delilas special lands most of a member's HP
    // at once) would drop the member unguarded.
    let threat = BIGGEST_HIT.with(std::cell::Cell::get);
    // A Delilas round varies by its swing count: Che's took 716 one round
    // and 903 two rounds later, so an Arts round waits while the member sits
    // within three twentieths again of the worst unguarded round seen, and
    // below five eighths of its HP (a 900-HP Gala at 62% swung into 903).
    // Half again starved the fight: Gala guarded and healed to the cap.
    let low = u32::from(a.hp) * 8 < u32::from(a.max_hp) * 5 || u32::from(a.hp) * 20 <= threat * 23;
    // The choice is latched per foe round: the fighter re-reads the menu on
    // every pad poll until the press lands, and a choice that flipped under
    // it would press something else.
    let round = ROUND_HISTORY.with(|h| h.borrow().len());
    match DUEL_SPIRIT_ROUND.with(std::cell::Cell::get) {
        Some(r) if r == round => return true,
        Some(r) if r + 1 == round && !low => return false,
        _ => {}
    }
    DUEL_SPIRIT_ROUND.with(|d| d.set(Some(round)));
    true
}

fn party_hp_key(w: &legaia_engine_core::world::World) -> u32 {
    let n = w.party.party_count.clamp(1, 3) as usize;
    (0..n).map(|i| u32::from(w.actors[i].battle.hp)).sum()
}

/// What one heal item restores on `slot`, given the HP it would have.
fn item_restore(w: &legaia_engine_core::world::World, id: u8, slot: usize, hp: u32) -> Option<u32> {
    use legaia_engine_core::items::ItemEffect;
    let max = u32::from(w.actors[slot].battle.max_hp);
    let e = w.tables.item_catalog.get(id)?;
    Some(match e.effect {
        ItemEffect::Revive { factor } if hp == 0 => (max * u32::from(factor) / 256).max(1),
        ItemEffect::HealAll if hp > 0 => max - hp.min(max),
        ItemEffect::Heal { amount } if hp > 0 => u32::from(amount).min(max - hp.min(max)),
        _ => return None,
    })
}

/// Each member's HP once the items the round's earlier members already
/// committed have landed - so two members never spend their turns on the
/// same wound.
fn projected_hp(w: &legaia_engine_core::world::World) -> Vec<u32> {
    use legaia_engine_core::battle_round::PendingPartyAction;
    let n = w.party.party_count.clamp(1, 3) as usize;
    let mut hp: Vec<u32> = (0..n).map(|i| u32::from(w.actors[i].battle.hp)).collect();
    for (actor, p) in w.battle.round_flow.pending.iter().enumerate().take(n) {
        let Some(PendingPartyAction::Item { item_id, .. }) = p else {
            continue;
        };
        let targets: Vec<usize> = if w.tables.item_catalog.is_all_party(*item_id) {
            (0..n).collect()
        } else {
            ITEM_TARGET
                .with(|t| t.borrow()[actor])
                .map(usize::from)
                .filter(|&t| t < n)
                .into_iter()
                .collect()
        };
        for t in targets {
            if let Some(r) = item_restore(w, *item_id, t, hp[t]) {
                hp[t] += r;
            }
        }
    }
    hp
}

/// The heal worth using now among `ids`, and the ally it is for: a revive
/// for a member who is down, else a party heal when two or more are in
/// danger, else the smallest single heal that lifts the worst-off member out
/// of danger (the largest when none does). "In danger" is under
/// [`HEAL_BELOW_PCT`] of max HP, or unable to take another hit the size of
/// the biggest one seen this battle; heals the round's earlier members
/// committed count as landed ([`projected_hp`]).
fn wanted_item(
    w: &legaia_engine_core::world::World,
    ids: impl Iterator<Item = u8>,
) -> Option<(u8, u8)> {
    let n = w.party.party_count.clamp(1, 3) as usize;
    let mut hp = projected_hp(w);
    // The backup heal. Turn order is drawn per round, so the member who
    // committed this round's heal may act after the foe. Against a
    // party-wide hitter that is the wipe: a member the next hit kills dies
    // before the heal lands (`jagaroom`'s Van Saryu, two Earthquakes of ~600
    // on consecutive rounds through a 1104-HP Vahn, the round's heal still
    // queued behind it). So while some member stands within one such hit of
    // death, one committed heal is not counted as landed and a second member
    // commits one too - whichever acts ahead of the foe saves the party, and
    // the other tops it up after the hit. A third member never does.
    let aoe = AOE_HIT.with(std::cell::Cell::get);
    let committed = w
        .battle
        .round_flow
        .pending
        .iter()
        .take(n)
        .filter(|p| {
            matches!(
                p,
                Some(legaia_engine_core::battle_round::PendingPartyAction::Item { .. })
            )
        })
        .count();
    let raw: Vec<u32> = (0..n).map(|i| u32::from(w.actors[i].battle.hp)).collect();
    // The backup never repeats the committed item when the bag holds only a
    // few: a second Healing Fruit behind the first spends one of two on what
    // the first already does whenever its owner acts ahead of the foe, and
    // the next Earthquake then finds the bag without one. An item the bag
    // holds by the dozen (a Healing Flower) is a fair backup. Nor does it
    // aim a second single heal or revive at the member the first one is for:
    // the backup is for someone else the hit would drop (Xain's single-target
    // swings drew two Flowers onto one member round after round, and the
    // fight ran out the pad budget on heals).
    let mut spent: Vec<u8> = Vec::new();
    let mut covered: Vec<usize> = Vec::new();
    if committed == 1 && aoe > 0 && raw.iter().any(|&h| h > 0 && h <= aoe + aoe / 8) {
        for (actor, p) in w.battle.round_flow.pending.iter().enumerate().take(n) {
            let Some(legaia_engine_core::battle_round::PendingPartyAction::Item {
                item_id, ..
            }) = p
            else {
                continue;
            };
            if w.party.inventory.get(item_id).copied().unwrap_or(0) < 10 {
                spent.push(*item_id);
            }
            if !w.tables.item_catalog.is_all_party(*item_id)
                && let Some(t) = ITEM_TARGET.with(|t| t.borrow()[actor])
            {
                covered.push(usize::from(t));
            }
        }
        hp = raw;
    }
    let threat = BIGGEST_HIT.with(std::cell::Cell::get);
    // The HP a member needs to hold to be out of danger.
    // A solo duel's member tops up below three fifths: its foe can act twice
    // across a round boundary, and two full rounds of Che's swings are most of
    // Gala's HP.
    let duel = solo_duel(w);
    let limit = |i: usize| {
        let max = u32::from(w.actors[i].battle.max_hp);
        let pct = max * heal_below_pct(w) / 100;
        // A party-wide hit's per-member share swings by a tenth or more
        // between casts (Van Saryu's Earthquake: 550, then 609 on a 690-HP
        // Gala), so the margin over it is a quarter.
        let hit = (threat + threat / 8).max(aoe + aoe / 4).min(max * 95 / 100);
        if duel {
            return pct.max(hit).max(max * 3 / 5);
        }
        pct.max(hit)
    };
    // A member a committed revive brings back is still down while the
    // window is open, and the target cursor refuses a heal on it (the
    // confirm bounces): it is not a heal target until it stands.
    let danger = |i: usize| {
        hp[i] > 0 && w.actors[i].battle.hp > 0 && hp[i] < limit(i) && !covered.contains(&i)
    };
    let dead: Vec<usize> = (0..n)
        .filter(|&i| hp[i] == 0 && !covered.contains(&i))
        .collect();
    let hurt: Vec<usize> = (0..n).filter(|&i| danger(i)).collect();
    if dead.is_empty() && hurt.is_empty() {
        return None;
    }
    let ids: Vec<u8> = ids
        .filter(|&id| {
            !spent.contains(&id)
                && w.tables
                    .item_catalog
                    .get(id)
                    .is_some_and(|e| e.usable_in_battle)
        })
        .collect();
    if let Some(&d) = dead.first() {
        let revive = ids
            .iter()
            .filter_map(|&id| Some((item_restore(w, id, d, 0)?, id)))
            .max();
        if let Some((_, id)) = revive {
            return Some((id, d as u8));
        }
    }
    if hurt.is_empty() {
        return None;
    }
    if hurt.len() >= 2 {
        let party = ids
            .iter()
            .filter(|&&id| w.tables.item_catalog.is_all_party(id))
            .filter_map(|&id| {
                let total: u32 = hurt
                    .iter()
                    .filter_map(|&i| item_restore(w, id, i, hp[i]))
                    .sum();
                (total > 0).then_some((total, id))
            })
            .max();
        // A party item only when it restores more than the best single
        // heal would on the worst-off member: three Healing Blooms' 200
        // apiece lose to one Healing Flower's 800, and spending the turn on
        // the smaller sum is how a party-wide hitter out-damages the heals.
        let worst = *hurt
            .iter()
            .min_by_key(|&&i| hp[i] * 1000 / u32::from(w.actors[i].battle.max_hp).max(1))?;
        let best_single = ids
            .iter()
            .filter(|&&id| !w.tables.item_catalog.is_all_party(id))
            .filter_map(|&id| item_restore(w, id, worst, hp[worst]))
            .max()
            .unwrap_or(0);
        if let Some((total, id)) = party
            && total > best_single
        {
            return Some((id, hurt[0] as u8));
        }
    }
    let worst = *hurt
        .iter()
        .min_by_key(|&&i| hp[i] * 1000 / u32::from(w.actors[i].battle.max_hp).max(1))?;
    let singles: Vec<(u32, u8)> = ids
        .iter()
        .filter(|&&id| !w.tables.item_catalog.is_all_party(id))
        .filter_map(|&id| Some((item_restore(w, id, worst, hp[worst])?, id)))
        .filter(|&(r, _)| r > 0)
        .collect();
    let enough = singles
        .iter()
        .filter(|&&(r, _)| hp[worst] + r >= limit(worst))
        .min();
    enough
        .or_else(|| singles.iter().max())
        .map(|&(_, id)| (id, worst as u8))
}

/// The MP the member's strongest damaging spell costs, if it knows one.
fn best_spell_cost(w: &legaia_engine_core::world::World, actor: u8) -> Option<u32> {
    use legaia_engine_core::spells::SpellEffect;
    let m = w
        .party
        .roster
        .members
        .get(w.party_roster_slot(usize::from(actor)))?;
    let list = m.spell_list();
    list.ids[..(list.count as usize).min(list.ids.len())]
        .iter()
        .filter_map(|&id| {
            let d = w.tables.spell_catalog.get(id)?;
            match d.effect {
                SpellEffect::Damage { base_power, .. } => Some((base_power, u32::from(d.mp_cost))),
                _ => None,
            }
        })
        .max()
        .map(|(_, cost)| cost)
}

/// The MP restorative worth drinking now, and who for: in a fight that
/// forbids running, a member who can no longer afford its strongest damage
/// spell drinks the largest MP item the bag offers. Against a boss whose
/// HP dwarfs the party's swings (Songi, 48000), a drink and a Gilium every
/// other turn outdamage two Arts strings.
fn wanted_mp_item(
    w: &legaia_engine_core::world::World,
    ids: impl Iterator<Item = u8>,
    actor: u8,
) -> Option<(u8, u8)> {
    use legaia_engine_core::items::ItemEffect;
    if !w.battle.no_escape || solo_duel(w) {
        return None;
    }
    // Only against a foe of twenty times the party's largest HP or more
    // (Songi, 48000 to ~1950): in a shorter fight the drinks cost turns the
    // Arts and heals win it with (Van Saryu at 11x lost two seeds in ten).
    let n = w.party.party_count.clamp(1, 3) as usize;
    let ours = (0..n)
        .map(|i| u32::from(w.actors[i].battle.max_hp))
        .max()
        .unwrap_or(1)
        .max(1);
    if !(n..w.actors.len()).any(|i| {
        let a = &w.actors[i].battle;
        a.hp > 0 && u32::from(a.max_hp) >= ours * 20
    }) {
        return None;
    }
    let cost = best_spell_cost(w, actor)?;
    let mp = u32::from(w.actors.get(usize::from(actor))?.battle.mp);
    if mp >= cost {
        return None;
    }
    ids.filter_map(|id| {
        let e = w.tables.item_catalog.get(id)?;
        match e.effect {
            ItemEffect::HealMp { amount } if e.usable_in_battle => Some((amount, id)),
            _ => None,
        }
    })
    // The smallest drink that pays for the spell, else the largest.
    .filter(|&(a, _)| mp + u32::from(a) >= cost)
    .min()
    .map(|(_, id)| (id, actor))
}

/// The Fury Boost worth using now, and on whom (the acting member itself):
/// in a fight that forbids running against a foe of ten times the party's
/// largest HP or more, a member whose Miracle Art string costs more than its
/// command pool, but not more than the pool the boost buys. The item raises
/// the actor's `+0x1F9` charge byte, and every round boundary after it
/// restores the pool `+0x154` to `base * 7 / 5 + 8` (cap `0x120`) instead of
/// the base (`FUN_801D88CC` loop A, `World::apply_fury_boost_item`), so one
/// turn spent buys a Miracle Art every round after. Koru's timed fight
/// (`0xB6`: 20000 HP, the all-party finisher on round counter `4`) is lost on
/// spells and short Arts strings alone.
fn wanted_fury_item(
    w: &legaia_engine_core::world::World,
    ids: impl Iterator<Item = u8>,
    actor: u8,
) -> Option<(u8, u8)> {
    use legaia_engine_core::items::ItemEffect;
    if !w.battle.no_escape || solo_duel(w) {
        return None;
    }
    let n = w.party.party_count.clamp(1, 3) as usize;
    let i = usize::from(actor);
    if i >= n || w.battle.fury_boost.get(i).copied().flatten().is_some() {
        return None;
    }
    let ours = (0..n)
        .map(|k| u32::from(w.actors[k].battle.max_hp))
        .max()
        .unwrap_or(1)
        .max(1);
    if !(n..w.actors.len()).any(|k| {
        let a = &w.actors[k].battle;
        a.hp > 0 && u32::from(a.max_hp) >= ours * 10
    }) {
        return None;
    }
    if !miracle_armed(w, actor) {
        return None;
    }
    let roster = w.party_roster_slot(i);
    let character = legaia_engine_core::battle_arts::character_for_slot(roster as u8);
    let miracle = legaia_art::MIRACLE_ARTS
        .iter()
        .find(|m| m.character == character)?;
    // Every direction costs the same outside the weapon arm (`+0x74`), and the
    // Miracle string is typed with whatever the arm costs: price it at the
    // dearest press, as the entry will.
    let press = w
        .battle
        .swing_costs
        .get(roster)
        .copied()
        .unwrap_or([legaia_engine_core::arts_command_input::FAVORED_COST; 4])
        .into_iter()
        .max()
        .unwrap_or(u16::MAX);
    let cost = u32::from(press) * miracle.commands.len() as u32;
    let base = u32::from(w.actors[i].battle.agl_base);
    let boosted = (base * 7 / 5 + 8).min(0x120);
    if cost <= base || cost > boosted {
        return None;
    }
    let mut ids = ids;
    ids.find(|&id| {
        w.tables
            .item_catalog
            .get(id)
            .is_some_and(|e| e.usable_in_battle && matches!(e.effect, ItemEffect::ActionGauge))
    })
    .map(|id| (id, actor))
}

/// The attack Elixir worth handing out now, and to whom: in the same fights
/// as [`wanted_fury_item`], a member that will not type a Miracle Art itself
/// gives one to a member that does (Fury-boosted or already paid for) and
/// carries no Attack buff yet. The class-7 Elixir ramps the target's attack
/// scalar `x6/5` for the rest of the battle, and a Miracle Art is one strike
/// against Koru's defence, so the ramp buys more than the summon the giver
/// gives up.
fn wanted_power_item(
    w: &legaia_engine_core::world::World,
    ids: impl Iterator<Item = u8>,
    actor: u8,
) -> Option<(u8, u8)> {
    use legaia_asset::item_effect::{StatItemEffect, StatTarget};
    use legaia_engine_core::spells::BuffStat;
    if !w.battle.no_escape || solo_duel(w) {
        return None;
    }
    let n = w.party.party_count.clamp(1, 3) as usize;
    if usize::from(actor) >= n || miracle_ready(w, actor) || miracle_armed_fury(w, actor) {
        return None;
    }
    let ours = (0..n)
        .map(|k| u32::from(w.actors[k].battle.max_hp))
        .max()
        .unwrap_or(1)
        .max(1);
    if !(n..w.actors.len()).any(|k| {
        let a = &w.actors[k].battle;
        a.hp > 0 && u32::from(a.max_hp) >= ours * 10
    }) {
        return None;
    }
    let target = (0..n as u8).find(|&m| {
        m != actor
            && w.actors[usize::from(m)].battle.hp > 0
            && (w
                .battle
                .fury_boost
                .get(usize::from(m))
                .copied()
                .flatten()
                .is_some()
                || miracle_ready(w, m))
            && !w
                .battle
                .buffs
                .iter()
                .any(|b| b.slot == m && b.stat == BuffStat::Attack)
    })?;
    let table = w.tables.item_effects.as_ref()?;
    ids.filter(|&id| {
        w.tables
            .item_catalog
            .get(id)
            .is_some_and(|e| e.usable_in_battle)
            && matches!(table.stat_effect(id),
                Some(StatItemEffect::BuffOneBattle(stats)) if stats.contains(&StatTarget::Attack))
    })
    // The single-stat Elixir before the four-stat one.
    .min_by_key(|&id| match table.stat_effect(id) {
        Some(StatItemEffect::BuffOneBattle(stats)) => stats.len(),
        _ => usize::MAX,
    })
    .map(|id| (id, target))
}

/// Whether `actor` is (or is about to be) a Miracle typist through a Fury
/// Boost: boosted already, or [`wanted_fury_item`] would buy one.
fn miracle_armed_fury(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    w.battle
        .fury_boost
        .get(usize::from(actor))
        .copied()
        .flatten()
        .is_some()
        && miracle_armed(w, actor)
        || wanted_fury_item(
            w,
            w.party
                .inventory
                .iter()
                .filter(|(_, c)| **c > 0)
                .map(|(id, _)| *id),
            actor,
        )
        .is_some()
}

/// Whether `actor` can type its Miracle Art this turn: the marker is armed
/// and the live command pool (`+0x154`, what the entry seeds from) pays for
/// the whole string at the dearest press.
fn miracle_ready(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    let i = usize::from(actor);
    if i >= w.party.party_count.clamp(1, 3) as usize || !miracle_armed(w, actor) {
        return false;
    }
    let roster = w.party_roster_slot(i);
    let character = legaia_engine_core::battle_arts::character_for_slot(roster as u8);
    let Some(miracle) = legaia_art::MIRACLE_ARTS
        .iter()
        .find(|m| m.character == character)
    else {
        return false;
    };
    let press = w
        .battle
        .swing_costs
        .get(roster)
        .copied()
        .unwrap_or([legaia_engine_core::arts_command_input::FAVORED_COST; 4])
        .into_iter()
        .max()
        .unwrap_or(u16::MAX);
    u32::from(press) * miracle.commands.len() as u32 <= u32::from(w.actors[i].battle.agl)
}

/// Whether `actor`'s Miracle marker is armed (a Ra-Seru equipped): the queue
/// builder replaces a typed Miracle string with the Miracle row only then.
fn miracle_armed(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    let roster = w.party_roster_slot(usize::from(actor));
    w.party.roster.members.get(roster).is_some_and(|m| {
        legaia_engine_vm::battle_action::miracle_marker_armed(
            (roster as u8).wrapping_add(1),
            &m.equipment().slots,
        )
    })
}

/// The Magic row worth casting: the strongest affordable damage spell.
fn wanted_spell(
    w: &legaia_engine_core::world::World,
    m: &legaia_engine_core::battle_magic::BattleSpellSession,
) -> Option<usize> {
    use legaia_engine_core::spells::SpellEffect;
    m.spells
        .iter()
        .enumerate()
        .filter(|(_, r)| r.affordable)
        .filter_map(|(i, r)| match w.tables.spell_catalog.get(r.id)?.effect {
            SpellEffect::Damage { base_power, .. } => {
                Some((i, u32::from(base_power) * spell_affinity_pct(w, r.id)))
            }
            _ => None,
        })
        .max_by_key(|&(_, p)| p)
        .map(|(i, _)| i)
}

/// The element-affinity percent a Seru-magic cast lands with on the first
/// standing foe: `matrix[summon creature element][foe element]`
/// (`FUN_801dd864`), the attacker element being the summoned creature's
/// record `+0x1D`, not the caster's. `100` when either element is unknown.
fn spell_affinity_pct(w: &legaia_engine_core::world::World, spell_id: u8) -> u32 {
    let n = w.party.party_count.clamp(1, 3) as usize;
    let Some(aff) = w.tables.element_affinity.as_ref() else {
        return 100;
    };
    let Some(atk) = w.tables.summon_creatures.get(&spell_id).map(|d| d.element) else {
        return 100;
    };
    let Some(def) = w
        .actors
        .iter()
        .skip(n)
        .find(|a| a.battle.hp > 0)
        .and_then(|a| {
            a.battle_element.or_else(|| {
                w.tables
                    .monster_catalog
                    .get(a.battle_monster_id?)
                    .map(|d| d.element)
            })
        })
    else {
        return 100;
    };
    u32::from(aff.affinity_pct(atk, def).unwrap_or(100))
}

thread_local! {
    /// Each character's Super Art physical inputs ([`arts_plan`]), derived
    /// once per process.
    static SUPER_INPUTS: std::cell::RefCell<
        HashMap<legaia_art::Character, Vec<Vec<legaia_art::Command>>>,
    > = Default::default();
}

/// The arts string worth entering, the way the queue builder will read it
/// (`FUN_801EED1C`, `World::build_arts_action_queue`):
///
/// - the character's **Miracle Art** string, when the slot's Miracle marker
///   is armed (a Ra-Seru equipped) and the command pool pays for all of it -
///   the builder replaces the whole queue with the Miracle row;
/// - else a **Super Art**'s physical input ([`legaia_art::derive_super_input`]
///   over the trigger table's `find` pattern), the longest the pool pays for;
/// - else the longest **normal** arts (constants `0x1F` up, two arrows or
///   more) the pool pays for, repeated while they fit, then the cheapest
///   plain direction until nothing more is affordable.
///
/// The Miracle and the three Hyper arts (constants `0x1B..=0x1E`) never
/// tokenize as arts on their own strings - the builder only reaches them
/// through the Miracle copy and the Super tail-replace - so typing a Hyper
/// art's string swings plain arrows. The fighter did exactly that for Vahn
/// (Fiery Miyawaki, `2 3 1 3 1`) and Noa (Illusion Kick), whose "arts" never
/// spent the gauge.
fn arts_plan(
    w: &legaia_engine_core::world::World,
    s: &legaia_engine_core::arts_command_input::ArtsCommandInputSession,
) -> Vec<u8> {
    use legaia_art::Command;
    // Nine: each character's Miracle Art is a nine-command string
    // (`art-data.md`), and a cap below it kept the hand from ever entering
    // one. The pool still decides what fits; this only stops the plan.
    const MAX_ENTRY: usize = 9;
    const NORMAL_ART_MIN_CONSTANT: u8 = 0x1F;
    // The occupying character's table: the actor's `character` key is only
    // written by the first arts commit, so before it every member reads
    // Vahn's.
    let roster = w.party_roster_slot(usize::from(s.actor));
    let character = legaia_engine_core::battle_arts::character_for_slot(roster as u8);
    let cost = |cmds: &[Command]| -> u16 { cmds.iter().map(|&c| s.cost_of(c)).sum() };
    let mut normal: Vec<(legaia_art::ActionConstant, &[Command])> = w
        .tables
        .art_records
        .iter()
        .filter(|((c, a), r)| {
            *c == character && r.commands.len() >= 2 && a.as_byte() >= NORMAL_ART_MIN_CONSTANT
        })
        .map(|((_, a), r)| (*a, r.commands.as_slice()))
        .collect();
    normal.sort_by_key(|(a, _)| a.as_byte());
    let bytes = |cmds: &[Command]| cmds.iter().map(|c| c.as_byte()).collect::<Vec<u8>>();
    if miracle_armed(w, s.actor)
        && let Some(m) = legaia_art::MIRACLE_ARTS
            .iter()
            .find(|m| m.character == character)
        && cost(m.commands) <= s.pool_max
        && m.commands.len() <= MAX_ENTRY
    {
        return bytes(m.commands);
    }
    // The derivation searches up to `4^12` strings, and the hand re-plans on
    // every pad poll of the entry: derive each character's Super inputs once.
    let supers = SUPER_INPUTS.with(|c| {
        c.borrow_mut()
            .entry(character)
            .or_insert_with(|| {
                legaia_art::SUPER_ARTS
                    .iter()
                    .filter(|sa| sa.character == character)
                    .filter_map(|sa| legaia_art::derive_super_input(&normal, sa.find))
                    .collect()
            })
            .clone()
    });
    if let Some(input) = supers
        .into_iter()
        .filter(|inp| inp.len() <= MAX_ENTRY && cost(inp) <= s.pool_max)
        .max_by_key(|inp| inp.len())
    {
        return bytes(&input);
    }
    let arts: Vec<&[Command]> = normal.iter().map(|(_, c)| *c).collect();
    let mut plan: Vec<Command> = Vec::new();
    let mut pool = s.pool_max;
    loop {
        let art = arts
            .iter()
            .filter(|c| cost(c) <= pool && plan.len() + c.len() <= MAX_ENTRY)
            .max_by_key(|c| {
                (
                    c.len(),
                    std::cmp::Reverse(c.iter().map(|x| x.as_byte()).collect::<Vec<u8>>()),
                )
            });
        if let Some(c) = art {
            pool -= cost(c);
            plan.extend_from_slice(c);
            continue;
        }
        let Some(&d) = [Command::Left, Command::Right, Command::Down, Command::Up]
            .iter()
            .filter(|&&d| s.cost_of(d) <= pool)
            .min_by_key(|&&d| s.cost_of(d))
        else {
            break;
        };
        if plan.len() >= MAX_ENTRY {
            break;
        }
        pool -= s.cost_of(d);
        plan.push(d);
    }
    plan.into_iter().map(|c| c.as_byte()).collect()
}

/// The ring press for one arts-entry direction byte.
fn direction_mask(b: u8) -> u16 {
    use legaia_art::queue::Command;
    match Command::from_byte(b) {
        Some(Command::Left) => PadButton::Left.mask(),
        Some(Command::Right) => PadButton::Right.mask(),
        Some(Command::Down) => PadButton::Down.mask(),
        _ => PadButton::Up.mask(),
    }
}

/// The pad mask a player presses this frame in a battle - every one a pad
/// press through the engine's own battle menus, no engine call:
///
/// - a member who is down, or in danger, gets the revive or heal
///   [`wanted_item`] picks, aimed at that member (the ring's up arm);
/// - otherwise a member with an affordable damaging Seru spell casts the
///   strongest one (the right arm);
/// - otherwise it attacks through the `Command` chip, entering the longest
///   art its command pool pays for ([`arts_plan`]);
/// - a random encounter on a pad travel leg is fled (the round prompt's
///   Run) unless the fight forbids running;
/// - message boxes and results screens are paged with Cross.
///
/// A sparring tutorial validates each commit against its lesson, so there
/// the hand keeps to the lesson: the up arm (using the first item) for the
/// Items lesson, the down arm for Spirit, and `Auto` for the attack lessons.
fn fight_pad(session: &BootSession) -> u16 {
    use legaia_engine_core::arts_command_input::ArtsInputPhase;
    use legaia_engine_core::battle_input::CommandPhase;
    use legaia_engine_core::battle_magic::SpellPhase;
    use legaia_engine_core::battle_tutorial::TutorialLesson;
    use legaia_engine_core::inventory_use::InventoryUseState;
    let w = &session.host.world;
    if !w.battle.tutorial_boxes.is_empty() {
        return PadButton::Cross.mask();
    }
    let lesson = w.battle.tutorial.as_ref().map(|t| t.lesson());
    if let Some(menu) = w.battle.item_menu.as_ref() {
        if lesson == Some(TutorialLesson::Items) {
            return match &menu.state {
                InventoryUseState::Browsing { .. } if !menu.filtered_items.is_empty() => {
                    PadButton::Cross.mask()
                }
                InventoryUseState::TargetSelect { .. } => PadButton::Cross.mask(),
                _ => PadButton::Circle.mask(),
            };
        }
        let listed = |i: usize| {
            menu.filtered_items
                .get(i)
                .and_then(|&k| menu.items.get(k))
                .copied()
        };
        let actor = w
            .battle
            .command
            .as_ref()
            .map_or(w.battle_ctx.active_actor, |c| c.actor);
        let want = wanted_item(w, (0..menu.filtered_items.len()).filter_map(listed))
            .or_else(|| wanted_mp_item(w, (0..menu.filtered_items.len()).filter_map(listed), actor))
            .or_else(|| {
                wanted_fury_item(w, (0..menu.filtered_items.len()).filter_map(listed), actor)
            })
            .or_else(|| {
                wanted_power_item(w, (0..menu.filtered_items.len()).filter_map(listed), actor)
            });
        return match &menu.state {
            InventoryUseState::Browsing { cursor } => match want {
                Some((id, _)) if listed(*cursor) == Some(id) => PadButton::Cross.mask(),
                Some(_) => PadButton::Down.mask(),
                None => {
                    let key = (actor, party_hp_key(w));
                    NO_ITEM.with(|n| n.borrow_mut().insert(key));
                    PadButton::Circle.mask()
                }
            },
            // The session seeds the cursor on the first ally the item helps;
            // the hand steps it to the one it chose, and remembers the aim so
            // the round's later members count the heal as coming.
            InventoryUseState::TargetSelect { cursor, .. } => {
                let aim = want.map(|(_, t)| t);
                let on = menu.targets.get(*cursor).map(|t| t.slot);
                if aim.is_none() || on == aim {
                    if let Some(slot) = on {
                        ITEM_TARGET.with(|t| {
                            if let Some(a) = t.borrow_mut().get_mut(usize::from(actor)) {
                                *a = Some(slot);
                            }
                        });
                    }
                    PadButton::Cross.mask()
                } else {
                    PadButton::Down.mask()
                }
            }
            _ => PadButton::Circle.mask(),
        };
    }
    if let Some(m) = w.battle.spell_menu.as_ref() {
        return match &m.phase {
            SpellPhase::Select { cursor } => match wanted_spell(w, m) {
                Some(i) if i == usize::from(*cursor) => PadButton::Cross.mask(),
                Some(_) => PadButton::Down.mask(),
                None => {
                    NO_MAGIC.with(|n| n.borrow_mut().insert(m.actor));
                    PadButton::Circle.mask()
                }
            },
            _ => PadButton::Cross.mask(),
        };
    }
    if let Some(arts) = w.battle.arts_input.as_ref() {
        return match &arts.phase {
            ArtsInputPhase::Entering => {
                let plan = arts_plan(w, arts);
                if arts.buffer.len() < plan.len() && plan.starts_with(&arts.buffer) {
                    direction_mask(plan[arts.buffer.len()])
                } else {
                    PadButton::Cross.mask()
                }
            }
            _ => PadButton::Cross.mask(),
        };
    }
    if let Some(cmd) = w.battle.command.as_ref() {
        let heal = || {
            let bag: Vec<u8> = w
                .party
                .inventory
                .iter()
                .filter(|(_, c)| **c > 0)
                .map(|(id, _)| *id)
                .collect();
            wanted_item(w, bag.into_iter()).is_some()
                && !NO_ITEM.with(|n| n.borrow().contains(&(cmd.actor, party_hp_key(w))))
        };
        // A Miracle Art the live pool pays for outdamages any summon the
        // hand can cast (Koru: ~2000 against Aluru's ~1300).
        let magic =
            || !NO_MAGIC.with(|n| n.borrow().contains(&cmd.actor)) && !miracle_ready(w, cmd.actor);
        let drink = || {
            let bag: Vec<u8> = w
                .party
                .inventory
                .iter()
                .filter(|(_, c)| **c > 0)
                .map(|(id, _)| *id)
                .collect();
            wanted_mp_item(w, bag.into_iter(), cmd.actor).is_some()
                && !NO_ITEM.with(|n| n.borrow().contains(&(cmd.actor, party_hp_key(w))))
        };
        let fury = || {
            let bag: Vec<u8> = w
                .party
                .inventory
                .iter()
                .filter(|(_, c)| **c > 0)
                .map(|(id, _)| *id)
                .collect();
            (wanted_fury_item(w, bag.clone().into_iter(), cmd.actor).is_some()
                || wanted_power_item(w, bag.into_iter(), cmd.actor).is_some())
                && !NO_ITEM.with(|n| n.borrow().contains(&(cmd.actor, party_hp_key(w))))
        };
        return match &cmd.phase {
            CommandPhase::Menu { .. } if lesson == Some(TutorialLesson::Items) => {
                PadButton::Up.mask()
            }
            CommandPhase::Menu { .. } if lesson == Some(TutorialLesson::Spirit) => {
                PadButton::Down.mask()
            }
            // The foe's heavy round is due: the Spirit stance halves it.
            CommandPhase::Menu { .. } if lesson.is_none() && wants_guard(w, cmd.actor) => {
                PadButton::Down.mask()
            }
            CommandPhase::Menu { .. } if lesson.is_none() && guard_before_heal(w, cmd.actor) => {
                PadButton::Down.mask()
            }
            // Evil Seru Magic can come this round: the stance.
            CommandPhase::Menu { .. }
                if lesson.is_none() && wants_evil_magic_guard(w, cmd.actor) =>
            {
                PadButton::Down.mask()
            }
            CommandPhase::Menu { .. } if lesson.is_none() && heal() => PadButton::Up.mask(),
            // Out of MP for its best spell in a boss fight: a drink.
            CommandPhase::Menu { .. } if lesson.is_none() && drink() => PadButton::Up.mask(),
            // A boss too big for spells and short strings: the turn that buys
            // a Miracle Art every round after.
            CommandPhase::Menu { .. } if lesson.is_none() && fury() => PadButton::Up.mask(),
            // One party-wide hit from death with nothing to heal: the stance.
            CommandPhase::Menu { .. } if lesson.is_none() && wants_aoe_guard(w, cmd.actor) => {
                PadButton::Down.mask()
            }
            // A solo duel builds its gauge between Arts turns.
            CommandPhase::Menu { .. } if lesson.is_none() && duel_wants_spirit(w, cmd.actor) => {
                PadButton::Down.mask()
            }
            // A duel's damage is its Arts; a summon spends the turn and the
            // MP for less.
            CommandPhase::Menu { .. } if lesson.is_none() && magic() && !solo_duel(w) => {
                PadButton::Right.mask()
            }
            CommandPhase::AttackMode { .. } if lesson.is_none() => PadButton::Right.mask(),
            // A random encounter on a pad travel leg is fled: the prompt's
            // Right takes Run. A lone member worn down by a string of fights
            // wipes on whichever one the RNG happens to deal - a finding
            // about the route, not the port. A fight that forbids running
            // (`no_escape`) is fought. A member low enough to want a heal
            // takes it first: monster swings play their own clips and roll
            // their own power bytes, so a failed run under a full enemy
            // round can now take a worn member from half HP to zero. But a
            // heal does not win a race the foes are winning: once the last
            // round took half of what the party still holds, the turn spent
            // healing is a turn under another such round, and the run is
            // the better draw (`vozz`'s three-monster F4 after a caught Run:
            // a Healing Leaf a round into ~250 a round, two members down).
            CommandPhase::RoundPrompt { .. }
                if FLEE_ENCOUNTERS.with(std::cell::Cell::get)
                    && !w.battle.no_escape
                    && (!heal() || losing_race(w)) =>
            {
                PadButton::Right.mask()
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
    // The engine seats monsters right behind the party, not at retail's
    // fixed seat 3 - a lone fighter's opponent sits in slot 1.
    let mobs: Vec<String> = (n..w.actors.len())
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

/// The party a traced battle starts with: each member's level, MP, equipment
/// and spell list, and the bag's battle-usable items.
fn party_dump(session: &BootSession) {
    let w = &session.host.world;
    let n = w.party.party_count.clamp(1, 3) as usize;
    for i in 0..n {
        let slot = w.party_roster_slot(i);
        let Some(m) = w.party.roster.members.get(slot) else {
            continue;
        };
        let list = m.spell_list();
        let spells: Vec<String> = list.ids[..(list.count as usize).min(list.ids.len())]
            .iter()
            .map(|&id| match w.tables.spell_catalog.get(id) {
                Some(d) => format!("{id:#x} {} mp{} {:?}", d.name, d.mp_cost, d.effect),
                None => format!("{id:#x} ?"),
            })
            .collect();
        let a = &w.actors[i].battle;
        eprintln!(
            "    [party] #{i} char {:?} lv {} hp {}/{} mp {} equip {:?} spells [{}]",
            a.character,
            m.level(),
            a.hp,
            a.max_hp,
            a.mp,
            m.equipment(),
            spells.join("; ")
        );
    }
    for i in 0..n {
        let ch = legaia_engine_core::battle_arts::character_for_slot(w.party_roster_slot(i) as u8);
        let mut arts: Vec<String> = w
            .tables
            .art_records
            .iter()
            .filter(|((c, _), _)| *c == ch)
            .map(|((_, a), r)| {
                let cmds: Vec<u8> = r.commands.iter().map(|c| c.as_byte()).collect();
                format!(
                    "{:#x} {:?} {cmds:?} pw{}",
                    a.as_byte(),
                    r.name,
                    r.power.len()
                )
            })
            .collect();
        arts.sort();
        eprintln!("    [arts] #{i} {ch:?}: {}", arts.join("; "));
    }
    let bag: Vec<String> = w
        .party
        .inventory
        .iter()
        .filter(|(_, c)| **c > 0)
        .filter_map(|(&id, &c)| {
            let e = w.tables.item_catalog.get(id)?;
            e.usable_in_battle
                .then(|| format!("{id:#x} {} x{c} {:?}", e.name, e.effect))
        })
        .collect();
    eprintln!("    [party] bag: {}", bag.join("; "));
}

/// Fight a battle with the pad ([`fight_pad`]). `None` when it ended and a
/// walking mode came back.
fn drain_battle(session: &mut BootSession) -> Option<Run> {
    // A fight's frames come off the pad segment's budget only past the
    // battle allowance: the deadline moves ahead of the fight by the most
    // it may need, and the unused part is taken back.
    let lent = if pad_hand() {
        PAD_BATTLE_LEFT
            .with(std::cell::Cell::get)
            .min(BATTLE_TICKS_CAP as u64)
    } else {
        0
    };
    PAD_DEADLINE.with(|d| d.set(d.get().saturating_add(lent)));
    let start = session.frames;
    let r = fight_battle(session);
    let used = session.frames.saturating_sub(start).min(lent);
    PAD_DEADLINE.with(|d| d.set(d.get().saturating_sub(lent - used)));
    PAD_BATTLE_LEFT.with(|l| l.set(l.get().saturating_sub(used)));
    r
}

fn fight_battle(session: &mut BootSession) -> Option<Run> {
    NO_ITEM.with(|n| n.borrow_mut().clear());
    NO_MAGIC.with(|n| n.borrow_mut().clear());
    DUEL_SPIRIT_ROUND.with(|d| d.set(None));
    BIGGEST_HIT.with(|b| b.set(0));
    ROUND_LOSS.with(|r| r.set([0; 3]));
    ROUND_HISTORY.with(|h| h.borrow_mut().clear());
    AOE_HIT.with(|a| a.set(0));
    CHARGE_SEEN.with(|c| c.set(false));
    let mut guarded = [false; 3];
    let mut was_window = false;
    ITEM_TARGET.with(|t| *t.borrow_mut() = [None; 3]);
    let party_hp = |s: &BootSession| -> Vec<u16> {
        let w = &s.host.world;
        let n = w.party.party_count.clamp(1, 3) as usize;
        (0..n).map(|i| w.actors[i].battle.hp).collect()
    };
    let mut party_prev = party_hp(session);
    let mut prev = 0u16;
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    if trace {
        eprintln!("    [battle] start: {}", battle_snapshot(session));
        party_dump(session);
    }
    let trace_hits = trace && std::env::var_os("LEGAIA_FGL_TRACE_HITS").is_some();
    let hp_all =
        |s: &BootSession| -> Vec<u16> { s.host.world.actors.iter().map(|a| a.battle.hp).collect() };
    let mut last_cmd: [String; 3] = Default::default();
    let mut hp_prev = hp_all(session);
    let foe_hp = |s: &BootSession| -> u32 {
        let w = &s.host.world;
        let n = w.party.party_count.clamp(1, 3) as usize;
        w.actors
            .iter()
            .skip(n)
            .map(|a| u32::from(a.battle.hp))
            .sum()
    };
    let mut foe_prev = foe_hp(session);
    let mut foe_dropped_at = 0usize;
    let mut spent = 0usize;
    for t in 0..BATTLE_TICKS_CAP {
        spent = t;
        if t >= BATTLE_TICKS && t - foe_dropped_at > BATTLE_STALL_TICKS {
            break;
        }
        if pad_budget(session).is_err() {
            break;
        }
        // A press is an edge: alternate the wanted mask with neutral.
        let want = fight_pad(session);
        let pad = if prev == 0 { want } else { 0 };
        prev = pad;
        session.host.world.set_pad(pad);
        if let Err(e) = session.tick() {
            return Some(Run::Error(format!("{e:#}")));
        }
        if foe_charged(&session.host.world) {
            CHARGE_SEEN.with(|c| c.set(true));
        }
        let foe_now = foe_hp(session);
        if foe_now < foe_prev {
            foe_dropped_at = t;
        }
        foe_prev = foe_now;
        let party_now = party_hp(session);
        // A command window opening ends the stretch the foes had.
        let window = session.host.world.battle.command.as_ref().is_some_and(|c| {
            matches!(
                c.phase,
                legaia_engine_core::battle_input::CommandPhase::RoundPrompt { .. }
            )
        });
        let mut run = ROUND_LOSS.with(std::cell::Cell::get);
        for (i, p) in session
            .host
            .world
            .battle
            .round_flow
            .pending
            .iter()
            .enumerate()
            .take(3)
        {
            if matches!(
                p,
                Some(legaia_engine_core::battle_round::PendingPartyAction::Spirit)
            ) {
                guarded[i] = true;
            }
        }
        if window && !was_window {
            ROUND_HISTORY.with(|h| h.borrow_mut().push(run.iter().sum()));
            if run.iter().filter(|&&l| l > 0).count() >= 2 {
                // A member that took the hit in the Spirit stance took half
                // of it: the hit itself is twice what it lost (Jette's Evil
                // Seru Magic, 612 a guarded member, 1224 the next round
                // unguarded - read as 612, the heal limit let the party sit
                // at 1130 into a hit that dropped all three).
                let peak = run
                    .iter()
                    .zip(guarded)
                    .map(|(&l, g)| if g { l * 2 } else { l })
                    .max()
                    .unwrap_or(0);
                AOE_HIT.with(|a| a.set(a.get().max(peak)));
            }
            if trace_hits {
                eprintln!(
                    "    [round] t={t} history {:?} due {} aoe {} hp {:?} mode {} foe mp {:?}",
                    ROUND_HISTORY.with(|h| h.borrow().clone()),
                    big_round_due(),
                    AOE_HIT.with(std::cell::Cell::get),
                    party_hp(session),
                    session.host.world.battle.monster_ai_state.mode_flags,
                    session
                        .host
                        .world
                        .actors
                        .iter()
                        .skip(session.host.world.party.party_count.clamp(1, 3) as usize)
                        .map(|a| a.battle.mp)
                        .filter(|&m| m > 0)
                        .collect::<Vec<_>>(),
                );
                last_cmd = Default::default();
            }
        }
        was_window = window;
        if window {
            run = [0; 3];
            guarded = [false; 3];
        }
        for (i, (a, b)) in party_prev.iter().zip(&party_now).enumerate().take(3) {
            run[i] += u32::from(a.saturating_sub(*b));
            BIGGEST_HIT.with(|h| h.set(h.get().max(run[i])));
        }
        ROUND_LOSS.with(|r| r.set(run));
        party_prev = party_now;
        if trace_hits {
            let w = &session.host.world;
            for (i, p) in w.battle.round_flow.pending.iter().enumerate() {
                if let Some(p) = p {
                    let s: String = format!("{p:?}").chars().take(70).collect();
                    if last_cmd[i] != s {
                        eprintln!("    [commit] t={t} actor {i}: {s}");
                    }
                    last_cmd[i] = s;
                }
            }
            let hp_now = hp_all(session);
            if hp_now != hp_prev {
                let who = w.battle_ctx.active_actor;
                let what = last_cmd.get(usize::from(who)).cloned().unwrap_or_default();
                let n = w.party.party_count.clamp(1, 3) as usize;
                let gauges: Vec<u16> = (0..n).map(|i| w.actors[i].battle.spirit_gauge).collect();
                eprintln!(
                    "    [hit] t={t} actor {who} ({what}) sm 0x{:02X} hp {:?} -> {:?} mob mp {} mode {} shield {} gauges {gauges:?}",
                    w.battle_ctx.action_state,
                    &hp_prev[..n + 1],
                    &hp_now[..n + 1],
                    w.actors.get(n).map_or(0, |a| a.battle.mp),
                    w.battle.monster_ai_state.mode_flags,
                    w.battle.monster_ai_state.flag_bd84,
                );
                hp_prev = hp_now;
            }
        }
        if trace && std::env::var_os("LEGAIA_FGL_TRACE_BATTLE").is_some() && t % 1000 == 0 {
            eprintln!("    [battle] t={t}: {}", battle_snapshot(session));
        }
        let w = &session.host.world;
        if w.game_over_hold || w.game_over {
            if trace {
                eprintln!("    [battle] wiped after {t} ticks");
            }
            return Some(Run::Battle(format!(
                "party wiped: {}",
                battle_snapshot(session)
            )));
        }
        if w.mode != SceneMode::Battle {
            if trace {
                eprintln!(
                    "    [battle] ended after {t} ticks: {}",
                    battle_snapshot(session)
                );
            }
            return None;
        }
    }
    Some(Run::Battle(format!(
        "battle unresolved after {spent} ticks: {}",
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
    /// The entry tile the door's scene change lands on in `dest`, when the
    /// door's own record names it.
    entry: Option<(u8, u8)>,
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
                WorldMapEntityConfig::OverworldPortal {
                    scene_name,
                    entry_x,
                    entry_z,
                    ..
                } if scene_name == dest => Some(Door {
                    tile: ((x >> 7) as u8, (z >> 7) as u8),
                    entry: Some((*entry_x & 0x7F, *entry_z & 0x7F)),
                }),
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
    let mut doors: Vec<Door> = sites
        .iter()
        .filter(|s| {
            s.scene_name == dest || s.conditional.as_ref().is_some_and(|c| c.scene_name == dest)
        })
        .map(|s| Door {
            tile: (s.overworld_x, s.overworld_z),
            entry: (s.scene_name == dest).then_some((s.entry_x & 0x7F, s.entry_z & 0x7F)),
        })
        .collect();
    // Sol's warp pads: a band whose record parks on the op-`49 04` floor
    // list and branches on the picked row to one scene change per floor
    // (`kor3` P2[9..12]). With every flag clear the record's path ends in no
    // `0x3F`, so the portal join reads no door off it; [`flag_window_pad`]
    // picks the row that names the hop's destination.
    for t in triggers.iter().filter(|t| t.gate == 1) {
        if doors.iter().any(|d| d.tile == (t.tile_x, t.tile_z))
            || triggers
                .iter()
                .find(|u| (u.tile_x, u.tile_z) == (t.tile_x, t.tile_z))
                .is_some_and(|u| u.record != t.record || u.gate != 1)
        {
            continue;
        }
        if flag_window_destinations(&mf, &man, usize::from(t.record)).contains(dest) {
            doors.push(Door {
                tile: (t.tile_x, t.tile_z),
                entry: None,
            });
        }
    }
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
                    entry: None,
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
/// Times one walk-time script may fire on a single pad walk without changing
/// a flag before the walk gives up on that way.
const WALK_SCRIPT_REFIRES: u32 = 24;
/// Cross-axis distance (world units) the pad walk does not correct while it
/// still has ground to cover on the other axis.
const WALK_DEADBAND: i16 = 8;
const MAX_PLAN_NODES: usize = 120_000;
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
        if dwx == 0 && dwz == 0 {
            return 0;
        }
        // Invert the overworld remap exactly: of the eight pad directions,
        // take the one whose world bits are the wanted step's, else the one
        // sharing most of them with none opposed. A rounded rotation turns a
        // cardinal step into a diagonal whenever the camera sits off an
        // axis, and a diagonal cannot thread a one-tile cave mouth.
        let want = |b: u16| -> (i16, i16) {
            (
                i16::from(b & 0x2000 != 0) - i16::from(b & 0x8000 != 0),
                i16::from(b & 0x1000 != 0) - i16::from(b & 0x4000 != 0),
            )
        };
        let target = (dwx.signum(), dwz.signum());
        let mut best: Option<(i32, (i32, i32))> = None;
        for sx in -1..=1i32 {
            for sy in -1..=1i32 {
                if sx == 0 && sy == 0 {
                    continue;
                }
                let (wx, wz) = want(world_map_camera_relative_bits(az, sx, sy));
                let axis = |w: i16, t: i16| -> i32 {
                    if w == t {
                        2
                    } else if w == 0 || t == 0 {
                        0
                    } else {
                        -4
                    }
                };
                let score = axis(wx, target.0) + axis(wz, target.1);
                if best.is_none_or(|(b, _)| score > b) {
                    best = Some((score, (sx, sy)));
                }
            }
        }
        best.map_or((0, 0), |(_, (sx, sy))| (sx as i16, sy as i16))
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

/// Kind-0 teleport trigger dispatch tile -> landing cell.
type WarpMap = HashMap<(i32, i32), Cell>;

thread_local! {
    /// The loaded field scene's kind-0 intra-scene teleports, keyed by the
    /// scene they were read from: trigger dispatch tile -> landing cell.
    static TELEPORTS: std::cell::RefCell<(String, WarpMap)> =
        std::cell::RefCell::new((String::new(), HashMap::new()));
    /// The loaded field scene's **ledge hops**, decoded once per scene: see
    /// [`ledge_hops`].
    static LEDGE_HOPS: std::cell::RefCell<(String, Vec<LedgeHop>, Vec<ForkedHop>)> =
        const { std::cell::RefCell::new((String::new(), Vec::new(), Vec::new())) };
}

/// One ledge hop: trigger tile, landing cell, and the record's C1 / C2
/// story gates (retail `FUN_8003BDE0`).
type LedgeHop = ((i32, i32), Cell, Vec<u16>, Vec<u16>);

/// A walk-on record whose landing hangs on story flags: trigger tile, body,
/// first-opcode offset and C1 / C2 gates ([`flag_path_landing`]).
type ForkedHop = ((i32, i32), Vec<u8>, usize, Vec<u16>, Vec<u16>);

/// The loaded field scene's **ledge hops** under the live flags: gate-1
/// walk-on tiles whose partition-2 record the flags let spawn and whose
/// body arcs the player to a landing tile (op `0x43` sub-0/1/A/B on the
/// `0xF8` channel, `FUN_801DE840` case `0x43`) - trigger tile -> landing
/// cell. A mountain joins its terraces with these the way a town joins its
/// rooms with kind-0 teleports (`rikuroa` P2[6..25]). A record that hops
/// more than once, or tests where the player stands first, is left out: its
/// landing is not one place.
fn ledge_hops(session: &BootSession) -> WarpMap {
    use legaia_asset::field_disasm::{ActorCtrlKind, InsnInfo, LinearWalker, MenuCtrlKind};
    use legaia_engine_core::man_field_scripts::{partition_record_span, partition2_record_gates};
    if session.host.world.mode != SceneMode::Field {
        return HashMap::new();
    }
    let name = scene_name(session);
    LEDGE_HOPS.with(|t| {
        let mut t = t.borrow_mut();
        if t.0 != name {
            let mut hops = Vec::new();
            let mut forked = Vec::new();
            if let Some((mf, man, triggers)) = scene_man_and_triggers(session) {
                for tr in triggers.iter().filter(|t| t.gate == 1) {
                    let rec = usize::from(tr.record);
                    let Some((start, pc0, len)) = partition_record_span(&mf, &man, 2, rec) else {
                        continue;
                    };
                    let mut lands = Vec::new();
                    let mut branches = false;
                    let mut leaves = false;
                    let mut forks = false;
                    let body = &man[start..start + len];
                    // Op `0x47` walk-to-tile legs on the player (`C7 F8 x z
                    // mode`, the kernel `FUN_8003774C` case `0x47`): a
                    // transporter pad carries the walker across a wall
                    // (`deroa` P2[12] / P2[14], the platforms between its
                    // shaft rooms).
                    let mut walks = Vec::new();
                    for insn in LinearWalker::new(body, pc0).flatten() {
                        if insn.opcode == 0x47
                            && insn.extended == Some(0xF8)
                            && let (Some(&x), Some(&z)) =
                                (body.get(insn.pc + 2), body.get(insn.pc + 3))
                        {
                            walks.push((x, z));
                        }
                        match insn.info {
                            InsnInfo::ActorCtrl {
                                kind: ActorCtrlKind::ArcJump { tile_x, tile_z, .. },
                                ..
                            } if insn.extended == Some(0xF8) && (tile_x, tile_z) != (0, 0) => {
                                lands.push((tile_x, tile_z));
                            }
                            InsnInfo::BBoxTest { .. } => branches = true,
                            InsnInfo::CondJmp { .. }
                            | InsnInfo::Picker { .. }
                            | InsnInfo::SystemFlag {
                                kind: legaia_asset::field_disasm::FlagKind::Test,
                                ..
                            }
                            | InsnInfo::GFlag {
                                kind: legaia_asset::field_disasm::FlagKind::Test,
                                ..
                            } => forks = true,
                            InsnInfo::SceneChange { .. }
                            | InsnInfo::MenuCtrl {
                                kind: MenuCtrlKind::FmvTrigger { .. },
                                ..
                            } => leaves = true,
                            _ => {}
                        }
                    }
                    let at = |b: u8| {
                        i16::from(b & 0x7F) * 128 + 0x40 + if b & 0x80 != 0 { 0x40 } else { 0 }
                    };
                    // A warp pad: no arc, and every player-channel snap the
                    // record carries (`0x23` MOVE_TO / `4C 51`, on whichever
                    // story-flag branch) lands on one tile. Rogue Tower's
                    // floors join only through these (`rugi` P2[0..37]).
                    let mut snaps: Vec<(i16, i16)> =
                        legaia_engine_core::man_field_scripts::player_moves_in_region(body, pc0)
                            .into_iter()
                            .filter(|m| m.is_teleport())
                            .map(|m| (m.world_x, m.world_z))
                            .collect();
                    snaps.dedup();
                    let land = match (&lands[..], &snaps[..]) {
                        ([(x, z)], []) => Some(cell_of(at(*x), at(*z))),
                        // A chain of arcs with no fork in the record - the
                        // stepping-stone hops (`nilboa` P2[8] / P2[9] /
                        // P2[12] / P2[13], two or three arcs each) - lands
                        // where its last arc does.
                        ([.., (x, z)], []) if !forks => Some(cell_of(at(*x), at(*z))),
                        ([], [(x, z)]) if !leaves => Some(cell_of(*x, *z)),
                        // A fork-free run of player walk legs ends where its
                        // last leg does.
                        ([], []) if !forks && !leaves => {
                            walks.last().map(|&(x, z)| cell_of(at(x), at(z)))
                        }
                        _ => None,
                    };
                    if let Some(land) = land
                        && !branches
                    {
                        let (c1, c2) = partition2_record_gates(&mf, &man, rec).unwrap_or_default();
                        hops.push(((i32::from(tr.tile_x), i32::from(tr.tile_z)), land, c1, c2));
                    } else if land.is_none() && forks && !branches && !leaves && !walks.is_empty() {
                        let (c1, c2) = partition2_record_gates(&mf, &man, rec).unwrap_or_default();
                        forked.push((
                            (i32::from(tr.tile_x), i32::from(tr.tile_z)),
                            body.to_vec(),
                            pc0,
                            c1,
                            c2,
                        ));
                    }
                }
            }
            *t = (name, hops, forked);
        }
        let w = &session.host.world;
        let mut map = HashMap::new();
        for (tile, land, c1, c2) in &t.1 {
            if w.p2_record_gates_pass(c1, c2) {
                map.entry(*tile).or_insert(*land);
            }
        }
        for (tile, body, pc0, c1, c2) in &t.2 {
            if w.p2_record_gates_pass(c1, c2)
                && let Some(land) = flag_path_landing(w, body, *pc0)
            {
                map.entry(*tile).or_insert(open_landing(w, land));
            }
        }
        map
    })
}

/// `land`, or the nearest open lattice cell within a tile of it. A walk
/// leg's target carries the half-tile bit (`0x80`), and the leg's own
/// arrival stops short of it: `deroa`'s hub P2[11] walks its west leg to
/// `0xB6` (x 7040) and the platform parks the player at x 6912, beside the
/// pillar the encoded point sits in. A landing in a wall cell joins nothing.
fn open_landing(w: &legaia_engine_core::world::World, land: Cell) -> Cell {
    let open = |c: Cell| {
        let (x, z) = cell_center(c);
        !w.field_tile_is_wall(x, z)
    };
    if open(land) {
        return land;
    }
    (1..=4)
        .flat_map(|r| {
            (-r..=r).flat_map(move |dx| (-r..=r).map(move |dz| (land.0 + dx, land.1 + dz)))
        })
        .find(|&c| open(c))
        .unwrap_or(land)
}

/// Where a walk-on record whose only forks are story-flag tests walks the
/// player under the live flags: the run is followed from its first opcode
/// (a `SysFlag.Test` jumps when its flag is set - the same reading
/// [`legaia_engine_core::man_field_scripts::placement_spawn_relocation`]
/// makes), and the last player walk leg (`C7 F8 x z`) or arc on it is the
/// landing. `deroa`'s transporter hub P2[11] sends its platform to one of
/// three shaft rooms by the switch flags `0x4EF` / `0x4F0`.
fn flag_path_landing(
    w: &legaia_engine_core::world::World,
    body: &[u8],
    pc0: usize,
) -> Option<Cell> {
    use legaia_asset::field_disasm::{ActorCtrlKind, FlagKind, InsnInfo, decode};
    let at = |b: u8| i16::from(b & 0x7F) * 128 + 0x40 + if b & 0x80 != 0 { 0x40 } else { 0 };
    let mut pc = pc0;
    let mut seen = HashSet::new();
    let mut last = None;
    while seen.insert(pc) {
        let &op = body.get(pc)?;
        if op == 0x21 {
            break;
        }
        let insn = decode(body, pc).ok()?;
        if insn.opcode == 0x47
            && insn.extended == Some(0xF8)
            && let (Some(&x), Some(&z)) = (body.get(pc + 2), body.get(pc + 3))
        {
            last = Some(cell_of(at(x), at(z)));
        }
        match insn.info {
            InsnInfo::SystemFlag {
                kind: FlagKind::Test,
                idx,
                target: Some(target),
                ..
            } => {
                if w.system_flag_test(idx) {
                    pc = target & 0xFFFF;
                    continue;
                }
            }
            InsnInfo::JmpRel { target, .. } => {
                pc = target & 0xFFFF;
                continue;
            }
            InsnInfo::ActorCtrl {
                kind: ActorCtrlKind::ArcJump { tile_x, tile_z, .. },
                ..
            } if insn.extended == Some(0xF8) && (tile_x, tile_z) != (0, 0) => {
                last = Some(cell_of(at(tile_x), at(tile_z)));
            }
            InsnInfo::BBoxTest { .. } | InsnInfo::CondJmp { .. } | InsnInfo::Picker { .. } => {
                return None;
            }
            _ => {}
        }
        pc += insn.size.max(1);
    }
    last
}

/// The loaded field scene's kind-0 intra-scene teleports (`.MAP` trigger
/// block, [`legaia_engine_core::field_regions::IntraSceneTeleport`]): the
/// tile that repositions the player outright, and the cell it lands in.
/// House interiors and dungeon rooms are laid out in the same field map as
/// the street outside, joined only by these, so a planner that does not
/// model them sees each room as a separate walk component.
fn teleports(session: &BootSession) -> WarpMap {
    if session.host.world.mode != SceneMode::Field {
        return HashMap::new();
    }
    let name = scene_name(session);
    TELEPORTS.with(|t| {
        let mut t = t.borrow_mut();
        if t.0 != name {
            let index = &session.host.index;
            let mut map = HashMap::new();
            if let Ok(scene) = Scene::load(index, &name)
                && let Ok((p, f)) = scene.field_intra_scene_teleports(index)
            {
                // The dispatch takes the first primary-then-fallback match.
                for tp in p.iter().chain(f.iter()) {
                    let (x, z) = tp.dest_world();
                    map.entry((i32::from(tp.tile_x), i32::from(tp.tile_z)))
                        .or_insert(cell_of(x, z));
                }
            }
            *t = (name, map);
        }
        let mut warps = t.1.clone();
        for (k, v) in ledge_hops(session) {
            warps.entry(k).or_insert(v);
        }
        warps
    })
}

/// Retail's walk-touch contact box half-extent (`FIELD_PROP_BOX_HALF`,
/// engine-core `world::config`): a movement probe point inside it posts the
/// placement's touch.
const TOUCH_BOX_HALF: i32 = 0x50;

/// Retail's actor-collision probe points per step direction (Z-, X-, Z+,
/// X+; engine-core `world::config::FIELD_ACTOR_PROBES`, table
/// `DAT_801f21b4`), applied as `(x + dx, z - dz)`: the points whose contact
/// with a walk-touch box posts the touch.
const ACTOR_PROBES: [[(i16, i16); 3]; 4] = [
    [(-32, 64), (0, 64), (32, 64)],
    [(-63, -32), (-63, 0), (-63, 32)],
    [(-32, -63), (0, -63), (32, -63)],
    [(64, -32), (64, 0), (64, 32)],
];

/// The loaded field scene's **object doors** under the live flags: each
/// `.MAP`-object walk-touch placement whose record, resolved against the
/// story flags as the contact dispatch resolves it, teleports the player
/// (`WalkTouchEvent::PlayerMoveTo`) - contact centre and landing cell.
/// Vahn's front door and every house door of a town are this class.
fn object_doors(session: &BootSession) -> Vec<((i16, i16), Cell)> {
    use legaia_engine_core::man_field_scripts::{WalkTouchEvent, resolve_walk_touch_arm};
    let w = &session.host.world;
    if w.mode != SceneMode::Field || w.props.walk_touch.is_empty() {
        return Vec::new();
    }
    let parsed = w
        .field_vm
        .channels_man
        .as_ref()
        .and_then(|man| Some((legaia_asset::man_section::parse(man).ok()?, man.clone())));
    let mut out = Vec::new();
    for (&slot, &(contact, event)) in &w.props.walk_touch {
        let live = w
            .props
            .walk_touch_records
            .get(&slot)
            .and_then(|&flat| {
                let (mf, man) = parsed.as_ref()?;
                resolve_walk_touch_arm(mf, man, flat, &|f| w.system_flag_test(f))
            })
            .unwrap_or(Some(event));
        if let Some(WalkTouchEvent::PlayerMoveTo {
            world_x, world_z, ..
        }) = live
        {
            out.push((contact, cell_of(world_x, world_z)));
        }
    }
    out
}

/// For each object door's contact centre, the contact centres of the doors
/// its record brackets for the arrival (`B1 <obj> 00`,
/// `World::arm_arrival_bracket`): the lift platform a ride lands the player
/// on. The engine exempts that platform from the touch dispatch and the
/// collision probe until the player is out of its reach, so the landing is
/// a pocket the player walks off, not a cell pinned against the platform.
fn door_partners(session: &BootSession) -> HashMap<(i16, i16), Vec<(i16, i16)>> {
    use legaia_engine_core::man_field_scripts::record_exempted_objects;
    let w = &session.host.world;
    let Some(man) = w.field_vm.channels_man.as_ref() else {
        return HashMap::new();
    };
    let Ok(mf) = legaia_asset::man_section::parse(man) else {
        return HashMap::new();
    };
    let mut out: HashMap<(i16, i16), Vec<(i16, i16)>> = HashMap::new();
    for (&slot, &rec) in &w.props.walk_touch_records {
        let Some(&(contact, _)) = w.props.walk_touch.get(&slot) else {
            continue;
        };
        let targets = record_exempted_objects(&mf, man, rec);
        for (&s2, &r2) in &w.props.walk_touch_records {
            if s2 != slot
                && targets.iter().any(|&t| usize::from(t) == r2)
                && let Some(&(pos, _)) = w.props.walk_touch.get(&s2)
            {
                out.entry(contact).or_default().push(pos);
            }
        }
    }
    out
}

/// Whether a player standing at cell `c` reaches the contact box centred on
/// `at` from its centre or any of its four probe sets.
fn in_reach(c: Cell, at: (i16, i16)) -> bool {
    let (cx, cz) = cell_center(c);
    std::iter::once((0, 0))
        .chain(ACTOR_PROBES.iter().flatten().copied())
        .any(|(dx, dz)| {
            let (px, pz) = (i32::from(cx) + i32::from(dx), i32::from(cz) - i32::from(dz));
            (px - i32::from(at.0)).abs() < TOUCH_BOX_HALF
                && (pz - i32::from(at.1)).abs() < TOUCH_BOX_HALF
        })
}

/// Anchors of the placed props whose bind record runs `31 00` at a clean
/// decode boundary: the touch pass of a door. Bit 0 of the actor flag word
/// `+0x10` is the collision / touch exemption (`FUN_801CF754` /
/// `FUN_801CF9F4` skip `flags & 3`), so the leaf stops blocking once the
/// player's press into it has posted the touch - the press is the opening.
fn self_opening_doors(session: &BootSession) -> HashSet<(u8, u8)> {
    door_openings(session)
        .into_iter()
        .filter(|(_, b)| b.is_none())
        .map(|(a, _)| a)
        .collect()
}

/// A door that opens only for a player standing in a tile box (the
/// `BBoxTest` on the player channel ahead of its `31 00`): the box, with
/// the collider centres of the door's leaves. `ropeway` P0[2], the station
/// door, opens for a player inside on tiles (27..30, 33..34) and is a wall
/// from the corridor south of it.
type TileBox = (u8, u8, u8, u8);
type BoxedDoor = (TileBox, Vec<(i16, i16)>);

fn boxed_doors(session: &BootSession) -> Vec<BoxedDoor> {
    let w = &session.host.world;
    door_openings(session)
        .into_iter()
        .filter_map(|(a, b)| {
            let b = b?;
            let leaves = w
                .props
                .colliders
                .iter()
                .filter(|c| c.anchor == Some(a) && !c.moving_box)
                .filter_map(|c| {
                    Some((
                        i16::try_from(c.center.0).ok()?,
                        i16::try_from(c.center.1).ok()?,
                    ))
                })
                .collect();
            Some((b, leaves))
        })
        .collect()
}

/// Every placed prop whose touch opens it (`31 00` reached from the
/// resume point): `None` when nothing can branch around the opening,
/// `Some(box)` when only a player-channel box test (tiles `x_min, z_min,
/// x_max, z_max`) stands before it.
fn door_openings(session: &BootSession) -> HashMap<(u8, u8), Option<TileBox>> {
    use legaia_asset::field_disasm::{InsnInfo, LinearWalker};
    use legaia_engine_core::man_field_scripts::partition_record_span;
    let w = &session.host.world;
    if w.mode != SceneMode::Field || w.props.bank.props.is_empty() {
        return HashMap::new();
    }
    let Some(man) = w.field_vm.channels_man.as_ref() else {
        return HashMap::new();
    };
    let Ok(mf) = legaia_asset::man_section::parse(man) else {
        return HashMap::new();
    };
    // The touch resumes the record past its spawn prologue (the first `21`),
    // or at its first op when it has none (`FUN_8003A55C` runs a prologue
    // only behind the `24` / `25` marker: `chitei2`'s lift-cage doors,
    // partition-0 records 6..11, are a bare `31 00` the touch runs);
    // the door opens itself only when the `31 00` comes before anything
    // that can branch around it, or behind one box test of the player.
    // A story-flag test on the way is followed against the live flags, as
    // the touch runs it: `ropeway2` P0[16], the elevator door, opens only
    // once `0x1D5` (the power cut) is set.
    let opens = |body: &[u8], pc0: usize| -> Option<Option<TileBox>> {
        let mut pc = if matches!(body.get(pc0), Some(0x24 | 0x25)) {
            LinearWalker::new(body, pc0)
                .map_while(Result::ok)
                .find(|i| i.opcode == 0x21)
                .map(|i| i.pc + i.size)?
        } else {
            pc0
        };
        let mut boxed = None;
        let mut seen = HashSet::new();
        while seen.insert(pc) {
            let insn = legaia_asset::field_disasm::decode(body, pc).ok()?;
            if insn.size == 0 {
                return None;
            }
            match insn.info {
                InsnInfo::CFlag {
                    kind: FlagKind::Set,
                    bit: 0,
                } if insn.extended.is_none() => return Some(boxed),
                InsnInfo::BBoxTest {
                    x_min,
                    z_min,
                    x_max,
                    z_max,
                    ..
                } if boxed.is_none() && insn.extended == Some(0xF8) => {
                    boxed = Some((x_min, z_min, x_max, z_max));
                }
                InsnInfo::SystemFlag {
                    kind: FlagKind::Test,
                    idx,
                    target: Some(target),
                    ..
                } => {
                    if w.system_flag_test(idx) {
                        pc = target;
                        continue;
                    }
                }
                InsnInfo::JmpRel { target, .. } => {
                    pc = target;
                    continue;
                }
                InsnInfo::SystemFlag {
                    kind: FlagKind::Test,
                    ..
                }
                | InsnInfo::BBoxTest { .. }
                | InsnInfo::Picker { .. }
                | InsnInfo::SpawnRecord { .. } => return None,
                _ if insn.opcode == 0x21 => return None,
                _ => {}
            }
            pc += insn.size;
        }
        None
    };
    w.props
        .bank
        .props
        .iter()
        .filter_map(|(&a, p)| {
            let (start, pc0, len) = partition_record_span(&mf, man, 0, p.record)?;
            Some((a, opens(&man[start..start + len], pc0)?))
        })
        .collect()
}

/// The actor-collision boxes one plan tests against, bucketed by the
/// 128-unit tiles each overlaps: every field NPC at its live position
/// (±40) and every solid placed prop (±80 static, ±40 moving). The
/// planner's copy of `World::field_actor_dir_blocked`, built once per plan
/// so a whole-map search does not re-walk every body per probe.
/// A body box: centre `(x, z)` and half-extent.
type BodyBox = (i32, i32, i32);

struct Blockers(HashMap<(i32, i32), Vec<BodyBox>>);

impl Blockers {
    fn build(
        w: &legaia_engine_core::world::World,
        pass: impl Fn(&legaia_engine_core::world::FieldPropCollider) -> bool,
    ) -> Self {
        const NPC_HALF: i32 = 0x40 - 0x18;
        let mut boxes: Vec<(i32, i32, i32)> = w
            .npcs
            .positions
            .values()
            .map(|&(x, z)| (i32::from(x), i32::from(z), NPC_HALF))
            .collect();
        for c in &w.props.colliders {
            if !c.solid || pass(c) {
                continue;
            }
            boxes.push(if c.moving_box {
                (c.live.0, c.live.1, NPC_HALF)
            } else {
                (c.center.0, c.center.1, TOUCH_BOX_HALF)
            });
        }
        let mut map: HashMap<(i32, i32), Vec<BodyBox>> = HashMap::new();
        for b in boxes {
            let (x, z, h) = b;
            for tz in ((z - h) >> 7)..=((z + h) >> 7) {
                for tx in ((x - h) >> 7)..=((x + h) >> 7) {
                    map.entry((tx, tz)).or_default().push(b);
                }
            }
        }
        Self(map)
    }

    /// Whether a body box covers `(px, pz)`, ignoring the static boxes
    /// centred on `skip` (the platforms under the arrival bracket).
    fn hit_except(&self, px: i32, pz: i32, skip: &[(i16, i16)]) -> bool {
        self.0.get(&(px >> 7, pz >> 7)).is_some_and(|v| {
            v.iter().any(|&(x, z, h)| {
                (px - x).abs() < h
                    && (pz - z).abs() < h
                    && !(h == TOUCH_BOX_HALF
                        && skip
                            .iter()
                            .any(|&(sx, sz)| (i32::from(sx), i32::from(sz)) == (x, z)))
            })
        })
    }

    fn dir_blocked_except(&self, cx: i16, cz: i16, dir: usize, skip: &[(i16, i16)]) -> bool {
        ACTOR_PROBES[dir].iter().any(|&(dx, dz)| {
            self.hit_except(
                i32::from(cx) + i32::from(dx),
                i32::from(cz) - i32::from(dz),
                skip,
            )
        })
    }
}

thread_local! {
    /// Teleport waypoints of the routes [`plan_path`] returned: the cell a
    /// route steps into a teleport tile or against an object door from, and
    /// the world point the follower presses toward there (the teleport
    /// tile's centre, the door's contact centre). The route's next cell is
    /// the landing, which no press reaches.
    static PRESS_AT: std::cell::RefCell<HashMap<Cell, (i16, i16)>> =
        std::cell::RefCell::new(HashMap::new());
}

thread_local! {
    /// Set while [`cross_over`] walks back out of a crossing scene: the
    /// pad hop then takes the reachable door farthest from where the player
    /// came in, instead of the nearest (which is the one it came in by).
    static FAR_DOOR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Set while [`cross_over`] walks back out of a crossing scene whose
    /// round trip the lattice planned: the landing tile the plan needs. The
    /// pad hop then prefers the doors whose own record lands there.
    static WANT_LANDING: std::cell::RefCell<Option<(i16, i16)>> =
        const { std::cell::RefCell::new(None) };
}

/// What a [`cross_over`] detour is for: a door toward another scene, or the
/// band tiles of a walk-on beat in another walk component of the scene.
#[derive(Debug, Clone)]
enum CrossTarget {
    /// A door of the loaded scene toward this scene.
    Door(String),
    /// Any of these tiles (a walk-on band).
    Tiles(Vec<(i16, i16)>),
}

impl CrossTarget {
    /// The tiles the lattice plans toward.
    fn tiles(&self, session: &BootSession, graph: &DiscGraph) -> Vec<(i16, i16)> {
        match self {
            CrossTarget::Door(toward) => doors_to(session, graph, toward)
                .map(|d| {
                    d.iter()
                        .map(|d| (i16::from(d.tile.0), i16::from(d.tile.1)))
                        .collect()
                })
                .unwrap_or_default(),
            CrossTarget::Tiles(t) => t.clone(),
        }
    }

    /// Whether the pad walk can now reach the target from where it stands.
    fn in_reach(&self, session: &BootSession, graph: &DiscGraph) -> bool {
        match self {
            CrossTarget::Door(toward) => door_in_reach(session, graph, toward),
            CrossTarget::Tiles(t) => tiles_in_reach(session, t),
        }
    }
}

/// Can the pad walk reach one of `tiles` (or a tile beside it) from where
/// the player stands?
fn tiles_in_reach(session: &BootSession, tiles: &[(i16, i16)]) -> bool {
    let avoid = hazards(session, "");
    let (x, z) = player_xz(session);
    let from = cell_of(x, z);
    tiles.iter().any(|&g| {
        plan_path(session, from, g, &avoid)
            .and_then(|p| p.last().copied())
            .is_some_and(|c| {
                let t = tile_of(cell_center(c).0, cell_center(c).1);
                (t.0 - g.0).abs() + (t.1 - g.1).abs() <= 1
            })
    })
}

/// Whether the milestone beat in partition-2 record `rec` should wait: its
/// record sets one of the milestone's missing reach flags and clears a flag
/// the live state holds, and another
/// missing reach flag's band lies outside the player's walk component. The
/// pad hand plays the far beat first, by a crossing, the way the story does:
/// `chitei2` P2[11] (the `0x470` climax) clears the Rapid Transport switch
/// flags `0x4EF` / `0x4F0` that route `deroa`'s platform hub to the room
/// whose door re-enters by the junction band P2[3] (`0x3C8`).
fn reach_beat_waits(
    session: &BootSession,
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    rec: usize,
) -> bool {
    use legaia_asset::field_disasm::{InsnInfo, LinearWalker};
    use legaia_engine_core::man_field_scripts::partition_record_span;
    let Some(target) = BEAT_TARGET.with(|t| t.borrow().clone()) else {
        return false;
    };
    let missing: Vec<u16> = target
        .reach_flags
        .iter()
        .copied()
        .filter(|&f| !session.host.world.system_flag_test(f))
        .collect();
    if missing.len() < 2 {
        return false;
    }
    let Some((start, pc0, len)) = partition_record_span(mf, man, 2, rec) else {
        return false;
    };
    let sets: Vec<u16> = LinearWalker::new(&man[start..start + len], pc0)
        .map_while(Result::ok)
        .filter_map(|i| match i.info {
            InsnInfo::SystemFlag {
                kind: FlagKind::Set,
                idx,
                ..
            } if missing.contains(&idx) => Some(idx),
            _ => None,
        })
        .collect();
    if sets.is_empty() {
        return false;
    }
    // Only a beat that takes something away waits: one that clears a flag
    // the live state holds (P2[11] clears the switch flags). A beat that only
    // adds state is played where it is met - P2[3] waiting on P2[11] in turn
    // would leave both unplayed.
    let w = &session.host.world;
    let takes = LinearWalker::new(&man[start..start + len], pc0)
        .map_while(Result::ok)
        .any(|i| {
            matches!(i.info, InsnInfo::SystemFlag { kind: FlagKind::Clear, idx, .. } if w.system_flag_test(idx))
        });
    if !takes {
        return false;
    }
    missing.iter().filter(|f| !sets.contains(f)).any(|&f| {
        let bands = reach_flag_bands(session, &[f]);
        !bands.is_empty() && !tiles_in_reach(session, &bands)
    })
}

/// The band tiles of the live walk-on records of the loaded scene whose own
/// body sets one of `flags` - the beats a milestone's missing reach flags
/// wait on (`chitei2` P2[3], the junction's north-south band, sets `0x3C8`).
fn reach_flag_bands(session: &BootSession, flags: &[u16]) -> Vec<(i16, i16)> {
    use legaia_asset::field_disasm::{InsnInfo, LinearWalker};
    use legaia_engine_core::man_field_scripts::{partition_record_span, partition2_record_gates};
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return Vec::new();
    };
    let w = &session.host.world;
    let mut out = Vec::new();
    for t in triggers.iter().filter(|t| t.gate == 1) {
        let r = usize::from(t.record);
        let pass = partition2_record_gates(&mf, &man, r)
            .is_some_and(|(c1, c2)| w.p2_record_gates_pass(&c1, &c2));
        if !pass {
            continue;
        }
        let Some((start, pc0, len)) = partition_record_span(&mf, &man, 2, r) else {
            continue;
        };
        let body = &man[start..start + len];
        let sets = LinearWalker::new(body, pc0).map_while(Result::ok).any(|i| {
            matches!(i.info, InsnInfo::SystemFlag { kind: FlagKind::Set, idx, .. } if flags.contains(&idx))
        });
        if sets {
            out.push((i16::from(t.tile_x), i16::from(t.tile_z)));
        }
    }
    out
}

/// A detour through a **crossing scene**: `cur`'s door toward `goal` lies in
/// another walk component of `cur` (`map01`'s north and south halves meet
/// only through `suimon`). A player crosses by entering a scene `X` that
/// `cur` has a door to and that has a door back, and leaving `X` by its
/// other door. Each candidate `X` is tried by pad hop in turn - an
/// unreachable door refuses before any walking - and `Ok(scene)` is where
/// the round trip landed.
fn cross_over(
    session: &mut BootSession,
    graph: &DiscGraph,
    cur: &str,
    goal: &str,
    target: &CrossTarget,
) -> Result<String, String> {
    // The doors a band crossing refused stay refused only for the crossing:
    // the segment's other hops still take them.
    let mut refused_here: Vec<(String, (i16, i16))> = Vec::new();
    let r = cross_over_inner(session, graph, cur, goal, target, &mut refused_here);
    REFUSED_DOORS.with(|r| {
        let mut r = r.borrow_mut();
        for k in refused_here {
            r.remove(&k);
        }
    });
    r
}

/// [`cross_over`]'s body; `refused_here` collects the doors it refused.
fn cross_over_inner(
    session: &mut BootSession,
    graph: &DiscGraph,
    cur: &str,
    goal: &str,
    target: &CrossTarget,
    refused_here: &mut Vec<(String, (i16, i16))>,
) -> Result<String, String> {
    let back = |x: &String| graph.edges.get(x).is_some_and(|e| e.contains(cur));
    let mut cands: Vec<String> = graph
        .edges
        .get(cur)
        .into_iter()
        .flatten()
        .filter(|x| x.as_str() != goal && x.as_str() != cur && back(x))
        .cloned()
        .collect();
    // The crossing the lattice names goes first ([`crossing_plan`]); the
    // rest keep their order behind it, so a chain the lattice cannot see is
    // still found by trial.
    let all = cands.clone();
    let plan = |session: &BootSession, shut: &BTreeSet<String>| {
        let open: Vec<String> = all.iter().filter(|x| !shut.contains(*x)).cloned().collect();
        let p = crossing_plan(session, graph, &open, &target.tiles(session, graph));
        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
            let (px, pz) = player_xz(session);
            eprintln!(
                "    [cross] {cur} toward {target:?} from {:?}: lattice plan {p:?} (shut {shut:?})",
                dispatch_tile(px, pz)
            );
        }
        p
    };
    // Crossings that brought the player back to the side it left, even with
    // their beats played: which side a crossing delivers to can be story
    // state (`suimon`'s water gate, `0x27B`, switches the `map01` entry to
    // the southern chamber), and the lattice cannot see it.
    let mut shut: BTreeSet<String> = BTreeSet::new();
    // Where the planned round trip should land, by crossing: leaving it,
    // the pad hand takes the door whose own record lands there.
    let mut want: HashMap<String, (i16, i16)> = HashMap::new();
    if let Some((x, landing)) = plan(session, &shut)
        && let Some(i) = cands.iter().position(|c| *c == x)
    {
        if let Some(l) = landing {
            want.insert(x.clone(), l);
        }
        let x = cands.remove(i);
        cands.insert(0, x);
    }
    let mut tried = Vec::new();
    let mut crossings = 0usize;
    let mut visited: BTreeSet<String> = BTreeSet::new();
    // A crossing that brings the player back to the side it left is taken
    // once more with its own beats played first.
    let mut queue: VecDeque<(String, bool)> = cands.into_iter().map(|x| (x, false)).collect();
    while let Some((x, again)) = queue.pop_front() {
        if crossings >= MAX_CROSSINGS {
            tried.push(format!("{MAX_CROSSINGS} crossings taken"));
            break;
        }
        let (fx, fz) = player_xz(session);
        let left_from = tile_of(fx, fz);
        LAST_HOP_DOOR.with(|l| *l.borrow_mut() = None);
        let r = pad_hop(session, graph, &x);
        let out_door = LAST_HOP_DOOR.with(|l| l.borrow().clone());
        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
            let (px, pz) = player_xz(session);
            eprintln!(
                "    [cross] {cur} via {x}: {:?} at {:?}",
                r.as_ref()
                    .map_err(|e| e.chars().take(120).collect::<String>()),
                dispatch_tile(px, pz)
            );
        }
        match r {
            Ok(entered) if entered == x => {
                crossings += 1;
                visited.insert(x.clone());
                let r = match run_while_moving(session, SCRIPT_CEILING) {
                    // An arrival script that carries the party straight back
                    // out (`rikuroa` turns a party away before its story
                    // opens) is a crossing like any other: where it lands is
                    // the answer.
                    Run::Entered(s) => {
                        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                            eprintln!("    [cross] {x}'s arrival carried the party to {s}");
                        }
                        // Let the landing scene's own arrival run before the
                        // position is read: a hand-off may land on the
                        // parked sentinel (127, 127) and leave the seat to
                        // the destination's script (`ropeway` P2[31] lands
                        // in `jiji`, whose entry spawns P2[10] and its
                        // `CC F8 51` puts the party at (67, 74)).
                        match run_while_moving(session, SCRIPT_CEILING) {
                            Run::Entered(s2) => Ok(s2),
                            _ => Ok(s),
                        }
                    }
                    Run::Released => {
                        // On the second visit, play the crossing's own beats
                        // first: a sluice gate or a lever is a story beat like
                        // any other.
                        let mut left = None;
                        if again {
                            let mut log = Vec::new();
                            left = play_beats(session, &mut log)
                                .map_err(|b| format!("in crossing {x}, playing beats: {b}"))?;
                        }
                        match left {
                            // A beat that changed scene may land on a
                            // cutscene that carries the party on (`suimon`'s
                            // water gate drains on `map01` at `(0, 0)` and
                            // returns to the drained chamber): let it run.
                            Some(s) => match run_while_moving(session, SCRIPT_CEILING) {
                                Run::Entered(s2) => Ok(s2),
                                _ => Ok(s),
                            },
                            None => {
                                FAR_DOOR.with(|f| f.set(true));
                                let wanted = want.get(&x).copied();
                                WANT_LANDING.with(|w| *w.borrow_mut() = wanted);
                                let mut r = pad_hop(session, graph, cur);
                                WANT_LANDING.with(|w| *w.borrow_mut() = None);
                                // The planned side's door is out of reach
                                // from where the crossing let the player in
                                // (its halves join on story state), or the
                                // walk there stalled: any door.
                                if wanted.is_some() && scene_name(session) == x && r.is_err() {
                                    r = pad_hop(session, graph, cur);
                                }
                                FAR_DOOR.with(|f| f.set(false));
                                // A door whose record runs a script before
                                // its scene change can report a release while
                                // the change is still to come (`dolk`'s south
                                // door): let the scene settle, and take the
                                // scene it lands in as the answer.
                                match r {
                                    Err(e) => {
                                        let settled = run(session, EXIT_TICKS, false);
                                        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                                            eprintln!(
                                                "    [cross] leaving {x}: {e}; settled {settled:?} in {}",
                                                scene_name(session)
                                            );
                                        }
                                        match settled {
                                            Run::Entered(s) => Ok(s),
                                            _ if scene_name(session) == cur => Ok(cur.to_string()),
                                            _ => Err(e),
                                        }
                                    }
                                    r => r,
                                }
                            }
                        }
                    }
                    other => return Err(format!("entering crossing {x}: {other:?}")),
                };
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    let (px, pz) = player_xz(session);
                    eprintln!(
                        "    [cross] left {x}: {r:?}, now {} at {:?}",
                        scene_name(session),
                        dispatch_tile(px, pz)
                    );
                }
                match r {
                    // Back in `cur`: done when the goal's door is now in
                    // reach, else the next crossing from here.
                    Ok(s) if s == cur => {
                        let (px, pz) = player_xz(session);
                        if target.in_reach(session, graph) {
                            if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                                eprintln!(
                                    "    [cross] {cur}: {target:?} in reach from {:?}",
                                    dispatch_tile(px, pz)
                                );
                            }
                            return Ok(s);
                        }
                        let back =
                            plan_path(session, cell_of(px, pz), left_from, &hazards(session, ""))
                                .and_then(|p| p.last().copied())
                                .is_none_or(|c| {
                                    let t = tile_of(cell_center(c).0, cell_center(c).1);
                                    i32::from((t.0 - left_from.0).abs() + (t.1 - left_from.1).abs())
                                        <= DOOR_APPROACH_SLACK
                                });
                        if back && again {
                            shut.insert(x.clone());
                        }
                        // Back on the side it left: the door it went out by
                        // leads to the part of `x` that only returns here
                        // (`chitei2`'s P2[4] lands north of `deroa`'s
                        // platform hub, P2[5] south of it, where the hub ride
                        // starts). The next round trip leaves by another.
                        let mut new_door = false;
                        if back
                            && matches!(target, CrossTarget::Tiles(_))
                            && let Some((s0, d)) = out_door.clone()
                            && s0 == cur
                        {
                            new_door = REFUSED_DOORS.with(|r| {
                                r.borrow_mut()
                                    .insert((s0.clone(), d), flags_of_world(session))
                                    .is_none()
                            });
                            if new_door {
                                refused_here.push((s0, d));
                            }
                        }
                        // The lattice names the next crossing from where the
                        // player landed. A crossing taken before is taken
                        // again with its beats played.
                        match plan(session, &shut) {
                            Some((y, landing)) => {
                                match landing {
                                    Some(l) => want.insert(y.clone(), l),
                                    None => want.remove(&y),
                                };
                                // A round trip by a door not taken before
                                // is a first visit: its beats wait.
                                let seen = visited.contains(&y) && !new_door;
                                queue.retain(|(q, _)| *q != y);
                                queue.push_front((y, seen));
                            }
                            None if !again => queue.push_front((x.clone(), true)),
                            None => {}
                        }
                        tried.push(format!(
                            "{x}: landed {} short of the door",
                            if back {
                                "back on the same side"
                            } else {
                                "on a new side"
                            }
                        ));
                    }
                    Ok(s) => return Ok(s),
                    Err(e) => return Err(format!("crossing {x} back to {cur}: {e}")),
                }
            }
            Ok(other) => return Ok(other),
            Err(e) => tried.push(format!("{x}: {}", e.chars().take(80).collect::<String>())),
        }
    }
    Err(format!(
        "no crossing scene reachable ({})",
        tried.join("; ")
    ))
}

/// The lattice cells of dispatch tile `t` (four 32-unit cells a side).
fn tile_cells(t: (i16, i16)) -> impl Iterator<Item = Cell> {
    (0..4).flat_map(move |i| (0..4).map(move |j| (t.0 * 4 + i, t.1 * 4 + j)))
}

/// The four tiles beside `t`.
fn tile_nbrs(t: (i16, i16)) -> [(i16, i16); 4] {
    [
        (t.0, t.1 + 1),
        (t.0, t.1 - 1),
        (t.0 + 1, t.1),
        (t.0 - 1, t.1),
    ]
}

/// A tile, and the landing it was seeded from (when a round trip's own
/// entry tile seeded it).
type Seed = ((i16, i16), Option<(i16, i16)>);

/// The first crossing of a chain, and the landing of its round trip.
type FirstHop = Option<(String, Option<(i16, i16)>)>;

/// Most [`cross_over`] detours one [`traverse`] takes per (scene, goal).
const MAX_CROSS_OVERS: usize = 3;

/// Most round trips one [`cross_over`] takes.
const MAX_CROSSINGS: usize = 8;

/// Most walk components one [`crossing_plan`] floods.
const MAX_SIDES: usize = 16;

/// The crossing to take first toward `toward`'s door, read off the loaded
/// scene's lattice. Every door of every scene is a boundary (stepping on it
/// leaves), so the scene splits into **sides**: walk components. A
/// candidate `x` joins each side that touches one of its door tiles to each
/// other such side, since a round trip through `x` lands beside the door it
/// leaves by. A breadth-first search from the player's side to one touching
/// a door toward `toward` names the first crossing of the shortest chain:
/// once its water gate is drained, `map01`'s north half reaches the `vell`
/// door through `suimon` and then `bylon`, while `dolk` - the nearer
/// crossing - only opens a pocket south of the castle. `None` when the lattice shows no chain; the caller then
/// tries every candidate in turn.
fn crossing_plan(
    session: &BootSession,
    graph: &DiscGraph,
    cands: &[String],
    targets: &[(i16, i16)],
) -> FirstHop {
    if targets.is_empty() {
        return None;
    }
    let xdoors: Vec<(String, Vec<(i16, i16)>)> = cands
        .iter()
        .filter_map(|x| {
            let d = doors_to(session, graph, x).ok()?;
            Some((
                x.clone(),
                d.iter()
                    .map(|d| (i16::from(d.tile.0), i16::from(d.tile.1)))
                    .collect(),
            ))
        })
        .collect();
    let avoid = hazards(session, "");
    let w = &session.host.world;
    let flood = |from: Cell| -> HashSet<Cell> {
        plan_search(session, from, (-64, -64), None, &avoid)
            .map(|(_, seen, _)| seen)
            .unwrap_or_default()
    };
    let touches = |side: &HashSet<Cell>, t: (i16, i16)| {
        tile_nbrs(t)
            .into_iter()
            .any(|n| tile_cells(n).any(|c| side.contains(&c)))
    };
    let here = scene_name(session);
    let (px, pz) = player_xz(session);
    let mut sides: Vec<HashSet<Cell>> = vec![flood(cell_of(px, pz))];
    // The first crossing of the chain that reached each side, and the
    // landing of that first round trip (when the side was seeded by one).
    let mut first: Vec<FirstHop> = vec![None];
    let mut i = 0;
    while i < sides.len() {
        if i > 0 && targets.iter().any(|&t| touches(&sides[i], t)) {
            return first[i].clone();
        }
        for (x, tiles) in &xdoors {
            if !tiles.iter().any(|&t| touches(&sides[i], t)) {
                continue;
            }
            // Where the round trip lands: the entry tiles of `x`'s own
            // scene changes back to this scene, else beside its doors here.
            // A landing on a door tile (a portal the entry re-seats the
            // player on) counts by its neighbours.
            let seeds: Vec<Seed> = match graph.landings.get(&(x.clone(), here.clone())) {
                Some(l) if !l.is_empty() => l
                    .iter()
                    .map(|&(lx, lz)| (i16::from(lx), i16::from(lz)))
                    .flat_map(|t| {
                        std::iter::once(t)
                            .chain(tile_nbrs(t))
                            .map(move |n| (n, Some(t)))
                    })
                    .collect(),
                _ => tiles
                    .iter()
                    .flat_map(|&t| tile_nbrs(t))
                    .map(|n| (n, None))
                    .collect(),
            };
            for (n, landing) in seeds {
                if sides.len() >= MAX_SIDES {
                    break;
                }
                if avoid.contains(&(i32::from(n.0), i32::from(n.1)))
                    || sides.iter().any(|s| tile_cells(n).any(|c| s.contains(&c)))
                {
                    continue;
                }
                let Some(start) = tile_cells(n).find(|&c| {
                    let (cx, cz) = cell_center(c);
                    !w.field_tile_is_wall(cx, cz)
                }) else {
                    continue;
                };
                let side = flood(start);
                // A sliver of open ground inside a wall is not a side.
                if side.len() < 16 {
                    continue;
                }
                sides.push(side);
                first.push(first[i].clone().or_else(|| Some((x.clone(), landing))));
            }
        }
        i += 1;
    }
    None
}

/// Can the pad walk reach a door of the loaded scene toward `dest`?
fn door_in_reach(session: &BootSession, graph: &DiscGraph, dest: &str) -> bool {
    let Ok(doors) = doors_to(session, graph, dest) else {
        return false;
    };
    let avoid = hazards(session, dest);
    let (x, z) = player_xz(session);
    let from = cell_of(x, z);
    doors.iter().any(|d| {
        let g = (i16::from(d.tile.0), i16::from(d.tile.1));
        plan_path(session, from, g, &avoid)
            .and_then(|p| p.last().copied())
            .is_some_and(|c| {
                let t = tile_of(cell_center(c).0, cell_center(c).1);
                door_approachable(session, t, g)
            })
    })
}

/// Can the follower press on from `end`, where the lattice stopped, to the
/// door band `goal`? A door tile reads as a wall from inside, so the lattice
/// routinely ends a few tiles short ([`DOOR_APPROACH_SLACK`]) - but not a
/// gap with a teleport tile in it, which carries the player off before the
/// band. `kor3`'s corridor from the west doorway (18..19, 39) lands at
/// (115, 13); the door to `kor` is (115, 11), and (115, 12) between them
/// teleports back to the doorway, so the door is reached from the north,
/// round the pillar over the spent P2[14] band at (113, 5..6).
fn door_approachable(session: &BootSession, end: (i16, i16), goal: (i16, i16)) -> bool {
    let d = i32::from((end.0 - goal.0).abs() + (end.1 - goal.1).abs());
    if d > DOOR_APPROACH_SLACK {
        return false;
    }
    if d <= 1 {
        return true;
    }
    let (lo_x, hi_x) = (i32::from(end.0.min(goal.0)), i32::from(end.0.max(goal.0)));
    let (lo_z, hi_z) = (i32::from(end.1.min(goal.1)), i32::from(end.1.max(goal.1)));
    let ends = [
        (i32::from(end.0), i32::from(end.1)),
        (i32::from(goal.0), i32::from(goal.1)),
    ];
    !teleports(session).keys().any(|&w| {
        (lo_x..=hi_x).contains(&w.0) && (lo_z..=hi_z).contains(&w.1) && !ends.contains(&w)
    })
}

/// Is `b` one lattice step from `a`?
fn adjacent(a: Cell, b: Cell) -> bool {
    (a.0 - b.0).abs() + (a.1 - b.1).abs() <= 1
}

/// One step of [`plan_path`]'s search: reach `to` from `cur` at `cost`,
/// through the waypoint `via` when the step is a teleport.
struct Search {
    parent: HashMap<Cell, Cell>,
    g: HashMap<Cell, i32>,
    open: std::collections::BinaryHeap<std::cmp::Reverse<(i32, i32, Cell)>>,
    goal: Cell,
}

impl Search {
    fn h(&self, c: Cell) -> i32 {
        i32::from((c.0 - self.goal.0).abs() + (c.1 - self.goal.1).abs())
    }

    fn step(&mut self, via: Option<Cell>, cur: Cell, to: Cell, cost: i32) {
        if self.g.get(&to).is_some_and(|&old| old <= cost) {
            return;
        }
        match via {
            Some(v) => {
                // The waypoint's cost is recorded too: every parent edge
                // must run from a cheaper cell to a dearer one, or a later
                // plain step into `v` (from a cell descended from `to`)
                // overwrites `parent[v]` and closes a cycle that the path
                // walk-back follows forever.
                self.parent.insert(v, cur);
                self.g.insert(v, cost - 1);
                self.parent.insert(to, v);
            }
            None => {
                self.parent.insert(to, cur);
            }
        }
        self.g.insert(to, cost);
        let f = cost + self.h(to);
        self.open.push(std::cmp::Reverse((f, cost, to)));
    }
}

thread_local! {
    /// Plans run and cells they expanded, for the trace's cost line.
    static PLAN_STATS: std::cell::Cell<(u64, u64)> = const { std::cell::Cell::new((0, 0)) };
}

/// A* over the collision lattice toward `goal`, never entering an `avoid`
/// dispatch tile (retail fires on a tile change, so occupying one is safe).
/// A step into a kind-0 teleport tile, or against an object door's contact
/// box, continues from its landing cell, so a route may pass through a door
/// into a room laid out elsewhere in the map. When the goal is out of
/// reach the route ends at the reachable cell nearest it.
fn plan_path(
    session: &BootSession,
    from: Cell,
    goal: (i16, i16),
    avoid: &HashSet<(i32, i32)>,
) -> Option<Vec<Cell>> {
    // A goal outside the start's walk component costs a whole-component
    // search every time it is asked for, and a stalled walk asks every
    // second. The component and its nearest cell do not move while the
    // scene, the flags and the avoid set stand, so a start inside a
    // remembered failure plans straight to that cell instead.
    let key = plan_key(session, goal, avoid);
    let hit = UNREACHED.with(|u| {
        u.borrow()
            .get(&key)
            .filter(|(seen, _)| seen.contains(&from))
            .map(|&(_, best)| best)
    });
    if let Some(best) = hit {
        if best == from {
            return None;
        }
        if let Some((path, _, _)) = plan_search(session, from, goal, Some(best), avoid)
            && path.last() == Some(&best)
        {
            return Some(path);
        }
    }
    let (path, seen, reached) = plan_search(session, from, goal, None, avoid)?;
    if !reached && std::env::var_os("LEGAIA_FGL_COMP_DEBUG").is_some() {
        let tiles: HashSet<(i32, i32)> = seen
            .iter()
            .map(|&c| {
                let (x, z) = cell_center(c);
                dispatch_tile(x, z)
            })
            .collect();
        let bl = Blockers::build(&session.host.world, |_| false);
        eprintln!(
            "      [comp] {} goal {goal:?} cells {}",
            scene_name(session),
            seen.len()
        );
        {
            // The same question at wall-bit granularity: 64-unit sub-cells,
            // a sub-cell open when its centre reads no wall, four-connected.
            let w = &session.host.world;
            let open = |sx: i32, sz: i32| {
                (0..256).contains(&sx)
                    && (0..256).contains(&sz)
                    && !w.field_tile_is_wall((sx * 64 + 32) as i16, (sz * 64 + 32) as i16)
            };
            let (fx, fz) = cell_center(from);
            let s0 = (i32::from(fx) >> 6, i32::from(fz) >> 6);
            let mut seen2: HashSet<(i32, i32)> = HashSet::from([s0]);
            let mut q = VecDeque::from([s0]);
            while let Some((x, z)) = q.pop_front() {
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let n = (x + dx, z + dz);
                    if open(n.0, n.1) && !avoid.contains(&(n.0 >> 1, n.1 >> 1)) && seen2.insert(n) {
                        q.push_back(n);
                    }
                }
            }
            let g = (i32::from(goal.0), i32::from(goal.1));
            let near = seen2
                .iter()
                .map(|&(x, z)| ((x >> 1) - g.0).abs() + ((z >> 1) - g.1).abs())
                .min()
                .unwrap_or(i32::MAX);
            eprintln!(
                "      [comp] sub-cell flood: {} sub-cells, nearest tile to goal {near} away",
                seen2.len()
            );
        }
        let w = &session.host.world;
        for (cfg, &(x, z)) in w
            .world_map
            .entity_configs
            .iter()
            .zip(w.world_map.entity_positions.iter())
        {
            eprintln!("      [comp-ent] ({},{}) {cfg:?}", x >> 7, z >> 7);
        }
        for (slot, &(x, z)) in &w.npcs.positions {
            eprintln!("      [comp-npc] slot {slot:?} at ({},{})", x >> 7, z >> 7);
        }
        eprintln!("      [comp-open] {:?}", self_opening_doors(session));
        for c in w.props.colliders.iter().filter(|c| c.solid) {
            eprintln!(
                "      [comp-prop] ({},{}) anchor {:?} moving {} interact {} P0[{:?}]",
                c.center.0 >> 7,
                c.center.1 >> 7,
                c.anchor,
                c.moving_box,
                c.interact,
                c.anchor
                    .and_then(|a| w.props.bank.props.get(&a))
                    .map(|p| p.record)
            );
        }
        for tz in 0..128 {
            let row: String = (0..128)
                .map(|tx| {
                    if (tx, tz) == (i32::from(goal.0), i32::from(goal.1)) {
                        'G'
                    } else if avoid.contains(&(tx, tz)) {
                        'A'
                    } else if tiles.contains(&(tx, tz)) {
                        'o'
                    } else if bl.0.contains_key(&(tx, tz)) {
                        'b'
                    } else if session
                        .host
                        .world
                        .field_tile_is_wall((tx * 128 + 64) as i16, (tz * 128 + 64) as i16)
                    {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect();
            eprintln!("      [comp {tz:3}] {row}");
        }
    }
    if !reached && seen.len() > UNREACHED_MIN_CELLS {
        let best = path.last().copied().unwrap_or(from);
        UNREACHED.with(|u| {
            let mut u = u.borrow_mut();
            if u.len() > 64 {
                u.clear();
            }
            u.insert(key, (seen, best));
        });
    }
    (!path.is_empty()).then_some(path)
}

/// Searches smaller than this are cheap enough to repeat.
const UNREACHED_MIN_CELLS: usize = 4_000;

/// What a failed plan's answer depends on: the scene, the goal, the avoid
/// set and the story flags (which open and shut doors and paint walls).
type PlanKey = (String, (i16, i16), u64, u64);

thread_local! {
    /// Failed plans: the cells the search reached and the one nearest the
    /// goal, by [`PlanKey`].
    static UNREACHED: std::cell::RefCell<HashMap<PlanKey, (HashSet<Cell>, Cell)>> =
        std::cell::RefCell::new(HashMap::new());
}

fn plan_key(session: &BootSession, goal: (i16, i16), avoid: &HashSet<(i32, i32)>) -> PlanKey {
    use std::hash::{Hash, Hasher};
    let mut a: Vec<&(i32, i32)> = avoid.iter().collect();
    a.sort();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    a.hash(&mut h);
    let mut f = std::collections::hash_map::DefaultHasher::new();
    session.host.world.flags.system_flags.hash(&mut f);
    (scene_name(session), goal, h.finish(), f.finish())
}

/// The A* behind [`plan_path`]: the route toward `goal`'s tile - or toward
/// `target` when one is given, a cell already known reachable - the cells
/// the search reached, and whether it reached its aim.
#[allow(clippy::type_complexity)]
fn plan_search(
    session: &BootSession,
    from: Cell,
    goal: (i16, i16),
    target: Option<Cell>,
    avoid: &HashSet<(i32, i32)>,
) -> Option<(Vec<Cell>, HashSet<Cell>, bool)> {
    let w = &session.host.world;
    let gw = tile_center(goal);
    // A goal tile whose centre is wall (a door band set in a wall strip)
    // is aimed at through its open part: the dispatch fires on entering the
    // tile, and the open corner says which side a player steps in from.
    // `teien`'s way down, band (42..43, 29), is wall but for its upper-left
    // sub-cell, so it is entered from the corridor above, never from below.
    let gc = target.unwrap_or_else(|| {
        let c = cell_of(gw.0, gw.1);
        let open = |c: Cell| {
            let (x, z) = cell_center(c);
            !w.field_tile_is_wall(x, z)
        };
        if open(c) {
            return c;
        }
        tile_cells(goal)
            .filter(|&t| open(t))
            .min_by_key(|t| (t.0 - c.0).abs() + (t.1 - c.1).abs())
            .unwrap_or(c)
    });
    let goal_tile = (i32::from(goal.0), i32::from(goal.1));
    let warps = teleports(session);
    let doors = object_doors(session);
    // The platforms under the engine's live arrival bracket: a plan that
    // starts on one (a re-plan just after a ride) may walk off it.
    let platforms: Vec<(i16, i16)> = w.props.arrival_exempt.iter().map(|&(_, p)| p).collect();
    let partners = door_partners(session);
    // Doors that open only for a player standing in their box: from a cell
    // in the box their leaves do not block (the press opens them).
    let boxed = boxed_doors(session);
    // Where a ride through the door at `contact` leaves the player able to
    // walk on: its landing, or - when the landing stands in reach of a
    // platform the record brackets - every cell just out of the platform's
    // reach that the landing's pocket opens onto, with the pocket distance.
    let arrivals = |contact: (i16, i16), land: Cell| -> Vec<(Cell, i32)> {
        let plats: Vec<(i16, i16)> = partners
            .get(&contact)
            .map(|v| v.iter().copied().filter(|&p| in_reach(land, p)).collect())
            .unwrap_or_default();
        if plats.is_empty() {
            if std::env::var_os("LEGAIA_FGL_PLAN_DEBUG").is_some() {
                eprintln!(
                    "      [plan] arrival {contact:?} at {land:?}: partners {:?}",
                    partners.get(&contact)
                );
            }
            return vec![(land, 0)];
        }
        let inside = |c: Cell| plats.iter().any(|&p| in_reach(c, p));
        // A ride's `MoveTo` sets the player on a tile centre, the low corner
        // of its lattice cell. A landing the walls seal there is carried
        // across the platform by the engine's walk-off
        // (`World::sealed_arrival_walk_off`), and the pocket starts where
        // that leaves the player.
        let corner = (land.0 * SUBCELL, land.1 * SUBCELL);
        let start = plats
            .iter()
            .find_map(|&p| w.sealed_arrival_walk_off(corner, p))
            .map_or(land, |(x, z)| cell_of(x, z));
        let mut dist: HashMap<Cell, i32> = HashMap::from([(start, 0)]);
        let mut q = VecDeque::from([start]);
        let mut exits = Vec::new();
        if !inside(start) {
            exits.push((start, 0));
            q.clear();
        }
        while let Some(c) = q.pop_front() {
            let d = dist[&c];
            if dist.len() > 400 {
                break;
            }
            let (ccx, ccz) = cell_center(c);
            for ((dx, dz), dir) in STEPS {
                let n = (c.0 + dx, c.1 + dz);
                if n.0 < 0 || n.1 < 0 || dist.contains_key(&n) || w.field_dir_blocked(ccx, ccz, dir)
                {
                    continue;
                }
                dist.insert(n, d + 1);
                if inside(n) {
                    q.push_back(n);
                } else {
                    exits.push((n, d + 1));
                }
            }
        }
        if std::env::var_os("LEGAIA_FGL_PLAN_DEBUG").is_some() {
            eprintln!(
                "      [plan] arrival {contact:?} at {land:?} platforms {plats:?}: pocket {} exits {exits:?}",
                dist.len()
            );
        }
        if exits.is_empty() {
            vec![(start, 0)]
        } else {
            exits
        }
    };
    // A closed door prop standing over a teleport tile, or beside an object
    // door's contact (the leaf of a two-part door), is solid until its touch
    // opens it; the route counts it open, and the follower's press into it
    // is the touch.
    // A touch-class door whose bind record runs `31 00` opens itself on the
    // touch wherever it stands, over a teleport or not (`jiji` P0[0], the
    // door across the corridor to the `map02` mouth at (66, 96)).
    let self_opening = self_opening_doors(session);
    let blockers = Blockers::build(w, |c| {
        !c.moving_box
            && !c.interact
            && (c.anchor.is_some_and(|a| self_opening.contains(&a))
                || warps.keys().any(|&(tx, tz)| {
                    let (lx, lz) = (tx * 128 - 64, tz * 128 - 64);
                    (lx..lx + 256).contains(&c.center.0) && (lz..lz + 256).contains(&c.center.1)
                    // A leaf anchored on the tile in front of the teleport
                    // (`dolk`'s inn stair door: anchor (76, 121), box
                    // centre z 121.3 tiles, teleport tile (76, 122)).
                    || c.anchor.is_some_and(|(ax, az)| {
                        (i32::from(ax) - tx).abs() + (i32::from(az) - tz).abs() <= 1
                    })
                })
                || doors.iter().any(|&((x, z), _)| {
                    (c.center.0 - i32::from(x)).abs() <= 128
                        && (c.center.1 - i32::from(z)).abs() <= 128
                }))
    });
    let mut s = Search {
        parent: HashMap::from([(from, from)]),
        g: HashMap::from([(from, 0)]),
        open: std::collections::BinaryHeap::new(),
        goal: gc,
    };
    s.open.push(std::cmp::Reverse((s.h(from), 0, from)));
    let mut best = from;
    while let Some(std::cmp::Reverse((_, gcur, cur))) = s.open.pop() {
        if s.g.get(&cur).is_some_and(|&v| v < gcur) {
            continue;
        }
        if s.parent.len() > MAX_PLAN_NODES {
            break;
        }
        if s.h(cur) < s.h(best) {
            best = cur;
        }
        if cur == gc {
            break;
        }
        let (cx, cz) = cell_center(cur);
        let here = dispatch_tile(cx, cz);
        let skip: Vec<(i16, i16)> = platforms
            .iter()
            .copied()
            .filter(|&p| in_reach(cur, p))
            .chain(
                boxed
                    .iter()
                    // One tile of slack: the leaf's own tile, which the
                    // player walks through once the press has opened it.
                    .filter(|((x0, z0, x1, z1), _)| {
                        (i32::from(*x0) - 1..=i32::from(*x1) + 1).contains(&here.0)
                            && (i32::from(*z0) - 1..=i32::from(*z1) + 1).contains(&here.1)
                    })
                    .flat_map(|(_, leaves)| leaves.iter().copied()),
            )
            .collect();
        for ((dx, dz), dir) in STEPS {
            let next = (cur.0 + dx, cur.1 + dz);
            if next.0 < 0 || next.1 < 0 {
                continue;
            }
            let (nx, nz) = cell_center(next);
            // Leaning into an object door's contact box posts its touch,
            // whose record teleports the player: the contact fires from the
            // same leading probe points that block the step, so a door set
            // in a wall is reached by pressing into the wall. The step
            // continues from the landing (the door cell is the waypoint the
            // follower presses toward).
            // The player comes to rest anywhere in its cell, not at its
            // centre, so the probes reach up to half a cell further than
            // the centre's (the lattice would otherwise miss a door whose
            // wall stops the player a few units short of the cell edge).
            let (sx, sz) = (dx * SUBCELL / 2, dz * SUBCELL / 2);
            let probes = ACTOR_PROBES[dir].map(|(px, pz)| {
                (
                    i32::from(cx) + i32::from(px) + i32::from(sx),
                    i32::from(cz) - i32::from(pz) + i32::from(sz),
                )
            });
            if let Some(&(contact, land)) =
                doors
                    .iter()
                    .filter(|(c, _)| !skip.contains(c))
                    .find(|((dx, dz), _)| {
                        probes.iter().any(|&(px, pz)| {
                            (px - i32::from(*dx)).abs() < TOUCH_BOX_HALF
                                && (pz - i32::from(*dz)).abs() < TOUCH_BOX_HALF
                        })
                    })
            {
                if land != next && !s.parent.contains_key(&next) {
                    for (to, d) in arrivals(contact, land) {
                        if to != next {
                            s.step(Some(next), cur, to, gcur + 2 + d);
                        }
                    }
                    PRESS_AT.with(|m| m.borrow_mut().insert(next, contact));
                    if std::env::var_os("LEGAIA_FGL_PLAN_DEBUG").is_some() {
                        eprintln!("      [plan] door {contact:?} from {cur:?} -> {land:?}");
                    }
                }
                continue;
            }
            if w.field_dir_blocked(cx, cz, dir) {
                continue;
            }
            let (nt, ct) = (dispatch_tile(nx, nz), dispatch_tile(cx, cz));
            // A teleport tile under a closed door prop reads actor-blocked
            // from every side: the prop is solid until the touch its body
            // posts runs the door's bind record (`31 00`), so pressing into
            // it opens the way. Only a static wall refuses the step.
            if nt != ct
                && nt != goal_tile
                && let Some(&land) = warps.get(&nt)
            {
                // Entering the teleport tile lands the player elsewhere; the
                // tile itself is a waypoint the follower steps into, never a
                // cell the walk continues from.
                if land != next && !s.parent.contains_key(&next) {
                    s.step(Some(next), cur, land, gcur + 2);
                    PRESS_AT.with(|m| {
                        m.borrow_mut()
                            .insert(next, tile_center((nt.0 as i16, nt.1 as i16)))
                    });
                }
                continue;
            }
            if blockers.dir_blocked_except(cx, cz, dir, &skip) {
                continue;
            }
            if nt != ct && avoid.contains(&nt) {
                continue;
            }
            s.step(None, cur, next, gcur + 1);
        }
    }
    PLAN_STATS.with(|p| {
        let (n, cells) = p.get();
        p.set((n + 1, cells + s.parent.len() as u64));
    });
    let reached = best == gc;
    let path = if best == from {
        Vec::new()
    } else {
        walk_back(&s.parent, from, best)?
    };
    Some((path, s.parent.into_keys().collect(), reached))
}

/// The route from `from` to `to` along `parent` links. A chain longer than
/// the map holds is a cycle: `None`, never an unbounded walk.
fn walk_back(parent: &HashMap<Cell, Cell>, from: Cell, to: Cell) -> Option<Vec<Cell>> {
    let mut path = vec![to];
    let mut c = to;
    while c != from {
        c = *parent.get(&c)?;
        if c != from {
            path.push(c);
        }
        if path.len() > parent.len() {
            return None;
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
        // A band whose record's spawn gates fail under the live flags is
        // floor: `kor3`'s P2[14] at (113, 5..6), the Cara and Grantes beat
        // into `koin1b`, spawns only while `0x426` is clear, and once it is
        // set the corridor to the door to `kor` runs over that band.
        let live = |rec: u8| {
            legaia_engine_core::man_field_scripts::partition2_record_gates(
                &mf,
                &man,
                usize::from(rec),
            )
            .is_none_or(|(c1, c2)| w.p2_record_gates_pass(&c1, &c2))
        };
        for s in overworld_portal_sites(&mf, &man, &triggers) {
            if s.scene_name != dest && live(s.record) {
                out.insert((i32::from(s.overworld_x), i32::from(s.overworld_z)));
            }
        }
        // A band that turned a split party's walker back is a wall to it.
        let turned: HashSet<u8> = TURNBACK.with(|t| {
            t.borrow()
                .iter()
                .filter(|(sc, _)| *sc == name)
                .map(|&(_, r)| r)
                .collect()
        });
        if w.dialog.three_actor_talk.is_some() {
            out.extend(
                triggers
                    .iter()
                    .filter(|t| t.gate == 1 && turned.contains(&t.record))
                    .map(|t| (i32::from(t.tile_x), i32::from(t.tile_z))),
            );
        }
    }
    out
}

thread_local! {
    /// `(scene, partition-2 record)` bands whose record ran to its release
    /// on a walk to somewhere else and left a three-actor split armed. The
    /// beat pass does not walk them again: `nilboa`'s split cutscene P2[0]
    /// runs from the entrance band P2[29] (`44 4E`), and walking its own
    /// band afterwards sent every member of the split across the map toward
    /// a beat already played - the Fire Ravine member re-dressing the
    /// boulder room (P2[26]) across the other member's way.
    static RAN_ON_WALK: std::cell::RefCell<HashSet<(String, u8)>> =
        std::cell::RefCell::new(HashSet::new());
    /// `(scene, partition-2 record)` bands that fired on a split party's
    /// walk and changed nothing - `nilboa`'s tunnel mouths, whose "go back
    /// outside?" No steps the walker back. While the split holds, the plan
    /// treats them as walls: the members' tunnels join only outside, and a
    /// route through a mouth loops on its refusal.
    static TURNBACK: std::cell::RefCell<HashSet<(String, u8)>> =
        std::cell::RefCell::new(HashSet::new());
}

/// Walk to a door toward `dest` with the pad only. `Ok(entered)` on a scene
/// change (which may not be `dest`; the caller checks).
fn pad_hop(session: &mut BootSession, graph: &DiscGraph, dest: &str) -> Result<String, String> {
    if let Ok(v) = std::env::var("LEGAIA_FGL_PROBE_AT")
        && let Some((scene, rest)) = v.split_once(':')
        && scene == scene_name(session)
    {
        let home = player_xz(session);
        for spec in rest.split(';') {
            if let Some((x, z)) = spec.split_once(',')
                && let (Ok(x), Ok(z)) = (x.parse::<i16>(), z.parse::<i16>())
            {
                restore_player_xz(session, (x, z));
                continue;
            }
            // A direction or a diagonal pair (`UL`, `DR`, ...).
            let mask = spec
                .chars()
                .map(|c| match c {
                    'U' => PadButton::Up.mask(),
                    'L' => PadButton::Left.mask(),
                    'D' => PadButton::Down.mask(),
                    'R' => PadButton::Right.mask(),
                    _ => 0,
                })
                .fold(0, |a, b| a | b);
            let from = player_xz(session);
            let mut trail = Vec::new();
            for f in 0..40 {
                session.host.world.set_pad(mask);
                let _ = session.tick();
                if f % 8 == 7 {
                    trail.push(player_xz(session));
                }
            }
            session.host.world.set_pad(0);
            let _ = run_while_moving(session, 600);
            eprintln!(
                "      [probe] {} {spec} from {from:?}: {trail:?} now {:?} exempt {:?} walls {:?} npcs near {:?}",
                scene_name(session),
                player_xz(session),
                session.host.world.props.arrival_exempt,
                {
                    let (x, z) = player_xz(session);
                    (0..4)
                        .map(|d| session.host.world.field_dir_blocked(x, z, d))
                        .collect::<Vec<_>>()
                },
                {
                    let (x, z) = player_xz(session);
                    session
                        .host
                        .world
                        .npcs
                        .positions
                        .iter()
                        .filter(|(_, p)| {
                            (i32::from(p.0) - i32::from(x)).abs() < 384
                                && (i32::from(p.1) - i32::from(z)).abs() < 384
                        })
                        .map(|(s, p)| (*s, *p))
                        .collect::<Vec<_>>()
                },
            );
            let (x, z) = player_xz(session);
            let w = &session.host.world;
            for c in w.props.colliders.iter().filter(|c| {
                (c.center.0 - i32::from(x)).abs() < 384 && (c.center.1 - i32::from(z)).abs() < 384
            }) {
                eprintln!(
                    "        [probe-prop] {:?} solid {} interact {} moving {} P0[{:?}]",
                    c.center,
                    c.solid,
                    c.interact,
                    c.moving_box,
                    c.anchor
                        .and_then(|a| w.props.bank.props.get(&a))
                        .map(|p| p.record)
                );
            }
        }
        restore_player_xz(session, home);
    }
    // A party too worn for the road rests first, where the scene offers it.
    if let Some(s) = pad_rest(session)? {
        return Ok(s);
    }
    let doors = match doors_to(session, graph, dest) {
        Ok(d) => d,
        // A hop no walk-on band carries is taken by talking, as a player does.
        Err(why) => return talk_hop(session, graph, dest).ok_or(why)?,
    };
    // Leaving a planned crossing: the doors whose record lands on the side
    // the plan needs, when any does.
    let doors = match WANT_LANDING.with(|w| *w.borrow()) {
        Some(want) => {
            let keep: Vec<Door> = doors
                .iter()
                .filter(|d| {
                    d.entry.is_some_and(|(ex, ez)| {
                        (i16::from(ex) - want.0).abs() + (i16::from(ez) - want.1).abs() <= 1
                    })
                })
                .cloned()
                .collect();
            if keep.is_empty() { doors } else { keep }
        }
        None => doors,
    };
    HOP_DEST.with(|d| *d.borrow_mut() = Some(dest.to_string()));
    let avoid = hazards(session, dest);
    if std::env::var_os("LEGAIA_FGL_WALK_DEBUG").is_some() {
        let tiles: Vec<(u8, u8)> = doors.iter().map(|d| d.tile).collect();
        eprintln!(
            "      [hop] {} -> {dest}: door tiles {tiles:?}",
            scene_name(session)
        );
        let w = &session.host.world;
        if let Some(t) = w.world_map.region_tracker.as_ref() {
            for r in t.table().active_regions() {
                eprintln!("      [region] {r:?}");
            }
        }
        let (px, pz) = player_xz(session);
        let me = dispatch_tile(px, pz);
        let warps = teleports(session);
        for tz in 0..128i32 {
            let row: String = (0..128i32)
                .map(|tx| {
                    if (tx, tz) == me {
                        return '@';
                    }
                    if tiles.contains(&(tx as u8, tz as u8)) {
                        return 'G';
                    }
                    if warps.contains_key(&(tx, tz)) {
                        return 'T';
                    }
                    let walls = [(32, 32), (96, 32), (32, 96), (96, 96)]
                        .iter()
                        .filter(|&&(dx, dz)| {
                            w.field_tile_is_wall((tx * 128 + dx) as i16, (tz * 128 + dz) as i16)
                        })
                        .count();
                    ['.', ',', '+', '*', '#'][walls]
                })
                .collect();
            if row.chars().any(|c| c != '#') {
                eprintln!("      [map {tz:3}] {row}");
            }
        }
    }
    let (sx, sz) = player_xz(session);
    let start = cell_of(sx, sz);
    // The door the lattice gets closest to.
    // One plan per door band, not per tile: the tiles of one band share a
    // reachability, and each plan can search the whole map.
    let me0 = tile_of(sx, sz);
    let mut tiles: Vec<(i16, i16)> = doors
        .iter()
        .map(|d| (i16::from(d.tile.0), i16::from(d.tile.1)))
        .collect();
    tiles.sort_by_key(|t| (t.0 - me0.0).abs() + (t.1 - me0.1).abs());
    // A band that turned the walk back under the live flags is tried last.
    let here_name = scene_name(session);
    let flags_now = flags_of_world(session);
    let refused = |t: &(i16, i16)| {
        REFUSED_DOORS.with(|r| {
            r.borrow().iter().any(|((s, d), f)| {
                *s == here_name && *f == flags_now && (d.0 - t.0).abs() + (d.1 - t.1).abs() <= 2
            })
        })
    };
    if tiles.iter().any(|t| !refused(t)) {
        tiles.retain(|t| !refused(t));
    }
    let mut picked: Vec<(i16, i16)> = Vec::new();
    for t in tiles {
        if picked.len() < 6
            && !picked
                .iter()
                .any(|p| (p.0 - t.0).abs() + (p.1 - t.1).abs() <= 2)
        {
            picked.push(t);
        }
    }
    let misses: Vec<((i16, i16), i32)> = picked
        .into_iter()
        .map(|g| {
            let miss = plan_path(session, start, g, &avoid)
                .and_then(|p| p.last().copied())
                .map_or(i32::MAX, |c| {
                    let t = tile_of(cell_center(c).0, cell_center(c).1);
                    i32::from((t.0 - g.0).abs() + (t.1 - g.1).abs())
                });
            (g, miss)
        })
        .collect();
    let me = tile_of(sx, sz);
    let far = |g: &(i16, i16)| i32::from((g.0 - me.0).abs() + (g.1 - me.1).abs());
    // On a crossing ([`cross_over`]) the way back is the reachable door
    // farthest from the one the player came in by.
    let goal = if FAR_DOOR.with(std::cell::Cell::get) {
        misses
            .iter()
            .filter(|(_, m)| *m <= DOOR_APPROACH_SLACK)
            .max_by_key(|(g, _)| far(g))
            .or_else(|| misses.iter().min_by_key(|(_, m)| *m))
    } else {
        misses.iter().min_by_key(|(_, m)| *m)
    }
    .map(|(g, _)| *g)
    .expect("doors_to is non-empty");
    LAST_HOP_DOOR.with(|l| *l.borrow_mut() = Some((scene_name(session), goal)));
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
        if !door_approachable(session, t, goal) {
            return Err(format!(
                "no walkable path: the start's walk component ends {} tiles short of door {goal:?} (closest tile {t:?})",
                dist(t)
            ));
        }
    }
    // On the walk, stay off the live walk-on bands as well as the wrong
    // doors: a band's record is a story beat (or a scripted game over) the
    // route does not want.
    let mut walk_avoid = pad_avoid(session, Some(goal));
    for d in &doors {
        walk_avoid.remove(&(i32::from(d.tile.0), i32::from(d.tile.1)));
    }
    // A band whose record clears the door record's own C1 latch is the way
    // in, not a beat to step around: `ropeway`'s gate to `ropeway2`
    // (P2[30], C1 `0x514`) is reached through the station door onto
    // (29, 35), whose P2[25] clears `0x514`; the corridor band P2[24] at
    // (27, 36) sets it, and stays avoided.
    let unlatch = unlatching_bands(session, goal);
    if std::env::var_os("LEGAIA_FGL_WALK_DEBUG").is_some() {
        let mut near: Vec<_> = walk_avoid
            .iter()
            .filter(|t| (t.0 - i32::from(goal.0)).abs() + (t.1 - i32::from(goal.1)).abs() <= 6)
            .collect();
        near.sort();
        eprintln!("      [hop] goal {goal:?} unlatching {unlatch:?}; avoided near {near:?}");
        let mut av = walk_avoid.clone();
        for t in &unlatch {
            av.remove(t);
        }
        let probe_from = tile_center((goal.0, goal.1 - 3));
        let p = plan_path(session, cell_of(probe_from.0, probe_from.1), goal, &av);
        eprintln!(
            "      [hop] plan from {:?}: {:?}",
            (goal.0, goal.1 - 3),
            p.map(|p| p
                .iter()
                .map(|&c| tile_of(cell_center(c).0, cell_center(c).1))
                .collect::<Vec<_>>())
        );
    }
    for t in &unlatch {
        walk_avoid.remove(t);
    }
    // The unlatching band is crossed first, as a player crossing it on the
    // way in does: P2[25] also re-opens the wall cells of the door frame at
    // (29, 35) (`4C 70`), so the lattice sees no way to the gate until it
    // has run.
    if let Some(&(bx, bz)) = unlatch.iter().min_by_key(|t| {
        let me = player_xz(session);
        let me = tile_of(me.0, me.1);
        (t.0 - i32::from(me.0)).abs() + (t.1 - i32::from(me.1)).abs()
    }) && let (Ok(bx), Ok(bz)) = (u8::try_from(bx), u8::try_from(bz))
        && let Ok(Walk::Entered(s)) = pad_step_onto(session, (bx, bz))
    {
        return Ok(s);
    }
    // A walk that stalls against the door's own frame is a door this side
    // cannot enter (`concnow`'s door to `conc2` at (15, 41) is entered from
    // the north; from (15, 42) its threshold is a wall): like a refusal, the
    // scene's other door to `dest` is tried.
    let at_frame = |e: &str| {
        e.contains("pad walk stalled")
            && e.split(" tiles short")
                .next()
                .and_then(|h| h.rsplit(' ').next())
                .and_then(|n| n.parse::<u32>().ok())
                .is_some_and(|n| n <= 2)
    };
    let walked = match pad_walk(session, goal, &walk_avoid, 0) {
        Err(e)
            if (e.contains("changed nothing") || at_frame(&e))
                && scene_name(session) == here_name =>
        {
            let first = REFUSED_DOORS.with(|r| {
                r.borrow_mut()
                    .insert((here_name.clone(), goal), flags_of_world(session))
                    .is_none()
            });
            // Another band of the scene leads to `dest`: walk to that one.
            if first
                && doors.iter().any(|d| {
                    let t = (i16::from(d.tile.0), i16::from(d.tile.1));
                    !refused(&t) && (t.0 - goal.0).abs() + (t.1 - goal.1).abs() > 2
                })
            {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!("    [hop] door {goal:?} to {dest} refused ({e}); trying another");
                }
                return pad_hop(session, graph, dest);
            }
            return Err(e);
        }
        other => other?,
    };
    match walked {
        Walk::Entered(s) => Ok(s),
        Walk::Arrived => {
            // On the band: keep pressing into the door while its record
            // runs up to the scene change.
            let door = tile_center(goal);
            if let Some(s) = pad_lean(session, move |_| Some(door), 30, |s| !released(s)) {
                return Ok(s);
            }
            let first = match run_while_moving(session, DEEP_EXIT_TICKS) {
                Run::Entered(s) => return Ok(s),
                other => other,
            };
            // Standing on the band and nothing left: the crossing was
            // consumed. The dispatch drops a tile change made while a script
            // holds the player (`0x801D214C`), and the step in can be the
            // very press that touches a neighbouring prop: `ropeway`'s gate
            // to `ropeway2` at (29, 36) sits beside the P0[2] door, whose
            // touch from the west runs (and fails) its box test on the
            // frame the player crosses. A player steps off and back on;
            // so does the hand, from each side in turn.
            if matches!(first, Run::Released) {
                for side in beside(session, goal) {
                    let off = pad_avoid(session, Some(goal));
                    if let Ok(Walk::Entered(s)) = pad_walk(session, side, &off, 0) {
                        return Ok(s);
                    }
                    if tile_of(player_xz(session).0, player_xz(session).1) != side {
                        continue;
                    }
                    let onto = pad_avoid(session, Some(goal));
                    match pad_walk(session, goal, &onto, 0) {
                        Ok(Walk::Entered(s)) => return Ok(s),
                        Ok(Walk::Arrived) => {
                            if let Run::Entered(s) = run_while_moving(session, DEEP_EXIT_TICKS) {
                                return Ok(s);
                            }
                        }
                        Err(_) => {}
                    }
                }
            }
            Err(format!("on door {goal:?} to {dest}: {first:?}"))
        }
    }
}

thread_local! {
    /// `(scene, formation)` of every boss fight a stager beat armed this
    /// segment. A stager's battle fires on the player's next field step, so
    /// it interrupts whatever walk comes next; that walk must fight it, not
    /// flee it as it would a random encounter.
    static STAGED_FIGHTS: std::cell::RefCell<BTreeSet<(String, Option<u16>)>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

thread_local! {
    /// Set while the pad tier drives: every beat is then played with pad
    /// input only - walked to, faced and pressed - instead of seated.
    static PAD_HAND: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn pad_hand() -> bool {
    PAD_HAND.with(std::cell::Cell::get)
}

/// Frames the pad tier may spend on one segment. Walking every beat of a
/// scene is slow, and a segment the pad hand cannot finish should read as a
/// stall rather than hold the whole run; a real play of one milestone-to-
/// milestone stretch is well inside it.
const PAD_SEGMENT_FRAMES: u64 = 216_000;

thread_local! {
    /// `BootSession::frames` at which the current pad segment's budget runs
    /// out.
    static PAD_DEADLINE: std::cell::Cell<u64> = const { std::cell::Cell::new(u64::MAX) };
}

/// Frames of battle a pad segment may spend beyond [`PAD_SEGMENT_FRAMES`].
/// The budget is for walking; a fight is already bounded by
/// [`BATTLE_TICKS_CAP`], and the pad hand's fights are slow - `nilboa`'s
/// Three Tunnels stage a lone member against a 9500..12000 HP boss in each
/// tunnel (`F29`..`F31`), and those three fights alone run past the walking
/// budget.
const PAD_BATTLE_ALLOWANCE: u64 = 2 * PAD_SEGMENT_FRAMES;

thread_local! {
    /// What is left of [`PAD_BATTLE_ALLOWANCE`] in the current segment.
    static PAD_BATTLE_LEFT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// `Err` once the pad segment's frame budget is spent.
fn pad_budget(session: &BootSession) -> Result<(), String> {
    if pad_hand() && session.frames >= PAD_DEADLINE.with(std::cell::Cell::get) {
        Err(format!("pad frame budget ({PAD_SEGMENT_FRAMES}) spent"))
    } else {
        Ok(())
    }
}

/// Where a [`pad_walk`] ended.
#[derive(Debug)]
enum Walk {
    /// The player stands within `within` tiles of the goal.
    Arrived,
    /// A scene change landed on the way (the goal tile's own record, or a
    /// scripted sequence the walk set off).
    Entered(String),
}

/// Every tile of the loaded scene a pad walk should not **enter** on its way
/// somewhere else: the walk-on door bands, the kind-0 intra-scene
/// teleports, and every gate-1 walk-on band whose partition-2 record the
/// live flags let spawn - stepping onto one runs its record (or arms a
/// warp) instead of the beat under test. A band whose record's story gates
/// shut it is inert and stays walkable: town gates and bridges are often
/// lined with them. `keep` is exempt - it is the goal.
/// Whether partition-2 record `rec` is an **inert band**: its body only
/// configures the camera or the view (`retona` P2[0], `4C 38` then a park,
/// laid across every pass off Mt. Letona's summit). Crossing one moves
/// nobody, writes no story flag and changes no scene, so the walk need not
/// step around it; avoiding it sealed the summit off from its way down.
fn inert_band(mf: &legaia_asset::man_section::ManFile, man: &[u8], rec: usize) -> bool {
    use legaia_asset::field_disasm::{InsnInfo, LinearWalker};
    use legaia_engine_core::man_field_scripts::partition_record_span;
    let Some((start, pc0, len)) = partition_record_span(mf, man, 2, rec) else {
        return false;
    };
    LinearWalker::new(&man[start..start + len], pc0).all(|i| {
        i.is_ok_and(|i| {
            i.extended.is_none()
                && match i.info {
                    InsnInfo::Nop
                    | InsnInfo::JmpRel { .. }
                    | InsnInfo::Camera { .. }
                    | InsnInfo::CamCfg { .. }
                    | InsnInfo::ViewWindow { .. }
                    | InsnInfo::Bgm { .. } => true,
                    InsnInfo::MenuCtrl { op0, .. } => (0x30..=0x3F).contains(&op0),
                    _ => false,
                }
        })
    })
}

fn pad_avoid(session: &BootSession, keep: Option<(i16, i16)>) -> HashSet<(i32, i32)> {
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let mut out = hazards(session, "");
    if session.host.world.mode == SceneMode::Field
        && let Some((mf, man, triggers)) = scene_man_and_triggers(session)
    {
        let w = &session.host.world;
        for t in triggers.iter().filter(|t| t.gate == 1) {
            let live = partition2_record_gates(&mf, &man, usize::from(t.record))
                .is_none_or(|(c1, c2)| w.p2_record_gates_pass(&c1, &c2));
            if live && !inert_band(&mf, &man, usize::from(t.record)) {
                out.insert((i32::from(t.tile_x), i32::from(t.tile_z)));
            }
        }
        let index = &session.host.index;
        if let Ok(scene) = Scene::load(index, &scene_name(session))
            && let Ok((p, f)) = scene.field_intra_scene_teleports(index)
        {
            out.extend(
                p.iter()
                    .chain(f.iter())
                    .map(|t| (i32::from(t.tile_x), i32::from(t.tile_z))),
            );
        }
    }
    if let Some(k) = keep {
        out.remove(&(i32::from(k.0), i32::from(k.1)));
    }
    out
}

/// The gate-1 walk-on tiles of the loaded field scene whose partition-2
/// record cleanly clears a C1 latch of the walk-on record on `door` - the
/// bands that open the way to it.
fn unlatching_bands(session: &BootSession, door: (i16, i16)) -> Vec<(i32, i32)> {
    use legaia_engine_core::man_field_scripts::{
        FlagBank, partition2_record_gates, walk_partition_gflag_sites,
    };
    if session.host.world.mode != SceneMode::Field {
        return Vec::new();
    }
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return Vec::new();
    };
    let latches: Vec<u16> = triggers
        .iter()
        .filter(|t| t.gate == 1 && (i16::from(t.tile_x), i16::from(t.tile_z)) == door)
        .filter_map(|t| partition2_record_gates(&mf, &man, usize::from(t.record)))
        .flat_map(|(c1, _)| c1)
        .collect();
    if latches.is_empty() {
        return Vec::new();
    }
    let clears: HashSet<usize> = walk_partition_gflag_sites(&mf, &man, 2)
        .iter()
        .filter(|s| {
            s.bank == FlagBank::System
                && s.kind == FlagKind::Clear
                && s.clean
                && latches.contains(&s.flag)
        })
        .map(|s| s.record)
        .collect();
    triggers
        .iter()
        .filter(|t| t.gate == 1 && clears.contains(&usize::from(t.record)))
        .map(|t| (i32::from(t.tile_x), i32::from(t.tile_z)))
        .collect()
}

/// [`plan_path`] around `avoid`; when that cannot reach `goal`, around the
/// door bands alone (a live walk-on band then gets crossed, as a player
/// crosses one that stands in the only way through).
fn plan_around(
    session: &BootSession,
    from: Cell,
    goal: (i16, i16),
    avoid: &HashSet<(i32, i32)>,
    doors: &HashSet<(i32, i32)>,
) -> Vec<Cell> {
    let gc = {
        let g = tile_center(goal);
        cell_of(g.0, g.1)
    };
    let miss = |p: &[Cell]| {
        p.last().map_or(i32::MAX, |c| {
            i32::from((c.0 - gc.0).abs() + (c.1 - gc.1).abs())
        })
    };
    let strict = plan_path(session, from, goal, avoid).unwrap_or_default();
    if miss(&strict) <= i32::from(TILE / SUBCELL) {
        return strict;
    }
    let loose = plan_path(session, from, goal, doors).unwrap_or_default();
    if miss(&loose) < miss(&strict) {
        loose
    } else {
        strict
    }
}

/// A route out of a wedge: the player can come to rest in the notch of a
/// stair-stepped diagonal wall, where every one of the lattice's four
/// probe-checked steps reads blocked although the free movement that got
/// it there can leave. Head for the nearest cell a step can leave; the
/// route is planned again from there.
fn unwedge(session: &BootSession, from: Cell) -> Vec<Cell> {
    let w = &session.host.world;
    let open = |c: Cell| {
        let (x, z) = cell_center(c);
        (0..4).any(|d| !w.field_dir_blocked(x, z, d))
    };
    // Only a real wedge: a start that can step somewhere but whose route
    // simply ends here (the goal is out of reach) is not one.
    if open(from) {
        return Vec::new();
    }
    for r in 1..=3i16 {
        for dz in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dz.abs()) != r {
                    continue;
                }
                let c = (from.0 + dx, from.1 + dz);
                let (x, z) = cell_center(c);
                if !w.field_tile_is_wall(x, z) && open(c) {
                    return vec![c];
                }
            }
        }
    }
    Vec::new()
}

/// The overworld's encounter step is a change of 128-unit tile, and a change
/// of one tile on **both** axes at once is one step, not two
/// (`FUN_801D9E1C` caches the tile and reads a region only when the new one
/// differs by at most one on each axis - `slti 0x2` at `0x801D9EF0` /
/// `0x801D9F08`; engine `region_encounter::is_region_step`). A player who
/// walks a diagonal through tile corners therefore drains the encounter
/// counter about half as fast as one who walks the staircase a four-way
/// lattice plans. On a crossing worn down to a lone member that is the
/// difference between two fights and three.
///
/// This is the tile route for that walk: eight-connected over tiles whose
/// four wall sub-cells are all open (a diagonal also needs both tiles it
/// cuts the corner of), every move costing one step, ending on the open
/// tile nearest `goal`. The lattice planner finishes from there.
fn overworld_tile_route(
    session: &BootSession,
    goal: (i16, i16),
    avoid: &HashSet<(i32, i32)>,
) -> Vec<(i32, i32)> {
    let w = &session.host.world;
    let open = |t: (i32, i32)| {
        (0..128).contains(&t.0)
            && (0..128).contains(&t.1)
            && !avoid.contains(&t)
            && [(32, 32), (96, 32), (32, 96), (96, 96)]
                .iter()
                .all(|&(dx, dz)| {
                    !w.field_tile_is_wall((t.0 * 128 + dx) as i16, (t.1 * 128 + dz) as i16)
                })
    };
    let (px, pz) = player_xz(session);
    let start = dispatch_tile(px, pz);
    if !open(start) {
        return Vec::new();
    }
    let g = (i32::from(goal.0), i32::from(goal.1));
    let far = |t: (i32, i32)| (t.0 - g.0).abs() + (t.1 - g.1).abs();
    let mut parent: HashMap<(i32, i32), (i32, i32)> = HashMap::from([(start, start)]);
    let mut q = VecDeque::from([start]);
    let mut best = start;
    while let Some(t) = q.pop_front() {
        if far(t) < far(best) {
            best = t;
        }
        for dz in -1..=1 {
            for dx in -1..=1 {
                let n = (t.0 + dx, t.1 + dz);
                if (dx, dz) == (0, 0) || parent.contains_key(&n) || !open(n) {
                    continue;
                }
                if dx != 0 && dz != 0 && !(open((t.0 + dx, t.1)) && open((t.0, t.1 + dz))) {
                    continue;
                }
                parent.insert(n, t);
                q.push_back(n);
            }
        }
    }
    let mut route = Vec::new();
    let mut t = best;
    while t != start {
        route.push(t);
        t = parent[&t];
    }
    route.reverse();
    route
}

/// The pad for one tick of an [`overworld_tile_route`] move from the tile the
/// player stands on to the adjacent `next`. A diagonal move is held only once
/// both axes are the same number of 2-unit sub-steps from their tile edge, so
/// both edges fall in one sub-step and the reader sees one step; until then
/// the axis further from its edge is walked alone - in single sub-steps when
/// close, by tapping (a released pad zeroes the overworld walk carry, so the
/// next pressed tick commits exactly one sub-step). `None` when the camera
/// gives no pad for the move's world direction.
fn overworld_corner_pad(session: &BootSession, next: (i32, i32), tapped: bool) -> Option<u16> {
    let (px, pz) = player_xz(session);
    let t = dispatch_tile(px, pz);
    let (dx, dz) = (next.0 - t.0, next.1 - t.1);
    let need = |p: i16, tile: i32, d: i32| -> i32 {
        if d > 0 {
            128 * (tile + 1) - i32::from(p)
        } else {
            i32::from(p) - 128 * tile + 1
        }
    };
    let pad_for = |sx: i32, sz: i32| -> Option<u16> {
        let pad = pad_for_step(session, sx as i16, sz as i16);
        let az = session
            .host
            .world
            .world_map
            .ctrl
            .as_ref()
            .map_or(0, |c| c.azimuth);
        let sx_pad = i32::from(pad & PadButton::Right.mask() != 0)
            - i32::from(pad & PadButton::Left.mask() != 0);
        let sy_pad = i32::from(pad & PadButton::Up.mask() != 0)
            - i32::from(pad & PadButton::Down.mask() != 0);
        let b = world_map_camera_relative_bits(az, sx_pad, sy_pad);
        let got = (
            i32::from(b & 0x2000 != 0) - i32::from(b & 0x8000 != 0),
            i32::from(b & 0x1000 != 0) - i32::from(b & 0x4000 != 0),
        );
        (got == (sx, sz)).then_some(pad)
    };
    if dx == 0 || dz == 0 {
        return pad_for(dx, dz);
    }
    let sx = (need(px, t.0, dx) + 1) / 2;
    let sz = (need(pz, t.1, dz) + 1) / 2;
    let (axis, gap) = match sx.cmp(&sz) {
        std::cmp::Ordering::Equal => return pad_for(dx, dz),
        std::cmp::Ordering::Greater => ((dx, 0), sx - sz),
        std::cmp::Ordering::Less => ((0, dz), sz - sx),
    };
    if gap < 3 && tapped {
        Some(0)
    } else {
        pad_for(axis.0, axis.1)
    }
}

/// Walk with the pad until the player's dispatch tile is within `within`
/// tiles of `goal`. Random encounters on the way are fled; a scripted
/// sequence the walk sets off (a band's record, a talk) is paged through.
fn pad_walk(
    session: &mut BootSession,
    goal: (i16, i16),
    avoid: &HashSet<(i32, i32)>,
    within: i32,
) -> Result<Walk, String> {
    let dist = |t: (i32, i32)| (t.0 - i32::from(goal.0)).abs() + (t.1 - i32::from(goal.1)).abs();
    let here = |s: &BootSession| {
        let (x, z) = player_xz(s);
        dispatch_tile(x, z)
    };
    if dist(here(session)) <= within {
        return Ok(Walk::Arrived);
    }
    // A player low on HP heals before setting out, not after the next
    // encounter has already rolled - or, with nothing to heal with, burns an
    // Incense so the next encounter never rolls. A travel leg's encounter
    // can open on a back attack, which takes a round before the party acts
    // (`vozz`'s F4: ~120 to each member, then a caught Run), and a three-
    // monster group can catch Run two or three rounds running (`map01`'s F5
    // took ~240 from Noa over three caught rounds), so the walk sets out
    // above four fifths.
    pad_field_heal(session, 800);
    pad_field_repel(session, 500);
    if std::env::var_os("LEGAIA_FGL_WALK_DEBUG").is_some() {
        let w = &session.host.world;
        eprintln!(
            "      [walk] {} from {:?} to {goal:?}: teleports {:?}; object doors {:?}; walk-touch {:?}",
            scene_name(session),
            here(session),
            teleports(session),
            object_doors(session),
            w.props
                .walk_touch
                .iter()
                .map(|(s, (c, e))| format!("{s}@{c:?}:{e:?}"))
                .collect::<Vec<_>>()
        );
        // The scene's tile triggers: `(x,z)g<gate>r<record>`, gate 1 the
        // partition-2 walk-on bands, gate 0 the partition-0 object binds.
        if let Some((_, _, triggers)) = scene_man_and_triggers(session) {
            let list: Vec<String> = triggers
                .iter()
                .map(|t| format!("({},{})g{}r{}", t.tile_x, t.tile_z, t.gate, t.record))
                .collect();
            eprintln!("      [triggers] {}", list.join(" "));
        }
        for (s, ((x, z), _)) in &w.props.walk_touch {
            let near: Vec<String> = w
                .props
                .colliders
                .iter()
                .filter(|c| {
                    (c.center.0 - i32::from(*x)).abs() < 256
                        && (c.center.1 - i32::from(*z)).abs() < 256
                })
                .map(|c| {
                    format!(
                        "{:?} solid {} interact {} moving {} anchor {:?}",
                        c.center, c.solid, c.interact, c.moving_box, c.anchor
                    )
                })
                .collect();
            eprintln!("      [door] {s}@({x},{z}) colliders {near:?}");
        }
    }
    if std::env::var_os("LEGAIA_FGL_WALK_DEBUG").is_some() {
        let w = &session.host.world;
        for &(tx, tz) in teleports(session).keys() {
            // For each side: the cell just outside the tile's edge, stepping in.
            let c = |x: i32, z: i32| ((x * 128 + 64) as i16, (z * 128 + 64) as i16);
            let (mx, mz) = c(tx, tz);
            let sides = [
                ((mx, mz + 96), 0usize),
                ((mx + 96, mz), 1),
                ((mx, mz - 96), 2),
                ((mx - 96, mz), 3),
            ];
            let s: Vec<String> = sides
                .iter()
                .map(|&((x, z), d)| {
                    format!(
                        "{}{}",
                        if w.field_dir_blocked(x, z, d) {
                            "W"
                        } else {
                            "."
                        },
                        if w.field_actor_dir_blocked(x, z, d) {
                            "A"
                        } else {
                            "."
                        }
                    )
                })
                .collect();
            eprintln!("      [warp] ({tx},{tz}) entry from z+/x+/z-/x-: {s:?}");
        }
    }
    let walking_mode = session.host.world.mode;
    let mut doors = hazards(session, "");
    doors.remove(&(i32::from(goal.0), i32::from(goal.1)));
    // A goal the lattice cannot get near is not walked at all: wandering
    // toward it only rolls encounters (a placement parked off the map, a
    // room behind a door the flags keep shut).
    {
        let (x, z) = player_xz(session);
        let plan = plan_around(session, cell_of(x, z), goal, avoid, &doors);
        if let Some(&end) = plan.last() {
            let (ex, ez) = cell_center(end);
            let miss = dist(dispatch_tile(ex, ez));
            if miss > within + DOOR_APPROACH_SLACK {
                return Err(format!(
                    "no walkable path: the walk component ends {miss} tiles short of {goal:?} (closest {:?})",
                    dispatch_tile(ex, ez)
                ));
            }
        }
    }
    let mut best = dist(here(session));
    let mut visited: HashSet<Cell> = HashSet::new();
    let mut since = 0u32;
    let mut planned_from = None;
    let mut scripted_next = session.host.world.encounters.scripted_formation_pending;
    let mut path = Vec::new();
    // The overworld's corner-crossing walk ([`overworld_tile_route`]).
    let mut corner_route: Vec<(i32, i32)> = Vec::new();
    let (mut corner_planned, mut corner_failed, mut corner_tapped) = (false, false, false);
    let mut corner_last: Option<(i16, i16)> = None;
    let mut corner_stuck = 0u32;
    let mut traced_tile = None;
    // Where the player stood when the previous walk frame was pressed.
    let mut pressed_at: Option<(i16, i16)> = None;
    let mut tap_owed = false;
    // Walk-time script firings per park site, with the flags they left.
    let mut refires: HashMap<String, (u32, BTreeSet<u16>)> = HashMap::new();
    for _ in 0..PAD_LEG_FRAMES {
        pad_budget(session)?;
        // Per overworld tile: the encounter step counter it left behind.
        if std::env::var_os("LEGAIA_FGL_WALK_DEBUG").is_some()
            && session.host.world.mode == SceneMode::WorldMap
        {
            let (x, z) = player_xz(session);
            let t = dispatch_tile(x, z);
            if traced_tile != Some(t) {
                traced_tile = Some(t);
                eprintln!(
                    "      [step] tile {t:?} counter {}",
                    session.host.world.encounters.step_counter
                );
            }
        }
        if session.host.world.mode == SceneMode::Battle {
            let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
            let f0 = flags_of_world(session);
            let formation = session
                .host
                .world
                .battle
                .active_formation
                .as_ref()
                .map(|f| f.formation_id);
            scripted_next |= formation.is_some()
                && STAGED_FIGHTS.with(|s| s.borrow().contains(&(scene_name(session), formation)));
            if trace {
                eprintln!(
                    "    [battle] {} ({}) at start: {}",
                    scene_name(session),
                    if scripted_next {
                        "scripted, fought"
                    } else {
                        "random, fled"
                    },
                    battle_snapshot(session)
                );
            }
            // A random encounter is fled; a fight a script installed (a
            // boss stager's, which fires on the next field step) is fought.
            FLEE_ENCOUNTERS.with(|f| f.set(!scripted_next));
            let r = drain_battle(session);
            FLEE_ENCOUNTERS.with(|f| f.set(false));
            scripted_next = false;
            if trace {
                eprintln!(
                    "    [battle] ended: {r:?}; flags +{:?}",
                    flags_of_world(session)
                        .difference(&f0)
                        .map(|f| format!("0x{f:03X}"))
                        .collect::<Vec<_>>()
                );
            }
            if let Some(r) = r {
                return Err(format!("battle on the walk to {goal:?}: {r:?}"));
            }
            pad_field_heal(session, 500);
            pad_field_repel(session, 500);
            planned_from = None;
            path.clear();
            since = 0;
            continue;
        }
        if close_scripted_menu(session) {
            continue;
        }
        if session.host.world.mode != walking_mode {
            return Err(format!("mode changed to {:?}", session.host.world.mode));
        }
        scripted_next = session.host.world.encounters.scripted_formation_pending;
        let (wx, wz) = player_xz(session);
        let cell = cell_of(wx, wz);
        if planned_from != Some(cell) {
            // Follow the planned route while the player stays on it; plan
            // again only off it (a slide, a teleport, a script's move).
            if let Some(i) = path.iter().position(|&c| c == cell) {
                // On a teleport waypoint the landing is next: keep pressing
                // from the waypoint until the jump happens.
                let jump = path.get(i + 1).is_some_and(|&n| !adjacent(cell, n));
                path.drain(..if jump { i } else { i + 1 });
            } else if let Some(i) = path
                .iter()
                .take(8)
                .position(|&c| (c.0 - cell.0).abs() + (c.1 - cell.1).abs() <= 2)
            {
                // A slide a cell or two off the route rejoins it rather than
                // re-planning the whole map.
                path.drain(..i);
            } else {
                path = plan_around(session, cell, goal, avoid, &doors);
                if path.is_empty() {
                    path = unwedge(session, cell);
                }
                // A route over a band that stages a fight walks into a boss:
                // a party under two thirds is topped up first, as before a
                // stager (`taiku` P2[27], the F9 boss on the way out, met
                // at 56% after a fled encounter).
                if !path.is_empty() && party_hp_permille(session) < 667 {
                    let fights = fight_band_tiles(session);
                    if path.iter().any(|&c| {
                        let (x, z) = cell_center(c);
                        fights.contains(&dispatch_tile(x, z))
                    }) && pad_field_heal(session, 900) > 0
                    {
                        planned_from = None;
                        path.clear();
                        continue;
                    }
                }
            }
            planned_from = Some(cell);
        }
        let (tx, tz) = match path.first() {
            Some(&c) if c == cell => PRESS_AT
                .with(|m| m.borrow().get(&c).copied())
                .unwrap_or(cell_center(c)),
            Some(&c) => cell_center(c),
            None => tile_center(goal),
        };
        // A cross-axis offset of a few units is the walk's own overshoot,
        // not a step to take: chasing it flips the diagonal every frame, and
        // along a terrace edge the side-step toward the drop is a ledge hop
        // (`tunnela`'s corridor at (89, 79) hops the party down to (89, 80),
        // a one-way drop the walk then cannot climb back).
        let (dx, dz) = (tx - wx, tz - wz);
        let lean = |d: i16, other: i16| {
            if d.abs() <= WALK_DEADBAND && other.abs() > WALK_DEADBAND {
                0
            } else {
                d.signum()
            }
        };
        let mut pad = pad_for_step(session, lean(dx, dz), lean(dz, dx));
        if walking_mode == SceneMode::WorldMap && !corner_failed {
            let t = dispatch_tile(wx, wz);
            if corner_route.is_empty() && !corner_planned {
                corner_route = overworld_tile_route(session, goal, &doors);
                corner_planned = true;
            }
            if let Some(i) = corner_route.iter().position(|&r| r == t) {
                corner_route.drain(..=i);
            }
            match corner_route.first() {
                Some(&n) if (n.0 - t.0).abs() <= 1 && (n.1 - t.1).abs() <= 1 => {
                    match overworld_corner_pad(session, n, corner_tapped) {
                        Some(p) => {
                            corner_tapped = p != 0;
                            pad = p;
                        }
                        None => corner_failed = true,
                    }
                }
                // Off the route (a slide, a script's seat): plan it again
                // from here.
                Some(_) => {
                    corner_route.clear();
                    corner_planned = false;
                }
                None => {}
            }
            // Pressing without moving: the tile route met something the
            // wall bits do not show; the lattice takes over.
            if pad != 0 && corner_last == Some((wx, wz)) {
                corner_stuck += 1;
                if corner_stuck > 8 {
                    corner_failed = true;
                    corner_route.clear();
                }
            } else {
                corner_stuck = 0;
            }
            corner_last = Some((wx, wz));
        }
        // Held against something that will not give: a player tries the
        // action button (a door that opens on a press, not on contact), and
        // the route is planned afresh (an NPC walked into it).
        //
        // `since` also counts a frame that moved without improving (a run
        // step that stays inside one cell), and a tap there drops the held
        // direction for a frame - no player stops to press while still
        // walking. In a field the tap waits for a frame the player did not
        // move: standing still for one frame of every few cost a timed
        // script its window (`jouind`'s switch pair is 50 vsyncs of free walk
        // apart).
        let moved = pressed_at.is_some_and(|p| p != (wx, wz));
        if since == 0 {
            tap_owed = false;
        }
        if since > 0 && since.is_multiple_of(60) {
            path.clear();
            planned_from = None;
            pad = 0;
            tap_owed = false;
        } else {
            tap_owed |= since % 60 == 1;
            if tap_owed && !(moved && walking_mode == SceneMode::Field) {
                pad = PadButton::Cross.mask();
                tap_owed = false;
            }
        }
        pressed_at = Some((wx, wz));
        session.host.world.set_pad(pad);
        let jump_from = player_xz(session);
        match session.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Ok(Walk::Entered(name)),
            Ok(_) => {}
            Err(e) => return Err(format!("tick: {e:#}")),
        }
        if session.host.world.mode == SceneMode::Battle {
            continue;
        }
        if std::env::var_os("LEGAIA_FGL_JUMP_DEBUG").is_some() {
            let to = player_xz(session);
            if (i32::from(to.0) - i32::from(jump_from.0)).abs()
                + (i32::from(to.1) - i32::from(jump_from.1)).abs()
                > 64
            {
                eprintln!(
                    "      [jump] {jump_from:?} -> {to:?} pad {pad:#06x} holder {} bracket {:?}",
                    holder(session),
                    session.host.world.props.arrival_exempt
                );
            }
        }
        let w = &session.host.world;
        if w.cutscene_timeline_active() || w.dialogue_owns_input() || w.active_fmv().is_some() {
            let site = format!("{} at {}", holder(session), park_site(session));
            let fired_record = timeline_p2_record(session);
            let rec_label = fired_record.map_or("-".to_string(), |r| format!("P2[{r}]"));
            // A script that changes nothing - an examined prop with nothing
            // to say (`ropeway`'s `21 26 FE FF` bind, which the stalled
            // follower's action tap keeps opening) - leaves the route as it
            // was: re-planning the whole walk component after each one costs
            // more than the walk.
            let before = (
                flags_of_world(session),
                cell_of(player_xz(session).0, player_xz(session).1),
            );
            // A band that polls the held pad once, on the tick it spawns
            // (`balden`'s elevator call bands P2[6] / P2[7], whose poll wants
            // Up toward the car), reads the pad the walk is holding: a hand
            // that walks across one going elsewhere fails the poll and walks
            // on. The scripted-sequence pad would answer the poll instead -
            // and its Up walks the player back off the band, so the walk
            // re-crosses it forever.
            let r = match poll_chain_pads(session) {
                Some(pads) if pad != 0 && !pads.contains(&pad) => {
                    session.host.world.set_pad(pad);
                    match session.tick() {
                        Ok(SceneTickEvent::SceneEntered { name }) => Run::Entered(name),
                        Ok(_) => run_while_moving(session, DEEP_EXIT_TICKS),
                        Err(e) => Run::Error(format!("{e:#}")),
                    }
                }
                _ => run_while_moving(session, DEEP_EXIT_TICKS),
            };
            if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                eprintln!(
                    "    [walk-script] tile {:?}: {site} [{}] -> {r:?}; game_over {}",
                    here(session),
                    rec_label,
                    session.host.world.game_over
                );
            }
            match r {
                Run::Entered(s) => return Ok(Walk::Entered(s)),
                Run::Released => {
                    if let Some(rec) = fired_record
                        && session.host.world.dialog.three_actor_talk.is_some()
                    {
                        RAN_ON_WALK.with(|t| t.borrow_mut().insert((scene_name(session), rec)));
                    }
                }
                other => return Err(format!("scripted sequence on the walk: {other:?}")),
            }
            let (px, pz) = player_xz(session);
            if before != (flags_of_world(session), cell_of(px, pz)) {
                planned_from = None;
                path.clear();
            }
            // A band that turns the walk back every time it is crossed
            // (`retock` P2[26], the Mt. Letona checkpoint: "Passage
            // forbidden" without Lord Saryu's key, then a step back) is a
            // wall to this walk; re-crossing it burns the segment budget.
            let flags_now = flags_of_world(session);
            // The script itself moved the walker and left the flags as
            // they were: a refusal's step back, not a band walked through
            // (`nilboa` P2[26] / P2[27] re-dress the boulder room in place).
            let turned_back = before.0 == flags_now && before.1 != cell_of(px, pz);
            let n = refires.entry(site).or_insert((0, flags_now.clone()));
            if n.1 == flags_now {
                n.0 += 1;
            } else {
                *n = (1, flags_now);
            }
            // On a split party, a band that fired twice on one walk and
            // changed nothing turned the walker back (`nilboa`'s tunnel
            // mouths: "go back outside?", No, a step back): the walk cannot
            // pass it, and the band is a wall to every later plan.
            if session.host.world.dialog.three_actor_talk.is_some()
                && n.0 >= 2
                && turned_back
                && let Some(rec) = fired_record
            {
                TURNBACK.with(|t| t.borrow_mut().insert((scene_name(session), rec)));
                session.host.world.set_pad(0);
                return Err(format!(
                    "no walkable path: band P2[{rec}] turned the walk to {goal:?} back at {:?}",
                    here(session)
                ));
            }
            if n.0 > WALK_SCRIPT_REFIRES {
                return Err(format!(
                    "pad walk stalled at tile {:?}: a script on the walk fired {} times and changed nothing",
                    here(session),
                    n.0
                ));
            }
        }
        let d = dist(here(session));
        if d <= within {
            session.host.world.set_pad(0);
            return Ok(Walk::Arrived);
        }
        let (px, pz) = player_xz(session);
        if d < best || visited.insert(cell_of(px, pz)) {
            best = best.min(d);
            since = 0;
        } else {
            since += 1;
            if since >= PAD_STALL_FRAMES {
                session.host.world.set_pad(0);
                let (px, pz) = player_xz(session);
                let next: Vec<(i16, i16)> = path.iter().take(4).map(|&c| cell_center(c)).collect();
                let w = &session.host.world;
                if std::env::var_os("LEGAIA_FGL_WALK_DEBUG").is_some() {
                    let lock = w
                        .player_actor_slot
                        .and_then(|sl| w.actors.get(usize::from(sl)))
                        .map(|a| a.move_state.flags);
                    eprintln!(
                        "      [stall] holder {} mode {:?} player flags {lock:x?} pad {:#06x} next {next:?} here-blocked {:?} actor {:?}",
                        holder(session),
                        w.mode,
                        w.input.pad(),
                        (0..4)
                            .map(|d| w.field_dir_blocked(px, pz, d))
                            .collect::<Vec<_>>(),
                        (0..4)
                            .map(|d| w.field_actor_dir_blocked(px, pz, d))
                            .collect::<Vec<_>>()
                    );
                    // 64-unit wall sub-cells around the player, Z rows. The
                    // goal tile reads `G` where it is wall, `g` where open.
                    for dz in -24i16..=24 {
                        let row: String = (-24i16..=24)
                            .map(|dx| {
                                let (x, z) = (px + dx * 64, pz + dz * 64);
                                if dx == 0 && dz == 0 {
                                    '@'
                                } else if dispatch_tile(x, z)
                                    == (i32::from(goal.0), i32::from(goal.1))
                                {
                                    if w.field_tile_is_wall(x, z) { 'G' } else { 'g' }
                                } else if w.field_tile_is_wall(x, z) {
                                    '#'
                                } else {
                                    '.'
                                }
                            })
                            .collect();
                        eprintln!("      [grid] {row}");
                    }
                }
                let (cx, cz) = cell_center(cell_of(px, pz));
                let block: String = (0..4)
                    .map(|dir| {
                        match (
                            w.field_dir_blocked(cx, cz, dir),
                            w.field_actor_dir_blocked(cx, cz, dir),
                        ) {
                            (true, _) => 'W',
                            (false, true) => 'A',
                            _ => '.',
                        }
                    })
                    .collect();
                return Err(format!(
                    "pad walk stalled at tile {:?} (world ({px},{pz})), {d} tiles short of {goal:?}; route head {next:?}; cell Z-/X-/Z+/X+ {block}",
                    here(session)
                ));
            }
        }
    }
    session.host.world.set_pad(0);
    Err(format!("pad walk to {goal:?} ran out of frames"))
}

/// Hold the d-pad toward world point `at` until `done` holds, for at most
/// `frames` ticks. A press toward a solid body turns the player to face it
/// and the collision stops the step, which is how a player lines up a talk
/// or leans on a door. `Some(scene)` if a scene change landed.
fn pad_lean(
    session: &mut BootSession,
    at: impl Fn(&BootSession) -> Option<(i16, i16)>,
    frames: usize,
    done: impl Fn(&BootSession) -> bool,
) -> Option<String> {
    for _ in 0..frames {
        if done(session) || !walking(session) {
            break;
        }
        let Some((ax, az)) = at(session) else { break };
        let (px, pz) = player_xz(session);
        let axis = |d: i16| if d.abs() > 16 { d.signum() } else { 0 };
        let pad = pad_for_step(session, axis(ax - px), axis(az - pz));
        session.host.world.set_pad(pad);
        if let Ok(SceneTickEvent::SceneEntered { name }) = session.tick() {
            session.host.world.set_pad(0);
            return Some(name);
        }
    }
    session.host.world.set_pad(0);
    None
}

/// The four tiles beside `tile`, nearest to the player first, skipping any
/// that carry a `.MAP` trigger (standing there would run its record).
fn beside(session: &BootSession, tile: (i16, i16)) -> Vec<(i16, i16)> {
    let claimed = claimed_tiles(session);
    let (px, pz) = player_xz(session);
    let me = dispatch_tile(px, pz);
    // A side whose shared edge with the band is a wall goes last: `taiku`'s
    // P2[15] (16, 28) lies in a corridor whose west wall the open collision
    // outside runs along, and the band is reached up the corridor, never
    // across the wall. The test is the locomotion's own leading-edge probe
    // (`FUN_801CFE4C`) swept over the half tile from the side's centre to the
    // shared edge. Its corner probes also catch the side walls of a one-tile
    // passage, so a walled side is demoted, not dropped.
    let w = &session.host.world;
    let walled = |from: (i16, i16)| {
        let (ax, az) = tile_center(from);
        let (sx, sz) = (tile.0 - from.0, tile.1 - from.1);
        let dir = match (sx, sz) {
            (0, -1) => 0,
            (-1, 0) => 1,
            (0, 1) => 2,
            _ => 3,
        };
        (0..=4).any(|k| w.field_dir_blocked(ax + sx * 16 * k, az + sz * 16 * k, dir))
    };
    let mut out: Vec<(i16, i16)> = [(-1i16, 0i16), (1, 0), (0, -1), (0, 1)]
        .iter()
        .map(|&(dx, dz)| (tile.0 + dx, tile.1 + dz))
        .filter(|&(x, z)| {
            (0..128).contains(&x) && (0..128).contains(&z) && !claimed.contains(&(x as u8, z as u8))
        })
        .collect();
    out.sort_by_key(|&t| {
        (
            w.mode == SceneMode::Field && walled(t),
            (i32::from(t.0) - me.0).abs() + (i32::from(t.1) - me.1).abs(),
        )
    });
    out
}

/// Step onto walk-on `tile` with the pad: walk to an inert tile beside it,
/// then onto it, so the dispatch sees a genuine tile change.
fn pad_step_onto(session: &mut BootSession, tile: (u8, u8)) -> Result<Walk, String> {
    let goal = (i16::from(tile.0), i16::from(tile.1));
    let avoid = pad_avoid(session, None);
    let mut last = String::from("no inert tile beside it");
    for side in beside(session, goal) {
        match pad_walk(session, side, &avoid, 0) {
            Ok(Walk::Entered(s)) => return Ok(Walk::Entered(s)),
            Ok(Walk::Arrived) => {}
            Err(e) => {
                last = e;
                continue;
            }
        }
        let onto = pad_avoid(session, Some(goal));
        match pad_walk(session, goal, &onto, 0) {
            Err(e) if !on_tile(session, goal) => {
                last = e;
                break;
            }
            r => return r,
        }
    }
    // No side reaches it: a teleport whose landing is the band is a tile
    // change onto it too. `taiku`'s exit band P2[15] (16, 28) sits in a
    // walled corridor whose only way in is the pad at (84, 59).
    let mut sources: Vec<(i16, i16)> = teleports(session)
        .iter()
        .filter(|&(_, &c)| tile_of(cell_center(c).0, cell_center(c).1) == goal)
        .map(|(&(x, z), _)| (x as i16, z as i16))
        .collect();
    sources.sort_unstable();
    for src in sources {
        let onto = pad_avoid(session, Some(src));
        match pad_walk(session, src, &onto, 0) {
            Ok(Walk::Entered(s)) => return Ok(Walk::Entered(s)),
            Ok(Walk::Arrived) => return Ok(Walk::Arrived),
            Err(_) if on_tile(session, goal) => return Ok(Walk::Arrived),
            Err(e) => last = format!("{last}; by the teleport at {src:?}: {e}"),
        }
    }
    Err(format!("pad step onto {tile:?}: {last}"))
}

/// Whether the player stands on `tile`'s dispatch tile.
fn on_tile(session: &BootSession, tile: (i16, i16)) -> bool {
    let (x, z) = player_xz(session);
    dispatch_tile(x, z) == (i32::from(tile.0), i32::from(tile.1))
}

// ---------------------------------------------------------------------------
// Healing through the pause menu
// ---------------------------------------------------------------------------

/// Press `mask` for one frame and release on the next: every menu surface
/// reads `just_pressed`, so a held mask is one event.
/// Press Start and wait out the pause wipe: retail's field session raises
/// the wipe level from black-free to full before it spawns the menu
/// (`FUN_801ED308`), and the menu takes no input until then
/// (`BootSession::pause_wipe`). A press sent into the wipe is lost, and the
/// field heal then walked out of a menu it never drove.
fn open_pause_menu(session: &mut BootSession) {
    tap_pad(session, PadButton::Start.mask());
    for _ in 0..120 {
        if session.field_menu.is_none() || session.pause_wipe().menu_spawned() {
            break;
        }
        let _ = session.tick();
    }
}

fn tap_pad(session: &mut BootSession, mask: u16) {
    session.host.world.set_pad(mask);
    let _ = session.tick();
    session.host.world.set_pad(0);
    let _ = session.tick();
}

/// Hand control to the next party member inside an armed three-actor talk:
/// press Square, the newly-pressed bit the talk controller's state-0 arm gate
/// reads (`FUN_801D27E0`, `_DAT_8007B874 & 0x80`), and wait out the
/// fade-swap-fade cycle. `true` when the leader changed.
fn swap_leader(session: &mut BootSession) -> bool {
    let armed = |s: &BootSession| {
        s.host
            .world
            .dialog
            .three_actor_talk
            .as_ref()
            .is_some_and(|t| t.swap.phase == 0)
    };
    if !armed(session) || !released(session) {
        return false;
    }
    let before = session.host.world.party.party_leader_slot;
    tap_pad(session, PadButton::Square.mask());
    for _ in 0..240 {
        if armed(session) && session.host.world.party.party_leader_slot != before {
            break;
        }
        if session.tick().is_err() {
            break;
        }
    }
    let now = session.host.world.party.party_leader_slot;
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        let (x, z) = player_xz(session);
        eprintln!(
            "    [swap] leader {before:?} -> {now:?}, now at {:?}",
            tile_of(x, z)
        );
    }
    now != before
}

/// The weakest living party member's HP as a fraction of its maximum, in
/// per-mille. `1000` for a party at full health.
fn party_hp_permille(session: &BootSession) -> u32 {
    let w = &session.host.world;
    let n = w.party.party_count.clamp(1, 3) as usize;
    (0..n)
        .filter_map(|i| {
            let b = &w.actors.get(i)?.battle;
            (b.max_hp > 0 && b.hp > 0).then(|| u32::from(b.hp) * 1000 / u32::from(b.max_hp))
        })
        .min()
        .unwrap_or(1000)
}

/// Heal the party with the pad, as a player does before a boss or after a
/// fight that left it low: Start opens the pause menu, the cursor walks to
/// Items, Use, the first HP-restoring item, the weakest member; repeat while
/// the weakest is below `threshold` per-mille and the bag has a restorative;
/// then Circle back out to the field. Every step is a pad edge through the
/// same menu sessions both play hosts drive. Returns how many items it used.
fn pad_field_heal(session: &mut BootSession, threshold: u32) -> usize {
    use legaia_engine_core::field_menu::FieldMenuRow;
    use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
    use legaia_engine_core::inventory_use::InventoryUseState;
    use legaia_engine_core::items::ItemEffect;
    use legaia_engine_core::pause_screens::PauseItemsFocus;
    if !walking(session) || !released(session) || party_hp_permille(session) >= threshold {
        return 0;
    }
    open_pause_menu(session);
    if session.field_menu.is_none() {
        return 0;
    }
    let items_row = FieldMenuRow::Items.index();
    let mut used = 0usize;
    for _ in 0..200 {
        let Some(menu) = session.field_menu.as_ref() else {
            break;
        };
        let pad = match session.field_menu_sub.as_ref() {
            None => match menu.phase() {
                legaia_engine_core::field_menu::FieldMenuPhase::Browsing { cursor } => {
                    if party_hp_permille(session) >= threshold
                        || used >= 4
                        || used > 0 && cursor != items_row
                    {
                        PadButton::Circle.mask()
                    } else if cursor != items_row {
                        PadButton::Down.mask()
                    } else if used > 0 {
                        PadButton::Circle.mask()
                    } else {
                        PadButton::Cross.mask()
                    }
                }
                _ => 0,
            },
            Some(FieldMenuSubsession::Items(p)) => {
                let heals = |id: u8| {
                    p.inner.catalog.get(id).is_some_and(|e| {
                        e.usable_in_field
                            && matches!(e.effect, ItemEffect::Heal { .. } | ItemEffect::HealAll)
                    })
                };
                let want = p
                    .inner
                    .filtered_items
                    .iter()
                    .position(|&i| p.inner.items.get(i).is_some_and(|&id| heals(id)));
                match (p.focus, &p.inner.state) {
                    _ if party_hp_permille(session) >= threshold || want.is_none() || used >= 4 => {
                        PadButton::Circle.mask()
                    }
                    (PauseItemsFocus::Command, _) => {
                        if p.command_cursor == 0 {
                            PadButton::Cross.mask()
                        } else {
                            PadButton::Up.mask()
                        }
                    }
                    (PauseItemsFocus::List, InventoryUseState::Browsing { cursor }) => {
                        let want = want.unwrap_or(0);
                        if *cursor < want {
                            PadButton::Down.mask()
                        } else if *cursor > want {
                            PadButton::Up.mask()
                        } else {
                            PadButton::Cross.mask()
                        }
                    }
                    (PauseItemsFocus::List, InventoryUseState::TargetSelect { cursor, .. }) => {
                        let weakest = p
                            .inner
                            .targets
                            .iter()
                            .enumerate()
                            .filter(|(_, t)| !t.is_enemy && t.alive && t.hp_max > 0)
                            .min_by_key(|(_, t)| u32::from(t.hp) * 1000 / u32::from(t.hp_max))
                            .map_or(0, |(k, _)| k);
                        if *cursor < weakest {
                            PadButton::Down.mask()
                        } else if *cursor > weakest {
                            PadButton::Up.mask()
                        } else {
                            used += 1;
                            PadButton::Cross.mask()
                        }
                    }
                    _ => PadButton::Circle.mask(),
                }
            }
            Some(_) => PadButton::Circle.mask(),
        };
        if pad == 0 {
            let _ = session.tick();
            continue;
        }
        tap_pad(session, pad);
    }
    // Whatever is still open closes on Circle.
    for _ in 0..16 {
        if session.field_menu.is_none() {
            break;
        }
        tap_pad(session, PadButton::Circle.mask());
    }
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "    [heal] used {used} item(s); weakest now {} per-mille; menu {}",
            party_hp_permille(session),
            if session.field_menu.is_some() {
                "STILL OPEN"
            } else {
                "closed"
            }
        );
    }
    used
}

/// Burn an Incense with the pad, as a player does who is too hurt to survive
/// the next encounter and has nothing to heal with: when the weakest member
/// is still below `threshold` per-mille after [`pad_field_heal`], the scene
/// rolls encounters, the bag holds an Incense (`0x8A`) and its window
/// (`_DAT_8007B600`) has run out, Start opens the pause menu and Items, Use,
/// the Incense row and the confirm's Yes commit one. Retail's class-`0x82`
/// applier skips the whole region roll while the window is open, so one use
/// buys `0x40` walk-regen ticks (`0x800` walking vsyncs) with no encounter at
/// all, whatever the rand stream deals (see `field-menu.md`, the Incense
/// route `FUN_801D8D94`). Returns whether one was used.
fn pad_field_repel(session: &mut BootSession, threshold: u32) -> bool {
    use legaia_engine_core::field_menu::FieldMenuRow;
    use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
    use legaia_engine_core::inventory_use::InventoryUseState;
    use legaia_engine_core::pause_screens::{INCENSE_ITEM_ID, PauseItemsFocus};
    let w = &session.host.world;
    if !walking(session)
        || !released(session)
        || party_hp_permille(session) >= threshold
        || !w.scene_can_roll_encounters()
        || w.locomotion.walk_regen_window != 0
        || !w
            .party
            .inventory
            .iter()
            .any(|(&id, &c)| id == INCENSE_ITEM_ID && c > 0)
    {
        return false;
    }
    open_pause_menu(session);
    if session.field_menu.is_none() {
        return false;
    }
    let items_row = FieldMenuRow::Items.index();
    let mut used = false;
    for _ in 0..200 {
        let Some(menu) = session.field_menu.as_ref() else {
            break;
        };
        let pad = match session.field_menu_sub.as_ref() {
            None => match menu.phase() {
                legaia_engine_core::field_menu::FieldMenuPhase::Browsing { cursor } => {
                    if used {
                        PadButton::Circle.mask()
                    } else if cursor != items_row {
                        PadButton::Down.mask()
                    } else {
                        PadButton::Cross.mask()
                    }
                }
                _ => 0,
            },
            Some(FieldMenuSubsession::Items(p)) => {
                // The hand walks every bag row; an Incense row routes to its
                // own confirm, not through the usable-in-context filter.
                let want = p.rows.iter().position(|r| r.id == INCENSE_ITEM_ID);
                if p.focus == PauseItemsFocus::SpecialRoute {
                    // The confirm opens with its cursor on Yes.
                    used = true;
                    PadButton::Cross.mask()
                } else {
                    match (p.focus, &p.inner.state, want) {
                        _ if used => PadButton::Circle.mask(),
                        // The Use list's filter is built on entering it.
                        (PauseItemsFocus::Command, _, _) => {
                            if p.command_cursor == 0 {
                                PadButton::Cross.mask()
                            } else {
                                PadButton::Up.mask()
                            }
                        }
                        (PauseItemsFocus::List, InventoryUseState::Browsing { .. }, Some(k)) => {
                            let cursor = p.list_cursor();
                            if cursor < k {
                                PadButton::Down.mask()
                            } else if cursor > k {
                                PadButton::Up.mask()
                            } else {
                                PadButton::Cross.mask()
                            }
                        }
                        _ => PadButton::Circle.mask(),
                    }
                }
            }
            Some(_) => PadButton::Circle.mask(),
        };
        if pad == 0 {
            let _ = session.tick();
            continue;
        }
        tap_pad(session, pad);
    }
    for _ in 0..16 {
        if session.field_menu.is_none() {
            break;
        }
        tap_pad(session, PadButton::Circle.mask());
    }
    let window = session.host.world.locomotion.walk_regen_window;
    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
        eprintln!(
            "    [repel] incense committed {used}; window now {window}; menu {}",
            if session.field_menu.is_some() {
                "STILL OPEN"
            } else {
                "closed"
            }
        );
    }
    used && window > 0
}

/// `(scene, door tile)` -> the flags the band refused under.
type RefusedDoors = HashMap<(String, (i16, i16)), BTreeSet<u16>>;

thread_local! {
    /// Door bands a pad walk was turned back from this segment - a script
    /// on the approach fired and fired and changed nothing (`kor3`'s warp
    /// pad rooms: P0[3] / P0[5] answer "The power is out." while `0x403` is
    /// clear) - each with the flags it refused under. [`pad_hop`] tries the
    /// scene's other doors toward the same destination before it gives the
    /// hop up; new flags re-open the band.
    static REFUSED_DOORS: std::cell::RefCell<RefusedDoors> =
        std::cell::RefCell::new(HashMap::new());
    /// The scene and door band [`pad_hop`] last walked to: a crossing that
    /// brought the player back to the side it left refuses that band, so the
    /// next round trip leaves by another door into the same scene.
    static LAST_HOP_DOOR: std::cell::RefCell<Option<(String, (i16, i16))>> =
        const { std::cell::RefCell::new(None) };
}

thread_local! {
    /// Scenes the pad hand has tried to rest in this segment: a rest that
    /// did not lift the party is not walked again on the next hop.
    static RESTED: std::cell::RefCell<BTreeSet<String>> =
        const { std::cell::RefCell::new(BTreeSet::new()) };
}

/// Does the record body carry the restore - op `4C 82 <slot>`, the
/// per-party-slot HP/MP refill (`FieldHost::op4c_n8_sub2_restore_party_slot`)
/// every bed and inn stay ends in - at a clean decode boundary?
fn record_restores(
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    part: usize,
    rec: usize,
) -> bool {
    use legaia_asset::field_disasm::{OpKey, clean_hit_offsets};
    use legaia_engine_core::man_field_scripts::partition_record_span;
    let Some((start, pc0, len)) = partition_record_span(mf, man, part, rec) else {
        return false;
    };
    let key = OpKey {
        opcode: 0x4C,
        sub: Some(0x82),
    };
    !clean_hit_offsets(&man[start..start + len], pc0, key).is_empty()
}

/// Rest before setting out, as a player with an empty bag does: when the
/// weakest member is still below half HP after [`pad_field_heal`], walk to
/// the scene's rest - a bed's walk-on band, or an NPC whose talk (or a
/// record it spawns) runs the `4C 82` restore - and take it. An inn's gold
/// gate is the record's own: a stay the purse cannot pay refuses itself.
/// `Ok(Some(scene))` when the rest's script left the scene.
///
/// Only where the walk to it is safe: in a scene that rolls encounters the
/// way to the bed is as dangerous as the way out. `dolk`'s mist-era bed
/// (P2[9], behind the inn's stair door) sits across a castle whose every
/// region rolls; a lone member at 17/219 walking to it wiped on 14 of 21
/// dealt streams (`LEGAIA_FGL_RNG_SEED`), against 3 of 21 walking straight
/// out to `map01`.
fn pad_rest(session: &mut BootSession) -> Result<Option<String>, String> {
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    if session.host.world.mode != SceneMode::Field
        || session.host.world.scene_can_roll_encounters()
        || !walking(session)
        || !released(session)
        || party_hp_permille(session) >= 500
    {
        return Ok(None);
    }
    pad_field_heal(session, 500);
    if party_hp_permille(session) >= 500 {
        return Ok(None);
    }
    let name = scene_name(session);
    if !RESTED.with(|r| r.borrow_mut().insert(name.clone())) {
        return Ok(None);
    }
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return Ok(None);
    };
    let trace = std::env::var_os("LEGAIA_FGL_TRACE").is_some();
    let doors: BTreeSet<u8> = overworld_portal_sites(&mf, &man, &triggers)
        .iter()
        .map(|s| s.record)
        .collect();
    // Walk-on rests: the first tile of each gate-1 band whose live
    // partition-2 record restores.
    let mut beds: BTreeMap<u8, (u8, u8)> = BTreeMap::new();
    for t in triggers.iter().filter(|t| t.gate == 1) {
        let owner = triggers
            .iter()
            .find(|u| (u.tile_x, u.tile_z) == (t.tile_x, t.tile_z));
        if owner.is_none_or(|u| u.record != t.record || u.gate != 1) || doors.contains(&t.record) {
            continue;
        }
        let rec = usize::from(t.record);
        let open = partition2_record_gates(&mf, &man, rec)
            .is_some_and(|(c1, c2)| session.host.world.p2_record_gates_pass(&c1, &c2));
        if open && record_restores(&mf, &man, 2, rec) {
            beds.entry(t.record).or_insert((t.tile_x, t.tile_z));
        }
    }
    // Talk rests: an innkeeper's record restores itself or spawns one that
    // does.
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let w = &session.host.world;
    let keepers: Vec<u8> = w
        .npcs
        .positions
        .keys()
        .copied()
        .filter(|s| w.npcs.dialog.contains_key(s) || w.npcs.dialog_prologue.contains_key(s))
        .filter(|&s| {
            record_restores(&mf, &man, 1, usize::from(s))
                || spawned_p2(&mf, &man, 1, usize::from(s), n0 + n1, 3)
                    .iter()
                    .any(|&r| record_restores(&mf, &man, 2, r))
        })
        .collect();
    if trace {
        eprintln!(
            "    [rest] {name}: weakest {} per-mille; beds {beds:?}; keepers {keepers:?}",
            party_hp_permille(session)
        );
    }
    for (rec, tile) in beds {
        let r = match pad_step_onto(session, tile) {
            Ok(Walk::Entered(s)) => Run::Entered(s),
            Ok(Walk::Arrived) => run_while_moving(session, DEEP_EXIT_TICKS),
            Err(e) => Run::Parked(e),
        };
        if let Some(w) = wiped(session) {
            return Err(w);
        }
        if trace {
            eprintln!(
                "    [rest] {name} bed P2[{rec}] at {tile:?} -> {r:?}; weakest now {} per-mille",
                party_hp_permille(session)
            );
        }
        match r {
            Run::Entered(s) => return Ok(Some(s)),
            Run::Battle(b) => return Err(format!("rest P2[{rec}]: {b}")),
            Run::Error(e) => return Err(e),
            Run::Released | Run::Parked(_) => {}
        }
        if party_hp_permille(session) >= 500 {
            // A rest that worked may be taken again after the next fights.
            RESTED.with(|r| r.borrow_mut().remove(&name));
            return Ok(None);
        }
    }
    for slot in keepers {
        let r = talk_to(session, slot);
        if trace {
            eprintln!(
                "    [rest] {name} talk P1[{slot}] -> {r:?}; weakest now {} per-mille",
                party_hp_permille(session)
            );
        }
        match r {
            Run::Entered(s) => return Ok(Some(s)),
            Run::Battle(b) => return Err(format!("rest talk P1[{slot}]: {b}")),
            Run::Error(e) => return Err(e),
            Run::Released | Run::Parked(_) => {}
        }
        if party_hp_permille(session) >= 500 {
            // A rest that worked may be taken again after the next fights.
            RESTED.with(|r| r.borrow_mut().remove(&name));
            return Ok(None);
        }
    }
    Ok(None)
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

/// A party wipe on a pad walk leaves the game-over screen up; nothing after
/// it is play.
fn wiped(session: &BootSession) -> Option<String> {
    let w = &session.host.world;
    (w.game_over_hold || w.game_over).then(|| format!("party wiped: {}", battle_snapshot(session)))
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
    let doors: BTreeSet<u8> = overworld_portal_sites(&mf, &man, &triggers)
        .iter()
        .map(|s| s.record)
        .collect();
    // A tile reaches a record only when no earlier entry claims it: the
    // dispatch takes the first primary-then-fallback match
    // (`FUN_801D5630`), so a fallback band shadowed by a primary entry on
    // the same tile never fires its own record.
    let mut first_tile: BTreeMap<u8, (u8, u8)> = BTreeMap::new();
    // Every tile of each band, for a walk that cannot reach the first.
    let mut band: BTreeMap<u8, Vec<(u8, u8)>> = BTreeMap::new();
    for t in triggers.iter().filter(|t| t.gate == 1) {
        let owner = triggers
            .iter()
            .find(|u| (u.tile_x, u.tile_z) == (t.tile_x, t.tile_z));
        if owner.is_some_and(|u| u.record == t.record && u.gate == 1) {
            first_tile.entry(t.record).or_insert((t.tile_x, t.tile_z));
            band.entry(t.record).or_default().push((t.tile_x, t.tile_z));
        }
    }
    // Talk and walk-on beats unlock each other (a conversation sets the flag
    // a walk-on record's C2 gate needs, and a walk-on cutscene sets the flag
    // a conversation branches on), so both passes repeat while a round still
    // gains flags. Each beat plays at most once per scene visit.
    let mut walked: BTreeSet<u8> = BTreeSet::new();
    let mut touched: BTreeSet<u8> = BTreeSet::new();
    let mut examined: BTreeSet<(u8, u8)> = BTreeSet::new();
    let overreach = overreaching_records(session, &mf, &man, 2);
    // A stager's record is often a staged conversation (`tunnelc` P1[4],
    // Xain: `0x323`, `0x324`, `0x325`, then the fight), each contact
    // playing the next stage, so stagers are approached every round until
    // a contact gains nothing.
    let mut stager_spent: BTreeSet<usize> = BTreeSet::new();
    for _round in 0..BEAT_ROUNDS {
        let round_start = flags_of_world(session);
        for p in boss_stager_placements(&mf, &man) {
            if stager_spent.contains(&p.placement_index) {
                continue;
            }
            STAGED_FIGHTS.with(|s| {
                s.borrow_mut()
                    .insert((name.clone(), Some(u16::from(p.formation_row))))
            });
            if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                eprintln!(
                    "    [stager] {name} P1[{}] row {} park {:?} (set {:?}, next anchor {:?}) station {:?} spawn {:?} parked {}",
                    p.placement_index,
                    p.formation_row,
                    p.park_gate_flag,
                    p.park_gate_flag
                        .map(|f| session.host.world.system_flag_test(f)),
                    p.park_gate_flag.map(next_anchor_has),
                    p.station_world,
                    p.spawn_world,
                    p.spawn_parked
                );
            }
            if p.park_gate_flag
                .is_some_and(|f| session.host.world.system_flag_test(f))
            {
                continue;
            }
            if stager_overreaches(&mf, &man, p.placement_index) {
                continue;
            }
            let at = match (p.station_world, p.spawn_parked) {
                (Some(st), _) => st,
                (None, false) => p.spawn_world,
                (None, true) => continue,
            };
            let (tx, tz) = tile_of(at.0, at.1);
            ran += 1;
            if pad_hand() {
                // A player walks into a boss at full strength.
                pad_field_heal(session, 900);
                // Walk up to the stager; its touch dispatch runs off the
                // locomotion step, as it does for a player.
                let avoid = pad_avoid(session, None);
                let walk = pad_walk(session, (tx, tz), &avoid, 1);
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!("    [stager] walk -> {walk:?}");
                }
                match walk {
                    Ok(Walk::Entered(s)) => {
                        finish(session, log, ran);
                        return Ok(Some(s));
                    }
                    Ok(Walk::Arrived) => {
                        let _ = pad_lean(session, move |_| Some(at), 24, |s| !released(s));
                        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                            eprintln!(
                                "    [stager] leaned: holder {} at {}; touch {:?}; player {:?}",
                                holder(session),
                                park_site(session),
                                session.host.world.props.active_walk_touch,
                                player_xz(session)
                            );
                        }
                    }
                    Err(_) => continue,
                }
            } else {
                session
                    .host
                    .world
                    .seat_player_at_tile(tx.clamp(0, 127) as u8, tz.clamp(0, 127) as u8);
            }
            let f0 = flags_of_world(session);
            let r = run_while_moving(session, DEEP_EXIT_TICKS);
            if flags_of_world(session) == f0 {
                stager_spent.insert(p.placement_index);
            }
            trace_beat(session, &f0, || {
                format!("{name} boss stager P1[{}] -> {r:?}", p.placement_index)
            });
            match r {
                Run::Entered(s) => {
                    finish(session, log, ran);
                    return Ok(Some(s));
                }
                Run::Battle(b) => {
                    return Err(format!("boss stager P1[{}]: {b}", p.placement_index));
                }
                Run::Error(e) => return Err(e),
                Run::Released | Run::Parked(_) => {}
            }
        }
        // A talk is re-tried each round while its flag stays clear: its
        // record may branch on a flag the previous round's beats set.
        for slot in talk_beats(session, &mf, &man) {
            if let Some(w) = wiped(session) {
                return Err(w);
            }
            pad_budget(session)?;
            if ran >= MAX_BEATS {
                continue;
            }
            ran += 1;
            let f0 = flags_of_world(session);
            if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                eprintln!("    [talk] {name} P1[{slot}] ...");
            }
            // A talk that stages a fight (Xain, `tunnelc` P1[4]: his
            // second stage's option 0 runs `3E FF 0A`) is a boss approach:
            // a player walks into it at full strength, as into a stager.
            if pad_hand() && record_stages_fight(&mf, &man, 1, usize::from(slot)) {
                pad_field_heal(session, 900);
            }
            let r = talk_to(session, slot);
            let r = fight_committed(session, r);
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
        let boss_band_live = first_tile.keys().any(|&rec| {
            record_stages_fight(&mf, &man, 2, usize::from(rec))
                && partition2_record_gates(&mf, &man, usize::from(rec))
                    .is_some_and(|(c1, c2)| session.host.world.p2_record_gates_pass(&c1, &c2))
        });
        for (&rec, &tile) in &first_tile {
            if let Some(w) = wiped(session) {
                return Err(w);
            }
            pad_budget(session)?;
            if RAN_ON_WALK.with(|t| t.borrow().contains(&(name.clone(), rec))) {
                walked.insert(rec);
            }
            if doors.contains(&rec)
                || walked.contains(&rec)
                || overreach.contains(&usize::from(rec))
                || ran >= MAX_BEATS
            {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() && !walked.contains(&rec) {
                    eprintln!(
                        "    [beat-skip] {name} P2[{rec}] door {} overreach {}",
                        doors.contains(&rec),
                        overreach.contains(&usize::from(rec))
                    );
                }
                continue;
            }
            if record_stages_fight(&mf, &man, 2, usize::from(rec))
                && record_overreaches(&mf, &man, 2, usize::from(rec), true)
            {
                continue;
            }
            let pass = partition2_record_gates(&mf, &man, usize::from(rec))
                .is_some_and(|(c1, c2)| session.host.world.p2_record_gates_pass(&c1, &c2));
            if !pass {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!(
                        "    [beat-skip] {name} P2[{rec}] gates {:?}",
                        partition2_record_gates(&mf, &man, usize::from(rec))
                    );
                }
                continue;
            }
            if pad_hand() && reach_beat_waits(session, &mf, &man, usize::from(rec)) {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!("    [beat-skip] {name} P2[{rec}] waits on the other reach beats");
                }
                continue;
            }
            walked.insert(rec);
            let f0 = flags_of_world(session);
            ran += 1;
            // A walk-on that stages a fight (`jagaroom` P2[9], Van Saryu) is
            // a boss approach like a stager's: a player tops the party up
            // before stepping on. So is any walk in a scene where such a band
            // is live, since the route to another beat can cross it: the
            // walk to `jagaroom` P2[4] runs over P2[9]'s tiles.
            if pad_hand() && boss_band_live {
                pad_field_heal(session, 900);
            }
            PLAYING_BEAT.with(|b| b.set(Some(rec)));
            let r = if pad_hand() {
                // A band is often several tiles wide and walled on some of
                // them (`retockin` P2[42], Lord Saryu's audience, spans
                // (104..107, 50) and (107, 50) is boxed in): a walk that
                // cannot reach its first tile tries the rest, nearest first.
                let (px, pz) = player_xz(session);
                let me = dispatch_tile(px, pz);
                let mut tiles: Vec<(u8, u8)> = band
                    .get(&rec)
                    .map(|v| v.iter().copied().filter(|&t| t != tile).collect())
                    .unwrap_or_default();
                tiles.sort_by_key(|&(x, z)| {
                    (i32::from(x) - me.0).abs() + (i32::from(z) - me.1).abs()
                });
                tiles.insert(0, tile);
                let mut r = Run::Parked("band has no tile".into());
                // A band out of the leader's walk component may be in another
                // party member's: inside an armed three-actor talk (`nilboa`'s
                // Three Tunnels, `43 02`) each member stands in its own tunnel
                // and Square hands control to the next ([`swap_leader`]).
                let mut swaps = 0;
                let mut queue: VecDeque<(u8, u8)> = tiles.iter().copied().collect();
                while let Some(t) = queue.pop_front() {
                    pad_budget(session)?;
                    r = match pad_step_onto(session, t) {
                        Ok(Walk::Entered(s)) => Run::Entered(s),
                        Ok(Walk::Arrived) => run_while_moving(session, DEEP_EXIT_TICKS),
                        Err(e) => Run::Parked(e),
                    };
                    // Retried only when the tile itself is the obstacle: no
                    // path to it at all, or a walk that stalled at its edge.
                    // A walk that stalled far off is the route's problem, and
                    // the next tile would only walk it again.
                    let short = |e: &str| {
                        e.split(" tiles short")
                            .next()
                            .and_then(|h| h.rsplit(' ').next().and_then(|n| n.parse::<u32>().ok()))
                    };
                    let unreached = matches!(&r, Run::Parked(e)
                        if e.contains("no walkable path")
                            || (e.contains("pad walk stalled") && short(e).is_some_and(|n| n <= 2)));
                    // A room-marker band crossed on the way may raise a flag
                    // (`0x52A`); only the record's own run ends the search -
                    // which may have come from another band on the way
                    // (`nilboa` P2[29] spawns the split, P2[0]).
                    if !unreached || RAN_ON_WALK.with(|t| t.borrow().contains(&(name.clone(), rec)))
                    {
                        break;
                    }
                    if queue.is_empty() && swaps < 2 && swap_leader(session) {
                        swaps += 1;
                        queue.extend(tiles.iter().copied());
                    }
                }
                r
            } else {
                step_onto(session, tile);
                run_while_moving(session, DEEP_EXIT_TICKS)
            };
            PLAYING_BEAT.with(|b| b.set(None));
            let r = fight_committed(session, r);
            // A band no member could walk to stays on the list: a later
            // beat may open the way (`nilboa` P2[39], the warp at the far
            // end of one tunnel, drops its member beside the P2[2] band).
            if matches!(&r, Run::Parked(e) if e.contains("no walkable path")) {
                walked.remove(&rec);
            }
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
        // Object doors whose live arm spawns a story beat: a `.MAP`-bound
        // partition-0 door record branches on the story flags, and one arm
        // runs op `0x44` instead of the teleport (`town01` P0[29], Vahn's
        // front door, spawns the P2[5] night beat that sets `0x227` once
        // `0x226` is up and `0x227` is not).
        for (slot, contact) in object_door_beats(session, &mf, &man) {
            if let Some(w) = wiped(session) {
                return Err(w);
            }
            pad_budget(session)?;
            if !touched.insert(slot) || ran >= MAX_BEATS {
                continue;
            }
            ran += 1;
            let flat = session
                .host
                .world
                .props
                .walk_touch_records
                .get(&slot)
                .copied();
            let (tx, tz) = tile_of(contact.0, contact.1);
            session.host.world.set_pad(0);
            if pad_hand() {
                // Walk up to the door and lean on it: the locomotion's own
                // contact probe posts the touch.
                let f0 = flags_of_world(session);
                let avoid = pad_avoid(session, None);
                let mut r = Run::Parked(format!("no approach to door object {slot}"));
                // Up to the door tile itself first (its box may leave the
                // tile only partly solid), then from each side.
                let approaches: Vec<((i16, i16), i32)> = std::iter::once(((tx, tz), 1))
                    .chain(beside(session, (tx, tz)).into_iter().map(|t| (t, 0)))
                    .collect();
                for (side, within) in approaches {
                    match pad_walk(session, side, &avoid, within) {
                        Ok(Walk::Entered(s)) => {
                            r = Run::Entered(s);
                            break;
                        }
                        Ok(Walk::Arrived) => {}
                        Err(e) => {
                            r = Run::Parked(e);
                            continue;
                        }
                    }
                    let started = |s: &BootSession| {
                        s.host.world.props.active_walk_touch == Some(slot) || !released(s)
                    };
                    if let Some(s) = pad_lean(session, move |_| Some(contact), 32, started) {
                        r = Run::Entered(s);
                        break;
                    }
                    if started(session) {
                        r = run_while_moving(session, DEEP_EXIT_TICKS);
                        break;
                    }
                    r = Run::Parked(format!("door object {slot} posted no touch"));
                }
                trace_beat(session, &f0, || {
                    format!("{name} door object {slot} (pad) -> {r:?}")
                });
                match r {
                    Run::Entered(s) => {
                        finish(session, log, ran);
                        return Ok(Some(s));
                    }
                    Run::Battle(b) => return Err(format!("door object {slot}: {b}")),
                    Run::Error(e) => return Err(e),
                    Run::Released | Run::Parked(_) => {}
                }
                continue;
            }
            session.host.world.props.active_walk_touch = None;
            session
                .host
                .world
                .seat_player_at_tile(tx.clamp(0, 127) as u8, tz.clamp(0, 127) as u8);
            let f0 = flags_of_world(session);
            // The touch dispatch runs from the locomotion step, so a seat
            // alone posts nothing: nudge the pad until the contact posts
            // (the stand-inside probe fires on the first stepped frame).
            let mut entered = None;
            for dir in [
                PadButton::Up,
                PadButton::Down,
                PadButton::Left,
                PadButton::Right,
            ] {
                if session.host.world.props.active_walk_touch == Some(slot) {
                    break;
                }
                session.host.world.set_pad(dir.mask());
                if let Ok(SceneTickEvent::SceneEntered { name }) = session.tick() {
                    entered = Some(name);
                    break;
                }
            }
            session.host.world.set_pad(0);
            let r = match entered {
                Some(s) => Run::Entered(s),
                None => run_while_moving(session, DEEP_EXIT_TICKS),
            };
            trace_beat(session, &f0, || {
                format!("{name} door object {slot} (flat record {flat:?}) at {contact:?} -> {r:?}")
            });
            match r {
                Run::Entered(s) => {
                    finish(session, log, ran);
                    return Ok(Some(s));
                }
                Run::Battle(b) => return Err(format!("door object {slot}: {b}")),
                Run::Error(e) => return Err(e),
                Run::Released | Run::Parked(_) => {}
            }
        }
        // Props whose own script writes a wanted flag. An interact-gated
        // one (a switch) is examined with Cross, never by contact (`chitei2`
        // P0[33] raises the `0x4F0` that opens the P2[11] walk-on setting
        // `0x470`); a touch-class one (a door) is walked into.
        for (anchor, at, gated) in prop_beats(session, &mf, &man) {
            if !examined.insert(anchor) || ran >= MAX_BEATS {
                continue;
            }
            ran += 1;
            let f0 = flags_of_world(session);
            let r = if gated {
                examine_prop(session, anchor, at)
            } else {
                touch_prop(session, anchor, at)
            };
            let r = fight_committed(session, r);
            trace_beat(session, &f0, || format!("{name} prop {anchor:?} -> {r:?}"));
            match r {
                Run::Entered(s) => {
                    finish(session, log, ran);
                    return Ok(Some(s));
                }
                Run::Battle(b) => return Err(format!("prop {anchor:?}: {b}")),
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

/// The `.MAP` object doors of the loaded field scene (walk-touch slot and
/// contact centre) whose record, resolved against the live flags, spawns a
/// partition-2 record that - itself or through what it spawns - cleanly
/// SETs a still-clear flag the next anchor carries.
fn object_door_beats(
    session: &BootSession,
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
) -> Vec<(u8, (i16, i16))> {
    use legaia_engine_core::man_field_scripts::{
        FlagBank, WalkTouchEvent, resolve_walk_touch_event, walk_partition_gflag_sites,
    };
    let w = &session.host.world;
    let wanted = |part: usize| -> BTreeSet<usize> {
        walk_partition_gflag_sites(mf, man, part)
            .iter()
            .filter(|s| {
                s.bank == FlagBank::System
                    && s.kind == FlagKind::Set
                    && s.clean
                    && !s.text_alias
                    && !s.debug_menu
                    && !w.system_flag_test(s.flag)
                    && next_anchor_has(s.flag)
            })
            .map(|s| s.record)
            .collect()
    };
    let setters0 = wanted(0);
    let setters2 = wanted(2);
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let mut out = Vec::new();
    for (&slot, &flat) in &w.props.walk_touch_records {
        let Some(&(contact, _)) = w.props.walk_touch.get(&slot) else {
            continue;
        };
        let live = resolve_walk_touch_event(mf, man, flat, &|f| w.system_flag_test(f));
        let Some(WalkTouchEvent::SpawnRecord { flat_index }) = live else {
            continue;
        };
        let Some(r2) = flat_index.checked_sub(n0 + n1) else {
            continue;
        };
        // The door's own record may latch the beat as it spawns it
        // (`town0d` P0[1] sets `0x3B9` and spawns the P2[28] song-night
        // chain, whose own writes are all inside its dialogue).
        if (flat < n0 && setters0.contains(&flat))
            || setters2.contains(&r2)
            || spawned_p2(mf, man, 2, r2, n0 + n1, 3)
                .iter()
                .any(|r| setters2.contains(r))
        {
            out.push((slot, contact));
        }
    }
    out
}

/// A prop beat: anchor tile, contact centre, and whether it is the
/// interact-gated (examine) class rather than the touch (door) class.
type PropBeat = ((u8, u8), (i32, i32), bool);

/// The props of the loaded field scene whose own bind record cleanly SETs a
/// still-clear flag the next anchor carries, or spawns a partition-2 record
/// that does, itself or through what it spawns (op `0x44`, three levels).
/// `rikuroa` P0[2], the Genesis Tree, is examined after Caruban: its body
/// spawns P2[53], which raises `0x28A` and `0x2C2` and carries the party
/// down to `map01`, where the latter plays the tree's revival. A prop whose
/// record overreaches (sets a latch the next anchor lacks) is left alone.
fn prop_beats(
    session: &BootSession,
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
) -> Vec<PropBeat> {
    use legaia_engine_core::man_field_scripts::{FlagBank, walk_partition_gflag_sites};
    let w = &session.host.world;
    let wanted = |part: usize| -> BTreeSet<usize> {
        walk_partition_gflag_sites(mf, man, part)
            .iter()
            .filter(|s| {
                s.bank == FlagBank::System
                    && s.kind == FlagKind::Set
                    && s.clean
                    && !s.text_alias
                    && !s.debug_menu
                    && !w.system_flag_test(s.flag)
                    && next_anchor_has(s.flag)
            })
            .map(|s| s.record)
            .collect()
    };
    let setters0 = wanted(0);
    let setters2 = wanted(2);
    let over0 = overreaching_records(session, mf, man, 0);
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let sets_wanted = |rec: usize| {
        setters0.contains(&rec)
            || (!over0.contains(&rec)
                && spawned_p2(mf, man, 0, rec, n0 + n1, 3)
                    .iter()
                    .any(|r| setters2.contains(r)))
    };
    w.props
        .bank
        .props
        .iter()
        .filter(|(_, p)| !p.collision_exempt())
        .filter(|(_, p)| sets_wanted(p.record))
        .map(|(&a, p)| {
            let at = if p.moving_box() { p.world } else { p.collider };
            (a, at, p.interact_gated())
        })
        .collect()
}

/// Walk into the touch-class prop anchored at `anchor` (a door: contact
/// result bit `4`, posted by the movement probe with no button): from each
/// tile beside it, hold the pad toward it until its record starts, then let
/// the run play out. `town0d` P0[1], the house door, sets `0x3B9` and spawns
/// the P2[28] song-night chain the first time it is opened.
fn touch_prop(session: &mut BootSession, anchor: (u8, u8), at: (i32, i32)) -> Run {
    let (tx, tz) = tile_of(at.0 as i16, at.1 as i16);
    let prop_run = |s: &BootSession| {
        s.host
            .world
            .dialog
            .inline
            .as_ref()
            .is_some_and(|id| id.prop_anchor == Some(anchor))
    };
    for (dx, dz) in [(-1i16, 0i16), (1, 0), (0, -1), (0, 1)] {
        let (sx, sz) = (tx + dx, tz + dz);
        if !(0..128).contains(&sx) || !(0..128).contains(&sz) {
            continue;
        }
        session.host.world.set_pad(0);
        session.host.world.seat_player_at_tile(sx as u8, sz as u8);
        let mut started = false;
        for _ in 0..40 {
            let pad = pad_for_step(session, -dx, -dz);
            session.host.world.set_pad(pad);
            match session.tick() {
                Ok(SceneTickEvent::SceneEntered { name }) => {
                    session.host.world.set_pad(0);
                    return Run::Entered(name);
                }
                Ok(_) => {}
                Err(e) => return Run::Error(format!("{e:#}")),
            }
            if prop_run(session) {
                started = true;
                break;
            }
        }
        session.host.world.set_pad(0);
        if started {
            return run_while_moving(session, DEEP_EXIT_TICKS);
        }
    }
    Run::Released
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
    let (px, pz) = player_xz(session);
    eprintln!(
        "    [beat] {} +{gained:?} (ends in {} {:?} at {:?} ({px},{pz}))",
        what(),
        scene_name(session),
        session.host.world.mode,
        tile_of(px, pz)
    );
}

thread_local! {
    /// The milestone a beats pass is played for, while that pass runs in
    /// the milestone's own scene with every waypoint behind it.
    static BEAT_TARGET: std::cell::RefCell<Option<Milestone>> =
        const { std::cell::RefCell::new(None) };
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
    session: &BootSession,
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
    let latching = |part: usize| -> BTreeSet<usize> {
        let sites = walk_partition_gflag_sites(mf, man, part);
        // A latch the record lowers again itself is a hold for its own
        // span, not a latch: `nilboa` P2[20] raises `0x00F` (the C1 gate of
        // the stepping-stone hops P2[8] / P2[9]) for the jump onto the
        // challenge ledge and clears it on landing, before it spawns the
        // Delilas family's challenge P2[21].
        // Only a record that carries the party along a chain of arcs (two
        // or more op `0x43` arcs on the `0xF8` channel: `nilboa`'s stepping
        // stones to the Koru ledge) is exempted. A stair band (`dohaty` P2[3..8]) holds `0x00F` the same
        // way around its walk legs, and a single-arc ledge (`jouind`
        // P2[0] / P2[6..9]) around one drop; both play nothing, and walking
        // one strands the hand on the far side.
        let base =
            mf.partitions.first().map_or(0, Vec::len) + mf.partitions.get(1).map_or(0, Vec::len);
        let carries = |rec: usize| {
            use legaia_asset::field_disasm::{ActorCtrlKind, InsnInfo, LinearWalker};
            use legaia_engine_core::man_field_scripts::partition_record_span;
            partition_record_span(mf, man, part, rec).is_some_and(|(start, pc0, len)| {
                LinearWalker::new(&man[start..start + len], pc0)
                    .flatten()
                    .filter(|i| {
                        i.extended == Some(0xF8)
                            && matches!(
                                i.info,
                                InsnInfo::ActorCtrl {
                                    kind: ActorCtrlKind::ArcJump { .. },
                                    ..
                                }
                            )
                    })
                    .count()
                    >= 2
            })
        };
        let lowered: BTreeSet<(usize, u16)> = sites
            .iter()
            .filter(|s| s.bank == FlagBank::System && s.kind == FlagKind::Clear)
            .filter(|s| carries(s.record))
            .map(|s| (s.record, s.flag))
            .collect();
        // A record that also raises a still-clear flag the next anchor
        // carries is the beat retail played, and its latch sits on another
        // branch: `concnow` P2[15], the third vision, raises `0x3EF` on the
        // visit the story takes and jumps past the `0x3D2` arm a later
        // visit plays.
        let w = &session.host.world;
        // A decode-coherent site counts even where its two bytes also read
        // as text (`nilboa` P2[25]'s `55 6D`, "Um"): the record's dialogue
        // is `0x1F`-framed, so the walk is past it.
        let wanted_by = |part: usize| -> BTreeSet<usize> {
            walk_partition_gflag_sites(mf, man, part)
                .iter()
                .filter(|s| {
                    s.bank == FlagBank::System
                        && s.kind == FlagKind::Set
                        && s.clean
                        && !s.debug_menu
                        && !w.system_flag_test(s.flag)
                        && NEXT_ANCHOR_FLAGS
                            .with(|a| a.borrow().as_ref().is_some_and(|f| f.contains(&s.flag)))
                })
                .map(|s| s.record)
                .collect()
        };
        let wanted_here = wanted_by(part);
        let wanted2 = wanted_by(2);
        // The chain counts: what the record spawns (three levels), and the
        // sequel the entry script spawns on a flag one of them raises
        // before a fight (`nilboa` P2[20] spawns the challenge P2[21],
        // whose `0x47A` has P1[0] spawn P2[25], which raises the `0x56D`
        // the next anchor carries).
        let set_by = |part: usize, rec: usize| -> BTreeSet<u16> {
            walk_partition_gflag_sites(mf, man, part)
                .iter()
                .filter(|s| {
                    s.record == rec && s.bank == FlagBank::System && s.kind == FlagKind::Set
                })
                .map(|s| s.flag)
                .collect()
        };
        let played: BTreeSet<usize> = sites
            .iter()
            .map(|s| s.record)
            .collect::<BTreeSet<usize>>()
            .into_iter()
            .filter(|&rec| {
                if wanted_here.contains(&rec) {
                    return true;
                }
                let spawned = spawned_p2(mf, man, part, rec, base, 3);
                let mut chain: Vec<(usize, usize)> = vec![(part, rec)];
                chain.extend(spawned.iter().map(|&r| (2, r)));
                spawned.iter().any(|r| wanted2.contains(r))
                    || chain.iter().any(|&(p, r)| {
                        entry_spawns_on(mf, man, &set_by(p, r))
                            .iter()
                            .any(|q| wanted2.contains(q))
                    })
            })
            .collect();
        sites
            .into_iter()
            .filter(|s| !played.contains(&s.record))
            .filter(|s| !lowered.contains(&(s.record, s.flag)))
            // Nor is a flag the record hands to the scene it leaves for,
            // whose entry script consumes it: `concnow` P2[18], the way into
            // Conkram's past, raises `0x423` and changes scene to `conc2`,
            // whose P1[0] tests and clears it.
            .filter(|s| !handoff_cleared(session, mf, man, part, s.record).contains(&s.flag))
            .filter(|s| {
                s.bank == FlagBank::System
                    && s.kind == FlagKind::Set
                    && s.clean
                    && !s.text_alias
                    && !s.debug_menu
                    && latches.contains(&s.flag)
                    && !next_anchor_has(s.flag)
            })
            .inspect(|s| {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!(
                        "    [latch] P{part}[{}] sets latch {:#05X}",
                        s.record, s.flag
                    );
                }
            })
            .map(|s| s.record)
            .collect()
    };
    let mut out = latching(partition);
    // A record also overreaches through what it spawns: `conc2` P2[11], the
    // King's audience, spawns that P2[12] as its epilogue.
    let latching2 = if partition == 2 {
        out.clone()
    } else {
        latching(2)
    };
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let n_part = mf.partitions.get(partition).map_or(0, Vec::len);
    for r in 0..n_part {
        if spawned_p2(mf, man, partition, r, n0 + n1, 3)
            .iter()
            .any(|s| latching2.contains(s))
        {
            out.insert(r);
        }
    }
    out
}

thread_local! {
    /// The system flags each scene's entry script (partition 1 record 0)
    /// clears, by scene name.
    static ENTRY_CLEARS: std::cell::RefCell<HashMap<String, BTreeSet<u16>>> =
        Default::default();
}

/// The flags record `(part, rec)` raises for the scene it changes to: those
/// the entry scripts of its `0x3F` destinations (other than this scene)
/// clear.
fn handoff_cleared(
    session: &BootSession,
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    part: usize,
    rec: usize,
) -> BTreeSet<u16> {
    use legaia_asset::field_disasm::{InsnInfo, LinearWalker, scene_change_name};
    use legaia_engine_core::man_field_scripts::{
        FlagBank, partition_record_span, walk_partition_gflag_sites,
    };
    let Some((start, pc0, len)) = partition_record_span(mf, man, part, rec) else {
        return BTreeSet::new();
    };
    let body = &man[start..start + len];
    let here = scene_name(session);
    let dests: BTreeSet<String> = LinearWalker::new(body, pc0)
        .flatten()
        .filter(|i| matches!(i.info, InsnInfo::SceneChange { .. }))
        .filter_map(|i| scene_change_name(body, &i))
        .map(|d| d.to_ascii_lowercase())
        .filter(|d| *d != here)
        .collect();
    let index = &session.host.index;
    dests
        .iter()
        .flat_map(|d| {
            ENTRY_CLEARS.with(|c| {
                c.borrow_mut()
                    .entry(d.clone())
                    .or_insert_with(|| {
                        let Some(dman) = Scene::load(index, d)
                            .ok()
                            .and_then(|sc| sc.field_man_payload(index).ok().flatten())
                        else {
                            return BTreeSet::new();
                        };
                        let Ok(dmf) = legaia_asset::man_section::parse(&dman) else {
                            return BTreeSet::new();
                        };
                        walk_partition_gflag_sites(&dmf, &dman, 1)
                            .iter()
                            .filter(|s| {
                                s.record == 0
                                    && s.bank == FlagBank::System
                                    && s.kind == FlagKind::Clear
                            })
                            .map(|s| s.flag)
                            .collect()
                    })
                    .clone()
            })
        })
        .collect()
}

/// Did the retail run leave boss stager placement `p1_record` alone before
/// the next milestone? Of the system flags its own record cleanly SETs and
/// no record of the scene ever CLEARs - flags that, once set, would still
/// show - the next anchor carries none. `town0b` P1[36], a loss-allowed
/// fight (`50 00` then `3E FF 03`), raises `0x5C0` / `0x5C1` before its
/// fight and the Hunter's Spring anchor has both clear; `town01` P1[10],
/// Tetsu's spar, is played for the `0x22E` its record also raises.
fn stager_overreaches(
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    p1_record: usize,
) -> bool {
    record_overreaches(mf, man, 1, p1_record, false)
}

/// [`stager_overreaches`] for any record: of the system flags record
/// `(partition, record)` cleanly SETs and no record of the scene ever
/// CLEARs, the next anchor carries none. A walk-on band that stages a fight
/// is held to it as a stager is: `deene` P2[18] raises `0x6B5` and installs
/// formation 11 (a 64800 HP fight), and no anchor from `zora_castle` on
/// carries `0x6B5` (cleared again by the entry script) or the `0x6B6` of
/// P2[19], the sequel the entry script spawns on it after the fight.
fn record_overreaches(
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    partition: usize,
    record: usize,
    follow_fight: bool,
) -> bool {
    use legaia_engine_core::man_field_scripts::{FlagBank, walk_partition_gflag_sites};
    let sites: Vec<_> = (0..3)
        .flat_map(|p| walk_partition_gflag_sites(mf, man, p))
        .filter(|s| s.bank == FlagBank::System && s.clean && !s.text_alias && !s.debug_menu)
        .collect();
    let cleared: BTreeSet<u16> = sites
        .iter()
        .filter(|s| s.kind == FlagKind::Clear)
        .map(|s| s.flag)
        .collect();
    let mut records = vec![(partition, record)];
    if follow_fight {
        // The fight's sequel: the entry script's post-battle return spawns
        // the record a pending flag of this one names.
        let pending: BTreeSet<u16> = sites
            .iter()
            .filter(|s| s.partition == partition && s.record == record && s.kind == FlagKind::Set)
            .map(|s| s.flag)
            .collect();
        records.extend(
            entry_spawns_on(mf, man, &pending)
                .into_iter()
                .map(|r| (2, r)),
        );
    }
    let lasting: BTreeSet<u16> = sites
        .iter()
        .filter(|s| {
            records.contains(&(s.partition, s.record))
                && s.kind == FlagKind::Set
                && s.flag != 0
                && !cleared.contains(&s.flag)
        })
        .map(|s| s.flag)
        .collect();
    !lasting.is_empty() && !lasting.iter().any(|&f| next_anchor_has(f))
}

/// The partition-2 records the scene's entry script (partition 1 record 0)
/// spawns right behind a test of one of `flags`: the post-battle hand-off a
/// pending flag set before `3E FF` arms (`deene` P1[0]: `76 B5`, `66 B5`,
/// `44 27`).
fn entry_spawns_on(
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    flags: &BTreeSet<u16>,
) -> BTreeSet<usize> {
    use legaia_asset::field_disasm::{FlagKind as Kind, InsnInfo, LinearWalker};
    use legaia_engine_core::man_field_scripts::partition_record_span;
    let mut out = BTreeSet::new();
    let Some((start, pc0, len)) = partition_record_span(mf, man, 1, 0) else {
        return out;
    };
    let base = mf.partitions.first().map_or(0, Vec::len) + mf.partitions.get(1).map_or(0, Vec::len);
    let n2 = mf.partitions.get(2).map_or(0, Vec::len);
    let mut armed = 0usize;
    for insn in LinearWalker::new(&man[start..start + len], pc0).flatten() {
        match insn.info {
            InsnInfo::SystemFlag {
                kind: Kind::Test,
                idx,
                ..
            } => armed = if flags.contains(&idx) { 4 } else { 0 },
            InsnInfo::SpawnRecord { global_index } if armed > 0 => {
                if let Some(r) = usize::from(global_index).checked_sub(base)
                    && r < n2
                {
                    out.insert(r);
                }
                armed = 0;
            }
            _ => armed = armed.saturating_sub(1),
        }
    }
    out
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
        // A venue cabinet's record runs the minigame door-warp (`3E` with
        // `op0 >= 100`): `balden` P1[24], Vidna's slot machine, raises
        // `0x5D2` on the way in. Its flags are the minigame's, not the
        // story's, and the hand does not play minigames.
        .filter(|&s| !record_enters_minigame(mf, man, 1, usize::from(s)))
        .filter(|&s| {
            // The talk's own record writes a wanted flag, or a partition-2
            // record it spawns does (`station`'s ticket seller spawns the
            // P2[19] departure that latches `0x36B`), or it hands a flag to
            // another scene's entry script (`suimon`'s water gate).
            setters1.contains(&usize::from(s))
                || spawned_p2(mf, man, 1, usize::from(s), n0 + n1, 3)
                    .iter()
                    .any(|r| setters2.contains(r))
                || hands_off_to_entry_script(session, mf, man, usize::from(s))
        })
        .collect();
    slots.sort_unstable();
    if pad_hand() {
        // A walking player talks to the nearest first: every tile walked is
        // an encounter roll.
        let (px, pz) = player_xz(session);
        let me = dispatch_tile(px, pz);
        slots.sort_by_key(|s| {
            w.npcs.positions.get(s).map_or(i32::MAX, |&(x, z)| {
                let t = dispatch_tile(x, z);
                (t.0 - me.0).abs() + (t.1 - me.1).abs()
            })
        });
    }
    slots
}

/// The live walk-on tiles of the loaded field scene whose record stages a
/// fight ([`record_stages_fight`]).
fn fight_band_tiles(session: &BootSession) -> HashSet<(i32, i32)> {
    use legaia_engine_core::man_field_scripts::partition2_record_gates;
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return HashSet::new();
    };
    let w = &session.host.world;
    let mut fights: HashMap<u8, bool> = HashMap::new();
    triggers
        .iter()
        .filter(|t| t.gate == 1)
        .filter(|t| {
            *fights.entry(t.record).or_insert_with(|| {
                partition2_record_gates(&mf, &man, usize::from(t.record))
                    .is_none_or(|(c1, c2)| w.p2_record_gates_pass(&c1, &c2))
                    && record_stages_fight(&mf, &man, 2, usize::from(t.record))
            })
        })
        .map(|t| (i32::from(t.tile_x), i32::from(t.tile_z)))
        .collect()
}

/// Whether record `(part, rec)`, or a partition-2 record it spawns, installs
/// a scripted battle (op `0x3E` with `op0 == 0xFF` or `< 100`).
fn record_stages_fight(
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    part: usize,
    rec: usize,
) -> bool {
    use legaia_asset::field_disasm::{InsnInfo, LinearWalker};
    use legaia_engine_core::man_field_scripts::partition_record_span;
    let fights = |part: usize, rec: usize| {
        partition_record_span(mf, man, part, rec).is_some_and(|(start, pc0, len)| {
            LinearWalker::new(&man[start..start + len], pc0)
                .flatten()
                .any(|i| matches!(i.info, InsnInfo::WarpOrInteract { is_warp: false, .. }))
        })
    };
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    fights(part, rec)
        || spawned_p2(mf, man, part, rec, n0 + n1, 3)
            .into_iter()
            .any(|r| fights(2, r))
}

/// Whether record `(part, rec)` runs the mode-24 minigame door-warp
/// (op `0x3E` with `op0 >= 100`, a venue cabinet's entry).
fn record_enters_minigame(
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    part: usize,
    rec: usize,
) -> bool {
    use legaia_asset::field_disasm::{InsnInfo, LinearWalker};
    use legaia_engine_core::man_field_scripts::partition_record_span;
    let Some((start, pc0, len)) = partition_record_span(mf, man, part, rec) else {
        return false;
    };
    LinearWalker::new(&man[start..start + len], pc0)
        .flatten()
        .any(|i| matches!(i.info, InsnInfo::WarpOrInteract { is_warp: true, .. }))
}

/// Whether talk record `P1[rec]` is a **hand-off**: it sets a system flag
/// the live state does not carry and changes scene, and the destination's
/// scene-entry script (its `P1[0]`) tests that flag. Such a talk is a story
/// beat whose effect lands in another scene, so no flag it writes itself is
/// one the next anchor shows: `suimon`'s Water Gate Controller (`P1[4]`)
/// sets `0x2C6` and enters `map01` at `(0, 0)`; `map01`'s `P1[0]` sees
/// `0x2C6` and spawns the drain cutscene `P2[15]`, which trades it for
/// `0x2C7` and returns the party to the drained chamber, where `suimon`'s
/// `P1[0]` trades that for `0x27B` - the flag that joins the two chambers.
fn hands_off_to_entry_script(
    session: &BootSession,
    mf: &legaia_asset::man_section::ManFile,
    man: &[u8],
    rec: usize,
) -> bool {
    use legaia_asset::field_disasm::{InsnInfo, LinearWalker, scene_change_name};
    use legaia_engine_core::man_field_scripts::{
        FlagBank, partition_record_span, walk_partition_gflag_sites,
    };
    let w = &session.host.world;
    let sets: BTreeSet<u16> = walk_partition_gflag_sites(mf, man, 1)
        .iter()
        .filter(|s| {
            s.record == rec
                && s.bank == FlagBank::System
                && s.kind == FlagKind::Set
                && s.clean
                && !s.text_alias
                && !s.debug_menu
                && !w.system_flag_test(s.flag)
        })
        .map(|s| s.flag)
        .collect();
    if sets.is_empty() {
        return false;
    }
    let Some((start, pc0, len)) = partition_record_span(mf, man, 1, rec) else {
        return false;
    };
    let body = &man[start..start + len];
    let here = scene_name(session);
    let dests: BTreeSet<String> = LinearWalker::new(body, pc0)
        .flatten()
        .filter(|i| matches!(i.info, InsnInfo::SceneChange { .. }))
        .filter_map(|i| scene_change_name(body, &i))
        .filter(|d| *d != here)
        .collect();
    let index = &session.host.index;
    dests.iter().any(|d| {
        let Some(dman) = Scene::load(index, d)
            .ok()
            .and_then(|sc| sc.field_man_payload(index).ok().flatten())
        else {
            return false;
        };
        let Ok(dmf) = legaia_asset::man_section::parse(&dman) else {
            return false;
        };
        walk_partition_gflag_sites(&dmf, &dman, 1).iter().any(|s| {
            s.record == 0
                && s.bank == FlagBank::System
                && s.kind == FlagKind::Test
                && sets.contains(&s.flag)
        })
    })
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
    if pad_hand() {
        return pad_talk_to(session, slot);
    }
    let Some(&(nx, nz)) = session.host.world.npcs.positions.get(&slot) else {
        return Run::Released;
    };
    interact_at(
        session,
        (nx, nz),
        &|w: &legaia_engine_core::world::World| w.field_interact_probe_slot() == Some(slot),
    )
}

/// Examine the interact-gated prop anchored at `anchor` (the cupboard class,
/// retail `FUN_801CFC40` result bit `1`): the same stand-beside, face and
/// press-Cross approach as a talk, aimed so the prop arm of the facing probe
/// (`FUN_801CF9F4`) lands on its contact box.
fn examine_prop(session: &mut BootSession, anchor: (u8, u8), at: (i32, i32)) -> Run {
    let pos = (at.0 as i16, at.1 as i16);
    interact_at(session, pos, &|w: &legaia_engine_core::world::World| {
        w.field_interact_probe_slot().is_none() && w.field_interact_prop_anchor() == Some(anchor)
    })
}

/// The approach [`talk_to`] and [`examine_prop`] share: from each tile
/// beside `(nx, nz)`, face the sector where `hits` holds, press Cross and
/// page what opens.
fn interact_at(
    session: &mut BootSession,
    (nx, nz): (i16, i16),
    hits: &dyn Fn(&legaia_engine_core::world::World) -> bool,
) -> Run {
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
            hits(&session.host.world)
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
        // A talk that hands the frame to a scripted beat - a record it
        // spawned, queued or already running - leaves the player to that
        // beat: `station`'s ticket seller spawns P2[19], whose first act is
        // to walk the player from the counter (`C7 F8`) to the cart.
        // Teleporting the player back to where it stood before the talk
        // strands that walk across the map.
        let w = &session.host.world;
        let beat_follows = !w.field_vm.pending_record_spawns.is_empty()
            || !w.field_vm.helper_contexts.is_empty()
            || w.cutscene.timeline.is_some();
        if walking(session) && !beat_follows {
            restore_player_xz(session, (bx, bz));
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
        restore_player_xz(session, (bx, bz));
    }
    last
}

/// Put the player back on the exact spot it stood before an interaction.
/// Re-seating on the centre of `tile_of` the spot instead moved a player
/// standing off-centre onto the neighbouring tile, and against a wall that
/// tile can be solid: `jiji`'s prop examine left the party at (69, 71),
/// walled on all four sides, after starting from (9020, 9216).
fn restore_player_xz(session: &mut BootSession, (x, z): (i16, i16)) {
    let w = &mut session.host.world;
    let y = w.sample_field_floor_height(i32::from(x), i32::from(z)) as i16;
    let slot = w.player_actor_slot.unwrap_or(0) as usize;
    if let Some(a) = w.actors.get_mut(slot) {
        a.move_state.world_x = x;
        a.move_state.world_y = y;
        a.move_state.world_z = z;
    }
}

/// Page an open conversation to its end with [`script_pad`]. `None` when it
/// ended with the frame back on the field; `Some(run)` for anything else.
fn page_talk(session: &mut BootSession) -> Option<Run> {
    for f in 0..DEEP_EXIT_TICKS {
        if let Err(e) = pad_budget(session) {
            return Some(Run::Parked(e));
        }
        if session.host.world.mode == SceneMode::Battle {
            if let Some(r) = drain_battle(session) {
                return Some(r);
            }
            continue;
        }
        let w = &session.host.world;
        if f >= 2 && w.dialog.inline.is_none() && !w.dialogue_owns_input() {
            return None;
        }
        let pad = script_pad(session, f);
        session.host.world.set_pad(pad);
        match session.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Some(Run::Entered(name)),
            Ok(_) => {}
            Err(e) => return Some(Run::Error(format!("{e:#}"))),
        }
    }
    Some(Run::Parked(format!(
        "{} at {}",
        holder(session),
        park_site(session)
    )))
}

/// [`talk_to`] with the pad only: walk to a free tile beside the NPC, lean
/// toward it until the retail interact probe lands on it (the press turns
/// the player; the NPC's body stops the step), release, press Cross, page
/// the conversation, then turn and step away so the confirm cannot re-open
/// the same talk. Each side is tried until one gains a flag, as the seated
/// hand does.
fn pad_talk_to(session: &mut BootSession, slot: u8) -> Run {
    if !session.host.world.npcs.positions.contains_key(&slot) {
        return Run::Released;
    }
    let avoid = pad_avoid(session, None);
    let mut last = Run::Released;
    let mut talked = false;
    // Up to the NPC first (a counter or a table often fills the tile
    // beside it, and the probe reaches over one), then from each side. The
    // NPC's tile is re-read per attempt: a routed NPC walks while the
    // player does.
    for attempt in 0..5usize {
        let Some(&(nx, nz)) = session.host.world.npcs.positions.get(&slot) else {
            break;
        };
        let npc_tile = {
            let t = dispatch_tile(nx, nz);
            (t.0 as i16, t.1 as i16)
        };
        let (side, within) = if attempt == 0 {
            (npc_tile, 1)
        } else {
            match beside(session, npc_tile).get(attempt - 1) {
                Some(&t) => (t, 0),
                None => break,
            }
        };
        let walk = pad_walk(session, side, &avoid, within);
        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
            eprintln!(
                "      [talk-walk] P1[{slot}] attempt {attempt} to {side:?}: {:?} at frame {}",
                walk.as_ref()
                    .map_err(|e| e.chars().take(100).collect::<String>()),
                session.frames
            );
        }
        match walk {
            Ok(Walk::Entered(s)) => return Run::Entered(s),
            Ok(Walk::Arrived) => {}
            Err(e) => {
                if !talked {
                    last = Run::Parked(format!("pad walk to talk P1[{slot}]: {e}"));
                }
                // A walk stopped by the counter in front of the NPC still
                // leans in: the probe decides whether it reaches over
                // (`concnow` P1[12] / P1[13], the gate guards at (90, 119),
                // behind the wall row 118 their talk paints away).
                let (px, pz) = player_xz(session);
                let me = dispatch_tile(px, pz);
                let near = (me.0 - i32::from(npc_tile.0)).abs() <= 2
                    && (me.1 - i32::from(npc_tile.1)).abs() <= 2;
                if attempt > 0 || !near {
                    continue;
                }
            }
        }
        let npc = move |s: &BootSession| s.host.world.npcs.positions.get(&slot).copied();
        let facing = |s: &BootSession| s.host.world.field_interact_probe_slot() == Some(slot);
        if let Some(scene) = pad_lean(session, npc, 48, facing) {
            return Run::Entered(scene);
        }
        if !facing(session) {
            if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                eprintln!(
                    "      [talk-face] P1[{slot}] player {:?} npc {:?} probe {:?}",
                    player_xz(session),
                    session.host.world.npcs.positions.get(&slot),
                    session.host.world.field_interact_probe_slot()
                );
            }
            if !talked {
                let w = &session.host.world;
                last = Run::Parked(format!(
                    "could not face P1[{slot}] from {side:?}: player {:?} npc {:?} probe hits {:?} holder {}",
                    player_xz(session),
                    w.npcs.positions.get(&slot),
                    w.field_interact_probe_slot(),
                    holder(session)
                ));
            }
            continue;
        }
        let before = flags_of_world(session);
        // Release, then press: the interact is edge-triggered.
        session.host.world.set_pad(0);
        if let Ok(SceneTickEvent::SceneEntered { name }) = session.tick() {
            return Run::Entered(name);
        }
        session.host.world.set_pad(PadButton::Cross.mask());
        match session.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Run::Entered(name),
            Ok(_) => {}
            Err(e) => return Run::Error(format!("{e:#}")),
        }
        talked = true;
        if let Some(r) = page_talk(session) {
            return r;
        }
        let w = &session.host.world;
        let beat_follows = !w.field_vm.pending_record_spawns.is_empty()
            || !w.field_vm.helper_contexts.is_empty()
            || w.cutscene.timeline.is_some();
        if walking(session) && !beat_follows {
            // Turn away and step off, as a player does before pressing on.
            let (px, pz) = player_xz(session);
            let away = move |_: &BootSession| {
                Some((
                    px.saturating_add(px.saturating_sub(nx).signum() * 256),
                    pz.saturating_add(pz.saturating_sub(nz).signum() * 256),
                ))
            };
            if let Some(scene) = pad_lean(session, away, 12, |_| false) {
                return Run::Entered(scene);
            }
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
    // The flags each scene's beats pass left behind (pad tier): a scene
    // the story came back to with new flags has new beats to play.
    // `retona`: the Songi fight, the P2[17] hop to `map02` and back, and
    // the summit scene P2[18] all come before the party can work the
    // summit's wall switches (P2[4] / P2[5]) toward the way down.
    let mut beaten_flags: BTreeMap<String, BTreeSet<u16>> = BTreeMap::new();
    // Crossing detours taken per (scene, goal). A detour that ends in a
    // third scene (`rikuroa` turns the party away and the walk out lands
    // by `cave01`) leaves the player on another side of `cur`, from which
    // a further crossing is the way on (`keikoku`'s west mouth); a detour
    // that came back to `cur`, or found no crossing at all, is not retried.
    let mut crossed: BTreeMap<(String, String), usize> = BTreeMap::new();
    // Both tiers play the same beats and waypoints; the pad tier plays them
    // with pad input only ([`PAD_HAND`]).
    PAD_HAND.with(|h| h.set(pad));
    RESTED.with(|r| r.borrow_mut().clear());
    REFUSED_DOORS.with(|r| r.borrow_mut().clear());
    TURNBACK.with(|t| t.borrow_mut().clear());
    RAN_ON_WALK.with(|t| t.borrow_mut().clear());
    PLAYING_BEAT.with(|b| b.set(None));
    // Each pass answers pickers from a fresh rotation: a count carried over
    // from the seated pass (or an earlier segment) made the pad hand's
    // first answer depend on what else had run in the process (`nilboa`
    // P2[0]'s "are you ready?" opened on its second option, "No").
    PICKS.with(|p| *p.borrow_mut() = (HashMap::new(), None));
    let mut via = 0usize;
    let vias: &[String] = &target.via;
    // Edges whose hop failed even after the scene's beats ran: the ladder
    // routes around them (see the `dead` arm below).
    let mut dead: BTreeSet<(String, String)> = BTreeSet::new();
    let mut steps = 0usize;
    while steps < MAX_HOPS + vias.len() * 4 + dead.len() * 4 {
        steps += 1;
        pad_budget(session)?;
        // Let whatever the last landing started finish first.
        let here = scene_name(session);
        match run_while_moving(session, SCRIPT_CEILING) {
            Run::Entered(s) => {
                // A waypoint whose own arrival script carries the party on
                // (`concend`'s P2[0] ends in the hop to `town0d`) was
                // visited: its beat is that script.
                if vias.get(via).is_some_and(|w| *w == here) {
                    via += 1;
                    trail.push(format!("[via {here}: arrival script]"));
                }
                trail.push(format!("{s}(scripted)"));
                // A milestone scene a scripted chain passes through is
                // reached: the ending's `edteien` is one link of the credits
                // chain (`edteien > edbylon > ... > edlast`), and its anchor
                // is a state taken mid-cutscene there, not a walkable stop.
                if via >= vias.len() && reached(session, target) {
                    return Ok(());
                }
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
        let mut next = String::new();
        let hop = if stuck_here {
            Err(String::new())
        } else {
            let Some(route) = graph.route_avoiding(&cur, &goal, &dead) else {
                return Err(format!("no scene route from {cur} to {goal}"));
            };
            next = route[1].clone();
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
            Err(e)
                if pad && e.contains("no walkable path") && {
                    let n = crossed.entry((cur.clone(), goal.clone())).or_insert(0);
                    *n += 1;
                    *n <= MAX_CROSS_OVERS
                } =>
            {
                match cross_over(
                    session,
                    graph,
                    &cur,
                    &goal,
                    &CrossTarget::Door(next.clone()),
                ) {
                    Ok(s) => {
                        // Only a detour that ended in a third scene earns
                        // another: one that came back to `cur` already
                        // played every crossing its lattice named.
                        if s == cur {
                            crossed.insert((cur.clone(), goal.clone()), MAX_CROSS_OVERS);
                        }
                        trail.push(format!("{s}(crossing)"));
                    }
                    Err(why) => {
                        if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                            eprintln!("    [cross] {e}; {why}");
                        }
                        // Try the scene's beats next, as for any failed hop.
                        beaten.remove(&cur);
                        crossed.insert((cur.clone(), goal.clone()), MAX_CROSS_OVERS);
                        continue;
                    }
                }
            }
            Err(e) => {
                if pad
                    && beaten.contains(&cur)
                    && beaten_flags
                        .get(&cur)
                        .is_some_and(|f| *f != flags_of_world(session))
                {
                    beaten.remove(&cur);
                }
                if beaten.insert(cur.clone()) {
                    if !e.is_empty() && std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                        eprintln!("    [hop] {e}; playing {cur}'s beats");
                    }
                    let mut log = Vec::new();
                    // Only the milestone's own beats pass may stop short of
                    // a fight it committed (see [`fight_committed`]).
                    let aim = (via >= vias.len()).then(|| target.clone());
                    BEAT_TARGET.with(|t| *t.borrow_mut() = aim);
                    let left = play_beats(session, &mut log);
                    BEAT_TARGET.with(|t| *t.borrow_mut() = None);
                    let left =
                        left.map_err(|b| format!("in {cur}, playing beats (after {e}): {b}"))?;
                    beaten_flags.insert(cur.clone(), flags_of_world(session));
                    trail.push(format!("[{}]", log.join("; ")));
                    if let Some(s) = left {
                        trail.push(format!("{s}(beat)"));
                    }
                    continue;
                }
                // The beats did not open the hop either: the edge is one
                // the story no longer takes (a one-shot transport - `station`
                // P2[23], the chapter-3 cart crash into `map03`, spawns only
                // while `0x36C` is clear), or one it has not opened yet (the
                // `rikuroa` P2[57] warp to `uru` waits on `0x3BC`). Route
                // around it when another way exists.
                if !stuck_here
                    && dead.insert((cur.clone(), next.clone()))
                    && graph.route_avoiding(&cur, &goal, &dead).is_some()
                {
                    if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                        eprintln!("    [hop] {cur} -> {next} is dead; rerouting: {e}");
                    }
                    trail.push(format!("[{cur}->{next} dead]"));
                    continue;
                }
                // The beat that sets a missing reach flag lies in another walk
                // component of the scene: a round trip through a neighbouring
                // scene that lands on its side (`chitei2`'s junction band
                // P2[3] is reached by re-entering from `deroa` at (93, 108)),
                // then the beats again.
                if stuck_here && pad {
                    let flags: Vec<u16> = target
                        .reach_flags
                        .iter()
                        .copied()
                        .filter(|&f| !session.host.world.system_flag_test(f))
                        .collect();
                    // The bands of the flags no reachable beat sets.
                    let bands: Vec<(i16, i16)> = flags
                        .iter()
                        .map(|&f| reach_flag_bands(session, &[f]))
                        .filter(|b| !tiles_in_reach(session, b))
                        .flatten()
                        .collect();
                    let n = crossed
                        .entry((cur.clone(), String::from("<band>")))
                        .or_insert(0);
                    *n += 1;
                    if !bands.is_empty() && *n <= MAX_CROSS_OVERS {
                        let t = CrossTarget::Tiles(bands);
                        match cross_over(session, graph, &cur, "", &t) {
                            Ok(s) => {
                                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                                    eprintln!("    [cross] {cur} to the reach band: landed in {s}");
                                }
                                trail.push(format!("{s}(band crossing)"));
                                beaten.remove(&s);
                                continue;
                            }
                            Err(why) => {
                                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                                    eprintln!("    [cross] {cur} to the reach band: {why}");
                                }
                            }
                        }
                    }
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
    // The last hop's landing is read like every other: the loop tests
    // `reached` at the top of a pass, and a hop that lands on the milestone
    // as the budget runs out (`kor5`, the Sol Tower summit, entered on the
    // sixteenth) would otherwise be called lost on arrival.
    if via >= vias.len() && reached(session, target) {
        return Ok(());
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
        // `LEGAIA_FGL_SEATED_RNG_SEED=<u32>`: deal the seated pass another
        // hand, as `LEGAIA_FGL_RNG_SEED` does the pad pass - a seated boss
        // fight (`chitei2` P2[13], Jette) is checked across streams with it.
        if let Some(s) = std::env::var("LEGAIA_FGL_SEATED_RNG_SEED")
            .ok()
            .and_then(|s| s.trim().parse().ok())
        {
            session.host.world.rng_state = s;
        }
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
            Ok(()) => {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!("    [seated] trail {}", trail.join(">"));
                }
                (Tier::Progresses, None, Some(flags))
            }
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
            PAD_DEADLINE.with(|d| d.set(session.frames + PAD_SEGMENT_FRAMES));
            PAD_BATTLE_LEFT.with(|l| l.set(PAD_BATTLE_ALLOWANCE));
            // `LEGAIA_FGL_RNG_SEED=<u32>`: deal the pad tier another hand.
            // Every random draw it meets comes off the world rand stream, so
            // a re-seeded run is how a pad route is checked for depending on
            // one stream's luck (see "A pad wipe that moves with an
            // unrelated change" in the ladder docs).
            if let Some(s) = std::env::var("LEGAIA_FGL_RNG_SEED").ok().and_then(|s| {
                let s = s.trim();
                s.strip_prefix("0x")
                    .map_or_else(|| s.parse().ok(), |h| u32::from_str_radix(h, 16).ok())
            }) {
                session.host.world.rng_state = s;
            }
            if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                eprintln!(
                    "    [pad] seeded {} at {:?} (anchor seat {:?})",
                    scene_name(&session),
                    player_xz(&session),
                    from_anchor.and_then(|a| a.seat)
                );
            }
            let start = session.frames;
            let mut trail = vec![scene_name(&session)];
            let r = traverse(&mut session, graph, to, true, &mut trail)
                .map_err(|e| format!("{e} [trail {}]", trail.join(">")));
            if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                let (n, cells) = PLAN_STATS.with(std::cell::Cell::take);
                eprintln!(
                    "    [pad] {} frames, {n} plans over {cells} cells",
                    session.frames - start
                );
            }
            r
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

/// The planner's parent links stay acyclic through a teleport waypoint, and
/// the walk-back refuses a cycle rather than growing without bound. Before
/// the waypoint's cost was recorded, a plain step into it from past the
/// landing overwrote its parent and the walk-back looped forever (the
/// kor3 pad hop's memory runaway). Disc-free.
#[test]
fn plan_search_parent_links_stay_acyclic() {
    let (a, v, land, past) = ((0, 0), (1, 0), (9, 9), (2, 0));
    let mut s = Search {
        parent: HashMap::from([(a, a)]),
        g: HashMap::from([(a, 0)]),
        open: std::collections::BinaryHeap::new(),
        goal: (20, 20),
    };
    // a -> (waypoint v) -> land, then land -> ... -> past, then past -> v.
    s.step(Some(v), a, land, 2);
    s.step(None, land, past, 3);
    s.step(None, past, v, 4);
    assert_eq!(
        s.parent[&v], a,
        "a dearer step must not re-parent the waypoint"
    );
    assert_eq!(walk_back(&s.parent, a, past), Some(vec![v, land, past]));
    // A cyclic map (the shape the defect produced) ends, it does not loop.
    let cyclic = HashMap::from([(a, a), (v, past), (land, v), (past, land)]);
    assert_eq!(walk_back(&cyclic, a, past), None);
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
    // `LEGAIA_FGL_EDGES=<scene>,...`: each scene's out- and in-edges.
    if let Ok(names) = std::env::var("LEGAIA_FGL_EDGES") {
        for n in names.split(',').map(str::trim) {
            let out: Vec<String> = graph
                .edges
                .get(n)
                .into_iter()
                .flatten()
                .map(|d| {
                    let walk = graph.walk_on.contains(&(n.to_string(), d.clone()));
                    format!("{d}{}", if walk { "" } else { "*" })
                })
                .collect();
            let inn: Vec<&String> = graph
                .edges
                .iter()
                .filter(|(_, ds)| ds.contains(n))
                .map(|(s, _)| s)
                .collect();
            eprintln!("[edges] {n} -> {out:?} (* = scripted); <- {inn:?}");
        }
    }

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

    // `LEGAIA_FGL_GAINED=<id>,...`: every flag the milestone's anchor gained
    // over its predecessor (and lost), with every disc SET site - the
    // evidence a waypoint is chosen from.
    if let Ok(ids) = std::env::var("LEGAIA_FGL_GAINED") {
        let ids: BTreeSet<&str> = ids.split(',').map(str::trim).collect();
        for (i, m) in spine.iter().enumerate() {
            if i == 0 || !ids.contains(m.id.as_str()) {
                continue;
            }
            let (Some(prev), Some(cur)) = (&anchors[i - 1], &anchors[i]) else {
                continue;
            };
            let show = |f: &u16| {
                let sites: Vec<String> = census
                    .get(f)
                    .into_iter()
                    .flatten()
                    .filter(|s| s.kind == FlagKind::Set)
                    .map(|s| {
                        format!(
                            "{} P{}[{}]{}",
                            s.scene_name,
                            s.partition,
                            s.record,
                            if s.clean { "" } else { "?" }
                        )
                    })
                    .collect();
                format!("0x{f:03X} <- {}", sites.join(" / "))
            };
            eprintln!("[gained] {}:", m.id);
            for f in cur.flags.difference(&prev.flags) {
                eprintln!("    + {}", show(f));
            }
            for f in prev.flags.difference(&cur.flags) {
                eprintln!("    - {}", show(f));
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
        let to = &spine[i + 1];
        if let Some(only) = &only
            && !only.contains(&to.id)
        {
            continue;
        }
        let seed_at = (0..=i).rev().find(|&j| spine[j].seeds_next).unwrap_or(i);
        let from = &spine[seed_at];
        let t0 = std::time::Instant::now();
        let rep = run_segment(
            &inp,
            &graph,
            from,
            anchors[seed_at].as_ref(),
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
                .take(if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    usize::MAX
                } else {
                    6
                })
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
