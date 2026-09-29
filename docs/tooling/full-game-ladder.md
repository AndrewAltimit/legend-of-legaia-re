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
- [Seeding, and what it corrects for](#seeding-and-what-it-corrects-for)
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
reach flags are unset, or when a hop's door does not fire. It approaches every
boss stager whose park gate is clear (the touch dispatch runs its placement
record) and steps onto every gate-1 walk-on tile whose partition-2 record the
live flags let spawn and that is not a door. Battles are fought with the pad
(below).

The **pad tier** repeats the route from a fresh seed with the random-encounter
roll armed. It walks to each door with a BFS over the collision lattice
(`World::field_dir_blocked` and its actor sibling) and a follower that inverts
the live camera remap - a condensed `critical_path_replay` walker. It plays no
beats.

In both passes a scripted sequence gets Cross on a press-2-release-14 duty
cycle, the naming prompt's Yes/No confirm gets Up first (it opens on No), and a
battle gets `critical_path_replay`'s command-ring presses: Begin, Attack, Auto,
confirm the target. Every one is a pad edge.

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
| `hop A -> B: no walk-on door to B (...)` | Scene A's `0x3F` to B is carried by a talk, touch or scripted record; seating cannot trigger it. |
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

## Seeding, and what it corrects for

The seed goes through the host's own resume path,
`BootSession::resume_save`, so a segment is a load of a real save. Around it,
the ladder applies three things that path does not, and each is a finding
about the port rather than a feature of the ladder:

- **The whole system-flag bank.** `legaia_save`'s story window covers the SC
  block at `0x14C0..0x16C0`, which reaches only the first `0xA8` bytes of the
  bank at `0x80085758` (flags `0x000..0x53F`). The bank runs to `0x80085958`,
  so every flag from `0x540` up is dropped by `World::load_full` - dozens per
  late-game save. The ladder copies the full `0x200`-byte bank.
- **The present party.** `Party::from_retail_sc_block` stops at the first
  all-zero record, and the New Game template populates all four, so a save
  made with Vahn alone loads as a multi-member party. The ladder seats the
  party from the count at `0x80084594` and the roster list at `0x80084598`.
- **The saved position.** A save carries the field position at `0x80084568` /
  `0x8008456C` (the card-boot states' live positions match it exactly); the
  resume path enters at the scene's own seat instead. The ladder arms
  `SceneHost::set_entry_seat` with it.

The naming prompt's pad routing lives in the hosts, not in
`BootSession::tick`: both play hosts feed each pad edge to
`World::step_name_entry` and skip the world tick while the prompt is open. A
headless driver that only ticks parks on the opening's op `0x49` forever, so
the ladder mirrors the hosts' arm.

## What it cannot measure

- The seated tier places the player. It proves doors, scripts, gates and the
  scene graph, not locomotion; the pad tier is the locomotion claim.
- Neither tier talks to an NPC, opens a menu, buys, equips or uses an item. A
  beat that waits on a conversation reads as a stall at that beat - a finding
  about what drives the story, not a ladder defect. Tetsu's sparring match and
  the mist-night FMV record are both of this kind.
- The pad fighter only swings. Boss fights a player wins with arts, magic and
  healing read as wipes.
- The pad planner does not route through a crossing scene, so a door on the
  far side of a split walk component reads as `no walkable path`.
  `critical_path_replay` carries the `map01` / `suimon` crossing by hand.
- Routes follow `0x3F` names and FMV hand-offs. A `0x3E` door warp carries a
  scene-type selector, not a name, so its edges are absent.
- Chapter-1 anchors come from several sessions, some with cheat-seeded
  parties; they seed a segment faithfully to that session, not to one
  canonical playthrough.
- A card save whose scene lies off the route between two milestones (the
  endgame card's last save, on `deene`) anchors no milestone.
