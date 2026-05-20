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
| `+0x4000` | **collision + floor grid** — 1 byte/tile, `0x80`-byte rows: **high nibble** = 4 sub-cell wall bits, **low nibble** = floor-elevation tier | **base**: the `+0x4000..+0x8000` region of the per-scene `.MAP` file (`FUN_8001f7c0`); **deltas**: field-VM `0x4C` opcode, outer-nibble 7 |
| `+0x8000` | **per-tile object/attribute map** — `u16`/tile, `0x80`-byte rows: low 9 bits = object-record index into the `+0x0000` table, high bits = per-tile flags (bit `0x400` = object footprint) | object placement at scene load; bit `0x400` ORed in by `FUN_8003aeb0` from field-pack records |
| `+0x12000` | field-pack region; `_DAT_8007b8d0 = base + 0x12800` | `FUN_8001f7c0` (scene asset loader) |

### Collision byte: walls + floor height

Each `+0x4000` byte packs two nibbles for its 128-unit tile:

- **High nibble — walls.** Four sub-cell wall bits (the `2×2` quadrant grid the collision check samples; see above).
- **Low nibble — floor-elevation tier.** A 4-bit index `0..15` into a 16-entry `short` height LUT at scratchpad `0x1f80035c` (`= 0x1f800314 + 0x48`). The object/actor spawn iterator `FUN_8003a55c` reads `LUT[byte & 0xf]` and adds it to each placed object's Y, so a tile's collision byte also encodes its floor height (raised platforms, multi-level rooms). The LUT is filled at scene entry by `FUN_8003aeb0` from the MAN asset header (`_DAT_8007b898 + 2`, 16 negated `short`s).

The **base wall + floor data is an on-disc blob**: it is the `+0x4000..+0x8000` region of the per-scene field map file (`DATA\FIELD\<scene>.MAP`), streamed into the field buffer at scene load by `FUN_8001f7c0` (see the load chain under [Open](#open)). On top of that base, the field VM's `0x4C` (MENU_CTRL) opcode with outer-nibble 7 (`op0` ∈ `0x70..0x7F`, 7-byte op `[4C, 0x7s, b1, b2, b3, b4, mask]`) applies **story-conditional deltas** — a rectangular paint that sets/clears the high-nibble wall bits over a tile range (`col ∈ [b1, b3+1)`, `row ∈ [b2+1, b4+2)`; sub-op `s` = clear-walkable / block-all / clear-mask / set-mask), gated behind system-flag tests in the prescript. (An earlier reading claimed the nibble-7 paints were the *sole* wall source and there was no on-disc blob; that was a CPU-store-only static search that missed the DMA-streamed base — see [Open](#open).) The nibble-7 op is the same dispatch row in [`script-vm.md`](script-vm.md#0x4c-menu_ctrl---outer-nibble-dispatch).

The `+0x8000` map is **not** a terrain-flag grid (an earlier reading). It is a per-tile object/attribute word: its low 9 bits index the `+0x0000` object-record table, which `FUN_8003a55c` walks at scene entry to spawn the NPCs/objects occupying each tile. `FUN_8003aeb0` (the field/town scene-entry map-init — note its `town_mode` / `baria_mode` debug strings) ORs the `0x400` footprint flag into these cells from the field-pack region records (`+0x12000`, offset/count at `+0x12006` / `+0x12008`, 4-byte records).

## Provenance

- Controller `FUN_801d01b0`, position writes `0x801D0684 / 06E4 / 0744 / 07B4` — see `ghidra/scripts/funcs/overlay_0897_801d0684.txt`.
- Collision `FUN_801cfe4c`, finer probe `FUN_801cfc40`, interaction `FUN_801cf9f4` — `ghidra/scripts/funcs/overlay_0897_801cfe4c.txt`.
- Pad remap `func_0x800467e8`, direction mask `FUN_80046494` — `ghidra/scripts/funcs/800467e8.txt` / `80046494.txt`.
- Scene-entry map-init `FUN_8003aeb0` (height LUT fill, `+0x8000` footprint OR, player-actor setup) — `ghidra/scripts/funcs/8003aeb0.txt`. Object spawn iterator `FUN_8003a55c` (low-nibble floor-height read, `+0x8000` index walk) — `ghidra/scripts/funcs/8003a55c.txt`.
- Runtime pin: `scripts/pcsx-redux/autorun_player_pos_watch.lua` (write-watchpoint on `*(0x8007c364) + 0x14/0x18`).

## Town / field parity

The controller is selected by **game mode**: mode `0x03` loads the field overlay (`overlay_0897`), which contains the single free-movement controller `FUN_801d01b0`. `FUN_801d01b0` was runtime-pinned on a walkable field scene (`map03`, mode `0x03`). Rim Elm — scene `town01` — also runs at game mode `0x03` (see `scripts/scenarios.toml`, the `v0_1_pre_battle_tetsu` anchor), so it loads the same overlay and the same controller. The shared scene-entry init `FUN_8003aeb0` corroborates this: it has an explicit `town_mode` debug-string branch and configures the same player actor (`_DAT_8007c364`: speed mult `+0x72 = 0x1000`, `+0x6a = 8`) for both towns and fields. So town locomotion is `FUN_801d01b0`, identical to the field.

## Engine port

The clean-room engine loads the base grid directly from the field map file. `SceneHost::enter_field_scene` resolves the `.MAP` entry via `Scene::field_map_index` — the unique CDNAME-block entry whose **extended on-disc footprint** is exactly `0x12000` bytes — and copies its `+0x4000..+0x8000` region into `World::field_collision_grid` (`World::load_field_collision_grid`). The grid byte format (high nibble = sub-cell wall bits, low nibble = floor-elevation tier) matches the runtime 1:1, so it copies verbatim; the field-VM `0x4C` nibble-7 hook then layers deltas on top as the prescript runs.

Footprint caveat: the TOC-**indexed** payload of the `.MAP` entry is only the first `0x4000` bytes (the object-record region); the collision grid and everything past it live in the entry's **trailing-gap sectors**, so the engine reads `ProtIndex::entry_bytes_extended`, not the indexed `SceneEntry::bytes`. Verified byte-exact: `town01`'s `entry10[0x4000..0x8000]` equals the live collision grid in a town01 save state (1297 wall tiles, zero diff). Disc-gated coverage: `crates/engine-core/tests/field_locomotion_disc.rs` (base grid non-empty on `town01` + `map03`, and the player stops at a real base wall).

### Scene-entry script

On entry the engine runs the scene's **scene-entry system script** (context channel `0xFB`), not event-script record 0. Record 0 of a per-scene event-script container is a trigger/dispatch table, not linear bytecode, so loading it as the field-VM buffer halts the VM at pc 0 and no entry logic runs. The retail per-frame driver `FUN_8003ab2c` builds the system script from the MAN asset's partition 1, first record; `Scene::field_man_entry_script` mirrors that resolve (`legaia_asset::man_section::ManFile::scene_entry_script` → `(start, pc0)`), and `SceneHost::enter_field_scene` loads the MAN slice from `start` with the VM PC at `pc0` (`World::load_field_script_at`). Slicing from the script start keeps the field VM's 16-bit-wrapping relative jumps anchored at the slice base, matching the retail `buffer_base = script_start`.

This applies to the kingdom-bundle scenes whose static bundle carries the MAN inline (e.g. `map03`). Standalone `SceneEventScripts` scenes such as `town01` carry no MAN in their static bundle (their runtime `_DAT_8007B898` source is fed at load and isn't in the bundle), so they fall back to running event-script record 0 until that source is pinned. The base collision grid (loaded from the `.MAP` above) is independent of which entry script runs; the entry script's `0x4C` nibble-7 wall-paint deltas are gated behind system-flag tests and only fire once the world's story flags are seeded to a matching scene-entry state. Disc-gated coverage asserts the MAN-backed scene's field VM advances past pc 0 (`map03`: 61 distinct PCs, settling into a per-frame loop).

## Open

- **The base collision grid is streamed from disc; nibble-7 ops are conditional deltas on top.** A runtime Write-watchpoint on the live grid region (`_DAT_1f8003ec + 0x4000`) during a Drake-Castle → Drake-world-map transition caught exactly one writer: the CD-DMA channel-3 read primitive **`FUN_8005D9A0`** (the DMA store at `0x8005DA50`), reached via the thin wrapper `FUN_8005C2C4` from the per-sector streaming poller **`FUN_8003EF14`** (return site `0x8003EF68`). `FUN_8003EF14` DMAs one CD sector per ready-IRQ — `FUN_8005C2C4(*(gp + 0x940), 0x200)`: `gp + 0x940` (= `0x8007BC58`) is the destination cursor (it had `_DAT_1f8003ec + 0x4000`, the grid base), `0x200` is the DMA word-count (one 2048-byte sector), then the cursor advances `0x800` and the remaining-sector count at `gp + 0x968` decrements. So the field buffer — collision grid (`+0x4000`), object map (`+0x8000`), field-pack (`+0x12000`) — is the **leading region of a multi-sector streaming CD read** issued at scene load. The grid jumped from 2093 to 6805 wall tiles across the transition while only **6** nibble-7 tile-writes fired — proof the bulk arrives as disc sectors, not from script paints. The field-VM `0x4C` nibble-7 ops (`| 0xf0`, `& ~(mask<<4)`, `| mask<<4`, `& 0xf` at `_DAT_1f8003ec + col + row*0x80 + 0x4000` — the sole *CPU-store* writer) are conditional modifications layered on afterward.
  - *Two earlier readings were wrong, corrected here:* (a) a static draft claimed nibble-7 was the only writer and the base was script-authored — that came from a CPU-store search that can't see the DMA path; (b) a first pass at this capture misread the DMA args as a single `0x10200`-byte block at LBA `0x20943`. In fact the BP fired *after* `FUN_8005D9A0` does `a1 |= 0x10000`, so the real size arg is `0x200` (one sector), and `0x20943` is a hardcoded constant inside the DMA primitive, not a per-scene LBA. `FUN_8005D9A0` takes `(dest, mode)` — see [`boot.md`](boot.md) for the CD-read API.
- **For the engine, base collision is a load step, not a script step**: the collision grid is the `+0x4000…+0x8000` region of the per-scene **main field file** — `DATA\FIELD\<scene>.MAP` by ISO9660 name in retail, or the field record's PROT entry in the debug path. Load that file and slice bytes `0x4000..0x8000`; no script execution is needed for the base walls. The nibble-7 ops still matter for story-conditional changes — they ride the scene's field-VM scripts, which run multi-context at load: the same transition fired `FUN_8003aeb0` (scene-entry init) → `FUN_8003ab2c` (MAN system-script prologue runner) and 17 distinct script contexts (two `0xFB` system channels + per-actor `0x06..0x14`), and the 6 nibble-7 paints came from the `0xFB` system context via `FUN_8003ab2c` — validating that path as the conditional-delta painter.
- **Field-buffer load chain (resolved end-to-end).** The per-scene field-asset loader `FUN_8001f7c0(dest, scene_name, field_record)` fills the field buffer at `dest` (the `_DAT_1f8003ec` base): the leading region (collision `+0x4000`, object map `+0x8000`) is the main `.MAP` file; the field-pack `+0x12000` and `efect.dat` `+0x12800` come from separate files. Two transports converge on shared streaming machinery:
  - *Retail*: builds `DATA\FIELD\<scene>.MAP`, opens it by ISO9660 name (`FUN_800608f0`), streams into `dest` via `FUN_8003e6bc`. The disc LBA is the ISO9660 directory entry.
  - *Debug* (`_DAT_8007b8c2 != 0`): `FUN_8003e8a8(field_record, 1)` sets the `CdlLOC` at `0x8007bc5c` from the in-RAM PROT TOC (`0x801C70F0`): `target_sector = CdPosToInt(base_loc@0x8007bc50) + toc[field_record + 2]` (the documented `start_lba = toc[p+2]`); then `FUN_8003e800(dest, 0x28, 1)` issues a 40-sector (`0x14000`-byte) streaming read.
  - Shared core: `FUN_8003e800(dest, count, flags)` (generic read entry; sets `gp+0x894`/`gp+0x97c`) → `FUN_8003f128` (arm: copies dest/count into `gp+0x940`/`gp+0x968`, seeds `gp+0x8c8` from the current CD position, issues `CdControl(CdlSetloc=2, &cdloc@0x8007bc5c)`, registers the data-ready callback) → `FUN_8003EF14` per-sector poller → `FUN_8005C2C4` → `FUN_8005D9A0` CD-DMA primitive (one sector per ready-IRQ, dest cursor `+= 0x800`). This is the same `FUN_8005D9A0` writer caught by the runtime watchpoint above — static loader trace and watchpoint agree.
  - The same generic entry serves other clients (`FUN_8003e104` = the `h:\mpack\monster_snd` pack loader), confirming `FUN_8003e800`/`FUN_8003f128` are shared streaming infrastructure, not field-specific. (This corrects an earlier note that ruled `FUN_8001f7c0` out as "loads `+0x12000`/`+0x12800` only" — it loads the base `.MAP` too, which *contains* the `+0x4000` grid.)
- `FUN_801cfc40` (the finer actor/edge collision probe contributing bits `1`/`4`) is not fully decoded.
