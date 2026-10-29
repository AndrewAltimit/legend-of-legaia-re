# Expansion packs

An expansion pack is a modder-authored bundle of new content for the port - custom maps, battles and bosses, a level curve, quests, dialogue, trading cards, game modes - that the engine loads beside the player's own disc. It is the port's own format, not a disc format: nothing here is decoded from retail, so the page carries no confidence labels. The parser is `legaia_expansion_format` ([`crates/expansion-format`](../../crates/expansion-format/README.md)); the runtime that applies a pack is `legaia_engine_expansion`.

A pack carries **no bytes from the disc**. Every disc asset it uses is named by a coordinate (a [`DiscRef`](#disc-references)) and resolved against the disc the player supplies, the same rule a shareable [language pack](../tooling/translation/index.md) follows. The flagship pack, [`packs/rebirth/`](../../packs/rebirth/README.md), is written with exactly the format and tooling on this page.

## At a glance

| Property | Value |
|---|---|
| Source form | A directory: `pack.toml` plus optional section files, `maps/`, `assets/` |
| Shareable form | One `.lxpack` file ([bundle](#the-lxpack-bundle)) |
| Text encoding | UTF-8 TOML for every manifest, section and map |
| Schema version | `1` (`manifest::SCHEMA_VERSION`) |
| Disc | NA release only, `SCUS-94254` |
| Parser | `ExpansionPack::from_files` (one parser for directory, bundle and browser input) |
| Writer | `ExpansionPack::to_files`, `bundle::write_bundle` |
| Validator | `validate::validate`, `validate::validate_files` |
| CLI | `asset expansion validate / bundle / unbundle / info` |

## Layout

Paths are relative to the pack root with `/` separators. Every file but `pack.toml` is optional.

```text
pack.toml          manifest (required)
battle.toml        monsters, formations, encounter overrides, bosses
progression.toml   level curve, XP and rewards
quests.toml        quests and challenges
dialog.toml        dialogue lines
cards.toml         trading cards
modes.toml         game modes
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
| `schema` | integer | no | Schema version the pack is written against; defaults to the current one. |
| `disc` | string | no | The disc the coordinates point into; defaults to `SCUS-94254`, the only value accepted. |
| `license` | string | no | Licence for the pack's own content: an SPDX expression or free text. |
| `homepage` | string | no | Where the pack is published, an `http(s)` URL. |
| `tags` | string list | no | Discovery tags, pack-id syntax. |
| `requires` | string list | no | Pack ids that must be loaded alongside this one. |
| `conflicts` | string list | no | Pack ids that must not be. |

A minimal manifest:

```toml
id = "tower"
name = "The Tower"
version = "0.1.0"
```

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
| `item` | `id` | Item id ([item-table.md](item-table.md)) | the `u8` range |
| `bgm` | `id` | Sound-test id ([music-tracks.md](../reference/music-tracks.md)) | `0..=80` |

PROT numbers are **extraction** indices, the `NNNN` in `extracted/PROT/NNNN_*.BIN`, not raw TOC indices ([cdname.md](cdname.md#numbering-space)).

Checking happens in two halves. `DiscRef::check_range` needs no disc and rejects a coordinate no NA disc can satisfy. `refs::DiscResolver` is the disc-backed half: the engine implements `resolve(&DiscRef) -> Result<(), String>` over the player's disc (does entry 874 hold a TMD with an object 3), and `validate::validate_with` runs every coordinate a section recorded through it.

## Sections

Each content section is a TOML file whose type lives in its own module of `legaia_expansion_format`. A section's keys are documented in its subsection here once its schema is defined; until then the section parses only as an empty table.

| File | Type | Module |
|---|---|---|
| `battle.toml` | `BattleSection` | `battle` |
| `progression.toml` | `ProgressionSection` | `progression` |
| `quests.toml` | `QuestSection` | `quest` |
| `dialog.toml` | `DialogSection` | `dialog` |
| `cards.toml` | `CardSection` | `cards` |
| `modes.toml` | `ModeSection` | `modes` |
| `maps/<id>.toml` | `MapDef` | `map` |

### battle.toml

Reserved; schema not yet defined.

### progression.toml

Reserved; schema not yet defined.

### quests.toml

Reserved; schema not yet defined.

### dialog.toml

Reserved; schema not yet defined.

### cards.toml

Reserved; schema not yet defined.

### modes.toml

Reserved; schema not yet defined.

### maps/&lt;id&gt;.toml

Reserved; schema not yet defined.

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

The writer deflates an entry only when that makes it smaller. The reader checks the magic, version, flags and trailer before trusting the table, refuses unsorted or duplicate paths, unknown methods, a stored entry whose lengths differ and trailing bytes, inflates each entry to exactly its declared length, and checks each entry's digest.

A path must be relative, `/`-separated, at most 255 bytes, with no empty, `.` or `..` component and none of `\`, `:` or NUL, which keeps an unpacked bundle inside its output directory on every host. Limits: 16384 entries, 64 MiB per entry and 512 MiB in total once decompressed.

## Validation

`validate::validate_files` checks a raw tree (paths, layout, parse errors) and then the parsed pack; `validate::validate` checks a parsed pack alone. Both return every issue rather than stopping at the first. An issue has a severity, the file it is in, a dotted field path, and a message. An **error** stops the engine loading the pack and stops `asset expansion bundle`; a **warning** does not.

| Rule | Severity |
|---|---|
| `pack.toml` missing or unparsable, or any section or map unparsable | error |
| `id`, `version`, `tags`, `requires` or `conflicts` malformed; empty `name` | error |
| `schema` other than the current version; `disc` other than `SCUS-94254` | error |
| A pack in its own `requires` / `conflicts`, or one pack in both | error |
| Content id malformed, pack-qualified at its definition, or defined twice | error |
| Reference to undefined content, or to a pack not in `requires` | error |
| `DiscRef` out of range, or rejected by the supplied resolver | error |
| Any [no-disc-bytes](#the-no-disc-bytes-rule) violation | error |
| Empty `description`, `authors` or `license`; non-URL `homepage`; a repeated list entry | warning |
| A file outside the [layout](#layout) | warning |

### Adding a section's checks

A section implements `validate::SectionRules`, whose two methods default to doing nothing. Validation calls `declare` on every section first, then `check` on every section, then resolves references, so a quest may name a boss from `battle.toml` whatever order the sections run in. The `Checker` passed in knows which file is under check:

| `Checker` method | Use |
|---|---|
| `declare(kind, id, field)` | Pass 1: register a content id the section defines. |
| `reference(kind, id, field)` | Pass 2: name content of `kind`, resolved after both passes. |
| `disc_ref(field, &DiscRef)` | Pass 2: range-check a coordinate and queue it for the resolver. |
| `error` / `warning(field, msg)` | Report against the current file. |
| `pack()`, `qualified(id)` | Read the whole pack; form `pack:id`. |

Every section type already sits in the section list in `validate.rs` with an empty placeholder `impl`. A section that grows checks implements the trait in its own module and deletes its placeholder line.

## The no-disc-bytes rule

Every file under `assets/` must pass all of these:

1. Its extension is one of `png`, `gltf`, `glb`, `ogg`, `wav`, `txt`, `md`, `toml` (any case).
2. Its bytes carry that type's signature: the PNG header, `glTF` version 2, `OggS`, `RIFF....WAVE`, or valid UTF-8 text with no NUL (a `.gltf` must open with `{`).
3. Its bytes do not start like a disc format: TIM (`0x00000010`), Legaia TMD (`0x80000002`), Sony TMD (`0x00000041`), VAB (`pBAV`), SEQ (`pQES`), VAG (`VAGp`), a PS-X executable, a raw CD sector's sync pattern, or an ISO 9660 volume descriptor; and contain no PS-X EXE header or sector sync pattern anywhere.
4. It is at most 8 MiB, and all assets together at most 64 MiB.

Anywhere in the pack, a file named like a disc file is refused: `PROT.DAT`, `DMY.DAT`, `SCUS_942.54`, `SYSTEM.CNF`, `CDNAME.TXT`, a disc image (`.bin`, `.iso`, `.cue`, `.img`, `.ccd`, `.sub`, `.mdf`, `.mds`, `.chd`, `.ecm`, `.pbp`), streamed media (`.str`, `.xa`), or a retail format's extension (`.dat`, `.exe`, `.tim`, `.tmd`, `.vab`, `.vag`, `.seq`, `.lzs`, `.man`, `.mes`, `.anm`, `.mdt`, `.dpk`, `.spk`, `.pch`).

The rule exists because packs are made to be shared, and the disc's bytes are not the project's to share. A coordinate costs the modder nothing - the engine reads the mesh, texture or cue from the player's own disc - and it keeps every pack legal to post. The type allowlist does most of the work: an LZS stream, a DATA_FIELD chunk or a headerless table has no PNG or Ogg signature, so it fails rule 2 even though it has no magic of its own for rule 3 to catch. The checks are a guard against accidents, not a proof; text a modder pastes in from the game is still the modder's responsibility.

## Tooling

```bash
asset expansion info                                   # schema, bundle version, layout
asset expansion validate packs/rebirth [--strict]      # a directory or an .lxpack
asset expansion bundle packs/rebirth -o rebirth.lxpack # refuses a pack with errors
asset expansion unbundle rebirth.lxpack -o out/        # out/ must be empty or absent
asset expansion info rebirth.lxpack [--json]
```

`validate` exits non-zero on any error, and with `--strict` on any warning. None of these reads a disc.

## See also

- [`packs/rebirth/`](../../packs/rebirth/README.md) - the flagship pack.
- [Language packs](../tooling/translation/index.md) - the same coordinate-only rule for translated text.
- [Engine](../subsystems/engine.md) - the fidelity model packs sit inside: with no pack loaded, the engine is retail.
