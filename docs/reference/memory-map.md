# RAM map + key globals

What lives where in Legaia's RAM. A lookup table: **grep it for your address**, or scan the region map below to work out which section an address falls in.

Two things to know before you use it:

- **An address in the overlay window `0x801C0000+` is ambiguous on its own.** Several overlays share that window and only one is resident at a time, so the same address means different things in field, battle, and menu mode. Rows in that range say which overlay they belong to. See [Overlay window](#overlay-window-0x801c0000).
- **`0x1F800000` is not main RAM.** It is the 1 KB PSX scratchpad, and Legaia keeps global story flags there - so the flag bank a script writes is not in the 2 MB map at all. See [PSX scratchpad](#psx-scratchpad-0x1f800000-0x1f8003ff).

Rows carry their provenance: a `FUN_` address, a `ghidra/scripts/funcs/` dump path, or the cheat code that pinned them. Where a global's semantics are only partly understood the row says so - "exact semantics **open**" is a real value here, not an omission.

## Region map

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
| `0x80085758` | u8[] | **Fourth flag bank** - bitfield accessed via SET / CLEAR / TEST `(idx >> 3, 0x80 >> (idx & 7))` (`FUN_8003CE08`/`_CE34`/`_CE64`). The field-VM opcode encoding spans `idx = 0x000..=0xFFF` plus `0x8000..=0x8FFF` (extended-prefix opcodes `0xD0..=0xFF`), so it is **not** a fixed 256-bit array. The earlier `0x80086D70` was a double-count of the `0x1618` save displacement onto `0x80085758` (which itself already `= 0x80084140 + 0x1618`); see [`subsystems/script-vm.md`](../subsystems/script-vm.md). |
| `0x80077828` | u8[] | **Per-monster steal table** (`DAT_80077828`). Indexed by 1-based monster id at `+id*2`; each entry is `[steal_chance_pct: u8, steal_item_id: u8]` (chance first). What the Evil God Icon steals - NOT in the PROT 867 record. See [`docs/formats/steal-table.md`](../formats/steal-table.md); parser `legaia_asset::steal_table`. |
| `0x80087AF8` | u32 | Result of `FUN_80020224` descriptor walker, set by town-overlay MAIN INIT. |
| `0x800845DC` | (mirror of `_DAT_80084570`) | Snapshot written by op 0x4C nibble-E sub-E. |
| `0x800845A4` | u32 | Casino coin bank. "Infinite Coins" cheat writes `0x05F5_E0FF`. |
| `0x800845B4` | u32 | **Point Card counter** (unmapped by every public cheat archive). The shop buy commit `FUN_801db7f4` (menu overlay) accrues `price/20 * qty` into it when item `0xFE` (the Point Card) is held (`func_0x80042f4c(0xFE)` inventory-has gate), capped at `9,999,999`. Menu display readers at `0x801d1008`/`0x801dce84`. GameShark-style max: 16-bit pair `800845B4 967F` + `800845B6 0098`. `see ghidra/scripts/funcs/overlay_shop_save_801db7f4.txt`. |
| `0x8007BB80` | u32 | Menu window-slide latch: non-zero while a window is sliding; every menu sub-screen SM gates its interactive phase on `== 0`. |
| `0x8007BB84` | u32 | Menu pad-**edge** word (remapped d-pad bits `0x1000` Up / `0x4000` Down / `0x8000` Left / `0x2000` Right) read by the kind-4 list kernel `FUN_80032A44` and the quantity pickers. |
| `0x8007BB88` | u32 | List-kernel selected-row **payload** (low 12 bits of the row entry - the bag slot on the item lists). Doubles as the name-entry grid cursor. |
| `0x8007BB90` | i32 | List-kernel persisted **scroll top** (`gp+0x878`); clamped in place by the allocator `FUN_80030104`. |
| `0x8007BB94` | i32 | List-kernel **mode/result**: 0 idle, 1 browsing, 2 row confirmed, 3 cancelled, 4 parked behind the command window. |
| `0x8007BB98` | i32 | List-kernel persisted **selected row** (`gp+0x880`); clamped to `count-1` on rebuild. |
| `0x8007BB9C` | u32 | Selected row's **class nibble** (`entry & 0xF000`) - the screen-id key `FUN_80034250` dispatches descriptions on. |
| `0x8007BBA0` | i32 | List-kernel **row count** (mirrored from the allocator; bounds the sell-list scroll fix-up in `FUN_801DBD94`). |

## Game-mode state machine

Companions of the 28-entry × 24-byte mode-dispatch table at `0x8007078C` (the table itself is documented in [`subsystems/boot.md`](../subsystems/boot.md) and [`functions.md` § Game-mode state machine](functions.md)).

| Address | Type | Purpose |
|---|---|---|
| `0x8007B83C` | u16 | **Next game-mode index** (master mode selector; stored via `sh`). Drives the per-frame mode dispatcher: `0x02` field-launch, `0x03` field-run, `0x15` battle, `0x1A` STR FMV. Title-attract underflow writes `0x1A` (see `0x801EF018`). Also indexes the mode table - `(&DAT_800707A0)[_DAT_8007B83C * 0x18]` (entry·24 + 0x14 = the mode's `param`) - which `FUN_8001DCF8` uses to seed the **lower 16 bits** of the field-VM flag word `0x1F800394` on each mode switch (see the `0x1F800394` row and [`save-screen.md`](../subsystems/save-screen.md)). |
| `0x8007B87C` | index | Mode index rendered on the dev **CONFIG / test screen**. `FUN_800188C8:340`: `(&PTR_s_CONFIG_8007078c)[_DAT_8007B87C * 6]` - indexes the `0x8007078C` table at stride **6 words = 24 bytes** (independently re-confirms the 24-byte entry stride) to fetch the entry's CONFIG label string for `FUN_8001AA68` to draw. Provenance: `ghidra/scripts/funcs/800188c8.txt`. |
| `0x8007B7AC` | flag | Mode-dispatch-cluster flag read by `FUN_8001DCF8:370` (`if (_DAT_8007B7AC == 1)`), a function that also branches on `_DAT_8007B83C` against mode constants `0x0E/0x02/0x18/0x14`. Used as a boolean gate here; exact semantics **open**. Provenance: `ghidra/scripts/funcs/8001dcf8.txt`. |

## Cheat-database-pinned globals

These are RAM cells the GameShark cheat database has named anchors
for. See [`docs/reference/cheats.md`](cheats.md) for the full
citation table.

| Address | Type | Purpose | Cheat citation |
|---|---|---|---|
| `0x80084540` | u16 | Active scene-name pool slot (also "Map Modifier"). | `View Credits` writes `0x030C` (credits scene). |
| `0x80084570` | u32 | Game-time play counter - advances ~per-frame (≈60/s), NOT per-second (the save screen divides it down for the `HH:MM:SS` display); a maxed save reads ~10.4M ≈ 48 h at 60/s. | `Game Time 0:00:00` zeroes it. |
| `0x80084594` | u8 | Party member count. | `Character Activator` writes `0x03`. |
| `0x80084599` | u8 | Noa "join the party" gate. | `Noa Activator` writes `0x01`. |
| `0x8008459A` | u8 | Gala join-party gate. | `Gala Activator` writes `0x02`. |
| `0x8008459C` | u32 | Party gold. | `Infinite Gold (Never Glitchy)`. |
| `0x800845A4` | u32 | Casino coin bank. | `Infinite Coins`. |
| `0x80085600..0x80085800` | u8[512] | Story-flag bitmap window (Door of Wind, town visited markers). | `Access All Towns` writes `0xF77F` / `0xF8FF`. |
| `0x80085958` | u8[] | **Item inventory** array (= SC `+0x1818`), 2-byte stride `(id, count)`. - [details ↓](#0x80085958---item-inventory) | `Have 99 Items` and `Item Modifier`. |
| `0x800EC9E8` | u8[0x2D4] × N | Battle actor pool, party-slot stride `0x2D4`. | `Infinite HP/MP (Vahn/Noa/Gala)` cheats target slots 0..2. |
| `0x8007A6BC` | u16 | Shared "currently-acting character" HP/MP scratch. | Every "Infinite HP/MP" cheat hits this first. |
| `0x8007A894` | u16 | Frame-pacing logic timer. | `Slow Motion` writes `0x68FB`. |
| `0x8007B450` | u16 | Menu-request register the menu overlay polls each frame. | `Save Anywhere`, `Status Modifier Menu`, `Shop Modifier`, `End of Game Stat Page`. |
| `0x8007B5FC` | u16 | Encounter step counter. | `No Random Battles` writes `0x0377`. |
| `0x8007B6A8` | u16 | Save-anywhere allow flag. | `Save Anywhere (Press Select+X)`. |
| `0x8007B6F4` | u16 | Camera mode word. | `Control Camera` and `Small Maps` cheats. |
| `0x8007B790` | u16 | Camera zoom-state register. | `Control Camera` reads here. |
| `0x80084708 + n*0x414` | u8[0x414] | Per-character record (4 slots; display name at internal `+0x2A7`). Slot 3 (Terra) runs into the story-flag bitmap at `0x80085600`, so its tail (`+0x2BC`..) aliases the globals - see [`docs/formats/save-record.md`](../formats/save-record.md). | Hundreds of cheats. |

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
| `0x8007B3A4` | Equipment-swap selector tables for `FUN_8001EBEC` (3 equip-condition byte-offsets at `+0x00` + 3 patched-group indices at `+0x04`); adjacent to the sound path-string cluster in BSS but not sound data. See [character-mesh.md](../formats/character-mesh.md#10-group-cap--equipment-conditional-swap). |
| `0x8007B3AC` | `"bse.dat"` master file name. |
| `0x8007B3B4` | `".dpk"`. |
| `0x8007B3BC` | `".MAP"`. |
| `0x8007B3C4` | `".PCH"`. |
| `0x8007B3D4` | `".pac"`. |
| `0x8007B3DC` | `"STR"`. |
| `0x8007B7F8` | sin lookup table. |
| `0x8007B81C` | cos lookup table. |
| `0x8007B824` | u32 | Party base index into `DAT_8007C018` (see the fuller entry below); read by `FUN_8001EBEC` to address the three active-party battle-TMD pointers `DAT_8007C018[0x8007B824 + 0..2]`. (Earlier "sound mode index" reading was wrong.) |
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
| `0x801C6ED8` | CD-XA streaming-clip table: 34 slots of `[CdlLOC][u32 byte_len]` (8-byte stride, indexed by clip id; `+0x0` = 4-byte BCD-MSF `CdlLOC` disc start, `+0x4` = length, zero = empty slot). **Slot `i` = file `XA<i+1>.XA`** - lengths byte-exact vs the disc. Filled at boot by `FUN_801CFA78` (PROT 0895 `init.pak`), which sprintf-generates `\XA\XA%d.XA;1` per slot and stages `[BCD-MSF][size]` via ISO9660 lookup `FUN_8005DBB4` - no XA LBA is stored absolutely, so the table survives disc relayout. Read by the XA cue starter `FUN_8003D53C` (via `msf_to_lba`, `FUN_8005C42C`), which drives the CdSync-callback state machine `FUN_8003D764` (`CdlSetfilter {file 1, chan}` per cue). See [`cutscene.md`](../subsystems/cutscene.md#xa-channel-selection). |
| `0x801C6460` | 64-entry × u16 scratchpad slot table. Written by op 0x4C nibble-C sub-A; adjusted by sub-B / sub-C. |
| `0x801C66A0` | 64-slot ramp scheduler pool (stride 0x20). |
| `0x8007C018` | TMD pointer table (`idx * 4` stride). Sole writer is `FUN_80026B4C`. All populated entries (`[0..DAT_8007BB38]`) are post-fixup Legaia TMDs. |
| `0x8007C348` | u32 | Free-list LIFO stack pointer for the actor allocator. |
| `0x8007C34C..0x36C` | u32[7] | Actor-list slot table consumed by `FUN_8002519c`. Seven linked-list heads at strides of 4 bytes (`+0x00`/`+0x04`/`+0x08`/`+0x0C`/`+0x10`/`+0x14`/`+0x20`). `FUN_80016444` walks five of them per frame as separate render passes; per-node entry-point is `node[+0x0C]` invoked via `jalr`. `_DAT_8007C354` and `_DAT_8007C364` are also read by `func_0x8003C83C` for the `0xF8`/`0xFB` motion-VM channel lookups (same list, two consumers). |
| `0x8007C364` | u32 | Player context pointer (`_DAT_8007C364`). Corpus-stable at `0x80083794` across the field/battle Tetsu captures. `+0x10` carries the `0x80000` "encounter active" flag the entity SM raises during install and clears at the battle handoff. |
| `0x8007BD24` | u32 | Battle context pointer (`_DAT_8007BD24`). `0x800EB654` while a battle is resident, `0` in the field. Base of the battle-actor / AI ctx block (`+0x13` = active slot, `+0x276` = tutorial byte gating arts-voice XA cues, `+0x28A` = battle-mode counter; the pending-SFX write counter lives at `+0x9`). |
| `0x8007B6D8` | u16[4] | Pending-SFX cue ring (counter = battle ctx `+0x9`, wraps past 3). Written by the cue router `FUN_8004FE5C` (and directly by minigame overlays), drained by `FUN_80016B6C` against the SFX descriptor table. Engine mirror `legaia_engine_core::sfx_cue`. |
| `0x8007B724` | u32 | Last-played SFX id - the cue router's dedupe compare. |
| `0x800788B8` | u16[N] | Per-arts-voice XA clip duration table (index = cue id − `0x100`); `FUN_8004FE5C` converts to sectors as `(raw*60 + 99)/100`. |
| `0x8007326C` | u32 | TMD per-mode descriptor table (8-byte stride × 6 entries). |
| `0x8007A940` | SsAPI per-note pitch / per-voice volume exponential lookup table (read by `FUN_80066E50` / `FUN_80067550`). |
| `0x801CD2B8` | SsAPI 16-bit slot-allocation bitmap. Bit `i` = sequencer slot `i` allocated. |
| `0x801CD2C0` | SsAPI 16-entry per-slot pointer table. Each entry → `0xB0`-byte sequence-state struct. |
| `0x801C4BEC` | libcd directory-entry cache (up to 128 entries, populated by `FUN_8005DEA0`). |
| `0x80074358` | Global 4×u32 ability bitmask. Written by `FUN_80042558` (OR-aggregate); read by `FUN_800431D0` (bit-test). |
| `0x80085758` | "Fourth flag bank" bitfield. Wired to field-VM ops `0x50` / `0x60` / `0x70` (and the move VM) via `FUN_8003CE08` / `_CE34` / `_CE64`. (Formerly mis-listed at `0x80086D70` - a double-count of the `0x1618` save offset.) |

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
| `0x80078DFC..0x80078E0F` | u32[5] | Statically-linked libgpu `MoveImage` packet template: `[tag 0x04FFFFFF][GP0 0x80000000][src yx][dst yx][wh]`. `FUN_80058490` (MoveImage) patches src/dst/wh in place per call, then submits through the driver vtable at `*(0x80078D4C)+8`. The frame-clear fill template (`x=0, y=4`, 320×224) sits just above at `0x80078DC0`. |
| `0x801F291C+` | records | Field-overlay effect-descriptor records `[0xFFFF0000][handler ptr][4 param words]` (persistent `0x801F0000+` region). Slot `0x801F2920` holds the CLUT cross-fade SM `FUN_801E4794` (the world-map palette-cycling writer); sibling slots point at `FUN_801E4D8C` / `FUN_801E5154` / `FUN_801E5338`. |

## World-map TMD and actor tables

The global asset-pointer table consumed by the world-map top-view
renderer (`FUN_801F69D8` → `FUN_80043390`). Live verification: see the
field-scene snapshot example in
[`formats/world-map-overlay.md`](../formats/world-map-overlay.md#how-slot-4-bytes-reach-cluster-a).
(The same `DAT_8007C018` table is filled identically by every field-scene load
- towns, dungeons, and the walk-view world map - via the single descriptor-walk
`FUN_80020224`; the verification capture is the `dolk` field scene, not a world
map, but the table layout is scene-independent.)

| Address | Type | Purpose |
|---|---|---|
| `0x8007C018` | `void *[N]` | **Global TMD pointer table.** Installer `FUN_80026B4C @ 0x80026BA8` is the *sole* writer. - [details ↓](#0x8007c018---global-tmd-pointer-table) |
| `0x8007B774` | u32 | Install counter for `DAT_8007C018`. Bumped by `FUN_80026B4C` on each install. `dolk` field-scene snapshot = `0x8F` (143 entries installed). |
| `0x8007BB38` | u32 | **Walker counter** (last installed index). Also written by `FUN_80026B4C` via `gp[+0x820]`; the `addu` between the gp-relative `lui+addiu` materialiser and the `sw` is what hides this store from Ghidra's xref database. Used by `FUN_801D8280` to bound the table walk. `dolk` snapshot = `0x8E` (= install counter − 1). |
| `0x8007B824` | u32 | Per-pack start index into `DAT_8007C018`. Set when a new pack begins, read by `FUN_8001E928` / `FUN_8001E890` post-install to update `DAT_8007B6F8`. `dolk` field-scene snapshot = `0`. |
| `0x8007B828` | u32 | TMD-magic-mismatch error bits. Set by `FUN_80026B4C` when an input fails the `*(input)==0x80000002` check (only flags the error; does not reject the install). `dolk` / `geremi` field-scene snapshots = `0x00000000` - every installed entry passes the magic check. |
| `0x8007B6F8` | u32 | **Kingdom-TMD prefix offset.** Count of party-character TMDs that precede the kingdom-bundle TMDs in `DAT_8007C018`. The world-map dispatcher does `DAT_8007C018[(actor_kind8 + DAT_8007B6F8) * 4]`, so this shifts world-map actor-kind indices past the party prefix. Writers: `FUN_80020118` (field-load entry; resets to 0) and `FUN_8001E890` / `FUN_8001E928` (set to `DAT_8007B824 + *player_pack_count`). `dolk` field-scene snapshot = `5`. |
| `0x8007B7DC` | `void *` | VDF buffer pointer. Set by asset-dispatcher case 7. `FUN_8001FBCC` walks each sub-entry and writes a parallel pointer table at `0x80083E58` (consumed by `FUN_801D77F4` for actor instance bring-up). |
| `0x8007B888` | `void *` | Slot-4 (MOVE) buffer pointer (set by `FUN_8001f05c` case 5; **not** installed into `DAT_8007C018`). Scene-dependent - a field-scene capture pinned it to `0x8011A624`, but the physical bytes get overwritten by the scene's TMD-pack install before steady state. |
| `0x80083E58` | `void *[N]` | Parallel VDF sub-entry pointer table. First entry points into VDF buffer; subsequent entries point into the actor-instance area. |

## Debug flags

| Address | Purpose |
|---|---|
| `0x8007B8C2` | Dev/retail loader-path flag. 40 `lh` reads in SCUS; written once, at cold boot, to `1`. |
| `0x8007B98F` | In-game debug menu enable. Accessed as the high byte of the word at `0x8007B98C`. |
| `0x8007B7C0` | Debug-dispatch trigger. |
| `0x8007B450` | Debug-dispatch parameter slot. Also used by the field-VM `STATE_RESUME` opcode (`0x49`) as its tristate state register. |
| `0x8007B6F4` | "Small maps" debug mode flag. |
| `0x8007B850` | Per-frame button mask (built by `FUN_8001822C`; retail truncates to port 0's low 16 bits - port 1 fills the high half only under `0x8007B98C != 0`). Packed layout, not the raw pad word: face/shoulder byte in bits 0-7, dpad/system byte in bits 8-15. Engine mirror `legaia_engine_core::retail_pad`. |
| `0x8007B7C0` | Previous-frame button mask. |
| `0x8007B7C4` | Changed-this-frame mask (`held ^ prev`, `FUN_8001822C`). |
| `0x8007B874` | "Newly pressed this frame" (edge detection). |
| `0x80089128` | 32 × u32 held-mask history ring, one entry per elapsed vsync (write index `gp+0x62C` = `0x8007B944`). |
| `0x8007B938` | AND of the whole `0x80089128` ring (`gp+0x620`) - bits held for the full 32-vsync window. |
| `0x8007B93C` | Auto-repeat pulse (`gp+0x624`): the AND-window mask on the frame the `0x8007B940` (`gp+0x628`) countdown underruns; rearms at `+8` vsyncs. Menu auto-repeat source. |

JP retail uses build-shifted addresses (`0x07D51F` for the in-game debug menu enable; +0x1B90 from the NA address).

### `0x8007B8C2` - build-mode (dev/retail) loader selector

This halfword is the single most-consulted build-mode switch in the executable.
Every one of its 40 read sites is an `lh` (signed halfword) at `0x8007B8C2`, and
every one splits the same way:

- **`!= 0` - retail.** Open by PROT-TOC index from the in-RAM table at
  `0x801C70F0` (`FUN_8003E8A8` + `FUN_8003E800`, or `FUN_8003EB98`).
- **`== 0` - dev.** Open by filename through `FUN_800608F0`, whose entire body
  is `break 0x103` - a PsyQ dev-station host trap. Retail hardware cannot
  service it, and the `h:\` paths it opens are not on the disc.

Read sites include the `FUN_8001FD44` scene-change packet
([`subsystems/asset-loader.md`](../subsystems/asset-loader.md)), field
locomotion's streaming read
([`subsystems/field-locomotion.md`](../subsystems/field-locomotion.md)), the
battle-data archive open at `FUN_8003E8A8(0x365)`
([`subsystems/battle.md`](../subsystems/battle.md)), and the world-map overlay
pool fill ([`formats/world-map-overlay.md`](../formats/world-map-overlay.md)).

**Retail boots with the halfword at `1`.** `main()` (`FUN_80015E90`) writes it
once during cold-boot init:

```
80015f00  jal  0x8003F084      ; body is `jr ra` / `addiu v0,zero,0x1` - returns 1
80015f04  nop
80015f08  sh   v0,0x5aa(gp)    ; gp = 0x8007B318, so EA = 0x8007B8C2
```

`FUN_8003F084` is a two-instruction leaf that returns the constant `1`
unconditionally, and `0x80015F00` is its only caller - a stubbed-out build-mode
predicate the dev build presumably returned `0` from. The store is **gp-relative**,
which is why address sweeps that searched only the absolute `lui 0x8008` /
`-0x473e` form reported "zero writers" (see
[`tooling/ghidra.md`](../tooling/ghidra.md#decompiler-artifacts-that-have-produced-false-claims)).

The flag is **not** established by BSS zero-init: `SCUS_942.54`'s PS-X EXE header
carries `b_addr = 0, b_size = 0`, so the BIOS clears no BSS for this executable
at all. The boot-time store is the only thing that sets it. Live save states
agree: the halfword reads `1` in all 60 captured states, across field, battle,
world-map, stock-disc and randomized runs.

The companion in-game debug-menu enable `0x8007B98F` is **not** stripped, and an
earlier claim here that "the dev branches that gate on it appear stripped at link
time; no references remain" is superseded. It has no *byte-granular* reader because
it is byte +3 (MSB, little-endian) of the 32-bit debug-mode word `_DAT_8007B98C`,
and that word is the consumer surface - statically pinned at `FUN_8001822C` plus the
resident field-overlay gates. Poking `_DAT_8007B98F = 1` brings up the debug menu on
SELECT+triangle in the NA retail build, which is the direct refutation of "no
references remain". See
[`open-rev-eng-threads.md`](open-rev-eng-threads.md#debug-flags-0x8007b8c2--0x8007b98f).
None of the 557 catalogued GameShark / Pro-Action-Replay codes in
[`legaia-cheats`](cheats.md) target `0x8007B8C2` or `0x8007B98F`.

**Not reachable from the script-VM flag ops.** The field-VM SET/CLEAR/TEST flag
ops (`0x50`/`0x60`/`0x70` → `FUN_8003CE08`/`_CE34`/`_CE64`, shared with the move
VM) write `(&DAT_80085758)[(int)idx >> 3]` (`sra`, i.e. arithmetic shift) into
the single fourth flag bank at `0x80085758`. The field-VM dispatcher constructs
`idx = (opcode & 0x8F) << 8 | operand` in full-width registers - always positive,
max `0x8FFF` - so its window is `[0x80085758, 0x80086957]`. The move-VM widget
wait-op instead reads its flag operand as an **i16**, whose sign-extended
negatives reach down to `0x80085758 - 0x1000 = 0x80084758`. Neither window
includes `0x8007B8C2` (it lies below both lower bounds) - there is no
out-of-bounds flag-index path from script bytecode to the build-mode selector or
the debug-menu enable. A clean-room engine should treat both as build-time
constants and keep its flag-bank writes bounded.

## PSX scratchpad (`0x1F800000-0x1F8003FF`)

The PSX has 1 KB of fast scratchpad RAM mapped here. Legaia uses the high end:

| Address | Type | Purpose |
|---|---|---|
| `0x1F800314` | i16[] | Inverted-Y mirror table (op 0x4C nibble-9 sub-E writes `-words[i]` here). |
| `0x1F800393` | u8 | Per-frame tick byte. Global frame-time scalar. Read by op 0x4A `WAIT_FRAMES` and the 0xFFFF sentinel in op 0x4C nibble-C sub-B/C. Also subtracted from the title-attract countdown at `0x801EF16C` every tick (see [`subsystems/boot.md`](../subsystems/boot.md#tick-function)) and exposed via `World::tick_move_vms_with_delta` in the engine port. |
| `0x1F800394` | u32 | **Field-VM transient flag word** (32-bit; **not** persisted - distinct from the saved story-flag bitmap at `0x80085600..0x80085800`, ops `FUN_8003CE08/CE34/CE64`). Script-VM bits are set/clear/tested by `GFLAG_SET` / `GFLAG_CLR` / `GFLAG_TST` (ops 0x2E / 0x2F / 0x30, `1 << (idx & 0x1f)` at `FUN_801DE840:5280/5284`); also gates op 0x4C nibble-4 sub-9's tristate dispatch via bits `0x01000000` / `0x02000000`. The **lower 16 bits** are re-seeded on every mode switch from `mode_table[_DAT_8007B83C].param` (`+0x14`) by `FUN_8001DCF8 @ 0x8001E17C` - its sole non-RMW writer (see [`save-screen.md`](../subsystems/save-screen.md)). Bit 0x40 is set by the scene-change packet `FUN_8001FD44` (a scene-transition-pending flag, **not** a "dialog active" lock - an earlier mislabel). |
| `0x1F8003E8` | u32 | Render-config block (op 0x46). |
| `0x1F8003EC` | u8[] | Tile-flag bitmap base used by op 0x4C nibble-7 (rectangle SET/CLEAR over `+0x4000` offset). |
| `0x1F8003F8` / `0x1F8003FA` | i16 | Camera-scroll values used by op 0x23 player path. |

## Overlay window (`0x801C0000+`)

The 256 KB overlay window is shared between several runtime overlays - only one is loaded at any time. See [`tooling/overlay-capture.md`](../tooling/overlay-capture.md) for the per-overlay capture protocol and [`subsystems/boot.md`](../subsystems/boot.md) for which overlay loads when.

| RAM range | Overlay | Subsystems |
|---|---|---|
| `0x801C0000+` | Title screen | Actor / sprite VM (`FUN_801D6628`); title-overlay tick `FUN_801DD35C` at `0x801DD35C` (decrement instruction at `0x801DDCCC`, see [`subsystems/boot.md`](../subsystems/boot.md#tick-function)) |
| `0x801CE818+` | Town / field / dialog (loaded from PROT entry `0897_xxx_dat`) | Field VM (`FUN_801DE840`), MES renderer, inventory hub, MAIN INIT |
| `0x801CE818+` | Battle (loaded from PROT entry `0898_xxx_dat`) | Per-actor state machine, battle main dispatcher, effect VM cluster |
| `0x801CE818+` | Options / pause / save / shop menu (loaded from PROT entry `0899_xxx_dat`; the historical "PROT 0896 @ `0x801C5818`" attribution is refuted - 0896's recovered base was an over-read artifact, and live field captures hold an ISO9660 directory cache at `0x801C5818`) | In-game menu UI |
| `0x801EF018` | Title-overlay state struct base | `+0x154` (u32) = title-attract countdown `_DAT_801EF16C` (init `0x8000`, decremented by `_DAT_1F800393` per frame, underflow writes `_DAT_8007B83C = 0x1A` → STR FMV mode 26 → `MV1.STR`); `+0x158` (u32) = title-overlay frame counter `_DAT_801EF170`. |
| `0x801F0000+` | Battle effect helpers extend into here | `0x801F5D90`, `0x801F5CF8` (effect_id specials), `0x801F8004 / 88FC / 8D4C / 8E6C / 8F28` (particle / emitter cluster) |

## Mini-game state regions

Each mini-game gets its own ~64 KB slab of upper RAM, loaded fresh when entered. See [`reference/builds.md`](builds.md) for the per-mini-game RAM addresses.

## Global / cell details

Full write-ups for the rows above whose detail outgrew a table cell. Linked from the section tables by **[details ↓]**.

### `0x80085958` - Item inventory

**Item inventory** array (= SC `+0x1818`), 2-byte stride `(id, count)`; the `Have 99 Items` cheat targets the count bytes over `0x80085958..0x800859E8`, which is that cheat's 72-slot general-item **page**, not an engine bound. Stacks cap at 99. The read/consume/merge accessors (`FUN_80042310`/`_42EE0`/`_42F4C`/`_423E0`/`_43048`) all scan/write within the active window `gp[+0x2D2]..gp[+0x2D4]` and fully bound the slot index on `gp[+0x2D4]`.

**The active window is context state, installed by `FUN_8004313C`** - the sole
`SCUS_942.54` writer of either halfword (11 callers; `FUN_800423E0` calls it
before normalizing). It branches on the party-member count at SC`+0x454`
(`0x80084594`):

| members | window installed |
|---|---|
| `0` | none - the previous window stays intact |
| `1` | story flag 20 (`FUN_8003CE64`) set: `[0, 256)`; clear: the half chosen by `0x80084598` - `[128, 256)` when nonzero, else `[0, 128)` |
| `>= 2` | `[0, 256)`, no flag test |

The length also lands in `gp[+0x2D6]`, so `gp[+0x2D4]` is only ever `128` or
`256`. Live cross-check on a mid-game battle state with a three-member party:
`3` at `0x80084594`, `(start, end, len) = (0, 256, 256)`, 160 contiguous
occupied slots. A sweep of the item-menu overlay (`overlay_menu.bin`, all 129 functions via `dump_menu_inventory_refs.py`) finds **zero** direct array writes: every one of its 17 inventory ops calls these SCUS helpers (passing item ids / helper-returned slots), so the menu has no raw-index sort/swap primitive.

The **add** helper `FUN_800421D4` is the one exception worth noting: its id store precedes the bound check, so a full-window add writes the item id **one slot past** the window (`0x80085958 + gp[+0x2D4]*2`); only its count store is guarded (see [`functions.md`](functions.md)).

**Where the OOB lands (CORRECTED).** The target is `0x80085958 + gp[+0x2D4]*2`, so with `FUN_8004313C`'s only two `end` values it is `0x80085A58` (`end = 128`) or `0x80085B58` (`end = 256`). The earlier "`0x800859E8` = SC+0x18A8, the first key-item slot" reading rested on the 72-slot page being the window; it is not.

**The `pc=0x800422BC` probe hits are not OOB evidence.** That store executes on
*every* successful add, before the guard - the free-slot scan runs once and
exits either at the first `id == 0` slot (ordinary, in-window) or at
`i == end` (the primitive). The probe's two hits - casino prize-exchange
`id=0x9C` at `0x800859E8` and equip-unequip `id=0xD0` at `0x800859EA` - are
consecutive slots 72 and 73, i.e. exactly the ordinary id store for a bag whose
first free slot was 72. **Reachability of the primitive: unreachable through the
retail add call sites in normal play.** A `[0,256)` window holds at most 255
distinct ids (the merge pass keys on the id byte, `0` is the empty sentinel), so
a hole always remains and the free-slot scan exits in-window; the half-windows
are a transient solo phase whose real item population is well below 128. Only a
non-add path (debug menu / cheat / crafted save) can force the `i == end` exit.
See [`re-settled-threads.md`](re-settled-threads.md#full-window-item-add-oob-reachability).

A memory-safe RE model of this accessor family (incl. the primitive surfaced as
`AddOutcome::OobIdWrite { oob_target, written_id }`) lives in
[`legaia_save::retail_inventory`](../../crates/save/src/retail_inventory.rs).

### `0x8007C018` - Global TMD pointer table

**Global TMD pointer table.** Installer `FUN_80026B4C @ 0x80026BA8` is the *sole* writer (verified across SCUS + every world-map overlay via [`find_addr_materializer_dat_8007c018.py`](../../ghidra/scripts/find_addr_materializer_dat_8007c018.py)). Every populated entry `[0..DAT_8007BB38]` is a post-fixup Legaia TMD: magic `0x80000002`, flags = 1, `group_count` at `+0x8`, group-descriptor array (`0x1C`-byte stride) at `+0xC`.

A settled **field-scene** snapshot (scene `dolk`, `game_mode 0x03`, scene id `0x3c` - note: the local capture file labelled "drake_world" is in fact this `dolk` field scene, **not** the Drake world map): 143 entries - `[0..4]` = 5 character-mesh TMDs (disc source: PROT 0874 `befect_data` section 0, byte-equality verified; see [`world-map-overlay.md` § Disc-side source of `[0..4]`](../formats/world-map-overlay.md#disc-side-source-of-04)),

`[5..142]` = 138 **scene-pack** TMDs - the scene's field-file TMD pack, installed as one contiguous 138-entry pack by the single descriptor-walk `FUN_80020224` → `FUN_8001f05c` case 2 → `FUN_80026B4C` (the 0x8011xxxx-region addresses formerly classified as "slot-4 body-aligned" are simply TMDs from that pack; type-`0x05` slot-4 does **not** install into `DAT_8007C018` - only cases `0x02`/`0x09` reach `FUN_80026B4C` - so the slot-4 outer-pack signature is absent from steady-state RAM). A mid-load snapshot (the `geremi` field scene, scene id `0xa5`, captured mid-load) shows fewer installed entries; reading past `DAT_8007BB38` returns stale pointers from the previous game state, **but no consumer ever does this**. Consumed by `FUN_801F69D8` (world-map top-view),

`FUN_80021B04` / `FUN_80024D78` (SCUS actor allocators), `FUN_801D77F4` (overlay actor allocator), `FUN_801D8280` (table walker), `FUN_8001E890` (per-pack count override), `FUN_8001EBEC` (per-party-member group-descriptor patch - equipment-conditional mesh swap for 3 party slots at `DAT_8007C018[DAT_8007B824 + 0..2]`).

## See also

- [`docs/reference/functions.md`](functions.md) - the functions that read and write these globals.
- [`docs/subsystems/boot.md`](../subsystems/boot.md) - how the PROT TOC and key globals get installed at `0x801C70F0`.
- [`docs/tooling/ghidra.md`](../tooling/ghidra.md) - the LUI+ADDIU writer hunt that pins these addresses.
