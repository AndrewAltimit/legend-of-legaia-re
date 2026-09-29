# Full-game ladder

One ordered, game-denominated instrument for the question the other ladders
each answer only in part: **from a cold New Game, how far through the whole
game - to the ending credits - does the port get, and where does it stop.**

- Test: [`crates/engine-shell/tests/full_game_ladder.rs`](../../crates/engine-shell/tests/full_game_ladder.rs)
- Spine: [`scripts/replays/full_game_spine.toml`](../../scripts/replays/full_game_spine.toml)
- Baseline: [`scripts/replays/full_game_baseline.toml`](../../scripts/replays/full_game_baseline.toml)

## Contents

- [How it relates to the other ladders](#how-it-relates-to-the-other-ladders)
- [The spine](#the-spine)
- [Segments and tiers](#segments-and-tiers)
- [How to run](#how-to-run)
- [How to read a stall](#how-to-read-a-stall)
- [Seeding](#seeding)
- [What it cannot measure](#what-it-cannot-measure)

## How it relates to the other ladders

| Instrument | Question |
|---|---|
| [`critical_path_replay`](../../crates/engine-shell/tests/critical_path_replay.rs) | How far a pad run gets along the chapter-1 route, with no seating. |
| `chapter1_frontier_ladder` (engine-core) | Which chapter-1 scenes load, script, walk and leave - breadth. |
| chapter-2 / chapter-3 / boss spine oracles | Whether the pinned gates of one arc sequence through the engine. |
| **this ladder** | How far each stretch of the **whole** story gets from a real save there, and how far a cold New Game gets without help. |

The chapter ladders are deeper where they reach; this one is shallow and
wide. A stall here names where to point the deep instruments.

## The spine

The spine is the main-story milestone sequence, New Game (`opdeene`) to the
credits crawl (`edlast`). Each milestone names its scene and a **retail
anchor**: a save on one of the library's memory cards, by card file and save
name, or a catalogued save state, by `scripts/scenarios.toml` label and backup
fingerprint. The anchor's bytes are read at test time and never committed.

The ladder reads the anchor's **SC save window** - RAM `0x80084140` to
`0x80086140`, which is exactly the block a memory card stores
(`block_offset = ram - 0x80084140`; [`save-record.md`](../formats/save-record.md)).
A card save and a live state therefore yield the same thing: party records,
bag, gold, the system-flag bank at `0x80085758`, the scene label at
`0x80084548`, and the field position snapshot at `0x80084568` / `0x8008456C`.

The order is the anchors' own:

- The two playthrough cards are one run. Their play-time counters
  (`0x80084570`) and system-flag populations rise together across every save,
  so they order themselves.
- Chapter 1 before Mt. Rikuroa has no card save. Its anchors are states from
  several sessions, ordered by flag inclusion: a state whose set flags are,
  save a few optional ones, a subset of another's comes first.
- The ending anchor is a debug credits run, so it anchors the scene and is
  excluded from the order check (`order_check = false`).

Part A (`part_a_spine_is_anchored_ordered_and_routed`) re-derives all of it:
every anchor loads and sits in the scene the spine names, the flag-inclusion
order holds pair by pair, every reach flag is set in its own anchor, and the
route between consecutive milestones exists in the disc's scene graph. It
prints that route with its hop kinds:

| Arrow | Hop |
|---|---|
| `>` | A `0x3F` scene change a walk-on `.MAP` band reaches. |
| `=>` | A `0x3F` only a talk, touch or scripted record carries. |
| `~fmv~>` | An FMV hand-off (`4C E2 id`) from a walk-on record; the destination is retail's post-play dispatch (`FUN_801CEA3C`). |
| `~fmv=>` | An FMV hand-off from a record no walk-on band reaches. |

Routes are cheapest-first with walk-on hops preferred, so a `=>` in a route
means no all-walk-on route exists.

What "reached" means is per milestone: `scene` (in the scene, walking),
`control` (and the pad is back), or `flags` (and every reach flag is set).
`flags` is for two consecutive milestones in one scene. Each reach flag is one
the anchor gained over its predecessor that a clean **partition-2** SET site in
that scene writes, per the disc-wide system-flag census; entry-script writes
are excluded because they fire on every visit. Part A prints the candidates.

A milestone may also name `via` scenes: story waypoints the anchors show the
retail run passed through between the previous milestone and this one, and
that the cheapest route skips. They are visited in order, each with its beats
pass played, before the route heads for the milestone's own scene. Each
carries a comment naming the flags that put it there - Rogue Tower goes
through `conc3` (whose P2[10] sets the `0x3E5` the `juui1` hand-off in `conc2`
waits on), Zora Castle through `son`, Noaru Valley through `concend` and `jou`.
Part A checks every waypoint is a disc scene.

## Segments and tiers

One pad run cannot cross the game, so the ladder is **segmented**: segment `i`
seeds the engine at milestone `i` and asks how far it gets toward milestone
`i + 1`. Segment 0 is seeded by `BootSession::start_new_game` - the cold
opening, not a save. Tiers are cumulative:

| # | Tier | Cleared when |
|---|---|---|
| 1 | `loads` | The seeded save lands in its milestone's scene, Field or WorldMap. |
| 2 | `enters` | The entry script settles: the pad comes back, or the script leaves the scene itself. |
| 3 | `progresses` | The next milestone is reached by **seated** traversal plus the beats pass. |
| 4 | `pad` | The next milestone is reached with pad input only. |

**Seated traversal** follows the route hop by hop. On a field scene it seats
the player one tile off a walk-on door and then onto it, so the engine's own
tile-change dispatch fires the door record; on an overworld it seats the
player on the installed portal. Scripted departures along the way are
followed. The engine does the scene change; the ladder only places the player.

The **beats pass** runs once per scene, when the scene is the target and its
reach flags are unset, at a waypoint, or when a hop's door does not fire. It
approaches every boss stager whose park gate is clear (the touch dispatch runs
its placement record), then plays two kinds of beat in rounds, repeating while
a round still gains flags, since each unlocks the other:

- **Talks.** A talk NPC is spoken to when its own partition-1 record, or a
  partition-2 record it spawns (op `0x44`, followed three levels), cleanly
  SETs a still-clear flag the next anchor carries. The hand stands on a tile
  beside the NPC, faces it so the retail interact probe (64 units ahead, the
  72-unit box) lands on it, presses Cross, pages the conversation to its end
  and steps back off, so the pulsing confirm cannot re-open the same talk.
  A record that branches on where the player stands is tried from each side.
- **Walk-ons.** The ladder steps onto every gate-1 walk-on tile whose
  partition-2 record the live flags let spawn and that is not a door. The
  tile must be the record's own - the dispatch takes the first
  primary-then-fallback entry on a tile - and the approach tile must carry no
  trigger of either kind, or a kind-0 teleport there arms a warp that carries
  the player off the tile under test.

A beat is skipped when its record sets a **latch** - a flag some partition-2
C1 gate of the scene reads - that the next anchor does not carry: retail had
not played it, and its latch shuts the record the story takes (`conc2` P2[12]
latches `0x3E1`, the C1 gate of the `juui1` hand-off P2[20]).

A hop no walk-on band carries is tried by talking to an NPC whose record, or
a record it spawns, names the destination in a `0x3F` or spawns the record
whose FMV hands off to it (`town01` P1[40] spawns P2[25], the mist-night
movie). Battles are fought with the pad (below).

The **pad tier** repeats the route from a fresh seed with the random-encounter
roll armed. It walks to each door with a BFS over the collision lattice
(`World::field_dir_blocked` and its actor sibling) and a follower that inverts
the live camera remap - a condensed `critical_path_replay` walker. It plays no
beats.

In both passes a scripted sequence gets Cross on a press-2-release-14 duty
cycle, the naming prompt's Yes/No confirm gets Up first (it opens on No), and a
battle gets `critical_path_replay`'s command-ring presses: Begin, Attack, Auto,
confirm the target. Every one is a pad edge. Beyond that:

- Field dialogue runs through the inline-script field-VM runner, as it does
  in both play hosts (`World::toggles.use_vm_dialogue`); `BootSession` leaves
  it off, and without it a talk only types its first segment.
- A conversation picker takes option `k` on its `k`-th opening. Option 0 is
  often "tell me again", a branch back to the same speech, so always
  confirming the default loops a talk forever.
- A record polling the held pad (`42 01 <i>`, the compass table - Rim Elm's
  "stand here and press Down" doors) gets that direction held.
- An op-`49 04` flag-window picker (the Uru Mais warp pads) gets the row
  whose branch in the record names the hop's destination; the rows are drawn
  flipped, so Up raises the selection.
- A sparring tutorial validates each commit against its lesson, so the
  fighter takes the ring's up arm (Item, using the first item) for the Items
  lesson and its down arm for Spirit.

`LEGAIA_FGL_TRACE=1` prints one line per played beat: what ran, how it ended
and the flags it set.

The **headline** is how many milestones a cold New Game reaches contiguously
at `progresses` and at `pad` - the count of leading segments that each cleared
the tier - plus the tier sum over all segments.

## How to run

The extracted tree and the save library live only in the main checkout, so a
worktree points at them:

```bash
LEGAIA_EXTRACTED_DIR=/path/to/extracted \
LEGAIA_SAVES_LIBRARY=/path/to/saves/library \
cargo test -p legaia-engine-shell --profile release-test \
  --test full_game_ladder -- --nocapture --test-threads 1
```

`LEGAIA_DISC_BIN` must be set; without it, or without either tree, both parts
print `[skip]` and pass. `LEGAIA_FGL_ONLY=<id>,...` runs only the segments
ending at those milestones and does not assert the baseline;
`LEGAIA_FGL_NO_PAD=1` skips the pad tier.

The ratchet asserts every per-segment tier and both headline counts `>=` the
baseline. Raise the baseline in a reviewed edit from the block the run prints;
never lower it to make a red run green.

## How to read a stall

Part B prints one table row per segment and a stall list. Each stall line is
one level down from the tier that failed:

| Line shape | Meaning |
|---|---|
| `entry script never released: <holder> at <ctx> pc=.. op=..` | The pad holder - cutscene timeline, dialogue, spawned record - parked on that instruction for a whole window. |
| `hop A -> B: no walk-on door to B (...)` | Scene A's `0x3F` to B is carried by a talk, touch or scripted record; seating cannot trigger it, and no talk the ladder can find leads there. |
| `no walk-on door to B; talks that lead there did not leave: ...` | A talk whose record (or a record it spawns) names B ran, and the scene stayed. |
| `hop A -> B: no overworld portal to B installed (...)` | The overworld seeder installed no portal to B under the live flags; the portal set is printed. |
| `N door(s) to B, none fired: ...; sites: P2[r] gates FAIL: 0x.. clear` | The door record's C1 / C2 story gates (retail `FUN_8003BDE0`) refuse it; the failing flags are named. |
| `B is reached by an FMV hand-off from record(s) {(p, r)}` | The hop is a movie whose trigger record is not on a walk-on band. |
| `reach flag(s) 0x.. never set` | The target scene was reached but the beat that separates the milestones did not play. |
| `battle unresolved ...: action SM ctx[7]=0x.. <state> actor N` | The battle action state machine (retail `FUN_801E295C`) sat in that state for the whole budget. |
| `party wiped: ...` | The pad fighter lost; it has no healing, arts or magic. |
| `no walkable path: the start's walk component ends N tiles short` | The lattice cannot reach the door from where the player stands; the planner does not route through a crossing scene. |
| `pad walk stalled at tile ..` | A path existed and the follower stopped making progress on it. |
| `PANIC: ...` | An engine panic, caught per segment. |

Under each failed seated segment, the ladder lists the flags the next anchor
has that the engine never set, each with the disc sites that SET it
(scene, partition, record) from the system-flag census. That is the "which
flag was never set" answer, and usually names the record to look at next.

## Seeding

The seed goes through the host's own resume path,
`BootSession::resume_save`, so a segment is a load of a real save. The save
import carries everything the SC block holds that a segment depends on: the
whole system-flag bank up to the item array at `0x80085958`, the party count
at `0x80084594` and the roster at `0x80084598`, and the field position
snapshot at `0x80084568` / `0x8008456C`, where the resume seats the party.

The one thing the ladder adds is for a **state** anchor taken mid-field: its
live position is not the snapshot, so the ladder arms
`SceneHost::set_entry_seat` with the live one. A card save seats from its own
snapshot, as a card load does.

`BootSession::tick` routes the naming prompt's pad edges to
`World::step_name_entry_frame` and drains the per-tick queues, as both play hosts
do, so a headless driver that only ticks crosses the opening's op `0x49`.

## What it cannot measure

- The seated tier places the player. It proves doors, scripts, gates and the
  scene graph, not locomotion; the pad tier is the locomotion claim.
- The seated tier talks only to NPCs whose record reaches a flag the next
  anchor carries or a destination the route needs, and picks conversation
  options by rotation, not by reading them. Neither tier opens a menu, buys,
  equips or uses an item outside a tutorial battle; a beat that waits on one
  reads as a stall at that beat.
- The pad fighter only swings. Boss fights a player wins with arts, magic and
  healing read as wipes.
- The pad planner does not route through a crossing scene, so a door on the
  far side of a split walk component reads as `no walkable path`.
  `critical_path_replay` carries the `map01` / `suimon` crossing by hand.
- Routes follow `0x3F` names and FMV hand-offs, and prefer walk-on bands. A
  transport an entry script spawns on a story flag (`map01`'s P1[0] spawns
  the P2[31] / P2[32] flights on `0x2C3` / `0x2C5`; `station`'s spawns the
  P2[23] crossing to `map03` on `0x36B`) is an edge of the scene it leaves
  **to**, taken on arrival, which the graph does not model. Op `0x3E` with
  `op0 >= 100` is the minigame door-warp, not a transport, so no story edge is
  lost with it.
- Waypoints are hand-set in the spine from the anchors' flag differences; a
  stretch whose retail path left no flag the census can place has none.
- Chapter-1 anchors come from several sessions, some with cheat-seeded
  parties; they seed a segment faithfully to that session, not to one
  canonical playthrough.
- A card save whose scene lies off the route between two milestones (the
  endgame card's last save, on `deene`) anchors no milestone.
