//! Read-only projections for the menu's screens: spell and equipment views, sell rows,
//! inventory items and the current screen label.

use super::*;

impl MenuRuntime {
    /// Spell list for the currently selected character, or `None` if
    /// `selected_char` is out of bounds.  Engines call this to populate
    /// the `StatusMagic` screen rows.
    pub fn spell_view(&self, world: &World) -> Option<SpellList> {
        world
            .party
            .roster
            .members
            .get(self.selected_char)
            .map(|r| r.spell_list())
    }

    /// Equipment slots for the currently selected character, or `None` if
    /// `selected_char` is out of bounds.  Engines call this to populate
    /// the `StatusEquipment` screen rows.
    pub fn equipment_view(&self, world: &World) -> Option<EquipmentSlots> {
        world
            .party
            .roster
            .members
            .get(self.selected_char)
            .map(|r| r.equipment())
    }

    /// The shop **sell** list's rows, in retail's order: the bag's active
    /// window walked by slot, sellable rows in place, unsellable (price `0`)
    /// rows dimmed and sorted last (`FUN_80030628` content id 2, ported as
    /// [`legaia_engine_menus::menu_list_rows::build_price_gated_rows`]).
    ///
    /// Both the list draw and the sell commit go through here, which is the
    /// point: the two used to build the rows separately - the draw
    /// id-sorted, the commit by slot - so on any bag whose slot order is not
    /// ascending by id the player sold a different stack than the one the hand
    /// was on. The row carries its bag slot, so the sale comes off the stack
    /// that was highlighted even when one id sits in two slots.
    ///
    /// Falls back to the plain occupied-slot walk when the disc tables are
    /// absent (no price table means nothing to gate on).
    pub fn sell_list_rows(world: &World) -> Vec<crate::bag_rows::BagRow> {
        if let Some(rows) = world.bag_sell_rows() {
            return rows;
        }
        let (start, end) = world.party.inventory.window_bounds();
        let slots = world.party.inventory.slots();
        slots[start.min(slots.len())..end.min(slots.len())]
            .iter()
            .enumerate()
            .filter(|(_, (id, count))| *id != 0 && *count > 0)
            .map(|(i, (id, count))| crate::bag_rows::BagRow {
                slot: (start + i) as u8,
                id: *id,
                count: *count,
                dim: false,
                alt_ink: false,
            })
            .collect()
    }

    /// Sorted `(item_id, count)` pairs from the world's global inventory,
    /// ascending by item ID, filtering out zero-count entries.  Engines
    /// call this to populate the `StatusInventory` screen rows.
    pub fn inventory_items(world: &World) -> Vec<(u8, u8)> {
        let mut items: Vec<(u8, u8)> = world
            .party
            .inventory
            .iter()
            .filter(|(_, c)| **c > 0)
            .map(|(id, c)| (*id, *c))
            .collect();
        items.sort_by_key(|&(id, _)| id);
        items
    }

    /// Engine-friendly label per active state - drives a HUD banner so the
    /// player sees *something* before the per-screen layouts ship.
    pub fn current_label(&self) -> &'static str {
        match MenuState::from_byte(self.ctx.state) {
            Some(MenuState::Closed) => "",
            Some(MenuState::Idle) => "MENU",
            Some(MenuState::StatusTop) => "STATUS",
            Some(MenuState::StatusCharacter) => "CHARACTER",
            Some(MenuState::StatusEquipment) => "EQUIP",
            Some(MenuState::StatusInventory) => "ITEMS",
            Some(MenuState::StatusMagic) => "MAGIC",
            Some(MenuState::StatusTacticalArts) => "ARTS",
            Some(MenuState::StatusConfig) => "CONFIG",
            Some(MenuState::StatusLog) => "LOG",
            Some(MenuState::SavePickSlot) => "SAVE - PICK SLOT",
            Some(MenuState::SaveConfirmOverwrite) => "SAVE - OVERWRITE?",
            Some(MenuState::SaveWriting) => "SAVING…",
            Some(MenuState::SaveDone) => "SAVED",
            Some(MenuState::LoadSlot) => "LOAD - PICK SLOT",
            Some(MenuState::LoadProgress) => "LOADING…",
            Some(MenuState::ShopMenu) => "SHOP",
            Some(MenuState::ShopBuy) => "SHOP - BUY",
            Some(MenuState::ShopSell) => "SHOP - SELL",
            Some(MenuState::ShopQuantity) => "SHOP - HOW MANY?",
            Some(MenuState::ShopConfirm) => "SHOP - CONFIRM",
            Some(MenuState::ShopTrade) => "SHOP - TRADE SERU",
            Some(MenuState::ShopTradeConfirm) => "SHOP - TRADE?",
            Some(MenuState::ShopExit) => "SHOP - DONE",
            Some(MenuState::InnConfirm) => "INN - REST?",
            Some(MenuState::InnSleep) => "INN - RESTING",
            Some(MenuState::ItemPickTarget) => "ITEM - TARGET",
            Some(MenuState::ItemApply) => "ITEM - APPLY",
            Some(MenuState::ItemDone) => "ITEM - DONE",
            Some(MenuState::Confirm) => "CONFIRM?",
            Some(MenuState::Closing) => "CLOSING",
            Some(MenuState::Deactivate) => "",
            None => "?",
        }
    }
}
