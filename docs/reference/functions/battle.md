# Key functions: battle

Part of the [key function directory](../functions.md) - the conventions for reading these tables (bare hex = function entry, `0x`-prefixed = data / instruction, overlay-VA caveats) are on the [index page](../functions.md#how-to-use-this-page).

## Battle subsystem

| Address | Role |
|---|---|
| `801D0290` | **Overlay-local PRNG** (battle-action overlay 0898). Twelve instructions, no frame; the whole state is the word at `0x801F6950`. `v = s*12 + 2` (built as `s<<2` + `s<<3`), then `s = (v << 16) + (v >> 16)`, which **is** exactly `rotate_left(16)` - see [the PRNG section](../../subsystems/battle-action.md#overlay-local-prng-fun_801d0290) for why the `addu` cannot carry. Distinct from the SCUS BIOS `rand` thunk `FUN_80056798`, so its draws do not perturb that stream. `overlay_0897` dumps a *different* body at this VA; read the 0898 image. Ported as `engine-vm::battle_action::OverlayRng`. |
| `801EC0DC` | **Monster escape roll** (battle overlay 0898): `(slot) -> bool`, "does this monster break off and flee?" The enemy-side mirror of the party roll `FUN_801E791C` - [details ↓](#801ec0dc). Ported as `engine-vm::battle_formulas::monster_escape_roll`. |
| `801DF570` | **Attack-approach distance clamp** (0898): `(slot, requested) -> i16`. Projects the attacker/target separation along the reverse bearing and clamps the requested step into `[3d/4, d]` - [details ↓](#801df570). Ported as `engine-vm::battle_approach`. |
| `801DBB8C` / `801D84C0` | **Battle party-name panel open / teardown** (0898). A matched pair over the label-actor block at `0x801F4E08`: the former registers a text actor through `FUN_8003541C` and stashes the handle, the latter builds the four panel buffers and clears that block - [details ↓](#801d84c0). Ported as `engine-vm::battle_party_panel`. |
| `801DBC30` | **Cross-out mark blit** (0898): `(x, y)`. One 1:1 `0x40 x 0x10` textured quad over `(x-8, y-4)..(x+0x37, y+0xB)`, gated on `ctx[+0x6CE] == 0`. `tpage 7` + CLUT `0x7704` resolve to the `etim` effect page `(448, 0)` through CLUT `(64, 476)`, texels `(0, 96)`-`(63, 111)` - the **red cross-out X**, not chrome; see [`battle.md`](../../subsystems/battle.md#battle-screen-chrome-packet-pinned) for what the chrome really is. Ported as `engine-vm::battle_party_panel::cross_out_mark`. |
| `801F30C4` | **Two-mode effect burst** (0898, 563 instructions spanning `0x801F30C4..0x801F398C`): `(record, mode)`. Four iterations around the compass, three spawns each, placed by a trig term plus bounded random jitter. The entry is **one loop written twice** - [details ↓](#801f30c4). Ported as `engine-vm::battle_burst`. |
| `801D5778` | **Screen-element placement re-mapped copy** (battle-action overlay 0898). `(dst_index, src_index)` - both **indices**, scaled `*0x18` into the placement table at `0x80076C10`. Copies `dst[+2] = src[+0xA]`, `dst[+4] = src[+0xC]`, `dst[+6] = src[+6]`, `dst[+0xA] = src[+0xA] - 0x140`, `dst[+0xC] = src[+0xC]`, `dst[+0x14] = src[+0x14]`. `overlay_battle_action_801d5778.txt`. |
| `801D57E8` | **Screen-element placement straight copy** (battle-action overlay 0898). `(dst_index, src_index)` over the same `0x80076C10` table and stride; clones `+0x02`, `+0x04`, `+0x06`, `+0x0A`, `+0x0C` (u16) and `+0x14` (u32), and deliberately leaves `+0x00`, `+0x08`, `+0x10`, `+0x12` alone. The un-remapped sibling of `801D5778`. `overlay_battle_action_801d57e8.txt`. |
| `80052FA0` / `800536BC` / `80053898` / `80053a28` | Party battle-mesh assembler (equipment-section splice) + CLUT decode + TSB/CBA relocation - [details ↓](#80052fa0) |
| `80052770` / `800558fc` / `8003e8a8` | Player-file loader (Vahn/Noa/Gala/Terra battle records) - [details ↓](#80052770--800558fc--8003e8a8) |
| `800520F0` | Battle scene loader (SCUS) - [details ↓](#800520f0) |
| `800513F0` / `800542C8` | Battle-form party-mesh install - [details ↓](#800513f0--800542c8) |
| `80020050` | Flame / effect-texture atlas loader (SCUS) - [details ↓](#80020050) |
| `0x801C9370` (data) | **Battle actor pointer table** - 8 entries × 4 bytes. Slots 0..2 = party, 3..7 = monsters. Resolved by `FUN_8004E2F0` and `FUN_80054CB0`. |
| `0x80074358..0x80074368` (data) | Global 4×u32 "active abilities" bitmask. `FUN_80042558` ORs each party member's `+0xF4..0x100` block into it every frame. The bit ids are the accessory passive-effect indices (descriptor `+3` byte); see [`formats/accessory-passive-table.md`](../../formats/accessory-passive-table.md). |
| `800431D0` | Global ability bit-test: `(bit_id) -> bool`. The read-side primitive for the bitmask above - `(&DAT_80074358)[bit_id >> 5] & (1 << (bit_id & 0x1F))`. Cited heavily across battle code. |
| `800349EC` | HP / threshold UI classifier: `(char_idx) -> color_idx`. Reads `[char_base + 0x0E]` (current HP) and `[char_base + 0x0C]` (max HP), returns `2`/`6`/`7`/`9` keyed on dead / quarter / half / healthy thresholds. Drives dialog HP-color tinting. |
| `80035EA8` | MP-side variant of `FUN_800349EC`. Reads `[char_base + 0x10]` / `[char_base + 0x12]`. |
| `8003FB10` | Per-slot target-validity walker (action validator). 18-arm jump table on the arm byte; tests per-slot HP/MP quads (battle-actor table / char records per mode), stats, party indirection, system flags, and the `FUN_80046898` inventory leaf, writing per-slot validity bits. Does **not** call `FUN_800431D0`. Ported: `engine-vm::battle_action::validate_action`. |
| `0x80084708` (data) | Character record table base. Stride `0x414` per character. See [`subsystems/battle.md`](../../subsystems/battle.md) → "Character record layout". |
| `80042558` | **Per-frame stat aggregator + accessory-passive assembler.** Walks the 3 party members' equip ids `char +0x196..0x19D`: each item's passive index (`kind==1`→equip `+5`, `kind==2`→descriptor `+3`; `<0x40`) sets a bit in the ability bitfield `+0xF4..0x103`; boosts rebuild the effective-stat block `+0x104..0x11B` from base `+0x11C..0x12D`, bitfields OR into the global mask; a separate arm grants Talisman + Ra-Seru spells (`+0x13D` list). **All fields are in the character record `+0xF4..+0x13D`, NOT the battle-actor runtime struct** (`+0x14C`/`+0x150`/`+0x176` are in the `DAT_801C9370` pool). Full field map + correction: [battle-action § aggregator](../../subsystems/battle-action.md#fun_80042558---per-frame-stat-aggregator). `see ghidra/scripts/funcs/80042558.txt`. |
| `80034250` | Goods description resolver (static): item id → descriptor `+3` passive index → `0x8007625C` record `+8` description pointer; the menu overlay's detail panel `FUN_801D0F1C` reads the same table's `+4` name pointer. |
| `8004CE2C` | **Per-frame battle actor maintenance pass** (3 KB, 757 instructions). Four passes over the `DAT_801C9370` actor table; the last is a **CLUT status recolour** latched once per affliction, not a per-frame damage flash. Not a mode dispatcher. Pass-by-pass walk: [`subsystems/battle.md` § Per-frame actor maintenance](../../subsystems/battle.md#per-frame-actor-maintenance-fun_8004ce2c). `0x8004CE30` is the **second instruction** (`addiu sp,sp,-0x38`), not an entry. `see ghidra/scripts/funcs/8004ce2c.txt`. |
| `8004DA00` | **Battle XA voice-stream selector** - the `+0x08` tick of the [static actor template](runtime-libs.md#static-actor-templates) at `0x800767F4`, which battle init `FUN_800513F0` spawns; it always ends by running the maintenance pass `FUN_8004CE2C` above. Arms one whole-clip stream per action, latched at `_DAT_8007BDB0` - [details ↓](#8004da00). Docs previously named this body `FUN_8004DA08`, which is its third instruction. `see ghidra/scripts/funcs/8004da00.txt`. |
| `8005126C` | **Battle actor on-screen test.** `(actor) -> bool`. Copies the `SVECTOR` at `+0x3C..+0x43` of the actor record the `&DAT_801C9370` table holds for `actor[+0x5A]` into `actor[+0x14..+0x1B]`, projects a square box of half-extent `actor[+0x58]` about it through the billboard projector `FUN_800195A8`, and reads back the **X of corner 0 and the X of corner 1** - two X coordinates, not an `(x, y)` pair; no Y is ever read. Rejects only when both are `>= 0x141` or both are `< 0`, so the test is "the box's horizontal span overlaps `[0, 0x140]`". Contrast the rectangle probe `FUN_8001B73C`: [`subsystems/renderer.md` § On-screen probes](../../subsystems/renderer.md#on-screen-probes-two-tests-that-are-not-the-same-test). `see ghidra/scripts/funcs/8005126c.txt`. |
| `80050D40` | **12-bit angle tween.** `(from, to, weight, slot)`. Wraps both angles into `0..0xFFF`, adds `0x1000` to whichever side makes the arc the short way round, accumulates the swept magnitude into `gp[0xA10]` (= `0x8007BD28`), records the adjusted endpoint pair as two halfwords at `0x801C9060 + (slot & 0xFF) * 4`, and returns `(to' + ((from' - to') * weight >> 4)) & 0xFFF`. A near-twin of the ANM angle interpolator `FUN_8001D088`, and **unreferenced** - see [Unreferenced SCUS entry points](#unreferenced-scus-entry-points). `see ghidra/scripts/funcs/80050d40.txt`. |
| `0x8007625C` (data) | Passive-effect name/description table: 64 × 12-byte `[u32 scope][u32 name_ptr][u32 desc_ptr]`, indexed by the passive-effect index. Scope `1` = party-wide. |
| `80043048` | **Inventory consume-by-slot:** `(slot: i16, amount, prev) -> remaining`. The stride-2 array at `_DAT_80085958` (= `0x80084140 + 0x1818` = SC `+0x1818`) is the **item inventory**: byte 0 = item id, byte 1 = stack count. Bounds-checked (`slot < gp[+0x2D4]`); subtracts `amount` from the count, clamps at 0, zeroes the id when the count reaches 0. (Previously mis-documented as a "status-effect timer decrementer" - the `0x80085958` table is the item bag the `Have 99 Items` / `Item Modifier` GameShark codes target, not a timer table, and its sibling helpers id-match + cap stacks at 99.) |
| `80042310` | **Inventory consume-by-id:** `(id, amount) -> slot`. Scans the active window `gp[+0x2D2]..gp[+0x2D4]` of `_DAT_80085958` for `id`, then decrements that slot's count (same clamp-at-0 / zero-id-at-0 as `FUN_80043048`). Bounds-checked. |
| `80042EE0` | **Inventory find-slot-by-id:** `(id) -> slot \| 0x100`. Scans `gp[+0x2D2]..gp[+0x2D4]` of `_DAT_80085958`, returns the first matching slot index or `0x100` (not found). |
| `80042F4C` | **Inventory find-count-by-id:** `(id) -> count`. Same scan as `FUN_80042EE0` but returns the matched slot's stack count byte (`_DAT_80085959`), or `0` when absent. |
| `800423E0` | **Inventory normalize (merge + squeeze):** calls `FUN_8004313C` to set the window, then walks `gp[+0x2D2]..gp[+0x2D4]` of `_DAT_80085958` merging duplicate-id slots - adds the stack counts, **caps the merged stack at 99** (`0x63`), and zeroes the absorbed slot. It then pulls occupied slots down into holes, keying occupancy on `id != 0` alone (a live id with a zero count survives). All loop bounds gated on `gp[+0x2D4]`. **Not** called from `FUN_80042310` - consume-by-id zeroes the emptied id in place and returns, so squeezing is always a separate hop. |
| `800421D4` | Inventory add (find-or-insert) - [details ↓](#800421d4) |
| `8004313C` | **Inventory window setup.** Selects the active inventory page into `gp[+0x2D2]` (start), `gp[+0x2D4]` (end), `gp[+0x2D6]` (count = end−start): the page flag at `0x80084594` plus a fourth-flag-bank TEST (`FUN_8003CE64(0x14)`) and the party-id byte at `0x80084598` choose start = `0` vs `0x80`, end = `0x100`. Called before each compaction pass; the start/end pair is the bound every inventory accessor checks against. |
| `8004E568` | Battle-end reward resolution - [details ↓](#8004e568) |
| `801E9504` | Level-up applier - [details ↓](#801e9504) |
| `80025358` | **Gated sub-overlay load sequencer** (`(void) -> u32`). Runs only while `_DAT_8007BC20 == 0`. Advances a 3-state counter `DAT_8007B6C8`: state 0 waits on the loader-ready poll `FUN_8003DE7C(1)`, then issues `overlay_loader_b(0x53, 0)` (`FUN_8003EC70`, extraction PROT 978 - the `"FIELD BACK READ"` / `"efect init"` slot-B blob) and bumps the counter; state 1 waits again and bumps; state 2 calls the loaded overlay's tick `func_0x801F6B24`. Returns `1` while still loading. Invoked from the battle-end reward routine `FUN_8004E568` to stage and then tick a sub-overlay. |
| `801F6B24` | **Slot-B "FIELD BACK READ" staged-loader tick** (PROT 0978 at the slot-B link base `0x801F69D8`, entry = file `+0x14C`; called by `FUN_80025358` state 2). A `DAT_8007B6C8`-indexed multi-phase streamer: phase 2 stages a `0xA000`-byte texture read into `_DAT_8007B728 + 0x28000` - dev path `h:\prot\field\other6\tim_int.tim` / `tim_int2.tim` (retail branch resolves PROT index `0x4C7 + sel` via `FUN_8003E8A8`), with the variant selected by a party-state compare (`u16 0x8008480E < u16 0x80084824 >> 1`); odd phases poll `FUN_8003DE7C(1)`. `see ghidra/scripts/funcs/overlay_0978_other_game_801f6b24.txt`. |
| `801CE844` | **Game-over overlay init** - [details ↓](#801ce844) |
| `80026018` | **Mode-24 minigame exit / return-warp handler** (`(void)`; called on exit by the slot-A minigame overlays; no battle-path caller in the dump corpus). **Restores the scene the `0x3E` warp left**: `memcpy(0x80084548, 0x8007BAE8, 8)` + `_DAT_80084540` from `0x8007BAC4`; commits session winnings into the casino-coin bank (`_DAT_800845A4 += _DAT_80084440`, cap `9999999`); latches `_DAT_8007B8B8 = 2`, `DAT_8007BD60 \|= 0x80`; sets mode 2 (field MAIN INIT reloads the restored scene; `0` when `_DAT_8007B8B8 == 0`). The old "victory/reward XP-bank commit beneath `FUN_8004E568`" reading is wrong (gold lands in `_DAT_8008459C` in `FUN_8004E568` itself). See [`subsystems/script-vm.md`](../../subsystems/script-vm.md#0x3e-warp-mode-24-minigame-door-warp); `funcs/80026018.txt`. |
| `8020E748` | **Per-slot item swap-back sync** (overlay 0897; alias `801C0F48` in overlay 0899 - byte-identical, relocated). `(char_idx) -> n_changed`. For each of 4 slots, compares the desired id `(&DAT_801E43E8)[i]` with the id stored at `record + char_idx*0x414 + 0x75E`; on mismatch it refunds the displaced old id to the bag via the add-item trio (`FUN_80042EE0` capacity -> `FUN_80043048` reserve -> `FUN_800421D4(old_id, 1)`) then writes the new id. Reclaims a replaced equip/consumable slot into inventory - not a fresh give. `see ghidra/scripts/funcs/overlay_0897_xxx_dat_8020e748.txt`. |
| `801E01F0` | **Typed equip-with-swap-back** (menu overlay 0899; the dump file keeps its historical `0896` capture label). `(item_id, char_idx, slot)`. Capacity-checks (`FUN_80042EE0`) and reserves (`FUN_80043048`), classifies the item by its record type bits `(type & 0x60) >> 5` to resolve the destination slot, writes the new id into `record + char_idx*0x414 + 0x75E`, refunds the displaced old id via `FUN_800421D4(old_id, 1)`, and plays the equip SFX `FUN_80035BD0(0x24)`. The single-slot parameterized form of the 4-slot swap-back sync `8020E748`. `see ghidra/scripts/funcs/overlay_0896_bat_back_dat_801e01f0.txt`. |
| `801F138C` | **Battle turn-resolution / next-actor select** (overlay 0897; alias `801FA38C` in overlay 0896 - byte-identical). Walks the battle-actor table (`0x801C9370`, `_DAT_8007BD24`), ages action gauges, picks the highest-`+0x16C` actor with a random tiebreak (`FUN_80056798`), runs the monster-AI picker `FUN_801E9FD4`, and commits via `FUN_801DB0F0/0F8/124`. On a resolved capture (`actor[+0x1DE]==1`) it pays the captured monster's item into the bag via `FUN_800421D4(actor[+0x1DF], 1)` and clears the flag. `see ghidra/scripts/funcs/overlay_0897_xxx_dat_801f138c.txt`. |
| `801C36B0` | **Shop / exchange buy-confirm** (overlay 0971). A pad-driven 0/1 cursor + prompt render; on confirm it sets a story-flag bit, adds the selected catalog record's item via `FUN_800421D4(rec+8, qty)` (id + price in a `0xC`-stride table `_DAT_801D90B8`, qty `_DAT_801D90B0`), and subtracts `price*qty` from the purse `_DAT_8008444C`. A priced, variable-quantity give (not a fixed/chest give). `see ghidra/scripts/funcs/overlay_0971_801c36b0.txt`. |
| `801D0F60` | **Minigame completion reward** (overlay `other_game` / PROT 0977 at slot-A base `0x801CE818`, file `+0x2748`; the historical `FUN_801C2748` citation was this function's printed address in a mis-based `0x801C0000` import - the true base is pinned by string anchors into 0977's own monster-name table). Restores the SC block (`FUN_8001A8B0`), toggles story flags, tallies the score into `_DAT_80084440` via the per-`(DAT_801d1a90, DAT_801d1a94)` score table `DAT_801d1860`, and - once, when the round counter `DAT_801d1a94 >= 0xD` and the flag-bank bit `FUN_8003CE64(0x6CB)` is clear - awards a single fixed item via `FUN_800421D4(0xCD, 1)`. `see ghidra/scripts/funcs/overlay_0977_slotA_801d0f60.txt`. |
| `800431FC` | Spell-list contains check: `(char_idx, spell_id) -> bool`. Walks `[char + 0x13d ..]` (count at `+0x13c`). |
| `80043264` | Equipment-slot contains check: `(char_idx, item_id) -> bool`. Walks `[char + 0x196 ..]` (8 slots). |
| `800432BC` | **Seru-magic equip:** `(char_idx, spell_id, src_slot)`. Reads the active-spell slot at `[char + 0x2B0 + src_slot*0x14]` - bytes `+1..+4` reassembled little-endian into the spell's u32 word, byte `+5` the level (**forced to 1 when zero**) - then inserts at the **head** of the spell list: shift ids `+0x13D`, levels `+0x161` and words `+0x8 + i*4` up by one, store the new entry at index 0, `count@+0x13C += 1`. Sister of `FUN_80042DBC`, which unequips. |
| `8004E2F0` | Battle range / line-of-sight: `(actor_a_id, actor_b_id) -> i16 distance`. Reads `[0x801C9370 + id*4]` for both, sums `+0x1F` size bytes, clamps per-tier. |
| `80054CB0` | Monster init: `(record, monster_slot)` populates `[0x801C9370 + (slot+3)*4]` from a monster record (HP/MP/stats), and builds the **hit-reaction tag map** at actor `+0x1EF..+0x1F3` (entry indices whose action tag is `2/3/4/5/0xB`, with a tag-4 → tag-2 fallback). Actor `+0x230` is **not** XP: `0x8005515C`/`0x80055164` are `lw v0,0x4(s4)` / `sw v0,0x230(v1)`, i.e. it takes record `+0x04`, the monster's battle-model / attack-effect pointer - see [`subsystems/battle.md`](../../subsystems/battle.md#monster-mesh-record-0x04). `see ghidra/scripts/funcs/80054cb0.txt`. |
| `80053CB8` | Party battle-actor init: `(file, slot)` copies the character name + seeds HP/MP/stats (with equipment bonuses via `DAT_80074F68`), zeroes `+0x1D9/+0x1DA`, and hardcodes the reaction map `+0x1EF..+0x1F3 = [2,3,4,5,0xB]` (the player files' identity entry layout). `80053cb8.txt`. |
| `8004AD80` | Battle **anim commit/transition**. Resolves `actor[+0x1DA]` to an action record (party `< 0x10`: `*(0x801C9360[slot] + id*4)`; monster: `+0x4C`-table; party `>= 0x10`: materializes from the record[0] `+0x58` art bank into dynamic slot `0x10`/`0x11`), installs `node+0x4C`, snaps `+0x1D9 = +0x1DA`, seeds loop-hold + sound cue, runs the end-of-clip chains (tag 4 → get-up / downed-party 7; tag 7 → 8). Also hosts the Evil-God-Icon steal roll, and calls the arts-voice cue `FUN_8004C140` when a party art fires. `8004ad80.txt`. |
| `8004C140` | **Arts-voice cue selector.** `(char_id, action_constant, flag)`. Fires the Tactical-Arts **shout** via `FUN_8003D53C(clip_slot = (char_id-1)*2+1, channel, dur)` - slot 1/3/5 = `XA2`/`XA4`/`XA6.XA` (Vahn/Noa/Gala). Picks a **random** channel (no immediate repeat, `gp+0xa4a`) from a per-action-constant candidate pool (range table `0x800781A4`; first/second-half candidate tables + `dur` at `0x80077A8C`; details in [battle-action.md](../../subsystems/battle-action.md)). Called from the anim materialiser `FUN_8004AD80`; capture-verified. Parser `legaia_art::arts_voice`. `8004c140.txt`. |
| `800402F4` | Battle **damage primitive** (multi-arm switch). Applies HP deltas and stages the target's **hit reaction** from the `+0x1EF` map: surviving target with no get-up entry → `+0x1DA = +0x1EF` (light flinch, exit-to-idle flag), else `+0x1DA = +0x1F1` (knockdown). Its HP-heal arms also carry the **menu-cast spell-XP accrual + level-up** (the pause-menu Magic screen's leveling arm - see [`field-menu.md`](../../subsystems/field-menu.md#menu-cast-spell-leveling--the-window-7-notice); engine `magic_xp::accrue_and_level` via `apply_spell_outcome`). Engine mirror `World::queue_battle_reaction`. `800402f4.txt`. |
| `8005567C` | **Battle-id → formation-cell expander** (SCUS). Reads the transient battle-id `DAT_8007b7fc` and writes the formation cell `0x8007BD0C..0F`: a plain id fills slots 0/1/2 (`DAT_8007bd0c/d/e`), id ranges `0x07..0x09` / `0x49..0x4d` / `0x88..0x8b` / `0xa2..0xff` get bespoke multi-monster / boss expansions (slot 1+ from `DAT_8007bd10..`), and id `0` falls back to `[4,_,4,4]`. The **alternate** formation source to the `FUN_801DA51C` `actor[+0x94]` record path - used for battles cued by a battle-id rather than an entity record. The cell *shape* distinguishes them (this writes 3 slots for a plain id; `FUN_801DA51C` writes only as many slots as the record's count). See [`formats/encounter.md`](../../formats/encounter.md#scripted-battle-id-path-fun_8005567c). |
| `80055B6C` | Battle-init (SCUS). Zeroes the per-battle scratch (`DAT_801C8FA0[0..0x10]`, `_DAT_8007bd08/34/38/44/48/4c`), then resolves the formation: when `DAT_8007b7fc != 0` it calls `FUN_80055B20` + `FUN_8005567C` (battle-id path); when `0` it only refreshes the cell from `FUN_8005567C` if the cell is empty (preserving an `actor[+0x94]`-installed formation). Calls `FUN_8005567C`. |
| `80046A20` | Post-battle **mode gate** (SCUS). Reads the transient battle-id `DAT_8007b7fc`: when `0` it selects the return game-mode `_DAT_8007b83c` (`0x18` field / `2` / `0`); a nonzero id routes the boss/scripted-battle continuation instead. The third and last **reader** of `DAT_8007b7fc` (with `FUN_8005567C` + `FUN_80055B6C`); a 47-program Ghidra sweep finds **no writer**, and a live write-watch (firehose from `chapter2_garmel_pre_zeto`, width 1 **and** 4) stayed **silent** across three Zeto fights - so it reads `0` in captured retail and may be **vestigial** (Zeto's formation comes from the `FUN_801DA51C` `actor[+0x94]` record path instead). See [`formats/encounter.md`](../../formats/encounter.md#scripted-battle-id-path-fun_8005567c). `see ghidra/scripts/funcs/80046a20.txt`. |
| `80055468` | Monster battle texture / CLUT pool loader: `(pool_ptr, tmd_ptr, wide_flag, slot)`. Builds a `StoreImage` RECT keyed on the battle slot - page at `(slot*0x40 + 0x140, 0x100)` (`= (slot*64 + 320, 256)`), width `0x20`/`0x40` fb-units per the wide flag - and calls `FUN_800583C8` twice to upload the 4bpp page and the CLUT region. The `_DAT_8007BD24+0x13` read selects the active battle slot for placement. Decoded into `legaia_asset::monster_archive`; see [battle](../../subsystems/battle.md#monster-mesh-record-0x04). |
| `80055B4C` | Side-band stream request arm. Writes `_DAT_8007BD24+0x26B = slot + 1`, `+0x26C = 0` - queues one `0x10800`-byte slot of `summon.dat` / `readef.DAT` for the transfer SM in `FUN_801F17F8` (bit 7 of `slot` selects the file). Both stores are `sb`, so slot `0xFF` wraps the request byte to the idle value and **disarms**; retail has no guard, and no caller reaches it. Ported as `engine-vm::battle_stream_slot::StreamSlotSm::arm`, the exact inverse of that module's `decode_request`. See [`formats/summon-readef.md`](../../formats/summon-readef.md). |
| `800557B8` | Action-record copy (the swing-record splice helper). Copies `0x2B` words (`0xAC` bytes) of action-entry header + `(parts * frames * 9 + 5) >> 2` words of the packed keyframe stream at `+0xAC` into the persistent buffer - the shape pin for the equipment swing records `FUN_80052FA0` installs at runtime slots `0xC..0xF`. Sibling `80055854` copies the equipment attach-object records linked into entry `+0x04`/`+0x08`. Ported as `legaia_asset::battle_char_assembly::swing_battle_animations`. `800557b8.txt`. |
| `8002B28C` | `"ME"` stream-archive reader: `(archive, dest, n)`. Magic `'M' 'E'`, `u8 count`, `u16 entry_sizes[count]` (bit 15 = compressed → `FUN_8002A9CC`, clear → raw copy). Called by `FUN_8004AD80` with `_DAT_8007BD74` (the side-band streaming buffer) to load an art record's keyframe stream - the archives live in `readef.DAT` slots `3*char+1`/`3*char+2`. Ported as `legaia_asset::me_archive`. `8002b28c.txt`. |
| `8002A9CC` | Channel-delta keyframe codec (the `"ME"` bit-15 decompressor). Header `(b0 & 0xC0) == 0x40` + u16 offsets to nibble / byte streams; selector bits pick 12-bit literal / previous-part-delta ± nibble / literal-nibble per channel; frame 0 accumulates spatially down the parts, later frames temporally; emits the packed `[parts][frames][9-byte TRS]` stream via scratchpad tables. Ported as `legaia_asset::me_archive::decode_channel_delta`. `8002a9cc.txt`. |
| `800558FC` / `80055A5C` / `800559EC` / `80055AC8` | Dual-mode stream-file API (open / seek / read / close). Retail (`_DAT_8007B8C2 != 0`, verified live) ignores the path string: open consumes its 4th argument as a **retail PROT TOC index** (`FUN_8003E8A8`), seek converts a byte offset to sectors relative to the entry base (`FUN_8003E964`), read issues `FUN_8003E800`. The debug branch (`FUN_800608F0` ISO9660 by path) is a trap stub in retail. See [`formats/summon-readef.md`](../../formats/summon-readef.md). |
| `80050E2C` | Generic linear pointer-table first-byte search: `(table, tag, count) -> idx_or_0xFF`. The battle-action SM resolves monster attack anims with it (tags `0x20`/`1`/`0x21`/`0x22` over the `+0x4C` action-record array); also battle UI lookups. First-match (not last) and the `0xFF` sentinel are confirmed against `SCUS_942.54` directly - the dump reports `0 instructions` and carries only decompiled C. |
| `801D0748` | Battle / level-up main tick (battle overlay). 11 KB / 2781 instructions / 26 outgoing. Per-frame driver for the battle + post-battle sequence. Reads sub-state byte at `_DAT_8007BD24[6]`; sub-states `0x1E`/`0x32`/`0x6E`/`0xFE` update camera yaw `_DAT_8007B792` from pad `DAT_1f800393`. Checks `_DAT_800846C8` (battle-active flag) and `_DAT_8007BD24[0x275]` (party-member count). After input handling calls `FUN_801D3444` + `FUN_801D9BBC`. Character-select input (L1/R1 = pad bits `0x2000`/`0x4000`/`0x1000`/`0x8000`) writes highlight byte to `(actor_table[n] + 0x1D)`. Captured as `overlay_magic_level_up_801d0748.txt`. |
| `801D388C` | Battle actor animation dispatcher (battle overlay). 7.8 KB / 39 callers. `(animation_type, param_2)`. Switch on `animation_type` (0..0x31+): cases 0/2 call `FUN_801DB318` and fall through; case 3 clears `actor[0x1E7]` and `actor[0x1DE]` for all 3 party slots; cases 5/7 compute `_DAT_80076D3A = func_0x80035F04(actor[0x1BC])` (animation-look-up into per-actor anim descriptor). Increments the battle frame counter at `_DAT_8007BD24[0x6B2]`. Actor pointers read from `DAT_801C9370/74/78`. Captured as `overlay_magic_level_up_801d388c.txt`. |
| `801D5854` | Battle per-pose **camera/presentation driver** (battle overlay). 6.5 KB / 47 callers. `(actor_slot, pose_id)`. Switch on `pose_id` (0..9) via the jump table at `0x801CEA00`, computing three i16[3] tween-target vectors handed to `0x801D7130`; secondary dispatch on `actor[+0x1DB]` (`0x11..0x18`, per-art camera variants). It never writes the anim fields (`+0x1D9/+0x1DA`) - the anim system proper is the `FUN_80047430` → `FUN_8004AD80` → `FUN_8004998C` chain. Poses update `_DAT_8007BD24[0x87C]` via pad accumulator and clamp `_DAT_8007BD24[0x26E/270]` at 200. Captured as `overlay_0898_801d5854.txt`. |
| `801D8DE8` | **HUD element renderer** (battle overlay; 3 KB / 77 incoming refs - the hottest battle helper). `(elem_id, mode, ...)`. Switches on `elem_id` through an 80-entry jump table at `0x801CEB68`, one case per on-screen element. Shared by the battle HUD and the Muscle Dome minigame: `0x0A/0x0B/0x0E` Spirit name / heading panels, `0x16..0x19` the 4-slot hand-card portraits, `0x1A` a formatted number, `0x52/0x53` player/opponent HP-bar values (`actor+0x170`), `0x58` opponent Spirit name, `0x59` (`mode==0`) the victory reward banner. The `(elem_id, mode)` pairs are the ones `PTR_DAT_801f4d34` indexes; see [`subsystems/minigame-muscle-dome.md`](../../subsystems/minigame-muscle-dome.md). `see ghidra/scripts/funcs/overlay_muscle_dome_801d8de8.txt`. |
| `801DA6B4` | Battle actor display-state controller (battle overlay). 204 bytes / 9 callers. `(visible)`. Walks battle actors 3..6 (`DAT_801C937C` array); for alive actors (`+0x14C != 0`): `visible=0` sets `actor[+0x21C] = 200` (opacity) and `actor[4] = 0x401004` (pose flags) for non-focused actors, `actor[+0x21C] = 5` for the focused one; `visible=1` clears `actor[+0x21C]` and `actor[+0x0C]`. `overlay_battle_action_801da6b4.txt`. |
| `801DB81C` | Next-valid-target scan (battle overlay). 152 bytes / 10 callers. Returns the next party slot after `_DAT_8007BD24[0x13]` whose battle actor has `+0x14C != 0` (alive) and `+0x16E & 0xF84 == 0` (no death/stone/silence). Used in level-up and action-select to advance the character cursor. `overlay_battle_action_801db81c.txt`. |
| `801DB8F4` | **Battle-HUD quad emitter** (battle-overlay copy, 208 bytes / 52 instructions). `(x, y)`. Early-outs on `ctx+0x6CE != 0` (`lh v0,0x6ce(v0)` off the ctx pointer at `0x8007BD24`), then allocates a `0x28`-byte primitive from the scratchpad pool at `0x1F800314+0x8C`, writes tag `0x09000000` and GP0 word `0x2C808080` (**`POLY_FT4`**, textured four-point), fills the four vertices at `x+0xF` × `y-4 .. y+0xB` with UV base `(0x10,0x70)`, CLUT `0x7FC7` and tpage `0x1E`, and posts it via `jal 0x8003D2C4`. Not a status-flag write. The `overlay_0897` body at this VA is a different 38-instruction routine - this row describes the battle-overlay resident. `see ghidra/scripts/funcs/overlay_battle_action_801db8f4.txt`. |
| `801DBDDC` | **Width-parameterised battle-HUD quad emitter** (battle-overlay copy, 232 bytes / 58 instructions). Same shape as `FUN_801DB8F4` - same `ctx+0x6CE` gate, same `0x1F800314` pool, same `0x2C808080` `POLY_FT4` and `FUN_8003D2C4` post - but the horizontal extent is caller-driven: `v1 = (width - 0x1E) >> 1` widens the quad symmetrically to `x+8-v1 .. x+0x27+v1`, with UV base `(0x50,0x60)`, CLUT `0x770B`, tpage `0x07`. A stretchable bar, not a timer ramp. The `overlay_0897` body at this VA is a different 19-instruction routine. `see ghidra/scripts/funcs/overlay_battle_action_801dbddc.txt`. |
| `801DD0AC` | Magic/summon damage calculator - [details ↓](#801dd0ac) |
| `801DD864` | Damage-roll scale stage - [details ↓](#801dd864) |
| `801DDB30` | Damage-roll finisher / committer - [details ↓](#801ddb30) |
| `801E295C` | Battle action state machine - [details ↓](#801e295c) |
| `801DE914` | Effect-bundle init / pack-fixup (battle overlay). |
| `801DFDF8` | Effect-bundle public spawn API (battle overlay): `(byte effect_id, short* world_pos, ushort angle)`. |
| `801E0088` | Effect-bundle per-frame walker (battle overlay). |
| `801F17F8` | `summon.dat` / `readef.DAT` side-band transfer SM (battle overlay, `FUN_800520F0` case `0xFF`). Opens TOC index `0x37F` / `0x380` (= extraction PROT 893 / 894) via `FUN_800558FC`, seeks `slot * 0x10800`, reads one slot into `*0x8007BD74`. See [`formats/summon-readef.md`](../../formats/summon-readef.md). |
| `801F12D0` | Side-band applier SM (battle overlay; dump `overlay_muscle_dome_801f12d0.txt`). 8-stage machine over `_DAT_8007BD24+0x276/+0x277`: odd stages request slots `base..base+3` via `FUN_80055B4C`, even stages upload the arrived slot's CLUT rows (VRAM rows 486/488/490) + 4bpp texture page (`(512,0)` / `(640,0)` / `(448,256)`) and copy the big-summon part pool to `*0x8007B85C + 0x44000`. The stage-4 tail resets the SM unless bit 7 is set or `base == 0x36`, so **readef groups stop after `base+1`** - the reason a character's main `"ME"` archive is the turn-resident slot (see [`formats/summon-readef.md` § Streaming state machine](../../formats/summon-readef.md#streaming-state-machine)). |
| `801F19EC` | Summon-creature actor install (battle overlay; dump `overlay_muscle_dome_801f19ec.txt`). Fixes up the last streamed slot's in-slot offsets to pointers (`[name][TMD][texture pool]` + part table at `+0x4A/+0x4C`), routes TMD + texture pool through `FUN_80055468`, and stages the summon as a battle actor (`FUN_80024C88`). |
| `801F811C` | **Screen-mask (iris) widget per-frame handler** - kind 1 of the PROT-0900 widget family ([move-vm.md § widget family](../../subsystems/move-vm.md#screen-effect-widget-family-prot-0900)). `(actor)`. `+0x3c/3e/40/42` (targets) vs `+0x14/16/18/1a` (latched current) are **screen-rect edges L/T/R/B**, not a world position: `+0x9E == 0` snaps; else `+0x9C += DAT_1F800393` (clamped), each *display* edge re-interpolates from the latched value via `FUN_801DE4C8(…, 1)`, latching on `+0x9C == +0x9E`. Then emits the **4 black border quads** framing the rect (GP0 `0x28`, OT `+0x1c`). Control API `FUN_801F8D4C`. `overlay_dance_801f811c.txt`. Ported as `screen_fx::MaskWidget` (corrects the "summon-part position update" reading). |
| `801F8D4C` | Mask-widget control API (PROT 0900). `(l, t, r, b, dur)`: find-or-spawn the `FUN_801F811C` widget on the effect-actor list (`FUN_8003CF04` / `FUN_80020DE0` with the `0x801F8FFC` descriptor; fresh spawn = fully open `[x0, 0, 0x140, H-1]`), substitute the full-open default for any edge passed `-1`, set targets `+0x3c..0x42`, `+0x9C = 0`, `+0x9E = dur`. Field-VM caller: op `0x43` sub-`0x11` (`jal` at `0x801DF974`). Ported as `screen_fx::MaskWidget::{spawn, set_rect}`. |
| `801F8004` | Sprite-widget spawner (PROT 0900). `(record)` → actor bound to handler `FUN_801F7A9C` (descriptor `0x801F8FE4`). Record `[x][y][w][h][tex_x][tex_y][clut_x][clut_y]` i16s + `rgb` u24 at `+0x10`, widget script at `+0x13`; derives `texpage = (tex_x>>6) + ((tex_y & ~0xff)>>4)`, `u = (tex_x & 0x3f)<<2`, `v = tex_y & 0xff`, `clut = (clut_y<<6) + (clut_x>>4)`. Field-VM caller: op `0x43` sub-`0x10` (`jal` at `0x801DF918`, inline record). Ported as `screen_fx::SpriteRecord::parse` + `SpriteWidget::spawn`. |
| `801F7A9C` | Sprite-widget per-frame handler (PROT 0900). Interprets the byte-coded widget script at `actor+0x90` (opcode `0x40`, sub-op at `+2` via the 5-entry head table `0x801F7B14/7B28/7B54/7B8C/7D90`: kill / wait-flag-set / wait-flag-clear / tween-pos+colour / tween-colour, story flags via `FUN_8003CE64`), then emits a GP0 `0x64` SPRT + texpage packet (OT `+0xc`). Tweens re-interpolate from start slots captured at `+0x9C == 0` (`+0x3c/3e` pos, `+0x7c` colour). Ported as `screen_fx::SpriteWidget::tick`. |
| `801F849C` / `801F88FC` / `801F8E6C` | Image-panel widget (PROT 0900): per-frame handler / spawner / move-scale API. Five tweened channels (x, y, w, h, first-page width `+0x24↔+0x26`); 1–2 textured GP0 `0x2C` quads over **15bpp** texpages (spawn ORs `0x100` into the page selector; a panel wider than 256px splits across two pages, second page in `+0xa2`), OT `+0x10`. `FUN_801F8E6C(x, y, scale, dur)` scales the `+0xb8/ba/bc` base sizes by 4.12 fixed `scale`. Field-VM callers: op `0x43` sub-`0x13` / sub-`0x14` (`jal` at `0x801DFA70` / `0x801DFABC`). Ported as `screen_fx::PanelWidget`. |
| `801F8A34` / `801F8F28` | Letterbox widget (PROT 0900): per-frame handler / config API. Six i16 config `[x_left][x_right][y0][y1][y2][y3]`; draws two solid black bands (`-y_off..y0`, `y3..H`) + two gradient feather strips (white→black / black→white, GP0 `0x3B` shaded semi-transparent behind a subtractive draw-mode packet `FUN_80059010(…, 0x55, …)`), OT `+0x4`. Field-VM caller: op `0x43` sub-`0x15` (`jal` at `0x801DFACC`). Ported as `screen_fx::Letterbox`. |
| `80020DE0` | Effect-widget actor allocator (SCUS). `(descriptor, list)`: allocates an actor on `list` (the screen-effect list global `_DAT_8007C34C` for the PROT-0900 widgets) and seeds it from the 0x18-byte handler-binding descriptor - per-frame handler from `descriptor+8` into `actor+0xc`, flags `descriptor+0xc \| 2` into `+0x10`, tween clock/duration `+0x9C/+0x9E` zeroed. `80020de0.txt`. |
| `8003CF04` | Effect-widget/actor list **finder** (SCUS) - a finder, not a kill function. `(list, handler)`: walks the list for a live actor with `actor[+0xc] == handler && !(actor[+0x10] & 8)` (bit 8 = killed). The find-or-spawn half of every PROT-0900 widget control API; also how the balloon spawner locates a live predecessor (the kill itself lives in the balloon handler `FUN_801DA7F0`'s first lines). `8003cf04.txt`. |
| `801DE4C8` | **Multi-mode interpolator.** `int(a target, b start, t time, D dur, mode)`. `if (a == b \|\| D <= t) return a;`. Mode 1 = plain linear `(a - b) * t / D + b` (integer truncating div, no rounding); mode 2 = quadratic ease-out `(e + (e/D)*(D-t))/D + b` with `e = (a-b)*t`; mode 3 = quadratic ease-in `((a-b)*t/D)*t/D + b`; mode 4 = two-segment ease-in-out (mode-3 to the midpoint over `D/2`, then mode-2). Overlay-resident at the field-VM RAM band - `overlay_dance_801de4c8.txt`. Ported as `engine-core::screen_fx::interp` (all four modes; `summon::lerp_axis` delegates the mode-1 arm). |
| `801DE648` | **Sized store helper.** `void(value, *dst, size)`: `size == 1 → (u8)`, `== 2 → (u16)`, `== 4 → (u32)`. The widget tween handlers store each channel's interp result through it. `overlay_baka_fighter_801de648.txt`. Collapses to a plain field write in the engine. |
| `801E9FD4` | Monster-AI action picker - [details ↓](#801e9fd4) |
| `801E7320` | **Monster-AI target resolver** - the `monster_setup` hook (`FUN_801E295C` `ActionSeed`, gated on `actor[+0x16e] & 0x380`). Expands the targeting class in `actor[+0x1DD]`: class `0..2` → living monster slot (`rand % ctx[+1] + party`), `3..6` → living party slot (`rand % ctx[+0]`), `8`/other → `rand%3` gate for all-target codes `8`/`9` / self. Ported exactly as `engine-core::World::resolve_monster_target`. `overlay_battle_action_801e7320.txt`. |
| `801DABA4` | **`recompute_battle_order`** - picks the next actor to act: zeroes dead actors' initiative keys (`+0x16c`), scans for the living actor with the highest key, breaks ties at random into `_DAT_8007BD24[0x274]`. For a monster pick it drives `FUN_801E9FD4` (the AI action picker). Also seeds the side-band group base `ctx+0x277` per turn (party `3*(char−1)`, enemy `3*monster_record[+0x1C]`) and, when no enemy lives, requests the base `"ME"` archive (`FUN_80055B4C(3*char+2)`) - see [`summon-readef.md`](../../formats/summon-readef.md#streaming-state-machine). Keys seeded per round from SPD by `overlay_0897_801e23ec`. Selection core ported as `engine-core::World::next_combatant_by_initiative`. `overlay_battle_action_801daba4.txt`. |
| `801E70BC` | **Summon-magic level-up check** (battle overlay 0898). `()`. After a summon returns (state `0x36` of `FUN_801E295C`): finds the cast spell id (`actor[+0x1DF]`) in the record spell-id list (`+0x13D`), reads the level byte (`+0x161` array) + accrued XP (u32 array at `+0x8`, fed by the `FUN_801DDB30` spell-XP tail), and levels up (`+1`, capped below 9, banner `0x65`) when `xp > (table[level-1] * mult) >> 1` (strict; table at SCUS `0x8007656C`; `mult` 3 for ids `0x86/88/8D/99/9B/A0`, else 2). Ported: `battle_formulas::summon_magic_levels_up` + `World::accrue_summon_spell_xp` + loader `magic_xp::thresholds_from_scus` - see [battle-formulas.md](../../subsystems/battle-formulas.md#summon-spell-xp--magic-level-up). `overlay_battle_action_801e70bc.txt`. |
| `801E6968` | **Lost Grail "Final Heal" auto-revive** (battle overlay 0898). `()`. Called by cleanup state `0x50` of `FUN_801E295C` before its liveness count. Acting actor's target byte (`+0x1DD`) `< 3` checks that party target, `== 8` sweeps the party. A downed member (`+0x14C == 0`) with ability bit `0x27` (`+0xF8 & 0x80`, the Lost Grail passive) revives at full max HP via `FUN_800402F4(4, 1, slot)` (statuses cleared), then **one equipped Lost Grail is consumed**: the first accessory slot (`+0x19B..+0x19D`) holding item id `0xE7` is zeroed and the bit cleared, re-set when a second Grail remains. Tail: a scripted boss-transition arm (`DAT_8007BD0C == 0xB5`), not modelled. Ported as `World::apply_final_heal_revives`. `overlay_battle_action_801e6968.txt`. |
| `801E7250` | **HP-bar drain settle check** (battle overlay 0898). `() -> 0/1`. Dispatches on the active actor's target byte (`+0x1DD`): party target `0..=2` → returns 1 while that actor's live HP (`+0x14C`) differs from its HP-bar display value (`+0x172`); monster targets `3..=7` and `> 8` → 0 immediately; target `8` ("all") → scans every slot up to the battle actor count. The state-`0x51` (fade-down) arm of `FUN_801E295C` freezes the `+0x6D8` countdown while this returns 1, so the action never concludes mid-drain. Ported as `hp_bar_drain_pending` in `crates/engine-vm/src/battle_action.rs` (the `hp` vs `hp_display` pair). `see ghidra/scripts/funcs/overlay_battle_action_801e7250.txt`. |
| `801E791C` | **Run / escape roll** (battle overlay 0898). `() -> 0/1`. Called by the state-`0x64` arm of `FUN_801E295C`; writes the `_DAT_8007726C` outcome pointer. Party score = per-slot `(SPD*3)>>1 + missingHP>>4`, enemy score = `SPD + missingHP>>5`, two rand draws modulo the scores; ability bits 52/55 (Chicken Heart x1.5 / Chicken King forced tie) fold from living wearers; fail iff `roll_p < roll_e` or `ctx+0x287`. ctx inputs set at setup: `ctx+0x287` (no-escape) by `FUN_800513F0`, `ctx+0x291` latched from `ctx+0x290` (`FUN_80051D84`). Full decode: [battle-action § escape roll](../../subsystems/battle-action.md#the-escape-roll-fun_801e791c). Ported as `battle_formulas::escape_roll` / `World::roll_battle_escape`. `see ghidra/scripts/funcs/overlay_battle_action_801e791c.txt`. |
| `801F02D0` (ref `0x801F0348`) | **Battle-UI widget-pool teardown** (battle overlay). `()`. Walks the 40-slot (`0x28`) tracked-widget table at ctx `+0x11B4` (stride `0xC`); for each slot with flag `+0x11B7` set and a live widget pointer at ctx `+0x1074 + slot*4`, releases it via `func_0x800319A8(widget[+8])`, then clears the pointer + `+0x11B4`/`+0x11B7`; finally zeroes 16 words at `0x801C8FA0`. Called from `FUN_801E295C` at action-begin (`0x0C`) and capture-finalize (`0x70`/`0x71`). `see ghidra/scripts/funcs/overlay_0897_801f0348.txt`. |
| `801F1CC8` (ref `0x801F1ED4`) | **Summon actor/camera re-frame** (battle overlay). `(anchor)`. Bounding-boxes all live actors' ground XZ (`actor[+0x34]`/`+0x38`; party `0..2` always, monsters gated on `+0x14C`), recenters every actor on the box centroid, and adds the centroid to the world/camera anchor globals `_DAT_80089118`/`_DAT_80089120`; when the caller's angle delta `> 0x800` it also Z-compresses. Called from summon states `0x34`/`0x35`/`0x36` (returns void; **not** the creature spawn - that is the `summon.dat` applier `FUN_801F12D0`/`FUN_801F19EC`). `see ghidra/scripts/funcs/overlay_0897_801f1ed4.txt`. |
| `801F3990` | **Cast audio-cue dispatcher** (battle overlay, PROT 0898 file `0x25178`; one `jal` at `0x801E3E04` in `FUN_801E295C`). Reads `ctx[+0x13]` + the char-kind table `0x8007BD10`, dispatches on `actor[+0x1E8]`, plays the per-class cast cues via `FUN_8004FCC8` (enemy leg `0x20C..0x20E`, player leg `char_kind*0x10 + 0xF8..0xFC`; full algebra in the port `legaia_engine_vm::battle_cast_cue::cast_audio_cue`). Does **not** read the move-power table - the "per-move score roll `FUN_801F3894`" reading was a mis-based dump's address space; see [move-power.md](../../formats/move-power.md#0x801f3990-is-a-real-function-and-not-a-consumer-of-this-table). |
| `801E92DC` | **Seru-spell learn / list prepend** (battle-action overlay 0898). `(spell_id)`: shifts the caster's three parallel spell arrays up one slot in a single descending loop (ids `+0x13D`, levels `+0x161`, u32 XP `+0x8`, off char record `0x80084140 + (id-1)*0x414`), then writes slot 0 as `id = spell_id - 0x80`, `level = 1`, `xp = 0` and bumps the count `+0x13C` - the newest Seru always lists first. The shiny-Seru randomizer hooks patch this routine's level write (`0x801E93B4`) + shift (`0x801E9320`), see [`randomizer.md`](../../tooling/randomizer.md). Port `engine-core::magic_xp::learn_spell_prepend`. `see ghidra/scripts/funcs/overlay_battle_action_801e92dc.txt`. |
| `801E91E8` | **Miracle-command token position lookup** (battle overlay). `(token) -> u8`. When the acting slot is a player (`ctx[+0x13] < 3`), its Miracle marker `ctx[+0x25F + slot]` is set and `_DAT_8007BAC0 == 0`, returns the 1-based position of `token` in the character's MSB-masked Miracle command string (char record count `+0x704`, bytes `+0x705..`, each stored `value + 0x80`), `0` when absent; in every other case returns `1` unconditionally. Called from the capture helper `FUN_801EC3E4`. Port `legaia_engine_vm::battle_action::miracle_command_position`. `see ghidra/scripts/funcs/overlay_battle_action_801e91e8.txt`. |
| `801F452C` | **Magic-level-increased banner composer** (battle-action overlay 0898, real entry): copies the cast spell's name (`DAT_800754D0[actor[+0x1DF]]`) into the context message buffer `_DAT_8007BD24 + 0x1F9` + the `"'s magic level increased."` suffix. The old "damage/HP-bar settle at `801F452C`, entry `<= 801F4498`" row described wrong-base prints: the `overlay_0896`/`0897` programs are `0x801C0000`-band imports carrying 0898's image shifted, so their "`801f452c`" fragments are interiors of `FUN_801D388C` / `FUN_801DDB30`. Port `engine-core::magic_xp::magic_level_increased_message`. `see ghidra/scripts/funcs/overlay_0898_static_801f452c.txt`. |
| `801DBA90` | **Skill/magic cast-caption composer** (battle-family overlays; byte-identical across `battle_action` 0898 / `magic_capture` / `magic_level_up` / `muscle_dome`). Builds a two-segment caption into the context message buffer `_DAT_8007BD24 + 0x1F9`: segment one is the acting character's label - active-actor index `ctx[+0x13]` through the char-kind selector `DAT_8007BD10` (`- 1`) into the overlay label-pointer table `0x801F4DFC` (`FUN_8003CA78` string copy); segment two is the spell name from [`DAT_800754D0`](../../formats/spell-table.md) at magic index `ctx[+0x269] + 0x80` (Seru-magic block, 0xC stride) plus the fixed suffix `0x801F4C28`. Sibling of the magic-level banner `FUN_801F452C`. `see ghidra/scripts/funcs/overlay_battle_action_801dba90.txt`. |
| `801F45A4` | **Per-round status-`0x400` RNG waker** (battle-action overlay 0898): loops the 7 battle-actor slots (`&DAT_801C9370`); for each live actor (`+0x14C != 0`) carrying status bit `0x400` in `+0x16E`, rolls the shared RNG (`FUN_80056798`) and on `rng & 7 == 0` clears exactly that bit (`andi 0xFBFF` at `0x801F4610`). The wake-up half of the latent bit-`0x400` lifecycle (see [battle.md](../../subsystems/battle.md#the-0x16e-status-halfword---retail-writer-inventory)). Engine port `engine-vm::battle_formulas::status_0x400_wakes`. `see ghidra/scripts/funcs/overlay_0898_static_801f45a4.txt`. |
| `80051D84` | **Formation-advantage roll** (SCUS battle setup), not an "escape-state setup". Compares both sides' mean SPD under a BIOS-rand spread and writes ctx `+0x290`: `1` = **back attack** (`sb v0,0x290(v1)` at `0x80051FE0`, then `+0x46 = 0x800` on the three **party** actors), `2` = **pre-emptive strike** (`0x80052098`, then `+0x46 = 0` on the four **monster** actors) - only the disadvantaged side is turned around. Skipped when the no-escape flag ctx `+0x287` is set. `FUN_801E295C` state `0x00` latches `+0x290` into `+0x291`; the escape roll reads the latch. Full decode: [`battle-formulas.md` § Formation advantage](../../subsystems/battle-formulas.md#formation-advantage-fun_80051d84). Ported as `battle_formulas::roll_formation_advantage`. `see ghidra/scripts/funcs/80051d84.txt`. |
| `801E7824` | **Captured-monster takedown** (battle overlay 0898). `(monster_slot)`. The state-`0x68` (capture-start) arm of `FUN_801E295C` calls it with the active slot: queues the monster-record action-table anim (`FUN_80050E2C(rec + 0x4C, 1, rec[0x4A])`, record from `(&DAT_801C9348)[slot - 3]`), increments the `+0x1DC` flag byte (raw `+1`), zeroes the HP pair (`+0x172` / `+0x14C`) and facing (`+0x46`), bumps the `+0x227` counter, retargets to `8`, points `_DAT_8007726C` at the ctx name buffer (`+0x1B9`) and copies the monster's name into it (`FUN_8003CA78` / `FUN_8003CAC4`), then opens the run-UI banner (`FUN_801D8DE8(0x43, 0)`). Ported as `capture_takedown` in `crates/engine-vm/src/battle_action.rs`. `see ghidra/scripts/funcs/overlay_battle_action_801e7824.txt`. |
| `801DA51C` | World-map / battle-entity SM (case 1 = encounter trigger). Fills the per-slot monster-id array `DAT_8007BD0C[slot]` from the inline encounter record at `actor+0x94` (`[+3]` = count, `[+4+slot]` = ids; the `docs/formats/encounter.md` format). `801da51c.txt`. |
| `8004C7B4` | Battle **facial animator** (per frame, party slots; Terra skipped). Reads the playing action entry's facial tracks (eyes `+0x8C`, mouth `+0x98`) and `MoveImage`-stamps the selected face frame over section 1's face rows from the static per-character frame tables. Called from `FUN_80047430` with the node `+0x68` cursor; live-pinned across a battle entry. The sibling pass `FUN_8004CCD4` follows (below - a mesh swap, not a stamp). Full layout: [`battle-data-pack.md` § Facial animation tracks](../../formats/battle-data-pack.md#facial-animation-tracks-entry-0x8c--0x98). `see ghidra/scripts/funcs/8004c7b4.txt`. |
| `8004CCD4` | Battle **equipment mesh-variant swap** (per frame; same guards as `FUN_8004C7B4`, called right after it). Not a stamp: writes TMD object pointers into the render node's per-channel model table (`*(node+0x44)+4`). Each surplus `0xFF` equipment object swaps onto its attach-bone channel while the playing entry's third track at `+0xA4` (two `[start,end]` byte windows per pair) is active, or unconditionally when the playing stream's part count differs from the idle's (pair from `ctx+0x240`). Re-run per after-image ghost by `FUN_80049348`. Plumbing: `FUN_80053898` / `FUN_800513F0`. Retail windows are Noa-only. Full decode: [`battle-data-pack.md`](../../formats/battle-data-pack.md#equipment-variant-track-entry-0xa4--fun_8004ccd4). `8004ccd4.txt`. |
| `80047430` | Battle per-frame **anim-node tick** + actor-update. Advances the 12.4 anim cursor (`node+0x68 += (frame_dt * actor[+0x21D] * record[+0x78]) >> 1`), detects end-of-clip, calls `FUN_8004AD80` (own caller unpinned - fn-ptr dispatch). Also sets the AI-delegation flag `actor[+0x16e] \|= 0x380` **only on party slots** with ability bit 45 (`+0xF8 & 0x2000` = passive `0x2D` **Rage**, Evil Medallion), mirrored into char record `+0x12E`; normal monsters keep `0x380` clear. The delegated action *pick* is not in the dumped corpus - see [battle-action.md](../../subsystems/battle-action.md#ai-delegated-0x380-party-members---what-is-and-isnt-pinned). `80047430.txt`. |
| `801E752C` | **Per-round status DoT ticker** (battle overlay 0898). Called by the round driver `FUN_801D0748` state `0x14` when the round counter `ctx[+0x28A] != 0`. Per living actor: Toxic (`+0x16E & 2`) drains `min(max_hp >> 4, 0x100)`, else Venom (`& 1`) drains `min(max_hp >> 5, 0x80)`, both clamped to `cur_hp - 1` (never lethal); also pays the Life Grail / Magic Grail per-round recoveries. Full arithmetic in [battle-formulas.md](../../subsystems/battle-formulas.md#per-round-status-dot-ticker---fun_801e752c); ported as `engine-vm::status_effects` `toxic_tick_damage` / `venom_tick_damage`. `overlay_battle_action_801e752c.txt`. |
| `80048A08` | Battle per-actor draw - [details ↓](#80048a08) |
| `8004998C` | **Per-object rigid-TRS keyframe decoder.** `(actor)`. Decodes the monster-animation packed stream into per-TMD-object translation + Euler rotation, interpolating between keyframes by the actor's 12.4 fixed-point phase. The decode counterpart of the battle draw `FUN_80048A08`. Full format in [`monster-animation.md`](../../formats/monster-animation.md); ported in `crates/engine-vm/src/anim_vm.rs` (`// PORT: FUN_8004998C`). `see ghidra/scripts/funcs/8004998c.txt`. |

### Ra-Seru capture overlay

All 78 functions dumped as `overlay_magic_capture_<addr>.txt`. Loaded during the
Ra-Seru capture mechanic (Gimard and other Ra-Serus); captured from a save state
taken during the capture animation. Shares actor struct layout
with the regular battle overlay (`_DAT_8007BD24` context pointer, `+0x1DE`
sub-state, `+0x07` action-type).

| Address | Role |
|---|---|
| `801D0748` | Capture outer dispatcher (11 KB, 26 outgoing). Same sub-state structure as the battle outer dispatcher; sub-states `0x1E`/`0x32`/`0x6E`/`0xFE` update camera yaw. `overlay_magic_capture_801d0748.txt`. |
| `801D388C` | Capture animation dispatcher (7.8 KB, 39 callers). Same interface as the battle overlay's `FUN_801D388C`. `overlay_magic_capture_801d388c.txt`. |
| `801D5854` | Capture actor pose driver (6.5 KB, 47 callers). Same interface as the battle overlay's `FUN_801D5854`. `overlay_magic_capture_801d5854.txt`. |
| `801D8DE8` | Hottest capture utility (3 KB, 75 callers). JT dispatcher; only callee is `FUN_801DB7B0` (the generic 4-byte JT helper). `overlay_magic_capture_801d8de8.txt`. |
| `801E295C` | **Capture battle state machine** (16.4 K-, 19 outgoing). Outer switch on `_DAT_8007BD24[7]` cases `0xB`/`0xC` (capture-specific action types). Inner switch on `actor[+0x1DE]`. Distinct from `overlay_battle_action_801e295c.txt` despite sharing the same entry address. `overlay_magic_capture_801e295c.txt`. |
| `801EC3E4` | Large capture helper (10 KB, 0 incoming - top-level from game-mode dispatch). Calls `FUN_801E91E8`. `overlay_magic_capture_801ec3e4.txt`. |
| `801E9FD4` | Capture sub-system (8.5 KB, 1 incoming). Calls `FUN_801EC0DC`. `overlay_magic_capture_801e9fd4.txt`. |

## Battle on-screen elements (HUD + 2D sprite/effect list)

Surfaced by the [trace-driven-coverage](../../tooling/playthrough-coverage.md) S5 (Tetsu spar) gap-set run - these SCUS functions are the always-resident on-screen-element helpers the battle HUD draws through (also used by the field/menu HUD; SCUS is shared). All confirmed live at `game_mode 0x15`.

| Address | Role |
|---|---|
| `8002C2E4` | **Party HUD row.** `(party_slot, x, y)`. For a live, unafflicted member draws an HP bar (`FUN_8002C488`) + HP number (`FUN_80034B78`); otherwise selects one **status icon** by priority from the character-record status bitfield (`record + slot*0x414 + 0x12E`, `u16`): `HP==0 → 0x20`, then bits `0x004 → 0x1a`, `0x400 → 0x1d`, `0x800 → 0x1e`, `0x380 → 0x1c`, `0x078 → 0x1b`, `0x1000 → 0x1f`, `0x002 → 0x19`, `0x001 → 0x18` (first match wins). Pins the HUD's view of the status bitfield's bit layout. |
| `8002C488` | **Icon / glyph sprite drawer.** `(x, y, icon_id) -> ...`. Emits a GP0 sprite into the OT `_DAT_1F8003A0` from a per-icon UV/tpage atlas table (`DAT_800732A8` stride 0xC). The shared status-icon / small-glyph renderer. |
| `8002C69C` | **Bar-widget dispatcher** (`POLY_FT4`/`SPRT` emitter). `(x, y, mode, value)`; widget kind pre-staged into `gp+0x14C` by `FUN_80034B6C`. Kind `0x31` = the status AP gauge: calls `FUN_8002C0B0` for the content, then the table-driven frame path (`0x800732A4` record class + corner/edge skin table `0x80073A00`). Kinds `0x33..0x35` route to `FUN_8002C2E4`. Its 3644-instruction body spans `0x8002C69C..0x8002FF88`, so `0x8002CDD0` (`sh v0,0xe(a1)`), `0x8002D988` (`beq v0,zero,0x8002ff00`) and `0x8002DAA4` (`sb v0,0x24(a1)`) are **interior** addresses of this row, not entries - each dump headers `(entry=8002c69c)` and none is even a branch target inside the body. |
| `8002C0B0` | **AP-gauge content renderer.** `(x, y, value)`. Meter fill = two untextured gouraud quads over `x+0x1B .. x+0x1B+value/2`, rows `y+5..y+10`, dark-red `(0x80,0x20,0x10)` -> gold `(0xC0,0xA0,0x40)` -> dark-red vertical diamond gradient; value = ICO `0x6B` ("100" glyph) at 100, else tens/ones digits ICO `0x6C+d` at `x+0x50`/`x+0x56`. See [`docs/subsystems/field-menu.md`](../../subsystems/field-menu.md). |
| `8002BDC4` | **Textured-image blit.** `(x, y, desc, clut, w, h)`. Draws a `desc`-described image (width/height default from `desc[2]/[3]`) at `(x,y)`; CLUT/tpage from `clut & 0x7F \| 0x7FC0`. HUD/label image helper. |
| `8002C0B0` | **Gauge-bar primitive.** `(x, y, value)`. Emits a filled GP0 quad (`0x39……` shaded-quad packet) whose width = `min(value>>1, 0xFF)` from `x+0x1B` - the HP/gauge bar the party HUD row draws. |
| `80021248` | **Camera-relative effect-actor spawn.** Allocates an effect actor (`FUN_80020DE0(&DAT_8007071C, _DAT_8007C34C)`), seeds position from record halfwords `[0xD]/[0xF]/[0x11]`, copies the 20-halfword body to `+0x80`, tracks it at `gp+0x750` (a fresh spawn flags the previous family actor via the `_DAT_1F800394` `0x100` latch), then **normalizes the copy against the live camera**: ten `(magnitude, reference)` pairs - 3 vs the angle triple `DAT_8007B790/92/94` (folded delta into the actor rotation, sign = fold XOR negative-delta), 3 vs `DAT_800840B8..C0`, 3 vs `DAT_80089118..20`, 1 vs GTE `H`. Ported as `legaia_engine_vm::camera_rel_actor` (module doc carries the full rules). `0x800212C4` in the gap-set is an interior instruction (label-call artifact). |
| `80031AE4` | **Screen-position tween pass** over the `gp+0x148` drawable list - the same list `FUN_80032434` builds and `FUN_80031D00` draws, not a separate one. Acts only on nodes carrying a tween descriptor at `+0x24`, interpolating `node+0xA`/`node+0xC` and advancing the timer `node+0x1E` by `DAT_1F800393`. **`node+0x20` is a signed *phase*, not a repeat/loop count**: the dispatch is `sel = phase + 2` over `0..=3` (`addiu v1,v0,2` at `0x80031B98`), giving parked-at-A / moving-to-A / parked-at-B / moving-to-B for `-2..=1`. The timer advances only in the moving phases, and expiry **decrements** the phase - the halfword is written exactly once, at `0x80031B74`, so nothing loops. Counts moving nodes into `gp+0x868`. Ported as `legaia_engine_core::float_tween`, which carries the rest. |
| `800355F0` | **2D floating-element list teardown.** Drains the same `gp+0x148` list, freeing each node via `FUN_800319A8`. The free-all counterpart to `FUN_80031AE4`. |
| `8004FE5C` | **Battle SFX-cue router.** `(id, category)`. Resolves a cue id into the 4-slot pending-SFX ring `DAT_8007B6D8` (drained by `FUN_80016B6C`): `< 0x48` enqueues `id-1` (element-tinted `id+0x281` for a non-party attacker at `id >= 0x1B`), `0x48..0x63` raw, `>= 0x64` the tinted `id+0x19C` - the attacker's element byte lands in byte `+4` of the `_DAT_8007B990` runtime-bank descriptor. Dedupes vs `DAT_8007B724`. `id >= 0x100` from a **party** attacker instead starts the arts-voice CD-XA clip (`FUN_8003D53C`; clip `(id-0x100)>>3` remap 1/3/5 → 26/27/28, channel `&7`, duration table `0x800788B8`), gated on the tutorial byte + read-idle poll `FUN_8003DE7C(1)` - the "uses the RNG" reading is superseded. Ported as `legaia_engine_core::sfx_cue`. |
| `800508DC` | **Battle voice/anim-cue select.** `(actor_id, param, key)`. Indexes the [8-slot actor table `DAT_801C9370`](#battle-subsystem) by `actor_id`, reads `actor+0x1F6`, range-tests the caller's `param+0x54` pair table, and for party slots keys into the live 0x414-stride character records (`DAT_8007BD10`-selected) to pick a battle voice cue (`FUN_8004FCC8(0x56/0x5C/0x62)`) or fall through to an anim id, with an RNG tiebreak (`FUN_80056798`). |
| `80050E00` | **3-slot table scan.** `(ptr)`. Tiny helper: scans up to 3 records, stopping at the first whose `+1` byte is `0` - counts populated entries in a 3-slot (party) action/id table. |
| `8005112C` | **Per-actor colour marker draw.** `(actor)`. Gated on `actor+0x68 != 0` and the render-class field `actor+0x5A < 3`; indexes `DAT_8007BD10[class]` and, matching the actor's element byte (`*(actor+0x4C)+0x77`) against specific ids, calls `FUN_80048310(actor, w, 3, rgb)` to draw a coloured element/affinity marker (RGB words `0x80FFC0` / `0x802040` / `0x204080` / `0x208040`). |
| `80046A20` | **Battle-scene per-frame tick** (2576 bytes): advances timers, drives the fade / hit-flash state machines, streams the next monster archive, and refreshes the party gauges. The one self-contained arithmetic kernel is the **HP/MP gauge-fill colour selector** at `0x80046AA8..0x80046D0C`: from actor `cur_hp`/`max_hp` (`+0x172`/`+0x14E`), `cur_mp`/`max_mp` (`+0x174`/`+0x152`) and a status flag (`+0x16E`) it picks a fill-colour index - `cur_hp==0 → 2`, `flag!=0 → 3`, else per bar `(max>>1)<cur → 7`, `(max>>2)<cur → 6`, else `9`. Ported (kernel only) as `legaia_engine_vm::battle_gauge::gauge_colors`; the rest is render/stream glue needing the full battle runtime. |

## Battle per-frame draw (overlay 0898, trace-surfaced)

New battle-overlay (`0898`) functions the S5 trace found live (`game_mode 0x15`), attributed by containment against the `overlay_battle_action_*` dumps (the aliased `overlay_0897`/`overlay_dance`/… stems the raw hit carried are VA-alias mismatches - the resident overlay at battle time is `0898`). Called each frame from the battle per-actor draw loop (`ra` in `0x80047EXX`/`0x80048130..48`).

| Address | Role |
|---|---|
| `801E2524` / `801E2650` | **Battle full-screen flash / fade overlay.** `FUN_801E2524` reads a trigger byte `ctx[+0x28B]` (`_DAT_8007BD24`) and, while `1..4`, draws up to four stacked full-screen layers via `FUN_801E2650(x, brightness%, tpage_flag, level)` - each a grey GP0 quad whose brightness `= min((brightness<<8)/100, 0xFF)` - then ramps the fade-progress byte `ctx[+0x28C]` by `DAT_1F800393*8` (capped `0xF0`, which gates off the brighter layers as it climbs). The white-flash / screen-dim used on impacts and battle transitions. |
| `801DF6B8` | **Per-actor battle draw/position loop** (1848 bytes). Iterates the 8 battle actors via the ctx order/select tables (`ctx+0x318` → `DAT_801C9370` slot, `ctx+idx*4+0x83C` liveness gate), reading each live actor's screen transform (`+0x3C`) and applying a `/10` scale (`0x66666667` magic). The builder that positions the on-screen per-actor elements (HP tags / markers) each frame; the top consumer of the SCUS on-screen-element helpers above. |
| `801D829C` | **Camera-state per-actor transform builder** (548 bytes). Reads the battle camera-state registers `DAT_8007B790/2/4` and composes per-actor transforms over the actor table (`DAT_801C9370`) + `DAT_800840BC` - the billboard/rotation setup that orients battle 2D-in-3D elements toward the orbit camera. |
| `801D71B8` | **Per-art attack-camera framing** (4.3 KB). Gated on the active actor (`ctx+0x13`) having a live target (`+0x14C != 0`), action category `+0x1DE == 3` (Attack), and `ctx+6 == 0xFF`. Builds a rotation / distance / look-at halfword triple on the stack (`0x400` seeds, look-at = the actor's *negated* position and facing) and dispatches per participant id `1`/`2`/`3` and then per art id `0x1A..=0x2A` through a 17-slot `jr` table, each arm folding its own halfword track from the per-phase data at `0x801F4E10`. One of the hottest attack-chain bodies. Gate + pose seed + dispatch + the `(anim_frame - 0x60) << 4` push ported as `engine-vm::battle_attack_camera`; the arms need the `0x801F4E10` table parsed. |
| `801E805C` | **Multi-cast value readout + UI teardown** (4.5 KB). Gated on `DAT_8007B64C` + the summon-overlay shared-buffer region `_DAT_801F697X`/`_DAT_8007BD14`. Two halves: it batches `FUN_801D8DE8(id, 0)` then `(id - 4, 0)` teardown pairs off the count at `_DAT_801F6974` (row `_DAT_801F6834 + (count-1)*4`), and it renders each populated slot in `_DAT_801F6988` as a label quad plus the slot's value from `_DAT_801F6980` split into decimal digits by reciprocal divides (`0xD1B71759 >> 45` = `/10000`, `0xCCCCCCCD >> 35` = `/10`), positioned off the HP-bar widget at `ctx[+0x1074 + ctx[+0x11B6 + slot*0xC]*4]`. Kernels ported as `engine-vm::battle_value_readout`. |
| `801E0080` | **Battle-arena emitter-driven sprite scatter** (2.4 KB, spans `0x801E0080..09F8` - just below `FUN_801E09F8`; the hits `0x801E0080`/`+0x338`/`+0x398`/`+0x518` are all interior). Gated on `DAT_8007BD58 != 0` and `DAT_8007BD71 == -1`. A 32-slot `0x1C`-stride **emitter** pool at `_DAT_8007BD30+0x1010` spawns into a 128-slot `0x20`-stride **particle** pool at `_DAT_8007BD30+0x10`, each record driven by its own byte script (emitter step 14 bytes, particle step 6, delay bytes `<< 3`), then a third pass emits one `0x28`-byte textured quad per live particle with a brightness ramp. Whole update repeats until its cost reaches `DAT_1F800393`. Ported as `engine-vm::battle_scatter` - [details ↓](#801e0080). |
| `801F0450` | **AI-side Arts command assembler** (3.7 KB, in `0898`'s render tail `0x801F0000..8000`; hits `0x801F0740`/`0x801F0ADC` interior). Two arms on the char-record `& 0x2000` / actor `+0x16E & 0x404` pair: a blind weighted draw from the character's learned-arts list, or a weighted candidate pool over the arts command table [`DAT_801C9360[char][cmd]`](../../subsystems/arts-command-gauge.md) (cmd from `0xC`) drawn against the AP gauge `actor[+0x154]`. It **writes** `actor[+0x1DF..]`, so it is an action producer rather than a display builder - [details ↓](#801f0450). Ported as `engine-vm::battle_arts_auto_combo`. (The tail also hosts already-documented `FUN_801EFE44` camera-bounds `+0x48C` = hit `0x801F02D0`, and `FUN_801F17F8` the side-band streaming SM.) |
| `801D02C0` | **Procedural battle ground grid** - the flat tiled floor the mode-`0x15` render draws under the combatants. Two GTE passes over a `_DAT_1F8003F8 x _DAT_1F8003FA` cell grid at pitch `0x200`; see [`battle.md`](../../subsystems/battle.md#backdrop-ground---a-procedural-flat-grid-func_0x801d02c0) for its place in the backdrop and [details ↓](#801d02c0) for the per-cell emit. CPU-side kernels ported as `engine-vm::battle_ground_grid`. |

## Battle sparring-tutorial overlay (PROT 0967)

A discrete overlay the [S5 trace](../../tooling/playthrough-coverage.md) surfaced: it drives the **in-battle tutorial prompts** of the scripted Tetsu sparring fight (the how-to-fight tutorial - basic attacks, items, spirit, Hyper-Arts lessons + the practice combo). Extraction **PROT 0967** (a distinct 14 KB overlay), loaded **co-resident at base `0x801F69D8`** (the shared summon/move-FX buffer `*DAT_80010390`), so its `0x801F6xxx..0x801F7xxx` code physically overlaps overlay `0898`'s *rodata* tail - `0898`'s own bytes there are menu label strings, which is why that region disassembled to garbage from the `0898` image.

Identity + base are pinned by a live-battle-RAM (`s5_tetsu_battle`) vs static-blob byte fingerprint (`overlay_effect_0967_*.txt`, imported at `0x801F69D8`). Its shifted sibling is PROT **0965** (`0965[0x5FE8:] == 0967[0x0:]`, 911/1024 identical over the first 4 KB at shift `0x5FE8`) — the same render-library re-imaging per game-mode context as `0900↔0901`; 0965 shares the code but at a different base offset. The overlay's tutorial-script strings are Sony text (not reproduced here).

| Address | Role |
|---|---|
| `801F71E0` | **Not an entry point** - a `bne` target inside the tutorial-message routine, and the second half of a `lui`/`lw` pair split across it. [Details ↓](#801f71e0-is-a-label-not-an-entry). The **pacing tail** that begins here is what the S5 trace hits: it decrements the message timer `ctx[+0x6B4]` (`_DAT_8007BD24`) by `DAT_1F800393 * DAT_1F80037D` (frame-count × rate) each frame; on underflow it advances the tutorial step (`ctx[+0x289]`/step index `ctx[+0x28A]`, loading the next line pointer from `ctx[+0x88C]` into `_DAT_8007B874`) and clears the pad latches (`0x8007B874`/`B938`/`B850`). A confirm-press (`_DAT_8007B874 != 0`, with `ctx[+0x6B2]==0`) skips the current timer. Sets `ctx[+0x6AE]=0` (line counter) / `ctx[+0x6B0]=1` (active). |
| `801F6C70` / `801F6D48` | **Tutorial-step text emitters.** Call the box helper `FUN_801F747C(str, mode)` with the step's message and run the same pacing tail as `FUN_801F71E0`. `FUN_801F6D48` dispatches on the step index (`0/1/2/3` → the attack-mode / item / spirit / Hyper-Arts lessons; step 2 also calls `FUN_801F7628`). Host (VA-aliasing): these emitters are the Tetsu-tutorial-battle slot-B overlay at `0x801F69D8` (this section's header), **not** PROT 0900 — at these same VAs PROT 0900's bytes are the field render library (`0x801F6D48` is a ground/tile renderer there). See the 0900 row in `crates/asset/data/static-overlays.toml` for the byte evidence. |
| `801F747C` | **Tutorial text-box display helper** `(str, style)`. Measures the prompt (`FUN_8003CBA8` lines, `FUN_80035F04` width) and registers it as a sized kind-`0xD` text actor: `FUN_8003541C(1 + waits, 0xD, str, x, y, width, lines*14 - 4, 0x44 - waits)`. Host (VA-aliasing): this is the PROT 0967 tutorial-battle occupant. `0x801F747C` is a genuine function head **only** in the 0967 image; in PROT 0900 the same VA falls *inside* `FUN_801F7088` (the field static-object/decoration renderer) — mid its scroll-window-clamp prologue (`lui v1,0x1f80; lb v1,0x3ea(v1)`), not a callable entry — and in PROT 0897 inside the inventory-hub body. So the "tutorial" reading is 0967-only, and this exact VA has hosted three distinct occupants at different times. |

### `801F71E0` is a label, not an entry

Read straight out of the extracted PROT 0967 image at its own base
`0x801F69D8`, the word **at** `0x801F71E0` is `lw v1,-0x42dc(v0)` and the word
**before** it is `lui v0,0x8008` - the two halves of one address load, split
across the address. `0x801F71E0` is also the target of
`bne v1,v0,0x801f71e0` sixteen bytes earlier, and there is no `jr ra` between
it and the only prologue below it (`0x801F6B78`, `addiu sp,sp,-0x38`); the
enclosing routine's sole `jr ra` is at `0x801F7474`.
[`locate-entry-image.py`](../../../scripts/ghidra-analysis/locate-entry-image.py)
agrees across every based image: no frame here, and no in-image `jal` to it.

The dump that reported a self-entry 167-instruction body has the right bytes
at the right base; only its `entry=` is wrong, because
`dump_effect_overlay_0967.py` creates a function at each traced hit address
and a trace hit lands on whatever instruction the breakpoint sat on.

## Battle command-block persistence + target menu (overlay 0898, trace-surfaced)

The [S6 trace](../../tooling/playthrough-coverage.md) (first non-tutorial battle, the Queen Bee ambush) surfaced the path that carries a party member's chosen **16-arm Arts command block** across battles and builds the enemy target list. All three live in overlay `0898` (containment-attributed against the resident battle-action image; the aliased dump stems are VA mismatches). The 16-byte block sits at live-actor `+0x1DF` and is mirrored into the persistent character record at `record[char-1]+0x1B7` (`0x80084708 + (char-1)*0x414 + 0x1B7`); the enemy formation ids come from `DAT_8007BD0C`, the same global the [charm](../../tooling/randomizer.md) / shiny-Seru features key on. See [`subsystems/arts-command-gauge.md`](../../subsystems/arts-command-gauge.md) for the command-gauge side.

| Address | Role |
|---|---|
| `801DA34C` | **Restore the lead actor's command block at battle start** (592 B). Reads the battle context `_DAT_8007BD24`; when the enter flag `DAT_8007BD04 == 0` it zeroes the live actor's 16-byte block (`DAT_801C9370[actor[+0x13]] + 0x1DF .. +0x1EE`). Otherwise, HP-gated (`+0x154 <= +0x156`), it copies the saved block from `record+0x1B7` back into the actor (or zeroes it when the record flag is clear). `DAT_8007BD10[actor]-1` selects the character record. |
| `801DA59C` | **Persist a party member's command block** (280 B). `FUN_801da59c(actor_index)`: guarded on `actor[+0x14C] != 0` and phase byte `actor[+0x1DE] == 3`, copies the actor's 16-byte `+0x1DF` block back out to `record+0x1B7`; a min/max compare on `+0x154`/`+0x156` picks which of two write banks. The save-side inverse of `FUN_801DA34C`. |
| `801D9D3C` | **Enemy target-selection-menu builder** (1552 B). Walks the formation-id table `DAT_8007BD0C`, collapsing consecutive identical monster ids into one menu row, and for each distinct enemy copies its name (`func_0x8003ca78` string copy into `_DAT_8007BD24 + slot*0x20 + 0x29`) plus stats (`DAT_801C9370[i+3] + 0x34` / `+0x1BC`). Produces the "which enemy" list the command menu offers. |

## Field->battle transition overlay (intro camera spin)

The [S6 trace](../../tooling/playthrough-coverage.md) captured the **field->battle transition** the mid-battle S5 anchor could not: the 3D camera spin that plays between the field and the battle load, when the **field overlay 0897 is still resident but the battle overlay 0898 has not yet loaded** (dumped from `overlay_field_battle_intro.bin`, a partial 0897 image). The trace's `0x801CFxxx` cluster lives here - it is *not* 0898 code (those VAs are a data table in the 0898 image; identity was settled by fingerprinting the live queen-bee RAM back to `overlay_field_0897.bin` at base `0x801CE818`). Aliased union stems (`str_fmv`, `save_ui`, `cutscene_dialogue`, `menu`) are VA mismatches.

| Address | Role |
|---|---|
| `801CF5BC` | **Field->battle transition state machine** (752 B; the hot S6 hit, interior hits `+0x2C` `0x801CF5E8` / `+0x2D0` `0x801CF88C`). A phase counter at `arg+0x22` sequences the battle handoff: phase 1 runs the battle-mesh assembly (`FUN_80052770`), phase 2 loads the battle BGM (`func_0x800567a8("battle_bgm_%d", id)`) and the battle-scene bundle (`func_0x8001fc00(0x36F + id, ...)`). A parallel camera-spin timer `arg+0x1a` is compared against the total intro duration `DAT_801D2458`: near the end it raises the ready bits `arg+0x2a \|= 1`/`2`, and at completion (`arg+0x2a == 3`) it writes the game-mode handoff **`_DAT_8007B83C = 0x14`** (enter battle). Ambush special-case: `DAT_8007BD0C == 0xA6` (first-monster id) forces `_DAT_8007B880 = 0`. |
| `801CFBB4` | **Battle-intro swirl/shatter particle builder** (492 B). Allocates a `0xDC00`-byte primitive buffer (`func_0x80017888`) and fills a grid of `0x2C`-stride sprite records (color `0x808080`, `0x40`x`0x40`, positions stepped from `-0x1400`/`-0x500`), each rotated through the sin/cos tables `_DAT_8007B7F8`/`_DAT_8007B81C` via `func_0x80019B28` - the visual effect drawn over the camera spin. |

## Unreferenced SCUS entry points

Three of the battle-band routines above are entry points that **nothing on the
disc reaches**. A sweep of `SCUS_942.54`, every based overlay image and the raw
bytes of every extracted `PROT.DAT` entry finds no literal address word, no
`jal`, no `j`, no PC-relative branch and no `lui`+`addiu` materialisation for
any of them
([`address-reference-scan.md`](../../tooling/address-reference-scan.md)).

They are functions rather than interior labels, but the evidence is the
**preceding epilogue**, not a prologue of their own: each is immediately
preceded by a `jr ra` whose delay slot closes the previous frame, so nothing
falls through into it. Only `8005126C` and `80035274` then open frames
(`addiu sp,sp,-0x38` and `-0x20`). `80050D40` opens none at all - it is a
frameless leaf, and a missing prologue is not a missing function
([`worklist-classification.md`](../../tooling/worklist-classification.md#the-three-kinds-of-ignore-claim)).

| Address | What it is | The surviving path it duplicates |
|---|---|---|
| `8005126C` | battle actor on-screen test | none - the sibling rectangle probe `FUN_8001B73C` is a different test, so no live pass consults a horizontal-span verdict |
| `80035274` | item / equipment passive-**name** draw | `FUN_80034250` (same chain, draws the passive *description*) and the menu overlay's window-34 renderer `FUN_801D4A80` |
| `80050D40` | 12-bit angle tween | `FUN_8001D088`, the ANM interpolator, which every live angle blend uses |

`FUN_80050D40` is worth separating from its twin precisely because it is
*almost* the same routine. Both wrap the pair into `0..0xFFF`, add a turn to
whichever side shortens the arc, accumulate the swept magnitude into the same
global `0x8007BD28`, journal the adjusted endpoints into a slot table, and
return `(to' + ((from' - to') * weight >> 4)) & 0xFFF`. Three things differ:
this one takes the short arc on `> 0x800` where `_D088` takes it on `>= 0x800`,
shifts the scaled delta logically (`srl`) where `_D088` shifts it
arithmetically (`sra`), and masks the slot argument to a byte into a stride-4
table at `0x801C9060` where `_D088` uses the raw argument into a stride-8 one
at `0x800891A8`. A port that treats them as one function inherits the wrong
sign behaviour on a negative delta.

The pattern each one falls into is the same: a static routine that a later,
overlay-resident or differently-parameterised sibling superseded, left linked
because the linker keeps whole objects. The consequence for the port is that
there is **nothing to wire them to** - the open question for `8005126C` was
never "which pass culls with it" but "does any pass", and the answer is no.
`80035274` is worth reading anyway for one fact it is the only decoded witness
to: for an item whose property-table kind byte is `1`, the passive index comes
from the [equipment record](../../formats/equipment-table.md)'s `+0x5` rather
than the item-effect record's `+0x3` - and since every retail equipment row
carries `0x40` there, that arm draws nothing even when reached.

A fourth address of the same shape sits one level up rather than at the
function: `FUN_80025054`'s
[actor template](runtime-libs.md#static-actor-templates) is `0x80070614`, and
*that record* is what nothing materialises. Its neighbours on the same grid are
each named by a `lui`+`addiu` pair at a spawn site; `0x80070614` is not, so the
tick is unreachable without any statement about the tick itself.

That negative needs the **whole table** swept, not just the record - an address
reached as `table_base + index` is not a materialisation pair, so a per-record
result means nothing until the base is checked too. Sweeping `0x800705FC`
onward settles it in the same pass. The table head is materialised exactly once,
in the field overlay at `0x801D6D6C`, and the two instructions after it are
`lw a1, 4(s0)` and `jal FUN_80020DE0` with `addiu a0, a0, 0x5fc` in the delay
slot: the head is handed to the allocator as one record, not walked. No site
indexes the grid, and `0x80070614` appears in no image in any form.

All four addresses are filed under the ignore list's `unreferenced` section
([`worklist-classification.md`](../../tooling/worklist-classification.md#the-reachability-claim)),
except `8005126C`, which was ported before the sweep ran.

## Function details

Full write-ups for the rows above whose detail outgrew a table cell. Linked from each section table by **[details ↓]**.

### `8004DA00`

The SCUS half of the battle voice-stream census in
[`audio.md`](../../subsystems/audio.md#streamed-cue-census-fun_8003eae4--fun_80019794).
It is a per-frame tick, not a call: its address is the `+0x08` word of the
[static actor template](runtime-libs.md#static-actor-templates) at
`0x800767F4`, and every path through it ends in the actor-maintenance pass
`FUN_8004CE2C`.

**Its spawner is the battle scene-loader `FUN_800513F0`**, whose last act
before returning is `FUN_80020DE0(0x800767F4, _DAT_8007C34C)` at `0x80051D3C` -
so the selector goes resident for the whole battle and runs once per frame from
the system actor pool. Nothing calls it directly, on the disc or anywhere else;
the template word is its only reference, which is why a `jal` sweep finds no
caller for it (see
[`address-reference-scan.md`](../../tooling/address-reference-scan.md)).

A stream is armed only when all four gates pass:

- the CD is free (`_DAT_8007BC20 == 0`);
- `_DAT_8007BD71 == 0xFF`;
- the battle context `gp[0xA0C]` has `+0x26B == 0`, `+0x276 != 0` and `+0x7 != 0x5A`;
- nothing is latched yet (`_DAT_8007BDB0 == -1`).

The three gates that mean "not ready yet" (`+0x26B`, `_DAT_8007BD71`,
`+0x276`) **reset the latch to `-1`** on their way out; the other two exits
(`+0x7 == 0x5A`, and a latch that is already set) leave it alone. So the latch
is cleared by the frames between actions, not by whoever consumes it.

The acting seat comes from `ctx[+0x274]`, which indexes both the party-order
byte table `DAT_8007BD10` and, for seats `0..2`, the three-entry actor-pointer
table at `0x801C9370` - the same table the on-screen test `FUN_8005126C` reads
(`lui 0x801d` + `addiu -0x6c90`; a `0x801D9370` reading of that pair is the
sign of the `addiu` dropped). The clip id then follows the action class at
`actor[+0x1DE]`:

| Class | Clip id | Files |
|---|---|---|
| `1` | `party_slot + 0x19` | `XA27`..`XA29` |
| `2` | `party_slot + 0x19` when the [spell record](../../formats/spell-table.md) `DAT_800754C8[actor[+0x1DF]]`'s first byte is `< 0x14`, else `7` | `XA27`..`XA29`, else `XA8` |
| `3`, `4` | `(party_slot - 1) * 2` | `XA1` / `XA3` / `XA5` |
| `0`, `>= 5` | none - returns without arming or touching the latch | - |
| seat `>= 3` | `0x800787AF[DAT_8007BD09[seat]]` (the class byte is never read on this arm) | the monster-side table |

`FUN_8003EAE4(0, clip)` starts it and the id is written to `_DAT_8007BDB0`, so
the latch both records what is playing and blocks a second arm until something
resets it to `-1`. `see ghidra/scripts/funcs/8004da00.txt`.

### `80052FA0`

**Party-character battle-mesh assembler + CLUT decode** (SCUS). For each active party member (`DAT_8007bd10[char] != 0`) allocates a `0x19000` work buffer and LZS-decodes `record[0]` + the 5 equipment-selected player-file sections into it.

- **Palette half**: STP-copies the embedded CLUT structs to VRAM rows `481 + slot` via `FUN_80053B9C` (CLUTs are `[u16 base][u16 count][BGR555]` at `record[0]+4`/`+8` and each flagged sub-record's trailing offset; clean-room port `legaia_asset::battle_char_palette`, byte-exact vs live battle VRAM).
- **Mesh half**: builds the character's **merged battle TMD** at `ctx+0x50` (`ctx = *(0x801C9360 + slot*4)`) - writes magic `0x80000002` at `blob+0x18`, `nobj = 0` at `blob+0x20`, then calls `FUN_800536BC` once per section.
- **`FUN_800536BC` (the object splice)**: appends the section's 7-word TMD object entries with vertex/normal/prim offsets relocated into the merged pool, copies the data words, `nobj += section_nobj`, and writes one bone-id byte per object at `blob+0` from the section's attach list - surplus objects tagged `0xFF`/`0xFE` = the equipment visual meshes.
- **`FUN_80053898` (post-pass)**: retags `0xFF`→200/201, `0xFE`→100+, records each extra's attach bone at `blob+nobj`, selection-sorts the object table so extras land last. `FUN_800513F0` then registers `blob+0x18` into `DAT_8007C018[slot]`.
- **`FUN_80053a28` (TSB/CBA relocation)**: called by `FUN_800513F0` per party slot right after the registration - rewrites every textured prim's CLUT row to `481 + slot` (column preserved) and texpage index to `0x18/0x19 + 2*slot` (the runtime band `x ∈ [512, 896), y = 256`). Clean-room port `legaia_asset::battle_char_assembly::relocate_tsb_cba`; see [`formats/character-mesh.md` § Battle render](../../formats/character-mesh.md#battle-render-load-time-tsbcba-relocation).

Byte-verified against the full-party battle save (`nobj=17`, bone bytes `[0..14,200,201]`, attach `[5,8]`; every vertex pool matches its equipment section). See [`formats/character-mesh.md` § Battle form](../../formats/character-mesh.md#battle-form---assembled-from-the-player-files).

See [`formats/character-mesh.md` § Palette](../../formats/character-mesh.md).

### `80052770` / `800558fc` / `8003e8a8`

**Player-file loader (Vahn/Noa/Gala/Terra battle records).** `FUN_80052770` points each character's table entry at `data\battle\PLAYERn` and opens it via the dual-mode wrapper `FUN_800558fc(path, …, char+0x360)`. The retail ISO9660 branch `FUN_800608f0` is a **`trap` stub** on this build, so it always takes the debug branch → `FUN_8003e8a8(char+0x360)`, which reads `toc[idx+2]` (in-RAM PROT TOC `0x801C70F0`) as a **sector offset into `PROT.DAT`**: Vahn `0x361`→`0x36E8000`, Noa `0x362`→`0x3791000`, Gala `0x363`→`0x3828800`, Terra `0x364`→`0x3897800` (four contiguous player files; the extractor's `0861`/`0864`/`0865` slices begin at these regions). Case 4 selects, per descriptor section, an equipment-id-matched entry or the `id==0` separator (unequipped default).

### `800520F0`

Battle scene loader (SCUS). Sequential state machine (sub-state at `gp+0xa59`) that pulls the `befect_data` cluster (CDNAME 872) via the dual-mode loader (retail dev-path string / debug PROT index): case 0x8 loads `h:\prot\battle\etim.dat` (the **effect sprite texels**), case 0xB loads `etmd.dat` = PROT `0x36a` (**874**, the `befect_data` §0 pack) + `vdf.dat`, case 0xC walks that pack and registers every entry via `FUN_80026B4C` (asserts magic `0x80000002`) into the **effect/model window `DAT_8007C018[3..]`** (a running base index, NOT the party `[0..=2]`), then loads `efect.dat` (PROT `0x36b`/875) into `_DAT_8007BD5C`, case 0xE calls effect-bundle init `0x801DE914(0x1000, 0xA00)`.

**This loader does NOT install the party battle meshes** - those are assembled per character from the player battle files (equipment-id-selected sections; `FUN_80052770` case 4 → `FUN_80052FA0` → `FUN_800536BC`, see [`formats/character-mesh.md` § Battle form](../../formats/character-mesh.md#battle-form---assembled-from-the-player-files)) and are registered into `DAT_8007C018[0..=2]` by `FUN_800513F0` + `FUN_800542C8` (below), **not** an overlay. State `9` (`LAB_800526C8`) dispatches the just-loaded `etim.dat` pack to VRAM by calling `FUN_800198E0` (→ `LoadImage`) per pack entry. See [`formats/effect.md`](../../formats/effect.md#battle-effect-cluster-befect_data).

### `800513F0` / `800542C8`

**Battle-form party-mesh install** (SCUS; the writers of `DAT_8007C018[0..=2]` for a normal battle). Both register PROT 1204 battle meshes through the generic registrar `tmd_register` (`FUN_80026B4C`, store at `0x80026BA8`); pinned by a `DAT_8007C018[0..2]` write-watchpoint at battle entry ([`autorun_battle_party_mesh_install.lua`](../../../scripts/pcsx-redux/autorun_battle_party_mesh_install.lua); installed pointers byte-match the battle form, e.g. Vahn → `0x80165F48`).

**`FUN_800513F0`** (battle scene-loader state handler) registers the active-party meshes in a `while (i<3)` loop, **per-slot gated** by the active-member-ID array `DAT_8007bd10[i]` (`1`=Vahn/`2`=Noa/`3`=Gala/`0`=empty): `if (DAT_8007bd10[i] != 0) tmd_register(*(actor+0x50)+0x18, 0)` with `actor = *(0x801C9360 + i*4)` (active-actor table) - immediately after running the party-palette decode `FUN_80052FA0`. Vahn-solo (`[1,0,0,0]`) installs only slot 0 here; a full party (`[1,2,3,0]`) installs all three (confirmed against the `mc1`/`mc6`/`mc7` full-party battle save states, `DAT_8007C018[0..2]=0x80165E38/0x8017A908/0x8018D550`).

**`FUN_800542C8`** (battle archive loader) registers each additional party member in a per-member loop bounded by `*(rec+0x4a)` - `tmd_register(*(*rec+4), 0)`. Both are reached **indirectly** (battle state-handler dispatch), so a static cross-reference on `0x8007C018` finds no writer - which is why the install was long mis-assumed to live in an overlay. Dumps `funcs/800513f0.txt` / `800542c8.txt`.

### `80020050`

**Flame / effect-texture atlas loader (SCUS).** Uploads PROT entry `0x366` (870 - the flame TIMs) into VRAM **twice**, via `FUN_8001fc00(0x366, 0, region, pass, …)` (a PROT-index→VRAM wrapper that dispatches `FUN_8003e8a8`, the TOC resolver). The destination VRAM region is set up by `FUN_80017888(0, 0xf000)` / `FUN_8001e54c(0, region, 0xf000)` (the `0xf000` argument recurs in both passes), then `FUN_80017b94` finalises the VRAM upload. It also calls `FUN_8002630c(hdr, body, vab_id, 1, partial)` - the libsnd VAB-bank upload helper (`SsVabOpenHead` / `SsVabTransBody` / `SsVabClose`, ignore-listed), so the effect loader installs the flame effect's associated **sound bank** alongside the texture atlas; it is not a second VRAM blit.

Gated on `_DAT_8007b868 == 0` (the same field-camera / mode gate `FUN_801dbe9c` reads). This is the VRAM blit site for PROT 870 - it is **not** loaded by the battle-bundle path `FUN_800520F0` (which pulls `0x367..0x36d`). See [`reference/open-rev-eng-threads.md`](../open-rev-eng-threads.md).

### `800421D4`

**Inventory add (find-or-insert):** `(id, amount) -> slot`. Scans the active window `gp[+0x2D2]..gp[+0x2D4]` of `_DAT_80085958` for an existing `id` stack; if absent, scans again for the first empty slot. Adds `amount` to the count, **caps the stack at 99**. BOUNDS NOTE: the **id write** `(&DAT_80085958)[slot*2] = id` (disasm `sb t0,0x1818(a0)` @ `0x800422BC`) is **unconditional and precedes the bound check** - only the count write at `+1` is guarded by `slot < gp[+0x2D4]`. When the window is full the second scan returns `slot == gp[+0x2D4]`, so the id byte is written **one slot past the window** (`0x80085958 + gp[+0x2D4]*2`) with the added item's id as the value. Whether normal play can reach a full-window add is a separate question; the helper itself does not bound the id store.

Callers - battle-loot reward writer `FUN_8004F0E8`, table item-give `FUN_8004AD80`, and the field item-give pair `FUN_801D71F0` / `FUN_801D7210` (overlay 0897) - do not visibly pre-check inventory room before the add, so the OOB is plausibly reachable via an item drop with a full bag (capture-confirm pending). NB this `FUN_801D71F0`/`_D7210` pair is the one earlier mis-cited as a "status-effect timer tick" on `FUN_80043048`; both are inventory item-give callers.

The full set of unchecked call sites (each a candidate full-bag trigger) is catalogued as `legaia_save::retail_inventory::AddHelperCaller`: battle-loot reward `FUN_8004E568` (adds at `0x8004F380`/`0x8004F608`), shop buy-confirm `FUN_801C36B0` (variable quantity, catalog id `rec+8`), captured-monster pay `FUN_801F138C` (`actor[+0x1DF]`), one-shot minigame reward `FUN_801D0F60` (fixed id `0xCD`), and equip swap-back refund `FUN_8020E748`/`FUN_801E01F0`.

The clean-room model surfaces the full-bag case as `AddOutcome::OobIdWrite { oob_target, written_id }`; the written byte is the added id and is **independent of quantity** (only the count store is guarded). Live confirmation probe: `scripts/pcsx-redux/autorun_inventory_oob_writer.lua` (watches `0x800859E8` for a store from the id-store PC `0x800422BC`).

### `8004E568`

**Battle-end reward resolution** (5984 B, spans `0x8004E568..0x8004FCC8`). The post-battle spoils routine: it accumulates the formation's gold into the party purse `_DAT_8008459C` (saturating cap `99999999`) and awards item drops by calling the inventory-add helper `FUN_800421D4` (at `0x8004F380` and `0x8004F608`), reading the active actor record at `gp[+0xA0C]` and gating on `gp[+0x332]`. It drives its multi-step sub-overlay loads through `FUN_80025358`. After dividing the monster XP pool by the alive-party count (`divu` at `0x8004F198`) it calls the **level-up applier `FUN_801E9504`** at `0x8004F34C` (`jal`, arg = active-party slot − 1) per surviving member.

**`FUN_8004F0E8`** - cited elsewhere as the "battle-loot reward writer" (e.g. the `FUN_800421D4` caller note above) - is an **in-body block address of this function**, not a separate entry; the reward writer's true entry is `0x8004E568`. The gold/EXP **scaling** (gold `>>1` per enemy + optional +25% ability bonus + halve; EXP `×3/4` then ceiling-split) is ported as pure kernels in `legaia_engine_vm::battle_formulas` (`victory_gold_per_monster` / `victory_gold_finalize` / `victory_exp_per_member`), wired into `engine-core` `World::apply_battle_loot` / `apply_battle_xp`.

### `801E9504`

**Level-up applier** (overlay-resident; `(party_slot) -> void`). Operates directly on the persistent character record. Reads the static-SCUS per-level XP-delta table `&DAT_80076AF4` (u16), sums it to the current level, scales it (`(sum × 9999999) / 0x140FE` for `level < 0x11`, else `sum × 0x79`) with a per-character ± correction for slots 1/2, and runs a `do…while (threshold ≤ record cumulative XP)` loop (`sltu` at `0x801E9714` / `0x801E9F70`) - each iteration bumps the record level and applies one round of stat growth.

**Stat growth** grows 8 stats at record `+0x6E4..+0x6F4` from two static SCUS tables: the per-stat 98-entry curves at `DAT_800769CC` (`addiu s4,v0,0x69CC`, stride `0x62`, indexed by `level-1`) and the **per-character** parameter block at `DAT_80076918` (`addiu a0,a0,0x6918`, stride `0x3C`) - 8 contiguous 6-byte sub-records `{u16 start, u16 max, u8 jitter, u8 row}`, `start` = base stat (validated: Gala matches the new-game template on all 8 stats), `row` selects the curve. Per-level gain (`0x801E9758..0x801E97F8`) = `max(1, (max-start)×curve[row][level-1]/0x24C0 + rand()%(2×jitter+1) − jitter)`, then caps (HP ≤ 9999, MP ≤ 999, AGL ≤ 0x118). The divisor `0x24C0` is the curve normalizer (each curve sums to `0x24C0`, so growth accumulates to exactly `max-start` by L99).

The canonical XP/growth source (supersedes the falsified `0x8007123C` / sin-LUT-slice readings).

**Validated** byte-exact against a single-level capture (Noa L2→L3: all 8 stat deltas within the core ± jitter band) - see [`subsystems/level-up.md`](../../subsystems/level-up.md) § Stat gains. Decoded structure in `legaia_asset::level_up_tables`; the engine drives all 8 stats from it (`LevelUpTracker::with_growth_tables` + `BootSession`, write-then-mirror into the live window). Only the per-level `rand()` jitter stream is left for a bit-exact port. Called from the reward resolver `FUN_8004E568` at `0x8004F34C`. Dump: `overlay_battle_action_801e9504.txt` (aliased into `overlay_magic_level_up` / `_magic_capture` / `_muscle_dome`).

### `801DD0AC`

**Magic/summon damage calculator** (battle overlay; 1028 bytes / 257 instr). `(u32 move_type, u8 attacker_slot, u8 defender_slot) -> i32 damage`. Resolves attacker/defender actors via the [8-slot actor table](../../subsystems/battle.md) `DAT_801C9370[slot]`. Two branches keyed on `attacker_slot`:

**`== 7` (summon path)** - roll = `rand % (INT@+0x168 + 1) + HP@+0x14c + DAT_801C9370[ctx+0x13]_INT * 2`; **`!= 7` (arts/physical)** - reads a 26-byte-stride per-move power table at `0x801F4F5C` indexed by `move_type` (`move_type*0x1a + 0x801F4F5C`), folding `power` and `power>>2` terms with caster INT/HP (the `+0x168` stat = record `+0x18`). Both subtract a defender-mitigation term built the same way from the defender's INT/HP/DEF and `return roll - mitigation` (after `FUN_801ddb30` finalize).

The **per-summon Seru-magic overlays call this** from their HP applier - PROT 0904/0912/0914 (damage) pass `(move_type=0x10..0x12, attacker_slot=7, target)`, clamp to current HP, and `HP -= result`; PROT 0903/0905/0910/0911/0913 (heal) instead apply `(power_byte<<5)+0xe0` inline (see [`formats/spell-table.md`](../../formats/spell-table.md#per-spell-damage-power-is-not-static-data---it-is-caster-state-derived)). So summon "power" is caster/summon-state-derived, not a static per-spell scalar; the only true per-move power scalar is the `0x801F4F5C` arts/physical table (it feeds melee/arts, **not** magic) - now located + parsed off the disc as `legaia_asset::move_power` (static battle-overlay data, PROT 0898 file `0x26744`).

`move_type` (`param_1`) is **not** the raw move id: it is `map[actor[+0x1df]]` from a 128-byte id→index map at `0x801F4E63` (the setup site `FUN_801DEA50` caches the resolved record at `ctx+0x1014`, and `FUN_801E09F8` passes `param_1 = byte_at(actor[+0x1df] + 0x801F4E63)`). The record's `+0` is power; `+0x04` seeds the action-timing counter `ctx+0x6c6`; `+0x0d` is a sound-cue id handed to `FUN_8004FCC8`. Joining the move-id space to the [spell table](../../formats/spell-table.md) labels the records: ids `0x25..0x74` = the named monster special-attacks (Tail Fire `0x27`, …), ids `0x04..0x1f` = the unnamed internal enemy-attack tiers.

The closed-form roll + scale stages of **both** branches are ported as pure kernels in `legaia_engine_vm::battle_formulas` - the summon branch as `summon_predamage` and friends, the arts/physical branch as `arts_attacker_roll` / `arts_bonus_roll` / `arts_physical_predamage` (defender roll shared); the `FUN_801DD864` scaler and `FUN_801DDB30` finisher it calls have their own rows below. Dump: `overlay_battle_action_801dd0ac.txt`.

### `801DD864`

**Damage-roll scale stage** (battle overlay; 716 bytes / 179 instr). `(byte attacker_slot, byte defender_slot, uint* atk_roll, uint* def_roll)`. Modifies the two rolls from `FUN_801DD0AC` in place. Resolves each side's element (party slot `< 3` → SC element byte `*(0x801F547F + DAT_8007BD10[slot])`; else the monster actor's `+0x1D`) and scales `*atk_roll *= affinity / 100` from the 8×8 byte matrix at `0x801F53E8` (`affinity[atk_elem*8 + def_elem]`; 100 = neutral, 200 = weakness, 0 = immune). Then applies the attacker's status bits `+0x16E` (`0x1` → 9/10, `0x2` → 7/10), the defender guard `+0x1DE == 4` → `*def_roll <<= 1`, and the defender's status bits the same way.

For the **summon** case (`attacker_slot == 7`) it adds the per-character magic-power tail `*atk_roll += *atk_roll * (power_byte - 1) >> 3`, with `power_byte` from the SC-block table `0x80084140 + char*0x414 + 0x729` matched against the cast spell-id at `+0x705`. Ported (affinity / status / magic-power helpers) in `legaia_engine_vm::battle_formulas`. Dump: `overlay_battle_action_801dd864.txt`.

### `801DDB30`

**Damage-roll finisher / committer** (battle overlay; 3556 bytes / 889 instr). `(byte attacker_slot, byte defender_slot, uint* atk_roll, uint* def_roll, int flag)`. The deeply-coupled tail that turns the scaled rolls into committed battle state.

In order: per-element **resistance** bits read from the defender's SC ability words (`+0x6BC` / `+0x6C0` on the `0x80084140`-based record) halve the margin `*atk − *def` when the attacker's element index (`actor+0x1D`) matches the bit; a guaranteed `rand % 9 + 8` floor when `*atk ≤ *def`; the **summon power-percent re-scale** (`attacker_slot == 7`: `margin = margin * pct / 100`, `pct = table[(caster_char_id - 1) * 8 + attacker_element]` from the per-caster table at `0x801F5468` - PROT 0898 file `0x26C50`, parsed as `legaia_asset::element_affinity::ElementAffinity::summon_power`); the **9999 damage cap**;
the **spirit-gauge** fill at defender `+0x170` (`+ margin/4` or `/10` per `+0x6C0` bits `0x200`/`0x100`, clamped to 100) plus the death-flag (`+0x16E & 4`) instant-kill arm; MP-drain / spirit-field accumulators; and a stat-**debuff** switch on the global field type `*(DAT_801C9358 + 0x1D)` (cases 0..6 each shave a defender stat in `+0x15C..+0x16A` / `+0x150` / `+0x156` / `+0x158` by `stat * _DAT_801F6960 / 100`).

**Closed-form finalisation arithmetic ported** as pure kernels in `legaia_engine_vm::battle_formulas`:
`damage_finish` (the six damage-rewrite stages - elemental-resistance halving / guard halve / `rand%9+8` floor / summon power-% scale / 9999 cap),
`spirit_gauge_fill` (the gauge accrual + the two gain-up bits), and
`summon_spell_xp_gain` (the **spell-XP accrual tail**, `attacker_slot == 7` only: the cast spell's slot in the caster record's `+0x13D` id list gains `damage * 12 / target_max_hp` per single-target hit / `* 4` group-target, flat `12`/`4` on a kill, nothing from a target below 2 HP, into the `+0x8` u32 XP array - the XP `FUN_801E70BC` then checks; gates `_DAT_8007BAC0` no-reward + `_DAT_8007BDB8` unmodelled),
all unit-tested. The state-mutating tail (damage-popup accumulator, AI revenge table, MP drain, the per-element stat-**debuff** switch) reads/writes ~20 battle globals and stays in the live battle context (see the `battle_formulas` boundary note). Dump: `overlay_battle_action_801ddb30.txt`.

### `801E295C`

**Battle action state machine** - `ctx[7]` dispatch, `+0x1DE` sub-state. 16 KB / 4099 instructions / 155 outgoing calls (the largest function in the battle overlay). Outer switch on `_DAT_8007BD24[7]` (the action-state cursor; 47 cases across bands `0x14`/`0x28`/`0x32`/`0x3C`/`0x46`/`0x50`/`0x5A`/`0x64`/`0x68`/`0x6E`); inner switch on `actor[+0x1DE]` (action category 0..5 = Martial-arts / Item / Magic / Attack / Spirit / Run). Reads battle actor pointers via `(&DAT_801C9370)[ctx[0x13]]`; ramps frame-timer at `ctx[+0x6D8]`; queues animations via `actor[+0x1DA]` and waits on `actor[+0x1D9]` to converge. Battle-end signalled via `DAT_8007BD71 = 0xFE`.

The global pseudo-action `case 0xFF` increments the battle-mode counter `_DAT_8007BD24[0x28A]` (the boss-phase gate the AI picker `FUN_801E9FD4` reads; ported as `engine-core::World::advance_battle_mode`). Cross-refs: `FUN_8004E2F0` (range/LOS, called from `0x14`/`0x16`/`0x19`), `FUN_80042558` ability bitmask (read indirectly via character record at `0x80084708 + (party_id-1)*0x414`), effect spawn via `FUN_801D8DE8` → `FUN_801DBF9C` → `FUN_801DFDF8`, pose driver `FUN_801D5854(actor, pose_id)` (~30 call sites). See [`subsystems/battle-action.md`](../../subsystems/battle-action.md). Captured from an action-menu-open save state as `overlay_battle_action_801e295c.txt`.

### `801E9FD4`

**Monster-AI action picker** (battle overlay; the magic-capture-overlay dump at the same address is a different routine). Called per monster from `FUN_801DABA4`. Generic core: rolls `rand % (1 + live_magic_count)` over the record's `+0x21..=+0x23` global magic ids → physical strike or a cast (gated on MP `actor[+0x150]` vs `spell_table[id*0xC+3]`), target by shape `spell_table[id*0xC+2] & 0x60`. Then a `switch` on `DAT_8007BD0C[slot]` overrides with scripted casts. `DAT_8007BD0C[slot]` is the **per-slot monster id** (`FUN_801DA51C` copies the encounter record's `[+4+slot]` ids into it), so each `switch` case is bespoke AI for a specific monster id - not an abstract AI-type. Writes `actor[+0x1DD]` (target/class), `+0x1DE` (action kind), `+0x1DF..` (chosen id / action chain queue).

Generic core ported as `engine-core::World::pick_monster_action`; the per-monster-id switch + recent-target ring ported as `engine-core::monster_ai` (`decide` / `apply_recent_target_ring`, over `MonsterAiState`). `overlay_battle_action_801e9fd4.txt`.

### `801EC0DC`

**Monster escape roll.** `(slot) -> bool` - the enemy-side mirror of the party
escape roll [`FUN_801E791C`](../../subsystems/battle-formulas.md#run--escape-roll---fun_801e791c),
called from the AI picker `FUN_801E9FD4`. Reads the same `ctx[+0x287]` no-escape
gate the party roll's failure arm tests and returns "no" outright when it is set.

```text
monster_sum = SUM over live monster slots:  maxHP + curHP>>1 + ATK
for each party slot:
    curHP == 0  ->  monster_sum <<= 1
    else        ->  party_sum += maxHP>>3 + curHP>>4 + ATK>>3
                    blocked |= record[+0xF8] & 0x400000
party_avg   = party_sum / party_count      +  (target.maxHP - target.curHP) >> 5
monster_avg = max(monster_sum / monster_count, (party_avg * 3) >> 1)
spread      = max(monster_avg - target.INT * 2, 1)
flee  iff   monster_avg + rand()%spread  <  party_avg + rand()%(party_avg + target.INT)
            and rand() & 7 == 0 and !blocked
```

Three things pin the direction of the compare, and they agree: a wounded monster
flees more easily (its own missing HP is added to the side it has to beat), a
winning monster flees less (each downed party member doubles the monster side),
and the blocking ability bit is `record[+0xF8] & 0x400000` = accessory-passive
index `0x36`, **No Escape** / Chicken Guard, whose in-game text is "enemies can't
escape" (see [`accessory-passive-table.md`](../../formats/accessory-passive-table.md)).
The flat `rand() & 7` gate makes a flee at most a one-in-eight event even when
the scores allow it. Stats are the actor block's ATK `+0x158` and INT `+0x168`
(see [`battle-formulas.md`](../../subsystems/battle-formulas.md)).

Ported as `engine-vm::battle_formulas::monster_escape_roll`;
`see ghidra/scripts/funcs/overlay_battle_action_801ec0dc.txt`.

### `801DF570`

**Attack-approach distance clamp.** `(slot, requested) -> i16`. Resolves the
acting actor and its target (`+0x1DD`), takes the bearing between them through
`FUN_80019B28`, adds a half turn (`+0x800`, masked to 12 bits), and projects the
separation:

```text
d = |(|actor.x - target.x| * sin[a]) >> 12| + |(|actor.z - target.z| * cos[a]) >> 12|
r = requested
if d  <u r      { r = d }        // cap at the separation
if r  <u 3d/4   { r = 3d/4 }     // floor at three quarters of it
```

Each of the four magnitudes is its own `bgez`/`negu` pair - the coordinate
deltas are absolutised *before* the multiply and the two products *again* after
the shift, so one axis can never cancel the other. Both clamp compares are
**`sltu`**, and `requested` arrives sign-extended from a halfword, so a negative
request compares as a huge unsigned value and takes the cap arm rather than the
floor arm.

A clamp whose output is confined to `[3d/4, d]` cannot close the last quarter of
an approach on its own, which is worth noting alongside the `0x19`
[approach-park](../../subsystems/battle-action.md#state-table) thread.

Ported as `engine-vm::battle_approach`. Read the mapped image, not a dump: the
`overlay_0897` slice at this VA reports 94 instructions against the
battle-action image's 82.

### `801D84C0`

**Battle party-name panel build + teardown**, with `FUN_801DBB8C` as its opening
half. The pair is fixed by shared state: `FUN_801DBB8C` writes the label-actor
block at `0x801F4E08` (`+0x01 = 0x80`, `+0x00`/`+0x02` cleared), publishes the
active participant id minus one at `0x8007BB8C`, registers a text actor via
`FUN_8003541C(0, 0xC, 0, -0x92, 0x24, 0x8A, 0x90, 3)` and stores the handle at
`+0x04`; `FUN_801D84C0` ends by clearing `+0x01`, `+0x02` and that handle - and
notably **not** `+0x00`.

`FUN_801D84C0` forks on the *second* party slot's participant id
(`DAT_8007BD11`). Zero takes a solo arm sourcing every buffer from the first
slot's name; non-zero takes a roster arm that sources three of four from fixed
strings and measures each with `FUN_8003CBF8(buf, 0xC1, 1)`, storing
`participant_id - 1` at the returned offset. Either way it then publishes the
four label buffers (`ctx+0xA9 / +0x129 / +0x159 / +0x189`) into the
screen-element placement table `0x80076C10`, resets **all three** party actors to `+0x1DD = 3` (target the
first monster) and `+0x1DE = 0` (Martial Arts), writes each portrait cell as
`participant_id + 0x32`, and anchors the panels by party size:

| Party size | Primary X | Secondary X |
|---|---|---|
| 1 | `0x72` | (not written) |
| 2 | `0x3F` | `0xA5` |
| 3 | `0x0C` | `0x72` |

A solo member sits centred and a pair splits outward - the same centring rule
the field VM's member picker `FUN_801F1278` uses, arrived at independently.

**The name pointer confirms the save record.** Both arms resolve a member's name
as `0x8008459B + id * 0x414`, which is exactly
`0x80084708 + (id - 1) * 0x414 + 0x2A7` - the live character record's display
name at the offset [`save-record.md`](../../formats/save-record.md) documents.

Ported as `engine-vm::battle_party_panel`. Read the mapped image: the
`overlay_0897` dump at `801D84C0` holds 212 instructions against the
battle-action image's 259, and the one at `801DBB8C` is a four-instruction
label-call slice leaving via `j 0x801EA7AC` rather than a function at all.

### `801CE844`

**Game-over overlay init.** PROT 0902 at slot-A base `0x801CE818`, entry = file
`+0x2C`; called by mode-18 `GAME OVER INIT` `FUN_80025B30` after
`FUN_8003EBE4(7)`. Retail-unreachable - nothing statically writes mode 18 - so
this is a dev harness, and worth having as the smallest complete example of the
overlay-init shape. Base pinned by the SCUS `jal`, the `+0x2C` prologue and
in-file string anchors (see the static-overlay map's 0902 row; its old slot-B row
was a `pointer_resolution` false positive -
[`static-overlay-pipeline.md`](../../tooling/static-overlay-pipeline.md)).

**Read it out of the `0902` image, not `0898`.** The VA falls inside the
battle-action image's footprint too, and the dump taken there reports `NOFUNC`
with a garbage decode window. The `0902` copy has the clean `addiu sp, sp, -0x58`
prologue: 193 instructions, `see
ghidra/scripts/funcs/overlay_0902_xxx_dat_801ce844.txt`.

Three phases, and only the third is renderer-free:

1. **Reset + stream.** GPU/heap resets (`FUN_8001DAF8`, `FUN_8001DCF8`,
   `FUN_80058068`, `FUN_8001E3B8`, a `0x32000`-byte `FUN_80017888`), game mode
   `_DAT_8007B83C = 0x13` (GAMEOVER MODE), counter seed `_DAT_800840C0 = 3000`,
   and a `gameover.pak` load forking on `_DAT_8007B8C2` between the dev-host path
   (`FUN_8003E6BC`) and the retail CD path (`FUN_8003EB98(1, buf, 1)`).
2. **Pak walk.** A `[u32 tag][…]` chunk loop over the loaded pak, dispatching
   kind `1` to `FUN_800198E0` (per entry, plus a nested `[count][offsets]` table)
   and kind `2` to `FUN_80026B4C`. Host-side asset installation.
3. **The banner stager.** Nine fixed slots on a line, one child actor per
   non-blank slot, each seated through `FUN_80021B04` on a **shared** move record
   whose `model_sel` the loop rewrites per letter (`sh v0, 0x0(s5)` in the `jal`'s
   delay slot). `model_sel = glyph_byte - 0x3F`; scale `0x1000`; the child's
   `+0x60` takes the letter ordinal and `+0x54` the move-VM wait timer. The loop
   retires with `_DAT_8007B6F4 = 0x140`.

Two details of phase 3 come from delay slots and are lost in the C rendering. The
pen advance `addiu $s3, $s3, 0x1c2` sits in the blank test's delay slot, so it
runs on **every** slot including the skipped one - which is what keeps the two
words of the label evenly spaced rather than butted together. The stagger
accumulator `addiu $s1, $s1, 0xf0` sits inside the spawn arm, so the wait timers
count **letters**, not slots. The pen is symmetric about zero by construction:
nine slots at `0x1C2` from `-0x708` give `-1800..+1800`, centre slot on the
origin.

Phases 1 and 2 are a deliberate non-port - host emission (GPU state, heap, CD
reads, asset install) with no arithmetic of its own. Phase 3 is ported as
`engine-vm::gameover_banner`, which takes the label bytes as an argument rather
than carrying them.

### `801F30C4`

**Move-VM opcode `0x17` - the battle-side escape.** `(actor, mode)`. The first
argument is an actor: the caller is `FUN_80023070` case `0x17` and `mode` is that
instruction's single operand, making this the exact sibling of the field escape
`0x2F` ([`move-vm-overlay-ext.md`](../../subsystems/move-vm-overlay-ext.md)).
It seats **twelve child actors** - four iterations round the compass, three spawn
blocks each - on one of two static move-VM stager records in `0898`'s tail.

The span is `0x801F30C4..0x801F398C` - 563 instructions, ending exactly where the
cast audio-cue dispatcher `func_0x801F3990` begins (`0x801F3988` is the `jr ra`,
`0x801F3990` a clean `addiu sp, sp, -0x20`). **`disasm-overlay-fn.py` historically
could not read it**: that tool stopped at the first unconditional `j` and reported
18 instructions here, so use raw capstone over the mapped `0898` image at base
`0x801CE818`.

**The entry is one loop written twice.** A three-way fork on `mode` (`0`, `1`,
and a fall-through that returns immediately) reaches two loop bodies of 260
instructions each. Diffed instruction by instruction, twelve differ, and three of
those are only the loop-latch shape (arm `0` exits on `beqz` and jumps back, arm
`1` falls through on `bnez`). The nine real differences are three constants
repeated once per spawn block:

| | arm `0` | arm `1` |
|---|---|---|
| stager record | `0x801F5DA4` | `0x801F5D0C` |
| cosine divisors | `/48`, `/72`, `/96` | `/96`, `/144`, `/192` |
| tail offsets | `+0x70`, `+0xA8`, `+0x38` | `+0x30`, `+0x48`, `+0x18` |

Two exact relations follow: every arm-`1` cosine divisor is **twice** its arm-`0`
counterpart (the same magic multiply with one extra `sra`), and every arm-`1`
tail offset is exactly **3/7** of its counterpart. So `mode` selects the same
burst at a smaller radius, not a different effect.

Per spawn block: copy eight bytes from `actor[+0x24]` - the rotation triple - to
a stack scratch, fold `sin[angle] / 2^n` plus a bounded jitter into the scratch's
second halfword, call `FUN_80050ED4(actor + 0x14, scratch, record, scale)`, then
write `cos[angle] / d + jitter` to the child's `+0x3E` and a second jitter to its
`+0x98` (`sh v0, 0x18(s0)` after `addiu s0, s0, 0x80`). Block `0` indexes the LUTs
on the four cardinals (`sll $s1, $s2, 0xb`, unmasked); blocks `1` and `2` share
the diagonals (`(iteration * 1024 + 512) & 0xFFF` - only this arm masks), block
`2` reusing block `1`'s index register.

The jittered scratch halfword is `param_2[1]`, which `FUN_80021B04` masks to 12
bits into the child's `+0x96` - the rotation-LUT index move-VM op `0x03` reads.
The value **is** the child's heading, modulo the 4096-step circle.

**The scale argument is per block, not per arm.** Blocks `0` and `1` load
`actor[+0x72]` with `lhu` and pass `>> 1`; block `2` loads it with `lh` and passes
it unshifted (its `jal` delay slot carries `move $a1, $s5` instead of the `srl`).
Both arms agree, so blocks `0`/`1` spawn at half the parent's scale and block `2`
at full.

**Reciprocal divides: fourteen distinct (magic, shift, divisor) triples, all
verified against plain truncating division.** The shift is the part that gets
dropped: `0x2AAAAAAB` is `/6` read bare, `/48` with its `>> 3`, `/96` with `>> 4`
and `/192` with `>> 5` - all four appear in this one function. `0x88888889` is the
signed magic-with-add form (`mfhi`, `addu` the original, `sra 3`) and needs signed
arithmetic to reproduce; it is `/15`. All are used as `x - (x / d) * d` except the
three cosine divides.

#### The two records, and how the burst is reached

`0x801F5DA4` and `0x801F5D0C` are **move-VM stager records**, not tables:
`[i16 model_sel][u16 flags][move-VM bytecode]`, the format
[`move-vm.md`](../../subsystems/move-vm.md#move-buffer-record-sources) documents
for every move-buffer source. Both are transform-node records terminating at op
`0x08` HALT, both run the same instruction sequence - a render-mode-2 child spawn
(op `0x23`) followed by a strictly alternating `WAIT_SET` / sprite-add strip - and
the two differ in **exactly one halfword**, operand 8 of that `0x23`, which lands
in the child's `+0xB2`. What `+0xB2` means under render mode `2` is open; the
ported actor tick names `+0xB0`/`+0xB2` for the mode-`5` SFX-emitter arm, which is
a different mode.

Each record is preceded in the tail by an 18-byte trigger of the shape
`WAIT_SET 0 / 0x17 <mode> / WAIT_SET 0 / HALT`, whose operand matches the arm
whose record follows one alignment word later (`0x801F5D90` → mode `0`,
`0x801F5CF8` → mode `1`). Those two addresses are cited on
[`level-up.md`](../../subsystems/level-up.md) as "binary animation tables passed
to particle spawner `FUN_80050ED4`" - they are neither tables nor direct callers
of the spawner. They are move programs, and it is the `0x17` inside them that
reaches it.

`FUN_80050ED4` is **not** a boundary either: it is decoded (`see
ghidra/scripts/funcs/80050ed4.txt`) - a 23-instruction scan of the 0x60-slot
pointer pool at `DAT_801C90F0` that forwards the same four arguments to
`FUN_80021B04`, sign-extending the low halfword of the fourth, stores the returned
actor pointer in the first null slot and returns it, or returns `0` when all 96
slots are taken. The port catalog carries it as subsumed glue.

Ported as `engine-vm::battle_burst`, including a record parser
(`BurstRecord::parse`) that slices an arm's record out of a supplied `0898` image
and walks its extent with the ported move-VM dispatcher rather than restating the
opcode sizes. The records are disc data; none of their bytes are reproduced, and
the structural claims above are asserted by the image-gated
`battle_burst_real_records` test.

### `80048A08`

**Battle per-actor draw.** `(actor)`. The per-frame draw for every battle actor (monster bodies, party, AND the player Seru-summon parts): loads the actor base matrix (`FUN_80026988`), runs the per-object rigid-TRS keyframe decoder `FUN_8004998C` (see [`monster-animation.md`](../../formats/monster-animation.md)), then for each TMD object composes a per-object Euler via `RotMatrixX/Y/Z` (`0x800461A4`/`629C`/`638C`) and emits through the cluster-A renderer `FUN_80043390`. Walks the actor `+0x44` mesh-table (`[u32 count, u32 group_desc_ptr[count]]`, 0x1C-byte group stride) and reads the monster-anim archive at `*(actor+0x4C)+0x88`. Ported as the battle draw in `crates/engine-vm/src/anim_vm.rs` (`// PORT: FUN_80048A08`).

**Live trace - player Gimard "Burning Attack" cast (scenarios `gimard_summon_start`/`_visible`/`_burning_attack`, Vahn solo): this is the path that draws the summon - `FUN_80048A08`→`FUN_80043390` fires 35-64×/frame, while the summon-rotation candidate `FUN_801F7088` fires 0× and the move VM `FUN_80023070` only 2-3× (not a per-part driver). The player summon is posed exactly like a battle monster (per-object rigid TRS keyframes), NOT the move-VM / `FUN_801F7088` camera+local-Euler path.** `see ghidra/scripts/funcs/80048a08.txt`.

### `801D02C0`

**Procedural battle ground grid.** Two GTE passes over a
`_DAT_1F8003F8 x _DAT_1F8003FA` cell grid, cell pitch `0x200`, sub-step `0x100`.
Grid origin is `x_min = -(w>>1) * 0x200` and `z_min = -(h>>1) * 0x200 - 0x200` -
centred in X, biased a whole cell toward the camera in Z.

Pass 1 `RTPS`-projects one probe per cell at `(x_min + 0x100 + col*0x200,
z_min + 0x100 + row*0x200)` and stores a class byte in the `0x1000`-byte buffer
`_DAT_8007B814`: `-1` when `IR3 + 0x200 <= 0` (behind the near plane), `0` when
that biased depth exceeds `0x6700` (too far), `1` otherwise. Pass 2 draws the `1`
class only.

Pass 2 `RTPT`-projects a **3x3 lattice** per visible cell (three rows of three at
the sub-step) and emits **four** `POLY_GT4` sub-quads - the cell subdivided 2x2 -
after a four-corner screen reject against `0x140 x 0xF0` (keep iff some corner is
inside each edge). Sub-quad `(row, col)` takes sub-tile `row*2 + col`, and the
four sub-tiles are the four `0x20 x 0x20` corners of the `(192..255)^2` UV window
in scan order (u rising with the column, v with the row), CBA `0x77C0`, tpage
`0x000D`, `0x34` bytes per prim.

Two readings this corrects. **The sub-tiling is deterministic**: a single 64x64
texture is stretched over one whole `0x200` cell as four quads - no cell picks a
single sub-tile and nothing about the choice is random. (The random corner mirror
does exist in the battle overlay, but it belongs to the *particle* scatter
`FUN_801E0080`, not here.) And **`Y` is the sign bit of `X`**: every vertex is
loaded as `mtc2 <x_word>, VXY<n>`, so the GTE's `VY` half takes the upper 16 bits
of the same sign-extended word - `0` for `X >= 0`, `-1` otherwise. That is the
exact sense in which the plane is "flat at `Y ~ 0`"; there is no per-cell Y.

The dump's C is not usable alone for this routine: it renders the GTE traffic as
`setCopReg`/`getCopReg` with raw immediates, drops which shifted scratchpad slot
each store lands in, and carries an
`Instruction at 0x801d06ec overlaps 0x801d06e8` warning where a branch delay slot
doubles as a jump target. Ported (CPU side) as `engine-vm::battle_ground_grid`;
`see ghidra/scripts/funcs/overlay_battle_action_801d02c0.txt`.

### `801E0080`

**Battle-arena emitter-driven sprite scatter.** Gated on `DAT_8007BD58 != 0` and
`DAT_8007BD71 == 0xFF` (battle live, no end signal). Two pools off the battle
scene buffer `_DAT_8007BD30`: 32 **emitters** at `+0x1010`, `0x1C` stride, and
128 **particles** at `+0x10`, `0x20` stride.

An emitter holds a spawn count (`+0x00`, zero = inactive), a spawn counter
(`+0x02`), a delay countdown (`+0x03`), a 12-bit heading (`+0x04`), a base
position (`+0x08`/`+0x0C`/`+0x10`) and a script cursor (`+0x18`). When the
countdown expires it scans the 128 particle slots for one whose `+0x00` is zero
and seeds it: type and lifetime from the definition the pointer table at
`_DAT_8007BD30+8` resolves, mirror flags from `rand() % 4`, the base position
copied through, the script's planar offset rotated by the heading (`>> 4`) and
its velocity pair rotated the same way (`>> 0xC`) - while the offset's Y is
*subtracted* scaled by `0x100` and the velocity's Y is copied unrotated.

Countdowns drain `-8` floored at zero. Positions integrate as
`pos += ((vel * script_speed * scene_scale) << 3) >> 15`. The two script advances
are **not** the same shape: the emitter reads its next delay byte at
`cursor + 1` *before* advancing 14 bytes, the particle advances 6 bytes *first*
and reads at the new `cursor + 1`.

The render pass emits one `0x28`-byte textured quad per live particle with a
brightness ramp: `total >> 3` is the fade-in length, below it the level is
`((steps+1) << 7) / ramp` and at or above it `((total - steps) << 7) /
(total - ramp)`, clamped at `0x80` by an **unsigned** `sltiu 0x81`. The level is
splatted into all three colour bytes and summed with `0x2E000000`. The mirror
bits are negative logic: with `mirror == 0` the *high* U lands in corners 0 and 2
and the *high* V in corners 0 and 1.

The whole emitter+particle update repeats: a pass that touched a live countdown
costs `1`, an idle pass costs `5`, and passes run while the accumulated cost is
below `DAT_1F800393`. Ported as `engine-vm::battle_scatter`;
`see ghidra/scripts/funcs/overlay_battle_action_801e0080.txt`.

### `801F0450`

**AI-side Arts command assembler** - the counterpart to the player queue-builder
`FUN_801EED1C`. Two arms, chosen by the character record's `+0xF8 & 0x2000` and
the actor's `+0x16E & 0x404`:

- **Bit set, status clear** - the auto-fill arm. Writes category `+0x1DE = 3`
  (Attack), rolls a target `rand() % monster_count + 3` and pushes it through the
  dead-target redirect `FUN_801DB124`, then loops: a stop roll (`rand() % 7 == 0`
  ends it), an index roll over the character's learned-arts list
  (`record[+0x185]` count, `record[+0x186 + i]` ids), and a floor test - a list
  entry below `6` for participant id `2` or below `4` otherwise is discarded and
  the slot re-rolled, otherwise `id + 0x1B` is appended. Stops at 15 entries.
- **Otherwise** - the pool arm. Scans arts commands `0x0C..=0x0F`, tracks the
  cheapest `+0x74` AP cost, gives each command a weight from a `8 -> 1 / 4 -> 2`
  ladder over two byte bands selected by the target monster's type byte
  (`+0x1E`: type `3` uses `..=0x10` and `0x16..=0x1A`, type `2` uses `0x11..=0x15`
  and `0x1B..=0x1F`, anything else leaves every weight at `8`), zeroes the weight
  when the command's guard mask at `0x801F672C` intersects the actor's `+0x16E`,
  and repeats the id that many times into a `0x10`-byte scratch. Then it draws
  uniformly from the scratch, refuses a pick the gauge `actor[+0x154]` cannot
  cover, consumes taken entries, and loops while the gauge still covers the
  cheapest cost.

So it is an action **producer** writing `actor[+0x1DF..]`, not a display builder -
which makes it the natural source of the observed AI-delegated multi-strike
streams (see
[`battle-action.md`](../../subsystems/battle-action.md#ai-delegated-0x380-party-members---what-is-and-isnt-pinned)).
The tail from `0x801F0B4C` is a second budget stage keyed on `actor[+0x170]` and
is not decoded. Ported as `engine-vm::battle_arts_auto_combo`;
`see ghidra/scripts/funcs/overlay_battle_action_801f0450.txt`.
