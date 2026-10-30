# Expansion workbench

The site page `tooling/expansion-workbench.html` builds, checks, previews and
exports an [expansion pack](../formats/expansion-pack.md) in the browser,
against the visitor's own disc. It is the expansion-pack sibling of the
[translation workbench](translation/index.md): the disc is read once, in the
tab, and nothing is uploaded.

| Part | Where |
|---|---|
| Page | `site/_content/tooling/expansion-workbench.html` |
| App, state, persistence, export | `site/js/expansion-workbench-app.js` |
| Form kit and section editors | `site/js/expansion-workbench-forms.js` |
| Tile painter | `site/js/expansion-workbench-map.js` |
| Map detail layer (prop sources, animators, lights, decorations) | `site/js/expansion-workbench-detail.js` |
| wasm session | [`crates/web-expansion`](../../crates/web-expansion/README.md) (`ExpansionWorkbench`) |
| Example pack | `site/packs/rebirth.json`, written by `site/_gen.py` from [`packs/rebirth/`](../../packs/rebirth/README.md) |

## The session

`ExpansionWorkbench` (`legaia_web_expansion::wasm`, over the native-testable
`session::Core`) keeps three things: the disc catalog, the files the pack was
loaded from, and the pack's `assets/` and documentation files. The page holds
the pack itself, as the JSON form of `ExpansionPack` - serde's, with the same
keys as the TOML - and every export runs JSON -> `ExpansionPack` ->
`ExpansionPack::to_files`, so what is checked is what is written.

| Export | Returns |
|---|---|
| `open_disc(image)` | The picker lists: monsters (id, name, base stats, element, drop, spells), items, spells, scenes, music. |
| `begin_files` / `add_file(path, bytes)` / `finish_files()` | Loads a pack tree (a folder pick or the example): `{ pack, files, issues }`. One leading directory shared by every path is dropped. |
| `load_bundle(bytes)` | The same from a `.lxpack`. |
| `start_fresh(id, name)` | A pack holding only its manifest. |
| `validate(pack)` | Every issue, `[{ severity, path, field, message }]`. |
| `files_view(pack)` | The exported tree, `[{ path, size, text }]`. |
| `export_bundle(pack)` | The `.lxpack` bytes; refused while the pack has an error. |
| `xp_chart(progression)` | Cumulative XP per level, retail and the pack's, and the enemy scaling per party level. |
| `check_dialog(line)` | A line's rows page by page: length, the row warning, markup that does not encode, the compiled size. |
| `map_preview(map)` | Per tile the floor tier and wall quarters, per cell the four corner heights and whether they agree. |
| `cutscene_timeline(pack, id, picks)` | A cutscene played on the engine's sequencer against a stand-in world (`legaia_engine_expansion::cutscene::preview`): each executed step with its frame and track, the frame it ends on, the flags it raises and the fight it ends on. `picks` is the option taken at each menu. |
| `asset_bytes(path)` / `audio_info(path)` | An asset's bytes (an audio preview) and an audio file's probe: `{ rate, channels, frames, seconds, loop }` or `{ error }`. |
| `scene_objects(scene)` | A base scene's geometry pack: per mesh its primitive count, extent, how often the retail scene places it, and its footprint seen from above. |

A file the editor left unchanged exports as the bytes it was loaded from:
`pack_files` compares each manifest, section and map with its loaded value and
keeps the original where they are equal, so an unedited section keeps its
comments. An edited one is re-serialised by `to_files`, which is deterministic,
so the same pack always gives the same `.lxpack`.

The crate sits in the engine layer: it reads scene geometry through
`legaia-engine-scene` (`scene_assembly`, `expansion_map`) and the level curve
through `legaia-engine-expansion::progression::xp_table`, so the previews are
the engine's own kernels. It names no crate above the kernels.

## The disc, and what a pack never holds

The disc gives the pickers their names and the validator its
[`DiscResolver`](../formats/expansion-pack.md#disc-references): a monster id is
a populated record of the PROT 867 archive, a scene a CDNAME block, an item a
named id, a PROT entry one the disc has. Only `PROT.DAT` stays resident; the
image is dropped once parsed.

A picker writes the coordinate (`{ kind = "monster", id = 109 }`) and never the
name beside it. A derived monster's name field shows the retail name as a
placeholder, not a value, so a pack carries a name only where the modder types
one. Every export also runs the validator's
[no-disc-bytes rule](../formats/expansion-pack.md#the-no-disc-bytes-rule).

## Editors

One tab per file, plus a Files tab. Every field carries `data-path`, its dotted
path in the validator's spelling (`monsters.ogre.hp`,
`quests[2].objectives[0]`), so clicking an issue opens its tab, selects the item
it is in and focuses the field with the longest matching path. Pickers for pack
content offer only ids the pack defines.

| Tab | Edits |
|---|---|
| Manifest | `pack.toml`. |
| Battle | Derived monsters (base picker, each stat unset / set / scaled with the retail value beside it, drop, steal, element, spells, the boss-script phase editor with the base's swings and the disc's spells in its move pickers), formations as four seats of pack or retail monsters, encounter overrides per scene, bosses. |
| Progression | Multipliers, the curve (retail, scaled, banded, explicit) with a live chart against retail, enemy scaling, level-up rewards. |
| Quests | Quests (prerequisites, objectives by kind, rewards, repeat, dialogue and battle hooks) and challenges. |
| Dialog | Lines with a per-row check under the text: the 36-character row warning and any markup with no glyph. |
| Cutscenes | Cutscenes: the step list of the main track and each named track, a form per step following its `do` key, and the timeline preview (`cutscene_timeline`) with the menu picks to play it with. |
| Cards | Cards, boosters, sell prices, drop rules. |
| Items | Pack items (kind, look, prices, the effect of a consumable, the stats, passives, element and set of equipment), sets and their tiers, shops and their gated stock. Every item picker elsewhere (drops, steals, rewards, collect objectives) offers retail and pack items together. |
| Arts | Pack arts: the command string from arrow buttons labelled with the hero's arm mapping, checked against the disc's arts-name table (a combo equal to or containing a retail one is flagged); the segment list, each a retail clip of the hero picked by name, with its power bytes and beats; a damage preview through the retail melee kernel for an ATK and DEF; rider, learn conditions; chains and their finisher. |
| Spells | Pack Seru: the base Seru (named from the disc), MP, power and growth with the per-level table, element, target, rider, who learns it, and the learn conditions with the captured pack monster. |
| Modes | Poker tables, card duels, golf courses, race tracks, prop hunt arenas. |
| Audio | `audio.toml`: tracks and sounds (file, gain, normalisation, loop; an import into `assets/audio/`, a Play button through the browser's own decoder, the validator's probe of rate, length and the file's own loop), the battle / boss / victory picks and the music per mode kind or instance. Every music field picks retail cues by sound-test name or a pack track. |
| Maps | The tile painter, with a Sound box for the map's `[audio]`: map music, the zone fade, music zones and ambient sources. |
| Files | Each exported file, to copy or download on its own. |

## The tile painter

The painter edits a `MapDef` 1:1: the canvas is `tiles.rows`, the palette is
`tiles.legend`. A legend entry sets its character, label, wall quarters (`tl`,
`tr`, `bl`, `br`; none and all write `"none"` / `"all"`), floor tier and ground
(a texture cell, a base-scene cell or a base-pack mesh). Tools: paint, fill
rectangle, pick, select / move, and place spawn, exit, trigger zone, encounter
zone, prop and NPC. An NPC's model picker lists the base scene's retail
placements by the first line each one says (the only name retail gives an
NPC) and stores only the `npc` coordinate; a selected prop edits its
standable `top` and footprint, and a legend entry its visible `wall` (a box or
a base-scene mesh). Each stroke is one undo step (`Ctrl+Z` / `Ctrl+Y`); `Ctrl+wheel`
zooms; a right or middle drag, or Space with a drag, pans. A placed prop draws
as its mesh's footprint, rotated by its yaw.

A selected prop's **mesh source** is the base scene's pack (the mesh list),
another scene's pack (`{ scene, object }`), a PROT coordinate, or a glTF model
in the pack's `assets/`; **Import glTF** reads a `.glb`, `.gltf` or `.png`
from the user's machine into `assets/models/` (`ExpansionWorkbench::add_asset`,
kept in the session's extras with the pack's other files and exported with
it - nothing is uploaded). The prop's animators are a JSON list and its light
a form. The **Detail layer** box under the selection holds the map's
`decor`, `scatter`, `edges`, `lighting`, `materials` and `fog` as JSON
editors, each parsed on change with the error shown in place, plus quick adds
for a scatter brush over the whole map and a lamp at its centre
([`expansion-pack-maps.md`](../formats/expansion-pack-maps.md#decorations)).
The `decals` and `emitters` sections have their own JSON editors there, and
an NPC's form has a **Scenery** checkbox (draws and plays its clip, no talk,
no collision). The canvas draws every decoration (green dots) and decal
(blue dots) where the map bakes it: `map_preview` expands the map's
`decor`, `scatter`, `edges` and `decals` and returns the positions
(`decor`), so a brush's coverage shows as it is edited. It builds the field
image without decorations and with every non-base prop mesh as mesh `0`
(neither touches the collision grid it reads).

The **Heights** overlay is read back from `map_preview`, which builds the
synthetic field image the engine enters
([`expansion-pack-maps.md`](../formats/expansion-pack-maps.md#the-synthetic-field-image))
and reads its collision grid. Each tile shows its tier, lighter for higher, and
a cell whose four corner heights disagree is hatched. That is the
[corner rule](../formats/expansion-pack-maps.md#tiers-are-corner-heights): a
tile's tier is the height of its low-X / low-Z corner, so along a raised
block's top and left edges the cell outside the block ramps up onto it, and
along its bottom and right edges the block's own last row and column ramp down.
The hover line gives the tile, its tier and height, its wall quarters and the
cell's four corner heights.

## Persistence and the play hand-off

The working pack autosaves to IndexedDB database `legaia-expansion-workbench`,
store `autosave`: key `current` holds `{ json, label, savedAt }`, key `source`
the tree or bundle it was loaded from. A returning visitor who loads a disc is
offered "resume". Every access is wrapped; without storage the page still
works.

**Play in browser** exports the bundle and stores it in database
`legaia-expansion`, store `packs`, key `play`, as
`{ id, name, version, bytes, savedAt }` (`bytes` a `Uint8Array` of the
`.lxpack`), then opens `play.html?expansion=1`. The play page reads that key
once its disc is up and loads the bundle through `LegaiaRuntime::load_expansion`,
the same call as its own *Load expansion* file input
([`browser-host.md`](../subsystems/browser-host.md)).

## The example pack

`site/_gen.py` writes `site/packs/<id>.json` for each id in `EXAMPLE_PACKS`:
`{ id, files: [{ path, text } | { path, base64 }] }`, every file of
`packs/<id>/` in path order. The page loads it the way it loads a picked
folder. The packs are the project's own content; the file is generated, not
committed.

## Tests

- `crates/web-expansion/tests/rebirth_round_trip.rs` (disc-free): Rebirth
  loads, validates clean and exports its own bytes; an edit to a monster and a
  map round-trips through the bundle and touches only those two files; the
  tower map's preview shows the corner rule; the XP chart and the dialog check.
- `crates/web-expansion/tests/disc_catalog.rs` (`LEGAIA_DISC_BIN`): the catalog
  lists, Rebirth with no coordinate error on the real disc, the tower scene's
  geometry pack, the preview with the base scene's ladder.
