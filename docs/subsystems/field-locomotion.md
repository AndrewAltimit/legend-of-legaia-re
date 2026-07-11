# Field free-movement locomotion

The player free-movement controller for normal towns, dungeons, and walkable field areas is **`FUN_801d01b0`** in the field overlay (`overlay_0897`). Each frame it reads the held pad, turns it into a camera-relative direction, advances the player actor's world position a fixed step per sub-frame with per-axis collision, and updates the player's facing angle. This is the general locomotion path - **not** the [tile-board grid mode](tile-board.md) (a puzzle / board minigame that happens to live in the same overlay).

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

World coordinates are plain `s16` in 1-unit resolution; one collision tile is `0x80` (128) units (see below). The field camera derives its origin by negating these - see [`world-map.md`](world-map.md) and the camera notes in [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

**Probe trap - read these as 16-bit, not `u32`.** `+0x14` (X), `+0x16` (facing), and `+0x18` (Z) are adjacent `s16` fields, so a 32-bit read of `+0x14` folds the facing word into the X high half and a 32-bit read of `+0x18` folds the next word into the Z high half. A headless nav probe that read them as `u32` measured the *facing* as position drift and (wrongly) concluded the camera-to-pad mapping was "dynamic"; reading `s16` shows the per-room camera is static and the pad maps to world consistently. See the [S4 grid-BFS capture](../tooling/playthrough-coverage.md#s4-captured-the-grid-bfs-door-nav-walks-out-of-vahns-house).

## Spawn position on scene entry

The player actor's spawn position is set by the per-scene initializer `FUN_801D6704` (MAIN_INIT), not by the locomotion controller. There are two cases, selected by the field-entry mode global `_DAT_8007b8b8`:

- **Cold entry (`_DAT_8007b8b8 == 0`).** The player actor is created at actor coords **`(0xA40, 0, 0xA40)`** - the centre of the camera's `0x20`-tile view window - via `func_0x80024c88(&local_68=…)`, which writes `actor+0x14 = sVar13 + 0xA40`, `actor+0x16 = 0`, `actor+0x18 = sVar14 + 0xA40`. On a cold entry the sub-tile terms `sVar13`/`sVar14` are zero, so the spawn is the fixed window centre. The camera itself is seeded onto the MAN anchor (`local_60`/`local_5e`, filled by `FUN_8003AEB0`), then the follow camera tracks the player. **Cold entry only ever happens for the New Game opening scene (`town01`, Rim Elm)** - every other scene change is a warp - so `(0xA40, 0xA40)` is effectively Vahn's authored opening spawn. Byte-checked walkable against town01's base collision grid.
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
   - **Terrain slow.** If the player's current collision tile has flag `0x4000` set and the scene control byte `_DAT_801c6ea4[+0x61] == 1`, `speed >>= 1` (half speed - mud / shallow water).
   - **Diagonal normalise.** Under camera mode 4 with both axes pressed, `speed -= speed >> 2` (×0.75).
5. **Step loop.** The function then loops, advancing **2 units per iteration** until `speed` units are consumed. Each iteration checks the candidate axis for collision and, only if clear, commits the step:

   ```text
   if (dir & 0x1000) and collide(player, scene, 2) == clear:  player.Z += 2;  dZ = +8
   else if (dir & 0x4000) and collide(player, scene, 0) == clear:  player.Z -= 2;  dZ = -8
   if (dir & 0x2000) and collide(player, scene, 3) == clear:  player.X += 2;  dX = +8
   else if (dir & 0x8000) and collide(player, scene, 1) == clear:  player.X -= 2;  dX = -8
   ```

   `collide` is `FUN_801cfe4c`; `dir`-codes are `0 = Z−, 1 = X−, 2 = Z+, 3 = X+`. The committed per-frame delta vector is stored at `_DAT_8007bde0` (X) / `_DAT_8007bde4` (Z) for the downstream transform-commit + camera follow. The step loop plays **no SFX** - walking and wall contact are silent. (An earlier note here read the controller's `0x20`/`0x23` cues as "step"/"bonk" sounds; they actually fire in the pre-movement header - `0x20` on the action-button accept and the menu-open accept, `0x23` as the deny buzz when the menu-open is refused under the `_DAT_1f800394 & 0x8000000` lock.)

After movement, the same function runs an interaction probe (`FUN_801cf9f4`) to detect adjacency to talk-able actors when the action button is pressed. It walks the active-actor list, computes each actor's footprint box from its object record, and box-tests the point just ahead of the player against it (half-extent ≈ `0x40` + box + margin); on a hit it stores that actor in `player[+0x98]` (the interaction target) for the dispatch that runs the actor's interaction script.

### Runtime actor frame == MAN placement frame

The probe compares the player's position against each actor's `+0x14`/`+0x18` **directly** (no transform), so the player and the placed actors share one coordinate frame - and that frame is the MAN placement frame. `FUN_8003A1E4` spawns each partition-1 placement at `world = tile*128 + 0x40` (the `+0x40`/`+0x80` half-tile centre, i.e. the placement's [`world_x`](../formats/encounter.md)) and `FUN_80024C88` writes it straight into `actor[+0x14/+0x16/+0x18]` with **no anchor subtraction**. The player cold-spawn `0xA40` (2624) is exactly `tile 20 * 128 + 0x40`, so the player starts at MAN tile 20 in the same frame. (A live actor's position can still drift from its spawn tile if it patrols - a moving NPC reads at a different tile than its placement - but the frame is identical.)

The engine ports the probe as `World::tick_field_interaction_probe` (`engine-core`): it stores each talkable NPC's placement position (`World::field_npc_positions`, keyed by the same slot as the dialogue) and, on a just-pressed action button, runs the retail facing probe (`World::field_interact_probe_slot` - the `DAT_801f2254` radius-64 compass point ahead of the facing, ±72 box), opens the matched NPC's dialogue via `World::trigger_field_interact`, and turns the player toward it (`World::face_field_npc`) - then dismisses a probe-opened box on the next press (a `dialog_input_consumed` per-tick guard keeps it from racing the field VM's `0x4C` dialog poll).
This is the input-driven counterpart to the scripted field-interact op; talking to the Rim Elm sparring partner this way starts the Tetsu fight through the dialogue-accept auto-arm.

`World::nav_step_toward(tx, tz, tol)` is the matching auto-navigation primitive: it steps the player one frame toward a world target using the same per-axis collision as the pad path (`advance_with_collision`) but a world-space direction, returning `true` on arrival. A driver loops it along a BFS route over the collision grid to walk the player to a target - e.g. the v0.1 oracle's emergent Battle leg walks from the cold-boot spawn to the sparring partner, then talks to it via the probe. (The partner's *placement* tile (76,65) is its post-tutorial village spot, in a town01 sub-area not walk-reachable from the spawn; the opening repositions it next to Vahn for the tutorial - see `RIM_ELM_SPARRING_CARRIER_TUTORIAL_POS`.)

## Collision - `FUN_801cfe4c`

`FUN_801cfe4c(player, scene, dir)` returns `0` when the move is clear and `2` when a static wall blocks it (plus bits `1`/`4` contributed by the finer `FUN_801cfc40` actor/edge probe). It samples a **per-scene collision tile map** through the base pointer `_DAT_1f8003ec`. The walkability grid lives at `*(_DAT_1f8003ec) + 0x4000`: **one byte per `128×128` world tile**, `0x80`-byte rows (up to `0x80` rows), the **high nibble** holding 4 sub-cell wall bits (the tile split into four `64×64` quadrants). A set bit = wall.

**Leading-edge footprint, not a centre point.** A direction is blocked if **any** of three probe points along the player's leading edge hits a wall sub-cell. The probe offsets are the per-direction table `DAT_801f2214` (16-byte stride; `dir` ∈ `{0=Z−, 1=X−, 2=Z+, 3=X+}`), three `(Δx, Δz)` pairs each, taken at the player's **pre-step** position. Disc-pinned (overlay `0897` @ `0x801CE818`, file offset `0x239FC`), the static-wall footprint is a row of three points **47–48 units ahead** of the player centre in the travel direction, **spread ±16 laterally** - 48 in the positive directions and 47 in the negative ones, the per-direction crossing distance under the biased cell mapping below. (Each on-disc row carries a fourth half-distance centre pair the wall probe never reads.)

| `dir` | leading-edge probes `(Δx, Δz)` (applied as `x+Δx`, `z−Δz`) |
|---|---|
| `0` Z− | `(−16,+48) (0,+48) (+16,+48)` → edge at `z−48`, ±16 in X |
| `1` X− | `(−47,−16) (−47,0) (−47,+16)` → edge at `x−47`, ±16 in Z |
| `2` Z+ | `(−16,−47) (0,−47) (+16,−47)` → edge at `z+47`, ±16 in X |
| `3` X+ | `(+48,−16) (+48,0) (+48,+16)` → edge at `x+48`, ±16 in Z |

For each probe the byte/sub-cell is derived as: `zc = (z>>6) + 2`, `xc = ((x + 0x3f) >> 6) − 1` (i.e. Z floored then **+2**, X **rounded up then −1**, with negative-coordinate corrections); byte index `= (xc/2 & 0x7f) + ((zc>>1) * 0x80) + 0x4000`; quadrant mask `= 1 << ((zc & 1)<<1 | (xc & 1))`. The `+2` (Z) and round-up/`−1` (X) push the half-tile-centred player (positions are `tile*128 + 64`) onto the **forward** tile, which is how the ~47-unit lateral lookahead lands a full tile ahead.

**Actor-collision probes (`FUN_801cfc40`, result bits `1`/`4`).** Before the wall probes, `FUN_801cfe4c` runs three calls to `FUN_801cfc40` with the `(Δx, Δz)` pairs of the **sibling table `DAT_801f21b4`** (same 16-byte per-direction stride, file offset `0x2399C`, same `x+Δx`/`z−Δz` application; the 4th half-distance pair is unread here too). The actor sweep is wider than the wall edge - **64 ahead in the positive directions / 63 in the negative ones, spread ±32 laterally** - because actors block with a body box, not a sub-cell edge:

| `dir` | actor probes `(Δx, Δz)` |
|---|---|
| `0` Z− | `(−32,+64) (0,+64) (+32,+64)` |
| `1` X− | `(−63,−32) (−63,0) (−63,+32)` |
| `2` Z+ | `(−32,−63) (0,−63) (+32,−63)` |
| `3` X+ | `(+64,−32) (+64,0) (+64,+32)` |

`FUN_801cfc40(actor, scene, Δx, Δz, ex, ez)` walks the **active-actor pointer table** `DAT_801c93c8` (count `_DAT_8007b6b8`) and box-tests the probe point against each other actor.
A **static entity** (`flags+0x10 & 0x1020000 == 0`) anchors at its **MAN object record** (`_DAT_1f8003ec + rec_idx[+0x60]*0x20`; anchor `= tile*128 + sub*16` from record bytes `+6`/`+7` and `+0xE`/`+0xF`, with a `flags+0x52 & 8` offset correction from record halfwords `+0`/`+4`) plus the actor's live `+0x14`/`+0x18`, and blocks within `±(0x40+0x10)` = **80 units** per axis (strict).
A **moving actor** uses its live position with caller extents `±(0x40 + ex−0x18)` (the locomotion passes `ex = ez = 0` → ±40).
A hit links the pair mutually at `+0x98`, posts `func_0x8003d038(other[+0x50])`, and contributes result bit `1` (`flags & 0x40020000` class - moving NPC/event actor) or `4` (static prop). When the actor table is full (`_DAT_8007b6b8 == 0x20`) the whole call delegates to the `FUN_801cf9f4` box-test variant.

**The locomotion gates a step on the actor bits and the wall bit together**: `FUN_801d01b0` commits each 2-unit axis step only when `FUN_801cfe4c` returns `0` (or the debug no-clip `_DAT_8007b98c`/`_DAT_8007b850 & 2` is on) - NPCs block movement exactly like walls.

Each sub-step it also runs the **touch/interact dispatch** (gated off while the player's `+0x10 & 0x80000` engaged flag, the scratch system-channel `_DAT_1f800394 & 0x400`, or the field-control dialog byte `_DAT_801c6ea4+0x62` is set):

- **Prop walk-touch is automatic**: a step whose probe result carries bit `4` (static entity) posts the touched entity's event on the spot - `FUN_801d5b5c` on the `+0x98` partner, every contact step, no button needed.
- **NPC interaction is button-gated**: with no bit `4`, and only when the configured interact button is **just-pressed** (`_DAT_8007b874 & _DAT_800846d0` - the assignable confirm mask from the `0x800846xx` input-config block), it runs one more facing-indexed probe: a third table **`DAT_801f2254`** (overlay file `0x23A3C`, one `(Δx, Δz)` pair per 45° facing sector, `sector = (facing & 0xfff) >> 9`) supplies a single **radius-64 compass point ahead of the player**, box-tested through `FUN_801cf9f4` with extents `0x20` (NPC box widens to `0x40+0x20−0x18` = ±72).
A bit-`1` hit posts the touch event (`FUN_801d5b5c` on the `+0x98` partner), turns the player toward the touched actor when the partner is a plain moving-class actor (`flags & 0x20010 == 0x20000`; `func_0x80019b28` arctan-LUT angle from the partner's position into player `+0x26`), and raises the field-control interact flag `_DAT_801c6ea4+0x60 = 1`.

| sector (facing) | `(Δx, Δz)` | probe point `(x+Δx, z−Δz)` |
|---|---|---|
| 0 (`0` = Z−) | `(0, +64)` | 64 ahead in Z− |
| 2 (`0x400` = X−) | `(−64, 0)` | 64 ahead in X− |
| 4 (`0x800` = Z+) | `(0, −64)` | 64 ahead in Z+ |
| 6 (`0xC00` = X+) | `(+64, 0)` | 64 ahead in X+ |
| odd | `(±64, ±64)` | diagonals |

**`FUN_801d5b5c` (the touch event post, decoded from a live overlay image - the static `overlay_0897` copy is garbled in this region)** marks the engagement: player `flags |= 0x80000` (the same bit that suppresses locomotion input at the top of `FUN_801d01b0`), touched actor `flags |= 0x100`, actor touch counter `+0x2a += 1`, field-control event counter `_DAT_801c6ea4+0xA += 1`, the actor's current facing `+0x26` saved into `+0x5A` (restored when the interaction ends), then `FUN_8003c9ac` - which sweeps the scene actor list and reloads every moving-class actor's `+0x5C`/`+0x88` timer from the per-actor byte table at `0x801C6470` (an NPC-motion pause kick while the interaction runs).
The **teardown** is the dialog SM's exit path (`FUN_80039b7c`): it restores the actor's facing `+0x26` from the `+0x5A` save (moving-class partners), subtracts the actor's `+0x2A` touch counter out of the field-control global `+0xA`, and when the global reaches zero clears the player's `0x80000` engaged flag and `ctrl+0x60` - so overlapping touches keep locomotion suppressed until every one is dismissed.
(The separate sampler `FUN_801d5718` reads the same `*(_DAT_1f8003ec) + 0x4000` grid with the identical nibble-and-mask shape, confirming the map layout.)

**The static-entity anchor decodes against the `.MAP` object records.** A static actor's box centre is its live position plus a **collision-footprint offset** from its object record (`actor[+0x60]` indexes the `+0x0000` record table): `off = (rec[+6]·0x80 + rec[+0xE]·0x10, rec[+7]·0x80 + rec[+0xF]·0x10)`, and when the actor's `+0x52 & 8` is set (mirrored at spawn from record flag bit `0x8`) further corrected by `(−x_off, +z_off)` (record halfwords `+0`/`+4`).
Live-verified against the spawned static collision actors of four catalogued captures (town01 records 315 + 137 - the latter the correction arm - town0c 331, koin3 116): the live actor position equals the placement spawn position and the live-computed centre equals the disc-computed one (`engine-shell/tests/field_prop_colliders_live.rs`).

**Engine model (clean-room).** [`World::field_tile_is_wall`] samples with **retail's exact sub-cell derivation** (`zc = (z>>6)+2`, `xc = ((x+0x3f)>>6)−1`, quad `(zc&1)<<1|(xc&1)`); [`World::advance_with_collision`] steps incrementally and blocks each axis either on a **single candidate-centre** test (default, kept for the locomotion oracles + BFS nav drivers) or on **retail's three-probe leading-edge footprint** (`World::field_dir_blocked` over the `DAT_801f2214` table, opt-in via `World::leading_edge_wall_probes` / `play-window --edge-collision`) - under the footprint the player rests 47–48 units off the wall plane exactly like retail.
The derivation and the footprint rest positions are pinned by two cheat-free Rim Elm wall-press captures (scenarios `rimelm_wall_press_left` / `rimelm_wall_press_down`; disc-gated `engine-shell/tests/field_collision_discriminator.rs`):

- **The quadrant-mask formula is identical** to retail (verified byte-for-byte against the decomp's branchy `bVar5` for all four parities, `world.rs::tests`). The earlier "inverted X parity" worry is **false**.
- **The `+2` Z bias is AUTHORED INTO THE WALL BITS - plain floor indexing is refuted.** In the down-press capture (screen-down = world `Z−`, toward the camera) the player legally rests at `(3386, 2606)` whose plain floor-indexed cell `(26, 20)` is an **all-quads wall byte** - under floor indexing the position would be unreachable. Under the biased read that wall byte covers world `z ∈ [2432, 2560)`, one tile north, exactly where the `Z−` leading-edge probe (`z−48 = 2558`) blocks with a step-exact standoff (blocked while `z ≤ 2607`, player rests at exactly 2606 on the even step parity).
- **A wall byte's two nibbles live under two different world→cell mappings.** The **floor sampler** (`FUN_80019278`, low/elevation nibble) indexes the *same* grid bytes with plain floor (`>>6`, then `>>1`, no bias) - confirmed from its decomp - while the **wall probe** (`FUN_801cfe4c`, high nibble) applies the `+2`/`ceil−1` bias. The engine mirrors both: `sample_field_floor_height` floors, `field_tile_is_wall` biases. A consequence: grid **row 0's wall bits are unreachable** for `z ≥ 0` (the bias maps it to negative z).
- **X alignment + the 47-unit standoff validated live (left press).** The clean left-press capture rests at `(1838, 2526)` against the full-height wall column at grid col 13: the probe `x−47 = 1791` reads the column's last wall sub-cell, one 2-unit step shallower reads clear. In X, retail's `ceil−1` equals the floor everywhere except exact 64-multiples - a divergence the even step parity never reaches.
- **The three-probe footprint is wired and rest-validated.** With `leading_edge_wall_probes` set, driving the engine stepper over each capture's **live grid** from a shallow start reproduces the captured retail rest position **byte-exactly** (left press rests at `x = 1838`, down press at `z = 2606`), while the candidate-centre default demonstrably walks deeper (`field_collision_discriminator.rs`, the `*_engine_rest_matches_retail` legs).
- **The full scene context reproduces the standoff too.** The `*_full_scene_rest_matches_retail` legs press the same walls inside a real `BootSession::enter_field_live` scene entry - the resolver-loaded `.MAP` grid plus the engine-executed prescript paints, walked through the pad -> camera-remap -> `step_field_locomotion` path - and rest at the captured retail positions byte-exactly.
- **The actor-collision arm is modelled too - and capture-classed.** `World::field_actor_dir_blocked` ports `FUN_801cfc40`'s **moving-actor arm** (result bit `1`): the three `DAT_801f21b4` probes box-tested against the NPC positions (`World::field_npc_positions`) with the **±40-unit** box (`0x40` core minus the locomotion's `0x18` extent bias), gated behind `World::solid_field_npcs` / `play-window --solid-npcs` - NPCs become solid, resting 102 units short of an NPC head-on (unit-pinned in `world.rs::tests`).
  The class is **capture-pinned by `rimelm_npc_press_tetsu`**: a live state with the player pressed into the sparring partner shows the mutual `+0x98` collision link active in-frame both ways and Tetsu's `flags+0x10 = 0x08020884` carrying the `0x20000` moving-class bit - village NPCs take the bit-1 arm, not the static prop arm. The disc-gated `npc_press_pins_moving_actor_arm` leg asserts the link, the class, and that the engine probe refuses the captured press direction while the stepper holds the captured rest.
- **The static prop arm (bit `4`) is modelled from the `.MAP` placements.** `Scene::field_object_placements` already returns exactly the collision-actor spawns (the placed flag `0x4` *is* the spawn gate; the numerous flag-`0x11/0x12/0x13` records are the terrain layer), and each placement now carries its `collider_x`/`collider_z` box centre (spawn position + the record's collision-footprint offset, `legaia_asset::field_objects::collision_footprint_offset`). `World::field_prop_colliders` installs them at field entry; `field_actor_dir_blocked` box-tests the same three probes against them at the static ±80 half-extent - a head-on press rests 142 units short of a prop centre (same pre-step parity as the NPC arm's 102). Gated behind the same `World::solid_field_npcs` / `--solid-npcs` flag.
- **The button-press interact dispatch is modelled faithfully.** `World::field_interact_probe_slot` ports the `DAT_801f2254` facing probe (the radius-64 compass point, ±72 interact box); a hit opens the NPC's dialogue and turns the player toward it (`World::face_field_npc`, the face-the-NPC step - shape-faithful float `atan2` rather than retail's arctan LUT). The engine's field heading stores `0` = Z+ where retail facing stores `0` = Z− (a Z+ walk writes `0x800` to `+0x26`), so the sector index adds a half-turn before quantising. The captured Tetsu press-rest position talks to him through this probe (`world.rs::tests::interaction_probe_matches_tetsu_capture_geometry`).
- **Field-NPC motion is modelled through the motion VM.** Each talk NPC's placement script carries its authored walk legs as `0x4C 0x51` NPC move-to-tile ops; `man_field_scripts::placement_motion_route` decodes the local waypoints and `World::tick_field_npc_motions` drives them through the ported motion VM (`FUN_8003774C`), one pursue step per field tick, writing the live position back into `World::field_npc_positions` - so the moving NPC's ±40 collision box and its interact box follow it, exactly as retail probes the live `+0x14`/`+0x18`.
  Autonomous patrol is opt-in (`World::animate_field_npcs` / `play-window --live-npcs`) and pauses while a dialogue is up (the retail interaction motion-pause kick); an interaction prologue's own `0x4C 0x51` runs the interacted NPC through the same kernel regardless of the flag. See [`motion-vm.md`](motion-vm.md#field-npc-walking). Disc-gated: `engine-core/tests/field_npc_motion_disc.rs` (town01 derives routes for many villagers; the engine walks them off-anchor; the collision box follows).
- **The prop walk-touch event post is modelled for the decoded script classes.** `man_field_scripts::placement_walk_touch_event` classifies each non-parked placement's script: a genuine `0x3E` door-warp (`Warp`) or a cross-context `0x23` into the player channel `0xF8` (`PlayerMoveTo` - the cave-guard throw-back / intra-scene teleport). `World::check_field_walk_touch` runs on the locomotion step: standing inside the placement's static ±80 contact box posts once per contact through the same `trigger_field_interact` dispatch the button-gated interact uses (surfacing a `FieldInteract` event - the engine analogue of the `FUN_801d5b5c` auto post on the `+0x98` partner) and applies the decoded effect (queue the door-warp transition / snap the player).
  Not modelled: the full post kernel (engaged flag, facing save/restore, touch counters) and prop scripts beyond those two decoded classes. Disc-gated: `engine-core/tests/field_walk_touch_disc.rs` (koin1 mine-exit warps; cave01 guard throw-backs).

Capture note: both wall-press captures park in the **`town0c`** Rim Elm variant. The live grid byte-matches the town01 map's base + paints - which is exactly what a town0c session *should* hold: under the universal `define−2` `.MAP` resolution (see "Engine port" below) town0c's own `.MAP` is PROT 0019, **byte-identical** to town01's (0001/0010 - the Rim Elm variants share one map). The earlier reading that PROT 0028 was "town0c's own different `.MAP`" mis-attributed the next block's map (0028 is `izumi`'s, `define 30 − 2`); the cold-vs-variant question this raised is dissolved.

## Where the collision grid comes from

`_DAT_1f8003ec` is the base of the **per-scene field buffer** (a scratchpad-resident pointer at `0x1F8003EC`). Its sub-regions:

| Offset from base | Content | Filled by |
|---|---|---|
| `+0x0000` | object / actor records (0x20-byte stride; up to 512) | scene loader / field VM |
| `+0x4000` | **collision + floor grid** - 1 byte/tile, `0x80`-byte rows: **high nibble** = 4 sub-cell wall bits, **low nibble** = floor-elevation tier | **base**: the `+0x4000..+0x8000` region of the per-scene `.MAP` file (`FUN_8001f7c0`); **deltas**: field-VM `0x4C` opcode, outer-nibble 7 |
| `+0x8000` | **per-tile object/attribute map** - `u16`/tile, `0x80`-byte rows: low 9 bits = object-record index into the `+0x0000` table, high bits = per-tile flags (bit `0x400` = object footprint) | object placement at scene load; bit `0x400` ORed in by `FUN_8003aeb0` from field-pack records |
| `+0x10000` | **trigger block** - shared header + four kind sub-tables (teleports / P2-record triggers / elevation overrides / region AABBs; see below) | the `+0x10000..+0x12000` region of the `.MAP` file (`FUN_8001f7c0`) |
| `+0x12000` | field-pack region; `_DAT_8007b8d0 = base + 0x12800`; also the trigger lookup's **fallback window** (same header shape - see below) | `FUN_8001f7c0` (scene asset loader) |

### Collision byte: walls + floor height

Each `+0x4000` byte packs two nibbles for its 128-unit tile:

- **High nibble - walls.** Four sub-cell wall bits (the `2×2` quadrant grid the collision check samples; see above).
- **Low nibble - floor-elevation tier.** A 4-bit index `0..15` into a 16-entry `short` height LUT at scratchpad `0x1f80035c` (`= 0x1f800314 + 0x48`). The object/actor spawn iterator `FUN_8003a55c` reads `LUT[byte & 0xf]` and adds it to each placed object's Y, so a tile's collision byte also encodes its floor height (raised platforms, multi-level rooms). The LUT is filled at scene entry by `FUN_8003aeb0` from the MAN asset header (`_DAT_8007b898 + 2`, 16 negated `short`s). The same low nibble is **terrain elevation**: `FUN_80019278` (SCUS) bilinearly interpolates a smooth ground height from the 2×2 block of floor nibbles here - `grid[0],[1],[0x80],[0x81]`, weighted by the sub-tile position - so the world-map walk-view continent is a heightfield surface,
  not a flat plane (see [`world-map.md`](world-map.md), "the continent ground is a procedural heightfield"). The engine ports this bilinear height branch as `World::sample_field_floor_height(world_x, world_z)` (the per-scene LUT loaded into `World::field_floor_height_lut`; the `+0x8000` attribute branches are not reproduced). The pad locomotion path can follow it: with `World::follow_terrain_height` set (the `--terrain-y` play-window flag), each committed step snaps the player actor's `world_y` to this sample so the player rides slopes and steps. It is gated off by default so the flat-Y locomotion oracles keep their constant Y, and it is applied only on the field path - the world-map walk derives height from the continent grid through its own mechanism.

The **base wall + floor data is an on-disc blob**: it is the `+0x4000..+0x8000` region of the per-scene field map file (`DATA\FIELD\<scene>.MAP`), streamed into the field buffer at scene load by `FUN_8001f7c0` (see [Field-buffer load chain](#field-buffer-load-chain)). On top of that base, the field VM's `0x4C` (MENU_CTRL) opcode with outer-nibble 7 (`op0` ∈ `0x70..0x7F`, 7-byte op `[4C, 0x7s, b1, b2, b3, b4, mask]`) applies **story-conditional deltas** - a rectangular paint that sets/clears the high-nibble wall bits over a tile range (`col ∈ [b1, b3+1)`, `row ∈ [b2+1, b4+2)`; sub-op `s` = clear-walkable / block-all / clear-mask / set-mask), gated behind system-flag tests in the prescript.
The nibble-7 op is the same dispatch row in [`script-vm.md`](script-vm.md#0x4c-menu_ctrl---outer-nibble-dispatch).

The `+0x8000` map is a per-tile object/attribute word, not a terrain-flag grid: its low 9 bits index the `+0x0000` object-record table, which `FUN_8003a55c` walks at scene entry to spawn the NPCs/objects occupying each tile. `FUN_8003aeb0` (the field/town scene-entry map-init - note its `town_mode` / `baria_mode` debug strings) ORs the `0x400` footprint flag into these cells from the fallback trigger window's kind-1 records (`+0x12000`, offset/count at `+0x12006` / `+0x12008`, 4-byte records - the gate-0 object-bind entries of the trigger block below).

### Trigger block (`+0x10000`) - four kind sub-tables

The `.MAP` file's `+0x10000..+0x12000` region is a **per-tile trigger block**: a shared header dispatching four kind sub-tables. For kind `k`, the sub-table body offset is the `s16` at `+4k+2` and the record count the `s16` at `+4k+4` (both relative to the block start):

| Kind | Records | Content |
|---|---|---|
| 0 | `[x][z][dest_half_x][dest_half_z]` | **Intra-scene teleports** - stepping on `(x, z)` relocates the player to the destination half-tile pair. |
| 1 | `[tile_x][tile_z][p2_record][gate]` | **Partition-2 record triggers.** `gate = 1`: walking onto the tile spawns MAN partition-2 record `p2_record` as a new field-VM context (`FUN_801D1EC4` → `FUN_801D5630(1, x, z)` → `FUN_8003BDE0(x, z, rec[2], rec[3])`, ra `0x801D218C`) - doors and the opening-cutscene records (`map01` / `town01`; the entry SEAT lands on the trigger tile and fires the same tick). `gate = 0`: object-bind entries consumed at scene init (`FUN_8003A55C`), never spawned. The record's own C1/C2 story-flag gates still apply (`FUN_8003BDE0` vs the bitmap at `DAT_80085758`; C1 = block if ANY set, the one-shot mechanism). |
| 2 | per-tile | **Elevation overrides** - the ramp-tile fast path `FUN_80019278` consults via `FUN_801D5630(2, …)` before the bilinear floor-nibble interpolation. |
| 3 | `[x0, z0, x1, z1, type, 0, 0, 0]` (8-byte stride) | **Region AABB table** - the resumable point-in-AABB scan `FUN_80017FBC` (body at `+0x1000E`, count at `+0x10010` = the kind-3 header slots). Region types feed the region-type bitmask `_DAT_8007B8F4` + the camera zone query `FUN_801DBA20`. Engine `legaia_engine_core::field_regions::RegionTable`. |

The per-tile lookup (`FUN_801D5630`) scans the `+0x10000` primary block first and **falls back to the `+0x12000` window** - the first sectors of the *next* PROT entry (the dev-build `DATA_FIELD<scene>` sibling), which the contiguous `0x28`-sector read from the `.MAP` LBA (`FUN_8001F7C0`) pulls in with the same header shape. Engine: [`field_regions::TileTrigger` / `parse_tile_triggers` / `lookup_tile_trigger`](../../crates/engine-core/src/field_regions.rs) + [`Scene::field_tile_triggers`](../../crates/engine-core/src/scene/scene_ty.rs). See [`cutscene.md`](cutscene.md#record-spawn-mechanisms-live-probe-pinned) for the opening-chain use.

**Engine runtime dispatch.** Both kind-1 gate classes run live in the port:

- **Gate 1 - walk-on record spawn** (`SceneHost::dispatch_walk_on_trigger`, the `FUN_801D1EC4` per-frame tile compare): when the player crosses into a new tile during free-roam field play (`tile = (world - 0x40) >> 7`, compared against the host's last-tile mirror; a scene entry / warp arrival marks the compare stale so the arrival tile fires on the first tick, matching retail's stale globals), a gate-1 hit spawns its partition-2 record through `World::install_gated_p2_record` (C1/C2 story-flag gates checked).
  This is how town exits work - Rim Elm's south-gate tiles reference the partition-2 record whose script runs the `0x3F` named scene-change to `map01` - and how walk-on story beats (the post-naming Vahn's-house chain) launch. Skipped while a spawned record / dialog / name entry owns the frame. The dispatch runs in **both** field and world-map mode: on the overworld a gate-1 record that IS a portal (carries a `0x3F`, tested by `SceneHost::p2_record_is_portal`) is left to the world-map entity SM (`OverworldPortal`), and only non-portal **beat** records spawn here - the Drake mist-wall force-walk bands (`map01` P2[34..36], `C1=[0x482]`), which shove the player back while their story flag is clear.
- **Gate 0 - object binds** (`SceneHost::enter_field_scene` install, the `FUN_8003A55C` scene-init consumption): each gate-0 trigger binds its **partition-0** record as a touch object at the trigger tile (`World::install_trigger_walk_touch`, synthetic walk-touch slots from `World::TRIGGER_WALK_TOUCH_SLOT_BASE`). House doors are these: the record's script cross-context-teleports the player (`0xA3 0xF8`, the ＩＮ/ＯＵＴ pair mechanism), decoded by `man_field_scripts::p0_record_walk_touch_event`.
  Partition-0 records carry their **own header form** `[u8 n][n*2 SJIS name][u8 attr]` (`pc0 = 1 + n*2 + 1`), not the partition-1 `[N][N*2 locals][4-byte header]` shape. Contact then routes through the same `check_field_walk_touch` dispatch as placement touches.

The partition-2 gate bitmap (`DAT_80085758`) **is** the field VM's `0x50`/`0x60`/`0x70` system-flag bank - one store, shared by the record dispatcher's C1/C2 test (`World::p2_gate_flag_set` = `system_flag_test`) and the VM's flag writes, so an opening-timeline `set` is immediately visible to the next record's gate. It also overlaps the saved story-flag window at byte `+0x158` (`0x80085758 - 0x80085600`); the engine save mirrors the bank into that window and reloads seed it back. Disc-gated coverage: `crates/engine-core/tests/walk_on_trigger_dispatch_disc.rs` (opening-to-free-roam progression, south-gate exit to `map01`, house-door contact teleport, ambient no-lock, gate-flag save round-trip).

**The C1 one-shot-latch idiom.** A walk-on beat that should play exactly once self-latches: its script `0x50 SET`s the very flag its `C1` lists, so the beat runs on the crossing that finds `C1` clear and its own `set` then blocks every later crossing (`C1` = block-if-ANY-set). The `town01` dinner chain is the canonical example - `P2[4]` (`C1=[550]`, sets `550`), `P2[5]` (`C1=[551]` `C2=[550]`, sets `551`), each a link that latches as it completes. The `step_cutscene_timeline` port applies those `set` ops in the system bank as the record runs (proven by `550` latching after the beat), so a completed timeline stops its own re-fire.

Two variants sit either side of that idiom. A record whose `C1`/`C2` are **empty** is spawned on *every* crossing by design and self-manages via an internal `0x70 TEST`/`0x50 SET` on a private flag (`town01` `P2[6]`: `TEST 558 → … → SET 558`, jumping straight to its end while the guard flag is clear, so it is a no-op rather than a re-fire lock). The overworld mist-wall bands (`C1=[0x482]`) invert the sense - they carry no `set`, staying live until an *external* story event sets `0x482`, after which the `C1` gate blocks them.

**Spawned-record execution semantics.** On-disc partition-2 records have **no end opcode**; the engine's modal cutscene-timeline stepper (`World::step_cutscene_timeline`) recovers a completion point three ways:

- **Choreography wrap**: an `Advance` jumping backward onto an already-executed PC completes the timeline. Records finish either in a tight `Nop`+`JmpRel`-to-self park (the fog-config / flag-reset ambients - `town01` P2[16]/P2[21]/P2[22], spawned on every crossing of their gate-1 tiles) or by looping back to their conversation top as a **resident actor-driver** (the Mei walk-on beat's op-`0x45` APPLY jump). Retail leaves both spinning as parallel contexts, invisible to the player; the modal timeline completes there instead so control returns. Real waits (`0x4A` WAIT_FRAMES, flag-test handshakes) `Halt` at their own PC and never trip the rule.
- **Inline dialog boxes**: a record byte with `& 0x7F < 0x20` at the PC is the retail dialog-SM transition (`FUN_80039B7C`), not an opcode. A `0x1F` lead opens a dialog panel over the record bytes and parks the timeline (frame cap frozen - a dialog waits on the player); confirm dismisses the box / commits a picker choice, resuming past the segment. Stray terminators (`0x00..0x1E`) the flow lands on are consumed.
- **Unresolved cross-context targets**: an `0x80`-bit op whose target id matches no spawned channel (partition-0 object contexts, e.g. the Mei beat's channel `0x01`) is skipped by its decoded width - running it against the timeline's own context corrupts the caller (the record's `CC 01 A0` channel-busy wait then hijacks the caller PC into the record header). Resolved-channel `4C A0` busy-waits fall through unconditionally: engine channel pokes complete synchronously, where retail's channel clears its own busy bit as its move plays out.

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
| `+0x08` | `u16` | rotation about world X, PSX angle units (`4096` = full rev); zero on every retail walk `.MAP`, rare prop tilts in towns (`koin2`) |
| `+0x0a` | `u16` | **rotation about world Y (yaw)** - the authored mesh orientation. The Sebucus island bridges' quarter-turns (`0x400`/`0xC00`) and the walk decoration layer's per-tree variety live here |
| `+0x0c` | `u16` | rotation about world Z, PSX angle units; zero on every retail walk `.MAP` |
| `+0x12` | `u16` | flags; bit `0x4` = placed/active, bit `0x800` ORs actor `+0x74` bit `0x10000000` |
| `+0x1e` | `u8`  | non-zero ORs actor `+0x74` bit `0x40000000` |

The anchor object is created by `FUN_80024c88(pos, …)` (writes `actor+0x14/16/18`), then
`FUN_8003a55c` writes `actor+0x60 = object_index` and copies record `+0x08/+0x0a/+0x0c`
into `actor+0x24/+0x26/+0x28` - the **rotation triple**. The per-actor render dispatcher
(`FUN_8001ADA4`) hands `actor+0x24` to the angle-triple → GTE-matrix builder
`FUN_80026988` (each component masked `& 0xFFF`, cos/sin LUT pointers at
`DAT_8007b7f8`/`_DAT_8007b81c`); for a pure-Y angle the result is the row-major
`[c 0 s; 0 1 0; -s 0 c]`, mapping local `+Z` to `(sin, 0, cos)` - the same forward
vector the locomotion integrator walks along.

**This table is the static environment placement** - the visible terrain
segments, buildings, and props, *not* (only) NPC spawns. Each placed tile
allocates a static-object actor (shared tick fn `0x8003BC08`); the actor draws
its mesh from the [`scene_asset_table`](../formats/scene-bundles.md) TMD pack
through its `+0x44` mesh chain. (Validated against a live `town01` save: object
id `137` = Vahn's house, anchor tile `(col 38, row 25)` -> `world (4864, _, 3208)`,
matching the live actor; the `+0x08..+0x0c` fields are the rotation triple, not
the mesh selector.) NPCs and event triggers ride
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

- Controller `FUN_801d01b0`, position writes `0x801D0684 / 06E4 / 0744 / 07B4` - see `ghidra/scripts/funcs/overlay_0897_801d0684.txt`.
- Collision `FUN_801cfe4c`, finer probe `FUN_801cfc40`, interaction `FUN_801cf9f4` - `ghidra/scripts/funcs/overlay_0897_801cfe4c.txt`.
- Pad remap `func_0x800467e8`, direction mask `FUN_80046494` - `ghidra/scripts/funcs/800467e8.txt` / `80046494.txt`.
- Scene-entry map-init `FUN_8003aeb0` (height LUT fill, `+0x8000` footprint OR, player-actor setup) - `ghidra/scripts/funcs/8003aeb0.txt`. Object spawn iterator `FUN_8003a55c` (low-nibble floor-height read, `+0x8000` index walk) - `ghidra/scripts/funcs/8003a55c.txt`.
- Runtime pin: `scripts/pcsx-redux/autorun_player_pos_watch.lua` (write-watchpoint on `*(0x8007c364) + 0x14/0x18`).

## Town / field parity

The controller is selected by **game mode**: mode `0x03` loads the field overlay (`overlay_0897`), which contains the single free-movement controller `FUN_801d01b0`. `FUN_801d01b0` was runtime-pinned on a walkable field scene (`map03`, mode `0x03`). Rim Elm - scene `town01` - also runs at game mode `0x03` (see `scripts/scenarios.toml`, the `v0_1_pre_battle_tetsu` anchor), so it loads the same overlay and the same controller. The shared scene-entry init `FUN_8003aeb0` corroborates this: it has an explicit `town_mode` debug-string branch and configures the same player actor (`_DAT_8007c364`: speed mult `+0x72 = 0x1000`, `+0x6a = 8`) for both towns and fields. So town locomotion is `FUN_801d01b0`, identical to the field.

The **overworld walk mode** shares it too. The world-map-walk overlay's locomotion is byte-for-byte the same `FUN_801d01b0` (same collision `FUN_801cfe4c`, same `_DAT_1f8003ec + 0x4000` grid); only the loaded overlay and grid contents differ. The three kingdom overworld scenes (`map01`/`map02`/`map03`) carry real wall data in that grid (≈ 7968 / 2283 / 3837 high-nibble wall sub-cells), so the overworld is bounded by the same tile-wall mechanism as towns - it is not a separate walkability format. See [`world-map.md`](world-map.md#overworld-collision--walkability).

## Engine port

The clean-room engine loads the base grid directly from the field map file. `SceneHost::enter_field_scene` resolves the `.MAP` entry via `Scene::field_map_index` - the scene's retail block's **first entry** (extraction `define − 2`; CDNAME defines are raw-TOC indices, see [cdname.md](../formats/cdname.md#numbering-space)), identified by its **extended on-disc footprint** of exactly `0x12000` bytes - and copies its `+0x4000..+0x8000` region into `World::field_collision_grid` (`World::load_field_collision_grid`).
The rule mirrors the runtime resolution (`FUN_8003e8a8`'s `toc[idx+2]`) and is **universal**, not kingdom-specific: a save-library census found the live `keikoku` field buffer matches PROT 0109 (`define 111 − 2`) with **zero** diffs while the neighbouring `0x12000` candidate (0118) differs by thousands, and `koin3` likewise matches 0559 exactly.
An earlier rule picked the first `0x12000` entry inside the era's unshifted scene window - the **next** scene's map - and loaded the wrong base grid for every field scene, masked only where adjacent Rim Elm variants byte-copy (town01/town0b/town0c share one identical map), the only scene it had been validated on. The grid byte format (high nibble = sub-cell wall bits, low nibble = floor-elevation tier) matches the runtime 1:1, so it copies verbatim; the field-VM `0x4C` nibble-7 hook then layers deltas on top as the prescript runs.

Footprint caveat: the TOC-**indexed** payload of the `.MAP` entry is only the first `0x4000` bytes (the object-record region); the collision grid and everything past it live in the entry's **trailing-gap sectors**, so the engine reads `ProtIndex::entry_bytes_extended`, not the indexed `SceneEntry::bytes`. Verified byte-exact: `town01`'s `entry10[0x4000..0x8000]` equals the live collision grid in a town01 save state (1297 wall tiles, zero diff). Disc-gated coverage: `crates/engine-core/tests/field_locomotion_disc.rs` (base grid non-empty on `town01` + `map03`, and the player stops at a real base wall).

### Environment geometry

A field/town scene's environment meshes (the terrain, buildings, and props) are Legaia TMDs packed inside **LZS streams of the scene_asset_table** PROT entry (`town01` = entry 4: 121 meshes, ≈8041 vertices). The clean-room `SceneResources` TMD pass scans each entry's LZS-decompressed sections in addition to its raw bytes (`tmd_scan::scan_entry`, the same path the TIM pass already used), so these meshes land in the scene TMD pool; the `scene_tmd_stream` skip still drops battle-character meshes in field mode. The field build uses `SceneLoadKind::Field` with `upload_all_tims`, matching retail's field loader (`FUN_8001f7c0`), which DMA-uploads every TIM - the environment meshes sample texture pages across the whole atlas, so a render-targeted upload drops most of their prims.
Per-mesh **world placement** for this static geometry is the [Object-record table](#object-record-format-0x0000-0x20-byte-stride) above (`FUN_8003a55c`: the object-index grid at `+0x8000` of the field map file selects a `+0x0000` object record per placed tile, giving the mesh its world translation; `legaia_asset::field_objects` parses it, `Scene::field_object_placements` exposes it). Each object's mesh resolves to a scene-pack index (`pack_index = obj_idx - 5` for the field-actor band `93..=118`, else the record's `+0x10` field). Per-tile **world Y** = `-floorHeightLUT[tile_nibble] + y_off`, the LUT being 16 `s16` at the MAN header `+0x02` (`Scene::field_floor_height_lut`). `legaia-engine play-window` renders the town from this:
`resolve_field_placement_draws` pairs each placement with its uploaded pack mesh + world transform (X/Z + floor-LUT Y) and draws them in `SceneMode::Field`.

### Scene-entry script

On entry the engine runs the scene's **scene-entry system script** (context channel `0xFB`), not event-script record 0. Record 0 of a per-scene event-script container is a trigger/dispatch table, not linear bytecode, so loading it as the field-VM buffer halts the VM at pc 0 and no entry logic runs. The retail per-frame driver `FUN_8003ab2c` builds the system script from the MAN asset's partition 1, first record; `Scene::field_man_entry_script` mirrors that resolve (`legaia_asset::man_section::ManFile::scene_entry_script` → `(start, pc0)`), and `SceneHost::enter_field_scene` loads the MAN slice from `start` with the VM PC at `pc0` (`World::load_field_script_at`). Slicing from the script start keeps the field VM's 16-bit-wrapping relative jumps anchored at the slice base,
matching the retail `buffer_base = script_start`.

Every field/town scene carries its MAN in a [`scene_asset_table`](../formats/scene-bundles.md): kingdom-bundle scenes use the `count = 7` form, and the early standalone towns (`town01` = Rim Elm, `town0c`, …) use a `count = 6` form in their block's 2nd PROT entry (e.g. `town01` = entry 4, MAN at descriptor 1). `find_bundle` resolves both, so `field_man_entry_script` runs the real entry script for all of them. The MAN source is pinned by a runtime write-watchpoint on `_DAT_8007B898`: the dispatcher `FUN_8001F05C` case 3 mallocs the buffer and LZS-decodes it from the table descriptor (see [`scene-bundles.md`](../formats/scene-bundles.md)). The base collision grid (loaded from the `.MAP` above) is independent of which entry script runs;
the entry script's `0x4C` nibble-7 wall-paint deltas are gated behind system-flag tests and only fire once the world's story flags are seeded to a matching scene-entry state. Disc-gated coverage asserts the MAN-backed scenes' field VM advances past pc 0 (`town01`: 65, `map03`: 61 distinct PCs, settling into a per-frame loop).

#### Story-conditional wall deltas (map03)

Tracing `map03`'s entry script pins the gate flags directly: `TEST` flag `0x6C2` (at script offset `0x2c`) routes into a sub-1 "block all" paint over tile (col 66, row 102), and `TEST` flag `0x378` (at `0x4f`) routes into a contiguous three-paint cluster (sub-0 "clear walls" at offsets `0x56` / `0x5c` / `0x62`). At a fresh boot both flags are clear, so the entry script skips all four paints and the grid stays at its disc-loaded base - which is correct: these are story-conditional terrain changes, not the base walls. Seeding the matching system flags (in real gameplay, loading a save whose story-flag block has them set) makes the paints fire. The flag-bank base is `0x80085758` (= SC offset `0x1618`); see [`script-vm.md`](script-vm.md#0x4c-menu_ctrl---outer-nibble-dispatch).
Disc-gated coverage: `crates/engine-core/tests/map03_conditional_walls_disc.rs` (with flag `0x6C2` seeded, the wall at tile (66, 102) appears; without it, it does not).

The nibble-7 paint format (retail handler `0x801e1c64`): the **row** range is `[row0+1, row1+2)`, and sub-0/1 paints are **6-byte** ops with no mask byte while sub-2/3 are 7-byte.

### Scene encounter table

The same MAN that supplies the entry script also carries the scene's **random-encounter table** in its section 0 (`FUN_8003AEB0` installs it into the runtime control block `_DAT_801C6EA4 + 0x20`; see [`encounter.md`](../formats/encounter.md) and [`man-section`](../formats/scene-bundles.md)). Because the `count = 6` detector now resolves the standalone towns' MAN, the field scene-entry path can pull the disc-resident table for them too. `Scene::field_man_encounter_table` resolves the MAN through `find_bundle`, decodes the encounter section via `legaia_engine_core::encounter_man::scene_encounter_from_man`, and `SceneHost::enter_field_scene` installs it (`World::install_man_encounter`): the per-formation rows become `EncounterEntry`s keyed by row index,
and the matching `FormationDef`s (row index → monster-id slots) are merged into the formation table so a triggered encounter resolves to a concrete monster set. The MAN carries formation monster-ids but not stat blocks, so the host installs the stat catalog separately; scenes whose bundle has no MAN keep the synthetic-pattern `EncounterRegistry` fallback.

Towns carry random encounters too: `town01`'s MAN encounter section declares **7 formations** at a low mean trigger rate (`6/256`), gated by its region records, overriding the synthetic-registry fallback. Disc-gated coverage: `crates/engine-core/tests/field_man_encounter_disc.rs` (boots `town01` / `town0c` / `map03`, asserts each installs a MAN encounter session whose row-index `formation_id`s all resolve to merged formation defs).

### Per-step encounter roll in the live loop

When `World::live_gameplay_loop` is set, locomotion feeds the encounter system directly: `World::live_field_tick` treats the player crossing into a new 128-unit collision tile (`pos >> 7`) as one *step* and drives a single `World::on_field_step` roll, mirroring the retail per-step counter rather than rolling every frame. A successful roll transitions `Field → Battle`; on victory the field actor table is restored and the player resumes where they stood. See the [live gameplay loop](battle.md#live-gameplay-loop--field--battle-in-tick) section in `battle.md` for the full round trip.

### Input is locked during an opening-cutscene timeline

`World::step_field_locomotion` is gated on `current_dialog`, an active tile-board, the per-actor movement-disabled flag (`move_state.flags & 0x0008_0000`), **and** an active opening-cutscene timeline (`World::cutscene_timeline_active`). During the `town01` opening's establishing sweep the spawned [cutscene timeline](cutscene.md) drives the lead actor through its own MoveTo ops, so the pad must not also walk the player out from under the cinematic camera. Control returns the frame the timeline drops - matching retail, where free-roam input is accepted only after the opening choreography ends.

## Field-buffer load chain

The base wall + floor grid is **streamed from disc**, not script-authored: it is the leading region of a multi-sector CD read issued at scene load. A runtime write-watchpoint on the live grid (`_DAT_1f8003ec + 0x4000`) during a Drake-Castle → Drake-world-map transition caught one bulk writer - the CD-DMA channel-3 read primitive **`FUN_8005D9A0`** (DMA store at `0x8005DA50`), reached via the wrapper `FUN_8005C2C4` from the per-sector streaming poller **`FUN_8003EF14`**. The poller DMAs one 2048-byte CD sector per ready-IRQ into the destination cursor at `gp + 0x940` (= `0x8007BC58`, holding `_DAT_1f8003ec + 0x4000`), advancing `0x800` per sector. So the field buffer - collision grid (`+0x4000`), object map (`+0x8000`), field-pack (`+0x12000`) - is the leading region of that streamed read.
Across the transition the grid jumped 2093 → 6805 wall tiles while only **6** nibble-7 CPU-store writes fired, confirming the bulk arrives as disc sectors and the field-VM `0x4C` nibble-7 ops are conditional deltas layered on afterward.

`FUN_8001f7c0(dest, scene_name, field_record)` fills the field buffer at `dest` (the `_DAT_1f8003ec` base). Two transports converge on shared streaming machinery:

- *Retail*: builds `DATA\FIELD\<scene>.MAP`, opens it by ISO9660 name (`FUN_800608f0`), streams into `dest` via `FUN_8003e6bc`.
- *Debug* (`_DAT_8007b8c2 != 0`): `FUN_8003e8a8(field_record, 1)` sets the `CdlLOC` at `0x8007bc5c` from the in-RAM PROT TOC (`target_sector = CdPosToInt(base_loc@0x8007bc50) + toc[field_record + 2]`, the documented `start_lba = toc[p+2]`); `FUN_8003e800(dest, 0x28, 1)` issues a 40-sector (`0x14000`-byte) read.
- Shared core: `FUN_8003e800` → `FUN_8003f128` (copies dest/count into `gp+0x940`/`gp+0x968`, issues `CdControl(CdlSetloc)`, registers the data-ready callback) → `FUN_8003EF14` per-sector poller → `FUN_8005C2C4` → `FUN_8005D9A0`. The same generic entry serves other clients (`FUN_8003e104` = the `monster_snd` pack loader), so `FUN_8003e800`/`FUN_8003f128` are shared streaming infrastructure. See [`boot.md`](boot.md) for the CD-read API.

**For the engine, base collision is a load step, not a script step**: slice bytes `0x4000..0x8000` of the per-scene `.MAP` file; no script execution is needed for the base walls. The nibble-7 ops ride the scene's field-VM scripts - which run multi-context at load (`FUN_8003aeb0` scene-entry init → `FUN_8003ab2c` MAN system-script runner, the `0xFB` system context being the conditional-delta painter) - and only matter for story-conditional terrain changes.

NPC walkers carry a live heading (`World::field_npc_headings`, the player's
12-bit `render_26` convention, derived from each motion-VM step's direction
and retained on arrival); the `play-window` field renderer rotates each NPC
model to it and plays the placement's scene-bundle ANM clip per frame
(`FieldClipPlayer` over the `anim_id - 1` record, the same posed-rebuild path
as the player's idle/walk pair).

## Open

- The full `FUN_801d5b5c` post-kernel state (the touch-event handler beyond the decoded entry kernel).
- Full per-actor field-VM channel execution with story-flag-conditioned branches (the engine loops decoded waypoint lists, and the initial-facing decode takes the fall-through branch - see [NPC initial facing](#npc-initial-facing) - rather than evaluating the prologue's `0x7x` flag-TEST chain against live flags, so a later-chapter branch's facing/position is not selected).

## NPC initial facing

The placement record carries **no facing byte** - its 4-byte header is `[model, anim, tile_x, tile_z]` only. A never-walked NPC's heading comes from a **spawn-time prologue pre-run**: the placement installer `FUN_8003A1E4` ends by executing the record's leading field-VM ops one at a time through `FUN_801DE840` when the first opcode is the `0x24`/`0x25` spawn-prologue marker, stopping at a `0x21` NOP terminator or any below-`0x20` byte (body `0x8003A474..0x8003A4F8`; see `ghidra/scripts/funcs/8003a1e4.txt`).

Two prologue ops write the actor's `+0x26` render heading from the 8-direction LUT at SCUS `0x80073F04` (entry `i` = `i * 0x200`; the LUT has 16 addressable slots but only 0..=7 are direction entries):

- `0x4C 0x51` (nibble-5 sub-1, the NPC move-to-tile op): the dispatcher writes `+0x14/+0x18` from the tile bytes **and** `+0x26 = table[b3 & 0xF]` - operand byte +3's low nibble is the facing index (`overlay_0897_801de840.txt`, case 5 sub 1);
- `0x38` CAM_CFG **simple path** (`op1 & 0x7F == 0`): `+0x26 = table[op0 & 0xF]`.

The heading space itself is pinned from the locomotion's pad→facing writes (`FUN_801d01b0` body `0x801d04b8..0x801d0548`): retail `0` = Z-, `0x400` = X-, `0x800` = Z+, `0xC00` = X+ - the engine's `render_26` convention (`0` = Z+) rotated a half-turn, `engine = (retail + 0x800) & 0xFFF`, no axis mirror.

Town prologues route the facing leg through a story-flag `0x7x`-TEST branch chain (jump when the flag is **set**), so the fall-through branch - the first leg in linear record order - is the fresh-game state.

The engine decodes that leg statically per placement ([`man_field_scripts::placement_initial_facing`](../../crates/engine-core/src/man_field_scripts/npc_motion.rs), skipping cross-context and park-sentinel legs), converts through [`facing_index_to_engine_heading`](../../crates/engine-core/src/man_field_scripts/npc_motion.rs), and seeds `World::field_npc_headings` at scene entry (`World::seed_field_npc_facings`) - a later walk overwrites the slot exactly as retail's per-step facing writes overwrite `+0x26`. Semantic pin: town01's side-by-side villager pair at tiles `(29,22)`/`(30,22)` derives LUT indices 6 (X+) and 2 (X-) - they face each other; disc-gated coverage in `field_npc_initial_facing_disc.rs`.

Note the facing pin also fixes what `0x4C 0x51` operand byte +3 **is**: bit 7 toggles the special-model flag, the low nibble is the facing-LUT index. The glide-speed model below reads the same byte's low 3 bits as the base-step selector pending the synthesised-motion-bytecode trace - that interim reading overlaps the facing nibble and should be revisited when the `0x4C 0x51` → motion-script write is traced.

## NPC glide speed

An NPC's per-frame glide is NOT the player's `+0x72` walk step (that premise is falsified: `FUN_8003774C` never reads `+0x72`). Retail's motion VM (ops 0x37/0x41/0x47) glides at `_DAT_1f800393 × 0x80 / (4 << bits)` units per frame, where `bits` is a base-step selector encoded **in the motion op's own operand** (`(op0>>5 & 4)|(op1>>6)` of the synthesised motion bytecode). With the cold-field delta scalar `_DAT_1f800393 = 1` that is `0x80 >> (2 + bits)`: base steps 32 / 16 / 8 / 4 / 2 / 1 for `bits` 0..5, floored at 1.

The engine derives each placement's glide speed off the disc rather than pacing every NPC at the flat player step: [`man_field_scripts::placement_glide_speed`](../../crates/engine-core/src/man_field_scripts/npc_motion.rs) reads the placement's first local `0x4C 0x51` motion op (the field-VM carrier of the motion script), maps its `depth` operand's low 3 bits through [`World::field_npc_glide_speed`](../../crates/engine-core/src/world/config.rs) (`0x80 >> (2 + (depth & 7))`), and stashes it in `World::field_npc_glide_speeds`. `World::start_field_npc_motion` writes that into the leg's motion-VM `speed`, so each placement glides at its authored cadence.

A placement with no decodable motion leg (and the actor-VM sprite glide, which has no MAN motion operand) falls back to the `FIELD_NPC_MOTION_SPEED` stand-in (base step 8 = `field_npc_glide_speed(2)`), so the default path is unchanged. Disc-gated `field_npc_glide_speed_disc.rs` confirms town01's placements span the whole base-step ladder (most differ from the stand-in) and the engine's motion state carries the derived value.

Modelling note: the exact `0x4C 0x51` → synthesised-motion-bytecode write is not yet traced, so the engine reads the base-step selector from the `0x4C 0x51` leg's `depth` operand (the field-VM operand that carries the glide granularity) rather than the two synthesised motion-op operand bytes directly; and `_DAT_1f800393` is taken at its cold-field value 1.

## See also

**Reference** -
[Field/event VM](script-vm.md) ·
[World map](world-map.md) ·
[Scene bundles](../formats/scene-bundles.md) ·
[Scene v12 table](../formats/scene-v12-table.md)
