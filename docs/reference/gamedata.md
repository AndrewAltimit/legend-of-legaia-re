# Curated game-data tables

The repo ships a curated dataset of *Legend of Legaia* (NTSC-U) game
data — arts, magic, items, weapons, armor, accessories, enemies,
shops, casino prizes, fishing prizes, and character profiles —
mined from public walkthroughs and exposed as typed Rust accessors.

This page documents:

1. the source of every table,
2. the on-disk schema,
3. the cross-validation invariants the test suite enforces, and
4. how the rest of the codebase is expected to consume it.

## Source attribution

Two GameFAQs walkthroughs supply the raw values:

- Tan Yong Hua, "Legend of Legaia Walkthrough" v6.6 (1999).
- Psycho Penguin (mcfaddendaman), "Legend of Legaia Walkthrough"
  (2001) — the more complete table dump (PSM Magazine attribution
  for the enemy drop / steal table).

Only the *factual* columns are committed (item names, prices, art
command sequences, MP costs, monster locations, drop tables). No
prose passages from either guide are reproduced in the repo —
lawyer-friendly, since prices and stats are not independently
copyrightable but the prose is. Walkthrough 2 is the primary
source; walkthrough 1 fills in gaps and contributes the boss-HP
estimates from its Master Course commentary.

Sony-owned bytes (asset data, executable, raw ROM bytes) remain
out of the repo entirely.

## Where the data lives

```
data/gamedata/
  README.md           - source attribution + cross-validation rules
  arts.toml           - per-character arts (regular/hyper/super/miracle)
  magic.toml          - 21 Seru spells + 8 Ra-Seru summons
  items.toml          - consumables, key items, art books, fishing tackle
  weapons.toml        - 27 weapons
  armor.toml          - 49 armor / helmet / shoes entries
  accessories.toml    - 70+ accessories with structured effect classes
  enemies.toml        - 130+ enemies with location + drop / steal table
  bosses.toml         - boss HP estimates (sanity-check data)
  shops.toml          - per-town shop inventories with item-key references
  casino.toml         - Sol/Vidna slot prizes + Muscle Dome courses + Baka Fighter
  sol_tower.toml      - Sol Tower floor map + side-quest chains
  fishing.toml        - Vidna/Buma fishing pond prizes
  characters.toml     - Vahn / Noa / Gala affinities and weapon classes
```

Implementation: [`crates/gamedata`](../../crates/gamedata).

## Schema highlights

### Arts

Each art has a `command` (player-facing input tokens: `Arms`,
`Ra-Seru`, `High`, `Low`) **and** a `directions` array of raw
direction bytes (`1=L, 2=R, 3=D, 4=U`) — exactly the prefix that
ends up in the on-disc Art Record per
[`docs/formats/art-data.md`](../formats/art-data.md). The mapping
between the two is per-character because Noa is left-handed:

| Token | Vahn / Gala | Noa |
|---|---|---|
| `Arms`    | `L = 1` | `R = 2` |
| `Ra-Seru` | `R = 2` | `L = 1` |
| `High`    | `U = 4` | `U = 4` |
| `Low`     | `D = 3` | `D = 3` |

Tests in [`crates/gamedata/tests/data_files.rs`](../../crates/gamedata/tests/data_files.rs)
re-derive the bytes from the tokens and check they match.

`action_constant` (`0x1B..=0x32`) cross-references the per-character
art name table in [`crates/art/src/tables.rs`](../../crates/art/src/tables.rs).
The test `arts_action_constants_match_legaia_art_tables` enforces
this.

### Magic

21 Seru spells + 8 Ra-Seru summons. Element is lowercase (`fire`,
`water`, `earth`, `wind`, `thunder`, `light`, `dark`, `evil`).
`target` is one of `single_enemy`, `all_enemies`, `single_ally`,
`all_allies`, `self`.

### Items / weapons / armor / accessories

Each row has a stable snake_case `key` used by `shops.toml`,
`casino.toml`, `fishing.toml`, and `enemies.toml`. The
`shop_inventory_keys_resolve` test asserts every reference resolves.

`accessories.toml` carries an informal `effect_class` taxonomy
(`hp_max_pct`, `ap_accrual_pct`, `mp_cost_pct`, `attack_pct`,
`speed_pct`, `revive_once`, `protect_status`, `elemental_def`,
`summon_seru`, …). This is *not* retail data — it's a structured
re-encoding of the walkthrough effect text designed for the engine
to dispatch on. As individual cheats / battle-formula reverse-
engineering pins exact retail mechanics, `effect_class` rows can
be promoted to retail-confirmed data.

### Enemies

Per enemy: `name`, `location`, optional `element` (for elemental
Seru enemies), optional `boss = true`, and optional `drop` /
`steal` item keys. The `enemy_drop_and_steal_keys_resolve` test
asserts every drop/steal target exists in the item tables.

### Shops

One `[[shop]]` per merchant. Inventories reference item keys; the
`gamedata-tool shop <town>` CLI joins those keys against the four
item tables and prints a fully-priced inventory.

### Casino + Muscle Dome

`casino.toml` carries three concept families:

- `[[slot_prize]]` rows for the Vidna casino counter and the Sol
  Tower Muscle Dome prize-exchange counter (`location = "Sol"`
  is the F4 counter, *not* the slot machine itself - slot machines
  only pay coins).
- `[[muscle_dome_course]]` rows (Beginner / Expert / Master) with
  entry fee, clear reward, and `restrictions` / `allowed` arrays
  capturing the equipment / item / magic gating. Master Course's
  `reward_first_clear = "war_god_icon"` requires Jette to have
  been defeated in Absolute Fortress. The flat `enemies = [...]`
  field is the encounter-order roster as plain strings.
- `[[muscle_dome_round]]` rows pin the round-by-round assignment
  for each course as a normalised `(course_key, round, boss_key,
  seru_level)` table. Beginner and Expert have 8 rows each;
  Master has 13 rows (the longest progression).
- `[[muscle_dome_boss]]` rows hold the full per-enemy stat block
  (HP / MP / ATK / UDF / LDF / `intelligence` / SPD / AGL / XP /
  `gold`), drop and steal items with chance percentages, attack
  list, immunity tags, element + weakness/strength arrays, plus
  `wiki_path` for provenance back to the Fandom source. Seru
  enemies (`kind = "seru"`) carry their Lv1 / Lv2 / Lv3 stats as
  nested `[[muscle_dome_boss.seru_level]]` blocks; the round
  table's `seru_level` field selects which block applies.
- `[baka_fighter_meta]` records the all-rounds-clear reward
  (`reward_coins = 460`) and the rule sketch. Per-round button
  sequences live as `[[baka_fighter]]` rows.
- `[[muscle_paradise_secret]]` records the Chicken King easter
  egg ("run from the first battle in all three difficulties").

### Sol Tower

`sol_tower.toml` is location-scoped data that doesn't fit any of
the type-scoped tables: a per-floor map (`[[floor]]` rows with
named sections) and the side-quest chains (`[[side_quest]]` rows
with ordered step lists and reward pointers). The
`scene_label = "town0d"` ties the data back to the CDNAME map in
`site/_gen.py` and the field-VM bundle.

## Cross-validation invariants

Run with `cargo test -p legaia-gamedata`. Enforced rules:

- Every art's `action_constant` resolves through
  `legaia_art::tables::art_name`, and the resulting canonical name
  matches the gamedata `name` modulo `(Hyper)` / `(Miracle)` / `1`/`2`/`3`
  suffix variants.
- Every art's direction-byte sequence is in `1..=4` and equals the
  per-character mapping of its `command` token list.
- AP costs align with kind: regulars `≤ 36`, hypers `30..=70`,
  supers `48..=72`, miracles `= 99`.
- Each character has exactly one Miracle Art.
- Magic table contains exactly 21 Seru + 8 Ra-Seru entries; every
  `element` is one of the eight canonical values.
- Every shop / slot / fishing / muscle-dome reference resolves to a
  row in `items.toml` / `weapons.toml` / `armor.toml` /
  `accessories.toml`.
- Every enemy `drop` / `steal` key resolves.
- Every armor `slot` is one of `armor` / `helmet` / `shoes`; every
  `equip` is one of `Vahn` / `Noa` / `Gala` / `None`.
- Every item `category` is one of `consumable` / `permanent_stat`
  / `key` / `art_book` / `fishing_lure`.

## Library API

```rust
use legaia_gamedata::{Database, Character, ArtKind, SpellFamily};

let db = Database::load();

// Per-character arts
for art in db.arts_for(Character::Vahn) {
    println!("{:?} {:>3} AP  {}", art.kind, art.ap, art.name);
}

// Look up an art by direction-byte sequence
let art = db.find_art_by_directions(Character::Vahn, &[1, 2, 4]).unwrap();
assert_eq!(art.name, "Hyper Elbow");

// Map an action constant to AP cost (for runtime damage tooling)
let ap = db.ap_cost_for_action(Character::Gala, 0x1B).unwrap();
assert_eq!(ap, 99); // Biron Rage

// Resolve a shop entry against the unified item lookup
for entry in db.shop_inventory("Rim Elm", "Variety Shop").unwrap() {
    println!("  {:>20}  {:>5}G", entry.name, entry.price.unwrap_or(0));
}
```

## CLI

```bash
cargo run -p legaia-gamedata --bin gamedata-tool -- list arts --character Noa
cargo run -p legaia-gamedata --bin gamedata-tool -- list magic --element light
cargo run -p legaia-gamedata --bin gamedata-tool -- find "Tornado Flame"
cargo run -p legaia-gamedata --bin gamedata-tool -- arts-by-command Vahn "Arms,Ra-Seru,High"
cargo run -p legaia-gamedata --bin gamedata-tool -- shop "Sol"
cargo run -p legaia-gamedata --bin gamedata-tool -- dump-json arts > arts.json
```

## How this helps decompilation

The walkthrough data acts as ground-truth labels for the binary
records being reverse-engineered:

- **Art records** in PROT entry `0x05C4` (per-character RAM tables
  at `0x80160EFC` / `0x80176998` / `0x8018BA54`) parse into
  command-byte sequences. The gamedata supplies the human name
  for each sequence so `crates/art` extracted output can be
  validated against retail.
- **Spell records** can be linked to MP costs, elements, and
  display names — anchoring the offsets that the
  `battle-formulas` reverse-engineering pins for damage / MP-cost
  arithmetic.
- **Inventory item-modifier values** seen in cheat databases will
  one day map to the byte IDs in `0x80085958..0x80085A40`. The
  gamedata's stable snake_case keys give every item a permanent
  identifier so when that mapping does land, it can be one extra
  TOML column without churning every consumer.
- **Enemy drop / steal tables** encode part of the retail combat
  data — once the per-encounter table lives at a known RAM offset,
  the gamedata enables a sanity-check that the parsed bytes match
  the published drop list.
- **Boss HP estimates** ground the damage-formula reverse engineering
  in [`docs/subsystems/battle-formulas.md`](../subsystems/battle-formulas.md):
  if the formulas predict the player needing N hits to defeat Jette
  at level 50, that result has to fall in the walkthrough's
  `33,200..=35,800` HP bracket.

## See also

- [Cheat databases](cheats.md) — the GameShark / Mednafen `.cht`
  pipeline that already pins many of these RAM offsets.
- [Per-character save record](../formats/save-record.md) — the
  `0x414`-byte runtime record cheats and gamedata both anchor on.
- [Art data format](../formats/art-data.md) — the on-disc Art
  Record layout the gamedata `directions` field cross-validates.
