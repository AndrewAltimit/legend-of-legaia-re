# Expansion-pack maps (`maps/<id>.toml`)

A pack map is a walkable field scene a modder lays out tile by tile, with no
disc bytes in the pack. It borrows its look from a retail **base scene** - the
scene's texture pages, palettes and geometry pack stay on the player's disc and
are named only by coordinate - and supplies everything that makes it a place:
walls, floor heights, props, spawns, exits, trigger zones and encounter zones.

Schema: [`legaia_expansion_format::map`](../../crates/expansion-format/src/map.rs).
Field image builder: [`legaia_engine_scene::expansion_map`](../../crates/engine-scene/src/expansion_map.rs).
Entry and per-tick step: `SceneHost::enter_expansion_map` in
[`engine-scene-host`](../../crates/engine-scene-host/src/scene_host/expansion_map.rs);
zone runtime: [`legaia_engine_expansion::map`](../../crates/engine-expansion/src/map.rs).
Sample: [`packs/rebirth/maps/rebirth_tower_1f.toml`](../../packs/rebirth/maps/rebirth_tower_1f.toml).

## File layout

| Key | Meaning |
|---|---|
| `name` | Display name. |
| `size` | Optional `[width, height]` in tiles; must match `tiles.rows`. |
| `base` | `scene` (a `DiscRef::Scene`) and `geometry` (`"none"` or `"keep"`). |
| `floor_heights` | Up to 16 tier heights, world units, up positive. Empty takes the base scene's ladder. |
| `tiles` | `origin`, `rows`, `legend` - the painter's grid, below. |
| `props` | Placed meshes from the base scene's geometry pack. |
| `spawns` | Named arrival points; the first is the default. |
| `exits` | Tile rectangles leading to a retail scene or another pack map. |
| `triggers` | Tile rectangles that raise a pack event (battle, dialog, quest). |
| `encounters` | Tile rectangles naming a pack encounter table. |
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
solid = true                                # close the prop's tile
```

`mesh` is either `{ object = N }` or `{ tmd = { kind = "tmd", prot = P, index = N } }`.
A TMD coordinate resolves only when `P` is the PROT entry holding the base
scene's geometry pack (its scene-asset bundle); anything else is refused at
entry. Props carry no collision box of their own: `solid` closes the tile.

## Exits, triggers and encounter zones

Zones fire on **entry** - stepping onto a tile of the rectangle from outside it
- the way a retail walk-on trigger fires on a tile crossing. Standing still
fires nothing, and the spawn tile does not fire on arrival.

- An exit `to = { kind = "retail", scene = { kind = "scene", name = "town01" }, tile = [28, 91], facing = 4 }`
  raises the named scene change a door's op `0x3F` raises, so it parks behind
  the streaming hold and arrives on that field-grid tile facing that compass
  sector. With no `tile` the player lands where the scene's own doors land them.
- An exit `to = { kind = "map", map = "...", spawn = "..." }` enters another map
  of the same pack at once.
- A trigger's `event` is `{ kind = "battle", formation = ... }`,
  `{ kind = "dialog", line = ... }` or `{ kind = "quest", event = ... }`;
  `once = true` fires it once per visit. The map runtime queues the event on
  `World::expansion.map` (`ActiveMap::take_events`) for the battle, dialog and
  quest runtimes.
- An encounter zone names a pack encounter table. The map rolls **no** retail
  encounters; `ActiveMap::encounter_table` reports the zone the player stands
  in, for the battle runtime's roll.

## Running a map

```bash
legaia-engine play-window --disc "Legend of Legaia (USA).bin" \
    --expansion packs/rebirth --map rebirth_tower_1f [--spawn dais]
```

`--expansion` takes a pack directory. Any scene change - an exit, a battle
return to a retail scene, a scene picker - leaves the map and drops its layer.

## Gaps

- **Trigger and encounter consumers.** The map runtime queues trigger events
  and reports the encounter table; nothing yet starts the battle, shows the
  line or advances the quest, and no encounter rolls in a zone.
- **Wall visuals.** A wall tile blocks but draws nothing; a visible wall is
  a prop or a `mesh` ground on the tile.
- **Props from other scenes.** A prop mesh must come from the base scene's
  geometry pack; a TMD from another PROT entry (and the VRAM it samples) is not
  loaded.
- **Prop scale.** `scale` is carried but props draw at scale 1.
- **Place-name banner.** `name` is not shown; the banner reads MAN text.
- **Saving on a map.** A save records the base scene's label, so a Load
  lands in the retail base scene, not the map.
- **`.lxpack` bundles.** The native host reads a pack directory only.
- **The browser host.** Only the native `play-window` enters pack maps.
