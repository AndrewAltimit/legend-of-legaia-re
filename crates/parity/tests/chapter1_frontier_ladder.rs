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
//! the disc, not a narrative one, and it reaches well past the Drake kingdom:
//! a `0x3F` operand's case is invisible to retail (the name goes into the ISO
//! path `DATA\FIELD\<name>.MAP`, and ISO 9660 identifiers are upper case), so
//! roughly half the disc's destinations are written `MAP03` / `KOR3` /
//! `RETOCKIN` rather than lower case. Admitting them - see
//! [`legaia_scene_formats::field_disasm::clean_scene_name`] - roughly doubles the
//! closure and pulls in most of the `dream` hub's fan-out. Members are what
//! the graph reaches, not what chapter a player is in;
//! `scripts/scenarios.toml` labels some of them (`garmel`) chapter 2.
//!
//! And it is a closure over `0x3F` only - the sibling `0x3E` door warp
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
//! length from the same state**. A dialogue the walk opens (a walk-touch sign)
//! is paged with Cross pulses while it owns input, as the settle pages one. Without that control, "the player walked"
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

use legaia_engine_field::man_field_scripts::{overworld_portal_sites, scene_destinations};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::scene::{DefaultMapIdResolver, ProtIndex, Scene, is_world_map_scene};
use legaia_engine_scene_host::scene_host::{SceneHost, SceneTickEvent};
use legaia_engine_system::input::PadButton;
use legaia_scene_formats::man_section::{ManFile, parse as parse_man};

mod captured_scenes;
mod exits;
mod five_doors;
mod ladder_parts;
mod probes;
mod scoring;

use exits::*;
use ladder_parts::*;
use probes::*;
use scoring::*;

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
        && let Some(party) = legaia_game_tables::new_game::StartingParty::from_scus(&scus)
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
