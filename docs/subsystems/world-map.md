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

The view-mode toggle flag lives at `DAT_801F2B94`. The world-map overlay
variants extend past `0x801F0000` - capture them with a wider window
(`0x801C0000..0x801F9000`, 228 KB) to include the prim-mode dispatch
table at `0x801F8968` and its eight overlay-resident emit leaves at
`0x801F7644..0x801F8690`. The old 192 KB default
(`0x801C0000..0x801EFFFF`) clips both. `scripts/extract-mednafen-overlay.py`
now defaults to the wider window.

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

### Bulk continent terrain emit mechanism (pinned)

The bulk continent prims are not produced by a procedural emitter
sibling of `FUN_801D7EA0`.
They come out of ordinary case-5 TMD rendering whose per-prim dispatch
is **mode-switched** to overlay-resident renderers when the world-map
overlay is paged in. `FUN_80043390` (the SCUS-side per-prim TMD
renderer at the leaf of the actor-mesh-chain walk) selects one of two
function-pointer tables based on `_DAT_1F800394 & 1`:

| Flag | Table base | Rows | Where it lives |
|---|---|---|---|
| clear | `0x8007657C` | 4 (alpha 0/50/A0/F0) | SCUS_942.54 |
| set | `0x801F8968` | 1 (alpha 0 only) | world-map overlay |

The overlay path skips the alpha offset (`_DAT_1F800028` is not added
on the overlay branch), so only the first row of the overlay table is
meaningful. Slots 8..11 of row 0 share the same low-mode dispatchers
as SCUS (`0x8004409C, 0x8004423C, 0x80044434, 0x800445B0`); slots
12..19 carry the eight overlay-resident high-mode renderers:

| Slot | Address | Role (per case-5 TMD prim mode) |
|---|---|---|
| 12 | `0x801F7644` | high-mode prim renderer A |
| 13 | `0x801F7838` | high-mode prim renderer B |
| 14 | `0x801F7F78` | high-mode prim renderer C |
| 15 | `0x801F8198` | high-mode prim renderer D |
| 16 | `0x801F7AA4` | high-mode prim renderer E |
| 17 | `0x801F7CCC` | high-mode prim renderer F |
| 18 | `0x801F8454` | high-mode prim renderer G |
| 19 | `0x801F8690` | high-mode prim renderer H |

Each is a per-primitive emitter that loads vertex indices from the TMD
prim body, looks vertices up in the actor's vertex pool (passed as
`a2`), runs them through the GTE, and emits one GPU prim packet into
the chain at `_DAT_1F8003A0`. Static `addprim` hunters never surfaced
them because (a) the cmd byte is loaded from a per-mode descriptor table
rather than as a `lui`/`li` immediate, and (b) the captured overlay
window stopped at `0x801F0000` (which clipped every leaf address).

#### Per-slot delta vs SCUS sibling

First-pass capstone disassembly (`scripts/disasm-overlay-fn.py --batch
leaves` against the extended overlay; `--batch scus-siblings` against
`SCUS_942.54`) shows every overlay leaf is the SCUS sibling body plus a
**distance-cue fog post-process**. The fog block is inserted between the
GTE projection and the OT packet write; its shape is constant across all
eight slots:

```mips
; --- common GTE projection (identical to SCUS sibling) ---
mtc2  v0, v1, v2                                ; vertex coords -> GTE
GTE.rtpt                                         ; perspective transform
GTE.nclip                                        ; backface test
GTE.avsz3 / avsz4                                ; average Z

; --- fog block (overlay-only) ---
lbu   s3, -0x2d1(gp)                             ; fog-enable byte
andi  s3, s3, 0x10                               ; bit 4 = "fog on"
bnez  s3, fog_path                               ; bypass if cleared
mfc2  z1, sxyz1; mfc2 z2, sxyz2; mfc2 z3, sxyz3  ; per-vertex Z
; Z_far = max(z1, z2, z3) >> *(u8 *)(gp+0x90)
sub/move/branch sequence to pick the max         ; min-of-mins clamp
lbu   shift, 0x90(gp); srlv Z_far, Z_far, shift
lw    fog_ref, -0x2e0(gp)                        ; far-plane reference
lwc2  fog_color, -0x2dc(gp)                      ; fog color -> GTE
or    cmd, cmd, fog_ref                          ; mix into prim cmd
mtc2  cmd, rgbc
GTE.dpcs                                          ; depth-cue single
; per-vertex RGB LUT tint: indexes lut[Z>>5] from gp-0x2bc
; for each of the three R,G,B sub-vertices and ADD-writes
; the result into offsets 8/0xc/0x10 of the OT packet.

; --- common OT write (identical to SCUS sibling) ---
sw    rgbc,    0(t1)
sw    p0_xy,   4(t1)
sw    p1_xy,   8(t1)
...
addi  t1, t1, 0x14                                ; stride 20 (5 words)
bgtz  t3, loop_top
addi  t3, t3, -1
j     0x80043580                                  ; tail call to SCUS
                                                  ; dispatcher continuation
```

The fog parameters all sit at GP-relative offsets in the per-frame
camera/render context block accessed through `$t2`:

| GP offset | Role |
|---|---|
| `-0x2e0` | Far-plane reference Z (mixed into prim cmd word). |
| `-0x2dc` | Fog color (loaded into GTE color register before `dpcs`). |
| `-0x2d1` | Fog-enable flags byte; bit `0x10` gates the whole fog path. |
| `-0x2bc` | Pointer to per-Z fog-tint LUT (2-byte entries, indexed by `Z >> 5`). |
| `+0x90`  | Z shift exponent (controls how aggressively far-plane Z compresses). |

Each overlay leaf is the SCUS sibling at 60-80 instructions plus
~60-80 instructions of the fog block, so every leaf roughly doubles in
size:

| Slot | SCUS sibling | Overlay leaf | Overlay-only GTE ops added |
|---|---|---|---|
| 12 | `0x80043658` (68 instr) | `0x801F7644` (125 instr) | `dpcs` |
| 13 | `0x80043768` (84 instr) | `0x801F7838` (155 instr) | `dpcs` |
| 14 | `0x80043B58` (69 instr) | `0x801F7F78` (136 instr) | `dpcs` |
| 15 | `0x80043C6C` (90 instr) | `0x801F8198` (175 instr) | `dpct` + `dpcs` |
| 16 | `0x800438B8` (75 instr) | `0x801F7AA4` (138 instr) | `dpcs` |
| 17 | `0x800439E4` (93 instr) | `0x801F7CCC` (171 instr) | `dpcs` |
| 18 | `0x80043DD4` (79 instr) | `0x801F8454` (143 instr) | `dpcs` |
| 19 | `0x80043F10` (99 instr) | `0x801F8690` (182 instr) | `dpct` + `dpcs` |

The two slots that use `dpct` (Depth-Cue Triple: applies the fog to all
three current GTE color registers at once) are the textured-quad modes
(slot 15 and 19) - the rest only need `dpcs` because they emit one color
per prim. Every leaf ends with `j 0x80043580; addiu $a1, $a1, 0xc` -
the tail call back to the SCUS dispatcher continuation plus the loop's
per-prim source-pointer advance.

The capstone dump source lives in `/tmp/leaves/` (overlay) and
`/tmp/scus-siblings/` (SCUS); regenerate with:

```bash
scripts/disasm-overlay-fn.py /tmp/overlay_world_map_top_ext.bin \
    --base 0x801C0000 --batch leaves        --out-dir /tmp/leaves
scripts/disasm-overlay-fn.py extracted/SCUS_942.54 \
    --base 0x80010000 --header 0x800 --batch scus-siblings --out-dir /tmp/scus-siblings
```

This is **why the bulk continent prims didn't show up under a single
emit function**: there isn't one. There are eight per-mode leaves
called once per TMD primitive across however many actor mesh chains are
active. The source mesh data is the same kingdom-bundle TMD pack
(slot 1, type `0x02`) the landmarks draw from - the world-map top-view
just routes its prim emit through the overlay-replaced per-mode
renderers, which apply whatever camera/fog/atmospheric transform the
top-view needs that the SCUS variants don't.

`mednafen-state prim-dispatch-table <save>` decodes both tables out of
a save state's main RAM and surfaces the eight overlay-resident
targets. Use `--overlay-targets-only` to pipe the eight addresses into
a Ghidra `dump_funcs.py` `TARGETS` list. See
[`legaia_mednafen::prim_dispatch`](../../crates/mednafen/src/prim_dispatch.rs).

Slot 4 of each kingdom bundle is **not** the bulk-terrain source. Its
records are something else (a runtime library of object-local 3D
meshes, see [`world-map-overlay`](../formats/world-map-overlay.md)) -
that hunt is independent of the continent terrain emit mechanism.

The horizon emitter is called by direct `jal` from SCUS - it does not
need function-pointer dispatch. Ghidra's reference manager misses the
cross-program call when sweeping the overlay alone; sweep SCUS to
surface the caller.

#### `FUN_80016444` jal-target audit (background)

Audit of `FUN_80016444`'s direct `jal` targets ([dump]({{ghidra}}/scripts/funcs/80016444.txt))
finds 12 unique targets across 60 call sites. **None is a bulk-terrain
emitter on its own** - that conclusion lines up with the dispatch-table
mechanism documented above (the bulk prims come from many small
case-5 TMD renders going through the overlay-replaced per-mode
renderers, not from a single emit function called from this site):

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
POLY_FT4 loop. The remaining static signals were initially confusing:

- **Static `addprim` hunters find only the horizon emitter inside any
  world_map overlay variant** (`addprim_emitters_overlay_world_map.bin.txt`,
  `*_top.bin.txt`, `*_walk.bin.txt` all report exactly one candidate:
  `FUN_801D7EA0`).
- **SCUS-side `addprim` candidates** are five non-terrain emitters:
  `FUN_8002C69C` (HUD sprite batch, 10 sites), `FUN_8006A420` (2 sites),
  `FUN_8001D424`, `FUN_8001C394`, `FUN_8002B994` - all sprite/digit
  batchers used by HUD/menu paths.

Both negatives are explained by the **descriptor-table-driven dispatch
documented above**: the world-map top-view's bulk terrain prims come out
of `FUN_80043390 → overlay-resident high-mode renderer` where the cmd
byte is loaded from `DAT_8007326C` (the per-mode descriptor table the
60-GTE Legaia TMD renderer uses), not built with `lui`/`li` immediates.
That makes the leaves invisible to addprim hunters, and the overlay
extraction window stopped at `0x801F0000` so the leaves weren't even in
the captured binary.

The dynamic prim-pool-writers probe at
[`scripts/pcsx-redux/autorun_prim_pool_writers.lua`](../../scripts/pcsx-redux/autorun_prim_pool_writers.lua)
confirms this: top-of-list PC hits land in the
`0x801F7344..0x801F8DBC` overlay-resident range, exactly the eight
high-mode renderer addresses pinned by the overlay dispatch table.

The bulk terrain's source mesh data is the same kingdom slot-1 TMD
pack the landmarks come from. Drake has 40 TMDs in slot 1; the
`world-overview.json` placement table surfaces 28 unique slot indices,
but the world-map top-view also walks runtime-positioned character /
NPC mesh chains (lists `_DAT_8007C354` and friends) - the 73 actors in
list #2 with mode=5 mesh chains pointing into `0x80128xxx..0x80139xxx`
(inside the landmark pack RAM range) account for the bulk prims when
each is rendered through the overlay-replaced renderers.

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
ground terrain follows the same dispatch-table pattern - via
`FUN_80043390`'s overlay-mode jump table at `0x801F8968` and its eight
overlay-resident high-mode renderers - documented earlier in this
section.

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
| `_DAT_80089120` | Top-view camera X scroll (adjusted ±8 per D-pad frame). Stored as the **negated map-origin X** (`_DAT_80089120 = -(int)*(short *)(actor + 0x18)` in `overlay_0978_801c5c58.txt:465`; identical in `overlay_slot_machine_801db8ec.txt:115`). The world camera target is `-_DAT_80089120`. |
| `_DAT_80089118` | Top-view camera Z scroll (adjusted ±8 per D-pad frame). Stored as the **negated map-origin Z** (`_DAT_80089118 = -(int)*(short *)(actor + 0x14)` in `overlay_0978_801c5c58.txt:464` / `overlay_slot_machine_801db8ec.txt:114,118`). The world camera target is `-_DAT_80089118`. |
| `_DAT_8007B794` | Top-view azimuth (adjusted ±0x14 per frame). |
| `_DAT_8007B6F4` | Shared word: in world-map mode this is the top-view zoom/height (adjusted ±4 per D-pad frame, retail walk-view loads `0x0170`); outside world-map mode it doubles as a camera-mode flag (the "Small Maps" debug toggle in `docs/reference/builds.md` / `docs/reference/cheats.md`; retail walk-in-field saves load `0x0002`). |
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

## World-overview viewer

The `/world-overview/` page in the static site renders each kingdom's
landmark layer in real-time WebGL 3D from a disc image. It exists to
make the world-map data layer reviewable end-to-end without a save
state or an emulator.

### Layout engine for unplaced slot-1 TMDs

The MAN placement table pins a small subset of each kingdom's slot-1
TMD pack at world coordinates (5 / 6 / 17 slots for Drake / Sebucus /
Karisto). The remaining slots are positioned at runtime by the
field-VM via actor-mesh chains and don't carry a static world coord.
The viewer's "show unplaced slot-1 TMDs" toggle drops those onto a
canonical layout grid, classified by `slot1_classification.toml`:

- **landmark** &mdash; row south of the kingdom bounds, sorted by slot.
- **decoration** &mdash; row north of the kingdom bounds.
- **ground_tile** &mdash; grid west of the kingdom (the runtime tiles
  them via the overlay-routed dispatch table).
- **npc_token** &mdash; hidden (reused generic actor bases; reporting
  the count avoids cluttering the view).
- **unknown** &mdash; grid east of the kingdom.

Two per-mesh transforms keep the layout legible:

1. **AABB-centroid anchor** &mdash; each unplaced TMD is drawn so its
   AABB centroid sits at the assigned grid slot, instead of its
   TMD-local origin (which can be far from the visual centre and
   shift the mesh out of frame).
2. **Class-conditional footprint normalisation** &mdash; per-class
   target footprints in world units (landmark ~600, decoration ~200,
   ground_tile ~1200, unknown ~600). Each mesh's larger XZ extent maps
   to the target via a per-placement scale so the row reads at a
   consistent size regardless of the TMD's native scale.

The "normalize unplaced" toggle disables both transforms (falls back
to the legacy constant scale + TMD-local-origin pivot) so the user
can ground-truth against retail.

### Distance-cue fog pass

The viewer's fog toggle approximates the retail world-map fog: the
diffuse term fades toward a per-kingdom haze colour with distance.
The math splits into two pieces the runtime keeps separate, and the
WebGL port mirrors that split:

- The **LUT** at `gp-0x2BC` (2048 u16 entries that climb from `0x0000`
  at near-Z to `~0x01FF` at far-Z) is a **per-Z scalar**, not a colour
  ramp. The retail overlay leaves at `0x801F7644..0x801F8690` `lh` the
  LUT entry, shift it left by 16, and add it to the high half of
  vertex SXY+offset words via `sw s1, 0x8(t1)` / `0xC(t1)` /
  `0x10(t1)`. The visible effect on flat triangles is a per-vertex
  screen-Y nudge proportional to `Z >> 5`.
- The **haze colour** is set per-kingdom via the GTE `FAR_COLOR`
  control register (loaded via `ctc2` during world-map enter, not
  surfaced by the `lwc2 t0, -0x2dc(t2)` load - that field is the
  `IR0` depth-cue factor, despite earlier doc tables labelling it
  "fog color").

The WebGL port runs this in a vertex + fragment shader:

- Per-vertex: `Z_far = exp2(-zShift) * dist(world, camera_origin)`,
  clamped to `[0, far_ref]` and normalised to `v_fog_t in [0..1]`.
  Approximates the runtime's `Z_far = Z >> shift` against the
  top-down camera origin.
- Per-fragment: sample `lut[clamp(v_fog_t * 2047, 0, 2047)]` as a
  scalar u16; normalise to `factor = lut_word / 511`; then
  `mix(lit, u_fog_color, factor)` with `u_fog_color` = the
  per-kingdom haze tint from `KINGDOM_FOG_TINT`. This produces the
  fade-toward-haze visual instead of treating the LUT entries as
  RGB tints (an earlier port did the latter and produced "richer
  textures" rather than fog).

The shader supports two LUT sources, in priority order:

1. **Disc-extracted LUT (default)** &mdash; the WASM viewer locates
   the 4 KiB (2048 u16) LUT inside `SCUS_942.54` via the
   `fog_lut::find` content-scan (monotone non-decreasing ramp with
   leading zero entries + saturating tail) and auto-uploads it on
   disc load. No file picker; one disc upload = full functionality.
   On the retail USA build the LUT sits at SCUS offset `0x05FCC0`
   (vaddr `0x8006FCC0`); the content scan handles regional variants
   without hardcoding.
2. **Kingdom-tinted fallback** &mdash; when SCUS extraction doesn't
   surface a LUT (raw PROT.DAT load, regional variant with shifted
   SCUS, modded disc), the shader falls back to using `v_fog_t`
   directly as the mix factor, still toward the kingdom haze tint.

The per-vertex math diverges from retail in one place: retail samples
Z from the GTE's screen-space pipeline after `rtpt`, while the
WebGL2 path uses XZ-plane distance to the fog origin (`fog_origin =
worldCam centre` by default). For a top-down ortho camera the two
quantities are equivalent up to a constant; for the orbit-camera mesh
inspector the fog toggle is hidden because it doesn't carry over.

### Bulk-terrain placement resolver (MAN `0x7F` sentinels)

MAN-record placements where ``(x_enc, z_enc) == (0x7F, 0x7F)`` static-
decode to the literal world coordinate ``(16320, 16320)`` (the
world's NE corner, just outside any visible kingdom). Those actors
are positioned at runtime by the FieldVM prescript embedded in the
record's trailing bytes, dispatched from ``FUN_8003A1E4`` (the MAN
placement walker in SCUS):

```c
// FUN_8003A1E4 lines 326-336 (excerpted):
uVar14 = (uint)*(byte *)(iVar11 + iVar10);    // script[PC]
if ((uVar14 - 0x24 < 2) && (... > 0x1F)) {    // op in {0x24, 0x25}
    while (true) {
        iVar10 = func_0x801de840(...);         // -> FieldVM dispatcher
        *(short *)(iVar9 + 0x9e) = (short)iVar10;
        if (uVar14 == 0x21) break;
        // walk next opcode
    }
}
```

Each actor is allocated by ``FUN_80024C88`` then its prescript runs
once through the FieldVM (``FUN_801DE840``). The prescript can write
``actor[+0x14] / actor[+0x18]`` (X / Z position), so the *resolved*
position differs from the literal MAN-record decode.

**Statically resolving these without running the FieldVM is not
covered by the asset extractor.** The MAN prescript is a per-record
bytecode that picks a position based on actor type, story-flag state,
overlay-resident lookup tables. A full clean-room port would need
the engine-vm field VM driving real actor records.

The practical alternative is a **runtime snapshot capture**:

- ``scripts/mednafen/resolve_bulk_terrain.py`` extracts the
  post-resolve placements out of mednafen save states. It walks every
  actor list head listed in `Globals used`, captures the actor's live
  ``+0x14 / +0x18`` coords plus its mesh chain at ``+0x44`` (resolved
  back to the kingdom TMD pack via reverse-magic-search), and tags
  each placement ``kind: 'bulk_terrain'`` when ``actor[+0x90]`` is
  outside the MAN buffer or ``'man_actor'`` otherwise.
- ``site/extract-world-placements.py`` merges the resulting JSON into
  ``site/world-overview.json`` under ``bulk_terrain_placements`` per
  kingdom (alongside the existing ``placements`` and
  ``live_placements`` fields). The world-overview viewer renders both
  layers in the same scene.
- ``crates/web-viewer::sentinel_placements`` is the Rust port of the
  RAM-side resolver (record parser + actor-list walker + TMD-pack
  reverse lookup) for downstream callers; the Python script is the
  end-to-end driver.

The Drake-only count produced by the existing PCSX-Redux capture
(``site/world-overview-live.json`` legacy single-bundle dict) lands
as ``man_actor`` under the new tagging since that capture script
predates the ``kind`` field.

### Per-kingdom fog colour

The atmospheric-tick actor (``actor[+0x0C] == FUN_801E3E00`` at
``0x801E3E00``) interpolates the per-kingdom haze RGB into its
``+0x74`` field per frame. That u32 is the input to ``FUN_80043390``'s
``ctc2`` writers to the GTE ``FAR_COLOR`` control regs (``$21 /
$22 / $23``):

```c
// FUN_8001ADA4 case 5 (line 861):
FUN_80043390(puVar12, piVar2[0x1d], *(undefined2 *)(piVar2 + 0x1e));
//                    ^^^^^^^^^^^^^
//                    actor[+0x74] = current fog RGB (0x00BBGGRR)

// FUN_80043390 (0x80043498..0x800434D0):
andi $s6, $a1, 0x00FF      // R from $a1 = actor[+0x74]
srl  $s5, $a1, 8           // G
andi $s5, $s5, 0x00FF
srl  $s4, $a1, 16          // B
andi $s4, $s4, 0x00FF
sll  $s6, $s6, 4           // 8-bit -> 12-bit
sll  $s5, $s5, 4
sll  $s4, $s4, 4
ctc2 $s6, $21              // FAR_COLOR.R
ctc2 $s5, $22              // FAR_COLOR.G
ctc2 $s4, $23              // FAR_COLOR.B
```

The script that drives ``actor[+0x74]`` lives in
``FUN_801E3E00`` (overlay-resident at
``ghidra/scripts/funcs/overlay_world_map_801e3e00.txt``) and reads
its R/G/B bytes from ``script[PC + 7 / +8 / +9]``. The script source
is a per-kingdom blob at ``actor[+0x94]``; the static walker that
installs it isn't fully reversed yet, so the practical capture path
is the runtime snapshot.

When ``scripts/mednafen/resolve_bulk_terrain.py`` finds an actor
with ``tick == 0x801E3E00`` and ``actor[+0x74] != 0``, it surfaces
the live RGB as ``fog_color: { r, g, b, u24 }`` per kingdom in
``site/world-overview.json``. The world-overview viewer reads that
field at priority above the hand-eyeballed ``KINGDOM_FOG_TINT``
fallback. World-map saves that don't have an active atmospheric tick
fall back to the hardcoded table.

### Ocean / coastline source — open, visualised by sampled tint

The retail ocean source mesh isn't yet pinned to a specific disc-side
asset. Survey results so far:

- **Slot 1 (TMD pack):** Every TMD in each kingdom's slot-1 pack is
  classified in ``site/world-overview/slot1_classification.toml`` -
  none of the 40 / 36 / 56 entries reads as an ocean / large-flat-
  blue plane.
- **Horizon emitter ``FUN_801D7EA0``:** emits ~670 prims per call
  with neutral-grey colour (`0x2C808080`), projected via the cos
  LUT - consistent with a sky / horizon plane, *not* the ocean.
- **Walk-view prim pool inspection:** ``mednafen-state prim-trace``
  on a walk-view save shows ~5000 textured POLY_FT4 tiles. The
  blue-dominant cluster across all three kingdoms converges on
  ``clut=0x7E80 tpage=0x001C`` (sampled CLUT colour ~``#1F2466``
  royal blue, hits 65-70 per kingdom). This is the shared ocean tile
  family that lives in VRAM persistently across kingdom loads; the
  same prim-pool family also draws in the top-view path.

The viewer doesn't draw the live prim mesh in 3D (it's screen-space
post-GTE), so until the disc-side ocean source is pinned the
world-overview viewer paints a **procedural ocean plane** at ``y=0``
in the captured ocean tint. Capture pipeline:

```
scripts/mednafen/resolve_bulk_terrain.py --bundles map01,map02,map03 \
    --json site/world-overview-live.json <mc1> <mc2> <mc3>
python3 scripts/extract-world-placements.py \
    --prot-dir extracted/PROT --out site/world-overview.json
```

``pick_ocean_color`` (in ``resolve_bulk_terrain.py``) walks every
POLY_FT4 cluster reported by ``mednafen-state prim-trace``, samples
each cluster's representative tile texel via its CLUT + tpage out of
the save's VRAM, and ranks blue-dominant clusters by
``hits × blue_dominance``. The winner's average RGB lands as
``site/world-overview.json[kingdom].ocean_color`` and drives the
viewer's ocean plane.

A true disc-side ocean source would let the viewer render real
geometry instead of the flat stand-in. Best next path:
``mednafen-state prim-trace --scan-all-ram`` against the ocean
cluster's fingerprint to find where the per-tile descriptor table
lives outside the default 139 KB / 91 KB scan windows.

### Camera anchors

Per-kingdom camera centres + zoom anchors live in two tables and a
JSON override:

- `KINGDOM_CAM` &mdash; walk-view spawn anchors (load-time map-origin
  coords from `_DAT_80089118` / `_DAT_80089120`, decoded by
  `mednafen-state world-map-camera --table <save>`). This is the
  default view when a kingdom tab is opened.
- `KINGDOM_TOPVIEW_CAM` &mdash; hardcoded fallback for the
  "lock to retail top-view" button.
- ``world-overview.json[kingdom].topview_cam`` &mdash; per-kingdom
  capture preferred over `KINGDOM_TOPVIEW_CAM` when present.
  ``resolve_bulk_terrain.py::capture_topview_cam`` writes this from
  ``mednafen-state world-map-camera`` against the user-supplied save
  state for each kingdom. The "lock to retail top-view" button reads
  this first; the values drive the world cam centre + frame the
  kingdom at its captured extent.

The captured anchor is the load-time map origin (`-_DAT_80089118` /
`-_DAT_80089120`). Top-view dev-menu captures (``DAT_801F2B94 != 0``)
would refine this with an interactively-scrolled centre + a refined
``zoom``; walk-view captures (``DAT_801F2B94 == 0``) match the spawn
anchor, which is good enough as a "lock" target since the dev-menu
top-view also enters from this anchor before user input scrolls it.
