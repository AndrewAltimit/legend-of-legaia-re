# Expansion pack: items, equipment and shops

`items.toml` adds new consumables and equipment to an [expansion pack](expansion-pack.md), groups equipment into sets with bonuses for wearing them together, and opens the pack's own shops. The parser is `legaia_expansion_format::items`; the runtime is `legaia_engine_expansion::{items, item_screen}`, wired to the `World` by `engine-core`'s `world::expansion_items`. Every hook is the identity with no pack active (or a pack with no items): nothing is compiled, no stat changes, no alias is lent, and a save carries no item record.

## At a glance

| Property | Value |
|---|---|
| File | `items.toml` at the pack root; optional |
| Declares | `item`, `item_set` and `shop` ids |
| Referenced from | `battle.toml` drops, steals and boss rewards; `progression.toml` level rewards; `quests.toml` rewards and `collect_item` objectives; map `shop` triggers and shop NPCs |
| Storage | Beside the retail bag, keyed by item id; saved as tail record `4` of the [expansion block](expansion-pack-quests.md#saves) |
| Screens | The pack item screen (shop counter, bag, equip compare) and the Journal's Items page |
| Sample | [`packs/rebirth/items.toml`](../../packs/rebirth/items.toml) |

## Where pack items live

Retail's item id is a `u8`. The static item table ([item-table.md](item-table.md)) names 250 of its 256 ids, and the six it leaves blank are `0x00` ("no item") and five cut slots: `0x12`, `0x1A`, `0x52`, `0xB9` and `0xFD`. Every other id is a retail item the player can hold. A pack item cannot take a retail id without displacing a real item, and five spare ids are too few for a gear set.

So a pack item is not a retail id. The engine keeps pack items in their own store, keyed by their local `items.toml` key, beside the retail structures rather than inside them:

| What | Retail | Pack |
|---|---|---|
| The bag | 256 `[id, count]` slots at `0x80085958` ([inventory.md](../subsystems/inventory.md)) | `ItemProgress::bag`, key -> count (stack cap 99) |
| Worn gear | The record's eight equipment bytes, `+0x196..+0x19D` ([save-record.md](save-record.md)) | `ItemProgress::worn`, `(roster slot, equipment byte)` -> key |
| Saved in | The SC block | The pack's expansion block, tail record `4` |

Neither retail structure ever holds a pack item, which is what keeps a retail save's bytes and a no-pack session untouched. The trade is that a pack item reaches the game only through the hooks below; the retail pause menu's Items and Equip screens do not list pack items, and the pack item screen does.

### Battle aliases

A fight is the one place a pack item needs a `u8`: a monster's drop byte, a steal entry and an Item command row are all item ids. For the length of a fight the engine lends the five cut ids (`items::ALIAS_POOL`) to the pack items that take part in it - first each pack drop and steal of the seated monsters, then each battle-usable pack consumable the party holds - and installs a catalog entry for each so the drop roll, the steal, the Item command, the item's handler and the results window all work on it unchanged. A grant or a consume of an alias goes to the pack bag (`World::grant_item_id`, `consume_item`). The aliases are returned at the next fight's prepare and at deactivation, not at the battle's end, because the results window names the drop after the fight closes.

An id the retail side already uses (a patched disc that named one) is skipped. At most five pack items take part in one fight; past that, a drop, steal or Item row of a further pack item is left out of that fight.

## `items.toml`

```toml
[items.rebirth_blade]
name = "Rebirth Blade"
description = "Forged where the Mist|was torn away."
kind = "weapon"
icon = { kind = "item", id = 0x27 }
characters = ["vahn"]
attack = 112
set = "rebirth"

[items.phoenix_plume]
name = "Phoenix Plume"
description = "Revive every fallen|ally at half HP."
kind = "consumable"
price = 12000
effect = { kind = "revive", percent = 50 }
all_party = true

[sets.rebirth]
name = "Rebirth"
bonuses = [{ pieces = 2, attack = 10 }, { pieces = 4, passives = [0x24] }]

[[shops]]
id = "tower_armory"
name = "Tower Armory"
stock = ["phoenix_plume", 0x79, { item = "rebirth_crown", requires = { bosses = ["iron_regent"] } }]
```

### `[items.<key>]`

| Key | Default | Meaning |
|---|---|---|
| `name` | - | The menu name, at most 20 characters. |
| `description` | empty | The info line; `\|` breaks it. |
| `kind` | `consumable` | `consumable`, `weapon`, `armor`, `helmet`, `boots`, `accessory` or `key`. |
| `icon` | none | A retail item (`DiscRef::Item`) whose look the item borrows: its icon, and for equipment the mesh and swing a battle reads. |
| `price` | `0` | Buy price in gold; `0` is never sold. |
| `sell` | half `price` | Sell price; `0` cannot be sold. |
| `characters` | everyone | Who may wear it: `vahn`, `noa`, `gala`. |
| `attack`, `defence`, `udf`, `ldf`, `speed`, `intelligence` | `0` | Flat bonuses while worn, `-255..=255`; `defence` adds to both `udf` and `ldf`. |
| `element` | none | `0..=6`: the wearer guards that element, the retail elemental-guard passive `0x1D + element`. |
| `passives` | none | [Accessory passive](accessory-passive-table.md) indices `0x00..=0x3F` granted while worn. |
| `set` | none | The `[sets.<key>]` the item belongs to. |
| `effect` | - | A consumable's effect (below). |
| `all_party` | `false` | A consumable applies to every ally at once. |
| `field`, `battle` | `true` | Whether a consumable is usable from the field and in battle. A `damage` effect is battle only. |

The equipment byte a worn item takes follows the record's own layout: armour `0`, head gear `1`, the weapon `2` (Noa's `3`: her Ra-Seru sits at `2`), footwear `4`, and the three Goods slots `5..=7` for an accessory.

### Effects

Each effect is one of the engine's retail item handlers (`legaia_engine_menus::items::ItemEffect`), so a pack consumable behaves exactly as a retail item with that handler would.

| `effect` | Handler |
|---|---|
| `{ kind = "heal_hp", amount = N }`, or `full = true` | `Heal` / `HealAll` |
| `{ kind = "heal_mp", amount = N }`, or `full = true` | `HealMp` / `HealMpAll` |
| `{ kind = "revive", percent = P }` | `Revive` at `P`% of max HP (`1..=100`) |
| `{ kind = "cure", status = "venom" }` | `Cure` one of `venom`, `toxic`, `numb`, `sleep`, `confuse`, `rot`, `curse`, `stone` |
| `{ kind = "cure_all" }` | `CureAll` |
| `{ kind = "damage", amount = N }` | `Damage` to one enemy |
| `{ kind = "as_item", item = 0x8E }` | Retail item `item`'s own handler and targeting: a stat raise, a one-battle buff, Fury Boost |

`all_party` fans heals, cures and revives across the party, as the retail party heals do.

### `[sets.<key>]`

`name` and `bonuses`, a list of tiers. A tier applies while at least `pieces` (two or more) items of the set are worn by one character, and tiers stack. A tier carries the same stat keys as an item (`attack`, `defence`, `udf`, `ldf`, `speed`, `intelligence`) and `passives`.

### `[[shops]]`

`id`, `name` and `stock`, the rows in order. A row is an item reference, or `{ item, requires, price }`: `requires` is the quest [prerequisite](expansion-pack-quests.md#queststoml) shape and lists the row only while it holds; `price` overrides the item's. A retail item sells at its retail price (the item record's `+2` halfword) unless the row sets one.

## Item references

Everywhere a pack names an item it writes an `ItemRef`:

| Form | Example | Names |
|---|---|---|
| An integer | `0x79` | Retail item id |
| An item coordinate | `{ kind = "item", id = 0x79 }` | The same retail item |
| A string | `"rebirth_blade"`, `"rebirth:rebirth_blade"` | A pack item; `other:item` names a required pack's |

The form is accepted in `battle.toml` monster `drop` and `steal` and boss `reward_items`, `progression.toml` `level_rewards`, `quests.toml` `rewards.items` and `collect_item` objectives, and shop `stock`. A pack key must be defined in `items.toml`; a retail id is a disc coordinate the resolver checks.

## Runtime

| Path | What happens | Where |
|---|---|---|
| Grants | A quest reward, a boss reward, a level-up reward: a retail id to the retail bag, a pack item to the pack bag with a `Got X` toast | `World::grant_item_ref` |
| Counts | A `collect_item` objective counts the bag the reference names; a hand-in takes from it | `World::held_item_count`, `take_item_ref` |
| Drops and steals | A pack monster's pack-item drop or steal rides a battle alias through retail's drop roll and steal | `World::expansion_lend_item_aliases` |
| Battle Item command | Held battle-usable pack consumables join the list under their aliases | `build_battle_item_session` |
| Field use | The pack bag's Items tab runs the item's handler on a party member | `World::use_pack_item` |
| Wearing | Wearing a pack item empties its retail byte (the retail item there goes to the retail bag) and records the pack item | `World::equip_pack_item` |
| Stat fold | The battle seed adds worn pack gear the way it adds retail gear | `World::seed_party_battle_stats` |
| Passives | Worn pack passives and set tiers join the record's ability bits | `World::refresh_party_ability_bits` |
| Look | A worn pack item's `icon` fills its empty byte for the battle mesh, swing costs and command chips | `World::member_look_equipment` |
| Reconcile | A retail item later equipped over a pack item (the retail equip screen, Best Equipment) sends the pack item back to the bag | `World::reconcile_pack_gear` |

### The stat fold

The battle seed treats a worn pack item as retail treats a worn retail item ([battle-formulas.md](../subsystems/battle-formulas.md)): its ATK goes to its byte's per-swing attack (folded at half per hit for the command that reads that slot, or summed for an art), its UDF, LDF and SPD into the seeded lines, and its passives into the record's ability bits, where the status and element guards, the per-turn passives and the max-HP boost read them. A percent stat boost (`0x00..=0x0C`) a pack item grants is applied from the base stats once, unless retail gear already grants the same index. Set-tier and accessory ATK join the base attack line. INT stays menu-only, as retail head gear's does.

### The item screen

The pack item screen owns the frame under `SceneMode::Expansion`, as the card screen does, and both play hosts draw it through the same seat (`engine-screens::expansion_modes`, `engine-ui::expansion_modes::items`).

| Opened by | Screen |
|---|---|
| A map's `{ kind = "shop", shop = "<id>" }` trigger, or an NPC whose `shop` names an item shop | The shop counter: **Buy** (the open stock rows) and **Sell** (pack items, at their sell price) |
| The pause menu's Journal, **Items** page, Cross | The pack bag: **Items** (consumables: use on a member) and **Gear** (wear or take off) |
| `play-window --mode shop[:ID]`, `items`, `items-gear` | The same screens, for testing |

L1 / R1 (or Left / Right) switch tabs, Up / Down move, Cross chooses, Circle backs out. Buying and selling ask for a quantity (Left / Right by one, Up / Down by ten). The Gear tab's member step shows, for the highlighted member, what the item's slot holds now and the five equipment-fed stats before and after (green up, red down); Cross on a member already wearing the item takes it off.

## Saves

The pack bag and worn gear are part of the pack's progress, so they save, park and adopt with it ([expansion-pack-quests.md](expansion-pack-quests.md#saves)). Tail record `4` of the expansion block:

```text
4      tag
u16    bag entries, then str key + u8 count each (key order)
u16    worn entries, then u8 roster slot + u8 equipment byte + str key each
```

It is written only while the player holds or wears a pack item, so a block without it is the layout an older writer made, and a save made with no pack carries no block at all. A session played without the pack keeps the block dormant: the worn pack item's retail byte stays empty and adds nothing, and the item is back when the pack is.

## Validation

| Check | Severity |
|---|---|
| Item: empty name, name over 20 characters, stat outside `-255..=255`, passive past `0x3F`, element past `6`, unknown set, a consumable with no effect or usable nowhere, a heal with no amount, revive outside `1..=100` | error |
| An `icon` or `as_item` that is not a retail item; price over 9,999,999 | error |
| Equipment with no `icon`; effect fields on equipment; stat fields on a consumable; non-ASCII text | warning |
| Set: a tier under 2 pieces | error |
| Set: fewer than two members, a tier needing more pieces than the set has | warning |
| Shop: a stock row naming an unknown pack item or item id `0`; a bad `requires` | error |
| Shop: no stock, a pack row with no price | warning |
| A map `shop` trigger naming no shop; an NPC `shop` naming neither an item shop nor a card dealer | error |

## Limits

- At most five pack items take part in one fight ([battle aliases](#battle-aliases)).
- Pack gear is worn by the three heroes only.
- The retail Items and Equip screens list no pack items (their rows are the retail bag's slots); the pack item screen and the Journal list them. A slot a pack item fills shows that item's name on the retail Equip and Status screens (`World::pack_worn_item_name`), though the record's byte under it stays `0`.
- A pack weapon's `icon` picks its battle mesh and swing costs from the retail weapon it names.

## Rebirth's gear

[`packs/rebirth/items.toml`](../../packs/rebirth/items.toml) is set against retail's endgame, the Ra-Seru set from the static equipment table (`DAT_80074F68`, [equipment-table.md](equipment-table.md)):

| Slot | Retail best | Rebirth |
|---|---|---|
| Vahn's weapon | Ra-Seru Blade ATK 98; shop Chaos Breaker 72 | Rebirth Blade ATK 112 (Iron Regent) |
| Noa's weapon | Ra-Seru Fangs 96; Golden Claw 86 | Rebirth Talon 110 (Azure Seraph) |
| Gala's weapon | Ra-Seru Club 100; Great Axe 88 | Rebirth Maul 114 (Verdant Colossus) |
| Body | Ra-Seru Armor 83/82, Robe 84/78, Plate 75/84; Triumph Armor 70/69 for 50,000 G | Warden tier 69-77 (sold from the start); Rebirth Mail 94/94, Robe 95/88, Plate 86/96 |
| Head | Ra-Seru Seal / Plume / Helmet UDF 62-64, INT 60-64 | Rebirth Crown UDF 71, INT 70 |
| Feet | Ra-Seru Boots / Shoes / Thongs ATK 86-90, LDF 60-64, SPD 28-32 | Rebirth Greaves ATK 98, LDF 70, SPD 35 |

Each Rebirth piece is about 12-15% above its Ra-Seru twin, and the set adds +10 ATK at two pieces, +12 defence at three and All Guard (`0x24`) at four. Its accessories pair two retail passives at the pair's price: Ysolde's Seal (Master Guard + HP Boost 1), Tower Sigil (AP Boost 2 + HP After), Mist Lantern (EXP Boost + Items Up), Sovereign's Eye (Attack x2 + Arts Power), Ember Heart (Fire Guard, +15 ATK). The consumables extend the retail line: Tower Draught (2500 HP), Mist Tonic (300 MP), Phoenix Plume (party revive at 50%), Sovereign Nectar (full HP, party). The 5F Tower Armory sells the consumables and the Warden tier at once and opens the rest boss by boss.

## See also

- [Expansion packs](expansion-pack.md) - the format, ids and validation.
- [Battles](expansion-pack-battle.md) - drops, steals and boss rewards.
- [Quests](expansion-pack-quests.md) - rewards, collect objectives and the save block.
- [Maps](expansion-pack-maps.md) - the `shop` trigger and shop NPCs.
- [Inventory](../subsystems/inventory.md) - the retail bag pack items live beside.
- [Item table](item-table.md), [equipment table](equipment-table.md), [accessory passives](accessory-passive-table.md) - the retail tables pack items borrow from.
