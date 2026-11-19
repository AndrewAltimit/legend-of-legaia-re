# Emulator timing rules

Every frame-cost figure the port is fitted or scored on is an emulator's. The pacer
`FUN_80016B6C` stores `max(CPU, GPU of the previous table)` in hblanks
([battle-stage-camera](../subsystems/battle-stage-camera.md#what-a-real-consoles-frame-costs)),
and the two emulators the library's states come from time both halves differently. This
page states each one's rules as its source code applies them, so a cost model can be
derived on either scale instead of fitted. The port's reference scale is PCSX-Redux's:
the fitted field model and the battle census reproduce it, and anything on Mednafen's
scale is an opt-in experiment.

Sources: PCSX-Redux at the tree the probes run (`src/core`), and Mednafen `1.29.0`, the
installed `/usr/games/mednafen` (release tarball, `src/psx`). Citations name the file and
the function; no emulator source is copied here.

## PCSX-Redux

| Part | Rule | Where |
|---|---|---|
| Instruction | `2` cycles each (`Emulator::BIAS`), whatever it is | `psxinterpreter.cc`, the interpreter's step; `psxemulator.h` |
| Data access | `+1` a read or write through the memory map, in every region (scratchpad included) | `psxmem.cc`, `Memory::read8/16/32` (data reads only), `write8/16/32` |
| Instruction cache, load delay, `MULT` / `DIV`, GTE | no cycle cost | `psxinterpreter.cc`; `gte-instructions.cc` keeps no count |
| GPU | free: the chain DMA completes a cycle a word, the raster is instantaneous, the status reads idle | `gpu.cc`, `GPU::dma`; `src/gpu/soft` |

So a PCSX-Redux frame cost is `2 * instructions + data accesses`, and its entry equals its
CPU word on all but a handful of passes.

## Mednafen

### CPU (`psx/cpu.cpp`, `psx/psx.cpp`)

| Part | Rule | Where |
|---|---|---|
| Issue | `1` cycle an instruction, unless a pending load's latency absorbs it | `PS_CPU::RunReal`, the decode step |
| Instruction cache | `4` KB direct-mapped, lines of four words, a full-address tag; a hit is free, a miss costs `3` plus a cycle a word from the missed word to the line's end (`4..7`) | `PS_CPU::ReadInstruction` |
| Uncached fetch | `+4` a word (`0xA0000000` and up, or the cache off) | `PS_CPU::ReadInstruction` |
| Main-RAM load | `+3` (`MemRW`) `+2`, charged at once; the next instructions that do not read the loaded register absorb that latency one cycle each | `PS_CPU::ReadMemory`, `ReadAbsorb` in `RunReal` |
| Load after a non-load | `+2` more, not absorbable (the `ReadFudge` term) | `PS_CPU::ReadMemory` |
| Scratchpad load | free | `PS_CPU::ReadMemory` |
| Store | free in any region (no write-buffer stall) | `PS_CPU::WriteMemory`, `MemRW` |
| `LWC2` | as a load at `+3 +1`, after waiting for the GTE; its latency is never absorbed | `RunReal`, the `LWC2` case |
| GTE command | starts once the previous one is done, then runs in the background | `RunReal`, the `COP2` case; `GTE_Instruction` |
| GTE access | `MFC2` / `CFC2` / `MTC2` / `CTC2` / `SWC2` wait for the running command | `RunReal` |
| `MULT` / `MULTU` | `7`, `10` or `14` cycles in the background by the operand's size (`MULT_Tab24`) | `PS_CPU::PS_CPU`, `RunReal` |
| `DIV` / `DIVU` | `37` in the background; `MFLO` / `MFHI` wait | `RunReal` |
| I/O reads | GPU, MDEC, timers `+1`; SPU `+16` / `+36`; CD `+6` a byte | `MemRW` |

GTE command latencies (`psx/gte.cpp`, each command's return value): `RTPS` `15`, `RTPT`
`23`, `NCLIP` `8`, `AVSZ3` / `AVSZ4` `5`, `MVMVA` `8`, `NCS` `14`, `NCT` `30`, `NCCS` `17`,
`NCCT` `39`, `NCDS` `19`, `NCDT` `44`, `CC` `11`, `CDP` `13`, `DPCS` `8`, `DPCT` `17`,
`INTPL` `8`, `SQR` `5`, `OP` `6`, `GPF` / `GPL` `5`, `DCPL` `8`.

### GPU (`psx/gpu.cpp`, `gpu_polygon.cpp`, `gpu_sprite.cpp`, `gpu_line.cpp`, `gpu_common.inc`)

The GPU keeps a time budget, `DrawTimeAvail`, in units of half a CPU cycle: `GPU_Update`
adds two a CPU cycle and caps it at `256`, every drawing command spends from it, and
`ProcessFIFO` will not start a command while it is negative. The GPU's idle bit reads set
only with the budget at or above zero, the FIFO empty and no command half-done, which is
what `DrawSync` waits for.

| Command | Units spent | Where |
|---|---|---|
| Any drawing command | `2`; the clip and offset words (`E3`..`E5`) spend nothing | `ProcessFIFO` |
| Polygon setup | `64 + 18` a packet's first triangle, `28 + 18` a quad's second; plus `450` Gouraud-textured, `288` Gouraud, `180` textured | `Command_DrawPolygon` |
| Polygon raster | `2` a pixel shaded or textured, `1.5` flat blended or mask-tested, `1` flat; a row the walk passes above or below the drawing area `2`; no per-row term inside it | `DrawTriangle`, `DrawSpan` |
| Rejected triangle | setup only: `512` or more high, `1024` or more wide, or degenerate | `DrawTriangle` |
| Sprite | `16`, then per row its width (`+` half again blended) | `Command_DrawSprite`, `DrawSprite` |
| Line | `16` a segment, then `2` a step | `DrawLine` |
| Fill | `46`, then `width / 8 + 9` a row, the width rounded up to `16` | `Command_FBFill` |
| VRAM copy | `2` a pixel | `Command_FBCopy` |
| Texture cache | `256` entries of a four-halfword VRAM block (sixteen 4-bit, eight 8-bit, four 15-bit texels); a miss `4`; a page change that moves the page or switches palette and direct colour empties it | `GetTexel`, `SetTPage` |
| CLUT cache | a changed CLUT word loads `16` (4-bit) or `256` (8-bit) | `Update_CLUT_Cache` |

Semi-transparency costs nothing extra on a shaded or textured polygon, and texture depth
costs nothing except through the texture cache.

### DMA and the line (`psx/dma.cpp`, `psx/gpu.cpp`, `psx/timer.cpp`)

| Part | Rule | Where |
|---|---|---|
| Linked list | a node header `15` cycles (`10` for an empty node), then a cycle a word | `RunChannelI` |
| FIFO gating | the GPU channel moves a word only while the GPU's FIFO is ready, so a packet's DMA waits for the command ahead of it | `ChCan`, `GPU_CalcFIFOReadyBit` |
| CPU during the list | not halted: only a sync-mode-`0` transfer halts the CPU | `RecalcHalt` |
| Line | `3412.5` GPU clocks (`3412` and `3413` alternating) at `53.693` MHz, `2152.6` CPU cycles | `GPU_Update`, `GPUClockRatio` |
| Hblank count | timer 1 in hblank mode ticks once a line at the retrace's start | `TIMER_ClockHRetrace` |

So on Mednafen a table costs, packet after packet, the longer of its DMA and its drawing,
with up to `128` cycles banked by an idle GPU, and the whole wait overlaps the next pass's
CPU work.

## The GPU model against these rules

[`gpu_cost.py`](../../scripts/mednafen/gpu_cost.py) carries two prices of a retail table.
`ot_cost` is DuckStation's measured v2 GPU costs (its row in
[battle-stage-camera](../subsystems/battle-stage-camera.md#what-a-real-consoles-frame-costs));
`med_ot_cost` is Mednafen's rules above, walked in the table's order with the texture and
CLUT caches simulated and the drawing and DMA in series. Where they part:

| Term | `ot_cost` (DuckStation) | Mednafen |
|---|---|---|
| Per-pixel rate | `1` cycle shaded / textured, `0.5` flat, `0.75` flat blended | the same |
| Per row | `2` cycles (`+1` blended textured, `+3` blended flat) | none inside the area, `1` a row passed outside it |
| `GT3` / `GT4` setup | `295` / `495` cycles | `267` / `515` |
| `FT3` / `FT4` setup | `130` / `215` | `132` / `245` |
| `F3` / `F4` setup | `50` / `50` | `42` / `65` |
| Texture and CLUT cache | none | `2` cycles a block miss, `8` / `128` a palette load |
| List node | `8` header, `+5` a non-empty block | `15` a non-empty header, `10` an empty one |
| DMA against drawing | summed | in series per packet, the longer of the two |
| Line length | `3413 * 7 / 11` = `2171.9` cycles | `2152.6` |

Over a mednafen recording's GPU-bound passes ([`frame-cost-film.py`](../../scripts/mednafen/frame-cost-film.py)
logs both, `med_drawn` beside `gpu_drawn` + `dma_drawn`; [`frame-cost-agreement.py --model
mednafen`](../../scripts/ci/frame-cost-agreement.py) scores the second), the ring entry
against each model, `entry - model` in hblanks:

| Recordings | GPU-bound passes | DuckStation median | Mednafen median | Mednafen within `5` | Mednafen p10 / p90 |
|---|---:|---:|---:|---:|---:|
| `keikoku_chest_pre`, `zenoir_summon_mid_cast`, `theeder_summon_mid_cast`, `party_battle_gobu_gobu`, `menu_status_field`, `v0_1_battle_command_submenu`, `zora_glare_petrify_post`, `vahn_walks_out` | `303` | `-19.9` | `-0.9` | `.86` | `-3.6` / `+6.0` |

Per recording the Mednafen median sits within `3` hblanks of the entry (`keikoku_chest_pre`
`+1.8`, `zenoir_summon_mid_cast` `-0.1`, `v0_1_battle_command_submenu` `-1.1`,
`zora_glare_petrify_post` `-1.1`, `menu_status_field` `-2.9`), where DuckStation's runs
`20` to `30` over on the heavy ones. On the `86` CPU-bound passes it passes the entry by more
than `10` on two. Over the library's single retail states the same holds: on the `33`
whose entry leads the CPU word by `10` or more, the Mednafen price lands within `10` of the
entry on `25`, at a median `entry - model` of `-0.7` (DuckStation's `-15.4`, `9` within
`10`). The misses are waits that are not the table (`gola_gola_summon_mid_cast` `231`
against `108`, `minigame_slot_machine` `147` against `91`, `ending_scene_load_gap`) and
single snapshots whose drawn table is not the one the entry waited on (`keikoku_chest_pre`
`287` against `270`, `zenoir_summon_mid_cast` `268` against `252`, which their films put at
`+1.8` and `-0.1`).

Without the texture cache the Mednafen price reads `6.5` hblanks low at the median, `24` on
`keikoku_chest_pre`'s field table. Interleaving matters too: pricing each packet on its own
as `max(dma, draw)` with the empty nodes after (the engine's form below) reads `2.6` hblanks
over the walk in order at the median, `6` at most.

### The engine's price

`legaia_engine_system::gpu_draw_cost` carries the Mednafen price beside the DuckStation one
([`gpu_draw_cost/mednafen.rs`](../../crates/engine-system/src/gpu_draw_cost/mednafen.rs)):
each packet the native window links costs `max(15 + words, units / 2)` cycles, each bucket
it leaves empty `10`, with the triangle walk, the texture cache (off the meshes' own UVs,
TSB and CBA words) and the CLUT cache as above. Over a retail table's polygons it gives
the Python price to the hundredth of an hblank (`keikoku_chest_pre` `255.72`,
`zenoir_summon_mid_cast` `234.85`, polygons only).
`WorldToggles::gpu_cost_timing` (`LEGAIA_GPU_COST_TIMING=mednafen`) hands that figure to the
GPU-cost frame steps in place of DuckStation's, and `WorldToggles::field_gpu_cost_step`
(`LEGAIA_FIELD_GPU_COST=1`) gives the field the same `max(cpu, gpu of the previous table)`
the battle step has. All three are off by default and touch nothing while off.

## Where the two CPUs part

The Mednafen / PCSX-Redux ratio on the same frame is content-dependent, `1.0` on light
field frames and `0.86..0.90` on heavy ones ([the field model](../subsystems/engine.md#the-field-frame-cost-model)),
because the rules above move it both ways. Against PCSX-Redux's flat `2` an instruction
and `+1` an access:

| Class | PCSX-Redux | Mednafen, cache hot |
|---|---:|---:|
| ALU, branch, jump | `2` | `1` |
| Store | `3` | `1` |
| Scratchpad load | `3` | `1` |
| Main-RAM load, used after its delay slot | `3` | `7` (`1 + 2 + 5`, one cycle absorbed) |
| Main-RAM load, five independent instructions after | `3` | `3` |
| `LWC2` from main RAM | `3` | `7` and the GTE wait |
| GTE command | `2` | `1`, then the next GTE access waits out its latency |
| Straight-line cold code (a miss a line) | `2` to `3` | `+1.75` an instruction |

Run over the battle dispatcher's leaves (`FUN_80043390`'s per-group handlers in SCUS, each a
software-pipelined loop a primitive) with the operand regions read off the disassembly
(`t2` is `0x1F800314`, the scratchpad; the vertex, normal and packet pointers are main RAM),
these rules give, per primitive, Mednafen against PCSX-Redux cycles:

| Leaf | Packet | Linked | Rejected by `NCLIP` | Cold, first pass |
|---|---|---:|---:|---:|
| `FUN_80043658` | `POLY_F3` | `100` / `119` (`.84`) | `74` / `72` (`1.03`) | `1.68` |
| `FUN_800438B8` | `POLY_FT3` | `117` / `139` (`.84`) | `71` / `72` (`.99`) | `1.68` |
| `FUN_80043DD4` | `POLY_GT3` | `132` / `151` (`.87`) | `71` / `72` (`.99`) | `1.69` |
| `FUN_800448B0` | `POLY_F3`, `DPCS` | `137` / `185` (`.74`) | `72` / `73` (`.99`) | `1.55` |
| `FUN_80044C14` | `POLY_FT3`, `DPCS` | `167` / `216` (`.77`) | `71` / `72` (`.99`) | `1.54` |
| `FUN_800453BC` | `POLY_GT3`, `DPCT` | `200` / `231` (`.87`) | `71` / `72` (`.99`) | `1.62` |
| `FUN_80044434` | `POLY_GT3`, `NCCT` | `215` / `183` (`1.18`) | `63` / `67` (`.94`) | `1.96` |
| `FUN_80043768` | `POLY_F4` | `154` / `148` (`1.04`) | `137` / `123` (`1.11`) | `1.80` |
| `FUN_80043C6C` | `POLY_FT4` | `169` / `166` (`1.02`) | `144` / `117` (`1.23`) | `1.77` |
| `FUN_80043F10` | `POLY_GT4` | `194` / `191` (`1.02`) | - | `1.73` |
| `FUN_80045194` | `POLY_FT4`, `DPCT` | `244` / `270` (`.90`) | `182` / `176` (`1.03`) | `1.66` |

A hot triangle leaf is cheaper on Mednafen: its ALU work runs at one cycle and its stores
are free, and the main-RAM loads are spread so the latency mostly absorbs. A quad leaf is
not: `RTPT` (`23`) and then `RTPS` (`15`), each followed by an `NCLIP` (`8`), leave the
CPU waiting on the GTE, which PCSX-Redux never does. A lit leaf waits out `NCCT` (`39`).
Cold code pays the cache fill. So a frame of many primitives through a few hot loops reads
below `1`, and a light frame, whose code runs once from a cold cache, reads at or over it.

The leaves alone do not predict the field's ratio. The engine counts the heavy shots'
primitives at the same mix (`rim_elm_zoom_intro` `2100` decoration and `2225` placed
primitives, `vahn_walks_out` `1331` and `1895`, both about `70%` quads), and the PCSX-Redux
profile puts `97%` and `94%` of their frames in the decoration, ground and draw passes. At
the leaves' linked ratios that mix reads `0.97..0.98` of PCSX-Redux (textured, unlit), against the measured
`.856` (`694` / `811`) and `.904` (`426` / `471`): the field's passes run their own code in
the field overlay, whose timing the battle leaves do not stand for, and the two shots'
difference is not in their primitive mix.

These ratios are the rules run over the disassembly (`see ghidra/scripts/funcs/<addr>.txt`
for each leaf), not a measurement of either emulator; they assume every branch takes the
linked or rejected path throughout and the loop already resident in the cache. The field
pass's own leaves sit in the field overlay and are not in the table. No cycle count from a
console exists to check either emulator against, which keeps the CPU half of the
[open thread](../reference/open-rev-eng-threads.md) open.
