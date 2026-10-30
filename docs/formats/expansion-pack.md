# Expansion packs

An expansion pack is a modder-authored bundle of new content for the port - custom maps, battles and bosses, a level curve, quests, dialogue, trading cards, game modes - that the engine loads beside the player's own disc. It is the port's own format, not a disc format: nothing here is decoded from retail, so the page carries no confidence labels. The parser is `legaia_expansion_format` ([`crates/expansion-format`](../../crates/expansion-format/README.md)); the runtime that applies a pack is `legaia_engine_expansion`.

A pack carries **no bytes from the disc**. Every disc asset it uses is named by a coordinate (a [`DiscRef`](#disc-references)) and resolved against the disc the player supplies, the same rule a shareable [language pack](../tooling/translation/index.md) follows. The flagship pack, [`packs/rebirth/`](../../packs/rebirth/README.md), is written with exactly the format and tooling on this page.

## At a glance

| Property | Value |
|---|---|
| Source form | A directory: `pack.toml` plus optional section files, `maps/`, `assets/` |
| Shareable form | One `.lxpack` file ([bundle](#the-lxpack-bundle)) |
| Text encoding | UTF-8 TOML for every manifest, section and map |
| Schema version | `2` (`manifest::SCHEMA_VERSION`); schemas `1` and `2` load |
| Disc | NA release only, `SCUS-94254` |
| Parser | `ExpansionPack::from_files` (one parser for directory, bundle and browser input) |
| Writer | `ExpansionPack::to_files`, `bundle::write_bundle` |
| Validator | `validate::validate`, `validate::validate_files` |
| Sharing | `share::import_bundle` (the import gates), `diff::diff_packs` (versions compared) |
| CLI | `asset expansion validate / bundle / unbundle / info / diff`, `legaia-engine packs` |

## Layout

Paths are relative to the pack root with `/` separators. Every file but `pack.toml` is optional.

```text
pack.toml          manifest (required)
battle.toml        monsters, formations, encounter overrides, bosses
progression.toml   level curve, XP and rewards
quests.toml        quests and challenges
dialog.toml        dialogue lines
cards.toml         trading cards
cutscenes.toml     scripted scenes
modes.toml         game modes
items.toml         items, equipment, sets and shops
audio.toml         music tracks, sounds, music picks
arts.toml          Tactical Arts and their chains
spells.toml        Seru magic
maps/<id>.toml     one custom map per file; the file stem is the map id
assets/...         the modder's own files (art, models, sound, text)
README.md, LICENSE, LICENSE.md, LICENSE.txt, CHANGELOG.md
```

The loader reads the files above and ignores anything else; the validator reports an ignored file as a warning so a misspelt section name does not vanish silently. A section file that is absent is the section's default (empty). `maps/` is flat: `maps/a/b.toml` is not a map.

The whole tree is one `PackFiles` value (`BTreeMap<path, bytes>`). The directory walker, the bundle reader and the browser workbench each produce one, so a pack parses the same way whichever form it arrived in.

## The manifest

`pack.toml` is `manifest::PackManifest`. Unknown keys are an error, so a typo fails loudly.

| Key | Type | Required | Meaning |
|---|---|---|---|
| `id` | string | yes | Machine id, `[a-z0-9_-]`, at most 48 characters, starting with a letter or digit. Namespaces every content id in the pack. |
| `name` | string | yes | Display name. |
| `version` | string | yes | Semantic version, `MAJOR.MINOR.PATCH[-pre][+build]`. |
| `authors` | string list | no | Who made it. |
| `description` | string | no | What the pack adds. |
| `schema` | integer | no | Pack-format version the pack is written against; defaults to the current one. |
| `disc` | string | no | The disc the coordinates point into; defaults to `SCUS-94254`, the only value accepted. |
| `license` | string | no | Licence for the pack's own content: an SPDX expression or free text. |
| `homepage` | string | no | Where the pack is published, an `http(s)` URL. |
| `tags` | string list | no | Discovery tags, pack-id syntax. |
| `requires` | string list | no | Packs that must be loaded alongside this one: `id`, or `id@range` ([dependencies](#dependencies)). |
| `conflicts` | string list | no | Packs that must not be, in the same form; a range limits the conflict to those versions. |
| `cover` | string | no | The cover image: a PNG under `assets/`, written from the pack root (`assets/cover.png`). Schema 2. |
| `min_engine` | string | no | The oldest engine version the pack runs on, semver. Schema 2. |

A minimal manifest:

```toml
id = "tower"
name = "The Tower"
version = "0.1.0"
```

## Versions and compatibility

Three versions meet when a pack loads: the pack's own `version`, the **pack format** it is written in (`schema`), and the **engine** reading it (`manifest::ENGINE_VERSION`, the workspace version every engine crate shares).

| Gate | Passes when | Refusal |
|---|---|---|
| Pack format | `schema` is in `MIN_SCHEMA_VERSION..=SCHEMA_VERSION` (`1..=2`) | "written in pack format N, newer than this engine reads ... update the engine" |
| Engine | `min_engine` is absent, or not newer than `ENGINE_VERSION` | "needs engine X or newer; this engine is Y" |

`ExpansionPack::from_files` runs both gates first, on a lenient read of `pack.toml` (`manifest::ManifestPeek`), before the strict parse. A pack from a newer format may carry keys this build has never heard of, and `deny_unknown_fields` would otherwise report the first of them as a typo; the gate reports the real reason instead. Since every reader parses through `from_files` - the native `--expansion`, the launcher, the play page, the workbench - every host refuses the same way. Old packs keep loading: schema 2 only added optional keys, so a schema-1 pack is a valid schema-2 pack with them absent. The validator warns when a schema-1 manifest uses a schema-2 key, since an older engine would trip on it.

### Dependencies

A `requires` or `conflicts` entry is a pack id, optionally followed by `@` and a version range (`manifest::PackDep`). A range is one or more comparators, all of which must hold, separated by commas or spaces (`version::VersionReq`). The operators are cargo's:

| Entry | Matches |
|---|---|
| `base` | any version of `base` |
| `base@1.2.3`, `base@^1.2.3` | `>=1.2.3, <2.0.0`; below `1.0.0` the caret holds the leftmost non-zero part (`^0.2.3` is `<0.3.0`) |
| `base@~1.2.3` | `>=1.2.3, <1.3.0` |
| `base@=1.2`, `base@1.2.*` | any `1.2.x` |
| `base@>=1.2 <2` | both bounds; a missing part reads as the whole range of it |

A pre-release (`2.0.0-rc.1`) satisfies a range only when a comparator names a pre-release of the same `MAJOR.MINOR.PATCH`. Content ids of another pack (`base:thing`) resolve against the id part of the entry. `manifest::check_dependencies` checks a manifest against a list of installed `(id, version)` pairs and names each missing pack, out-of-range version and installed conflict.

## Content ids and namespacing

Content a section defines (a monster, a quest, a card, a map) has a local id: `[a-z0-9_]`, at most 48 characters, unique within its kind across the whole pack. The pack id is its namespace, so `tower_king` in pack `rebirth` is `rebirth:tower_king` to the engine and to other packs.

A reference to pack content takes one of three forms:

- `tower_king` or `rebirth:tower_king` (the pack's own id): must be defined somewhere in this pack.
- `other:thing`: content of pack `other`, which must be listed in `requires`. It is resolved by the engine once that pack is loaded, not by the validator.

A definition never carries a `:`; writing one is an error.

## Disc references

`refs::DiscRef` is the only way a pack names disc content. It is a TOML inline table tagged by `kind`:

```toml
boss_model = { kind = "monster", id = 77 }
arena      = { kind = "scene", name = "town01" }
mesh       = { kind = "tmd", prot = 874, index = 3 }
```

| `kind` | Fields | Coordinate space | Range checked without a disc |
|---|---|---|---|
| `monster` | `id` | 1-based monster record id in the PROT 867 archive ([battle.md](../subsystems/battle.md)) | `1..=186` |
| `scene` | `name` | CDNAME scene name ([scene-names.md](../reference/scene-names.md)) | `[a-z0-9_]`, at most 32 characters |
| `tmd` | `prot`, `index` | PROT extraction index ([prot.md](prot.md)), `index`th TMD object | `prot` in `0..=1232` |
| `tim` | `prot`, `index` | PROT extraction index, `index`th TIM in its pack | `prot` in `0..=1232` |
| `item` | `id` | Retail item id ([item-table.md](item-table.md)); a pack item is named by its key instead ([items](expansion-pack-items.md#item-references)) | the `u8` range |
| `bgm` | `id` | Sound-test id ([music-tracks.md](../reference/music-tracks.md)) | `0..=80` |
| `npc` | `scene`, `index` | A retail NPC placement: the scene's MAN partition-1 record `index` ([maps](expansion-pack-maps.md#npcs)) | scene-name syntax; `index` not `0` (the scene controller) |

PROT numbers are **extraction** indices, the `NNNN` in `extracted/PROT/NNNN_*.BIN`, not raw TOC indices ([cdname.md](cdname.md#numbering-space)).

Checking happens in two halves. `DiscRef::check_range` needs no disc and rejects a coordinate no NA disc can satisfy. `refs::DiscResolver` is the disc-backed half: the engine implements `resolve(&DiscRef) -> Result<(), String>` over the player's disc (does entry 874 hold a TMD with an object 3), and `validate::validate_with` runs every coordinate a section recorded through it.

## Sections

Each content section is a TOML file whose type lives in its own module of `legaia_expansion_format`. Every key in a section is optional (`#[serde(default)]`), so a pack writes only what it uses. The subsections below summarise each one; the battle, quest and map sections have pages of their own.

| File | Type | Module |
|---|---|---|
| `battle.toml` | `BattleSection` | `battle` |
| `progression.toml` | `ProgressionSection` | `progression` |
| `quests.toml` | `QuestSection` | `quest` |
| `dialog.toml` | `DialogSection` | `dialog` |
| `cards.toml` | `CardSection` | `cards` |
| `cutscenes.toml` | `CutsceneSection` | `cutscene` |
| `modes.toml` | `ModeSection` | `modes` |
| `items.toml` | `ItemSection` | `items` |
| `audio.toml` | `AudioSection` | `audio` |
| `arts.toml` | `ArtSection` | `arts` |
| `spells.toml` | `SpellSection` | `spells` |
| `maps/<id>.toml` | `MapDef` | `map` |

### battle.toml

Monsters derived from a retail record (set or scaled stats, drop, steal, element, spells), formations of up to four monsters, per-scene encounter overrides and named bosses. Formations compile to engine ids from `0xE000`; a once-only boss's victory is recorded in the save's expansion block. Declares `monster`, `formation` and `boss` ids. Reference: [`expansion-pack-battle.md`](expansion-pack-battle.md).

### progression.toml

The XP curve (retail, scaled, banded or explicit), EXP and gold multipliers, enemy level scaling and level-up item rewards. Declares no ids. Reference: [`expansion-pack-battle.md`](expansion-pack-battle.md).

### quests.toml

Quests (giver, prerequisites keyed to retail system flags, party level and other quests, ordered or unordered objectives, rewards, repeat rules, a battle or boss started on the quest's start) and challenges (rule sets over one battle with a score). Declares `quest` and `challenge` ids; references `quest`, `line`, `formation` and `boss`. Reference: [`expansion-pack-quests.md`](expansion-pack-quests.md).

### dialog.toml

Dialogue lines in the translation markup, as rows and pages, with two- to four-option menus that set pack flags, chain a line or start a fight. Every line is compiled at validation, so text with no glyph is an error. Declares `line` ids; references `line`, `formation` and `boss`. Reference: [`expansion-pack-quests.md`](expansion-pack-quests.md#dialogtoml).

### cutscenes.toml

Scripted scenes: step lists (walks, turns, clips, camera moves, fades, lines with branches on the option picked, flags, items, a closing fight) on a main track and named tracks beside it, run by the engine's own sequencer. A map trigger, a map's arrival list, a pack NPC or a quest's start or completion plays one; Start skips one already seen. Declares `cutscene` ids; references `line`, `formation`, `boss`, `quest` and `map`. Reference: [`expansion-pack-cutscenes.md`](expansion-pack-cutscenes.md).

### cutscenes.toml

Scripted scenes: step lists (walks, turns, clips, camera moves, fades, lines with branches on the option picked, flags, items, a closing fight) on a main track and named tracks beside it, run by the engine's own sequencer. A map trigger, a map's arrival list, a pack NPC or a quest's start or completion plays one; Start skips one already seen. Declares `cutscene` ids; references `line`, `formation`, `boss`, `quest` and `map`. Reference: [`expansion-pack-cutscenes.md`](expansion-pack-cutscenes.md).

### cards.toml

Trading cards, the boosters they come in, the dealers that sell them and the rules that hand them out. Card, booster and dealer ids are the section's own (cards keep hyphenated ids such as `mist-wisp`, which save files key on), so the section checks them itself rather than declaring pack-wide ids; it references `formation` and `boss`, and records each card's art and each monster reward source as a disc coordinate.

| Key | Meaning |
|---|---|
| `[[cards]]` | `id`, `name`, `flavor`, `rarity` (`common` / `uncommon` / `rare` / `legendary`), optional `element`, `sides = [top, right, bottom, left]` (each `1..=10`), `art` (a `DiscRef`; a monster is drawn as the card's portrait), optional `sell` (gold) |
| `[sell_prices]` | Gold per sold card by rarity; a card's own `sell` overrides; omitted, a built-in table |
| `starter` | Card ids handed over once, the first time the collection opens: a duel deck to begin with |
| `[[boosters]]` | `id`, `name`, `cards_per_pack`, `pool` (card ids; empty = every card), `weights` by rarity, `guaranteed` (the last slot's minimum rarity), `price` (a booster with no price is never sold) |
| `[[dealers]]` | `id`, `name`, `stock` (booster ids; empty = every priced booster), `buys_cards` (default true), `sell_percent` of a card's sell value (default 100) |
| `[[rewards]]` | `source`, then `booster` and / or `card`, `chance_percent` (default 100) and `count` (rolls, default 1) |

A reward `source` is one of:

| Source | Rolls when |
|---|---|
| `{ kind = "monster", id = N }` | A won fight seated retail monster `N`, once per seat - any fight while the pack is active, retail fights included. A pack monster fights on its retail base, so its base id is the key |
| `{ kind = "formation", id = F }` | The pack formation `F` was won |
| `{ kind = "boss", id = B }` | The pack boss `B` was beaten |
| `{ kind = "mode", id = M }` | The player won mode instance `M` (`modes.toml`) |

The validator rejects a repeated or empty id, a side outside `1..=10`, a booster pool, starter, reward or dealer stock naming a card or booster that does not exist, a booster whose weights are all zero, a reward with neither a booster nor a card, a chance over 100 and a mode source no `modes.toml` instance has; it warns on a dealer stocking an unpriced booster and on a dealer paying over the sell value. The runtime is in [`expansion-modes.md`](../subsystems/expansion-modes.md#trading-cards).

### modes.toml

Instances of the port's own game modes: a pack picks the numbers and the layout, and the rules live in `legaia_engine_modes` ([`expansion-modes.md`](../subsystems/expansion-modes.md)). Each instance has an `id` (the key a `cards.toml` reward source of `kind = "mode"` names), a display `name` and, where the mode is staged somewhere, an optional `location` (`DiscRef`).

| Table | Keys (defaults) |
|---|---|
| `[[poker]]` | `seats` (4, `2..=6` with the player), `buy_in` (1000), `small_blind` (10), `big_blind` (20), `currency` (`gold` / `coins`), `chip_value` (1), `ai_tier` (`novice` / `regular` / `expert`), `max_hands`, `blind_double_every` |
| `[[card_duels]]` | `deck` (the opponent's five `cards.toml` ids), `ai_tier`, `elemental_cells`, `chain`, `stakes`, `win_booster`, `map`, `spawn`, `[card_duels.table]` |
| `[[golf]]` | `[[golf.holes]]`, each `par` (3), `rows` (tile strings), `heights` (digit strings of the same shape), `max_strokes` (0 = `par + 5`) |
| `[[races]]` | `laps` (3), `points` (closed centerline, world units), `half_width` (64), `boost_segments`, `racers` (4), `ai_tier` |
| `[[prop_hunt]]` | `props` (`id`, `mesh`, `radius`), `decoys` (`prop`, `x`, `y`), `bounds`, `blockers`, `rounds` (3), `hunters` (1), `players` (4), `hide_frames`, `round_frames`, `hunter_health` (100), `miss_penalty` (10), `ai_tier` |
| `[[party_nights]]` | `order` (`fixed` / `shuffle`), `points` ([10, 6, 4, 2]), `last_place_picks` (false), `double_final` (true), `players` (4, a night alone's seats), `rivals` (names), `ai_tier`, `podium`, `[[party_nights.rounds]]`, `[[party_nights.prizes]]` |
| `podium` | `map` (a pack map), `tile` (the first-place block), `facing` (compass sector), `bodies` (map NPCs that play the rivals, in rival order): where a night played alone ends, in the world ([expansion-modes.md](../subsystems/expansion-modes.md#the-podium-in-the-world)) |
| `[[party_nights.rounds]]` | `mode` (an instance id), `name`, `hands`, `stack` (poker), `holes` (golf, 1-based), `laps` (race), `rounds` (prop hunt), `double`, `wager` (0..=50 percent) |
| `[[party_nights.prizes]]` | `place` (1-based), `gold`, `coins`, `boosters`, `cards` |

A golf hole's tile legend is one character per tile: `.` fairway, `,` rough, `s` sand, `w` water, `#` wall, space out of bounds, `T` tee, `O` cup, and `^` `v` `<` `>` fairway sloping downhill that way.

The validator checks the party nights (ids, each round naming an instance and only the knobs its kind takes, hole numbers, wager range, the prizes' boosters and cards) and the duel tables (the `map` exists, the `tile` is on it, the `facing` is a compass sector, every NPC seated is one of the map's), and nothing else of the section; the rules of a night are in [expansion-modes.md](../subsystems/expansion-modes.md#party-night). An instance is built when it is entered (`legaia_engine_modes::active`): one whose data cannot make a session (a hole with no tee, a duel deck naming an unknown card) is refused there, and the mode picker shows the reason as its last entry error.

A golf course or a race may name a pack map, and is then played in the 3D field scene that map builds ([in-world golf and racing](../subsystems/expansion-modes.md#in-world-golf-and-racing)):

| Key | On | Meaning |
|---|---|---|
| `map` | `[[golf]]`, `[[races]]` | The `maps/<id>.toml` stem the instance is played on |
| `spawn` | `[[golf]]`, `[[races]]` | The map spawn the host enters at; the map's first when empty |
| `surfaces` | `[[golf]]` | Map legend character -> `fairway`, `rough`, `sand`, `water`, `wall` or `out` |
| `tee`, `cup` | `[[golf.holes]]` | Map tiles `[column, row]` of the tee and the cup; both required on a map course |
| `area` | `[[golf.holes]]` | Inclusive map-tile rectangle `[col0, row0, col1, row1]` the hole plays in; the whole map when left out |
| `waypoints` | `[[races]]` | The centerline as map tiles, in driving order, closing back on the first; replaces `points` |
| `bodies` | `[[golf]]`, `[[races]]` | Map NPC ids (`[[npcs]]` of the map) that play the other seats, in seat order with the player's skipped |

A Grid Duel may name a pack map and a table on it, and is then played on a board laid on that table in the 3D field scene ([Grid Duel at a table](../subsystems/expansion-modes.md#grid-duel-at-a-table)). `map` and `spawn` work as above; `[card_duels.table]` places the table:

| Key (default) | Meaning |
|---|---|
| `tile` | Map tile `[column, row]` the board is centred over |
| `offset` (`[0, 0, 0]`) | World units from the tile centre, `[x, y, z]`, y up; `y` raises the whole table |
| `facing` (0) | Compass sector `0..8` (an NPC's facing table) the player looks across the board in |
| `height` (64) | The table top above the floor, world units |
| `board` (132) | The board's width, world units; its depth is 1.36 times that |
| `seat_distance` (120) | Each seat's distance from the board's centre |
| `draw_table` (true) | Draw the built-in wooden table under the board; off when a prop of the map is the table |
| `opponent` | `{ npc, name, think, cheer, slump }`: the map NPC seated across the board, the name the banners and results card give it, and the clips it plays while thinking, on a win or a big capture, and on a loss |
| `spectators` | The same shape for NPCs that watch from where they stand: each turns to the board, cheers for the player and slumps against them |
| `surface` | `{ tpage, clut, uv = [u, v, w, h] }`: a texture of the base scene's VRAM the board's cells sample instead of the built-in felt |
| `sfx` | `{ deal, place, flip, win, lose }`: sounds, each a retail cue (`{ kind = "sfx", id = N }`) or a pack sound (`{ kind = "sound", id = "..." }`); menu cues by default |

A poker table names a map and a table the same way (`map`, `spawn`, `[poker.table]`):

| Key (default) | Meaning |
|---|---|
| `tile`, `offset`, `facing`, `height` (64), `draw_table` (true), `surface`, `sfx` | As for a duel table; `sfx` plays `deal` per card, `place` for chips pushed out, `flip` per board card, `win` / `lose` for a pot |
| `radius` (130) | The felt's half depth toward the player, world units; its half width is 1.35 times that |
| `players` | The other seats' NPCs, `{ npc, name, think, cheer, slump, fold, all_in }`, in seat order with the player's seat skipped; a seat with none stays empty-chaired |
| `spectators` | NPCs watching from their tiles; they cheer when the player wins a pot |

A table prop of the map should not be standable (`top`): the board sits `height` above the floor the sampler answers at the table's centre. Sample: the `duel_table` test pack, `crates/engine-modes/tests/fixtures/duel_table/`.

A map hole needs no `rows`: its walls, void and heights are the map's tiles. A legend character missing from `surfaces` plays as the surface its style's `label` names (rough; sand or bunker; water or pond; lava, chasm or pit as out of bounds), else as fairway. A map race's `half_width` is in race units, two field world units each. Sample: the `links` test pack, `crates/engine-modes/tests/fixtures/links/`.

### items.toml

New consumables and equipment beside the retail item id space, equipment sets with tiered bonuses, and the pack's shops. A consumable runs one of the retail item handlers; worn gear folds into the battle seed like retail gear; a drop, steal, reward or shop row names a retail id or a pack item key. Declares `item`, `item_set` and `shop` ids. Reference: [`expansion-pack-items.md`](expansion-pack-items.md).

### audio.toml

The pack's own music tracks and sounds (WAV / Ogg Vorbis under `assets/`, with loop points and normalisation) and its music picks: the battle and boss themes of its fights, victory music, the music of each game mode. A map's `[audio]` table adds its music, music zones and positional ambient sources. Declares `track` and `sound` ids. Reference: [`expansion-pack-audio.md`](expansion-pack-audio.md).

### arts.toml and spells.toml

New Tactical Arts, played with the hero's own retail art clips under the pack's power bytes, damage percent, element and status rider; chains that end a turn's arts in a pack finisher; and new Seru magic that casts as a retail Seru spell with the pack's MP, power, element, target shape and rider, learned by capturing a pack monster. Declare `art`, `art_chain` and `spell` ids. Reference: [`expansion-pack-arts.md`](expansion-pack-arts.md).

### maps/&lt;id&gt;.toml

One custom map per file, the file stem its id: a tile grid with walls and floor tiers laid over a retail base scene, props, NPCs borrowed from retail placements, spawns, exits, trigger and encounter zones, and the entrances that lead into the map from the retail game. Declares `map` ids and the pack-wide NPC ids; references `formation`, `boss`, `line`, `quest`, the `items.toml` shops and the `cards.toml` dealers. Reference: [`expansion-pack-maps.md`](expansion-pack-maps.md).

## The `.lxpack` bundle

The shareable form of a pack is one file: `bundle::write_bundle` archives a `PackFiles` tree and `bundle::read_bundle` restores it. The bundle adds no metadata of its own - the manifest is `pack.toml`, one of the files - and no timestamps, so the same tree always gives the same bytes. All integers are little-endian.

| Offset | Size | Field |
|---|---|---|
| `0x00` | 8 | Magic `LXPACK\x1A\n` (`4C 58 50 41 43 4B 1A 0A`) |
| `0x08` | 2 | Bundle format version, `1` |
| `0x0A` | 2 | Flags, `0` |
| `0x0C` | 4 | Entry count |
| `0x10` | | Entry table, then data, then trailer |

Each entry-table record, in path-byte order with no duplicates:

| Offset | Size | Field |
|---|---|---|
| `+0x00` | 2 | Path length in bytes |
| `+0x02` | 1 | Method: `0` stored, `1` raw DEFLATE (RFC 1951) |
| `+0x03` | 1 | Reserved, `0` |
| `+0x04` | 4 | Raw (decompressed) length |
| `+0x08` | 4 | Stored length |
| `+0x0C` | 32 | SHA-256 of the raw bytes |
| `+0x2C` | path length | Path, UTF-8 |

The data region follows the table: each entry's stored bytes back to back in table order, with no padding. The file ends with a 32-byte SHA-256 of every byte before it.

The writer deflates an entry only when that makes it smaller. The reader checks the magic, the file size against `MAX_BUNDLE_LEN`, the version (before the checksum, so a bundle from a newer writer says so whatever else changed), the flags and the trailer before trusting the table, refuses unsorted or duplicate paths, unknown methods, a stored entry whose lengths differ and trailing bytes, inflates each entry to exactly its declared length, and checks each entry's digest.

A path must be relative, `/`-separated, at most 255 bytes, with no empty, `.` or `..` component and none of `\`, `:` or NUL, which keeps an unpacked bundle inside its output directory on every host. Limits: 16384 entries, 64 MiB per entry and 512 MiB in total once decompressed.

Every refusal carries a `bundle::BundleErrorKind` inside its error - `not_a_bundle`, `corrupt`, `unsupported_version`, `too_large` or `bad_path` - so a reader can word it for a person rather than print the parser's message. `asset expansion bundle` writes only the files of the [layout](#layout), which is everything the loader reads, so a bundle unpacks to exactly the editable tree and the workbench's import-and-export reproduces it byte for byte.

## Validation

`validate::validate_files` checks a raw tree (paths, layout, parse errors) and then the parsed pack; `validate::validate` checks a parsed pack alone. Both return every issue rather than stopping at the first. An issue has a severity, the file it is in, a dotted field path, and a message. An **error** stops the engine loading the pack and stops `asset expansion bundle`; a **warning** does not.

| Rule | Severity |
|---|---|
| `pack.toml` missing or unparsable, or any section or map unparsable | error |
| `id`, `version`, `tags`, `requires` or `conflicts` malformed; empty `name` | error |
| `schema` outside the formats this build reads; `disc` other than `SCUS-94254` | error |
| `min_engine` malformed or newer than this engine; a `requires` / `conflicts` entry or its range malformed | error |
| `cover` not a `.png` under `assets/`, absent from the pack, or over 1 MiB | error |
| A file anywhere named like a program, script or scriptable document ([trust](#sharing-and-trust)) | error |
| A pack in its own `requires` / `conflicts`, or one pack in both | error |
| Content id malformed, pack-qualified at its definition, or defined twice | error |
| Reference to undefined content, or to a pack not in `requires` | error |
| `DiscRef` out of range, or rejected by the supplied resolver | error |
| Any [no-disc-bytes](#the-no-disc-bytes-rule) violation | error |
| Empty `description`, `authors` or `license`; no `cover`; non-URL `homepage`; a repeated list entry; a schema-2 key in a schema-1 manifest | warning |
| A file outside the [layout](#layout) | warning |
| A [flow lint](#flow-lints): a pack flag or counter read and never written, or written and never read; a map, quest or challenge the player cannot reach; a quest prerequisite cycle; a card, booster or pack item nothing (reachable) grants | warning |

### Flow lints

The flow lints (`flow::lint`) run over the pack's reference model (`xref`): one walk over the pack's JSON form that records every place a pack id is defined, written, read or used, the nearest definition holding each place (the quest a reward is in, the map a trigger is on, the line a choice is in) and the gates on its way down. The model follows engine flags named after an id (`cutscene.<id>`, `map.<id>.<trigger>`) to that id, and is checked against every declaration and reference the section rules resolve.

Reachability starts at the maps with an entrance from a retail scene and the `auto_start` quests, and repeats until nothing changes: a place takes effect once its holder is reached and its gates hold, and then reaches what it names; a `talk` event counts as talking to its NPC, so it offers that giver's quests. A gate holds once the quests and bosses it needs are reached and its flags are written by something reached; what it needs clear holds from the start.

The pass is optimistic (a reached quest is taken as completed, a reached boss as beaten), so what it reports unreachable is unreachable on every route. A booster with an empty pool grants every card. A flag or counter written and never read is one warning per id, at its first writer; one read and never written is a warning at each reader.

### Adding a section's checks

A section implements `validate::SectionRules`, whose two methods default to doing nothing. Validation calls `declare` on every section first, then `check` on every section, then resolves references, so a quest may name a boss from `battle.toml` whatever order the sections run in. The `Checker` passed in knows which file is under check:

| `Checker` method | Use |
|---|---|
| `declare(kind, id, field)` | Pass 1: register a content id the section defines. |
| `reference(kind, id, field)` | Pass 2: name content of `kind`, resolved after both passes. |
| `disc_ref(field, &DiscRef)` | Pass 2: range-check a coordinate and queue it for the resolver. |
| `error` / `warning(field, msg)` | Report against the current file. |
| `pack()`, `qualified(id)` | Read the whole pack; form `pack:id`. |

Every section type sits in the section list in `validate.rs`. The battle, progression, quest, dialogue and card sections implement the trait in their own modules, and maps in `map_rules`; `modes.toml` alone keeps the empty `impl` in `validate.rs`, since its instances are checked when they are built.

## The no-disc-bytes rule

Every file under `assets/` must pass all of these:

1. Its extension is one of `png`, `gltf`, `glb`, `ogg`, `wav`, `txt`, `md`, `toml` (any case).
2. Its bytes carry that type's signature: the PNG header, `glTF` version 2, `OggS`, `RIFF....WAVE`, or valid UTF-8 text with no NUL (a `.gltf` must open with `{`).
3. Its bytes do not start like a disc format: TIM (`0x00000010`), Legaia TMD (`0x80000002`), Sony TMD (`0x00000041`), VAB (`pBAV`), SEQ (`pQES`), VAG (`VAGp`), a PS-X executable, a raw CD sector's sync pattern, or an ISO 9660 volume descriptor; and contain no PS-X EXE header or sector sync pattern anywhere.
4. It is at most 8 MiB, and all assets together at most 64 MiB.

Anywhere in the pack, a file named like a disc file is refused: `PROT.DAT`, `DMY.DAT`, `SCUS_942.54`, `SYSTEM.CNF`, `CDNAME.TXT`, a disc image (`.bin`, `.iso`, `.cue`, `.img`, `.ccd`, `.sub`, `.mdf`, `.mds`, `.chd`, `.ecm`, `.pbp`), streamed media (`.str`, `.xa`), or a retail format's extension (`.dat`, `.exe`, `.tim`, `.tmd`, `.vab`, `.vag`, `.seq`, `.lzs`, `.man`, `.mes`, `.anm`, `.mdt`, `.dpk`, `.spk`, `.pch`).

The rule exists because packs are made to be shared, and the disc's bytes are not the project's to share. A coordinate costs the modder nothing - the engine reads the mesh, texture or cue from the player's own disc - and it keeps every pack legal to post. The type allowlist does most of the work: an LZS stream, a DATA_FIELD chunk or a headerless table has no PNG or Ogg signature, so it fails rule 2 even though it has no magic of its own for rule 3 to catch. The checks are a guard against accidents, not a proof; text a modder pastes in from the game is still the modder's responsibility.

## Sharing and trust

A pack is downloaded from strangers and opened in a browser tab or beside a game, so its contents are **data only**. Nothing in a pack is ever executed or interpreted as code: the engine reads TOML into typed structures, decodes PNG, glTF, Ogg and WAV with its own decoders, and shows text as text. There is no scripting language in the format - a cutscene, a boss script or a quest is a fixed vocabulary of steps the engine's own kernels run, bounded by the validator - and no field holds a URL the engine fetches; `homepage` is shown, never followed.

`share::import_bundle` is the one gate a shared `.lxpack` passes through, on every reader (the workbench's Open and drop, the play page's library and its install, the launcher's check), in this order:

| Gate | Refuses | Kind |
|---|---|---|
| Bundle | Not an `.lxpack`; a newer bundle format; a file, entry or tree over its size limit; a bad checksum, length or table; a path that could leave the pack directory | `not_a_bundle`, `unsupported_version`, `too_large`, `corrupt`, `bad_path` |
| Trust (`validate::trust_issues`) | A disc file or disc format by name; a program, script or scriptable document (`.js`, `.html`, `.svg`, `.wasm`, `.exe`, `.sh`, `.py`, `.lua`, ...) anywhere in the tree; an asset outside the type allowlist, without its type's signature, carrying disc bytes, or over the asset budgets | `unsafe` |
| Format | A newer pack format, or a newer engine than this one | `newer_format`, `older_format`, `newer_engine` |
| Parse | `pack.toml` or a section that does not parse | `unparsable` |

A pack that passes them is opened; ordinary validation errors (a dangling reference, a value out of range) are left for the workbench to show and keep the engine from activating the pack. The play page never trusts its own stored copy either: every activation re-reads the bytes through the gates and the validator.

What the checks do not promise: they guard against accidents and obvious lures, not against every way to hide data in a valid PNG or Ogg file, and they cannot tell whether text a modder pasted in came from the game. A pack's author answers for its content; the engine's part is that opening one can do nothing but add content to the game. The browser pages store packs only in the viewer's own IndexedDB and upload nothing.

## Save compatibility

The save's expansion block ([`expansion-pack-quests.md`](expansion-pack-quests.md)) records the pack version it was written under and names pack content by id. A save made with one version of a pack and loaded with another goes through `legaia_engine_expansion::save_compat::reconcile` whenever its progress is adopted for the active pack - on a load with the pack active, or when the pack activates after the load:

| Saved entry | When the pack no longer defines it |
|---|---|
| Quest, challenge, once-only boss | Dropped. A kept quest's objective progress is resized to its current objective count (new objectives start at zero). |
| Pack-map location | Dropped; the resume lands in the map's base scene. |
| Card, booster set | Dropped with all copies; the duel deck and seen marks keep the cards that remain. |
| Pack item held or worn, art, spell | Dropped; a worn slot falls empty (the retail record's byte under pack gear is already `0`). |
| Pack flags and counters | Kept: free-form names, not definitions. |

Nothing fails and nothing is invented. The report (`CompatReport`) is logged line by line and shown as one toast on both hosts (`Saved with rebirth 0.1.0: 3 saved entries are gone from the pack and were dropped`), and the play page's expansion status repeats it (`World::expansion_compat_report`). The next save writes the reconciled progress under the new version.

## Diff and changelog

`diff::diff_packs(old, new)` compares two versions of a pack through each file's TOML value, so it is blind to comments, key order and layout. Within a file every top-level key that holds a table of entries (`monsters`) or an array of tables with an `id` (`quests`, `cards`) is a collection, its entries matched by key or id and reported added, removed or changed; a change lists each differing field as a dotted path with its old and new value. Other top-level keys are entries of the file's `settings`; each map file is an entry of `maps`, each asset of `assets` (by content), and the manifest one entry of its own.

`PackDiff::bump` reads the change as semver: a removal is major (it can break saves and dependent packs; below `1.0.0` it bumps the minor), an addition minor, a change alone a patch, and the manifest's own fields do not count. `suggested_version` applies it to the old version, and `changelog` drafts a Markdown section (`## 0.2.0`, then Added / Changed / Removed lines naming each entry and its file) for the top of `CHANGELOG.md`. The workbench's Share panel and `asset expansion diff` both run it.

## Tooling

```bash
asset expansion info                                   # schema, bundle version, layout
asset expansion validate packs/rebirth [--strict]      # a directory or an .lxpack
asset expansion bundle packs/rebirth -o rebirth.lxpack # refuses a pack with errors
asset expansion unbundle rebirth.lxpack -o out/        # out/ must be empty or absent
asset expansion info rebirth.lxpack [--json]
asset expansion diff old.lxpack packs/rebirth [--changelog | --json]
legaia-engine packs [--dir DIR] [--json]                # the installed packs
legaia-engine play-window --expansion rebirth           # an installed pack, by id
```

`validate` exits non-zero on any error, and with `--strict` on any warning. None of these reads a disc. The native host's installed packs live in `packs/` under its data directory (`launcher::packs_dir`): `legaia-engine packs` lists each `.lxpack` and pack folder there with its version, authors, pack format and whether it plays, `--expansion` takes a pack id as well as a path, and the launcher's pack picker opens in that folder.

## See also

- [`packs/rebirth/`](../../packs/rebirth/README.md) - the flagship pack.
- [Language packs](../tooling/translation/index.md) - the same coordinate-only rule for translated text.
- [Engine](../subsystems/engine.md) - the fidelity model packs sit inside: with no pack loaded, the engine is retail.
