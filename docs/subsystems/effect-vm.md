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
(`Pool::spawn`) APIs faithfully, and executes the full pass-1 algebra in
`Pool::tick_retail` (master spawn cadence + child anim/motion walk) with the
pass-2 per-child computation exposed as `Pool::child_billboards` (brightness
envelope, atlas resolution, sprite scaling, UV-mirror corner order). The
`EffectHost` trait supplies the RNG and the summon routing. The engine's live
path runs this walker: `engine-core::World::tick_effects` sweeps
`Pool::tick_retail` once per retail frame and `World::active_effect_sprites`
is a direct mapping of `Pool::child_billboards`.

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

The walker body runs only when the ready flag `DAT_8007BD71` reads `0xFF`. Pass 1 (spawn cadence + child animation/motion) repeats `DAT_1F800393` times per call - the adaptive frame-skip factor, so effect time tracks wall-clock under frame skip - and a sweep that finds zero active masters and zero active children adds 4 to the sweep counter, skipping the remaining catch-up iterations (fully, at any retail frame-skip factor `<= 5`). Pass 2 (render) runs once per call.

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

The spawn loop: seed the next free child slot from the current 14-byte record (allocation scans forward with a cursor that persists across masters within one sweep; on **pool exhaustion the record is still consumed** with no child - effects degrade rather than stall), then advance - `spawn_cursor += 1`, `script_cursor += 14`, `wait = record.delay << 3` - and repeat while the new wait is zero, so zero-delay records spawn as one burst. The wait store is a byte, so a delay `>= 32` frames wraps mod 32 (`sb` truncates the `<< 3`); the same truncation applies to the child frame delays below. When `spawn_cursor` reaches `child_count` the master frees itself (`+0` = 0) and forces `wait = 8` to exit the loop.

### Child slot lifecycle (32-byte stride, 128 slots at pool `+0x10`)

Seeding (walker pass 1, from the spawn record + master): `frame_count`(+0) = pack0 byte 0 (doubles as the active flag); `mirror`(+1) = `rand() % 4` - **random UV flip bits** for sprite variety (bit 0 = horizontal, bit 1 = vertical, consumed by pass 2); `frame_cursor`(+2) = 0; `wait`(+3) = first frame's delay `<<3`; velocity (+4/+6/+8 i16 x/y/z) = the record's planar legs rotated by the master angle (`>>12`) with `vel_y` direct; position (+0xC/+0x10/+0x14, 16.8) = master origin, `y -= height << 8`, x/z offset by the rotated planar legs (`>>4`); anim cursor (+0x1C) = pack0 `entry + 2`.

Tick: `wait` non-zero → decrement by 8 plus one motion step. Zero → loop { advance one anim frame (`anim_cursor += 6`, `frame_cursor += 1`, `wait` = new frame's delay `<<3`; reaching `frame_count` retires the slot), then one motion step } while the new wait is zero. A motion step is `pos += vel * frame.speed * pool_scale_0 * 8 >> 15` per axis - with the retail init scalar `0x1000` at pool `+0` this reduces exactly to `pos += vel * frame.speed`.

Retirement quirk: retiring zeroes both the active flag and the wait, but the frame-advance loop only tests the wait - so retail keeps consuming 6-byte strides past the batch end **on the already-retired slot** until it hits a non-zero byte in the delay position. The extra reads and motion steps touch only the dead slot (the next seed rewrites every field), so the port (`Pool::tick_retail`) breaks at retirement instead.

### Pass 2 - render

For each live child, one flat textured **semi-transparent quad** (9-word GPU packet, tag `0x09000000`, prim code `0x2E`):

- **Brightness envelope**: with `n = frame_count >> 3`, the modulation ramps in over the first eighth of the animation (`0x80 * (frame_cursor+1) / n`) then back out over the rest (`0x80 * (frame_count - frame_cursor) / (frame_count - n)`), clamped at `0x80` (neutral) and written as `r = g = b`.
- **Size**: atlas `w/h * pool_scale_1 >> 8` (retail init `0xA00` → ×10 texel size), projected through `FUN_800195A8` and inserted into the OT at `_DAT_1F8003F4 + depth * 4`.
- **UV corners**: base/extent from the 8-byte atlas entry, corner order swapped per the child's random mirror bits; CLUT from atlas `+4`, tpage from atlas `+6`.

The semi-transparency is a property of the **prim code**, not of the atlas entry. `0x2E`
is GP0 `0x20 | quad | textured | semi-transparent`, so every effect child blends,
whatever page it names - and the page's own ABR bits then choose how. The `efect.dat`
inline atlas's two entries name pages `0x25` (`(320,0)`, ABR `1` = `B + F`) and `0x66`
(`(384,0)`, ABR `3` = `B + 0.25F`), both against CLUT rows 474/475 of the flame atlas.

That matters to a port because the enable has nowhere to come from except the code byte:
the atlas stores its page in a single byte, so a billboard builder that pushes the page
verbatim into a TSB word can never set the port's prim-ABE bit, and the whole effect
system rasterises opaque. Flame CLUT row 474 is a fire ramp whose hot end (`0xC73F` =
`(248, 200, 136)`) is a pale tan; drawn additively those texels are a glow over a dark
arena, and drawn opaque they are solid tan blobs. Index `0` is `0x0000` and discards
either way, so the blobs keep a puff-shaped silhouette - which is what made them read as
stray geometry rather than as mis-blended sprites. Port: `effect_sprite_tsb` in the
native window's `geometry.rs`, pinned by
`every_effect_billboard_corner_is_semi_transparent`.

## Lifetime + render bridge (engine port)

The algebra above is executed by `Pool::tick_retail` (pass 1: master spawn
cadence over the catalog's pack1 records + child anim/motion walk over the
pack0 frames, with the `frame_skip` catch-up factor) and `Pool::child_billboards`
(pass 2: per-child brightness envelope, atlas resolution off the current
frame, `sprite_scale` sizing, and the random UV-mirror corner order - the GTE
projection `FUN_800195A8` and the OT insert stay with the renderer). The only
host callback the faithful walker consumes is `EffectHost::next_random`. Two
deliberate port-side deltas, both invisible to retail behaviour: the
retirement-loop overrun is cut at retirement (see the quirk note above), and
`master.field_14` - a retail dead lane - is bumped once per call per active
master as an age counter for age-based render fades.

This walker is the engine's only per-frame effect path. `engine-core`'s
`World::tick_effects` runs one `tick_retail` sweep per retail logic frame
(`World::tick` gates it on the ~60 Hz retail-frame sub-clock, so the 5.3
wait-counter cadence tracks retail wall-speed from the 100 Hz sim), and
`World::active_effect_sprites` maps `child_billboards` one-for-one. The
pre-algebra host-delegating shim (`Pool::tick` +
`EffectHost::advance_state` / `accumulate_child_motion`, a fixed-lifetime
countdown) is retired; the dev-only `World::spawn_debug_effect*` helpers
keep a fixed budget, but they live outside the pool
(`World::debug_effects`) so the walker never sees them.

### Catalog load

The runtime effect catalog (PROT 0873 `efect.dat`) loads at scene entry via `EffectCatalog::from_efect_dat_bytes` (the 2-pack parser - see [`formats/effect.md`](../formats/effect.md)), staying resident on `World::effect_catalog` across field/battle transitions. So the action SM's `ui_element` spawns (`FUN_801D8DE8 → FUN_801DFDF8`, ported as `World::try_spawn_effect`) resolve to real effect scripts. The catalog carries the pack1 effect scripts + per-child descriptors, the pack0 animation batches, and the inline sprite atlas.

A spawn is seated at an **actor**, never at the world origin: the retail spawn caller copies the owning actor's own world position (`actor+0x34..0x3B` via `lwl`/`lwr` into the position buffer), offsets it by the per-effect planar legs rotated through the facing's sin/cos LUTs, and passes the facing halfword (`actor+0x46`) as the spawn angle - `FUN_8004998C`'s effect arm at `0x8004A634..0x8004A81C` calling `FUN_801DFDF0(id, sp+0x10, actor+0x46)` (disassembly; see `ghidra/scripts/funcs/8004998c.txt`). The engine's `BattleActionHost::ui_element` mirrors this by spawning at the acting actor's battle seat with its `facing_angle`.

A second producer feeds the same two spawn seams per battle frame: the per-action **effect-script walk** (`FUN_801DEA50`, see [`battle-action.md`](battle-action.md#the-per-action-effect-script-fun_801dea50)). Its `0x80`-flagged records route into the pool via `World::try_spawn_effect`; its table-form records stage a `0x801F6324` prototype scene via `World::spawn_action_table_effect` (a small move-VM scene-graph in `World::active_action_fx`, ticked by `World::tick_move_fx` and drawn through `World::active_move_fx_part_draws`).

### Render snapshots

Two render-agnostic seams expose the live pool:

- `World::active_effect_markers` - one coarse `EffectMarker` per effect still in its spawn phase (origin + age), plus the dev `debug_effects`. For hosts/tests that only need effect positions.
- `World::active_effect_sprites` - the faithful per-child billboard view (the textured-quad path): a one-for-one mapping of `Pool::child_billboards` over the pool's live child slots - each child's integrated 16.8 position, its current pack0 frame's atlas rect + `tpage`/`clut`, the pass-2 sprite sizing (`atlas w/h * sprite_scale >> 8`), the retail brightness envelope, and the random UV-mirror corner order. `FUN_801E0088` pass 2, one GPU sprite primitive per child.

Both hosts draw each `EffectSprite` as a **camera-facing textured quad**: the native window through the VRAM-mesh pipeline (`upload_vram_mesh`, sampling the scene VRAM at the sprite's atlas page/clut/uv as a `SceneDraw`, modulated by the pass-2 brightness with the mirror-resolved UV corner order), the play page through the same shape in `web-viewer::play_battle_fx`.

Each host also carries a **tinted outline** builder - a flat rectangle around the quad, faded by animation age - and on both it is a diagnostic that is **off by default**, because retail draws no such rectangle. See [the outline is a diagnostic](#the-billboard-outline-is-a-diagnostic-and-defaults-off-on-both-hosts).

`World::spawn_debug_effect` seats a synthetic marker by hand (the `E` key in `play-window`); it is not a retail path and lives outside the pool.

#### The billboard outline is a diagnostic, and defaults off on both hosts

The outline predates the battle-entry flame-atlas blit. It existed so a spawn stayed readable when its texels were not resident; with the atlas resident the textured quad draws on its own and the rectangle is only in the way.

It is also not a faint marking. The strips are untextured, carry no ABE bit, and so rasterise in the **opaque** pass, and the tint law `(80 + 175f, 200f, 255f)` for `f = 1 - age01` is red-dominant at every point of a sprite's animation - pale rose at spawn, dark red at the end. What that draws is a solid red-ish box around every effect sprite in a fight.

The two gates are host-shaped, because a WASM module has no process environment to read:

| host | builder | gate | default |
|---|---|---|---|
| native `play-window` | `effect_sprite_line_geometry` (`UploadedLines`) | env `LEGAIA_DIAG_FX=1` | off |
| browser play page | `play_battle_fx` outline strips (hybrid-flat quads) | `LegaiaRuntime::set_battle_fx_outline(true)` | off |

This is a worked example of the drift shape in [`tooling/host-drift.md`](../tooling/host-drift.md): the native gate landed on its own, the browser twin kept drawing, and a diff of the gating commit reads as complete because the file it touched is complete. The pairing to check is "does the other host reach the same builder under the same condition", not "was the builder edited".

#### The quad half-extent is a view-space quantity, so the battle camera scale must be divided back out

`FUN_800195A8` transforms the sprite **centre** through the GTE camera matrix (`FUN_8003D344`, one `MVMVA`), then forms the four corners by adding the half-extents to that *already-transformed* view-space centre, then resets the rotation matrix to identity with `TRX/TRY/TRZ = 0` before the `RTPT`. The camera matrix therefore multiplies the centre and never touches the half-extents.

In battle that matrix carries retail's base matrix `0x8007BF10` = `16384 * I`, a **4x uniform scale**. A port that offsets the corners in *world* space and draws the quad under the same scaled MVP puts the half-extents through the 4x a second time, so every battle effect sprite comes out exactly `BATTLE_WORLD_SCALE` too large - a 32-texel puff spanning 1280 view units instead of 320, three quarters of an actor's height instead of a fifth. The shared correction is `engine-vm::effect_billboard::world_half_extents(size, view_scale)`; the native window passes its `fx_scale` (the same factor it composes into `fx_cam`) into `effect_sprite_corners`.

#### "The spawns fire and nothing appears" is mostly the atlas, not the pipeline

Measured on `play-window --scene town01 --battle 4` by differencing two otherwise byte-identical frames with the billboard draw suppressed: the layer contributes a real, in-frame delta (up to ~53 per channel, mean ~10 over the puff), so the spawn, the projection, the texel residency and the semi-transparent blend pass are all working. What it does *not* look like is a visible effect, for two data reasons worth knowing before re-opening that thread:

- Every effect the Rim Elm spar fires (`0x01`, `0x05`, `0x06`) resolves through pack0 anim batch `1` to atlas page **`0x66`** = `(384, 0)`, whose texpage bits carry **ABR 3 = `B + 0.25*F`**, under CLUT `0x76C0` = row 475 palette 0 - a dark warm-grey ramp topping out at `(184, 144, 112)`. A quarter of a dark grey ramp added over a bright tan arena floor is a few percent.
- The bright effects are the other pages. `0x25` = `(320, 0)` and `0x27` = `(448, 0)` are **ABR 1 = `B + F`**, and those are the pages a retail melee-hit-spark capture shows the spark drawing from. They belong to other effect ids (`0x04`, `0x0B..0x0E`, `0x10..0x14`, `0x16`, `0x17`, `0x1C..0x1E`), which the spar's clips never request.

So a battle whose only live effect is the walk-clip dust reads as an empty effect layer even when the layer is correct. `LEGAIA_DIAG_FX=1` on the native window logs each live sprite's world position, quad size, page/CLUT, brightness, projected NDC and a VRAM texel-residency verdict, which separates "behind the eye" / "off-screen" / "texels absent" / "drawing but faint" in one line. `LEGAIA_DIAG_NOFX=1` suppresses the billboard draw so two runs can be differenced, and `LEGAIA_DIAG_NOSEMI=1` turns the semi-transparency blend pass off so a deferred fragment draws opaque instead of vanishing.

**Two effect-texel pools, both pixel-verified.** The retail `befect_data` block (CDNAME defines `872..875` → extraction entries **870..873**) holds the four battle effect files - `etim.dat` (0870), `etmd.dat` (0871), `vdf.dat` (0872), `efect.dat` (0873) - pulled by `FUN_800520F0` at raw TOC indices `0x368..0x36B`; see the verified case→index→entry map in [`formats/effect.md`](../formats/effect.md#battle-effect-cluster-befect_data). The texels effects sample come from two pools:

- **`etim.dat` = extraction 0870** (three 64×256 4bpp TIMs targeting VRAM `(320,0)`/`(384,0)`/`(448,0)`, CLUTs rows 474..476) is byte-verified loaded at battle and is **battle-only** - those columns hold town stage textures during a field scene, so uploading it at field entry would clobber field rendering. The engine uploads it on **battle entry** (`scene::upload_flame_atlas_into_vram`, called from the play-window battle-render setup into a throwaway VRAM copy that battle exit discards).
- **The `player_data` §2 band (extraction 0874 §2** - previously mislabeled "etim" here; it is `player.lzs` section 2, the field-character texture pack, see [`formats/character-mesh.md`](../formats/character-mesh.md#textures-field-form)**)**: eight TIMs at `fb_y=256+` whose pages are **field-resident** through battle (the `fb(320,256)`/`fb(384,256)` pages match a `town01` field capture 256 rows byte-exact, and a mid-cast battle capture byte-matches the `(832..880, 256+)` tiles). The Gimard flame model samples *this* band (page `(832,256)`, CLUT row 478). The engine uploads it at scene entry (`scene::upload_effect_textures_into_vram`); the field VRAM-parity oracle uploads image-pages-only (`upload_clut = false`) since retail uploads the CLUT rows at battle entry.

Full byte evidence: [`formats/effect.md` § Effect texels in VRAM](../formats/effect.md#effect-texels-in-vram---pixel-verified).

The **3D-model render path** is wired: `World::active_effect_models` snapshots each dev-spawned model effect (`EffectModel` = global-TMD-pool index + world position + age, from the pool-external `World::debug_effects` exerciser - the production effect-id → model selection is the move/art-VM path, `World::spawn_move_fx`), and the native host (`play-window`) builds a textured `legaia_tmd` VRAM mesh for it through the standard mesh pipeline, drawing it at the effect origin with the `etim` texels resident.

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

See [`battle-action.md`](battle-action.md#seru-magic-summon-overlay-dispatch) and the [`re-settled-threads.md`](../reference/re-settled-threads.md) "Seru-magic summon visual" row for the full reconciliation.

This is distinct from the 2D billboard path here:

- `World::active_effect_sprites` builds billboards from the `efect.dat` atlas. An earlier reading held that its `0x7680` field was a tpage sampling VRAM **page (0,0), 8bpp** - falsified by the pass-2 consumer.
- That `0x7680` is the atlas entry's **CLUT**, not its tpage - the `+4`/`+6` fields are CLUT (u16) / tpage (byte), the reverse of an earlier reading (the emit at `~0x801E0980` writes `atlas[4..5]` into the primitive's CLUT field and `atlas[6]` into its tpage field). `0x7680` decodes as CBA fb `(0,474)`, an effect-CLUT row, *not* page `(0,0)`.
- Confirmed from a melee hit-spark battle capture: no prim samples page (0,0)/8bpp/`0x7680`, and the spark draws as textured quads sampling the loaded effect pages (PROT 870 flame atlas `(320,0)`/`(448,0)`, effect-band CLUTs).
- The engine's `SpriteAtlasEntry` reads the fields in the correct order, so `active_effect_sprites` yields the real effect page + CLUT and the billboards sample the resident PROT 870 / `etim` texels. The faithful per-frame cadence ([pass-1 algebra](#the-extracted-pass-1-state-algebra)) is executed by `Pool::tick_retail`, with the pass-2 computation exposed as `Pool::child_billboards` - and the `engine-core` snapshot `active_effect_sprites` maps those live child slots directly (the earlier uniform-loop stand-in is gone).

### The floating value readout rides the same atlas

The numeral a landed hit throws is not an effect-pool child, but it samples the
same texture page: `etim.dat`'s third TIM, page `(448, 0)`, through the
sub-palette at VRAM `(48, 476)` (CBA `0x7703`, tpage `0x27`). The sheet's
layout - ten 24x24 digit cells in strip order `1234567890`, plus the `DAMAGE` /
`HIT` / `TOTAL` labels - is in
[`formats/effect.md`](../formats/effect.md#the-battle-value-readouts-glyph-sheet-lives-here-too).

The geometry is read out of retail's own display list. Both frame arenas of the
`battle_melee_hit_spark` capture carry the same two-digit run, so the pair is an
animation: the run's horizontal **centre** holds while the cell **grows**
toward its 1:1 24-px size, and the run **rises** to a fixed screen row `y = 32`.
Cell pitch is the drawn width plus one; the quads are `0x2C` at colour
`0x808080`, so retail neither modulates nor fades the numeral. Port:
`engine-vm::battle_value_readout::value_cells`.

Placing it is a **host** job, not a HUD-builder job: the seat is the struck
actor's projected screen position, which only the layer holding the camera
knows. The native window projects the actor under the FX camera and emits the
cells as screen-space VRAM quads (retail's own texels, since the battle loader
has already made the atlas resident); the browser play page has no
screen-space VRAM sink, so it draws the same layout through
`engine-ui::battle_value_readout_draws_for`, the dialog-font fallback - retail's
cells and pitch, different letterforms. `engine-ui`'s HUD builder draws the
popup queue only under `LEGAIA_DIAG_HUD`.

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

Two producers of the 2D-pool spawn wrapper `FUN_801DFDF0` are confirmed: the move-power `+0x12`/`+0x16` effect-id lists dispatched by `FUN_801e09f8`, and the per-move effect-list spawner `FUN_801e22c8` (called by the battle effect driver `FUN_800402f4`), which walks a 5-byte-stride list at `0x801F6470` through the same bit-7 multiplex. See [`effect.md` § the bit-7 multiplex](../formats/effect.md#how-a-move-reaches-this-2d-pool---the-bit-7-multiplex).

## See also

**Reference** -
[efect.dat format](../formats/effect.md) ·
[Battle action SM](battle-action.md) ·
[Move-table VM](move-vm.md)
