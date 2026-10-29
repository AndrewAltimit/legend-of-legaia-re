# Expansion-pack maps (`maps/<id>.toml`)

A pack map is a walkable field scene a modder lays out tile by tile, with no
disc bytes in the pack. It borrows its look from a retail **base scene** - the
scene's texture pages, palettes and geometry pack stay on the player's disc and
are named only by coordinate - and supplies everything that makes it a place:
walls, floor heights, props, NPCs, spawns, exits, trigger zones and
encounter zones.

Schema: [`legaia_expansion_format::map`](../../crates/expansion-format/src/map.rs).
Field image builder: [`legaia_engine_scene::expansion_map`](../../crates/engine-scene/src/expansion_map.rs).
Entry and per-tick step: `SceneHost::enter_expansion_map` in
[`engine-scene-host`](../../crates/engine-scene-host/src/scene_host/expansion_map.rs);
zone and NPC runtime: [`legaia_engine_expansion::map`](../../crates/engine-expansion/src/map.rs);
validation: [`legaia_expansion_format::map_rules`](../../crates/expansion-format/src/map_rules.rs).
Sample: [`packs/rebirth/maps/rebirth_tower_1f.toml`](../../packs/rebirth/maps/rebirth_tower_1f.toml).

## File layout

| Key | Meaning |
|---|---|
| `name` | Display name. |
| `size` | Optional `[width, height]` in tiles; must match `tiles.rows`. |
| `base` | `scene` (a `DiscRef::Scene`) and `geometry` (`"none"` or `"keep"`). |
| `floor_heights` | Up to 16 tier heights, world units, up positive. Empty takes the base scene's ladder. |
| `tiles` | `origin`, `rows`, `legend` - the painter's grid, below. |
| `props` | Placed meshes from the base scene's geometry pack; a prop with a `top` is standable. |
| `npcs` | Field NPCs wearing a retail placement's model, saying pack lines. |
| `spawns` | Named arrival points; the first is the default. |
| `exits` | Tile rectangles leading to a retail scene or another pack map. |
| `triggers` | Tile rectangles that raise a pack event (battle, dialog, quest). |
| `encounters` | Tile rectangles naming a pack encounter table. |
| `entrances` | Tile rectangles in retail field scenes that lead into the map. |
| `bgm` | A `DiscRef::Bgm` (sound-test id). |
| `camera` | `"default"`, `"retail"`, `"far"` or `"farther"` (the follow camera's distance presets). |

Every coordinate a map carries is a **map-local tile** `[column, row]`, counted
from the first character of the first row: column grows with world `+X`, row
with world `+Z`. A rectangle is inclusive, `[col0, row0, col1, row1]`.

## The tile layer

`rows` is a list of strings, one character per tile; `legend` maps each
character to a style. A space is void (no floor, every quarter wall) unless the
legend names it; a row shorter than the widest is padded with void. This pair
is the whole tile format, so a 2D painter reads and writes it 1:1: the painter's
palette is the legend and its canvas is the rows.

A style is:

```toml
"#" = { label = "wall", walls = "all" }
"h" = { walls = ["tl", "br"] }          # two of the four quarters
"^" = { height = 1, ground = { kind = "texture", tpage = 28, clut = 32576, tile = 0 } }
"." = { ground = { kind = "base_cell", cell = [8, 11] } }
"d" = { ground = { kind = "mesh", index = 4 } }
"W" = { walls = "all", wall = { kind = "box", height = 160, tpage = 28, clut = 32576, tile = 2 } }
```

- `walls` is `"none"`, `"all"`, or a list of quarters. A tile is four `64 x 64`
  sub-cells: `tl` low X / low Z, `tr` high X / low Z, `bl` low X / high Z, `br`
  high X / high Z - top-left to bottom-right in the painter's view.
- `height` is a tier, an index into `floor_heights`.
- `ground` is what draws on the floor: a `texture` (one `32 x 32` cell of the
  `8 x 8` atlas on texture page `tpage` through palette `clut`, all in the base
  scene's VRAM), a `base_cell` (the ground texture of one of the base scene's
  own cells, in its field-grid coordinates), or a `mesh` (a base-pack mesh
  stamped on the tile as a decoration cell). No `ground` draws nothing.
- `wall` is what stands on the tile so a modder can see it
  ([Visible walls](#visible-walls)). It draws only: collision stays `walls`.

### Tiers are corner heights

The retail floor is a vertex grid, not a set of flat tiles. The sampler
`FUN_80019278` reads a tile's tier as the height at its **low-X / low-Z
corner** and interpolates across the cell from the four corner tiles' tiers
([`field-locomotion.md`](../subsystems/field-locomotion.md#floor-height-two-models));
the ground mesh is built the same way. A cell is flat only where its four
corners agree, so a raised block of `N x N` tiles is flat over its first
`(N-1) x (N-1)` cells and slopes down along its high-X and high-Z edges. The
player walks the same surface the ground draws. Void tiles take the tier of a
floor neighbour above or to the left, so the map's own edges stay level.

## The base scene

The map is entered as an ordinary field entry of the base scene with a
synthetic field layer laid over it. The base scene's VRAM, its geometry pack
and its sound load as they would for the retail scene, which is what draws the
map. Its MAN does not: no entry script, NPCs, object binds, shops or encounter
table of the base scene's run. With `geometry = "keep"` the base scene's own
placed objects, decoration cells and ground draw under the map (a retail room
re-purposed with new walls and zones); with `"none"` only the map's own ground
and props draw.

## The synthetic field image

The engine bakes the tile layer into a `.MAP`-shaped image in the layout of
[`field-map.md`](field-map.md) and seats it on the scene; every `.MAP` reader
(collision grid, floor sampler, placements, decoration cells, ground) parses it
unchanged. The image is the modder's layout run through the documented format.

| Region | What the map writes |
|---|---|
| `+0x04000` collision grid | Tile `(c, r)`'s tier in byte `(c, r)`'s low nibble; its wall quarters in byte `(c, r + 1)`'s high nibble. Everything off the map is wall. |
| `+0x00000` object records | One record per distinct ground texture, decoration mesh and prop. |
| `+0x08000` object cells | Each tile's record index; the draw gates are derived by the grid-mark refresh. |
| `+0x10000` trigger block | The empty four-table header chain. |
| `+0x12000` fallback window | Zeros (no `.PCH` triggers). |

The one-row shift of the wall nibble is the wall probe's `+2` Z bias
([`field-locomotion.md`](../subsystems/field-locomotion.md#wall-probes)): the
probe reads a point's wall bits from the grid row below the one its floor
nibble sits in, so a wall painted on tile row `r` is stored in byte row `r + 1`.
That is why the map keeps one free grid row below it (`origin` row plus height
must stay under 127).

The draw gates come out of the record flags the way they do on a retail map.
`FUN_80017BEC` mirrors flag bit `0x1` into cell bit `0x1000` (the ground
heightfield's gate) and bit `0x2` into `0x2000` (the decoration / placed-object
draw gate), and clears flag bit `0x1` when the record's `+0x16` countdown is
zero. On a ground record `+0x16` is the palette word, which is never zero for a
real palette, so a textured tile keeps its gate.

| Record | Flags | Fields |
|---|---|---|
| Ground texture | `0x0001` | `+0x14` atlas cell, `+0x15` tpage, `+0x16` clut |
| Decoration mesh | `0x0012` | `+0x10` pack mesh |
| Wall | `0x0012` (`0x0013` on a textured tile) | `+0x10` wall mesh, the tile's ground texture |
| Prop | `0x0016` (`0x0017` on a textured tile) | `+0x00..+0x0C` offset and rotation, `+0x10` pack mesh, the tile's ground texture |

A cell names one record, so a prop's record also carries the ground texture of
the tile it stands on, and a tile holds at most one prop.

## Props

```toml
[[props]]
mesh = { object = 7 }                       # base geometry pack mesh 7
tile = [2, 2]
offset = [0, 0, 0]                          # world units from the tile centre, y up
rotation = [0.0, 45.0, 0.0]                 # degrees; y is the yaw
scale = 0.75                                # uniform; 1 when left out
solid = true                                # close the prop's tile
top = 64                                    # standable: a floor 64 units up
footprint = [128, 128]                      # the top's extent, world units
```

`mesh` is either `{ object = N }` or `{ tmd = { kind = "tmd", prot = P, index = N } }`.
A TMD coordinate resolves only when `P` is the PROT entry holding the base
scene's geometry pack (its scene-asset bundle); anything else is refused at
entry. `scale` folds into the prop's model matrix after its rotation, where a
retail placed object's render scale lands. Props carry no collision box of
their own: `solid` closes the tile.

### Standable props

A prop with a `top` is a floor box: a flat surface `top` world units up (the
`floor_heights` convention) over its `footprint`, centred on the tile centre
plus `offset`, the whole tile when the footprint is left out. It does not
close its tile, so `solid` is unread.

The floor sampler (`World::sample_field_floor_height`, the
`FUN_80019278` port, and its camera-ladder sibling) answers the higher of
the grid floor and the top inside the box. The tier grid is untouched: a
top is not a corner height, so it neither slopes into its neighbours nor
bends the ground mesh around it, which is why it is a separate surface
rather than a raised tier ([Tiers are corner heights](#tiers-are-corner-heights)).
The prop's mesh draws on the tile's floor as before; matching `top` to the
mesh's visible height is the modder's call.

| Mover | On a standable prop |
|---|---|
| Action movement | Stands, walks across, climbs a rise up to its step height, jumps onto a taller top and lands on it, falls off its edge. |
| The retail walk | Refused at the edge of any top higher than the one it stands on: the walk climbs nothing. Walking off a top is not refused. |
| NPC patrols | Walk the sampled floor; route them around a top. |

## NPCs

```toml
[[npcs]]
id = "tower_guide"                              # the quest talk key
model = { kind = "npc", scene = "tower", index = 3 }
tile = [7, 8]
facing = 0                                      # compass sector, 0..8
clip = 2                                        # optional idle clip id
patrol = [[9, 8], [9, 6]]                       # optional loop of tiles
walk_clip = 4                                   # optional clip a patrol leg plays
line = "tower_guide_hello"                      # a dialog.toml line
shop = "tower_dealer"                           # optional: a cards.toml dealer
```

An NPC is not an asset: a retail NPC is one record of its scene's MAN actor
placement list, whose model byte indexes the scene's model pool and whose
clip byte names a record of the scene's ANM bundle
([`anm.md`](anm.md), [`script-vm.md`](../subsystems/script-vm.md)). A pack NPC
borrows one by coordinate, `DiscRef::Npc { scene, index }`, the placement's
partition-1 index (the site's NPC page lists it as the NPC's slot; `0` is
the scene controller). Its model and idle clip come from that record; `clip`
overrides the clip with another record of the same bundle (id = record + 1).
The model scene must be the base scene - the mesh indexes the base scene's
pool and samples its VRAM - unless the placement is a party member or save
point (model byte `0xF0` and up), whose mesh is the resident global pool.

At entry the engine reads each NPC's placement off the disc
(`legaia_engine_scene::expansion_map::resolve_map_npcs`) and seats it as an
actor placement of its own, slot `1 + n` for the map's `n`th NPC
(`map::npc_slot`): the pack map runs no MAN, so nothing else holds those
slots. From there the NPC is a field NPC on every path a retail one takes:

| Path | What reads the pack NPC |
|---|---|
| Draw | The live-scene placement list (`npc_catalog::live_scene_placements`, `catalog_live_scene_npcs`) both hosts build their NPC meshes and clips from, posed by the base scene's ANM bundle. |
| Collision | Its anchor in `World::npcs.positions`: the moving-actor box (`FUN_801CFC40`'s moving arm, ±40 units) blocks the player as a village NPC does. |
| Talk | The facing probe (`FUN_801CF9F4`'s port) picks it on Cross; `World::trigger_field_interact` turns it to the player and hands the talk to the map. |
| Patrol | A loop of scripted walk legs through the motion VM (`World::start_field_npc_motion`): tile, each waypoint, back, each leg cueing `walk_clip` the way a `4C 51` walk cues its move clip. A leg starts only while no dialogue holds the field. |

Talking to an NPC runs its `line` through the pack dialogue runner
(`World::expansion_say`) and then raises the quest event `Talked(id)`
(`World::expansion_talked`), so a quest naming the NPC as `giver` or in a
`talk` objective advances; a quest line the talk starts plays after the
NPC's own. An NPC with a `shop` (a `cards.toml` `[[dealers]]` id) opens that
dealer's counter instead (`World::open_card_dealer`, the `card_dealer`
trigger's path), and says its `line` only when the counter cannot open.

## Visible walls

A legend style's `wall` draws on every tile of that style:

- `{ kind = "box", height, tpage, clut, tile }` - a box the tile's size,
  `height` world units tall, each side and the top textured with one `32 x 32`
  atlas cell of the base scene's VRAM (the three words of a `texture`
  ground). Faces taller than `128` units stack.
- `{ kind = "mesh", index }` - the base scene's geometry-pack mesh `index`.

The box is built when the map loads
(`legaia_engine_scene::expansion_map::wall_box_tmd`): the pack carries the
five numbers, and the engine runs them through the Legaia TMD encoder. The
scene's resource build appends each distinct box to the base scene's
geometry pack after the pack's own meshes, so box `k` is pack mesh
`geometry_len + k`, where `geometry_len` is the pack's length as the field
load's descriptor walk registers it
(`scene_resources::field_geometry_pack`). Each wall tile is a decoration
cell naming that mesh, which both hosts draw through the ordinary
decoration-cell path. A scene whose geometry the descriptor walk cannot
enumerate has no length to append to, and a box on it is refused at entry.
A tile holds one cell mesh, so a style cannot carry both a `mesh` ground
and a `wall`.

## Validation

The validator (`legaia_expansion_format::validate`) checks each map against
the sections it names, with or without a disc:

| Check | Severity |
|---|---|
| `tiles` resolve (legend, sizes, tiers); at most 16 floor tiers | error |
| Every spawn, exit, zone, prop, NPC tile and patrol waypoint lies on the grid | error |
| The map has a spawn; spawn, exit, trigger, encounter and prop ids are unique in the map | error |
| NPC ids are unique across the pack | error |
| A trigger's `battle` / `boss` / `dialog` / `start_quest` / `card_dealer` names a defined formation, boss, line, quest, dealer | error |
| An NPC's `line` and `shop` name a defined line and dealer | error |
| An encounter zone's `table` is a `battle.toml` `[[encounters]]` `scene` | error |
| ... whose table has no `steps` (it can never roll on a map) | warning |
| A `talk` trigger's NPC id that no quest gives from or talks to | warning |
| An exit to another map names a map of the pack and a spawn it has | error |
| Facings are sectors `0..8`; a retail exit's tile is on the `128 x 128` grid | error |
| Disc refs: the base scene, retail exit scenes, NPC models, BGM, prop TMDs (range rules, then the disc when one is loaded) | error |
| One prop and one NPC a tile; a style with both a `mesh` ground and a `wall` | error |
| Wall box height `1..=512`; standable footprint `1..=1024` a side | error |
| An NPC model from a scene other than the base scene | warning |
| A spawn, NPC or waypoint on a void or fully walled tile; an NPC on a prop's tile | warning |
| With a disc: a geometry-pack index (`{ object = N }` props, `mesh` grounds and walls) past the base scene's pack | error |

## Exits, triggers and encounter zones

Zones fire on **entry** - stepping onto a tile of the rectangle from outside it
- the way a retail walk-on trigger fires on a tile crossing. Standing still
fires nothing, and the spawn tile does not fire on arrival.

### Exits

- `to = { kind = "retail", scene = { kind = "scene", name = "town01" }, tile = [20, 44], facing = 4 }`
  raises the named scene change a door's op `0x3F` raises, so it parks behind
  the streaming hold and arrives on that field-grid tile facing that compass
  sector. With no `tile` the player lands where the scene's own doors land them.
- `to = { kind = "map", map = "...", spawn = "..." }` enters another map of the
  same pack at once.

### Triggers

A trigger's `event` names content of another pack section, and the scene host
hands it to that section's runtime the tick it fires:

| `event` | Runs |
|---|---|
| `{ kind = "battle", formation = F }` | `World::start_expansion_battle(F)` - a `battle.toml` formation |
| `{ kind = "boss", boss = B }` | `World::start_expansion_boss(B)` - a `battle.toml` boss |
| `{ kind = "dialog", line = L }` | `World::expansion_say(L)` - a `dialog.toml` line |
| `{ kind = "talk", npc = N }` | `World::expansion_talked(N)` - a quest's talk objective |
| `{ kind = "start_quest", quest = Q }` | `World::expansion_start_quest(Q)` |
| `{ kind = "set_flag", flag = X }` | raises pack flag `X` |
| `{ kind = "counter", counter = C, add = 1 }` | adds to pack counter `C` |
| `{ kind = "card_dealer", dealer = D }` | opens the `cards.toml` dealer `D`'s counter (`World::open_card_dealer`; empty = the first dealer) |

`once = true` fires a trigger once **ever**: firing raises the pack flag
`map.<map id>.<trigger id>` (`map.<map id>.<index>` for a trigger with no id),
which saves with the pack's progress, and a trigger whose flag is up stays
quiet on every later visit. Quests can read the flag like any other.

### Encounter zones

A zone's `table` names a `battle.toml` `[[encounters]]` entry by its `scene`
key - a free string, so a pack map's tables sit beside the retail scenes'
overrides:

```toml
# battle.toml
[[encounters]]
scene = "tower_1f"
steps = [12, 24]
formations = [{ formation = "ogre_pack", weight = 2 }, { formation = "forge_guard", weight = 1 }]
```

On a map, the pack's encounter lookup keys on the table of the zone under the
player instead of the scene label, so the pack's own step counter and formation
pick - the same roll a retail scene override uses - run while the player walks
inside a zone. Outside every zone the map rolls nothing; it never rolls the
base scene's retail encounters. A table needs `steps` to roll on a map: with
no retail rate to borrow, a rate-less entry has nothing to fight on.

## Entrances

An entrance is the way into a map from the retail game: a rectangle of
field-grid tiles in a retail scene that enters the map when the player steps
onto it. It lives on the map it leads into.

```toml
[[entrances]]
id = "rim_elm_mist_stair"
scene = { kind = "scene", name = "town01" }
rect = [17, 43, 18, 43]
spawn = "entrance"
requires = { min_level = 30 }
```

| Key | Meaning |
|---|---|
| `id` | Name, for logs. |
| `scene` | The retail scene (`DiscRef::Scene`). |
| `rect` | Inclusive `[x0, z0, x1, z1]` in the scene's `128 x 128` field grid, `0..=127` per axis. |
| `spawn` | The spawn of this map the player arrives at; the first when empty. |
| `requires` | The quest prerequisite shape: `story_flags`, `story_flags_clear`, `flags`, `quests`, `min_level`. Empty is always open. |

Coordinates here are **retail field-grid tiles**, not map-local ones: tile
`x` covers world `x * 128 .. x * 128 + 127`, the `world >> 7` quantisation the
retail walk-on trigger dispatcher uses
([`field-locomotion.md`](../subsystems/field-locomotion.md)). An exit's
arrival `tile` is in the same space, so a map's exit can land beside its own
entrance.

An entrance fires the way a zone does: on a step onto its rectangle from
outside it, while the gate holds, in a plain field tick the player walks
freely (no pack map, no scene change parked, no dialogue or cutscene holding
the player). Any other tick forgets the last tile, so arriving on an entrance
(a door, a battle return, a Load) does not fire it. A shut entrance stays
shut until the player steps off and back on after it opens.
`World::step_expansion_entrance` holds the rule and `SceneHost` enters the
map it names (`scene_host/expansion_entrance.rs`); the kernel is
`legaia_engine_expansion::entrance::EntranceWatch`.

Pick a rectangle the scene's walk reaches and that no retail trigger shares:
a retail door's walk-on tile fires its own record on the same step. The
validator checks the scene coordinate, the tile range, the spawn and the
quests `requires` names; it cannot see the scene's walls without the disc.

The pause menu's **Travel** page (the Journal, Select on the root list)
enters any of the pack's maps directly, which is the way in for testing a map
whose entrance is out of reach ([`field-menu.md`](../subsystems/field-menu.md#expansion-pack-journal-engine-extension)).

## Saving on a map

A save made on a map writes the map id, the player's world position and
facing as tail record 2 of the pack's expansion block (`LXP1`,
[`expansion-pack-quests.md`](expansion-pack-quests.md#saves)); a save made off
every map writes no such record, so its block is the one it was before. The
save's resume scene is the base scene, so a Load lands there first;
`BootSession::resume_save` - the native window's resume and the browser
page's `play_resume_save` alike - then re-enters the map with the player
seated where the save was made. Without the pack active the save lands in the
base scene.

## Running a map

```bash
legaia-engine play-window --disc "Legend of Legaia (USA).bin" \
    --expansion packs/rebirth --map rebirth_tower_1f [--spawn dais]
```

`--expansion` takes a pack directory or an `.lxpack` bundle and activates it
through `World::activate_expansion`; with `--resume-save` the save's own map
(if any) is the landing instead of `--map`. Without `--map` the pack is simply
active, for the title's New Game and Continue. The browser play page loads an
`.lxpack` through `LegaiaRuntime::load_expansion` and enters a map through
`enter_expansion_map`, the same `SceneHost::enter_expansion_map` entry; its
scene build reads the map's ground, walls and props off the same `Scene`
([`browser-host.md`](../subsystems/browser-host.md)). Any scene change - an exit, a
scene picker - leaves the map and drops its layer; a battle fought on the map
returns to it. The map's `name` shows as the place-name banner on entry.

## Gaps

- **Props from other scenes.** A prop mesh must come from the base scene's
  geometry pack; a TMD from another PROT entry (and the VRAM it samples) is not
  loaded.
- **Camera preset on an in-world entry.** A map entered through an entrance
  or a Travel pick keeps the follow camera's distance; the map's `camera`
  preset applies on a host's direct entry (`--map`, the play page's
  *Enter map*).
- **NPC clips.** A patrolling NPC keeps `walk_clip` (or its idle clip) at the
  ends of its legs; nothing cues the idle clip back between legs.
