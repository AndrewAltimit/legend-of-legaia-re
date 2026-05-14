# RAM map + key globals

PSX RAM is 2 MB total at KSEG0 base `0x80000000`. Legaia's runtime layout:

```
0x80000000 - 0x8000FFFF    BIOS scratchpad area (kernel + thread state)
0x80010000 - 0x800FFFFF    SCUS_942.54 code + data (~960 KB)
0x80100000 - 0x801BFFFF    runtime data buffers (asset slabs, character struct, save state)
0x801C0000 - 0x801FFFFF    overlay window (256 KB, see "Overlays" below)
0x80200000+                 extended overlay region
```

Plus the PSX-specific scratchpad at `0x1F800000-0x1F8003FF` (1 KB) which Legaia uses for global story flags and a few per-frame transients.

## Static (`SCUS_942.54`-resident) globals

| Address | Type | Purpose |
|---|---|---|
| `0x800840F8` | u32 | BIOS pad data (read by `FUN_8001822C`). |
| `0x80084340` | inventory base | Per-page inventory state, 0x414-byte stride. |
| `0x80084540` | u16 | Current map / scene PROT base index. |
| `0x80084594` | u8 | Party member count. |
| `0x80084598` | u8[] | Party member IDs (sorted insertion, cap 4). |
| `0x80084628` | i16 | Set by op 0x4C nibble-8 sub-8. |
| `0x80086D70` | u8[32] | **Fourth flag bank** - 256-bit bitfield, accessed via SET / CLEAR / TEST `(idx >> 3, 0x80 >> (idx & 7))`. |
| `0x80087AF8` | u32 | Result of `FUN_80020224` descriptor walker, set by town-overlay MAIN INIT. |
| `0x800845DC` | (mirror of `_DAT_80084570`) | Snapshot written by op 0x4C nibble-E sub-E. |
| `0x800845A4` | u32 | Casino coin bank. "Infinite Coins" cheat writes `0x05F5_E0FF`. |

## Cheat-database-pinned globals

These are RAM cells the GameShark cheat database has named anchors
for. See [`docs/reference/cheats.md`](cheats.md) for the full
citation table.

| Address | Type | Purpose | Cheat citation |
|---|---|---|---|
| `0x80084540` | u16 | Active scene-name pool slot (also "Map Modifier"). | `View Credits` writes `0x030C` (credits scene). |
| `0x80084570` | u32 | Game-time seconds counter. | `Game Time 0:00:00` zeroes it. |
| `0x80084594` | u8 | Party member count. | `Character Activator` writes `0x03`. |
| `0x80084599` | u8 | Noa "join the party" gate. | `Noa Activator` writes `0x01`. |
| `0x8008459A` | u8 | Gala join-party gate. | `Gala Activator` writes `0x02`. |
| `0x8008459C` | u32 | Party gold. | `Infinite Gold (Never Glitchy)`. |
| `0x800845A4` | u32 | Casino coin bank. | `Infinite Coins`. |
| `0x80085600..0x80085800` | u8[512] | Story-flag bitmap window (Door of Wind, town visited markers). | `Access All Towns` writes `0xF77F` / `0xF8FF`. |
| `0x80085958..` | u8[144] | 72-slot inventory array, 2-byte stride `(id, count)`. | `Have 99 Items` and `Item Modifier`. |
| `0x800EC9E8` | u8[0x2D4] × N | Battle actor pool, party-slot stride `0x2D4`. | `Infinite HP/MP (Vahn/Noa/Gala)` cheats target slots 0..2. |
| `0x8007A6BC` | u16 | Shared "currently-acting character" HP/MP scratch. | Every "Infinite HP/MP" cheat hits this first. |
| `0x8007A894` | u16 | Frame-pacing logic timer. | `Slow Motion` writes `0x68FB`. |
| `0x8007B450` | u16 | Menu-request register the menu overlay polls each frame. | `Save Anywhere`, `Status Modifier Menu`, `Shop Modifier`, `End of Game Stat Page`. |
| `0x8007B5FC` | u16 | Encounter step counter. | `No Random Battles` writes `0x0377`. |
| `0x8007B6A8` | u16 | Save-anywhere allow flag. | `Save Anywhere (Press Select+X)`. |
| `0x8007B6F4` | u16 | Camera mode word. | `Control Camera` and `Small Maps` cheats. |
| `0x8007B790` | u16 | Camera zoom-state register. | `Control Camera` reads here. |
| `0x80084708 + n*0x414` | u8[0x414] | Per-character record (4 slots). | Hundreds of cheats; see [`docs/formats/save-record.md`](../formats/save-record.md). |

### Mini-game scratch cells

Cheat-pinned mini-game RAM. Outside the engine's current scope but
worth recording so we can recognise them in saves.

| Address | Mini-game | Purpose |
|---|---|---|
| `0x8008444C` | Fishing | Persistent fishing-points counter. |
| `0x801D9168` | Fishing | Tension gauge. |
| `0x801D91CC` | Fishing | Active fish ID. |
| `0x801D9274` | Fishing | Casting power. |
| `0x801D9298` | Fishing | Fish life. |
| `0x801DBFC4` | Baka Fighter | Player life. |
| `0x801DBFF0` | Baka Fighter | Rounds-won counter. |
| `0x801DC06C` | Baka Fighter | Computer life. |
| `0x801D3CAC` | Wild Card slot machine | Punch-mode unlock. |
| `0x801D53CC` | Dance | Dance-points counter. |
| `0x801D078C` `0x801D071C` `0x801D065C` `0x801D06BC` | Field overlay | Walk-through-walls collision-state cells. |

### Code-patch sites in `SCUS_942.54`

`0x2400` is the MIPS `nop` opcode; cheats that write `0x2400` are
patching an instruction. Useful Ghidra anchors.

| Address | Effect | Cheat |
|---|---|---|
| `0x800422F4` | Inventory-add `count = 99` patch | `Bought Any Item / Find Items You Will Get 99 Quantity` |
| `0x8004309E` | Inventory count-decrement nop | `Infinite Items All Slots` |
| `0x80043910` (range `0x80043900..0x80043920`) | Vahn chest draw-call nop | `Remove Vahn's Chest` |
| `0x8007EA96` | HP-write branch nop | `Maxed HP for All Characters` |

## Sound + audio path

| Address | Purpose |
|---|---|
| `0x8007B380` | 12-byte per-extension flag/mode metadata table. |
| `0x8007B38C` | Path prefix `"sound\"` for streaming-asset loads. |
| `0x8007B394` | `".spk"` extension. |
| `0x8007B39C` | `".LZS"`. |
| `0x8007B3A4` | Two 4-byte mode descriptors used by `FUN_8001EBEC`. |
| `0x8007B3AC` | `"bse.dat"` master file name. |
| `0x8007B3B4` | `".dpk"`. |
| `0x8007B3BC` | `".MAP"`. |
| `0x8007B3C4` | `".PCH"`. |
| `0x8007B3D4` | `".pac"`. |
| `0x8007B3DC` | `"STR"`. |
| `0x8007B7F8` | sin lookup table. |
| `0x8007B81C` | cos lookup table. |
| `0x8007B824` | u32 | Mode index read by `FUN_8001EBEC`. |
| `0x8007B840` | MOVE2 buffer base. |
| `0x8007B888` | MOVE buffer base. |
| `0x8007B8D0` | u32 | `bse.dat` master bank pointer (0x1800-byte buffer). |
| `0x8007BAC8` | u16 | BGM ID written by field-VM op 0x35 sub-1. |
| `0x8007BC64` | u16 | Global BGM pool base for IDs ≥ 2000. |
| `0x8007BD30` | 5008 bytes | Effect-runtime pool: 16-byte head + 128 child slots + 32 master slots. |
| `0x8007BD5C` | u32 | Effect 2-pack wrapper buffer pointer (post-init). |

## Runtime PROT TOC + asset chain

| Address | Purpose |
|---|---|
| `0x801C70F0` | In-RAM PROT TOC - populated at boot by `FUN_8003E4E8`. Different stride from on-disc. |
| `0x801C6EA4` | Current world / scene struct pointer. |
| `0x801C6460` | 64-entry × u16 scratchpad slot table. Written by op 0x4C nibble-C sub-A; adjusted by sub-B / sub-C. |
| `0x801C66A0` | 64-slot ramp scheduler pool (stride 0x20). |
| `0x8007C018` | TMD pointer table (`idx * 4` stride). Written by `FUN_80026B4C`. |
| `0x8007C348` | u32 | Free-list LIFO stack pointer for the actor allocator. |
| `0x8007C34C..0x36C` | u32[7] | Actor-list slot table consumed by `FUN_8002519c`. Seven linked-list heads at strides of 4 bytes (`+0x00`/`+0x04`/`+0x08`/`+0x0C`/`+0x10`/`+0x14`/`+0x20`). `FUN_80016444` walks five of them per frame as separate render passes; per-node entry-point is `node[+0x0C]` invoked via `jalr`. `_DAT_8007C354` and `_DAT_8007C364` are also read by `func_0x8003C83C` for the `0xF8`/`0xFB` motion-VM channel lookups (same list, two consumers). |
| `0x8007C364` | Player context pointer. |
| `0x8007326C` | TMD per-mode descriptor table (8-byte stride × 6 entries). |
| `0x8007A940` | SsAPI per-note pitch / per-voice volume exponential lookup table (read by `FUN_80066E50` / `FUN_80067550`). |
| `0x801CD2B8` | SsAPI 16-bit slot-allocation bitmap. Bit `i` = sequencer slot `i` allocated. |
| `0x801CD2C0` | SsAPI 16-entry per-slot pointer table. Each entry → `0xB0`-byte sequence-state struct. |
| `0x801C4BEC` | libcd directory-entry cache (up to 128 entries, populated by `FUN_8005DEA0`). |
| `0x80074358` | Global 4×u32 ability bitmask. Written by `FUN_80042558` (OR-aggregate); read by `FUN_800431D0` (bit-test). |
| `0x80086D70` | 256-bit "fourth flag bank" bitfield. Wired to field-VM ops `0x50` / `0x60` / `0x70` via `FUN_8003CE08` / `_CE34` / `_CE64`. |

## World-map render pipeline

Globals read or written by the per-frame world-map POLY_FT4 batch
chain. End-to-end walkthrough in [`subsystems/world-map.md`](../subsystems/world-map.md#render-pipeline).

| Address | Type | Purpose |
|---|---|---|
| `0x8007BC3C` | u32 | World-map submode register. `FUN_80016444` gates its `jal 0x801D7EA0` on this being `2`. Six SCUS writers (`FUN_80016230` / `FUN_80025980` / `FUN_80025DA0` / `FUN_8001D424`). |
| `0x8007BCD0..D8` | u32[3] | Source globals for the gate-arm scale / step / OT-layer params. `FUN_801D1344` reads these and forwards as args to `FUN_801D8258`. |
| `0x801F351C` | u32 | One-shot gate flag for the world-map POLY_FT4 batch emitter. `FUN_801D8258` sets it to `1`; `FUN_801D7EA0` (and 0897 sibling `FUN_801C9688`) clear it after one emission. Lives in the persistent `0x801F0000+` region so survives overlay swaps. |
| `0x801F3518` | u32 | Running camera angle for the cos-rotation POLY_FT4 batch. Advanced by `DAT_1F800393 * _DAT_801F3524` per `FUN_801D7EA0` call; masked to the 4096-entry cos LUT at `0x8007B81C`. |
| `0x801F3520` | u32 | Render scale / range. Sourced from `_DAT_8007BCD4` via `FUN_801D8258`'s `param_2`. Used both as `local_3c` and `local_3c / 5`. |
| `0x801F3524` | u32 | Angle step per frame tick. Sourced from `_DAT_8007BCD8` via `FUN_801D8258`'s `param_3`. |
| `0x801F3528` | u32 | OT layer / draw priority. Sourced from `_DAT_8007BCDC` via `FUN_801D8258`'s `param_4`. |

## World-map TMD and actor tables

The global asset-pointer table consumed by the world-map top-view
renderer (`FUN_801F69D8` → `FUN_80043390`). Live verification: see the
"Live snapshot (Drake world-map)" example in
[`formats/world-map-overlay.md`](../formats/world-map-overlay.md#how-slot-4-bytes-reach-cluster-a).

| Address | Type | Purpose |
|---|---|---|
| `0x8007C018` | `void *[N]` | **Global asset-pointer table.** Installer `FUN_80026B4C @ 0x80026BA8` stores any pointer here (TMD-magic mismatch only sets `DAT_8007B828` bits, doesn't reject). In a Drake world-map snapshot 194 of the first 256 entries are populated: 45 TMD pointers (`[0..4]` = 5 party TMDs from `player.lz`, `[5..44]` = 40 kingdom slot-1 TMDs), 20 slot-4 body-aligned pointers at `[94..113]`, plus vertex-pool / texture / zero-padded slots. Consumed by `FUN_801F69D8` (world-map top-view), `FUN_80021B04` (SCUS actor allocator), `FUN_801D77F4` (overlay actor allocator), `FUN_801D8280` (table walker). |
| `0x8007B774` | u32 | Write counter for `DAT_8007C018`. Bumped by `FUN_80026B4C` on each install. Drake snapshot = `0x2D` (45 entries installed). |
| `0x8007BB38` | u32 | Walk count (last valid index). Drake snapshot = `0x2C`. |
| `0x8007B824` | u32 | Per-pack start index into `DAT_8007C018`. Set when a new pack begins, read by `FUN_8001E928` / `FUN_8001E890` post-install to update `DAT_8007B6F8`. Drake snapshot = `0`. |
| `0x8007B828` | u32 | TMD-magic-mismatch error bits. Set by `FUN_80026B4C` when a non-TMD pointer (e.g. slot-4 body) is installed. Drake snapshot = `0` (no errors observed in steady-state). |
| `0x8007B6F8` | u32 | **Kingdom-TMD prefix offset.** Count of party-character TMDs that precede the kingdom-bundle TMDs in `DAT_8007C018`. The world-map dispatcher does `DAT_8007C018[(actor_kind8 + DAT_8007B6F8) * 4]`, so this shifts world-map actor-kind indices past the party prefix. Writers: `FUN_80020118` (field-load entry; resets to 0) and `FUN_8001E890` / `FUN_8001E928` (set to `DAT_8007B824 + *player_pack_count`). Drake snapshot = `5`. |
| `0x8007B7DC` | `void *` | VDF buffer pointer. Set by asset-dispatcher case 7. `FUN_8001FBCC` walks each sub-entry and writes a parallel pointer table at `0x80083E58` (consumed by `FUN_801D77F4` for actor instance bring-up). Drake snapshot points at `0x8011A2F4`. |
| `0x8007B888` | `void *` | Slot-4 (MOVE) buffer pointer. Drake snapshot = `0x8011A624`. |
| `0x80083E58` | `void *[N]` | Parallel VDF sub-entry pointer table. First entry points into VDF buffer (`0x8011A2FC` in Drake snapshot); subsequent entries point into the `0x800D9xxx` actor-instance area. |

## Debug flags

| Address | Purpose |
|---|---|
| `0x8007B8C2` | Dev/retail loader-path flag. Read by 26 SCUS functions; no static writers. |
| `0x8007B98F` | In-game debug menu enable. Accessed as the high byte of the word at `0x8007B98C`. |
| `0x8007B7C0` | Debug-dispatch trigger. |
| `0x8007B450` | Debug-dispatch parameter slot. Also used by the field-VM `STATE_RESUME` opcode (`0x49`) as its tristate state register. |
| `0x8007B6F4` | "Small maps" debug mode flag. |
| `0x8007B850` | Per-frame button mask (built by `FUN_8001822C`). |
| `0x8007B7C0` | Previous-frame button mask. |
| `0x8007B874` | "Newly pressed this frame" (edge detection). |

JP retail uses build-shifted addresses (`0x07D51F` for the in-game debug menu enable; +0x1B90 from the NA address).

## PSX scratchpad (`0x1F800000-0x1F8003FF`)

The PSX has 1 KB of fast scratchpad RAM mapped here. Legaia uses the high end:

| Address | Type | Purpose |
|---|---|---|
| `0x1F800314` | i16[] | Inverted-Y mirror table (op 0x4C nibble-9 sub-E writes `-words[i]` here). |
| `0x1F800393` | u8 | Per-frame tick byte (read by op 0x4A `WAIT_FRAMES` and the 0xFFFF sentinel in op 0x4C nibble-C sub-B/C). |
| `0x1F800394` | u32 | **Global story-flag word.** Read by `GFLAG_TST` (0x30); written by `GFLAG_SET` / `GFLAG_CLR` (0x2E / 0x2F); also gates op 0x4C nibble-4 sub-9's tristate dispatch via bits `0x01000000` / `0x02000000`. Set by the dialog opener with bit 0x40 (`"dialog active"` lock). |
| `0x1F8003E8` | u32 | Render-config block (op 0x46). |
| `0x1F8003EC` | u8[] | Tile-flag bitmap base used by op 0x4C nibble-7 (rectangle SET/CLEAR over `+0x4000` offset). |
| `0x1F8003F8` / `0x1F8003FA` | i16 | Camera-scroll values used by op 0x23 player path. |

## Overlay window (`0x801C0000+`)

The 256 KB overlay window is shared between several runtime overlays - only one is loaded at any time. See [`tooling/overlay-capture.md`](../tooling/overlay-capture.md) for the per-overlay capture protocol and [`subsystems/boot.md`](../subsystems/boot.md) for which overlay loads when.

| RAM range | Overlay | Subsystems |
|---|---|---|
| `0x801C0000+` | Title screen | Actor / sprite VM (`FUN_801D6628`) |
| `0x801CE818+` | Town / field / dialog (loaded from PROT entry `0897_xxx_dat`) | Field VM (`FUN_801DE840`), MES renderer, inventory hub, MAIN INIT |
| `0x801CE818+` | Battle (loaded from PROT entry `0898_xxx_dat`) | Per-actor state machine, battle main dispatcher, effect VM cluster |
| `0x801C5818+` | Options / config menu (PROT 0896 = 0897 + 36 KB prefix) | In-game options UI |
| `0x801F0000+` | Battle effect helpers extend into here | `0x801F5D90`, `0x801F5CF8` (effect_id specials), `0x801F8004 / 88FC / 8D4C / 8E6C / 8F28` (particle / emitter cluster) |

## Mini-game state regions

Each mini-game gets its own ~64 KB slab of upper RAM, loaded fresh when entered. See [`reference/builds.md`](builds.md) for the per-mini-game RAM addresses.
