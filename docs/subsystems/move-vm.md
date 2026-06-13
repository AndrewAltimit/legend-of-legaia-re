# Move-table opcode VM

A bytecode VM dedicated to per-actor animation, motion, and state. Distinct from the [field/event script VM](script-vm.md) and the [actor / sprite VM](actor-vm.md): the move VM lives in `SCUS_942.54`, runs on the per-actor "move buffer" set up by `FUN_800204F8`, and is invoked every frame from the actor tick. The opcode space is **71 instructions** (`0x00..0x46`); opcode `0x2F` escapes to an overlay-resident extension dispatcher (`FUN_801D362C`, **61 sub-opcodes** `0x00..0x3C`) that is loaded by many overlays (town, world-map, dialog, cutscene, ...) at the same RAM address; each overlay supplies its own JT contents.

## Three-VM picture

| VM | Driver fn | Where | Opcode count | Operand width |
|---|---|---|---|---|
| Actor / sprite VM | `FUN_801D6628` | Title-screen overlay (0971) | 13 (`docs/subsystems/actor-vm.md`) | byte stream |
| **Move VM** | `FUN_80023070` | `SCUS_942.54` | 71 (`0x00..0x46`) | u16 stream |
| Field / event VM | `FUN_801DE840` | Town overlay (0897) | 43 (`0x21..0x4F`, with gaps) + 0x5x/6x/7x default-route | byte stream |

The three are wired:
- Field VM op `0x22` `EXEC_MOVE` calls `FUN_800204F8`, which finds the move record for `move_id` and stages it into the actor at `actor[+0x48]` (buffer base) / `actor[+0x70]` (PC).
- Actor tick (`FUN_80021DF4`, per frame) and actor spawn (`FUN_80021B04`, one-shot) both call `FUN_80023070(actor)` to step the move buffer.
- Move-VM opcode `0x2F` calls `FUN_801D362C(actor, opcode_ptr)` for **overlay-defined extension opcodes**. The dispatcher exists in many overlays (town, world-map and its variants, dialog, cutscene); each overlay carries its own copy with its own JT contents.

## Function signature

```c
int FUN_80023070(int actor);
```

No PC argument: the PC is stored on the actor at `+0x70`. The function loops, executing opcodes until one of them clears the loop flag (e.g. opcode `0x08` "halt"). Each handler updates `actor[+0x70]` by writing `param_3` (the local "size in u16 units") into a register that the loop epilogue commits.

## Top-level dispatch

```c
short* op = (short*)(actor[+0x48] + actor[+0x70] * 2);   // u16-aligned PC
short  v1 = op[0];
if (v1 >= 0x47) goto epilogue;                            // out-of-range → end loop
v0 = jt[v1];                                              // JT at 0x80010778
goto v0;                                                  // computed jump
```

- Buffer base: `actor[+0x48]`. The pointer `_DAT_8007B888` (MOVE) or `_DAT_8007B840` (MOVE2) is the runtime root, but per-actor offsets are baked in at setup time by `FUN_800204F8`.
- PC: `actor[+0x70]` (i16, in **u16 units**). Each handler advances by its case-specific size.
- Jump table: `0x80010778` in `SCUS_942.54`, **71 entries × 4 bytes**.

## Actor-side state surfaced by opcodes

The move VM rewrites a wide swath of the actor struct:

| Offset | Type | Use |
|---|---|---|
| `+0x10` | u32 | Actor flag word. Bit `0x8` set by op `0x08`; `0x2`/`0x1000`/`0x10000`/`0x40000000` toggled by various opcodes. |
| `+0x14`/`+0x16`/`+0x18` | u16 | World X / Y / Z (op `0x07` absolute set, op `0x01` add, op `0x03` rotate-add). |
| `+0x22` | u16 | Y-rotation (per-frame ramp by op `0x2D`/`0x35`/`0x37`). |
| `+0x24`/`+0x26`/`+0x28` | u16 | Camera/render slots (op `0x05` add, op `0x06` write `+0x26`, op `0x39` write all three). |
| `+0x2A` | u16 | World Y mirror (kept in sync with `+0x16` for collision lookup). |
| `+0x3C`/`+0x3E`/`+0x40` | u16 | Animation bank (op `0x00`: `[v << 3]`). |
| `+0x44` | int* | Pointer to a per-actor scratch struct (used by `0x3C` and the `0x44+` family). |
| `+0x54` | u16 | Wait/timer (op `0x09` set; ticked down by `FUN_80021DF4`). |
| `+0x62` | u16 | Local flag bank - 16 bits. AND/OR by ops `0x31`/`0x32`. |
| `+0x70` | i16 | **The move-VM PC** (in u16 units). |
| `+0x74` | u32 | Composite control word (op `0x33` clears bit `0x40000000`). |
| `+0x80`/`+0x82`/`+0x84` | u16 | Animation slots, `[v << 3]` (op `0x04`). |
| `+0x90`/`+0x92`/`+0x94` | u16 | Tween source (op `0x35`/`0x37` absolute / increment). |
| `+0x96`/`+0x98`/`+0x9A` | u16 | Tween scale (op `0x2E`, `[v << 3]`). |
| `+0x9C`/`+0xA0`/`+0xA4`/`+0xA8` | i32 | Tween block (op `0x34`, 9-word setup). |
| `+0xAC..+0xCA` | mixed | Per-frame anim slots (key/curve data; op `0x2C` configures, `+0xC0` is the duration). |
| `+0xB0..` | u8 | Per-keyframe descriptor (op `0x0A` writes `count` 3-byte slots). |
| `+0xB2` | i16 | Misc (op `0x38` add). |

## Opcode reference

The cases below are paraphrased from `FUN_80023070` (`ghidra/scripts/funcs/80023070.txt`). Sizes are in **u16 units** (each opcode + operands consumes `param_3` halfwords).

### 0x00 - `ANIM_BANK_SET` (size 4)

`[u16 op, u16 v1, u16 v2, u16 v3]`. `actor[+0x3C..+0x40] = v1..v3 << 3`.

### 0x01 - `WORLD_ADD` (size 4)

`actor[+0x14] += v1; actor[+0x16] += v2; actor[+0x2A] += v2; actor[+0x18] += v3` (Y mirror in `+0x2A`).

### 0x02 - `BANK_SET_98` (size 2)

`actor[+0x98] = v1 << 3`.

### 0x03 - `WORLD_ROTATE_ADD` (size 2)

Adds rotated `v1` into world X / Z using sin/cos table at `_DAT_8007B81C` / `DAT_8007B7F8` indexed by `actor[+0x96] & 0xFFF`.

### 0x04 - `ANIM_BANK_2` (size 4)

`actor[+0x80..+0x84] = v1..v3 << 3`.

### 0x05 - `RENDER_BANK_ADD` (size 4)

`actor[+0x24..+0x28] += v1..v3`.

### 0x06 - `WRITE_26` (size 2)

`actor[+0x26] = v1`.

### 0x07 - `WORLD_SET` (size 4)

Absolute `actor[+0x14..+0x18] = v1..v3` (Y mirror in `+0x2A` matches `v2`).

### 0x08 - `HALT` (size 0, ends loop)

`actor[+0x10] |= 0x8`. Sets `bVar3 = false` so the dispatcher exits without advancing PC.

### 0x09 - `WAIT_SET` (size 2, ends loop)

`actor[+0x54] = v1 << 3` (the wait timer; `FUN_80021DF4` ticks it down each frame).

### 0x0A - `KEYFRAME_LOAD` (variable, size = `3 + count*3`)

`actor[+0x10] |= 0x1000`. `actor[+0x6C] = byte(op[2])`. Then loops `count = op[2]` writing `actor[+0xB0+i] = byte(op[3+3*i])` and two scaled curves for `+0xB8` / `+0xC8` per slot.

### 0x16 - `STUB` (size 2)

Calls `FUN_80024C80(actor, op[1])`. The body is a pure `jr ra` / nop - the opcode is a no-op. A clean-room port can implement it as PC-advance only; see `ghidra/scripts/funcs/80024c80.txt`.

### 0x2C - `KEY_BUFFER_ALLOC` (size 5)

`[op, count_x, count_y, w, h]` configures `actor[+0xA0..+0xA6] = ops[1..4]`. If `w >= 0x11` allocates `w * h * 2` bytes via `FUN_80017888` and stores at `actor[+0xA8]`; else uses the inline buffer at `actor[+0xAC]`. Then `FUN_8005842C` initialises the descriptor at `actor[+0xA0]`.

### 0x2D - `WORLD_INC_VARIANT` (size 4)

`actor[+0x90] += v1; actor[+0x92] += v2; actor[+0x94] += v3`.

### 0x2E - `TWEEN_SCALE_SET` (size 4)

`actor[+0x96] = v1 << 3; actor[+0x98] = v2 << 3; actor[+0x9A] = v3 << 3`.

### 0x2F - `OVERLAY_EXT` (size = handler return)

```c
param_3 = func_0x801d362c(actor, op);
```

**Escape to overlay-defined extension opcodes.** `FUN_801D362C` reads `op[1]` as a 16-bit sub-opcode (range `0x00..0x3C`) and dispatches via its own JT at `0x801CE868` (61 entries × 4 bytes), bounds-checked so there is no out-of-bounds-jump path. **61/61 sub-ops are dispatched in `crates/engine-vm`.**

The full sub-op reference — bounds check, the shared `&DAT_801F3498` scratch table, world-position lerps, bbox/distance gates, self-modifying bytecode ops, HSV color ramps, the `DAT_80085758` fourth flag bank, and per-sub-op coverage — is in **[move-vm-overlay-ext.md](move-vm-overlay-ext.md)**.

### 0x30 - `KEY_BUFFER_FREE` (size 0, ends loop / falls into 0x22)

If `actor[+0xA8]` was heap-allocated, calls `FUN_800583C8(actor + 0xA0, buf)`; else uses the inline buffer at `actor + 0xAC`. Clears `actor[+0x9C]`. Goto `caseD_22` epilogue.

### 0x31 - `LFLAG_AND` (size 2)

`actor[+0x62] &= v1`. Per-actor local flag bank (16 bits).

### 0x32 - `LFLAG_OR` (size 2)

`actor[+0x62] |= v1`.

### 0x33 - `CLEAR_BIT_40000000` (size 1)

`actor[+0x74] &= ~0x40000000`.

### 0x34 - `TWEEN_SETUP` (size 9)

Loads 8 i16 operands into `actor[+0xAC..+0xC8]` (with `+0xAC`/`+0x9C..0xA8` zero-extended to i32). 9-u16 instruction.

### 0x35 - `WORLD_INC_VARIANT2` (size 3)

`actor[+0x90] += v1; actor[+0x92] += v2`.

### 0x36 - `TWEEN_DURATION_SET` (size 3)

`actor[+0x98] = v1 << 3; actor[+0x9A] = v2 << 3; actor[+0xB8] = 0`.

### 0x37 - `WORLD_SET_VARIANT2` (size 3)

`actor[+0x90] = v1; actor[+0x92] = v2`.

### 0x38 - `B2_ADD` (size 2)

`actor[+0xB2] += v1`.

### 0x39 - `RENDER_BANK_SET` (size 4)

Absolute `actor[+0x24..+0x28] = v1..v3`.

### 0x3A / 0x3B - `FLAG_2_SET` / `FLAG_2_CLEAR` (size 1)

`actor[+0x10] |= 2` / `actor[+0x10] &= ~2`.

### 0x3C - `SCRATCH_WRITE` (size ?)

Writes `(int)(short)v1` into `*actor[+0x44]` (the indirect scratch slot).

### 0x40 - `MOVE_IMAGE` (size 7)

```c
RECT r = { op[1], op[2], op[3], op[4] };   // source x, y, w, h (VRAM halfwords)
MoveImage(&r, (short)op[5], (short)op[6]); // FUN_80058490 - dest x, y
```

A literal-operand VRAM-to-VRAM copy through the libgpu `MoveImage` wrapper.
This is the engine's **animated-texture strip** primitive: the frames are
authored inside the scene/character texture uploads (parked in VRAM next to
the live rect), and a move program cycles them by stamping one frame per
`0x40` instruction over the displayed texel rect. Live-traced via an exec
breakpoint on `FUN_80058490` (`scripts/pcsx-redux/autorun_battle_moveimage_trace.lua`):
field scenes run 4-frame strip cycles (e.g. 16×64 strips at one-frame
cadence) from this op. (The battle party's facial-texel stamps share the
`MoveImage` primitive but are NOT this op — they come from the dedicated
facial animator `FUN_8004C7B4`; see
[`battle-data-pack.md` § Facial animation tracks](../formats/battle-data-pack.md#facial-animation-tracks-entry-0x8c--0x98).)
Engine hook: `MoveVmHost::move_image` (`crates/engine-vm/src/move_vm.rs`).

(Cases beyond `0x3C` not listed here are documented in the function dump and follow the same pattern; consult `ghidra/scripts/funcs/80023070.txt` for the exhaustive list.)

## Control flow

The interpreter loops: read opcode at `actor[+0x70]`, dispatch, write the new PC `actor[+0x70] += param_3`, then either continue or break depending on `bVar3`. Break opcodes:

- `0x08` (HALT): clears the loop flag.
- `0x09` (WAIT_SET): wait timer is set; further opcodes deferred to next frame.
- A few epilogue cases (e.g. `0x30`) jump to the epilogue without advancing.

After the loop exits, the function returns; the caller (`FUN_80021B04` or `FUN_80021DF4`) gets a "tick complete for this actor" signal. The next frame's `FUN_80021DF4` updates physics, then re-enters `FUN_80023070` from the saved PC.

## Summon part interpolation (P3)

> **Reconciliation:** the summon scene-graph driver below is the engine's
> **stand-in** render for a Seru-magic cast, not retail's player-summon path. A
> live trace resolved that the **player** summon is drawn as an ordinary battle
> actor via the per-object TRS-keyframe decoder `FUN_8004998C` (ported in
> `engine-vm/anim_vm.rs`), with the move VM firing only as noise — see
> [`battle-action.md`](battle-action.md#seru-magic-summon-overlay-dispatch). The
> move-VM stager records (extraction PROT 903..913) are real on-disc data and this driver
> runs them faithfully opcode-for-opcode, but they aren't the player render
> path.
>
> **Provenance correction (`FUN_801F811C`):** a full static decode of PROT 0900
> at the slot-B link base `0x801F69D8` resolves `FUN_801F811C` as the per-frame
> handler of the 2D **screen-mask (iris) widget** — kind 1 of the
> [screen-effect widget family](#screen-effect-widget-family-prot-0900) below —
> **not** a summon-part position update. Its four tweened channels
> (`+0x3c/3e/40/42` targets vs `+0x14/16/18/1a` current) are the left / top /
> right / bottom edges of a screen rectangle, and the "4 render quads" are the
> black border bands framing that rectangle. The engine's
> `summon::apply_translation_update` keeps the tween *shape* as an interpreted
> per-part translation glide (documented as such in `summon.rs`); the faithful
> port of the retail function is `engine-core::screen_fx::MaskWidget`.

The summon scene-graph driver (`crates/engine-core/src/summon.rs`) ticks each
part through this move VM, then applies an interpreted render-side translation
glide shaped like the `FUN_801F811C` tween: snap to `origin + anim banks` when
no tween is active (`+0x9E == 0`); otherwise advance `+0x9C += frame_delta`
(clamp to `+0x9E`), lerp each axis with the `FUN_801DE4C8` mode-1 arm, and
latch exactly on `+0x9C == +0x9E` (clearing `+0x9E`). The engine models anim
banks as summon-local offsets, so `summon::SummonScene` adds the cast-target
`origin` to each axis's endpoint.

## Screen-effect widget family (PROT 0900)

The resident slot-B overlay PROT 0900 (link base `0x801F69D8`) hosts a
four-kind family of 2D screen widgets — the cutscene-style presentation layer
(iris mask, scripted sprites, image panel, letterbox bands). Engine port:
`crates/engine-core/src/screen_fx.rs`; layout pinned on disc bytes by the
disc-gated `screen_fx_disc` test.

Widgets are actors on the generic effect-actor list (`_DAT_8007C34C`).
SCUS `FUN_80020DE0(descriptor, list)` allocates one and binds the per-frame
handler from `descriptor+8` at `actor+0xc`; `FUN_8003CF04(list, handler)` finds
a live widget by handler. The four 0x18-byte handler-binding descriptors sit at
`0x801F8FE4/8FFC/9014/902C` (`[u32 0][u16 0][u16 0xFFFF][u32 handler][u32 0]…`):

| kind | handler | per-frame behaviour | spawn / control API |
|---|---|---|---|
| sprite | `FUN_801F7A9C` | widget-script-driven tweened 2D sprite: GP0 `0x64` SPRT (pos `+0x14/16`, size `+0xa8/aa`, UV `+0xa4/a6`, CLUT `+0xa2`, RGB `+0x74`), texpage packet from `+0xa0`, OT `+0xc` | `FUN_801F8004(record)` — record `[x][y][w][h][tex_x][tex_y][clut_x][clut_y]` i16s + `rgb` u24, script at `+0x13`; derives `texpage = (tex_x>>6) + ((tex_y & ~0xff)>>4)`, `u = (tex_x & 0x3f)<<2`, `v = tex_y & 0xff`, `clut = (clut_y<<6) + (clut_x>>4)` |
| mask | `FUN_801F811C` | 4-edge rect tween + **4 black border quads** (GP0 `0x28`, colour 0, OT `+0x1c`): top `(x0,0)-(0x140,T)`, bottom `(x0,B)-(0x140,H-1)`, left `(x0,T)-(L-1,B)`, right `(R,T)-(0x140,B)` — `x0`/`H` from render scratch `0x1F800388`/`0x1F80038E` | `FUN_801F8D4C(l,t,r,b,dur)` — `-1` per edge selects the full-open default; fresh spawn starts fully open `[x0, 0, 0x140, H-1]` |
| panel | `FUN_801F849C` | **five**-channel tween (x, y, w, h, first-page width `+0x24↔+0x26`) + 1–2 textured quads (GP0 `0x2C`, colour `0x888888`, OT `+0x10`) over **15bpp** texpages (spawn ORs `0x100` into the page selector — no CLUT); a panel wider than 256px splits across two pages | `FUN_801F88FC(rec)` spawn (`[x][y][w][h][tex_x][tex_y]` from operand `+1`; `w > 0x100` computes the second page + clamps the first-page width); `FUN_801F8E6C(x, y, scale, dur)` move/scale — `scale` is 4.12 fixed against the `+0xb8/ba/bc` base sizes |
| letterbox | `FUN_801F8A34` | no tween: two solid black bands (`-y_off..y0`, `y3..H`) + two gradient feather strips (`y0`→`y1` white→black, `y2`→`y3` black→white; GP0 `0x3B` shaded semi-transparent behind a **subtractive**-blend draw-mode packet `FUN_80059010(…, 0x55, …)`), OT `+0x4` | `FUN_801F8F28(block)` — six i16s `[x_left][x_right][y0][y1][y2][y3]` |

The **sprite widget script** (cursor at `actor+0x90`) is byte-coded: opcode
`0x40`, sub-op at `+2` dispatched through the 5-entry table at the overlay head
(`0x801F7B14/7B28/7B54/7B8C/7D90`; the same table the overlay-resident
dispatcher `FUN_801F2D68` consumes via `jr *(0x801F69D8 + sub*4)`):

| sub | operands | semantics |
|---|---|---|
| 0 | — | kill: set actor flag bit 8 (suppresses the draw; `FUN_8003CF04` skips it) |
| 1 | `flag:i16@3` | wait until story flag set (`FUN_8003CE64`, bank `0x80085758`); then `cursor += 5` and continue same-frame |
| 2 | `flag:i16@3` | wait until story flag **clear**; then `cursor += 5` |
| 3 | `x:i16@3, y:i16@5, rgb:u24@7, mode:u8@0xA, dur:i16@0xB` | tween position + colour; `cursor += 0xD` on completion |
| 4 | `rgb:u24@3, mode:u8@6, dur:i16@7` | tween colour only; `cursor += 9` on completion |

All tweens share `FUN_801DE4C8(a, b, t, D, mode)` — `if (a == b || D <= t)
return a;` mode 1 = linear `(a-b)*t/D + b`, mode 2 = quadratic ease-out,
mode 3 = quadratic ease-in, mode 4 = two-segment ease-in-out (integer
truncating division throughout; `overlay_dance_801de4c8.txt`; ported as
`screen_fx::interp`, all four modes). Results store via the sized-store
`FUN_801DE648(value, *dst, size)` (`overlay_baka_fighter_801de648.txt`).
Crucially, a tween **re-interpolates from a captured start value each frame**
(mask: the latched `+0x14..` edges; sprite: the `+0x3c/3e` / `+0x7c` start
slots written when `+0x9C == 0`) — not iteratively from the moving current
value — and latches exactly on `+0x9C == +0x9E`.

**Consumers.** The spawn/control APIs are called by **field-VM op `0x43`
sub-ops `0x10`/`0x11`/`0x13`/`0x14`/`0x15`**
([script-vm.md § 0x43 sub-0x10..0x15](script-vm.md)), dispatched through the
0x43 sub-op JT at `0x801CEDA8` — `jal` sites inside `FUN_801DE840` at
`0x801DF918` (sub-`0x10` sprite, inline record), `0x801DF974` (sub-`0x11`
mask, operands `[L][T][R][B][dur]` i16s), `0x801DFA70` (sub-`0x13` panel),
`0x801DFABC` (sub-`0x14` panel move/scale), `0x801DFACC` (sub-`0x15`
letterbox); on disc only the eight ending-sequence scenes' cutscene-timeline
(partition-2) scripts invoke them. The earlier reading that the summon stagers 0910..0915 reference
these handlers was **VA aliasing**: those hits are in-file `FUN_80021B04` part
records whose addresses coincide with the handler VAs under the shared slot-B
base. PROT 0900 file `0x0640..0x2660` (the whole family) is byte-resident at
`0x801F7018..0x801F9038` in the fingerprinted `battle_gimard_tail_fire_a`
save, and the function bodies are byte-identical to the dance / baka-fighter
overlay images (`overlay_dance_801f811c.txt` etc.).

**Battle (enemy "Fire Tail") does NOT drive the widget path.** PROT 0900 is the
slot-B occupant in the enemy Gimard "Fire Tail" mid-cast frames (loader-B id 5;
byte-exact at the residency pin file `0x1628` ↔ `0x801F8000`), but the
screen-widget path is dormant there: an effect-actor-list walk of both
catalogued frames (`battle_gimard_tail_fire_a/_b`) finds **zero** live
mask/sprite/panel/letterbox actors. The Fire Tail's live effect is instead a
single **move-VM part-actor** in the part pool `DAT_801C90F0`, ticked each frame
by the generic SCUS actor tick `FUN_80021DF4` (→ `FUN_80023070`); its
`[i16 model_sel][u16 flags][bytecode]` record sits in the **battle overlay
(0898)** resident data at `0x801F5xxx` (below the 0900 slot-B link base), with
`model_sel` reading `-1` (transform node) / `5` (library mesh) — the summon
part-record format, sourced from the battle overlay rather than a per-spell
stager. So the widget family stays **ending-scene-exclusive**, and `FUN_80021DF4`
is pinned as the live part render-tail. Disc + library gated
`firetail_movefx_liveness` (crate `legaia-mednafen`).

## Connection to other crates

- **`crates/mdt`** - parses the [MDT format](../formats/mdt.md). The per-frame data inside an MDT record is exactly the move-VM bytecode this VM consumes. With the move-VM opcode set documented, `crates/mdt` can grow a disassembler.
- **`crates/engine-vm`** - clean-room Rust port lives in `move_vm.rs`. The dispatcher (`step` / `run_until_break`), every opcode handler, and the `MoveHost` callback trait live there. Per-frame entry is `move_vm::actor_tick`, which mirrors the gate at `FUN_80021DF4 + 0x80022B94..0x80022BBC`: skip when `wait_timer >= 0`, otherwise step the VM and report `Halted` if the post-call `flags & 0x8` bit is set. `decrement_wait_timer` is the matching pre-tick helper.
- **Field VM op `0x22`** (`EXEC_MOVE`) - the gateway from script-VM into move-VM; calls `FUN_800204F8` to set up the per-actor buffer.

## Decompile quirks worth knowing

- **Operand units are u16**, not bytes. `op[1]` is the 16-bit operand at offset 2 from the opcode word.
- **PC is also in u16 units.** `actor[+0x70]` × 2 is the byte offset.
- **`param_3` is the size in u16 units**, not bytes. The dispatcher epilogue does `actor[+0x70] += param_3`.
- **0x47-bound check**: `sltiu v0, v1, 0x47`. Out-of-range opcodes silently fall through to the loop-exit, the same shape as the field-VM `default` arm. Treat any opcode `>= 0x47` as "end of move buffer" rather than "unknown opcode".
- **Cases that "look like NOPs" in the C decompile** still advance the PC via `param_3` - the increment is set in MIPS branch-delay slots and is invisible at the C level (same pattern as the field VM, see `script-vm.md` § "Decompile quirks").

## See also

**Reference** —
[Move table (MDT)](../formats/mdt.md) ·
[Motion VM](motion-vm.md) ·
[Battle action SM](battle-action.md) ·
[Actor VM](actor-vm.md)
