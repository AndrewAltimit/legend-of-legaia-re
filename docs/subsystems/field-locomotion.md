# Field free-movement locomotion

The player free-movement controller for normal towns, dungeons, and walkable field areas is **`FUN_801d01b0`** in the field overlay (`overlay_0897`). Each frame it reads the held pad, turns it into a camera-relative direction, advances the player actor's world position a fixed step per sub-frame with per-axis collision, and updates the player's facing angle. This is the general locomotion path - **not** the [tile-board grid mode](tile-board.md) (a puzzle / board minigame that happens to live in the same overlay).

**Port counterpart.** `World::step_field_locomotion` in `engine-core`
(`decode_field_direction`), against the per-scene walkability grid.

**The thing that catches people out:** static analysis will not find this
function. It was pinned with a runtime write-watchpoint on the player position
fields (`scripts/pcsx-redux/autorun_player_pos_watch.lua`): walking in a field
scene fires write hits at the four `sh` stores `0x801D0684 / 06E4 / 0744 / 07B4`
(player Z± / X±), all inside `FUN_801d01b0`. The writes are buried in a
1964-byte function, and the field overlay only loads at runtime - so there is no
static call site to follow. See [Provenance](#provenance).

## Contents

- [Player actor fields used](#player-actor-fields-used) · [spawn position](#spawn-position-on-scene-entry) · [per-frame flow](#per-frame-flow)
- [Collision - `FUN_801cfe4c`](#collision---fun_801cfe4c) · [where the grid comes from](#where-the-collision-grid-comes-from) · [collision byte](#collision-byte-walls--floor-height) · [floor height](#floor-height-two-models) · [trigger block](#trigger-block-0x10000---four-kind-sub-tables) · [object records](#object-record-format-0x0000-0x20-byte-stride) · [the object bind](#the-object-bind-which-sweep-owns-the-object-and-its-rest-pose) · [the door swing](#the-door-swing-how-a-bind-script-drives-the-clip)
- [Provenance](#provenance) · [Town / field parity](#town--field-parity)
- [Engine port](#engine-port) - [environment geometry](#environment-geometry) · [scene-entry script](#scene-entry-script) · [encounter table](#scene-encounter-table) · [per-step encounter roll](#per-step-encounter-roll-in-the-live-loop) · [input lock during cutscenes](#input-is-locked-during-an-opening-cutscene-timeline)
- [Field-buffer load chain](#field-buffer-load-chain)
- [Intra-scene doorways](#intra-scene-doorways---the-walk-touch-teleport-family) - [the three player-move op forms](#the-three-player-move-op-forms) · [the record is a branch](#the-record-is-a-branch-not-a-constant) · [pairing convention](#the-pairing-convention) · [geometry](#geometry) · [landing records](#landing-records) · [Rim Elm](#rim-elm) · [engine port](#engine-port-1)
- [Open](#open) · [NPC initial facing](#npc-initial-facing) · [NPC glide speed](#npc-glide-speed)
- [Engine port: movement compass + opt-in precise movement](#engine-port-movement-compass--opt-in-precise-movement)

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

`FUN_801cfc40(actor, scene, Δx, Δz, ex, ez)` walks the **collision candidate table** `DAT_801c93c8` (count `_DAT_8007b6b8`) and box-tests the probe point against each other actor.
A **static entity** (`flags+0x10 & 0x1020000 == 0`) anchors at its **MAN object record** (`_DAT_1f8003ec + rec_idx[+0x60]*0x20`; anchor `= tile*128 + sub*16` from record bytes `+6`/`+7` and `+0xE`/`+0xF`, with a `flags+0x52 & 8` offset correction from record halfwords `+0`/`+4`) plus the actor's live `+0x14`/`+0x18`, and blocks within `±(0x40+0x10)` = **80 units** per axis (strict).
A **moving actor** uses its live position with caller extents `±(0x40 + ex−0x18)` (the locomotion passes `ex = ez = 0` → ±40).
A hit links the pair mutually at `+0x98`, posts `func_0x8003d038(other[+0x50])` (stores the touched bind-record index into `DAT_80073F1C` unless the per-record `DAT_801C6470` byte is `0x8C` - the motion-VM wait-for-touch opcode at `0x8003882C` consumes and resets it), and contributes result bit `1` (`flags & 0x40020000` class) or `4` (static prop). When the actor table is full (`_DAT_8007b6b8 == 0x20`) the whole call delegates to the `FUN_801cf9f4` box-test variant.

The candidate table itself is rebuilt per frame by **`FUN_801cf754`** (`ghidra/scripts/funcs/overlay_0897_door2_801cf754.txt`): it walks the live actor linked list, culls to ±`0x180` of the player, caps at `0x20` entries - and **skips any actor whose `+0x10 & 3 != 0`**. `FUN_801cf9f4` applies the same `flags & 3` skip inline (`0x801cfa4c`). Those two bits are the placed-prop **collision/touch kill switch**, and prop bind scripts author them:

- A **door's touch pass runs `31 00`** (field-VM CFLAG_SET bit 0 on its own `+0x10`) immediately after its swing-start ops (`2C 07 / 2C 01 / 2B 03`, e.g. `town01` P0[0] offset `0x24`) and *before* the `2D 08` end-latch spin - so a closed door is **solid** (bit-4 contact blocks the step while the same probe posts the touch), and its collision + touch box drop **at touch-resume, as the swing starts**, not at full-open. Props born pass-through carry `31 00` in their spawn prologue instead.
- A **searchable prop's spawn prologue runs `31 1E`** (`+0x10 |= 0x40000000`, e.g. the `town01` cupboard P0[12] offset `0x09`), flipping its contact class to result bit `1`: it still blocks, but the locomotion dispatch **never auto-posts bit-1 partners** - only the just-pressed-confirm facing probe does. That single authored op is the whole door-vs-cupboard discriminator: doors open on body contact, cupboards only on the interact button. (`31 11` - bit 17, `0x20000` - also selects the bit-1 class *and* the moving-arm box.)

**The locomotion gates a step on the actor bits and the wall bit together**: `FUN_801d01b0` commits each 2-unit axis step only when `FUN_801cfe4c` returns `0` (or the debug no-clip `_DAT_8007b98c`/`_DAT_8007b850 & 2` is on) - NPCs block movement exactly like walls.

Each sub-step it also runs the **touch/interact dispatch** (gated off while the player's `+0x10 & 0x80000` engaged flag, the scratch system-channel `_DAT_1f800394 & 0x400`, or the field-control dialog byte `_DAT_801c6ea4+0x62` is set):

- **Prop walk-touch is automatic for the static (bit-4) class only**: a step whose probe result carries bit `4` posts the touched entity's event on the spot - `FUN_801d5b5c` on the `+0x98` partner (`0x801d0800..0x801d0808`), every contact step, no button needed. A bit-1-class prop (the `31 1E` cupboards) never reaches this arm; it fires only from the button-gated probe below.
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
- **The placed-prop arms are modelled from the `.MAP` placements, and props are solid by default.**
  `Scene::field_object_placements` already returns exactly the collision-actor spawns (the placed
  flag `0x4` *is* the spawn gate; the numerous flag-`0x11/0x12/0x13` records are the terrain layer),
  and each placement carries its `collider_x`/`collider_z` box centre (spawn position + the record's
  collision-footprint offset, `legaia_asset::field_objects::collision_footprint_offset`).
  `SceneHost::install_field_props` builds one [`FieldPropCollider`] row per placement at field
  entry, classed by its bind record's spawn-prologue `0x31` ops
  (`interact`/`moving_box`/born-exempt); `advance_with_collision` blocks on them **unconditionally**
  (retail's props always sit in the `FUN_801cf754` candidate list) - a head-on press rests 142 units
  short of a static prop centre (same pre-step parity as the NPC arm's 102), and the same refused
  step latches a static-class prop's touch into `World::pending_prop_touch`. A prop whose script has
  run `31 00` (`FieldPropCollider::solid = false`) blocks and touches nothing, exactly like retail's
  `flags & 3` skip. Only the NPC arm stays behind `World::solid_field_npcs` / `--solid-npcs`.
- **The button-press interact dispatch is modelled faithfully.** `World::field_interact_probe_slot` ports the `DAT_801f2254` facing probe (the radius-64 compass point, ±72 interact box); a hit opens the NPC's dialogue and turns the player toward it (`World::face_field_npc`, the face-the-NPC step - shape-faithful float `atan2` rather than retail's arctan LUT). The engine's field heading stores `0` = Z+ where retail facing stores `0` = Z− (a Z+ walk writes `0x800` to `+0x26`), so the sector index adds a half-turn before quantising. The captured Tetsu press-rest position talks to him through this probe (`world.rs::tests::interaction_probe_matches_tetsu_capture_geometry`).
- **Field-NPC motion is modelled through the motion VM.** Each talk NPC's placement script carries its authored walk legs as `0x4C 0x51` NPC move-to-tile ops; `man_field_scripts::placement_motion_route` decodes the local waypoints and `World::tick_field_npc_motions` drives them through the ported motion VM (`FUN_8003774C`), one pursue step per field tick, writing the live position back into `World::field_npc_positions` - so the moving NPC's ±40 collision box and its interact box follow it, exactly as retail probes the live `+0x14`/`+0x18`.
  Autonomous patrol is opt-in (`World::animate_field_npcs` / `play-window --live-npcs`) and pauses while a dialogue is up (the retail interaction motion-pause kick); an interaction prologue's own `0x4C 0x51` runs the interacted NPC through the same kernel regardless of the flag. See [`motion-vm.md`](motion-vm.md#field-npc-walking). Disc-gated: `engine-core/tests/field_npc_motion_disc.rs` (town01 derives routes for many villagers; the engine walks them off-anchor; the collision box follows).
- **The prop walk-touch event post is modelled for the decoded script classes.**
  `man_field_scripts::placement_walk_touch_event` classifies each non-parked placement's script: a
  genuine `0x3E` door-warp (`Warp`) or a cross-context `0x23` into the player channel `0xF8`
  (`PlayerMoveTo` - the cave-guard throw-back / intra-scene teleport).
  `World::check_field_walk_touch` runs on the locomotion step: contact is tested with the **same
  forward probe points that block movement** (the `DAT_801f21b4` rows of the directions held this
  tick, plus the stand-inside fallback) against the placement's static ±80 contact box, posting once
  per contact through the same `trigger_field_interact` dispatch the button-gated interact uses and
  applying the decoded effect (queue the door-warp transition / snap the player) - so a **solid**
  doorway object still fires its teleport while the player stands pressed against its box, exactly
  as retail's one probe both refuses the step and posts the touch.
  Not modelled: the facing save/restore and touch counters of the post kernel. Disc-gated: `engine-core/tests/field_walk_touch_disc.rs` (koin1 mine-exit warps; cave01 guard throw-backs).
- **Prop bind records run through the field VM on touch / interact - the door swing, its collision
  drop, and the cupboard search.** A static-class prop touch (`World::pending_prop_touch`, the bit-4
  auto-post) or an interact-class confirm press (`World::field_interact_prop_anchor`, the
  facing-probe prop arm wired into `tick_field_interaction_probe`) starts
  `World::start_prop_interaction`: the prop's bind record runs through the inline field-VM runner
  from the prop's parked cursor (`PropAnimState::parked_pc`, the engine's `actor+0x9E`) with the
  executing context bridged to the prop actor - `ctx.local_flags` ↔ `+0x62` (the clip control word
  the `2B`/`2C`/`2D` ops drive), `ctx.flags` ↔ `+0x10` (whose `31 00` drops the prop's collision
  row), `ctx.field_6a` ↔ `+0x6A`. Waitable ops (`2D 08` until the per-frame anim tick latches the
  clip end, `4A` frame waits) **park** the run; `0x1F` segments open the real dialog panel
  (item/character name escapes resolved via `OwnedDialogPanel::substitutions`); `39` GIVE_ITEM
  grants through the host; the raw `21` ends the interaction and re-parks the record. The player's
  movement-disabled flag (`+0x10 & 0x80000`) is held for the run's duration - the retail engaged
  flag `FUN_801d5b5c` raises and the dialog SM teardown clears - so the player stands while the door
  swings and the message is up. Engine: `world/prop_interact.rs`; disc-gated:
  `engine-core/tests/field_prop_anim_disc.rs` (a closed door blocks at the retail standoff, opens on
  the blocked step's touch, and stops blocking via its `31 00`; the cupboard blocks silently, opens
  only on interact, grants once under its `70 xx` searched-flag guard, shows the found/empty
  message, and swings shut when the box is dismissed).

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
  not a flat plane (see [`world-map.md`](world-map.md), "the continent ground is a procedural heightfield"). But this bilinear surface is only **half** the floor model - see [Floor height: two models](#floor-height-two-models) below.

### Floor height: two models

`FUN_80019278` is the runtime floor sampler: given an entity's `(x, z)` it returns the ground height under it. It picks between **two** models per tile, on bit `0x800` of that tile's word in the **object grid** (`.MAP` `+0x8000`, one `u16` per tile - read at `+0x8000 + tile_z*0x100 + tile_x*2`):

| Cell `0x800` | Model |
|---|---|
| clear | **Bilinear nibble surface.** The four corner tiles' elevation tiers through the LUT, weighted by the sub-tile position (`x & 0x7F`, `z & 0x7F`) and `>> 14`. All four corners equal short-circuits to the LUT value. This is flat ground and gentle terrain. |
| set | **Elevation override.** The tile's height is the **flat mean** of its four corner tiers (`sum >> 2`) plus the delta from the tile's [kind-2 trigger record](#trigger-block-0x10000---four-kind-sub-tables): `rec[2] * -0x20` (whole-tile step) + `((rec[3] >> shift) & 3) * -0x10` (per-sub-cell step, `shift = ((x>>6) & 1) * 2 + ((z>>6) & 1) * 4`). No interpolation at all. A flagged tile with no record keeps just the mean. |

**Ramps and staircases are the second model, and only the second model.** A ramp tile's collision nibble carries no useful elevation - Rim Elm's two shore ramps sit on nibble-`0` (sea-level) tiles and hold their entire elevation in the kind-2 records, whose two step fields (`-32` per whole-tile count, `-16` per 64-unit sub-cell) are what make a 128-unit tile a *staircase* rather than a plane. Interpolating a ramp's nibbles instead reads the whole ramp as sea level: an actor walking off the plateau drops the full tier height at the lip and travels **under** the drawn stair mesh. The kind-2 record is not an optional "fast path" layered on the bilinear branch - it replaces it.

Engine port: `World::sample_field_floor_height(world_x, world_z)` carries both branches. Its inputs are the per-scene LUT (`World::field_floor_height_lut`), the collision grid, the object-grid cell words (`World::field_object_cells`, tested against `world::CELL_ELEVATION_OVERRIDE`), and the parsed kind-2 records (`World::field_elevation_overrides`, `world::field_elevation`) - all installed at field entry. The pad locomotion path follows the sample: with `World::follow_terrain_height` set (on by default in `play-window`; `--flat-y` opts out), each committed step snaps the player actor's `world_y` to it, so the player rides slopes and stairs. Field NPCs and props are floor-snapped through the same sampler.

The **base wall + floor data is an on-disc blob**: it is the `+0x4000..+0x8000` region of the per-scene field map file (`DATA\FIELD\<scene>.MAP`), streamed into the field buffer at scene load by `FUN_8001f7c0` (see [Field-buffer load chain](#field-buffer-load-chain)). On top of that base, the field VM's `0x4C` (MENU_CTRL) opcode with outer-nibble 7 (`op0` ∈ `0x70..0x7F`, 7-byte op `[4C, 0x7s, b1, b2, b3, b4, mask]`) applies **story-conditional deltas** - a rectangular paint that sets/clears the high-nibble wall bits over a tile range (`col ∈ [b1, b3+1)`, `row ∈ [b2+1, b4+2)`; sub-op `s` = clear-walkable / block-all / clear-mask / set-mask), gated behind system-flag tests in the prescript.
The nibble-7 op is the same dispatch row in [`script-vm.md`](script-vm.md#0x4c-menu_ctrl---outer-nibble-dispatch).

The `+0x8000` map is a per-tile object/attribute word, not a terrain-flag grid: its low 9 bits index the `+0x0000` object-record table, which `FUN_8003a55c` walks at scene entry to spawn the NPCs/objects occupying each tile. `FUN_8003aeb0` (the field/town scene-entry map-init - note its `town_mode` / `baria_mode` debug strings) ORs the `0x400` footprint flag into these cells from the fallback trigger window's kind-1 records (`+0x12000`, offset/count at `+0x12006` / `+0x12008`, 4-byte records - the gate-0 object-bind entries of the trigger block below).

That `0x400` bit is load-bearing, and the on-disc `.MAP` already carries it (a live town field buffer is byte-identical to the disc bytes here): read on a placed object's **footprint-anchor** tile it says *"this object is the init sweep's - do not re-create it"*, which is how the second placed-object spawner `FUN_801d7b50` stays disjoint from `FUN_8003a55c`. See [The object bind](#the-object-bind-which-sweep-owns-the-object-and-its-rest-pose).

### Trigger block (`+0x10000`) - four kind sub-tables

The `.MAP` file's `+0x10000..+0x12000` region is a **per-tile trigger block**: a shared header dispatching four kind sub-tables. For kind `k`, the sub-table body offset is the `s16` at `+4k+2`, the record count the `s16` at `+4k+4` (both relative to the block start), and the record stride the byte at `DAT_8007B318 + k` - kinds 0..2 are 4-byte records, kind 3 is 8 (`FUN_801D5AE0`; the four sub-tables tile the block back-to-back at exactly those strides in every scene). The generic lookup matches `rec[0] == tile_x && rec[1] == tile_z`:

| Kind | Records | Content |
|---|---|---|
| 0 | `[tile_x][tile_z][dest_x][dest_z]` | **Intra-scene teleports** - the second door class. Crossing onto `(tile_x, tile_z)` repositions the player: `world_x = dest_x*64 + 64`, `world_z = (dest_z + 1)*64` (the destination is in **half-tiles**; landing tile = `dest >> 1`). No object, no script, no record name. Retail then re-samples the floor height, resets the camera, and re-queries the **kind-1** table at the landing tile so the arrival's own record spawns. Engine `legaia_engine_core::field_regions::IntraSceneTeleport`. |
| 1 | `[tile_x][tile_z][p2_record][gate]` | **Partition-2 record triggers.** `gate = 1`: walking onto the tile spawns MAN partition-2 record `p2_record` as a new field-VM context (`FUN_801D1EC4` → `FUN_801D5630(1, x, z)` → `FUN_8003BDE0(x, z, rec[2], rec[3])`, ra `0x801D218C`) - doors and the opening-cutscene records (`map01` / `town01`; the entry SEAT lands on the trigger tile and fires the same tick). `gate = 0`: object-bind entries consumed at scene init (`FUN_8003A55C`), never spawned. The record's own C1/C2 story-flag gates still apply (`FUN_8003BDE0` vs the bitmap at `DAT_80085758`; C1 = block if ANY set, the one-shot mechanism). |
| 2 | `[tile_x][tile_z][coarse: i8][quads: u8]` | **Elevation overrides** - the floor height of every ramp / staircase tile. `FUN_80019278` consults this table via `FUN_801D5630(2, …)` for any tile whose object-grid cell carries bit `0x800`, and uses it **instead of** the bilinear nibble surface: `coarse` is a whole-tile step (`× -0x20`), `quads` packs four 2-bit per-64-unit-sub-cell steps (`× -0x10`). See [Floor height: two models](#floor-height-two-models). Engine `legaia_engine_core::world::field_elevation`. |
| 3 | `[x0, z0, x1, z1, type, 0, 0, 0]` (8-byte stride) | **Region AABB table** - the resumable point-in-AABB scan `FUN_80017FBC` (body at `+0x1000E`, count at `+0x10010` = the kind-3 header slots). Region types feed the region-type bitmask `_DAT_8007B8F4` + the camera zone query `FUN_801DBA20`. Engine `legaia_engine_core::field_regions::RegionTable`. |

The per-tile lookup (`FUN_801D5630`) scans the `+0x10000` primary block first and **falls back to the `+0x12000` window** - the first sectors of the *next* PROT entry (the dev-build `DATA_FIELD<scene>` sibling), which the contiguous `0x28`-sector read from the `.MAP` LBA (`FUN_8001F7C0`) pulls in with the same header shape. Engine: [`field_regions::TileTrigger` / `parse_tile_triggers` / `lookup_tile_trigger`](../../crates/engine-core/src/field_regions.rs) + [`Scene::field_tile_triggers`](../../crates/engine-core/src/scene/scene_ty.rs). See [`cutscene.md`](cutscene.md#record-spawn-mechanisms-live-probe-pinned) for the opening-chain use.

**Engine runtime dispatch.** All three trigger classes run live in the port. The per-frame tile compare is `SceneHost::dispatch_walk_on_trigger` (the `FUN_801D1EC4` port); it quantises `tile = world >> 7` (retail's raw shift at `0x801d2068`, **not** the `(world - 0x40) >> 7` form the region refresh uses - the two agree at tile centres and differ by a half-tile band, and a door tile is only one tile deep), compares against the host's last-tile mirror, and on a crossing runs the kind-1 arm and then the kind-0 arm - the same order retail falls through. A scene entry / warp arrival marks the compare stale so the arrival tile fires on the first tick, matching retail's stale globals.

- **Gate 1 - walk-on record spawn**: a gate-1 kind-1 hit spawns its partition-2 record through `World::install_gated_p2_record` (C1/C2 story-flag gates checked).
  This is how town exits work - Rim Elm's south-gate tiles reference the partition-2 record whose script runs the `0x3F` named scene-change to `map01` - and how walk-on story beats (the post-naming Vahn's-house chain) launch. Skipped while a spawned record / dialog / name entry owns the frame. The dispatch runs in **both** field and world-map mode: on the overworld a gate-1 record that IS a portal (carries a `0x3F`, tested by `SceneHost::p2_record_is_portal`) is left to the world-map entity SM (`OverworldPortal`), and only non-portal **beat** records spawn here - the Drake mist-wall force-walk bands (`map01` P2[34..36], `C1=[0x482]`), which shove the player back while their story flag is clear.
- **Gate 0 - object binds** (`SceneHost::enter_field_scene` install, the `FUN_8003A55C` scene-init consumption): a gate-0 trigger is **not** a tile the player steps on. It is the **lookup key** an `.MAP` *object* uses to find its script: `FUN_8003A55C` walks the object-index map, and for each spawned object looks the kind-1 trigger up at the object's **key tile** (`object_tile + (i8)desc[+0x06], (i8)desc[+0x07]`), then resolves `trigger[2]` as a **flat** MAN record index (`FUN_8003C8F0` with partition base 0 - partitions 0/1/2 concatenated).
  The record becomes the object's script (`actor+0x90` / `+0x9E`), its trailing header byte the anim id (`actor+0x5C`) - and the **flat record index becomes the actor's script id** (`actor+0x50 = trigger[2]`, the `sh t3,0x50(s0)` at `0x8003a8c4`), so a bound object IS a resolvable cross-context target through the `FUN_8003C83C` actor-list walk.
  A record whose first opcode is `0x24`/`0x25` gets its prologue pre-run at bind time (the inline `FUN_801DE840` loop, stopping at a `0x21`, a stalled PC, or a dialog byte) - how the Vahn's-house door context carries its `4C 41` angle-ramp seed before anything pokes it. Engine: `field_channels::spawn_object_channels` + `World::seed_object_channels` (poke-target channels; not autonomously stepped).
  The touch box is therefore the **object's**, not the trigger tile's: `FUN_801CFC40` centres it at `object_world + (desc[+0x06] * 128 + (i8)desc[+0x0E] * 16, desc[+0x07] * 128 + (i8)desc[+0x0F] * 16)` with half-extent `0x40 + 0x10`. Binding at the trigger tile instead makes most doors unreachable - key tiles are routinely inside a wall (Rim Elm's own house-door key tile `(38,25)` is a collision wall). Engine: `field_regions::parse_map_objects` → `man_field_scripts::object_walk_touch_binds` → `World::install_trigger_walk_touch_with_records` (synthetic walk-touch slots from `World::TRIGGER_WALK_TOUCH_SLOT_BASE`); contact routes through the same `check_field_walk_touch` dispatch as placement touches.
  Record headers differ **per partition**, so the flat index must be resolved to its partition before the script offset is computed: P0 `[u8 n][n*2 SJIS name][u8 attr]` (`pc0 = 1 + 2n + 1`), P1 `[u8 N][N*2 locals][4-byte placement header]` (`pc0 = 1 + 2N + 4`), P2 name + three condition blocks (`FUN_8003BDE0`). Engine: `man_field_scripts::flat_record_span`.
- **Kind 0 - intra-scene teleport** (`SceneHost::dispatch_intra_scene_teleport`, the `FUN_801D1EC4` arm at `0x801d21c0..0x801d2268`): crossing onto a kind-0 tile seats the player at the record's landing (`dest_x*64 + 64`, `(dest_z + 1)*64`), re-samples the floor height, and leaves the last-tile compare stale so the landing tile's own kind-1 record fires next tick (retail queries it inline and runs a ~`0x26`-frame fade across the reposition; the engine warps instantly). Skipped while the player's movement-disabled flag (`+0x10 & 0x80000`) is set. This is the class most house **exits** belong to - see [Intra-scene doorways](#intra-scene-doorways---the-walk-touch-teleport-family).
  Retail gates both arms on the crossed tile's object-index word: `cell & 0x600 != 0` (`0x801d2140`) - a fast filter in front of the table scan. Every kind-0 trigger tile on the disc carries those bits, so the engine's exact-match table lookup subsumes it.

The partition-2 gate bitmap (`DAT_80085758`) **is** the field VM's `0x50`/`0x60`/`0x70` system-flag bank - one store, shared by the record dispatcher's C1/C2 test (`World::p2_gate_flag_set` = `system_flag_test`) and the VM's flag writes, so an opening-timeline `set` is immediately visible to the next record's gate. It also overlaps the saved story-flag window at byte `+0x158` (`0x80085758 - 0x80085600`); the engine save mirrors the bank into that window and reloads seed it back. Disc-gated coverage: `crates/engine-core/tests/walk_on_trigger_dispatch_disc.rs` (opening-to-free-roam progression, south-gate exit to `map01`, house-door contact teleport, ambient no-lock, gate-flag save round-trip).

**The C1 one-shot-latch idiom.** A walk-on beat that should play exactly once self-latches: its script `0x50 SET`s the very flag its `C1` lists, so the beat runs on the crossing that finds `C1` clear and its own `set` then blocks every later crossing (`C1` = block-if-ANY-set). The `town01` dinner chain is the canonical example - `P2[4]` (`C1=[550]`, sets `550`), `P2[5]` (`C1=[551]` `C2=[550]`, sets `551`), each a link that latches as it completes. The `step_cutscene_timeline` port applies those `set` ops in the system bank as the record runs (proven by `550` latching after the beat), so a completed timeline stops its own re-fire.

Two variants sit either side of that idiom. A record whose `C1`/`C2` are **empty** is spawned on *every* crossing by design and self-manages via an internal `0x70 TEST`/`0x50 SET` on a private flag (`town01` `P2[6]`: `TEST 558 → … → SET 558`, jumping straight to its end while the guard flag is clear, so it is a no-op rather than a re-fire lock). The overworld mist-wall bands (`C1=[0x482]`) invert the sense - they carry no `set`, staying live until an *external* story event sets `0x482`, after which the `C1` gate blocks them.

**Spawned-record execution semantics.** On-disc partition-2 records have **no end opcode**; the engine's modal cutscene-timeline stepper (`World::step_cutscene_timeline`) recovers a completion point three ways:

- **Choreography wrap**: an `Advance` jumping backward onto an already-executed PC completes the timeline. Records finish either in a tight `Nop`+`JmpRel`-to-self park (the fog-config / flag-reset ambients - `town01` P2[16]/P2[21]/P2[22], spawned on every crossing of their gate-1 tiles) or by looping back to their conversation top as a **resident actor-driver** (the Mei walk-on beat's op-`0x45` APPLY jump). Retail leaves both spinning as parallel contexts, invisible to the player; the modal timeline completes there instead so control returns. Real waits (`0x4A` WAIT_FRAMES, flag-test handshakes) `Halt` at their own PC and never trip the rule.
- **Inline dialog boxes**: a record byte with `& 0x7F < 0x20` at the PC is the retail dialog-SM transition (`FUN_80039B7C`), not an opcode. A `0x1F` lead opens a dialog panel over the record bytes and parks the timeline (frame cap frozen - a dialog waits on the player); confirm dismisses the box / commits a picker choice, resuming past the segment. Stray terminators (`0x00..0x1E`) the flow lands on are consumed.
- **Cross-context target space**: a record's `0x80`-bit ops resolve against BOTH channel families - the partition-1 placement contexts (`script id = N0 + placement`, `FUN_8003A1E4`) and the `.MAP` object-bind contexts (`script id = flat record index`, `FUN_8003A55C` - see the Gate-0 bullet above).
  The `town01` Mei walk-on beat (`P2[4]`) uses both: `CC 46 51 11 1D 00 3C` seats placement 34 (Mei) at the Vahn's-house door tile `(17,29)` - the poke that makes her VISIBLE for the conversation - while the `CC 01 …` ops swing the door object (flat record 1).
  A spawned-record channel poke of the `4C 51` family SEATS the target exactly (retail's run dispatch settles on the op target, the same pin as the entry pre-run), hide-box `(127,127)` seats included - the beat's closing choreography despawns Mei that way, and retail keeps her hidden until the next scene entry re-runs her prologue.
  An id that matches NO channel is skipped by its decoded width - running it against the timeline's own context corrupts the caller (a `B1 <id> 00` sets the caller's own busy bit, and the `CC <id> A0` busy-wait then hijacks the caller PC into the record header). Resolved-channel `4C A0` busy-waits fall through unconditionally: engine channel pokes complete synchronously, where retail's channel clears its own busy bit as its move plays out. Disc oracle: `crates/engine-core/tests/field_npc_entry_positions_disc.rs` (the Mei-beat test).

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
reads it via `Scene::field_object_placements`. Each object's drawn mesh is the
object record's `+0x10` `u16` field - **uniformly, for every object id** (retail
`FUN_80020f88`: `actor+0x64 = record[+0x10] + DAT_8007b6f8`; the id selects the
*record*, never the mesh). Ids `1/2/3` are the protagonist/NPC meshes from the
shared pool. The `anim_id` resolved separately via the object bind
(`func_0x801d5630`, below) never picks geometry - it *poses* it.

> A positional "field-actor band" reading (`pack_index = obj_idx - 5` for ids
> `93..=118`) is **falsified**. Rim Elm cell `(30, 17)` carries object id `99`
> whose record `+0x10 = 2`, and the retail GPU prim pool draws that cell's
> surface from env-pack mesh **2**: the quad's `cba = 0x7D00` / `tsb = 0x000C`
> and its UV set match mesh 2's primitive byte-for-byte, and its four screen
> vertices are exactly that cell's four corners. The band rule silently swapped
> ten town meshes per Rim Elm map - among them the terrain slab south-east of
> the spawn, whose absence left the ground's clear colour showing through.
> (Its supporting "windmill id 96 -> mesh 91" datum does not survive contact
> with the disc: record `96` is all zeros, and no cell references it.)

### The object bind: which sweep owns the object, and its rest pose

A placed record becomes an actor through **one of two sweeps**, never both.

`FUN_8003a55c` (SCUS, scene init, whole grid) resolves the record's **object
bind** by its *footprint-anchor* tile (`col + record[+0x06]`,
`row + record[+0x07]`): `func_0x801d5630(1, anchor_col, anchor_row)` returns the
`.MAP` kind-1 tile-trigger entry sitting on that tile, whose `record` byte indexes
the MAN's flat record-offset table - partition 0 comes first in that table, so it
names a **partition-0 record**. When the lookup misses, `FUN_8003a55c` skips the
tile.

`FUN_801d7b50` (field overlay, the sub-area **window rebuild**: it frees the whole
actor list and re-populates it from the cells inside the current window) does the
same placed-flag grid walk with **no bind lookup at all**. Its only extra gate is
the `0x400` footprint bit on the anchor tile (`801d7ccc: andi v0,v0,0x400` ->
`bne` skips the tile) - the bit `FUN_8003aeb0` stamps into the object-index grid
from the gate-0 bind triggers. So it creates exactly the records the init sweep
did *not*.

The two sets are complementary on the disc: across `town01` / `town0c` / `koin3` /
`map01`, every placement whose anchor tile carries a bind trigger also carries
`0x400` (37 / 58 / 6 of them), and every placement without one has the bit clear
(9 / 5 / 0). **The union is every placed record**, so a whole-map renderer draws
them all - the bound ones posed by their bind's clip, the rest raw. Rim Elm's
cavern shell (record `168` at cell `(32, 93)`, env mesh `72` - a ~3100 x 4000-unit
round chamber with an entry corridor) is the window sweep's, and reading the bind
as a *spawn gate* deletes the cave interior outright.

The init sweep's half is live-verified against a Rim Elm capture's actor list: its
37 static-object actors are exactly `town01`'s 37 bound placements, and each
actor's `+0x5C` equals the anim id its bind resolves.

- **The bind carries the object's animation id.** A partition-0 record's header
  is `[u8 n][n*2 name bytes][u8 anim_id]` (its own shape - the partition-1
  `1 + 2n + 4` formula desyncs here); `FUN_8003a55c` stores the record base into
  `actor+0x90`, the post-header offset into `actor+0x9E`, and that trailing byte
  into `actor+0x5C`. The window sweep does none of this, so its actors have no
  script and `+0x5C == 0`.

The anim id decides *how the mesh is drawn*. With `+0x5C == 0` the actor stays
at draw kind `5`, whose `FUN_8001ada4` arm draws every TMD object of the mesh
with the actor's single transform - right for a single-object prop. With a
nonzero id the per-actor anim tick `FUN_800204f8` binds scene-ANM record
`anim_id - 1` (bundle base `DAT_8007b75c`) into `actor+0x4C` and flips the actor
to draw kind `1`, whose walker `FUN_8001b964` applies the clip's **per-bone
rigid transform to each TMD object** before drawing it, and refuses to draw at
all unless bone count equals object count.

So a **multi-object placed prop is posed, not stamped**: its TMD objects are the
clip's bones, authored about their own pivots, and the clip's **frame 0 is the
rest state**. Rim Elm's searchable cupboard (object id `230`, env mesh `15`) is
the clearest one - three objects (cabinet + two doors) driven by a 3-bone,
30-frame clip whose frame 0 closes the doors flush into the cabinet's front
opening and whose later frames swing them open. Drawn unposed, the door objects
hang at the cabinet's mid-depth and sink through the floor.

Parsers: `legaia_engine_core::field_env::object_binds` (the bind lookup +
header decode; the anchor tile is `Placement::anchor_col`/`anchor_row`) and
`resolve_placed_env_draws` (the spawn gate + `EnvDraw::anim_id`). Disc-gated
coverage: `crates/engine-core/tests/field_object_binds_disc.rs`.

### The door swing: how a bind script drives the clip

The bind record is not just a header carrying an anim id - it **is** the prop's
field-VM script, and running it is what opens a door. Its passes are delimited
by the `0x21` park opcode, and it drives the clip entirely through the anim
control word `actor+0x62` plus the rate `actor+0x6A`.

The per-frame advancer is `FUN_800204f8` (called from the actor tick
`FUN_80021df4`). It rebinds `actor+0x4C` whenever the requested id `+0x5C`
differs from the bound id `+0x5E`, then walks the **frame cursor** `actor+0x68`,
which is in **1/16-frame units** - `FUN_8001b964` poses from
`frame = (i16)(actor+0x68) >> 4`. The step is `actor+0x6A` (scaled by the clip's
own `clip[1] & 1` / `clip[6]` divisor when set). `actor+0x62` selects the mode:

| bit | name | effect in `FUN_800204f8` |
|---|---|---|
| `0x0002` | hold | skip the cursor advance - the clip freezes |
| `0x0008` | clamp | stop at the clip's end; clear = wrap (loop) |
| `0x0080` | reverse | count the cursor down instead of up |
| `0x0100` | end | latched by the tick when the cursor reaches an end |
| `0x0200` | restart | consumed by the next tick: snap to frame 0 (or the last frame, reversed) |

The field-VM ops that write them are `0x2B <bit>` (set), `0x2C <bit>` (clear) and
`0x2D <bit>` (test / spin) - all three address `actor+0x62`, not the per-actor
flag word `actor+0x10` (that is `0x31`/`0x32`/`0x33`). `0x4C` nibble-4 sub-1
writes the rate (`+0x6A = max(1, operand >> 1)`), and `0x4C` nibble-3 sub-5 /
sub-6 are the two pose snaps: `+0x62 = (+0x62 & !reverse) | 0x20A` (restart at
frame 0, one-shot, **hold**) and `+0x62 |= 0x28A` (restart at the *last* frame,
reversed, one-shot, hold). `0x22 <id>` is SET_ANIM (`+0x5C`, forces a rebind via
`+0x5E = 0xFFFE`, and picks draw kind `1` / `5` by whether the id is nonzero).

An actor is born with the placed-object template `DAT_80073E70`'s
`+0x62 = 0x0015` (no hold, no clamp - i.e. **looping**) and `+0x6A = 0x10`, which
`FUN_8003a55c` halves to `8`. So the passes read:

- **spawn** - `FUN_8003a55c` runs the record's prologue itself (its loop stops on
  the first `0x21`). A door's is `0x4C 0x41 <rate>` then `0x4C 0x35`: rate `16`
  (one frame per tick), then reset-and-hold. The door is shut and frozen. A prop
  whose prologue carries **no** `0x4C 0x35` keeps the looping template flags and
  turns forever - that is Rim Elm's windmill (`風車`).
- **touch** - resumed when the player's body hits the prop (`FUN_801cfc40` links
  the two actors through their `+0x98` partner slots; `FUN_801d5b5c` posts the
  engagement and the dialog SM `FUN_80039b7c` runs the touched actor's parked
  script through the dispatcher). A house door's pass is a creak
  (`0x36` sub-`0x8000` → the SFX cue player `FUN_80035b50`) then
  `2C 07` / `2C 01` / `2B 03` - clear reverse, **clear hold**, set clamp - then
  **`31 00`** (CFLAG_SET bit 0 on `+0x10`: the door leaves the collision
  candidate list as the swing starts - see the `FUN_801cf754` `flags & 3`
  filter above), then `2C 08` / `2D 08`, which spins until the tick latches the
  end. The clip plays forward and clamps open, and the opened door neither
  blocks nor re-fires.

Rim Elm's cupboard continues past that spin with its **search body** - a
`70 xx` searched-flag guard, `50 xx` flag SET + `39 xx` GIVE_ITEM, the `0x1F`
message segments ("There's a `C2 xx` in the cupboard!" on the fresh arm, "The
cupboard is empty!" on the guarded one; `C2` = the item-name escape matching
the granted id) - and only then the closing segment (`2B 07` / `2C 01` /
`2B 03`, set reverse and play again). Because the script resumes only when the
pager returns, the doors swing shut **after the message is dismissed** - and
every idle capture finds them closed. Locked-house doors (`town01` P0[1]) are
the same shape with story-flag arms: while locked, the touch pass shows "The
house is locked..." and never reaches the open ops (nor the `31 00`), so a
locked door stays solid.

Live PCSX-Redux Rim Elm captures read exactly those words back off the actor
list: a resting door is `+0x62 = 0x001F` / cursor `0`, the door the player is
standing at is `+0x62 = 0x011D` / cursor `479` (`= 30 * 16 - 1`, the last frame
of the 30-frame swing), and one that has played back shut is `+0x62 = 0x019D` /
cursor `0`. The scene's NPCs sit at the untouched template `0x0015`.

Engine port: `legaia_engine_core::field_env` - `PropAnim::tick` (the
`FUN_800204f8` arithmetic), `decode_prop_program` (the record's spawn/touch
command shape + the `0x31` class bits), and `PropAnimBank`, which holds one
cursor **per placement** (so touching one cupboard leaves its three siblings
shut) plus each prop's record, parked cursor and `+0x10` word. The touch /
interact dispatch runs the record itself through the field VM
(`World::start_prop_interaction`, `world/prop_interact.rs` - see the engine
bullet list above), so the search body, the collision drop and the
close-on-dismiss sequencing are the script's own. The play-window keeps the
baked frame-0 mesh for every prop at rest and re-poses only the ones whose
clip is running. Disc-gated coverage:
`crates/engine-core/tests/field_prop_anim_disc.rs`. Raw record evidence:
`cargo run -p legaia-engine-core --example dump_prop_scripts -- town01`.

The record's `+0x1E` byte is **not** part of this: it is the object's cull
radius in `0x40` units, copied to `actor+0x58` and read by the screen-space
bounding-box cull `FUN_8001b73c`.

See [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

## Provenance

- Controller `FUN_801d01b0`, position writes `0x801D0684 / 06E4 / 0744 / 07B4` - see `ghidra/scripts/funcs/overlay_0897_801d0684.txt`; the touch/interact dispatch body (`0x801d07c0..0x801d08dc`) in `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d01b0.txt` (the `overlay_0897` copy is garbled in this region).
- Collision `FUN_801cfe4c`, finer probe `FUN_801cfc40`, interaction `FUN_801cf9f4` - `ghidra/scripts/funcs/overlay_0897_801cfe4c.txt`, `overlay_0897_door_801cfc40.txt`, `overlay_0897_801cf9f4.txt`. Candidate-list builder `FUN_801cf754` (the `flags & 3` skip + ±`0x180` cull + `0x20` cap) - `ghidra/scripts/funcs/overlay_0897_door2_801cf754.txt`. Touch-post side-band `FUN_8003d038` (`DAT_80073F1C`) - `ghidra/scripts/funcs/8003d038.txt`; its motion-VM consumer at `0x8003882C` inside `ghidra/scripts/funcs/80038158.txt`.
- Pad remap `func_0x800467e8`, direction mask `FUN_80046494` - `ghidra/scripts/funcs/800467e8.txt` / `80046494.txt`.
- Scene-entry map-init `FUN_8003aeb0` (height LUT fill, `+0x8000` footprint OR, player-actor setup) - `ghidra/scripts/funcs/8003aeb0.txt`. Object spawn iterator `FUN_8003a55c` (low-nibble floor-height read, `+0x8000` index walk) - `ghidra/scripts/funcs/8003a55c.txt`.
- Floor sampler `FUN_80019278` (both height models: the `cell & 0x800` elevation-override branch and the bilinear nibble branch) - `ghidra/scripts/funcs/80019278.txt`. Its kind-table lookups `FUN_801D5630` / `FUN_801D5AE0` - `ghidra/scripts/funcs/overlay_cutscene_mapview_801d5630.txt`, `ghidra/scripts/funcs/overlay_0896_801d5ae0.txt`.
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
Per-mesh **world placement** for this static geometry is the [Object-record table](#object-record-format-0x0000-0x20-byte-stride) above (`FUN_8003a55c`: the object-index grid at `+0x8000` of the field map file selects a `+0x0000` object record per placed tile, giving the mesh its world translation; `legaia_asset::field_objects` parses it, `Scene::field_object_placements` exposes it). Each object's mesh resolves to a scene-pack index from the record's `+0x10` field (every object id; see the falsified band rule above). Per-tile **world Y** = `-floorHeightLUT[tile_nibble] + y_off`, the LUT being 16 `s16` at the MAN header `+0x02` (`Scene::field_floor_height_lut`). `legaia-engine play-window` renders the town from this:
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

When `World::live_gameplay_loop` is set, locomotion feeds the encounter system directly: `World::live_field_tick` treats the player crossing into a new 128-unit collision tile (`pos >> 7`) as one *step* and drives a single `World::on_field_step` roll, mirroring the retail per-step counter rather than rolling every frame. A successful roll transitions `Field → Battle`; on victory the field actor table is restored and the player resumes where they stood. See the [live gameplay loop](battle.md#live-gameplay-loop---field--battle-in-tick) section in `battle.md` for the full round trip.

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

## Intra-scene doorways - the walk-touch teleport family

Walking into a town house is **not** a scene change. The scene name buffers
(`0x8007050C` / `0x80084548`) are unchanged across the warp: the interior is a
sub-area of the *same* 128×128 collision grid, parked in an otherwise unused
corner of it, and the door merely repositions the player.

**There are two door mechanisms, and one house can use one of each.**

1. **Script doors.** A `.MAP` **object** whose key tile resolves a gate-0
   kind-1 trigger to a MAN record (`FUN_8003A55C`); that record's script
   cross-context-teleports the *player* channel. The player fires it by
   touching the **object's** contact box. This is the ＩＮ / ＯＵＴ family
   described below.
2. **Map doors** - the `.MAP` **kind-0** intra-scene-teleport table. No object,
   no script, no record name: a plain tile carries a destination, and crossing
   onto it repositions the player (`FUN_801D1EC4`'s kind-0 arm; see
   [Trigger block](#trigger-block-0x10000---four-kind-sub-tables)).

Class 2 is the **larger** of the two and is where most house *exits* live -
a MAN-only census cannot see it at all, because there is nothing in the MAN to
see. Rim Elm's own Vahn's-house door is the mixed case: a script door in, a map
door out.

### The three player-move op forms

A door record repositions the player by addressing the **player system
channel** `0xF8` from another context. Three distinct ops do it, and a decoder
that knows only the first misses two thirds of the door surface:

```text
A3 F8 <xb> <zb>                 ; op 0x23 | 0x80  MOVE_TO      - instant teleport
CC F8 51 <xb> <zb> <depth> <mv> ; op 0x4C nibble-5 sub-1       - teleport + move anim
C7 F8 <xb> <zb> <mode>          ; op 0x47 | 0x80  walk-to-tile - animated glide
```

- The `0xF8` prefix is the entire discriminator between a door and an NPC's
  self-placement: a **plain** `0x23` moves the *executing* actor (a prop
  positioning itself), and the disc carries hundreds of those. Any census of
  doors must filter on the channel byte.
- World coords are the usual `(b & 0x7F) * 0x80 + 0x40` (+`0x40` when bit 7 is
  set).
- `0x23` and the `0x4C` nibble-5 sub-1 form are **teleports** (the position is
  written outright); `0x47` is an animated walk to a tile and is what a landing
  choreography uses to step the player away from the door it arrived at.
- The `0x4C` form's `depth & 0xF` is its own facing index; otherwise the
  arrival facing comes from a preceding `B8 F8 <dir> 00` (op `0x38 | 0x80`
  CAM_CFG, simple path `op1 & 0x7F == 0`), which copies the SCUS compass LUT
  entry `0x80073F04 + (op0 & 0xF) * 2` into the player's `+0x26` heading.
  Retail authors ＩＮ records with LUT index 4 (into the room) and ＯＵＴ
  records with index 0 (back out into the street), so the player never emerges
  facing the door just used.

Disc-wide, the trigger-bound records carry all three in quantity - the `0x23`
form is not even the most common. Census tool:
`cargo run -p legaia-engine-core --example scan_door_triggers` (no args = every
CDNAME scene, with a per-class count).

### The record is a branch, not a constant

A door record is a **script**, and the retail door surface uses that: the same
key tile runs different arms depending on story flags. `town01`'s
主人公の家の中 ("inside the protagonist's house") is three arms deep - a
`0x7x` TEST chain over flags `0x226` / `0x227` selecting between a plain
teleport and a `0x44` SPAWN_RECORD of the in-house dinner beat. Taking a
record's *first* teleport unconditionally therefore fires the wrong arm for
every story-gated door.

The engine resolves the arm at **contact time**, against live flags, by
walking the record from its script start following the VM's real branch
semantics (`SysFlag.Test` jumps when the flag is **set**; `JmpRel` follows;
a revisited PC = the trailing idle park, stop):
`man_field_scripts::resolve_walk_touch_event`. The first player teleport it
reaches is the door's landing; a `0x44` SPAWN_RECORD it reaches instead makes
the touch spawn that record as a field-VM context
(`WalkTouchEvent::SpawnRecord`), which is how a door that leads into a cutscene
works.

### The pairing convention

Doorway records pair by their fullwidth SJIS record name: `…ＩＮ` / `…ＯＵＴ`
(digit-suffixed when an inn has several exits), 入口 / 出口 for gates, Ａ / Ｂ
for elevator endpoints. The ＯＵＴ record **is** the return trip - a door in
its own right, bound by its own object, warping the player back to the
doorstep. An exit may own **several** objects (a wide doorway), so a scene can
carry more ＯＵＴ contacts than ＩＮ ones.

The ＩＮ/ＯＵＴ naming is a convention, not the mechanism, and it does not
cover the whole family: an exit can live in **partition 2**, reached through a
gate-0 record's SPAWN_RECORD arm rather than through an object of its own.

### Geometry

The gate-0 kind-1 trigger tile is the object's script **lookup key**, not a
tile the player steps on - it is routinely a wall (Rim Elm's house-door key
tile `(38,25)` is a collision wall). The contact box is the object's
(`FUN_801CFC40`; see the gate-0 bullet under
[Trigger block](#trigger-block-0x10000---four-kind-sub-tables)), centred at the
object world position offset by its descriptor's coarse `(desc[6], desc[7])`
tile deltas plus fine `(desc[0x0E], desc[0x0F])` 16-unit deltas, half-extent
`0x50`. Each landing is placed clear of the paired door's contact box, so an
arrival cannot immediately re-fire the door it came through - the ping-pong a
naive box model would otherwise produce is authored out in the data, not
guarded in code.

### Landing records

The tile a door lands on frequently carries a *gate-1* trigger of its own -
サウンド内 / サウンド外 ("sound inside/outside") ambience switches, 閉扉
("closed door"), or a story beat. These spawn as ordinary partition-2 records
on arrival; a landing record that opens an inline dialog box parks the frame
until the player confirms, exactly as retail does.

### Rim Elm

`town01` / `town0b` / `town0c` are three story states that share one
partition-0 table and one `.MAP` trigger table. Two complete object-bound
doorway pairs - 恋人ＩＮ/恋人ＯＵＴ (Mei's house) and 木ＩＮ/木ＯＵＴ (the
tree) - plus 主人公の家の中, Vahn's own house.

Vahn's house (主人公の家の中) is the **mixed** case, and it is not an exit-less
story-entry warp:

- **In** = a script door. The P0 record's arms are: flag `0x226` clear →
  `A3 F8` teleport to interior tile `(97,10)`; `0x226` set and `0x227` clear →
  SPAWN_RECORD the in-house beat (`town01` P2[5], which seats the player with
  the `0x4C` nibble-5 sub-1 form and later walks him with `0x47`); both set →
  teleport. The door object is **recessed** - the collision grid walls its
  contact box on three sides, leaving one walkable channel due north of it.
- **Out** = a map door. The `.MAP` kind-0 record at interior tile `(97,9)`,
  one tile back toward the doorway, lands the player at half-tile `(72,46)` =
  world `(4672, 3008)` = tile `(36,23)`, the doorstep. **No story flag gates
  it**; it is live on a cold `town01` entry.

There is no ＯＵＴ *record* for Vahn's house anywhere in the MAN, and there
never was one to find: the exit is not a MAN record. That is why a byte-scan
over partition 0 for `A3 F8` reads as "an ＩＮ with no ＯＵＴ" and concludes,
wrongly, that the door is a story-entry warp.

Disc-wide the kind-0 class is large: **2330 records across 73 scenes** (`nilboa2`
alone carries 128), against 114 `0x23` + 67 `4C 51` + 207 `0x47` player-move ops
in the trigger-bound MAN records. An engine that dispatches only the MAN doors
lets the player into most interiors and never back out.

### Engine port

`man_field_scripts::object_walk_touch_binds` joins the `.MAP` object layer to
the trigger table and the flat MAN record space; the binds install in
`SceneHost::enter_field_scene` (keyed at each object's contact centre, with the
record index kept alongside); `World::check_field_walk_touch` re-resolves the
record's arm against live story flags on contact
(`man_field_scripts::resolve_walk_touch_event`) and applies position + facing +
a fresh floor-height sample (the interior sits at its own elevation on the
shared grid, so the landing must be re-seated on the floor rather than keeping
the doorstep's height), or spawns the record the arm names.

Map doors run through the tile-crossing dispatch instead:
`Scene::field_intra_scene_teleports` caches the kind-0 tables at scene load and
`SceneHost::dispatch_intra_scene_teleport` seats the player on a crossing.

Disc-gated coverage: `crates/engine-core/tests/rim_elm_door_roundtrip_disc.rs`
(the script-door pairs: both members install with their decoded target and
facing in all three Rim Elm scenes, the pairs are reciprocal and cannot re-fire,
and the locomotion walks each doorway in and back out);
`crates/engine-core/tests/vahn_house_roundtrip_disc.rs` (the mixed case - pad-walk
in through the script door and back out through the map door, no story flags);
`crates/engine-core/tests/walk_on_trigger_dispatch_disc.rs`.

## Open

- The `FUN_801d5b5c` post kernel's facing save/restore (`+0x26` -> `+0x5A`) and touch counters (`+0x2A` / `_DAT_801c6ea4+0xA`); the engaged flag and the parked-script resume are modelled (`world/prop_interact.rs`).
- Full per-actor field-VM channel execution with story-flag-conditioned branches (the engine loops decoded waypoint lists, and the initial-facing decode takes the fall-through branch - see [NPC initial facing](#npc-initial-facing) - rather than evaluating the prologue's `0x7x` flag-TEST chain against live flags, so a later-chapter branch's facing/position is not selected). The **door** path does evaluate its branches live (see [Intra-scene doorways](#intra-scene-doorways---the-walk-touch-teleport-family)); the general actor path does not yet.

## NPC initial facing

The placement record carries **no facing byte** - its 4-byte header is `[model, anim, tile_x, tile_z]` only. A never-walked NPC's heading comes from a **spawn-time prologue pre-run**: the placement installer `FUN_8003A1E4` ends by executing the record's leading field-VM ops one at a time through `FUN_801DE840` when the first opcode is the `0x24`/`0x25` spawn-prologue marker, stopping at a `0x21` NOP terminator or any below-`0x20` byte (body `0x8003A474..0x8003A4F8`; see `ghidra/scripts/funcs/8003a1e4.txt`).

Two prologue ops write the actor's `+0x26` render heading from the 8-direction LUT at SCUS `0x80073F04` (entry `i` = `i * 0x200`; the LUT has 16 addressable slots but only 0..=7 are direction entries):

- `0x4C 0x51` (nibble-5 sub-1, the NPC move-to-tile op): the dispatcher writes `+0x14/+0x18` from the tile bytes **and** `+0x26 = table[b3 & 0xF]` - operand byte +3's low nibble is the facing index (`overlay_0897_801de840.txt`, case 5 sub 1);
- `0x38` CAM_CFG **simple path** (`op1 & 0x7F == 0`): `+0x26 = table[op0 & 0xF]`.

The heading space itself is pinned from the locomotion's pad→facing writes (`FUN_801d01b0` body `0x801d04b8..0x801d0548`): retail `0` = Z-, `0x400` = X-, `0x800` = Z+, `0xC00` = X+ - the engine's `render_26` convention (`0` = Z+) rotated a half-turn, `engine = (retail + 0x800) & 0xFFF`, no axis mirror.

Town prologues route the facing leg through a story-flag `0x7x`-TEST branch chain (jump when the flag is **set**), so the fall-through branch - the first leg in linear record order - is the fresh-game state.

The engine decodes that leg statically per placement ([`man_field_scripts::placement_initial_facing`](../../crates/engine-core/src/man_field_scripts/npc_motion.rs), skipping cross-context and park-sentinel legs), converts through [`facing_index_to_engine_heading`](../../crates/engine-core/src/man_field_scripts/npc_motion.rs), and seeds `World::field_npc_headings` at scene entry (`World::seed_field_npc_facings`) - a later walk overwrites the slot exactly as retail's per-step facing writes overwrite `+0x26`. Semantic pin: town01's side-by-side villager pair at tiles `(29,22)`/`(30,22)` derives LUT indices 6 (X+) and 2 (X-) - they face each other; disc-gated coverage in `field_npc_initial_facing_disc.rs`.

Note the facing pin also fixes what `0x4C 0x51` operand byte +3 **is**: bit 7 toggles the special-model flag, the low nibble is the facing-LUT index - and the raw case-5-sub-1 asm reads the byte **nowhere else**, so the op carries no speed operand (byte +4 is the move-anim id written to `+0x5C`; the trailing `FUN_801D81E0` is an active-list relink via `FUN_800204A4`/`FUN_80020454`, not a bytecode builder). The old glide-speed reading of the same byte is a misattribution of the walk-kernel op `0x47`'s own operand encoding - see the reconcile note under [NPC glide speed](#npc-glide-speed).

## NPC glide speed

An NPC's per-frame glide is NOT the player's `+0x72` walk step (that premise is falsified: `FUN_8003774C` never reads `+0x72`). Both walk kernels encode the base step **in the walk op's own operands**, on the shared ladder `numerator >> (2 + bits)` units per frame (base steps 32 / 16 / 8 / 4 / 2 / 1 for `bits` 0..5 at numerator `0x80`, floored at 1):

- **Field-VM yield ops** (`FUN_8003774C` - scripted glide legs): per-frame magnitude `_DAT_1f800393 × numerator / (4 << bits)`. `bits = (op0>>5 & 4)|(op1>>6)` for the axis-glide ops 0x37/0x41, `b2 & 7` (high nibble = approach-mode selector) for the walk-to-tile op 0x47. The numerator is `0x80` for 0x37/0x47 but **`0x40` for 0x41** - half speed, the `li a1,0x40`/`li a1,0x80` split at `0x80037908`. `_DAT_1f800393` is taken at its cold-field value 1.
- **Tail-section-1 motion streams** (`FUN_80038158` - the ambient town-NPC wander; see [motion-vm.md](motion-vm.md#the-second-motion-vm---fun_80038158)): the directional steps 0x03/0x19/0x20 carry `bits` in operand byte 1's low nibble; the pad-echo step 0x06 and the AABB wander 0x18 scatter a 4-bit selector over their four operand bytes' high bits (`(b1&0x80)>>4 | (b2&0x80)>>5 | (b3&0x80)>>6 | b4>>7`). All step `0x80 >> (2 + bits)`.

There is **no synthesised motion bytecode** for the yield ops: 0x37/0x41/0x47 are the field VM's own yield-class opcodes. The dispatcher parks the op's instruction pointer at actor `+0x94` (progress cursor `+0x54`, HALT flag `0x400`) and `FUN_8003774C` interprets the record bytes in place each frame, resolving the same `0x80` extended-target convention as the field VM ([script-vm.md](script-vm.md) § 0x37-0x42).

The engine decodes each placement's glide speed from those real operands off the disc:
[`man_field_scripts::placement_glide_speed`](../../crates/engine-core/src/man_field_scripts/npc_motion.rs)
tries the placement's bound tail-section-1 stream first (`placement_wander_step` - binding id = `N0 + placement_index`, default variant first),
then the record's own pre-text field-VM yield ops (`placement_yield_step` - own-context only, with the park-sentinel/locality filters on a 0x47's target),
maps the selector through [`World::field_npc_walk_step_speed`](../../crates/engine-core/src/world/config.rs),
and stashes it in `World::field_npc_glide_speeds`. `World::start_field_npc_motion` writes that into the leg's motion-VM `speed`.
Disc-gated `field_npc_glide_speed_disc.rs` pins town01's wandering villagers to their 0x18-decoded steps
(e.g. binding `0x30` = slot 12, `bits` 3 = step 4) and the plaza nudge NPCs to their 0x41-decoded step 16.

A placement with no walk-kernel op in either carrier falls back to the facing-nibble **heuristic** (`facing_nibble_glide_speed`: the first local `4C 51` leg's byte-+3 low nibble through `field_npc_glide_speed`); a placement with no decodable motion leg at all (and the actor-VM sprite glide, which has no MAN motion operand) falls back to the `FIELD_NPC_MOTION_SPEED` stand-in (base step 8), so the default path is unchanged.

Modelling note (reconcile outcome): the raw `4C 51` handler pins its byte +3 as `[bit7 special-model | facing nibble]` with **no speed field** - retail `4C 51` is a teleport + move-anim start, and the only speed-carrying ops are the walk kernels' own operands (above). The heuristic arm therefore reads a *facing nibble* as the base-step selector - a stable per-NPC variation with no retail speed semantics - and fires only when no real walk-kernel op decodes.

## Engine port: movement compass + opt-in precise movement

The engine mirrors retail's camera-remapped pad in `World::step_field_locomotion` (`decode_field_direction`): the held d-pad is rotated by `World::field_camera_azimuth` **quantised to the nearest 90°** - the same job `func_0x800467e8` does - and stepped through the per-axis collision above. The azimuth feed is `Camera::compass_azimuth_units()` (engine-core): scripted yaw + the user's `manual_orbit` (the play-window's left-mouse drag-orbit) + the host renderer's `render_yaw_bias` (the follow camera's fixed base yaw, compass sense = the negated PSX render yaw), pushed into the world each `BootSession::tick`. All three terms default to 0, so headless hosts keep the identity remap.

Two non-retail, opt-in knobs layer on top (play-window keybinds, persisted in `legaia-options.toml`):

- **Camera distance** (`Camera::distance`, presets retail / far / farther; `T` cycles) - a pure framing scale on the follow camera's eye-back depth. Never feeds the simulation; the engine-core default stays `retail` so oracle/replay paths are bit-identical, while the windowed host defaults to `far`.
- **Precise movement** (`World::precise_movement`; `R` toggles, default off) - swaps the quantised remap for a continuous decode (`decode_field_direction_precise`): the azimuth rotates the screen vector at full angular resolution, key diagonals walk true 45° vectors at normalised speed (no ×0.75 cut - the vector itself is unit length), and a deflected analog stick (`InputState::lstick`) passes its angle through. The step still routes through the same 2-unit per-axis collision probes (`advance_with_collision_vector`, Z before X per sub-step), with a sub-step remainder carried across frames so shallow angles keep their exact slope.

## See also

**Reference** -
[Field/event VM](script-vm.md) ·
[World map](world-map.md) ·
[Scene bundles](../formats/scene-bundles.md) ·
[Scene v12 table](../formats/scene-v12-table.md)
