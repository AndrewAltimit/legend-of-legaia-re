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
//! | 3 | pad-walk `map01` across the continent | overworld remap + collision + the encounter round trip |
//! | 4 | pad-walk onto the `keikoku` portal | overworld route + portal engage |
//!
//! Rungs 3 and 4 are one leg scored twice, and they are split because the
//! overworld leg is sixty-odd tiles long: under a single portal check, "walked
//! nowhere" and "crossed the continent and was turned back at the last ridge"
//! are the same number. Rung 3's threshold is
//! [`OVERWORLD_CROSSING_TILES`] **and** at least one random encounter fought
//! and survived - the capability [`drain_battle`] documents, and the one a
//! regression would otherwise re-break silently.
//!
//! ## Navigation
//!
//! Waypoints come from a BFS over the walkability grid, with each edge
//! validated by [`World::field_dir_blocked`] (retail `FUN_801cfe4c`'s
//! static-wall arm) *and* [`World::field_actor_dir_blocked`] (its prop / NPC
//! sibling) at the source node - so the planner and the engine consult the
//! same obstacles. The follower converts the desired world step into a pad
//! mask by inverting whatever remap the world is currently walking under,
//! which is what a player does when they look at the screen and press the
//! direction that moves them the way they want.
//!
//! There are **two** such remaps and they are not interchangeable: the field
//! walk goes through `decode_field_direction` (world axes, quadrant rotation),
//! the overworld walk through `world_map_camera_relative_bits` (camera-relative).
//! Inverting the field one on the overworld sends the player off at an angle,
//! and map01's collision leaves the sea open, so nothing stops them - the leg
//! walks off the map rather than stalling against a wall. See
//! [`pad_for_step`].
//!
//! On the overworld the route has two more constraints a field route does not:
//! other scenes' portals are **hazards** to route around, because stepping on
//! one enters it ([`portal_hazards`]), and a scene with several doors needs
//! the door the walk can actually reach ([`portal_tile`]).
//!
//! A leg that stops making progress is reported as `Stalled` with the tile it
//! died on, not as a bare assertion failure - a stall is the finding.
//!
//! ## A leg is not always walking
//!
//! Two modes take the frame away mid-leg and they are not the same problem.
//! A dialogue or cutscene timeline owns *input* while the world stays put
//! ([`drain_scripted`]). A random encounter replaces the *world*: in
//! [`SceneMode::Battle`] the player actor's `move_state` is the battle arena
//! transform, so [`player_world`] stops meaning an overworld position at all.
//! [`drain_battle`] sits both out, and reading its doc comment first will save
//! re-deriving why an unguarded run reports a coordinate off the corner of a
//! map the player is standing in the middle of.
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

use std::collections::{HashMap, HashSet, VecDeque};
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

/// Rim Elm's south-gate exit tile: the band carrying the `0x3F` scene change.
///
/// The `.MAP` kind-1 table gives this gate two bands, and only one of them is
/// a door. Record 10 over tiles `(24..26, 45)` + `(25, 44)` is a **content-free
/// park** - five bytes of `Nop; Nop; JmpRel`-to-self, no scene change, no
/// walk - so standing in it does nothing by design. Record 0 over
/// `(24..26, 46)` is the exit: its script is `CFlag.Set`, an `Effect` fade and
/// the `0x3F` naming `map01` at entry tile `(0x60, 0x19)`.
///
/// Record 0's band is walled on a fresh boot, and the wall is **not** a fixed
/// map feature: it is the gate itself, opened by
/// [`GATE_OPEN_FLAGS`](self::GATE_OPEN_FLAGS). See that constant.
const TOWN01_SOUTH_GATE: (u8, u8) = (25, 46);

/// The two system flags Rim Elm's south gate is authored on: `327` ("the gate
/// scenery exists") and `321` ("the gate is open").
///
/// `town01`'s gate-object script `P0[20]` - bound to the object at tile
/// `(23, 43)` by the `.MAP`'s gate-0 kind-1 trigger, and run by the scene-init
/// bind prologue (`FUN_8003A55C`) - clears the approach band with three
/// `0x4C` nibble-7 sub-0 paints and then branches:
///
/// | `327` | `321` | what the script paints | gate |
/// |---|---|---|---|
/// | clear | - | nothing more; the base map's row-47 wall stands | shut |
/// | set | clear | re-blocks rows 44..46 and seats the gate at `(24, 44)` | shut |
/// | set | set | `sub-0` over cols `24..25`, rows `46..47` | **open** |
///
/// Only that last arm clears grid row 47 cols 24-25, and those are the cells
/// that block the walk - so on a cold boot a player cannot leave Rim Elm, in
/// the port or in retail, and the disc says so rather than the engine.
///
/// Seeding the pair here stands in for the town's story beats the same way the
/// other progression oracles seed their gates: this ladder measures locomotion,
/// collision and the pad remap, not story progression. Note what is *not*
/// seeded - `562`, record 10's own `C2` gate. Setting it would spawn that
/// content-free park as a modal timeline mid-walk; it gates a beat, not the
/// door.
const GATE_OPEN_FLAGS: [u16; 2] = [327, 321];

/// `0x482` - the Drake **mist walls**.
///
/// `map01`'s partition-2 records `P2[34..36]` are `C1 = [0x482]` walk-on
/// *beat* bands with no `0x3F`: while the flag is clear they shove the player
/// back off a not-yet-unlocked path, and `SceneHost::dispatch_walk_on_trigger`
/// spawns them as cutscene timelines that stand overworld locomotion down
/// while they run (see `world-map.md`, overworld walk-on beat records). Retail
/// lifts them on the post-Zeto Drake-revival beat.
///
/// So on a cold boot the Ravine is **closed by design**, and the port is
/// faithful about it: an unseeded run walks the overworld for sixty tiles and
/// is turned back short of every `keikoku` mouth, having run the band records
/// on the way. Seeding it here is the same move [`GATE_OPEN_FLAGS`] makes for
/// Rim Elm's gate and for the same reason - this ladder scores locomotion,
/// collision and the pad remap, not story progression.
const MIST_WALL_FLAG: u16 = 0x482;

/// Frames a leg may spend before it is called a timeout.
const LEG_FRAME_BUDGET: u32 = 6_000;

/// Frames without closing distance on the goal before a leg is called stalled.
/// Generous enough to absorb a wall-slide detour around a building.
const STALL_FRAMES: u32 = 240;

/// Frames to tick (pad neutral) waiting for the scene's opening choreography
/// to hand control back before a leg starts driving.
const INPUT_RELEASE_BUDGET: u32 = 3_600;

/// Manhattan tile distance from the `map01` arrival that counts as having
/// crossed the overworld rather than having taken a few steps on it.
///
/// The `keikoku` mouths sit 60-odd tiles from where Rim Elm's gate lands the
/// player, and a leg that dies on the first tile and a leg that dies two
/// tiles short are the same score under a pass/fail portal check. 24 tiles is
/// three screens of walking - far enough that reaching it needs the remap, the
/// collision probe, the region-encounter round trip and the replanner all
/// working, and short enough that it does not silently become a second name
/// for the portal rung.
const OVERWORLD_CROSSING_TILES: i32 = 24;

/// Frames one random encounter may run before the leg gives up on it.
///
/// A leg on the overworld **will** be interrupted: `map01` carries a live
/// region-keyed encounter table and the walk crosses tile after tile, so a
/// pad-driven crossing fights several battles on the way. A battle is not
/// part of the leg's own frame or stall budget - see [`drain_battle`] - so
/// this is sized as a safety net (an observed `map01` encounter resolves in
/// well under a thousand ticks), not as a target.
const BATTLE_FRAME_BUDGET: u32 = 20_000;

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

/// The **overworld** pad inversion, which is a different space.
///
/// The two walks do not share a remap:
/// [`World::step_field_locomotion`] routes the pad through
/// `decode_field_direction` (world axes, quadrant rotation), while
/// `World::step_world_map_locomotion` routes it through
/// `world_map_camera_relative_bits` - screen-up -> world `(-cosθ, -sinθ)`,
/// screen-right -> world `(sinθ, -cosθ)`. Inverting the field remap on the
/// overworld sends the player off at an angle, and map01's collision grid
/// leaves the sea open, so there is nothing to stop them: the leg walks off
/// the map instead of stalling against a wall.
///
/// That 2x2 is a reflection, so it is its own inverse - the screen delta for
/// a desired world step is the same matrix applied to the step. The `T` band
/// matches the forward map's 8-direction quantisation.
fn world_map_pad_for_world_step(azimuth: i32, dwx: i16, dwz: i16) -> u16 {
    let (dx, dz) = (f32::from(dwx), f32::from(dwz));
    let len = (dx * dx + dz * dz).sqrt();
    if len == 0.0 {
        return 0;
    }
    let theta = (azimuth as f32) / 4096.0 * std::f32::consts::TAU;
    let (sin, cos) = theta.sin_cos();
    let (dx, dz) = (dx / len, dz / len);
    let sx = dx * sin - dz * cos;
    let sy = -dx * cos - dz * sin;
    /// sin(22.5°) - the same cardinal band `world_map_camera_relative_bits` uses.
    const T: f32 = 0.382_683_43;
    let mut pad = 0u16;
    if sy > T {
        pad |= PadButton::Up.mask();
    } else if sy < -T {
        pad |= PadButton::Down.mask();
    }
    if sx > T {
        pad |= PadButton::Right.mask();
    } else if sx < -T {
        pad |= PadButton::Left.mask();
    }
    pad
}

/// Pick the pad for a desired world step in whichever space the world is
/// currently walking in.
fn pad_for_step(host: &SceneHost, dwx: i16, dwz: i16) -> u16 {
    match host.world.mode {
        SceneMode::WorldMap => world_map_pad_for_world_step(
            host.world.world_map_ctrl.as_ref().map_or(0, |c| c.azimuth),
            dwx,
            dwz,
        ),
        _ => pad_for_world_step(host.world.field_camera_azimuth, dwx, dwz),
    }
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
/// `avoid` names **dispatch tiles** (`world >> 7`) the route must not enter.
/// On the overworld those are the other scenes' portals: standing on one is
/// how you enter it (`World::auto_engage_world_map_portals` matches the
/// player's `>> 7` tile against the entity's), so a route that crosses one
/// never arrives - it ends the leg in the wrong scene. Retail's own player has
/// the same constraint and routes around it; the planner has to as well.
///
/// `None` only when nothing at all is reachable from `from`.
fn plan_path(
    host: &SceneHost,
    from: Cell,
    goal_tile: (i16, i16),
    avoid: &HashSet<(i32, i32)>,
) -> Option<Vec<Cell>> {
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
            if host.world.field_dir_blocked(cx, cz, dir)
                || host.world.field_actor_dir_blocked(cx, cz, dir)
            {
                continue;
            }
            if !avoid.is_empty() {
                let (nx, nz) = cell_center(next);
                if avoid.contains(&dispatch_tile(nx, nz)) {
                    continue;
                }
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
    /// How many scripted sequences (dialogue / cutscene timeline) ran during
    /// the leg. Non-zero at a stall means a walk-on trigger *did* fire and
    /// its record *did* execute - the leg failed after the dispatch, not
    /// before it, which is a different defect entirely.
    scripted: u32,
    /// Random encounters fought during the leg ([`drain_battle`]). Reported
    /// separately because it splits "the walk never got anywhere" from "the
    /// walk was interrupted N times and still got nowhere" - and because a
    /// leg that fights *nothing* while crossing an overworld with a live
    /// region-encounter table is itself a finding.
    fought: u32,
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
    /// A random encounter never returned control to the walking mode.
    BattleUnresolved {
        at: (i16, i16),
        ended_in: Option<SceneMode>,
    },
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
            )
            .and_then(|()| {
                if s.scripted > 0 {
                    write!(f, "; {} scripted sequence(s) ran", s.scripted)
                } else {
                    Ok(())
                }
            })
            .and_then(|()| {
                if s.fought > 0 {
                    write!(f, "; {} random encounter(s) fought", s.fought)
                } else {
                    Ok(())
                }
            }),
            Leg::Timeout { at, goal } => {
                write!(f, "out of frames at tile {at:?} heading for {goal:?}")
            }
            Leg::BattleUnresolved { at, ended_in } => write!(
                f,
                "a random encounter near tile {at:?} never returned control \
                 (mode {ended_in:?})"
            ),
        }
    }
}

fn player_world(host: &SceneHost) -> (i16, i16) {
    let slot = host.world.player_actor_slot.expect("player actor") as usize;
    let ms = &host.world.actors[slot].move_state;
    (ms.world_x, ms.world_z)
}

/// Outcome of letting a scripted sequence run to completion.
enum Drain {
    /// The sequence performed a scene change - the leg's success condition.
    Entered(String),
    /// It finished and handed input back.
    Released,
    /// It never handed input back inside the budget.
    Locked,
}

/// Tick with a neutral pad while a dialogue or cutscene owns input, surfacing
/// a scene change if the sequence performs one.
fn drain_scripted(host: &mut SceneHost) -> Drain {
    for _ in 0..INPUT_RELEASE_BUDGET {
        if !host.world.cutscene_timeline_active() && !host.world.dialogue_owns_input() {
            return Drain::Released;
        }
        host.world.set_pad(0);
        match host.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Drain::Entered(name),
            Ok(_) => {}
            Err(_) => return Drain::Locked,
        }
    }
    Drain::Locked
}

/// Outcome of sitting out a random encounter.
enum Fight {
    /// The battle ended and the walking mode came back.
    Survived,
    /// Still in [`SceneMode::Battle`] after [`BATTLE_FRAME_BUDGET`] ticks.
    Stuck,
    /// The battle handed the world to some third mode (a wipe / game over).
    Diverted(SceneMode),
}

/// Sit out a random encounter with a neutral pad and report how it ended.
///
/// **This is not an optional nicety, it is the difference between a leg that
/// measures locomotion and a leg that measures nothing.** `map01` has a live
/// region-keyed encounter table (`World::set_world_map_regions`), so a
/// pad-driven crossing enters [`SceneMode::Battle`] every few hundred ticks.
/// While that mode is up the player actor's `move_state` is the **battle
/// arena** transform, not an overworld position, so a follower that keeps
/// sampling [`player_world`] reads a coordinate from a different space
/// entirely - `(0, -825)` on the first `map01` encounter, which is off the
/// north-west corner of a map the player is standing in the middle of. Every
/// downstream number then lies in the same direction at once: the distance
/// check sees no progress and trips the stall detector, and the stall site
/// reports a tile the player was never on with all four walls set (an
/// out-of-grid probe reads as solid). Read as locomotion it looks exactly like
/// an inverted movement axis; it is a mode confusion.
///
/// A neutral pad is deliberate: the engine's battle resolves under its own
/// action state machine, and pressing into it would make the leg's outcome a
/// function of the battle UI rather than of the walk. Battle ticks are charged
/// to [`BATTLE_FRAME_BUDGET`], never to the leg's frame or stall budget -
/// fighting is not stalling.
fn drain_battle(host: &mut SceneHost, resume: SceneMode) -> Fight {
    for _ in 0..BATTLE_FRAME_BUDGET {
        host.world.set_pad(0);
        if host.tick().is_err() {
            return Fight::Stuck;
        }
        if host.world.mode == resume {
            return Fight::Survived;
        }
        if host.world.mode != SceneMode::Battle {
            return Fight::Diverted(host.world.mode);
        }
    }
    Fight::Stuck
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

/// What a leg did, independently of whether it ended where it was aimed.
///
/// A pass/fail outcome cannot distinguish "the walk never started" from "the
/// walk crossed the continent and was turned back at the last ridge", and on a
/// 60-tile leg that is the whole of the interesting range. These are what the
/// ladder scores the crossing on.
#[derive(Default)]
struct LegStats {
    /// Random encounters entered and survived ([`drain_battle`]).
    fought: u32,
    /// Furthest Manhattan tile distance from the leg's start the player ever
    /// reached - not the final position, which a wall-slide back can shorten.
    reach: i32,
}

/// Drive the player to `goal` using only the pad. Returns the leg outcome.
fn walk_to(
    host: &mut SceneHost,
    goal: (i16, i16),
    avoid: &HashSet<(i32, i32)>,
    stats: &mut LegStats,
) -> Leg {
    if !wait_for_input(host) {
        return Leg::InputLocked;
    }

    let start_w = player_world(host);
    let start = tile_of(start_w.0, start_w.1);
    if plan_path(host, cell_of(start_w.0, start_w.1), goal, avoid).is_none() {
        return Leg::NoPath { at: start, goal };
    }

    let dist = |a: (i16, i16), b: (i16, i16)| ((a.0 - b.0).abs() + (a.1 - b.1).abs()) as i32;
    let mut best = dist(start, goal);
    let mut since_progress = 0u32;
    let mut scripted = 0u32;
    // The mode this leg walks in. A leg never changes it: leaving it means a
    // battle (drained below) or a scene change (the leg's success).
    let walking_mode = host.world.mode;

    // The plan is always rooted at the tile the player is standing on, and
    // replanned the moment they leave it. Following a fixed waypoint list
    // does not survive contact with the collision step: `advance_with_collision`
    // wall-slides, so the player routinely arrives on a tile that is not the
    // next waypoint, and a follower that only pops on an exact match then
    // steers back at a waypoint behind them and oscillates in place.
    let mut planned_from = cell_of(start_w.0, start_w.1);
    let mut path = plan_path(host, planned_from, goal, avoid).unwrap_or_default();

    for _ in 0..LEG_FRAME_BUDGET {
        // A random encounter took the frame. `player_world` is meaningless
        // until it hands back (see `drain_battle`), so sit it out *before*
        // sampling anything - and charge none of it to the leg.
        if host.world.mode == SceneMode::Battle {
            stats.fought += 1;
            match drain_battle(host, walking_mode) {
                Fight::Survived => {
                    // Fighting is not stalling: the walk resumes from where it
                    // was interrupted, so give it a fresh stall window.
                    since_progress = 0;
                    continue;
                }
                Fight::Stuck => {
                    return Leg::BattleUnresolved {
                        at: goal,
                        ended_in: None,
                    };
                }
                Fight::Diverted(mode) => {
                    return Leg::BattleUnresolved {
                        at: goal,
                        ended_in: Some(mode),
                    };
                }
            }
        }

        let (wx, wz) = player_world(host);
        let here = tile_of(wx, wz);
        stats.reach = stats.reach.max(dist(here, start));
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
            path = plan_path(host, cell, goal, avoid).unwrap_or_default();
            planned_from = cell;
        }
        let (tx, tz) = match path.first() {
            Some(&c) => cell_center(c),
            None => tile_center(goal),
        };
        let pad = pad_for_step(host, (tx - wx).signum(), (tz - wz).signum());

        host.world.set_pad(pad);
        match host.tick() {
            Ok(SceneTickEvent::SceneEntered { name }) => return Leg::Transitioned(name),
            Ok(_) => {}
            Err(_) => return Leg::Timeout { at: here, goal },
        }

        // The tick that just ran may have rolled an encounter. Hand the frame
        // straight back to the loop head rather than measuring progress off a
        // battle-arena coordinate.
        if host.world.mode == SceneMode::Battle {
            continue;
        }

        // A dialogue or cutscene that opened mid-leg owns input now; let it
        // finish rather than burning frames against it. Crucially it may also
        // be the *success* path - a walk-on trigger's partition-2 record
        // installs as a timeline and that timeline is what performs the scene
        // change - so the drain has to surface `SceneEntered` rather than
        // swallow it. (It did swallow it, which read as a stall at the gate.)
        if host.world.cutscene_timeline_active() || host.world.dialogue_owns_input() {
            scripted += 1;
            match drain_scripted(host) {
                Drain::Entered(name) => return Leg::Transitioned(name),
                Drain::Released => {}
                Drain::Locked => return Leg::InputLocked,
            }
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
                if std::env::var_os("LEGAIA_CPR_DEBUG").is_some() {
                    for c in &host.world.field_prop_colliders {
                        if (c.center.0 - i32::from(sx)).abs() < 400
                            && (c.center.1 - i32::from(sz)).abs() < 400
                        {
                            eprintln!("[dbg] near prop {c:?}");
                        }
                    }
                    eprintln!(
                        "[dbg] npc positions near: {:?}",
                        host.world
                            .field_npc_positions
                            .values()
                            .filter(|&&(ax, az)| (i32::from(ax) - i32::from(sx)).abs() < 400
                                && (i32::from(az) - i32::from(sz)).abs() < 400)
                            .collect::<Vec<_>>()
                    );
                }
                let probe = |f: &dyn Fn(usize) -> bool| [f(0), f(1), f(2), f(3)];
                return Leg::Stalled(Box::new(StallSite {
                    tile: now,
                    world: (sx, sz),
                    goal,
                    want: plan_path(host, cell_of(sx, sz), goal, avoid)
                        .and_then(|p| p.first().copied())
                        .map(tile_of_cell),
                    wall: probe(&|d| host.world.field_dir_blocked(sx, sz, d)),
                    actor: probe(&|d| host.world.field_actor_dir_blocked(sx, sz, d)),
                    scripted,
                    fought: stats.fought,
                }));
            }
        }
    }
    let at = tile_of(player_world(host).0, player_world(host).1);
    Leg::Timeout { at, goal }
}

/// Tile of the overworld portal to `dest` the player can actually **walk to**.
///
/// Two things make "the first row naming that scene" the wrong answer, and the
/// second one is not obvious:
///
/// 1. `world_map_entity_positions` is in **world** units; [`walk_to`] navigates
///    in tiles. Handing the raw world pair straight through overflows
///    `tile_center`'s `t * 128` on an `i16` and aims the follower at a wrapped
///    coordinate off the map - which, because map01's collision leaves the sea
///    open, it then walks to without ever hitting a wall.
/// 2. **A scene has several overworld doors and they are not interchangeable.**
///    `map01` carries six `keikoku` portals - the Ravine is a corridor with
///    entrances on both sides of the continent. From Rim Elm's exit the
///    table's *first* one is on the far side of terrain the walk cannot cross,
///    and the follower's best-effort push toward it runs it over a **different
///    scene's** portal on the way (`suimon`), which auto-engages and ends the
///    leg in the wrong place. That reads as a navigation failure and is really
///    a goal-selection failure.
///
/// So the goal is chosen by flooding the walkability grid once per candidate
/// and keeping the one the flood gets closest to - the same
/// [`plan_path`] the follower will use, so "reachable" means reachable to the
/// thing that has to do the walking, not to a straight line.
fn portal_tile(host: &SceneHost, dest: &str, avoid: &HashSet<(i32, i32)>) -> Option<(i16, i16)> {
    let from = {
        let (x, z) = player_world(host);
        cell_of(x, z)
    };
    host.world
        .world_map_entity_configs
        .iter()
        .zip(host.world.world_map_entity_positions.iter())
        .filter_map(|(cfg, &(x, z))| match cfg {
            WorldMapEntityConfig::OverworldPortal { scene_name, .. } if scene_name == dest => {
                Some(tile_of(x, z))
            }
            _ => None,
        })
        .map(|goal| {
            let goal_w = tile_center(goal);
            let goal_c = cell_of(goal_w.0, goal_w.1);
            // Residual = how close the flood gets. `None` (nothing reachable
            // at all) scores worse than any route.
            let residual = plan_path(host, from, goal, avoid)
                .and_then(|p| p.last().copied())
                .map_or(i32::MAX, |end| {
                    i32::from((end.0 - goal_c.0).abs() + (end.1 - goal_c.1).abs())
                });
            if std::env::var_os("LEGAIA_CPR_DEBUG").is_some() {
                eprintln!("[dbg] candidate {dest} @ {goal:?} residual {residual}");
            }
            (residual, goal)
        })
        .min_by_key(|&(residual, _)| residual)
        .map(|(_, goal)| goal)
}

/// Dispatch tiles of every overworld portal that is **not** a door to `dest`.
///
/// These are hazards, not obstacles in the collision sense: the grid says they
/// are walkable and the engine agrees, but stepping on one ends the leg in
/// another scene. The route has to go round them, so they are handed to
/// [`plan_path`] as blocked cells. Keyed in the dispatch frame (`world >> 7`)
/// because that is the frame `World::auto_engage_world_map_portals` compares
/// in - matching it in the planner's `(world - 0x40) >> 7` frame would leave a
/// half-tile band of the hazard open.
fn portal_hazards(host: &SceneHost, dest: &str) -> HashSet<(i32, i32)> {
    host.world
        .world_map_entity_configs
        .iter()
        .zip(host.world.world_map_entity_positions.iter())
        .filter_map(|(cfg, &(x, z))| match cfg {
            WorldMapEntityConfig::OverworldPortal { scene_name, .. } if scene_name != dest => {
                Some(dispatch_tile(x, z))
            }
            _ => None,
        })
        .collect()
}

/// Ablation over the four inputs that can seal the overworld route, printed
/// under `LEGAIA_CPR_ABLATE=1`.
///
/// Rung 4 stalls with the flood unable to reach any `keikoku` mouth, and
/// `plan_path` rejects a step for four independent reasons: the walkability
/// grid ([`World::field_dir_blocked`]), placed-prop boxes and live NPC boxes
/// (both inside [`World::field_actor_dir_blocked`]), and the caller's portal
/// hazard set. A residual measured with all four live cannot say which one is
/// the seal, and `handoff/lane6.md` reports its table as "walls only" when
/// `plan_path` in fact consults every one of them - so the ablation is run
/// rather than reasoned about.
///
/// Each row clears exactly one input and re-floods to every `keikoku` mouth.
/// A row whose residual collapses names the seal.
fn ablate_rung4_inputs(host: &mut SceneHost, hazards: &HashSet<(i32, i32)>) {
    let from = {
        let (x, z) = player_world(host);
        cell_of(x, z)
    };
    let goals: Vec<(i16, i16)> = host
        .world
        .world_map_entity_configs
        .iter()
        .zip(host.world.world_map_entity_positions.iter())
        .filter_map(|(cfg, &(x, z))| match cfg {
            WorldMapEntityConfig::OverworldPortal { scene_name, .. } if scene_name == "keikoku" => {
                Some(tile_of(x, z))
            }
            _ => None,
        })
        .collect();

    let residuals = |host: &SceneHost, avoid: &HashSet<(i32, i32)>| -> Vec<i32> {
        goals
            .iter()
            .map(|&goal| {
                let goal_w = tile_center(goal);
                let goal_c = cell_of(goal_w.0, goal_w.1);
                plan_path(host, from, goal, avoid)
                    .and_then(|p| p.last().copied())
                    .map_or(i32::MAX, |end| {
                        i32::from((end.0 - goal_c.0).abs() + (end.1 - goal_c.1).abs())
                    })
            })
            .collect()
    };

    let none: HashSet<(i32, i32)> = HashSet::new();
    let n_props = host.world.field_prop_colliders.len();
    let n_npcs = host.world.field_npc_positions.len();
    eprintln!(
        "[ablate] map01 from {from:?}: {} keikoku mouths, {n_props} props, {n_npcs} npcs, \
         {} hazards",
        goals.len(),
        hazards.len()
    );
    eprintln!("[ablate] mouths {goals:?}");

    let row = |label: &str, host: &SceneHost, avoid: &HashSet<(i32, i32)>| {
        let r = residuals(host, avoid);
        let best = r.iter().copied().min().unwrap_or(i32::MAX);
        eprintln!("[ablate] {label:<28} best {best:>6}  all {r:?}");
    };

    // How big is the pocket the player is actually standing in? A bounded
    // reachable set is the difference between "a terrain barrier the route has
    // to go round" and "the grid decode put the player inside a sealed room".
    let flood_stats = |host: &SceneHost| -> (usize, (i32, i32), (i32, i32)) {
        let mut seen: HashSet<Cell> = HashSet::new();
        let mut queue = VecDeque::new();
        seen.insert(from);
        queue.push_back(from);
        let (mut lo, mut hi) = (from, from);
        while let Some(cur) = queue.pop_front() {
            lo = (lo.0.min(cur.0), lo.1.min(cur.1));
            hi = (hi.0.max(cur.0), hi.1.max(cur.1));
            let (cx, cz) = cell_center(cur);
            for ((dx, dz), dir) in STEPS {
                let next = (cur.0 + dx, cur.1 + dz);
                if next.0 < 0 || next.1 < 0 || seen.contains(&next) {
                    continue;
                }
                if host.world.field_dir_blocked(cx, cz, dir)
                    || host.world.field_actor_dir_blocked(cx, cz, dir)
                {
                    continue;
                }
                seen.insert(next);
                queue.push_back(next);
            }
        }
        (
            seen.len(),
            (i32::from(lo.0), i32::from(lo.1)),
            (i32::from(hi.0), i32::from(hi.1)),
        )
    };
    let reached: HashSet<Cell> = {
        let mut seen: HashSet<Cell> = HashSet::new();
        let mut queue = VecDeque::new();
        seen.insert(from);
        queue.push_back(from);
        while let Some(cur) = queue.pop_front() {
            let (cx, cz) = cell_center(cur);
            for ((dx, dz), dir) in STEPS {
                let next = (cur.0 + dx, cur.1 + dz);
                if next.0 < 0 || next.1 < 0 || seen.contains(&next) {
                    continue;
                }
                if host.world.field_dir_blocked(cx, cz, dir)
                    || host.world.field_actor_dir_blocked(cx, cz, dir)
                {
                    continue;
                }
                seen.insert(next);
                queue.push_back(next);
            }
        }
        seen
    };
    let (n, lo, hi) = flood_stats(host);
    eprintln!(
        "[ablate] reachable pocket: {n} sub-cells, bbox cells {lo:?}..{hi:?} \
         = tiles ({},{})..({},{})",
        lo.0 / 4,
        lo.1 / 4,
        hi.0 / 4,
        hi.1 / 4
    );

    // The wall bits themselves across the pocket's southern edge, at the
    // 64-unit sub-cell granularity the bits are authored in. A boundary that
    // is authored terrain is ragged; one that is an indexing artifact is a
    // straight line.
    eprintln!(
        "[ablate] x tiles 45..110, z tiles 56..72  ('#' wall, 'o' flood-reached, '.' open+unreached):"
    );
    for sz in (56 * 2)..(72 * 2) {
        let zw = sz * 64 + 32;
        let mut line = String::new();
        for sx in (45 * 2)..(110 * 2) {
            let xw = sx * 64 + 32;
            let hit = (0..2).any(|qz| {
                (0..2).any(|qx| reached.contains(&(((sx * 2 + qx) as i16), ((sz * 2 + qz) as i16))))
            });
            line.push(if host.world.field_tile_is_wall(xw as i16, zw as i16) {
                '#'
            } else if hit {
                'o'
            } else {
                '.'
            });
        }
        eprintln!("[ablate] z{:>3}.{} {line}", sz / 2, sz % 2);
    }

    // The x=64 corridor, cell by cell: what the *tile* read says vs what the
    // *directional* probe the planner actually consults says. They are
    // different functions and only the second one gates the flood.
    eprintln!("[ablate] x=64 corridor, tile-read vs dir-probe (Z+ = south):");
    for sz in (62 * 4)..(70 * 4) {
        let zw = sz * 32 + 16;
        let xw = 64 * 128 + 64; // tile centre
        let wall = host.world.field_tile_is_wall(xw as i16, zw as i16);
        let dir_z = host.world.field_dir_blocked(xw as i16, zw as i16, 2);
        let act_z = host.world.field_actor_dir_blocked(xw as i16, zw as i16, 2);
        eprintln!(
            "[ablate]   z {:>3}.{}  world({xw},{zw})  tile_is_wall {:<5}  dir_blocked(Z+) {:<5}  actor {}",
            sz / 4,
            sz % 4,
            wall,
            dir_z,
            act_z
        );
    }

    row("as-is (all inputs live)", host, hazards);
    row("hazards cleared", host, &none);

    let props = std::mem::take(&mut host.world.field_prop_colliders);
    row("props cleared", host, hazards);
    row("props+hazards cleared", host, &none);

    let npcs = std::mem::take(&mut host.world.field_npc_positions);
    row("props+npcs+hazards (walls)", host, &none);

    host.world.field_npc_positions = npcs;
    host.world.field_prop_colliders = props;
    row("restored (sanity, == as-is)", host, hazards);
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

    // Run the collision model **both shipped hosts run**. `play-window`
    // (`window/run.rs`) and the browser play page (`web-viewer/runtime.rs`)
    // each set this pair; a bare `World` defaults both off, so a ladder that
    // left them alone would be scoring a third model no player ever meets.
    //
    // It is also what makes the planner honest. `plan_path` certifies its
    // edges with `field_dir_blocked`, and its doc calls that "the same probe
    // the locomotion step runs" - true only under `leading_edge_wall_probes`.
    // With the flag off, `advance_with_collision` tests the *candidate centre*
    // (`field_tile_is_wall`) instead, so planner and mover disagree about
    // where the walls are, and the disagreement surfaces as a stall whose site
    // reports "walls none" while the player does not move.
    //
    // `solid_field_npcs` only became survivable once `plan_path` started
    // consulting `field_actor_dir_blocked` as well: with solid NPCs and a
    // wall-only planner, a Rim Elm townsperson parked in the route stalls
    // rung 2 at tile `(25, 22)`. The two edits belong together.
    host.world.leading_edge_wall_probes = true;
    host.world.solid_field_npcs = true;

    // Rim Elm's south exit is **story-locked**, and correctly so - but the
    // lock is the gate's own collision, not a script gate on the `0x3F`. The
    // exit record P2[0] (tiles (24..26, 46)) has empty C1/C2; what stops a
    // cold boot leaving is grid row 47, which `P0[20]` only cuts through on
    // its `327`+`321` arm. See `GATE_OPEN_FLAGS`.
    for flag in GATE_OPEN_FLAGS {
        host.world.system_flag_set(flag);
    }
    // The overworld's own story gate. See `MIST_WALL_FLAG`.
    host.world.system_flag_set(MIST_WALL_FLAG);

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
    // No hazards in a town: a field scene's exits are walk-on trigger bands,
    // not entity portals, and the only one on the route is the goal.
    let leg = walk_to(host, goal, &HashSet::new(), &mut LegStats::default());
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
    let hazards = portal_hazards(host, "keikoku");
    let arrival = {
        let (x, z) = player_world(host);
        tile_of(x, z)
    };
    if std::env::var_os("LEGAIA_CPR_ABLATE").is_some() {
        ablate_rung4_inputs(host, &hazards);
    }
    let (leg, stats) = match portal_tile(host, "keikoku", &hazards) {
        None => (None, LegStats::default()),
        Some(goal) => {
            let mut stats = LegStats::default();
            let leg = walk_to(host, goal, &hazards, &mut stats);
            (Some(leg), stats)
        }
    };

    // --- 3. …and the crossing itself, which is the leg's first half. ------
    let crossed = stats.fought > 0 && stats.reach >= OVERWORLD_CROSSING_TILES;
    rungs.push(Rung {
        label: "pad-walk map01 across the continent",
        detail: match &leg {
            None => "no keikoku overworld portal on map01".to_string(),
            Some(_) => format!(
                "reached {} tiles from the {arrival:?} arrival, {} random encounter(s) fought \
                 (needs {OVERWORLD_CROSSING_TILES} and 1)",
                stats.reach, stats.fought
            ),
        },
        cleared: crossed,
    });
    if !crossed {
        return rungs;
    }

    // --- 4. …and the portal engage at the end of it. ----------------------
    rungs.push(Rung {
        label: "pad-walk map01 -> keikoku (Ravine)",
        detail: leg.as_ref().map_or_else(String::new, |l| format!("{l}")),
        cleared: matches!(&leg, Some(Leg::Transitioned(n)) if n == "keikoku"),
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

    /// The overworld inversion is checked against the engine's **own** forward
    /// remap, not a re-derivation of it - the two walks use different spaces
    /// and the field inversion above is silently wrong here.
    ///
    /// The contract is *not* an exact round trip, and asserting one would be a
    /// bug in the test: a 4-way d-pad through
    /// `world_map_camera_relative_bits` reaches only 8 world directions, and
    /// which 8 depends on the azimuth, so at a rotated framing no press yields
    /// a pure cardinal. What the follower needs - and what is asserted - is
    /// that the press always moves the player *toward* the request: the
    /// resulting world direction has a strictly positive dot product with it,
    /// i.e. it is within 90°. Disc-free.
    #[test]
    fn world_map_pad_inversion_always_moves_toward_the_request() {
        use legaia_engine_core::world::world_map_camera_relative_bits;

        for azimuth in [0i32, 700, 1024, 1800, 2048, 2900, 3072, 4000] {
            for (dwx, dwz) in [(1i16, 0i16), (-1, 0), (0, 1), (0, -1)] {
                let pad = world_map_pad_for_world_step(azimuth, dwx, dwz);
                assert_ne!(pad, 0, "azimuth {azimuth} step ({dwx},{dwz}): no press");
                let sx = i32::from(pad & PadButton::Right.mask() != 0)
                    - i32::from(pad & PadButton::Left.mask() != 0);
                let sy = i32::from(pad & PadButton::Up.mask() != 0)
                    - i32::from(pad & PadButton::Down.mask() != 0);
                let bits = world_map_camera_relative_bits(azimuth, sx, sy);
                let gz = i32::from(bits & 0x1000 != 0) - i32::from(bits & 0x4000 != 0);
                let gx = i32::from(bits & 0x2000 != 0) - i32::from(bits & 0x8000 != 0);
                let dot = gx * i32::from(dwx) + gz * i32::from(dwz);
                assert!(
                    dot > 0,
                    "azimuth {azimuth} step ({dwx},{dwz}) walked the wrong way \
                     (pad {pad:#06x} -> screen ({sx},{sy}) -> world ({gx},{gz}))"
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
