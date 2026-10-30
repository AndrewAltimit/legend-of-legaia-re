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
| 3D preview and Play here | `site/js/expansion-workbench-preview3d.js`; on the play page `site/js/play-expansion-live.js` and `site/js/play-view-editor-overlay.js` |
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

## The 3D preview

**Open 3D preview** on the Maps tab (and the Cutscenes, Battle and Cards tabs) frames the play
page itself, as `play.html?xwpreview=1`, above the editor. It is not a second
renderer: the framed page runs its own `LegaiaRuntime`, reads the disc from
the site's shared disc cache, loads the working pack's `.lxpack` and enters
the map through the play page's own entry
(`LegaiaRuntime::enter_expansion_map`, the scene build of
[`browser-host.md`](../subsystems/browser-host.md#expansion-packs-play_expansion)).
So the picture is the engine's build of the map - tiles, walls, props,
decorations, scatter, decals, materials, emitters and animators running,
lights, mood and fog - drawn by the play page's renderer with its default
enhancements.

| Control | Does |
|---|---|
| Orbit | The free camera: the play page's debug orbit turned around a focus of its own (drag turns, wheel zooms out to a whole map, right or middle drag pans). Opens on the whole map from its low-Z side, as the field camera looks: the painter's row 0 nearest, its columns left to right. |
| Player | The field camera: the follow view the game draws the map with, on the player. |
| Fit map | Back to the whole map in the orbit camera. |
| Walk from selection | Stands the player on the selected tile (`expansion_seat_player`) and switches to the field camera; the arrow keys walk in the picture. |
| Layers | Walls, encounter zones, triggers and exits, NPCs (with their patrol loops), spawns (with their facing), props: translucent volumes over the picture. |

The volumes are read back from the engine, not from the editor:
`expansion_overlay_json` is the definition the engine runs, with each tile's
floor height from the world's floor sampler, so a volume stands on the floor
the player walks. They draw twice over the finished frame, faint through
everything and solid where nothing is in front, in the scene's log-depth
space. Their colours are the painter's.

**Edits** re-export the pack a moment after the last one (debounced) and post
the bytes to the frame, which hot-reloads them (`reload_expansion`): the pack
activates again and the map is re-entered with the player where they stood,
the follow camera's framing kept and no place-name banner. A pack with an
error is not sent; the preview keeps the last good build and says why. While
a fight is armed or running in the frame the reload answers busy and is
retried (the frame closes a card binder it opened itself first). What
is not an edit rebuilds nothing: the selection, the layers, the camera and a
cutscene's path are messages to the frame.

**Selection runs both ways.** A click in the picture ray-casts the frame's
own view-projection against the tile floors and selects that tile in the
painter (the first object on it, as the Select tool picks, else the tile);
selecting in the painter outlines the object's tile or zone in 3D.

**Cutscenes.** The timeline preview of a cutscene with a `map` has a scrub
slider: the 3D preview draws the camera's path over that map (each camera
step's focus, an actor focus at the actor's tile) and a marker where the
camera looks at the scrubbed frame, and the timeline row running at that
frame is highlighted. **Play in 3D** plays the cutscene in the frame through
the engine (`expansion_preview_cutscene`, the trigger path).

**Battles and cards.** The same frame previews what a pack adds outside a
map. A formation's or a boss's **Fight in 3D** starts that fight in the frame
(`expansion_preview_fight`, the path a map trigger's `battle` / `boss` event
takes), so the monsters are the engine's battle models with their
animations, and a boss runs its script, music and phases. **Card faces in
3D** on the Cards tab puts one of each pack card into the frame's collection
and opens the binder (`expansion_preview_cards`): the card faces the engine
draws, portraits included. The frame's world is the workbench's own, never a
player's save. With no map open on the Maps tab, the pack's first map stands
in under the fight or the binder.

The frame reports its cost once a second (`frameMs`, the CPU time of one
page frame, its worst, and the display rate) beside the last build's time.
Closing the preview removes the frame, and with it the second engine and its
copy of the disc. The frame runs on the workbench's own main thread (a
same-origin frame shares its parent's), so it keeps out of the editor's way:
its loop stops on a tab without the preview, while the dock is scrolled out
of view and through a painter stroke; it draws at most every other display
frame, and one that cost more than a display frame is followed by up to
three idle ones (the sim clock is wall time, so the world keeps its speed);
and its canvas draws at the size the frame shows rather than the play
page's 960 x 720. A rebuild waits for a pause in the editing that grows with
the last build's time (450 ms at least, 5 s at most), and with **Auto**
cleared only **Reload** rebuilds.

### Cost on a big map

Measured headless (Chromium with SwiftShader, the software GL, on the real
disc) on the largest map the format allows: 120 x 120 tiles, walls round the
edge and a wall pillar every 8 tiles, and a scatter brush at density 0.6 over
its top half - 1152 draws. Rebirth's atrium (15 x 13) is the small case.

| Step | Atrium | 120 x 120 |
|---|---|---|
| `map_preview`, `validate`, `export_bundle` (each edit, in the workbench) | - | 20 - 100 ms, 26 - 127 ms, 66 - 321 ms |
| Painter redraw on a hover | - | 1 - 6 ms |
| Hot reload in wasm (`reload_expansion`: activation, map entry, the page's scene build) | 0.5 - 0.65 s | 2.6 s |
| Hot reload, the page's GL upload after it (`PlayView._rebuild`) | - | 0.4 s |
| Hot reload, post to built | 0.6 - 1.0 s | 3.3 s |
| One preview frame: sim, scene sync and draw list, no GL | - | 4 ms |
| One preview frame under SwiftShader, `gl.finish` included | - | 46 - 244 ms |
| Editor overlay | - | 15 357 vertices |

The ranges are the same steps on a quiet and a busy machine. Every
workbench-side step stays a few frames long, so painting a big map stays
live. Two costs do not. The **rebuild** of a big decorated map is seconds of
wasm, and it blocks the page while it runs - hence the delay that grows with
the last rebuild and the **Auto** switch. Most of it is the scene build's
cross-draw coplanar pass
([`renderer.md`](../subsystems/renderer.md#coplanar-surfaces-retails-ordering-model-the-ports-depth-policy)):
94 006 planes, 1.8 million candidate pairs after the pass's in-plane sweep
(the quadratic walk before the sweep made it 12 s). The headless **frame**
is GL fill on a software rasteriser, which the throttles above keep off the
editor's input; a hardware GPU draws the same 1152 draws in a few
milliseconds. `play_expansion_live::tests::big_map_reload_cost` (ignored,
disc-gated) builds the same map natively and splits the reload into the
engine's map entry and the page's scene build.

## Play here

**Play here** plays the map the Maps tab is on in a play tab, for an
edit-and-look loop with the game's own controls. It posts the bundle on
BroadcastChannel `legaia-expansion-live` as `{ type: "reload", nonce, id,
map, bytes }`; a play tab whose active pack has that id hot-reloads it in
place (`PlayView.reloadExpansion`, the preview's call) and answers `{ type:
"reloaded", nonce, ok, map, reentered, ms, errors }` - on the map it was on,
with the player where they stood. With no answer the bundle goes to the
hand-off slot below and a play tab opens as
`play.html?expansion=1&map=<id>`, which enters that map once the pack loads.
With **Live** ticked every edit goes to the open play tab as it is made.

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

- `crates/web-play/src/play_expansion_live.rs` (`play_expansion_live::tests`,
  disc-gated): the hot reload is the play page's own build - entering the
  Rebirth atrium twice uploads the same scene arrays, a reload keeps the
  player's seat and uploads exactly what a fresh entry uploads, an edited
  tile shows in the overlay the engine reads back, a reload waits as busy
  while the card binder is up or a fight is armed, and a pack with an error
  leaves the running map alone. Disc-free: a reload with no disc refuses.

- `crates/web-expansion/tests/rebirth_round_trip.rs` (disc-free): Rebirth
  loads, validates clean and exports its own bytes; an edit to a monster and a
  map round-trips through the bundle and touches only those two files; the
  tower map's preview shows the corner rule; the XP chart and the dialog check.
- `crates/web-expansion/tests/disc_catalog.rs` (`LEGAIA_DISC_BIN`): the catalog
  lists, Rebirth with no coordinate error on the real disc, the tower scene's
  geometry pack, the preview with the base scene's ladder.
