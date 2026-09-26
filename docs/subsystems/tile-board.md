# Tile-board grid (puzzle / board minigame mode)

A discrete tile-board mode used by puzzle rooms / board minigames inside the field overlay. The board is a `width × height` array of byte cells, the player occupies one `(col, row)` cell, and each accepted d-pad press advances the player exactly one cell. The cell array *is* the collision data - a destination cell value of `2` is a wall. Each cell value also indexes a tile-actor table that the board renderer draws as a tile sprite at the cell's world position.

**This is not general town/field locomotion.** Legaia towns use free movement, which
is a separate path with its own controller (`FUN_801d01b0`) - see
[`field-locomotion.md`](field-locomotion.md). The tile-board system was initially
mistaken for town walking because it lives in the same field overlay (`0897`) and
also reads the pad to move the player; the giveaways that it is a special board mode
are the per-cell tile-actor rendering and the procedural board fill below.

## Where the board comes from

Field-VM op `0x49` **sub-op `0x05`** points the global `_DAT_8007b450` at a board header. The header lives **inline in the field-VM event script** (the "data is an operand of the install op" pattern, same as encounter records - see [`formats/encounter.md`](../formats/encounter.md)). It is a **fixed 14-byte structure** - the install op advances the script cursor by a **constant `+0xe`** regardless of `width × height`, so the cells are never carried inline (see [always-procedural](#always-procedural-no-inline-cell-boards)). Byte layout from the opcode (`_DAT_8007b450` points at byte `[1]`, the sub-op, so the doc `+N` offsets below are opcode byte `[N+1]`):

| From opcode | `_DAT_8007b450` offset | Meaning |
|---|---|---|
| `[0]` | - | opcode `0x49` |
| `[1]` | `+0` | sub-op `0x05` |
| `[2]` | `+1` | world tile origin X (added to `col`) |
| `[3]` | `+2` | world tile origin Z (added to `row`) |
| `[4]` | `+3` | board width (columns), `u8` |
| `[5]` | `+4` | board height (rows), `u8` |
| `[6]` | `+5` | draw/scan radius around the player |
| `[7]` | `+6` | mode flag (`0` = full-board draw, else windowed draw around the player) |
| `[8]`..`[9]` | `+7`..`+8` | event-flag **base A** (`u16` LE) - event-SET base |
| `[10]`..`[11]` | `+9`..`+0xa` | event-flag **base B** (`u16` LE) - TEST / already-done gate base |
| `[12]` | `+0xb` | player actor template id |
| `[13]` | `+0xc` | tile-actor template base id (one per drawable cell value) |

To scan a disc for boards, search the decompressed field scripts for the two-byte prefix `49 05`.

The mutable runtime board is a separate `width × height` byte buffer at `DAT_801f35c0`; the player's live cell is `DAT_801f35c8` (col) / `DAT_801f35cc` (row); the per-cell-value tile-actor table is `DAT_801f35bc` (0x3c bytes, ~15 entries).

### Always procedural (no inline-cell boards)

Sub-op-5 boards are **always procedurally generated** - there is no fixed inline-cell board variant. The proof is structural: the op `0x49` case advances the script cursor by a **constant `+0xe` (14 bytes)** independent of `width * height`, so the cell array can never be part of the operand stream. Confirmed in the instruction stream at `0x801e093c..0x801e0948`: the sub-op-5 arm is `beq v1,v0` (v0 = 5) then `j 0x801e3624` with delay slot `addiu fp,fp,0xe`.

The cells are filled at install by the procedural fill at **`0x801EF334`** (see the address note below): every cell `rand()%6 + 2`, then **4 animated tiles** and **3 event tiles** scattered in. Read from the disassembly:

| Phase | Addresses | Behaviour |
|---|---|---|
| Base fill | `0x801ef418..0x801ef450` | `rand()%6 + 2` per cell, for `width * height` cells. The `%6` is a magic-multiplier divide (`lui s1,0x2aaa; ori s1,s1,0xaaab`), multiply-back `x6`, `subu`, then `addiu v0,v0,2`. |
| Animated tiles | `0x801ef484..0x801ef500` | Four cells, values **`0xB`, `0xC`, `0xD`, `0xE`** (`addiu a0,s3,0xb` with `s3` = 0..3), each placed at `rand() % (width * height)` - anywhere on the **whole** board. |
| Event tiles | `0x801ef508..0x801ef5b0` | Three cells, values `8` / `9` / `0xA`, placed at `col = rand() % width`, `row = rand() % ((height+1)>>1) + (height>>1)` - the **bottom half** only. |

Two corrections this table carries against an earlier reading of these bytes: the four animated tiles are **not** all value `0xB`, and only the *event* tiles use the half-height modulus - the animated ones scatter over the full board. `legaia_engine_core::tile_board::procedural_fill` already implements both correctly; it was the prose that drifted.

The cell buffer **is** heap-allocated at install: `DAT_801F35C0 = FUN_80017888(0, width * height)` at `0x801EF3E8..0x801EF3F4`, immediately before the base fill, with `width` and `height` re-read from the header (`_DAT_8007B450[3]` / `[4]`) and multiplied - so the buffer is exactly one byte per cell and nothing pads it. `FUN_80017888` is the logging wrapper over the game's allocator `FUN_8002B468` (a best-fit walk of a doubly-linked free list, `(size + 3) & ~3` alignment, heap index in `a0`); its failure arm prints `malloc err size %d` and bumps a byte counter at `gp+0x510`.

The walk SM's teardown state `0xE` frees it, together with the tile-actor table `DAT_801F35BC`, through the matching wrapper `FUN_80017B94` (`0x801EFE78` / `0x801EFE88`; it decrements the live-allocation count at `gp+0x488` that `FUN_80017888` raises). An earlier reading here said nothing frees the buffer; it had only looked at the per-scene control-block reset, which zeroes `_DAT_8007B450` and not this pointer.

The pointer has exactly **one** writer in the whole disc corpus (`find-gp-relative-refs.py --va 0x801f35c0`: 12 references across 84 images, one store), so the install site above is the allocation, not one of several. (`func_0x800204f8`, also called on the install path, is the move-table consumer, not an allocator.)

#### Address note: there is no `FUN_801e0b1c`

The procedural fill was previously cited as `FUN_801e0b1c`. **That address does not exist as a function.** The dump filed under that name was produced against the field overlay loaded at base `0x801C0000` instead of its correct base `0x801CE818`, so every address in it is short by exactly `0xE818`. The real code is at **`0x801EF334`**, which is not a function entry either - it is an **interior label of `FUN_801ef2b0`** (the tile-board walk SM, extent `0x801ef2b0..0x801f03ec`), promoted to a fake `FUN_` entry by the label-call idiom.

The same page previously gave that extent as `0x801ef2b0..0x801efe9c`. It is short by `0x550` bytes, and it stops at exactly the wrong place: `0x801EFE9C` is the last instruction *before* the render tail at [`0x801EFEA0`](#the-render-tail-0x801efea0), so the truncated extent cut the routine's whole draw half out of view. The real end is the epilogue at `0x801F03E8` (`jr ra` / `addiu sp,sp,0x48`, unwinding the `addiu sp,sp,-0x48` at the entry), 1104 instructions from the top.

The alias is recorded rather than silently renumbered because the old address appears in older notes: `FUN_801e0b1c` + `0xE818` = `0x801EF334`. At VA `0x801e0b1c` the field overlay actually holds `addiu v0,v0,-5`, part of an unrelated operand-nibble table lookup.

Provenance:
- Install: field-VM op `0x49` in `overlay_0897_801de840.txt` (a multi-subtype map-command opcode; `_DAT_8007b450 = pbVar47` arms the header pointer).
- Walk SM: `overlay_0897_801ef2b0.txt`.
- Procedural fill: `0x801EF334`, an interior label of `FUN_801ef2b0` (see the address note above - the old `FUN_801e0b1c` citation was a wrong-base alias).
- Board renderer: the tail block at **`0x801EFEA0`**, inside the walk SM (`overlay_0897_801efea0.txt`). It draws each cell value > 1 as `DAT_801f35bc[cell]` at the cell's world position. It is **not** a function: `FUN_801E0F3C`, the address this page used to cite for it, is another `+0xE818` print of the same routine - see the [address note](#address-note-there-is-no-fun_801e0b1c) and [the render tail](#the-render-tail-0x801efea0).

**Roster: the tile board is a field-overlay (`0897`) construct only.** Every install / walk-SM / fill / render site lives in `0897` and is reached from the field/event VM (op `0x49`). So the board is used by field/puzzle scenes, not by the hub minigames. **Confirmed** (`overlay_0897_801de840.txt` / `..._801ef2b0.txt`).

The `_DAT_8007b450` references in the dedicated minigame overlays (`dance` / `slot_machine` / `baka_fighter` / `fishing`) are all inside one shared library function `FUN_801e5b4c` - the equipment/stat **comparison-panel renderer** (2228 bytes, byte-identical across the `dance`/`cutscene`/`world_map`/`slot_machine` overlay dumps, i.e. resident in every overlay), which reads `_DAT_8007b450` only as a boolean *layout hint* (`== 0` → row pitch `0xe`, else `0xd`). It neither installs nor drives a board. The dance-core functions (`FUN_801cf470` / `FUN_801d1af4` / `FUN_801d231c`) do not touch `_DAT_8007b450` at all. **Confirmed** (`overlay_dance_801e5b4c.txt`).

## Cell value semantics

Cells are indexed `board[row * width + col]`. Confirmed value classes:

| Value | Meaning |
|---|---|
| `2` | **wall** - destination cell `== 2` rejects the move |
| `3`..`6` | walkable terrain types; sets `_DAT_8007b5f0 = (v - 3) * 2` (step variant) - [the walker's octant store](#the-walkers-octant-store) |
| `7` | trigger; routes the walk SM to its event sub-state |
| `8`..`10` | event / transition tile; consumes header `+7`/`+9` as flag **bases** into the system-flag bank `DAT_80085758` (reader `func_0x8003ce9c`; SET `func_0x8003ce08` / TEST `func_0x8003ce64`), and applies a half-tile world offset. Handled in walk SM `overlay_0897_801ef2b0.txt` case 8. |
| `0xb`..`0xe` | animated tiles; the arrival sub-state cycles the value `0xb → 0xe → 0xb` each visit, **and** writes the same `(v - 3) * 2` octant as `3`..`6` - [below](#the-walkers-octant-store) |
| other | plain walkable floor |

### The walker's octant store

The step-variant write is two bands, not one, and the clear in front of them is
unconditional. The whole cluster is six instructions at `0x801EF8A4`..`0x801EF8CC`
in the walk SM (field overlay 0897):

```text
801ef89c  lbu   s3, (v0)          ; the cell byte
801ef8a4  addiu v1, s3, -3        ; v1 = cell - 3, and it is NOT recomputed below
801ef8a8  sltiu v0, v1, 4         ; band 1: cells 3..6
801ef8ac  beqz  v0, 801ef8bc
801ef8b0  sw    zero, -0x4a10(a0) ; DELAY SLOT - always runs, taken or not
801ef8b4  sll   v0, v1, 1
801ef8b8  sw    v0, -0x4a10(a0)   ; gp+0x2D8 = (cell - 3) * 2
801ef8bc  addiu v0, s3, -0xb      ; band 2: cells 0x0B..0x0E
801ef8c0  sltiu v0, v0, 4
801ef8c4  beqz  v0, 801ef8d0
801ef8c8  sll   v0, v1, 1         ; DELAY SLOT - still (cell - 3) * 2
801ef8cc  sw    v0, -0x4a10(a0)
```

Two things a backward-only read of the second band gets wrong. The `sw zero` at
`0x801EF8B0` sits in a branch **delay slot**, so every walkable cell clears the
octant first and only the two banded ranges then write one - a cell outside both
bands leaves the octant at 0, it does not leave it alone. And the second band's
shift in the `beqz` delay slot at `0x801EF8C8` re-uses `v1`, which is still
`cell - 3` from the first band's `addiu` - not `cell - 0xB`. So the animated
tiles `0x0B`..`0x0E` store `16`, `18`, `20`, `22`, which the pad remapper's
`& 7` folds back onto octants `0`, `2`, `4`, `6`: the animated band aliases onto
the even half of the `3`..`6` band.

`0x801EFE7C` is not a third write of a fresh octant - it is the **restore** half
of a save/restore pair around the board mode: `0x801EF320` saves the incoming
`gp+0x2D8` into `0x801F35C4` on entry and `0x801EFE7C` puts it back on exit.

The octant is a plain scene-authored word, not a camera reading - see
[script-vm-menuctrl.md](script-vm-menuctrl.md#0x4c-nibble-2---the-camera-octant--pad-rotation-setter)
for its complete writer/reader census.

**Event-flag bases.** For an event cell whose event index is `evt`, base **A** (header `+7`, `u16` LE) is the **SET** base and base **B** (header `+9`, `u16` LE) is the **TEST/gate** base, both into the system-flag bank `DAT_80085758`: landing sets slot `A + evt + 1` (with `A` itself the first-visit master flag), while `B + evt` is the already-done guard tested before re-firing the event. (`overlay_0897_801ef2b0.txt` case 8.)

## Tile ↔ world coordinates

Each tile is `0x80` (128) world units; the actor sits at the tile centre:

```text
world_x = (header[+1] + col) * 0x80 + 0x40
world_z = (header[+2] + row) * 0x80 + 0x40
```

(`overlay_0897_801ef2b0.txt` case 4, target-position setup.)

## Rendering

The board carries **no geometry or texture data** - only ids and dimensions. It draws real field **actors** keyed by cell value: the per-cell-value tile-actor table `DAT_801f35bc[cell]` selects the actor for each cell. Slot `0` is the player, spawned from the header `+0xb` template id; slots `2`..`14` are the tile actors, spawned from the header `+0xc` base id as `header[+0xc] + (slot - 2)`. Each frame the renderer repositions the selected actor to the cell centre (`X = (originX + col) * 0x80 + 0x40`, `Z = (originZ + row) * 0x80 + 0x40`) and draws it. The header `+6` mode flag selects between the two draw passes (full-board vs. windowed around the player).

### The render tail (`0x801EFEA0`)

The renderer is not a function. It is the tail block of the walk SM
`FUN_801ef2b0`, entered by nineteen `j` / `beq` sites spread across the SM's
cases and running to that routine's own epilogue at `0x801F03E8`. Every case
therefore ends by falling into one shared draw pass, which is why no separate
render entry exists to cite. Provenance: `overlay_0897_801efea0.txt`.

It opens by re-reading the SM state at `actor[+0x54]` and gating on it. States
`0` and `0xE` draw nothing and return. State `5` - the confirm menu - first
draws its panel: a box through `FUN_8002C69C(0x64, 0x5C, 0x78, 0x28)`, three
strings through `FUN_80036888`, and a selection highlight through
`FUN_8002B994` keyed on `_DAT_8007BB88`. Every other state, and state `5` after
its panel, falls into the board pass.

The board pass then forks on the header `+6` mode flag, and the two arms are
not variants of one loop - they differ in what they skip and in what runs after
them:

| Header `+6` | Draw set | Cells skipped | Fixed-slot pass after |
|---|---|---|---|
| `0` (full board) | every cell of the board | value `< 2` | no - returns straight to the epilogue |
| non-zero (windowed) | the `+5`-radius square around the player cell, clamped to `[0, width)` / `[0, height)` | value `< 2` **and** values `8`..`0xA` | yes |

Both arms walk rows outer / columns inner, step the world position by `0x80`
per cell rather than recomputing it, look the cell value up in
`DAT_801f35bc[value]`, store the position into the actor's `+0x14` / `+0x18`,
commit it through `func_0x8003d344` and draw through `func_0x8001b964`.

**The full-board arm advances its cell cursor only on a drawn cell.** The
windowed arm computes `row * width + col` per iteration, but the full-board arm
keeps a running index that it increments inside the draw branch, so a skipped
cell leaves it pointing at the same byte for the next `(row, col)`. No retail
board can show it: the [procedural fill](#always-procedural-no-inline-cell-boards)
emits `rand()%6 + 2` and then values `8`..`0xE`, so no cell is ever below `2`
and the skip branch never runs. A port that permits cell values `0` or `1` -
which the engine's board type does - diverges from retail there, and only there.

**The fixed-slot pass** (windowed mode only) walks tile-actor slots `2`..`14`
once more. Slots `8`, `9` and `0xA` - the three event tiles - take their
`(col, row)` from the 2-byte-stride table at `DAT_801f35e0` and are positioned
and drawn at board coordinates. Every other slot is moved to `(0x3FC0, 0x3FC0)`
and **not** drawn, which is what stops the per-value shared actor from being
left standing at the last cell the board pass moved it to.

## Walk state machine

The board controller is a state machine keyed on the controller actor's `+0x54` field: fifteen states, bounded by `sltiu 0xF` and dispatched through the jump table at `0x801CF65C` (read from the PROT 0897 image; `overlay_0897_801ef2b0.txt` is the matching dump). Every arm falls into the render tail at `0x801EFEA0`.

| State | Entry | Role |
|---|---|---|
| `0` | `0x801EF310` | init: allocate the cell buffer + tile-actor table, spawn the player + tile actors from header ids, run the procedural fill at `0x801EF334`; its tail clears system flags `A..A+3` (`FUN_8003CE34`, `A` = header `+7`), seats the player cell at column `4`, row `0` with the walk-in target `(hdr[1] * 128 + 0x240, hdr[2] * 128 + 0x40)`, saves the octant `_DAT_8007B5F0` into `DAT_801F35C4` and zeroes `+0x9C` |
| `1` | `0x801EF680` | fade-in: `+0x9C += (d * 3) << 5` (`d` = `DAT_1F800393`), copied into every tile actor's `+0x72` render scale; at `0x1000` it clamps and goes to `2`, the walk-in |
| `2` | `0x801EFA88` | interpolate the actor's world position toward the target cell centre (`DAT_801f35d0`/`d4`); on arrival → `3` |
| `3` | `0x801EF6FC` | arrival: cell `7` → state `7`; cells `8..0xA` → state `8`; otherwise every animated cell **on the whole board** steps `0xB → 0xC → 0xD → 0xE → 0xB`, then → `4` |
| `4` | `0x801EF824` | **read input + collision + commit**: see below |
| `5` | `0x801EFBD0` | quit prompt (entered on the menu edge `_DAT_8007b874 & 0x10`, Triangle, which also sets `+0x9C = 0x1000` and the cursor `_DAT_8007BB88 = 1`): `FUN_80031D00`, then the two-choice picker `FUN_801E9DC8(0x8007BB88, 2, 1)` - Up/Down wrap (SFX `0x21`), confirm SFX `0x36`, cancel SFX `0x37`; confirm with `*0x8007BB88 == 0` → `6`, confirm on row `1` or cancel back to `4` |
| `6`, `7` | `0x801EFC2C` | → `9` (quit, trigger cell) |
| `8` | `0x801EFC38` | event cell: the flag writes below, then → `0xB` |
| `9`, `0xB` | `0x801EFCD0` | `+0x54 += 1` |
| `0xA`, `0xC` | `0x801EFCE4` | fade-out: `+0x9C -= DAT_1F800393 << 8`, copied into every tile actor's `+0x72` except the event tile the player stands on (cell `8..0xA` under the player skips the slot of that value); below zero → `0xD` |
| `0xD` | `0x801EFDA8` | park every tile actor at `(0x3FC0, 0x3FC0)` (`FUN_8003D344`), the same event tile excepted, → `0xE` |
| `0xE` | `0x801EFE64` | teardown: free the cell buffer and the tile-actor table (`FUN_80017B94`), restore `_DAT_8007B5F0` from `DAT_801F35C4`, zero the scene control block's `+0x3E` |

Zeroing `+0x3E` is how the board ends: the op-`0x49` subsystem actor (descriptor `0x8007065C`) polls it (`0x801F163C..0x801F16AC`), retires, and sets `_DAT_8007B450 = 1`, which the parked op reads as "done" and advances past its operand block.

### Event-cell flags (state 8)

With `v = cell - 8` and the two header bases read through the sign-extending halfword reader `FUN_8003CE9C` (`A` = `+7`, `B` = `+9`), state 8 **sets** system flag `A + v + 1` (`FUN_8003CE08`), **tests** flag `B + v` (`FUN_8003CE64`), and when that test is clear also **sets** flag `A`. So each of the three event cells owns one flag above the base, the base flag records "an event cell was reached", and the `B` bank lets a script pre-mark cells whose reach should not raise the base flag.

### State 4 - input, collision, commit

1. If the menu-button edge (`_DAT_8007b874 & 0x10`) is set, go to state `5`.
2. Read the pad `_DAT_8007b850` and remap it through `func_0x800467e8`, so "screen up" maps to the world direction the current octant names. The remap is a **quantized 45° (1/8-turn) rotation**, not a fixed 90° snap and not a continuous rotation: `FUN_800467e8` isolates the direction bits (`mask & 0xf000`), finds their index in the 8-entry ring `DAT_800766fc` (the 8 compass octants incl. diagonals), and re-emits `ring[(index + gp[0x2d8]) & 7]`. So the rotation amount is one of eight octants. (`800467e8.txt`.) `gp[0x2d8]` is a **scene-authored** word - the board's own cell bands and the field VM's `[4C 2x]` write it, nothing derives it from the camera - see [the walker's octant store](#the-walkers-octant-store).
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

## From-scratch port

[`legaia_engine_core::tile_board::TileBoard`](../../crates/engine-core/src/tile_board.rs) holds the board (dims + origin + cell bytes + player cell). [`World::tick`](../../crates/engine-core/src/world.rs) drives a board step in the `SceneMode::Field` arm when a board is installed (`World.board.grid`), reading `World.input` (the [input contract](engine.md)): it decodes one direction, gates against `cell == 2`, commits the player cell, and interpolates the player actor to the destination tile centre. The board stays inert (no-op) until installed, so it does not affect ordinary field scenes.

The install is wired to the field VM: op `0x49` **sub-op 5** hands the host the
13-byte inline header (`TileBoardHeader::parse`, the window retail points
`_DAT_8007b450` at - the sub-op byte plus the `+1..+0xC` fields above);
`World::try_install_tile_board` fills the cells with the ported procedural
fill (`tile_board::procedural_fill`, the `0x801EF334` algorithm:
every cell `rand()%6 + 2`, four animated tiles `0xB..0xE` at random cells
anywhere on the board, three event tiles `8..0xA` scattered into the bottom
half-board), seats the
player at the start-cell centre, and holds the script suspended through the
op-49 tristate. The arrival pass mirrors the walk SM's state 3: a trigger cell (`7`) or an
event cell (`8..=0xA`) exits the board mode - the suspended script reads
`Done` and resumes past the install op - and an event cell first writes the
state-8 flags above (`tile_board::event_cell_flag_writes`); any other arrival
advances every animated cell on the board (`tile_board::advance_animated_cells`).
The exits are not immediate: the arrival sets the walk SM's state
(`World::board.sm`, values in `tile_board::sm`) to `7` or `0xB`, and
`World::tick_tile_board` walks it through the one-tick step states, the
fade-out, the park and the teardown before the board goes and the op-`0x49`
script reads `Done`. The install starts in the fade-in, which ignores input
until the tiles reach full scale, and clears the header's four set-base flags
as state `0` does. The Triangle edge on an idle frame opens the quit prompt
(state `5`), a wrapping two-row picker on the menu cursor
(`menu_input::menu_cursor_nav`). The fade value `World::board.fade` is what
every tile actor's render scale follows (`World::tile_board_cell_scale`,
carried on `TileActorDraw::scale` to both hosts), and both hosts draw the
prompt panel through `legaia_engine_ui::tile_board_prompt_sprites_for` /
`tile_board_prompt_text_draws_for` with its three lines read off the field
overlay's image (`SceneHost::tile_board_prompt_lines`). The fades count one
`DAT_1F800393` unit per world tick (one vsync), the same wall-clock ramp as
retail's `d = 2` per game tick.

Two parts of state `0` are not modelled: the walk-in from the player's
position to column `4`, row `0` (the port seats the player on the board's
start cell at install and returns to input after the fade-in, so the state-3
arrival pass does not run on the start cell), and the octant save/restore
around the board (`_DAT_8007B5F0` into `DAT_801F35C4` and back at teardown).
The header's actor-template ids are kept on `World::board.header` for the
render consumers.

**Tile-actor spawn + reposition.** At install `World::try_install_tile_board`
spawns one field actor per distinct drawable cell value present on the board
(`2..=14`): each resolves its template `tile_template_base + (value - 2)`
through the same global-TMD + VDF-buffer path the `0x4C 0xD8` field allocator
uses (the shared `World::spawn_field_actor` helper), and the resulting
actor-pool slots are recorded in the per-cell-value tile-actor table
`World::board.actor_slots` (retail `DAT_801f35bc`; slot `0` = the reused player
actor, `2..=14` = the tile actors). Each field tick
`World::refresh_tile_board_draw_list` rebuilds `World::board.draw_list`:
for every drawable cell in the active draw set - the full board when the header
`+6` mode flag is `0`, else the windowed square of Chebyshev `+5` radius around
the player cell (`TileBoard::draw_cells`) - it selects the cell value's tile
actor and records it at the tile world-centre
(`(origin + idx) * 0x80 + 0x40`), repositioning the actor there (retail moves
the selected actor before drawing it). Because one actor backs each cell value,
a repeated value's actor ends at the last drawn cell while the draw list still
carries the full per-cell set the deferred renderer consumes. Board teardown
(`tile_board_arrival` on an event cell) despawns the tile actors and clears the
table + draw list so they don't leak into the next scene; the player actor
(drawn by the normal field path) survives.

## Open

- ~~Whether any board is *fixed* (inline-script cells) rather than procedurally filled.~~ **Resolved (negative):** sub-op-5 boards are **always procedural**. The install op advances the script cursor a constant `+0xe` regardless of `width × height` (`addiu fp,fp,0xe` at `0x801e0948`), so a cell array cannot ride the operand stream, and the cells are rand-filled at `0x801EF334`. There is no fixed-board variant to lift. See [always procedural](#always-procedural-no-inline-cell-boards).
- ~~The event-cell arrival's header `+7`/`+9` flag-operand consumption.~~ **Resolved and ported:** `+7` (base A) is the event-SET base and `+9` (base B) the TEST/gate base, both into the system-flag bank `DAT_80085758`, consumed in walk SM state 8 - see [event-cell flags](#event-cell-flags-state-8).
- ~~Per-cell tile-actor **rendering**.~~ **Resolved, on both hosts.** See [Rendering the board](#rendering-the-board).
- **No retail scene installs a board.** A disc-wide census (every partition record of all scene MANs, scripted-table + v12-embedded forms, walked with the field-VM disassembler, plus a raw byte-pair sweep) finds zero op-`0x49` sub-5 sites - the board is a script-reachable but retail-unused mode (pinned by the negative census test in `tile_board_draw_live.rs`).
  The [field-op census](../tooling/field-op-census.md) agrees: `asset field-op-census --only "49 05"` finds **0** clean occurrences across every scene MAN and event-script carrier, and its single non-clean decode (in `deene`'s event carrier) sits past a decode error. The board's states beyond the walk are therefore ported only as far as a synthetic board exercises them. The play-window `LEGAIA_TILE_BOARD_DEMO=1` env var synthesizes a retail-shaped 14-byte install near the player for that reason. Consequences: the intended per-cell tile *art* (retail header `+0xc` template base into `DAT_801f35bc`) and the board-plane Y behaviour have no retail reference to compare against - only a live capture of a debug-menu entry into the mode could pin them.

## Rendering the board

The assembly is `legaia_engine_core::tile_board`: `tile_board_actor_draws`
(per-cell draws off `World::board.draw_list`, floor-snapped Y, one mesh
instance per drawable cell), `tile_actor_slots_needing_mesh` and
`is_tile_actor_slot`. Unresolved templates degrade to no-draw rather than a
panic - and that is the whole of what is left here. `DAT_801F35BC` is a
**runtime** pointer array the install pass fills, not a table of art on the
disc, and its inputs are the header's `+0xb` / `+0xc` template ids. Since no
retail scene installs a board, no retail `+0xc` value exists anywhere on the
disc to read, so no byte sweep can name the intended art; the only instrument
that can is a live capture of a debug-menu entry into the mode, recording the
installed header's `+0xb`/`+0xc` and the board-plane Y at the same instant.

Each host uploads a board slot's template mesh once and skips board-owned
slots in its **generic** actor loop - a tile actor's own transform only holds
the last repositioned cell, so drawing it there as well ghosts one tile:

- play-window - `legaia_engine_shell::tile_board_draws` (a re-export of the
  three above) and the redraw pass;
- browser play page - `legaia_web_viewer::play_tile_board` and
  [`site/js/play-app.js`](../../site/js/play-app.js).

The assembly first lived in the shell crate, which pulls winit and does not
build for wasm32 - so the play page ran the walk SM against a board it never
drew, and a `CELL_WALL` cell blocked a player who could see nothing there.
That is a walk into an invisible wall, not a missing decoration, which is why
the shared home is `engine-core` rather than `engine-ui` (the UI crate takes
view structs, not a `World`).

Confirmed via offscreen screenshot diff (13 tile-actor meshes instanced per
cell). Disc-gated coverage:
`crates/engine-shell/tests/tile_board_draw_live.rs`.

**The re-export is not a second port site.** The `PORT:` tag for the draw pass
belongs on the `engine-core` function; the `engine-shell` module of the same
name is four `pub use` names and carries a `REF:`. Tagging both left an anchor
on a module nothing can call, which the live-reachability audit correctly read
as an inert port - a leftover of moving the assembly down into `engine-core`,
not a host that stopped drawing. The address those tags cite is itself a
wrong-base print: see [the render tail](#the-render-tail-0x801efea0) for why
`overlay_0897_801e0f3c.txt` is a second print of `0x801EFEA0`'s instructions
and no function begins where the tag says.

## See also

**Reference** -
[Field locomotion](field-locomotion.md) ·
[Field/event VM](script-vm.md) ·
[Encounter record](../formats/encounter.md)
