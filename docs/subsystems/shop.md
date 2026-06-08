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

## Gold-shop stock source

A gold town merchant's stock is **not** an overlay data table — it lives **inline
in the scene's field-VM script** (the MAN, asset type `0x03`), as field-VM op
`0x49` (`STATE_RESUME`) sub-op `0` carrying `[count][item_ids][ASCII name]`. The
`count` over-counts the purchasable stock by a trailing run of unsellable,
price-`0` *template* ids (the `Ra-Seru Meta $N` placeholders `0x01/0x02/0x03`, or
a lone `0x03`) that the on-screen shop skips — see the sellable-mask note below.
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
`World::finish_field_shop` when the player leaves — flipping the tristate
Armed -> Done so the field VM resumes past the merchant op. Non-shop op-0x49
sub-0 payloads (inn / save prompts carry MES text, not a priced item list) fail
the validation and arm nothing; with no `item_shop_data` installed (disc-free)
the path is inert. `play-window` wires this end to end (it opens the menu-runtime
shop on the pending signal and finishes on close).

Buy **prices** come from the static `SCUS_942.54` item table — the `u16` at item
record `+2` (`legaia_asset::item_names::item_price`, base `TABLE_BASE_VA`), the
same field the gold-debiting buy handler `FUN_801db380` reads (`_DAT_8008459C -=
price[item_id]`). A price of `0` marks a quest / key / found-only / internal item
the game never sells, so the price table doubles as a **sellable mask** (price
`> 0`) for the shop-record scan. The mask does double duty: a record must lead
with a sellable item (rejecting non-shop `0x49` payloads — inn / save prompts
carry MES text, not a priced list), and the trailing unsellable template-id
padding the `count` over-counts (the `Ra-Seru Meta $N` slots `0x01..=0x03`, which
*are* named but priced `0`) is trimmed out of the stock. Across the disc every
shop partitions cleanly — a leading priced run then an unsellable tail (≤3 ids),
never interleaved — and the priced prefix matches the curated walkthrough stock
(e.g. "Market" decodes to 10 ids but sells 7). Both the engine and the randomizer
now use this mask, so each surfaces exactly the real stock; the whole gold-shop
population decodes (earlier the "every id sellable" rule dropped every shop that
carried the padding). Validated against the Rim Elm Variety Store's 10 pinned ids
(a tail-less list) and the disc-wide partition guard.

> The casino / prize-exchange table at `0x801E4518` (8-byte `[u16 item_id][u16
> gate][u32 price]` records in `0x60`-byte blocks) is a different thing — its buy
> handler (`overlay_shop_save_801dc1cc.txt`) debits `_DAT_800845A4` (the **casino
> coin bank**, not party gold), so it is already parsed by the randomizer's
> `casino::CasinoExchange`.

## Open items

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
