//! Critical-path replay: the chapter-1 spine walked by **pad**, and scored.
//!
//! Every other progression oracle in this crate is *disc-denominated* - it
//! asks whether the disc's bytes decode, whether a leg's records resolve,
//! whether a transition fires. This one is **game-denominated**: it asks how
//! far a player pressing buttons can actually get from a cold boot, and
//! reports that as a number that goes up.
//!
//! ## Why this is not the spine oracle
//!
//! [`chapter1_spine_oracle`](chapter1_spine_oracle.rs) proves the same legs,
//! but it moves the player with [`World::seat_player_at_tile`] - its own
//! `walk_onto_tile` helper is a *teleport pair* (seat one tile off the
//! trigger, then seat onto it) that synthesises the tile crossing the walk-on
//! dispatch keys off. `scripts/replays/chapter1_spine.toml` says so plainly:
//! the `[[event]]` pad rows "document the traversal" while the disc-gated diff
//! "drives the transitions by seating the player on the trigger tiles".
//!
//! That is the right design for asking *does the scene graph connect*. It is
//! structurally blind to the class of defect that teleporting steps over:
//! locomotion speed and heading, the collision probe, the walkability grid,
//! the camera-relative pad remap. A port whose lead actor glides, or whose
//! remap is a quadrant off, or whose collision seals a doorway, passes the
//! spine oracle unchanged.
//!
//! Here movement is produced **only** by [`World::set_pad`] +
//! [`SceneHost::tick`]. Nothing is seated. If the player cannot walk out of
//! Rim Elm, this test says so and names the tile they stopped on.
//!
//! ## The ladder
//!
//! Milestones are ordered and cumulative - the run stops at the first one it
//! cannot reach, and the score is the count it cleared:
//!
//! | # | milestone | what it proves |
//! |---|---|---|
//! | 1 | `town01` free-roam, input released | cold boot hands control to the player |
//! | 2 | pad-walk to the south gate -> `map01` | field locomotion + collision + walk-on trigger |
//! | 3 | pad-walk to the `keikoku` portal -> `keikoku` | overworld locomotion + portal engage |
//!
//! ## Navigation
//!
//! Waypoints come from a BFS over the walkability grid, with each edge
//! validated by [`World::field_dir_blocked`] (retail `FUN_801cfe4c`'s
//! static-wall arm) at the source tile's centre - so the planner and the
//! engine consult the same walls. The follower converts the desired world
//! step into a pad mask by inverting the camera quadrant that
//! `decode_field_direction` applies, which is what a player does when they
//! look at the screen and press the direction that moves them the way they
//! want.
//!
//! A leg that stops making progress is reported as `Stalled` with the tile it
//! died on, not as a bare assertion failure - a stall is the finding.
//!
//! ## Ratchet
//!
//! `scripts/replays/critical_path_baseline.toml` carries the highest score
//! reached so far. The test asserts `score >= baseline` (no regression) and
//! prints the line to paste when the score goes up. It never auto-writes the
//! baseline: raising it is a reviewed edit, the same contract
//! `scripts/ci/disc-coverage.py` uses.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or
//! `extracted/` missing.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::{SceneHost, SceneTickEvent};
use legaia_engine_core::world::{SceneMode, WorldMapEntityConfig};

/// Field tile pitch in world units (128 per tile; see `field-locomotion.md`).
const TILE: i16 = 128;

/// Planner lattice pitch, in world units.
///
/// Two constraints fix this, and both are tighter than the obvious "one wall
/// bit" answer:
///
/// 1. **It must be shorter than the probe reach.** [`World::field_dir_blocked`]
///    certifies a direction by sampling leading-edge points ~47 units ahead of
///    the *source* position (the standoff `field-locomotion.md` records as the
///    player resting 47-48 units off the wall plane). An edge longer than that
///    is not covered by its own certificate: at 128 the probe from a tile
///    centre reaches only halfway, so a wall on the far tile boundary is
///    invisible to the planner and the follower walks into it. That is exactly
///    what stalled the first run at `town01` tile `(26, 22)` - the centre probe
///    read `Z+` clear and the player, 126 units deeper, read it blocked.
/// 2. **It must contain tile centres.** A tile centre is `128t + 64`, so the
///    pitch has to divide 64. Off-centreline nodes cannot represent standing in
///    a one-tile doorway, and the probe's three *laterally spread* points then
///    catch the gate posts and read the opening as sealed.
///
/// 32 satisfies both (32 < 47, and `128t + 64 = 32(4t + 2)`). The wall data
/// itself resolves to 64 (`field_tile_is_wall` indexes `z >> 6`), so the
/// lattice deliberately oversamples it rather than matching it.
const SUBCELL: i16 = 32;

/// Rim Elm's south-gate trigger tile, as a navigation *goal* rather than a
/// teleport target - which is why it is **not** the `(25, 46)` the spine
/// oracle seats onto.
///
/// The `.MAP` kind-1 table gives this gate two bands: record 10 over tiles
/// `(24..26, 45)` (world `z` `5632..5887`) and record 0 over `(24..26, 46)`
/// (`5888..6015`). Record 0's band is sealed - grid row `47` reads `7 3 B`
/// across columns `24/25/26` and every even-`z_cell` bit is set, so
/// `z ∈ [5888, 5951]` is a solid 64-unit wall band across the doorway. A
/// seat lands on it; a walk cannot. Retail's captured pre-transition frame
/// parks at `(3264, 5824)`, inside record 10's band, and warps from there.
///
/// So the goal is record 10's band. Reaching it and *not* transitioning is
/// the honest reading of the current defect - see the `town01` south-gate
/// thread in `docs/reference/open-rev-eng-threads.md`.
const TOWN01_SOUTH_GATE: (u8, u8) = (25, 45);

/// Frames a leg may spend before it is called a timeout.
const LEG_FRAME_BUDGET: u32 = 6_000;

/// Frames without closing distance on the goal before a leg is called stalled.
/// Generous enough to absorb a wall-slide detour around a building.
const STALL_FRAMES: u32 = 240;

/// Frames to tick (pad neutral) waiting for the scene's opening choreography
/// to hand control back before a leg starts driving.
const INPUT_RELEASE_BUDGET: u32 = 3_600;

/// BFS ceiling. A field map is 256x256 tiles = 512x512 sub-cells, so this
/// admits a full-map flood with room to spare and still bounds a runaway.
const MAX_PLAN_NODES: usize = 300_000;

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn extracted_dir() -> Option<PathBuf> {
    let d = repo_root().join("extracted");
    if d.is_dir() {
        Some(d)
    } else {
        eprintln!("[skip] extracted/ missing - run legaia-extract first");
        None
    }
}

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    Some(SceneHost::open_extracted(&extracted).expect("open SceneHost"))
}

fn baseline_path() -> PathBuf {
    repo_root().join("scripts/replays/critical_path_baseline.toml")
}

/// Parse the `reached = N` line out of the baseline file. A missing file
/// reads as `0`, so a fresh clone starts from "no progress claimed" rather
/// than failing on a file it cannot see.
fn read_baseline() -> usize {
    let Ok(text) = std::fs::read_to_string(baseline_path()) else {
        return 0;
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| {
            l.strip_prefix("reached")?
                .trim()
                .strip_prefix('=')?
                .trim()
                .parse()
                .ok()
        })
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tile / pad geometry
// ---------------------------------------------------------------------------

/// A walkability sub-cell: world coordinates shifted down by
/// [`SUBCELL`], the granularity [`World::field_tile_is_wall`] indexes at.
type Cell = (i16, i16);

/// World centre of a tile. Mirrors `seat_player_at_tile_inner`'s
/// `(b & 0x7F) * 0x80 + 0x40` for the near half.
fn tile_center(t: (i16, i16)) -> (i16, i16) {
    (t.0 * TILE + 0x40, t.1 * TILE + 0x40)
}

/// Tile containing a world point - the inverse the spine oracle uses to
/// check the op-`0x3F` arrival seat.
fn tile_of(x: i16, z: i16) -> (i16, i16) {
    ((x - 0x40) >> 7, (z - 0x40) >> 7)
}

/// Nearest lattice node to a world point.
///
/// The lattice sits **on** the 64-unit grid lines, not in the middle of the
/// cells between them, and that alignment is load-bearing: a tile centre is
/// `128t + 64 = 64(2t + 1)`, an exact multiple of `SUBCELL`, so a lattice of
/// node centres `64c` contains every tile centre while a lattice of `64c + 32`
/// contains none of them. Off-centre nodes cannot represent standing on a
/// doorway's centreline, and [`World::field_dir_blocked`] probes three
/// *laterally spread* leading-edge points - so from 32 units off-centre the
/// side probes catch the gate posts and a one-tile-wide opening reads as
/// sealed. Rim Elm's south gate did exactly that: the flood stopped two cells
/// short of it.
fn cell_of(x: i16, z: i16) -> Cell {
    ((x + SUBCELL / 2) / SUBCELL, (z + SUBCELL / 2) / SUBCELL)
}

/// World position of a lattice node.
fn cell_center(c: Cell) -> (i16, i16) {
    (c.0 * SUBCELL, c.1 * SUBCELL)
}

/// Tile a sub-cell belongs to, in the same frame as [`tile_of`].
fn tile_of_cell(c: Cell) -> (i16, i16) {
    let (x, z) = cell_center(c);
    tile_of(x, z)
}

/// Neighbour offsets paired with the `dir` row
/// [`World::field_dir_blocked`] probes: `0` = Z-, `1` = X-, `2` = Z+,
/// `3` = X+.
const STEPS: [((i16, i16), usize); 4] = [((0, -1), 0), ((-1, 0), 1), ((0, 1), 2), ((1, 0), 3)];

/// Pad mask that walks the player one step along world direction
/// `(dwx, dwz)`.
///
/// `decode_field_direction` rotates the *screen* delta into world space by
/// the camera quadrant `((azimuth + 512) / 1024) & 3`; this inverts that
/// rotation so the caller can think in world axes. Quadrant 0 is identity
/// (screen-up -> Z+, screen-right -> X+).
fn pad_for_world_step(azimuth: u16, dwx: i16, dwz: i16) -> u16 {
    let quadrant = ((azimuth as u32).wrapping_add(512) / 1024) & 3;
    let (sx, sy) = match quadrant {
        0 => (dwx, dwz),
        1 => (-dwz, dwx),
        2 => (-dwx, -dwz),
        _ => (dwz, -dwx),
    };
    let mut pad = 0u16;
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

// ---------------------------------------------------------------------------
// Planner
// ---------------------------------------------------------------------------

/// BFS the reachable set from `from`, and return the route to whichever
/// reachable node sits closest to `goal_tile`'s centre.
///
/// Best-effort by design, because the exact goal is routinely **not** a
/// standable node: `seat_player_at_tile`'s own docs record that "a door tile
/// reads as a wall", and a scene exit is a door. Rim Elm's south gate is the
/// worked example - the gate cell `(3264, 5952)` is open only from the south,
/// and from inside the town `Z+` blocks 96 units short of it. A planner that
/// insisted on standing *on* the exit would report `NoPath` on a gate the
/// player walks through every playthrough.
///
/// What actually opens a scene exit is the walk-on trigger band on the
/// approach, so the follower's job is to get as close as the walls allow and
/// keep pressing; the leg succeeds on [`Leg::Transitioned`], not on arrival.
///
/// Edges are certified by [`World::field_dir_blocked`] - retail
/// `FUN_801cfe4c`'s static-wall arm, the same probe the locomotion step runs -
/// evaluated at the source node, with the lattice pitch held under the probe
/// reach (see [`SUBCELL`]).
///
/// `None` only when nothing at all is reachable from `from`.
fn plan_path(host: &SceneHost, from: Cell, goal_tile: (i16, i16)) -> Option<Vec<Cell>> {
    let goal_w = tile_center(goal_tile);
    let goal_c = cell_of(goal_w.0, goal_w.1);
    let score = |c: Cell| (c.0 - goal_c.0).abs() + (c.1 - goal_c.1).abs();

    let mut seen: HashMap<Cell, Cell> = HashMap::new();
    let mut queue = VecDeque::new();
    seen.insert(from, from);
    queue.push_back(from);
    let mut best = from;

    while let Some(cur) = queue.pop_front() {
        if seen.len() > MAX_PLAN_NODES {
            break;
        }
        if score(cur) < score(best) {
            best = cur;
        }
        if cur == goal_c {
            break;
        }
        let (cx, cz) = cell_center(cur);
        for ((dx, dz), dir) in STEPS {
            let next = (cur.0 + dx, cur.1 + dz);
            if next.0 < 0 || next.1 < 0 || seen.contains_key(&next) {
                continue;
            }
            if host.world.field_dir_blocked(cx, cz, dir) {
                continue;
            }
            seen.insert(next, cur);
            queue.push_back(next);
        }
    }
    if best == from {
        return None;
    }
    let mut path = vec![best];
    let mut step = best;
    while step != from {
        step = seen[&step];
        if step != from {
            path.push(step);
        }
    }
    path.reverse();
    Some(path)
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

/// Everything known about the spot a leg died on.
///
/// The two wall arms are reported separately because they fail for different
/// reasons: `wall` is [`World::field_dir_blocked`] (retail `FUN_801cfe4c`'s
/// static-wall probe, the same one the planner consults), `actor` is
/// [`World::field_actor_dir_blocked`] (the NPC-body arm, which the planner
/// does **not** consult - a walker parked in a doorway is invisible to the
/// route and blocks it anyway).
struct StallSite {
    tile: (i16, i16),
    world: (i16, i16),
    goal: (i16, i16),
    /// Next tile the planner wanted, if it still had a route.
    want: Option<(i16, i16)>,
    /// Per-direction block, indexed as [`STEPS`]: Z-, X-, Z+, X+.
    wall: [bool; 4],
    actor: [bool; 4],
}

/// The tile as the **walk-on dispatch** quantises it: a raw `world >> 7`,
/// not the `(world - 0x40) >> 7` the planner and the region refresh use.
/// The two agree at tile centres and differ by a half-tile band elsewhere,
/// so a stall reported only in planner tiles can sit in a trigger band while
/// naming the tile before it. Retail's own dispatcher uses this form
/// (`FUN_801D1EC4`); see `SceneHost::dispatch_walk_on_trigger`.
fn dispatch_tile(x: i16, z: i16) -> (i32, i32) {
    (i32::from(x) >> 7, i32::from(z) >> 7)
}

impl StallSite {
    fn dirs(flags: [bool; 4]) -> String {
        const NAME: [&str; 4] = ["Z-", "X-", "Z+", "X+"];
        let hit: Vec<_> = NAME
            .iter()
            .zip(flags)
            .filter_map(|(n, b)| b.then_some(*n))
            .collect();
        if hit.is_empty() {
            "none".to_string()
        } else {
            hit.join(",")
        }
    }
}

/// Outcome of driving one leg of the ladder.
enum Leg {
    /// A scene transition fired mid-leg - the leg's real success shape when
    /// the goal is a door tile.
    Transitioned(String),
    /// Standing on the goal tile with no transition.
    Reached,
    /// Control never came back: the opening choreography or a dialogue box
    /// held input for the whole budget.
    InputLocked,
    /// The walkability grid offers no route from here to the goal.
    NoPath { at: (i16, i16), goal: (i16, i16) },
    /// Movement stopped closing distance. Carries what the engine itself
    /// thinks of the spot, because "the player stopped here" and "the engine
    /// says every useful direction is a wall" are different findings and the
    /// bare tile cannot tell them apart.
    Stalled(Box<StallSite>),
    /// Still moving, but out of frames.
    Timeout { at: (i16, i16), goal: (i16, i16) },
}

/// Rendered into the printed table. Written by hand rather than derived so
/// the failure shapes name their tile in the line a reader actually sees -
/// a stall's whole value is the coordinate it died on.
impl std::fmt::Display for Leg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Leg::Transitioned(name) => write!(f, "entered {name}"),
            Leg::Reached => write!(f, "reached the goal tile"),
            Leg::InputLocked => write!(f, "control never released"),
            Leg::NoPath { at, goal } => {
                write!(f, "no walkable route {at:?} -> {goal:?}")
            }
            Leg::Stalled(s) => write!(
                f,
                "STALLED at tile {:?} world {:?} (dispatch tile {:?}) heading for {:?} \
                 (next waypoint {:?}); walls {}; actors {}",
                s.tile,
                s.world,
                dispatch_tile(s.world.0, s.world.1),
                s.goal,
                s.want,
                StallSite::dirs(s.wall),
                StallSite::dirs(s.actor),
            ),
            Leg::Timeout { at, goal } => {
                write!(f, "out of frames at tile {at:?} heading for {goal:?}")
            }
        }
    }
}

fn player_world(host: &SceneHost) -> (i16, i16) {
    let slot = host.world.player_actor_slot.expect("player actor") as usize;
    let ms = &host.world.actors[slot].move_state;
    (ms.world_x, ms.world_z)
}

/// Tick with a neutral pad until the world hands control back, so a leg does
/// not spend its budget shoving against a cutscene lock.
fn wait_for_input(host: &mut SceneHost) -> bool {
    for _ in 0..INPUT_RELEASE_BUDGET {
        if !host.world.cutscene_timeline_active() && !host.world.dialogue_owns_input() {
            return true;
        }
        host.world.set_pad(0);
        if host.tick().is_err() {
            return false;
        }
    }
    false
}

/// Drive the player to `goal` using only the pad. Returns the leg outcome.
fn walk_to(host: &mut SceneHost, goal: (i16, i16)) -> Leg {
    if !wait_for_input(host) {
        return Leg::InputLocked;
    }

    let start_w = player_world(host);
    let start = tile_of(start_w.0, start_w.1);
    if plan_path(host, cell_of(start_w.0, start_w.1), goal).is_none() {
        return Leg::NoPath { at: start, goal };
    }

    let dist = |a: (i16, i16), b: (i16, i16)| ((a.0 - b.0).abs() + (a.1 - b.1).abs()) as i32;
    let mut best = dist(start, goal);
    let mut since_progress = 0u32;

    // The plan is always rooted at the tile the player is standing on, and
    // replanned the moment they leave it. Following a fixed waypoint list
    // does not survive contact with the collision step: `advance_with_collision`
    // wall-slides, so the player routinely arrives on a tile that is not the
    // next waypoint, and a follower that only pops on an exact match then
    // steers back at a waypoint behind them and oscillates in place.
    let mut planned_from = cell_of(start_w.0, start_w.1);
    let mut path = plan_path(host, planned_from, goal).unwrap_or_default();

    for _ in 0..LEG_FRAME_BUDGET {
        let (wx, wz) = player_world(host);
        let here = tile_of(wx, wz);
        if here == goal {
            return Leg::Reached;
        }

        let cell = cell_of(wx, wz);
        if cell != planned_from {
            // A plan that cannot improve is not a dead end: the last stretch
            // into a scene exit is *supposed* to run out of standable nodes,
            // because the door tile reads as a wall. Fall through with an
            // empty path and press straight at the goal - that press is what
            // crosses the walk-on band. The stall detector below still bounds
            // it, so a genuine dead end is reported rather than looped on.
            path = plan_path(host, cell, goal).unwrap_or_default();
            planned_from = cell;
        }
        let (tx, tz) = match path.first() {
            Some(&c) => cell_center(c),
            None => tile_center(goal),
        };
        let pad = pad_for_world_step(
            host.world.field_camera_azimuth,
            (tx - wx).signum(),
            (tz - wz).signum(),
        );

        host.world.set_pad(pad);
        match host.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Leg::Transitioned(name),
            Ok(_) => {}
            Err(_) => return Leg::Timeout { at: here, goal },
        }

        // A dialogue or cutscene that opened mid-leg owns input now; let it
        // finish rather than burning frames against it.
        if (host.world.cutscene_timeline_active() || host.world.dialogue_owns_input())
            && !wait_for_input(host)
        {
            return Leg::InputLocked;
        }

        // Progress is measured against the best distance ever achieved, so
        // wall-sliding sideways for a while is tolerated but circling is not.
        // Replanning happens above on every tile change, so reaching
        // STALL_FRAMES here means the route is being followed and still not
        // closing - that is a stall, not a stale plan.
        let now = tile_of(player_world(host).0, player_world(host).1);
        let d = dist(now, goal);
        if d < best {
            best = d;
            since_progress = 0;
        } else {
            since_progress += 1;
            if since_progress >= STALL_FRAMES {
                let (sx, sz) = player_world(host);
                let probe = |f: &dyn Fn(usize) -> bool| [f(0), f(1), f(2), f(3)];
                return Leg::Stalled(Box::new(StallSite {
                    tile: now,
                    world: (sx, sz),
                    goal,
                    want: plan_path(host, cell_of(sx, sz), goal)
                        .and_then(|p| p.first().copied())
                        .map(tile_of_cell),
                    wall: probe(&|d| host.world.field_dir_blocked(sx, sz, d)),
                    actor: probe(&|d| host.world.field_actor_dir_blocked(sx, sz, d)),
                }));
            }
        }
    }
    let at = tile_of(player_world(host).0, player_world(host).1);
    Leg::Timeout { at, goal }
}

/// Tile of the first overworld portal to `dest` on the loaded map.
fn portal_tile(host: &SceneHost, dest: &str) -> Option<(i16, i16)> {
    host.world
        .world_map_entity_configs
        .iter()
        .zip(host.world.world_map_entity_positions.iter())
        .find_map(|(cfg, &(x, z))| match cfg {
            WorldMapEntityConfig::OverworldPortal { scene_name, .. } if scene_name == dest => {
                Some((x, z))
            }
            _ => None,
        })
}

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

/// One rung's verdict, for the printed table.
struct Rung {
    label: &'static str,
    detail: String,
    cleared: bool,
}

/// Drive the ladder, returning the rungs attempted. Stops at the first
/// failure - later rungs depend on earlier ones having landed.
fn run_ladder(host: &mut SceneHost) -> Vec<Rung> {
    let mut rungs = Vec::new();

    // --- 1. Cold boot into Rim Elm free-roam with control released. -------
    host.enter_field_scene("town01", 0).expect("enter town01");
    let field = host.world.mode == SceneMode::Field;
    let released = field && wait_for_input(host);
    rungs.push(Rung {
        label: "town01 free-roam, input released",
        detail: if released {
            let (x, z) = player_world(host);
            format!("player at tile {:?}", tile_of(x, z))
        } else if field {
            "control never released (cutscene timeline or dialogue held input)".into()
        } else {
            format!("scene mode is {:?}, expected Field", host.world.mode)
        },
        cleared: released,
    });
    if !released {
        return rungs;
    }

    // --- 2. Walk out of the south gate onto the overworld. ----------------
    let goal = (
        i16::from(TOWN01_SOUTH_GATE.0),
        i16::from(TOWN01_SOUTH_GATE.1),
    );
    let leg = walk_to(host, goal);
    let cleared = matches!(&leg, Leg::Transitioned(n) if n == "map01");
    rungs.push(Rung {
        label: "pad-walk town01 south gate -> map01",
        detail: format!("{leg}"),
        cleared,
    });
    if !cleared {
        return rungs;
    }

    // --- 3. Walk the overworld onto the Ravine portal. --------------------
    let detail;
    let cleared = match portal_tile(host, "keikoku") {
        None => {
            detail = "no keikoku overworld portal on map01".to_string();
            false
        }
        Some(goal) => {
            let leg = walk_to(host, goal);
            let ok = matches!(&leg, Leg::Transitioned(n) if n == "keikoku");
            detail = format!("{leg}");
            ok
        }
    };
    rungs.push(Rung {
        label: "pad-walk map01 -> keikoku (Ravine)",
        detail,
        cleared,
    });

    rungs
}

#[test]
fn critical_path_score_does_not_regress() {
    let Some(mut host) = open_host() else {
        return;
    };

    let rungs = run_ladder(&mut host);
    let score = rungs.iter().take_while(|r| r.cleared).count();
    let baseline = read_baseline();

    eprintln!("\n=== critical-path replay (pad-driven, one session) ===");
    for r in &rungs {
        eprintln!(
            "  [{}] {:<40} {}",
            if r.cleared { "ok" } else { "--" },
            r.label,
            r.detail
        );
    }
    eprintln!(
        "  score {score} / {} rungs attempted (baseline {baseline})",
        rungs.len()
    );
    if score > baseline {
        eprintln!(
            "  baseline can be raised: set `reached = {score}` in {}",
            baseline_path().display()
        );
    }
    eprintln!();

    assert!(
        score >= baseline,
        "critical-path score regressed: {score} < baseline {baseline}. \
         The first unclear rung above names the tile the run died on."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pad inversion must round-trip through the same quadrant rotation
    /// `decode_field_direction` applies, for every quadrant and every
    /// cardinal world step. Disc-free: this is arithmetic, and it is the one
    /// piece of the navigator that can be wrong in a way that still looks
    /// like movement (the player walks confidently in the wrong direction).
    #[test]
    fn pad_inversion_round_trips_through_every_quadrant() {
        // Mirror of World::decode_field_direction's screen -> world rotation.
        fn forward(azimuth: u16, pad: u16) -> (i16, i16) {
            let mut sx = 0i16;
            let mut sy = 0i16;
            if pad & PadButton::Up.mask() != 0 {
                sy += 1;
            }
            if pad & PadButton::Down.mask() != 0 {
                sy -= 1;
            }
            if pad & PadButton::Right.mask() != 0 {
                sx += 1;
            }
            if pad & PadButton::Left.mask() != 0 {
                sx -= 1;
            }
            let quadrant = ((azimuth as u32).wrapping_add(512) / 1024) & 3;
            let (wx, wz) = match quadrant {
                0 => (sx, sy),
                1 => (sy, -sx),
                2 => (-sx, -sy),
                _ => (-sy, sx),
            };
            (wx.clamp(-1, 1), wz.clamp(-1, 1))
        }

        // One azimuth inside each quadrant, including the wrap boundary.
        for azimuth in [0u16, 700, 1024, 1800, 2048, 2900, 3072, 4000] {
            for (dwx, dwz) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (-1, 1),
                (1, -1),
                (-1, -1),
            ] {
                let pad = pad_for_world_step(azimuth, dwx, dwz);
                assert_eq!(
                    forward(azimuth, pad),
                    (dwx, dwz),
                    "azimuth {azimuth} step ({dwx},{dwz}) did not round-trip (pad {pad:#06x})"
                );
            }
        }
    }

    /// A missing baseline file reads as zero rather than panicking, so a
    /// fresh clone runs the ladder instead of failing on a file it has not
    /// been given.
    #[test]
    fn absent_baseline_reads_as_zero() {
        // `read_baseline` resolves a fixed repo path; this asserts the parse
        // contract on the text form directly.
        let parse = |text: &str| -> usize {
            text.lines()
                .map(str::trim)
                .filter(|l| !l.starts_with('#'))
                .find_map(|l| {
                    l.strip_prefix("reached")?
                        .trim()
                        .strip_prefix('=')?
                        .trim()
                        .parse()
                        .ok()
                })
                .unwrap_or(0)
        };
        assert_eq!(parse(""), 0);
        assert_eq!(
            parse("# reached = 9\n"),
            0,
            "a commented row is not a value"
        );
        assert_eq!(parse("reached = 2\n"), 2);
        assert_eq!(parse("# note\nreached = 3\n"), 3);
    }
}
