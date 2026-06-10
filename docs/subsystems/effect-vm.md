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

**Two effect-texel systems - 3D-model textures pinned, 2D-sprite source open.** The `befect_data` cluster is cleanly extractable via `asset befect-cluster` (footprint-bounded entries + LZS-container expansion + content classification; see [`formats/effect.md`](../formats/effect.md#battle-effect-cluster-befect_data-cdname-872)). Entry 874 is an LZS container of the effect 3D models (`etmd.dat`), a pack (`vdf.dat`), and the **effect-model textures** (`etim.dat`). `etim` is pixel-verified against a live battle VRAM capture (Gimard's *Tail Fire*, a 3D flame mesh): its tiles byte-match VRAM at `fb_x≥320`, and the `etmd` model primitives reference exactly its CLUT rows. `etim` is **field-resident** (its `fb(320,256)`/`fb(384,256)` pages match a `town01` field capture 256 rows byte-exact),
not battle-only. The engine uploads `etim` into the scene VRAM at scene entry (`scene::upload_effect_textures_into_vram`), so those texels are resident for effect-model rendering; the field VRAM-parity oracle uploads them image-pages-only (`upload_clut = false`) since retail uploads their CLUTs at battle entry.

The sibling **PROT 870 flame-texture atlas** (three 64×256 4bpp TIMs targeting VRAM `(320,0)`/`(384,0)`/`(448,0)`, CLUTs rows 474..476) is byte-verified loaded at battle and is **battle-only** - those columns hold town stage textures during a field scene, so uploading it at field entry would clobber field rendering. The engine uploads it on **battle entry** (`scene::upload_flame_atlas_into_vram`, called from the play-window battle-render setup into a throwaway VRAM copy that battle exit discards). See [`formats/effect.md`](../formats/effect.md#texel-source---etimdat-pixel-verified).

The **3D-model render path** is wired: `World::active_effect_models` snapshots each live effect that has a model assigned (`EffectModel` = global-TMD-pool index + world position + age), and the native host (`play-window`) builds a textured `legaia_tmd` VRAM mesh for it through the standard mesh pipeline, drawing it at the effect origin with the `etim` texels resident.

**The real effect-model library (PROT 871) is loaded.** `engine-core::scene::seed_effect_model_library_from_etmd` reads PROT 871 (`etmd.dat`, an uncompressed 30-entry `asset::pack` of Legaia TMDs spanning the entry's *extended* footprint) at scene entry and registers all 30 into `World::global_tmd_pool[3..=32]` - the same `DAT_8007C018[3..=32]` window retail fills at battle init (`FUN_800520F0` → `FUN_80026B4C`), overwriting the two trailing slots of the §0 field head exactly as retail's load order does. Gimard's *Tail Fire* is `GIMARD_TAIL_FIRE_MODEL_INDEX = 26` (pack entry 23); the `F`-key dev spawn in `play-window` draws it from the loaded library, falling back to the §0 preview mesh (`ETMD_TAIL_FIRE_MODEL_INDEX`) only when the library isn't resident.

**Summon animation — render path RESOLVED (live trace); CLUT cycling falsified.** The model geometry is retail-accurate and the static flame renders with the correct baked row-478 CLUT.

- **The flame motion is geometric, not palette.** Two animation-distinct Tail Fire frames have a **byte-identical** CLUT band (VRAM rows 470..499) while the framebuffer differs ~21% (this **falsifies** the earlier "fire flicker = CLUT cycling" reading).
- **A live PCSX-Redux trace of a player Gimard *Burning Attack* cast pinned what draws the summon.** Across all three phases `FUN_801F7088` fired **0×**, the move VM `FUN_80023070` fired only **2-3×** (noise), and the **battle per-actor draw `FUN_80048A08` fired 35-64×/frame** → the per-object rigid-TRS keyframe decoder `FUN_8004998C` → cluster-A `FUN_80043390`.
- **So the player summon is posed exactly like an enemy monster body** (per-object rigid TRS keyframes), and the faithful render is the **battle TRS-keyframe draw already ported in `engine-vm/anim_vm.rs`** (`FUN_80048A08` / `FUN_8004998C`) — *not* a move-VM scene-graph and *not* `FUN_801F7088` (which is the world-map top-view tile renderer aliasing the same `0x801Fxxxx` band).
- **The PROT 905 stager overlay *does* contain real move-VM part records** (recovered under the corrected link base `0x801F69D8` by `legaia_asset::summon_overlay` — superseding the wrong-link-base "PROT 905 has zero `jal 0x80023070` → no move VM" reading, where the `jal` actually lives in the SCUS stager `FUN_80021B04`, not inside the overlay), and the engine drives them as a **stand-in** (`summon::SummonScene`); but the live trace shows that scene-graph is not the player summon's per-frame render path.
- **SCOPE:** the trace covers the **player** "Burning Attack" only — the **enemy** Gimard *Fire Tail* boss move is untraced and may still use the overlay/move-VM path.

See [`battle-action.md`](battle-action.md#seru-magic-summon-overlay-dispatch) and the open-rev-eng-threads "Seru-magic summon visual" row for the full reconciliation.

This is distinct from the 2D billboard path here:

- `World::active_effect_sprites` builds billboards from the `efect.dat` atlas, whose `tpage = 0x7680` samples VRAM **page (0,0), 8bpp** (confirmed via `FUN_801E0088` pass 2).
- That `0x7680` is the atlas entry's **CLUT**, not its tpage — the `+4`/`+6` fields are CLUT (u16) / tpage (byte), the reverse of an earlier reading (the emit at `~0x801E0980` writes `atlas[4..5]` into the primitive's CLUT field and `atlas[6]` into its tpage field). `0x7680` decodes as CBA fb `(0,474)`, an effect-CLUT row, *not* page `(0,0)`.
- Confirmed from a melee hit-spark battle capture: no prim samples page (0,0)/8bpp/`0x7680`, and the spark draws as textured quads sampling the loaded effect pages (PROT 870 flame atlas `(320,0)`/`(448,0)`, effect-band CLUTs).
- The engine's `SpriteAtlasEntry` now reads the fields in the correct order, so `active_effect_sprites` yields the real effect page + CLUT and the billboards sample the resident PROT 870 / `etim` texels. The faithful per-frame token cadence (`FUN_801E0088` pass 1 state algebra) is also still inlined-only; the render loops each child's anim batch uniformly over the effect lifetime as a stand-in.

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

The runtime buffer per slot is `0x10800` = 67584 bytes; the file format is not yet
decoded, and **the PROT entry these dev paths map to is unpinned**. The earlier
"summon.dat = PROT `0x37F` / readef.dat = PROT `0x380`" reading is falsified - 895 /
896 are the boot init pak and the contested mode-24 overlay remnant, and the
`0879..=0890` band that guess sat in is all `VABp` sound banks. See
[`effect.md`](../formats/effect.md#side-band-streaming-effect-handler) for the full
correction and how to pin the real entry.

## Effect-ID → human effect name mapping

Effect IDs are anonymous; no string table maps id → "fireball / thunder / heal". To name effects, trace call sites of `FUN_801DFDF8` in damage / battle-action code (in town/level-up overlays). Each caller passes a literal byte for `effect_id`; correlate with the action that triggered it (a Tactical Arts move, an item use, a spell cast).

## See also

**Reference** —
[efect.dat format](../formats/effect.md) ·
[Battle action SM](battle-action.md) ·
[Move-table VM](move-vm.md)
