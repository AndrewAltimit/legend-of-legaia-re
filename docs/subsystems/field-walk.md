# Field walk + tile-grid collision

How the player character moves around a field/town scene, and where per-scene collision lives.

Field movement is **grid-based**: the scene is a `width × height` array of byte cells, the player occupies one `(col, row)` cell, and each accepted d-pad press advances the player exactly one cell. Collision is the cell array itself — a destination cell value of `2` is a wall. There is no free 2D movement and no separate collision-geometry format; the walkable grid *is* the collision data.

This supersedes the closing guess in [`formats/navmesh.md`](../formats/navmesh.md) that per-scene collision lives in field-pack schema slots. The player walk grid is installed inline in the field-VM event script, not a field-pack slot.

## The map-grid header (`_DAT_8007b450`)

A field-VM opcode points the global `_DAT_8007b450` at a map-grid header that lives **inline in the field event-script bytecode** (the same "data is an operand of the install op" pattern as encounter records — see [`formats/encounter.md`](../formats/encounter.md)). Confirmed byte fields (offsets into the header):

| Offset | Meaning |
|---|---|
| `+1` | world tile origin X (added to `col`) |
| `+2` | world tile origin Z (added to `row`) |
| `+3` | grid width (columns), `u8` |
| `+4` | grid height (rows), `u8` |
| `+5` | draw/scan radius around the player |
| `+6` | mode flag (gates the bulk tile-draw pass) |
| `+7`, `+9` | event-flag operands consumed when the player lands on a trigger cell |
| `+0xb` | player actor template id |
| `+0xc` | NPC actor template base id (consecutive ids follow) |

The mutable runtime grid is a separate `width × height` byte buffer at `DAT_801f35c0`, allocated on scene init and seeded from the header. The player's live cell is `DAT_801f35c8` (col) / `DAT_801f35cc` (row). A per-cell-value display table (`DAT_801f35bc`, 0x3c bytes) maps cell values to tile actor handles for the draw pass.

Provenance: install site is field-VM op `0x49` in `overlay_0897_801de840.txt` (`_DAT_8007b450 = pbVar47` at the `0x49` case); the grid loader + walk state machine is `overlay_0897_801ef2b0.txt`.

## Cell value semantics

Cells are indexed `grid[row * width + col]`. Confirmed value classes:

| Value | Meaning |
|---|---|
| `2` | **wall** — destination cell `== 2` rejects the move |
| `3`..`6` | walkable terrain types; sets `_DAT_8007b5f0 = (v - 3) * 2` (step variant) |
| `7` | trigger; routes the walk SM to its event sub-state |
| `8`..`10` | event / transition tile (door, stairs); reads header `+7`/`+9` as flag operands via the field-VM flag helpers (`func_0x8003ce08` set / `func_0x8003ce64` test), and applies a half-tile world offset |
| `0xb`..`0xe` | animated tiles; the arrival sub-state cycles the value `0xb → 0xe → 0xb` each visit |
| other | plain walkable floor |

## Tile ↔ world coordinates

Each tile is `0x80` (128) world units; the actor sits at the tile centre:

```text
world_x = (header[+1] + col) * 0x80 + 0x40
world_z = (header[+2] + row) * 0x80 + 0x40
```

(`overlay_0897_801ef2b0.txt` case 4, target-position setup.)

## Walk state machine

The walk controller is a small state machine keyed on the controller actor's `+0x54` field (`overlay_0897_801ef2b0.txt`, switch on `*(param_1 + 0x54)`):

| State | Role |
|---|---|
| `0` | init: allocate the cell display table + runtime grid, spawn the player + NPC actors from header ids |
| `1` | fade-in (ramps `actor[+0x9c]`) |
| `2` | interpolate the actor's world position toward the target cell centre (`DAT_801f35d0`/`d4`); on arrival → `3` |
| `3` | arrival: read the current cell; `7` → trigger sub-state, `8..10` → event sub-state, otherwise run the animated-tile decay pass, then → `4` |
| `4` | **read input + collision + commit**: see below |
| `5` | menu / confirm (entered when the menu button edge `_DAT_8007b874 & 0x10` fires) |

### State 4 — input, collision, commit

1. If the menu-button edge (`_DAT_8007b874 & 0x10`) is set, go to state `5`.
2. Read the pad `_DAT_8007b850` and remap it by camera facing via `func_0x800467e8` (so "screen up" maps to the correct world direction regardless of camera azimuth).
3. Decode one direction from the remapped mask into a candidate `(col, row)`:

   | mask bit | delta |
   |---|---|
   | `0x1000` | `row + 1` |
   | `0x4000` | `row - 1` |
   | `0x2000` | `col + 1` |
   | `0x8000` | `col - 1` |
   | none | no move |

4. Reject the move (play bonk `func_0x80035bd0(0x23)`, stay put) when the candidate is out of bounds **or** `grid[candidate] == 2`.
5. Otherwise accept: play the walk action (`func_0x80035b50(0x21)`), compute the target world position, commit `DAT_801f35c8/cc = candidate`, and go to state `2` to interpolate.

Provenance: `overlay_0897_801ef2b0.txt` case 4 (`0x801ef…`); the denser duplicate of this logic also appears inside the field main loop `overlay_0897_801f7b88.txt`.

## Clean-room port

[`legaia_engine_core`](../../crates/engine-core/) drives the walk in the `SceneMode::Field` arm of `World::tick`, reading `World.input` (the [input contract](engine.md)). The grid is held on `World` as a `FieldGrid` (dims + origin + cell bytes + player col/row); the per-frame step decodes one direction, gates it against the grid, and advances the player cell with the same `cell == 2` wall rule and tile-centre world mapping.

## Open

- The exact byte offset where the cell array begins in the inline script header (the header is variable-length; `+7`/`+9` are read through the field-VM operand reader `func_0x8003ce9c`). Pinning it enables a disc-gated parser that lifts the real grid out of a scene's event script. The movement + collision math above does not depend on it.
- Whether `func_0x800467e8`'s facing remap is a fixed 90° quadrant snap or a finer rotation.
