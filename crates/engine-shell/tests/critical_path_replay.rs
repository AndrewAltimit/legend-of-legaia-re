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
//! | 1 | `town01` loads into free-roam Field | the scene boots to a controllable player |
//! | 2 | pad-walk to the south gate -> `map01` | field locomotion + collision + walk-on trigger |
//! | 3 | pad-walk `map01` across the continent | overworld remap + collision + the encounter round trip |
//! | 4 | pad-walk on to the Ravine, via `suimon` | multi-scene overworld route + portal engage |
//! | 5 | pad-walk the Ravine interior out of a *different* door | dungeon locomotion + the scene's own exit bands |
//!
//! Rung 5 exists because rungs 2-4 all end the instant a door fires, so none
//! of them walks a dungeon *interior*. That is its own surface: the exits are
//! `.MAP` walk-on bands rather than overworld entity portals and the corridors
//! are the narrowest geometry the leading-edge probe meets. Under a four-rung
//! ladder "the Ravine loads" and "the Ravine can be walked" scored the same.
//!
//! What it does **not** cover is the dungeon's own encounter table, and that
//! is a finding rather than a design choice - `keikoku` decodes to a single
//! rate-0 whole-map region and rolls nothing however the loop is armed. The
//! `LEGAIA_CPR_FIGHT=1` run prints the region set at the rung's start.
//!
//! It is scored on **which door the player came out of**, not on the bare
//! transition. `keikoku` carries four scene-change records and all four return
//! to `map01`, each at its own tile, so the arrival coordinate names the door
//! exactly ([`ExitRecord`]) - and the one thing a step back through the
//! entrance can never produce is a different one.
//!
//! Rung 3 is the first leg of rung 4's route, scored on its own, because that
//! leg is sixty-odd tiles long: under a single portal check, "walked nowhere"
//! and "crossed the continent and was turned back at the last ridge" are the
//! same number. Rung 3's threshold is [`OVERWORLD_CROSSING_TILES`] **and** at
//! least one random encounter fought and survived - the capability
//! [`drain_battle`] documents, and the one a regression would otherwise
//! re-break silently.
//!
//! Rung 4 is **three** legs, not one, because `map01` is not one walk
//! component: the arrival is on the northern half, every `keikoku` mouth is on
//! the southern half, and the crossing between them is the `suimon` scene.
//! See [`SUIMON_SOUTH_EXIT`] and [`WATER_GATE_FLAG`].
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
//! A hazard is a tile it is unsafe to **enter**, never a tile it is unsafe to
//! *occupy*, and the difference is not cosmetic. Retail's dispatcher fires on
//! a tile *change*, so a step that stays inside one tile fires nothing - and a
//! dungeon arrival is seated on the door it came in through, i.e. inside a
//! hazard. A planner that tests only the destination tile therefore refuses
//! its own first step and reports the scene as sealed. See [`plan_path`].
//!
//! A leg that stops making progress is reported as `Stalled` with the tile it
//! died on, not as a bare assertion failure - a stall is the finding.
//!
//! ## A leg is not always walking
//!
//! Two modes take the frame away mid-leg and they are not the same problem.
//! A dialogue or cutscene timeline owns *input* while the world stays put
//! ([`drain_scripted`], which pages through it the way a player does - a
//! neutral pad is enough for a sequence that runs on its own clock and is not
//! enough for one that waits on a confirm). A random encounter replaces the
//! *world*: in
//! [`SceneMode::Battle`] the player actor's `move_state` is the battle arena
//! transform, so [`player_world`] stops meaning an overworld position at all.
//! [`drain_battle`] takes the frame off the walk and hands it to
//! [`FightPolicy`], which *plays* the encounter through the battle command UI
//! rather than sitting it out; reading both doc comments first will save
//! re-deriving why an unguarded run reports a coordinate off the corner of a
//! map the player is standing in the middle of, and why a neutral pad is a
//! fighting model rather than the absence of one.
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

/// The Ravine approach is **three** legs, because `map01` is not one walk
/// component.
///
/// Its wall bits split the kingdom in two: a flood from Rim Elm's arrival tile
/// `(96, 25)` covers ~850 tiles' worth of ground and reaches exactly four
/// overworld doors - `jou`, `town0c`, `izumi` and `suimon` - while all six
/// `keikoku` mouths sit on the other side of a band at minimum four 64-unit
/// wall sub-cells thick. Refining the planner lattice does not help: the
/// reachable area is flat from 32 units of pitch down to **2**, which is the
/// retail stepper's own increment, so no finer discretisation exists
/// ([`probe_rung4_lattice`]).
///
/// `suimon` is the crossing. `map01` partition-2 record `18` (trigger tiles
/// `(55, 62)` / `(56, 61)`) enters it; `suimon`'s records `0`/`1` come back to
/// `map01` tile `(54, 61)` on the **northern** component and its record `2`
/// comes back to `(59, 61)` on the **southern** one. Neither is story-gated
/// (`C1`/`C2` empty), so the crossing is open from a cold boot.
///
/// So routing straight at a `keikoku` mouth cannot work, and treating `suimon`
/// as a portal hazard to walk around - which is what [`portal_hazards`] does
/// for any scene that is not the leg's destination - walls the route off
/// entirely. See `docs/subsystems/world-map.md`.
///
/// This is `suimon`'s southern door: a tile inside record `2`'s trigger band,
/// which runs `(30, 80)`..`(37, 92)`.
const SUIMON_SOUTH_EXIT: (i16, i16) = (34, 85);

/// `suimon`'s **northern** doors, as dispatch tiles for [`plan_path`]'s
/// `avoid` set - the overworld [`portal_hazards`] idea applied to a field
/// scene.
///
/// Entering from `map01` lands the player at `suimon` tile `(68, 44)`
/// (`map01` P2[18]'s `0x3F` at bytecode `+0x1C`; the sibling at `+0x37`, entry
/// `(21, 84)`, is the `SysFlag.Test 0x27B` arm). That is the northern chamber,
/// and record `0`'s trigger band runs straight down its west wall at `x = 66`,
/// `z = 40..53`, with record `1` capping it at `z = 38..39`. A route that
/// heads straight for the southern door crosses one of them and is sent back
/// to `map01` tile `(54, 61)` on the component it started from - the leg
/// reports `Transitioned("map01")` and has gone nowhere.
///
/// So the two northern bands are hazards, exactly like another scene's portal
/// on the overworld: the way south is to leave the chamber past `z = 53` on
/// the far side of the `x = 66` band first. The rectangles over-approximate
/// the on-disc trigger tiles (`suimon`'s `.MAP` fallback table), which is safe
/// - none of them touches record `2`'s band at `x = 30..37`.
fn suimon_north_doors() -> HashSet<(i32, i32)> {
    let mut out = HashSet::new();
    for z in 79..=92 {
        out.insert((19, z));
    }
    for z in 40..=53 {
        out.insert((66, z));
    }
    for x in 19..=29 {
        for z in 77..=80 {
            out.insert((x, z));
        }
    }
    for x in 66..=74 {
        for z in 38..=39 {
            out.insert((x, z));
        }
    }
    out
}

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

/// `0x27B` - the `suimon` water gate, which is the switch on **which chamber**
/// of `suimon` the `map01` crossing delivers you to.
///
/// `map01` P2[18] is a two-armed `0x3F`: `SysFlag.Test 0x27B` at bytecode
/// `+0x0E` jumps to `+0x2D` when the flag **is set** (the field VM's `0x7_`
/// route takes the branch on set), and the two arms name different `suimon`
/// entry tiles - `(68, 44)` on the clear arm, `(21, 84)` on the set one.
///
/// They are not interchangeable. `(68, 44)` is the **northern** chamber, and
/// flooding `suimon`'s grid from it with its own trigger tiles honoured
/// reaches 6,425 sub-cells and **none** of record `2`'s twenty southern-door
/// tiles - its only exits are records `0`/`1`, which return to `map01` tile
/// `(54, 61)` on the component the player came from. From `(21, 84)` the same
/// flood reaches over a million sub-cells and all twenty. So with `0x27B`
/// clear the crossing is a dead end by design: `suimon` is a sluice-gate
/// puzzle, and its own scene-entry script `P1[0]` is what sets the flag.
///
/// Seeding it is the same move [`GATE_OPEN_FLAGS`] and [`MIST_WALL_FLAG`]
/// make, for the same reason - this ladder scores locomotion, collision and
/// the pad remap, not story progression. See [`SUIMON_SOUTH_EXIT`].
const WATER_GATE_FLAG: u16 = 0x27B;

/// The healing item the ladder's fighter carries: `0x77`, **Healing Leaf**.
///
/// Retail's item table gives `0x77` a single-target HP restore
/// (`docs/formats/item-effect-table.md`; the literal amount is overlay-resident
/// and the engine's `ItemCatalog::vanilla` carries the disc description's own
/// figure of 200, which is a full heal on a level-1 record).
const BAG_HEAL_ITEM: u8 = 0x77;

/// How many Healing Leaves the ladder leaves Rim Elm with, and what they cost.
///
/// This is the one part of the fighting model that is *not* produced by a pad
/// press, so it is grounded rather than picked: a New Game grants exactly 500
/// gold (`NEW_GAME_STARTING_GOLD`, the literal in retail's data-init
/// `FUN_80034A6C`), and Rim Elm's Variety Shop sells the Healing Leaf at 100
/// (`legaia_gamedata`'s curated shop table, the `resolve_rim_elm_shop`
/// fixture). Five leaves is therefore the whole starting purse spent on
/// healing before stepping out of the gate - the most a player *could* carry
/// out of Rim Elm, not an amount invented to make the leg survivable.
///
/// The gold is deducted, so this is a purchase and not a gift; a run that
/// later reaches a shop has the same money a player would.
///
/// Why it is seeded rather than bought by pad: the shop is a field-VM
/// dialogue on a Rim Elm NPC (`docs/subsystems/shop.md`), so buying it would
/// add an interact-and-menu leg to a ladder that scores locomotion. It stands
/// in for the town's beats exactly as [`GATE_OPEN_FLAGS`] stands in for its
/// story flags, and it is called out in the report for the same reason.
const BAG_HEAL_COUNT: u8 = 5;
/// Rim Elm Variety Shop price of one [`BAG_HEAL_ITEM`].
const BAG_HEAL_PRICE: i32 = 100;

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

/// Manhattan tile distance a dungeon leg must **walk** before a transition
/// counts as having traversed the dungeon.
///
/// The Ravine is a corridor with mouths on both sides of the continent, so
/// every one of its ends is a `map01` door and `Transitioned("map01")` alone
/// cannot tell "crossed the Ravine" from "turned round and walked back out the
/// way I came in". The same shape [`OVERWORLD_CROSSING_TILES`] guards on rung
/// 3, and the same fix: score the distance, not just the event.
///
/// The first draft of rung 5 applied it to the **goal** instead, which reads
/// the same and measures nothing: it aimed 47 tiles away, walked two, tripped
/// the entrance band and reported a clean `Transitioned("map01")`.
///
/// This threshold is the *second* of the rung's two conditions, not the whole
/// of it. The first is which door the leg came out of - see [`ExitRecord`] -
/// and that one is a disc coordinate rather than a heuristic. 12 tiles is
/// under the corridor's own length and well past the entry chamber, so
/// together they cannot be cleared by a step back through the entrance.
const DUNGEON_TRAVERSE_TILES: i32 = 12;

/// How many tiles of one exit band the rung-5 goal picker scores with a full
/// flood. A band is many adjacent tiles, so an unbounded list costs dozens of
/// BFS passes to rank rows that are the same door.
const MAX_EXIT_CANDIDATES: usize = 12;

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
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    // A cold boot has a party: seed the retail New Game roster (Vahn, the
    // SCUS `0x80078C4C` template) exactly as `BootSession::begin_new_game`
    // does. Without it every party slot is hollow (`max_hp == 0`, no
    // record) - the port-only unseeded state whose encounters can neither be
    // fought nor survived, which made rung 3's "one encounter fought and
    // survived" unmeasurable (and, before the wipe hold existed, silently
    // scored a total party kill as `Fight::Survived`).
    let scus = std::fs::read(extracted.join("SCUS_942.54")).expect("read SCUS_942.54");
    let party =
        legaia_asset::new_game::StartingParty::from_scus(&scus).expect("SCUS new-game template");
    host.world.seed_starting_party(&party);
    Some(host)
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
                let (nt, ct) = (dispatch_tile(nx, nz), dispatch_tile(cx, cz));
                // A hazard tile is dangerous to **enter**, and a step that
                // stays inside one tile is not an entry: retail's dispatcher
                // fires on a tile *change*
                // (`SceneHost::dispatch_walk_on_trigger` compares against the
                // previous frame's tile), so the band under the player's own
                // feet has already fired and cannot fire again while it is
                // occupied. Testing the destination alone conflates "occupy"
                // with "enter", and that is a **self-block** whenever the walk
                // starts inside an avoided tile - which a dungeon arrival
                // always does, because the player is seated on the door they
                // came in through. See [`ExitRecord`].
                if nt != ct && avoid.contains(&nt) {
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
    /// held input for the whole budget. Carries the world position it
    /// happened at, because a scripted sequence that never ends is a
    /// **record**, and the tile is what names which one - a lock at the leg's
    /// first frame and a lock seven tiles in are different defects.
    ///
    /// The world pair, not the tile: the trigger that spawned the sequence was
    /// matched in the dispatch frame (`world >> 7`) and the two frames differ
    /// by a half tile, so a lock reported only in planner tiles names the tile
    /// *before* the band it is standing in. This one did - `(56, 29)` for a
    /// record whose band starts at `(56, 30)`.
    InputLocked { at: (i16, i16) },
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
    /// A random encounter killed the whole party. This is a leg outcome of
    /// its own because it must never be conflated with either neighbour: it
    /// is not `BattleUnresolved` (the battle resolved, against us) and it
    /// must never score as survival - the pre-hold engine returned a wiped
    /// party to the field and this ladder walked three rungs with dead
    /// heroes.
    PartyWiped { heading_for: (i16, i16) },
}

/// Rendered into the printed table. Written by hand rather than derived so
/// the failure shapes name their tile in the line a reader actually sees -
/// a stall's whole value is the coordinate it died on.
impl std::fmt::Display for Leg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Leg::Transitioned(name) => write!(f, "entered {name}"),
            Leg::Reached => write!(f, "reached the goal tile"),
            Leg::InputLocked { at } => write!(
                f,
                "control never released at world {at:?} tile {:?} (dispatch tile {:?})",
                tile_of(at.0, at.1),
                dispatch_tile(at.0, at.1)
            ),
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
            Leg::PartyWiped { heading_for } => write!(
                f,
                "PARTY WIPED in a random encounter (game-over hold) heading for {heading_for:?}"
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
    for f in 0..INPUT_RELEASE_BUDGET {
        if !host.world.cutscene_timeline_active() && !host.world.dialogue_owns_input() {
            return Drain::Released;
        }
        // Page through it the way a player does. A neutral pad is enough for a
        // sequence that runs on its own clock (`town01`'s opening, the arrival
        // beats), but it is **not** enough for one that pages: `keikoku`'s
        // partition-2 record `7` - the shared band across all four chambers'
        // inner doorways, guarded by the one-shot `SysFlag 0x2BB` - is a story
        // cutscene with narration pages, and a page waits on a confirm. Held
        // neutral it never advances and the whole leg reads as an engine hang
        // at the tile the band starts on.
        //
        // Pulsed, not held: the advance is edge-triggered, so a held Cross
        // reads as one press and pages once. The duty cycle is
        // press-2-release-14, which is a human mash rather than a per-frame
        // strobe - fast enough to clear a long sequence inside the budget,
        // slow enough that a page that opens an option picker is not
        // double-answered by the same press.
        host.world.set_pad(if f % 16 < 2 {
            PadButton::Cross.mask()
        } else {
            0
        });
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
    /// The battle handed the world to some third mode.
    Diverted(SceneMode),
    /// The party was killed. A wipe no longer leaves Battle mode on its own:
    /// the game-over hold (`World::game_over_hold`) freezes the scene in
    /// Battle until a host's GameOverSession resolves it, so it must be read
    /// off the hold flags, not off a mode change - and it must be scored as
    /// a wipe, never as `Survived` (the pre-hold engine returned the wiped
    /// party to the field and the ladder kept walking with dead heroes).
    Wiped,
}

/// The ladder's fighter, expressed as *the pad mask it presses this frame*.
///
/// There is no engine call anywhere in this type. Every decision it makes -
/// pick Begin, pick Attack, pick Auto, confirm the target, open the bag, walk
/// to the Healing Leaf row, confirm it on the leader - comes out as a d-pad or
/// face-button mask that [`drain_battle`] hands to `World::set_pad`, which is
/// the same call the walking legs make. That is deliberate: the whole claim
/// this ladder makes is "a person pressing buttons gets this far", and an
/// engine API called mid-battle would quietly weaken it to "a person pressing
/// buttons, plus a robot with debug access, gets this far".
///
/// ## What it plays
///
/// The command ring is retail's four-arm diamond, selected by **direction**,
/// committing on the press (`FUN_801D0748` state `0x28`; see
/// `crate::battle_input`). So:
///
/// | phase | press | chip |
/// |---|---|---|
/// | `RoundPrompt` (`0x1E`) | Left | `Begin` |
/// | `Menu` (`0x28`) | Left / Up | `Attack` / `Item` |
/// | `AttackMode` (`0x78`) | Left | `Auto` |
/// | `Targeting` | Cross | the cursor's enemy |
///
/// `Item` is taken only when the acting party is hurt past
/// [`Self::heal_below_pct`] **and** the bag still holds the heal id - a policy
/// that opens an empty bag has to back out again, which costs a turn and reads
/// as a hang if the back-out is ever missed.
///
/// A queued battle message box (the formation banner, a tutorial page) parks
/// the whole battle tick ahead of the command session
/// (`World::tick_battle_tutorial_boxes`), so a box gets Cross and nothing
/// else - pressing a ring direction into a box is how a run stalls with the
/// menu apparently open and no input arriving.
struct FightPolicy {
    /// `false` restores the pre-existing neutral-pad model: no presses at all,
    /// and the live loop auto-commits `arm_party_physical` for every party
    /// turn. Kept as the ladder's own **contrast control** - it is the only
    /// way to show that a change in the score came from the fighter rather
    /// than from the route. Set by `LEGAIA_CPR_NEUTRAL_FIGHT=1`.
    driven: bool,
    /// `false` leaves Rim Elm with an **empty bag** while still driving the
    /// command ring by pad. The ladder's second contrast control, and the one
    /// that answers the question the wipe was hiding: `Attack -> Auto` seeds
    /// the same two-swing queue `arm_party_physical` does
    /// (`seed_basic_attack_queue`, retail `FUN_801EED1C`'s no-input arm), so
    /// command selection alone changes *nothing* about the damage traded -
    /// with this off, a driven run and a neutral run should die in the same
    /// place. Set by `LEGAIA_CPR_NO_BAG=1`.
    bag: bool,
    /// HP percentage at or below which the fighter reaches for the bag
    /// instead of swinging.
    heal_below_pct: u32,
    /// Item id the fighter heals with ([`BAG_HEAL_ITEM`]).
    heal_item: u8,
}

impl Default for FightPolicy {
    fn default() -> Self {
        Self {
            driven: std::env::var_os("LEGAIA_CPR_NEUTRAL_FIGHT").is_none(),
            bag: std::env::var_os("LEGAIA_CPR_NO_BAG").is_none(),
            heal_below_pct: 50,
            heal_item: BAG_HEAL_ITEM,
        }
    }
}

impl FightPolicy {
    /// The pad mask for this frame. `0` means "press nothing", which is what
    /// the neutral model does for the whole battle.
    fn pad_for(&self, host: &SceneHost) -> u16 {
        use legaia_engine_core::battle_input::CommandPhase;
        use legaia_engine_core::inventory_use::InventoryUseState;

        if !self.driven {
            return 0;
        }
        let w = &host.world;
        // A message box owns the frame ahead of everything else.
        if !w.battle_tutorial_boxes.is_empty() {
            return PadButton::Cross.mask();
        }
        if let Some(menu) = w.battle_item_menu.as_ref() {
            return match menu.state {
                InventoryUseState::Browsing { .. } => {
                    if menu.filtered_items.is_empty() {
                        // Nothing usable in here - back out and swing instead.
                        PadButton::Circle.mask()
                    } else if menu.current_item().map(|e| e.id) == Some(self.heal_item) {
                        PadButton::Cross.mask()
                    } else {
                        // Walk the list to the heal row, one press at a time.
                        PadButton::Down.mask()
                    }
                }
                // The target strip opens on the party leader, which is who
                // the heal is for in a solo party.
                InventoryUseState::TargetSelect { .. } => PadButton::Cross.mask(),
                _ => 0,
            };
        }
        if let Some(session) = w.battle_command.as_ref() {
            return match &session.phase {
                CommandPhase::RoundPrompt { .. } => PadButton::Left.mask(), // Begin
                CommandPhase::Menu { .. } => {
                    if self.wants_heal(host) {
                        PadButton::Up.mask() // Item
                    } else {
                        PadButton::Left.mask() // Attack
                    }
                }
                CommandPhase::AttackMode { .. } => PadButton::Left.mask(), // Auto
                CommandPhase::Targeting { .. } => PadButton::Cross.mask(),
                _ => 0,
            };
        }
        0
    }

    /// Is any living party member hurt past the threshold, with a heal left
    /// in the bag to spend on them?
    fn wants_heal(&self, host: &SceneHost) -> bool {
        let w = &host.world;
        if w.inventory.get(&self.heal_item).copied().unwrap_or(0) == 0 {
            return false;
        }
        (0..w.party_count.clamp(1, 3) as usize).any(|i| {
            let a = &w.actors[i].battle;
            a.max_hp > 0
                && a.hp > 0
                && u32::from(a.hp) * 100 <= u32::from(a.max_hp) * self.heal_below_pct
        })
    }
}

/// Fight an encounter **through the battle command UI**, the way a player
/// does, and report how it ended.
///
/// **Sitting the encounter out is not an optional nicety, it is the difference
/// between a leg that measures locomotion and a leg that measures nothing.**
/// `map01` has a live region-keyed encounter table
/// (`World::set_world_map_regions`), so a pad-driven crossing enters
/// [`SceneMode::Battle`] every few hundred ticks. While that mode is up the
/// player actor's `move_state` is the **battle arena** transform, not an
/// overworld position, so a follower that keeps sampling [`player_world`]
/// reads a coordinate from a different space entirely - `(0, -825)` on the
/// first `map01` encounter, which is off the north-west corner of a map the
/// player is standing in the middle of. Every downstream number then lies in
/// the same direction at once: the distance check sees no progress and trips
/// the stall detector, and the stall site reports a tile the player was never
/// on with all four walls set (an out-of-grid probe reads as solid). Read as
/// locomotion it looks exactly like an inverted movement axis; it is a mode
/// confusion.
///
/// ## Why the pad presses, and why they are still pad presses
///
/// The first version of this held the pad neutral for the whole battle. That
/// is not "no fighting model", it is a specific one: with
/// [`World::battle_player_driven`] off the live loop auto-commits
/// `arm_party_physical` for every party turn - a two-swing basic attack at the
/// first living monster, retail's own AI-party queue
/// (`FUN_801EED1C`'s `(&DAT_8007BD10)[slot] == 4` arm) - and nothing else. No
/// command choice, no items, no Spirit, no Run. A leg that dies under it
/// cannot distinguish "the port cannot walk this route" from "the ladder's
/// fighter is incompetent", which is exactly what the wipe on the
/// `map01 -> suimon` leg had been reporting.
///
/// So the fighter drives [`World::battle_command`] - the same
/// [`crate::battle_input::BattleCommandSession`] the windowed host binds its
/// keyboard to - with **pad presses only** ([`FightPolicy::pad_for`]). No
/// engine API is called to pick a command, choose a target or use an item;
/// every one of those is a `set_pad` + `tick` pair, edge-triggered exactly as
/// a human press is. That keeps the rung's claim intact: what the ladder
/// scores is still reachable by a person holding a controller.
///
/// Battle ticks are charged to [`BATTLE_FRAME_BUDGET`], never to the leg's
/// frame or stall budget - fighting is not stalling.
fn drain_battle(host: &mut SceneHost, resume: SceneMode, policy: &FightPolicy) -> Fight {
    let report = std::env::var_os("LEGAIA_CPR_FIGHT").is_some();
    let opened = fight_snapshot(host);
    let mut ticks = 0u32;
    let mut pad_prev = 0u16;
    let mut outcome = Fight::Stuck;
    for _ in 0..BATTLE_FRAME_BUDGET {
        // A press is an *edge*: the session reads `just_pressed`, so a held
        // mask pages exactly once and then does nothing. Alternate every
        // frame between the wanted mask and neutral rather than holding it,
        // which is the same duty cycle `drain_scripted` uses on Cross.
        let want = policy.pad_for(host);
        let pad = if pad_prev == 0 { want } else { 0 };
        pad_prev = pad;
        host.world.set_pad(pad);
        if host.tick().is_err() {
            outcome = Fight::Stuck;
            break;
        }
        ticks += 1;
        if host.world.game_over_hold || host.world.game_over {
            outcome = Fight::Wiped;
            break;
        }
        if host.world.mode == resume {
            outcome = Fight::Survived;
            break;
        }
        if host.world.mode != SceneMode::Battle {
            outcome = Fight::Diverted(host.world.mode);
            break;
        }
    }
    if report {
        let closed = fight_snapshot(host);
        eprintln!(
            "[fight] {opened} -> {closed} in {ticks} ticks: {}",
            match &outcome {
                Fight::Survived => "survived".to_string(),
                Fight::Wiped => "WIPED".to_string(),
                Fight::Stuck => "stuck".to_string(),
                Fight::Diverted(m) => format!("diverted to {m:?}"),
            }
        );
    }
    outcome
}

/// One line of battle state: the formation, the party's HP, the leader's
/// level, and the bag. Printed at the open and close of every encounter under
/// `LEGAIA_CPR_FIGHT=1`.
///
/// Deliberately reads the *live* mirrors rather than the roster record: the
/// battle actor's `hp`/`max_hp` is what the wipe scan and the damage fold both
/// consult, and a report keyed on the record would be a frame behind them.
fn fight_snapshot(host: &SceneHost) -> String {
    let w = &host.world;
    let party: Vec<String> = (0..w.party_count.clamp(1, 3) as usize)
        .map(|i| {
            let a = &w.actors[i].battle;
            format!("{}/{}", a.hp, a.max_hp)
        })
        .collect();
    let monsters: Vec<String> = (w.party_count.clamp(1, 3) as usize..w.actors.len())
        .filter(|&i| w.actors[i].battle.max_hp > 0)
        .map(|i| {
            let a = &w.actors[i].battle;
            format!("{}/{}", a.hp, a.max_hp)
        })
        .collect();
    let formation = w
        .active_formation
        .as_ref()
        .map(|f| {
            let ids: Vec<String> = f.slots.iter().map(|s| s.monster_id.to_string()).collect();
            format!("F{}[{}]", f.formation_id, ids.join(","))
        })
        .unwrap_or_else(|| "F-".to_string());
    let lvl = w.roster.members.first().map_or(0, |r| r.level());
    let bag: usize = w.inventory.values().map(|&n| n as usize).sum();
    format!(
        "{formation} party[{}] mob[{}] Lv{lvl} bag{bag}",
        party.join(" "),
        monsters.join(" ")
    )
}

/// Tick with a neutral pad until the world hands control back, so a leg does
/// not spend its budget shoving against a cutscene lock.
fn wait_for_input(host: &mut SceneHost) -> bool {
    held_frames_before_input(host).is_some()
}

/// [`wait_for_input`] with the answer nobody was reading: **how many frames
/// anything actually held input**, or `None` if it never let go.
///
/// `Some(0)` is not "control was released promptly", it is "no scripted
/// sequence ever ran". The two are indistinguishable to a caller that only
/// asks whether input is free now, and rung 1 asked exactly that - so a scene
/// whose opening choreography does not play at all scored the rung labelled
/// "cold boot hands control to the player". `town01` under
/// `SceneHost::enter_field_scene(name, 0)` returns `Some(0)`: no timeline
/// frame, no dialogue frame, not during the wait and not across a further
/// 1200 idle ticks. So rung 1 is evidence that the scene loads into a
/// controllable Field state, and is **not** evidence that the opening plays.
/// The count is printed in the rung's own row so the distinction is visible
/// rather than inferred.
fn held_frames_before_input(host: &mut SceneHost) -> Option<u32> {
    for f in 0..INPUT_RELEASE_BUDGET {
        if !host.world.cutscene_timeline_active() && !host.world.dialogue_owns_input() {
            return Some(f);
        }
        host.world.set_pad(0);
        host.tick().ok()?;
    }
    None
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
    fight: &FightPolicy,
) -> Leg {
    if !wait_for_input(host) {
        return Leg::InputLocked {
            at: player_world(host),
        };
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
            match drain_battle(host, walking_mode, fight) {
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
                Fight::Wiped => {
                    return Leg::PartyWiped { heading_for: goal };
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
                Drain::Locked => {
                    let (x, z) = player_world(host);
                    if std::env::var_os("LEGAIA_CPR_DEBUG").is_some() {
                        eprintln!(
                            "[dbg] input lock at world ({x},{z}) tile {:?} dispatch {:?}: \
                             timeline {} dialog {} inline {}",
                            tile_of(x, z),
                            dispatch_tile(x, z),
                            host.world.cutscene_timeline_active(),
                            host.world.current_dialog.is_some(),
                            host.world.inline_dialogue.is_some(),
                        );
                    }
                    return Leg::InputLocked { at: (x, z) };
                }
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

/// One of the current field scene's **scene-change records**: a partition-2
/// record whose script carries a `0x3F`, together with every `.MAP` gate-1
/// trigger tile that spawns it and the arrival tile it lands on.
///
/// A field scene has no entity portals - the overworld's [`portal_tile`] has
/// nothing to read here. Its doors are `.MAP` kind-1 gate-1 trigger bands
/// joined to their partition-2 records, which is exactly the join
/// `man_field_scripts::overworld_portal_sites` performs for the overworld, so
/// this asks the disc where the scene's doors are rather than probing tiles.
struct ExitRecord {
    /// Partition-2 record index.
    record: u8,
    /// Destination CDNAME scene label from the record's `0x3F`.
    dest: String,
    /// Arrival tile at the destination - the coordinate that tells two doors
    /// of the same scene apart, since every `keikoku` door returns to `map01`.
    dest_tile: (i16, i16),
    /// Every trigger tile that spawns this record.
    tiles: Vec<(i16, i16)>,
}

impl ExitRecord {
    /// Manhattan tile distance from `t` to the nearest tile of this band.
    fn near(&self, t: (i16, i16)) -> i32 {
        self.tiles
            .iter()
            .map(|&(tx, tz)| ((tx - t.0).abs() + (tz - t.1).abs()) as i32)
            .min()
            .unwrap_or(i32::MAX)
    }

    /// This band as a [`plan_path`] `avoid` set.
    fn hazard(&self) -> HashSet<(i32, i32)> {
        self.tiles
            .iter()
            .map(|&(tx, tz)| (i32::from(tx), i32::from(tz)))
            .collect()
    }
}

/// Every scene-change record of the loaded field scene, grouped by record.
///
/// Grouping by **record** rather than by tile is what makes a dungeon leg
/// scoreable. `keikoku` is one connected canyon with four such records - 0, 1,
/// 2, 3, returning to `map01` tiles `(52, 94)`, `(64, 67)`, `(77, 68)` and
/// `(82, 83)` - plus 31 further gate-1 tiles that are story *beats* and change
/// no scene at all. A tile-keyed view cannot tell those three populations
/// apart, so a leg aimed at "the nearest trigger tile 12+ away" routinely aims
/// at a beat band while treating the corridor onward as an obstacle.
fn exit_records(host: &SceneHost) -> Vec<ExitRecord> {
    let Some(scene) = host.scene.as_ref() else {
        return Vec::new();
    };
    let Ok(Some(man)) = scene.field_man_payload(&host.index) else {
        return Vec::new();
    };
    let Ok(mf) = legaia_asset::man_section::parse(&man) else {
        return Vec::new();
    };
    let Ok((primary, fallback)) = scene.field_tile_triggers(&host.index) else {
        return Vec::new();
    };
    let mut triggers = primary;
    triggers.extend(fallback);
    let mut by_record: HashMap<u8, ExitRecord> = HashMap::new();
    for site in legaia_engine_core::man_field_scripts::overworld_portal_sites(&mf, &man, &triggers)
    {
        by_record
            .entry(site.record)
            .or_insert_with(|| ExitRecord {
                record: site.record,
                dest: site.scene_name.clone(),
                dest_tile: (i16::from(site.entry_x), i16::from(site.entry_z)),
                tiles: Vec::new(),
            })
            .tiles
            .push((i16::from(site.overworld_x), i16::from(site.overworld_z)));
    }
    let mut out: Vec<ExitRecord> = by_record.into_values().collect();
    out.sort_by_key(|r| r.record);
    out
}

/// Ablation over the four inputs that can seal the overworld route, printed
/// under `LEGAIA_CPR_ABLATE=1`.
///
/// Rung 4 stalls with the flood unable to reach any `keikoku` mouth, and
/// `plan_path` rejects a step for four independent reasons: the walkability
/// grid ([`World::field_dir_blocked`]), placed-prop boxes and live NPC boxes
/// (both inside [`World::field_actor_dir_blocked`]), and the caller's portal
/// hazard set. A residual measured with all four live cannot say which one is
/// the seal, and a table that reports "walls only" is wrong: `plan_path` in
/// fact consults every one of them - so the ablation is run rather than
/// reasoned about.
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

    // Where does the *directional* probe disagree with the *tile* read?
    // Retail's own grid for this scene is byte-identical to the port's decode
    // (read offline from a `map01` save state), and retail parks the player
    // inside the x=64 corridor this flood never reaches - so the seal is not
    // the data. Every frontier cell whose neighbour is an open tile the flood
    // refused to enter is a place `field_dir_blocked` is stricter than the
    // walls it is derived from.
    let mut disagreements: Vec<(Cell, usize)> = Vec::new();
    for &cur in &reached {
        let (cx, cz) = cell_center(cur);
        for ((dx, dz), dir) in STEPS {
            let next = (cur.0 + dx, cur.1 + dz);
            if next.0 < 0 || next.1 < 0 || reached.contains(&next) {
                continue;
            }
            let (nx, nz) = cell_center(next);
            if host.world.field_tile_is_wall(nx, nz) {
                continue; // genuinely walled - not a disagreement
            }
            if host.world.field_actor_dir_blocked(cx, cz, dir) {
                continue; // a prop/NPC, already ablated above
            }
            disagreements.push((cur, dir));
        }
    }
    eprintln!(
        "[ablate] probe-vs-tile disagreements on the flood frontier: {}",
        disagreements.len()
    );
    if let (Some(lo_x), Some(hi_x), Some(lo_z), Some(hi_z)) = (
        disagreements.iter().map(|(c, _)| c.0).min(),
        disagreements.iter().map(|(c, _)| c.0).max(),
        disagreements.iter().map(|(c, _)| c.1).min(),
        disagreements.iter().map(|(c, _)| c.1).max(),
    ) {
        eprintln!(
            "[ablate]   spread: cells x {lo_x}..{hi_x} z {lo_z}..{hi_z} \
             = tiles ({},{})..({},{})",
            lo_x / 4,
            lo_z / 4,
            hi_x / 4,
            hi_z / 4
        );
        for (c, dir) in disagreements.iter().take(8) {
            let (wx, wz) = cell_center(*c);
            eprintln!(
                "[ablate]   at cell {c:?} tile ({},{}) world ({wx},{wz}) dir {dir}",
                c.0 / 4,
                c.1 / 4
            );
        }
    }

    // Is the probe *asymmetric*? It samples points ahead of the source, so
    // `A -> B` can block while `B -> A` is clear, and a BFS from the arrival
    // then cannot enter a region a BFS from the destination would leave. Flood
    // backward from the tile retail actually parks the player on
    // (`map01` save state, world (8266, 8700) = tile (64, 67)) and see whether
    // it reaches the arrival the forward flood started from.
    let retail_cell = cell_of(8266, 8700);
    let back: HashSet<Cell> = {
        let mut seen: HashSet<Cell> = HashSet::new();
        let mut queue = VecDeque::new();
        seen.insert(retail_cell);
        queue.push_back(retail_cell);
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
    eprintln!(
        "[ablate] backward flood from retail's tile (64,67): {} sub-cells; \
         reaches the arrival cell {:?}: {}; forward flood reached retail's cell: {}",
        back.len(),
        from,
        back.contains(&from),
        reached.contains(&retail_cell)
    );

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

/// Lattice-pitch sweep + portal-reachability report for the rung-4 flood,
/// printed under `LEGAIA_CPR_LATTICE=1`.
///
/// [`ablate_rung4_inputs`] eliminates the flood's *inputs* (grid, props, NPCs,
/// hazards) one at a time; this eliminates the flood's *discretisation*, then
/// says which overworld portals the surviving component actually contains.
///
/// **Pitch.** The planner walks a [`SUBCELL`]-pitch lattice while retail's
/// locomotion is continuous in 2-unit sub-steps - the world-map-walk copy of
/// `FUN_801d01b0` increments the actor's `+0x14`/`+0x18` by 2 and re-runs
/// `FUN_801cfe4c` each time (`ghidra/scripts/funcs/overlay_world_map_walk_801d01b0.txt`).
/// A lattice can only ever *under*-report reachability against that, because
/// [`World::field_dir_blocked`]'s three probe points spread +/-16 units
/// laterally and which sub-cells that footprint straddles depends on the
/// position's phase modulo 64. Pitch 2 is therefore the ceiling: a node set
/// closed under 2-unit cardinal steps is exactly the set of positions the
/// retail stepper can reach, and nothing coarser can beat it.
///
/// The measured answer is that the pitch buys nothing - the reachable area is
/// flat from 32 down to 2 and never contains a `keikoku` mouth - so the seal
/// is not the lattice. What the portal listing then shows is that the
/// component *does* contain `suimon`, which is the scene that crosses to the
/// component the `keikoku` mouths are on. See
/// [`docs/subsystems/world-map.md`].
///
/// Walls only: props / NPCs are already falsified by the ablation, and the
/// per-node actor test is a linear scan the fine pitches cannot afford.
fn probe_rung4_lattice(host: &SceneHost) {
    /// World extent of the collision grid: 128 tiles x 128 units.
    const SPAN: i32 = 128 * 128;
    /// Pitches to sweep, coarsest first; the last is the retail step unit.
    const PITCHES: [i32; 5] = [32, 16, 8, 4, 2];
    /// A `map01` save state parks retail's own player here, just outside the
    /// `keikoku` entrance - a ground-truth standable point on the far side.
    const RETAIL_STAND: (i16, i16) = (8266, 8700);

    let (px, pz) = player_world(host);
    let portals: Vec<(String, (i16, i16))> = host
        .world
        .world_map_entity_configs
        .iter()
        .zip(host.world.world_map_entity_positions.iter())
        .filter_map(|(cfg, &(x, z))| match cfg {
            WorldMapEntityConfig::OverworldPortal { scene_name, .. } => {
                Some((scene_name.clone(), (x, z)))
            }
            _ => None,
        })
        .collect();
    eprintln!(
        "[lattice] arrival world ({px},{pz}); retail stand {RETAIL_STAND:?}; \
         {} overworld portals",
        portals.len()
    );

    for pitch in PITCHES {
        let stride = (SPAN / pitch) as usize;
        let snap = |w: i16| -> i32 { (i32::from(w) + pitch / 2).div_euclid(pitch) };
        let inside = |nx: i32, nz: i32| -> bool {
            nx >= 0 && nz >= 0 && (nx as usize) < stride && (nz as usize) < stride
        };
        let at = |nx: i32, nz: i32| -> usize { nz as usize * stride + nx as usize };

        let (sx, sz) = (snap(px), snap(pz));
        if !inside(sx, sz) {
            eprintln!("[lattice] pitch {pitch:>3}: the arrival is off the lattice");
            continue;
        }
        let mut seen = vec![false; stride * stride];
        let mut queue = VecDeque::new();
        seen[at(sx, sz)] = true;
        queue.push_back((sx, sz));
        let mut nodes = 0u64;
        while let Some((cx, cz)) = queue.pop_front() {
            nodes += 1;
            let (wx, wz) = ((cx * pitch) as i16, (cz * pitch) as i16);
            for ((dx, dz), dir) in STEPS {
                let (nx, nz) = (cx + i32::from(dx), cz + i32::from(dz));
                if !inside(nx, nz) || seen[at(nx, nz)] {
                    continue;
                }
                if host.world.field_dir_blocked(wx, wz, dir) {
                    continue;
                }
                seen[at(nx, nz)] = true;
                queue.push_back((nx, nz));
            }
        }

        // Reached "near" = within 128 world units, so a portal tile that is
        // itself a wall (a door reads as one) still reports as arrived-at.
        let near = |wx: i16, wz: i16| -> bool {
            let (gx, gz) = (snap(wx), snap(wz));
            let span = 128 / pitch;
            (-span..=span).any(|dz| {
                (-span..=span).any(|dx| inside(gx + dx, gz + dz) && seen[at(gx + dx, gz + dz)])
            })
        };
        // Node count as tile-equivalents, so the pitches compare directly.
        let tiles = nodes * (pitch as u64) * (pitch as u64) / (128 * 128);
        let (rx, rz) = (snap(RETAIL_STAND.0), snap(RETAIL_STAND.1));
        eprintln!(
            "[lattice] pitch {pitch:>3}: {nodes:>9} nodes = {tiles:>5} tile-equivalents; \
             reaches retail's stand: {}",
            inside(rx, rz) && seen[at(rx, rz)]
        );
        if pitch == *PITCHES.last().expect("non-empty") {
            for (scene, (wx, wz)) in &portals {
                eprintln!(
                    "[lattice]   portal -> {scene:<10} world ({wx:>5},{wz:>5}) \
                     tile {:?}  reached: {}",
                    tile_of(*wx, *wz),
                    near(*wx, *wz)
                );
            }
        }
    }
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
    let fight = FightPolicy::default();

    // The fighting model. Without this the live loop auto-commits a two-swing
    // basic attack for every party turn and nothing else - see
    // [`drain_battle`] - so the ladder's fighter could not choose a command,
    // could not use an item, and could not run. `SceneHost::open_extracted`
    // is the bare scene host, not `BootSession`, so the catalogs the play
    // hosts install (`BootSession::enter_field_live`) have to be installed
    // here too: **an empty item catalog makes every bag row inadmissible**,
    // which is a silent way for a healing policy to do nothing at all.
    if fight.driven {
        host.world.battle_player_driven = true;
        // Arm the **field** side of the loop as well, which is what both play
        // hosts do (`BootSession::enter_field_live`) and what makes a dungeon
        // roll its own encounters. It is inert on this route today - see the
        // `keikoku encounter model` line below - and arming it anyway means
        // the day the region decode is fixed, rung 5 starts fighting in the
        // Ravine instead of quietly continuing not to. Set directly rather
        // than through `arm_live_loop`, whose `encounter.is_none()` arm would
        // install the *fabricated* vanilla formation table over the scene's
        // own.
        host.world.live_gameplay_loop = true;
        host.world
            .set_item_catalog(legaia_engine_core::items::ItemCatalog::vanilla());
        // Leave Rim Elm with the purse spent on healing. See [`BAG_HEAL_COUNT`]
        // for why five, and why this is the model's one non-pad input.
        if fight.bag {
            host.world.inventory.insert(BAG_HEAL_ITEM, BAG_HEAL_COUNT);
            host.world.money -= BAG_HEAL_PRICE * i32::from(BAG_HEAL_COUNT);
        }
    }

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
    // The overworld's own story gates. See `MIST_WALL_FLAG` (the Drake mist
    // walls) and `WATER_GATE_FLAG` (which `suimon` chamber the crossing lands
    // in - without it the crossing is a dead end by design).
    host.world.system_flag_set(MIST_WALL_FLAG);
    host.world.system_flag_set(WATER_GATE_FLAG);

    // --- 1. Cold boot into Rim Elm free-roam with control released. -------
    host.enter_field_scene("town01", 0).expect("enter town01");
    let field = host.world.mode == SceneMode::Field;
    let held = if field {
        held_frames_before_input(host)
    } else {
        None
    };
    let released = held.is_some();
    rungs.push(Rung {
        label: "town01 loads into free-roam Field",
        detail: if let Some(held) = held {
            let (x, z) = player_world(host);
            format!(
                "player at tile {:?}; the opening held input for {held} frame(s){}",
                tile_of(x, z),
                if held == 0 {
                    " - i.e. no opening choreography ran, so this rung says the scene \
                     loads, NOT that its opening plays"
                } else {
                    ""
                }
            )
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
    let leg = walk_to(
        host,
        goal,
        &HashSet::new(),
        &mut LegStats::default(),
        &fight,
    );
    let cleared = matches!(&leg, Leg::Transitioned(n) if n == "map01");
    rungs.push(Rung {
        label: "pad-walk town01 south gate -> map01",
        detail: format!("{leg}"),
        cleared,
    });
    if !cleared {
        return rungs;
    }

    // --- 3. Walk the overworld toward the Ravine. -------------------------
    //
    // Three legs, not one - see [`RAVINE_CROSSING`]. Leg A crosses the
    // northern component to the `suimon` door, leg B walks `suimon` to its
    // southern door, leg C crosses the southern component to a `keikoku`
    // mouth. Rung 3 is scored on leg A, which is the long overworld walk.
    let arrival = {
        let (x, z) = player_world(host);
        tile_of(x, z)
    };
    if std::env::var_os("LEGAIA_CPR_ABLATE").is_some() {
        let hazards = portal_hazards(host, "keikoku");
        ablate_rung4_inputs(host, &hazards);
    }
    if std::env::var_os("LEGAIA_CPR_LATTICE").is_some() {
        probe_rung4_lattice(host);
    }

    let a_hazards = portal_hazards(host, "suimon");
    let a_goal = portal_tile(host, "suimon", &a_hazards);
    let mut stats = LegStats::default();
    let leg_a = a_goal.map(|goal| walk_to(host, goal, &a_hazards, &mut stats, &fight));

    // --- 3. …and the crossing itself, which is leg A. ---------------------
    let crossed = stats.fought > 0 && stats.reach >= OVERWORLD_CROSSING_TILES;
    rungs.push(Rung {
        label: "pad-walk map01 across the continent",
        detail: match &leg_a {
            None => "no suimon overworld portal on map01".to_string(),
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

    // --- 4. Through `suimon` and on to a Ravine mouth. --------------------
    let mut trail = vec![format!("map01 -> suimon: {}", fmt_leg(leg_a.as_ref()))];
    let mut arrived = matches!(&leg_a, Some(Leg::Transitioned(n)) if n == "suimon");
    if arrived {
        if std::env::var_os("LEGAIA_CPR_DEBUG").is_some() {
            let (x, z) = player_world(host);
            eprintln!(
                "[dbg] suimon entry world ({x},{z}) tile {:?} dispatch {:?}",
                tile_of(x, z),
                dispatch_tile(x, z)
            );
        }
        let leg_b = walk_to(
            host,
            SUIMON_SOUTH_EXIT,
            &suimon_north_doors(),
            &mut LegStats::default(),
            &fight,
        );
        if std::env::var_os("LEGAIA_CPR_DEBUG").is_some() {
            let (x, z) = player_world(host);
            eprintln!(
                "[dbg] after leg B: {leg_b} at world ({x},{z}) dispatch {:?}",
                dispatch_tile(x, z)
            );
        }
        trail.push(format!("suimon -> map01: {leg_b}"));
        arrived = matches!(&leg_b, Leg::Transitioned(n) if n == "map01");
    }
    if arrived {
        let c_hazards = portal_hazards(host, "keikoku");
        let leg_c = portal_tile(host, "keikoku", &c_hazards)
            .map(|goal| walk_to(host, goal, &c_hazards, &mut LegStats::default(), &fight));
        trail.push(format!("map01 -> keikoku: {}", fmt_leg(leg_c.as_ref())));
        arrived = matches!(&leg_c, Some(Leg::Transitioned(n)) if n == "keikoku");
    }
    rungs.push(Rung {
        label: "pad-walk map01 -> suimon -> map01 -> keikoku (Ravine)",
        detail: trail.join(" | "),
        cleared: arrived,
    });
    if !arrived {
        return rungs;
    }

    // --- 5. Walk the Ravine itself, entrance to far exit. -----------------
    //
    // Every rung up to here ends the moment a door fires; none of them walks a
    // *dungeon interior*, and that is a different surface. A dungeon's exits
    // are `.MAP` walk-on bands rather than overworld entity portals, its
    // encounter table is the scene's own rather than the overworld region
    // set, and its corridors are the narrow geometry the leading-edge probe is
    // hardest on. Under the four-rung ladder "the Ravine loads" and "the Ravine
    // can be walked" were the same score.
    //
    // Success is leaving by a **different scene-change record** than the one
    // the player arrived through, after walking at least
    // [`DUNGEON_TRAVERSE_TILES`]. See [`ExitRecord`] for why the unit is a
    // record and not a tile, and [`DUNGEON_TRAVERSE_TILES`] for why a bare
    // `Transitioned` event is not enough on a scene whose every door returns
    // to `map01`.
    let entry_tile = {
        let (x, z) = player_world(host);
        tile_of(x, z)
    };
    // Why rung 5 fights nothing, printed where a reader will meet the claim.
    //
    // The rung's own header says its encounters "come from the scene's own
    // table rather than the overworld region set". They do not - not because
    // the loop is unarmed (it is, above) but because `keikoku`'s MAN encounter
    // section decodes to exactly ONE region: the whole map (`x 0..128`,
    // `z 0..128`) at `rate 0` with a one-row formation range. `any_rollable`
    // therefore answers `false` and no step ever rolls, while the same MAN
    // registers 37 formation rows that nothing can reach. A 128x128 AABB with
    // a zero rate does not read like authored dungeon data; the suspect is
    // `man_section::region_records` finding a header or terminator row rather
    // than the region array. Until that is settled, rung 5 measures dungeon
    // *locomotion* and its exit bands, and nothing about its encounters.
    if std::env::var_os("LEGAIA_CPR_FIGHT").is_some() {
        eprintln!(
            "[fight] keikoku encounter model: live_loop {} rollable {} region_tracker {} \
             session {} formations {:?}",
            host.world.live_gameplay_loop,
            host.world.scene_encounters_rollable,
            host.world.field_region_tracker.is_some(),
            host.world.encounter.is_some(),
            host.world.registered_formation_ids(),
        );
        if let Some(t) = host.world.field_region_tracker.as_ref() {
            for r in &t.table().regions {
                eprintln!(
                    "[fight]   region x{}..{} z{}..{} rate {} formations {}..+{}",
                    r.tile_x_min,
                    r.tile_x_max,
                    r.tile_z_min,
                    r.tile_z_max,
                    r.rate_increment,
                    r.formation_base,
                    r.formation_count
                );
            }
        }
    }
    let records = exit_records(host);
    // The door the player came in through is the record whose band the arrival
    // is standing on the approach to. On `keikoku` that is unambiguous by a
    // factor of twenty: the arriving chamber's own record is 2 tiles away and
    // the next-nearest is 41.
    let arrival_rec = records
        .iter()
        .min_by_key(|r| r.near(entry_tile))
        .map(|r| r.record);
    let hazards5 = arrival_rec
        .and_then(|rec| records.iter().find(|r| r.record == rec))
        .map(ExitRecord::hazard)
        .unwrap_or_default();
    // Rank the *other* records' band tiles by the residual `plan_path` leaves,
    // the same measure `portal_tile` picks an overworld door with - a band
    // across a wall is a band the follower cannot use, however close it looks.
    let from5 = {
        let (x, z) = player_world(host);
        cell_of(x, z)
    };
    // Non-vacuity control (`LEGAIA_CPR_RUNG5_BACKOUT=1`): aim the leg at the
    // arrival record's **own** band and hazard nothing, i.e. walk straight
    // back out the door the player came in through. That reproduces the exact
    // observable the first draft of this rung passed on - a clean
    // `Transitioned("map01")` a couple of tiles into the scene - so a run
    // under this flag must report the rung **unclear**. If it ever clears, the
    // rung is measuring the event again instead of the traverse.
    let backout = std::env::var_os("LEGAIA_CPR_RUNG5_BACKOUT").is_some();
    let hazards5 = if backout { HashSet::new() } else { hazards5 };
    let mut candidates: Vec<(u8, (i16, i16), i32)> = records
        .iter()
        .filter(|r| (Some(r.record) == arrival_rec) == backout)
        .filter_map(|r| {
            let mut tiles = r.tiles.clone();
            tiles.sort_by_key(|&(tx, tz)| {
                ((tx - entry_tile.0).abs() + (tz - entry_tile.1).abs()) as i32
            });
            tiles.truncate(MAX_EXIT_CANDIDATES);
            tiles
                .into_iter()
                .map(|t| {
                    let (wx, wz) = tile_center(t);
                    let goal_c = cell_of(wx, wz);
                    let residual = plan_path(host, from5, t, &hazards5)
                        .and_then(|p| p.last().copied())
                        .map_or(i32::MAX, |end| {
                            i32::from((end.0 - goal_c.0).abs() + (end.1 - goal_c.1).abs())
                        });
                    (r.record, t, residual)
                })
                .min_by_key(|&(_, _, residual)| residual)
        })
        .collect();
    candidates.sort_by_key(|&(_, _, residual)| residual);
    if std::env::var_os("LEGAIA_CPR_DEBUG").is_some() {
        for r in &records {
            eprintln!(
                "[dbg] keikoku exit record {} -> {} @ {:?}: {} band tile(s), nearest {} tiles",
                r.record,
                r.dest,
                r.dest_tile,
                r.tiles.len(),
                r.near(entry_tile)
            );
        }
        eprintln!(
            "[dbg] keikoku entry tile {entry_tile:?}, arrival record {arrival_rec:?}, \
             candidates {candidates:?}"
        );
    }
    let mut stats5 = LegStats::default();
    let leg_d = candidates
        .first()
        .map(|&(_, goal, _)| walk_to(host, goal, &hazards5, &mut stats5, &fight));
    // Where the transition put the player names **which door** it was, because
    // every `keikoku` record returns to `map01` at its own tile. That is the
    // check a step back through the entrance cannot pass, and it comes off the
    // disc rather than off a distance heuristic. The reach threshold is kept
    // alongside it: together they say "walked the canyon" rather than "a door
    // fired".
    let landed = matches!(&leg_d, Some(Leg::Transitioned(_))).then(|| {
        let (x, z) = player_world(host);
        tile_of(x, z)
    });
    let arrival_return = arrival_rec
        .and_then(|rec| records.iter().find(|r| r.record == rec))
        .map(|r| r.dest_tile);
    let traversed =
        left_by_another_door(landed, arrival_return) && stats5.reach >= DUNGEON_TRAVERSE_TILES;
    rungs.push(Rung {
        label: "pad-walk the Ravine out of a different door",
        detail: match (&leg_d, candidates.first()) {
            (None, _) => format!(
                "keikoku carries no scene-change record other than the {arrival_rec:?} the \
                 {entry_tile:?} arrival came through ({} record(s) found)",
                records.len()
            ),
            (Some(leg), Some(&(rec, goal, residual))) => format!(
                "from arrival {entry_tile:?} (record {arrival_rec:?}, returns to {arrival_return:?}) \
                 toward record {rec}'s band at {goal:?} (route residual {residual}): {leg} at \
                 {landed:?}; reached {} tiles (needs {DUNGEON_TRAVERSE_TILES}), \
                 {} random encounter(s) fought",
                stats5.reach, stats5.fought,
            ),
            (Some(leg), None) => format!("{leg}"),
        },
        cleared: traversed,
    });

    rungs
}

/// Did the dungeon leg come out somewhere other than where its entrance
/// record returns to?
///
/// `landed` is the tile the transition seated the player on, `arrival_return`
/// the tile the record the player came in through returns to. Both are `None`
/// when there was no transition / no identifiable entrance record, and a
/// missing half is **not** a pass: an unknown door is not a different one.
/// Split out so this can be exercised without a two-minute disc run - the
/// clause is the half of rung 5 that the `LEGAIA_CPR_RUNG5_BACKOUT` control
/// cannot isolate, because a backed-out leg also fails the reach threshold.
fn left_by_another_door(landed: Option<(i16, i16)>, arrival_return: Option<(i16, i16)>) -> bool {
    matches!((landed, arrival_return), (Some(l), Some(a)) if l != a)
}

/// Render an optional leg, so a missing goal reads as a leg outcome rather
/// than as an empty string.
fn fmt_leg(leg: Option<&Leg>) -> String {
    leg.map_or_else(|| "no reachable portal".to_string(), |l| format!("{l}"))
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

    /// Rung 5's door-identity clause. The four real `keikoku` return tiles
    /// are used as the fixture, because the shape that matters is "two doors
    /// of the same scene differ only in their arrival tile" - the case a
    /// scene-name comparison silently passes. Disc-free.
    #[test]
    fn a_door_is_only_different_when_both_ends_are_known() {
        // keikoku partition-2 records 0..3, in order.
        const RETURNS: [(i16, i16); 4] = [(52, 94), (64, 67), (77, 68), (82, 83)];
        let entrance = RETURNS[1]; // arrived through record 1
        for (i, &r) in RETURNS.iter().enumerate() {
            assert_eq!(
                left_by_another_door(Some(r), Some(entrance)),
                i != 1,
                "landing on record {i}'s return tile {r:?} after entering by record 1"
            );
        }
        // No transition at all is not a traverse, however far the walk went.
        assert!(!left_by_another_door(None, Some(entrance)));
        // Nor is a transition whose entrance record could not be identified:
        // an unknown door is not a different door.
        assert!(!left_by_another_door(Some(RETURNS[0]), None));
        assert!(!left_by_another_door(None, None));
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
