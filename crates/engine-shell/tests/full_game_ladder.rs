//! Full-game ladder: how far through the **whole** game - New Game to the
//! ending credits - the port gets, as one ordered, game-denominated score.
//!
//! The other progression instruments each cover a slice. The pad-driven
//! [`critical_path_replay`](critical_path_replay.rs) stops in chapter 1;
//! `chapter1_frontier_ladder` (parity) measures chapter-1 scene
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
//! Segment 0 is seeded by the New Game path itself (`NativeSession::start_new_game`),
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
//! beats by walking to them; neither tier buys, and the pad tier opens the
//! pause menu only to heal, burn an Incense, or - replaying a segment a boss
//! wiped - equip the bag's guard against that boss's element, so a story beat that waits on a purchase reads as
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

use legaia_engine_core::world::world_map_camera_relative_bits;
use legaia_engine_field::field_regions::TileTrigger;
use legaia_engine_field::man_field_scripts::{overworld_portal_sites, scene_destinations};
use legaia_engine_field::world_map_entity_config::WorldMapEntityConfig;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::man_census::{scene_man_carriers, system_flag_census};
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_engine_scene_host::scene_host::{FmvHandoffOutcome, SceneTickEvent};
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;
use legaia_scene_formats::field_disasm::FlagKind;
use serde::Deserialize;

mod beat_records;
mod beats;
mod continuous;
mod crossings;
mod disc_graph;
mod errands;
mod fight_choices;
mod fight_reads;
mod fight_turn;
mod interactions;
mod pad_field_upkeep;
mod pad_follower;
mod pad_hop;
mod pad_shopping;
mod pad_training;
mod path_planner;
mod script_runner;
mod seated_hops;
mod segment;
mod session;
mod spine;
mod walk_links;

use beat_records::*;
use beats::*;
use continuous::{EXCHANGE, worth_fighting};
use crossings::*;
use disc_graph::*;
use errands::*;
use fight_choices::*;
use fight_reads::*;
use fight_turn::*;
use interactions::*;
use pad_field_upkeep::*;
use pad_follower::*;
use pad_hop::*;
use pad_shopping::*;
use pad_training::*;
use path_planner::*;
use script_runner::*;
use seated_hops::*;
use segment::*;
use session::*;
use spine::*;
use walk_links::*;

// ---------------------------------------------------------------------------
// Baseline
// ---------------------------------------------------------------------------

/// Attempts a pad segment gets at a fight that wipes the party, the guard
/// retry included: a player reloads after a game over, and a fight the
/// anchor's party wins about half the time (`noaru`'s Songi at the `PRO-04`
/// party's level, whose armed party-wide cast lands every round below half
/// HP) should not decide the segment on one hand of the rand stream.
const PAD_WIPE_ATTEMPTS: u32 = 6;

fn baseline_path() -> PathBuf {
    repo_root().join("scripts/replays/full_game_baseline.toml")
}

#[derive(Debug, Default, Deserialize)]
struct Baseline {
    #[serde(default)]
    headline: BTreeMap<String, usize>,
    #[serde(default)]
    segments: BTreeMap<String, usize>,
    /// Part C: milestones one continuous New Game reaches.
    #[serde(default)]
    continuous: BTreeMap<String, usize>,
    /// Part C under `LEGAIA_FGL_RNG_SEED=<n>`: the same score on another
    /// hand of the rand stream, keyed `seed_<n>` (decimal).
    #[serde(default)]
    continuous_seeds: BTreeMap<String, usize>,
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

/// `koin1`'s slot machines are the minigame cabinets the walk's idle tap
/// keeps off ([`minigame_talks`]): P1[51..56] run the `3E 68` door-warp,
/// the interact probe reaches them as props, and the Prize Counter
/// (P1[5]) is not one. Disc-gated.
#[test]
fn koin1_slot_machines_are_minigame_cabinets() {
    let Some(inp) = inputs() else { return };
    let mut session = open_session(&inp.extracted);
    session
        .enter_scene_live("koin1", &live_opts(false))
        .expect("enter koin1");
    let cabinets = minigame_talks(&session);
    for slot in 51..=56u8 {
        assert!(cabinets.contains(&slot), "P1[{slot}] in {cabinets:?}");
    }
    assert!(!cabinets.contains(&5), "the Prize Counter is no cabinet");
    let w = &session.host.world;
    for slot in 51..=56u8 {
        assert!(
            w.props.walk_touch.contains_key(&slot),
            "P1[{slot}] is a probe target"
        );
    }
}

// ---------------------------------------------------------------------------
// Part A: the spine is anchored, ordered and routed
// ---------------------------------------------------------------------------

/// Every anchor loads, sits in the scene the spine names, and the anchors
/// fall in story order; prints the disc route for each segment and how many
/// of each anchor's flags lie past the save bridge's story window.
/// The pad hand's field press inverts the engine's eighth-turn remap
/// (`World::remap_pad_direction`): on the four axis-aligned cameras it is
/// the quarter-turn mapping the hand used before, and on every camera the
/// press it picks walks the asked step once the engine has remapped it.
#[test]
fn field_step_press_inverts_the_engine_remap() {
    use legaia_engine_core::world::World;
    let quarter = |az: u16, dx: i16, dz: i16| match (u32::from(az).wrapping_add(512) / 1024) & 3 {
        0 => (dx, dz),
        1 => (-dz, dx),
        2 => (-dx, -dz),
        _ => (dz, -dx),
    };
    let world_step = |az: u16, (sx, sy): (i16, i16)| {
        let mut held = 0u16;
        if sy > 0 {
            held |= 0x1000;
        } else if sy < 0 {
            held |= 0x4000;
        }
        if sx > 0 {
            held |= 0x2000;
        } else if sx < 0 {
            held |= 0x8000;
        }
        let r = World::remap_pad_direction(held, (u32::from(az).wrapping_add(0x100) >> 9) & 7);
        let has = |b: u16| i16::from(r & b != 0);
        (has(0x2000) - has(0x8000), has(0x1000) - has(0x4000))
    };
    for az in (0u16..4096).step_by(64) {
        for dx in -1..=1i16 {
            for dz in -1..=1i16 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let press = field_step_press(az, dx, dz);
                assert_eq!(world_step(az, press), (dx, dz), "az {az} step ({dx}, {dz})");
                if az % 1024 == 0 {
                    assert_eq!(press, quarter(az, dx, dz), "az {az} step ({dx}, {dz})");
                }
            }
        }
    }
}

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

    // `LEGAIA_FGL_SITES=0x19F,0x52B,...`: every disc site that sets or clears
    // each named flag - how a flag in the continuous run's carried-state diff
    // is traced to the record that wrote it.
    if let Ok(list) = std::env::var("LEGAIA_FGL_SITES") {
        for f in list.split(',').filter_map(|s| {
            let s = s.trim();
            u16::from_str_radix(s.strip_prefix("0x").unwrap_or(s), 16).ok()
        }) {
            let sites = |kind: FlagKind| -> String {
                let v: Vec<String> = census
                    .get(&f)
                    .into_iter()
                    .flatten()
                    .filter(|s| s.kind == kind)
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
                v.join(" / ")
            };
            eprintln!(
                "[sites] 0x{f:03X} set: {} | clear: {}",
                sites(FlagKind::Set),
                sites(FlagKind::Clear)
            );
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

/// The rapids' landing poll is reached across the waits ahead of it: a
/// standard yield on the player, a context-flag busy-wait, local-flag
/// writes and an animate, then the `42 01` compass chain (`jouinb` P2[6] at
/// `0x0281..0x02A3`). A jump or a text segment in between ends the look.
/// Disc-free.
#[test]
fn poll_ahead_reaches_a_poll_past_waits() {
    let ride: Vec<u8> = [
        &[0xC1, 0xF8, 0x02, 0xC1][..], // yield on the player
        &[0xB3, 0xF8, 0x0A],           // wait on its context flag
        &[0xAC, 0x0B, 0x0B],           // local flag clear
        &[
            0xCB, 0x0B, 0x02, 0x03, 0x66, 0x00, 0x66, 0x00, 0x66, 0x00, 0x66, 0x00,
        ],
        &[0x42, 0x01, 0x00, 0x4D, 0x01], // the Down poll
    ]
    .concat();
    assert_eq!(poll_ahead(&ride, 0), Some(ride.len() - 5));
    // A relative jump before the poll: the poll may never run.
    let jumped: Vec<u8> = [&[0x26, 0x03, 0x00][..], &[0x42, 0x01, 0x00, 0x4D, 0x01]].concat();
    assert_eq!(poll_ahead(&jumped, 0), None);
    // A flag-bit test (`42 00`) is not a pad poll.
    assert_eq!(poll_ahead(&[0x42, 0x00, 0x05, 0x10, 0x00], 0), None);
}

/// `jouine`'s arrival record P2[5] commits Cort (`3E FF`) on the way in, and
/// the field ticks on through the battle intro. On those frames neither the
/// pause menu (its Save row) nor a host session save may land: a save there
/// holds the flags past the commit without the fight, and its reload walked
/// onto the epilogue band P2[2] with Cort unbeaten. Seeded from the
/// `bio_castle` anchor (`jouind`) and played to `cort_chamber` on the pad,
/// which reaches it on arrival with the fight still committed; Start is then
/// pressed on every other frame until the battle begins. Disc-gated.
#[test]
fn jouine_cort_commit_refuses_the_menu_and_a_host_save() {
    let Some(inp) = inputs() else { return };
    let spine = load_spine();
    let at = |id: &str| spine.iter().position(|m| m.id == id).expect("spine id");
    let (from, to) = (&spine[at("bio_castle")], &spine[at("cort_chamber")]);
    let Some(anchor) = from
        .anchor
        .as_ref()
        .and_then(|r| load_anchor(&inp.library, r).ok())
    else {
        eprintln!("[skip] bio_castle anchor not in the library");
        return;
    };
    let index = ProtIndex::open_extracted(&inp.extracted).expect("open ProtIndex");
    let graph = DiscGraph::build(&index);
    let opts = live_opts(true);
    let mut session = open_session(&inp.extracted);
    seed(&mut session, from, Some(&anchor), &opts).expect("seed bio_castle");
    PAD_DEADLINE.with(|d| d.set(session.frames + PAD_SEGMENT_FRAMES));
    PAD_BATTLE_LEFT.with(|l| l.set(PAD_BATTLE_ALLOWANCE));
    let mut trail = vec![scene_name(&session)];
    traverse(&mut session, &graph, to, true, &mut trail).expect("reach cort_chamber");
    PAD_DEADLINE.with(|d| d.set(u64::MAX));
    assert_eq!(scene_name(&session), "jouine");
    assert!(
        session.host.world.field_scripts_held_for_battle(),
        "the milestone is reached with Cort committed"
    );
    let mut held_frames = 0;
    for f in 0..20_000 {
        if session.host.world.mode == SceneMode::Battle {
            break;
        }
        let held = session.host.world.field_scripts_held_for_battle();
        if held {
            held_frames += 1;
            assert!(!session.host.world.field_menu_open_allowed(), "frame {f}");
            assert!(!session.host.world.host_save_allowed(), "frame {f}");
        }
        let pad = if f % 2 == 0 {
            PadButton::Start.mask()
        } else {
            0
        };
        session.host.world.set_pad(pad);
        let _ = session.tick();
        assert!(
            !(held && session.field_menu.is_some()),
            "Start opened the pause menu with Cort committed (frame {f})"
        );
    }
    assert!(held_frames > 0);
    assert_eq!(
        session.host.world.mode,
        SceneMode::Battle,
        "Cort's fight began"
    );
}

/// `jouinb`'s rapids (P2[6]) loop their lap until the player holds a
/// direction (or Circle) at a landing: the poll chains `42 01 00..08` at
/// `0x02A3` / `0x03AC` are the only ways out, and the lap's backward `26` at
/// `0x03F0` polls the pad, so it is not a wrap. Retail has no cap on a
/// script context, so a rider who presses nothing rides on; one who holds a
/// direction jumps off at the next landing. No random encounter rolls on
/// the way: the ride holds the player's engaged bit. Disc-gated.
#[test]
fn jouinb_rapids_ride_until_the_player_jumps_off() {
    let Some(inp) = inputs() else { return };
    let ride = |hold: u16| {
        let mut session = open_session(&inp.extracted);
        session
            .enter_scene_live("jouinb", &live_opts(false))
            .expect("enter jouinb");
        for _ in 0..60 {
            session.host.world.set_pad(0);
            let _ = session.tick();
        }
        let (mf, man, _) = scene_man_and_triggers(&session).expect("jouinb MAN");
        assert!(
            session
                .host
                .world
                .install_cutscene_timeline_record(&mf, &man, 2, 6, false),
            "install P2[6]"
        );
        let mut moves = 0usize;
        let mut last = player_xz(&session);
        let mut frames = 0usize;
        for f in 0..150_000usize {
            // The script runner raises the player's engaged bit on every
            // frame it steps the ride, and the region roll returns on it
            // (`FUN_801D9E1C`, `encounter.md`'s steps that do not roll).
            assert_ne!(
                session.host.world.mode,
                SceneMode::Battle,
                "an encounter rolled on the ride (frame {f})"
            );
            if !session.host.world.cutscene_timeline_active() {
                break;
            }
            frames = f;
            // The "dive in?" picker takes its default (Yes); the ride then
            // holds `hold`.
            let boxed = session
                .host
                .world
                .cutscene
                .timeline
                .as_ref()
                .is_some_and(|t| t.dialog.is_some());
            let pad = if boxed {
                if f % 8 == 0 {
                    PadButton::Cross.mask()
                } else {
                    0
                }
            } else {
                hold
            };
            session.host.world.set_pad(pad);
            let _ = session.tick();
            let now = player_xz(&session);
            if now != last {
                moves += 1;
                last = now;
            }
        }
        (
            session.host.world.cutscene_timeline_active(),
            frames,
            moves,
            std::mem::take(&mut session.host.world.cutscene.capped),
        )
    };
    let (riding, frames, moves, capped) = ride(0);
    assert!(
        moves > 500,
        "the ride carried the player ({moves} moves in {frames} frames)"
    );
    assert!(
        capped.is_empty(),
        "the anti-hang cap cut the ride: {capped:?}"
    );
    assert!(riding, "a rider who presses nothing is still on the rapids");
    let (riding, frames, _, capped) = ride(PadButton::Right.mask());
    assert!(capped.is_empty(), "{capped:?}");
    assert!(
        !riding && frames < 5_000,
        "holding Right jumps off ({frames} frames)"
    );
}
