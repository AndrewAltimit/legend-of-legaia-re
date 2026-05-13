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
so the horizon emitter can fire from any of the 14 modes that route
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
mesh.

The case-5 path of the [per-actor render dispatcher `FUN_8001ADA4`](#per-actor-render-dispatcher---fun_8001ada4)
draws every **landmark** TMD (castle, towers, bridges, gates) - each
world-map actor's `actor[+0x44]` mesh chain points into Drake's
40-TMD landmark pack at PROT entry 0085 slot 1, which the dispatcher
walks once per frame through `FUN_8002735C` (the 60-GTE Legaia TMD
renderer). That accounts for the landmark prims in the GPU pool.

**Open**: the bulk continent ground terrain (~3500-4000 POLY_FT4 prims
of green/rocky tiles in the prim pool) is **not** sourced from any
TMD-magic-bearing disc payload that has been located so far. Slot 4
of each kingdom bundle (type byte `0x05`, see [`world-map-overlay`
format doc](../formats/world-map-overlay.md)) was a logical candidate
but visual inspection across every projection and topology mode
falsified the wireframe / coastline reading - the container is solved
but the records carry something else (likely a library of object-local
3D meshes, consumer not yet pinned in Ghidra). The remaining hypothesis
for the bulk continent terrain is a procedural emitter sibling of
`FUN_801D7EA0` reachable from the same per-frame tick - that sibling
is the most likely site of the bulk terrain generation but has not been
located yet.

The horizon emitter is called by direct `jal` from SCUS - it does not
need function-pointer dispatch. Ghidra's reference manager misses the
cross-program call when sweeping the overlay alone; sweep SCUS to
surface the caller.

#### Bulk continent terrain emitter investigation

Audit of `FUN_80016444`'s direct `jal` targets ([dump]({{ghidra}}/scripts/funcs/80016444.txt))
finds 12 unique targets across 60 call sites. None of the unmapped
targets are a bulk-terrain emitter:

| Target | Calls | Role |
|---|---:|---|
| `0x8001a068` | 19 | Actor-list pass dispatcher (8 of the 19 hit at the function head). |
| `0x8005fb84` | 18 | Early-return block; skipped when submode == 2. |
| `0x8002519c` | 6 | Per-frame render-pass iterator (5 lists per frame). |
| `0x8001d140` | 6 | Stack-swap wrapper → `FUN_8001ADA4` (per-actor render dispatcher). |
| `0x800172c0` | 1 | GTE matrix setup helper (`FUN_80026988(&local_18, 0x1F8003A8)`). |
| `0x800179c0` | 1 | Small helper. |
| `0x800188c8` | 1 | Debug-HUD text renderer (`PSX_TEST_PROGRAM` string). |
| `0x8001d058` | 2 | 48-byte scratchpad / GPU register flush. |
| `0x8002b688/790` | 2 / 2 | Tiny accessors (60-80 bytes each). |
| `0x80046978` | 1 | Screen-tint fade emitter (gated on `gp+0x9d4`). |
| `0x801d7ea0` | 1 | **Horizon emitter** (overlay-resident, single direct call). |

None of these dispatch into a function whose body contains a bulk
POLY_FT4 loop. The remaining static signals are negative:

- **Static `addprim` hunters find only the horizon emitter inside any
  world_map overlay variant** (`addprim_emitters_overlay_world_map.bin.txt`,
  `*_top.bin.txt`, `*_walk.bin.txt` all report exactly one candidate:
  `FUN_801D7EA0`).
- **SCUS-side `addprim` candidates** are five non-terrain emitters:
  `FUN_8002C69C` (HUD sprite batch, 10 sites), `FUN_8006A420` (2 sites),
  `FUN_8001D424`, `FUN_8001C394`, `FUN_8002B994` - all sprite/digit
  batchers used by HUD/menu paths.

The strongest remaining hypothesis is a **table-driven emitter** -
i.e. a function whose POLY_FT4 cmd byte is loaded from a per-mode
descriptor table (the same trick `FUN_8002735C` uses, sourcing the
cmd from `DAT_8007326C`). Static addprim hunters miss those because
the cmd byte is never an `lui/li` immediate. Two leaf candidates
inside the case-5 dispatcher chain:

| Function | Where it's called | Role |
|---|---|---|
| `FUN_80043390` (SCUS, 712 bytes) | case-5 default branch | Textured TMD renderer; uses GTE colour matrix setup; reads structural fields at `mesh+0x14/+0xc/+0x18`. |
| `FUN_80029888` (SCUS) | case-5 env-mapped branch | Environment-mapped TMD renderer; triggered when `actor[+0x7a] != 0`. |

If the bulk terrain is built from many small textured-TMD "tile"
actors whose render fn routes through `FUN_80043390`, then static
analysis correctly rules out a single bulk emitter - the prims emerge
from N small calls, each emitting a handful of polys from its own
small mesh chain. Verifying this requires a dynamic probe: log every
PC that writes into the prim pool (`0x800AD400..`) for one second of
top-view gameplay, then bucket by caller. Hook lives at
[`scripts/pcsx-redux/autorun_prim_pool_writers.lua`](../../scripts/pcsx-redux/autorun_prim_pool_writers.lua).

The bulk terrain's source mesh data may then be the same kingdom
slot-1 TMD pack the landmarks come from - just slots that
`world-overview.json` doesn't surface today because no `MAN` placement
references them. Drake has 40 TMDs in slot 1 but the placement table
only uses 28 unique slots; the unused slots (or slots with `pos=0`
that the scatter view filters out) could be the ground tiles, placed
at runtime by a generator that hasn't been pinned to a function yet.

### Per-frame render-pass iterator - `FUN_8002519c`

Five times per frame, `FUN_80016444` invokes the SCUS-resident actor-list
iterator `FUN_8002519c` (328 bytes) against five linked-list heads at
`_DAT_8007C34C..._DAT_8007C36C`. Each list is one render pass. The
iterator walks the chain and per node:

```c
for (node = *(list_head); node != NULL; node = node->next) {
    if (node->flags & 0x8) {
        if (node->flags & 0x200) {
            // already-emitted path: skip the heavy work
        } else if (node->fn == &FUN_80021df4) {
            // standard per-frame actor tick
            ...
        }
    } else {
        ((void (*)(void *))node->fn)(node);   // jalr node->fn
    }
    // mark `flags |= 0x200` to dedupe in case the list is walked again
}
```

Per-actor record layout consumed by the iterator:

| Offset | Type | Role |
|---|---|---|
| `+0x00` | `actor *` | Next pointer (singly linked list, `NULL` terminates). |
| `+0x0C` | `void (*)(actor *)` | Tick function (the entry point `jalr` calls). |
| `+0x10` | `u32` | Flags; bit `0x8` selects the early-return path, bit `0x200` is the "already-emitted this frame" guard. |
| `+0x14` | `u32` | Saved next-pc copy used by the early-return path. |
| `+0x18` | `u16` | Halfword count exposed at `+0x20` for the early-return path. |
| `+0x44` | `chain *` | Optional prim-chain head; freed via `FUN_80017b94` when bit `0x800` is set. |
| `+0x48` | `u8 *` | Move-VM bytecode base (for actors whose tick is `FUN_80021df4`). |
| `+0x70` | `u16` | Move-VM PC in halfword units; the actual byte offset is `2 * actor[+0x70]`. |

Standard tick functions observed in the world-map render passes:

| Tick function | Where | Role |
|---|---|---|
| `FUN_80021DF4` (SCUS) | per-frame actor tick | Steps the move VM via `FUN_80023070(actor)`. The eight actors in list `_DAT_8007C350` use this tick. |
| `FUN_8003BC08` (SCUS) | per-actor tick | Calls the motion VM (`FUN_8003774C`), move-buffer setup (`FUN_800204F8`), and overlay helper `FUN_801D79E8`. The fourteen actors in list `_DAT_8007C354` use this tick. |
| `FUN_801E76D4` (world_map overlay) | world-map controller | Top-view debug toggle + camera scroll/azimuth/zoom + dev-menu render. |
| `FUN_801DA51C` (world_map overlay) | per-entity tick | 5-state SM on `entity[+0x8A]` (see [actor-vm](actor-vm.md)). |
| `FUN_801D1344` (world_map overlay) | horizon gate-arm wrapper | See the gate-arm chain below. |

### Per-actor render dispatcher - `FUN_8001ADA4`

In addition to the five TICK calls into `FUN_8002519c`, the same frame
issues six RENDER calls into the stack-swap wrapper `FUN_8001D140`,
which forwards into the per-actor render dispatcher `FUN_8001ADA4`
(2456 bytes). The render dispatcher walks the same actor lists but
runs a different switch - on `actor[+0x56]` (render mode `1..0xB`):

- **case 4** (multi-target). Dispatches on `actor[+0x9e]` flags:
  - bit `0x4000` → `FUN_8002A5A4` (SCUS).
  - bit `0x2000` → `FUN_801CFA48` (overlay-resident).
  - else → `FUN_80028158` (SCUS, distinct from the 6692-byte motion
    bytecode VM `FUN_80038158`).
- **case 5** (full TMD). Iterates the mesh chain at `actor[+0x44]`
  (`puVar5[0]` = count, `puVar5[1..n]` = mesh pointers) and per
  entry calls:
  - `FUN_80043390(mesh, color, tpage)` - textured TMD (default).
  - `FUN_80029888(...)` - environment-mapped TMD when
    `actor[+0x7a] != 0`.
  - `FUN_8002735C(...)` - 60-GTE Legaia TMD renderer (the
    **landmark emit leaf** — each landmark TMD in Drake's 40-mesh
    kingdom pack passes through here; the bulk continent ground
    terrain is *not* drawn from here).
- **cases 1, 2, 3, 6, 7, 8, B** - distance-LOD / particle / sprite-billboard
  branches calling per-effect helpers (`FUN_8001B73C`, `FUN_8001B964`,
  `FUN_800480D8`, `FUN_8002B944/94C/954`, `FUN_8001C204`).

Static `addprim` hunters do not surface `FUN_8002735C` as a POLY_FT4
emitter because the cmd byte is read from the per-mode descriptor
table at `DAT_8007326C`, not built with `lui/li` immediates. That is
why the landmark TMD emitter eluded static analysis: the addprim scan
flags every direct emitter (the horizon, the HUD sprite batch
`FUN_8002C69C`, the screen-tint, etc.) but skips the TMD renderer
where the landmark prims actually originate. The bulk continent
ground terrain that fills the rest of the prim pool is a separate
emit path that has *not* been located yet.

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
| `_DAT_8007C34C..0x36C` | Seven actor-list heads consumed by `FUN_8002519c`. `_DAT_8007C34C` / `_DAT_8007C350` / `_DAT_8007C354` / `_DAT_8007C358` / `_DAT_8007C35C` / `_DAT_8007C360` / `_DAT_8007C36C` correspond to the five world-map render passes that `FUN_80016444` issues per frame plus two scratch heads. |
