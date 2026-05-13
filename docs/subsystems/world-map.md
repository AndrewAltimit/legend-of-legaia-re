# World Map Subsystem

Covers the overworld traversal mode: normal walk view and the debug top-down view.
Sources: `overlay_world_map.bin` (walk-view) and `overlay_world_map_top.bin` (top-view debug)
captures from mednafen save states; decompiled at `ghidra/scripts/funcs/overlay_dialog_801e76d4.txt`,
`overlay_dialog_801ead98.txt`, and `801cfc40.txt`.

## Overlay structure

Two world-map overlay variants are paged into `0x801C0000..0x801EFFFF`:

| Variant | First prologue | Triggered by |
|---|---|---|
| Normal walk (`overlay_world_map`) | `0x801CFC40` | Standard world-map mode |
| Top-view debug (`overlay_world_map_top`) | `0x801CE850` | Debug toggle combo (see below) |

Both variants share the core field VM (`FUN_801DE840`), move-VM extension (`FUN_801D362C`),
and all rendering helpers. The top-view variant adds extra rendering code that starts ~0x1400
bytes earlier in the code window.

The view-mode toggle flag lives at `DAT_801F2B94` (outside the 192 KB extraction window
`0x801C0000..0x801EFFFF`; not captured in the binary dump).

## Key functions

### `FUN_801E76D4` - world map controller (9320 bytes)

Entry: `(ctx_ptr)`. Handles:

1. **Top-view debug toggle** - fires when `_DAT_8007B98C != 0` (debug flag) AND
   `_DAT_8007B850 == 0x4A` (pad mask) AND `_DAT_8007B874 == 0x40` (held mask).
   On trigger: `DAT_801F2B94 ^= 1` (flips walk/top-view), captures current actor
   camera position into `_DAT_801F35A8/AA/AC`, clears `ctx[+0x54]` and `ctx[+0x50]`,
   calls `FUN_80035C10`.

2. **Top-view camera controls** (active when `DAT_801F2B94 != 0`):
   - `_DAT_8007B850 & 0x1000` / `0x4000` → `_DAT_80089120 -= 8` / `+= 8` (X scroll)
   - `_DAT_8007B850 & 0x2000` / `0x8000` → `_DAT_80089118 -= 8` / `+= 8` (Z scroll)
   - `_DAT_8007B850 & 0x20` / `0x80` → `_DAT_8007B794 += 0x14` / `-= 0x14` (azimuth)
   - `_DAT_8007B850 & 8` / `2` → `_DAT_8007B6F4 -= 4` / `+= 4` (zoom/height)
   - Bit `DAT_801F2B95 & 1`: enables `FUN_801E75DC` (overlay animation step)
   - Bit `DAT_801F2B95 & 2`: second animation flag

3. **Normal-walk path** (`DAT_801F2B94 == 0`): standard per-frame world-map update
   (field VM tick, actor step, camera follow via motion VM).

### `FUN_801EAD98` - world map debug menu renderer (7280 bytes)

Entry: `(ctx_ptr, x, y, scroll_idx, max_visible)`. Renders a vertically scrolling
menu list for the world map developer menu. String table at `0x801CF344..`:

| Index | Label |
|---|---|
| 0 | `MAP_CHANGE` (or `CLOSED` when `_DAT_8007B868 != 0`) |
| 1 | `CARD_OPTION` (or `CLOSED`) |
| 2 | `PLAYER_STATUS` |
| 3 | `CAMERA` - shows `_DAT_80089120/_DAT_80089118` as `000 000` |
| 4 | `ENCOUNT` - shows encounter rate from `DAT_8007B5F8` |
| 5 | `OTHER_SETTINGS` |
| 6 | `BGM_CALL` - shows `_DAT_801F2E90` as `00` |
| 7 | `DEBUG` |
| … | At least 24 entries total (bounds check `local_40 > 0x17`) |

Called by `FUN_801ECA08` when the debug menu panel is active
(`ctx[+0x54]` mod-6 dispatch resolves to cases 1 or 3).

### `FUN_801CA08` - world map panel sizer / menu caller (256 bytes)

Entry: `(ctx_ptr, row_start, row_end, col_idx, ...)`. Computes panel height
`= (row_end - row_start + 1) * 8`; vertical offset `= 0xD0 - height` (centres
a 208-pixel viewport). Writes height/offset into a panel descriptor at
`0x801F2B98 + col_idx * 28`. Dispatches on `ctx[+0x54]` (6-way JT at
`0x801CF4CC`); cases 1 and 3 call `FUN_801EAD98` to draw the menu list.

### `FUN_801EE90C` - world map text-box dispatcher (128 bytes)

Entry: `(ctx_ptr)`. Dispatches on `ctx[+0x54]` via a 15-entry jump table at
`0x801CF5FC`. When `ctx[+0x54] >= 15` but `< 10`: falls through to
`FUN_80031D00` (text-actor tick - advances the MES bytecode one frame).

### `FUN_801CFC40` - world map sprite batcher (524 bytes, top-view only)

Entry: `(actor_ptr, ?, screen_x, screen_y, ?, ?)`. When `_DAT_8007B6B8 == 0x20`
delegates to `FUN_801CF9F4`; otherwise writes actor screen coordinates into
GPU registers `0x1F800020/22/24` from `actor[+0x14/+0x16/+0x18]`, then
iterates the sprite-descriptor list at `DAT_801C93C8`. Present only in the
`world_map_top` overlay variant.

### `FUN_801DA51C` - world map entity tick (260 bytes)

Entry: `(entity_ptr)`. 5-state dispatcher on `entity[+0x8A]` (jump table at
`0x801CEC28`). When `_DAT_80083808 == 0` and the entity state is 0: calls
`FUN_800243F0` (BGM/asset resolver) to look up the scene associated with the
entity's location. Handles pad-button checks against `_DAT_8007BB38` for
entity interaction. Called once per world-map entity per frame by the entity
pool tick loop.

#### Encounter-record installation

The body at `0x801DA620..0x801DA678` populates the global encounter formation
cell from a per-encounter record pointed at by `entity[+0x94]`:

1. Clear the 4-slot formation array at `0x8007BD0C..0x8007BD0F` (slots 3, 2,
   1, then 0 — slot 0 is cleared in the delay slot of `JAL 0x801DE190`).
2. Read `monster_count = entity[+0x94][+0x3]`.
3. Copy `entity[+0x94][+0x4 .. +0x4 + monster_count]` into the formation
   cell, byte-for-byte.

The encounter-record format consumed here is documented in
[`formats/encounter.md`](../formats/encounter.md). The 4-byte formation cell
at `0x8007BD0C` is the input to the battle-scene loader (`FUN_800520F0`); the
adjacent byte at `0x8007BD11` is a battle-data PROT-id selector that picks
between PROT entries `0x367` and `0x36D`.

The pointer at `entity[+0x94]` is set by field-VM op handlers inside the
script VM dispatcher (`FUN_801DE840`); see
[`subsystems/script-vm.md`](script-vm.md) and the op-handler family at
`0x801DEEDC` / `0x801DEF08` / `0x801DEFA0` / `0x801DF038` / `0x801DF3FC` /
`0x801E1C38` / `0x801E1F44` / `0x801E21C0`. Each handler is a different
"trigger encounter on actor X" op; they all share the clause:

```mips
sw   <record_ptr>, 0x94(<actor>)
sh   $zero,         0x54(<actor>)
ori  $tmp, $tmp, 0x400      ; raise "encounter armed" flag in actor[+0x10]
sw   $tmp,         0x10(<actor>)
```

## Render pipeline

The per-frame world-map render dispatches from the SCUS-resident game loop
into the overlay-loaded code window. Two chains converge: a per-frame
dispatch tick that reads the prim emitter's gate flag, and a one-shot
arm path that sets the gate plus a small param block.

### Per-frame dispatch (SCUS-resident)

Two SCUS-resident handlers from the 28-mode dispatch table at
`0x8007078C` ([game-mode state machine](../reference/functions.md#game-mode-state-machine))
reach the world-map render tick:

| Address | Mode-table role | Tick call |
|---|---|---|
| `FUN_80025EEC` | Default per-mode handler (used by 13 of 28 modes - not world-map-specific). | `FUN_8001698C` → `FUN_80016444(1)` → `FUN_80016B6C`. |
| `FUN_80025F2C` | Mode 13 (MAPDSIP MODE) - field/world-map display per-frame handler. | `FUN_8001698C` → `func_0x801CE850` (overlay entry) → `FUN_80016444(0)`. |

The `a0` arg controls whether `FUN_80016444` skips its early
`FUN_8005FB84` block (Mode 13 skips it; the default handler runs
it). Both reach the world-map render branch deeper in the function,
so the terrain emitter can fire from any of the 14 modes that route
through `FUN_80016444` whenever the submode register holds `2`.

### `FUN_80016444` - SCUS world-map render tick (1352 bytes)

Entry: `(submode_flag)`. Iterates the world-map render passes for one
frame. The pipeline includes a gated direct call into the overlay-
resident POLY_FT4 emitter:

```mips
80016750  lui   v1, 0x8008
80016754  lw    v1, -0x43c4(v1)        ; v1 = _DAT_8007BC3C (submode register)
80016758  li    v0, 0x2
8001675c  bne   v1, v0, 0x8001676c     ; skip unless submode == 2
80016764  jal   0x801d7ea0             ; -> overlay-resident emitter
```

Same `0x8007BC3C` register has six SCUS writers (`FUN_80016230` -
the two-write set/clear path - plus `FUN_80025980`, `FUN_80025DA0`,
`FUN_8001D424`); the writer that stores `2` is the entry point for
the world-map render branch.

### `FUN_801D7EA0` - world-map POLY_FT4 batch emitter (832 bytes)

Entry: `()`. One-shot emitter gated by `_DAT_801F351C`:

```c
if (_DAT_801F351C != 0) {
    _DAT_801F351C = 0;                   // self-clear gate
    iVar11 = 4;
    local_30 = 0x2C808080;                // POLY_FT4 GP0 cmd + neutral grey
    uVar6 = _DAT_801F3518
          + DAT_1F800393 * _DAT_801F3524; // angle += per-frame-tick * step
    _DAT_801F3518 = uVar6;
    local_3c = _DAT_801F3520;
    local_34 = _DAT_801F3520 / 5;
    local_38 = _DAT_801F3520 - local_34;
    do {
        iVar10 = cos_table[(uVar6 & 0xFFF)];   // 0x8007B81C cos LUT
        // emit 2x POLY_FT4 (chain tag 0x9000000) + 1 small prim
        // (chain tag 0x3000000); vertex coords are cos-rotation-
        // projected with local_3c/local_38 as scale moduli.
        ...
        uVar6 += 0x10;
        iVar11++;
    } while (iVar11 < 0xE4);              // 224 iterations
}
```

`_DAT_8007B81C` is the cos lookup table (`docs/reference/memory-map.md`).
The function emits ~670 prims per call (2 POLY_FT4 + 1 small per iter
across 224 iters). Vertex coordinates project via the cos table, so
the rendered output rotates with the camera angle - consistent with
a horizon / sky / animated-background plane, not a fixed continent
mesh. The bulk-continent POLY_FT4 chain (`~5000 prims` per the
prim-pool decoder) likely involves additional emitters that are
still to be identified.

The function is called by direct `jal` from SCUS - it does not need
function-pointer dispatch. Ghidra's reference manager misses the
cross-program call when sweeping the overlay alone; sweep SCUS to
surface the caller.

### Gate-arm chain - `FUN_801D1344` -> `FUN_801D8258`

The one-shot gate `_DAT_801F351C` is armed by a 40-byte trigger
function called from a 1332-byte parameter-prep wrapper:

| Address | Role |
|---|---|
| `FUN_801D1344` | World-map gate-arm wrapper. 1332 bytes; function-pointer-only entry (Ghidra `incoming=0`). Reads three globals at `_DAT_8007BCD0/_D4/_D8` and forwards them to `FUN_801D8258` as the scale / step / OT-layer params at PC `0x801D1470: jal 0x801D8258`. **Same RAM address holds a different function when the dialog overlay is paged in** - that variant is the actor frame handler (see [`reference/functions.md`](../reference/functions.md#dialog-overlay-actor-frame-helpers)). |
| `FUN_801D8258` | 40-byte gate setter. Writes `_DAT_801F351C = 1`, then `_DAT_801F3520 = param_2`, `_DAT_801F3524 = param_3`, `_DAT_801F3528 = param_4` - the inputs the emitter consumes on its next run. |
| `FUN_801C2B2C` | Code-identical relocation copy of `FUN_801D1344` in the 0897 field overlay. Same body, different load address; calls `jal 0x801D8258` at PC `0x801C2C58`. Active during field-mode entry transitions. |

The gate flag `_DAT_801F351C` is in the persistent `0x801F0000+` region,
so it survives overlay swaps. The flag is shared - both the world-map
overlay's `FUN_801D7EA0` and the 0897 field overlay's
`FUN_801C9688` read + clear it.

## Globals used

| Address | Role |
|---|---|
| `DAT_801F2B94` | View-mode flag: `0` = walk, `1` = top-view debug. Outside 192 KB extraction window. |
| `DAT_801F2B95` | Top-view animation bitfield (`& 1` = anim-A enable, `& 2` = anim-B). |
| `_DAT_80089120` | Top-view camera X scroll (adjusted ±8 per D-pad frame). |
| `_DAT_80089118` | Top-view camera Z scroll (adjusted ±8 per D-pad frame). |
| `_DAT_8007B794` | Top-view azimuth (adjusted ±0x14 per frame). |
| `_DAT_8007B6F4` | Top-view zoom/height (adjusted ±4 per frame). |
| `_DAT_8007B868` | Door/portal open flag: `0` = open (MAP_CHANGE/CARD_OPTION visible), `1` = CLOSED. |
| `_DAT_8007B6B8` | Game-mode discriminator (value `0x20` = alternate sprite path). |
| `_DAT_80083808` | World-map entity activation gate. |
| `_DAT_8007BC3C` | World-map submode register. `FUN_80016444` gates its `jal 0x801D7EA0` on this being `2`. Six SCUS writers (`FUN_80016230` / `FUN_80025980` / `FUN_80025DA0` / `FUN_8001D424`). |
| `_DAT_801F351C` | One-shot gate flag for the POLY_FT4 batch emitter. `FUN_801D8258` sets it to `1`; `FUN_801D7EA0` (and the 0897 sibling `FUN_801C9688`) clear it after one emission. Lives in the persistent `0x801F0000+` region and survives overlay swaps. |
| `_DAT_801F3518` | Running camera angle. Advanced by `DAT_1F800393 * _DAT_801F3524` per `FUN_801D7EA0` call; masked to 4096 entries when indexing the cos LUT at `0x8007B81C`. |
| `_DAT_801F3520` | Render scale / range. Sourced from `_DAT_8007BCD4` via `FUN_801D8258`'s `param_2`. The emitter uses it both as `local_3c` and `local_3c / 5`. |
| `_DAT_801F3524` | Angle step per frame tick. Sourced from `_DAT_8007BCD8` via `FUN_801D8258`'s `param_3`. |
| `_DAT_801F3528` | OT layer / draw priority. Sourced from `_DAT_8007BCDC` via `FUN_801D8258`'s `param_4`. |
| `_DAT_8007BCD0..D8` | Three contiguous u32 globals that `FUN_801D1344` reads and forwards as the gate-setter's scale / step / OT-layer params. |
