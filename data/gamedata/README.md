# `data/gamedata/`

Curated TOML tables of *Legend of Legaia* (NTSC-U) game data, mined
from third-party walkthroughs:

1. Tan Yong Hua, "Legend of Legaia Walkthrough" v6.6 (1999).
2. Psycho Penguin (mcfaddendaman), "Legend of Legaia Walkthrough" (2001).
3. Meth962, "Legend of Legaia 100% Walkthrough" v1.11 (2015) - the
   `enemies.toml` per-enemy stat columns, the `bosses.toml` per-fight
   moveset / rewards layer, the Seru-magic `absorb_lv1`/`lv2`/`lv3`
   fields, and the magic-leveling tables in `legaia_gamedata::magic_leveling`.

Both are public GameFAQs guides; we keep the *factual* data only (item
names, prices, art command sequences, MP costs, monster locations). No
prose passages from either guide are committed; that would be a
copyright issue. Only the tables are mined and cross-validated between
the two sources, with the table.rs canonical names from
[`crates/art`](../../crates/art) used as the authority for arts where
they overlap.

Why this exists: the existing reverse-engineering work has the raw
record bytes (PROT entry `0x05C4` for Arts, RAM `0x80160EFC+` etc.)
but no human-readable labels for them and no public reference to
cross-validate the parsed numeric fields against. These TOML files
close that gap so:

- Art records parsed from the binary can be sanity-checked against
  retail AP costs and command sequences.
- Inventory item IDs that appear as cheat-modifier values can be
  resolved to display names.
- Spell records can be linked to their MP / element / target type.
- Monster encounter records can be linked to known retail names.

The [`legaia-gamedata`](../../crates/gamedata) crate parses these
files at compile time with `include_str!` + `toml::from_str` and
exposes typed accessors.

## Files

| File | Coverage |
|---|---|
| `arts.toml` | Per-character arts (regular / hyper / super / miracle) with command sequences (Arms / Ra-Seru / High / Low → raw direction bytes) and AP costs |
| `magic.toml` | 21 Seru spells + 8 Ra-Seru summons (name, MP, element, attack name, target shape); Seru spells additionally carry per-level absorb-chance fields |
| `items.toml` | Consumables, key items, and special items (name, price, effect text) |
| `weapons.toml` | Weapons (name, price, attack stat, primary user, alternate users) |
| `armor.toml` | Body armor, helmets, shoes (name, type, price, UDF, LDF, equip restriction) |
| `accessories.toml` | Accessories (name, price, effect text + structured effect class where pinned) |
| `enemies.toml` | Enemies with location, drop / steal, and full per-enemy stat columns (HP / MP / EXP / Gold / ATK / SPD / UDF / LDF / INT / AGL / element) |
| `bosses.toml` | Per-fight boss summaries: named attacks + MP cost, XP / gold / item rewards, recommended party level (18 main-story bosses + Lapis + Muscle Dome rounds) |
| `shops.toml` | Per-town shop inventories (Rim Elm through Conkram); each entry references an item key |
| `casino.toml` | Sol/Vidna slot prizes + Muscle Dome courses |
| `fishing.toml` | Vidna/Buma fishing pond prizes |
| `music.toml` | Music-track disambiguation: every BGM cue across its four naming spaces (debug sound-test id + title / in-game context / official OST title / proposed relocalization). Contributed by Stann0x; see [`docs/reference/music-tracks.md`](../../docs/reference/music-tracks.md) |

## Cross-validation invariants

Tests in [`crates/gamedata/tests`](../../crates/gamedata/tests/)
enforce:

- Every `arts.toml` entry's name resolves to an action constant
  through `legaia_art::tables` (or is annotated as a non-action-table
  art, e.g. some Hyper Arts that share an action constant).
- Every `weapons.toml` / `armor.toml` / `accessories.toml` `equip`
  field is one of `Vahn`, `Noa`, `Gala`, `Any`, or `None`.
- Every shop inventory item key matches a row in `items.toml`,
  `weapons.toml`, `armor.toml`, or `accessories.toml`.
- Magic table covers exactly 21 Seru + 8 Ra-Seru entries.

Run with `cargo test -p legaia-gamedata`.

## License of the data

The names, prices, and stats are factual game data, not creative
expression - they aren't independently copyrightable. The walkthroughs
are credited above. Sony-owned bytes (asset data, executable, raw
ROM bytes) remain out of this repo entirely.
