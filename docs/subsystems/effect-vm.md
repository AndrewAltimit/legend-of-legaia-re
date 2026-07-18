# Effect VM (battle effect cluster)

The runtime that drives battle-spawn effects: spell casts, item-use animations, hit
sparks. It lives in the battle overlay (`0898_xxx_dat`); the per-frame walker is
`FUN_801E0088`. Port:
[`legaia_engine_vm::effect_vm`](../../crates/engine-vm/src/effect_vm.rs).

**What catches people out: this is the one member of
[the runtime VM family](move-vm.md#the-runtime-vm-family) that is not a bytecode
VM at all.** There is no central switch on a per-slot opcode byte; state
transitions are inlined throughout 600+ instructions of the walker. It is named a
"VM" for symmetry with its four siblings, but it is a per-slot **state machine**,
and looking for its opcode table is a dead end - see
[How it dispatches](#how-it-dispatches).

The port models the slot pool (`Pool`), the `MasterSlot` / `ChildSlot` /
`EffectScript` data structures, ports the init (`Pool::init`) and spawn
(`Pool::spawn`) APIs faithfully, and exposes a state-machine frame (`Pool::tick`)
that delegates per-state transitions to the host through the
`EffectHost::advance_state` callback. Engines wire the renderer, RNG, and any
per-effect transition logic through `EffectHost`.

Three functions:

| Function | Span | Role |
|---|---|---|
| `0x801DE914` | 0x138 | Init / pack-fixup. Called from `FUN_800520F0` case `0xE` with `(id=0x1000, param=0xA00)`. |
| `0x801DFDF8` | 0x290 | Public spawn-effect API: `(byte effect_id, short* world_pos, ushort angle)`. |
| `0x801E0088` | 0x970 | Per-frame walker (update + render). |

The on-disc input format is the [runtime 2-pack wrapper](../formats/effect.md) (PROT entry 873, `data\battle\efect.dat`). Each pack0 entry is a frame-batch animation record; each pack1 entry is an effect-ID script.

## How it dispatches

There is no opcode byte anywhere: the "state" bytes are **wait counters**, and the only data consumed are the pack1 spawn records and the pack0 anim frames. The full lifecycle is extracted below - it is a pair of countdown-driven cursor walks, not a token dispatch.

## The extracted pass-1 state algebra

Traced instruction-for-instruction from `overlay_battle_801e0088.txt` (walker) and `overlay_battle_801dfdf8.txt` (spawn). Every wait counter in the system is **5.3 fixed-point**: a frame count is stored `<<3` and decremented by 8 per logic frame (a value already `< 8` clamps to 0), so fractional catch-up ticks stay cheap.

The walker body runs only when the ready flag `DAT_8007BD71` reads `0xFF`. Pass 1 (spawn cadence + child animation/motion) repeats `DAT_1F800393` times per call - the adaptive frame-skip factor, so effect time tracks wall-clock under frame skip - and a sweep that finds zero active masters and zero active children skips the remaining catch-up iterations. Pass 2 (render) runs once per call.

### Master slot lifecycle (28-byte stride, 32 slots at pool `+0x1010`)

| Offset | Field | Behaviour |
|---|---|---|
| `+0` | `child_count` | Total spawn records (pack1 header byte 0). Doubles as the active flag - 0 = free slot. |
| `+1` | `flags` | pack1 header byte 1 (bit 0 = randomized offsets, consumed at spawn time). |
| `+2` | `spawn_cursor` | Records consumed so far. |
| `+3` | `wait` | 5.3 wait counter. Non-zero: decrement by 8 and stop. Zero: run the spawn loop. |
| `+4` | `angle` | Spawn angle `& 0xFFF` (12-bit PSX angle). |
| `+8..+0x10` | `origin x/y/z` | World position, 16.8 fixed (`i16 << 8` at spawn). |
| `+0x14` | - | Never written by the spawn API; its copy into `child[+0x18]` is a dead lane. |
| `+0x18` | `script_cursor` | pack1 `entry + 4`, advanced `+14` per record. |

The spawn loop: seed the next free child slot from the current 14-byte record (allocation scans forward with a cursor that persists across masters within one sweep; on **pool exhaustion the record is still consumed** with no child - effects degrade rather than stall), then advance - `spawn_cursor += 1`, `script_cursor += 14`, `wait = record.delay << 3` - and repeat while the new wait is zero, so zero-delay records spawn as one burst. When `spawn_cursor` reaches `child_count` the master frees itself (`+0` = 0) and forces `wait = 8` to exit the loop.

### Child slot lifecycle (32-byte stride, 128 slots at pool `+0x10`)

Seeding (walker pass 1, from the spawn record + master): `frame_count`(+0) = pack0 byte 0 (doubles as the active flag); `mirror`(+1) = `rand() % 4` - **random UV flip bits** for sprite variety (bit 0 = horizontal, bit 1 = vertical, consumed by pass 2); `frame_cursor`(+2) = 0; `wait`(+3) = first frame's delay `<<3`; velocity (+4/+6/+8 i16 x/y/z) = the record's planar legs rotated by the master angle (`>>12`) with `vel_y` direct; position (+0xC/+0x10/+0x14, 16.8) = master origin, `y -= height << 8`, x/z offset by the rotated planar legs (`>>4`); anim cursor (+0x1C) = pack0 `entry + 2`.

Tick: `wait` non-zero → decrement by 8 plus one motion step. Zero → loop { advance one anim frame (`anim_cursor += 6`, `frame_cursor += 1`, `wait` = new frame's delay `<<3`; reaching `frame_count` retires the slot), then one motion step } while the new wait is zero. A motion step is `pos += vel * frame.speed * pool_scale_0 * 8 >> 15` per axis - with the retail init scalar `0x1000` at pool `+0` this reduces exactly to `pos += vel * frame.speed`.

### Pass 2 - render

For each live child, one flat textured **semi-transparent quad** (9-word GPU packet, tag `0x09000000`, prim code `0x2E`):

- **Brightness envelope**: with `n = frame_count >> 3`, the modulation ramps in over the first eighth of the animation (`0x80 * (frame_cursor+1) / n`) then back out over the rest (`0x80 * (frame_count - frame_cursor) / (frame_count - n)`), clamped at `0x80` (neutral) and written as `r = g = b`.
- **Size**: atlas `w/h * pool_scale_1 >> 8` (retail init `0xA00` → ×10 texel size), projected through `FUN_800195A8` and inserted into the OT at `_DAT_1F8003F4 + depth * 4`.
- **UV corners**: base/extent from the 8-byte atlas entry, corner order swapped per the child's random mirror bits; CLUT from atlas `+4`, tpage from atlas `+6`.

## Lifetime + render bridge (engine port)

The engine port does not yet execute the algebra above: `EffectHost::advance_state` models the lifecycle as a fixed-frame countdown - each work tick increments `master.field_14`, and the slot retires once it reaches `effect_vm::DEFAULT_EFFECT_LIFETIME_FRAMES`. Without this an effect terminated on its first work tick and never persisted to draw. Now that the retail algebra is fully specified, a faithful `Pool::tick` is a mechanical port (tracked in [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md)).

The per-frame walker splits into two host hooks because retail runs two
distinct passes at different cadences.
`EffectHost::advance_state` is the `state == 0` *script* work and is gated on
the state byte.
`EffectHost::accumulate_child_motion` is the *per-child position integration*
(`child+0xc/+0x10/+0x14 += velocity * accel * frame_delta`) and runs **every
frame for every active slot regardless of `state`** - `FUN_801E0088` performs
that accumulation in both its `state == 0` work loop and its `state != 0`
wait-countdown branch, so a child billboard keeps drifting during a wait state.
`Pool::tick` therefore calls `accumulate_child_motion` *before* the state-byte
gate; gating it behind `advance_state` (the earlier shape) froze waiting
effects. The hook's default is a no-op until the child-motion renderer lands,
so the contract is faithful even though no host integrates motion yet.

### Catalog load

The runtime effect catalog (PROT 0873 `efect.dat`) loads at scene entry via `EffectCatalog::from_efect_dat_bytes` (the 2-pack parser - see [`formats/effect.md`](../formats/effect.md)), staying resident on `World::effect_catalog` across field/battle transitions. So the action SM's `ui_element` spawns (`FUN_801D8DE8 → FUN_801DFDF8`, ported as `World::try_spawn_effect`) resolve to real effect scripts. The catalog carries the pack1 effect scripts + per-child descriptors, the pack0 animation batches, and the inline sprite atlas.

### Render snapshots

Two render-agnostic seams expose the live pool:

- `World::active_effect_markers` - one coarse `EffectMarker` per effect (origin + age). For hosts/tests that only need effect positions.
- `World::active_effect_sprites` - the faithful per-child billboard view (the textured-quad path). For each active effect it resolves the effect's children through the catalog, walks each child's pack0 animation to the current frame, and reads that frame's sprite-atlas entry for size + VRAM `(u, v)` / `tpage` / `clut`. Mirrors `FUN_801E0088` pass 2 (one GPU sprite primitive per child).

The native host (`play-window`) draws each `EffectSprite` two ways: a **camera-facing textured quad** through the VRAM-mesh pipeline (`upload_vram_mesh`, sampling the scene VRAM at the sprite's atlas page/clut/uv as a `SceneDraw`), plus a **tinted outline** through the `UploadedLines` pipeline so the billboard is visible regardless of VRAM contents, faded by age. `World::spawn_debug_effect` seats a synthetic effect by hand (the `E` key in `play-window`); it is not a retail path.

**Two effect-texel pools, both pixel-verified.** The retail `befect_data` block (CDNAME defines `872..875` → extraction entries **870..873**) holds the four battle effect files - `etim.dat` (0870), `etmd.dat` (0871), `vdf.dat` (0872), `efect.dat` (0873) - pulled by `FUN_800520F0` at raw TOC indices `0x368..0x36B`; see the verified case→index→entry map in [`formats/effect.md`](../formats/effect.md#battle-effect-cluster-befect_data). The texels effects sample come from two pools:

- **`etim.dat` = extraction 0870** (three 64×256 4bpp TIMs targeting VRAM `(320,0)`/`(384,0)`/`(448,0)`, CLUTs rows 474..476) is byte-verified loaded at battle and is **battle-only** - those columns hold town stage textures during a field scene, so uploading it at field entry would clobber field rendering. The engine uploads it on **battle entry** (`scene::upload_flame_atlas_into_vram`, called from the play-window battle-render setup into a throwaway VRAM copy that battle exit discards).
- **The `player_data` §2 band (extraction 0874 §2** - previously mislabeled "etim" here; it is `player.lzs` section 2, the field-character texture pack, see [`formats/character-mesh.md`](../formats/character-mesh.md#textures-field-form)**)**: eight TIMs at `fb_y=256+` whose pages are **field-resident** through battle (the `fb(320,256)`/`fb(384,256)` pages match a `town01` field capture 256 rows byte-exact, and a mid-cast battle capture byte-matches the `(832..880, 256+)` tiles). The Gimard flame model samples *this* band (page `(832,256)`, CLUT row 478). The engine uploads it at scene entry (`scene::upload_effect_textures_into_vram`); the field VRAM-parity oracle uploads image-pages-only (`upload_clut = false`) since retail uploads the CLUT rows at battle entry.

Full byte evidence: [`formats/effect.md` § Effect texels in VRAM](../formats/effect.md#effect-texels-in-vram---pixel-verified).

The **3D-model render path** is wired: `World::active_effect_models` snapshots each live effect that has a model assigned (`EffectModel` = global-TMD-pool index + world position + age), and the native host (`play-window`) builds a textured `legaia_tmd` VRAM mesh for it through the standard mesh pipeline, drawing it at the effect origin with the `etim` texels resident.

**The real effect-model library (extraction 0871, `etmd.dat`, raw index `0x369`) is loaded.**
`engine-core::scene::seed_effect_model_library_from_etmd` reads entry 0871 (an
uncompressed 30-entry `asset::pack` of Legaia TMDs spanning the entry's
*extended* footprint) at scene entry and registers all 30 into
`World::global_tmd_pool[3..=32]` - the same `DAT_8007C018[3..=32]` window
retail fills at battle init (`FUN_800520F0` → `FUN_80026B4C`), overwriting the
two trailing slots of the field character pack exactly as retail's load order
does. Gimard's *Tail Fire* is `GIMARD_TAIL_FIRE_MODEL_INDEX = 26` (pack entry
23); the `F`-key dev spawn in `play-window` draws it from the loaded library,
falling back to the field-character-pack preview mesh
(`ETMD_TAIL_FIRE_MODEL_INDEX`, the flame-like auxiliary TMD of extraction
0874 §0) only when the library isn't resident.

**Summon animation - render path RESOLVED (live trace); CLUT cycling falsified.** The model geometry is retail-accurate and the static flame renders with the correct baked row-478 CLUT.

- **The flame motion is geometric, not palette.** Two animation-distinct Tail Fire frames have a **byte-identical** CLUT band (VRAM rows 470..499) while the framebuffer differs ~21% (this **falsifies** the earlier "fire flicker = CLUT cycling" reading).
- **A live PCSX-Redux trace of a player Gimard *Burning Attack* cast pinned what draws the summon.** Across all three phases `FUN_801F7088` fired **0×**, the move VM `FUN_80023070` fired only **2-3×** (noise), and the **battle per-actor draw `FUN_80048A08` fired 35-64×/frame** → the per-object rigid-TRS keyframe decoder `FUN_8004998C` → cluster-A `FUN_80043390`.
- **So the player summon is posed exactly like an enemy monster body** (per-object rigid TRS keyframes), and the faithful render is the **battle TRS-keyframe draw already ported in `engine-vm/anim_vm.rs`** (`FUN_80048A08` / `FUN_8004998C`) - *not* a move-VM scene-graph and *not* `FUN_801F7088` (which is the world-map top-view tile renderer aliasing the same `0x801Fxxxx` band).
- **The summon stager overlays (extraction PROT 903..913) *do* contain real move-VM part records** (recovered under the corrected link base `0x801F69D8` by `legaia_asset::summon_overlay` - superseding the wrong-link-base "PROT 905 has zero `jal 0x80023070` → no move VM" reading, where the `jal` actually lives in the SCUS stager `FUN_80021B04`, not inside the overlay), and the engine drives them as a **stand-in** (`summon::SummonScene`); but the live trace shows that scene-graph is not the player summon's per-frame render path.
- **SCOPE:** the trace covers the **player** "Burning Attack" only - the **enemy** Gimard *Fire Tail* boss move is untraced and may still use the overlay/move-VM path.

See [`battle-action.md`](battle-action.md#seru-magic-summon-overlay-dispatch) and the open-rev-eng-threads "Seru-magic summon visual" row for the full reconciliation.

This is distinct from the 2D billboard path here:

- `World::active_effect_sprites` builds billboards from the `efect.dat` atlas. An earlier reading held that its `0x7680` field was a tpage sampling VRAM **page (0,0), 8bpp** - falsified by the pass-2 consumer.
- That `0x7680` is the atlas entry's **CLUT**, not its tpage - the `+4`/`+6` fields are CLUT (u16) / tpage (byte), the reverse of an earlier reading (the emit at `~0x801E0980` writes `atlas[4..5]` into the primitive's CLUT field and `atlas[6]` into its tpage field). `0x7680` decodes as CBA fb `(0,474)`, an effect-CLUT row, *not* page `(0,0)`.
- Confirmed from a melee hit-spark battle capture: no prim samples page (0,0)/8bpp/`0x7680`, and the spark draws as textured quads sampling the loaded effect pages (PROT 870 flame atlas `(320,0)`/`(448,0)`, effect-band CLUTs).
- The engine's `SpriteAtlasEntry` now reads the fields in the correct order, so `active_effect_sprites` yields the real effect page + CLUT and the billboards sample the resident PROT 870 / `etim` texels. The faithful per-frame cadence is specified above ([pass-1 algebra](#the-extracted-pass-1-state-algebra)) but not yet executed by the port; the render loops each child's anim batch uniformly over the effect lifetime as a stand-in.

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

- `data\battle\summon.dat` - selected when `_DAT_8007BD24[0x26B] & 0x80 != 0`.
- `data\battle\readef.dat` - opposite branch.

**Resolved**: in retail `FUN_800558FC` ignores the path string and consumes its
fourth argument as a retail TOC index - `summon.dat` = `0x37F`, `readef.DAT` =
`0x380`, which are **extraction entries 893 / 894** (the retail in-RAM TOC keeps
the PROT.DAT 8-byte header, so retail index = extraction index + 2). Each file
is an exact array of `0x10800`-byte slots (103 / 78) carrying per-special-attack
CLUT rows + 4bpp texture pages and summon-creature actor records. Byte-verified
RAM↔disc and VRAM↔disc in a mid-cast save state. Full format + verification:
[`summon-readef.md`](../formats/summon-readef.md); parser
`legaia_asset::summon_readef`.

## Effect-ID → human effect name mapping

Effect IDs are anonymous; no string table maps id → "fireball / thunder / heal". To name effects, trace call sites of `FUN_801DFDF8` in damage / battle-action code (in town/level-up overlays). Each caller passes a literal byte for `effect_id`; correlate with the action that triggered it (a Tactical Arts move, an item use, a spell cast).

## See also

**Reference** -
[efect.dat format](../formats/effect.md) ·
[Battle action SM](battle-action.md) ·
[Move-table VM](move-vm.md)
