# Shop Subsystem

Covers the buy / sell / quantity / confirm flow used whenever the player enters a
town shop. The shop UI lives inside the **menu overlay** - the same 129-function
binary that hosts the save screen and status screens. No separate shop overlay
exists. (The inn is *not* a menu-overlay session - see [inn.md](inn.md).)

Per-scene stock lives inline in the scene MAN's field-VM script and prices in
the static `SCUS_942.54` item table (see [Gold-shop stock
source](#gold-shop-stock-source) below); the menu overlay supplies the UI. The
buy-list render layout is traced from `FUN_801d5de0` in `overlay_shop_save.bin`.

## Flow overview

The retail engine enters the shop from the field-VM WARP / shop-trigger opcode.
The menu overlay dispatches on a sub-screen ID (pointer table at `0x801E4F40`,
same table used by the save screen). The shop sub-screens handle:

| Phase | Sub-screen | Description |
|---|---|---|
| Buy list | `ShopBuy` | Shows available items + prices. Cursor selects an item. |
| Sell list | `ShopSell` | Shows player inventory. Cursor selects an item to sell. |
| Quantity | `ShopQuantity` | Numeric selector (1..9). Confirms how many to buy/sell. |
| Confirm | `ShopConfirm` | Yes / No prompt. Yes commits the transaction. |
| Exit | `ShopExit` | Clears session, returns to field. |

Gold and inventory deltas are applied to the world on `ShopConfirm` slot 0
(Yes): `try_buy` deducts gold and credits inventory; `try_sell` credits gold
and decrements inventory.

**Point Card accrual (retail).** The retail buy commit `FUN_801db7f4` also
credits the Point Card counter `_DAT_800845B4` (u32) before the gold debit:
when the party holds item `0xFE` (the Point Card - inventory-has check
`func_0x80042f4c(0xFE)`), it adds `price / 20` per unit bought, capped at
`9,999,999`. Sell transactions never accrue. The single-unit buy path of the
recipient picker `FUN_801db380` (row 0 = "into the bag") applies the same
accrual. Ported as `engine-core::shop::{point_card_credit,
apply_point_card}` inside `BuyQuantitySession`; the engine's
`buy_from_shop` world kernel does not yet apply it.
`see ghidra/scripts/funcs/overlay_shop_save_801db7f4.txt`.

### Retail quantity pickers (menu-overlay sub-screens)

The pause-shop's quantity screens are two sibling state machines in the
menu overlay, both sharing one pad decode - Right/Left step the quantity
by 1, Down/Up by 10, clamped to `[1, max]`, every step gated so walking
off either end is a silent no-op:

- **Buy** (`FUN_801DB7F4`): `max = min(gold / price, 99, 99 - held)`;
  the commit runs Point Card accrual, bag add, gold debit
  `price * qty`, and - only when the Point Card toast was shown - waits
  for a button press before returning to the buy list. Port:
  `engine-core::shop::BuyQuantitySession`.
- **Sell** (`FUN_801DBD94`, sub-screen `0x1F`): `max` = the staged bag
  slot's count; the commit credits `(price * qty) >> 1` gold (purse cap
  `9,999,999`) and applies a sell-list scroll fix-up (selling the last
  row while it sits alone on the final page steps the selection and
  scroll back); a whole-stack sale that empties the bag runs a
  `0x11`-unit delay and exits to the shop root instead of the sell
  list. Port: `engine-core::shop::SellQuantitySession` (+
  `sell_credit` / `apply_sale_gold` / `sell_list_fixup`).

Their sibling is the **buy recipient picker** (`FUN_801DB380`): before
the quantity screen the buy flow asks who the purchase is for - row 0
buys one copy into the bag, a party row runs an equippability check
(equip-record `+6` mask vs the per-character mask byte
`0x801E43F0[char]`; mismatch buzzes) and on a match buys **and equips
immediately**, returning the replaced piece to the bag; the purchase
itself never enters the bag. Same Point Card accrual and toast. Port:
`engine-core::shop::BuyRecipientSession`.

Both prices are the **item table's** halfword (`0x80074368 + id*0xC +
2`) - the retail item shop carries no per-shop gold price, which is
why the sell-side proceeds derive from the same table the buy list
shows. (The casino prize exchange's coin table at `0x801E4518` is a
separate system with its own stock records.) The sessions are not yet
wired into the hosts' menu flow (the `ShopQuantity` screen still
drives `ShopSession::set_quantity`).

### State-machine routing

The menu state machine (`engine-vm::menu`) owns the per-screen transition graph
(`commit_route` for Cross, `back_route` for Triangle); the `MenuHost` commit
hooks only apply side effects. The shop walks:

```
ShopBuy/ShopSell --Cross--> ShopQuantity --Cross--> ShopConfirm --Cross--> ShopBuy
       ^                          |                       |
       | Triangle (teardown)      | Triangle              | Triangle
   ShopExit  <----------- ShopBuy <--------------- ShopQuantity
```

Confirm (either Yes or No) routes back to the buy list so the player can shop
again; the only way out is Triangle from the list, which routes through the
transient `ShopExit` screen. `ShopExit` is auto-advancing: on entry it fires its
one-shot commit (clears the session via `MenuRuntimeHost::commit` / `cancel`),
holds for the render layer's fade (`transient_hold_frames`), then routes to the
menu's `Closing` state. The same routing drives the inn (`InnConfirm` Yes →
transient `InnSleep` fade → close; No → close).

## Key data structures

### `ShopItem` (`engine-core::shop`)

One item the shop offers:

| Field | Type | Meaning |
|---|---|---|
| `item_id` | `u8` | Item identifier (matches inventory slot IDs) |
| `price` | `u32` | Buy price in gold |

Sell price: `max(buy_price / 2, 1)`. Items not in the shop's buy list can still
be sold; their sell price is 1 gold.

### `ShopInventory` (`engine-core::shop`)

The set of items a particular shop stocks:

| Field | Type | Meaning |
|---|---|---|
| `shop_id` | `u8` | Opaque ID tying this stock list to a CDNAME scene block |
| `items` | `Vec<ShopItem>` | Ordered list of buy-side items |

### `ShopSession` (`engine-core::shop`)

Mutable state for one open shop interaction. Installed on
`MenuRuntime` by `open_shop` before the menu VM enters `ShopBuy`.

| Field | Type | Meaning |
|---|---|---|
| `inventory` | `ShopInventory` | The shop's stock list |
| `pending_item_id` | `Option<u8>` | Item cursor selected during current sub-flow |
| `pending_quantity` | `u8` | Quantity chosen at `ShopQuantity` |
| `pending_is_buying` | `bool` | `true` = buy, `false` = sell |

Key methods:
- `select_buy_item(cursor)` - set `pending_item_id` from buy list cursor
- `select_sell_item(cursor, sell_items)` - set `pending_item_id` from player inventory
- `set_quantity(slot)` - `pending_quantity = slot + 1`
- `try_buy(world_money) -> Option<(item_id, qty, gold_delta)>` - validates affordability; `gold_delta` is negative
- `try_sell(held_count) -> Option<(item_id, qty, gold_delta)>` - clamps to held quantity; `gold_delta` is positive

## Buy-list render layout

Traced from `FUN_801d5de0` (`overlay_shop_save.bin`). The buy list iterates up
to 8 visible rows (scroll managed by `_DAT_8007bb98` / `_DAT_8007bb90`), each
row rendered at a fixed vertical stride:

| Element | X offset (px) | Y stride (px) | Notes |
|---|---|---|---|
| Cursor `>` | +0 | - | Drawn only on the selected row |
| Item name | +20 (`0x14`) | +14 (`0x0E`) per row | `func_0x80036888` |
| Price | +112 (`0x70`) | same row | `func_0x80034b78`, 6-digit field |
| Gold footer | +0 | below last row | `func_0x80034b78`, 8-digit field |

Row colour logic (retail `_DAT_8007b454` palette index):
- **White** - normal affordable item.
- **Dim** - item is unaffordable (`price > gold`) or held count exceeds 98.
- **Blue** - item has an "equipped-comparison" flag set.

The quantity-selector sub-screen (`FUN_801d5510`) uses the same 14 px line
height, showing "Have N [item]" + "How many will you buy?" + a quantity×price
line at y+34 (`0x22`) from the panel top.

The sell-item detail panel (`FUN_801d5ae8`) shows item name, type description,
and sell price (buy price ÷ 2) at y+43 (`0x2b`) with an icon at x+84.

`engine-render::shop_draws_for` implements the above layout using these
confirmed constants. The cost prompt and Yes/No cursor are rendered in
`legaia-engine play-window` whenever `MenuState::ShopConfirm` is active.

## Mode-select panel (Buy / Sell / Quit)

The mode selector is menu-overlay **window 0x2A** in the window-descriptor
table at `0x801E4738` (see [field-menu.md](field-menu.md)): content rect
`(x 42, y 46, w 80, h 38)`, renderer VA `0x801D4868`. Like every window
content renderer it receives the live window struct and reads its content
origin from `+0xa` / `+0xc` (`WX` / `WY`); the 9-slice frame is caller-drawn.

`FUN_801d4868` (see `ghidra/scripts/funcs/overlay_shop_save_801d4868.txt`)
draws three rows through the shared string primitive
`func_0x80036888(str, 0, 0, x, y)`:

| Row | String (overlay rodata) | X | Y |
|---|---|---|---|
| Buy | `0x801CEB94` | WX + 20 (`0x14`) | WY |
| Sell | `0x801CEB9C` | WX + 20 | WY + 14 (`0x0E`) |
| Quit | `0x801CEBA4` | WX + 20 | WY + 28 (`0x1C`) |

Same 20 px text indent and 14 px line height as the buy list; the strings sit
at an 8-byte stride with a leading control byte. The CLUT-staging global
`_DAT_8007B454` (read only by the string primitive - see
[field-menu.md](field-menu.md)) is set to `7` (normal white) on entry; before
the Sell row the function scans the inventory id/count pair array at
`0x80085958` (`DAT_80084140 + 0x1818`, slot bounds `_DAT_8007B5EA` ..
`_DAT_8007B5EC` - the array pinned in [cheats.md](../reference/cheats.md))
and, when no slot has both a non-zero id **and** a non-zero count, clears the
global to `0` so **Sell renders dim when the bag is empty**.

After each row the cursor sprite `func_0x8002b994(0, mode, WX, rowY)` (the
16x16 bobbing menu cursor, drawn at the window origin X - the same "+0"
cursor column as the buy list) is gated on the picker cursor word
`DAT_801E46BC`:

- low 12 bits - selected row index (0 Buy / 1 Sell / 2 Quit); the cursor
  draws only on the matching row;
- bit `0x1000` - blink phase; the sprite mode argument is the inverted bit
  (1 = animated frame, 0 = static);
- bit `0x2000` - parked/unfocused presentation: the row-index gate is
  bypassed and every row gets a mode-4/0 draw keyed to the blink bit;
- bit `0x4000` - cursor suppressed entirely.

Input lives in the picker dispatcher `FUN_801dafd4` (its sub-state var is
`DAT_801E46AC`): the cursor clamp is a literal `li a1,0x3` at `0x801DB098`
(rows 0..2); on confirm, row 2 runs the Quit action at `0x801DB0D0`
(sound cue + session exit) and rows 0/1 fall through to the buy/sell check
at `0x801DB0E8`. The shop's window choreography is actor-VM widget scripts
interpreted by `FUN_801d6628` over the window table: the open script
`DAT_801E4E38` slides in windows `0x21` (vendor name) / `0x2A` (this picker)
/ `0x20` (gold) / `0x28` / `0x22`, and the Sell transition's close script
`DAT_801E4E54` slides away `0x28` / `0x2A` / `0x22` while keeping the gold +
vendor-name plates. (These instruction/descriptor words are byte-verified by
the randomizer's seru-trading vendor, which patches exactly these seams -
cursor clamp, a detour after the Quit text draw, and the window record's
height field - to grow the panel to four rows; see
`crates/rando/src/seru_overlay/consts.rs` and
[randomizer.md](../tooling/randomizer.md).)

## Gold-shop stock source

A gold town merchant's stock is **not** an overlay data table - it lives **inline
in the scene's field-VM script** (the MAN, asset type `0x03`), as field-VM op
`0x49` (`STATE_RESUME`) sub-op `0` carrying `[count][item_ids][ASCII name]`. The
`count` over-counts the purchasable stock by a trailing run of unsellable,
price-`0` *template* ids (the `Ra-Seru Meta $N` placeholders `0x01/0x02/0x03`, or
a lone `0x03`) that the on-screen shop skips - see the sellable-mask note below.
The shared scanner [`legaia_asset::shop_stock`] (a byte-scan, robust to the
dialogue-picker jump tables a linear walk desyncs on) locates these records;
[`legaia_engine_core::shop_catalog`] pairs them with item prices to build a priced
[`ShopInventory`]. `SceneHost::enter_field_scene` populates `World::scene_shops`
for the active scene, and `World::scene_shop_session(idx)` hands a host a
ready-to-open [`ShopSession`].

### Live trigger (op `0x49` sub-0)

Opening a merchant in-game is the field VM's own op `0x49` (`STATE_RESUME`).
On the Idle->arm edge the VM hands the host the instruction bytes
(`FieldHost::op49_menu_request`); `World::try_arm_field_shop` runs the same
sellable-mask-gated record validation directly on those bytes, and on a match
stages a priced `ShopSession` on `World::pending_field_shop` and arms the
op-0x49 tristate (so the script stays suspended exactly the way the name-entry
overlay suspends it). The host drains `World::take_pending_field_shop`, drives
the buy/sell UI (the engine's `MenuRuntime` shop screens), and calls
`World::finish_field_shop` when the player leaves - flipping the tristate
Armed -> Done so the field VM resumes past the merchant op. Non-shop op-0x49
sub-0 payloads (inn / save prompts carry MES text, not a priced item list) fail
the validation and arm nothing; with no `item_shop_data` installed (disc-free)
the path is inert. `play-window` wires this end to end (it opens the menu-runtime
shop on the pending signal and finishes on close).

Buy **prices** come from the static `SCUS_942.54` item table - the `u16` at item
record `+2` (`legaia_asset::item_names::item_price`, base `TABLE_BASE_VA`), the
same field the gold-debiting buy handler `FUN_801db380` reads (`_DAT_8008459C -=
price[item_id]`). A price of `0` marks a quest / key / found-only / internal item
the game never sells, so the price table doubles as a **sellable mask** (price
`> 0`) for the shop-record scan. The mask does double duty: a record must lead
with a sellable item (rejecting non-shop `0x49` payloads - inn / save prompts
carry MES text, not a priced list), and the trailing unsellable template-id
padding the `count` over-counts (the `Ra-Seru Meta $N` slots `0x01..=0x03`, which
*are* named but priced `0`) is trimmed out of the stock. Across the disc every
shop partitions cleanly - a leading priced run then an unsellable tail (≤3 ids),
never interleaved - and the priced prefix matches the curated walkthrough stock
(e.g. "Market" decodes to 10 ids but sells 7). Both the engine and the randomizer
now use this mask, so each surfaces exactly the real stock; the whole gold-shop
population decodes (earlier the "every id sellable" rule dropped every shop that
carried the padding). Validated against the Rim Elm Variety Store's 10 pinned ids
(a tail-less list) and the disc-wide partition guard.

> The casino / prize-exchange table at `0x801E4518` (8-byte `[u16 item_id][u16
> gate][u32 price]` records in `0x60`-byte blocks) is a different thing - its buy
> handler (`overlay_shop_save_801dc1cc.txt`) debits `_DAT_800845A4` (the **casino
> coin bank**, not party gold), so it is already parsed by the randomizer's
> `casino::CasinoExchange`. The prize-exchange UI is a **menu-overlay session**
> like the gold shop: a save state taken inside the ticket-counter prize shop
> holds `game_mode 0x17` (the CARD/menu pair, same as the pause menu) with the
> menu overlay PROT 0899 resident in slot A and the field overlay swapped out -
> while talking to the counter attendant the game is still field mode 3 under
> the field overlay (the dialog itself is not a menu session).

Retail enforces a max held count of 98 per item before dimming additional buy
attempts; the port mirrors the gate in the grant kernel
(`World::buy_from_shop` refuses a buy that would push the held count past
`shop::SHOP_HELD_CAP`).

## Open items

- **Mode-select panel - RESOLVED.** Full layout (window 0x2A rect, row
  geometry, empty-bag Sell dim, cursor-word bits, input dispatcher seams) is
  documented above (*Mode-select panel*).

## Relationship to `legaia_save`

Gold is stored at `_DAT_8008459C` in retail RAM and in `World::money` in the
engine. Inventory is a `HashMap<u8, u8>` (`item_id → count`) in `World::inventory`.
`SaveFile` / `SaveExt` round-trips both through the `LGSF v2` format.

## See also

**Reference** -
[Inn](inn.md) ·
[Level-up](level-up.md) ·
[Save screen](save-screen.md) ·
[Game-data tables](../reference/gamedata.md)
