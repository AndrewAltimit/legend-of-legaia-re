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
| menu screens | the pause-menu sub-screens, the title family (word clear) and the prize exchange `0x20` | a menu state's sub-screen word `DAT_801E46A4` |

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

## See also

- [retail-compare](retail-compare.md) - the corpus this report measures.
- [pcsx-redux-automation](pcsx-redux-automation.md) - how a new retail state is
  captured and registered.
- [full-game-ladder](full-game-ladder.md) - the story-order anchors a capture
  can start from.
