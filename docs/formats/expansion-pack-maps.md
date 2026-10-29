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
scale = 0.75                                # uniform; 1 when left out
solid = true                                # close the prop's tile
```

`mesh` is either `{ object = N }` or `{ tmd = { kind = "tmd", prot = P, index = N } }`.
A TMD coordinate resolves only when `P` is the PROT entry holding the base
scene's geometry pack (its scene-asset bundle); anything else is refused at
entry. `scale` folds into the prop's model matrix after its rotation, where a
retail placed object's render scale lands. Props carry no collision box of
their own: `solid` closes the tile.

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
facing at the tail of the pack's expansion block (`LXP1`, after the challenge
records); a save made off every map writes no tail, so its block is the one it
was before. The save's resume scene is the base scene, so a Load lands there
first; `NativeSession::resume_save` then re-enters the map with the player
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

- **Wall visuals.** A wall tile blocks but draws nothing; a visible wall is
  a prop or a `mesh` ground on the tile.
- **Props from other scenes.** A prop mesh must come from the base scene's
  geometry pack; a TMD from another PROT entry (and the VRAM it samples) is not
  loaded.
- **Map references are partly validated.** The validator checks a map's
  entrances but not yet its trigger, encounter and exit ids against the
  sections they name; a broken one logs at run time.
- **Camera preset on an in-world entry.** A map entered through an entrance
  or a Travel pick keeps the follow camera's distance; the map's `camera`
  preset applies on a host's direct entry (`--map`, the play page's
  *Enter map*).
