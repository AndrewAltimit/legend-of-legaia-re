//! The route's fixed facts and the walker's budgets: the tiles, gates and
//! story flags the chapter-1 path runs through, the bag the run is given, and
//! the frame / plan limits.

use super::*;

/// Field tile pitch in world units (128 per tile; see `field-locomotion.md`).
pub(super) const TILE: i16 = 128;

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
pub(super) const SUBCELL: i16 = 32;

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
pub(super) const TOWN01_SOUTH_GATE: (u8, u8) = (25, 46);

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
pub(super) const SUIMON_SOUTH_EXIT: (i16, i16) = (34, 85);

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
pub(super) fn suimon_north_doors() -> HashSet<(i32, i32)> {
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
pub(super) const GATE_OPEN_FLAGS: [u16; 2] = [327, 321];

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
pub(super) const MIST_WALL_FLAG: u16 = 0x482;

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
pub(super) const WATER_GATE_FLAG: u16 = 0x27B;

/// The healing item the ladder's fighter carries: `0x77`, **Healing Leaf**.
///
/// Retail's item table gives `0x77` a single-target HP restore
/// (`docs/formats/item-effect-table.md`; the literal amount is overlay-resident
/// and the engine's `ItemCatalog::vanilla` carries the disc description's own
/// figure of 200, which is a full heal on a level-1 record).
pub(super) const BAG_HEAL_ITEM: u8 = 0x77;

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
pub(super) const BAG_HEAL_COUNT: u8 = 5;
/// Rim Elm Variety Shop price of one [`BAG_HEAL_ITEM`].
pub(super) const BAG_HEAL_PRICE: i32 = 100;

/// Frames a leg may spend before it is called a timeout.
pub(super) const LEG_FRAME_BUDGET: u32 = 6_000;

/// Frames without closing distance on the goal before a leg is called stalled.
/// Generous enough to absorb a wall-slide detour around a building.
pub(super) const STALL_FRAMES: u32 = 240;

/// Frames to tick (pad neutral) waiting for the scene's opening choreography
/// to hand control back before a leg starts driving.
pub(super) const INPUT_RELEASE_BUDGET: u32 = 3_600;

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
pub(super) const OVERWORLD_CROSSING_TILES: i32 = 24;

/// Frames one random encounter may run before the leg gives up on it.
///
/// A leg on the overworld **will** be interrupted: `map01` carries a live
/// region-keyed encounter table and the walk crosses tile after tile, so a
/// pad-driven crossing fights several battles on the way. A battle is not
/// part of the leg's own frame or stall budget - see [`drain_battle`] - so
/// this is sized as a safety net (an observed `map01` encounter resolves in
/// well under a thousand ticks), not as a target.
pub(super) const BATTLE_FRAME_BUDGET: u32 = 20_000;

/// BFS ceiling. A field map is 256x256 tiles = 512x512 sub-cells, so this
/// admits a full-map flood with room to spare and still bounds a runaway.
pub(super) const MAX_PLAN_NODES: usize = 300_000;

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
pub(super) const DUNGEON_TRAVERSE_TILES: i32 = 12;

/// How many tiles of one exit band the rung-5 goal picker scores with a full
/// flood. A band is many adjacent tiles, so an unbounded list costs dozens of
/// BFS passes to rank rows that are the same door.
pub(super) const MAX_EXIT_CANDIDATES: usize = 12;
