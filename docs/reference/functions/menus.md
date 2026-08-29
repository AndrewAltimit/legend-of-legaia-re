# Key functions: menus, shop + inventory

Part of the [key function directory](../functions.md) - the conventions for reading these tables (bare hex = function entry, `0x`-prefixed = data / instruction, overlay-VA caveats) are on the [index page](../functions.md#how-to-use-this-page).

## Records / stats screen

The "records" page (battles fought, escapes, play time, per-character maximums) is rendered by a single function in the field overlay. Stats globals are persistent save data.

| Address | Role |
|---|---|
| `FUN_801ED710` (field overlay) | Records-screen renderer. Draws **nine** label-heading rows via `FUN_8003CC98` (single-line text) with `FUN_80034B78` (number formatter): two global counters "No. of Battles" (`_DAT_800846A4`, cap 99999) and "No. of Escapes" (`_DAT_800846A8`, cap 99999); play time (`_DAT_800845DC` divided twice by `0x3C` for `H:MM:SS`, cap 99h59m59s); then **six** per-character stat categories, each a 3-iteration loop (`s2 < 3`) over the record at `0x80084140 + n*0x414` reading `+0x6B4` / `+0x6B0` (u32) then `+0x660` / `+0x664` (u32) then `+0x74D` / `+0x704` (u8); and a final averages row dividing the `0x801C6460` counters. Per-draw depth 3..9 written to `_DAT_8007B454`. `see ghidra/scripts/funcs/overlay_0897_801ed710.txt`. |
| `FUN_801DC6B4` (menu overlay) | Save-screen per-frame state machine. Sub-state in `_DAT_8007B43C` (0 = init, 1 = fade-in, …). Init (state 0): sets panel origin `DAT_801E4A4E = 0xB4` (x=180), `DAT_801E4A52 = 0x18` (y=24), adjusted +/-0xE when `func_0x8003CE64(8)` (flag-8 test) is non-zero; sets up screen-fade via `_DAT_8007B440 = 0xF2`, `DAT_801E46A0 = -0xF2`. Entry-context pointer `_DAT_8007B450` routes to sub-state: `NULL`/0→0x1A (normal save), `\x01`→0x19, `\x07`→0x20, `\r`→4. Reads pad from `_DAT_1F8003A0`. Captured as `overlay_shop_save_801dc6b4.txt`; see also [`subsystems/save-screen.md`](../../subsystems/save-screen.md). |

### Reading the records fields off a character record

Every displacement in the table row above is taken off the **save-block**
base `0x80084140`, not off a character record. The four-record character
array starts `0x5C8` into that block (`0x80084708`) and both sides use the
same `0x414` stride, so subtracting `0x5C8` once rebases the whole per-character
loop onto a bare record:

| Records field | Save-block `+` | Record `+` | Width |
|---|---|---|---|
| Maximum Hits | `0x6B4` | `0xEC` | u32 |
| Maximum Damage | `0x6B0` | `0xE8` | u32 |
| Knockouts | `0x660` | `0x98` | u32 |
| Monsters | `0x664` | `0x9C` | u32 |
| Hyper Arts | `0x74D` | `0x185` | u8 |
| Magic | `0x704` | `0x13C` | u8 |

The same `-0x5C8` reproduces the ability bitfield's independently pinned pair
(save-block `+0x6BC`/`+0x6C0` = record `+0xF4`/`+0xF8`, see
[`battle-action.md`](../../subsystems/battle-action.md)), which is what makes
the delta a measurement rather than an assumption.

The two byte fields are list **lengths**, not scalars. `+0x185` counts the
learned-Arts id list running from `+0x186` - the pair the Arts-list panel
renderer reads - and `+0x13C` counts the learned-magic id list from `+0x13D`,
which is the byte window 7's prompt (`FUN_801DCCB4`) substitutes a glyph out
of. So the records page's "Hyper Arts" and "Magic" columns and those two
list readers are three views of the same two fields.

Ports: the layout + the record reader are `engine-ui::ui_menu::records_screen`
(`record_counters`, `records_screen_draws_for`); the clamps and the H:MM:SS
split are `engine-vm::world_map_overlay::records_screen`. The three block-level
values (battles, escapes, the 1/60 s play clock) and the treasure census
(`DAT_801C6460` / `DAT_801C6462`, a world-map overlay global rather than save
data) have no engine-side counter yet.

## Field-overlay status / equip panels (overlay 0897)

A cluster of status/equipment-panel draw helpers resident in the field/town overlay (`overlay_0897`), keyed on the active menu screen-id `_DAT_8007BB9C`, the cursor row `_DAT_8007BB88`, and the panel-enable flag `_DAT_8007BBA0`. They anchor their draws at a screen X/Y taken from the passed record (`+0xA`/`+0xC`) and set the ink/depth staging global `_DAT_8007B454` before each string draw. Menu-side layout detail lives in [`subsystems/field-menu.md`](../../subsystems/field-menu.md).

| Address | Role |
|---|---|
| `801D7334` | **Not a function - a phantom print of `801E5B4C`.** `overlay_0897_801d7334.txt` is instruction-for-instruction identical to `overlay_0897_801e5b4c.txt` offset by exactly `0xE818`, i.e. a dump based at `0x801C0000` instead of `0x801CE818`, and truncated to 115 of that routine's 557 instructions. Read the equipment stat-bonus aggregator + panel dispatch at its real VA `801E5B4C` below; see [`phantom-print-index.md`](../../tooling/phantom-print-index.md) for the class. |
| `801D831C` | **Glyph selection-grid panel.** `(record)`. Draws a 17-column x 6-row glyph grid from the string table `0x801F29F0` via `FUN_80036888` (skipping `\|` / space cells), anchored at record `+0xA`/`+0xC`, ink `_DAT_8007B454` = 7 then 6; a cursor box (`FUN_8002B994`) draws unless `_DAT_8007BB94` == 4. Cursor `_DAT_8007BB88` selects the highlight: `< 0x66` maps to a grid cell (`idx / 0x11`, `idx % 0x11`) then `j 0x801E6C28` into the GTE cell-marker routine; `>= 0x66` special `f`/`d` rows read `0x801F2B2C` / `0x801F2B30`. `see ghidra/scripts/funcs/overlay_0897_801d831c.txt`. |
| `801D7E14` | **Mode-gated on-screen indicator draw.** `(record)`. No-op unless `_DAT_8007BBA0` is set. Dispatches on `_DAT_8007BB9C`: `0x3000` looks the cursor `_DAT_8007BB88` up in the `0x80074368` table and tail-calls `FUN_801E66D8`; `0x1000` emits two glyph/sprite draws via `FUN_80036888` at `(x+0x30, y+0x20)` from descriptors `0x801F2ACC` / `0x801F2AD4`, writing `_DAT_8007B454` = 4 then 9. `see ghidra/scripts/funcs/overlay_0897_801d7e14.txt`. |
| `801E5834` | **Pooled menu-actor spawn helper** `(a, b, c, d)`. Allocates an entry from the actor pool `_DAT_8007c34c` for descriptor `0x801f2978` via `FUN_80020de0`; on success clears `+0x54` and stores the four args as `+0x50` (type/id), `+0x14`/`+0x16` (screen position) and `+0x9c`. `see ghidra/scripts/funcs/801e5834.txt`. |
| `801E58A8` | **List row-count seed** `(actor)`. Writes sentinel `+0x5e = -2`, then derives the row count `+0x5c` from `_DAT_8007bdd8` (special-cased `== 99` -> `_DAT_8007b8f8 + 1`); when the actor flag `+0x10 & 0x1000000` is set it uses `_DAT_8007b6ac` (`base + count - 1`, re-latching the bit around a `FUN_800204f8` call), else `_DAT_8007bdd8 + _DAT_8007b8f8*7`. Always ticks `FUN_800204f8(actor)`. `see ghidra/scripts/funcs/801e58a8.txt`. |
| `801E5A08` | **Equip-item commit** `(item_id, char_idx, slot) -> 0/1`. Reserves the item (`FUN_80042EE0`, miss = `0x100`; `FUN_80043048`), resolves the destination slot from the item's equipment-table slot-type bits (`0x80074F68 +7 & 0x60 >> 5`; id via item table `0x80074368 +1`) or `slot+1` when `slot >= 4`, refunds any prior occupant (`FUN_800421D4(old, 1)`), writes the new id into char record `0x80084140 + char*0x414 + 0x75E + slot`, plays SFX `FUN_80035BD0(0x24)`, returns 1. Same computation as the `801E01F0` row (likely the same routine captured at a different base). `see ghidra/scripts/funcs/801e5a08.txt`. |
| `801E662C` | **Selection quantity readout** `(window)`. No-op unless `_DAT_8007bba0` is set. On mode `_DAT_8007bb9c == 0x3000` reads a count from the item table (`0x80074368 + _DAT_8007bb88*0xC`, halfword `+2`); `== 0x1000` maps `_DAT_8007bb88` through a byte table then that item field (`>>1`); else returns. Draws either an empty-state string (`0x801f2ad4`, ink `_DAT_8007b454 = 9`) or the count string (`0x801f2acc`, ink 4) plus the number (`FUN_80034b78`) at the window origin `+0xa`/`+0xc`. `see ghidra/scripts/funcs/801e662c.txt`. |
| `801E6778` | **Equip-target character list** `(window)`. Draws one row per active party member (`DAT_80084594` count, class bytes at `DAT_80084598+`): resolves the selected item's equip-character mask (item table `0x80074368 +1` -> equipment table `0x80074F68 +6`), then colours each row via `_DAT_8007b454` (7 equippable / 0 greyed) by testing the member's class bit (`0/1/2`) against that mask, draws a trailing return-to-bag row, and emits the selection cursor (`FUN_8002b994`) on row `DAT_8007b468`. `see ghidra/scripts/funcs/801e6778.txt`. |
| `801E71D0` | **Captioned member-list panel** `(window)`. Draws a fixed story caption (`FUN_80036888`) then up to three party rows: for each member whose context byte (`_DAT_8007b450 + i + 1`) is non-zero it builds a formatted row (`FUN_8003cbf8` marker/index fields plus a per-member digit) and draws it (`FUN_8003cd00`), stepping Y by `0xe`; finishes with the selection cursor (`FUN_8002b994`). Ink staged through `_DAT_8007b454`. `see ghidra/scripts/funcs/801e71d0.txt`. |
| `801E733C` | **Two-field value panel** `(window)`. Builds two small formatted fields from the selection context `_DAT_8007b450` (byte `+2`, and `+3 + (+2)*0x40`) via `FUN_8003cbf8` and draws them stacked (`FUN_8003cd00`, the second `+0xf` in Y), then the selection cursor (`FUN_8002b994`). Sibling of `FUN_801E71D0`. `see ghidra/scripts/funcs/801e733c.txt`. |

## Inventory / spell list

| Address | Role |
|---|---|
| `80042DBC` | **Seru-magic unequip** (spell-list pop): `(char_idx, spell_id, dst_slot)`. Record stride `0x414`. Searches the spell list `[char + 0x13D ..]` for `spell_id`, writes the matched entry back out to the active-spell slot at `[char + 0x2B0 + dst_slot*0x14]` (word `+0x8 + i*4` split little-endian into slot bytes `+1..+4`, level byte `+0x161 + i` into slot `+5`), shifts ids / levels / words down over `i`, then `count@+0x13C -= 1`. **Quirk:** when `spell_id` is absent the search ends at `i == count`, the shift does nothing, and the count is decremented anyway - an absent-id unequip silently drops the last list entry. |
| `80042FE8` | **Inventory count-add with the 99 cap.** `(slot: i16, delta: u8) -> new_count`. Bounds `slot` against the window size `gp[+0x2D4]`, indexes the 2-byte inventory record at `0x80084140 + 0x1818 + slot*2` (= `0x80085958`), returns 0 when the id byte is zero, otherwise adds `delta` to the count byte and clamps at `0x63` before storing. The 99 stack cap in its primitive form - it saturates rather than overflowing or rejecting. Unreferenced in retail: no word, `jal`, `j`, branch or `lui`/`addiu` pair targets it in any image. `see ghidra/scripts/funcs/80042fe8.txt`. |
| `800431FC` | Knows-spell predicate: `(char_idx, spell_id) -> bool`. Scans the same `+0x13D` spell list (count at `+0x13C`) for `spell_id`. `see ghidra/scripts/funcs/800431fc.txt`. |
| `80043264` | Accessory-equipped predicate: `(char_idx, item_id) -> bool`. Scans the character's equip-id bytes `+0x19B..0x19D` (slots 5..7 of the `+0x196..0x19D` block - the Goods slots) for `item_id`. `see ghidra/scripts/funcs/80043264.txt`. |
| `800430AC` | Party-wide accessory unequip-by-id: `(item_id) -> 0 \| 0x100`. For each active party member (`DAT_80084594` count, member ids at `0x80084598+`), scans the record's Goods slots `+0x19B..0x19D`; on the first match zeroes the slot and returns `0`, else `0x100`. Ghidra's auto-analysis leaves this body undisassembled (the `800430ac` function record is degenerate until re-created); the dump is force-created. Port: `engine-core::equipment::party_unequip_accessory_by_id`. `see ghidra/scripts/funcs/800430ac.txt`. |
| `800302E4` | Equipment/item stat-field accessor - `(_, id, field 0..3)`. Decodes the id-space tag in the id's high nibble (`0x1000`/`0x6000`/`0x9000` resolve through an inventory slot then the item-name table `PTR_DAT_8007436C`; `0x7000` is a direct equipment id), and returns the field `a2` selects from the equipment stat table `DAT_80074F68` (8-byte stride; [`formats/equipment-table.md`](../../formats/equipment-table.md)). `see ghidra/scripts/funcs/800302e4.txt`. |
| `80035274` | **Item / equipment passive-name draw.** `(item_id, _, arg)`. Resolves the item record at `0x80074368 + item_id*12` - the table whose `+0x4` name pointer is [`item-table.md`](../../formats/item-table.md)'s `PTR_DAT_8007436C` - through its two leading bytes into an [accessory-passive](../../formats/accessory-passive-table.md) slot, then draws that slot's **name**. Unreferenced in retail - [details ↓](#80035274). `see ghidra/scripts/funcs/80035274.txt`. |

## Shop screen panels

Content-only panel draws in the menu overlay (`overlay_menu` and `overlay_shop_save` carry byte-identical copies); the window frame is caller-drawn. `_DAT_8007B454` is the text-ink selector each row sets before drawing - `7` normal, `0` greyed/unavailable, `4`/`5`/`6`/`9` the accent inks. Architecture in [`subsystems/shop.md`](../../subsystems/shop.md).

| Address | Role |
|---|---|
| `801D4868` | **Shop root command menu.** `(window)`. Draws "Buy" / "Sell" / "Quit" at `(x+0x14, y + 0 / 0xE / 0x1C)` via `FUN_80036888`, each row's highlight cursor emitted by `FUN_8002B994` keyed off the selection word `DAT_801E46BC` (low 12 bits = row, bit `0x1000` = blink phase, `0x2000` / `0x4000` = alternate/suppressed cursor states). Between the "Buy" and "Sell" draws it scans the party inventory pair array at `0x80084140 + 0x1818` over `_DAT_8007B5EA.._DAT_8007B5EC`; when no entry has both bytes non-zero it drops the ink to `0` and never restores it, so an empty bag greys Sell **and Quit**. Ported: `engine-core::shop::shop_root_command_rows` (+ `shop_cursor_mode`). `see ghidra/scripts/funcs/overlay_menu_801d4868.txt`. |
| `801D5510` | **Buy-quantity prompt panel.** `(window)`. Draws "Have" + the owned count of the highlighted item (`FUN_80042EE0`; the sentinel `0x100` means none held, and the panel prints " None" instead), then "How many will you buy?", the quantity `DAT_801E46B4` x the stock cap `DAT_801E46B8`, and the running total `qty * price`. The unit price is the `u16` at item record `+2` (`0x8007436A + id*0xC`); the total's digit field width is chosen by magnitude against `99` / `999` / `9999` (4..7 columns) so the number stays right-aligned in the box. Ported: `engine-core::shop::{shop_buy_quantity_panel, shop_total_digit_field}`. `see ghidra/scripts/funcs/overlay_menu_801d5510.txt`. |
| `801D5AE8` | **Item detail / sell panel.** `(window)`. For the highlighted item id `DAT_801E46B0` draws the name (record `+4`) and the description (record `+8`, via `FUN_800337B0`), then either "Price" + `price >> 1` - **exactly half the buy price** - or "Cannot sell" (ink `9`) when the price word is `0`. The passive-effect text resolves off the class byte at record `+0`: class `1` reads the index from equip record `+5`, otherwise from item-effect record `+3`; `< 0x40` draws accessory-passive fields `+4` and `+8`. Layout + the two-table chain (re-run per draw) in [shop.md](../../subsystems/shop.md#item-detail--sell-panel-fun_801d5ae8). Ported: `engine-core::shop::{shop_sell_detail_panel, item_passive_index}`. `see ghidra/scripts/funcs/overlay_menu_801d5ae8.txt`. |
| `801D5DE0` | **Casino prize list** (window 44's `renderer_va`) - **not** the shop stock list this row once called it ([shop.md](../../subsystems/shop.md#row-layout-whose-list-this-is)). `(window)`. Walks `DAT_801EF0D0` rows through the row-order byte array `DAT_801EF0E0`, name + price per line (`y += 0xE`). Prize records at `DAT_801E4518`, **8-byte stride**, `0x60` per block, keyed by `*(_DAT_8007B450 + 1)`. Affordability gates the **coin bank** `_DAT_800845A4`; the gold purse `_DAT_8008459C` is absent. Ink tests are sequential **overwrites** ([shop.md](../../subsystems/shop.md#row-ink-is-last-rule-wins-not-first-match)). No gold footer. Ported as kernel reuse: `engine-core::shop::shop_stock_row_ink`. `see ghidra/scripts/funcs/overlay_menu_801d5de0.txt`. |
| `801CF5D0` | **Per-character equipment snapshot stage.** `(char_index)`. Copies eight `u16` fields out of the live character record (`0x80084140 + n*0x414`, offsets `+0x6CC`, `+0x6D0`, then `+0x6D8..+0x6E2` contiguous) into the eight-word overlay scratch block at `0x801EF080..0x801EF09C`, widening each to `u32`. The menu/shop equip panels read the scratch block rather than the record, so this is the staging step a re-equip must re-run. Note the deliberate gap: `+0x6D2..+0x6D6` is skipped. `see ghidra/scripts/funcs/overlay_menu_801cf5d0.txt`. |
| `80034250` | **Highlighted-entry description dispatcher.** `(window)`. Resolves the description string for the current cursor and hands it to `FUN_800337B0` at `(window[+0xA], window[+0xC])`. Screen id `gp+0x884` selects the source: `0x1000` / `0x6000` (bag lists) and `0x9000` take the item id from the inventory byte array `0x80085958 + cursor*2`, `0x7000` uses the cursor `gp+0x870` directly as the id. `0x1000` / `0x6000` / `0x7000` read the description pointer at item record `+8`; `0x9000` instead chains item record `+1` -> item-effect `+3` -> accessory-passive record (`0x8007625C`, `0xC` stride). Suppressed when `gp+0x87C == 4`; a resolved id of `0` draws nothing. Ported: `engine-core::menu_list_rows::description_source`. `see ghidra/scripts/funcs/80034250.txt`. |
| `801DB380` | **Buy recipient picker SM** (menu-overlay sub-screen). Phase 1 navigates `party_count + 1` rows (`FUN_801D688C`, wrap). Row 0 confirms a plain single-unit buy: bag add `FUN_800421D4(id, 1)`, gold `0x8008459C -= price`, and - while item `0xFE` (Point Card) is held (`FUN_80042F4C`) - `price/20` into the Point Card counter `0x800845B4` (cap `0x98967F`). A party row first tests equippability: item record `+1` -> equip record `+6` mask vs the per-character mask byte `0x801E43F0[char]`; a mismatch buzzes (SFX `0x23`), a match buys **and equips now** (the old piece returns to the bag; the purchase never enters it). Cancel -> sub-screen `0x1B` (the buy list). Ported: `engine-core::shop::BuyRecipientSession`. `see ghidra/scripts/funcs/overlay_menu_801db380.txt`. |
| `801DB7F4` | **Buy quantity + commit SM** (menu-overlay sub-screen). Phase 0 derives `max = min(gold/price, 0x63, 0x63 - held)` (held via the bag scan `FUN_80042EE0`). Phase 1: Right/Left step the quantity `DAT_801E46B4` by 1, Down/Up by 10, clamped `[1, max]`; confirm SFX `0x2C`, cancel `0x37` -> `0x1B`. Commit credits the Point Card (`price/20 * qty`, gated on holding item `0xFE`, cap `0x98967F`) **before** the gold debit `price * qty`, adds the stack (`FUN_800421D4`), and - Point Card only - waits on a toast for a button press (SFX `0x20`) before returning to the buy list. Ported: `engine-core::shop::BuyQuantitySession`. `see ghidra/scripts/funcs/overlay_menu_801db7f4.txt`. |
| `801DBD94` | **Sell quantity SM** (menu-overlay sub-screen `0x1F`). Phase 0 seeds quantity 1, max = the staged slot's bag count (`0x80085959 + slot*2`). Phase 1: same pad decode as `801DB7F4`; confirm consumes the stack slice (`FUN_80042310`), credits `(price * qty) >> 1` gold (cap `0x98967F`) and applies the sell-list scroll fix-up (last row alone on the final page steps selection and scroll back); a whole-stack sale rescans the bag and, when empty, runs a `0x11`-unit exit delay before dropping to the shop root (`0x1A`) instead of the sell list (`0x1E`). Ported: `engine-core::shop::SellQuantitySession`. `see ghidra/scripts/funcs/overlay_menu_801dbd94.txt`. |
| `801DB21C` | **Shop buy list SM** (menu-overlay sub-screen `0x1B`). State 2 confirm: gold `0x8008459C` vs the item price halfword (buzz `0x23` + stay on short), then routes on the item kind byte - `1` -> `0x1C` recipient picker, `2` -> `0x1D` quantity picker, else `0x1A`; cancel parks the list and returns to `0x1A`. Ported: `engine-core::shop::buy_list_confirm_route`. `see ghidra/scripts/funcs/overlay_menu_801db21c.txt`. |
| `801DC1CC` | **Casino prize-exchange session** (menu-overlay sub-screen `0x20`, entry-context `\x07`). Builds visible rows from the `0x801E4518` table (stop at zero id, hide set one-shot gates), gates redeems on the coin bank `0x800845A4` and held `< 0x63`, Yes/No defaults No, commit grants + debits coins + sets the gate flag + rebuilds. Ported: `engine-core::prize_exchange`. `see ghidra/scripts/funcs/overlay_menu_801dc1cc.txt`. |
| `801D99F0` | **Equip slot browse** (menu-overlay sub-screen `0x13`): 8 rows, row 0 = Best Equipment auto-equip via `FUN_801CF88C` + `FUN_801CF760`, rows 1..7 -> `0x14`, cancel -> `0x12`. Ported: `engine-core::equip_session::slot_browse_confirm`. `see ghidra/scripts/funcs/overlay_menu_801d99f0.txt`. |
| `801D9C14` | **Equip candidate list + commit** (menu-overlay sub-screen `0x14`): kind-4 list protocol, trial-equip stat preview through the `DAT_801EF0C8` staging save/restore + `FUN_801CF650`, commit swaps through the bag, Remove row returns the equipped item. Ported: `engine-core::equip_session::{preview_candidate, unequip}`. `see ghidra/scripts/funcs/overlay_menu_801d9c14.txt`. |
| `801CF760` | **Best-equipment applier**: per armament slot, skip when candidate == equipped or absent from the bag, else take one, return the old item, write the slot; returns the changed count. Ported: `equip_session::apply_best_equipment`. `see ghidra/scripts/funcs/overlay_menu_801cf760.txt`. |
| `801CF88C` | **Best-equipment candidate scan** (menu overlay; the producer of the `DAT_801EF0C0` array `801CF760` consumes). Seeds the four armament slots with what the character wears, then walks the bag keeping one winner per slot - eligible = item record `+0 == 1` and equipment record `+6` sharing a bit with `0x801E43F0[char]`; the slot is `+7`'s `(bits & 0x60) >> 5` permuted `[2,1,0,3]` (weapon-first). Armour ranks on `+2 + +3` (UDF+LDF) **only**, so pure-INT / pure-SPD gear never wins; the weapon ranks on `+1 + FUN_801DD0C0(char, id, 1)`, whose flat `1000` outweighs any ATK byte. Everything around the scan is a trial equip it undoes. Ported: `equip_session::{best_equipment_candidates, armament_slot_of}`. `see ghidra/scripts/funcs/overlay_menu_801cf88c.txt`. |
| `801D8F10` | **Magic caster picker** (sub-screen `0x0E`): confirm gated on spell count `record[0x13C]` and the Ra-Seru equip slot (via the `0x8007B424` per-character offset table). Ported: `engine-core::spell_menu`. `see ghidra/scripts/funcs/overlay_menu_801d8f10.txt`. |
| `801D9110` | **Magic spell list** (sub-screen `0x0F`): kind-4 list (content id 5); confirm routes on spell-stat byte `+2` bit `0x20` to `0x10` (group) / `0x11` (single-target). Ported: `spell_menu::spell_targets_group`. `see ghidra/scripts/funcs/overlay_menu_801d9110.txt`. |
| `801D9280` | **Magic group-cast flow** (sub-screen `0x10`): picker with count 0 (confirm/cancel only), SFX `0x25` commit. Ported: `engine-core::spell_menu` (folded into `TargetSelect`). `see ghidra/scripts/funcs/overlay_menu_801d9280.txt`. |
| `801D9594` | **Magic single-target pick + apply** (sub-screen `0x11`): party-row picker, `FUN_8003FB10` revalidation, MP through `FUN_80035394`, apply through `FUN_800402F4`. Ported: `engine-core::spell_menu`. `see ghidra/scripts/funcs/overlay_menu_801d9594.txt`. |
| `801E3294` | **Libcd card I/O state machine** (menu overlay): 5 states, shared retry budget `DAT_801E4FC4` (5), both-acked latch `DAT_801EED20`, results `1` / `-1` / `-2` / `-3`. Ported: `engine-core::save_select::CardIoMachine`. `see ghidra/scripts/funcs/overlay_menu_801e3294.txt`. |
| `801E1114` | **Per-frame card ticker**: advances `FUN_801E3294` under the `_DAT_801F329C < 3` gate and, on the commit beat (`_DAT_801F021C == 3` + request `_DAT_801F0224`), sequences `FUN_801E3AF0` -> `FUN_801E3BA0` -> `FUN_801E1208`. Ported: `save_select::card_frame_tick`. `see ghidra/scripts/funcs/overlay_menu_801e1114.txt`. |
| `801D6E18` | **Developer character-parameter editor tick.** A 12-row cursor over one character's stat fields; L1/R1 move it, left/right step the hovered field by `±1` scaled `×8` / `×64` by two modifier bits, and row 11 scales by `0x10` into the XP word. Ends **every** tick with a two-stage clamp over all four records - a sanity reset to `1` outside `1..=0x4E1F` (level `1..=0xC7`), then per-field ceilings: HP `9999`, MP `999`, the `+0x120` cap constant `100`, the six battle stats `999`, level `99`. Those ceilings are the game's own stat caps. Ported (input + clamp halves only): `engine-core::debug_char_editor`. `see ghidra/scripts/funcs/overlay_save_ui_801d6e18.txt`. |
| `801DA2A0` | **Save/menu sub-screen `0x15`** - one body serving three per-character lists (ability bitfield `+0xF4` population count, Ra-Seru-gated spell count `+0x13C`, a byte at `+0x185`), selected by the step counter; confirm swaps two rows across three parallel arrays. The "debug-editor page-navigation SM" label is falsified. Kernels ported as `engine-core::save_subscreen::sub15_*`; see [`save-screen.md`](../../subsystems/save-screen.md#sub-screen-0x15---the-per-character-list-screen-fun_801da2a0). `see ghidra/scripts/funcs/overlay_save_ui_801da2a0.txt`. |
| `801E13B8` | **Card write / format state machine** over `DAT_801F329C`. States `1`/`2` wait for a positive poll (a `-2` interrupts with menu mode `0x17` save / `0x13` load); `3` burns a `0x18` delay, then opens the directory, sizes it, looks the file up, composes the block (`FUN_801E1934`) and writes - a *create* additionally needs a free block; `5` reads back; `7` formats with 5 busy-retries, and only result `1` is success. Ported: `engine-core::card_flow::CardWriteMachine`. `see ghidra/scripts/funcs/overlay_menu_801e13b8.txt`. |
| `801E16E0` | **Card-health fold.** Composes a status name off the last poll `DAT_801F3804` into a stack buffer nothing reads, then dispatches the latched poll `DAT_801F3800` through a second `result+3` jump table. Maintains two saturating (`0x400`) counters: `0x801F0218`, the no-card fault count its own `printf("not card %d", n)` names - the argument survives in `a1` and the decompiled C drops it - and `0x801F01BC`, the card-changed debounce that needs **two** consecutive `-2`s. A live fault clears the cached directory scan. Ported: `engine-core::card_flow::CardHealth::fold`. `see ghidra/scripts/funcs/overlay_menu_801e16e0.txt`. |
| `801E39A8` | **Card event drain**: four `TestEvent` calls with results discarded (TestEvent consumes the pending flag). Ported: `save_select::card_events_drain`. `see ghidra/scripts/funcs/overlay_menu_801e39a8.txt`. |
| `801E06C0` | **Save-block grid renderer**: per cell interpolates the slide base `0x15A + slot*64 -> 0x5A` (12-bit fixed point), adds `col*40 + row*4`; the cell drawer `FUN_801E0FD0` adds `+8`; focused cell full modulation. Ported: `engine-ui` `slot_grid_quad_x` / `slot_preview_grid_draws_for`. `see ghidra/scripts/funcs/overlay_menu_801e06c0.txt`. |
| `801E02A4` | **Save-UI backdrop dim**: title art re-emitted as two `0x64` sprites (192 + 128 across texture pages 8/9) with all RGB bytes = the brightness parameter. Ported: `engine-ui::backdrop_dim_sprites`. `see ghidra/scripts/funcs/overlay_menu_801e02a4.txt`. |
| `801E0418` | **Card-message / two-choice text stack**: five centred rows (y `0x50/0xA0/0xAE/0xBE/0xCC`), unselected choice at half brightness off `_DAT_8007B820`; computes a triangle-wave pulse it never reads. Ported: `engine-ui::card_message_rows`. `see ghidra/scripts/funcs/overlay_menu_801e0418.txt`. |
| `801E3FF0` | **Sprite-record quad drawer**: record `idx` of the 12-byte table at `0x801E5048` -> GP0 `0x2C` quad at a pen with the caller's RGB word. Ported: `engine-ui::save_ui_record_quad`. `see ghidra/scripts/funcs/overlay_menu_801e3ff0.txt`. |
| `801D1DAC` | **Window-10 Yes/No prompt renderer** (the Door of Light confirm, submenu `0xB`): prompt at the content origin, options at `WX+0x44` pitch `0xE` in ink 5, hand at `WX+0x30`, cursor `DAT_801E46D0`. Ported: `engine-ui::confirm_prompt_draws`. `see ghidra/scripts/funcs/overlay_menu_801d1dac.txt`. |
| `801D1F10` | **Window-12 Yes/No prompt renderer** (the Incense confirm, submenu `0xD`): second prompt line indents `+0xC`, option block one pitch lower. Ported: `engine-ui::confirm_prompt_draws`. `see ghidra/scripts/funcs/overlay_menu_801d1f10.txt`. |
| `801DD330` | **Options sub-screen wrapper** (`0x17`): `FUN_801DA9F8(0, 9, 0x30, 1)` and nothing else. Ported: `engine-core::options::OPTIONS_SUBSCREEN_*`. `see ghidra/scripts/funcs/overlay_menu_801dd330.txt`. |

## Menu / HUD globals

| Address | Role |
|---|---|
| `80034A6C` | **New-game data-init**, not a menu routine - [details ↓](#80034a6c) - listed here because its seed writes land in the `0x800845xx` window this page's screens read. |
| `800337B0` | Menu-string formatter and renderer. 27 KB switch-on-mode that drives the character-status / equipment / spell-screen pages via `FUN_8003CD00` (multi-line) and `FUN_80036888` (raw draw) keyed on string buffers at `&DAT_8007B4B0..` and the multi-line label table at `gp + 0x13c + 0x7F86`. |
| `8004313C` | Inventory active-window setup, gated on party member count (`DAT_80084594`) - [details ↓](#8004313c) |
| `801D688C` | **Menu cursor-navigation primitive.** `(cursor: *u32, count, mode) -> 0/1/2/3`. The shared list-navigation helper across the menu / shop / save-slot state-handlers. Reads the overlay confirm / cancel pad masks (`DAT_801EF0F0` / `DAT_801EF0F4`) against `_DAT_8007B874`: confirm → SFX cue `0x36`, return `1`; cancel → SFX `0x37`, return `2`. Otherwise (when `count != 0`) reads held-pad `_DAT_8007BB84`: left (`0x1000`) decrements the low-12-bit cursor (when `> 0`), right (`0x4000`) increments it (when `cursor+1 < count`), each playing SFX `0x21` and returning `3` (moved); `mode != 0` is the wrap variant. SFX go through the cue enqueue `FUN_80035B50`. Ported: `engine_core::menu_input::menu_cursor_nav`. `see ghidra/scripts/funcs/overlay_save_ui_select_801d688c.txt`. |
| `80032A44` | **Kind-4 list kernel** (SCUS): per-frame navigate / confirm / PAGE-header / row-draw of every allowlisted list window. Full spec in [`field-menu.md`](../../subsystems/field-menu.md#the-kind-4-list-kernel-scus-fun_80032a44). Navigation ported: `engine-core::pause_screens::list_kernel_navigate`. |
| `80030104` | **List-node allocator.** `(out, window, count, fallback_text)`. `count > 0`: allocates the `count*2 + 0x2A` list node hung at window `+0x18` (`+0x0` scroll top, `+0x2` visible rows = `(window_h - 4)/14`, `+0x4` count, `+0x6` selected, rows from `+0x28`), clamping the persisted selection globals `0x8007BB98`/`0x8007BB90` **in place** (selection to `count-1`, scroll top down to the selection) and mirroring `count` into `0x8007BBA0`. `count == 0`: copies `fallback_text` into a fresh 0x80-byte buffer instead, width-measured via `FUN_8003CC90` into `+0x12`, marker `+0x14 = 0x80` - the "empty list" text panel. Ported: `engine-core::menu_list_rows::list_alloc`. `see ghidra/scripts/funcs/80030104.txt`. |
| `8002FF8C` | **Row-name resolver.** `(row_entry) -> *name`. Switches on the entry's class nibble: `0x2000`/`0x5000` spell-table name (`0x800754D0` side of the 12-byte record); `0x3000`/`0x7000`/`0xA000` item-table name with the payload (`& 0x3FF`) as the id; `0x1000`/`0x6000`/`0x9000` dereference the bag slot at `0x80085958 + slot*2` first; `0x8000` the 32-byte landmark name cells at `0x80073B18` via placement byte `0x80073A98[payload*6]`; `0x4000` the verb pointer table `0x8007329C`; anything else the shared empty string at `gp+0x168`. Ported: `engine-core::menu_list_rows::row_name_source`. `see ghidra/scripts/funcs/8002ff8c.txt`. |

## Menu-overlay callees (PROT 0899)

Callees of the pause/field menu overlay (loaded by the mode-22 CARD pair via `FUN_8003EBE4(4)`), resident at slot-A base `0x801D0000+`. Dumps are correctly based (`overlay_menu_*`).

| Address | Role |
|---|---|
| `801DAD6C` | **Menu-open init state machine** (5-state jump table on `DAT_801E46AC`): stages the menu actors through the actor/sprite VM `FUN_801D6628`, spawns the cursor actor via `FUN_80020DE0`, advances a phase counter, then always ticks the text-actor list `FUN_80031D00`. `see ghidra/scripts/funcs/overlay_menu_801dad6c.txt`. |
| `801D1290` | **Equip stat-compare panel** (window 25). Draws the active character's name, then one stat pair or triple, each row printing the live value and - only when it differs - a rise/fall arrow plus the trial-equip value. There is **no jump table** anywhere in its 548 instructions; the row set comes from a three-way unsigned split on one category byte. See [`field-menu.md`](../../subsystems/field-menu.md#equip-stat-compare-panels-windows-25-and-41). Ported: `engine-ui::equip_compare_panel_fields`. `see ghidra/scripts/funcs/overlay_menu_801d1290.txt`. |
| `801D4C28` | **Party-wide equip stat-compare panel** (window 41). Per roster member (`0x37` pitch): name, then "Equipped" / "Cannot Equip", else the ATK / UDF / LDF triple computed by a trial equip - back up the eight equip bytes, write the staged id, re-run `FUN_801CF650` + `FUN_80042558`, restore. Ported: `engine-ui::party_compare_panel_fields`. `see ghidra/scripts/funcs/overlay_menu_801d4c28.txt`. |
| `801D61B0` / `801D6360` | **Menu option-list row drawers** - emit each row's text via `FUN_80036888` and its selection-cursor highlight via `FUN_8002B994`. `see ghidra/scripts/funcs/overlay_menu_801d61b0.txt`. |
| `801D56FC` | **Item-target party-panel renderer.** `(window)`. For the current item id `DAT_801E46B0` derives the item's equip / usability mask (item record category byte -> equipment stat table `0x80074F68 +6`, [`equipment-table.md`](../../formats/equipment-table.md)), then draws a header row plus one row per active party member (count `DAT_80084594`, roster ids at `0x80084598`): each row's label is picked from its roster byte and greyed (ink `0` via `_DAT_8007B454`) when the item mask misses the per-character mask `DAT_801E43F0[member]` - the same equippability test the shop buy-recipient picker `FUN_801DB380` runs. Cursor highlight (`FUN_8002B994`) from `DAT_801E46C0`; the party-target panel `FUN_801D8308` drives. `see ghidra/scripts/funcs/801d5780.txt`. |
| `801E420C` | **Menu textured-quad sprite emitter** (GP0 `POLY_FT4`, cmd base `0x2E`): allocates a `0x28`-byte packet from the scratchpad pool `0x1F800314+0x8C`, fills four verts/UVs from the per-index geometry table `0x801E5048` (stride `0xC`), posts via `FUN_8003D2C4`. `see ghidra/scripts/funcs/overlay_menu_801e420c.txt`. |
| `801E2DC4` / `801E2EE4` / `801E4190` | Sibling menu GPU-primitive emitters - build a GP0 packet in the `0x1F800314` pool and link it into the OT via `FUN_8003D2C4` (their only call). `see ghidra/scripts/funcs/overlay_menu_801e2dc4.txt`. |
| `801E1934` | **Save-block composer.** Stamps the PSX header (`"SC"`, icon descriptor `0x11`, block count `1`), writes the slot number into the title as two full-width digits (`0x4F + digit`), copies the party summary (per member: name from record `+0x2A7`, level `+0x130`, HP/MP cur+max), grabs the three icon frames and the CLUT **out of** VRAM with `StoreImage`, then `memcpy`s `0x1A18` bytes of live state and stores `FUN_801E38D8`'s checksum at `+0x1FFC`. The `/10` is the two title digits, not a counter readout. Ported: `engine-core::card_flow::save_block_summary`. `see ghidra/scripts/funcs/overlay_menu_801e1934.txt`. |
| `801E3A00` / `801E3A98` | Kernel event-handle poll helpers (call `TestEvent` `FUN_80056658`). `see ghidra/scripts/funcs/overlay_menu_801e3a00.txt`. |
| `801E3BEC` | Formatted-string build + print (sprintf-shape `FUN_80056738` + print `FUN_800567A8`). `see ghidra/scripts/funcs/overlay_menu_801e3bec.txt`. |
| `801E37CC` | **Three-argument dev trace print** - the sibling of `801E3BEC`. Formats into a `0x20`-byte stack buffer with `FUN_800567B8(buf, &DAT_801CF4B8, a, b, c)` (the `printf`-class formatter) and hands the buffer to the BIOS B-vector thunk `FUN_80056718`, whose routine selector is the caller-set `$t1`. Sixteen instructions, no game state touched. Dumped identically under `overlay_menu_` and `overlay_shop_save_`. `see ghidra/scripts/funcs/overlay_menu_801e37cc.txt`. |
| `801E4140` | Widget frame/box draw wrapper (`FUN_8002C69C`). `see ghidra/scripts/funcs/overlay_menu_801e4140.txt`. |
| `801D4A80` | **Window 34 content renderer** - the item / accessory description box, rect `(138, 166, 168, 38)`. Gated on the selected item id `_DAT_801E46B0 > 0`; draws the item's name from `0x80074368 + id*0x0C` (`+0x04`) at ink `6`, then either the accessory-passive description (`0x8007625C + idx*0x0C`, `+0x08`) when the item record's leading byte is `2` and the item-effect `+0x03` index is `< 0x40`, or the item's own `+0x08` string. See [`field-menu.md`](../../subsystems/field-menu.md). `see ghidra/scripts/funcs/overlay_menu_801d4a80.txt`. |
| `801DCA0C` / `801DCA50` / `801DCA94` / `801DCAD8` / `801DCB1C` / `801DCFE4` | **The plain title-tab renderers** (windows 0..=4 and 43) - six copies of one 17-instruction routine: stage ink `7`, then `FUN_80036888(str, 0, 0, WX, WY)`. They differ **only** in the string pointer (`addiu a0,a0,-0x1630` vs `-0x1394`), which is why one painter serves all six. Ported: `engine-ui::title_tab_draws_for`, selected by [renderer_va dispatch](../../subsystems/field-menu.md#which-painter-draws-a-descriptor-renderer_va-dispatch). `see ghidra/scripts/funcs/overlay_menu_801dca0c.txt`. |
| `801DCF84` / `801DD028` | **The two counter windows** (32 / 45) - one routine with two literals changed: pictogram `0x62` + party gold `_DAT_8008459C`, or `0x66` + the casino coin bank `_DAT_800845A4`. Pictogram at `(WX, WY+2)`, an 8-digit right-aligned field at `(WX+0x28, WY)`. Ported: `engine-ui::counter_panel_draws_for` + `CounterSource`. `see ghidra/scripts/funcs/overlay_menu_801dcf84.txt`. |
| `801D603C` | **Window 46 content renderer** - two-row toggle panel, rect `(16, 84, 104, 42)`. Two labels drawn through `FUN_80036888` at ink `7` / `5`, each with a marker from the cursor family `FUN_8002B994` whose kind is decoded from bits `0x4000` / `0x2000` / `0x1000` and the low 12 bits of the state word `_DAT_801E46D0`. Which screen owns it is **Unknown**. `see ghidra/scripts/funcs/overlay_menu_801d603c.txt`. |
| `801E36A0` | 9-instruction thunk into the menu routine `FUN_801DD35C`. `see ghidra/scripts/funcs/overlay_menu_801e36a0.txt`. |
| `801E4138` | Empty stub - 2-instruction `jr ra; nop`. `see ghidra/scripts/funcs/overlay_menu_801e4138.txt`. |

## Function details

Full write-ups for the rows above whose detail outgrew a table cell. Linked from each section table by **[details ↓]**.

### `80034A6C`

**New-game data-init**, and the row above is on this page only because of where
it writes. Every store is an `sb` / `sw` off `s0 = 0x80084140`, the live
game-state window (`see ghidra/scripts/funcs/80034a6c.txt`), and the canonical
description is [`game-modes.md`](game-modes.md)'s row - party gold
`0x8008459C = 500` (`li v0,0x1f4` at `0x80034A94`), party count
`0x80084594 = 3`, the `0x800846D0..0x800846DC` quad `0x44 / 0x21 / 0x10 / 0x48`,
the starting-item seed `SC+0x1818 = 0x77` count `5`, and the tail call
`FUN_800560B4` that expands the starting-party template.

Two claims that read as menu work do not survive the disassembly:

- **The zeroed block is the story-flag bank, not a save-data scratch slot.** The
  descending loop at `0x80034B1C` stores through `0x1618(v1)` with `v1` starting
  at `s0 + 0x1FF`, so the addresses written are `0x80085758..0x80085957` - the
  256-bit fourth flag bank `FUN_8003CE08` / `CE34` / `CE64` operate on
  ([`runtime-libs.md`](runtime-libs.md)). `0x80084340..0x8008453F` is the range
  the *register* sweeps, not the range any store lands in.
- **The `0x800845xx` writes are new-game seeds, not UI defaults.** They are
  cursor-shaped only by coincidence of address; the port models the same fifteen
  cells as `legaia_asset::new_game::new_game_seed_words`, keyed by `SC` offset.

### `80035274`

The one routine that reads the item table's first two bytes as fields, so it is
where their meaning comes from. Byte `+0x0` is a **class** and byte `+0x1` a
**sub-index** into whichever per-class table the class selects:

| `+0x0` | Table | Field taken |
|---|---|---|
| `1` | equipment stat table `DAT_80074F68` ([`equipment-table.md`](../../formats/equipment-table.md), 8-byte stride) | byte `+0x5` |
| anything else | item-effect table `DAT_800752C0` ([`item-effect-table.md`](../../formats/item-effect-table.md), 4-byte stride) | byte `+0x3` |

Either field lands in the same 64-slot index space, and only values `< 0x40` are
drawn: the text ink `gp[0x13C]` is set to `4`, the pointer at
`0x8007625C + index*12 + 4` goes through the sprite/text emitter `FUN_80036888`,
and the ink is restored to `7`. An index `>= 0x40` draws nothing at all.

`+4` of that record is the passive's **name**, not its description
([`accessory-passive-table.md`](../../formats/accessory-passive-table.md)) -
which is what separates this routine from the two that survive into retail
menus. `FUN_80034250` and the window-34 renderer `FUN_801D4A80` both take `+8`,
the description. Nothing on the disc reaches `FUN_80035274` at all: it has no
caller, no jump-table slot and no materialisation site in any image, so the
equipment arm's `+0x5` reading is decoded evidence about the item tables rather
than about anything retail draws (see
[`battle.md` § Unreferenced SCUS entry points](battle.md#unreferenced-scus-entry-points)).

### `8004313C`

Inventory active-window setup, gated on party member count (`DAT_80084594`). Writes the window start/end/span triple `gp+0x2D2` (start), `gp+0x2D4` (end), `gp+0x2D6` (span = end - start): a single-member party (`< 2`) with flag-bank bit `0x14` clear (`FUN_8003CE64(0x14)`) collapses the window to `0..0x80` (or `0x80..0x100` when `DAT_80084598`, the first party id, is non-zero); otherwise the full `0..0x100`. It is the inventory-window setup the field-VM **`GIVE_ITEM` op `0x39`** runs before adding the inline item id via `FUN_800421D4` (dispatcher `FUN_801DE840` case 0x39 at `0x801E0448`: `FUN_8004313C()` then `FUN_800421D4(item_id, 1)`; see [`script-vm.md`](../../subsystems/script-vm.md)).

The op-0x39 handler in `crates/engine-vm/src/field.rs` is the `FieldHost::give_item` hook (the world impl adds the inline item id to the inventory, capped at the retail stack cap), and the disassembler decodes it as `InsnInfo::GiveItem`. (Both were once mislabelled `play_sfx`/`PlaySfx`; corrected - SFX cues go through `FUN_80035B50`, not op 0x39.) The window bound it writes is the one the inventory accessors (e.g. `FUN_800423E0` compaction) walk against. `see ghidra/scripts/funcs/8004313c.txt`.
