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
| `0x8006F180` | data segment start | First byte of the `SCUS_942.54` data segment (50816 bytes to the image end, zero `jr ra`); `disc-coverage.py`'s `data_floor` for SCUS. |
| `0x8007668C` | u32[12] | Bank-3 cluster-A primitive-handler table, kinds 8..19 (`0x8004409C` .. `0x800453BC`, last slot `FUN_80045BB4`). |
| `0x800840F8` | u32 | BIOS pad data (read by `FUN_8001822C`). |
| `0x8007BD10` | u8[] | Present-party list: battle slot -> **roster character id** (1-based); the queue builder, actor seeding and strike loop index `0x80084140 + (id - 1) * 0x414` through it. |
| `0x8007B7CC` | u32 | Save / title block-grid cursor, linear `col + row*5` over the 5x3 grid; three references disc-wide (all PROT 0899), single writer `0x801DED2C`. |
| `0x8007C364` | ptr | Player field ctx pointer (`0x8007C348 + 0x1C`); op-`0x23` MOVE_TO and the `0x43` arm pick the player by comparing the executing ctx against it. |
| `0x80084340` | inventory base | Per-page inventory state, 0x414-byte stride. |
| `0x80084540` | u16 | Current map / scene PROT base index. |
| `0x80084594` | u8 | Party member count. |
| `0x80084598` | u8[] | Party member IDs (sorted insertion, cap 4). |
| `0x80084628` | i16 | Set by op 0x4C nibble-8 sub-8. |
| `0x800846D0..DC` | 4 × u32 | Button-mask config words: `0x44` Cross\|L1, `0x21` Circle\|L2, `0x10` Triangle, `0x48` Cross\|R1 (the field run button, tested against the held mask `_DAT_8007B850`). Seeded once by the new-game data init `FUN_80034A6C`; inside the saved block; no other writer. |
| `0x80085758` | u8[] | **Fourth flag bank** - bitfield accessed via SET / CLEAR / TEST `(idx >> 3, 0x80 >> (idx & 7))` (`FUN_8003CE08`/`_CE34`/`_CE64`). The field-VM opcode encoding spans `idx = 0x000..=0xFFF` plus `0x8000..=0x8FFF` (extended-prefix opcodes `0xD0..=0xFF`), so it is **not** a fixed 256-bit array. The earlier `0x80086D70` was a double-count of the `0x1618` save displacement onto `0x80085758` (which itself already `= 0x80084140 + 0x1618`); see [`subsystems/script-vm.md`](../subsystems/script-vm.md). |
| `0x80077828` | u8[] | **Per-monster steal table** (`DAT_80077828`). Indexed by 1-based monster id at `+id*2`; each entry is `[steal_chance_pct: u8, steal_item_id: u8]` (chance first). What the Evil God Icon steals - NOT in the PROT 867 record. See [`docs/formats/steal-table.md`](../formats/steal-table.md); parser `legaia_asset::steal_table`. |
| `0x80087AF8` | u32 | Result of `FUN_80020224` descriptor walker, set by town-overlay MAIN INIT. |
| `0x800845DC` | (mirror of `_DAT_80084570`) | Snapshot written by op 0x4C nibble-E sub-E. |
| `0x800845A4` | u32 | Casino coin bank. "Infinite Coins" cheat writes `0x05F5_E0FF`. |
| `0x800845B4` | u32 | **Point Card counter** (unmapped by every public cheat archive). The shop buy commit `FUN_801db7f4` (menu overlay) accrues `price/20 * qty` into it when item `0xFE` (the Point Card) is held (`func_0x80042f4c(0xFE)` inventory-has gate), capped at `9,999,999`. Menu display readers at `0x801d1008`/`0x801dce84`. Also the sink of applier selector `0x0E` (Point Card discharge), which clamps through `FUN_801F44A0` to `min(counter, 0x270F)`. GameShark-style max: 16-bit pair `800845B4 967F` + `800845B6 0098`. `see ghidra/scripts/funcs/overlay_shop_save_801db7f4.txt`. |
| `0x8007BB80` | u32 | Menu window-slide latch: non-zero while a window is sliding; every menu sub-screen SM gates its interactive phase on `== 0`. |
| `0x8007BB84` | u32 | Menu pad-**edge** word (remapped d-pad bits `0x1000` Up / `0x4000` Down / `0x8000` Left / `0x2000` Right) read by the kind-4 list kernel `FUN_80032A44` and the quantity pickers. |
| `0x8007BB88` | u32 | List-kernel selected-row **payload** (low 12 bits of the row entry - the bag slot on the item lists). Doubles as the name-entry grid cursor. |
| `0x8007BB90` | i32 | List-kernel persisted **scroll top** (`gp+0x878`); clamped in place by the allocator `FUN_80030104`. |
| `0x8007BB94` | i32 | List-kernel **mode/result**: 0 idle, 1 browsing, 2 row confirmed, 3 cancelled, 4 parked behind the command window. |
| `0x8007BB98` | i32 | List-kernel persisted **selected row** (`gp+0x880`); clamped to `count-1` on rebuild. |
| `0x8007BB9C` | u32 | Selected row's **class nibble** (`entry & 0xF000`) - the screen-id key `FUN_80034250` dispatches descriptions on. |
| `0x8007BBA0` | i32 | List-kernel **row count** (mirrored from the allocator; bounds the sell-list scroll fix-up in `FUN_801DBD94`). |
| `0x8007BB04` / `0x8007BB08` | ptr | Two `0x8000`-byte render scratch buffers allocated by `FUN_800271A8` via `FUN_80017888` (each guarded by a `== 0` first-use check). `0x8007BB08` is filled with a `0x4000`-entry `u16` depth ramp; the pair feeds the GTE / primitive-buffer reset (`FUN_8005B268` / `FUN_8003D1A4` / `FUN_8003D254`). See `ghidra/scripts/funcs/800271a8.txt`. |
| `0x80070644` | actor template | Third record of the static actor-template table at `0x800705FC` (`+0x02` id `0x15`, flag word `0x80`, initial state `1`, `+0x08` tick `FUN_801D820C`). Materialised by exactly one site on the disc - the `lui`+`addiu` pair at `0x801D8370` inside the actor-clone helper `FUN_801D835C` - so the clip timer runs on the clone field-VM op `0x4C` sub-1 sub-op `0x14` spawns and nothing else. See [`functions/game-modes.md`](functions/game-modes.md#801d820c). |
| `0x80076BBC` | 4 x 12 B | Muscle Dome **direction-chip seat** array (SCUS file `0x673BC`, immediately before the placement table). Seat x per command slot = `176 / 216 / 216 / 256`; case `9`'s four-iteration loop reads `+0` per slot and then subtracts `(cost - 30) * K[slot] / 2`. See [`minigame-muscle-dome.md`](../subsystems/minigame-muscle-dome.md#the-pennant-geometry-is-linear-in-the-commands-ap-cost). |
| `0x8007B650` | u8[4] | The `K` weights `[2, 1, 1, 0]` of that re-centring term (SCUS file `0x6BE50`, followed by the `Auto` / `Command` strings - which is what fixes the file/VA pairing). `K` makes the arm chip grow *away* from the D-pad glyph between the pair. |
| `0x8007B820` | u32 | Title **row counter / 2-option cursor** for sub-mode `0x10`; `AttractIdle` steps it free-running and wraps it with `andi v1,v1,0x1` at `0x801DDC00`. Up/down are `pad & 0x4000` / `0x1000`, confirm `0x844`. |
| `0x8007BA88` | u32 | SFX **force-latch**: `FUN_800243F0` reads it at `0x800244C0`, and while it is non-zero every cue is forced onto mixer channel `6` ([`sfx-table.md`](../formats/sfx-table.md)). Cleared by the side-band stream teardown `FUN_801D8450` (field-VM op `0x36` sub `3`). |
| `0x8007BAA0` | i32 | Side-band `vab_01` bank **acknowledge** cell, latched from the request by `FUN_800243F0` at `0x8002448C` / `0x800244F0`. `-1` is the *idle* sentinel, not an error. |
| `0x8007BABC` | i32 | Side-band `vab_01` bank **request id** - the other half of the pair. The field overlay seeds `(8, -1)` at `0x801D6880..0x801D688C` and tears down to `(-1, -1)` at `0x801D74AC..0x801D74B8`; op `0x36`'s subs `0`/`1`/`2` are request, gated store and wait on the pair's equality ([`script-vm.md`](../subsystems/script-vm.md)). |
| `0x8007BAB4` | u32 | Title **pre-attract hold**, seeded `0x100` by the SCUS stager at `0x8002579C` (its only writer outside the tick) and spent at `8 * frame_scalar` per frame by sub-mode `0x11` `AttractDelay` - 33 frames before the menu comes up. |
| `0x8007BB00` | u32 | Title **entry word**. Raised unconditionally by the boot `init.pak` itself (`li s2,0x1` / `sw s2,-0x4500(s0)` at `0x801CEB84`, `s0 = 0x80080000`), which is why a cold boot never shows title sub-mode `0x02` ([`boot.md`](../subsystems/boot.md#a-cold-boot-always-shows-sub-mode-0x10-never-0x02)). Reads `2` on a re-entry from the attract FMV. Both master-mode-`2` writers (`0x801DFC00` new game, `0x801DFAFC` load) clear it. |
| `0x8007BAC0` | u32 | **Special-battle restriction word.** Low byte = `(course << 4) + round + 1`; bit `0x100` bars Item, bit `0x200` bars magic. Written only by SCUS battle init, keyed on the *first enemy monster id* (`ori v0,v0,0x200` at `0x800519FC` after the `lbu -0x42f4`; `0x3D..=0x3F` under mode `0xC`/`0x15` at `0x8005200C`) - so no Muscle Dome round raises the magic bit, the ladder topping out at monster `0xAA`. Non-zero gates the readers `0x801D322C` / `0x801DE450` / `0x801E6578` / `0x801E9224` / `0x801EA994`; `0x100` also gates the award arms `0x801E7978` / `0x801E7B40`. The dome's own arena init `FUN_801D0088` writes the low byte and nothing else. |
| `0x8007B8B8` | u32 | **Field-state latch / ambient-template gate.** Raised by the mode-entry prologue `FUN_80016230` when the mode being left is the field, and tested `== 0` by the field MAIN INIT `FUN_801D6704` before it spawns the one plain template at descriptor `0x801F271C` (`0x801D6FD8`). What sequences its writers across a load is [an open thread](open-rev-eng-threads.md#field--locomotion). |
| `0x8007BCAC` | i16 | **Camera vertical offset** (not a yaw). Seeded `0x3C` by the field MAIN INIT `FUN_801D6704` at `0x801D67B8`; written alongside `ctrl[+0x4A]` by the bit-24 arm of field-VM op `0x4C` nibble-4 sub-9 at `0x801E1560`, and eased toward the player's Y by `FUN_801DA390` (`ctrl[+0x4A] - actor[+0x16]`, `0x801DA3B4` / `0x801DA3B8`). Port `World::tick_camera_offset_ease`. |
| `0x800846C4` | u32 | Saved **Auto / Command** battle-preference word - inside the saved block, alongside the button-mask config at `0x800846D0`. |
| `0x8007BB4C` | u32 | Selector between PROT 0901's **two** ground-cell emitters: `FUN_801F69EC` (file `+0x14`) and `FUN_801F6D48` (file `+0x370`), picked at `0x801F79A0`. Both gate on `cell & 0x1000`; the decoration pass `FUN_801F7088` is a third routine gating on `0x2000`. |
| `0x8007BD71` | u8 | Battle **run state** byte: `0xFF` while a battle is running, `0xFE` once it is ending. It is the second gate on SCUS's `jal 0x801F7B88` at `0x800481A0`, which is why that call is an in-battle path rather than a teardown one. |
| `0x8007B464` | u32 | Nine-slice **style** selector (`gp[+0x14C]`). `FUN_8002C69C` indexes `0x800732A4 + style*12` with it and `jr`s the table at `0x80010D18` on the descriptor's byte 0. Style `0x03` is the post-battle report band; `0x01` / `0x02` are the HUD plaques. |
| `0x800732A4` | 12 B/row | Nine-slice **style descriptor table**. Row = `[kind, clut byte, tile-set index, ...]`; kind picks the emitter arm, the CLUT byte becomes `0x7FC0 + n` (style `0x03` -> `0x7FC2`, styles `0x01`/`0x02` -> `0x7FC4` / `0x7FCC`). |
| `0x80073A00` | tiles | Nine-slice **tile set 0** - the eight corner/edge/centre tiles style `0x03` lays, byte-identical to the 36-`SPRT` capture of a post-battle report band. Sets 3 and 4 are the HUD plaques'. |
| `0x80014FA0` | u32[132] | Battle-applier **selector jump table**. 132 slots over 15 distinct targets; 116 of them point at the shared epilogue `0x800421A8`, so the space above `0x0E` is almost entirely empty. The one body above `0x0E` is slot `0x82` at `0x800421A0` (a brightness ramp). |
| `0x800111C4` | u32 | CD / XA **transport state word** - the 11 states `FUN_8003D764` walks. Engine mirror `xa_transport`. |
| `0x800156EC` | text | The literal ASCII `MoveImage`, materialised by `FUN_80058490` for the PsyQ debug-name registration `FUN_80058170`. It is the anchor that settles that routine's identity: `FUN_80058490` is a `MoveImage` wrapper, not a sound-driver lane, so the table at `0x801F6418` holds VRAM **x** coordinates rather than cue ids. |
| `0x8007BDC0` | u32 | `gp+0xAA8`. The PROT 0920 (`cast_slippery`) **effect-drain counter** whose non-zero arm is the only thing that reaches SCUS `0x800481A0`'s `jal 0x801F7B88`; nothing in the save-state corpus has the gate up ([`cast-module.md`](../subsystems/cast-module.md)). |

## Game-mode state machine

Companions of the 28-entry × 24-byte mode-dispatch table at `0x8007078C` (the table itself is documented in [`subsystems/boot.md`](../subsystems/boot.md) and [`functions.md` § Game-mode state machine](functions.md)).

| Address | Type | Purpose |
|---|---|---|
| `0x8007B83C` | u16 | **Next game-mode index** (master mode selector; stored via `sh`). Drives the per-frame mode dispatcher: `0x02` field-launch, `0x03` field-run, `0x15` battle, `0x1A` STR FMV. Title-attract underflow writes `0x1A` (see `0x801EF018`). The front-end writes it six times on the way to the field - [details ↓](#0x8007b83c---the-front-end-mode-chain). Also indexes the mode table - `(&DAT_800707A0)[_DAT_8007B83C * 0x18]` (entry·24 + 0x14 = the mode's `param`) - which `FUN_8001DCF8` uses to seed the **lower 16 bits** of the field-VM flag word `0x1F800394` on each mode switch (see the `0x1F800394` row and [`save-screen.md`](../subsystems/save-screen.md)). |
| `0x8007B87C` | index | Mode index rendered on the dev **CONFIG / test screen**. `FUN_800188C8:340`: `(&PTR_s_CONFIG_8007078c)[_DAT_8007B87C * 6]` - indexes the `0x8007078C` table at stride **6 words = 24 bytes** (independently re-confirms the 24-byte entry stride) to fetch the entry's CONFIG label string for `FUN_8001AA68` to draw. Provenance: `ghidra/scripts/funcs/800188c8.txt`. |
| `0x8007B7AC` | mode | Mode-dispatch-cluster cell read by `FUN_8001DCF8:370` (`if (_DAT_8007B7AC == 1)`), a function that also branches on `_DAT_8007B83C` against mode constants `0x0E/0x02/0x18/0x14`. Reads as the **outgoing / previous** game mode rather than a boolean: the mode-entry prologue `FUN_80016230` gates its whole field-state snapshot (player XZ into `0x80084568`/`0x8008456C`, `_DAT_8007B8B8 = 1`, the actor-pool park into VRAM) on `_DAT_8007B7AC == 3`, and `3` is MAIN MODE - a guard whose job is "only preserve field state when the mode being left is the field". That is an inference from the guard's purpose, not a pinned writer; a write-watchpoint across a field→battle transition closes it. Provenance: `ghidra/scripts/funcs/8001dcf8.txt`, `80016230.txt`. |

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
| `0x8007B7F8` | Pointer to the **cosine** view of the shared trig LUT - `0x8007122C`, the sine base offset by a quarter turn. Installed by `FUN_80026BE0` (`0x80026C0C`). |
| `0x8007B81C` | Pointer to the **sine** base `0x80070A2C` (`FUN_80026BE0` at `0x80026C00`). The table is `4096 * sin(2*pi*i / 4096)` over 5120 `i16` entries, `0x80070A2C..0x8007322C` - one revolution plus the quarter turn the cosine view needs, which is why consumers mask the angle with `0xFFF`. |
| `0x8007B824` | u32 - Party base index into `DAT_8007C018` (see the fuller entry below); read by `FUN_8001EBEC` to address the three active-party battle-TMD pointers `DAT_8007C018[0x8007B824 + 0..2]`. (Earlier "sound mode index" reading was wrong.) |
| `0x8007B840` | MOVE2 buffer base - the type-`0x0B` high-id animation-clip bank (`actor[+0x5C] >= 0x400`). |
| `0x8007B888` | MOVE buffer base - the type-`0x05` low-id animation-clip bank; on the world map this is kingdom slot 4 (`world-map-overlay.md`). |
| `0x8007B75C` | Party / character animation-clip bank (`actor[+0x10] & 0x01000000`); live `map01`: `0x801589E4`, 23 clips. |
| `0x8007B750` | u32 - Sound flag word coordinating the BGM track-swap handshake (bit 1 = pause, bit 3 = load settled, bit 4 = release ack); full bit map + writer census in [`audio.md`](../subsystems/audio.md#the-track-swap-handshake-fun_800243f0--op-0x35-sub-op-0xa). |
| `0x8007B868` | u32 - Dev/dual-mode gate the actor-sound family and several loaders check (`retail 0`). No static writer sets it - its only store, in `FUN_8001DCF8` (`0x8001E008`), clears bit 1. |
| `0x8007B8D0` | u32 - sound subsystem current-bundle pointer (`gp+0x5B8`): `bse.dat`'s 0x1800-byte buffer during battle, repointed at the scene's prescript bundle on every field load (`FUN_8001F7C0` at `0x8001F864`). See [`bse-dat.md`](../formats/bse-dat.md). |
| `0x8007B64B` | u8 - **backdrop last-object keep flag** (`gp+0x333`). Zero means the battle scene loader `FUN_800513F0` removes object index 1 from both backdrop actors' part lists (`0x80051ABC..0x80051BAC`) - the Muscle Dome arena's dust decal is trimmed this way. One writer: the field handoff `FUN_801D9E1C` at `0x801DA0AC`, `(s2[+8] >> 5) & 1`. Readers `0x80051ABC`, `0x80046D34`, 0979 `0x801CF700`. Capture-confirmed `0x00` through a Muscle Dome contest (zero writes). |
| `0x8007B64A` | u8 - battle **stage id**; cleared by the stage module itself (`0x801F7120` in PROT 0968) when it hands the flow back. |
| `0x8007BC20` | u32 - the executable's **`xa_flag`** debug counter (XA-drive state): `FUN_80016B6C` loads it at `0x80016EB8` for the debug printf at `0x80016EC0` (format string `0x80010238`); `FUN_8004DA00` zeroes it on five arms. The melee grunt gate `0x801EEAB8` reads it as `< 2`. Not a character level. |
| `0x801F73F8` | u32 - PROT 0968 stage-module-local dt countdown (module base `0x801F69D8` + `0xA20`); its expiry hands `ctx[+0x06]` back to `0x0B` at `0x801F713C`. |
| `0x801CF56C` | u32[32] - **capture-class cast-tick arm table** (PROT 0898 data; sibling of the `0x801CF4EC` table behind `FUN_801F1ED4`). Read by `FUN_801F2160`, indexed by the spell record's `+1` sub-id; arm `i` calls into PROT `935 + i`. |
| `0x8007B990` | ptr - runtime SFX descriptor-bank record table (`gp+0x678`); `bse.dat`'s in battle. Written once by `FUN_8001FA88` at `0x8001FBC0`; read only by the cue router `FUN_8004FE5C` and the overlay-0971 debug sound test, both rewriting byte `+4` (category) of `record[cue_id - 0x200]`. Readers form it with `lui`+`lw`, invisible to the word scan. |
| `0x8007BAC8` | u16 - BGM ID written by field-VM op 0x35 sub-1. |
| `0x8007BC64` | u16 - Global BGM pool base for IDs ≥ 2000. |
| `0x8007BD30` | 5008 bytes - Effect-runtime pool: 16-byte head + 128 child slots + 32 master slots. |
| `0x8007BD5C` | u32 - Effect 2-pack wrapper buffer pointer (post-init). |

## Runtime PROT TOC + asset chain

| Address | Purpose |
|---|---|
| `0x801C70F0` | In-RAM PROT TOC - populated at boot by `FUN_8003E4E8`. Different stride from on-disc. |
| `0x801C6EA4` | Current world / scene struct pointer. |
| `0x801C6ED8` | CD-XA streaming-clip table: 34 slots of `[CdlLOC][u32 byte_len]` (8-byte stride, indexed by clip id; `+0x0` = 4-byte BCD-MSF `CdlLOC` disc start, `+0x4` = length, zero = empty slot). **Slot `i` = file `XA<i+1>.XA`** - lengths byte-exact vs the disc. Filled at boot by `FUN_801CFA78` (PROT 0895 `init.pak`), which sprintf-generates `\XA\XA%d.XA;1` per slot and stages `[BCD-MSF][size]` via ISO9660 lookup `FUN_8005DBB4` - no XA LBA is stored absolutely, so the table survives disc relayout. Read by the XA cue starter `FUN_8003D53C` (via `msf_to_lba`, `FUN_8005C42C`), which drives the CdSync-callback state machine `FUN_8003D764` (`CdlSetfilter {file 1, chan}` per cue). See [`cutscene.md`](../subsystems/cutscene.md#xa-channel-selection). |
| `0x801C6460` | 64-entry × u16 scratchpad slot table. Written by op 0x4C nibble-C sub-A; adjusted by sub-B / sub-C. |
| `0x801C66A0` | 64-slot ramp scheduler pool (stride 0x20). Installed by `FUN_8003C5F0`, walked by `FUN_80036D80`. |
| `0x8007BB20` | Timed sound-source auto-release **armed flag** (`gp+0x808`); set by `FUN_800267A8`, cleared on expiry by `FUN_800267FC`. |
| `0x8007BB24` | Audio level latched at arm time (`gp+0x80C`) - the value of `_DAT_8007B910` when `FUN_800267A8` ran. |
| `0x8007B910` | **Live audio level** (`0..255`), halved into libsnd's `0..0x7F` by every reader; persistent reference at `0x8008457C`. Not screen brightness - that is `0x8007B440`. See [`battle-action.md`](../subsystems/battle-action.md#the-_dat_8007b910-ramps-are-an-audio-duck). |
| `0x8007BB28` | Caller tag stored at arm time (`gp+0x810`). |
| `0x8007BB2C` | Auto-release **deadline** in vsyncs (`gp+0x814`). |
| `0x8007BB34` | Auto-release **elapsed** accumulator (`gp+0x81C`); advanced by `DAT_1F800393`, so the deadline is cadence-invariant. |
| `0x80076C10` | **Screen-element placement table** - 103 initialised records of `0x18` bytes, running to `0x800775B8`. Three subsystems index it and each named it after itself; they are one table. [Details ↓](#0x80076c10---one-table-three-names) |
| `0x801F6950` | u32 - **battle-action overlay PRNG state** (`FUN_801D0290`). Overlay-resident, so it is not the SCUS `rand()` seed and its draws do not perturb that stream. |
| `0x801F69D8` | u32 + code - **slot-B overlay window / cast-module link base**. Three tenants share the address: PROT 0900's jump-table head, the world-map band's `FUN_801F69D8`, and - while a capture-class cast runs - the paged per-spell module, whose **word-0 entry VA** this word then holds (`0x001000E2` when empty). See [cast-module.md](../subsystems/cast-module.md) and [overlay-va-aliases.md](overlay-va-aliases.md). |
| `0x801D9184` / `0x801D918C` | Two tracked 2-D points in the **fishing** overlay (`i16` at `+0` = x, `+4` = y); `FUN_801D765C` returns their separation in 64-unit sub-cells, `FUN_80019B28` the bearing between them. |
| `0x801E46B0` | i32 - menu-overlay **selected item id** for the window-34 description box (`FUN_801D4A80`); `<= 0` draws nothing. |
| `0x801E46D0` | u32 - menu-overlay packed **toggle state word** for window 46 (`FUN_801D603C`); bits `0x4000` / `0x2000` / `0x1000` and the low 12 bits select each row's marker kind. |
| `0x1F800314 +0x6A` | u16 - scratchpad draw-context **depth clip bound** used by `FUN_801D5C2C`. |
| `0x1F800314 +0x74`..`+0x7A` | u16 x4 - scratchpad draw-context **2-D clip rect** used by `FUN_801D56E4`: `+0x74` x-min, `+0x76` y-min, `+0x78` x-max, `+0x7A` y-max. |
| `0x801D91DC` / `0x801D91E4` | Fishing overlay **reel-cadence ring**: write index, then 16 `(button, duration)` records of two words. `FUN_801D746C` clears both; `FUN_801D3DB4` walks them. Not a catch log. |
| `0x1F80037E` | u16 - scratchpad **near cutoff**; `FUN_801D5C2C` rejects a segment whose two transformed Z both fall inside it. |
| `0x8007329C` | The two words between the TMD descriptor table's last row and the widget class table `DAT_800732A4`: pointers `0x8007B41C` and `0x8007B418`. Role unidentified; recorded so they are not mistaken for the head of the class table. Everything below them back to `0x8007326C` is that descriptor table's own six rows, not a separate block. |
| `0x8007C018` | TMD pointer table (`idx * 4` stride). Sole writer is `FUN_80026B4C`. All populated entries (`[0..DAT_8007BB38]`) are post-fixup Legaia TMDs. |
| `0x8007C348` | u32 - Free-list LIFO stack pointer for the actor allocator. |
| `0x8007C34C..0x36C` | u32[7] - Actor-list slot table consumed by `FUN_8002519c`. Seven linked-list heads at strides of 4 bytes (`+0x00`/`+0x04`/`+0x08`/`+0x0C`/`+0x10`/`+0x14`/`+0x20`). `FUN_80016444` walks five of them per frame as separate render passes; per-node entry-point is `node[+0x0C]` invoked via `jalr`. `_DAT_8007C354` and `_DAT_8007C364` are also read by `func_0x8003C83C` for the `0xF8`/`0xFB` motion-VM channel lookups (same list, two consumers). |
| `0x8007C364` | u32 - Player context pointer (`_DAT_8007C364`). Corpus-stable at `0x80083794` across the field/battle Tetsu captures. `+0x10` carries the `0x80000` "encounter active" flag the entity SM raises during install and clears at the battle handoff. |
| `0x8007BD0C` | u8[4] - **Formation cell** - the per-slot monster id, indexed by 0-based monster index (monster `m` is battle-actor slot `m + 3`, `0x801EA008`). Filled from the encounter record by `FUN_801DA51C`, or from a scripted battle id by `FUN_8005567C`. The AI picker `FUN_801E9FD4` switches on it (`0x801EB73C`), so each case is bespoke AI for one monster id. `see ghidra/scripts/funcs/overlay_battle_action_801e9fd4.txt`. |
| `0x8007BD10` | u8[] - **Per-seat character id**, 1-based (`1` Vahn, `2` Noa, `3` Gala, `0` empty), indexed by battle-actor slot. Selects the character record, the battle-mesh install slot (`FUN_800513F0`), the element row, the caption label and the attack camera's per-character jump table (`FUN_801D71B8` at `0x801D7290`). |
| `0x8007BD24` | u32 - Battle context pointer (`_DAT_8007BD24`). `0x800EB654` while a battle is resident, `0` in the field. Base of the battle-actor / AI ctx block (`+0x07` = the action-state cursor the whole battle keys on, `+0x13` = active slot, `+0x276` = tutorial byte gating arts-voice XA cues, `+0x279` = module phase byte while a capture-class [cast module](../subsystems/cast-module.md) is paged, `+0x28A` = battle-mode counter; the pending-SFX write counter lives at `+0x9`). |
| `0x8007BD74` | u32 - Battle **side-band streaming buffer** pointer (`_DAT_8007BD74`, `0x800B990C` in the battle captures): one `0x10800`-byte slot of `summon.dat` / `readef.DAT` at a time, filled by `FUN_801F17F8`; `FUN_8002B28C` decodes art `"ME"` keyframe streams out of whichever slot is resident at commit ([`battle-data-pack.md`](../formats/battle-data-pack.md#me-stream-archives-readefdat)). |
| `0x8007B6D8` | u16[4] - Pending-SFX cue ring (counter = battle ctx `+0x9`, wraps past 3). Written by the cue router `FUN_8004FE5C` (and directly by minigame overlays), drained by `FUN_80016B6C` against the SFX descriptor table. Engine mirror `legaia_engine_core::sfx_cue`. |
| `0x8007B724` | u32 - Last-played SFX id - the cue router's dedupe compare. |
| `0x80070536` | i16 - Bound voice id of the field-BGM sound source (`0x8007052C + 0xA`; the id `FUN_80026478` resumes). Runtime-written at track attach (`0` in the static image); read by `FUN_800267A8` and the battle-intro phase 0 (`FUN_801CF5BC`) as the `SsSeqSetVol` slot argument. |
| `0x800788B8` | u16[N] - Per-arts-voice XA clip duration table (index = cue id − `0x100`); `FUN_8004FE5C` converts to sectors as `(raw*60 + 99)/100`. |
| `0x8007326C` | u32 - TMD per-mode descriptor table (8-byte stride × 6 entries). |
| `0x8007A940` | SsAPI per-note pitch / per-voice volume exponential lookup table (read by `FUN_80066E50` / `FUN_80067550`). |
| `0x801CD2B8` | SsAPI 16-bit slot-allocation bitmap. Bit `i` = sequencer slot `i` allocated. |
| `0x801CD2C0` | SsAPI 16-entry per-slot pointer table. Each entry → `0xB0`-byte sequence-state struct. |
| `0x801CB408` | libcd directory-entry cache, up to 128 entries of stride `0x2C`, populated by `FUN_8005DEA0` (`lui at,0x801d` + `sw ...,-0x4bf8(at)`; the cap is the two `slti a3,0x80` tests at `0x8005E0E0` / `0x8005E100`). The long-standing `0x801C4BEC` was the *offset* half of that pair pasted into an address's high half - the routine forms no address in `0x801C4***`. |
| `0x8007B318..1B` | The four **trigger-block per-kind record strides** `gp[0..3]` = `4, 4, 4, 8`, read by the sub-table walker `FUN_801D5AE0`. Kind 2 is the height-override band; its one runtime writer is field-VM op `0x4C` sub `0x83` at `0x801E20A8`. |
| `0x80074358` | Global 4×u32 ability bitmask. Written by `FUN_80042558` (OR-aggregate); read by `FUN_800431D0` (bit-test). |
| `0x80085758` | "Fourth flag bank" bitfield. Wired to field-VM ops `0x50` / `0x60` / `0x70` (and the move VM) via `FUN_8003CE08` / `_CE34` / `_CE64`. (Formerly mis-listed at `0x80086D70` - a double-count of the `0x1618` save offset.) |

## World-map render pipeline

Globals read or written by the per-frame world-map POLY_FT4 batch
chain. End-to-end walkthrough in [`subsystems/world-map.md`](../subsystems/world-map.md#render-pipeline).

| Address | Type | Purpose |
|---|---|---|
| `0x8007BC3C` | u32 | World-map submode register. `FUN_80016444` gates its `jal 0x801D7EA0` on this being `2`. Six SCUS writers (`FUN_80016230` / `FUN_80025980` / `FUN_80025DA0` / `FUN_8001D424`). |
| `0x8007BCD0..D8` | u32[3] | Source globals for the gate-arm scale / step / OT-layer params. `FUN_801D1344` reads these and forwards as args to `FUN_801D8258`. Field-VM op `0x4C` nibble-4 sub-9 dispatches the arm table `0x801E1480..0x801E162C` that writes them; subs `0xA` / `0xB` / `0xC` store the horizon gate in the **delay slot** of the `j` leaving the arm (`0x801E1648` / `0x801E1688` / `0x801E16C8`), and sub `0xD` scales `_DAT_8008457C >> 12` into `0x8007B910`. |
| `0x801F351C` | u32 | One-shot gate flag for the world-map POLY_FT4 batch emitter. `FUN_801D8258` sets it to `1`; `FUN_801D7EA0` (and 0897 sibling `FUN_801C9688`) clear it after one emission. Lives in the persistent `0x801F0000+` region so survives overlay swaps. |
| `0x801F3518` | u32 | Running camera angle for the cos-rotation POLY_FT4 batch. Advanced by `DAT_1F800393 * _DAT_801F3524` per `FUN_801D7EA0` call; masked to the 4096-entry trig LUT reached through `0x8007B81C` (the sine base). |
| `0x801F3520` | u32 | Render scale / range. Sourced from `_DAT_8007BCD4` via `FUN_801D8258`'s `param_2`. Used both as `local_3c` and `local_3c / 5`. |
| `0x801F3524` | u32 | Angle step per frame tick. Sourced from `_DAT_8007BCD8` via `FUN_801D8258`'s `param_3`. |
| `0x801F3528` | u32 | OT layer / draw priority. Sourced from `_DAT_8007BCDC` via `FUN_801D8258`'s `param_4`. |
| `0x80078DFC..0x80078E0F` | u32[5] | Statically-linked libgpu `MoveImage` packet template: `[tag 0x04FFFFFF][GP0 0x80000000][src yx][dst yx][wh]`. `FUN_80058490` (MoveImage) patches src/dst/wh in place per call, then submits through the driver vtable at `*(0x80078D4C)+8`. The frame-clear fill template (`x=0, y=4`, 320×224) sits just above at `0x80078DC0`. |
| `0x801F291C+` | records | Field-overlay effect-descriptor records `[0xFFFF0000][handler ptr][4 param words]` (persistent `0x801F0000+` region). Slot `0x801F2920` holds the CLUT cross-fade SM `FUN_801E4794` (the world-map palette-cycling writer); sibling slots point at `FUN_801E4D8C` / `FUN_801E5154` / `FUN_801E5338`. |

## World-map TMD and actor tables

The global asset-pointer table consumed by the world-map top-view
renderer (`FUN_801F69D8` → `FUN_80043390`). Live verification: see the
field-scene snapshot example in
[`formats/world-map-overlay.md`](../formats/world-map-overlay.md#consumer-call-sites).
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
| `0x8007B888` | `void *` | Type-`0x05` (MOVE) animation-clip bank pointer (set by `FUN_8001F05C` case 5 at `0x8001F3A8`; **not** installed into `DAT_8007C018`). On the world map it is kingdom slot 4 - the scene's ANM bank, read by the clip selector `FUN_800204F8` (1-based lookup) and the animated renderer `FUN_8001B964`; live `map01` pins it at `0x8011A624`. Six references image-wide, none in a render-dispatch path. |
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
[`re-settled-threads.md`](re-settled-threads.md#_dat_8007b98f-is-byte-3-of-the-debug-mode-word-_dat_8007b98c).
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
the debug-menu enable. A from-scratch engine should treat both as build-time
constants and keep its flag-bank writes bounded.

## PSX scratchpad (`0x1F800000-0x1F8003FF`)

The PSX has 1 KB of fast scratchpad RAM mapped here. Legaia uses the high end:

| Address | Type | Purpose |
|---|---|---|
| `0x1F800314` | block base | Base of the scratchpad block every cell below is a displacement off - the `lui 0x1F80; ori 0x314` pair, then `lb`/`lh`/`lw` at `+0x48`, `+0x8C`, `+0xD4`, ... . An absolute-address scan never sees any of them. |
| `0x1F80035C` | i16[16] | **Scene floor-elevation ladder** (`= 0x1F800314 + 0x48`) - the sixteen heights a collision byte's low nibble indexes. Port `World::field_floor_height_lut`. Not an "inverted-Y mirror table", and not at `0x1F800314` - [details ↓](#0x1f80035c---the-floor-elevation-ladder). |
| `0x1F800393` | u8 | Per-frame tick byte. Global frame-time scalar. Read by op 0x4A `WAIT_FRAMES` and the 0xFFFF sentinel in op 0x4C nibble-C sub-B/C. Also subtracted from the title-attract countdown at `0x801EF16C` every tick (see [`subsystems/boot.md`](../subsystems/boot.md#tick-function)) and exposed via `World::tick_move_vms_with_delta` in the engine port. |
| `0x1F800394` | u32 | **Field-VM transient flag word** (32-bit; **not** persisted - distinct from the saved story-flag bitmap at `0x80085600..0x80085800`, ops `FUN_8003CE08/CE34/CE64`). Script-VM bits are set/clear/tested by `GFLAG_SET` / `GFLAG_CLR` / `GFLAG_TST` (ops 0x2E / 0x2F / 0x30, `1 << (idx & 0x1f)` at `FUN_801DE840:5280/5284`); also gates op 0x4C nibble-4 sub-9's tristate dispatch via bits `0x01000000` / `0x02000000`. The **lower 16 bits** are re-seeded on every mode switch from `mode_table[_DAT_8007B83C].param` (`+0x14`) by `FUN_8001DCF8 @ 0x8001E17C` - its sole non-RMW writer (see [`save-screen.md`](../subsystems/save-screen.md)). Bit 0x40 is set by the scene-change packet `FUN_8001FD44` (a scene-transition-pending flag, **not** a "dialog active" lock - an earlier mislabel). |
| `0x1F8003A0` | ptr | **Active primitive/packet write cursor** (`[0x1F800314]+0x8C`). The `POLY_*` emitters (`FUN_8003C43C` G4, `FUN_8003C510` G3, `FUN_8002BDC4` gradient tile, and the TMD per-primitive emitter `FUN_80027C6C`) allocate their packet here, post-increment by the packet size, then link it through `FUN_8003D2C4`. |
| `0x1F80037C` | u8 | Current walk-region kind (SCUS `FUN_800180EC`); `0` disables the camera scroll clamp. |
| `0x1F800384..87` | 4 × u8 | Current walk-region AABB in tiles, store order `rec[0], [3], [2], [1]`; default `0, 0, 0x7F, 0x7F`. |
| `0x1F8003E8..EB` | 4 × i8 | Camera **visible tile window** `[nearX, nearZ, farX, farZ]`, signed tile offsets from the camera tile. Written by `FUN_801DBC20` and field-VM op `0x46`; read by the render library's cell emitters, `FUN_801DAA50`, `FUN_801D6058` and dev-menu rows `0x12..0x15`. Formed as `lui 0x1F80; ori 0x314; lb 0xD4(rX)`, so the word scan never sees it. |
| `0x801F2778 / 7C / 80 / 84` | 4 × i32 | Write-only mirrors of the window above; no reader on the disc. |
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
| `0x801F3600+` | PROT 0895 (`init.pak`, mode 16) own globals | `0x801F369C` six-record logo sprite-descriptor table; `0x801F3714` / `0x801F3764` effect-part tables; `0x801F3978..0x801F39A4` memory-card driver state; `0x801F3EA8` fade level; `0x801F3EB0` boot flags. All above the image's code, a cross-check that the slot-A base is right. |
| `0x801F6990` | Battle overlay: the arts queue-builder's 16-word per-token side array | `1` = an accepted art's starter (build loop), `4` = a Super tail-replace starter; read by the marked-starter reorder pass `0x801EF8A0..0x801EF968`. |
| `0x801C6220` | Menu overlay (PROT 0899): shop buy-list **staging array** | The buy-row builder splits the walked rows at `record_count - 3`; rows below the split stage here tagged `0x3000` and are appended after the rows at or above it (tagged `0xA000`, ink 5 - the highlighted "new in this town" strip). `0x80030F1C..0x80030F90`; see [`shop.md`](../subsystems/shop.md#the-last-rows-come-first). |
| `0x801CEC40` | Field overlay (PROT 0897): `u32[8]` screen-frame **corner-writer table** | The eight `jr` targets `801DDA90` / `AB8` / `AE0` / `B00` / `B20` / `B4C` / `B78` / `BA0` that the per-actor frame emitter `FUN_801DD9D4` dispatches through, one per iteration; each writes four `POLY_F4` corner halfword pairs at `a1[+0x08..+0x16]` and `j`s the shared tail `0x801DDBC8`. These words are the family's only references. |
| `0x801EF120` | Title overlay: `state[-0xEE0]` **pre-roll / fade ramp** | Counted **down** by `0x80 * frame_scalar` by `AttractIdle` (`0x10`) while the screen is still animating in, and counted **up** to `0x1000` by sub-mode `0x18` before it hands to `0x14`. |
| `0x801F0204` | Title overlay: `state[+0x204]` **sub-mode selector** | Drives `FUN_801DD35C`'s jump table; 56 stores across the graph. `0x801DD920` is the *instruction* address of the `sw v0,0x204(a2)` that writes it, not the word - see [`boot.md`](../subsystems/boot.md#a-cold-boot-always-shows-sub-mode-0x10-never-0x02). |
| `0x801F0208` | Title overlay: `state[+0x208]` **previous sub-mode** | The sub-mode the previous frame ran; the tick preamble compares it against `+0x204` to log a mode change. |
| `0x801F35C0` | Field overlay: tile-board **cell buffer pointer** | `DAT_801F35C0 = FUN_80017888(0, width * height)` at `0x801EF3E8..0x801EF3F4` - exactly one byte per cell, nothing pads it, and nothing frees it. See [`tile-board.md`](../subsystems/tile-board.md). |
| `0x801F271C` | Field overlay (PROT 0897): the one plain **actor template** MAIN INIT spawns | 24-byte descriptor whose `+0x08` handler is `FUN_801D6058`. Materialised once, by `addiu a1,0x271c` at `0x801D6FC0` -> `jal 0x80024c88` at `0x801D6FD8`, with `sh s0,0x1a(v0)` seeding the `+0x1A = 1` scene arm; gated on `_DAT_8007B8B8 == 0`. It is not a cutscene element. |
| `0x801D5C08` / `0x801D5D60` | Field overlay (PROT 0897): two descriptors **nothing references** | Both have the shape of actor templates, and a sweep of all five reference forms (word, `lui` pair, `jal`, `j`, PC-relative branch) across 84 images reaches neither. Either a retained list seats them through a `jalr`, or they are dead authored data - [open thread](open-rev-eng-threads.md#field--locomotion). |
| `0x801F9D28` / `0x801F9D2C` | Slot-B module PROT 0955 (`cast_white_shield`): the two words its tick bodies share | Module-resident (`base + 0x3350` / `+0x3354` of a `0x3800`-byte image), read and written by 48 sites inside 0955 alone. `0x801F9D28` is the per-arm **frame countdown**, drawn down by the `scratch[0x37D] * scratch[0x393]` product; `0x801F9D2C` is the accessory **slot index** the Void Accessories arm rolls. The countdown's frame values are the last unmeasured leg of the twelve tick bodies - [open thread](open-rev-eng-threads.md#battle--rendering). |
| `0x801C8FE4` | Battle overlay: the **element-cycling counter** | The last accepted `rand() % 3`; PROT 0964's element-change tick (`0x801F88EC`) redraws until the roll differs from it, then writes the enemy record's `+0x1D` element and its `+0x1C` ME group from the in-image table `0x801F9B70`. It is the only routine on the disc that mutates a live enemy record's element. |
| `0x801F696C` | Battle overlay: "**a Super fired this build**" latch | Set to `1` by the Super trigger arm (`0x801EF5A8`/`0x801EF5B4`) and by the tail-replace `FUN_801EF9E4` (`0x801EFBD4`); what makes Miracle-before-Super ordering observable. See [`battle-action.md`](../subsystems/battle-action.md). |

## Mini-game state regions

Each mini-game gets its own ~64 KB slab of upper RAM, loaded fresh when entered. See [`reference/builds.md`](builds.md) for the per-mini-game RAM addresses.

## Global / cell details

Full write-ups for the rows above whose detail outgrew a table cell. Linked from the section tables by **[details ↓]**.

### `0x1F80035C` - the floor-elevation ladder

Sixteen signed heights, indexed by the low nibble of a collision byte. Three
things install a ladder and three read one, and mixing them up is what produced
the "inverted-Y mirror table" reading.

**Writers.** Scene entry: `FUN_8003AEB0` copies the MAN header's sixteen
**negated** `short`s from `_DAT_8007B898 + 2`. Script: field-VM op `0x4C`
nibble-9 sub-`0xE` rewrites the whole ladder, subs `0..2` set one rung
oscillating through `FUN_801DDE34` -> `FUN_801DA930`, and sub-`0xF` retires the
oscillators. The fishing bring-up `FUN_801CF070` seeds its own sixteen rungs
descending `-0x20 * n` from `0x1F80037A`, so a fishing venue does not inherit
the field scene's ladder.

**Readers.** The object/actor spawn iterator `FUN_8003A55C`, the bilinear
ground sampler `FUN_80019278` (which is what makes it a *height* table - its
other two inputs are the actor's X and Z), and the bulk terrain-cell emitter
`FUN_801F89B8` at `0x801F8A7C`.

The nibble-9 arm's own write-up is in
[`script-vm-menuctrl.md`](../subsystems/script-vm-menuctrl.md#nibble-9-is-the-floor-height-ladder-not-a-fade).

### `0x8007B83C` - the front-end mode chain

Six stores carry a cold boot from the logo to the field, and each one lives in
the handler that hands off - there is no table field naming the next mode. That
is why a trace which samples the mode word once, early, reads the title screen
as running under `0x10`.

| Store site | Writes | Meaning |
|---|---|---|
| `0x8001D5B8` | `0x10` | READ INIT - one frame of logo initialisation |
| `0x801CEC94` | `0x11` | the `init.pak` hand-off, in a `jal` delay slot |
| `0x801CF4D4` | `0x16` | (the alternate arm at `0x801CF4E4` is the dev CONFIG route on the entry word) |
| `0x80025974` | `0x17` | **CARD** - the mode the title screen actually runs under (`li v0,0x17` at `0x8002596C`) |
| `0x801DFC00` | `0x02` | MAIN INIT, from the menu overlay PROT 0899's NEW GAME arm |
| `0x80025E50` | `0x03` | MAIN - field per-frame |

See [`boot.md`](../subsystems/boot.md).

### `0x80085958` - Item inventory

Mechanism-first treatment (the window, the helper family, the design intent, the folklore): [`inventory.md`](../subsystems/inventory.md).

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

### `0x80076C10` - one table, three names

Three subsystems index this array, and each was documented after the consumer
that found it: a battle **pose-slot array**, the party-panel **publish
target**, and the Muscle Dome's **element layout table** / "battle HUD block
base". All three cite the same base and the same `0x18` stride, so there was
never a disagreement about the bytes - only three names for one table, none of
which said what a record is.

A record is a **screen-element placement**, and the disassembly settles it. The
"pose-slot re-mapped copy" `FUN_801D5778` clones a record and writes
`dst[+0xA] = src[+0xA] - 0x140`. `0x140` is 320 - the PSX display width - so
the field it shifts is a **screen X**, and the operation is "place this element
one screen to the left". Poses do not get offset by a display width; that is a
slide-in / off-screen staging idiom. The initialised data agrees: `+0x08` reads
`0x0C` in every record (a 12-pixel line height), `+0x0A`/`+0x0C` carry a second
x/y that goes negative (e.g. `-44`) where `+0x02`/`+0x04` do not, and `+0x14`
is either null or a pointer into the `0x8007B6xx` gp-pool band.

| Offset | Field |
|---|---|
| `+0x00` / `+0x01` | element id, seat A / seat B (usually equal; records 41 / 42 carry them byte-swapped) |
| `+0x02` / `+0x04` | seat A x / y |
| `+0x06` / `+0x08` | content width / box height (shared by both seats) |
| `+0x0A` / `+0x0C` | seat B x / y - the pair `FUN_801D5778` pushes by a screen width. "From / to" is the safe name: the plaque, chip and panel families park at B, record 42 parks at A |
| `+0x0E` / `+0x0F` | kind, seat A / seat B |
| `+0x10` | `13` on the framed-window and roster rows (kinds `0x03` / `0x07` / `0x44`), `0` elsewhere; no spawn arm reads it |
| `+0x12` | zero in all 103 records |
| `+0x14` | content **string** pointer - measured by the rendered-width kernel `FUN_80035F04` into `+0x06`; the battle name plaque points it at the acting actor's display-name buffer `actor+0x1BC` (not an animation descriptor) |

The record is a **two-seat pair**: `FUN_801D8DE8` has one spawn arm per seat
(`0x801D92E8` for A, `0x801D935C` for B, chosen by `mode & 1`) and arms the
glide `FUN_801DB7B0` toward the seat it did not spawn from. Three movers copy
records around the table: `FUN_801D5718` (land: seat B into both seats),
`FUN_801D5778` (launch: seat B pushed `-0x140`), `FUN_801D57E8` (clone).

The array holds **103 initialised records**, `0x80076C10..0x800775B8`; parser
`legaia_asset::screen_elements`, disc-gated oracle
`crates/asset/tests/screen_elements_real.rs`. Records 41/42 (`+0x3D8` /
`+0x3F0`) are the pair `FUN_801D5854` shifts inline (42 inherits 41's string,
width and seat-B x; 41 takes `actor+0x1BC`, is measured, and re-seated at
centre x 232 with a right clamp at 304 and a park at 328 or beyond).

Two earlier readings of the extent are corrected by the disc bytes. The run is
**not** 200 records to `0x80077ED0`: index 129 is `0x80077828`, the per-monster
[steal table](../formats/steal-table.md), so 129 is a hard ceiling, and the
placement shape itself stops at 103 - every record below keeps all four
coordinates within `+/-416` while record 105 already carries `-1000`. And
`+0x08` is **not** `0x0C` throughout: `0x0C` is the line height of the
plate-run family (name plaque, party status bar, command chips), while the
roster panels carry `50` and the framed windows `120` / `42` / `28` / `26`.

A record is a **content box**, and the chrome around it is derived rather than
stored - `pen = (x, y - 2)` and `plate = (x - 8, y - 6)` sized `(w + 16, 20)`.
[`battle.md`](../subsystems/battle.md#one-placement-record-derives-every-plate)
has the packet evidence and the per-surface table.

Prefer "screen-element placement table" when naming it. "Pose-slot array" is
the narrowest of the three names and the only one the record layout
contradicts.

## See also

- [`docs/reference/functions.md`](functions.md) - the functions that read and write these globals.
- [`docs/subsystems/boot.md`](../subsystems/boot.md) - how the PROT TOC and key globals get installed at `0x801C70F0`.
- [`docs/tooling/ghidra.md`](../tooling/ghidra.md) - the LUI+ADDIU writer hunt that pins these addresses.
