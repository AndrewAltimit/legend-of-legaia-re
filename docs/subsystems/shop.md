# Shop Subsystem

Covers the buy / sell / quantity / confirm flow used whenever the player enters a
town shop. The shop UI lives inside the **menu overlay** - the same 129-function
binary that hosts the save screen, inn, and status screens. No separate shop
overlay exists.

Per-scene item lists and prices are encoded in the menu overlay DATA segment
(see [Open items](#open-items) below). The buy-list render layout is traced from
`FUN_801d5de0` in `overlay_shop_save.bin`.

## Flow overview

The retail engine enters the shop from the field-VM WARP / shop-trigger opcode.
The menu overlay dispatches on a sub-screen ID (pointer table at `0x801E4F40`,
same table used by the save screen and inn). The shop sub-screens handle:

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

## Open items

- **Per-scene item tables.** The retail shop stocks are encoded in the menu
  overlay's DATA segment at `0x801E4518` (8-byte strides, 0x60 bytes per
  scene; dispatched by `FUN_801DC6B4`). Locating the per-shop item list
  requires tracing the scene-index → DATA offset mapping.
- **Quantity cap.** Retail enforces a max held count of 98 per item before
  dimming additional buy attempts; the current port allows unlimited stacking.
- **Mode-select panel.** The Buy / Sell / Quit selector (`FUN_801d4868`) uses
  x+20 for text, 14 px line height - same constants as the item list.

## Relationship to `legaia_save`

Gold is stored at `_DAT_8008459C` in retail RAM and in `World::money` in the
engine. Inventory is a `HashMap<u8, u8>` (`item_id → count`) in `World::inventory`.
`SaveFile` / `SaveExt` round-trips both through the `LGSF v1` format.

## See also

**Reference** —
[Inn](inn.md) ·
[Level-up](level-up.md) ·
[Save screen](save-screen.md) ·
[Game-data tables](../reference/gamedata.md)
