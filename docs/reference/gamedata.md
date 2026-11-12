# Curated game-data tables

The repo ships a curated dataset of *Legend of Legaia* (NTSC-U) game
data - arts, magic, items, weapons, armor, accessories, enemies,
shops, casino prizes, fishing prizes, and character profiles -
mined from public walkthroughs and exposed as typed Rust accessors.

This page documents:

1. the source of every table,
2. the on-disk schema,
3. the cross-validation invariants the test suite enforces, and
4. how the rest of the codebase is expected to consume it.

## Source attribution

Three GameFAQs walkthroughs supply the raw values:

- Tan Yong Hua, "Legend of Legaia Walkthrough" v6.6 (1999).
- Psycho Penguin (mcfaddendaman), "Legend of Legaia Walkthrough"
  (2001) - the more complete table dump (PSM Magazine attribution
  for the enemy drop / steal table).
- Meth962, "Legend of Legaia 100% Walkthrough" v1.11 (2015, FAQ
  53721) - the [`enemies.toml`](../../data/gamedata/enemies.toml)
  per-enemy stat columns, the [`bosses.toml`](../../data/gamedata/bosses.toml)
  per-fight moveset / rewards layer, the Seru-magic
  `absorb_lv1`/`lv2`/`lv3` fields, and the magic-leveling tables in
  `legaia_gamedata::magic_leveling`.

The music-track table is contributed directly by Stann0x rather than
mined; see [`music-tracks.md`](music-tracks.md). So are the scene
identifications behind the scene-name table; see
[`scene-names.md`](scene-names.md).

Only the *factual* columns are committed (item names, prices, art
command sequences, MP costs, monster locations, drop tables). No prose
passages from any guide are reproduced in the repo - lawyer-friendly,
since prices and stats are not independently copyrightable but the prose
is.

Psycho Penguin's guide is the primary source for the item / art / shop
columns; Tan Yong Hua fills in gaps and contributes the boss-HP
estimates from its Master Course commentary. Meth962's stat columns
carry more weight than either where they overlap: they were extracted
from in-RAM memory dumps rather than estimated from play, which is why
they are the ones the binary records get cross-validated against.

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
  armor.toml          - 51 armor / helmet / shoes entries (body 20, head 15,
                        footwear 16)
  accessories.toml    - 70+ accessories with structured effect classes
  enemies.toml        - 177 enemies with location + drop / steal (and
                        `steal_chance`, the steal % sourced byte-exact from
                        the SCUS_942.54 steal table DAT_80077828) + full
                        per-enemy stat columns (HP / MP / EXP / Gold /
                        ATK / SPD / UDF / LDF / INT / AGL / element)
  bosses.toml         - 22 main-story B-code boss rows over 20 distinct
                        meth_ids (B000C is shared by the three Delilas
                        siblings) + Lapis + 7 Muscle Dome rounds;
                        per-fight named attacks + MP cost,
                        XP / gold / item rewards, recommended level
  shops.toml          - per-town shop inventories with item-key references
  casino.toml         - Sol/Vidna slot prizes + Muscle Dome courses + Baka Fighter
  sol_tower.toml      - Sol Tower floor map + side-quest chains
  fishing.toml        - Vidna/Buma fishing pond prizes
  characters.toml     - Vahn / Noa / Gala affinities and weapon classes
  music.toml          - 81-track BGM disambiguation (debug id + title /
                        in-game context / OST title / relocalization);
                        contributed by Stann0x (see music-tracks.md)
  scenes.toml         - display name per CDNAME scene + the disc's banner
                        + Stann0x's reading (see scene-names.md)
```

Implementation: [`crates/gamedata`](../../crates/gamedata).

## Schema highlights

### Arts

Each art has a `command` (player-facing input tokens: `Arms`,
`Ra-Seru`, `High`, `Low`) **and** a `directions` array of raw
direction bytes (`1=L, 2=R, 3=D, 4=U`) - exactly the prefix that
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

An armour row carries the equipment record's five stat bonuses: `udf` /
`ldf` always, and `atk` / `spd` (footwear) and `int` (helmets) where
non-zero. The walkthrough these were mined from printed two numbers per
piece, which for helmets and footwear were not UDF and LDF.

`accessories.toml` carries an informal `effect_class` taxonomy
(`hp_max_pct`, `ap_accrual_pct`, `mp_cost_pct`, `attack_pct`,
`speed_pct`, `revive_once`, `protect_status`, `elemental_def`,
`summon_seru`, …). This is *not* retail data - it's a structured
re-encoding of the walkthrough effect text designed for the engine
to dispatch on. As individual cheats / battle-formula reverse-
engineering pins exact retail mechanics, `effect_class` rows can
be promoted to retail-confirmed data.

### Enemies

Per enemy: `name`, `location`, optional `element` (absent for the
neutral element), optional `boss = true`, optional `drop` / `steal` item
keys with the `steal_chance` percent, and the stat columns. The
`enemy_drop_and_steal_keys_resolve` test asserts every drop/steal target
exists in the item tables; `enemy_stats_vs_disc` pins every field to the
monster record.

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
  `equip` is one of `Vahn` / `Noa` / `Gala`.
- Every item `category` is one of `consumable` / `permanent_stat`
  / `key` / `art_book` / `fishing_lure`.

### Disc-gated cross-checks (skip when the extracted disc files are absent)

These join the curated tables to the disc record each field comes from, so
the disc is the tie-breaker when the two disagree. A disagreement is fixed in
the TOML (with a comment naming the disc source), not allow-listed; the few
rows with no disc counterpart are named in the test that skips them.

- `arts_scus_oracle` - every regular, hyper and miracle art joins the SCUS
  arts-name table (`DAT_80075EC4`) at `index = action_constant - 0x1B`:
  name, AP, directions and the miracle flag. Every Super Art joins the
  trigger table by finisher: its directions are the input the retail
  tokenizer (`legaia_art::derive_super_input`) turns into the `find`
  pattern, and its AP is the sum of the chained arts' AP. Each Miracle Art
  carries the trigger table's command. Every SCUS row has a curated art.
- `magic_vs_disc` - every spell joins the SCUS spell table (`Juggernaut` is
  id `0x99`, "Evil Seru Magic"): MP, target shape, the element plate its
  name opens with, and each Seru's Lv1 / Lv2 / Lv3 absorb chance (the catch
  rate of the monster records carrying its Seru id).
- `item_prices_vs_disc` - every item, weapon, armour and accessory joins a
  disc item id (except the walkthrough's "Soren Book of Arts", which names
  no item) at the item record's `+2` price, an absent curated price meaning
  `0`; consumables' "Restores N HP / MP" text is the item-effect table's
  amount.
- `equip_slots_vs_disc` - the equipment record (`DAT_80074F68`) slot
  categories map one-to-one to the curated slots, and every weapon and
  armour piece carries the record's five bonuses (`int`, `atk`, `udf`,
  `ldf`, `spd`), wearer mask and slot. See
  [`equipment-table.md`](../formats/equipment-table.md).
- `accessory_passives_vs_disc` - every accessory joins and its structured
  `effect_class` is the passive index its descriptor carries.
- `enemy_stats_vs_disc` - every enemy joins its monster record (PROT 0867;
  the archive's `$2` / `$3` level suffix reads as `Lv2` / `Lv3`, and the
  multi-record bosses are pinned by id). The curated stats are the
  **in-battle** stats: the record through the battle loader's
  **scripted-fight** boost (`FUN_80054cb0`; `udf` / `ldf` x2, `atk` +1/4,
  `intel` +1/8, `hp` / `mp` / `spd` / `agl` as recorded; a random encounter
  installs the other profile, see `MonsterRecord::battle_stats_random`).
  `exp` is the record's EXP less a quarter and `gold` the payout for one
  enemy without the Golden Book (`h - (h >> 1)`, `h = gold >> 1`); element,
  drop, steal item and steal chance are the record's and the SCUS steal
  table's. All exact. The regional difference (the JP and PAL executables
  install the record unboosted) was first surfaced by **Zetopheonix**; see
  [`battle.md`](../subsystems/battle.md#monster-record-source-layout) and
  [`battle-formulas.md`](../subsystems/battle-formulas.md#victory-spoils-rewards).
- `bosses_vs_disc` - each boss row's HP, MP, 100% drop, gold payout and
  attack MP costs (the spell table). The rewards a story script hands out
  instead of the spoils (Gaza's Astral Sword, Tetsu's Mettle Gem and gold)
  are named in the test; `exp_reward` is a per-member share and is not
  pinned.
- `muscle_dome_vs_disc` - the arena ladder (PROT 0977 `0x801D1920`) fixes
  every course line-up and round; each `[[muscle_dome_boss]]` stat block is
  its record under the scripted-fight boost (a dome leg is a scripted
  fight), with the record's element, drop, steal, gold payout and catch
  rate.
- `shop_inventory_vs_disc` - every curated shop matches a gold-shop stock
  record (op `0x49` sub-op `0`, `legaia_scene_formats::shop_stock`), found in
  a scene MAN or a raw field-script carrier (Drake Castle, Usha), in
  **on-screen order**: the buy list hoists the record's last three slots -
  the Platinum Card stock - to the top, and those rows are the curated
  `featured` list. Every disc shop is curated except Soru's Bakery (only
  Soru Bread); Karisto's "Honest Person's Store" has no stock record.
- `casino_prizes_vs_disc` - the coin prize-exchange table in PROT 0899
  (`[u16 item_id][u16 story_gate][u32 coin_price]` records in `0x60`-byte
  blocks; block 1 Vidna, block 0 the Sol Tower counter) and the curated
  prizes join both ways on (item, price); the Earth Egg @ 100000 is a
  separate hidden exchange.
- `characters_vs_disc` - each character's element and strong / weak lists
  are the battle overlay's element tables (PROT 0898): the cell of the
  affinity matrix down the character's element column below / above 100.
- `music_vs_disc` - each track's `id` and `debug_title_jp` are its row of the
  music test in PROT 0974 ([`music-tracks.md`](music-tracks.md)).
- `fishing_exchange_real` (in `legaia-minigame-formats`) - the curated
  fishing prizes are the two venues' exchange rows.
- `scene_name_table_banners_match_the_disc` (in `legaia-patcher`) - each
  scene's `banner` is its MAN section-2 name.

## Library API

```rust
use legaia_gamedata::{Database, Character, ArtKind, SpellFamily};

let db = Database::load();

// Per-character arts
for art in db.arts_for(Character::Vahn) {
    println!("{:?} {:>3} AP  {}", art.kind, art.ap, art.name);
}

// Look up an art by direction-byte sequence
let art = db.find_art_by_directions(Character::Vahn, &[1, 2, 1]).unwrap();
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
cargo run -p legaia-gamedata --bin gamedata-tool -- arts-by-command Vahn "Arms,Ra-Seru,Arms"
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
  display names - anchoring the offsets that the
  `battle-formulas` reverse-engineering pins for damage / MP-cost
  arithmetic.
- **Inventory item-modifier values** seen in cheat databases will
  one day map to the byte IDs in `0x80085958..0x80085A40`. The
  gamedata's stable snake_case keys give every item a permanent
  identifier so when that mapping does land, it can be one extra
  TOML column without churning every consumer.
- **Enemy drop / steal tables** encode part of the retail combat
  data - once the per-encounter table lives at a known RAM offset,
  the gamedata enables a sanity-check that the parsed bytes match
  the published drop list.
- **Per-enemy stat columns** + **per-boss stat blocks** ground the
  damage-formula reverse engineering in
  [`docs/subsystems/battle-formulas.md`](../subsystems/battle-formulas.md):
  if the formulas predict the player needing N hits to defeat Jette
  at level 50, that result has to land near Meth's HP-34,567 /
  ATK-277 / UDF-412 / SPD-274 stat block. Same direction for the
  monster-stat records `crates/actor-formats/src/monster_archive.rs` lifts
  from PROT 0867 - the gamedata stat columns are the labels.
- **Per-Seru absorption rates** ground the capture-roll reverse
  engineering: a kill against a Gimard rolls capture at 55% per
  attempt before any Ivory Book modifier; the engine's
  `record_capture` path in
  [`crates/engine-battle/src/seru_learning.rs`](../../crates/engine-battle/src/seru_learning.rs)
  looks the rate up via `legaia_gamedata::Spell::absorb_lv1`.
- **Magic XP curve** + **damage scaling tables** in
  `legaia_gamedata::magic_leveling` close the loop for the
  per-cast levelling code path - once the engine routes a successful
  cast through `xp_single_target` / `xp_multi_target` and bumps a
  level when `XP_TO_LEVEL[N]` is crossed, the `SCALING_BP[N]`
  multiplier kicks in on the next cast.

## See also

- [Cheat databases](cheats.md) - the GameShark / Mednafen `.cht`
  pipeline that already pins many of these RAM offsets.
- [Per-character save record](../formats/save-record.md) - the
  `0x414`-byte runtime record cheats and gamedata both anchor on.
- [Art data format](../formats/art-data.md) - the on-disc Art
  Record layout the gamedata `directions` field cross-validates.
- [Music-track disambiguation](music-tracks.md) - the sibling
  curated table this crate also exposes (the `music` accessors).
- [Scene names](scene-names.md) - the scene-name table this crate
  also exposes (`scene_names` / `scene_by_id`).
