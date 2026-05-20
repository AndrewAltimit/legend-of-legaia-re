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

After movement, the same function runs an interaction probe (`FUN_801cf9f4`, a `0x20 × 0x20` box test against `player[+0x98]`) to detect adjacency to talk-able actors when the action button is pressed.

## Collision — `FUN_801cfe4c`

`FUN_801cfe4c(player, scene, dir)` returns `0` when the move is clear and `2` when a static wall blocks it (plus bits `1`/`4` contributed by the finer `FUN_801cfc40` actor/edge probe). It samples a **per-scene collision tile map** through the base pointer `_DAT_1f8003ec`:

- The walkability grid lives at `*(_DAT_1f8003ec) + 0x4000`.
- The player world position is converted to tile space by `(coord + bias) >> 6`, then the byte index is `(tileX / 2 & 0x7f) + ((tileZ) * 0x40 & 0x3f80)` — i.e. rows of `0x40` bytes, up to `0x80` rows.
- Each map **byte's high nibble holds 4 sub-cell walkability bits**: the tile is split into a 2×2 quadrant grid, and `byte >> 4 & quadrant_mask` selects the relevant quadrant (`quadrant_mask` ∈ `{1,2,4,8}` from `tileX&1`, `tileZ&1`). A set bit = wall.
- So one map byte covers a `0x80 × 0x80` (128×128) world tile, divided into four `64×64` sub-cells.
- Direction-specific probe offsets come from the tables `DAT_801f21b4` / `DAT_801f2214` (16-byte stride per direction); the function probes three nearby points so the player's footprint (not just its centre) is tested against walls.

The sibling sampler `FUN_801d5718` reads the same `*(_DAT_1f8003ec) + 0x4000` grid with the identical nibble-and-mask shape, confirming the map layout.

## Where the collision grid comes from

`_DAT_1f8003ec` is the base of the **per-scene field buffer** (a scratchpad-resident pointer at `0x1F8003EC`). Its sub-regions:

| Offset from base | Content | Filled by |
|---|---|---|
| `+0x0000` | object / actor records (0x20-byte stride; up to 512) | scene loader / field VM |
| `+0x4000` | **collision + floor grid** — 1 byte/tile, `0x80`-byte rows: **high nibble** = 4 sub-cell wall bits, **low nibble** = floor-elevation tier | high nibble: field-VM `0x4C` opcode, outer-nibble 7; low nibble: scene data |
| `+0x8000` | **per-tile object/attribute map** — `u16`/tile, `0x80`-byte rows: low 9 bits = object-record index into the `+0x0000` table, high bits = per-tile flags (bit `0x400` = object footprint) | object placement at scene load; bit `0x400` ORed in by `FUN_8003aeb0` from field-pack records |
| `+0x12000` | field-pack region; `_DAT_8007b8d0 = base + 0x12800` | `FUN_8001f7c0` (scene asset loader) |

### Collision byte: walls + floor height

Each `+0x4000` byte packs two nibbles for its 128-unit tile:

- **High nibble — walls.** Four sub-cell wall bits (the `2×2` quadrant grid the collision check samples; see above).
- **Low nibble — floor-elevation tier.** A 4-bit index `0..15` into a 16-entry `short` height LUT at scratchpad `0x1f80035c` (`= 0x1f800314 + 0x48`). The object/actor spawn iterator `FUN_8003a55c` reads `LUT[byte & 0xf]` and adds it to each placed object's Y, so a tile's collision byte also encodes its floor height (raised platforms, multi-level rooms). The LUT is filled at scene entry by `FUN_8003aeb0` from the MAN asset header (`_DAT_8007b898 + 2`, 16 negated `short`s).

So the **wall data is authored in the scene event script**: the field VM's `0x4C` (MENU_CTRL) opcode with outer-nibble 7 (`op0` ∈ `0x70..0x7F`, 7-byte op `[4C, 0x7s, b1, b2, b3, b4, mask]`) is a rectangular paint that sets/clears the high-nibble wall bits over a tile range (`col ∈ [b1, b3+1)`, `row ∈ [b2+1, b4+2)`; sub-op `s` = clear-walkable / block-all / clear-mask / set-mask). There is no separate on-disc wall blob — walls are inline operands in the prescript, the same pattern as encounter records and the tile board. See the nibble-7 row of the [0x4C dispatch table in `script-vm.md`](script-vm.md#0x4c-menu_ctrl---outer-nibble-dispatch).

The `+0x8000` map is **not** a terrain-flag grid (an earlier reading). It is a per-tile object/attribute word: its low 9 bits index the `+0x0000` object-record table, which `FUN_8003a55c` walks at scene entry to spawn the NPCs/objects occupying each tile. `FUN_8003aeb0` (the field/town scene-entry map-init — note its `town_mode` / `baria_mode` debug strings) ORs the `0x400` footprint flag into these cells from the field-pack region records (`+0x12000`, offset/count at `+0x12006` / `+0x12008`, 4-byte records).

## Provenance

- Controller `FUN_801d01b0`, position writes `0x801D0684 / 06E4 / 0744 / 07B4` — see `ghidra/scripts/funcs/overlay_0897_801d0684.txt`.
- Collision `FUN_801cfe4c`, finer probe `FUN_801cfc40`, interaction `FUN_801cf9f4` — `ghidra/scripts/funcs/overlay_0897_801cfe4c.txt`.
- Pad remap `func_0x800467e8`, direction mask `FUN_80046494` — `ghidra/scripts/funcs/800467e8.txt` / `80046494.txt`.
- Scene-entry map-init `FUN_8003aeb0` (height LUT fill, `+0x8000` footprint OR, player-actor setup) — `ghidra/scripts/funcs/8003aeb0.txt`. Object spawn iterator `FUN_8003a55c` (low-nibble floor-height read, `+0x8000` index walk) — `ghidra/scripts/funcs/8003a55c.txt`.
- Runtime pin: `scripts/pcsx-redux/autorun_player_pos_watch.lua` (write-watchpoint on `*(0x8007c364) + 0x14/0x18`).

## Town / field parity

The controller is selected by **game mode**: mode `0x03` loads the field overlay (`overlay_0897`), which contains the single free-movement controller `FUN_801d01b0`. `FUN_801d01b0` was runtime-pinned on a walkable field scene (`map03`, mode `0x03`). Rim Elm — scene `town01` — also runs at game mode `0x03` (see `scripts/scenarios.toml`, the `v0_1_pre_battle_tetsu` anchor), so it loads the same overlay and the same controller. The shared scene-entry init `FUN_8003aeb0` corroborates this: it has an explicit `town_mode` debug-string branch and configures the same player actor (`_DAT_8007c364`: speed mult `+0x72 = 0x1000`, `+0x6a = 8`) for both towns and fields. So town locomotion is `FUN_801d01b0`, identical to the field.

## Open

- **Collision-grid zero-init site.** The high-nibble wall bits are painted by opcode `0x70..0x7F`, but where the `+0x4000` grid is first cleared to "all walkable" at scene entry is not pinned. Ruled out: `FUN_8001f7c0` (clears only the `+0x12000` field-pack region, and only when the field file is missing), `FUN_8003a024` (allocates the 100-byte scene control block `_DAT_801c6ea4`), `FUN_800513f0` (allocates the 190 KB *battle* work buffer, not the field buffer). The clear is likely a wholesale memset of the field buffer by the scene-boot allocator that sets `_DAT_1f8003ec` — not yet dumped.
- `FUN_801cfc40` (the finer actor/edge collision probe contributing bits `1`/`4`) is not fully decoded.
