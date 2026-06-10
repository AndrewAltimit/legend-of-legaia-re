# `legaia-gamedata`

Curated *Legend of Legaia* (NTSC-U) game-data tables exposed as typed
Rust accessors. The data lives in [`data/gamedata/`](../../data/gamedata/)
as TOML files; this crate parses them at compile time with `include_str!`
and surfaces them through a stable API.

## What's covered

| Module | Coverage |
|---|---|
| `arts` | Per-character regular / hyper / super / miracle arts: command sequences, raw direction bytes, AP costs, action constants. Cross-checked against `legaia_art::tables`. |
| `magic` | 21 Seru spells + 8 Ra-Seru summons: element, MP cost, attack name, target shape. |
| `items` | Consumables, key items, art books, fishing rods/lures with prices and effect text. |
| `weapons` | Weapons with attack stat, primary user, alternate users. |
| `armor` | Armor / helmets / shoes with UDF / LDF and equip restrictions. |
| `accessories` | Accessories with structured `effect_class` taxonomy where pinned. |
| `enemies` | Enemies with location, item drop, and steal item + `steal_chance` (the steal %, byte-exact from the `SCUS_942.54` steal table `DAT_80077828`). |
| `bosses` | Boss HP estimates from walkthrough commentary (sanity-check data, not retail). |
| `shops` | Per-town shop inventories (item key references the four item tables). |
| `casino` | Sol/Vidna slot prizes + Muscle Dome courses + Baka Fighter button patterns. |
| `fishing` | Vidna/Buma pond prize tables. |
| `characters` | Vahn / Noa / Gala affinities and favorite-weapon classes. |

## Source attribution

Data is mined from two public GameFAQs walkthroughs (Tan Yong Hua 1999,
Psycho Penguin 2001) and cross-validated. See
[`data/gamedata/README.md`](../../data/gamedata/README.md) for full
provenance. The curated tables are checked against the user's disc by a set
of disc-gated oracles (item prices, equipment slots, enemy stats, magic,
shop inventories, casino prizes) - each joins a curated table to its
authoritative on-disc source and asserts the numeric fields byte-exact; the
enumerated invariants are in
[`docs/reference/gamedata.md`](../../docs/reference/gamedata.md).

## Library API

```rust
use legaia_gamedata::{Database, Character, ArtKind};

let db = Database::load();

// Look up an art by exact command sequence
let art = db.find_art_by_directions(Character::Vahn, &[1, 2, 4]).unwrap();
assert_eq!(art.name, "Hyper Elbow");
assert_eq!(art.ap, 18);
assert_eq!(art.kind, ArtKind::Regular);

// Resolve a shop entry against the unified item lookup
for entry in db.shop_inventory("Rim Elm", "Variety Shop").unwrap() {
    println!("{:>20}  {:>6}G", entry.name, entry.price.unwrap_or(0));
}

// Spell metadata
let nighto = db.spell_by_name("Nighto").unwrap();
assert_eq!(nighto.element, "dark");
assert_eq!(nighto.mp, 13);
```

The `Database` is constructed once from baked-in TOML and is cheap to
clone; it owns no on-disc handles.

## CLI

```bash
cargo run -p legaia-gamedata --bin gamedata-tool -- list arts --character Vahn
cargo run -p legaia-gamedata --bin gamedata-tool -- list magic
cargo run -p legaia-gamedata --bin gamedata-tool -- find "Tornado Flame"
cargo run -p legaia-gamedata --bin gamedata-tool -- arts-by-command Noa "Low,High,Low,Ra-Seru,Arms"
cargo run -p legaia-gamedata --bin gamedata-tool -- shop "Rim Elm"
cargo run -p legaia-gamedata --bin gamedata-tool -- dump-json arts > arts.json
```

## Cross-validation tests

`cargo test -p legaia-gamedata` runs:

- Every TOML file parses.
- Every art `name` resolves to the canonical name in
  `legaia_art::tables::art_name(...)` (or is annotated as a derived
  variant like `"Tornado Flame (Hyper)"`).
- Every art `directions` byte is in `1..=4` and the `command` token
  list matches the byte sequence under the per-character mapping.
- Every shop inventory item key resolves through one of `items`,
  `weapons`, `armor`, `accessories`.
- Magic table has exactly 21 Seru + 8 Ra-Seru entries.
- Slot / fishing / Muscle Dome `item` references resolve.
