# Capture coverage

The [retail comparison corpus](retail-compare.md) scores the engine at every
captured moment, and its headline is only as broad as the moments it holds: a
median image score over states that all stand in a handful of scenes, fighting
a handful of monsters, says nothing about the rest of the game. This report
takes the denominator from the **disc**, the way
[disc coverage](disc-coverage.md) does for code, and asks of every item the
disc defines whether at least one retail corpus state shows it. Its output is a
ranked worklist of what to capture next.

| Piece | Lives in | Role |
|---|---|---|
| census | [`retail_coverage.rs`](../../crates/parity/src/retail_coverage.rs) | Read every corpus state's RAM (no engine run) and the disc's monster, spell and formation lists |
| `legaia-engine retail-compare --census <json>` | [`retail_compare_cli.rs`](../../crates/parity/src/retail_compare_cli.rs) | Write the census and exit |
| driver | [`scripts/ci/capture-coverage.py`](../../scripts/ci/capture-coverage.py) | Build, run the census, join it with the scene table and the baseline, write the report |

```bash
scripts/ci/capture-coverage.py                # build, census, report under captures/capture-coverage/
scripts/ci/capture-coverage.py --no-build     # reuse target/release-test/legaia-engine
scripts/ci/capture-coverage.py --census F     # report from an existing census JSON
scripts/ci/capture-coverage.py --summary      # the headline table only
```

The census and the report carry monster and spell names read off the disc, so
both stay under the gitignored `captures/` directory. Like the corpus driver,
the script exits 0 with a `[skip]` line when the save library or the extracted
disc is missing.

## What counts as covered

Two tiers, per item:

- **held** - some corpus state made on a retail disc shows the item. A state
  with a `resident_patch` ([the ratchet](retail-compare.md#the-ratchet))
  replays a modified executable and never covers anything.
- **scored** - that state is in
  [`retail-compare-baseline.json`](../../scripts/ci/retail-compare-baseline.json)
  with an `image` channel, so the corpus median already measures it.

A held item that is not scored is a capture the corpus cannot yet seed: its
class is one the [seeding model](retail-compare.md#the-corpus) does not reach,
or its frame is a fade.

## The dimensions

| Dimension | Denominator (disc source) | A state covers an item when |
|---|---|---|
| scenes | every `town` / `field` / `world_map` block of [`scenes.toml`](../reference/scene-names.md) | its loaded scene `0x80084540` is the block, in any class |
| scenes walked | the same blocks | a `field` / `world_map` state stands in it |
| monsters | every populated monster record in PROT 0867 | the formation cell `0x8007BD0C` holds the id |
| enemy attacks | per monster: its physical attack, plus each magic attack id its record carries (`+0x21..=+0x23`) | the acting seat `ctx[+0x13]` is that monster's, with category `3` in the attack band, or category `2` with that spell id queued at `+0x1DF` |
| enemy magic attacks | the distinct ids of the row above | as above, any monster |
| player casts | the named summon-band spells `0x81..=0xA0` of the [spell table](../formats/spell-table.md) | a party seat acts with that id queued, category `1` or `2` |
| cast modules | the slot-B images PROT `0903..=0966` | an acting cast pages the module ([below](#which-module-a-cast-pages)) |
| mode classes | the corpus classes | a state of the class |
| menu screens | every player-reachable id of the [sub-screen table](../subsystems/save-screen.md#sub-screen-function-pointer-table), the title family (word clear) and the map display (game mode `0x0D`) | a menu state's sub-screen word `DAT_801E46A4` |
| battle screens | the [command flow](../subsystems/battle-command-flow.md)'s surfaces and the results window | a battle state's `ctx[+0x06]`, or `ctx[+0x07] = 0x5A` past the hand-off |

A heal-class spell runs the Spirit / Item flow `0x3C..=0x40`
([the magic arm's override](../subsystems/battle-action.md#outer-dispatch---ctx7-action-state-cursor)),
so a category `2` action in that band counts as well; the harvester saves it at
`0x3E`, and the oracle begins its recording there.

An action counts only while it is on screen: the action-state cursor
`ctx[+0x07]` sits in the attack chain `0x14..=0x20`, the magic / item flow
`0x28..=0x2E`, the summon flow `0x32..=0x38` or the module sequence
`0x6E..=0x71` ([battle-action](../subsystems/battle-action.md#outer-dispatch---ctx7-action-state-cursor)).
A prompt, a commit screen or the clean-up band after the hit covers the
monsters on the field but no attack.

A boss's scripted cast - one of the picker's per-monster arms in
`FUN_801E9FD4`, or a duel's fixed script - names an id the monster's record
does not carry. The census is the only witness to it, so a seen scripted cast
joins the denominator as its own item (`(scripted)` in the report). The
denominator therefore grows as such casts are captured; the record-carried
items are fixed by the disc.

### Which module a cast pages

`spell_module_prot` mirrors the two loader paths of the
[cast-module](../subsystems/cast-module.md) page:

- the summon band `0x81..=0xA0` pages `FUN_8003EC70(id - 0x79)`, extraction
  entry `id - 0x79 + 0x37F` (`0903..=0934`) - by id, whatever the record's
  class byte, so the ally-side Vera (`0x83`) and Orb (`0x89`), whose class is
  not the summon class, still land on their own module;
- a capture-class record (`+0 == 0x63`) pages `record[+1] + 0x28 + 0x37F`
  (`0935..=0966`).

Summon-band id `0x98` has no named record, so its module `0926` shows an
empty spell column: an uncovered module with no spell is not necessarily a
cast the game reaches.

## Ranking the worklist

Each dimension's uncovered items are listed most valuable first:

- **monsters and enemy attacks** by how many MAN formation rows across all
  scenes hold the monster (`encounter_man::formation_defs_from_man` over every
  CDNAME scene), with the scenes it is met in - the scenes to walk to capture
  it. A monster no row holds is a scripted or battle-id fight.
- **enemy magic attacks** by how many monster records carry the id.
- **cast modules** by how many spells share the module.
- **scenes** towns first, then fields, then the overworlds, in CDNAME order.

The report closes with every held item and the states that hold it.

## Filling the worklist

The enemy-attack rows are filled by
[`autorun_battle_harvest.lua`](../../scripts/pcsx-redux/autorun_battle_harvest.lua)
([catalogue](pcsx-redux-automation.md#battle-flow-camera-ui-and-animation)):
from a retail field state it walks until an encounter, plays the rounds with
Spirit so the monsters act, and saves a raw state whenever a monster's action
is mid-way. A dungeon state the worklist names (its "where to capture"
column) is the source to start from; a town has no encounters. From a
probe-poked pre-cast state (`koru_pre_*`) it saves the forced cast mid-module
instead, and the new state inherits that state's poke.

A fight need not wait for the walk to find it. `LEGAIA_WANT=147+18;15+120` names
formations by their monster ids. In the field the probe finds the scene's row
holding exactly those ids, points every region of the live MAN's region table at
that row alone, and runs the step counter `0x8007B5FC` down, so the next step rolls
it through retail's own `FUN_801D9E1C`: the stage id, the world-map return point and
the battle flags stay the scene's. Those region rows are the only RAM it writes
before the fight, and they stay resident in the saved states. `LEGAIA_TOUGH=1` keeps
a low-level party alive (live HP, readout and pending delta together: the readout
alone parks state `0x51`; `LEGAIA_TOUGH=2` also raises each seat's maximum to 9999, for a party one hit would wipe, which the state keeps on its HUD; the seed reads the seat word against the character record and stands the raised maximum on the engine's seat (`RetailBattle::party_seat_pool_raised`), since the fight and the HUD read it), `LEGAIA_WOUND=<round>` cuts the monsters to a third of
their HP so a heal arm has a patient (`LEGAIA_WOUND_EVERY=1` repeats the cut each round), and `LEGAIA_FINISH=1` ends the fight after the
Spirit rounds. The action in each state is the AI's own, and `LEGAIA_SAVE_PRE=1`
keeps the whole emulator state from that action's `0x0A`, ahead of the pick, which
replays the same cast for the [cast-sequence oracle](cast-sequence-oracle.md). A
cast the AI will not pick in the rounds given still needs `--monster-pick`.

A one-swing monster sits in the strike loop `0x1E` for a single pass: the loop stages
its last byte and leaves for `0x1F` together
([battle-action](../subsystems/battle-action.md#attack-chain---strike-loop-0x1e)), so the
default `LEGAIA_HOLD` of 8 vsyncs never sees it held and the attack stays uncovered however
often the monster swings. `LEGAIA_HOLD=1` saves it on the pass after the band opens
(`battle_map01_hornet_strike`). The sparring tutorial's prompts take Attack and refuse
Spirit, so its fight wants `LEGAIA_SPIRIT_ROUNDS=0` (`battle_town01_tetsu_strike`, from
`s5_tetsu_battle`).

`LEGAIA_WANT` points every region at the row but keeps each region's tile box, so a
scene whose live [condition group](../formats/encounter.md#the-condition-array-story-flag-gated-region-groups)
covers only tiles the walk never reaches rolls nothing. `LEGAIA_WANT_WIDE=1` also widens
every box to the whole map; the state's description says so. A source state whose player
carries the movement-disabled bit `0x80000` at `+0x10` (an event holds it) takes no step at
all, so no roll fires there either way (`concnow_field_door_hop_settled`,
`chapter2_garmel_pre_zeto`).

Party strikes are not a census dimension yet, and they are where a harvest finds
the most new reading: a command ring with `LEGAIA_PARTY_ARTS` set (per seat, a
`,` list of arts as art-data tokens, `1` Left, `2` Right, `3` Down, `4` Up, cycled
over the rounds) takes Attack, enters the art and commits, and each seat's strike
loop `0x1E` is captured at `LEGAIA_PARTY_HOLDS` vsyncs in. `LEGAIA_MONSTER_TOUGH=1`
restores the monsters at each round prompt so a fight outlasts the list, and
`LEGAIA_PARTY_TRACE=1` writes `party_trace.csv` (state, cursor, committed and staged
clip, close-up accumulator, yaw) per vsync of a party's attack chain. The states
register as `battle_<scene>_party_<char>_<art>_{open,mid,late}`.

A state is a valid run of the game whichever way it was reached, so state-making runs
take `run_probe.sh --fast --turbo`: no display, several at once, well above 1x
([headless runs](pcsx-redux-automation.md#headless-runs)).

The scene rows are filled the same way: with `LEGAIA_SCENES=1` the probe also
saves one field state per scene its walk reaches, once the loaded scene has
run a settle window and the player has moved in it. A save point in an
uncovered scene is reached cold: `isolate_card_save.py` stages the one save
on a scratch card, `autorun_boot_continue.lua` loads it from the title, and
the walk starts from that state.

A random walk finds the doors near where it starts and few beyond. A scene
two or three doors from any save point, a minigame cabinet, or a room behind
a guard is reached by a **planned** walk instead:

1. `asset scene-nav <scene>...` writes each scene's navigation facts as JSON:
   the `.MAP` collision grid and trigger tables
   ([field-map](../formats/field-map.md)), the MAN's door sites with the
   partition-2 slot each trigger names, and the placements.
2. [`plan_walk_route.py`](../../scripts/pcsx-redux/plan_walk_route.py) searches
   the wall sub-cells of each leg (`scene:start:goal`, a goal being a door's
   trigger tile, a point, or `far:N` steps) with the kind-0 teleports as edges,
   and writes a route file. Other doors' tiles are walls and are named on an
   `AVOID` line; an event tile is crossed only when nothing else connects.
3. [`autorun_walk_route.lua`](../../scripts/pcsx-redux/autorun_walk_route.lua)
   walks it on the retail disc, pad only: it steers on the live player
   position through the scene's pad octant, fights a random encounter with
   the harvester's hand, and saves a state in every scene the route enters.
   An `ACT` line presses Cross at a cabinet or a sign-up counter and saves
   the minigame that opens at chosen vsyncs; a `TALK` line opens and pages a
   conversation that must be had to pass (the Gold Card guard at `koin3`).

The planner sees walls, trigger tiles and spawn points only. A door actor, a
moving NPC or a story-gated paint can block a planned leg; the probe then
sidesteps, and a leg that stays blocked ends the run with the states it
already saved.

A scene change need not be a door. `conc3` is `conc` after the inn's stay: the
innkeeper (`conc` P1[9]) loads it from the Yes of the stay picker, so
`asset scene-nav` lists it under the partition-1 record with no partition-2 slot,
and a route reaches it with a `TALK` line (`conc3_field_inn_night`).

When the walk is not the point, [`autorun_door_hop.lua`](../../scripts/pcsx-redux/autorun_door_hop.lua)
crosses a chain of doors by holding the player on each door's walk-on tile
(the `.MAP` kind-1 gate-1 records `plan_walk_route.py` reads). The door's own
record runs with its gates and its flag writes, so a door the story has not
opened refuses and ends the run; nothing is poked but the position, and the
scenario says so. It saves one state per scene it arrives in, once the pad
moves the player there.
`LEGAIA_REFUSE` sets how long a door gets before it counts as refused: a bed
or cutscene door runs a long record first. A door whose record tests a story
flag the source state lacks takes `LEGAIA_FLAGS_SET` / `LEGAIA_FLAGS_CLEAR`,
written once at the start, and the scenario names them in `ram_poked`.

A screen behind a conversation or a menu path takes
[`autorun_screen_walk.lua`](../../scripts/pcsx-redux/autorun_screen_walk.lua): a
`LEGAIA_STEPS` list of waits, presses, door or NPC pokes and saves, each stop
gated on the game mode or the sub-screen word, so a shop's lists come from a town
state and one run. A step that stays unmet ends the run with what it saved.

A harvested state is a candidate, not a corpus state, until:

1. its census row (`retail-compare --census` over a scratch manifest naming
   it) shows the acting seat, category and queued id its label will claim;
2. its retail frame, from a scratch `retail-compare --images` run, shows that
   attacker and, for a cast, the spell's name banner;
3. it is backed up with `manage-states.py backup` and its `[[scenarios]]`
   block names the source state, the probe and the vsync, and any poke it
   inherits.

The scores of that scratch run are what the state adds to the corpus; the
baseline takes them at the next reviewed bless.

## See also

- [retail-compare](retail-compare.md) - the corpus this report measures.
- [pcsx-redux-automation](pcsx-redux-automation.md) - how a new retail state is
  captured and registered.
- [full-game-ladder](full-game-ladder.md) - the story-order anchors a capture
  can start from.
