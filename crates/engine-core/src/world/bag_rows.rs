//! The bag's **retail row order**: `World`'s seat at the SCUS list-content
//! builder `FUN_80030628`.
//!
//! [`crate::menu_list_rows`] ports the builder's per-content-id cases; this
//! module is what hands them a bag and turns their class-tagged `u16` entry
//! words back into rows a menu screen can draw. The two halves were written
//! apart because the engine's bag used to be a `HashMap<u8, u8>` keyed by item
//! id, which has no slot coordinate for the builders' `slot | ink` payload to
//! carry. [`crate::world::ItemBag`] is retail's 256-slot array now, so the
//! payload has an index again.
//!
//! What a row order is worth: retail's Items screen does **not** list a
//! player's bag by item id. It walks the physical slots of the active window
//! and splits them into three buffers - in-place rows first, then the kind-1
//! (equipment) rows, then the effect-flag-`0x8` tail - and it dims rows it will
//! not let you confirm. Sorting by id instead produces a list whose rows are in
//! an order no retail screen ever shows.

use crate::menu_list_rows::{
    EFFECT_FLAG_BATTLE_USABLE, ITEM_DOOR_OF_LIGHT, ITEM_DOOR_OF_WIND, ItemRowTables, ROW_ALT_INK,
    ROW_DISABLED, ROW_NAME_PAYLOAD_MASK, UseListCtx, build_price_gated_rows, build_throw_out_rows,
    build_use_list_rows,
};
use crate::world::World;

/// One decoded list row: the bag slot the entry word carried, the id in it,
/// and the two ink bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BagRow {
    /// Physical bag slot (the entry word's payload).
    pub slot: u8,
    /// Item id in that slot.
    pub id: u8,
    /// Stack count in that slot.
    pub count: u8,
    /// [`ROW_DISABLED`] - the row draws grey and confirming it buzzes.
    pub dim: bool,
    /// [`ROW_ALT_INK`] - the tail group's second ink.
    pub alt_ink: bool,
}

/// [`ItemRowTables`] over the world's disc tables.
///
/// Three tables, three sources, all disc-parsed at boot. `kind` / `subtype` /
/// `effect_flags` come from the item-effect table; `price` is the item
/// record's `+2` halfword, which `ShopItemData` already carries for every id
/// (it is the buy-price table the merchant scan validates records against, not
/// an open shop's stock list); `equip_flags` is the equipment record's `+7`
/// byte, read off the raw stat-bonus table by **bonus row**, because that is
/// the index retail's own readers form (`0x80074F68 + subtype * 8`).
struct WorldRowTables<'a> {
    effects: &'a legaia_asset::item_effect::ItemEffectTable,
    prices: Option<&'a crate::shop_catalog::ShopItemData>,
    equip: Option<&'a legaia_asset::equip_stats::EquipStatTable>,
}

impl ItemRowTables for WorldRowTables<'_> {
    fn kind(&self, id: u8) -> u8 {
        self.effects.kind(id)
    }
    fn subtype(&self, id: u8) -> u8 {
        self.effects.subtype(id)
    }
    fn price(&self, id: u8) -> u16 {
        self.prices.map(|p| p.price(id)).unwrap_or(0)
    }
    fn effect_flags(&self, subtype: u8) -> u8 {
        self.effects
            .descriptor(subtype)
            .map(|d| d.flags)
            .unwrap_or(0)
    }
    fn effect_marker(&self, subtype: u8) -> u8 {
        self.effects
            .descriptor(subtype)
            .map(|d| d.marker)
            .unwrap_or(crate::menu_list_rows::GOODS_NO_PASSIVE_MARKER)
    }
    fn equip_flags(&self, subtype: u8) -> u8 {
        self.equip
            .and_then(|t| t.rows().get(subtype as usize))
            .map(|b| b.raw[7])
            .unwrap_or(0)
    }
}

impl World {
    /// The row tables every builder in [`crate::menu_list_rows`] reads.
    fn row_tables(&self) -> Option<WorldRowTables<'_>> {
        Some(WorldRowTables {
            effects: self.tables.item_effects.as_ref()?,
            prices: self.shops.item_shop_data.as_ref(),
            equip: self.tables.equip_stats.as_ref(),
        })
    }
}

/// Decode one builder's entry words back into rows against the bag.
fn decode(world: &World, words: &[u16]) -> Vec<BagRow> {
    words
        .iter()
        .map(|&w| {
            let slot = (w & ROW_NAME_PAYLOAD_MASK) as u8;
            let (id, count) = world.party.inventory.slot(slot);
            BagRow {
                slot,
                id,
                count,
                dim: w & ROW_DISABLED != 0,
                alt_ink: w & ROW_ALT_INK != 0,
            }
        })
        .collect()
}

impl World {
    /// The item-id byte of every slot in the bag's **active window**, plus the
    /// window's first slot index - the `(bag_ids, slot_base)` pair every
    /// [`crate::menu_list_rows`] builder walks.
    ///
    /// Retail bounds the walk with `gp+0x2D2..gp+0x2D4`, which is exactly what
    /// [`crate::world::ItemBag::window_bounds`] answers.
    fn bag_window_ids(&self) -> (Vec<u8>, u16) {
        let (start, end) = self.party.inventory.window_bounds();
        let slots = self.party.inventory.slots();
        let ids = slots[start.min(slots.len())..end.min(slots.len())]
            .iter()
            .map(|&(id, _)| id)
            .collect();
        (ids, start as u16)
    }

    /// Whether using `id` on *anybody* would do something - the applicability
    /// probe the Use-list builder dims a row on.
    ///
    /// Retail is the party scan `FUN_8003043C`, which runs the action validator
    /// `FUN_8003FB10` over each member and dims when no member accepts (every
    /// ally at full HP for a heal, nobody poisoned for an antidote). The port
    /// scans the same set through the engine's own effect resolver, so the
    /// answer tracks whatever the catalog says the item does.
    ///
    /// An item with no catalog entry is **not** dimmed: retail's validator
    /// answers for a descriptor, and a catalog gap is the port's, not the
    /// player's.
    fn item_applies_to_anyone(&self, id: u8) -> bool {
        let Some(entry) = self.tables.item_catalog.get(id) else {
            return true;
        };
        // The marker effects resolve against live state at use time, so the
        // table-less probe cannot answer for them; retail's validator does let
        // them through.
        if matches!(
            entry.effect,
            crate::items::ItemEffect::StatUp
                | crate::items::ItemEffect::BattleBuff
                | crate::items::ItemEffect::ActionGauge
                | crate::items::ItemEffect::ArtsBook
                | crate::items::ItemEffect::PointCardStrike
        ) {
            return true;
        }
        let members = self.party.roster.members.len();
        (0..members).any(|i| {
            let hms = self.party.roster.members[i].hp_mp_sp();
            let status_mask = self
                .battle
                .status_effects
                .statuses(i as u8)
                .iter()
                .fold(0u8, |m, s| m | crate::items::status_bit(s.kind));
            let snap = crate::items::TargetSnapshot {
                hp: hms.hp_cur,
                hp_max: hms.hp_max,
                mp: hms.mp_cur,
                mp_max: hms.mp_max,
                is_dead: hms.hp_cur == 0 && hms.hp_max > 0,
                status_mask,
            };
            !matches!(
                crate::items::apply_effect(entry.effect, &snap),
                crate::items::ItemOutcome::NoEffect
            )
        })
    }

    /// The Items screen's **Use** list, in retail's row order.
    ///
    /// `battle` is the menu context word `gp+0x85C`: the field context dims an
    /// unusable row **in place**, the battle context sorts it to the tail
    /// instead - so the two contexts do not merely differ in ink.
    ///
    /// Returns `None` when no on-disc item-effect table is installed. The row
    /// order is a property of the descriptors (`kind`, the usability flags, the
    /// tail-group flag); with no table there is nothing to order by, and the
    /// caller keeps its id-sorted fallback rather than inventing a grouping.
    ///
    /// REF: FUN_80030628 (content id 3; the builder itself is
    /// `crate::menu_list_rows::build_use_list_rows`)
    pub fn bag_use_rows(&self, battle: bool) -> Option<Vec<BagRow>> {
        let tables = self.row_tables()?;
        let (ids, slot_base) = self.bag_window_ids();
        let applicable = |id: u8| self.item_applies_to_anyone(id);
        // The two scratchpad scene gates (`0x1F800394` bits `0x100000` /
        // `0x200000`) are the region reader's: every region hit raises both
        // and the long record layout drops them per `region[+8]` bits 7 / 6
        // ([`crate::region_encounter::region_battle_setup`]). Open until a
        // region has been stood in.
        let (door_light_blocked, door_wind_blocked) = self.region_door_gates();
        let ctx = UseListCtx {
            battle,
            door_light_blocked,
            door_wind_blocked,
            applicable: &applicable,
        };
        Some(decode(
            self,
            &build_use_list_rows(&ids, slot_base, &tables, &ctx),
        ))
    }

    /// The shop **sell** list, in retail's row order: sellable rows in place,
    /// zero-price rows dimmed and sorted last.
    ///
    /// The gate is the item record's `+2` halfword for **every** id, which is
    /// the table `ShopItemData` holds - not an open shop's stock list, whose
    /// answer for an id it does not sell is a floor of `1` that can never dim
    /// a row. `None` when either disc table is absent, and the caller then
    /// keeps its own ordering.
    ///
    /// REF: FUN_80030628 (content id 2; the builder itself is
    /// `crate::menu_list_rows::build_price_gated_rows`)
    pub fn bag_sell_rows(&self) -> Option<Vec<BagRow>> {
        self.shops.item_shop_data.as_ref()?;
        let tables = self.row_tables()?;
        let (ids, slot_base) = self.bag_window_ids();
        Some(decode(
            self,
            &build_price_gated_rows(&ids, slot_base, &tables),
        ))
    }

    /// The Items screen's **Throw Out** list, in retail's row order.
    ///
    /// The same three-buffer walk as the Use list with a discardability gate:
    /// a key item (effect flag `0x1`) dims in place, an equipment piece sorts
    /// to the tail and dims when its record's `+7` bit `0x01` is set, and the
    /// effect-flag-`0x8` group goes last in the alt ink.
    ///
    /// `None` when the item-effect table is absent. The equipment table may be
    /// absent on its own: no record then reports the no-discard bit and the
    /// equipment rows all stay confirmable, which is the disc-free default
    /// rather than a claim about the disc.
    ///
    /// REF: FUN_80030628 (content id `0x22`; the builder itself is
    /// `crate::menu_list_rows::build_throw_out_rows`)
    pub fn bag_throw_out_rows(&self) -> Option<Vec<BagRow>> {
        let tables = self.row_tables()?;
        let (ids, slot_base) = self.bag_window_ids();
        Some(decode(
            self,
            &build_throw_out_rows(&ids, slot_base, &tables),
        ))
    }

    /// Whether the descriptor for `id` carries the battle-usability flag - the
    /// gate the battle item list applies before the row order even runs.
    pub fn item_battle_usable(&self, id: u8) -> bool {
        self.tables
            .item_effects
            .as_ref()
            .and_then(|t| t.effect(id))
            .map(|e| e.flags & EFFECT_FLAG_BATTLE_USABLE != 0)
            .unwrap_or(false)
    }

    /// Discard the **whole stack** at a physical bag slot - the Throw Out
    /// confirm's write.
    ///
    /// Retail's confirm (`FUN_801D8734` phase 3) zeroes both bytes of
    /// `bag[cursor*2]`, where `cursor` is the selected row's payload: a bag
    /// **slot** index, not a position in the displayed list and not an item id.
    /// The distinction is only invisible on a bag with no holes. The pause item
    /// list hides empty slots, so with the bag holed the row ordinal and the
    /// slot diverge above the selection; and removing by id instead picks
    /// whichever slot the window scan reaches first, which is the wrong stack
    /// whenever one id sits in two slots.
    ///
    /// Retail does **not** compact the bag when the menu opens - the normalize
    /// helper `FUN_800423E0` has one call site in the dump corpus, a field-VM
    /// arm at `0x801E05D0` behind two equality tests on the dispatcher's
    /// context (`+0x454 == 2`, `+0x458 == 0x100`) - so holes really do reach
    /// the list.
    ///
    /// Returns the id that was discarded, or `0` for an already-empty slot.
    ///
    /// REF: FUN_801D8734 (Throw Out confirm, the `bag[cursor*2]` zeroing),
    /// FUN_80043048 (the by-slot consume this spends the whole stack through)
    pub fn discard_bag_slot(&mut self, slot: u8) -> u8 {
        let (id, count) = self.party.inventory.slot(slot);
        if id == 0 {
            return 0;
        }
        // The whole stack: the helper zeroes the id byte in place once the
        // count reaches 0, which is the pair-zeroing the confirm writes.
        self.party.inventory.consume_slot(slot, count.max(1));
        id
    }

    /// Take one unit off a physical bag slot, clearing it when the stack
    /// empties - what a Use spends.
    ///
    /// Same reason as [`Self::discard_bag_slot`] for addressing the slot: the
    /// row's payload is a slot, so the unit comes off the stack the player
    /// pointed at.
    ///
    /// Returns the id spent, or `0` when the slot was already empty.
    ///
    /// REF: FUN_80043048 (ported as `RetailInventory::consume_slot`)
    pub fn consume_bag_slot(&mut self, slot: u8) -> u8 {
        let (id, _) = self.party.inventory.slot(slot);
        if id == 0 {
            return 0;
        }
        self.party.inventory.consume_slot(slot, 1);
        id
    }

    /// `true` when `id` is one of the two scene-gated Door rows the Use-list
    /// builder tests by id.
    pub fn is_door_item(id: u8) -> bool {
        id == ITEM_DOOR_OF_LIGHT || id == ITEM_DOOR_OF_WIND
    }
}
