# legaia-randomizer

The randomizer's planning logic, shared by the disc patcher
([`legaia-patcher`](../patcher/README.md)) and the engine's run rules
([`legaia-engine-run-rules`](../engine-run-rules/README.md)). Every planner is
a pure function of its inputs and a seed; nothing here reads or writes a disc,
and the crate has no dependencies. The patcher applies a plan as same-size
disc edits, the engine applies its picks at the readers
([`docs/subsystems/run-rules.md`](../../docs/subsystems/run-rules.md)).

- `rng` - `SplitMix64`, the version-stable generator every seeded pass draws
  from, and `seed_from_str`, the one rule that turns a seed string into a
  number, so a published seed reproduces a run on any build and in both.
- `drops` - drop-table planning (`plan_drops`, `DropMode`: shuffle the
  existing drops or draw each from a pool); the steal pass reuses it.
- `monsters` - the monster-id safety lists: the tutorial enemies, the story
  bosses (every form), their union, and the scale-pinned sparring partner.
- `encounters` - the formation guards (`PROTECTED_FORMATION_IDS`,
  `MAX_DISTINCT_SPECIES`) and `reroll_formation`, a random formation's seats
  redrawn from a scene's pool under them.
- `items` - `ItemPools`: the sellable pool (priced items) and the kept set
  (named items priced zero - the quest and key items that never move).
- `sites` - order-independent picks keyed by the seed and a site (`site_rng`,
  `pick`, `reassign_item`), for a reader that meets its sites one at a time.
- `bands` - `band_stats`: a record's stats each moved inside a permille band,
  seeded per record, protected monsters untouched.
