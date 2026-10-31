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
validation: [`legaia_expansion_format::map_rules`](../../crates/expansion-format/src/map_rules.rs);
the detail layer: [`legaia_expansion_format::map_detail`](../../crates/expansion-format/src/map_detail.rs).
Sample: [`packs/rebirth/maps/rebirth_tower_1f.toml`](../../packs/rebirth/maps/rebirth_tower_1f.toml).

## File layout

| Key | Meaning |
|---|---|
| `name` | Display name. |
| `size` | Optional `[width, height]` in tiles; must match `tiles.rows`. |
| `base` | `scene` (a `DiscRef::Scene`), `geometry` (`"none"` or `"keep"`) and `scenery` (seat the base scene's animated props, [Disc animation clips](#disc-animation-clips)). |
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
| `decor`, `scatter`, `edges` | Decorations: any number of meshes a tile, placed one by one, by seeded brush, or along wall edges ([Decorations](#decorations)). |
| `lighting` | Enhanced-lighting mood and placed lamps ([Lights](#lights)). |
| `materials` | Animated texture words: palette cycling, hue cycling, texel scrolling ([Animated materials](#animated-materials)). |
| `fog` | The retail field fog pool over tile regions ([Fog](#fog)). |
| `decals` | Flat textured floor polygons, one by one or by brush ([Decals](#decals)). |
| `emitters` | Particle emitters ([Particles](#particles)). |
| `terrain` | A sculpted floor in place of the tier grid: heights, ground materials, water, horizon ([Terrain](#terrain)). |
| `surround` | The ground past the floor's edge: a drop below it and the edge faces down to it ([Surround](#surround)). |

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

### A kept room's floor

Many retail rooms raise no floor through the tier grid at all (the tower
floors' bases `teien`, `taiku2`, `korb3` and `chitei2` hold tier `0` on every
tile): their stairs, daises and terraces stand on cells flagged `0x800` whose kind-2 trigger records give the step
([`field-locomotion.md`](../subsystems/field-locomotion.md#floor-height-two-models)).
The step is built into the room's own meshes, so a kept room
(`geometry = "keep"`) walks right only with the records. The image keeps
them: the base map's kind-2 records whose tile lies on the pack map go into
the image's trigger block, the copied cells keep their `0x800` bit (a pack
record written over one keeps it), and the floor sampler steps the player
up the retail stair as it does in the retail room.

The same step lifts what stands on the tile. A prop, a decoration, a
scattered mesh and a decal on a flagged tile stand on its tier height plus
the record's step at their own position
(`legaia_engine_scene::expansion_map::KeptFloor`), so `offset[1] = 0` is
the floor the player walks everywhere, on a kept room as on a map of its
own. The workbench's map preview reads the result back: its `floor` array
is each tile's walk height at its centre and `kept_steps` the records the
room carries.

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
| `+0x10000` trigger block | The four-table header chain: empty, except that a kept room carries its base map's kind-2 elevation records ([A kept room's floor](#a-kept-rooms-floor)). |
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
solid = true                                # close the ground under the mesh
top = 64                                    # standable: a floor 64 units up
footprint = [128, 128]                      # the top's extent, world units
on = "table"                                # rest on that prop's or decoration's top
hangs = false                               # hung or wall-mounted: not on anything
```

`mesh` takes four forms ([`PropMesh`](../../crates/expansion-format/src/map.rs)):

| Form | Mesh |
|---|---|
| `{ object = N }` | Mesh `N` of the base scene's geometry pack. |
| `{ scene = "town01", object = N }` | Mesh `N` of any field scene's geometry pack ([Meshes from other scenes](#meshes-from-other-scenes)). |
| `{ tmd = { kind = "tmd", prot = P, index = N } }` | Mesh `N` of the geometry pack held in PROT entry `P`, whichever scene's block holds it. |
| `{ asset = "models/x.glb", colors = 256 }` | A glTF model the pack carries ([Model assets](#model-assets)). |

`scale` folds into the prop's model matrix after its rotation, where a
retail placed object's render scale lands.

`animate` is a list of procedural animators and `light` a light the prop
carries ([Animated props](#animated-props), [Lights](#lights)). A cell
names one record, so a prop whose tile's cell already draws a mesh - a kept
retail cell, a wall, a mesh ground - anchors its record on the nearest tile
within three whose cell draws none, its offset carrying the draw back to
where it was placed (`FieldImage::prop_anchors`); the mesh keeps drawing.
Only with no free tile in reach does the prop replace it, and the entry
logs the tiles that happened to. Its solid tile is always its own.

The prop's record carries its anchor tile's ground texture: the tile
style's, or, on a kept map whose style names none, the texture the cell's
retail record already drew, so a prop on a kept floor leaves no hole. The
builder never writes records `1..=3`: those object ids name the
protagonist and NPC meshes, which the field draw never takes from the
scene pack. A prop's `offset[1]` is from the floor the player walks on its
tile: its tier, plus in a kept room the retail floor step there
([A kept room's floor](#a-kept-rooms-floor)). `0` stands the mesh's origin
on the floor.

### Solid props

`solid` closes the ground the mesh stands on. The footprint is the convex
hull, in the floor plane, of the mesh's vertices in the lower half of its
height once rotated and scaled
([`footprint`](../../crates/engine-scene/src/expansion_map/footprint.rs)):
a cottage's walls and not its overhanging eaves, a tree's trunk and lower
boughs and not its whole canopy. Every `64 x 64` wall sub-cell whose centre
lies inside the hull is closed, and so is the prop's own tile, so a house
turned to any angle blocks where it draws without the tile layer having to
trace it. The player's wall probe leads it by 47 units, so it stops just
short of the walls.

A prop that stands in the player's way - more than a tile and a half across
on an axis, taller than a step, and reaching down to body height - but is
neither `solid` nor standable is logged at entry
(`FieldImage::open_large_props`): drawn buildings nobody closed. The
disc-gated `expansion_solid_props_disc` test holds the flagship pack to it,
and walks the player at each solid prop from every side.

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

### Resting placement

A prop or a `[[decor]]` that stands on something names it: `on = "<id>"`, the
id of a prop or a decoration of the same map. Its lowest point is set on the
support's top - a standable prop's `top`, else the highest point of the
support's mesh - and `offset[1]` lifts it from there, so a cup is placed on a
table without knowing how tall the table's mesh is. A support may itself rest
on another. The engine resolves it when the map loads, from the meshes'
vertices ([`map_rest::rest_on`](../../crates/expansion-format/src/map_rest.rs)),
and builds and runs the map from the resolved offsets; an `on` naming no item,
or resting an item on itself, is a validation error.

The placement is also checked
([`map_rest::rest_issues`](../../crates/expansion-format/src/map_rest.rs)). An
item whose base is more than `10` units over the highest ground under its
footprint must be on the top of a support its footprint is over: a prop or
decoration (the floor-plane hull of its mesh, or a standable top's
footprint), or a tile whose whole area is a wall box (a counter, a ledge).
The support is the one holding the most of the item's footprint (of those
holding as much, the one whose top is nearest its base), not the one under
its origin, so a mesh built off its origin is judged where it draws.
Otherwise it is reported:

| Report | Means |
|---|---|
| floats N units over the ground | Nothing is under it. Rest it with `on`, or mark it `hangs` (a lantern on a chain, a banner on a wall). |
| hovers N units over / sinks N units into the top of X | It stands over X but not on its top. Sunk, it is reported only when X holds at least half its footprint: a rock leaning into the next one of a pile is not a mug sunk into a table. |
| reaches N units past the edge of X | It rests on X and is wider than X's top by more than `12` units. |

The base is the mesh's lowest point, except on a flat card - a picture
drawn on one upright plane, a flame or a sprite tree, whose floor-plane
hull has no area. Its lowest vertex says little about where the picture
stands (`rugi`'s torch card, object 15, reaches 64 units below its origin,
under the floor of any tile it is set on), so a card is read from its
origin when it spans the origin, else from its nearer end
([`map_rest::visual_base`](../../crates/expansion-format/src/map_rest.rs)).

Every placement of a `[[scatter]]` mesh and of an `[[edges]]` run is
judged the same way, and each brush mesh or run is reported once, at its
worst placement, with how many placements rest badly
(`scatter[2].meshes[0] ("grass"), 14 placements, the worst floats ...`).
A brush has no `on`; its fix is its `y`, or `hangs = true` on the brush or
the run (a run of sconces mounted on the walls it follows). Brush
placements are judged, never supports.

A mesh's origin is not always its base: a retail mesh can be built standing
well above its origin (a plant pot whose lowest vertex is `91` units up), so
the check measures the vertices rather than trusting `offset[1]`. That needs
the meshes, so it runs where they are: the engine logs each report when it
prepares the map (`expansion: map '<id>': rest: ...`), the disc-gated
`expansion_rest_rebirth_disc` test lists the flagship pack's, and the
validator warns with a resolver that measures meshes
(`DiscResolver::mesh_shape`, which the workbench's disc catalog implements
off each scene's geometry pack); without one it judges nothing. An item or
support whose mesh is not measured (a model asset in the workbench) is not
judged either.
[`map_rest::rest_report`](../../crates/expansion-format/src/map_rest.rs) is
the editor's side of the same reading: each prop's and decoration's
resolved height, base and top, and the surfaces under its footprint, most
held first, with the `offset[1]` that rests it on each.

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
pause = 90                                      # optional ticks it stands at each stop
line = "tower_guide_hello"                      # a dialog.toml line
shop = "tower_dealer"                           # optional: an items.toml shop or a cards.toml dealer
requires = { quests = ["tower_rumour"] }        # optional: silent until it holds
line_after = "tower_guide_thanks"               # optional: said once `after` holds
after = { bosses = ["cinder_warden"] }
cutscene = "guide_story"                        # optional: played in place of the line
present = { flags = ["cutscene.tower_intro"] }  # optional: off the map until it holds
```

An NPC is not an asset: a retail NPC is one record of its scene's MAN actor
placement list, whose model byte indexes the scene's model pool and whose
clip byte names a record of the scene's ANM bundle
([`anm.md`](anm.md), [`script-vm.md`](../subsystems/script-vm.md)). A pack NPC
borrows one by coordinate, `DiscRef::Npc { scene, index }`, the placement's
partition-1 index (the site's NPC page lists it as the NPC's slot; `0` is
the scene controller). Its model comes from that record; `clip` names its
idle clip, another record of the same bundle (id = record + 1).

Left out, the clips come from the placement's own motion stream where it
installs a default-move record (op `0x17` `[move, anim]`,
`legaia_scene_formats::man_motion::placement_default_move`): the actor
rests in `move` and walks in `anim`, so `clip` takes the first and a
patrolling NPC's `walk_clip` the second
(`expansion_map::fill_borrowed_npc_clips`, at entry). The placement's clip
byte is not always the rest clip - Rim Elm's old man (`town01` placement
37) is placed on his walk cycle, which his stream replaces on its first
tick - and a pack map runs no stream, so a borrowed NPC without this marks
time in place and a patrol glides in its idle pose. A placement whose
stream installs nothing keeps its clip byte.
A placement's mesh is its model byte's entry of its scene's model pool and
its clips are records of that scene's ANM bundle. A party member or save
point (model byte `0xF0` and up) draws from the resident global pool in
any scene. Townsfolk of another scene than the base are carried in
(`expansion_map::detail::NpcBank`): each model joins the map's pool right
after the base scene's own models, its textures copied into free VRAM the
way a [foreign mesh's](#meshes-from-other-scenes) are, and the borrowed
scene's ANM bundle stands in for the base scene's
(`npc_catalog::scene_anm_bundle`), so the clip ids a pack writes for them
are that scene's own. One bundle poses a map's townsfolk, so they come
from one scene - the base, or one other - and a map borrowing from another
scene cannot also keep the base scene's animated props (`base.scenery`);
entry refuses either mix. The Thornwood's hermit is Rim Elm's old man over
the West Voz Forest, which has no townsfolk of its own.

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
| Patrol | A loop of scripted walk legs through the motion VM (`World::start_field_npc_motion`): tile, each waypoint, back, each leg cueing `walk_clip` the way a `4C 51` walk cues its move clip. At each stop the NPC turns back to `clip` and stands `pause` ticks (`ActiveMap::patrol_tick`). A leg starts only while no dialogue holds the field. |

Talking to an NPC raises the quest event `Talked(id)`
(`World::expansion_talked`), so a quest naming the NPC as `giver` or in a
`talk` objective advances, and runs its `line` through the pack dialogue
runner (`World::expansion_say`). When the talk starts, completes or reminds
a quest that has a line of its own, that line is the answer and the NPC's
`line` is not said before it: one box per talk. An NPC with a `shop` opens that counter instead: an
`items.toml` `[[shops]]` id opens the pack item shop (`World::open_item_shop`,
the `shop` trigger's path, [items](expansion-pack-items.md#the-item-screen)),
a `cards.toml` `[[dealers]]` id the dealer's counter
(`World::open_card_dealer`, the `card_dealer` trigger's path); an id that is
both opens the item shop. It says its `line` only when the counter cannot
open.
While its `requires` gate is shut the NPC says nothing and opens no shop
(the talk still counts as `Talked(id)`); once its `after` gate holds it says
`line_after` in place of `line` (`ActiveMap::talk_gated`). An NPC with a
`cutscene` plays that `cutscenes.toml` cutscene in place of its line
([`expansion-pack-cutscenes.md`](expansion-pack-cutscenes.md)). One whose
`present` gate is shut is off the map for the visit - parked on retail's
off-map hide box (`World::expansion_hide_absent_npcs`) - until a cutscene's
`show` step brings it on.

### NPC clip postures

A clip id says nothing about the pose it plays: a borrowed placement's ANM
bundle holds standing idles, gestures, walk cycles and seated poses side by
side, and a seated clip on a bare tile draws an NPC sitting on air (the
`koin1` slot players' own clip is one). The posture table
([`legaia_expansion_format::npc_postures`](../../crates/expansion-format/src/npc_postures.rs),
`crates/expansion-format/data/npc_postures.toml`) lists, for each
placement of the scenes it covers, every clip the model can play (bone
count equal to the model's object count) under the pose it measures as.
It holds disc coordinates and clip ids only.

The measurement
([`legaia_web_expansion::npc_posture`](../../crates/web-expansion/src/npc_posture.rs))
poses each clip frame by frame through the placement's mesh the way both
hosts draw it and reads three numbers off the posed vertices: the clip's
mean height, how far its lowest point rises above the model's standing
floor, and how far the lower body travels from frame 0. The model's
standing height and floor are the upper quartile over its clips: a bundle
mixes clips authored for other rigs of the same bone count, whose origins
sit at other heights, so the reference is the model's own clips rather
than the actor origin.

| Pose | Rule |
|---|---|
| `seated` | Lowest point at least 20 units above the model's standing floor, height at most 0.85 of the standing height. |
| `crouched` | Height at most 0.8 of the standing height, not lifted. |
| `walking` | Lower body travels at least 25 units. |
| `standing` | Everything else: idles and upper-body gestures. |

The example `cargo run -p legaia-web-expansion --example npc_postures --
extracted <scene>...` regenerates the table, and a disc-gated test checks
the committed one against the disc. The validator reads it (rows below), and
the workbench's NPC form lists the borrowed placement's clips by pose,
measured live off the loaded disc. A pose the table names is the vertices'
shape, not the animator's intent: a clip authored for another model of the
same bone count is listed under whatever shape it gives this one, and a
placement that is not a figure (town01's windmill sails) is classed all
the same, which is why the validator skips `scenery` NPCs.

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

A wall cell draws at the mean of its four corner tiles' floor heights, as
every decoration cell does, so a box on the edge between two floors would
stand clear of the lower one. The box reaches on down to its cell's lowest
corner (`wall_box_tmd_from`), which is what lets a wall take the tier of
the floor on its north or west side: tiers are corner heights
([Tiers are corner heights](#tiers-are-corner-heights)), so a wall on
another tier than that floor would tilt the floor's last row up or down
into it, while the wall's own cell can slope unseen under its box. A wall
between a terrace and the lower hall south of it stands on the terrace's
tier and meets the hall's floor.

## Validation

The validator (`legaia_expansion_format::validate`) checks each map against
the sections it names, with or without a disc:

| Check | Severity |
|---|---|
| `tiles` resolve (legend, sizes, tiers); at most 16 floor tiers | error |
| Every spawn, exit, zone, prop, NPC tile and patrol waypoint lies on the grid | error |
| The map has a spawn; spawn, exit, trigger, encounter and prop ids are unique in the map | error |
| NPC ids are unique across the pack | error |
| A trigger's `battle` / `boss` / `dialog` / `toast` / `start_quest` / `card_dealer` / `shop` names a defined formation, boss, line, quest, dealer, shop | error |
| A trigger's `challenge` / `mode` names a defined challenge and mode instance; `start = true` on a challenge with no formation | error |
| An NPC's `line`, `line_after` and `shop`, and an exit's `locked_line`, name a defined line and an item shop or dealer | error |
| A gate's quests and bosses are defined; no empty pack flag id; `min_level` in `1..=99` | error |
| A `line_after` with no `after` gate; a `locked_line` on an exit with no `requires` | warning |
| An encounter zone's `table` is a `battle.toml` `[[encounters]]` `scene` | error |
| ... whose table has no `steps` (it can never roll on a map) | warning |
| A `talk` trigger's NPC id that no quest gives from or talks to | warning |
| A `toast` line whose first page runs past 40 characters (one toast row) | warning |
| An exit to another map names a map of the pack and a spawn it has | error |
| Facings are sectors `0..8`; a retail exit's tile is on the `128 x 128` grid | error |
| Disc refs: the base scene, retail exit scenes, NPC models, BGM, prop TMDs (range rules, then the disc when one is loaded) | error |
| One prop and one NPC a tile; a style with both a `mesh` ground and a `wall` | error |
| Wall box height `1..=512`; standable footprint `1..=1024` a side | error |
| NPC models from two different scenes (the townsfolk pose from one clip bank; party-member placements excepted) | warning |
| A seated clip with no decoration or standable prop on the NPC's tile; a patrol `walk_clip` that does not walk; a `clip` the model cannot play ([NPC clip postures](#npc-clip-postures)) | warning |
| A spawn, NPC or waypoint on a void or fully walled tile; an NPC on a prop's tile | warning |
| With a disc: a geometry-pack index (`{ object = N }` props, `mesh` grounds and walls) past the base scene's pack | error |
| Decorations: brushes have meshes and a weight; the map expands within 8192 decorations and 32 a tile; rectangles on the map; scales positive | error |
| Decorations on a placed prop's tile (left out at load) | warning |
| An `on` naming no prop or decoration of the map, or resting an item on itself | error |
| With a resolver that measures meshes: an item floating, off its support's top, or overhanging it ([Resting placement](#resting-placement)) | warning |
| A model asset is in the pack, a glTF 2.0 binary or JSON, `colors` 16 or 256 | error |
| Animator periods at least 1 (a spin may be negative); pulse amount `0..1`; light radius, flicker `0..=1` | error |
| Materials: periods at least 1, palette ranges inside 256 entries | error |
| A `{ scene, object = N }` mesh, with a disc: `N` past that scene's pack | error |
| Decals: `tile` / `rect` on the map, `sides` `3..=16`, size `1..=1024`, density `0..=8`; a decal placing nothing | error / warning |
| Emitters: rectangle on the map, at most 512 particles, life at least 1 frame | error |
| Terrain: resolution `1`, `2` or `4`; skirt at most 12; knobs in range; shapes' radii positive and rectangles on the map; a material, cliff or water with no cells; a character two materials paint | error |
| Terrain: a spawn or NPC tile too steep or too deep to stand on; an exit with no open tile | warning |

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

An exit with a `requires` gate leads nowhere while the gate is shut, and
says its optional `locked_line` (a `dialog.toml` line) on each step onto it
instead - as a toast rather than a dialog box with `locked_toast = true`, so
the player learns why without being stopped on every step.

### Triggers

A trigger's `event` names content of another pack section, and the scene host
hands it to that section's runtime the tick it fires:

| `event` | Runs |
|---|---|
| `{ kind = "battle", formation = F }` | `World::start_expansion_battle(F)` - a `battle.toml` formation |
| `{ kind = "boss", boss = B }` | `World::start_expansion_boss(B)` - a `battle.toml` boss |
| `{ kind = "dialog", line = L }` | `World::expansion_say(L)` - a `dialog.toml` line |
| `{ kind = "toast", line = L }` | `World::expansion_toast_line(L)` - line `L`'s first page as one toast row (`boss_script::banner_text`); takes no input, stops nothing |
| `{ kind = "talk", npc = N }` | `World::expansion_talked(N)` - a quest's talk objective |
| `{ kind = "start_quest", quest = Q }` | `World::expansion_start_quest(Q)` |
| `{ kind = "set_flag", flag = X }` | raises pack flag `X` |
| `{ kind = "counter", counter = C, add = 1 }` | adds to pack counter `C` |
| `{ kind = "card_dealer", dealer = D }` | opens the `cards.toml` dealer `D`'s counter (`World::open_card_dealer`; empty = the first dealer) |
| `{ kind = "shop", shop = S }` | opens the `items.toml` shop `S`'s counter (`World::open_item_shop`; empty = the first shop) |
| `{ kind = "challenge", challenge = C, start = false }` | arms the `quests.toml` challenge `C` (`World::expansion_take_challenge`); `start = true` also starts the fight against its formation. A refusal (a party rule already broken) shows as a toast |
| `{ kind = "mode", mode = M }` | enters the `modes.toml` instance `M` (`World::enter_pack_mode`) |
| `{ kind = "rest", cost = 0, save = false }` | a rest point (below) |
| `{ kind = "cutscene", cutscene = C }` | plays the `cutscenes.toml` cutscene `C` (`World::expansion_play_cutscene`) |

A map's `[[enter_cutscenes]]` (`cutscene`, `requires`, `repeat`) plays on
arrival: the first entry whose gate holds and whose cutscene is unseen, or
any with `repeat = true`. A save resumed on the map plays none. While a
cutscene runs, the map's zones and exits fire nothing (`ActiveMap::observe`).
| `{ kind = "sound", sound = S }` | plays a retail cue or a pack sound (`World::expansion_play_sound`; [audio](expansion-pack-audio.md#map-sound-triggers)) |

`once = true` fires a trigger once **ever**: firing raises the pack flag
`map.<map id>.<trigger id>` (`map.<map id>.<index>` for a trigger with no id),
which saves with the pack's progress, and a trigger whose flag is up stays
quiet on every later visit. Quests can read the flag like any other.

`press = true` makes a trigger something the player examines rather than
a tile that fires on entry: walking onto it fires nothing, and the action
button fires it while the player stands on it or faces it from the next tile
(`ActiveMap::press_gated`, asked by the field interaction probe once no NPC
or prop answers the press, `World::expansion_press_triggers`). Objects (a
brazier, a bell, a notice board) and quest givers that are not NPCs want
it; walk-on triggers are for what has to happen as the player arrives, and
a line the player did not ask for reads best as a `toast`.

A trigger with a `requires` gate fires only while it holds; a shut trigger
fires nothing and spends no `once`, so it fires on the first entry after it
opens.

### Rest points

`kind = "rest"` restores every roster member's HP and MP to full
(`World::revive_party_full`, the game-over Retry's restore) for `cost` gold.
A purse short of `cost` refuses with a toast and changes nothing. With
`save = true` the save screen follows: the rest arms the menu press a
retail save point's op `0x49` sub-op `1` arms (`World::record_op49_park`),
which each host answers by opening the pause menu on the card screen, as it
does for a save point in a retail scene. `World::expansion_rest` holds the
rule.

### Gates

Exits, triggers and NPCs take a `requires` table of the quest prerequisite
shape ([`expansion-pack-quests.md`](expansion-pack-quests.md#prerequisites)):
quests done, pack flags set (`flags`) or clear (`flags_clear`), once-only
bosses beaten (`bosses`) or still standing (`bosses_unbeaten`), retail story
flags, a party level. An empty table is always open. A boss's challenge line
that a won fight should not repeat is the usual use: gate the offer on
`bosses_unbeaten`, and say a second line from a trigger on the same tiles
gated on `bosses`. The scene host answers each gate through
`World::expansion_prerequisites_hold` as the player steps (`ActiveMap::step_gated`).

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
| `requires` | The quest prerequisite shape ([gates](#gates)). Empty is always open. |

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
enters a pack map the party has been to on this save directly, and lists a
map whose entrance gate holds now before its first visit; the *Unlock
all Travel maps* cheat (`--cheat-travel`, the play page's Cheats panel) lists
every map, which is the way in for testing a map whose entrance is out of
reach ([`field-menu.md`](../subsystems/field-menu.md#expansion-pack-journal-engine-extension)).

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

### Entry cost and preparing ahead

Most of a map's entry depends only on the disc and the pack: the mesh
library, the synthetic field image (terrain tile meshes and baked decoration
cells are the bulk of it), the NPC and scenery placements, and the base
scene's resource build with the map's layer seated. A terrain-heavy island
map spends several hundred milliseconds there on a native release build; a
tower floor over a kept room, tens. `legaia_engine_scene_host::expansion_preload`
splits that half off as a `PreparedMap`, which the entry then seats on the
running world.

`SceneHost` keeps prepared maps in a small least-recently-used cache, bounded
by count and by footprint (a terrain map prepares to tens of MiB, a tower
floor to one or two). Every entry fills it, so walking back to a map just
left swaps it in. With preloading on (`SceneHost::set_expansion_preload`,
which both play hosts set), entering a map also starts preparing every map
its exits lead to: on a worker thread natively, and on the browser page a
phase per tick once the player has stood idle on the field for half a second
(the wasm build has no threads). Natively the requests start on the frame
after the entry, once the window's own rebuild of the entered map is done. An exit taken while its map is still
preparing waits for that preparation rather than starting another. The
native window also starts its own render build of the map from the entry's,
rather than building the same scene a second time, and its scene upload (any
scene's, retail included) builds the CPU half of each environment mesh, and
the coplanar offsets, occluder set and fog ground field, in parallel - the
same products in the same order as a serial build.

The preparation reads only the `ProtIndex` and the pack - no `World` state,
no RNG - and its product is what the entry would build itself, so an entry
from the cache plays bit-identically to a cold one
(`crates/engine-scene-host/tests/expansion_map_preload_disc.rs`). A pack
activation, a hot reload included, empties the cache. Retail scene entry is
untouched.

## Meshes from other scenes

A mesh named from another scene's pack samples that scene's VRAM, which the
map does not load. At entry the engine builds the source scene's field
resources, takes the mesh, and relocates what it samples
([`expansion_map::detail`](../../crates/engine-scene/src/expansion_map/detail.rs)):

1. For every texture page its primitives read, the bounding rectangle of
   their UVs; for every palette, its 16 or 256 entries.
2. Each that the map's VRAM already holds word for word stays put - the
   boot-resident system UI, a page two scenes share.
3. The rest are copied into rectangles of free VRAM
   ([`VramAlloc`](../../crates/engine-scene/src/expansion_map/vram_alloc.rs)),
   and the primitives' CLUT, page and UV words are rewritten to the copies.

Free VRAM is what the base scene's build leaves unwritten - every TIM the
scene uploads, the system UI, the effect-texture pool and the field
character atlas the hosts layer with it count as written - minus the
battle transition's capture pages (`320 <= x < 640`) and the dialog font's
page (`(896, 0)`, `64 x 256`). Retail's display buffers (`x < 320`) are free:
the port presents from its own render target. A texture rectangle stays inside one texture page
and a palette starts on a 16-halfword boundary, which is all a primitive's
words can address. All the meshes of one source scene relocate together, so
a page they share is copied once.

The copies are written by the scene's resource build after its own uploads
(`SceneResources::build_targeted_with_options`), so both hosts, and every
rebuild of the map's VRAM - the return from a battle included - carry them.
The base scene's own words are never written. A map whose foreign meshes do
not fit the free VRAM is refused at entry.

### The mesh catalog

A coordinate does not say what it names: `{ scene = "town01", object = 47 }`
is a bed and `object = 48` is the stern of a boat cut open where it meets a
wall, and both have been placed as boats. The mesh catalog
([`legaia_expansion_format::mesh_catalog`](../../crates/expansion-format/src/mesh_catalog.rs),
`crates/expansion-format/data/mesh_catalog.toml`) labels every mesh of the
geometry packs the Rebirth pack borrows from (`town01`, `vell`, `rugi`,
`cave01`, `uru`), one entry per `(scene, object)`:

| Field | Meaning |
|---|---|
| `label` | What the mesh is, in a few words. |
| `kind` | `building`, `plant`, `rock`, `furniture`, `structure`, `light`, `vehicle`, `terrain` or `other`. |
| `size` | The base footprint's width (`X`) and depth (`Z`) and the mesh's height, world units at scale 1, unrotated ([Solid props](#solid-props) measures the same hull). A flat card - a sprite tree, a fern - has no floor-plane hull, so its size is its bounding box. |
| `solid` | At scale 1 it stands in the player's way (`Footprint::is_large_obstacle`): a placement wants `solid = true`. |
| `tris` | Triangles it draws. |
| `note` | How it reads and what it suits: an interior wall open behind, a black occluder block that is no prop, a card that draws from one side. |

The numbers are measured off the disc and the labels are hand-written from a
contact sheet: `legaia-engine mesh-catalog --scene <s> --catalog <file>
--sheet-dir <dir>` measures each scene's pack (`legaia_engine_export::mesh_catalog`),
merges the measurements into the file keeping every label already there,
and draws a numbered sheet of the meshes (a red corner marks a `solid` one)
to read new labels from. The sheet draws disc meshes and stays local; the
file holds labels and numbers only. A disc-gated test checks the committed
sizes against the disc. It sits beside the [posture table](#npc-clip-postures):
that one says what a borrowed NPC's clips are, this one what a borrowed
mesh is.

## Model assets

`{ asset = "path" }` names a glTF 2.0 file under the pack's `assets/` (a
`.glb`, or a `.gltf` with its buffers and PNG textures beside it or in
`data:` URIs). At entry it is converted into the renderer's form
([`expansion_map::gltf`](../../crates/engine-scene/src/expansion_map/gltf.rs)):

| Input | Output |
|---|---|
| The default scene's node tree (`matrix` or TRS) | Baked into the vertices. |
| Triangle primitives: `POSITION`, `TEXCOORD_0`, `indices` | `Ft3` primitives (textured) or `G3` (no texture). |
| A material's `baseColorTexture` (PNG) | Box-filtered to at most `256 x 256`, median-cut to `colors` (16 or 256), written into free VRAM as a 4bpp or 8bpp page with its palette. |
| `baseColorFactor` with no texture | The flat colour of the `G3` primitives. |

One glTF unit is `100` world units (a tile is `128`), `+Y` up; the
conversion flips into the field frame's `Y` down and keeps each face's
winding. A model converts at most `16384` triangles. Skins, morph targets,
animations, normals and other texture channels are not read; a model
animates through `animate` like any prop.

A model is always palettised: there is no full-colour path. That keeps it
inside the scene's one shading law - PSX mode snaps and dithers it like
the disc's meshes, clean mode draws the same texels without the snap - and
it is the honest look for a mesh standing beside retail art.

The validator checks the file is in the pack, has the `glTF` version-2
header (or is JSON for `.gltf`), and that `colors` is 16 or 256; the
conversion's own checks run at entry. [`scripts/models/generate_ember_crystal.py`](../../scripts/models/generate_ember_crystal.py)
writes the Rebirth pack's crystal and is a worked example of a model a
modder builds; [`scripts/models/generate_rebirth_props.py`](../../scripts/models/generate_rebirth_props.py)
builds its floors' set pieces from prisms, tori and gears over a banded
texture strip.

## Decorations

A tile holds one prop, but any number of decorations. They draw only,
unless marked `solid` (`[[decor]]`, a `[[scatter]]` brush, an `[[edges]]`
run): a solid decoration closes its mesh's footprint as a solid prop does
([Solid props](#solid-props)) - an orchard, a fence line, a market stall
the player walks round. When the map loads, every decoration of a tile is
baked with the tile's own cell mesh (if any) into one mesh
([`tmd_ops::bake`](../../crates/engine-scene/src/expansion_map/tmd_ops.rs)),
which the tile's cell draws through the ordinary decoration-cell path on
both hosts. The bake keeps each part's primitive section byte for byte -
the light-source rows included - and only moves its vertices, so a baked
cell draws exactly as its parts would side by side.

Three sections paint decorations, and all expand into one list
(`MapDef::decorations`), deterministically: the same file yields the same
decorations on every host, with or without a disc.

```toml
[[decor]]                                   # one at a time
mesh = { scene = "town01", object = 34 }
tile = [1, 9]
offset = [-30, 0, -36]                      # world units, y up
rotation = [0.0, 20.0, 0.0]
scale = 0.7

[[scatter]]                                 # a seeded brush
rect = [1, 1, 13, 11]
on = ".,"                                   # legend chars painted; empty = every floor tile
density = 0.45                              # expected meshes a tile
seed = 1101
jitter = 44                                 # world units off the tile centre
yaw = [0.0, 360.0]
scale = [0.25, 0.45]
tilt = 0.0                                  # degrees off upright, pitch and roll
keep_clear = true                           # skip spawns, exits, NPCs, patrols, props
meshes = [
  { mesh = { scene = "rugi", object = 18 }, weight = 2 },
  { mesh = { object = 27 }, weight = 3, scale = [0.45, 0.7], y = 0 },
]

[[edges]]                                   # along wall edges
mesh = { scene = "rugi", object = 15 }
walls = "#"                                 # chars that count as wall; empty = void / fully walled
on = ""                                     # floor chars; empty = every floor tile
every = 3                                   # every third edge tile...
phase = 0                                   # ...where (col + row + phase) % every == 0
inset = 6                                   # world units from the wall face
y = 120
yaw = 0.0                                   # added to facing into the room
hangs = true                                # mounted on the wall: not on anything
```

A brush's draw for a tile is a pure function of its `seed`, its index and
the tile, so editing one brush never reshuffles another. A brush paints
floor tiles; it paints a walled tile only when its `on` names that tile's
character, which is how a forest stands its trees on the thicket that walls
a path in. An edge copy is
pushed against the walled side and turned to face into the room (a mesh
whose front faces `+Z` needs no `yaw`); a corner tile gets one copy per
walled side.

| Limit | Value |
|---|---|
| Decorations a map | 8192 |
| Decorations a tile | 32 |
| A decorated tile's records | One a tile, out of the map's 508 |

A map decorated on more tiles than it has free records bakes **cells of
`c x c` tiles**: each cell's decorations move onto one anchor tile (offset by
the tile distance, lifted by the floor tiers' difference), the smallest `c`
that fits. The draw is the same; a baked cell is culled with its anchor
tile, so on a very large map a decoration at a cell's far edge can drop out
a frame early at the view's rim. Decorations on the tile a placed prop's
record anchors on are left out (that record is placed, the bake is a
decoration cell); the validator warns of decorations on a prop's tile.

### Organic placement

Nothing has to sit on the grid:

| Knob | Effect |
|---|---|
| `[[scatter]]` `jitter`, `yaw`, `scale`, `tilt` | Each mesh off its tile centre, turned, scaled and tilted inside the ranges. |
| `[[scatter]]` `cluster = { centre, radius, falloff }` | Density falls from the centre (map-local tiles, the rectangle's centre when left out) as `(1 - d / radius) ^ falloff`: a dense clump, a sparse rim. |
| `[[scatter]]` `clearance = { radius, front, keep }` | The brush thins round the walkable floor ([Clearance](#clearance)): the player and the path read through a forest from the follow camera. |
| `[[edges]]` `jitter`, `yaw_jitter`, `scale_range`, `seed` | Each copy slides along its wall (and a quarter of `jitter` toward the room), turns and scales, seeded per tile. |
| `[[decals]]` brushes | Jitter, yaw range and `size_jitter` per decal. |

### Clearance

A forest scattered on the thicket that walls a path in stands its trees on
the camera's side of the path as well, and from the follow camera - which
stands on a map's `-Z` side (rows shrinking, the top of the painter's
grid) looking toward `+Z` - those
trunks and canopies cover the path and the player. A brush's `clearance`
keeps them off:

```toml
[[scatter]]
on = "Tt"
clearance = { radius = 0.9, front = 3.5, keep = 0.0 }
```

A tile's reach `e` is its least distance to a walkable tile centre (one
neither void nor fully walled), each axis over the band's half-width on
that side: `front` tiles for a tile on the camera's side of the walkable
one, `radius` tiles to the sides and behind. Up to `e = 1` the brush keeps
`keep` of its density; from there it ramps back to full by `e = 1.5`, so
the clearing's edge is a thinning, not a line
([`clearance_weight`](../../crates/expansion-format/src/map_detail.rs)).
The draw stays a pure function of the file: the clearance only scales the
density each tile's own seeded stream reads. Retail forests are laid out
the same way - the West Voz Forest (`vell`) flanks its path with trees and
keeps the camera's side of it low.

The camera-occlusion fade (see-through walls) applies to a pack map's
decorations like any field geometry: a decoration cell is a terrain-tile
draw, so it is both an occluder the visibility gate ray-casts and a draw
the fade dissolves. The gate arms only when the player is completely
hidden, so a canopy that half covers them does not fade - which is what the
clearance is for.

### Decals

```toml
[[decals]]
cell = { tpage = 28, clut = 32577, tile = 4 }      # a base-scene atlas cell
cells = [{ tpage = 28, clut = 32577, tile = 5 }]   # more a brush picks from
span = 1                                           # cells a side the texture spans
rect = [1, 1, 6, 5]                                # a brush; or `tile = [c, r]` for one
density = 0.9
seed = 3101
size = [90, 90]
size_jitter = 0.35
sides = 9                                          # 4 = rectangle, 3..=16 a polygon
offset = [0, 2, 0]                                 # lifted off the floor (y up)
jitter = 40
yaw = [0.0, 360.0]
color = [128, 128, 128]                            # modulation; 128 is the texel
semi = false                                       # blend with the floor
```

A decal is a flat textured polygon in the floor plane - a crack, a moss
patch, a stain, a rune - built when the map loads
(`expansion_map::decal_tmd`) and baked into its tile's cell like any
decoration, both windings so it reads from any side. A rectangle maps the
whole span of cells; a polygon maps the span's inscribed circle. Its words
name the base scene's VRAM. A decal also caps a mesh: the Atrium's pillars
carry octagonal stone decals at their tops.

The Sky Gardens Tower's pillars are an example of why: their top faces are
authored as untextured `G4` primitives with colour `[0, 0, 0]` (the tower's
pillars run up into a dark ceiling the retail camera never sees over), so
from the far camera they read as black discs. That is the mesh, not a
draw fault; a decal over the cap is the fix.

## Animated props

```toml
animate = [
  { kind = "spin", axis = "y", period = 300 },          # a turn every 300 frames; negative turns back
  { kind = "bob", amplitude = 10, period = 150, phase = 0.25 },
  { kind = "sway", axis = "z", degrees = 8.0, period = 90 },
  { kind = "orbit", radius = 70, period = 240 },
  { kind = "pulse", amount = 0.1, period = 60 },
]
```

Each animator is a pure function of `World::frame`
([`legaia_engine_expansion::map_detail::pose`](../../crates/engine-expansion/src/map_detail.rs));
`phase` is a fraction of the period. A prop's animators compose in list
order. The pose reaches both hosts through the placed-draw tables they
already fold in every frame - `World::object_draw_turn_matrices` (turn about
the draw's origin) and `World::object_draw_displacements` (move) - under a
record key of the prop's own (`expansion_map::PROP_RECORD_BASE + n`, past
every retail bind record), which the hosts give the prop's draw from
`SceneOverride::prop_records`. Animation is draw only: a solid prop's tile
and a standable top stay where the prop rests, and nothing animated is
saved or replayed.

### Disc animation clips

A prop's mesh is a placed object and plays no clip. A disc-animated object
- a flickering brazier, a turning wheel, a waving banner - is a scene
**actor** in retail: a MAN placement whose clip the scene's ANM bundle
holds. A pack map gets one two ways, both through the field NPC draw both
hosts run (the base scene's ANM bundle, the hosts' clip players):

- `[[npcs]]` with `scenery = true`: the placement draws and plays its clip
  (or `clip`) on its tile, takes no anchor in the NPC position table - so
  nothing collides with it and the talk probe never finds it - and needs no
  line (`World::install_expansion_npcs`). Its model is the base scene's, as
  any pack NPC's.
- `base = { ..., scenery = true }`: every placement of the base scene's MAN
  that neither talks nor leads anywhere and carries a clip is seated as
  scenery where its MAN header places it
  (`expansion_map::base_scenery`), in placement slots after the map's own
  NPCs. A kept retail room otherwise stands still, since a pack map runs no
  MAN. Spawn scripts do not run, so a prop a retail script moves stands at
  its header position, facing sector 0.

## Lights

Enhanced lighting (`render-kernels::scene_lighting`, default-on on both
hosts) lights a pack map as it lights a retail scene: from the emitter
primitives of the meshes it draws - an additive flame on a torch from
another scene, baked into a decoration, lights the wall it hangs on - plus
the map's own lights:

```toml
[lighting]
mood = "cave"                               # day, dusk, night, cave; omit for the base scene's
lamps = [{ tile = [7, 6], offset = [0, 140, 0], color = [110, 160, 255], radius = 760, flicker = 0.1, period = 40 }]

[[props]]
light = { color = [255, 150, 70], radius = 560, height = 0, flicker = 0.25, period = 18 }
```

`mood` stands in for the base scene's mood while the time of day is
`Auto`; a time the player picks (`F8`) still wins. A lamp sits at its tile
centre plus `offset` above the floor; a prop's light rides the prop's
animated move. `flicker` is the depth of a flame-like waver
(`map_detail::flicker_gain`), deterministic per lamp. Both hosts read the
mood and the lights through one kernel (`engine-screens::field_frame`), and
the nearest lights to the player shade each frame, as in a retail scene.

With enhanced lighting off the map renders retail-faithful: baked texel
colours through the depth cue, no light at all. Placed lamps then draw
nothing; a torch's flame still reads because its glow is in the art.

## Animated materials

```toml
[[materials]]
kind = "cycle"                              # palette cycling
clut = 32576
start = 1
count = 6
period = 8
reverse = false

[[materials]]
kind = "hsv"                                # hue and brightness cycling
clut = 32592
start = 0
count = 16
period = 240
hue = 360.0                                 # degrees a period; 0 pulses brightness only
value = 32                                  # brightness swing

[[materials]]
kind = "tint"                               # a standing recolour, applied once
clut = 32448
start = 0
count = 16
hue = 30.0                                  # degrees turned
value = 0                                   # brightness shift
color = [255, 150, 80]                      # per-channel multiply, 128 = unchanged

[[materials]]
kind = "scroll"                             # texel scrolling
cell = { tpage = 28, clut = 32576, tile = 0 }
dx = 1
dy = 0
period = 8
```

A material rewrites words of the scene's VRAM each frame
(`World::step_expansion_materials`, inside the field VRAM step both hosts
already run and re-upload after), from a capture of the words it first
found. Every primitive that samples them animates: ground tiles, wall
boxes, the base scene's own meshes. `cycle` rotates palette entries
(water, lava, conveyor lights); `hsv` runs the entries through the retail
mode-3 HSV cell kernel (`FUN_80019D50`'s port,
`engine-field::clut_cell_fx::apply_hsv_cell`); `tint` recolours entries once and holds them: a
static hue turn and brightness shift through the same kernel, then a
per-channel multiply (`legaia_tim::tint::tint_bgr555`), so a kept room's
art takes the map's mood; `scroll` rotates an atlas
cell's texels by whole texels, wrapping, at any page depth. The words named
are the base scene's VRAM, unless the material says `from`:

```toml
[[materials]]
kind = "cycle"
from = "rugi"            # the palette's address in rugi's VRAM
clut = 32448
start = 1
count = 8
period = 6
```

`from = "scene"` names the palette or atlas cell in that scene's VRAM, and
the map's load resolves it to the copy it made for a mesh carried in from
that scene (`detail::resolve_materials`, through
`MeshLibrary::relocations`): a palette to its copy's CLUT word, a scrolled
cell to the VRAM rectangle (`rect`) its texels now occupy, clipped to the
copied rectangle. A palette or page the base scene already held is not
copied and keeps its address. A scene none of the map's meshes came from,
or a cell outside every copy, logs a warning and animates nothing.

## Fog

```toml
[fog]
regions = [{ rect = [1, 1, 13, 3], angle = 90.0, spread = 40.0, speed = 18 }]
```

Arms the retail field fog pool over the map
([`field-ambient-fx.md`](../subsystems/field-ambient-fx.md)): each region
becomes a MAN-section-4 region record (`render-kernels::fog_volume::FogRegion`,
its open bounds one tile outside the rectangle), and the ambient particle
gate a scene's script raises goes up (`World::arm_expansion_fog`). The
wisps are the pool's own: they drift at `speed` toward `angle` (degrees,
`0` = `+Z`) within `spread`, and both hosts draw them through
`World::fog_render_step`. The pool draws from the world's random stream, as
retail's does, so a fogged map's rolls differ from an unfogged one's -
deterministically.

## Surround

```toml
[surround]
ground = { tpage = 5, clut = 30720, tile = 0 }      # the drop's floor, one cell a tile
side = { tpage = 11, clut = 32523, tile = 38 }      # the floor edge's face down to it
depth = 448                                         # world units below tier 0
reach = 8                                           # tiles past the map's edges
shade = 72                                          # vertex colour, 128 = the texel as is
backdrop = { tpage = 11, clut = 32523, tile = 38, height = 1152, distance = 2, sides = "esw", shade = 96, fade = 0 }
```

A map with `geometry = "none"` draws nothing past its floor, so a follow
camera standing near the map's edge - an arrival landing beside the low
parapet the camera looks over - frames the clear colour there as a black
void. The surround fills it with a drop: a flat ground `depth` below tier
`0` on every void tile of the map and on every grid tile up to `reach`
past its edges (clipped to the field grid), and, with `side`, a face from
the floor down to the drop along every edge where such a tile borders the
map's floor or walls, so the floor reads as a ledge with depth below it
rather than a sheet hung in nothing. `shade` darkens both the
`texel * colour / 128` way; an [animated material](#animated-materials) on
the ground's cell animates the drop with it - a lava floor far below a
stair of fire.

It is drawn, never walked: the tiles it covers are walls already (void
tiles and everything off the map). It goes in through the decoration-cell
path ([`expansion_map::surround`](../../crates/engine-scene/src/expansion_map/surround.rs)):
a cell that already draws something - a kept retail cell, a wall, a
decoration - keeps it, and one shared record serves every cell of the
same bordered sides and floor height (a cell draws at its corners' mean
tier height, so the mesh carries the rest of the drop), which costs a
handful of the map's object records. A terrain draws its own `skirt`
instead, and the validator warns of a surround on one.

A pack map's ground cells carry the depth-sort bit `0x8000`
([`CELL_GROUND_DEPTH_SORTED`](../../crates/scene-formats/src/field_objects.rs)),
which few retail floors do. A cell without it goes into the ground pass's
far bucket and draws under every other primitive whatever its depth -
harmless when nothing lies below the floor, but from the follow camera the
drop past the map's far edge projects onto the floor rows in front of that
edge, and a far-bucket floor let the drop and its faces paint over them.
Sorted by its own depth, the floor covers what lies under it.

An interior's far walls stop where the follow camera still looks: past
their tops it frames the clear colour, a black sky over a room that has
none. `backdrop` stands a wall round the drop, `distance` tiles past the
map's rectangle along each of its `sides` (`n` is `-Z`, `e` `+X`, `s` `+Z`,
`w` `-X`), rising from the drop to `height` above tier `0` and facing the
map: the tower's outer wall behind the room's own. The follow camera
stands on the map's `-Z` side, so the default `"esw"` leaves that side open
rather than walling the camera out. Its faces draw at `shade` and darken
over their top two fifths to `fade` (`0`, black, by default), so a top in
view fades into the dark instead of ending on a line. The faces ride the
surround's own cells, one record for each shape, and are drawn, never
walked. The validator holds `height` to `1..=4096`, `distance` to
`1..=reach` and `sides` to those four letters.

## Particles

```toml
[[emitters]]
kind = "embers"          # embers, sparkles, dust, leaves, drips, smoke
rect = [3, 4, 3, 4]      # map-local tiles they spawn over
height = [40, 90]        # spawn height above the floor
count = 14               # alive at once (at most 512)
seed = 11
# optional, over the kind's preset:
life = 110
velocity = [0.0, 1.1, 0.0]
spread = [0.4, 0.5, 0.4]
gravity = 0.0
sway = 10.0
size = [10, 4]
color = [255, 150, 60]
color_end = [160, 30, 10]
additive = true
```

Each particle is a camera-facing sprite drawn through the field
screen-effect pass the retail fog sheets go through
(`HostView::fog_prims`, which both hosts call), projected by the same
field view (`World::expansion_particle_quads`). Its whole life is a pure
function of the emitter's seed, its index and `World::frame`
(`map_detail::Emitter::particle`): no state, no random stream, so particles
draw the same on both hosts and in every run, and a replay never sees them.
The kinds are presets of the other fields - embers rise and fade, sparkles
hang and twinkle, dust drifts, leaves fall and sway, drips fall, smoke
swells - any of which the file overrides.

The sprites are the engine's own: four `16 x 16` 4bpp shapes (a soft dot, a
spark, a leaf, a drop) drawn at load into free VRAM
(`detail::particle_sheet`) with two grey ramps, one whose entries carry the
STP bit (the glowing kinds and smoke blend additively, ABR 1)
and one without (leaves draw opaque). The packet colour tints them.

## Terrain

```toml
[terrain]
resolution = 4          # vertices a tile side: 1, 2 or 4 (a vertex every 32 units)
smooth = 2              # box-blur passes over the tier base
max_step = 40           # height range a 64 x 64 sub-cell may span and stay walkable
skirt = 8               # tiles of terrain drawn past each edge
seed = 4120
variation = 0.2         # macro brightness noise over the vertex colours
blend = 0.55            # width of the dithered boundary between materials
detail = 4              # sub-quads a side on a material boundary: 1, 2 or 4
detail_reach = 3        # tiles from the walkable floor that boundaries draw in full (0 = all)
sun = [150.0, 42.0]     # baked sun: azimuth, elevation (degrees)
shading = 0.6
shapes = [
  { kind = "ridge", from = [14.0, 9.0], to = [27.0, 6.5], radius = 2.6, height = 30 },
  { kind = "plateau", rect = [40, 18, 41, 33], height = 22, edge = 0.18 },
  { kind = "flatten", at = [9.5, 9.5], radius = 2.6 },          # no height: level to the mean
  { kind = "noise", rect = [1, 5, 62, 24], amplitude = 5, scale = 0.7 },
]
materials = [
  { name = "sand", chars = ".", tint = [134, 130, 122], cells = [{ tpage = 12, clut = 32005, tile = 3 }] },
  { name = "lane", chars = "d", lift = -4, cells = [{ tpage = 12, clut = 32004, tile = 4 }] },
]
cliff = { slope = 0.95, cells = [{ tpage = 12, clut = 32006, tile = 6 }] }
water = { level = -10, cells = [{ tpage = 9, clut = 31880, tile = 56 }], depth = 56, foam = 16, wade = 5, surf = 150 }
sky = { tpage = 10, clut = 31104, radius = 6, span = [-300, 2200], repeat = 4 }
```

A map with a `[terrain]` table floors on a **heightfield** instead of the
tier grid ([`legaia_expansion_format::terrain`](../../crates/expansion-format/src/terrain.rs)):
`resolution` vertices a tile side over the whole map plus its `skirt`.
The tiers are its base - each tile's `floor_heights` rung (plus its
material's `lift`, and a per-tile jostle of up to its `roughness`) at the
tile centre, bilinear between centres, blurred `smooth` times - and the
`shapes` sculpt it in order:

| Shape | Effect |
|---|---|
| `raise` | Adds `height` (negative lowers) under a round brush, a cosine falloff to `radius` tiles. |
| `ridge` | `raise` along a segment `from` - `to`: a dune, a ridge, a ditch. |
| `plateau` | Sets the tiles of `rect` to `height`, an `edge` tiles wide slope round it, `roughness` tiles of noise on the edge line. |
| `ramp` | A straight slope from `height[0]` at `from` to `height[1]` at `to`, `radius` tiles either side. |
| `flatten` | Pulls the ground under a brush toward `height`, or with no `height` toward its mean there (a house pad on a hillside). |
| `smooth` | Blurs the ground under a brush. |
| `noise` | Fractal noise, `amplitude` either way, features `scale` tiles across, over `rect` or the whole map. |
| `stroke` | A brush dragged along `path` (a list of points): `op = "raise"` adds `height`, `"flatten"` pulls toward `height` (or the mean along the path), `"smooth"` blurs, each under a `radius` brush at the polyline's nearest point, scaled by `strength`. What the workbench's 3D sculpt brush writes. |
| `heightmap` | A grayscale PNG under the pack's `assets/` (`image`), stretched over `rect` (or the whole map, image row 0 at the low-Z edge): black is `height[0]`, white `height[1]`. Pulls the ground toward it by `strength`, or with `add = true` adds it. 8- or 16-bit, grey or colour (luminance). |

Positions are map-local tile coordinates as floats (`[2.5, 3.5]` is the
centre of tile `[2, 3]`); heights are world units, up positive. The whole
build is a pure function of the pack: the same heights on every host,
with or without a disc. A `heightmap` reads its image when the pack loads
(`MapDef::resolve_terrain_images`, from `ExpansionPack::from_files`); the
validator errors on an image missing from `assets/` or one that does not
decode.

### Walking the terrain

The floor sampler (`World::sample_field_floor_height`, the `FUN_80019278`
port, and its camera-ladder sibling) answers the terrain's surface on a
terrain map (`ActiveMap::terrain_floor_at`): `Heightfield::sample_world`
interpolates over the triangle the mesh draws there - a quad's corners
`a b / c d` split along `b`-`c`, the PSX quad's two triangles - so the
player stands on the drawn ground to the unit. The map writes every
collision tier `0` and an all-zero ladder (`floor_lut`), so placed props,
decoration cells and the hosts' floor reads stand at the heights the map
gives them: a prop's record carries the surface under it in its `+0x02`
lift, a decoration bakes it into its vertices, NPCs, lamps and emitters
sample the floor at run time. A standable top still wins where it stands
higher.

Steepness is collision: every `64 x 64` sub-cell whose heights span more
than `max_step` is a wall quarter, and so is one whose centre lies more
than `water.wade` under the water (`MapDef::terrain_walls`, OR-ed into the
style's own quarters). A gentle slope walks; a cliff face and deep water
block, for the retail walk and action movement alike. The validator warns
of a spawn or NPC the terrain walls in and an exit it closes.

### Drawing the terrain

The terrain draws through the decoration-cell path both hosts run
([`expansion_map::terrain`](../../crates/engine-scene/src/expansion_map/terrain.rs)):
each map tile gets a mesh of its quads (an edge tile also carries the
skirt beyond it), baked with the tile's decorations into its cell, so no
host draws anything new. A tile's style draws no flat ground; its `wall`
or `mesh` ground is lifted onto the tile's lowest corner and baked in
too. On a map with more tiles than free records, the cells are chunked as
any decorated map's are; a cell whose anchor holds a placed prop anchors
on another of its tiles, so the ground under a prop still draws.

| Layer | What draws |
|---|---|
| Ground | `GT3` triangles, each quad textured with its material. |
| Water | A flat textured surface at `water.level` over every quad that dips below it. The opaque surface meets the ground in the depth buffer, so the shoreline follows the sculpted slope, not the quad grid. |
| Foam | An additive band over the shallows, brightest at the waterline. |
| Horizon | `sky`: a ring of wall round the map, `radius` tiles past its half-diagonal, textured with a page of the base scene's VRAM (a retail sky page) repeated round it mirrored. |

**Materials.** A material paints the tile characters in `chars` with one
of its `cells` (atlas cells of the base scene's VRAM, as a `texture`
ground names them; cells may mix palettes), picked per tile; a tile whose
character no material names paints with its style's `ground` texture. A
quad takes the material with the highest weight at its centre - the four
nearest tile centres' materials, bilinear - after a noise of `blend`
width, so a boundary is a dithered, organic edge about a tile wide rather
than the tile grid's step. Each tile turns and mirrors its cell one of
eight ways (`rotate`, on by default) against visible repeats. A quad
steeper than `cliff.slope` (rise over run) takes the cliff material.

**Boundary detail.** A quad is `32` units across (at `resolution = 4`), so a
material boundary steps in `32`-unit stairs however its noise meanders.
With `detail = 2` or `4`, a quad whose triangles or corners disagree on
their material is cut into `detail x detail` sub-quads, each triangle
choosing at its own centroid (`TerrainSurface::fine`), so the boundary
steps every `16` or `8` units. The sub-quads lie on the quad's own two
triangles - split as the quad is, heights and vertex colours interpolated
over the triangle they fall in - so the surface, and the floor the player
walks, are unchanged; only boundary quads pay the extra triangles. A
sub-quad samples only a few texels, and the hosts drop a prim whose texels
are all zero words (their test for an unwritten page), so a quad any of
whose sub-prims would read that way against the map's VRAM draws whole
(`MeshLibrary::terrain_vram`).

`detail_reach` limits where a boundary draws in full to the tiles within
that many tiles (Chebyshev) of the walkable floor - a tile neither void nor
fully walled - a skirt quad counting its overshoot past the map's edge.
Past it a quad keeps its own two triangles' materials, with no subdivision
and no soft overlay. A forest's thicket or a cliff band is ground seen only
from afar and through what stands on it, where the finer boundary is
triangles nobody sees; `0` (the default) draws every boundary in full.

**Textures from another scene.** A material, the `cliff` or the `water`
whose atlas cells name another retail scene's VRAM says so with
`from = "<scene>"`:

```toml
materials = [
  { name = "moss rock", chars = ",", from = "cave02", cells = [{ tpage = 10, clut = 32580, tile = 40 }] },
]
```

At entry the engine builds that scene's resources and copies each named
cell's `32 x 32` texels into a free atlas-cell slot of a page of the same
depth, and its palette into a free palette row
([`detail::borrow_terrain_cells`](../../crates/engine-scene/src/expansion_map/detail.rs)),
then paints with the copies (`MeshLibrary::terrain`); a cell or palette two
materials share is copied once. It is the animated materials' `from` for
the ground: a base scene with a narrow palette set borrows what it lacks.

**Vertex colours** carry what a blend would: each vertex's colour is the
material tints (`tint`, `128` = the texel as is) blended by the same
weights, times a low-frequency brightness noise (`variation`), times the
baked sun (`sun`, `shading`), so a slope facing away darkens with lighting
off. That is the retail field's own law - `texel * colour / 128` through
the depth cue, no light source - and both render modes draw it; enhanced
lighting lights it like any field mesh.

**Water** is coloured by depth, `shallow` to `deep` over `depth` units.
Animate its texture with `[[materials]]` (`scroll` or `cycle` on the
water cell). The foam band is the engine's own texture (`terrain::foam_sheet`,
written into free VRAM at load: a `4 x 4` block and one STP palette per
depth band), blended additively, its vertex colour `foam_color` fading to
black away from the line. Its four depth bands brighten in turn, deepest
first, every `surf` frames (`terrain::surf_level`, written by
`World::step_expansion_materials`): a crest rolling in to the beach.

**Decals** on a terrain drape over it: the polygon is cut into pieces
about `32` units across and each vertex stands on the ground under it
(`expansion_map::decal_tmd_draped`), relative to the ground at the decal's
centre where the bake stands it, so a decal on a slope follows the slope.
Off a terrain a decal is one flat polygon, as before.

The Rebirth pack's Isles beach, village, grotto and Thornwood are built on
it.

## How large a map can be

One map is one field grid: at most `120 x 120` tiles of `128` world units
(`MAX_MAP_TILES`; the grid is `128` a side and the map keeps a margin row
below it), about 15,000 units a side - larger than any retail room. Two
budgets bound how much it can hold: `508` object records (`4..=511`; ground textures,
walls, props and decorated cells share them; decorations fall back to
baked cells when they run out) and the VRAM the base scene leaves free for
foreign meshes and model textures. A world larger than one grid is several
maps joined by `exits` with `to = { kind = "map" }`; a map exit enters at
once, with no streaming.

## Gaps
- **Terrain.** The water surface is flat (its waves are texture and foam
  animation, not vertex motion): the terrain draws through baked
  decoration cells, which neither host deforms per frame.
- **Camera preset on an in-world entry.** A map entered through an entrance
  or a Travel pick keeps the follow camera's distance; the map's `camera`
  preset applies on a host's direct entry (`--map`, a pack map row in the
  play page's scene list).
- **NPC clips.** A patrolling NPC with no `pause` keeps `walk_clip` at the
  ends of its legs: it sets out again on the next tick.
