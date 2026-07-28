# Key functions: renderer, GPU + dialog text

Part of the [key function directory](../functions.md) - the conventions for reading these tables (bare hex = function entry, `0x`-prefixed = data / instruction, overlay-VA caveats) are on the [index page](../functions.md#how-to-use-this-page).

## Renderer

| Address | Role |
|---|---|
| `8002735C` | Legaia TMD renderer. 60 GTE ops; per-mode descriptor table at `DAT_8007326C`. Reached as the **landmark** emit leaf via `FUN_8001ADA4` case-5 - each landmark TMD in a kingdom slot-1 pack passes through here. The bulk world-map continent does **not** flow through this path; it flows through `FUN_80043390`'s per-prim dispatcher (textured-TMD default for case-5), which mode-switches to overlay-resident fog leaves when the world-map overlay is paged in. Cmd byte read from `DAT_8007326C`, so static `addprim` hunters miss both. |
| `80029888` | **Light-source TMD renderer** - the sibling of `FUN_8002735C` over the same `DAT_8007326C` table, and the only other function in `SCUS_942.54` that reads it. Handles the lit descriptor rows 0/1 (their quad chain has no `+2`). Despite the name it runs **no** GTE light op: its only colour op is `DPCS` (`cop2 0x780010`, the depth cue), whose far colour it stages from `param_2` (each byte `<< 4` into cr21-23) and whose `IR0` it takes from `param_3`. Field shading is baked in the prim colour word and applied by the GPU (`texel * colour / 128`). Reached via `FUN_8001ADA4` case 5. See [`subsystems/renderer.md`](../../subsystems/renderer.md#lighting). `see ghidra/scripts/funcs/80029888.txt`. |
| `8005B648` | PsyQ `SetLightMatrix` - writes the GTE light matrix `L` into control registers cr8-12 (`ctc2 .., 0x4000/0x4800/0x5000/0x5800/0x6000`). The matrix is populated on the field path but never consumed there (no `NC*` op runs); its only readers are the world-map slot-4 mesh handlers. |
| `8005B678` | PsyQ `SetColorMatrix` - writes the GTE light-colour matrix `LC` into control registers cr16-20 (`ctc2 .., 0x8000/0x8800/0x9000/0x9800/0xA000`). Same story as `FUN_8005B648`: consumed only by the `NCCS`/`NCCT` sites in the world-map slot-4 handlers (`FUN_8004409C` / `FUN_8004423C` / `FUN_80044434` / `FUN_800445B0`) - the only GTE light ops in the game. |
| `8001ADA4` | Per-actor RENDER dispatcher (2456 bytes). Switch on `actor[+0x56]` (render mode 1..0xB). Case 4 (multi-target): dispatches on `actor[+0x9e]` flags - bit `0x4000` → `FUN_8002A5A4`, bit `0x2000` → `FUN_801CFA48` (overlay-resident), else → `FUN_80028158`. Case 5 (full TMD): iterates the mesh chain at `actor[+0x44]` and calls `FUN_80043390` (textured) / `FUN_80029888` (env-mapped, `actor[+0x7a] != 0`) / `FUN_8002735C` (bone-animated TMD). Case `0xB` = the ocean CLUT-walk emitter: `acc += DAT_1F800393` into `actor[+0x68]`; on `acc >= hold` fires the 16x1 `MoveImage` for the actor's kingdom-slot-5 table entry, resets `acc = 0`, advances the frame index (`legaia_asset::clut_walk`). Called 6x per frame via the `FUN_8001D140` wrapper against the same actor lists as the tick pass. |
| `80017EC8` | **Tile-window background render pass.** `(cx, cz, sx, sz)`. Sets the scroll-origin globals `_DAT_1F8003F8 = sx` / `_DAT_1F8003FA = sz`, refreshes the per-tile region-attribute mask via `FUN_800180EC`, then walks a tile window centred on `(cx, cz)` (`cx - 0xF + i`, `cz - 0xD + j`) emitting one per-cell background draw each. The engine renders field/board backgrounds through its own wgpu path, so this per-frame GP0 tile emission is not ported. `see ghidra/scripts/funcs/80017ec8.txt`. |
| `8001D140` | Tiny stack-swap wrapper (`_DAT_1F8002BC = scratch; jal FUN_8001ADA4`). Called 6x per frame from `FUN_80016444` against `_DAT_8007C34C..0x36C` - the render-pass counterpart to the tick-pass `FUN_8002519C`. |
| `8002519C` | Per-frame actor-list TICK iterator (328 bytes). Walks the linked list, calls `actor[+0x0c]` (tick fn). Called 5x per frame from `FUN_80016444` against actor lists at `_DAT_8007C34C..0x36C` (different render passes). Distinct from `FUN_8001D140` (render pass). |
| `8002C69C` | HUD / dialog / menu sprite-batch emitter. 10 `cmd=0x2C` (POLY_FT4) lui/li sites in SCUS - the most prolific addprim emitter on a static scan. All callers pass small counts (`a3 = 0xb..0x44` = 11..68 prims each); total across all world-map call sites is ~120 prims. UI text rows, dialog frames, dev-menu strips. NOT the bulk continent emitter. |
| `800460AC` | GTE billboard fan helper. Loads 3 vertices via SVTX0/1/2 with-`(X-0x20, Y, Z), (X, Y, Z), (X+0x20, Y, Z)`, runs RTPT (cop opcode `0x280030`) 3 iterations decreasing Z, stores SXY/SZ at scratchpad `0x1F8002FC..`. Stage decoration / billboard sprite projection. |
| `80059BD4` | VRAM image/CLUT upload (LoadImage-equivalent) - [details ↓](#80059bd4) |
| `800198E0` | Per-TIM VRAM uploader + texpage→CLUT-row recorder - [details ↓](#800198e0) |
| `0x8007BEC0` (data) | **Texpage→CLUT-row table** (32 × `u16`). Written by `FUN_800198E0` at each textured upload (`[texpage & 0x1f] = clut_row`); read by the battle render path (`FUN_8004AD80` / `FUN_8004CE2C`) to resolve a primitive's CLUT **row** by its texpage. The TMD2's stored CBA supplies the CLUT **x** (sub-CLUT) but the **row** comes from here - the mechanism behind the battle-form party palettes appearing at scene-specific rows rather than the disc-nominal 490..495. |
| `800583C8` | **PsyQ `LoadImage(RECT *rect, u_long *data)`** (dev string `"LoadImage"`). Enqueues a CPU→VRAM transfer: calls the GPU-queue add (`FUN_8005A1C0`) with `handler = table[8] = FUN_80059BD4`, the rect, and the source data. Sibling wrappers in the same cluster are `ClearImage` (`FUN_80058298`/`8005832C`), `StoreImage` (`FUN_8005842C`), `MoveImage` (`FUN_80058490`), `DrawOTag` (`FUN_80058704`), `PutDrawEnv` (`FUN_80058778`), `DrawOTagEnv` (`FUN_8005887C`) - Legaia's statically-linked PsyQ libgpu. |
| `8005A1C0` | **GPU command-queue enqueue** (PsyQ-style). `FUN_8005A1C0(handler, rect/data, inline_size, src)`: when the ring has room, writes ring entry at `0x801C9590 + tail*0x60` = `{[+0]=handler, [+4]=rect (or inline copy of `inline_size` bytes), [+8]=src}`, bumps tail `0x80078E58`, and kicks `FUN_8005A4A0`. The flusher dispatches `entry[+0](../entry[+4], entry[+8])`. Head `0x80078E5C` / tail `0x80078E58`; reset by `FUN_8005A78C` / `FUN_8005AA64`. |
| `80078D0C` (data) | **GPU-op handler dispatch table** (op-type → handler fn-ptr; 18 entries). Index 8 = `FUN_80059BD4` (image/CLUT upload), 9 = `FUN_8005A4A0`. `0x80078D4C` holds a pointer back to the table base (the live table ref the enqueuer/flusher dereference). The enqueuer (`FUN_80057C44` materialises the base; `FUN_800589D0` etc. read `0x80078D4C`) looks up `handler = table[op_type]` and writes `{handler, rect, src}` into the upload ring. So GPU uploads are queued **by op-type**, not by passing the handler - which is why no LUI+ADDIU site materialises `0x80059BD4`. |
| `8005A4A0` | GPU upload-**queue flusher** (748 B) - [details ↓](#8005a4a0) |
| `0x8007326C` (data) | Per-prim-mode descriptor table. 6 entries × 8 bytes - see [`formats/tmd.md`](../../formats/tmd.md). |
| `0x8007C018` (data) | Global TMD pointer table - [details ↓](#0x8007c018-data) |
| `80026B4C` | Per-TMD installer. Verifies TMD magic `0x80000002`, stores `tmd_ptr` at `DAT_8007C018[DAT_8007B774++]`, then calls `FUN_800268DC` (builds the `+0xC` group descriptors). Reached from `FUN_8001F05C` case 2 (TMD-pack) and case 9 (TMD2). 35 instructions; tiny. |
| `801F69D8` | World-map top-view tile-visibility dispatcher (in `overlay_world_map_top_ext`). 643 instr / 2572 B. Bulk-copies camera struct from `0x8007BF10` into scratchpad, nested-loops over visible tile cells in scratchpad table `_DAT_1F8003EC + 0x8000`, dereferences each 0x20-byte object record, applies frustum + GTE RTPT, then routes the TMD via `DAT_8007C018[(object_kind8 + DAT_8007B6F8)*4]` and calls `FUN_80043390(tmd+0xC, color, fog)`. Color = `0xD0D0D0` default / `0x40D0D0D0` if interactive / OR `0x10000000` if extra flag. Fog = `clamp((GTE_z - 0x5000) >> 3, 0, 0x1000)`. The warp-transition cluster-A caller (capture-pinned: Drake Read-bp's `ra = 0x801F725C`). |
| `801D8280` | `DAT_8007C018` table walker (overlay-resident, in every world-map / cutscene-mapview / 0897 overlay variant). Iterates entries `0..DAT_8007BB38` and for each pointed-to TMD calls `FUN_801D5E20` on each 0x1C-byte sub-record. 55 instr. |
| `801D77F4` | Overlay-resident actor allocator (alt to `FUN_80021B04`) - [details ↓](#801d77f4) |
| `80021B04` | SCUS-resident actor-spawn helper. Looks up `DAT_8007C018[actor[+0x64].i16]`, copies position/rotation into actor fields, populates per-actor OBJECT pointer table at `actor[+0x44]` (`[0] = tmd_group_count`, `[1..n] = sub-record pointers at stride 0x1C`). Then calls `FUN_80023070` (move-VM entry) and `FUN_8003D344` (5-op GTE transform). |
| `80024D78` | Per-actor OBJECT-table rebuild. Fills `actor[+0x44]` straight off the pool TMD at `DAT_8007C018[actor[+0x64]]`: `[0] = *(tmd+8)` (the live group count - post-cap 10 for the party meshes), `[1..n]` = group descriptor pointers at `tmd + 0xC + i*0x1C`. `FUN_8001B964` requires `[0] ==` the anim record's bone count before drawing, making the bone→object mapping strictly one-to-one. |
| `8001B964` | **Per-actor animated character-mesh renderer.** `(actor)`. Builds the actor's GTE transform from its record (Euler→matrix `FUN_80026988` over `actor+0x24`), selects the camera-relative rotation path (`FUN_8001CF50` when `actor+0x52 & 0x780`, else the plain matrix-vector `FUN_8005B3A8`), then walks the bones decoding each per-(bone, frame) entry via `FUN_8001BE80` and emits the assembled skinned mesh to the OT. The runtime consumer of the `legaia_asset::player_anm` layout; the clean-room engine reproduces it via wgpu + the ported `BoneTransform::decode` (`FUN_8001BE80`). `see ghidra/scripts/funcs/8001b964.txt`. |
| `8001C204` | **World-map object-effect GTE transform builder.** `(actor)`. Composes the scratchpad working transform at `0x1F800314` for object-effect kind `*(actor+0x42) - 1`: a per-type param table at `0x80083FF8` (stride `0x14`; rotation angles at `+0/+2/+4`, extra u16s copied to `0x1F800380/382` from `+0x10/+0x12`) is rotated (`FUN_800461A4/629C/638C` = per-axis RotMatrix) and composed with the actor's own `+0x24` Euler and `+0x14` position through the GTE matrix ops `FUN_8003D20C/D1A4/D344`; the resulting translation lands at `0x1F800328/32C/330`. Reached from the world-map object dispatch (see [`world-map.md`](../../subsystems/world-map.md)). Render-track (GTE); unported (engine transforms via wgpu). `see ghidra/scripts/funcs/8001c204.txt`. |
| `8001C394` | **Object drop-shadow / floor-decal emitter.** `(actor)`. Emits a 2x2 grid of gouraud quads (packet tag `0x09000000`, base colour `0x2E808080`) projected through the GTE from the actor position (`FUN_800460AC(actor+0x14)`), depth-sorted into the scratchpad OT `_DAT_1F8003F4` by `((sum_z + 0xA0) >> 4) >> (DAT_1F8003A4 & 0x1F)` - and into the *behind* bucket (`-0x28`) when `*(actor+0x10) & 0x800000`. Adds a per-vertex depth-cue offset from the fog LUT `_DAT_8007BB04` when `_DAT_1F800394 & 1`. The "general graphics-library" object-decal drawer (see [`playthrough-coverage.md`](../../tooling/playthrough-coverage.md)). Render-track (GPU packet); unported (engine rasterises via wgpu). `see ghidra/scripts/funcs/8001c394.txt`. |
| `80024E08` | **Set-model primitive** for script-driven (non-`.MAP`-grid) actors. `(actor, model_idx)`. Writes `actor+0x64 = model_idx` (the index `FUN_80021B04` later resolves through `DAT_8007C018`), clears `actor+0x5C`, clears draw-flag bit `0x1000` at `actor+0x10`, mirrors the model to `actor+0x60` when game mode `_DAT_8007B83C == 0xF`, then re-stages the actor via `FUN_80020F88`. See [`subsystems/world-map.md`](../../subsystems/world-map.md). `see ghidra/scripts/funcs/80024e08.txt`. |
| `80020F88` | Actor **render binding + render-node allocation** - resolves the mesh-pool index off the `.MAP` placement record and allocates the `0x9C`-byte node the draw path walks - [details ↓](#80020f88) |
| `800480D8` | Per-actor **battle draw tick** - sequences tint / trail / draw, and stamps the defeated-monster grey - [details ↓](#800480d8) |
| `80031D00` | Per-frame text-actor tick. Walks the actor list at `gp[+0x148]` and dispatches on `actor[+0x1C]`: cases 0/1/D/11 render text via `FUN_80036888`/`FUN_8003CC98`; cases 4/6/C/21 hand off to sub-routines. The per-frame driver behind dialog/labels. |
| `8001EBEC` | Equipment-conditional per-character TMD group-descriptor patch (the OBJECT 10/11 pose swap) - [details ↓](#8001ebec) |
| `8001E890` | "DATA_FIELD player loader" - loads `data\field\player.lzs` via the disc index `0x36C` r... - [details ↓](#8001e890) |
| `8003E8A8` | PROT-by-index size lookup. Reads `start_lba = PROT_TOC[p+2]` and `next_lba = PROT_TOC[p+3]` (TOC base `0x801C70F0`; see [`prot.md`](../../formats/prot.md)) and returns `next_lba - start_lba` (LBA count for the entry). Also stows `start_lba` at `gp[+0x8F0]` and the entry index at `gp[+0x90C]` so the matching `FUN_8003E800` read can pick them up. |
| `8003E800` | Issues the actual sector read scheduled by `FUN_8003E8A8`. `param_1` = destination buffer, `param_2` = LBA count, `param_3` = flag bits (`& 1` enables the libcd request via `FUN_8003F128`; `& 2` blocks on completion). The pair `FUN_8003E8A8` + `FUN_8003E800` is wrapped by `FUN_8003EB98(prot_idx, dst, 1)` for one-shot PROT-by-index loads. |
| `8001ED60` | Load + checksum PROT entry `0x36C` (party field data). Allocates a 256 KB buffer, then dev-path (`FUN_8003E6BC` named-file open) or retail-path (`FUN_8003E8A8`/`FUN_8003E800` async PROT read) loads the entry and word-sums it into the checksum cells `gp+0x6B8` / `gp+0x690`; sizes rounded up to a word land at `gp+0x6C8` / `gp+0x69C`. (Ghidra aliases this entry as `8001ED9C`, an interior branch delay slot.) `see ghidra/scripts/funcs/8001ed9c.txt`. |

## Renderer / GPU primitives

| Address | Role |
|---|---|
| `80024EE4` | Push textured-quad GPU primitive onto the OT chain. `(layer, depth, color)` - writes a 6-word PSX GP0 packet (`0x05000000` length + `0x2B` polygon-with-tex command + four corner verts at `_DAT_1F80038C/0x18E` × `0xFFFC`) at `_DAT_1F8003A0`, then linkPrim via `FUN_8003D2C4`. Used by `FUN_800196A4` for the screen-fade / dim overlay. |
| `80024E80` | **Screen-fade primitive spawn.** `(fade_template, mode: u16)`. Allocates an actor from pool `&DAT_80070674` via the actor allocator `FUN_80020DE0` (free-list `_DAT_8007C34C`); on success stores `mode` at `actor[+0x18]` and calls `FUN_80020B00(actor + 0x7C, fade_template)` to load the fade state (start RGB and frame count copied `<< 6`; per-frame RGB deltas = `((end - start) << 6) / duration`). Returns the new actor, or 0 when the pool is exhausted. The battle-action SM stages the summon backdrop fade (state `0x33`) and the successful-escape white-out (state `0x66`) through this (`func_0x80024E80(&DAT_801C9070, …)`). Port `engine-core::fade::spawn_fade`. `see ghidra/scripts/funcs/80024e80.txt`. |
| `80020B00` | **Fade-state loader.** `(i16* state, i16* template)`. Converts a 13-`i16` fade template into the pool actor's `+0x7C` ramp state in 10.6 fixed point: `state[0..2] = start_rgb << 6` (current), `state[4..6] = end_rgb << 6`, `state[8..10] = ((end − start) × 0x40) / duration` (per-frame delta), duration + three mode words copied verbatim. The displayed colour each frame is `current >> 6`, landing exactly on `end` after `duration` frames. Ported as `legaia_engine_core::fade::FadeState::load`; the escape template (kind 2, `0x40` frames, black → white) is pinned from the SM's case-`0x66` write. `see ghidra/scripts/funcs/80020b00.txt`. |
| `801DE478` | **Field-overlay fade-actor spawn.** `(mode)`. The overlay-resident sibling of the SCUS pair `80024E80` / `80020B00`, and a *different* family: it allocates from the overlay template `&DAT_801F2810` (not `&DAT_80070674`) through the same allocator `FUN_80020DE0(template, _DAT_8007C34C)`, then forces `mode = 1` when the field/dual-mode gate `_DAT_8007B868` is non-zero, and stores the resulting `mode` as a halfword at `actor[+0x54]`. Seven independent RAM captures (baka-fighter / dance / debug-menu / fishing / slot-machine / both cutscene overlays) dump the same 20-instruction body here. `see ghidra/scripts/funcs/overlay_baka_fighter_801de478.txt`. |
| `801DDC20` | **Field-overlay fade-actor RGB ramp tick.** `(actor)`. Per-frame body for the actors `801DE478` spawns: lerps a colour triple over a duration and pushes the packed result to the fade-quad emitter `FUN_80024EE4` - [details ↓](#801ddc20) |
| `80020C14` | **SCUS fade-actor ramp step.** `(actor) -> packed RGB or -1`. The per-frame arithmetic over the `+0x7C` block `FUN_80020B00` loads: counts the start delay `+0x1C`, the duration `+0x20` and the hold `+0x1E` down by the scratchpad vsync delta `DAT_1F800393`, accumulates `current += delta * dt` per channel with a clamp onto the target on overshoot and onto `[0, 0x3FC0]` either way, and packs `R | G<<8 | B<<16` with each channel `>> 6`. Returns `-1` while still delaying and once the hold expires; duration expiry raises `actor[+0x62] \| 0x100`, hold expiry raises `actor[+0x10] \| 8`. [Details ↓](#80020c14--80025000). `see ghidra/scripts/funcs/80020c14.txt`. |
| `80025000` | **SCUS fade-actor tick** - the pool actor's `+0x0C` handler, and the SCUS counterpart of `801DDC20`. Steps `FUN_80020C14` and, unless it returned `-1`, pushes the quad through `FUN_80024EE4(block[+0x22], block[+0x18], rgb)` - i.e. the id `FUN_80024E80` stamped into the template's last word, and the fade kind. Its address is the `+0x08` tick word of two [static actor templates](runtime-libs.md#static-actor-templates) - the records at `0x80070674` (the one `FUN_80024E80` spawns from) and `0x800706A4` (which no site materialises). [Details ↓](#80020c14--80025000). `see ghidra/scripts/funcs/80025000.txt`. |
| `8002BC38` | Gradient sprite emitter - `(x, y, uv_rect, flags)` builds one `0x34`-byte `POLY_GT4` in the scratchpad prim arena `*0x1F8003A0`: GP0 code `0x3C` / `0x3E` selected by flag bit `0x80`, CLUT id `(flags & 0x7F) + 0x7FC0`, corners `(x, y)`..`(x+w, y+h)` from `uv_rect[2]` / `uv_rect[3]`, UV origin `uv_rect[0] + 0x80` / `uv_rect[1]`, and a fixed vertical white-to-black gouraud ramp (`0xFF`, `0xA0`, `0x50`, `0x00`); links with `FUN_8003D2C4`. No caller in `SCUS_942.54` or the 31 extracted overlays, and no pointer to it in any of them. `see ghidra/scripts/funcs/8002bc38.txt`. |
| `0x801DDD44` (instruction) | Interior of `FUN_801DDC20`: the `nop` in the delay slot of that body's `bne a3,zero` divide-by-zero guard. Not an entry. |
| `0x801DDDEC` (instruction) | Interior of `FUN_801DDC20`: the join label every arm of the ramp branches to before the flag test and the `FUN_80024EE4` push. Not an entry. |
| `800195A8` | Billboard / screen-space textured-quad projector - [details ↓](#800195a8) |
| `80035CB8` / `80035DA0` / `80035E44` | Text-actor sub-handlers. Children of the per-frame text-actor tick (`FUN_80031D00`). Each measures a row via `FUN_80036044` and renders via `FUN_8003CC98`. `_DA0` resolves a magic-name string from `PTR_DAT_80075DB0` keyed by the `0x800754CC + idx*0xC` magic table; `_CB8` advances state at gp `+0x87c` / `+0x13c`. |
| `8003541C` | Text-actor / label register-and-draw - [details ↓](#8003541c) |
| `80030628` | Menu/HUD content builder + layout dispatcher - [details ↓](#80030628) |
| `80034B78` / `80034E4C` | Monospaced base-10 number formatter - [details ↓](#80034b78--80034e4c) |
| `80034B6C` | One-instruction tail fragment immediately preceding `FUN_80034B78` - Ghidra split a `sw param_1, gp[+0x14c]` store into a phantom leaf. `gp[+0x14c]` is the current text-row state byte the text-actor tick (`FUN_80031D00`) writes from `actor[+0x1d]`; it is **not** a GPU-packet allocator (an earlier engine-port REF guessed that). Not a real call target. |
| `8003C1F8` | **Dialog-font glyph-cell sprite emitter.** `(cell_idx, x, y)`. Pushes a 4-word GP0 `0x64`-command textured sprite (`0x64808080` tag, 8x12 px) at the scratchpad OT cursor `_DAT_1F8003A0`, source UV `u = cell_idx*8 + 0x50`, `v = 0xD0`, CLUT word `DAT_8007B454 + 0x7F86` (the live dialog-font color index 0..15, per [`formats/dialog-font.md`](../../formats/dialog-font.md)), then a draw-mode prim via `FUN_80059010`; each packet links into `_DAT_1F8003F4 + 4` through the linkPrim helper `FUN_8003D2C4`. The per-glyph draw primitive behind the proportional dialog font (sibling of the text-actor tick `FUN_80031D00`); distinct from the dev monospaced path `FUN_8001AA68`. `see ghidra/scripts/funcs/8003c1f8.txt`. |
| `8003C310` | Push `POLY_F3` (flat-shaded triangle) GPU primitive onto the OT. Writes size + color + verts; uses Y-offset `_DAT_8007B454`. |
| `8003F348` | Per-frame sprite/animation renderer tick. Walks list at `DAT_8007B7E0`, accumulates draw cost into `gp[+0x990]`. |
| `8003F3FC` | Per-frame particle--ctor update. Clip-tests against viewport `_DAT_1F800384..387`, accumulates physics (`vx*dt`), tests against camera at `_DAT_8007C364+0x14/+0x18`, emits two GP0 line packets (cmd `0x9000000`) via `_DAT_1F8003A0` OT pointer. Calls `FUN_8003F838` (RNG) + `FUN_8003F86C` (line-clip + emit). |
| `8003F838` | Particle PRNG step - 13-instr LCG: `seed = seed * 12 + 2`, byte-swap. State at `_DAT_1F8002A8`. |
| `8003F86C` | OT line-segment emitter with GTE-projected endpoints. 148 instrs: cop2 `0x280030` (RTPT) + `0x1400006` (NCLIP); inserts into ordering table at `_DAT_1F8003F4`. Returns `1` on emit / `0` on cull. |
| `8001FA68` | Generic ringbuffer push-u16: `*(u16*)(p2 + (++*p1)*2) = val`. |
| `8001AD38` | **Per-glyph sprite emitter of the dev monospaced text path** (`FUN_8001AA68`'s primitive; distinct from the proportional dialog-font emitter `FUN_8003C1F8`). `(x, y, u, v, clut)`. Takes 0x10 bytes from the scratchpad packet cursor at `_DAT_1F8003A0`, bumps the cursor, and writes a four-word GP0 packet: tag `0x03000000` (3 payload words), command word `0x74808080` (8x8 textured sprite, opaque, grey `0x808080` modulation), position `(x-4, y-4)` as two halfwords, then the `u`/`v` bytes and the CLUT halfword. Links through `FUN_8003D2C4` into the OT pointer at `_DAT_1F8003E0`. The `-4` on both axes is what makes the caller's coordinate the cell's **centre**. `see ghidra/scripts/funcs/8001ad38.txt`. |
| `80017D98` | Fixed-cell wrapper over `FUN_8001AD38`: sign-extends `(x, y)` and calls it with `u = 0x50`, `v = 0xF8`, `clut = 0x7F80` - one particular cell of the dev-font page, drawn centred at the caller's point. `see ghidra/scripts/funcs/80017d98.txt`. |
| `80049348` | Battle **arts after-image (motion-trail) renderer**. `(node)`. Walks the actor's 31-deep history rings (positions `+0x4C`, anim cursors `+0x17A`, staged anim ids `+0x1FB`, anim contexts `+0x234` - shifted per frame by `FUN_80047430`) at a stride derived from the anim-rate byte `actor+0x21D`; for each history entry whose staged id byte is `> 0x10` (party: the dynamic-art slot-`0x11` clips; enemies: `0x10 + attach_key` specials) it draws a darkened translucent ghost via `FUN_80048A08` (prim word `\| 0x85000000`, RGB `− 0x101010`), re-running the equipment-variant swap `FUN_8004CCD4` with the ghost's historical cursor + entry first. Per-character trail tint from `DAT_80076908` (party) / `DAT_80076914`. `80049348.txt`. |
| `8004A908` | NTSC/PAL-adaptive color dithering + brightness mixer for OT primitives. Reads `_DAT_80078D4C` mode flag. |
| `80046978` | Palette fade / tint engine. Reads RGB components, applies global brightness from `_DAT_1F800393`. |
| `8004695C` | Initiates a color-fade operation: writes RGBA -nto `gp[+0x9D0]`, sets active-flag at `gp[+0x9D4]`. Mode byte at `_DAT_8007B6CC`. |
| `8005724C` | OT primitive initializer for sprite rectangle - pos / size / color / clip. Calls `FUN_800608E0` for display config and `FUN_80057FEC` for palette query. |
| `80059568` / `80059634` / `80059700- | OT coordinate packer trio for textured / textured-variant / opaque sprite primitives. Display-mode-aware mask + shift, COP2 tag bytes `0xE3` / `0xE4` / `0xE5`. |
| `800198E0` | **TIM-upload helper.** Consumes either a custom Legaia sprite descriptor (magic `0x11`, single LoadImage call) OR a real PSX TIM (flags bit 3 = "has CLUT", two LoadImage calls - one for CLUT, one for pixels). Dispatches to `FUN_800583C8` for each block. Optional alpha-bit ORing (`*entry \|= 0x8000`) per CLUT entry when `_DAT_8007B998 != 0`. Confirmed in `ghidra/scripts/funcs/800198e0.txt`. |
| `800583C8` | **`LoadImage` wrapper.** Pushes a libgpu `LoadImage(RECT*, void*)` request - identified by the literal debug-format string reference `s_LoadImage_800156d4`. The actual PSX BIOS `LoadImage` call site lives downstream. |
| `80058490` | **`MoveImage` wrapper.** Sister to `FUN_800583C8`. Identified by the debug-format-string reference `s_MoveImage_800156ec`. Push a libgpu VRAM-to-VRAM `MoveImage(RECT*, dest_x, dest_y)` request. |
| `80058068` | `SetDispMask` wrapper - controls display enable/disable via GP1 command `0x300` / `0x3000001`. |
| `8005800C` | DrawSync callback registration- |
| `80057C44` | Display-mode reset dispatcher - calls GTE init, memory clear, resolution setup. |
| `80058F1C` / `80058FA0` | Rect / Line OT primitive builder pair using COP2 coordinate transforms via the packer trio. |
| `8005AFB0` | GTE control-reg initializ-r (COP2 ctl regs `0xC000..0xF000`). |
| `8005B038` | **Weighted vertex-delta blend loop.** `(dst_verts, deltas, count, weight)`. IR0 = `weight`, per 8-byte packed delta runs `GPF sf=1` (`cop2 0x198003D` - general-purpose interpolation, **not** RTPS/matrix-multiply as earlier readings had it) and adds the `(delta * weight) >> 12` triple onto the destination GTE vertex. The applier under the VDF morph stager `FUN_8001C604`. Ported as `legaia_engine_vm::vdf_morph::apply_weighted_deltas`. `see ghidra/scripts/funcs/8005b038.txt`. |
| `8005B0B8` | GTE shift-converter for texture / color bit packing. |
| `8005B618` | GTE matrix-loader (COP2 MTX regs `0x0..0x2000`). |
| `80021EAC` (data: `_DAT_8007BD24+0x26B`) | Reads the side-band stream-request byte armed by `FUN_80055B4C` (see [`formats/summon-readef.md`](../../formats/summon-readef.md)). |
| `80028158` / `8002A5A4` / `801CFA48` | **Actor render-mode-4 multi-target primitive emitters** - the three leaves the RENDER dispatcher `FUN_8001ADA4` case 4 selects on `actor[+0x9e]` (`0x4000` → `8002A5A4`, `0x2000` → `801CFA48` (overlay 0898), else → `80028158`). Each takes `(out_buf, colour, packed, src)`, zeroes `out_buf[0..0x8]`, builds a GPU packet chain at `out_buf+0xC`, and unpacks a primitive **count** from `packed >> 8`. `8002A5A4` writes a 4-vertex packet with the `src` coordinates halved; `801CFA48` OR-s the GT4 command base `0x3C000000`. Render-track (GPU packet builders); unported. `see ghidra/scripts/funcs/80028158.txt`, `8002a5a4.txt`, `overlay_battle_action_801cfa48.txt`. |
| `80019D50` | **BGR555 cell-grid quad emitter.** Unpacks a `dims[+0x4] × dims[+0x6]` array of `u16` BGR555 cells - channels `(c & 0x1f) << 3` / `(c >> 2) & 0xf8` / `(c >> 7) & 0xf8`, plus the `0x8000` STP (semi-transparent) bit - and emits one coloured quad per **non-zero** cell into the ordering-table cursor `_DAT_1F800314 + 0x8c` (per-cell setup through `FUN_8001A78C`). Render-track (GPU primitives); unported. `see ghidra/scripts/funcs/80019d50.txt`. |
| `800351C0` | **Full-screen backdrop-quad emitter.** Writes one `0x140 × 0xE0` (320×224) primitive - packet tag `0x08000000`, colour word `0x39808080`, both edges clamped `-4` - into the ordering-table cursor `_DAT_1F800314 + 0x8c`, advancing it `0x24` bytes. The solid-fill backdrop primitive behind menu / mode screens. Render-track; unported. `see ghidra/scripts/funcs/800351c0.txt`. |
| `8001B73C` | **GTE on-screen visibility test.** `(actor)`. Builds the four corners of the actor's screen box from `actor[+0x14/+0x16/+0x18]` scaled by `actor[+0x58]`, RTPTs them (`cop2 0x280030`), reads back the SXY FIFO, and returns `1` as soon as **any** projected corner lands inside the `320×240` screen bounds (`x < 0x140`, `0 <= y <= 0xF0`), else `0`. A GTE culling / off-screen probe (no packet emit); unported. `see ghidra/scripts/funcs/8001b73c.txt`. |
| `80029DD8` | **GTE 3D primitive emitter** (499 instrs, 39 `cop2` ops + OT writes) - a sibling of the two TMD renderers `FUN_8002735C` / `FUN_80029888` in the SCUS render band that projects and ordering-table-links geometry through the GTE. Render-track; unported (the engine rasterises via wgpu). `see ghidra/scripts/funcs/80029dd8.txt`. |
| `801E5338` | **World-map actor sparkle-burst emitter.** `(actor)`. A per-actor state machine on `actor[+0x54]` (0 init / 1 spawn / 2 drain) that spawns up to 8 short-lived sprite particles at RNG-jittered offsets (`% 0x20` X, `% 0x10` Y) around the actor origin, ramps each particle's brightness `lifetime*0x18` over a 10-frame life, and emits one semi-transparent textured sprite (GP0 `0x66808080`, `0x18²`) per active particle. Palette bytes come from the Sony table `0x801F2960` (stride 8 per `actor[+0x50]` row). Ported clean-room (SM + draw-list; palette table + RNG caller-supplied) as `legaia_engine_vm::world_map_particle_burst`. `see ghidra/scripts/funcs/801e5338.txt`. |
| `8001763C` | **Draw-environment pair refresh.** Indexes the `0x74`-stride draw-env pair at `0x80083F30` by the current buffer index `gp+0x434` and issues the libgpu draw-env op `FUN_800589D0`; when `a0 == 2` it first stamps the background RGB (`0xFF` into `+0x19/+0x1a/+0x1b` of both env buffers). Uses the same `gp+0x434`-at-stride-`0x74` env-pair indexing the frame-begin driver's dither re-stamp rides (see [`subsystems/renderer.md`](../../subsystems/renderer.md#retails-dither-law-stated-separately-from-the-ports-default)). `see ghidra/scripts/funcs/8001763c.txt`. |
| `8001A374` | Fixed-cell ASCII glyph sprite emitter - `(x, y, char)`. Maps the character code to a font-atlas UV cell (digits / upper / lower / a set of punctuation), builds a textured-sprite GP0 packet at the OT cursor `0x1F800314 + 0x8C` (tpage from `gp+0x660`), and links it via `FUN_8003D2C4`; the dev/HUD monospaced glyph draw. `see ghidra/scripts/funcs/8001a374.txt`. |
| `8001CD68` | Textured-quad (POLY_FT4) emitter - `(x, w, y, h, u, v, clut, tpage, semi)`. Builds a 0x28-byte GP0 `0x2C`/`0x2E` packet (four corners, explicit UV/CLUT/tpage, colour `0x808080`) at the OT cursor and links via `FUN_8003D2C4`. `see ghidra/scripts/funcs/8001cd68.txt`. |
| `8003479C` | Screen wipe / curtain emitter - `(progress 0..0xF2)`. Draws top+bottom fill bars (`FUN_8003C510`) whose extent tracks progress, a gradient pair (`FUN_8003C43C`) below `0x81`, and a full-screen fill at `>= 0xF2`; links via `FUN_8003D2C4`. `see ghidra/scripts/funcs/8003479c.txt`. |
| `800597C8` | Display-mode-aware **X mirror** - reads the display-mode / interlace flags (`_DAT_80078D54` / `_DAT_80078D57`) and returns a rect's mirrored left edge `0x400 - w - x` from the `+0x0` (x) and `+0x4` (w) halfwords; display mode 2 halves `w` first, and each flag-clear path returns `x` or `x/2` unmirrored instead. `0x400` is VRAM **width**, which is what makes this an X mirror and not the Y convert it was long recorded as - a Y mirror would fold about `0x200`. Ported as `engine-vm::battle_helpers::screen_x_mirror`. `see ghidra/scripts/funcs/800597c8.txt`. |
| `80046870` | Brightness/fade ramp-up - increments `gp+0x2E8` by `0x40` and clamps at `0x100`. `see ghidra/scripts/funcs/80046870.txt`. |

## ANM animation container

The container parser is documented in [`formats/anm.md`](../../formats/anm.md). The per-record bytecode dispatcher is overlay-resident (not yet captured); the public SCUS entry point only stages the per-record state on an actor.

| Address | Role |
|---|---|
| `80024CFC` | `play_anm_by_id(id, actor, ?)` - allocates an actor (via `FUN_80020DE0`), reads the per-record offset from `_DAT_8007B7C8 + (id*4) + 4` (the kingdom slot-5 CLUT-walk table installed by `FUN_8001F05C` case 6; parser `legaia_asset::clut_walk`), and stores `(table_base + record_offset)` in `actor[+0x4C]`. Writes `0xB` to `actor[+0x56]` (render mode) and `100` to `actor[+0x68]` (accumulator seed, `>=` any hold so the first copy fires at scene entry). The per-frame walk is `FUN_8001ADA4` case `0xB`; spawner = field-init `FUN_801D6704`, one actor per table entry. |

## MES / dialog text interpreter
-
The MES bytecode interpreter is **statically linked into SCUS_942.54** - not overlay-resident as previously assumed. Four functions cover the encoding fully; the dialog window pager is overlay-resident in the dialog/town overlay. See [`formats/mes.md`](../../formats/mes.md) for the per-byte decoding table.

| Address | Role |
|---|---|
| `8003CA38` | Glyph stride walker. 16 instructions: returns count of bytes until next terminator (`< 0x1F`). For each `(byte & 0xF0) == 0xC0` it consumes an extra byte. |
| `8003CBF8` | Delimited-field offset locator. Same `(byte & 0xF0) == 0xC0` two-byte-token stride as `8003CA38`, but counts occurrences of a delimiter byte `param_2`: returns the glyph-string byte offset reached when the `param_3`-th match is found (terminator = NUL). On no match it sets the debug error-bits `_DAT_8007B828 = 0x174B` (only when the debug flag `_DAT_8007B98C` is non-zero) and returns 0. Ported as `legaia_asset::field_disasm::delimited_field_offset`. `see ghidra/scripts/funcs/8003cbf8.txt`. |
| `80036044` | Text width measurement. Same byte classification as the stride walker plus substitution dispatch on `(byte + 0x40) < 8` (catches `0xC0..0xC7`); the explicit cases `0xC1..0xC5` and `0xC7` follow substitution pointers into character-name / item / magic / spell / quest tables and recursively walk the substituted string. |
| `80036888` | Text renderer. Same opcode dispatch as `FUN_80036044`, but emits glyphs into the text-actor buffer instead of just measuring. Calls `FUN_80036514` to expand substitutions before walking. |
| `80036514` | Substitution expander. Copies from source bytecode to a working buffer, normalising the input-time aliases (`0x5E XX` → `0xCE (XX-0x2D)`, `0xFF` → `0xCF`) and inlining `0xC1..0xC5` / `0xC7` substitutions into glyph runs. |
| `80035F04` | **Max-line-width measure.** Expands the source string via `FUN_80036514` into a stack buffer (default escape value seeded by `FUN_80056788`), then walks the expanded bytes accumulating per-glyph advance: `0x7C` resets the running width and tracks the max-so-far (column separator), `0xCE`-class escapes add a fixed width from the dialog-font escape table at `0x80074050`, ordinary glyphs add their proportional width from the table at `0x80073F3C` offset by `_DAT_800740E8`. Returns the widest line. The label-/name-width helper the title and name-entry renderers call before centring. Cited from `crates/engine-vm/src/title_prim.rs` (`FUN_80035f04` label measure) and `FUN_801E6B34` (name-entry caret width). `see ghidra/scripts/funcs/80035f04.txt`. |
| `FUN_801D84D0` (dialog overlay) | Dialog window pager. 26-state machine (`_DAT_801F2734`) for per-frame paging, 16-line buffer at `_DAT_801F3540`, terminator test `(byte & 0x7F) < 0x20`. Drives the actual on-screen dialog window. |
| `FUN_8001FD44` | **Scene-change packet** (full entry in [asset loading + disc I/O](asset-loading.md#disc--loader-chain)). Sets `_DAT_1F800394 \|= 0x40` (scene-transition-pending flag - *not* a "dialog active" lock, an earlier mislabel) and copies the destination scene name into `0x8007050C`/`0x80084548`. Called from field-VM op `0x3F` (named scene-change), which carries the destination name inline. |

## Dialog-overlay actor-frame helpers

Per-frame substeps of `FUN_801D1344` (the actor frame handler in the dialog overlay). They split the frame into "compute screen position", "step actor physics", "emit sprite primitives", and "build collision bitmask".

| Address | Role |
|---|---|
| `FUN_801CF754` (dialog overlay) | Camera-frame projector. Caches `_DAT_1F800020/24` from the active camera struct (`+0x14/+0x18`), then walks the linked actor list at `*param_2`, looking up each actor's tile descriptor at `_DAT_1F8003EC + slot * 0x20` and computing screen-space `(X, Y)` via the `(s8 << 7) + (s8 << 4)` packing the renderer expects. Skips actors with state bits `0x3` set. |
| `FUN_801D0B90` (dialog overlay) | Walk-regen tick (the "recover while walking" accessory passives). Runs only while `_DAT_801F2274` exceeds `0x20` (minus `0x20` per call); walks the member-id table at `+0x458` from `0x80084140` (count `+0x454`, stride `0x414`), three flag→(field, step, cap) bumps gated on the u32 at `+0x6C0` (record `+0xF8`, ability-bitfield word 1): `0x1000000` (bit `0x38` HP Walk) bumps `+0x6CE` by 8 clamp `+0x6CC`; `0x2000000` (`0x39` MP Walk) `+0x6D2` by 2 clamp `+0x6D0`; `0x4000000` (`0x3A` AP Walk) `+0x6D6` by 1 clamp `+0x6D4` - record-space: the HP/MP/AP currents `+0x106/+0x10A/+0x10E` toward the effective maxima `+0x104/+0x108/+0x10C`. Tail decrements `_DAT_8007B600`, arming a dialog-window callback at zero. Port: `engine-core::walk_regen`. |
| `FUN_801D1BA0` (dialog overlay) | Vertical-step physics for the active actor. Computes `step = DAT_1F800393 * 0xC` (halved when actor flag `0x2000` is set), clamps Y delta by ground-collision via `FUN_801D1878`, and writes back to `actor[+0x16]`. Also resolves the special "frozen drop" path when `actor[+0x9E] == 0`. |
| `FUN_801D9D30` (dialog overlay) | Camera-shake jitter. Subtracts cached camera offsets, then if `_DAT_8007B630 != 0` calls the LCG RNG (`func_0x80056798`) twice to seed new shake offsets at `DAT_801C6EA4 + 0x18/0x1C`, masked against `0xFFFFFF >> (0x15 - amplitude)` (= `(1 << (amplitude + 3)) - 1` for `1..=0x15` - the window *grows* with amplitude). X is centered (`- (mask+1)>>1`); Y is half-range and negated (upward-only). Ported as `engine-vm::battle_camera::apply_shake`; also duplicated verbatim as the tail of `FUN_801DB510`. |
| `FUN_801DB510` (dialog overlay) | Per-frame camera mover + shake - the **same function** as the cutscene overlay's copy (dumps instruction-identical; the "actor sprite emitter" reading is falsified). Moves the typed param table at `0x801F2798`/`0x801F2804` toward the op-`0x45` staged targets; the capture-pinned glide law lives in [`subsystems/cutscene.md`](../../subsystems/cutscene.md). The static `srav` right-shift step belongs to the follow/scroll modes (`DAT_8007B607>>4 == 5` eases `_DAT_80089118/80089120`), not the opening-chain glides. Tail = the `FUN_801D9D30` shake. Ported as `CutsceneCameraInterp`. Only the *menu* overlay hosts different code at this VA (overlays alias). |
| `FUN_801DE234` (dialog overlay) | Tile-collision bitmask builder. Iterates `func_0x80017FBC(idx, x_tile, y_tile)` until it returns 0, ORing `1 << (hit[+4] & 0x1F)` into `_DAT_8007B8F4`. Used by the actor's footprint test gated on flag `0x400000`. |

## Function details

Full write-ups for the rows above whose detail outgrew a table cell. Linked from each section table by **[details ↓]**.

### `80020F88`

**Actor render binding + render-node allocation.** `(actor) -> 0 / -1`. Two
refresh arms off the flag word `actor+0x10`, both reading the `0x20`-byte `.MAP`
placement record table at `_DAT_1F8003EC`:

- bit `0x8000` reads the record at `(actor+0x60)*0x20` and writes
  `actor+0x58 = rec[+0x1E]`, `actor+0x64 = rec[+0x10] + DAT_8007B6F8`,
  `actor+0x52 = rec[+0x12] & 0x3E8`;
- bit `0x100000` picks the kind from `rec[+0x12] & 3` (`0 / 6 / 7 / 8`) - indexing
  the table by `actor+0x64` rather than by `+0x60` - and then re-derives all three
  fields from `+0x60` again, this time masking with `0x380`.

Bit `0x40000` clear also clears bit `0x2`. Kinds `1..=5`, `7` and `8` - **not**
`6`, which is exactly what `rec[+0x12] & 3 == 1` selects - then allocate a
`0x9C`-byte node into `actor+0x44` via `FUN_80017888` (idempotent under bit
`0x800`; on failure the kind resets to `0`, `_DAT_8007B828 |= 0x4000` and the
function returns `-1`), zero `actor+0x7C`, seed `node+0x94/+0x96/+0x98 = 0` and
`node+0x9A = -1`, and - unless bit `0x40000` is set - run `FUN_80024D78`, falling
through to `FUN_800204A4` only on a zero result.

A debug bound check (`_DAT_8007BB38 + 1` against the sign-extended `actor+0x64`,
compared unsigned, so a negative index fails too) prints rather than aborts. The
mesh-index rule this establishes - **`rec[+0x10] + prefix`, not the object's
position in the pack** - is the one recorded in
[`subsystems/renderer.md`](../../subsystems/renderer.md). Ported as
`legaia_engine_render::actor_bind`, `NOT WIRED` (that crate holds no actor pool).
`see ghidra/scripts/funcs/80020f88.txt`.

### `800480D8`

**Per-actor battle draw tick.** `(actor)`. Returns immediately on
`actor+0x10 & 8`.

A set `ctx+0x272` first runs the scene-teardown preamble, itself guarded on the
effect-VM ready flag `DAT_8007BD71 == 0xFF`: four battle-overlay shutdowns
(`FUN_801E0080`, `FUN_801E09F8`, `FUN_801DF6B8`, `FUN_801E2524`), then a sweep
voiding every `DAT_801C90F0[0..0x80]` entry whose target carries flag bit `0x8`,
plus `FUN_801F7B88` when `_DAT_8007BDC0 != 0`. The byte is cleared whether or not
the ready flag let the body run.

Then `FUN_8004A908` (the tint / fade pass) unconditionally, and a split on
`actor+0x74 & 0x00FFFFFF`:

- **zero** - the actor is drawn **only** if a four-way gate passes: seat
  `actor+0x5A` in `3..=6` (the monster seats), `ctx+0x287` set, `gp+0x9F5` clear,
  and `*(DAT_801C9370 + seat*4) + 0x21C == 2`. Failing it means no draw at all
  this frame.
- **non-zero** - `FUN_8005112C`, trail flag `actor+0x6A = 1`, `FUN_80049348`,
  `FUN_8004A908`, then the flag back to `0` unless the seat is exactly `7`; the
  same four-way gate follows, but failing it still draws, untinted.

Passing the gate stamps `actor+0x74 = 0x00808080` and draws. That constant is
**24-bit mid-grey RGB** - `lui v0,0x80 ; ori v0,v0,0x8080` - the same `0x808080`
the after-image ghost and the move-FX streak use, and the mask beside it is
`0x00FFFFFF`. It is not a `0x80808080` flag word. Ported as
`legaia_engine_render::battle_actor_tick`, `NOT WIRED` (none of the five passes it
sequences live in that crate). `see ghidra/scripts/funcs/800480d8.txt`.

### `80059BD4`

**VRAM image/CLUT upload (LoadImage-equivalent).** `FUN_80059bd4(a0 = RECT*, a1 = src_ptr)` where `RECT` is `[+0]=x, [+2]=y, [+4]=w, [+6]=h` (shorts) clamped against the VRAM extent at `0x8007_8D58`/`0x8007_8D5A`. Sends GP0(`0xA0`) "copy CPU→VRAM" then streams `a1` to the GP0 data port (CPU-FIFO loop at `0x80059D78`) or sets up DMA channel 2 (`MADR=a1` at `0x80059DBC`, `BCR`, `CHCR`); the GPU register pointers live in BSS at `gp-0x71D8`(GP1)/`-0x71DC`(GP0)/`-0x71D4`(D2_MADR)/`-0x71D0`(D2_BCR)/`-0x71CC`(D2_CHCR). Pinned by a read-watchpoint on Vahn's CLUT source (`0x800E96A0`) + the dump.

Hook its entry to capture every VRAM upload's `(dest rect, source ptr)` - the [`autorun_clut_upload_hook.lua`](../../../scripts/pcsx-redux/autorun_clut_upload_hook.lua) probe filters to the character CLUT band (dst rows 488..499) and dumps each source. The character CLUT band (rows 488/490/497/498/499 + the row-495/496 effect sub-CLUTs) flows through here from scattered RAM sources; **Vahn's row-490 source is the resident field-scene buffer at `0x800E9690`**.

### `800198E0`

**Per-TIM VRAM uploader + texpage→CLUT-row recorder** - the battle/scene texture-install leaf, called by `FUN_800520F0`'s per-descriptor loop (and `FUN_8001F05C`). Takes an asset chunk `[type, flags, clut-block, image-block, data...]`; if `flags & 8`, uploads the CLUT block via `LoadImage` (`FUN_800583C8`) at the chunk's **declared** `(x,y)`, then uploads the image block.

**Crucially**, after the image upload it writes `table_0x8007BEC0[ ((img_x>>6) + (img_y>>8)*0x10) & 0x1f ] = clut_y` - i.e. it records the CLUT's VRAM row keyed by the image's **PSX texpage** (`(y/256)*16 + (x/64)`). So uploaded textures register "texpage T's palette is at row clut_y". This is the **CLUT-row "relocation"**: there is no per-battle dynamic VRAM allocator - each scene's character TIMs simply declare their own rows, the upload puts the CLUT there, and this table lets the renderer resolve a primitive's CLUT row from its texpage at draw time, **overriding the TMD's nominal CBA row**. Different scenes declaring different rows for the same character (mc2 vs a map01 battle) is why the party CLUT band shifts between captures.

### `8005A4A0`

GPU upload-**queue flusher** (748 B). Drains the ring at base `0x801C9590` (0x40 entries × 0x60 B; `[+0]`=handler, `[+4]`=rect, `[+8]`=src), head idx `0x80078E5C` / tail `0x80078E58`, and `jalr`s `entry[+0]`.

**The battle character CLUTs are sourced from the active field scene's decompressed sec0 TIM_LIST** (LZS on disc): every slot-5 battle CLUT upload (VRAM rows 490/495/496/497/498/499) is byte-present in `0086_map01` sec0 decompressed, and renders as a character palette. They upload through op-type 8 to **dynamically-allocated VRAM rows**, so the disc TMD2's nominal CBA (e.g. Noa→492) is relocated per-battle to the allocated row - explaining why mc2 shows the party at rows 492/494 while a map01 battle uses 490/495..499. Original entry below: Drains a ring of pending upload requests (head/tail indices at `gp-0x71A4`/`-0x71A8`, mod `0x40`), waits on GPUSTAT bit `0x4000000`, and for each entry indirect-`jalr`s the handler (`FUN_80059BD4` for image/CLUT uploads) with the queued `(rect, src)`.

So upload sources are set by whatever **enqueues** into this ring, then flushed here once per frame. The character CLUT band is enqueued during battle-actor render (only when the party characters actually render - not at battle-init), which is why headless probes that can't drive real combat never observe the Noa/Gala (rows 492/494) uploads even though Vahn/effects (490/495..499) flow through every frame.

### `0x8007C018` (data)

Global TMD pointer table. Installed by `FUN_80026B4C` (asset-dispatcher case 2 per-TMD; `sw a0, 0(v1)` where `v1 = lui+addiu(0x8007C018) + idx*4` - Ghidra's static-xref misses the store because the intermediate `addu` defeats constant propagation). Counters: `DAT_8007B774` (write/next-free), `DAT_8007BB38` (walk). Each entry points to a TMD blob with magic `0x80000002`; `+0x8` is `group_count`, `+0xC..` is the `count × 0x1C-byte` group descriptors. Consumed by `FUN_80021B04` (actor allocator), `FUN_801D77F4` (overlay actor allocator + vertex copy), `FUN_801D8280` (table walker), `FUN_801F69D8` (world-map top-view tile dispatcher), `FUN_8001E890` (per-pack count override).

See [`formats/world-map-overlay.md`](../../formats/world-map-overlay.md#dat_8007c018---global-tmd-pointer-table-the-actual-cluster-a-source).

### `801D77F4`

Overlay-resident actor allocator (alt to `FUN_80021B04`). Script-VM `4C D8` host hook (9-byte opcode). Takes `(vdf_idx: i16, tmd_idx: i16, kind: u16, variant: u16)`. Allocates actor slot via `FUN_80020DE0(0x8007068C, _DAT_8007C34C)`; resolves TMD from `DAT_8007C018[(i16)tmd_idx]` and VDF body from `_DAT_8007B7DC + body_offsets[(i16)vdf_idx]`. Two-pass vertex-pool build: sum `TMD_groups[record.idx].vertex_count * 8` into `_DAT_8007BA74`, malloc via `FUN_80017888`, then copy each referenced group's vertices into the pool. Populates `actor[+0x3C]=kind, [+0x3E]=variant, [+0x48]=TMD_ptr, [+0x4C]=VDF_body_ptr, [+0x90]=vertex_pool` (and zeros `+0x56/+0x5C/+0x68/+0x6E`).

Dev printf strings `"tmd"`/`"otbl"`/`"vdf_n"` (preserved in the cutscene_dialogue overlay dump) confirm the structure. 125 instr / 500 B.

### `8001EBEC`

Equipment-conditional per-character TMD group-descriptor patch (the OBJECT 10/11 swap): for each of the three active party slots (indexed by the slot-4 freeze flag `_DAT_8007B824`) it picks one of two pre-built `0x1C`-byte group descriptors (`TMD+0x124` = group 10 vs `TMD+0x140` = group 11) per an equipment byte and overwrites the indexed live group (copying 7 × u32 = 28 bytes), toggling the equipped/unequipped pose. It writes **no** object/group count, so it does **not** add the runtime `nobj` +2 (15→17) - that comes from the player-file equipment-section splice (`FUN_800536BC`, see `80052FA0`), not this swap. (The earlier "Also:
mode-aware sound-driver extension dispatcher" reading is false - the dump has no sound-driver / dispatch path.) See [`character-mesh.md`](../../formats/character-mesh.md) and the `FUN_8001EBEC` reader row in [`world-map-overlay.md`](../../formats/world-map-overlay.md#dat_8007c018---global-tmd-pointer-table-the-actual-cluster-a-source).

### `8001E890`

"DATA_FIELD player loader" - loads `data\field\player.lzs` via the disc index `0x36C` resolver (the dev path `h:\prot\all\data\field\player.lz` is the debug branch). The loaded container is the same 3-descriptor `parse_player_lzs` shape the per-entry extractor labels **PROT 0874** (`befect_data`); the resolver reads it by sector offset, so the PROT-876 *bytes* (a different file) don't match. `FUN_8001E890` LZS-decodes all three descriptors at `piVar2[2..7]`: §0 → the 5-TMD character mesh pack installed into `DAT_8007C018[0..4]` (see [`docs/formats/world-map-overlay.md` § Disc-side source of `[0..4]`](../../formats/world-map-overlay.md#disc-side-source-of-04)),

§1 → the **party field-locomotion ANM container**: 23 records = three 7-record
walk/run/idle banks (Vahn `0..=6` / Noa `7..=13` / Gala `14..=20`, bank slot 1 =
standing idle) plus the savepoint record 21 and aux record 22. Byte-identical
post-LZS to the live runtime container the party actors' `+0x4C` anim pointers
resolve into; parser `legaia_asset::character_pack::field_locomotion_anm`, see
[`docs/formats/anm.md`](../../formats/anm.md#disc-source---the-party-locomotion-bundle-prot-0874-1).
And **§2 → an asset `pack` whose entries are each uploaded to VRAM via `FUN_800198e0` - the field-character texture atlas** (3 pages at texpage `(832,256)` + per-character CLUTs on row 478; see [`docs/formats/character-mesh.md` § Textures (field form)](../../formats/character-mesh.md#textures-field-form) and the parser `legaia_asset::field_char_textures`). It then applies the post-install group-count cap (`entry[+0x08] = 10`) to `DAT_8007C018[0..2]` and dispatches the equipment-conditional patch into `FUN_8001EBEC`.

### `800195A8`

**Billboard / screen-space textured-quad projector.** `(center_vec, half_w: i16, half_h: i16, angle12, sxy0_out..sxy3_out)`. Projects a sprite quad about a center point: `FUN_8003D344` runs one `MVMVA` (rotation × V0 + TR, sf=1) taking the center vector to **view space** (the caller reads MAC1..3 back with `lhu`, so the position wraps to i16); four corners are built in view space as `center ± half_w` (X) / `center ± half_h` (Y), all sharing the view Z.
`FUN_8003D178` resets the GTE rotation to identity **and zeroes TRX/TRY/TRZ**, and when `angle12 != 0` `FUN_8004638C` (`RotMatrixZ` compose, masked to 12 bits) spins the corners in-plane about the **camera axis** (the corner vectors include the view-space center).
`FUN_8005BAC8` then projects - `RTPT` on corners 0..2 plus one `RTPS` on corner 3 - into the caller's four out-pointers (the order `FUN_801E1AB0` writes straight into `POLY_FT4.xy0..xy3`), and `FUN_8003D1A4` restores the saved GTE control words from `&DAT_1F8003C8`. Returns the projected depth (`SZ3 >> 2`, shifted by the scratchpad OT-resolution byte `DAT_1F8003A4`).
Reached from the battle / cutscene / world-map quad emitters (e.g. `FUN_800485BC`); the afterimage caller passes a **dynamic half-width** (fx-state halfword `+0x6C6` − `0x200`) with constant half-height `0x100`.

Ported as `legaia_engine_render::billboard::project_billboard` (the afterimage call shape: `afterimage::project_streak_corners`). The `RotMatrix*` trig source is the in-image q3.12 LUT pair - sine at `0x80070A2C + 2*angle`, cosine read from the same table `0x400` entries (90°) ahead at `0x8007122C` - generated as `4096*sin(2π·angle/4096)` **truncated toward zero**, pinned entry-for-entry by the disc-gated oracle `engine-shell/tests/gte_sin_lut_real.rs` against `billboard::psx_sin`/`psx_cos`.

`see ghidra/scripts/funcs/800195a8.txt`.

### `8003541C`

**Text-actor / label register-and-draw.** `(priority, kind, record_string, p4, p5, p6, p7, sub_kind) -> *node`. The producer side of the text/label drawable list: lazily allocates the same 0x34-byte sentinel-circular doubly-linked head at `gp+0x148` that `FUN_80032434` builds and the per-frame tick `FUN_80031D00` walks, then inserts (or reuses) a node sorted ascending by `priority` (node `+0x8`). Fills the kind byte (`+0x1C`, the `FUN_80031D00` / `FUN_80030628` dispatch selector), the record-string pointer (`+0x18`), a precomputed glyph count (`+0x14`, summed from the length-prefixed `record_string` whose entries are `1 + len*2` bytes) and position/config halfwords (`+0xA..+0x10`), zeroes the per-frame scratch, then calls the layout / sprite-emit dispatcher `FUN_80030628`.

Does not touch the OT cursor directly (delegated to `FUN_80030628`). `see ghidra/scripts/funcs/8003541c.txt`.

### `80030628`

**Menu/HUD content builder + layout dispatcher.** The layout half of the text-actor draw list (producer = `FUN_8003541C`). Switches on the node kind byte (`+0x1C`, cases 2/3/4/6/`0x19`/`0x21`/…) to populate the per-frame element-id scratch arrays at `DAT_801C6020` / `_6220` / `_6420` - party-member rows, item/usability flags resolved against the item table `DAT_8007436A` and spell table `DAT_800754C8`, and the world-map quick-travel landmark menu (case `0x19`, walking the 6-byte `DAT_80073A98` placement records, see [`world-map-overlay.md`](../../formats/world-map-overlay.md) / `legaia_asset::worldmap_menu`) - then emits them via `FUN_80030104`.

Mixed function: the content-selection (item-usability / discovery-flag gating, party/landmark lists) is game logic; the trailing GP0 emission is replaced by the engine's wgpu overlay. `see ghidra/scripts/funcs/80030628.txt`.

### `80034B78` / `80034E4C`

**Monospaced base-10 number formatter** (two byte-identical variants; differ only in leading-zero branch ordering). `(value, min_digits, x, y)` - decodes `value` into nine decimal digits by successive subtraction against the 9-entry pair table at `DAT_80073DCC` (each pair `[high, low]`: `digit_acc += 4` per multiple of the high threshold, `+1` per multiple of the low), then emits each digit as one GP0 `0x64` glyph sprite via `FUN_8003C11C` at the fixed 8 px column stride (`digit << 3` selects the U coordinate in the HUD-number glyph cell).

`gp[+0x15c]` is the leading-zero-suppression latch (set once the first nonzero digit prints); `min_digits` forces zero-padding for fields like the save-screen play-time `HH:MM:SS` (`see ghidra/scripts/funcs/overlay_save_ui_saving_801e08d8.txt` callsite, value clamped to 99/0x3B then `/0x3C` decomposed). This is the integer formatter behind the `0xCE`-escape variable substitution (`FUN_80036888`, see [`formats/dialog-font.md`](../../formats/dialog-font.md)) and the records / save-screen stat counters. `see ghidra/scripts/funcs/80034b78.txt` / `80034e4c.txt`.


### `80020C14` / `80025000`

The SCUS fade family's per-frame half, and the exact reason
[`fade.rs`](../../../crates/engine-core/src/fade.rs)'s ramp used to carry a
guessed endpoint: the loader `FUN_80020B00` and the spawn `FUN_80024E80` were
dumped, the tick was not.

Reading the loader's stores against the tick's loads pins the whole `+0x7C`
block, so the template's three trailing "mode words" are named rather than
opaque:

| Block offset | Template `i16` | Meaning |
|---|---|---|
| `+0x00` / `+0x02` / `+0x04` | `[3..=5] << 6` | current RGB, 10.6 fixed |
| `+0x08` / `+0x0A` / `+0x0C` | `[7..=9] << 6` | target RGB, 10.6 fixed |
| `+0x10` / `+0x12` / `+0x14` | `((end - start) << 6) / [1]` | per-frame delta |
| `+0x18` | `[0]` (word) | fade kind |
| `+0x1C` | `[10]` | start delay, in vsyncs |
| `+0x1E` | `[11]` | hold after the ramp; `-1` = no hold |
| `+0x20` | `[1]` | duration, in vsyncs |
| `+0x22` | `[12]` | the id `FUN_80024E80` stamps, passed on as `FUN_80024EE4`'s first argument |

Every countdown steps by `DAT_1F800393`, the scratchpad vsync delta, so the ramp
is cadence-invariant in the same way the overlay sibling `801DDC20` is. The two
families differ in how they get there: the overlay one **lerps** off the
install-time endpoints each frame, this one **accumulates** a per-frame delta and
clamps onto the target - so a delta whose sign disagrees with `target - current`
never converges, and the clamp is what bounds it.

Two flags come out of the countdowns rather than out of the colour: duration
expiry sets `actor[+0x62] |= 0x100` and keeps drawing, hold expiry sets
`actor[+0x10] |= 8` (the actor-list "finished" bit) and stops. A hold of `-1`
skips the second entirely, which is why the battle-escape template's `[11]` is
`-1`.

Port: `legaia_engine_core::fade_ramp`. `see ghidra/scripts/funcs/80020c14.txt`,
`80025000.txt`, `80020b00.txt`.

### `801DDC20`

**Field-overlay fade-actor RGB ramp tick.** `(actor)`. The per-frame body of the
actors `FUN_801DE478` spawns from the overlay template `&DAT_801F2810`.

Record fields (all on the spawned actor):

| Offset | Type | Meaning |
|---|---|---|
| `+0x10` | u32 | Flags; bit `8` = finished. |
| `+0xB8` / `+0xBA` / `+0xBC` | u16 | Start colour R / G / B. |
| `+0xBE` / `+0xC0` / `+0xC2` | i16 | Target colour R / G / B. |
| `+0xC4` | i16 | Start time. |
| `+0xC6` | i16 | Hold after the ramp; `-1` = no hold. |
| `+0xC8` | u16 | Cursor. |
| `+0xD2` / `+0xD6` | i16 | The two selector arguments passed to `FUN_80024EE4`. |
| `+0xD4` | i16 | Duration. |

The cursor advances by `DAT_1F800393`, so the ramp is denominated in vsyncs and
is cadence-invariant. Three regimes:

- `cursor <= start` - nothing is written; the cursor advances and the call
  returns.
- `start < cursor < start + duration` - each channel is
  `start_ch + (target_ch - start_ch) * (cursor - start) / duration`. MIPS `div`
  truncates toward zero, and `duration` is never rewritten, so this is a straight
  lerp off the install-time endpoints rather than an accumulation.
- `cursor >= start + duration` - the channels are reloaded from the target
  fields, and unless `+0xC6 == -1` the hold expiry (`cursor` past
  `start + duration + hold`) raises flag bit `8`.

While the flag is clear the tick packs `R | G<<8 | B<<16` and calls
`FUN_80024EE4(actor[+0xD6], actor[+0xD2], rgb)`. Seven independent RAM captures
dump the same 133-instruction body here. `0x801DDD44` and `0x801DDDEC` are
interior addresses of it. `see ghidra/scripts/funcs/overlay_baka_fighter_801ddc20.txt`.
