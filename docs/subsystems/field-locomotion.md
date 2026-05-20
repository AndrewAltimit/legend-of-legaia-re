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

## Provenance

- Controller `FUN_801d01b0`, position writes `0x801D0684 / 06E4 / 0744 / 07B4` — see `ghidra/scripts/funcs/overlay_0897_801d0684.txt`.
- Collision `FUN_801cfe4c`, finer probe `FUN_801cfc40`, interaction `FUN_801cf9f4` — `ghidra/scripts/funcs/overlay_0897_801cfe4c.txt`.
- Pad remap `func_0x800467e8`, direction mask `FUN_80046494` — `ghidra/scripts/funcs/800467e8.txt` / `80046494.txt`.
- Runtime pin: `scripts/pcsx-redux/autorun_player_pos_watch.lua` (write-watchpoint on `*(0x8007c364) + 0x14/0x18`).

## Open

- **Collision-map source.** Where the scene loader populates `*(_DAT_1f8003ec) + 0x4000` (which field-pack slot or MAN section carries the on-disc walkability grid) is not yet pinned. A write-watchpoint on the map base during scene load would close it.
- **Town vs. field parity.** `FUN_801d01b0` was pinned on a walkable field/overworld scene (`map03`, game mode `0x03`); towns (`town01`) run the same overlay and game mode and almost certainly use the same controller, but a confirming watchpoint run on a town save has not been done.
- `FUN_801cfc40` (the finer actor/edge collision probe contributing bits `1`/`4`) is not fully decoded.
