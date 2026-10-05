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
- `west_voz_forest` (scene `vell`) is placed by the flag its entry writes,
  not by its anchor. `0x489` (setters `vell` P1[0] and `map02` P2[13]) is
  clear in every anchor through `dolk2_market_noa` and set in every card
  save from `PRO-01` on, so the playthrough's first visit falls between
  `drake_castle_revisited` and `voz_forest`. The anchor, `vell_fog_field`,
  is a door-tile poke from a state standing outside Rim Elm, so its flags are
  that state's plus `vell`'s three entry writes: it anchors the scene but
  neither the order (`order_check = false`) nor the next segment's seed
  (`seeds_next = false` - `voz_forest`'s segment seeds from
  `dolk2_market_noa`, the last save the run made before it).

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
A `scene` milestone a scripted chain only passes through is reached on the
landing: the ending's `edteien` is one link of the credits chain that runs on
to `edlast`, and its anchor is a state taken mid-cutscene there.

A milestone may also name `via` scenes: story waypoints the anchors show the
retail run passed through between the previous milestone and this one, and
that the cheapest route skips. They are visited in order, each with its beats
pass played, before the route heads for the milestone's own scene. Each
carries a comment naming the flags that put it there - the Fire Path goes
through `geremi`, Vidna and `stone` (the Star Pearl that opens the `tunnela`
door; the cheapest route rides `ropeway2`'s elevator, whose door opens only
after Xain), Rogue Tower through `conc3` (whose P2[10] sets the `0x3E5` the `juui1` hand-off in `conc2`
waits on), Zora Castle through `son`, Noaru Valley back through its own start
scene `chitei2` (whose chain ends on the `0x4C8` that opens the `map03` portal
to `concend`), then `concend`, `jou` and `retockin` (the way back from Drake
was by land and cart, not the Uru Mais warp), and Sol Tower back through its own
start scene `dohaty` (whose P2[13] sets the `0x1D4` every `station`
placement, the ticket seller among them, waits on). A waypoint whose own
arrival script carries the party on counts as visited: `concend`'s P2[0] is
its beat, and it ends in the hop to `town0d`. Part A checks every waypoint is
a disc scene.

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

A segment whose seed fails or lands outside its milestone scene, or whose
seated pass panics, scores tier 0, `none`.

**Seated traversal** follows the route hop by hop. On a field scene it seats
the player one tile off a walk-on door and then onto it, so the engine's own
tile-change dispatch fires the door record; on an overworld it seats the
player on the installed portal. Scripted departures along the way are
followed. The engine does the scene change; the ladder only places the player.
A hop that still fails after the scene's beats have run marks its edge dead,
and the route is recomputed around it when another way exists: `station`'s
only `0x3F` to `map03` is P2[23], the chapter-3 cart crash, which the entry
script spawns only while `0x36C` is clear, so later crossings go through the
ticket talk's hop to `station3`.

The **beats pass** runs once per scene visit - in the pad tier, again when
the story came back to the scene with new flags (`retona`: the Songi fight,
the `P0[1]` -> P2[17] hop to `map02` and P2[10]'s carry back, then the summit
scene P2[18], before the summit's wall switches P2[4] / P2[5] open the way
down) - when the scene is the target and its
reach flags are unset, at a waypoint, or when a hop's door does not fire. It
approaches every boss stager whose park gate is clear (the touch dispatch runs
its placement record) - every round, until a contact gains nothing, since a
stager's record is often a staged conversation (`tunnelc` P1[4], Xain) - and
plays four kinds of beat in rounds, repeating
while a round still gains flags, since each unlocks the other:

- **Talks.** A talk NPC is spoken to when its own partition-1 record, or a
  partition-2 record it spawns (op `0x44`, followed three levels), cleanly
  SETs a still-clear flag the next anchor carries. The hand stands on a tile
  beside the NPC, faces it so the retail interact probe (64 units ahead, the
  72-unit box) lands on it, presses Cross, pages the conversation to its end
  and steps back off, so the pulsing confirm cannot re-open the same talk.
  A talk that hands the frame to a record it spawned stays put instead: the
  beat owns the player (`station`'s P2[19] walks it from the counter to the
  cart).
  A record that branches on where the player stands is tried from each side.
  A venue cabinet - a record that runs the minigame door-warp (`3E` with
  `op0 >= 100`), `balden` P1[24]'s slot machine among them - is never a
  talk beat: the flags it raises are the minigame's.
- **Walk-ons.** The ladder steps onto every gate-1 walk-on tile whose
  partition-2 record the live flags let spawn and that is not a door. The
  tile must be the record's own - the dispatch takes the first
  primary-then-fallback entry on a tile - and the approach tile must carry no
  trigger of either kind, or a kind-0 teleport there arms a warp that carries
  the player off the tile under test.
- **Object doors.** A `.MAP`-bound partition-0 door record branches on the
  story flags, and one arm may run op `0x44` in place of its teleport. When
  that arm, resolved against the live flags, spawns a partition-2 record, and
  the door's own record or the spawned one (itself or through what it
  spawns) sets a wanted flag, the hand is seated on the door and nudges the
  pad, so the locomotion's own touch dispatch posts the contact. `town01`
  P0[29], Vahn's front door, spawns the P2[5] night beat that sets `0x227`
  once `0x226` is up - the flag the spar's post-fight branch in P1[10] tests.
- **Props.** A placed prop whose own bind record cleanly SETs a wanted flag,
  or spawns a partition-2 record that does (op `0x44`, three levels) - the
  latch rule below applies to the prop's record as to a talk's. `rikuroa`
  P0[2], the Genesis Tree, is the spawn case: examining it after Caruban
  spawns P2[53], which raises `0x28A` and carries the party down to `map01`.
  An interact-gated one (the cupboard class, contact result bit `1`) is
  examined the way a talk is: stand beside it, face it so the prop arm of the
  facing probe lands on its box, press Cross and page what opens - `chitei2`
  P0[33], a transport switch, raises the `0x4F0` that the P2[11] walk-on
  setting `0x470` waits on. A touch-class one (a door, bit `4`) is walked
  into from a tile beside it - `town0d` P0[1], the house door, sets `0x3B9`
  and spawns the P2[28] night scene.

A beat is skipped when its record, itself or through a record it spawns, sets
a **latch** - a flag some partition-2 C1 gate of the scene reads - that the
next anchor does not carry: retail had not played it, and its latch shuts the
record the story takes (`conc2` P2[12] latches `0x3E1`, the C1 gate of the
`juui1` hand-off P2[20], and the walk-on P2[11] spawns P2[12] as its
epilogue). A boss stager is skipped on the same evidence: its own record
sets a flag the next anchor lacks and no record of the scene clears, so
retail never fought there (`town0b` P1[36], a loss-allowed fight, raises
`0x5C0` before its `3E FF 03`).

A talk or walk-on beat whose record ends on `3E FF` has committed a fight
that starts only after the record is gone, so the beat is fought before the
next one runs: what the story does next often hangs on the post-battle
return, where the scene system script re-runs and spawns the next record
(`chitei2` P2[13] stages the Jette fight; the return spawns P2[14], which
raises the `0x6D1` behind the `map03` hand-off setting `0x4C8`). A beat that
has already reached the milestone it is played for stops short of its fight
(`chitei2` P2[11] sets `0x470`, then stages a battle).

A hop no walk-on band carries is tried by talking to an NPC whose record, or
a record it spawns, names the destination in a `0x3F` or spawns the record
whose FMV hands off to it (`town01` P1[40] spawns P2[25], the mist-night
movie). Battles are fought with the pad (below).

The **pad tier** repeats the route from a fresh seed with the random-encounter
roll armed, and plays the same waypoints and beats with pad input only. It
plans with an A* over the collision lattice (`World::field_dir_blocked` and the
actor boxes) and follows with a walker that inverts the live camera remap - a
condensed `critical_path_replay` walker. The planner models the two ways a
field map joins its own parts, since a town's house interiors sit in the same
map as its street:

- a **kind-0 teleport** tile is an edge to its landing cell. A closed door prop
  standing over one reads solid from every side, and the planner counts it
  open: pressing into it is the touch that opens it (`31 00`). A leaf is
  over the teleport when its box centre lies within the tile's half-tile
  margin or its anchor tile is one step from it - `dolk`'s inn stair door
  is anchored at (76, 121) in front of the (76, 122) teleport, its box
  centre short of the margin. A touch-class door whose touch reaches `31 00`
  opens itself wherever it stands, so it counts open too: `jiji` P0[0], the
  door across the corridor to the `map02` mouth at (66, 96), stands over no
  teleport. The touch is read from the record's resume point (past its first
  `21`) with story-flag tests followed against the live flags; a door whose
  opening sits behind a box test of the player opens only from a cell inside
  that box (`ropeway` P0[2], the Octam station door, opens for a player on
  tiles (27..30, 33..34) and is a wall from the corridor south of it).
- an **object door** - a walk-touch placement whose record, resolved against
  the live flags, moves the player - is an edge wherever the leading actor
  probe points reach its contact box, including through the wall the door is
  set in. A second door leaf beside the contact counts as open for the same
  reason. A lift ride lands on the partner's platform under the engine's
  arrival bracket (see [`field-locomotion.md`](../subsystems/field-locomotion.md#the-arrival-bracket)),
  so its edge ends not on the landing but on each cell just out of the
  platform's reach that the landing's pocket opens onto; a plan that starts
  under a live bracket walks the platform freely.
- a **ledge hop** - a gate-1 walk-on tile whose partition-2 record the live
  flags let spawn and whose body arcs the player to one landing tile (op
  `0x43` sub-0/1/A/B on the `0xF8` channel) - is an edge like a teleport. A
  mountain joins its terraces this way: the way down from `rikuroa`'s summit
  is a kind-0 teleport into the west strip and then the P2[6..25] hops. A
  record that hops more than once, or box-tests the player first, is left
  out: its landing is not one place.

The follower leaves a cross-axis offset of a few units alone while the
other axis still has ground to cover: chasing its own overshoot flips the
diagonal every frame, and along a terrace edge the side-step toward the
drop is a ledge hop (`tunnela`'s corridor at (89, 79) drops the party to
(89, 80), a one-way hop it cannot climb back). A script that fires on the walk and changes no flag, more than a couple of
dozen times on one walk, makes that walk a stall: `retock` P2[26], the Mt.
Letona checkpoint, turns the party back every time it is crossed without
Lord Saryu's key, and re-crossing it ate the segment's budget. The follower presses a teleport waypoint until the jump lands, backs out of a
diagonal-wall notch where all four lattice steps read blocked, and, held
against something for a second, tries the action button. In a field the tap
waits for a frame the player did not move: the stall counter also counts a run
step that stays inside one cell, and a tap there stood the player still for a
frame of every few. A script that runs
on the way and changes nothing - no flag, no cell, no scene (an examined prop
with nothing to say) - leaves the route as it was rather than re-planning the
walk component. Walks avoid the live
walk-on bands (a band whose record's story gates shut it is walkable, and so
is an inert band that only sets the camera or the view, such as `retona`
P2[0] across the summit's passes) unless one is the only way through. A band
that turns the party back (`retona` P2[24], until `0x367`) stays avoided.

A door tile whose centre is wall is aimed at through its open sub-cell, which
says the side a player steps in from (`teien`'s way down at (42..43, 29) is
entered from the corridor above). A walk-on band whose record clears a C1
latch of the target door's own record is the way in rather than a beat to
step around, and the hand crosses it first (`ropeway` P2[25] clears `0x514`,
the latch of the gate to `ropeway2`, and re-opens the door frame in front of
it). Standing on a door band whose crossing was consumed - the dispatch drops
a tile change made while a script holds the player - the hand steps off and
back on from each side.

The pad hand's beats are the seated tier's, played as a player plays them:

- a **talk** walks up to the NPC (re-reading a routed NPC's position),
  leans toward it until the retail interact probe lands on it, presses Cross,
  pages the conversation and steps away. Talks go nearest first;
- a **walk-on** walks to an inert tile beside the band, then onto it. A band
  is often several tiles wide and boxed in on some of them (`retockin`
  P2[42], Lord Saryu's audience, spans (104..107, 50) and (107, 50) is walled
  in): when the walk finds no path to its tile, or stalls at its edge, the
  band's other tiles are tried, nearest first;
- an **object door** or a **boss stager** is walked up to and leaned on. A
  stager's fight fires on the next field step, so the walk that step belongs
  to fights it instead of fleeing it;
- before a boss, and whenever the weakest member is below half HP, the hand
  heals through the pause menu: Start, Items, Use, the first HP restorative,
  the weakest member, Circle back out;
- still below half HP after that, in a scene that rolls encounters, with an
  Incense (`0x8A`) in the bag and its window `_DAT_8007B600` run out, the
  hand burns one: Start, Items, Use, the Incense row, Yes. The region roll
  skips while the window is open, so one use is `0x40` walk-regen ticks
  (`0x800` walking vsyncs) with no encounter, whatever the stream deals;
- leaving a scene that rolls no encounters with the weakest member still
  below half HP and nothing in the bag to heal it, the hand **rests**: it
  walks onto a live walk-on band whose partition-2 record runs the `4C 82`
  restore (a bed), or talks to an NPC whose record or a record it spawns
  does (an innkeeper; the gold gate is the record's own). In a scene that
  rolls, the walk to the bed costs what the walk out does, so it heads out.

A pad segment has a frame budget (`PAD_SEGMENT_FRAMES`); a segment the hand
cannot finish inside it stalls with `pad frame budget (N) spent` rather than holding
the run. `LEGAIA_FGL_TRACE` prints each pad segment's frames and planner cost,
and `LEGAIA_FGL_WALK_DEBUG` adds a stalled walk's wall map, the scene's
teleports and door colliders. `LEGAIA_FGL_PLAN_DEBUG` prints each object-door
edge the planner adds, and `LEGAIA_FGL_COMP_DEBUG` prints the tile map of every
walk component a plan failed inside (`o` reached, `A` avoided door tile, `b`
actor box, `G` the goal) with the overworld's installed entities, and a
sub-cell flood of the wall bits alone beside it (when that flood stops short
too, the gap is a join the walls do not show - a script, a story gate - not
the planner). The component map also lists the live NPCs, the solid props with their
bind records, and the self-opening doors. `LEGAIA_FGL_JUMP_DEBUG` prints every
pad-walk frame that moves the player more than 64 units (a door or lift
ride) with the live arrival bracket, and
`LEGAIA_FGL_PROBE_AT=<scene>:<x>,<z>;U;R;...` seats the player at the start
of the first pad hop in that scene and holds each listed direction for 40
frames, printing where it went - the way to ask the engine, rather than the
lattice, whether a spot can be walked off. `LEGAIA_FGL_POS_TRACE` prints every tick of a scripted run that
moves the player or changes the pad holder's park site, and the trace prints
the seated tier's trail on success, so a pad stall can be read against the
route the seated tier took.

A plan that cannot reach its goal searches the start's whole walk component,
and a stalled walk re-plans every second, so a failed plan is remembered -
the cells it reached and the one nearest the goal, keyed by scene, goal,
avoid set and story flags - and a start inside it plans straight to that
cell. On the overworld the follower inverts the camera remap exactly (of the
eight pad directions, the one whose world bits are the step's): a rounded
rotation turns a cardinal step diagonal whenever the camera sits off an axis.

On the overworld the pad tier walks diagonals **through tile corners**. The
encounter step is a change of 128-unit tile, and a change of one tile on both
axes at once is one step: `FUN_801D9E1C` caches the tile and reads a region
only when the new one differs by at most one on each axis (`slti 0x2` at
`0x801D9EF0` / `0x801D9F08`). The lattice's four-way staircase pays two steps
for every diagonal tile it gains, so a kingdom crossing drains the counter
about twice as fast as a player cutting corners does - the difference between
two fights and three for a lone member walking out of a dungeon worn down.
The tile route is eight-connected over tiles whose four wall sub-cells are
open (a diagonal also needs both tiles whose corner it cuts), one step per
move, to the open tile nearest the goal; the lattice finishes from there and
takes over outright when the route presses without moving. A diagonal is held
only once both axes are the same number of 2-unit sub-steps from their tile
edge, so both edges fall in one sub-step. Until then the farther axis walks
alone, tapping when close: a released pad zeroes the overworld walk carry, so
the next pressed tick commits exactly one sub-step.

A door whose walk component the start cannot reach is tried through a
**crossing scene**: a scene the current one has a door to and a door back
from, entered and left by its reachable door farthest from where the player
came in. The crossing to take is read off the lattice first: with every door
tile a boundary, the scene splits into walk components, and a crossing joins
the component its door touches to the components holding the entry tiles of
its own `0x3F`s back (a town with two gates has two landings). Only the
partition-2 door records' `0x3F`s count as landings: those are the exits a
walk-on band or a played beat leaves by, and a talk record's `0x3F` is that
conversation's own exit on its own story gate. `dolk2` P1[47], the castle's
old machine, lands on `map01` `(65, 50)`; read as a landing, it made `dolk2`
the way to `vell`'s side while both of `dolk2`'s gate bands land where the
party came in, and the planner spent its crossings there. The playthrough
drains `suimon` instead (`0x27B` is set, P1[47]'s other write `0x17D` is
not, in every card save from `PRO-01` on). A
breadth-first search over those joins names the first crossing of the
shortest chain to the component of the wanted door, and the chain is walked
one round trip at a time, re-planned from each landing. Leaving the crossing,
the hand prefers the door whose own record lands on the planned side, and
takes any door when that one is out of reach. A crossing that
returns to the same side is taken once more with its own beats played first,
and after that it is left out of the plan: which side it delivers to is story
state the lattice cannot see (`suimon`'s two chambers join only once its water
gate is drained). Candidates the lattice cannot place are still tried in
turn behind the planned one. An arrival script that carries the party
straight back out counts as a round trip, landing and all. A detour that ends
in a third scene leaves the player on another side of the scene it set out
from, and a further detour is taken from there (up to three per hop): from
`rim_elm_restored`, `rikuroa` turns the party away and the walk out lands by
`cave01`, on the side of `map01` that holds `keikoku`'s west mouth.

In both passes a scripted sequence gets Cross on a press-2-release-14 duty
cycle, the naming prompt's Yes/No confirm gets Up first (it opens on No), and a
battle is fought through the command ring, one pad edge at a time, by a
fighter shaped like a player: a member who is down gets a revive, and a
member in danger - standing, and under 45% of its HP, or unable to take another loss the
size of the biggest it took this battle between two of the party's command
windows (a round, not a hit: a fast foe acts twice in one, and a cast lands
its flurry and its burst as separate HP writes) - gets a heal: a party heal when two
or more are in danger, else the smallest single heal that lifts the worst-off
member clear, aimed at that member. Heals the round's earlier members already
committed count as landed, so two members never spend their turns on one
wound. Otherwise a member with an
affordable damaging Seru spell casts the strongest one; otherwise it attacks
through `Command`, entering the longest art its command pool pays for and
spending the rest on plain directions (whether a matched art fires is the
queue builder's call, out of the Spirit gauge). The plan runs up to nine
commands, the length of each character's Miracle Art. A random encounter that
interrupts a pad-tier walk is fled instead (the round prompt's Run), unless
the fight forbids running.

A foe that winds up is guarded against the turn its blow lands: a
capture-class charge body (Xain's Bull Charge, PROT 0953) sets its caster's
ability latch (`0x801C8FE0`) and deals nothing, and the next cast through the
body is the party-wide Terio Punch (power `0x274`), which reaches about 900
on each member of a party in the 800s. A talk whose record, or a record it
spawns, stages a fight is walked into at full strength, as a stager is.

The fighter also reads a boss's cadence the way a player does. It keeps each
round's total party HP loss, and when the last four rounds went heavy, quiet,
heavy, quiet (no two heavy rounds back to back all battle), the coming round
is the heavy one: a member whose HP, once the round's committed heals land,
the guard's halving lets live through it takes Spirit instead of attacking
(Rogue alternates Element Change with a party-wide hit, off its picker's
round parity). A foe on any other cycle gets no guard. A fight runs at least
60 000 ticks and goes on past that while a foe's HP is still dropping, up to
240 000; it is unresolved only once 12 000 ticks pass with no foe losing HP.

The party's battle forms - the action clips the hit events are paced by and
the art records the arts input tokenizes - are the engine's to install at
battle entry (`SceneHost::ensure_battle_party_forms`, see
[`battle.md`](../subsystems/battle.md)), so a headless fight swings the same
clips and matches the same arts a windowed one does. Beyond that:

- Field dialogue runs through the inline-script field-VM runner, as it does
  in both play hosts (`World::toggles.use_vm_dialogue`); `BootSession` leaves
  it off, and without it a talk only types its first segment.
- A picker takes option `k` on its `k`-th opening, in a conversation and in
  a script's own box alike, counted by the record's bytes and the picker's
  offset across every talk of the run (the modal timeline's, else the first spawned
  record holding one). Option 0 is often "tell me again", a branch back to
  the same speech, so always confirming the default loops a talk forever
  (`town0d` P1[5], Tetsu's sparring offer).
- A record polling the held pad (`42 01 <i>`, the compass table - Rim Elm's
  "stand here and press Down" doors) gets that direction held.
  A band whose record polls once, on the tick it spawns, and that the pad
  walk crosses on its way elsewhere is crossed holding the walk's own pad
  for that tick, as a player walking through does: `balden`'s elevator call
  bands (P2[6], P2[7]) want Up toward the car, and answering their poll
  walked the player back off the band onto which the walk re-crossed it.
- An op-`49 04` flag-window picker (the Uru Mais warp pads) gets the row
  whose branch in the record names the hop's destination; the rows are drawn
  flipped, so Up raises the selection.
- A sparring tutorial validates each commit against its lesson, so the
  fighter takes the ring's up arm (Item, using the first item) for the Items
  lesson and its down arm for Spirit.

`LEGAIA_FGL_TRACE=1` prints one line per played beat: what ran, how it ended
and the flags it set - and, as `[hop]`, the hop failure that sent the pass to
the beats, which the final stall would otherwise hide. Each fought battle
prints a `[battle]` line at its start and end (formation, both sides' HP, the
action-SM state); `LEGAIA_FGL_TRACE_BATTLE=1` adds one every 1000 ticks.

The **headline** is how many milestones a cold New Game reaches contiguously
at `progresses` and at `pad` - the count of leading segments that each cleared
the tier - plus the tier sum over all segments.

## How to run

The extracted tree and the save library default to `extracted/` and
`saves/library/` under the repo root. They live only in the main checkout, so
a worktree points at them:

```bash
LEGAIA_EXTRACTED_DIR=/path/to/extracted \
LEGAIA_SAVES_LIBRARY=/path/to/saves/library \
cargo test -p legaia-engine-shell --profile release-test \
  --test full_game_ladder -- --nocapture --test-threads 1
```

`LEGAIA_DISC_BIN` must be set; without it, or without either tree, both parts
print `[skip]` and pass. `LEGAIA_FGL_ONLY=<id>,...` runs only the segments
ending at those milestones and does not assert the baseline;
`LEGAIA_FGL_NO_PAD=1` skips the pad tier, and `LEGAIA_FGL_RNG_SEED=<u32>`
re-seeds the world rand stream once a pad segment is seeded, dealing it
another hand. `LEGAIA_SCUS` defaults to the
extracted tree's `SCUS_942.54`. Two part-A diagnostics:
`LEGAIA_FGL_EDGES=<scene>,...` prints each named scene's out- and in-edges
(`*` marks a scripted one), and `LEGAIA_FGL_GAINED=<id>,...` prints every flag
each named milestone's anchor gained or lost over its predecessor, with its
disc SET sites - the evidence a waypoint is chosen from.

The ratchet asserts every per-segment tier, both headline counts and the tier
sum `>=` the baseline. Raise the baseline in a reviewed edit from the block the run prints;
never lower it to make a red run green.

## How to read a stall

Part B prints one table row per segment and a stall list. Each stall line is
one level down from the tier that failed:

| Line shape | Meaning |
|---|---|
| `seed: ...` / `seed landed A in <mode>, milestone scene is B` | The resume failed, or did not land walking in the milestone's scene (tier 0). |
| `entry script never released: <holder> at <ctx> pc=.. op=..` | The pad holder - cutscene timeline, dialogue, spawned record - parked on that instruction for a whole window. |
| `hop A -> B: no walk-on door to B (...)` | Scene A's `0x3F` to B is carried by a talk, touch or scripted record; seating cannot trigger it, and no talk the ladder can find leads there. |
| `no walk-on door to B; talks that lead there did not leave: ...` | A talk whose record (or a record it spawns) names B ran, and the scene stayed. |
| `hop A -> B: no overworld portal to B installed (...)` | The overworld seeder installed no portal to B under the live flags; the portal set is printed. |
| `N door(s) to B, none fired: ...; sites: P2[r] gates FAIL: 0x.. clear` | The door record's C1 / C2 story gates (retail `FUN_8003BDE0`) refuse it; the failing flags are named. |
| `B is reached by an FMV hand-off from record(s) {(p, r)}` | The hop is a movie whose trigger record is not on a walk-on band. |
| `reach flag(s) 0x.. never set` | The target scene was reached but the beat that separates the milestones did not play. |
| `battle unresolved ...: action SM ctx[7]=0x.. <state> actor N` | The battle action state machine (retail `FUN_801E295C`) sat in that state for the whole budget. |
| `party wiped: ...` | The fighter lost. It heals, casts, enters arts and guards a two-round cadence's heavy round, but it never changes equipment. |
| `no walkable path: the start's walk component ends N tiles short` | The lattice cannot reach the door from where the player stands, through the scene's teleports and object doors; a pad hop then tries a crossing scene. |
| `pad walk stalled at tile ..` | A path existed and the follower stopped making progress on it. |
| `PANIC: ...` | An engine panic, caught per segment. |

Under each failed seated segment, the ladder lists the flags the next anchor
has that the engine never set, each with the disc sites that SET it
(scene, partition, record) from the system-flag census. That is the "which
flag was never set" answer, and usually names the record to look at next.

### A pad wipe that moves with an unrelated change

The pad tier is deterministic, but every random draw it meets - the
encounter step, the formation roll, the escape roll, enemy targeting - comes
off the one world rand stream, and so does any field system that draws per
frame (the fog spawner `FUN_801D629C` draws only once its region gate
passes). A change that alters how many draws happen before a fight -
gating a fog region off, say - deals every later fight a different hand.
Where a segment is seeded with a lone member near death and an empty bag,
that hand decides the tier: a back attack plus one caught Run is a wipe.

So a `party wiped` stall that appears or disappears with a change that does
not touch battle is first a rand-stream question. Trace it with
`LEGAIA_FGL_TRACE=1 LEGAIA_FGL_TRACE_HITS=1` (each HP change with its actor
and action state): a back attack shows as an enemy hit before the party's
first command, and the fight's opening formation and HP read off the
`[battle] start` line. If the draws are retail's, the move is the route's
fragility, not a regression.

`LEGAIA_FGL_RNG_SEED` measures that fragility directly: run the segment
under a spread of seeds and count the wipes. `rim_elm_restored` shows
what such a fragility usually is: a property of the anchor rather than of
the hand. The segment seeds from `player_steal_skeleton_banner`: Vahn alone
at 17 of 219 HP, mid-battle in a mist-era `dolk` whose every region rolls
(`0x142` clear), on the frame after his killing blow stole an **Incense**
from the skeleton. Its sibling `player_steal_skeleton_pre`, the frame
before the steal resolves, holds the same SC block without the Incense, and
seeded from it the pad tier is a draw: a pair of `map01` monsters
out-speeds a 17-HP fleer and deals more than 17 in one round whether the
Run is caught or not. Walking straight out clears on 23 of 30 dealt
streams, fighting every encounter instead on 13 of 21, and the one rest in
reach, `dolk` P2[9] (the bed behind the inn's stair door), wipes on 14 of
21 on the way there. From the post-steal frame the hand burns the Incense
before it sets out, no region rolls between `dolk` and Rim Elm, and the
pad tier clears on the default stream and on seeds 1..40 alike.

## Seeding

The seed goes through the host's own resume path,
`BootSession::resume_save`, so a segment is a load of a real save. The save
import carries everything the SC block holds that a segment depends on: the
whole system-flag bank up to the item array at `0x80085958`, the party count
at `0x80084594` and the roster at `0x80084598`, and the field position
snapshot at `0x80084568` / `0x8008456C`, where the resume seats the party.

The anchor's **Field Move** word (`0x800846CC`, SC `+0x58C`) goes onto
`World::locomotion.run_default`: the engine keeps it as a host option rather
than save data, but retail restores it with the block, and the ladder's player
runs or walks as the anchor's player did. Timed scripts are paced for it -
`jouind`'s switch pair resets unless the second switch is reached within 50
vsyncs of free walk (system script slot `0`, `4C CB 00 FF FF` against
`4E 00 50 32`), which a walking pad hand misses by a few frames.

The other thing the ladder adds is for a **state** anchor taken mid-field: its
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
  options by rotation, not by reading them. Neither tier buys or equips, and
  only the pad tier opens the pause menu (to heal); a beat that waits on a
  purchase or an equip reads as a stall at that beat.
- The fighter guards only a foe that hits hard every other round, never
  targets a weakness or changes equipment, and it flees a travel leg's random
  encounter. A fight that needs any of those reads as a wipe.
- The pad planner finds a crossing scene by trial, not by reading which side
  each of its doors lands on, and a crossing whose side is story state
  (`suimon`'s water gate `0x27B`) needs that beat played first. The talk
  beats count a **hand-off** as a story beat: a talk that sets a flag the
  live state lacks and changes scene to one whose entry script tests it.
  `suimon`'s Water Gate Controller (`P1[4]`) sets `0x2C6` and enters `map01`
  at `(0, 0)`, which is retail's own landing for a cutscene visit: `map01`'s
  `P1[0]` spawns the drain (`P2[15]`, `0x2C6` -> `0x2C7`), which returns the
  party to the drained chamber, where `suimon`'s `P1[0]` sets `0x27B`. None
  of those flags but the last is in the next anchor, so a flag-reach test
  alone never picks the controller. A crossing whose beat changed scene lets
  the landing's arrival run before the position is read.
- The pad hand heals only with items it already carries and with a free
  or affordable rest in a scene that rolls no encounters, and wards off
  encounters only with an Incense it carries; it does not buy items or use
  magic.
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
- From `rim_elm_restored`, `vell` has no walkable way: on `map01` Rim Elm's
  side and `vell`'s are separate walk components of the static collision
  grid (the grid in a retail RAM image of the overworld matches the disc
  `.MAP` except Rim Elm's gate paint), and every crossing open at that story
  point lands elsewhere - `suimon`'s two chambers join only once its water
  gate is drained (the controller, P1[4], wants `0x26F`, the Water Gate key,
  whose one clean setter is `dolk2` P2[7], after Caruban), and `keikoku`'s
  west lane is held by P2[7] at `(42, 89)` until `0x142`, the Caruban beat.
  That is why `west_voz_forest` sits after `drake_castle_revisited`, where
  the pad hand drains `suimon` and crosses `bylon` to the `vell` door.
