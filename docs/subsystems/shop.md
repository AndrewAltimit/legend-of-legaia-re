# Shop Subsystem

Covers the buy / sell / quantity / confirm flow used whenever the player enters a
town shop. The shop UI lives inside the **menu overlay** - the same 129-function
binary that hosts the save screen and status screens. No separate shop overlay
exists. (The inn is *not* a menu-overlay session - see [inn.md](inn.md).)

Per-scene stock lives inline in the scene MAN's field-VM script and prices in
the static `SCUS_942.54` item table (see [Gold-shop stock
source](#gold-shop-stock-source) below); the menu overlay supplies the UI. The
buy list has no dedicated renderer of its own - see [Row layout: whose list this
is](#row-layout-whose-list-this-is).

## Flow overview

The retail engine enters the shop from the field-VM WARP / shop-trigger opcode.
The menu overlay dispatches on a sub-screen ID (pointer table at `0x801E4F40`,
same table used by the save screen). The shop sub-screens handle:

| Phase | Sub-screen | Description |
|---|---|---|
| Buy list | `ShopBuy` | Shows available items + prices. Cursor selects an item. |
| Sell list | `ShopSell` | Shows player inventory. Cursor selects an item to sell. |
| Quantity | `ShopQuantity` | Retail's in-place **stepper**, on both hosts: one number under the pad, bounded, committing with no confirm screen after it - see [the pickers below](#retail-quantity-pickers-menu-overlay-sub-screens). |
| Confirm | `ShopConfirm` | Yes / No prompt in the engine's menu graph; the live buy / sell path never enters it (the quantity stepper commits on its own). |
| Exit | `ShopExit` | Clears session, returns to field. |

Gold and inventory deltas are applied when the quantity stepper confirms -
retail has no Yes / No screen between the number and the transaction:
`MenuRuntime::tick_quantity` takes the stepper's `Bought` / `Sold` event to
`apply_quantity_buy` / `apply_quantity_sell`. `ShopInventory::try_buy` /
`try_sell` remain as kernels the live path does not call.

## Point Card

Item `0xFE` is a **third purse**, not a consumable: its own on-disc
description says it earns "points worth 5% of the price when you shop", and
the effect descriptor its subtype resolves to carries **neither** the field
nor a meaningful use arm - the item is passive while held.

The counter is `_DAT_800845B4` (u32, cap `9,999,999` - the same
`0x0098967F` clamp as the gold purse). The retail buy commit `FUN_801db7f4`
credits it **before** the gold debit: gated on the party holding `0xFE`
(bag-slot scan `func_0x80042f4c(0xFE)`), it adds `price / 20` per unit
bought - the `0xCCCCCCCD` reciprocal-multiply plus `srl 4` at
`0x801dbadc..0x801dbb10`, times the quantity. Sell transactions never accrue.
The recipient picker `FUN_801db380` applies the same accrual on both of its
purchase arms. `see ghidra/scripts/funcs/overlay_menu_801db7f4.txt`.

**The toast is window 31.** After crediting, the commit hands the widget-VM
a script whose entire body is `01 1F` plus the terminator - one command,
"open window `0x1F`" (`0x801E4EDC` from the quantity commit, `0x801E4EA8`
from the recipient picker), and then parks in a phase that returns to the
buy list only on a confirm / cancel press. Window 31's renderer
`FUN_801DCE20` prints the counter as an 8-digit field between a heading and
a "point(s)" unit label; see
[field-menu.md](field-menu.md#ported-painters).

The counter has a second, non-shop reader: the shared item-info panel
`FUN_801D0F1C` branches on the staged item id being `0xFE` and prints the
bank under a "Points Left" label instead of the accessory-passive lines, so
the pause **Items** screen shows the running total whenever the hand is on
the card.

Port: `World::minigames.point_card` is the bank, with `World::point_card_held` /
`World::credit_point_card` beside it (the accrual + clamp);
`engine-core::shop::{point_card_credit, apply_point_card}` are the
arithmetic kernels. `MenuRuntime` runs the accrual on the stepper's `Bought`
event (`arm_point_card_toast`) and on both recipient-picker arms, and holds
`MenuRuntime::point_card_toast` - the window-31 beat - until a press, with
the menu VM frozen behind it.

### What spends it

The bank has a debit arm, and it is not a shop screen. Entry `14` of the
effect-arm jump table `0x80014FA0` (`0x8004209C`) reads `_DAT_800845B4`,
does nothing when it is zero, otherwise takes `min(bank, 9999)`, subtracts
that from the bank and applies it as damage to a battle target - the
`0x801C9370` actor table, HP at `+0x14C`, the usual reaction-byte staging at
`+0x1DA`/`+0x1DC`. That is the Point Card **strike**, and it is why the
capture harness's `LEGAIA_POINT_CARD_MAX` knob one-shots bosses (see
[pcsx-redux-automation.md](../tooling/pcsx-redux-automation.md)).

Two things about that arm are worth keeping straight, because both cut
against the obvious reading:

- **No item on the disc selects arm 14 through its effect descriptor.**
  Decoding all 256 item records against the descriptor table yields classes
  `0..8`, `11..13`, `126..131` and nothing else. The Point Card's own
  descriptor is class `1`. So the arm is staged from the **battle** side -
  the battle-action caller passes the actor's `+0x1E8` byte as the selector,
  not a descriptor class - and the "jump table indexed by the descriptor
  class byte" model in
  [item-effect-table.md](../formats/item-effect-table.md) describes only the
  field item-use caller.
- **The Point Card's descriptor carries flag bit `0x40`**, and that bit is the
  descriptor's **target side**: set means the enemy party. The battle Item
  command reads it at `0x801D18E0` in `FUN_801D0748` and forks with `0x20`
  (all vs. one) into four target modes; the same fork runs on the spell table's
  `+2` byte at `0x801D1C50`. So the five `0x40` subtypes - the Point Card and
  the two summon-flute pairs - are simply the consumables that point at the
  enemies rather than the party, which is the same reason the Point Card's
  effect is staged from the battle side. Decoded in
  [item-effect-table.md](../formats/item-effect-table.md#0x40-is-the-target-side-and-it-has-exactly-one-reader).

### Retail quantity pickers (menu-overlay sub-screens)

The pause-shop's quantity screens are two sibling state machines in the
menu overlay, and **neither is a list**. Both share one pad decode on a
scalar at `DAT_801E46B4` - Right (`andi 0x2000`) `+1`, Left (`andi 0x8000`)
`-1`, Down (`andi 0x4000`) `+10`, Up (`andi 0x1000`) `-10`, clamped to
`[1, max]`, every step gated so walking off either end is a silent no-op.
Neither calls `FUN_801D688C`, the cursor-nav primitive the recipient picker
on the same screen does call, and neither reads a row table or a stride.
Confirm commits straight out of the stepper; there is no Yes / No
sub-screen between them and the transaction.

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

The scroll fix-up repairs a paged list's persisted `(scroll_top, selected)`
pair after a sale. The engine's shop lists page the kernel's way (Up / Down
wrap inside a page, Left / Right flip it -
`pause_screens::list_kernel_navigate_rows` over `shop::shop_list_page_rows`)
from one flat cursor whose page is derived, and the hand returns to its row
when a quantity stepper closes, clamped to the rebuilt list - the kind-4 list
keeps its selection behind the stepper. Selling away a lone last-page row so
leaves the hand on the new last row a page back, which is the fix-up's step.

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
separate system with its own stock records.)

Wiring: the **recipient picker is live**. `MenuRuntime`'s buy-list
commit runs the retail state-2 dispatch
(`engine-core::shop::buy_list_confirm_route`, `FUN_801DB21C` - the
affordability refusal beat plus the item-record `+0` kind switch), and
an equipment row opens `BuyRecipientSession` behind the
`MenuRuntime::retail_equipment_buy` opt-in - both play hosts enable it and
draw windows 36 / 25 / 41 over the parked buy list.
The two quantity **sessions** are the hosts' quantity screen.
`MenuRuntime::quantity_session` installs one the moment a list stages a
stack (`shop::QuantityPicker`) and takes the pad for the whole screen, the
way the recipient picker does; the buy list opens the buy picker, the sell
list the sell one, a cancel hands the pad back to the list it came from, and
a whole-stack sale that empties the bag still runs the exit delay back to
the shop root. `MenuRuntime::quantity_view` is what a host lays the window
out from, so neither host reads a list cursor for that screen and neither
builds rows for it.

The Point Card accrual and its window-31 toast *are* live on both the
quantity picker's own commit and the recipient picker - `MenuRuntime` owns the
gate and the beat, `World::minigames.point_card` the bank. It stays out of
`World::buy_from_shop` on purpose: that kernel is also the randomizer
runtime oracles' entry point, and retail's own kernel-equivalent (the
bag add plus the purse store, `FUN_801DB7F4` case 3) carries no accrual
either - the accrual is the sub-screen's, one phase earlier.

### State-machine routing

The menu state machine (`engine-vm::menu`) owns the per-screen transition graph
(`commit_route` for Cross, `back_route` for Triangle); the `MenuHost` commit
hooks only apply side effects. The shop walks:

```
ShopBuy/ShopSell --Cross--> ShopQuantity --Cross--> ShopConfirm --Cross--> ShopBuy
       |                          |                       |
       | Triangle                 | Triangle              | Triangle
   ShopMenu --Triangle--> ShopExit   ShopBuy          ShopQuantity
```

Confirm (either Yes or No) routes back to the buy list so the player can shop
again. Triangle from the buy or sell list returns to the top shop menu
(`ShopMenu`), and only Triangle there leaves, through the transient
`ShopExit` screen. `ShopExit` is auto-advancing: on entry it fires its
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

The live sale credits `sell_credit(price, qty) = (price * qty) >> 1`, the item
table's price halfword (not a per-shop price), floored, and capped at
`GOLD_CAP`; an item whose table price is `0` prints the cannot-sell line
instead. (`ShopInventory::sell_price`'s `max(buy / 2, 1)` belongs to the
unreached `try_sell`.)

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

## Row layout: whose list this is

**The layout below is the casino prize list's, not the shop's.** It was traced
from `FUN_801D5DE0`, which this page previously filed as the shop buy list on
the strength of the `overlay_shop_save.bin` dump filename. That filename names
the *image* the routine was dumped from, and that overlay carries menu and
casino code as well - it is not evidence about what the routine does. The same
mistake reached `crates/engine-menus/src/shop.rs` and the browser host's module
docs; both now carry the correction.

What the disassembly says: `FUN_801D5DE0` indexes the casino prize table
`0x801E4518` at `base + block*0x60 + row*8`, taking the block byte from the
entry-context pointer `_DAT_8007B450[1]`, and gates affordability on
`_DAT_800845A4` - the **coin bank**. The party gold purse `_DAT_8008459C`
appears nowhere in its 72 instructions. It is window 44's `renderer_va` in the
prize-exchange window set (43 tab / 44 list / 45 coin counter / 46 confirm).

The shop's own buy list has no dedicated renderer: it is a content-builder
list window, `FUN_80030628` case `0x0B`, drawn by the shared kind-4 list
kernel from row words the builder emits. Its geometry is therefore the
kernel's, not the prize list's - the strides below do not carry over. See
[the buy-list builder](#the-buy-list-builder-fun_80030628-case-0x0b) for the
row words themselves and
[field-menu.md](field-menu.md#the-kind-4-list-kernel-scus-fun_80032a44) for
the pens the kernel draws them at.

The prize list iterates up to 8 visible rows (scroll managed by
`_DAT_8007bb98` / `_DAT_8007bb90`), each row rendered at a fixed vertical
stride:

| Element | X offset (px) | Y stride (px) | Notes |
|---|---|---|---|
| Cursor | +0 | - | Hand sprite `FUN_8002B994`, gated by `_DAT_8007BB98` |
| Item name | +20 (`0x14`) | +14 (`0x0E`) per row | `func_0x80036888` |
| Price | +112 (`0x70`) | same row | `func_0x80034b78`, 6-digit field |

The row count is the byte at `DAT_801EF0D0` and each row indexes the prize
table through the row-order byte array at `DAT_801EF0E0`; the window renderer
draws **no currency footer** - the counter is its own window (45), and
`FUN_801D5DE0` reads the coin bank `_DAT_800845A4` only to decide a row's ink.

### The buy-list builder (`FUN_80030628` case `0x0B`)

The buy list is built once, at window create / content refresh, by the SCUS
content builder's case `0x0B` (`0x80030D48..0x80030F98`; jump table
`0x80010D38`, index `content_id - 2`). Its source is the field-VM
entry-context record `_DAT_8007B450` **directly** - `[+2]` the id count,
`[+3 + i]` the item ids - i.e. the same op-`0x49` sub-`0` stock record
[`legaia_asset::shop_stock`] scans off the scene MAN. Port:
`engine-core::menu_list_rows::{build_shop_buy_rows, shop_buy_row_order}`.

Each row word is `[class nibble][0x800 = dim][item id]`, and the dim bit is a
plain OR of two tests - `_DAT_8008459C < price` (item record `+2`) or a held
count that has stopped being `< 0x63`. The ink is **not** that bit alone: the
kind-4 list kernel `FUN_80032A44` stages it in its shared `0x3000` / `0xA000`
arm (`0x80033548..0x800335A0`), last rule wins - ink `7`; the dim bit makes it
`0` unless the list is parked (`_DAT_8007BB94 == 4`); class `0xA000` then makes
it `5` **even over a dim row**. Ink `5` is the teal pen (the CLUT row staging
value 5 selects). An earlier reading said this list had "no `0x400` alt-ink at
all" and so let the casino renderer's `shop_stock_row_ink` stand in for it;
that missed the class arm. Port: `engine-core::shop::shop_buy_row_ink`.

#### The last rows come first

The on-screen order is **not** the record order. The builder splits the walked
rows at `record_count - 3`: rows below the split stage into a scratch array at
`0x801C6220` tagged `0x3000`, rows at or above it are written straight into
the row buffer tagged `0xA000`, and the staged group is appended afterwards
(`0x80030F1C..0x80030F90`). The hoisted band is therefore a teal strip at the
top of the list.

The band's width is `3 - padding_len`, because a second filter decides how far
the emit loop walks. Ids below `0x1A` are skipped, and the same test first
*shrinks* the row count (`0x80030E10..0x80030E44`) - so the loop covers
`count - low_ids` entries. Every item id below `0x1A` carries price `0` in the
static item table, which is why retail's id-range filter and this page's
price-`> 0` sellable mask agree over the whole id space; it also means the
builder **depends** on the unsellable ids being a trailing run, since it walks
a prefix rather than filtering in place.

That closes the loop on the "template padding" the record `count`
over-counts: every record reserves three tail slots and pads the unused ones
with `Ra-Seru Meta $N`. On the retail disc the padding is 3, 1 or 0 ids, so the
band is 0, 2 or 3 rows wide, and the widths line up one-for-one with the `*`
markers the curated walkthrough tables carry
([gamedata.md](../reference/gamedata.md)). Rim Elm's Variety Shop is the worked
example: its record decodes ten ids with no padding, and hoisting the last
three reproduces the walkthrough's order (Hunter Clothes / Scarlet Jewel /
Azure Jewel first, then Survival Knife onward) exactly - for a party that can
see those three at all (next section).

#### The three-row tail is the Platinum Card's

Whether those last three record entries are walked at all is decided before
any of it, by two probes at `0x80030D54..0x80030DE8`: the held-count lookup
`FUN_80042F4C(0xFF)` and an eight-byte `0xFF` sweep of every present party
member's equipment block (`char + 0x196..+0x19D` - armour, head gear, weapon,
the Seru lock byte, leg gear and the three accessory slots). Either probe
answering non-empty keeps the tail; both empty subtracts `3` from the walk,
dropping the band.

Item `0xFF` is the **Platinum Card** (the item table at `0x80074368` names it,
next to the Point Card at `0xFE`), a Goods item - which is why the second probe
reads the equipment blocks. `FUN_80042F4C` returns the count byte of the first
bag slot in the active window whose id matches its argument, and the bag's
empty sentinel is id `0` ([inventory.md](inventory.md)), so the first probe is
"a Platinum Card is carried", not a free-slot test. The band is therefore the
card's exclusive stock: a record padded with three template ids loses nothing
without the card, and a shorter-padded one withholds two or three items. Across
the disc's shop records, the ones carrying a band are the ones a card-less
party sees shortened (`crates/engine-core/tests/shop_catalog_disc.rs` pins that
the card-less list is always the card list minus its band).

Retail confirms the gate. `scripts/pcsx-redux/autorun_shop_buy_list.lua`
opens Retock's "Items Shop" (P1 placement 36, a 13-id record with no template
padding) from the `retock_field_card_boot` state, whose party carries a
Platinum Card, and logs the builder at three breakpoints: the bag probe's
answer at `0x80030D5C`, the walk count `s4` at `0x80030E54`, and the emitted
row words at the exit jump `0x80030F94`. With the card in the bag the probe
answers `1`, the walk is `13`, and the three last record entries come first
with class `0xA000`. With the bag slot cleared before the conversation (no
party member wears one), the probe answers `0`, the walk is `10`, and the list
is the first ten entries in record order, all class `0x3000`. The unit test
`retock_items_shop_rows_match_the_retail_capture_with_and_without_the_card` in
`engine-core::menu_list_rows` pins both word lists.

This page used to call the probe "the held-count lookup for the empty-slot
marker id" and to say both probes are "all but always satisfied", with the port
passing `true`. Both were wrong: `0xFF` is an item, the retail captures in the
Mednafen save-state library hold no `0xFF` equipment byte in any party, and a party
without the card fails both probes. The band's reading as "new in this town",
taken from the walkthroughs' `*`, goes with it; the same `*` item appears in
more than one town's band.

Port: `engine-core::menu_list_rows::{shop_tail_rows_allowed,
build_shop_buy_rows}`, reached through `ShopInventory::from_stock_record`.
The field-VM merchant (`World::try_arm_field_shop`, the path both play hosts
open through `take_pending_field_shop`) runs the live probe over the party;
`shop_catalog::scene_shops`, which has no party, lists the band as a card
holder sees it. Before this, the merchant path built its rows in record order
with no probe at all, and only the catalog applied the hoist.

### Row ink is last-rule-wins, not first-match

Three tests run in a fixed order and each one **overwrites** the previous
verdict, so the ink is not a priority list:

1. ink starts at `7` (white);
2. held count not `< 0x63` (a stack at 99) -> `0`, grey;
3. stock record `+2` non-zero (the "already owned / restricted" marker)
   -> `6`, the accent pen - **even when the stack is full**;
4. `_DAT_800845A4 < price` -> `0`, grey - **even when the marker set `6`**.

Ported with the geometry constants as
`engine-core::shop::{shop_stock_row_ink, shop_cursor_mode}`. The prize
screen's `PrizeRow` is the caller of `shop_stock_row_ink`; the gold shop's buy
list inks its rows through `shop_buy_row_ink` (the `0x3000` / `0xA000` row-ink
arm of `FUN_80032A44`) inside `legaia_engine_screens::gold_shop_screen`.

The quantity-selector sub-screen (`FUN_801d5510`) is **window 35** of the
menu-overlay descriptor table (rect `(138, 100, 168, 50)`; the table is the
52 records at PROT 0899 file offset `0x15F20`, see
[field-menu.md](field-menu.md)). It uses the same 14 px line height, and its
three lines are the held line at the content origin, the prompt at `+0xE`,
and a value row at `+0x22`.

The value row reads `quantity / bound`, not `quantity x price`: the row's
second number call loads `DAT_801E46B8` (`0x801D563C`), which is the word
phase 0 fills with `min(gold / price, 99, 99 - held)` - the quantity
maximum - and a separator glyph (`FUN_8003C1F8` code `6`) prints between the
two. The unit price appears once, in the running total right-packed at
`WX + 0x62`, whose digit-field width is chosen from the magnitude of the
**unit price** rather than of the total (cascading compares against `99` /
`999` / `9999` giving 4..7 columns), so the number stays aligned as the
quantity climbs. A currency pictogram labels it at `(WX + 0x58, WY + 0x24)`.

Ported as `engine-ui::ui_menu_window_painters::buy_quantity_draws_for`,
beside its sell-side sibling, so one builder serves both hosts per window.
Window 37's total packs the other way - its pens move left as the field
widens, keeping the number's right edge on the box.

### Item detail / sell panel (`FUN_801D5AE8`)

**Window 39** of the same table, rect `(14, 95, 144, 53)`.

Rows off the window content origin: item name (record `+4`, ink `6`) at
`(WX, WY)`, description (record `+8`) at `WY + 0xE` - through the
line-breaking printer `FUN_80036888`, which drops a `0x7C` (`|`) break to
the next row `0xE` down, so a two-line description fills both rows above the
price, as it does in the buy-side info window 34 - then the price row at
`WY + 0x2B` - the "Price" label at `WX + 0x24` (ink `5`), the currency glyph
at `WX + 0x54`, and the value at `WX + 0x64` as a **5-digit** field. The sell
price is `buy_price >> 1`, exactly half; a `0` price replaces the whole row
with a "Cannot sell" string at `WX + 0x50` in ink `9`.

Below it the item's accessory passive prints twice over: its name (accessory
record `+4`, ink `4`) at `WY + 0x45` and its description (record `+8`, ink
`7`) at `WY + 0x55`. The index is **re-derived for each of the two draws**
rather than cached, through the same two-table chain both times - item record
`+0 == 1` reads the passive index from equipment record `+5`, anything else
from item-effect record `+3`, and an index `>= 0x40` is the no-passive
sentinel that suppresses the draw.

The whole body is gated on the staged id word `DAT_801E46B0` being
**positive**, with one exception: the `0x90 x 0x28` shade box at
`(WX, WY + 0x45)` draws unconditionally, so an empty panel is not an empty
rectangle. Ported as `engine-core::shop::{shop_sell_detail_panel,
item_passive_index}`; **both** hosts draw it for the sell list in place of the
buy-side info window 34, through one shared draw
(`legaia_engine_screens::sell_detail_window_draws`). The two windows are alternatives, not
siblings - their rects overlap and both print the name / description head - so a
host that drew 34 and 39 together would double that text rather than gain a
panel.

The equipment arm of that passive chain is **inert on an unmodified disc**:
every equipment bonus record carries the `0x40` sentinel in `+5`, so every
passive line this window prints in practice comes from the item-effect arm. See
[equipment-table.md](../formats/equipment-table.md) for the measurement and for
why the port's mirror of the column is row-keyed.

Both hosts compose the gold shop through
`legaia_engine_screens::gold_shop_screen` using these confirmed constants;
`engine-ui::ui_overlay::shop_draws_for` now draws only the inn and Seru-trade
fallback panels.

## Screen composition

A retail shop screen is a set of menu-overlay windows on a **black**
backdrop. The shop is a menu-overlay session (the field overlay is swapped
out), so nothing draws the scene: on open the field fades to black and the
windows slide in over it. Which windows are up is the widget scripts'
business (see [window-script.md](../formats/window-script.md)); what each
screen leaves on screen, in draw order, capture-confirmed on Retock's Items
Shop (`scripts/pcsx-redux/autorun_talk_to_npc.lua` driving the merchant, pad
steps scripted per screen):

| Screen | Windows | Scripts |
|---|---|---|
| Buy / Sell / Quit | 33 vendor plate, 42 picker, 32 purse, 40 buy list (parked), 34 item info | `0x801E4E38` |
| Buy list | 33, 32, 40, 34, 41 party compare | `0x801E4E64` |
| Buy quantity | 33, 32, 34, 41, 35 | `0x801E4EB0` (moves 40 off screen) |
| Buy recipient | 33, 32, 34, 41, 36 | `0x801E4E84` (moves 40 off screen) |
| Sell list | 33, 32, 38 sell list, 39 detail (plus its widget box) | `0x801E4E54`, `0x801E4EE4` |
| Sell quantity | 33, 32, 38, 39, 37 | `0x801E4F08` |

The buy-list script never closes the picker: window 41's frame covers it.
The Point Card toast (window 31) rides over whichever set is up. The root
screen shows the buy list **parked** - no hand, no page triangles, every row
in its pen - which is why its stock reads white there and greys only once the
hand enters the list. Window 41 shows the hovered item's party compare for the
whole buy flow, not only beside the recipient picker; an accessory the member
already wears prints the "Equipped" note in the green pen (ink 4).

Both lists are pages of the kind-4 kernel's geometry
([field-menu.md](field-menu.md#the-kind-4-list-kernel-scus-fun_80032a44)):
window 40 pages 7 rows (name `WX + 0x18`, a 5-digit price at `WX + 0x80`),
window 38 pages 11 (name `WX + 0xC`, a 3-cell count at `WX + 0x6C`), each
under a PAGE header, with the hand at `WX - 6`; Up / Down wrap inside the
page and Left / Right flip it. Window 39's price is a 5-digit field at
`WX + 0x64`. The page triangles (UI-icons `0x27` / `0x28`, 8x8) draw only
while the list itself has the pad - not on the root screen's parked list, not
under a quantity stepper - and blink: the kernel tests its frame word
`0x80084570 & 0x18` and draws them while it is non-zero (`0x80032FC0`), so
they are off eight frames in every thirty-two.

**Numbers.** Every price, count, stat, purse and page number is a sprite, not
a dialog-font glyph. The fixed-width number primitive `FUN_80034B78` blits
8x12 numerals off the menu-glyph page (`uv = (d * 8, 208)`, the cells the
battle HUD reads) through the staged ink's palette; the PAGE header is the
UI-icon tag `0x76` plus 6x8 digits `0x7A..=0x83` and slash `0x79` on the
system-UI sheet. The pause menu's numbers go through the same primitive, so
the port attaches all of these to the font atlas as sprite cells
(`save_menu_atlas::menu_font_cells`, `legaia_font::Font::with_sprite_cells`)
and every fixed-cell number builder in `engine-ui` draws them, tinted by the
ink, on both hosts.

A buy-list row whose stack is already at 99 is **refused at the list**: the
kernel tests the row word's `0x800` bit on confirm and buzzes before the
sub-screen's own state-2 dispatch runs, so the hand stays on the row
(capture-confirmed: a 99-stack row neither opens the stepper nor buys).

Port: `engine-core::shop::{ShopScreenPhase, shop_screen_windows,
party_compare_members}` and `MenuRuntime::{shop_screen_phase,
covers_field}`; `engine-ui::shop_screen` composes the frames (window 33
wears the carved plaque), the picker, the paged list, window 41 and the
painters' hand / currency-pictogram sprites, and
`legaia_engine_screens::gold_shop_screen` projects the live session into it,
so the native window and the browser play page draw one screen through one
call. Both clear to black under a shop.

The casino prize counter is the same stack: on black, the "Exchange" plaque
(window 43, kind 2), the framed prize list (44) and coin counter (45), and the
purchase confirm (46) while it is up (the `casino_prize_shop` library state).
Both hosts frame it through `engine-ui::shop_screen::prize_screen_draws`.

**Slides.** Stepped one vsync at a time, every window that joins a screen
leaves its descriptor's park edge (`+0x1`: bottom, left, top, right) from just
off screen and lands home **eleven frames** later at a constant speed, all of
a screen's new windows together; a window that leaves travels back out the
same way. The port keeps the timing in `shop::ShopSlides` (stepped by the menu
tick toward the screen's set) and moves each window's draws with it
(`engine-ui::shop_screen::apply_shop_slides`).

**Fade.** Before the first slide the field fades to black: linear, from the
last dialogue frame to black in fourteen frames (the same per-vsync capture).
The port runs it from the merchant open (`MenuRuntime::shop_fade_level`, a
subtractive full-screen quad over the frozen field on both hosts) and starts
the slides when it lands. Retail then holds black for about fifty frames while
the menu overlay loads before the windows move; that load wait is not
reproduced.

**After a quantity screen** the hand returns to the list row it left - the
kind-4 list keeps its selection behind the stepper - clamped to the rebuilt
list after a sale.

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

The scan sits *between* the Buy draw and the Sell draw, and nothing restores
the global afterwards, so an empty bag greys **Sell and Quit together** - Buy
is the only row that is always white. Ported as
`engine-core::shop::shop_root_command_rows`, which both hosts consult for the
Sell row's ink.

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

The stock list `FUN_801D5DE0` re-runs the identical four-way decode against
its own word `_DAT_8007BB98`; the shared kernel is
`engine-core::shop::shop_cursor_mode`.

Input lives in the picker dispatcher `FUN_801dafd4` (its sub-state var is
`DAT_801E46AC`): the cursor clamp is a literal `li a1,0x3` at `0x801DB098`
(rows 0..2); on confirm, row 2 runs the Quit action at `0x801DB0D0`
(sound cue + session exit) and rows 0/1 fall through to the buy/sell check
at `0x801DB0E8`. The shop's window choreography is actor-VM widget scripts
interpreted by `FUN_801d6628` over the window table
([format page](../formats/window-script.md); parser
`legaia_asset::widget_script`; the engine runs the same disc programs on
the same transitions via `engine-core::menu_widget`): the open script
`DAT_801E4E38` slides in windows `0x21` (vendor name) / `0x2A` (this picker)
/ `0x20` (gold) / `0x28` / `0x22`, and the Sell transition's close script
`DAT_801E4E54` slides away `0x28` / `0x2A` / `0x22` while keeping the gold +
vendor-name plates. (These instruction/descriptor words are byte-verified by
the randomizer's seru-trading vendor, which patches exactly these seams -
cursor clamp, a detour after the Quit text draw, and the window record's
height field - to grow the panel to four rows; see
`crates/code-hooks/src/seru_overlay/consts.rs` and
[randomizer.md](../tooling/randomizer.md).)

## Gold-shop stock source

A gold town merchant's stock is **not** an overlay data table - it lives **inline
in the scene's field-VM script** (the MAN, asset type `0x03`), as field-VM op
`0x49` (`STATE_RESUME`) sub-op `0` carrying `[count][item_ids][ASCII name]`. The
`count` over-counts the purchasable stock by a trailing run of unsellable,
price-`0` *template* ids (the `Ra-Seru Meta $N` placeholders `0x01/0x02/0x03`, or
a lone `0x03`) that the on-screen shop skips - see the sellable-mask note below.
The shared scanner [`legaia_asset::shop_stock`] (a byte-scan, independent of
how the script reaches the op) locates these records - the largest, `rayman2`'s
"Items Shop 1", declares seventeen ids (fourteen sellable plus the three-id
template tail), so a record bound of sixteen hid one shop from every consumer;
[`legaia_engine_core::shop_catalog`] pairs them with item prices to build a priced
[`ShopInventory`]. `SceneHost::enter_field_scene` populates `World::shops.scene_shops`
for the active scene, and `World::scene_shop_session(idx)` hands a host a
ready-to-open [`ShopSession`].

### Recipe: reading a town's stock off a disc

```bash
asset shop-stock --prot extracted/PROT.DAT --scus extracted/SCUS_942.54 \
                 --cdname extracted/CDNAME.TXT [--scene bylon | --entry 53] [--json]
```

The command joins the two files this stock is spread across - the scene MAN
inside `PROT.DAT` for the ids, and `SCUS_942.54`'s item record table for the
names and prices - and prints one block per shop. Full walkthrough:
[extracting-assets.md](../guides/extracting-assets.md#shop-inventories---a-table-joined-from-two-files).

Three things the output is deliberately explicit about, each a trap when the
same thing is done by hand:

| Trap | What the tool does |
|---|---|
| `count` over-counts by the unsellable template tail | prints both numbers ("decodes 10 ids, sells 7") and flags the tail rather than truncating it |
| a linear opcode walk desyncs on a confirm-picker's option-jump table and silently misses shops (Biron Monastery's Corey) | byte-scans for the op-`0x49` signature; never walks |
| CDNAME `#define <name> N` names extraction entry `N − 2`, so a filename label can disagree with the retail scene at a block edge | resolves the scene column through `block_for_extraction_index`; the entry column stays extraction-space |

The curated walkthrough shop tables and this command are two halves of one
check: the tables are human-readable ground truth, the command is the decoded
record. Agreement confirms both.

### Live trigger (op `0x49` sub-0)

Opening a merchant in-game is the field VM's own op `0x49` (`STATE_RESUME`).
On the Idle->arm edge the VM hands the host the instruction bytes
(`FieldHost::op49_menu_request`); `World::try_arm_field_shop` runs the same
sellable-mask-gated record validation directly on those bytes, and on a match
stages a priced `ShopSession` on `World::shops.pending_shop` and arms the
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

Retail fills a stack at **99** per item id, and both gates carry the same
`0x63` literal: the buy-list row builder dims a row once the held count
stops being `< 0x63` (`sltiu v0,v0,0x63` at `0x80030f0c` / `ori s0,s0,0x800`
at `0x80030f18`, shop-row case of `FUN_80030628` - see
`ghidra/scripts/funcs/80030628.txt`, recomp-corroborated), and the
buy-quantity maximum clamps to `min(gold/price, 99, 99 - held)`
(`slti v0,v0,0x64; li v0,0x63` at `0x801db89c..0x801db8a4` and
`li a0,0x63; subu a0,a0,v1` at `0x801db8d0..0x801db8dc` in `FUN_801DB7F4` -
see `ghidra/scripts/funcs/overlay_menu_801db7f4.txt`). The port mirrors the
gate in the grant kernel (`World::buy_from_shop` refuses a buy that would
push the held count past `shop::SHOP_HELD_CAP` = 99; the picker side is
`shop::buy_qty_max`).

### The quantity screen is a stepper

The screen the hosts draw is retail's: one number, bounded, moving under
Right / Left (by one) and Down / Up (by ten), each step gated so walking off
either end is a silent no-op, and a confirm that commits straight into the
transaction. `MenuState::ShopConfirm` is no longer reached from the shop's
buy or sell flow at all - retail has no Yes/No screen between the number and
the sale, and the port now has none either.

Two earlier readings are worth keeping so they are not re-derived. The
first had the shapes swapped - the "nine-row list whose cursor is the
quantity" was taken for retail's and the stepper for the engine's; it is the
other way round, and the nine was not retail's number at all. The second
survived the bound being fixed: with the row count corrected to retail's own
maximum the numbers agreed, which made the screen look finished while the
interaction was still a different one. A shared bound is not a shared
screen.

## Sound

Every shop screen keys its own blips, and none of them is the pause menu's
edge rule. The buy list (built by `FUN_80030628` case `0x0B`) and the sell
list are paged by the kind-4 list kernel `FUN_80032A44`, which pushes a cursor step `0x21` only when the hand
actually moved, a confirm `0x20` on an enabled row or the dim-row buzz
`0x23` on a dim one (a buy the purse cannot cover), and a cancel `0x37`.
The Buy / Sell / Trade picker is given the same rule: its renderer
`FUN_801D4868` keys no cue of its own, and that it is a kernel-paged list is
an inference rather than a traced call. The sub-screens add their own ring
writes:

| Screen | Step | Commit | Cancel |
|---|---|---|---|
| Buy quantity `FUN_801DB7F4` | `0x21`, behind each bound test (`0x801DB9B0` and siblings) | `0x2C`, through the overwrite producer `FUN_80035BD0` (`0x801DB940`) | `0x37` (`0x801DB970`) |
| Sell quantity `FUN_801DBD94` | `0x21` (`0x801DC064` and siblings) | `0x36` (`0x801DBE78`) | `0x37` (`0x801DC020`) |
| Buy recipient `FUN_801DB380` | the kernel's `0x21` | `0x2C` into the bag (`0x801DB480`), `0x24` buy-and-equip (`0x801DB5C8`), `0x23` a member who cannot wear it (`0x801DB5AC`) | the kernel's `0x37` |

The recipient picker's commits use `FUN_80035BD0` because the list kernel has
already pushed its own `0x20` for the same press: the overwrite replaces it,
so the player hears one cue. The port raises the final cue of a tick from
`MenuRuntime::tick` (`MenuRuntime::take_ui_cue`) and both play hosts key it
through their SFX channel. The quantity commit sounds one tick after the press
in the port, on the tick the transaction lands. The casino prize counter's
cues are not modelled.

## Open items

- **Mode-select panel - RESOLVED.** Full layout (window 0x2A rect, row
  geometry, empty-bag Sell dim, cursor-word bits, input dispatcher seams) is
  documented above (*Mode-select panel*).

## Relationship to `legaia_save`

Gold is stored at `_DAT_8008459C` in retail RAM and in `World::party.money` in the
engine. Inventory is the `ItemBag` over retail's 256 slots in `World::party.inventory`
([inventory.md](inventory.md)). `SaveFile` / `SaveExt` round-trip both through
LGSF (format version 4, with the optional `LGX6` slot-level block).

## See also

**Reference** -
[Inn](inn.md) ·
[Level-up](level-up.md) ·
[Save screen](save-screen.md) ·
[Game-data tables](../reference/gamedata.md)
