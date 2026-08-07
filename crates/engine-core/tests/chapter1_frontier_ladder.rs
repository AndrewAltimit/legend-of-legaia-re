//! Per-scene frontier ladder: **which chapter-1 scenes the engine can load,
//! script, walk and leave** - one verdict per scene, scored on a rung ladder
//! and printed as a table.
//!
//! ## Why this is not `scene_chain_e2e`
//!
//! [`scene_chain_e2e`](scene_chain_e2e.rs) walks every CDNAME scene's typed
//! sub-assets (MES / SEQ / TMD / VAB) and proves they compose. That is an
//! **asset-layer** claim and it is silent about whether the scene *runs*: a
//! scene whose MAN never parses, whose entry script parks on its first
//! instruction, or whose floor is sealed passes it unchanged.
//!
//! It is also not `critical_path_replay` (engine-shell), which is one route
//! walked by pad and scored as a single number. That answers "how far can a
//! player get"; this answers "of the scenes chapter 1 reaches, which ones
//! work" - a breadth question the route ladder cannot express, because a
//! route visits five scenes and says nothing about the other thirty.
//!
//! And it is not the three Drake hub oracles (`chapter1_hub_sweep_oracle`,
//! `chapter1_hub_breadth_oracle`, `chapter1_hub_depth_oracle`, all in
//! engine-shell). Those hand-pick between one and five named legs and pin
//! exact facts about each. Nothing enumerated the *whole* chapter-1 reachable
//! set and gave every member a verdict, so "the engine stops at the Ravine"
//! and "no fixture drives past the Ravine" were indistinguishable.
//!
//! ## The scene graph comes from the disc
//!
//! The scene set is **not** a hand-written list. It is the BFS closure of
//! `town01` over each scene's own decoded `0x3F` named-scene-change
//! destinations ([`scene_destinations`] - the partition-1 destination table
//! plus the partition-2 door records). Expansion stops at a **kingdom
//! boundary**: a destination that is another kingdom overworld (`mapNN` other
//! than `map01`) is recorded as a boundary edge and never expanded, because
//! `map02` / `map03` are the chapter-2 / chapter-3 arcs and have their own
//! oracles. Everything else the closure touches is swept.
//!
//! Two things this set is not. It is a **reachability** partition taken from
//! the disc, not a narrative one: the Drake kingdom is what `map01` reaches,
//! and `scripts/scenarios.toml` labels some of its members (`garmel`) chapter
//! 2. And it is a closure over `0x3F` only - the sibling `0x3E` door warp
//! carries a 7-id scene-*type* selector rather than a name, so Rim Elm's
//! house interiors are not in it. That is a limit of what the disc says in
//! bytes, not a claim that those scenes do not work.
//!
//! ## The ladder
//!
//! Rungs are ordered and cumulative - a scene's score is the count it cleared
//! before the first one it did not:
//!
//! | # | rung | what it proves |
//! |---|--------|-------------------------------------------------------|
//! | 1 | assets | `Scene::load` resolves and a MAN payload comes back |
//! | 2 | man    | the MAN parses: partition table + `0x3F` destinations |
//! | 3 | enter  | `SceneHost` enters it in `Field` / `WorldMap` |
//! | 4 | script | its entry script settles and hands control back |
//! | 5 | walk   | pad-only input displaces the player at least a tile |
//! | 6 | exit   | an exit record fires and lands in the scene it names |
//!
//! Rung 4 extends this repo's own definition of "control released" - neither
//! [`World::cutscene_timeline_active`] nor [`World::dialogue_owns_input`],
//! the predicate `critical_path_replay`'s rung 1 scores on - with "and no
//! spawned record still running". The third clause is not cosmetic: a
//! first-visit record is a helper context rather than a timeline, so the
//! two-clause version returned while `izumi`'s spring choreography was still
//! moving the player. A scene
//! whose entry script *leaves* the scene on its own (a scripted `0x3F` with
//! no player input) clears rung 4 and stops there: rungs 5 and 6 are marked
//! `~` (not applicable) rather than failed, and the note names the
//! destination. The score credits **only rungs actually demonstrated**, so
//! such a scene scores 4 - an `~` is not a pass. The summary reports how many
//! scenes ended that way, so the shortfall is not read as engine failure.
//!
//! Rung 5 is pad-only: [`World::set_pad`] plus [`SceneHost::tick`], nothing
//! seated - and it is scored against a **released-pad control run of the same
//! length from the same state**. Without that control, "the player walked"
//! and "a script moved the player" are one measurement, and they are not one
//! claim. When the driven run fails to beat its control the scene is probed
//! once more as a *revisit* (flag banks left latched), which separates "the
//! engine cannot walk here" from "this scene's first-visit script owns the
//! player"; the rung still scores the first visit, because that is the visit
//! a player makes first.
//!
//! Rung 6 is **not** pad-only and does not claim to be - the player is seated
//! onto the exit's own `.MAP` trigger tile (the teleport-pair the spine
//! oracle uses) and the rung scores the transition and its landing, not the
//! locomotion to the door. Walking to a door under pad is
//! `critical_path_replay`'s job, and it is a different and harder claim.
//!
//! A stall is a finding, not a bare assertion failure. A rung-4 park reports
//! `(pc, opcode)` off the live field VM; a rung-5 stall reports the tile the
//! player died on; a rung-6 failure names the destination the record claimed
//! and what was entered instead.
//!
//! ## What rung 6 does and does not gate
//!
//! The rung is cleared when the entered scene is the one the record's `0x3F`
//! *names*. Whether the arrival **tile** matches the record's `entry_x` /
//! `entry_z` is measured and reported in the summary as its own count, but it
//! does not gate the rung: the arrival seat is separate machinery from the
//! transition, and folding them together would make one number answer two
//! questions.
//!
//! ## Ratchet
//!
//! `scripts/replays/chapter1_frontier_baseline.toml` carries three numbers -
//! the closure size, the summed rung score, and the count of scenes that
//! cleared the whole ladder. Each is asserted `>=`, so a decode or engine
//! regression that shrinks any of them fails the run. Raising them is a
//! reviewed edit, never an auto-write; the test prints the block to paste.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset,
//! `extracted/` missing. Part C additionally skips when `saves/library` or
//! `scripts/scenarios.toml` is absent - a disc-gated test skips on every
//! input it reads, not only on the disc.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;

use legaia_asset::man_section::{ManFile, parse as parse_man};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::man_field_scripts::{overworld_portal_sites, scene_destinations};
use legaia_engine_core::scene::{
    DefaultMapIdResolver, ProtIndex, Scene, SceneHost, SceneTickEvent, is_world_map_scene,
};
use legaia_engine_core::world::SceneMode;

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn library_dir() -> Option<PathBuf> {
    for c in ["saves/library", "../saves/library", "../../saves/library"] {
        let d = PathBuf::from(c);
        if d.is_dir() {
            return Some(d);
        }
    }
    None
}

fn manifest_path() -> Option<PathBuf> {
    for c in [
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ] {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Open a `SceneHost` with a real New Game party seeded.
///
/// The roster matters even in a breadth sweep: the walk probe can roll a
/// random encounter, and an unseeded party is the port-only state whose
/// members have `max_hp == 0` - the first hit "kills" them and the sweep
/// would be measuring a corpse's locomotion. Same seed the critical-path
/// ladder uses (the SCUS `0x80078C4C` template).
fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    if let Ok(scus) = std::fs::read(extracted.join("SCUS_942.54"))
        && let Some(party) = legaia_asset::new_game::StartingParty::from_scus(&scus)
    {
        host.world.seed_starting_party(&party);
    }
    Some(host)
}

// ---------------------------------------------------------------------------
// The disc-sourced chapter-1 scene graph
// ---------------------------------------------------------------------------

/// Where the closure starts: Rim Elm, the scene a cold New Game hands off to.
const CH1_ROOT: &str = "town01";

/// The chapter-1 kingdom overworld. Any *other* `mapNN` is a boundary: it is
/// the next chapter's arc and has its own spine oracle.
const CH1_OVERWORLD: &str = "map01";

/// Hard cap on the closure, so a decode regression that starts naming
/// phantom scenes cannot turn the sweep into an unbounded run. The assert
/// below fails if it is ever hit.
const MAX_CLOSURE: usize = 96;

/// A scene's MAN through the engine's own resolution order (bundle first,
/// streaming variant carrier fallback) - the path the live host uses.
fn scene_man(index: &ProtIndex, name: &str) -> Option<(ManFile, Vec<u8>)> {
    let scene = Scene::load(index, name).ok()?;
    let man = scene.field_man_payload(index).ok()??;
    let mf = parse_man(&man).ok()?;
    Some((mf, man))
}

/// The chapter-1 scene graph: scenes in BFS order, each scene's decoded
/// destination set, and the kingdom-boundary edges that were deliberately not
/// expanded.
struct Closure {
    /// Scenes in BFS order from [`CH1_ROOT`].
    order: Vec<String>,
    /// Every decoded `0x3F` edge, including the boundary ones.
    edges: BTreeMap<String, BTreeSet<String>>,
    /// `(from, to)` where `to` is another kingdom's overworld.
    boundary: BTreeSet<(String, String)>,
}

/// The BFS closure of [`CH1_ROOT`] over the disc's own `0x3F` destinations,
/// refusing to expand past a kingdom boundary.
fn chapter1_closure(index: &ProtIndex) -> Closure {
    let mut order: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut boundary: BTreeSet<(String, String)> = BTreeSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();

    seen.insert(CH1_ROOT.to_string());
    queue.push_back(CH1_ROOT.to_string());
    while let Some(name) = queue.pop_front() {
        order.push(name.clone());
        if order.len() > MAX_CLOSURE {
            break;
        }
        let Some((mf, man)) = scene_man(index, &name) else {
            // A leaf whose MAN does not resolve: it still gets a verdict row
            // (rung 1 / 2 will say so), it just cannot contribute edges.
            continue;
        };
        for d in scene_destinations(&mf, &man) {
            let dest = d.scene_name;
            if dest == name {
                continue;
            }
            edges.entry(name.clone()).or_default().insert(dest.clone());
            if is_world_map_scene(&dest) && dest != CH1_OVERWORLD {
                boundary.insert((name.clone(), dest));
                continue;
            }
            if seen.insert(dest.clone()) {
                queue.push_back(dest);
            }
        }
    }
    Closure {
        order,
        edges,
        boundary,
    }
}

// ---------------------------------------------------------------------------
// Rungs
// ---------------------------------------------------------------------------

const RUNGS: [&str; 6] = ["assets", "man", "enter", "script", "walk", "exit"];

/// Ticks a scene's entry script gets to settle before it is called parked.
/// Comfortable headroom over the longest first-visit record the sweep meets
/// (izumi's spring choreography needs ~1500 sim ticks; those scenes report a
/// scripted departure instead and never reach the cap).
const SETTLE_TICKS: usize = 2800;

/// Ticks each of the four pad directions is held in the walk probe.
const WALK_TICKS: usize = 60;

/// Ticks an exit gets to fire after the player is seated on its trigger tile.
const EXIT_TICKS: usize = 24;

/// Exit sites tried per scene before the rung is called failed. Sites are
/// tried in `.MAP` trigger order and the first success wins.
const EXIT_SITES_TRIED: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    /// Cleared.
    Pass,
    /// Attempted and failed - the note says how.
    Fail,
    /// Not attempted: an earlier rung failed.
    Skip,
    /// Not applicable - the scene's script leaves before the rung can run.
    Na,
}

impl Mark {
    fn glyph(self) -> char {
        match self {
            Self::Pass => '*',
            Self::Fail => 'X',
            Self::Skip => '.',
            Self::Na => '~',
        }
    }
}

struct Verdict {
    scene: String,
    marks: [Mark; 6],
    /// Rungs cleared in order before the first non-`Pass`.
    score: usize,
    /// True when the ladder stopped because the scene's own script left it,
    /// not because the engine could not do the rung.
    departed: bool,
    /// Whether the exit that fired also landed on the tile the record names.
    /// `None` when no exit fired.
    landed_on_entry_tile: Option<bool>,
    note: String,
}

/// The player's world `(x, z)`.
fn player_xz(host: &SceneHost) -> (i16, i16) {
    let slot = host.world.player_actor_slot.unwrap_or(0) as usize;
    let ms = &host.world.actors[slot].move_state;
    (ms.world_x, ms.world_z)
}

fn tile_of(x: i16, z: i16) -> (i16, i16) {
    ((x - 0x40) >> 7, (z - 0x40) >> 7)
}

/// The live field VM's `(pc, opcode)` - where a parked script came to rest.
fn park_site(host: &SceneHost) -> Option<(usize, u8)> {
    let pc = host.world.field_pc;
    host.world.field_bytecode.get(pc).map(|op| (pc, *op))
}

/// The flag state every entry of a scene starts from.
///
/// The ladder re-enters a scene several times (the driven probe, its
/// released-pad control, one attempt per exit site) and those entries are
/// only comparable if they are all the *same visit*. Most chapter-1 interiors
/// carry a C1-gated first-visit record that self-latches, so the second entry
/// of a scene is a materially different scene - `izumi`'s spring
/// choreography spawns once and then never again. Restoring the banks before
/// every entry is what keeps the control a control.
#[derive(Clone)]
struct FlagBaseline {
    system: Vec<u8>,
    story: u32,
}

impl FlagBaseline {
    fn snapshot(host: &SceneHost) -> Self {
        Self {
            system: host.world.system_flags.clone(),
            story: host.world.story_flags,
        }
    }
    fn restore(&self, host: &mut SceneHost) {
        host.world.system_flags.clone_from(&self.system);
        host.world.story_flags = self.story;
    }
}

/// Enter `name` through the mode-appropriate entry point, leaving the flag
/// banks exactly as they are - so whatever the previous entry latched stays
/// latched. This is a *revisit*.
fn enter_raw(host: &mut SceneHost, name: &str) -> bool {
    let r = if is_world_map_scene(name) {
        host.enter_world_map_scene(name)
    } else {
        host.enter_field_scene(name, 0)
    };
    r.is_ok() && matches!(host.world.mode, SceneMode::Field | SceneMode::WorldMap)
}

/// Restore the flag banks and enter `name`. This is a *first visit*.
fn enter(host: &mut SceneHost, name: &str, base: &FlagBaseline) -> bool {
    base.restore(host);
    enter_raw(host, name)
}

/// Outcome of letting a freshly-entered scene's script run itself out.
enum Settle {
    /// Control came back to the player.
    Released,
    /// The script left the scene on its own (a scripted `0x3F`).
    Departed(String),
    /// The budget ran out with the timeline or a dialogue still holding
    /// input; `(pc, opcode)` is where the field VM came to rest.
    Parked {
        held_by: &'static str,
        at: Option<(usize, u8)>,
    },
}

/// Tick with a released pad until the scene hands control back.
///
/// "Control back" is three things, not one: no cutscene timeline, no dialogue
/// owning input, **and** no spawned record still running. The third is not
/// cosmetic - a first-visit record is a helper context rather than a
/// timeline, so a settle that ignored them returned while `izumi`'s spring
/// choreography was still `MoveTo`-ing the player, and the walk probe then
/// scored thirty tiles of script-driven motion as locomotion.
fn settle(host: &mut SceneHost) -> Settle {
    for _ in 0..SETTLE_TICKS {
        if !host.world.cutscene_timeline_active()
            && !host.world.dialogue_owns_input()
            && host.world.helper_contexts.is_empty()
        {
            return Settle::Released;
        }
        host.world.set_pad(0);
        match host.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Settle::Departed(name),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let held_by = if host.world.cutscene_timeline_active() {
        "cutscene timeline"
    } else if host.world.dialogue_owns_input() {
        "dialogue"
    } else {
        "a spawned record"
    };
    Settle::Parked {
        held_by,
        at: park_site(host),
    }
}

/// What one locomotion probe observed.
struct Probe {
    /// Furthest Chebyshev tile distance from the probe's start the player
    /// ever reached - not the final position, which a wall-slide can shorten.
    tiles: i32,
    /// A walk-on trigger took the scene away mid-probe, and where it went.
    left: Option<String>,
    /// A random encounter replaced the world mid-probe (after which
    /// `player_world` is a battle-arena transform, so the probe stops).
    encounter: bool,
    /// The tile the probe started on - what a stall is reported against.
    start_tile: (i16, i16),
}

/// Run `pads` in sequence, `WALK_TICKS` ticks each, and report the furthest
/// the player got. `pads` is `[0]` for the neutral control and the four
/// cardinals for the driven probe.
fn probe(host: &mut SceneHost, pads: &[u16], ticks_each: usize) -> Probe {
    let (sx, sz) = player_xz(host);
    let start_tile = tile_of(sx, sz);
    let mut out = Probe {
        tiles: 0,
        left: None,
        encounter: false,
        start_tile,
    };
    for &mask in pads {
        for _ in 0..ticks_each {
            host.world.set_pad(mask);
            match host.tick() {
                Ok(SceneTickEvent::SceneEntered { name }) => {
                    out.left = Some(name);
                    return out;
                }
                Ok(_) => {}
                Err(_) => return out,
            }
            if host.world.mode == SceneMode::Battle {
                out.encounter = true;
                return out;
            }
            let (x, z) = player_xz(host);
            let (tx, tz) = tile_of(x, z);
            out.tiles = out
                .tiles
                .max((tx - start_tile.0).abs().max((tz - start_tile.1).abs()) as i32);
        }
    }
    host.world.set_pad(0);
    out
}

/// The four cardinals, held in turn.
///
/// The sweep is deliberate: a scene whose spawn faces a wall on one axis
/// still proves locomotion on another, and scoring a single direction would
/// report a wall as a stall.
const CARDINALS: [PadButton; 4] = [
    PadButton::Up,
    PadButton::Down,
    PadButton::Left,
    PadButton::Right,
];

/// Seat the player one tile off `(tx, tz)` then onto it, ticking between, so
/// the walk-on dispatch sees a genuine tile crossing (retail's dispatcher
/// fires on a tile *change*).
fn step_onto_tile(host: &mut SceneHost, tx: u8, tz: u8) {
    host.world.set_pad(0);
    let off = if tx > 0 { tx - 1 } else { tx + 1 };
    host.world.seat_player_at_tile(off, tz);
    let _ = host.tick();
    host.world.seat_player_at_tile(tx, tz);
}

/// Why a scene produced no exit site, in terms of the two disc structures the
/// join needs: the `.MAP` walk-on triggers and the partition-2 records that
/// carry a `0x3F`.
///
/// "No exit site" has three very different causes and the join cannot tell
/// them apart on its own: the scene has no `0x3F` anywhere; it has one but no
/// walk-on band reaches the record that carries it (the door is an
/// interactive actor or a `0x3E` warp instead); or it has both and they do
/// not join. This names which.
fn exit_diagnosis(host: &SceneHost, name: &str, mf: &ManFile, man: &[u8]) -> String {
    let p2_count = mf.header.partition_counts[2].max(0) as usize;
    // Which partition-2 records carry a `0x3F` at all? Probe each with a
    // synthetic gate-1 trigger on its own tile, reusing the real decoder.
    let synthetic: Vec<legaia_engine_core::field_regions::TileTrigger> = (0..p2_count.min(256))
        .map(|r| legaia_engine_core::field_regions::TileTrigger {
            tile_x: (r % 128) as u8,
            tile_z: (r / 128) as u8,
            record: r as u8,
            gate: 1,
        })
        .collect();
    let carriers: BTreeSet<u8> = overworld_portal_sites(mf, man, &synthetic)
        .iter()
        .map(|s| s.record)
        .collect();

    let (g1, g0) = match Scene::load(&host.index, name)
        .ok()
        .and_then(|s| s.field_tile_triggers(&host.index).ok())
    {
        Some((primary, fallback)) => {
            let all: Vec<_> = primary.into_iter().chain(fallback).collect();
            let g1: BTreeSet<u8> = all
                .iter()
                .filter(|t| t.gate == 1)
                .map(|t| t.record)
                .collect();
            let g0 = all.iter().filter(|t| t.gate == 0).count();
            (g1, g0)
        }
        None => (BTreeSet::new(), 0),
    };

    if carriers.is_empty() {
        return format!(
            "no partition-2 record carries a 0x3F ({p2_count} P2 records, \
             {} gate-1 / {g0} gate-0 .MAP triggers) - the scene's exits, if any, \
             are partition-1 or 0x3E-carried",
            g1.len()
        );
    }
    format!(
        "P2 records carrying a 0x3F: {carriers:?}, but the {} gate-1 .MAP trigger(s) \
         name records {g1:?} - the door is not on a walk-on band",
        g1.len()
    )
}

/// One exit attempt's result.
struct ExitTry {
    claimed: String,
    entered: Option<String>,
    /// Did the arrival land on the tile the record's `0x3F` names?
    on_entry_tile: bool,
}

/// Try the scene's decoded exit sites until one fires. Each attempt re-enters
/// the scene fresh, because a fired exit has already left it.
fn try_exits(
    host: &mut SceneHost,
    name: &str,
    mf: &ManFile,
    man: &[u8],
    base: &FlagBaseline,
) -> Vec<ExitTry> {
    let Ok(scene) = Scene::load(&host.index, name) else {
        return Vec::new();
    };
    let Ok((primary, fallback)) = scene.field_tile_triggers(&host.index) else {
        return Vec::new();
    };
    let mut triggers = primary;
    triggers.extend(fallback);
    let sites = overworld_portal_sites(mf, man, &triggers);

    let mut out = Vec::new();
    for site in sites.iter().take(EXIT_SITES_TRIED) {
        if !enter(host, name, base) {
            break;
        }
        // Let the entry script run first; seating onto a trigger while a
        // cutscene owns the frame tests nothing, and a script that departs or
        // parks has already taken the scene away.
        if !matches!(settle(host), Settle::Released) {
            break;
        }
        step_onto_tile(host, site.overworld_x, site.overworld_z);
        let mut entered = None;
        for _ in 0..EXIT_TICKS {
            match host.tick() {
                Ok(SceneTickEvent::SceneEntered { name }) => {
                    entered = Some(name);
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let on_entry_tile = entered.is_some() && {
            let (x, z) = player_xz(host);
            let (tx, tz) = tile_of(x, z);
            tx == i16::from(site.entry_x & 0x7F) && tz == i16::from(site.entry_z & 0x7F)
        };
        let fired = entered.is_some();
        out.push(ExitTry {
            claimed: site.scene_name.clone(),
            entered,
            on_entry_tile,
        });
        if fired {
            break;
        }
    }
    out
}

/// Run one scene up the ladder.
#[allow(clippy::too_many_lines)]
fn score_scene(host: &mut SceneHost, name: &str) -> Verdict {
    let base = FlagBaseline::snapshot(host);
    let mut marks = [Mark::Skip; 6];
    let mut departed = false;
    let mut landed_on_entry_tile = None;

    let finish = |marks: [Mark; 6], note: String, departed: bool, landed: Option<bool>| {
        // Only rungs actually demonstrated count. An `~` (not applicable,
        // because the scene's own script left before the rung could run) is
        // deliberately NOT credited - crediting it would let a scene that
        // never walked and never exited score the same as one that did.
        let score = marks.iter().take_while(|m| **m == Mark::Pass).count();
        Verdict {
            scene: name.to_string(),
            marks,
            score,
            departed,
            landed_on_entry_tile: landed,
            note,
        }
    };

    // -- Rung 1: assets ----------------------------------------------------
    let scene = match Scene::load(&host.index, name) {
        Ok(s) => s,
        Err(e) => {
            marks[0] = Mark::Fail;
            return finish(
                marks,
                format!("Scene::load: {e}"),
                departed,
                landed_on_entry_tile,
            );
        }
    };
    let man = match scene.field_man_payload(&host.index) {
        Ok(Some(m)) => m,
        Ok(None) => {
            marks[0] = Mark::Fail;
            return finish(
                marks,
                "no MAN resolves".into(),
                departed,
                landed_on_entry_tile,
            );
        }
        Err(e) => {
            marks[0] = Mark::Fail;
            return finish(
                marks,
                format!("field_man_payload: {e}"),
                departed,
                landed_on_entry_tile,
            );
        }
    };
    marks[0] = Mark::Pass;

    // -- Rung 2: man -------------------------------------------------------
    let mf = match parse_man(&man) {
        Ok(mf) => mf,
        Err(e) => {
            marks[1] = Mark::Fail;
            return finish(
                marks,
                format!("parse MAN: {e}"),
                departed,
                landed_on_entry_tile,
            );
        }
    };
    let dests = scene_destinations(&mf, &man);
    marks[1] = Mark::Pass;

    // -- Rung 3: enter -----------------------------------------------------
    if !enter(host, name, &base) {
        marks[2] = Mark::Fail;
        return finish(
            marks,
            format!("entered as {:?}, expected Field/WorldMap", host.world.mode),
            departed,
            landed_on_entry_tile,
        );
    }
    marks[2] = Mark::Pass;

    // -- Rung 4: script ----------------------------------------------------
    match settle(host) {
        Settle::Released => marks[3] = Mark::Pass,
        Settle::Departed(to) => {
            marks[3] = Mark::Pass;
            marks[4] = Mark::Na;
            marks[5] = Mark::Na;
            departed = true;
            return finish(
                marks,
                format!("entry script departs -> {to}"),
                departed,
                landed_on_entry_tile,
            );
        }
        Settle::Parked { held_by, at } => {
            marks[3] = Mark::Fail;
            let where_ = at.map_or_else(
                || "no live field bytecode".to_string(),
                |(pc, op)| format!("pc=0x{pc:04X} op=0x{op:02X}"),
            );
            return finish(
                marks,
                format!(
                    "script parked after {SETTLE_TICKS} ticks ({held_by} holds input, {where_})"
                ),
                departed,
                landed_on_entry_tile,
            );
        }
    }

    // -- Rung 5: walk ------------------------------------------------------
    // The driven probe, then the same budget of released pad from the same
    // settled state, as a CONTROL. Without it "the player walked" and "a
    // still-running record dragged the player" are the same measurement -
    // and they are not the same claim. `izumi`'s spring choreography moves
    // the player thirty tiles with the pad released.
    let driven = probe(host, &CARDINALS.map(PadButton::mask), WALK_TICKS);
    let neutral = if enter(host, name, &base) && matches!(settle(host), Settle::Released) {
        probe(host, &[0], WALK_TICKS * CARDINALS.len())
    } else {
        Probe {
            tiles: 0,
            left: None,
            encounter: false,
            start_tile: driven.start_tile,
        }
    };
    let pad_did_it =
        driven.tiles > neutral.tiles || (driven.left.is_some() && neutral.left.is_none());
    let drift = if neutral.tiles > 0 || neutral.left.is_some() {
        format!(
            " (released-pad control drifted {} tile(s){})",
            neutral.tiles,
            neutral
                .left
                .as_ref()
                .map(|n| format!(" and left for {n}"))
                .unwrap_or_default()
        )
    } else {
        String::new()
    };
    if !pad_did_it {
        marks[4] = Mark::Fail;
        // Which failure is it? "The engine cannot walk here" and "this
        // scene's first-visit script owns the player" look identical from
        // one measurement. Re-enter WITHOUT restoring the flag banks - the
        // first-visit records latched during the two probes above, so this
        // entry is a revisit - and say which. The rung stays failed either
        // way: it is scored on the first visit, which is the visit a player
        // actually makes first.
        let revisit = if enter_raw(host, name) && matches!(settle(host), Settle::Released) {
            let r = probe(host, &CARDINALS.map(PadButton::mask), WALK_TICKS);
            format!("; on a revisit the pad moves {} tile(s)", r.tiles)
        } else {
            "; the revisit did not settle either".to_string()
        };
        return finish(
            marks,
            format!(
                "no pad direction beat the released-pad control off tile {:?}: \
                 driven {} tile(s) vs control {} tile(s){revisit}",
                driven.start_tile, driven.tiles, neutral.tiles
            ),
            departed,
            landed_on_entry_tile,
        );
    }
    marks[4] = Mark::Pass;
    let mut note = match (&driven.left, driven.encounter) {
        (Some(to), _) => format!("walk-on trigger fired -> {to}{drift}"),
        (None, true) => format!("walked {} tile(s), then an encounter{drift}", driven.tiles),
        (None, false) => format!("walked {} tile(s){drift}", driven.tiles),
    };

    // -- Rung 6: exit ------------------------------------------------------
    let tries = try_exits(host, name, &mf, &man, &base);
    if tries.is_empty() {
        marks[5] = Mark::Fail;
        let dest_names: Vec<&str> = dests.iter().map(|d| d.scene_name.as_str()).collect();
        return finish(
            marks,
            format!(
                "{note}; no exit site: {}. MAN lists {dest_names:?}",
                exit_diagnosis(host, name, &mf, &man)
            ),
            departed,
            landed_on_entry_tile,
        );
    }
    if let Some(hit) = tries
        .iter()
        .find(|t| t.entered.as_deref() == Some(t.claimed.as_str()))
    {
        marks[5] = Mark::Pass;
        landed_on_entry_tile = Some(hit.on_entry_tile);
        note = format!(
            "{note}; exit -> {}{}",
            hit.claimed,
            if hit.on_entry_tile {
                ""
            } else {
                " (arrival tile != record's entry tile)"
            }
        );
    } else {
        marks[5] = Mark::Fail;
        let detail: Vec<String> = tries
            .iter()
            .map(|t| {
                format!(
                    "{} -> {}",
                    t.claimed,
                    t.entered.as_deref().unwrap_or("(nothing fired)")
                )
            })
            .collect();
        note = format!("{note}; exits tried: {}", detail.join(", "));
    }
    finish(marks, note, departed, landed_on_entry_tile)
}

// ---------------------------------------------------------------------------
// Baseline
// ---------------------------------------------------------------------------

fn baseline_path() -> PathBuf {
    repo_root().join("scripts/replays/chapter1_frontier_baseline.toml")
}

/// Parse `key = N` out of the baseline file. A missing file or key reads as
/// `0`, so a fresh clone starts from "no progress claimed" rather than
/// failing on a file it cannot see.
fn read_baseline(key: &str) -> usize {
    let Ok(text) = std::fs::read_to_string(baseline_path()) else {
        return 0;
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| {
            l.strip_prefix(key)?
                .trim()
                .strip_prefix('=')?
                .trim()
                .parse()
                .ok()
        })
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Part A: the graph
// ---------------------------------------------------------------------------

/// The chapter-1 closure is disc-sourced and terminates at a kingdom
/// boundary, and the boundary is real (a chapter-2 overworld is named by some
/// chapter-1 scene, and it is not expanded).
#[test]
fn part_a_chapter1_closure_is_disc_sourced_and_bounded() {
    let Some(host) = open_host() else {
        return;
    };
    let Some(_) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let Closure {
        order,
        edges,
        boundary,
    } = chapter1_closure(&host.index);
    assert!(
        order.len() <= MAX_CLOSURE,
        "closure hit the {MAX_CLOSURE} cap - the destination decode is naming phantoms"
    );
    assert_eq!(order.first().map(String::as_str), Some(CH1_ROOT));
    assert!(
        order.iter().any(|s| s == CH1_OVERWORLD),
        "the closure must reach the Drake overworld"
    );
    assert!(
        order.iter().any(|s| s == "keikoku"),
        "the closure must reach the Ravine - the frontier this ladder extends past"
    );
    // Past the Ravine: the boss chain and the castle interior are in the
    // closure, reached by the disc's own destination tables and not by a
    // hand-written list.
    for past in ["rikuroa", "jou", "jouina"] {
        assert!(
            order.iter().any(|s| s == past),
            "the closure must reach {past} (past keikoku); got {order:?}"
        );
    }
    eprintln!(
        "[graph] chapter-1 closure = {} scenes from {CH1_ROOT}",
        order.len()
    );
    for s in &order {
        let outs = edges.get(s).map(BTreeSet::len).unwrap_or(0);
        eprintln!("  {s:<10} -> {outs} destination(s)");
    }
    eprintln!("[graph] kingdom boundary edges (not expanded):");
    for (from, to) in &boundary {
        eprintln!("  {from} -> {to}");
    }
    assert!(
        !boundary.is_empty(),
        "chapter 1 must name at least one other kingdom overworld - a closure \
         with no boundary means the bound is not being exercised"
    );
    // The Drake kingdom hands off to Sebucus at exactly one place, and it is
    // Jiji Village. Pinning the edge (rather than just "some boundary
    // exists") is what makes the bound a measurement: a decode change that
    // opened a second handoff, or moved this one, trips here.
    assert!(
        boundary.contains(&("jiji".to_string(), "map02".to_string())),
        "the Drake -> Sebucus handoff is jiji -> map02; got {boundary:?}"
    );
}

// ---------------------------------------------------------------------------
// Part B: the per-scene ladder
// ---------------------------------------------------------------------------

#[test]
#[allow(clippy::too_many_lines)]
fn part_b_chapter1_scene_frontier_ladder() {
    let Some(mut host) = open_host() else {
        return;
    };
    let Some(_) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let Closure {
        order, boundary, ..
    } = chapter1_closure(&host.index);

    // Every scene is scored from the same starting state: the flag banks are
    // snapshotted and restored, so a verdict is a property of the scene and
    // not of whatever the previous scene latched.
    let flags0 = host.world.system_flags.clone();
    let story0 = host.world.story_flags;

    let mut verdicts: Vec<Verdict> = Vec::new();
    for name in &order {
        host.world.system_flags = flags0.clone();
        host.world.story_flags = story0;
        verdicts.push(score_scene(&mut host, name));
    }

    // -- The table --------------------------------------------------------
    eprintln!();
    eprintln!("chapter-1 scene frontier ladder (closure of {CH1_ROOT} over the disc's 0x3F table)");
    eprintln!(
        "rungs: 1 {} | 2 {} | 3 {} | 4 {} | 5 {} | 6 {}",
        RUNGS[0], RUNGS[1], RUNGS[2], RUNGS[3], RUNGS[4], RUNGS[5]
    );
    eprintln!("marks: * cleared   X failed   . not attempted   ~ n/a (script departs)");
    eprintln!();
    eprintln!("{:<10} 123456  score  note", "scene");
    eprintln!("{}", "-".repeat(96));
    for v in &verdicts {
        let glyphs: String = v.marks.iter().map(|m| m.glyph()).collect();
        eprintln!("{:<10} {glyphs}  {:>5}  {}", v.scene, v.score, v.note);
    }
    eprintln!("{}", "-".repeat(96));

    // -- The summary ------------------------------------------------------
    let scenes = verdicts.len();
    let rung_total: usize = verdicts.iter().map(|v| v.score).sum();
    let full_ladder = verdicts.iter().filter(|v| v.score == RUNGS.len()).count();
    let departed = verdicts.iter().filter(|v| v.departed).count();
    let landed = verdicts
        .iter()
        .filter(|v| v.landed_on_entry_tile == Some(true))
        .count();
    let exit_fired = verdicts
        .iter()
        .filter(|v| v.landed_on_entry_tile.is_some())
        .count();
    let mut stopped_at: BTreeMap<usize, Vec<&str>> = BTreeMap::new();
    for v in &verdicts {
        if v.score < RUNGS.len() && !v.departed {
            stopped_at
                .entry(v.score)
                .or_default()
                .push(v.scene.as_str());
        }
    }

    eprintln!("scenes             {scenes}");
    eprintln!("rung_total         {rung_total} / {}", scenes * RUNGS.len());
    eprintln!("full_ladder        {full_ladder}");
    eprintln!("scripted departure {departed} (ladder capped at rung 4 by design)");
    eprintln!("arrival tile match {landed} / {exit_fired} exits that fired");
    eprintln!("kingdom boundary   {} edge(s)", boundary.len());
    for (score, scenes) in &stopped_at {
        let rung = RUNGS.get(*score).copied().unwrap_or("?");
        eprintln!(
            "  stopped before rung {} ({rung}): {}",
            score + 1,
            scenes.join(" ")
        );
    }

    // -- The ratchet ------------------------------------------------------
    let b_scenes = read_baseline("scenes");
    let b_total = read_baseline("rung_total");
    let b_full = read_baseline("full_ladder");
    if scenes > b_scenes || rung_total > b_total || full_ladder > b_full {
        eprintln!();
        eprintln!("baseline can be raised in {}:", baseline_path().display());
        eprintln!("  scenes = {scenes}");
        eprintln!("  rung_total = {rung_total}");
        eprintln!("  full_ladder = {full_ladder}");
    }
    assert!(
        scenes >= b_scenes,
        "chapter-1 closure shrank: {scenes} < baseline {b_scenes} - a destination \
         decode regression drops scenes off the graph"
    );
    assert!(
        rung_total >= b_total,
        "chapter-1 frontier score regressed: {rung_total} < baseline {b_total}"
    );
    assert!(
        full_ladder >= b_full,
        "scenes clearing the whole ladder regressed: {full_ladder} < baseline {b_full}"
    );
}

// ---------------------------------------------------------------------------
// Part C: cross-check against the capture library
// ---------------------------------------------------------------------------

/// Retail was demonstrably standing in these scenes, so the engine has to be
/// able to enter them.
///
/// The cataloged captures are the only *retail* evidence this repo has about
/// chapter-1 scenes past the Ravine. For every capture whose recorded scene
/// is in the chapter-1 closure, the ladder's verdict for that scene must have
/// cleared at least rung 3 (`enter`) - an engine claim checked against a
/// capture, rather than against itself.
///
/// Capture-grounded. Skips when the library or the manifest is absent.
#[test]
fn part_c_captured_scenes_are_scenes_the_engine_can_enter() {
    let Some(mut host) = open_host() else {
        return;
    };
    let (Some(manifest_path), Some(library)) = (manifest_path(), library_dir()) else {
        eprintln!("[skip] scripts/scenarios.toml or saves/library missing (capture-gated)");
        return;
    };
    let manifest =
        legaia_mednafen::ScenarioManifest::from_path(&manifest_path).expect("parse manifest");

    let Closure { order, .. } = chapter1_closure(&host.index);
    let closure: BTreeSet<&str> = order.iter().map(String::as_str).collect();

    // Retail-observed scene names, read out of the capture's own main RAM
    // (not out of the manifest's `expected_active_scene` annotation - the
    // annotation is a human note, the RAM is the measurement).
    let mut observed: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for scn in &manifest.scenarios {
        let Some(path) = manifest.library_save_path(scn, library.as_path()) else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let ram = match path.extension().and_then(|e| e.to_str()) {
            Some("sstate") => legaia_pcsxr::SaveState::from_path(&path)
                .ok()
                .map(|s| s.main_ram().to_vec()),
            _ => legaia_mednafen::SaveState::from_path(&path)
                .ok()
                .and_then(|s| s.main_ram().ok().map(<[u8]>::to_vec)),
        };
        let Some(ram) = ram else { continue };
        let scene = legaia_mednafen::game_anchors::scene_name(&ram);
        if closure.contains(scene.as_str()) {
            observed.entry(scene).or_default().push(scn.label.clone());
        }
    }
    if observed.is_empty() {
        eprintln!("[skip] no capture in the library sits in a chapter-1 closure scene");
        return;
    }

    let flags0 = host.world.system_flags.clone();
    let story0 = host.world.story_flags;
    let mut failures: Vec<String> = Vec::new();
    for (scene, labels) in &observed {
        host.world.system_flags = flags0.clone();
        host.world.story_flags = story0;
        let v = score_scene(&mut host, scene);
        eprintln!(
            "[capture] {scene:<10} score {} <- {} capture(s): {}",
            v.score,
            labels.len(),
            labels.join(", ")
        );
        if v.score < 3 {
            failures.push(format!(
                "{scene}: retail was in it ({}) but the engine scored {} - {}",
                labels.join(", "),
                v.score,
                v.note
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "captured chapter-1 scenes the engine cannot enter:\n{}",
        failures.join("\n")
    );
    eprintln!(
        "[ok] Part C: {} captured chapter-1 scene(s) all enter in-engine",
        observed.len()
    );
}

// ---------------------------------------------------------------------------
// Disc-free unit checks (these run in a CI with no disc data)
// ---------------------------------------------------------------------------

mod unit {
    use super::*;

    /// A missing baseline file reads as zero rather than panicking, so a
    /// fresh clone starts from "no progress claimed".
    #[test]
    fn absent_baseline_key_reads_as_zero() {
        assert_eq!(read_baseline("a_key_that_does_not_exist"), 0);
    }

    /// The kingdom-boundary rule is the bound on the closure, so it has to be
    /// exactly "another kingdom overworld" - not "any scene named map*".
    #[test]
    fn kingdom_boundary_rule_is_the_other_overworlds_only() {
        assert!(is_world_map_scene("map02") && "map02" != CH1_OVERWORLD);
        assert!(is_world_map_scene("map03") && "map03" != CH1_OVERWORLD);
        assert!(is_world_map_scene(CH1_OVERWORLD));
        // A scene whose name merely starts with `map` is not an overworld.
        assert!(!is_world_map_scene("map"));
        assert!(!is_world_map_scene("mapping"));
    }

    /// The score counts leading *demonstrated* rungs only - a later pass
    /// after a failure must not be credited, and a not-applicable rung must
    /// not be credited either.
    #[test]
    fn score_counts_leading_demonstrated_rungs_only() {
        let score = |m: [Mark; 6]| m.iter().take_while(|m| **m == Mark::Pass).count();
        assert_eq!(
            score([
                Mark::Pass,
                Mark::Pass,
                Mark::Fail,
                Mark::Skip,
                Mark::Skip,
                Mark::Skip,
            ]),
            2,
            "a rung after a failure is never credited"
        );
        assert_eq!(
            score([
                Mark::Pass,
                Mark::Pass,
                Mark::Pass,
                Mark::Pass,
                Mark::Na,
                Mark::Na,
            ]),
            4,
            "an n/a rung is not a pass - the scene never walked and never exited"
        );
    }
}
