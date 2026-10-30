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
| `decor`, `scatter`, `edges` | Decorations: any number of meshes a tile, placed one by one, by seeded brush, or along wall edges ([Decorations](#decorations)). |
| `lighting` | Enhanced-lighting mood and placed lamps ([Lights](#lights)). |
| `materials` | Animated texture words: palette cycling, hue cycling, texel scrolling ([Animated materials](#animated-materials)). |
| `fog` | The retail field fog pool over tile regions ([Fog](#fog)). |

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

`mesh` takes four forms ([`PropMesh`](../../crates/expansion-format/src/map.rs)):

| Form | Mesh |
|---|---|
| `{ object = N }` | Mesh `N` of the base scene's geometry pack. |
| `{ scene = "town01", object = N }` | Mesh `N` of any field scene's geometry pack ([Meshes from other scenes](#meshes-from-other-scenes)). |
| `{ tmd = { kind = "tmd", prot = P, index = N } }` | Mesh `N` of the geometry pack held in PROT entry `P`, whichever scene's block holds it. |
| `{ asset = "models/x.glb", colors = 256 }` | A glTF model the pack carries ([Model assets](#model-assets)). |

`scale` folds into the prop's model matrix after its rotation, where a
retail placed object's render scale lands. Props carry no collision box of
their own: `solid` closes the tile.

`animate` is a list of procedural animators and `light` a light the prop
carries ([Animated props](#animated-props), [Lights](#lights)). A prop
standing on a tile whose cell already drew a mesh - a kept retail cell, a
wall, a mesh ground - replaces that mesh (a cell names one record); the
entry logs the tiles it happened to. A decoration bakes in beside a cell's
mesh instead, which is how to dress a kept retail room without losing any
of it.

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
NPC's own. An NPC with a `shop` opens that counter instead: an
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
| A trigger's `battle` / `boss` / `dialog` / `start_quest` / `card_dealer` / `shop` names a defined formation, boss, line, quest, dealer, shop | error |
| A trigger's `challenge` / `mode` names a defined challenge and mode instance; `start = true` on a challenge with no formation | error |
| An NPC's `line`, `line_after` and `shop`, and an exit's `locked_line`, name a defined line and an item shop or dealer | error |
| A gate's quests and bosses are defined; no empty pack flag id; `min_level` in `1..=99` | error |
| A `line_after` with no `after` gate; a `locked_line` on an exit with no `requires` | warning |
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
| Decorations: brushes have meshes and a weight; the map expands within 8192 decorations and 32 a tile; rectangles on the map; scales positive | error |
| Decorations on a placed prop's tile (left out at load) | warning |
| A model asset is in the pack, a glTF 2.0 binary or JSON, `colors` 16 or 256 | error |
| Animator periods at least 1 (a spin may be negative); pulse amount `0..1`; light radius, flicker `0..=1` | error |
| Materials: periods at least 1, palette ranges inside 256 entries | error |
| A `{ scene, object = N }` mesh, with a disc: `N` past that scene's pack | error |

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
instead.

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
| `{ kind = "shop", shop = S }` | opens the `items.toml` shop `S`'s counter (`World::open_item_shop`; empty = the first shop) |
| `{ kind = "challenge", challenge = C, start = false }` | arms the `quests.toml` challenge `C` (`World::expansion_take_challenge`); `start = true` also starts the fight against its formation. A refusal (a party rule already broken) shows as a toast |
| `{ kind = "mode", mode = M }` | enters the `modes.toml` instance `M` (`World::enter_pack_mode`) |
| `{ kind = "rest", cost = 0, save = false }` | a rest point (below) |
| `{ kind = "cutscene", cutscene = C }` | plays the `cutscenes.toml` cutscene `C` (`World::expansion_play_cutscene`) |

A map's `[[enter_cutscenes]]` (`cutscene`, `requires`, `repeat`) plays on
arrival: the first entry whose gate holds and whose cutscene is unseen, or
any with `repeat = true`. A save resumed on the map plays none. While a
cutscene runs, the map's zones and exits fire nothing (`ActiveMap::observe`).

`once = true` fires a trigger once **ever**: firing raises the pack flag
`map.<map id>.<trigger id>` (`map.<map id>.<index>` for a trigger with no id),
which saves with the pack's progress, and a trigger whose flag is up stays
quiet on every later visit. Quests can read the flag like any other.

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
modder builds.

## Decorations

A tile holds one prop, but any number of decorations. They have no
collision and draw only. When the map loads, every decoration of a tile is
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
```

A brush's draw for a tile is a pure function of its `seed`, its index and
the tile, so editing one brush never reshuffles another. An edge copy is
pushed against the walled side and turned to face into the room (a mesh
whose front faces `+Z` needs no `yaw`); a corner tile gets one copy per
walled side.

| Limit | Value |
|---|---|
| Decorations a map | 8192 |
| Decorations a tile | 32 |
| A decorated tile's records | One a tile, out of the map's 511 |

A map decorated on more tiles than it has free records bakes **cells of
`c x c` tiles**: each cell's decorations move onto one anchor tile (offset by
the tile distance, lifted by the floor tiers' difference), the smallest `c`
that fits. The draw is the same; a baked cell is culled with its anchor
tile, so on a very large map a decoration at a cell's far edge can drop out
a frame early at the view's rim. Decorations on a placed prop's tile are
left out (the prop's record is placed, the bake is a decoration cell) and
the validator warns.

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

Disc ANM clips do not play on a prop: a retail scene's animated scene props
are bound to their scene's MAN actors and ANM bank, which a pack map does
not run. A prop animates procedurally or not at all.

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
`engine-field::clut_cell_fx::apply_hsv_cell`); `scroll` rotates an atlas
cell's texels by whole texels, wrapping, at any page depth. The words named
are the base scene's VRAM - a palette or cell of a mesh carried in from
another scene sits at an address chosen at entry and cannot be named.

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

## How large a map can be

One map is one field grid: at most `120 x 120` tiles of `128` world units
(`MAX_MAP_TILES`; the grid is `128` a side and the map keeps a margin row
below it), about 15,000 units a side - larger than any retail room. Two
budgets bound how much it can hold: `511` object records (ground textures,
walls, props and decorated cells share them; decorations fall back to
baked cells when they run out) and the VRAM the base scene leaves free for
foreign meshes and model textures. A world larger than one grid is several
maps joined by `exits` with `to = { kind = "map" }`; a map exit enters at
once, with no streaming.

## Gaps
- **Camera preset on an in-world entry.** A map entered through an entrance
  or a Travel pick keeps the follow camera's distance; the map's `camera`
  preset applies on a host's direct entry (`--map`, the play page's
  *Enter map*).
- **NPC clips.** A patrolling NPC keeps `walk_clip` (or its idle clip) at the
  ends of its legs; nothing cues the idle clip back between legs.
