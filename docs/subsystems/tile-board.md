# Tile-board grid (puzzle / board minigame mode)

A discrete tile-board mode used by puzzle rooms / board minigames inside the field overlay. The board is a `width × height` array of byte cells, the player occupies one `(col, row)` cell, and each accepted d-pad press advances the player exactly one cell. The cell array *is* the collision data — a destination cell value of `2` is a wall. Each cell value also indexes a tile-actor table that the board renderer draws as a tile sprite at the cell's world position.

**This is not general town/field locomotion.** Legaia towns use free movement; that path is separate and still being reverse-engineered (see [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md)). The tile-board system was initially mistaken for town walking because it lives in the field overlay (`0897`) and reads the pad to move the player; the giveaways that it is a special board mode are the per-cell tile-actor rendering and the procedural board fill below.

## Where the board comes from

A field-VM opcode points the global `_DAT_8007b450` at a board header. The header lives **inline in the field-VM event script** (the "data is an operand of the install op" pattern, same as encounter records — see [`formats/encounter.md`](../formats/encounter.md)). Confirmed header byte fields:

| Offset | Meaning |
|---|---|
| `+1` | world tile origin X (added to `col`) |
| `+2` | world tile origin Z (added to `row`) |
| `+3` | board width (columns), `u8` |
| `+4` | board height (rows), `u8` |
| `+5` | draw/scan radius around the player |
| `+6` | mode flag (full-board draw vs. windowed draw around the player) |
| `+7`, `+9` | event-flag operands consumed when the player lands on a trigger cell |
| `+0xb` | player actor template id |
| `+0xc` | tile-actor template base id (one per drawable cell value) |

The mutable runtime board is a separate `width × height` byte buffer at `DAT_801f35c0`; the player's live cell is `DAT_801f35c8` (col) / `DAT_801f35cc` (row); the per-cell-value tile-actor table is `DAT_801f35bc` (0x3c bytes, ~15 entries).

At least one board instance is **procedurally generated**: the filler `overlay_0897_801e0b1c` seeds every cell with BIOS `rand` (`func_0x80056798` = A0(0x2F)) as `rand()%6 + 2`, then scatters animated and feature tiles. A fixed board would instead be carried by the inline script header; the exact byte offset of the cell array within the (variable-length) header is not yet pinned.

Provenance:
- Install: field-VM op `0x49` in `overlay_0897_801de840.txt` (a multi-subtype map-command opcode; `_DAT_8007b450 = pbVar47` arms the header pointer).
- Walk SM: `overlay_0897_801ef2b0.txt`.
- Procedural fill: `overlay_0897_801e0b1c.txt`.
- Board renderer: `overlay_0897_801e0f3c.txt` (draws each cell value > 1 as `DAT_801f35bc[cell]` at the cell's world position).

## Cell value semantics

Cells are indexed `board[row * width + col]`. Confirmed value classes:

| Value | Meaning |
|---|---|
| `2` | **wall** — destination cell `== 2` rejects the move |
| `3`..`6` | walkable terrain types; sets `_DAT_8007b5f0 = (v - 3) * 2` (step variant) |
| `7` | trigger; routes the walk SM to its event sub-state |
| `8`..`10` | event / transition tile; reads header `+7`/`+9` as flag operands via the field-VM flag helpers (`func_0x8003ce08` set / `func_0x8003ce64` test), and applies a half-tile world offset |
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

The board controller is a small state machine keyed on the controller actor's `+0x54` field (`overlay_0897_801ef2b0.txt`, switch on `*(param_1 + 0x54)`):

| State | Role |
|---|---|
| `0` | init: allocate the cell tile-actor table + runtime board, spawn the player + tile actors from header ids |
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

4. Reject the move (play bonk `func_0x80035bd0(0x23)`, stay put) when the candidate is out of bounds **or** `board[candidate] == 2`.
5. Otherwise accept: play the step action (`func_0x80035b50(0x21)`), compute the target world position, commit `DAT_801f35c8/cc = candidate`, and go to state `2` to interpolate.

Provenance: `overlay_0897_801ef2b0.txt` case 4; a denser duplicate of this logic also appears inside `overlay_0897_801f7b88.txt`.

## Clean-room port

[`legaia_engine_core::tile_board::TileBoard`](../../crates/engine-core/src/tile_board.rs) holds the board (dims + origin + cell bytes + player cell). [`World::tick`](../../crates/engine-core/src/world.rs) drives a board step in the `SceneMode::Field` arm when a board is installed (`World.tile_board`), reading `World.input` (the [input contract](engine.md)): it decodes one direction, gates against `cell == 2`, commits the player cell, and interpolates the player actor to the destination tile centre. The board stays inert (no-op) until installed, so it does not affect ordinary field scenes.

## Open

- Which specific minigames / puzzle rooms use this board, and whether any board is fixed (inline-script cells) vs. always procedural. The `dance` / `baka_fighter` / `slot_machine` overlays reference the same `_DAT_8007b450` globals.
- The exact byte offset where the cell array begins in the inline script header (the header is variable-length; `+7`/`+9` are read through the field-VM operand reader `func_0x8003ce9c`). Needed to lift a fixed board from a scene's event script.
- Whether `func_0x800467e8`'s facing remap is a fixed 90° quadrant snap or a finer rotation.
