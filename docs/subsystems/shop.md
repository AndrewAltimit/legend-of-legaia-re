# Shop Subsystem

Covers the buy / sell / quantity / confirm flow used whenever the player enters a
town shop. The shop UI lives inside the **menu overlay** — the same 129-function
binary that hosts the save screen, inn, and status screens. No separate shop
overlay exists.

Exact per-scene item lists, prices, and the full buy-menu render layout are
pending a complete trace of `overlay_shop_save` (the menu overlay). The
clean-room port (`engine-core::shop`) supplies the session state machine;
once the overlay is traced the item tables can be wired in.

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
- `select_buy_item(cursor)` — set `pending_item_id` from buy list cursor
- `select_sell_item(cursor, sell_items)` — set `pending_item_id` from player inventory
- `set_quantity(slot)` — `pending_quantity = slot + 1`
- `try_buy(world_money) -> Option<(item_id, qty, gold_delta)>` — validates affordability; `gold_delta` is negative
- `try_sell(held_count) -> Option<(item_id, qty, gold_delta)>` — clamps to held quantity; `gold_delta` is positive

## Engine-render overlay

`engine-render::shop_draws_for(session, cursor, world)` returns a list of
`DrawCall` values for the current shop state. The cost prompt and Yes/No cursor
are rendered in `legaia-engine play-window` whenever `MenuState::ShopConfirm`
is active.

The full 4-column buy-menu layout (item name, item icon, quantity, price) and
the sell-mode item list mirror the retail overlay; exact pixel offsets are
pending the overlay trace.

## Open items

- **Per-scene item tables.** The retail shop stocks are encoded in the menu
  overlay's DATA segment (function-pointer table at `0x801E4F40`, dispatched
  by `FUN_801DC6B4`). Locating the per-shop item list requires tracing from
  the sub-screen that handles `ShopBuy` entry through its scene-specific data
  reference.
- **Render layout.** The 4-column buy-menu and sell-list render are pending
  the overlay binary capture (`overlay_shop_save`; see §3.1 of the PRD).
- **Quantity cap.** Retail may enforce a per-item inventory cap; the current
  port allows unlimited stacking.

## Relationship to `legaia_save`

Gold is stored at `_DAT_8008459C` in retail RAM and in `World::money` in the
engine. Inventory is a `HashMap<u8, u8>` (`item_id → count`) in `World::inventory`.
`SaveFile` / `SaveExt` round-trips both through the `LGSF v1` format.
