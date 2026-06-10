# Field free-movement locomotion

The player free-movement controller for normal towns, dungeons, and walkable field areas is **`FUN_801d01b0`** in the field overlay (`overlay_0897`). Each frame it reads the held pad, turns it into a camera-relative direction, advances the player actor's world position a fixed step per sub-frame with per-axis collision, and updates the player's facing angle. This is the general locomotion path — **not** the [tile-board grid mode](tile-board.md) (a puzzle / board minigame that happens to live in the same overlay).

`FUN_801d01b0` was pinned with a runtime write-watchpoint on the player position fields (`scripts/pcsx-redux/autorun_player_pos_watch.lua`): walking in a field scene fires write hits at the four `sh` stores `0x801D0684 / 06E4 / 0744 / 07B4` (player Z± / X±), all inside `FUN_801d01b0`. Static analysis alone never surfaced it because the writes are buried in a 1964-byte function and the field overlay only loads at runtime.

## Player actor fields used

The player actor pointer is the global `_DAT_8007c364`. Confirmed fields on the actor struct:

| Offset | Meaning |
|---|---|
| `+0x10` | flags; bit `0x80000` = movement disabled (encounter pending / cutscene), bit `0x1000000` = action/interact requested |
| `+0x14` | world X (`s16`) |
| `+0x16` | facing angle for the renderer (`s16`, set elsewhere) |
| `+0x18` | world Z (`s16`) |
| `+0x26` | heading (8-direction movement angle, set from the pad direction) |
| `+0x5c` | running/dash state counter (`> 0` switches the walk-animation select) |
| `+0x72` | per-actor speed multiplier (fixed-point, `>> 12`) |
| `+0x94` | encounter-record pointer (see [encounter format](../formats/encounter.md)) |
| `+0x98` | interaction-target actor pointer |

World coordinates are plain `s16` in 1-unit resolution; one collision tile is `0x80` (128) units (see below). The field camera derives its origin by negating these — see [`world-map.md`](world-map.md) and the camera notes in [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

## Spawn position on scene entry

The player actor's spawn position is set by the per-scene initializer `FUN_801D6704` (MAIN_INIT), not by the locomotion controller. There are two cases, selected by the field-entry mode global `_DAT_8007b8b8`:

- **Cold entry (`_DAT_8007b8b8 == 0`).** The player actor is created at actor coords **`(0xA40, 0, 0xA40)`** — the centre of the camera's `0x20`-tile view window — via `func_0x80024c88(&local_68=…)`, which writes `actor+0x14 = sVar13 + 0xA40`, `actor+0x16 = 0`, `actor+0x18 = sVar14 + 0xA40`. On a cold entry the sub-tile terms `sVar13`/`sVar14` are zero, so the spawn is the fixed window centre. The camera itself is seeded onto the MAN anchor (`local_60`/`local_5e`, filled by `FUN_8003AEB0`), then the follow camera tracks the player. **Cold entry only ever happens for the New Game opening scene (`town01`, Rim Elm)** — every other scene change is a warp — so `(0xA40, 0xA40)` is effectively Vahn's authored opening spawn. Byte-checked walkable against town01's base collision grid.
- **Warp entry (`_DAT_8007b8b8 == 2`).** `sVar13`/`sVar14` carry the sub-tile offset of the saved transition coords `_DAT_80084568`/`_DAT_8008456C` (`saved & 0x7F` minus `0x40`), so the player lands at the destination door rather than the window centre.

Provenance: `ghidra/scripts/funcs/overlay_0897_801d6704.txt` (the `func_0x80024c88` call + the `_DAT_8007b8b8 == 2` sub-tile block), `ghidra/scripts/funcs/80024c88.txt` (sets `actor+0x14/16/18` from the arg vec). Engine mirror: `legaia_engine_core::world::FIELD_COLD_SPAWN_XZ`, applied in `SceneHost::enter_field_scene`.

## Per-frame flow

1. **Disabled gate.** If `player.flags & 0x80000` is set, skip all movement (an encounter is queued or a cutscene owns the player).
2. **Action button.** An edge-pad action bit (`_DAT_8007b874 & 4`, gated by `DAT_8007b6a8`) plays the confirm SFX `func_0x80035b50(0x20)` and raises `player.flags |= 0x1000000` (talk / examine), short-circuiting movement that frame.
3. **Direction decode.** `func_0x800467e8(&_DAT_8007b850)` rewrites the held pad in place into a *camera-relative* mask (so "screen up" maps to the correct world axis regardless of camera azimuth). `FUN_80046494(player)` reads that remapped mask (`gp+0x538`) and returns the movement direction in bits `& 0xf000`, resolving diagonals (`0x9000 / 0xc000 / 0x3000 / 0x6000`). The player heading `+0x26` is set to one of eight angle constants from the same mask.

   | mask bit (post-remap) | axis delta |
   |---|---|
   | `0x1000` | Z + |
   | `0x4000` | Z − |
   | `0x2000` | X + |
   | `0x8000` | X − |

   (Same bit→direction convention as the tile board, because both call `func_0x800467e8`.)
4. **Speed.** The frame's travel distance is

   ```text
   speed = ((base_step * player[+0x72]) >> 12) * DAT_1f800393
   ```

   where `base_step` is `8` walking (`5` / `0xc` / `0x18` in special states such as run / forced-walk), `player[+0x72]` is the per-actor multiplier, and `DAT_1f800393` is the per-frame delta scalar (frame-rate compensation). Modifiers:
   - **Terrain slow.** If the player's current collision tile has flag `0x4000` set and the scene control byte `_DAT_801c6ea4[+0x61] == 1`, `speed >>= 1` (half speed — mud / shallow water).
   - **Diagonal normalise.** Under camera mode 4 with both axes pressed, `speed -= speed >> 2` (×0.75).
5. **Step loop.** The function then loops, advancing **2 units per iteration** until `speed` units are consumed. Each iteration checks the candidate axis for collision and, only if clear, commits the step:

   ```text
   if (dir & 0x1000) and collide(player, scene, 2) == clear:  player.Z += 2;  dZ = +8
   else if (dir & 0x4000) and collide(player, scene, 0) == clear:  player.Z -= 2;  dZ = -8
   if (dir & 0x2000) and collide(player, scene, 3) == clear:  player.X += 2;  dX = +8
   else if (dir & 0x8000) and collide(player, scene, 1) == clear:  player.X -= 2;  dX = -8
   ```

   `collide` is `FUN_801cfe4c`; `dir`-codes are `0 = Z−, 1 = X−, 2 = Z+, 3 = X+`. The committed per-frame delta vector is stored at `_DAT_8007bde0` (X) / `_DAT_8007bde4` (Z) for the downstream transform-commit + camera follow. Step SFX is `func_0x80035b50(0x20)`; a fully-blocked move plays the bonk `func_0x80035bd0(0x23)`.

After movement, the same function runs an interaction probe (`FUN_801cf9f4`) to detect adjacency to talk-able actors when the action button is pressed. It walks the active-actor list, computes each actor's footprint box from its object record, and box-tests the point just ahead of the player against it (half-extent ≈ `0x40` + box + margin); on a hit it stores that actor in `player[+0x98]` (the interaction target) for the dispatch that runs the actor's interaction script.

### Runtime actor frame == MAN placement frame

The probe compares the player's position against each actor's `+0x14`/`+0x18` **directly** (no transform), so the player and the placed actors share one coordinate frame — and that frame is the MAN placement frame. `FUN_8003A1E4` spawns each partition-1 placement at `world = tile*128 + 0x40` (the `+0x40`/`+0x80` half-tile centre, i.e. the placement's [`world_x`](../formats/encounter.md)) and `FUN_80024C88` writes it straight into `actor[+0x14/+0x16/+0x18]` with **no anchor subtraction**. The player cold-spawn `0xA40` (2624) is exactly `tile 20 * 128 + 0x40`, so the player starts at MAN tile 20 in the same frame. (A live actor's position can still drift from its spawn tile if it patrols — a moving NPC reads at a different tile than its placement — but the frame is identical.)

The engine ports the probe as `World::tick_field_interaction_probe` (`engine-core`): it stores each talkable NPC's placement position (`World::field_npc_positions`, keyed by the same slot as the dialogue) and, on a just-pressed action button, opens the dialogue of the NPC within ±1 tile of the player via `World::trigger_field_interact` — and dismisses a probe-opened box on the next press (a `dialog_input_consumed` per-tick guard keeps it from racing the field VM's `0x4C` dialog poll). This is the input-driven counterpart to the scripted field-interact op; talking to the Rim Elm sparring partner this way starts the Tetsu fight through the dialogue-accept auto-arm.

`World::nav_step_toward(tx, tz, tol)` is the matching auto-navigation primitive: it steps the player one frame toward a world target using the same per-axis collision as the pad path (`advance_with_collision`) but a world-space direction, returning `true` on arrival. A driver loops it along a BFS route over the collision grid to walk the player to a target — e.g. the v0.1 oracle's emergent Battle leg walks from the cold-boot spawn to the sparring partner, then talks to it via the probe. (The partner's *placement* tile (76,65) is its post-tutorial village spot, in a town01 sub-area not walk-reachable from the spawn; the opening repositions it next to Vahn for the tutorial — see `RIM_ELM_SPARRING_CARRIER_TUTORIAL_POS`.)

## Collision — `FUN_801cfe4c`

`FUN_801cfe4c(player, scene, dir)` returns `0` when the move is clear and `2` when a static wall blocks it (plus bits `1`/`4` contributed by the finer `FUN_801cfc40` actor/edge probe). It samples a **per-scene collision tile map** through the base pointer `_DAT_1f8003ec`. The walkability grid lives at `*(_DAT_1f8003ec) + 0x4000`: **one byte per `128×128` world tile**, `0x80`-byte rows (up to `0x80` rows), the **high nibble** holding 4 sub-cell wall bits (the tile split into four `64×64` quadrants). A set bit = wall.

**Leading-edge footprint, not a centre point.** A direction is blocked if **any** of three probe points along the player's leading edge hits a wall sub-cell. The probe offsets are the per-direction table `DAT_801f2214` (16-byte stride; `dir` ∈ `{0=Z−, 1=X−, 2=Z+, 3=X+}`), three `(Δx, Δz)` pairs each. Decoded from the disc (overlay `0897` @ `0x801CE818`), the static-wall footprint is a row of three points **~47–48 units ahead** of the player centre in the travel direction, **spread ±16 laterally**:

| `dir` | leading-edge probes `(Δx, Δz)` (applied as `x+Δx`, `z−Δz`) |
|---|---|
| `0` Z− | `(−16,+48) (0,+48) (+16,+48)` → edge at `z−48`, ±16 in X |
| `1` X− | `(−47,−16) (−47,0) (−47,+16)` → edge at `x−47`, ±16 in Z |
| `2` Z+ | `(−16,−47) (0,−47) (+16,−47)` → edge at `z+47`, ±16 in X |
| `3` X+ | `(+48,−16) (+48,0) (+48,+16)` → edge at `x+48`, ±16 in Z |

For each probe the byte/sub-cell is derived as: `zc = (z>>6) + 2`, `xc = ((x + 0x3f) >> 6) − 1` (i.e. Z floored then **+2**, X **rounded up then −1**, with negative-coordinate corrections); byte index `= (xc/2 & 0x7f) + ((zc>>1) * 0x80) + 0x4000`; quadrant mask `= 1 << ((zc & 1)<<1 | (xc & 1))`. The `+2` (Z) and round-up/`−1` (X) push the half-tile-centred player (positions are `tile*128 + 64`) onto the **forward** tile, which is how the ~47-unit lateral lookahead lands a full tile ahead.

The finer actor/edge probe `FUN_801cfc40` uses a sibling table at `DAT_801f21b4` (64-unit lookahead + a 4th `±32` point) and contributes the `1`/`4` result bits. The sibling sampler `FUN_801d5718` reads the same `*(_DAT_1f8003ec) + 0x4000` grid with the identical nibble-and-mask shape, confirming the map layout.

**Engine model (clean-room).** [`World::field_tile_is_wall`] / [`World::advance_with_collision`] use a **single candidate-centre** point test (`sx = x>>6`, `sz = z>>6`, then `byte>>4 & (1<<((sz&1)<<1|(sx&1)))`) stepped incrementally, rather than retail's three leading-edge probes with the `+2`/`−1` sub-cell biases. Two facts are pinned from the disc, two are open:

- **The quadrant-mask formula is identical** to retail (verified byte-for-byte against the decomp's branchy `bVar5` for all four parities, `world.rs::tests`). The earlier "inverted X parity" worry is therefore **false** — the mask *selection* matches.
- **The sub-cell INDEX derivation differs.** Retail computes `zc = (z>>6)+2` and `xc = ((x+0x3f)>>6)−1`; the engine floors both (`z>>6`, `x>>6`). The `+2` applies to *every* sample (not just a look-ahead), so for the same world point retail indexes **one tile further in Z** and the **opposite X parity**: a half-tile-centred player at `(320, 448)` (tile-2/tile-3 centre) maps to retail `(col 2, row 4, quad 2)` but engine `(col 2, row 3, quad 3)`.
- **Open (capture-gated): whether that offset reads the WRONG wall in a live scene.** It depends on how the disc grid is authored relative to the world-tile origin (e.g. a 1-tile border row would mean the engine's floor indexing is off by a tile). town01 walks acceptably under coarse observation and the disc test only checks that the player stops at *some* base wall, so the offset is not yet proven to be a player-visible bug. Pinning it needs a town01 capture of the live player position against the `+0x4000` grid byte that blocks it (compare retail's `(col,row,quad)` to the engine's for the same position) **before** changing the working derivation.
- **Open (fidelity): the three-probe leading-edge footprint.** retail blocks when any of three body-edge points (~47 ahead, ±16 lateral) hits a wall; the engine tests one candidate centre. This is a standoff/feel difference layered on top of the indexing question.

## Where the collision grid comes from

`_DAT_1f8003ec` is the base of the **per-scene field buffer** (a scratchpad-resident pointer at `0x1F8003EC`). Its sub-regions:

| Offset from base | Content | Filled by |
|---|---|---|
| `+0x0000` | object / actor records (0x20-byte stride; up to 512) | scene loader / field VM |
| `+0x4000` | **collision + floor grid** — 1 byte/tile, `0x80`-byte rows: **high nibble** = 4 sub-cell wall bits, **low nibble** = floor-elevation tier | **base**: the `+0x4000..+0x8000` region of the per-scene `.MAP` file (`FUN_8001f7c0`); **deltas**: field-VM `0x4C` opcode, outer-nibble 7 |
| `+0x8000` | **per-tile object/attribute map** — `u16`/tile, `0x80`-byte rows: low 9 bits = object-record index into the `+0x0000` table, high bits = per-tile flags (bit `0x400` = object footprint) | object placement at scene load; bit `0x400` ORed in by `FUN_8003aeb0` from field-pack records |
| `+0x12000` | field-pack region; `_DAT_8007b8d0 = base + 0x12800` | `FUN_8001f7c0` (scene asset loader) |

### Collision byte: walls + floor height

Each `+0x4000` byte packs two nibbles for its 128-unit tile:

- **High nibble — walls.** Four sub-cell wall bits (the `2×2` quadrant grid the collision check samples; see above).
- **Low nibble — floor-elevation tier.** A 4-bit index `0..15` into a 16-entry `short` height LUT at scratchpad `0x1f80035c` (`= 0x1f800314 + 0x48`). The object/actor spawn iterator `FUN_8003a55c` reads `LUT[byte & 0xf]` and adds it to each placed object's Y, so a tile's collision byte also encodes its floor height (raised platforms, multi-level rooms). The LUT is filled at scene entry by `FUN_8003aeb0` from the MAN asset header (`_DAT_8007b898 + 2`, 16 negated `short`s). The same low nibble is **terrain elevation**: `FUN_80019278` (SCUS) bilinearly interpolates a smooth ground height from the 2×2 block of floor nibbles here — `grid[0],[1],[0x80],[0x81]`, weighted by the sub-tile position — so the world-map walk-view continent is a heightfield surface,
  not a flat plane (see [`world-map.md`](world-map.md), "the continent ground is a procedural heightfield"). The engine ports this bilinear height branch as `World::sample_field_floor_height(world_x, world_z)` (the per-scene LUT loaded into `World::field_floor_height_lut`; the `+0x8000` attribute branches are not reproduced). The pad locomotion path can follow it: with `World::follow_terrain_height` set (the `--terrain-y` play-window flag), each committed step snaps the player actor's `world_y` to this sample so the player rides slopes and steps. It is gated off by default so the flat-Y locomotion oracles keep their constant Y, and it is applied only on the field path — the world-map walk derives height from the continent grid through its own mechanism.

The **base wall + floor data is an on-disc blob**: it is the `+0x4000..+0x8000` region of the per-scene field map file (`DATA\FIELD\<scene>.MAP`), streamed into the field buffer at scene load by `FUN_8001f7c0` (see [Field-buffer load chain](#field-buffer-load-chain)). On top of that base, the field VM's `0x4C` (MENU_CTRL) opcode with outer-nibble 7 (`op0` ∈ `0x70..0x7F`, 7-byte op `[4C, 0x7s, b1, b2, b3, b4, mask]`) applies **story-conditional deltas** — a rectangular paint that sets/clears the high-nibble wall bits over a tile range (`col ∈ [b1, b3+1)`, `row ∈ [b2+1, b4+2)`; sub-op `s` = clear-walkable / block-all / clear-mask / set-mask), gated behind system-flag tests in the prescript.
The nibble-7 op is the same dispatch row in [`script-vm.md`](script-vm.md#0x4c-menu_ctrl---outer-nibble-dispatch).

The `+0x8000` map is a per-tile object/attribute word, not a terrain-flag grid: its low 9 bits index the `+0x0000` object-record table, which `FUN_8003a55c` walks at scene entry to spawn the NPCs/objects occupying each tile. `FUN_8003aeb0` (the field/town scene-entry map-init — note its `town_mode` / `baria_mode` debug strings) ORs the `0x400` footprint flag into these cells from the field-pack region records (`+0x12000`, offset/count at `+0x12006` / `+0x12008`, 4-byte records).

### Object-record format (`+0x0000`, 0x20-byte stride)

`FUN_8003a55c` reads each record at `field_buffer + idx*0x20` (the `.MAP` file's authored
copy; the runtime region is mutated). Decoded fields:

| Offset | Type | Meaning |
|---|---|---|
| `+0x00` | `i16` | X offset; `world_x = col*128 + this + 0x40` |
| `+0x02` | `i16` | Y offset added to the tile floor height (`heightLUT[grid_byte & 0xf]`) |
| `+0x04` | `i16` | Z offset; `world_z = row*128 - (this - 0x40)` |
| `+0x06` | `i8`  | footprint column delta to the anchor tile |
| `+0x07` | `i8`  | footprint row delta to the anchor tile |
| `+0x12` | `u16` | flags; bit `0x4` = placed/active, bit `0x800` ORs actor `+0x74` bit `0x10000000` |
| `+0x1e` | `u8`  | non-zero ORs actor `+0x74` bit `0x40000000` |

The anchor object is created by `FUN_80024c88(pos, …)` (writes `actor+0x14/16/18`), then
`FUN_8003a55c` writes `actor+0x60 = object_index` and copies record `+0x08/+0x0a/+0x0c`
into `actor+0x24/+0x26/+0x28`.

**This table is the static environment placement** — the visible terrain
segments, buildings, and props, *not* (only) NPC spawns. Each placed tile
allocates a static-object actor (shared tick fn `0x8003BC08`); the actor draws
its mesh from the [`scene_asset_table`](../formats/scene-bundles.md) TMD pack
through its `+0x44` mesh chain. (Validated against a live `town01` save: object
id `137` = Vahn's house, anchor tile `(col 38, row 25)` -> `world (4864, _, 3208)`,
matching the live actor; the near-zero `+0x08..+0x0c` fields are not the mesh
selector.) NPCs and event triggers ride
the same map via a sibling path (`FUN_8003a1e4`, partition-1 records, the
`0x7F,0x7F` parked-sentinel decode); the small actor pool note (about 8 slots,
`0xD8` stride, list heads `0x8007C34C..0x36C`, player `_DAT_8007c364`) refers to
that NPC/player set, while the static objects are a larger placed set.

Clean-room parser: [`legaia_asset::field_objects`](../../crates/asset/src/field_objects.rs)
(`parse_placements` + `pack_mesh_index` over the field map file); the engine
reads it via `Scene::field_object_placements`. Each object's drawn mesh comes
from the scene_asset_table TMD pack (byte-verified): `pack_index = obj_idx - 5`
for the field-actor band `93..=118`, otherwise the object record's `+0x10`
`u16` field; ids `1/2/3` are the protagonist/NPC meshes from the shared pool.
The `anim_id` resolved separately via the MAN script (`func_0x801d5630`) only
drives animation; it does not pick geometry. See
[`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

## Provenance

- Controller `FUN_801d01b0`, position writes `0x801D0684 / 06E4 / 0744 / 07B4` — see `ghidra/scripts/funcs/overlay_0897_801d0684.txt`.
- Collision `FUN_801cfe4c`, finer probe `FUN_801cfc40`, interaction `FUN_801cf9f4` — `ghidra/scripts/funcs/overlay_0897_801cfe4c.txt`.
- Pad remap `func_0x800467e8`, direction mask `FUN_80046494` — `ghidra/scripts/funcs/800467e8.txt` / `80046494.txt`.
- Scene-entry map-init `FUN_8003aeb0` (height LUT fill, `+0x8000` footprint OR, player-actor setup) — `ghidra/scripts/funcs/8003aeb0.txt`. Object spawn iterator `FUN_8003a55c` (low-nibble floor-height read, `+0x8000` index walk) — `ghidra/scripts/funcs/8003a55c.txt`.
- Runtime pin: `scripts/pcsx-redux/autorun_player_pos_watch.lua` (write-watchpoint on `*(0x8007c364) + 0x14/0x18`).

## Town / field parity

The controller is selected by **game mode**: mode `0x03` loads the field overlay (`overlay_0897`), which contains the single free-movement controller `FUN_801d01b0`. `FUN_801d01b0` was runtime-pinned on a walkable field scene (`map03`, mode `0x03`). Rim Elm — scene `town01` — also runs at game mode `0x03` (see `scripts/scenarios.toml`, the `v0_1_pre_battle_tetsu` anchor), so it loads the same overlay and the same controller. The shared scene-entry init `FUN_8003aeb0` corroborates this: it has an explicit `town_mode` debug-string branch and configures the same player actor (`_DAT_8007c364`: speed mult `+0x72 = 0x1000`, `+0x6a = 8`) for both towns and fields. So town locomotion is `FUN_801d01b0`, identical to the field.

The **overworld walk mode** shares it too. The world-map-walk overlay's locomotion is byte-for-byte the same `FUN_801d01b0` (same collision `FUN_801cfe4c`, same `_DAT_1f8003ec + 0x4000` grid); only the loaded overlay and grid contents differ. The three kingdom overworld scenes (`map01`/`map02`/`map03`) carry real wall data in that grid (≈ 7968 / 2283 / 3837 high-nibble wall sub-cells), so the overworld is bounded by the same tile-wall mechanism as towns — it is not a separate walkability format. See [`world-map.md`](world-map.md#overworld-collision--walkability).

## Engine port

The clean-room engine loads the base grid directly from the field map file. `SceneHost::enter_field_scene` resolves the `.MAP` entry via `Scene::field_map_index` — the unique CDNAME-block entry whose **extended on-disc footprint** is exactly `0x12000` bytes — and copies its `+0x4000..+0x8000` region into `World::field_collision_grid` (`World::load_field_collision_grid`). The grid byte format (high nibble = sub-cell wall bits, low nibble = floor-elevation tier) matches the runtime 1:1, so it copies verbatim; the field-VM `0x4C` nibble-7 hook then layers deltas on top as the prescript runs.

Footprint caveat: the TOC-**indexed** payload of the `.MAP` entry is only the first `0x4000` bytes (the object-record region); the collision grid and everything past it live in the entry's **trailing-gap sectors**, so the engine reads `ProtIndex::entry_bytes_extended`, not the indexed `SceneEntry::bytes`. Verified byte-exact: `town01`'s `entry10[0x4000..0x8000]` equals the live collision grid in a town01 save state (1297 wall tiles, zero diff). Disc-gated coverage: `crates/engine-core/tests/field_locomotion_disc.rs` (base grid non-empty on `town01` + `map03`, and the player stops at a real base wall).

### Environment geometry

A field/town scene's environment meshes (the terrain, buildings, and props) are Legaia TMDs packed inside **LZS streams of the scene_asset_table** PROT entry (`town01` = entry 4: 121 meshes, ≈8041 vertices). The clean-room `SceneResources` TMD pass scans each entry's LZS-decompressed sections in addition to its raw bytes (`tmd_scan::scan_entry`, the same path the TIM pass already used), so these meshes land in the scene TMD pool; the `scene_tmd_stream` skip still drops battle-character meshes in field mode. The field build uses `SceneLoadKind::Field` with `upload_all_tims`, matching retail's field loader (`FUN_8001f7c0`), which DMA-uploads every TIM — the environment meshes sample texture pages across the whole atlas, so a render-targeted upload drops most of their prims.
Per-mesh **world placement** for this static geometry is the [Object-record table](#object-record-format-0x0000-0x20-byte-stride) above (`FUN_8003a55c`: the object-index grid at `+0x8000` of the field map file selects a `+0x0000` object record per placed tile, giving the mesh its world translation; `legaia_asset::field_objects` parses it, `Scene::field_object_placements` exposes it). Each object's mesh resolves to a scene-pack index (`pack_index = obj_idx - 5` for the field-actor band `93..=118`, else the record's `+0x10` field). Per-tile **world Y** = `-floorHeightLUT[tile_nibble] + y_off`, the LUT being 16 `s16` at the MAN header `+0x02` (`Scene::field_floor_height_lut`). `legaia-engine play-window` renders the town from this:
`resolve_field_placement_draws` pairs each placement with its uploaded pack mesh + world transform (X/Z + floor-LUT Y) and draws them in `SceneMode::Field`.

### Scene-entry script

On entry the engine runs the scene's **scene-entry system script** (context channel `0xFB`), not event-script record 0. Record 0 of a per-scene event-script container is a trigger/dispatch table, not linear bytecode, so loading it as the field-VM buffer halts the VM at pc 0 and no entry logic runs. The retail per-frame driver `FUN_8003ab2c` builds the system script from the MAN asset's partition 1, first record; `Scene::field_man_entry_script` mirrors that resolve (`legaia_asset::man_section::ManFile::scene_entry_script` → `(start, pc0)`), and `SceneHost::enter_field_scene` loads the MAN slice from `start` with the VM PC at `pc0` (`World::load_field_script_at`). Slicing from the script start keeps the field VM's 16-bit-wrapping relative jumps anchored at the slice base,
matching the retail `buffer_base = script_start`.

Every field/town scene carries its MAN in a [`scene_asset_table`](../formats/scene-bundles.md): kingdom-bundle scenes use the `count = 7` form, and the early standalone towns (`town01` = Rim Elm, `town0c`, …) use a `count = 6` form in their block's 2nd PROT entry (e.g. `town01` = entry 4, MAN at descriptor 1). `find_bundle` resolves both, so `field_man_entry_script` runs the real entry script for all of them. The MAN source is pinned by a runtime write-watchpoint on `_DAT_8007B898`: the dispatcher `FUN_8001F05C` case 3 mallocs the buffer and LZS-decodes it from the table descriptor (see [`scene-bundles.md`](../formats/scene-bundles.md)). The base collision grid (loaded from the `.MAP` above) is independent of which entry script runs;
the entry script's `0x4C` nibble-7 wall-paint deltas are gated behind system-flag tests and only fire once the world's story flags are seeded to a matching scene-entry state. Disc-gated coverage asserts the MAN-backed scenes' field VM advances past pc 0 (`town01`: 65, `map03`: 61 distinct PCs, settling into a per-frame loop).

#### Story-conditional wall deltas (map03)

Tracing `map03`'s entry script pins the gate flags directly: `TEST` flag `0x6C2` (at script offset `0x2c`) routes into a sub-1 "block all" paint over tile (col 66, row 102), and `TEST` flag `0x378` (at `0x4f`) routes into a contiguous three-paint cluster (sub-0 "clear walls" at offsets `0x56` / `0x5c` / `0x62`). At a fresh boot both flags are clear, so the entry script skips all four paints and the grid stays at its disc-loaded base — which is correct: these are story-conditional terrain changes, not the base walls. Seeding the matching system flags (in real gameplay, loading a save whose story-flag block has them set) makes the paints fire. The flag-bank base is `0x80085758` (= SC offset `0x1618`); see [`script-vm.md`](script-vm.md#0x4c-menu_ctrl---outer-nibble-dispatch).
Disc-gated coverage: `crates/engine-core/tests/map03_conditional_walls_disc.rs` (with flag `0x6C2` seeded, the wall at tile (66, 102) appears; without it, it does not).

The nibble-7 paint format (retail handler `0x801e1c64`): the **row** range is `[row0+1, row1+2)`, and sub-0/1 paints are **6-byte** ops with no mask byte while sub-2/3 are 7-byte.

### Scene encounter table

The same MAN that supplies the entry script also carries the scene's **random-encounter table** in its section 0 (`FUN_8003AEB0` installs it into the runtime control block `_DAT_801C6EA4 + 0x20`; see [`encounter.md`](../formats/encounter.md) and [`man-section`](../formats/scene-bundles.md)). Because the `count = 6` detector now resolves the standalone towns' MAN, the field scene-entry path can pull the disc-resident table for them too. `Scene::field_man_encounter_table` resolves the MAN through `find_bundle`, decodes the encounter section via `legaia_engine_core::encounter_man::scene_encounter_from_man`, and `SceneHost::enter_field_scene` installs it (`World::install_man_encounter`): the per-formation rows become `EncounterEntry`s keyed by row index,
and the matching `FormationDef`s (row index → monster-id slots) are merged into the formation table so a triggered encounter resolves to a concrete monster set. The MAN carries formation monster-ids but not stat blocks, so the host installs the stat catalog separately; scenes whose bundle has no MAN keep the synthetic-pattern `EncounterRegistry` fallback.

Towns carry random encounters too: `town01`'s MAN encounter section declares **7 formations** at a low mean trigger rate (`6/256`), gated by its region records, overriding the synthetic-registry fallback. Disc-gated coverage: `crates/engine-core/tests/field_man_encounter_disc.rs` (boots `town01` / `town0c` / `map03`, asserts each installs a MAN encounter session whose row-index `formation_id`s all resolve to merged formation defs).

### Per-step encounter roll in the live loop

When `World::live_gameplay_loop` is set, locomotion feeds the encounter system directly: `World::live_field_tick` treats the player crossing into a new 128-unit collision tile (`pos >> 7`) as one *step* and drives a single `World::on_field_step` roll, mirroring the retail per-step counter rather than rolling every frame. A successful roll transitions `Field → Battle`; on victory the field actor table is restored and the player resumes where they stood. See the [live gameplay loop](battle.md#live-gameplay-loop--field--battle-in-tick) section in `battle.md` for the full round trip.

### Input is locked during an opening-cutscene timeline

`World::step_field_locomotion` is gated on `current_dialog`, an active tile-board, the per-actor movement-disabled flag (`move_state.flags & 0x0008_0000`), **and** an active opening-cutscene timeline (`World::cutscene_timeline_active`). During the `town01` opening's establishing sweep the spawned [cutscene timeline](cutscene.md) drives the lead actor through its own MoveTo ops, so the pad must not also walk the player out from under the cinematic camera. Control returns the frame the timeline drops — matching retail, where free-roam input is accepted only after the opening choreography ends.

## Field-buffer load chain

The base wall + floor grid is **streamed from disc**, not script-authored: it is the leading region of a multi-sector CD read issued at scene load. A runtime write-watchpoint on the live grid (`_DAT_1f8003ec + 0x4000`) during a Drake-Castle → Drake-world-map transition caught one bulk writer — the CD-DMA channel-3 read primitive **`FUN_8005D9A0`** (DMA store at `0x8005DA50`), reached via the wrapper `FUN_8005C2C4` from the per-sector streaming poller **`FUN_8003EF14`**. The poller DMAs one 2048-byte CD sector per ready-IRQ into the destination cursor at `gp + 0x940` (= `0x8007BC58`, holding `_DAT_1f8003ec + 0x4000`), advancing `0x800` per sector. So the field buffer — collision grid (`+0x4000`), object map (`+0x8000`), field-pack (`+0x12000`) — is the leading region of that streamed read.
Across the transition the grid jumped 2093 → 6805 wall tiles while only **6** nibble-7 CPU-store writes fired, confirming the bulk arrives as disc sectors and the field-VM `0x4C` nibble-7 ops are conditional deltas layered on afterward.

`FUN_8001f7c0(dest, scene_name, field_record)` fills the field buffer at `dest` (the `_DAT_1f8003ec` base). Two transports converge on shared streaming machinery:

- *Retail*: builds `DATA\FIELD\<scene>.MAP`, opens it by ISO9660 name (`FUN_800608f0`), streams into `dest` via `FUN_8003e6bc`.
- *Debug* (`_DAT_8007b8c2 != 0`): `FUN_8003e8a8(field_record, 1)` sets the `CdlLOC` at `0x8007bc5c` from the in-RAM PROT TOC (`target_sector = CdPosToInt(base_loc@0x8007bc50) + toc[field_record + 2]`, the documented `start_lba = toc[p+2]`); `FUN_8003e800(dest, 0x28, 1)` issues a 40-sector (`0x14000`-byte) read.
- Shared core: `FUN_8003e800` → `FUN_8003f128` (copies dest/count into `gp+0x940`/`gp+0x968`, issues `CdControl(CdlSetloc)`, registers the data-ready callback) → `FUN_8003EF14` per-sector poller → `FUN_8005C2C4` → `FUN_8005D9A0`. The same generic entry serves other clients (`FUN_8003e104` = the `monster_snd` pack loader), so `FUN_8003e800`/`FUN_8003f128` are shared streaming infrastructure. See [`boot.md`](boot.md) for the CD-read API.

**For the engine, base collision is a load step, not a script step**: slice bytes `0x4000..0x8000` of the per-scene `.MAP` file; no script execution is needed for the base walls. The nibble-7 ops ride the scene's field-VM scripts — which run multi-context at load (`FUN_8003aeb0` scene-entry init → `FUN_8003ab2c` MAN system-script runner, the `0xFB` system context being the conditional-delta painter) — and only matter for story-conditional terrain changes.

## Open

- `FUN_801cfc40` (the finer actor/edge collision probe contributing bits `1`/`4`) is not fully decoded.

## See also

**Reference** —
[Field/event VM](script-vm.md) ·
[World map](world-map.md) ·
[Scene bundles](../formats/scene-bundles.md) ·
[Scene v12 table](../formats/scene-v12-table.md)
