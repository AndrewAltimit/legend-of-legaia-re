# Expansion workbench

The site page `tooling/expansion-workbench.html` builds, checks, previews and
exports an [expansion pack](../formats/expansion-pack.md) in the browser,
against the visitor's own disc. It is the expansion-pack sibling of the
[translation workbench](translation/index.md): the disc is read once, in the
tab, and nothing is uploaded.

| Part | Where |
|---|---|
| Page | `site/_content/tooling/expansion-workbench.html` |
| App, state, history, persistence, shortcuts, templates, export | `site/js/expansion-workbench-app.js` |
| Find and cross-reference | `site/js/expansion-workbench-xref.js` |
| Rename with references | `site/js/expansion-workbench-rename.js` |
| Flow tab (the progression graph) | `site/js/expansion-workbench-graph.js` |
| Form kit and section editors | `site/js/expansion-workbench-forms.js` |
| Tile painter | `site/js/expansion-workbench-map.js` |
| Map detail layer (prop sources, lights, fog, emitters, entrances) | `site/js/expansion-workbench-detail.js` |
| Map detail list forms (decorations, brushes, edge runs, decals, animated materials, animators, the texture-cell picker) | `site/js/expansion-workbench-detail-forms.js` |
| Point lists with a 2D pad (race centre lines, prop-hunt blockers) | `site/js/expansion-workbench-points.js` |
| Contextual help (the "?" per tab and section) | `site/js/expansion-workbench-help.js` |
| Sharing: import gates, drop, Share panel | `site/js/expansion-workbench-share.js` |
| Pack library (shared with the play page) | `site/js/play-pack-library.js` |
| 3D preview and Play here | `site/js/expansion-workbench-preview3d.js`; on the play page `site/js/play-expansion-live.js` and `site/js/play-view-editor-overlay.js` |
| Editing in the 3D preview (gizmos, brushes, multi-select) | `site/js/expansion-workbench-gizmos.js`; on the play page `site/js/play-view-editor-gizmos.js` |
| Map terrain (sculpt brush, materials, water, horizon) | `site/js/expansion-workbench-terrain.js` |
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
| `load_bundle(bytes)` | The same from a `.lxpack`, unchecked (the resume path, over bytes it stored itself). |
| `import_bundle(bytes)` | A shared `.lxpack` through every [import gate](../formats/expansion-pack.md#sharing-and-trust): `{ ok, pack, files, issues, summary }`, or `{ ok: false, error: { kind, message } }`. |
| `share_status(pack)` | The Share panel's identity facts: engine and format gates, each dependency parsed, the cover's size. |
| `diff_against(old, pack)` | The pack against an older `.lxpack` of it: `{ diff, counts, bump, suggested_version, changelog, same_pack }`. |
| `set_doc(path, text)` / `doc_text(path)` | A documentation file (`CHANGELOG.md`) written into or read from the pack. |
| `sources_zip(pack)` | The source tree as a zip under one `<id>/` folder. |
| `start_fresh(id, name)` | A pack holding only its manifest. |
| `validate(pack)` | Every issue, `[{ severity, path, field, message }]`. |
| `files_view(pack)` | The exported tree, `[{ path, size, text }]`. |
| `export_bundle(pack)` | The `.lxpack` bytes; refused while the pack has an error. |
| `xp_chart(progression)` | Cumulative XP per level, retail and the pack's, and the enemy scaling per party level. |
| `check_dialog(line)` | A line's rows page by page: length, the row warning, markup that does not encode, the compiled size. |
| `map_preview(map)` | Per tile the floor tier and wall quarters, per cell the four corner heights and whether they agree. |
| `cutscene_timeline(pack, id, picks)` | A cutscene played on the engine's sequencer against a stand-in world (`legaia_engine_expansion::cutscene::preview`): each executed step with its frame and track, the frame it ends on, the flags it raises and the fight it ends on. `picks` is the option taken at each menu. |
| `asset_bytes(path)` / `audio_info(path)` | An asset's bytes (an audio preview) and an audio file's probe: `{ rate, channels, frames, seconds, loop }` or `{ error }`. |
| `xref_sites(pack)` | The reference model (`legaia_expansion_format::xref`): every site of every pack id, `{ kind, id, role, file, field, key, owner }`. |
| `rename_plan(pack, kind, old, new)` / `rename_id(...)` | A rename's changes (`{ file, field, role, key, before, after }`), or the pack with them made: `{ ok, pack, changes }`, or `{ ok: false, error }`. |
| `flow_graph(pack)` | The flow graph (`flow::graph`): nodes with their place, reachability and findings, and edges by role. |
| `scene_objects(scene)` | A base scene's geometry pack: per mesh its primitive count, extent, how often the retail scene places it, and its footprint seen from above; and `cells`, every texture cell (`tpage`, `clut`, `tile`) the pack's textured prims sample, most used first. |

A file the editor left unchanged exports as the bytes it was loaded from:
`pack_files` compares each file's canonical form (what `to_files` writes for
the edited pack) with the loaded pack's and keeps the original where they are
equal, so an unedited section keeps its comments and an edit that is
reverted gives the loaded bytes back. An edited one is re-serialised by `to_files`, which is deterministic,
so the same pack always gives the same `.lxpack`. A section file that was
loaded but holds only its default (an empty `battle.toml`) is kept too, so an
unedited pack exports exactly the layout files it came from: a `.lxpack`
imported and exported again is the same bytes.

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
| Battle | Derived monsters (base picker, each stat unset / set / scaled with the retail value beside it, drop, steal, element, spells, the boss-script phase editor with the base's swings and the disc's spells in its move pickers and each phase's screen flash and tint), formations as four seats of pack or retail monsters and their later waves, encounter overrides per scene, bosses (music, intro and outro cutscenes, rewards), and a formation's or a boss's battle stage. |
| Progression | Multipliers, the curve (retail, scaled, banded, explicit) with a live chart against retail, enemy scaling, level-up rewards. |
| Quests | Quests (prerequisites, objectives by kind, rewards, repeat, dialogue and battle hooks) and challenges. |
| Dialog | Lines with a per-row check under the text: the 36-character row warning and any markup with no glyph. |
| Cutscenes | Cutscenes: the step list of the main track and each named track, a form per step following its `do` key, and the timeline preview (`cutscene_timeline`) with the menu picks to play it with. |
| Cards | Cards, boosters, sell prices, drop rules, the starter deck, dealers. |
| Items | Pack items (kind, look, prices, the effect of a consumable, the stats, passives, element and set of equipment), sets and their tiers, shops and their gated stock. Every item picker elsewhere (drops, steals, rewards, collect objectives) offers retail and pack items together. |
| Arts | Pack arts: the command string from arrow buttons labelled with the hero's arm mapping, checked against the disc's arts-name table (a combo equal to or containing a retail one is flagged); the segment list, each a retail clip of the hero picked by name, with its power bytes and beats; a damage preview through the retail melee kernel for an ATK and DEF; rider, learn conditions; chains and their finisher. |
| Spells | Pack Seru: the base Seru (named from the disc), MP, power and growth with the per-level table, element, target, rider, who learns it, and the learn conditions with the captured pack monster. |
| Modes | Poker tables, card duels (on a map: the table, its opponent, spectators, surface and sounds), golf courses and race tracks (on a map: spawn, surfaces, waypoints, the NPCs in the other seats), prop hunt arenas, party nights (rounds, prizes, and a podium in the world: its map, block tile, facing and the NPCs that stand in for the rivals). A centre line and a set of blockers are point lists on a 2D pad (below). |
| Audio | `audio.toml`: tracks and sounds (file, gain, normalisation, loop; an import into `assets/audio/`, a Play button through the browser's own decoder, the validator's probe of rate, length and the file's own loop), the battle / boss / victory picks and the music per mode kind or instance. Every music field picks retail cues by sound-test name or a pack track. |
| Maps | The tile painter; map settings with arrival cutscenes and the entrances from retail scenes; the detail layer; a Sound box for the map's `[audio]`: map music, the zone fade, music zones and ambient sources. |
| Flow | No file: the progression graph over the whole pack, with the flow lints on it (below). |
| Files | Each exported file, to copy or download on its own. |

## Coverage

Every part of the pack format against what edits it on the page. **Form**
is typed fields with pack and disc pickers; a **list** form adds, removes,
duplicates and reorders its items; a **point list** is a list of rows beside
a 2D pad that a click adds to and a drag moves; **JSON** a JSON box parsed on
change with the error shown in place. Every row's fields carry their
validator path, so a check clicks through to the field.

| File | Part | Editor |
|---|---|---|
| `pack.toml` | Manifest | Form |
| `battle.toml` | Derived monsters, stats, drop, steal | Form |
| `battle.toml` | Boss script: phases, moves, summons, immunities, last stand, each phase's screen flash and tint | Form (phase list) |
| `battle.toml` | Formations, seats | Form |
| `battle.toml` | Formation waves (`wipe` / `refill`, count, max standing) | Form (wave list) |
| `battle.toml` | Encounter overrides, bosses (intro / outro cutscenes), reward items | Form |
| `battle.toml` | Battle stage on a formation or a boss (scene, stage index, `keep_object_1`, tint) | Form: disc stage pickers, tint swatch, Fight in 3D |
| `progression.toml` | Multipliers, XP curve, enemy scaling, level rewards | Form, live chart |
| `quests.toml` | Quests, prerequisites, objectives, rewards, repeat | Form |
| `quests.toml` | Challenges: rules, score, rewards | Form |
| `dialog.toml` | Lines, choices | Form, per-row check |
| `cutscenes.toml` | Cutscenes, main and named tracks, steps (the `banner` step among them) | Form (step list), timeline |
| `cards.toml` | Cards, boosters, sell prices, drop rules (monster, formation, boss, mode) | Form |
| `cards.toml` | Starter deck | Form (ordered list) |
| `cards.toml` | Dealers | Form |
| `items.toml` | Items, effects, equipment, sets, shops, gated stock | Form |
| `arts.toml` | Arts, segments, learn conditions, chains | Form, damage preview |
| `spells.toml` | Pack Seru, growth, learn conditions | Form |
| `modes.toml` | Poker, on a map with a table (players, spectators, surface, sounds); prop hunt; party nights and their podium | Form |
| `modes.toml` | Card duels, on a map with a table (opponent, spectators, surface, sounds) | Form |
| `modes.toml` | Golf: hole grids, or a map course (spawn, surfaces, golfers, tee, cup, area) | Form |
| `modes.toml` | Races: centre-line points, or a map race (spawn, waypoints over the map's tiles, racers) | Point list / Form |
| `modes.toml` | Prop hunt blockers (drawn on the pad among the decoys) | Point list |
| `modes.toml` | Table seats: NPC, name, clips side by side | Form |
| `audio.toml` | Tracks, sounds, music picks | Form, playback |
| `maps/<id>.toml` | Settings, base, arrival cutscenes | Form |
| `maps/<id>.toml` | Entrances from retail scenes | Form (list) |
| `maps/<id>.toml` | Tiles, legend, spawns, exits, triggers, encounter zones, props, NPCs | Painter and selection form |
| `maps/<id>.toml` | Map `[audio]` | Form (Sound box) |
| `maps/<id>.toml` | Lighting (mood, lamps), fog regions, particle emitters | Form (lists) |
| `maps/<id>.toml` | Prop mesh source and light | Form |
| `maps/<id>.toml` | Terrain: sculpt strokes, resolution and settings | Sculpt tool, form |
| `maps/<id>.toml` | Terrain materials, cliff, water table, horizon, shape list | JSON |
| `maps/<id>.toml` | Decorations, scatter brushes (meshes, cluster), edge runs, decals, animated materials (cycle, hsv, tint, scroll), prop animators | Form (lists), tile and rectangle Pick on the painter, texture-cell picker |
| `assets/` | glTF models, audio | Import only |
| `README.md`, licences | Documentation | Carried unchanged |

What is not on the page: the run rules (difficulty, New Game+, ironman,
randomizer, Boss Rush) are a New Game screen choice of the player
([`run-rules.md`](../subsystems/run-rules.md)), not pack content, and the
terrain's tables stay JSON while the terrain editor's own tools are built.

## The page

**Starting.** Before a pack is open the page offers three starting points:
*Rebirth example*, *New pack ...* (the id, name and template form) and *Open your pack*
(an `.lxpack` or a folder). A choice made before the disc is read waits for
it and runs once the disc is in, instead of being refused; step 1 of the
panel says whether the disc is read. Once a pack is open, the disc picker and
the pack buttons fold to one summary line ("Disc and pack", the disc's file
name and the pack) that unfolds to open another pack or change the disc, and
the page scrolls to the editor.

**Checks on the field.** Every check on the open tab marks the field it names
(the longest `data-path` holding its field) with a red edge, amber for a
warning, and its message under the field until it is fixed; a click in the
Checks list still jumps there and flashes it. The Maps tab is left out (its
panel is one map of several). A value the format's types refuse, such as
`999` in a `u8` field, fails the whole pack's parse in the session; the page
maps the column serde reports back to the JSON path it wrote and lists it as
an ordinary check on that field, with the field's range. **Download**,
**Play in browser** and **Play here** refuse while the pack has an error and
open the first one, with `F8` to step through the rest.

**Long lists.** A list editor of more than twelve entries (monsters, dialogue
lines, quests) has a filter over its labels; `Enter` opens the first match,
and the text survives an edit's re-render.

**Undo and redo** cover the whole pack: every edit, a painter stroke as one
step, a burst of typing as one step (`Ctrl+Z`, `Ctrl+Y` or `Ctrl+Shift+Z`;
in a text box the box's own undo runs first). The history holds up to 200
steps of the pack's JSON. An editor that fills in a default as it draws
(an empty gate box) is not an edit and makes no step.

**Find** (the box above the checks, `Ctrl+K` or `/`) lists every place a
value sits, as `file` and validator field path, so a hit is a jump target
exactly as a check is. An id lists, per kind, where it is defined, set, read
and referenced, from the format crate's reference model (`xref_sites`) - the
model the rename, the Flow tab and the validator's
[flow lints](../formats/expansion-pack.md#flow-lints) use, so the four
agree. `monster:109`, `item:5`, `scene:town01` or `bgm:12` list a retail
coordinate's users; anything else is a text search. Empty, it lists the
pack's flags and counters with their set and read counts, flagging one read
and never set.

**Rename.** Each kind an id has in Find carries a **Rename**: typing the new
id lists every field the rename rewrites (file, field, old and new value,
each a jump) before **Rename** applies it, as one step of the undo. The
definition, every reference and the engine flags named after the id
(`cutscene.<id>`, `map.<id>.<trigger>`) change together; a qualified
reference keeps its `pack:` prefix, and a taken or malformed id is refused.
Changing an id in a tab's own Id box (a monster, a quest, a line, a mode, a
map's file name) runs the same rename; **References** beside it opens Find
on the id. Rebirth's tests rename ids of every kind and check that the pack
still validates, that exactly the planned files export changed, and that
renaming back gives the pack again.

**Flow.** The Flow tab draws the pack's progression as a node graph: an entry
node for the retail game, the maps, quests, challenges, bosses, cutscenes and
modes, and the pack flags and counters between them. Edges are an entrance,
a lead (an exit, a trigger, a choice, a hook, a start), a write, a read (a
gate, an objective) and a giver offering its quest; lines, NPCs, formations
and the like fold into the node that holds them. The layout is longest-path
layers from the entry node with back edges set aside and barycentre sweeps
within a layer, drawn as SVG. A node the player cannot reach is dashed, one
with a flow lint finding is outlined with its count, and a click opens the
node's field; the findings are also listed under the graph. Chips hide a
kind, **findings only** fades the rest, and the zoom fits the graph to the
width.

**Help.** Each tab opens with the file it edits and a **?** to the format
reference's section for it, and each section heading with a reference
section of its own carries one too (the map detail boxes, a prop's
animators, the battle, quests, cards, items, arts, modes and audio
sections). The links open the site's format pages in a new tab.

The pack bar shows **unsaved changes** while the pack differs from what was
loaded or last downloaded, and the autosave's state (saved at a time, or
unavailable without browser storage). `Ctrl+S` downloads the `.lxpack`, `F8`
steps through the checks, `Alt+1` .. `Alt+0` and `Alt+[` / `Alt+]` switch
tabs, `?` lists the keys.

**New pack** starts empty (the manifest), from the **minimal template** (a
derived monster in a formation, a dialogue line, a quest to win the fight
and an 11 x 6 map whose trigger zones show the line and start the fight,
over the disc's first scene), or as a **clone of Rebirth** under the
modder's id, name and version 0.1.0; the clone's other files are Rebirth's
own bytes until edited.

## Point lists

A race's centre line (`points`, in the race's own units, or `waypoints`, the
tiles of its map) and a prop-hunt arena's `blockers` are rows of typed
numbers beside a 2D pad. The pad fits the arena's `bounds` (a race off a map
fits its points; a map race draws the map's tiles in the painter's
colours). A click on empty pad adds a point into the leg of the loop it
lengthens least (the end, while there are two points or fewer; a race's
`boost_segments` count legs, so check them after) and a drag draws a
blocker; a drag on a point or a blocker moves it; the row being edited is
outlined. Each row moves up and down, duplicates and removes, and its
validator path (`races[0].points[3]`) takes a check's click. A drag is one
step of the page's undo.

## Round trip

`crates/web-expansion/tests/rebirth_round_trip.rs` pins the export:

- an unedited Rebirth exports the `.lxpack` the CLI builds
  (`asset expansion bundle packs/rebirth`) byte for byte;
- an edit and its revert, in the manifest, a section, a cutscene and a map,
  export the loaded bytes again;
- every section file the page edits exports its edit;
- a shop row in either spelling (bare or `{ item }`) is the same row;
- opening every map's detail forms and the modes point lists, which fill an
  absent list with `[]` and the lighting and fog boxes with their defaults,
  exports the loaded bytes; so does a Duplicate and its Remove, a reorder
  and its reverse, and a material's kind switched and switched back;
- a battle stage unticked and ticked back, its tint box off and on, a fresh
  stage ticked and unticked, and a tint material re-coloured or switched to
  a cycle and put back, export the loaded bytes; each real edit touches its
  own file only.

A file keeps its loaded bytes while its canonical form (`to_files` of the
edited pack) equals the loaded pack's. An edited file is re-serialised
canonically: its comments go and its keys take the format's order. That is
the one loss, and it is per file - the files left alone keep theirs.

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
a base-scene mesh). Each stroke is one step of the page's undo (`Ctrl+Z` / `Ctrl+Y`; the painter keeps no history of its own); `Ctrl+wheel`
zooms; a right or middle drag, or Space with a drag, pans. A placed prop draws
as its mesh's footprint, rotated by its yaw.

A selected prop's **mesh source** is the base scene's pack (the mesh list),
another scene's pack (`{ scene, object }`), a PROT coordinate, or a glTF model
in the pack's `assets/`; **Import glTF** reads a `.glb`, `.gltf` or `.png`
from the user's machine into `assets/models/` (`ExpansionWorkbench::add_asset`,
kept in the session's extras with the pack's other files and exported with
it - nothing is uploaded). The prop's **animators** (spin, sway, bob, orbit,
pulse, each with its period and phase) are a list form, and its light a form.

The **Detail layer** box under the selection holds the map's `decor`,
`scatter`, `edges`, `decals` and `materials` as list forms, then its
`lighting` (mood and lamps), `fog` regions and particle `emitters`, plus
quick adds for a scatter brush over the whole map and a lamp at its centre
([`expansion-pack-maps.md`](../formats/expansion-pack-maps.md#decorations)).
Every item adds, duplicates (a fresh id), moves up and down and removes;
every field is the TOML key with its validator path. A decoration, brush,
edge run or decal names its mesh with the prop's mesh-source editor. A
tile or rectangle field has a **Pick** button: the painter shows a prompt and
the next click (a drag, for a rectangle) on the canvas fills the field
(`Esc` cancels). A lamp's tile has one, and so does a party night's podium on
the Modes tab: its Pick opens the podium's map in the painter and comes back
once the tile is clicked. A decal is placed once on a tile or brushed over a
rectangle, with its further `cells`; a material switches between palette
cycle, hue cycle, standing tint (hue turn, brightness shift and a
per-channel multiply with a swatch) and texel scroll, and its palette picks from the palettes
the base scene samples. A **texture cell** (a decal's, a scroll's) picks from
the cells the base scene's meshes sample (`scene_objects`' `cells`, labelled
page, palette, cell and prim count), takes the three words by hand, and
shows the page's 8 x 8 atlas as a grid of buttons, the sampled cells marked:
a coordinate and a count, never a texel. An NPC's form has a **Scenery**
checkbox (draws and plays its clip, no talk, no collision). The canvas draws
every decoration (green dots) and decal (blue dots) where the map bakes it:
`map_preview` expands the map's `decor`, `scatter`, `edges` and `decals` and
returns the positions (`decor`), so a brush's coverage shows as it is
edited. It builds the field image without decorations and with every
non-base prop mesh as mesh `0` (neither touches the collision grid it reads).

The **Terrain** box turns a map's `[terrain]` on and holds its knobs, the
water level, and the `materials`, `cliff`, `water`, `sky` and `shapes`
tables as JSON editors
([`expansion-pack-maps.md`](../formats/expansion-pack-maps.md#terrain)). The
**Sculpt** tool (`K`) writes strokes into `shapes`: a click stamps the
brush (raise, lower, smooth, or flatten to the mean under it), a drag with
raise or lower lays a ridge along it; **Remove last stroke** pops one. The
3D preview's **Sculpt** and **Paint** modes are the full brush set
([Editing in 3D](#editing-in-3d)). **Heightmap** imports a grayscale PNG
into `assets/terrain/` and adds a `heightmap` stroke naming it, black at
**Low** and white at **High** over the whole map; **Boundary detail** is
the terrain's `detail`. The
canvas shades the relief and the water from `map_preview`'s `terrain` - the
heightfield the engine's floor sampler and meshes are built from - and the
painter's wall quarters include the ones the terrain adds (too steep, too
deep), so what is walkable reads off the canvas. The 3D preview draws the
sculpted ground as the engine builds it, and its volumes stand on the
terrain's floor.

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
| Orbit | The free camera: the play page's debug orbit turned around a focus of its own (drag turns, wheel zooms out to a whole map, right or middle drag pans). Opens on the whole map. |
| Match painter | On (the default), the orbit view reads as the painter's grid: row 0 the far edge at the top, columns left to right. Off, the map from its low-Z side with the game's own handedness, as the field camera looks. |
| Arrival | Off (the default), the preview enters a map without its arrival cutscene and place-name banner. Ticked, the map is entered again at its spawn as a player arrives. |
| Player | The field camera: the follow view the game draws the map with, on the player. |
| Fit map | Back to the whole map in the orbit camera. |
| Walk from selection | Stands the player on the selected tile (`expansion_seat_player`) and switches to the field camera; the arrow keys walk in the picture. |
| 2D / Split / 3D | The layout on the Maps tab (kept per browser): the dock above the painter; the picture in a sticky column beside the painter and its forms; or the picture as large as the window allows, the painter's canvas put away and its forms under it. Split and 3D open the preview. |
| Layers | The tile grid, walls, encounter zones, triggers and exits, NPCs (with their patrol loops), spawns (with their facing), props: translucent volumes over the picture. |

**Orientation.** The game draws the world with screen X mirrored (the
retail horizontal flip, `buildWorldOrbitVp`), so its picture of a map seen
from above is the painter's grid flipped top to bottom - no camera turn makes
the two agree. *Match painter* turns the orbit to the map's high-Z side and
mirrors the frame's canvas back (a CSS flip, so the renderer's winding parity
is untouched): screen right is +X and row 0 is at the top, as painted. The
click-to-tile pick and the pan read the flip back (`mirrorView`); the Player
camera is never flipped.

**Arrivals.** The preview sets `SceneHost::quiet_expansion_arrivals`
(`LegaiaRuntime::expansion_set_quiet_arrivals`), so an entry skips the map's
`enter_cutscenes` and its place-name balloon; a player's entry, and the
preview with *Arrival* ticked, play both.

**Layouts.** The frame never moves in the page (moving an iframe reloads
it): Split and 3D are classes on the page (`xw-view-split`, `xw-view-3d`)
whose rules `expansion-workbench-preview3d.js` installs, and the framed page
resizes its canvas to the width it is shown at (4:3, 320 to 960 wide).

**The grid.** The Grid layer draws each tile's edges at its four corner
heights, and fills a cell whose corners disagree - a ramp, by the
[corner rule](../formats/expansion-pack-maps.md#tiers-are-corner-heights).
The heights are `map_preview`'s `corners` (the synthetic field image's
tiers), or on a terrain map its heightfield sampled at each tile corner, so
the grid is the one the Heights overlay shades in 2D.

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

**Drafts.** Between rebuilds the volumes follow the editor at once: each
`map_preview` the painter computes (a moment after an edit) goes to the
frame with the map's object lists (`xw-overlay` `grid` and `defs`), and the
frame draws that draft - spawns, exits, triggers, encounter zones, props
and NPCs, and each tile whose walls, void or corner heights differ from the
`map_preview` of the pack the last rebuild was sent (its floor the mean of
its corners) - in place of its reading of the last build. Tiles the edit
left alone keep the engine's reading. The rebuild's own reading replaces the draft when it lands. A
draft of a map of another size than the built one waits for the rebuild.

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
animations, and a boss runs its script, music and phases. A fight's
**Battle stage** box (absent until ticked) picks the scene from the disc's
blocks that carry battle-stage streams and the stage from that block's
streams (the catalog's `stages` per scene, listed without filling the PROT
entry cache, and `stage_pick`, the one left-out `index` means), with the
`keep_object_1` box and a tint: three channels, `128` neutral, and a swatch
that is also a picker (the colour a mid-grey texel takes). Its **Fight on
this stage in 3D** starts the fight on the stage as set. **Card faces in
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

### Editing in 3D

The **Edit** bar under the preview's head turns the picture into an editor.
**View** leaves it a picture (a click picks a tile, as above). The other
modes take the left button; Alt with a drag still turns the camera, and a
drag that starts on nothing in **Select** turns it too.

The editor belongs to the Maps tab: on the Cutscenes, Battle and Cards tabs
the frame drops to **View** and the bar hides (`suspendEdit`), and back on
Maps both return as they were. While a mode other than **View** is on, and
under the Orbit camera in any mode, the field HUD - the party readout and
the passive-ability badges - stays off the frame (`set_editor_hud_hidden`),
so it never covers the map being edited; the Player camera shows it.

| Mode | Does |
|---|---|
| Select | A click picks the object under it - a prop, decoration, NPC, spawn, lamp, emitter, trigger, exit or encounter zone; Shift or Ctrl adds or takes away. The selection gets a gizmo: **Move** (two arrows for one axis, a square for both; dragging an object moves it), **Rotate** (a ring: props and decorations in 15-degree steps, NPCs and spawns by facing sector), **Scale** (a corner handle: props and decorations; a zone's corner resizes it). A drag snaps to the tile or to the 32-unit sub-tile grid (**Snap**); Shift places freely. **Duplicate** (Ctrl+D) copies the selection one tile over (a prop to the nearest free tile); **Delete** removes it. |
| Sculpt | A ring cursor of the brush's radius rides the ground, an inner ring at its strength; `[` and `]` size it. A drag lays one stroke: raise, lower, smooth and flatten write a `stroke` along the drag (a click writes the single-point shape), flatten levelling to the height where the drag starts; ridge and ramp drag from one point to another (a ramp from the ground under its start to the ground under its end plus **Height**); plateau and noise drag a rectangle. |
| Paint | The tiles under the ring take a legend character: a terrain material's (named by the material) or any walkable style's. |

Objects stand on the ground: the frame samples the heightfield `map_preview`
returns (with the terrain map's `origin` and `step`) the way
`Heightfield::sample_world` does, or a tile map's floors from the overlay.
A prop moved onto a tile that holds another prop keeps its own tile and
reaches over with its `offset` (one prop a tile); NPCs and spawns stand on
whole tiles; zones move by whole tiles.

**Nothing in the frame writes the pack.** A finished drag is a message to
the workbench (`xw-transform` with each object's new world pose, `xw-sculpt`
with the shape, `xw-paint` with the tiles), which writes the map definition
the painter edits, one page-history step a drag (`ctx.commit` before and
after it), so the edit shows in the painter, its forms and the TOML at
once and `Ctrl+Z` takes it back from either view. The rebuild takes seconds on a big
map, so the frame draws a **proxy** until it lands: the objects' boxes at
their new poses (the start pose faint), the sculpted ground as a patch at
its new heights - the stroke run over the frame's copy of the heightfield
(`play-view-editor-gizmos.js` repeats the format's stroke kernels for this
only), tinted by which way it moved - and the painted tiles tinted.

The painter shares the selection: in its **Select** tool a drag on empty
ground draws a box that selects what it holds, Shift adds, a drag on one
object of several moves them all, and Delete and Ctrl+D act on the whole
selection.

The undo seam is `snapshot` and `refresh` in the editor's `api`
(`expansion-workbench-gizmos.js`, `mountMapEditor`): they close the page
history's step around each 3D edit (`ctx.commit`).

### Cost on a big map

Measured headless (Chromium with SwiftShader, the software GL, on the real
disc) on the largest map the format allows: 120 x 120 tiles, walls round the
edge and a wall pillar every 8 tiles, and a scatter brush at density 0.6 over
its top half - 1152 draws. Rebirth's atrium (15 x 13) is the small case.

| Step | Atrium | 120 x 120 |
|---|---|---|
| `map_preview`, `validate`, `export_bundle` (each edit, in the workbench) | - | 20 - 100 ms, 26 - 127 ms, 66 - 321 ms |
| Painter redraw on a hover | - | 1 - 6 ms |
| Hot reload, native build of the same call (`big_map_reload_cost`), of which the coplanar pass | - | 0.92 s, 0.30 s (2.5 s, 1.8 s with the bucket walk) |
| Hot reload, the page's GL upload after it (`PlayView._rebuild`) | - | 0.4 s |
| One preview frame: sim, scene sync and draw list, no GL | - | 4 ms |
| One preview frame under SwiftShader, `gl.finish` included | - | 46 - 244 ms |
| Editor overlay | - | 15 357 vertices |

The ranges are the same steps on a quiet and a busy machine. Every
workbench-side step stays a few frames long, so painting a big map stays
live. The **rebuild** of a big decorated map blocks the page while it runs -
hence the delay that grows with the last rebuild and the **Auto** switch. The
largest part of it was the scene build's cross-draw coplanar pass
([`renderer.md`](../subsystems/renderer.md#coplanar-surfaces-retails-ordering-model-the-ports-depth-policy)):
94 006 planes, each bucketed under 81 keys of a normal-and-distance
straddle. Walked bucket by bucket it sorted 7.6 million entries and met the
ground's tiles again in every bucket of the straddle (1.8 s natively, 12 s
before the in-bucket sweep). The pass now gathers each normal key's planes
once (`coplanar_draws::grouped_pair_hits`): two planes share a bucket of a key
exactly when their distance quanta along it are at most two apart, so a pair
is met once per key, cut on both in-plane axes, and the hits sorted by the
bucket the walk would have met them in give its order - the same pairs, the
same offsets, in 0.3 s. The headless **frame** is GL fill on a software
rasteriser, which the throttles above keep off the editor's input; a hardware
GPU draws the same 1152 draws in a few milliseconds.
`play_expansion_live::tests::big_map_reload_cost` (ignored, disc-gated)
builds the same map natively and splits the reload into the engine's map
entry and the page's scene build.
`isles_map_entry_cost` does the same for Rebirth's Isles maps, with a hash of
the arrays the page uploads, which the grouped pass leaves unchanged on every
map. The pass's share of each entry:

| Map | Coplanar pass, bucket walk | Grouped | Page build after |
|---|---|---|---|
| Thornwood (262 087 planes) | 4.9 s | 1.1 s | 3.1 s |
| Village, beach, grotto | 0.7 - 0.9 s | 0.2 - 0.3 s | 0.8 - 0.9 s |

On Thornwood what remains of the build is spread out: the plane summaries
(0.7 s, the meshes' own planes), the rest of the scene build (1.4 s) and the
uploaded arrays (1.5 s), beside 0.8 s of engine map entry.

## Sharing

**Open .lxpack** and a drop anywhere on the page (an `.lxpack`, or a pack
folder) go through `import_bundle`, which runs the
[import gates](../formats/expansion-pack.md#sharing-and-trust) before
anything is parsed; a refusal names its gate and what it means ("It needs a
newer engine than this one", "Packs are data only: ..."), and the session
keeps the pack it had. A folder drop is walked with the File and Directory
Entries API and loads like **Open folder**; a dropped disc image is pointed at
the disc picker.

**Share** opens a panel over the pack:

- **Identity**: the cover (set from a PNG, stored as `assets/cover.png`),
  the version, authors, the pack format and minimum engine against this
  engine, and each dependency as parsed. The Manifest tab edits the fields.
- **Compare with an older .lxpack**: the
  [diff](../formats/expansion-pack.md#diff-and-changelog) file by file and
  entry by entry, each changed field with its old and new value, the bump it
  calls for with a button that sets the suggested version, and an editable
  changelog draft to copy or put at the top of `CHANGELOG.md` (which then
  ships in the next export).
- **Download sources (.zip)**: the editable tree under one `<id>/` folder,
  deterministic; unzipped, **Open folder** reads it back.
- **Add to play library** stores the bundle as an installed pack and makes it
  active; **Play** is Play in browser.

## Play here

**Play here** plays the map the Maps tab is on in a play tab, for an
edit-and-look loop with the game's own controls (the native window's twin is
`--expansion-watch`, [below](#native-hot-reload)). It posts the bundle on
BroadcastChannel `legaia-expansion-live` as `{ type: "reload", nonce, id,
map, bytes }`; a play tab whose active pack has that id hot-reloads it in
place (`PlayView.reloadExpansion`, the preview's call) and answers `{ type:
"reloaded", nonce, ok, map, reentered, ms, errors }` - on the map it was on,
with the player where they stood. With no answer the bundle goes to the
hand-off slot below and a play tab opens as
`play.html?expansion=1&map=<id>`, which enters that map once the pack loads.
The opened tab is `play.html?expansion=1&lib=<id>&map=<id>`: the bundle went
into the pack library first (below). With **Live** ticked every edit goes to
the open play tab as it is made.

### Native hot reload

The native window reloads the pack it was started with in place: `End`
(`HostHotkey::ReloadPack`) re-reads `--expansion`'s folder or bundle, and
`--expansion-watch` polls the pack's files every half second (no file-system
notification dependency) and reloads once a change has held still for one
poll, so an editor's save-and-rename lands as one reload. The reload is the
page's: the validator first (a pack with an error is refused and the running
pack stays), the one activation path, then the map re-entered seated where
the player stood (`SceneHost::expansion_reload_seat`, the reading both hosts
take), the follow camera's framing kept and no banner, and the window's scene
render state rebuilt. In a fight or a screen it waits and is retried. Both
hosts word the outcome through `hotkeys::pack_reload_notice`; on the play page
`End` reloads the library's copy of the active pack the same way.

## Persistence and the play hand-off

The working pack autosaves to IndexedDB database `legaia-expansion-workbench`,
store `autosave`: key `current` holds `{ json, label, savedAt, tab, edited,
clean }`, key `source` the tree or bundle it was loaded from. `edited` says
whether the pack differs from what was opened or last downloaded, and
`clean` holds that baseline while it does. A returning visitor who loads a
disc is offered the pack back: an edited one as unsaved changes to resume (a
discard asks first), an unedited one as "pick up where you left off". A
resume reopens the tab it was on and keeps the **unsaved changes** mark. A
record without `edited` counts as edited. Every access is wrapped; without
storage the page still works.

**Play in browser** (and Play here with no play tab open) exports the bundle
and installs it in the play page's pack library below - an older version
under the same id replaced, the pack made active - then opens
`play.html?expansion=1&lib=<id>`, which plays the library's copy once its
disc is up through `LegaiaRuntime::load_expansion`, the same call as its own
*Install a pack* file input ([`browser-host.md`](../subsystems/browser-host.md)).
The bundle is also written to the older hand-off slot (database
`legaia-expansion`, store `packs`, key `play`, `{ id, name, version, bytes,
savedAt }`), which a play page opened as plain `?expansion=1` reads.

The play page's **installed packs** (`site/js/play-pack-library.js`) live in
the same database and store, so neither side bumps its schema version: key
`lib:<id>` holds `{ id, name, version, authors, description, requires,
conflicts, bytes, cover, installedAt }` and key `active` the active pack's id.
Each row runs the format's dependency check against the rest of the library
(`LegaiaRuntime::expansion_library_check`, over
`manifest::check_dependencies`) and lists an unmet requirement and a conflict
with an installed pack, the active one in red; the engine plays
[one pack at a time](../formats/expansion-pack.md#one-active-pack). Installing (the file
input, or a drop on the expansion card) reads the bundle through the import
gates with `LegaiaRuntime::expansion_bundle_info` / `expansion_bundle_cover`,
replaces any version installed under the same id, and plays it; the list shows
each pack's cover, version and authors with Play and Remove. With no
`?expansion=1` hand-off, the page loads the active pack once its disc is up;
**Unload** clears the active pack. Every load re-reads the stored bytes
through the engine, never trusting the stored copy.

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
- The same file, sharing (disc-free): a Rebirth `.lxpack` imported and
  exported again is the same bytes, and its sources zip holds every file; an
  empty section file survives the round trip; cutscene and audio edits reach
  the export; refused imports name their gate; the diff against an older
  bundle counts, bumps and drafts the changelog.
- `crates/web-expansion/tests/disc_catalog.rs` (`LEGAIA_DISC_BIN`): the catalog
  lists, Rebirth with no coordinate error on the real disc, the tower scene's
  geometry pack, the preview with the base scene's ladder.
