//! `EquipSession`: its state / event types, construction, accessors and the per-slot candidate rows.

use super::*;

/// Bag + audio bindings the ported per-slot applier
/// ([`legaia_engine_dev_menu::dev_equip_commit::commit_equip`]) needs, backed by
/// an [`EquipSession`]'s own inventory map.
///
/// Retail's bag index space is a slot index into the 256-entry bag; the
/// engine's bag is keyed by item id, so this adapter uses the id itself as
/// the index. That is injective over the id space and can never collide
/// with the miss sentinel, which is `0x100` and therefore out of `u8` range.
pub(super) struct SessionEquipHost<'a> {
    pub(super) inventory: &'a mut crate::item_bag::ItemBag,
    /// The cue the applier played, if it reached its tail.
    pub(super) sfx: Option<u8>,
}

impl legaia_engine_dev_menu::dev_equip_commit::EquipCommitHost for SessionEquipHost<'_> {
    fn find_in_bag(&self, item_id: u8) -> u16 {
        match self.inventory.get(&item_id) {
            Some(qty) if *qty > 0 => item_id as u16,
            _ => legaia_engine_dev_menu::dev_equip_commit::BAG_MISS,
        }
    }

    fn take_from_bag(&mut self, bag_index: u16, qty: u8) {
        if let Some(have) = self.inventory.get_mut(&(bag_index as u8)) {
            *have = have.saturating_sub(qty);
        }
    }

    fn give_to_bag(&mut self, item_id: u8, qty: u8) {
        let _ = self.inventory.add(item_id, qty);
    }

    fn play_sfx(&mut self, cue: u8) {
        self.sfx = Some(cue);
    }
}

/// One equippable item the player can choose from in the browse phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EquipItem {
    pub id: u8,
    /// Slot index this item is meant for (`0..=7`). Items targeting other
    /// slots get filtered out of the browse list.
    pub slot: u8,
    /// `true` when the player owns at least one of these.
    pub owned: bool,
    /// `true` on the Goods list's **equipped** row: the class-`0x7000` entry
    /// the builder puts second, naming the item already in the slot.
    /// Confirming it commits nothing - `FUN_801D9C14`'s confirm arm tests
    /// only the `0x4000` (Remove) and `0x6000` / `0x9000` (bag) classes, so a
    /// `0x7000` row falls through to the shared tail (`0x801DA1DC`), which
    /// advances the hand to the next slot row.
    pub equipped: bool,
}

/// [`crate::menu_list_rows::ItemRowTables`] over the session's disc-pinned
/// Goods index, so the Goods candidate builder runs on the same acceptance
/// set [`DiscEquipInfo::install_goods`] computed from the item-effect table
/// (class `2`, `+3` byte not `0x41`). Only the two answers the Goods builder
/// asks are meaningful.
struct GoodsIndexTables<'a>(&'a DiscEquipInfo);

impl crate::menu_list_rows::ItemRowTables for GoodsIndexTables<'_> {
    fn kind(&self, id: u8) -> u8 {
        if self.0.is_goods_candidate(id) { 2 } else { 0 }
    }
    fn subtype(&self, id: u8) -> u8 {
        id
    }
    fn price(&self, _id: u8) -> u16 {
        0
    }
    fn effect_flags(&self, _subtype: u8) -> u8 {
        0
    }
    fn effect_marker(&self, subtype: u8) -> u8 {
        if self.0.is_goods_candidate(subtype) {
            0
        } else {
            crate::menu_list_rows::GOODS_NO_PASSIVE_MARKER
        }
    }
    fn equip_flags(&self, _subtype: u8) -> u8 {
        0
    }
}

/// Phase of the equip session SM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipState {
    /// Cursor on the equipment grid. Cross opens the item picker;
    /// Triangle cycles characters; Circle exits.
    ///
    /// `cursor` is a **row** in the retail slot-browse row space, not a
    /// slot index: row `0` is the "Best Equipment" auto-equip and row
    /// `n` is equip slot `n - 1` ([`SLOT_BROWSE_ROWS`]). Retail's row
    /// word is `DAT_801E46C0 & 0xFFF` and `FUN_801D99F0` branches on it
    /// being zero (`beq v0,zero` at `0x801D9B4C`), which is why the
    /// engine's slot rows start at `1` as well.
    SlotPicker { cursor: u8 },
    /// Cursor on the item list filtered to the active slot. Cross
    /// confirms; Circle goes back to slot picker.
    ///
    /// Row `0` of that list is the **Remove** row whenever the slot is
    /// occupied ([`EquipSession::items_for_slot`]).
    ItemPicker { slot: u8, cursor: u16 },
    /// "Equip `<X>`?" Yes/No prompt. Cross confirms; Circle abandons.
    Confirm { slot: u8, item_id: u8, cursor: u8 },
    /// Session done - committed or aborted.
    Done(EquipOutcome),
}

/// Result of a finished equip session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipOutcome {
    /// Player committed a swap. `removed` is the previous item id (0 for
    /// "nothing"); `added` is the newly-equipped id.
    Committed { slot: u8, removed: u8, added: u8 },
    /// Session was cancelled without committing.
    Cancelled,
}

/// Per-frame input bundle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EquipInput {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub cross: bool,
    pub circle: bool,
    pub triangle: bool,
}

/// Events the session emits per `input()` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EquipEvent {
    /// Cursor moved in either picker phase.
    CursorMoved { state: EquipStateKind, cursor: u16 },
    /// Player opened the item picker for a slot.
    EnteredItemPicker { slot: u8 },
    /// Player advanced to the confirm prompt.
    EnteredConfirm { slot: u8, item_id: u8 },
    /// Player committed a swap.
    Committed { slot: u8, removed: u8, added: u8 },
    /// Player cancelled - session terminating.
    Cancelled,
    /// Player tried to equip an item they don't own / item filter rejected.
    InvalidConfirm,
    /// Best Equipment (slot-browse row 0) swapped `changed` armament slots.
    /// Retail cues SFX `0x24` here (`0x801D9B78`).
    BestEquipmentApplied { changed: u32 },
    /// Best Equipment found nothing to change - retail's buzz `0x23`
    /// (`0x801D9B98`).
    BestEquipmentUnchanged,
    /// The per-slot applier played its confirm cue
    /// (`FUN_80035BD0(0x24)` at the tail of `FUN_801E5A08`). Hosts turn
    /// this into a one-shot SFX; it is emitted only on a commit that
    /// actually wrote the record.
    ConfirmSfx { cue: u8 },
}

/// Phase tag for `EquipEvent::CursorMoved`. Avoids needing a clone of
/// `EquipState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipStateKind {
    SlotPicker,
    ItemPicker,
    Confirm,
}

/// Equip session.
#[derive(Debug, Clone)]
pub struct EquipSession {
    /// Editable copy of the character's stat record. Engines pass in a
    /// snapshot at session start; on commit the session writes back.
    pub(super) record: StatRecord,
    /// Catalog of items the player owns (one entry per id). Used to
    /// build the per-slot picker list.
    pub(super) inventory: crate::item_bag::ItemBag,
    /// Equipment-stat table (resolves modifiers on commit).
    pub(super) equipment: EquipmentTable,
    /// Status modifiers pulled from the world for the recompute pass.
    pub(super) modifiers: StatusModifiers,
    /// Currently-applied status set on the character.
    pub(super) active_status: Vec<StatusKind>,
    /// Current SM state.
    pub(super) state: EquipState,
    /// Last computed `BattleStats` snapshot for HUD display ("equipping
    /// this item changes your ATK by +5"). Refreshed when the cursor
    /// moves over a candidate item.
    pub preview_stats: BattleStats,
    /// Events drained per `input()`.
    pub(super) events: Vec<EquipEvent>,
    /// Optional disc-pinned equip restrictions (`DAT_80074F68`). When set,
    /// the per-slot item list is gated on the active character's equip mask
    /// (`+6`) and, for the four unambiguous UI slots, the disc slot category
    /// (`+7`). When `None`, the legacy `id >> 5` placeholder rule is used.
    pub(super) restrictions: Option<DiscEquipInfo>,
    /// Active party slot (`0` Vahn, `1` Noa, `2` Gala) the session is editing.
    /// Drives the equip-mask gate when `restrictions` is set.
    pub(super) active_party_slot: u8,
    /// Optional weapon-category favour table (`FUN_801DD0C0`'s data, parsed
    /// by [`crate::menu_item_category::parse_category_table`]). Empty means
    /// the Best Equipment weapon rank falls back to raw ATK.
    pub(super) weapon_category: Vec<crate::menu_item_category::CategoryEntry>,
    /// The character picker owns the pad ([`Self::set_slot_cursor_hidden`]).
    slot_cursor_hidden: bool,
    /// Swaps the Best Equipment row made on this session's own record and
    /// bag, as `(engine slot, removed, added)` in the order they landed.
    /// Retail's applier (`FUN_801CF760`) writes the live record and the bag
    /// directly; the session edits copies, so its host replays these when
    /// the session closes ([`Self::best_equipment_swaps`]).
    pub(super) best_swaps: Vec<(u8, u8, u8)>,
}

impl EquipSession {
    /// Construct a session for an actor.
    pub fn new(
        record: StatRecord,
        inventory: crate::item_bag::ItemBag,
        equipment: EquipmentTable,
        modifiers: StatusModifiers,
        active_status: Vec<StatusKind>,
    ) -> Self {
        let preview_stats = compute_battle_stats(&record, &equipment, &active_status, &modifiers);
        Self {
            record,
            inventory,
            equipment,
            modifiers,
            active_status,
            state: EquipState::SlotPicker { cursor: 0 },
            preview_stats,
            events: Vec::new(),
            restrictions: None,
            active_party_slot: 0,
            weapon_category: Vec::new(),
            slot_cursor_hidden: false,
            best_swaps: Vec::new(),
        }
    }

    /// Every swap the Best Equipment row committed during this session, as
    /// `(engine slot, removed, added)`. Unlike a single-slot confirm these
    /// do not end the session, so they are not in [`Self::outcome`]: the
    /// host applies them to the live record and bag however the session
    /// ended.
    pub fn best_equipment_swaps(&self) -> &[(u8, u8, u8)] {
        &self.best_swaps
    }

    /// Hide the slot-browse hand while the Equip screen's character picker
    /// (`0x12`) owns the pad: the slot rows still show the hovered
    /// character's equipment, but no row is under the cursor.
    pub fn set_slot_cursor_hidden(&mut self, hidden: bool) {
        self.slot_cursor_hidden = hidden;
    }

    /// Whether the slot-browse hand is hidden ([`Self::set_slot_cursor_hidden`]).
    pub fn slot_cursor_hidden(&self) -> bool {
        self.slot_cursor_hidden
    }

    /// End the session as cancelled - the character picker's cancel, which
    /// leaves the Equip screen for the root list.
    pub fn cancel(&mut self) {
        self.state = EquipState::Done(EquipOutcome::Cancelled);
    }

    /// Re-open the slot browse on its Best Equipment row - the state the
    /// character picker hands the pad to on a confirm, and the one a
    /// cancelled browse is rewound to when it goes back to the picker.
    pub fn reopen_slot_browse(&mut self) {
        self.state = EquipState::SlotPicker { cursor: 0 };
    }

    /// Install the weapon-category favour table the Best Equipment weapon
    /// rank consults (`FUN_801DD0C0`, ported as
    /// [`crate::menu_item_category::category_check`]).
    ///
    /// Without it the weapon slot ranks on the raw ATK byte alone, which is
    /// *not* retail: a category-favoured weapon scores a flat
    /// [`crate::menu_item_category::CATEGORY_MATCH_SCORE`] on top and no ATK
    /// byte can reach it.
    pub fn with_weapon_category(
        mut self,
        table: Vec<crate::menu_item_category::CategoryEntry>,
    ) -> Self {
        self.weapon_category = table;
        self
    }

    /// Set which party slot (`0` Vahn, `1` Noa, `2` Gala) this session is
    /// editing, without also supplying the disc restriction table
    /// ([`Self::new_with_restrictions`] sets both at once).
    ///
    /// The slot is not cosmetic: it is the `char_index` argument of the
    /// weapon-category check and the bit the Best-Equipment candidate mask
    /// is tested against, so a session left on the default `0` ranks every
    /// character's weapons as if they were Vahn's. That is harmless only
    /// while the category table is empty - which is exactly the state the
    /// table's arrival ends.
    pub fn with_active_party_slot(mut self, slot: u8) -> Self {
        self.active_party_slot = slot;
        self
    }

    /// Install the disc-pinned restriction table + active party slot on an
    /// already-built session (the builder form of
    /// [`Self::new_with_restrictions`], for callers that compose the session
    /// through the other `with_*` hooks first).
    pub fn with_restrictions(mut self, restrictions: DiscEquipInfo, active_party_slot: u8) -> Self {
        self.restrictions = Some(restrictions);
        self.active_party_slot = active_party_slot;
        self
    }

    /// Construct a session that gates the item list on the disc-pinned equip
    /// restrictions for `active_party_slot` (`0` Vahn, `1` Noa, `2` Gala).
    /// Each candidate item must be equippable by that character (the `+6`
    /// mask) and, for the weapon / body / helmet / boots slots, match the
    /// item's disc slot category (`+7`).
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_restrictions(
        record: StatRecord,
        inventory: crate::item_bag::ItemBag,
        equipment: EquipmentTable,
        modifiers: StatusModifiers,
        active_status: Vec<StatusKind>,
        restrictions: DiscEquipInfo,
        active_party_slot: u8,
    ) -> Self {
        let mut s = Self::new(record, inventory, equipment, modifiers, active_status);
        s.restrictions = Some(restrictions);
        s.active_party_slot = active_party_slot;
        s
    }

    /// Current state.
    pub fn state(&self) -> EquipState {
        self.state
    }

    /// Live record (post-edits, pre-commit).
    pub fn record(&self) -> &StatRecord {
        &self.record
    }

    /// Read access to the in-progress inventory map. Engines render the
    /// per-candidate count from this.
    pub fn inventory(&self) -> &crate::item_bag::ItemBag {
        &self.inventory
    }

    /// Read access to the equipment-stat table backing this session.
    /// Engines render per-candidate stat-delta previews by comparing
    /// `equipment.get(id)` for the candidate against the slot's current
    /// occupant.
    pub fn equipment(&self) -> &EquipmentTable {
        &self.equipment
    }

    /// `true` if the session is in `Done` state.
    pub fn is_done(&self) -> bool {
        matches!(self.state, EquipState::Done(_))
    }

    /// Outcome - only valid when `is_done()`.
    pub fn outcome(&self) -> Option<EquipOutcome> {
        match self.state {
            EquipState::Done(out) => Some(out),
            _ => None,
        }
    }

    /// Drain events the SM emitted since the last call.
    pub fn drain_events(&mut self) -> Vec<EquipEvent> {
        std::mem::take(&mut self.events)
    }

    /// Filter the inventory to items targeting `slot` and owned ≥ 1.
    /// The disc-free placeholder sorts by item id so cursor positions are stable
    /// across HashMap iteration orderings.
    ///
    /// When the session carries disc-pinned [`DiscEquipInfo`] restrictions,
    /// the list is gated on the active character's equip mask (`+6`) and, for
    /// the four unambiguous UI slots (weapon / body / helmet / boots), the
    /// item's disc slot category (`+7`). Otherwise it falls back to the legacy
    /// `id >> 5` placeholder rule.
    ///
    /// With the disc restrictions installed (both play hosts) every slot
    /// takes retail's row order: [`Self::armament_rows`] for the armament
    /// rows, [`Self::goods_rows`] for the Goods rows - Remove first, the worn
    /// id next, then bag order. The disc-free placeholder below is id-sorted
    /// and puts **Remove** ([`REMOVE_ROW_ID`], the class-`0x4000` payload-`0`
    /// entry `FUN_801D9C14` tests for at `0x801DA0B4`) only on an occupied
    /// slot; [`Self::unequip`] keeps retail's empty-slot buzz guard.
    pub fn items_for_slot(&self, slot: u8) -> Vec<EquipItem> {
        if let Some(info) = &self.restrictions
            && matches!(
                EquipSlot::from_index(slot),
                Some(EquipSlot::Ring1 | EquipSlot::Ring2 | EquipSlot::Accessory)
            )
        {
            return self.goods_rows(slot, info);
        }
        if self.restrictions.is_some() {
            return self.armament_rows(slot);
        }
        let mut out: Vec<EquipItem> = self
            .inventory
            .iter()
            .filter_map(|(id, qty)| {
                if *qty == 0 {
                    return None;
                }
                if !self.item_fits_slot(*id, slot) {
                    return None;
                }
                Some(EquipItem {
                    id: *id,
                    slot,
                    owned: *qty > 0,
                    equipped: false,
                })
            })
            .collect();
        out.sort_by_key(|i| i.id);
        if self.slot_is_occupied(slot) {
            out.insert(
                0,
                EquipItem {
                    id: REMOVE_ROW_ID,
                    slot,
                    owned: true,
                    equipped: false,
                },
            );
        }
        out
    }

    /// An **armament** slot's candidate list, in retail's row order.
    ///
    /// `FUN_80030628`'s armament cases (content ids `0x15`..`0x18`, also
    /// `7`..`10`) build the same leading rows as the Goods family: the Remove
    /// verb (`0x4000`, payload `0`) always (`sh v0,0x6020(v1)` at
    /// `0x800314D0`), the equipped id (`0x7000 | id`) when the slot holds one
    /// (`0x800314DC`), then every bag slot of the active window in **slot**
    /// order (`0x80031544..0x800315E4`) whose item is equipment (record `+0`
    /// class `1`), whose `+7` category matches the row and whose `+6` mask
    /// takes the character - skipping the equipped id itself (`0x80031554..
    /// 0x8003156C`). Confirming Remove on an empty slot is retail's buzz,
    /// which [`Self::unequip`] keeps.
    ///
    /// REF: FUN_80030628 (armament cases `7`..`10` / `0x15`..`0x18`)
    fn armament_rows(&self, slot: u8) -> Vec<EquipItem> {
        let (start, end) = self.inventory.window_bounds();
        let slots = self.inventory.slots();
        let window = &slots[start.min(slots.len())..end.min(slots.len())];
        let equipped_id = self.record.equip.get(slot as usize).copied().unwrap_or(0);
        let mut out = vec![EquipItem {
            id: REMOVE_ROW_ID,
            slot,
            owned: true,
            equipped: false,
        }];
        if equipped_id != 0 {
            out.push(EquipItem {
                id: equipped_id,
                slot,
                owned: true,
                equipped: true,
            });
        }
        out.extend(
            window
                .iter()
                .filter(|&&(id, qty)| {
                    id != 0 && qty > 0 && id != equipped_id && self.item_fits_slot(id, slot)
                })
                .map(|&(id, qty)| EquipItem {
                    id,
                    slot,
                    owned: qty > 0,
                    equipped: false,
                }),
        );
        out
    }

    /// The three **Goods** slots' candidate list, in retail's row order.
    ///
    /// Runs [`crate::menu_list_rows::build_goods_candidate_rows`] over the
    /// bag's active window: the Remove verb always leads (even on an empty
    /// slot, where confirming it buzzes), the equipped id follows when the
    /// slot holds one, then every accepted bag slot in **slot** order, the
    /// equipped id skipped. The armament slots' twin is [`Self::armament_rows`].
    ///
    /// REF: FUN_80030628 (Goods cases `0xE`..`0x10` / `0x1C`..`0x1E`)
    fn goods_rows(&self, slot: u8, info: &DiscEquipInfo) -> Vec<EquipItem> {
        let (start, end) = self.inventory.window_bounds();
        let slots = self.inventory.slots();
        let window = &slots[start.min(slots.len())..end.min(slots.len())];
        let ids: Vec<u8> = window.iter().map(|&(id, _)| id).collect();
        let equipped_id = self.record.equip.get(slot as usize).copied().unwrap_or(0);
        let words = crate::menu_list_rows::build_goods_candidate_rows(
            &ids,
            start as u16,
            &GoodsIndexTables(info),
            equipped_id,
        );
        let class_mask = crate::menu_list_rows::ROW_CLASS_MASK;
        words
            .iter()
            .filter_map(|&w| {
                let payload = w & !class_mask;
                match w & class_mask {
                    crate::menu_list_rows::CLASS_VERB => Some(EquipItem {
                        id: REMOVE_ROW_ID,
                        slot,
                        owned: true,
                        equipped: false,
                    }),
                    crate::menu_list_rows::CLASS_ITEM_ICON => Some(EquipItem {
                        id: payload as u8,
                        slot,
                        owned: true,
                        equipped: true,
                    }),
                    _ => {
                        let (id, qty) = *slots.get(payload as usize)?;
                        Some(EquipItem {
                            id,
                            slot,
                            owned: qty > 0,
                            equipped: false,
                        })
                    }
                }
            })
            .collect()
    }

    /// `true` when equip slot `slot` currently holds an item - the condition
    /// the Remove row and its draw-side label are both gated on.
    pub fn slot_is_occupied(&self, slot: u8) -> bool {
        self.record
            .equip
            .get(slot as usize)
            .is_some_and(|&id| id != 0)
    }

    /// Whether `id` is a valid candidate for UI `slot`. Uses the disc-pinned
    /// restrictions when installed, else the legacy `id >> 5` rule.
    fn item_fits_slot(&self, id: u8, slot: u8) -> bool {
        match &self.restrictions {
            Some(info) => {
                // The three Goods rows are a different list family, not a
                // looser version of the armament one: retail reaches them
                // through content ids `0x1C`..`0x1E`, whose filter takes
                // item class 2 and the item-effect `+3` byte and applies
                // **no** character mask
                // (`crate::menu_list_rows::build_goods_candidate_rows`).
                // Asking the armament question here is what left every
                // Goods row with an empty list.
                if matches!(
                    EquipSlot::from_index(slot),
                    Some(EquipSlot::Ring1 | EquipSlot::Ring2 | EquipSlot::Accessory)
                ) {
                    return info.is_goods_candidate(id);
                }
                // Must be equippable by the active character (the `+6` mask).
                if !info.can_equip(id, self.active_party_slot) {
                    return false;
                }
                // For the four UI slots the disc `+7` byte resolves cleanly,
                // require a category match; the ambiguous head/hand slots are
                // mask-gated only (the disc cannot separate them).
                match EquipSlot::from_index(slot).and_then(engine_slot_disc_category) {
                    Some(wanted) => info.category(id) == Some(wanted),
                    None => true,
                }
            }
            // Legacy placeholder: the item id encodes its target slot in the
            // upper 3 bits (slot = id >> 5).
            None => ((id >> 5) & 0x07) == slot,
        }
    }
}
