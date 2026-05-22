# Effect VM (battle effect cluster)

The runtime that drives battle-spawn effects (spell casts, item-use animations, hit sparks). Implemented as a per-slot state machine rather than a clean bytecode dispatcher - there's no central switch on a per-slot opcode byte; state transitions are inlined throughout 600+ instructions of the per-frame walker.

The Rust port lives at `crates/engine-vm/src/effect_vm.rs`. It models the slot pool (`Pool`), the `MasterSlot` / `ChildSlot` / `EffectScript` data structures, ports the init (`Pool::init`) and spawn (`Pool::spawn`) APIs faithfully, and exposes a state-machine frame (`Pool::tick`) that delegates per-state transitions to the host through the `EffectHost::advance_state` callback. Engines wire the renderer, RNG, and any per-effect transition logic through `EffectHost`.

Lives in the battle overlay (`0898_xxx_dat`). Three functions:

| Function | Span | Role |
|---|---|---|
| `0x801DE914` | 0x138 | Init / pack-fixup. Called from `FUN_800520F0` case `0xE` with `(id=0x1000, param=0xA00)`. |
| `0x801DFDF8` | 0x290 | Public spawn-effect API: `(byte effect_id, short* world_pos, ushort angle)`. |
| `0x801E0088` | 0x970 | Per-frame walker (update + render). |

The on-disc input format is the [runtime 2-pack wrapper](../formats/effect.md) (PROT entry 873, `data\battle\efect.dat`). Each pack0 entry is a frame-batch animation record; each pack1 entry is an effect-ID script.

## How it dispatches

Each 28-byte master slot carries `(state, counter, ?, sub_state, pos_x, pos_y, pos_z, data_ptr)`. The walker reads `*data_ptr` as the next-state token, but state transitions are inlined throughout `FUN_801E0088` (600+ instructions). To produce a clean-room opcode table you'd need to extract per-state-byte transition logic by hand (~10–20 cases).

The cleaner port path: model the walker as a state-machine class and accept its decompile-shape rather than insisting on an opcode table. The 32-master / 128-child slot pool, the spawn API, and the per-frame walker are all well-understood - the port itself is straightforward; the question is just whether to label the format an "opcode table" or a "state machine".

## Lifetime + render bridge (engine port)

The retail per-state token algebra (`FUN_801E0088` pass 1) is inlined and not yet extracted, so `EffectHost::advance_state` models the lifecycle as a fixed-frame countdown: each work tick increments `master.field_14`, and the slot retires once it reaches `effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES`. Without this an effect terminated on its first work tick and never persisted to draw.

`World::active_effect_markers` is the render-agnostic seam between the pool and the host. It returns one `EffectMarker` per active master slot (world position decoded from the 8.8-fixed pool coords, spawn angle, and a `0.0..=1.0` lifetime fraction for fade). The native host (`play-window`) turns each marker into a world-space billboard cross through the existing `UploadedLines` pipeline (`Scene.overlay_lines`), fading it as it ages. `World::spawn_debug_effect` seats a synthetic effect at a point so the bridge can be exercised by hand (the `E` key in `play-window` spawns one at the player); it is not a retail path.

**Deferred (needs new infra):** the faithful render is Pass 2 of the walker - per-child billboarded PSX sprite primitives sampling the effect bundle's inline atlas (`u,v,page,clut`) from VRAM. That requires decoding the [2-pack wrapper's](../formats/effect.md) inline sprite atlas, uploading the effect-bundle textures into VRAM, and a billboard sprite primitive in the renderer. The runtime effect catalog (PROT 873 `efect.dat`) is also not yet loaded into the battle-enter path, so the action SM's `ui_element` spawns currently resolve to no script - wiring that catalog is the prerequisite for real (non-debug) effect spawns.

## Pool layout (`_DAT_8007BD30`, 5008 bytes total)

```
+0x000  16 bytes   table-head record set by init
+0x010  4096 bytes 128 × 32-byte child slots - per-sprite render state
+0x1010 896 bytes  32 × 28-byte master slots - per-effect-instance state
+0x1390 1968 bytes (unused / future expansion)
```

32 max simultaneous effects × ~4 sprites avg = 128-child sprite pool.

## Side-band streaming-effect handler (`0x801F17F8`)

Called from `FUN_800520F0` case `0xFF`. Streams two specific runtime-only files via `FUN_800558FC`:

- `data\battle\summon.dat` (PROT `0x37F`) - selected when `_DAT_8007BD24[0x26B] & 0x80 != 0`.
- `data\battle\readef.dat` (PROT `0x380`) - opposite branch.

Buffer size per slot: `0x10800` = 67584 bytes. Format unverified; may share the 2-pack layout but not yet confirmed.

## Effect-ID → human effect name mapping

Effect IDs are anonymous; no string table maps id → "fireball / thunder / heal". To name effects, trace call sites of `FUN_801DFDF8` in damage / battle-action code (in town/level-up overlays). Each caller passes a literal byte for `effect_id`; correlate with the action that triggered it (a Tactical Arts move, an item use, a spell cast).
