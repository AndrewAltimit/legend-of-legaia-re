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
  enemies.toml        - 177 enemies with location + drop / steal (and
                        `steal_chance`, the steal % sourced byte-exact from
                        the SCUS_942.54 steal table DAT_80077828) + full
                        per-enemy stat columns (HP / MP / EXP / Gold /
                        ATK / SPD / UDF / LDF / INT / AGL / element)
  bosses.toml         - 18 main-story B-code bosses + Lapis + 7 Muscle
                        Dome rounds; per-fight named attacks + MP cost,
                        XP / gold / item rewards, recommended level
  shops.toml          - per-town shop inventories with item-key references
  casino.toml         - Sol/Vidna slot prizes + Muscle Dome courses + Baka Fighter
  sol_tower.toml      - Sol Tower floor map + side-quest chains
  fishing.toml        - Vidna/Buma fishing pond prizes
  characters.toml     - Vahn / Noa / Gala affinities and weapon classes
  music.toml          - 81-track BGM disambiguation (debug id + title /
                        in-game context / OST title / relocalization);
                        contributed by Stann0x (see music-tracks.md)
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

Seru spells additionally carry `absorb_lv1` / `absorb_lv2` /
`absorb_lv3` integer-percent fields - the per-encounter chance a kill
rolls capture success against the Lv1 / Lv2 / Lv3 mist variants of
the source Seru. Source: Meth962 v1.10 Seru-magic table (range 1-80%;
Gimard easiest at 55/60/80, Gilium hardest at 1/1/1). Ra-Seru summons
are egg-derived and don't have an absorption mechanic.

The magic-levelling curve (XP-to-level, damage / heal scaling per
level) lives in [`legaia_gamedata::magic_leveling`](../../crates/gamedata/src/lib.rs):

- `XP_TO_LEVEL`: 18, 51, 93, 145, 209, 289, 393, 537 (cumulative cost
  Lv2 -> Lv9).
- `SCALING_BP`: damage / heal multiplier in basis points
  (Lv1 = 10000 / +0%, Lv9 = 20000 / +100%).
- `VERA_HEAL` / `ORB_HEAL` / `SPOON_HEAL`: per-level healer outputs
  (Vera 256-512 HP, Orb 512-1024 AoE HP, Spoon 1024-2048 AoE HP).
- `NIGHTO_HIT_PCT`: Nighto's Confuse / Death roll base hit-chance per
  level (Confuse:Death ratio is fixed at 8:1 regardless of level).
- `xp_single_target` / `xp_multi_target`: bracket-resolvers that map
  a cast's damage-as-fraction-of-target-HP to the 0..=12 (single) or
  0..=4 (multi, per enemy) XP award per the FAQ tables.

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

### Disc-gated cross-checks (skip when `extracted/SCUS_942.54` is absent)

These validate the curated tables against the real executable, so the disc
is the tie-breaker when the two disagree:

- `item_prices_vs_disc` — every priced weapon / armor / accessory whose name
  resolves to a disc item id has a curated `price` equal to the authoritative
  `SCUS` shop-price field (the `u16` at `+2` of the item-property record). 119+
  cross-checks, **zero** mismatches. This oracle pinned three walkthrough price
  errors that were corrected to the disc values (Forest / Magic Amulet
  4000→2000, Evil Medallion 9998→9999).
- `equip_slots_vs_disc` — the disc equip-stat table's four `+7` slot categories
  map name-exactly to the four gamedata armour/weapon slots (body 20, head 15,
  footwear 16), and none of the 77 accessories appear in that table (they are a
  separate system). See [`equipment-table.md`](../formats/equipment-table.md).
- `enemy_stats_vs_disc` — joins `enemies.toml` to the monster-stat archive
  (`PROT 0867`) by name (unambiguous names only — multi-form bosses like Gaza
  are skipped). The curated bestiary stats are **scaled derivations** of the raw
  disc record, not copies, by fixed factors: `hp`/`spd` ×1, `udf`/`ldf` ×2,
  `atk` ×5/4, `exp` ×3/4, `gold` ×5/16 (all exact, ±1 on the fractional ones).
  Two curated labels are disc stats in disguise: curated `agl` **is** the disc
  *spirit* (SP) stat (×1), and curated `intel` is the disc *agility* stat ×9/8.
  So the disc is the raw ground truth; the test pins all nine fields across 120+
  enemies. See [`monster-animation.md`](../formats/monster-animation.md) and
  `legaia_asset::monster_archive`.
- `magic_vs_disc` — joins `magic.toml` to the static spell table in
  `SCUS_942.54` (`legaia_asset::spell_names`) by spell name. All **21** Seru
  spells (ids `0x81..=0x95`) and **7** of the 8 Ra-Seru summons (`0x9a..=0xa0`;
  the hidden `Juggernaut` isn't in the contiguous named region) name-join, and
  every one's **MP cost is byte-exact** against the disc `+2…+3` record. Target
  shape (`+2` byte) agrees for all joins except the revive Ra-Seru `Horn` /
  "Resurrector", whose byte is enemy-side though the effect revives all allies
  (checked explicitly). The oracle pinned one curated target error — `Mushura` /
  "Crazy Driver" is single-enemy, not all-enemies — corrected to the disc value.
  See [`spell-table.md`](../formats/spell-table.md).
- `shop_inventory_vs_disc` — scans every PROT entry for the gold-shop stock
  records embedded inline in each scene MAN (`legaia_asset::shop_stock`, op
  `0x49` sub-op `0`), decodes each record's sellable item ids to names, and
  joins disc shops to curated shops by the **item-name set** (order-independent;
  the item set is far more distinctive than the shop title, which repeats as
  "Arms Shop" / "Items Shop" across towns). Every located disc shop's stock
  matches a curated inventory as an exact set, with one documented disc-only
  exception — **Soru's Bakery**, which sells only the novelty "Soru Bread" and
  has no curated counterpart (asserted explicitly). This oracle pinned a curated
  item-name error: the Gala helmet the disc names **"Power Earring"** (singular)
  was curated as "Power Earrings", which broke the Wind Cave and
  Biron-after-mist joins until corrected to the disc spelling (the price oracle
  missed it — a name-mismatch is a tolerated miss there). The shop-record format
  is documented in `legaia_asset::shop_stock`.
- `casino_prizes_vs_disc` — joins `casino.toml` to the coin prize-exchange table
  in the menu/save/shop overlay's data segment (PROT entry 0899, stored raw; the
  canonical reader is `legaia_rando::casino::CasinoExchange`). Each prize is an
  8-byte record `[u16 item_id][u16 story_gate][u32 coin_price]` in `0x60`-byte
  blocks; block 1 is the Vidna casino counter, block 0 the Sol Tower Muscle Dome
  counter (high-value prizes story-gated via the `+2` word), and blocks 2/3 are
  short pre-progression states (one cheap healing item each). Every curated
  prize joins a disc record byte-exact on **(item name, coin price)** across both
  full lists, and every disc record in those lists is a curated prize — with one
  documented exception, **Earth Egg @ 100000 coins** (the Muscle Paradise
  "Chicken King" easter egg), a separate hidden exchange that is not in the
  four-block table (asserted explicitly).

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
- **Per-enemy stat columns** + **per-boss stat blocks** ground the
  damage-formula reverse engineering in
  [`docs/subsystems/battle-formulas.md`](../subsystems/battle-formulas.md):
  if the formulas predict the player needing N hits to defeat Jette
  at level 50, that result has to land near Meth's HP-34,567 /
  ATK-277 / UDF-412 / SPD-274 stat block. Same direction for the
  monster-stat records `crates/asset/src/monster_archive.rs` lifts
  from PROT 0867 - the gamedata stat columns are the labels.
- **Per-Seru absorption rates** ground the capture-roll reverse
  engineering: a kill against a Gimard rolls capture at 55% per
  attempt before any Ivory Book modifier; the engine's
  `record_capture` path in
  [`crates/engine-core/src/seru_learning.rs`](../../crates/engine-core/src/seru_learning.rs)
  looks the rate up via `legaia_gamedata::Spell::absorb_lv1`.
- **Magic XP curve** + **damage scaling tables** in
  `legaia_gamedata::magic_leveling` close the loop for the
  per-cast levelling code path - once the engine routes a successful
  cast through `xp_single_target` / `xp_multi_target` and bumps a
  level when `XP_TO_LEVEL[N]` is crossed, the `SCALING_BP[N]`
  multiplier kicks in on the next cast.

## See also

- [Cheat databases](cheats.md) — the GameShark / Mednafen `.cht`
  pipeline that already pins many of these RAM offsets.
- [Per-character save record](../formats/save-record.md) — the
  `0x414`-byte runtime record cheats and gamedata both anchor on.
- [Art data format](../formats/art-data.md) — the on-disc Art
  Record layout the gamedata `directions` field cross-validates.
- [Music-track disambiguation](music-tracks.md) — the sibling
  curated table this crate also exposes (the `music` accessors).
