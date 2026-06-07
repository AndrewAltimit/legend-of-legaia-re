//! Inventory item-use session.
//!
//! State machine that drives the "open inventory → pick item → pick target
//! → use it" flow shared between the field menu and the battle command
//! menu. Engines own a single [`InventoryUseSession`] for the lifetime of
//! the inventory screen; per-frame they push input events and drain
//! [`InventoryUseEvent`]s to forward into render / sound / world side-
//! effects.
//!
//! ## State graph
//!
//! ```text
//! Browsing(item_cursor)
//!    ↓ Confirm        ↑ Cancel
//! TargetSelect(target_cursor)
//!    ↓ Confirm        ↑ Cancel
//! Done(ItemOutcome)    ← terminal; engines re-construct the session for
//!                       the next inventory open
//! ```
//!
//! `Cancel` from `Browsing` returns `Aborted` (engine closes the menu).
//! `Cancel` from `TargetSelect` falls back to `Browsing` with the same
//! `item_cursor`. `Confirm` on a row whose item isn't usable in the current
//! context (battle / field) bounces with no state change.
//!
//! ## Inputs
//!
//! Engines forward generic [`InventoryUseInput`] events. The mapping from
//! actual key bindings (Z = Confirm, X = Cancel, arrows for navigation)
//! lives in `crate::input` and is engine-side.

use crate::items::{ItemCatalog, ItemEffect, ItemEntry, ItemOutcome, TargetSnapshot};

/// Where the session is being driven from. Filters which items show up
/// (`usable_in_battle` vs `usable_in_field`) and which targets are
/// pickable (party only in field, party + monsters in battle).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryContext {
    Battle,
    Field,
}

impl InventoryContext {
    /// `true` if `entry` is usable in this context.
    pub fn allows(&self, entry: &ItemEntry) -> bool {
        match self {
            InventoryContext::Battle => entry.usable_in_battle,
            InventoryContext::Field => entry.usable_in_field,
        }
    }
}

/// Per-target view the session shows in the target-select column.
#[derive(Debug, Clone)]
pub struct TargetRow {
    /// Slot index (0..=2 party, 3..=7 monster).
    pub slot: u8,
    /// Display name. Engines populate from save record / monster data.
    pub name: String,
    /// `true` if the target is `is_dead == false`.
    pub alive: bool,
    /// `true` for an enemy (monster) row. Offensive items (Damage / Capture)
    /// require this; heals / cures / revives require it to be `false`.
    pub is_enemy: bool,
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
}

impl TargetRow {
    pub fn new(slot: u8, name: impl Into<String>) -> Self {
        Self {
            slot,
            name: name.into(),
            alive: true,
            is_enemy: false,
            hp: 0,
            hp_max: 0,
            mp: 0,
            mp_max: 0,
        }
    }

    pub fn with_stats(mut self, hp: u16, hp_max: u16, mp: u16, mp_max: u16) -> Self {
        self.hp = hp;
        self.hp_max = hp_max;
        self.mp = mp;
        self.mp_max = mp_max;
        self
    }

    /// Mark this row as an enemy (monster) target. Offensive items route here.
    pub fn with_enemy(mut self, is_enemy: bool) -> Self {
        self.is_enemy = is_enemy;
        self
    }

    pub fn is_dead(&self) -> bool {
        !self.alive
    }

    /// Snapshot the target's stats for the item resolver.
    pub fn snapshot(&self) -> TargetSnapshot {
        TargetSnapshot {
            hp: self.hp,
            hp_max: self.hp_max,
            mp: self.mp,
            mp_max: self.mp_max,
            is_dead: !self.alive,
        }
    }
}

/// Inputs the session consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryUseInput {
    Up,
    Down,
    Confirm,
    Cancel,
}

/// State the session is currently in. Engines render based on this plus
/// the `cursor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryUseState {
    /// Browsing the item list. `cursor` indexes into [`InventoryUseSession::filtered_items`].
    Browsing { cursor: usize },
    /// Picking a target. `cursor` indexes into [`InventoryUseSession::targets`].
    /// `item_cursor` is preserved so Cancel returns to the same item row.
    TargetSelect { item_cursor: usize, cursor: usize },
    /// Terminal - the engine should drain the outcome and dispose of the
    /// session.
    Done(ItemOutcome),
    /// Terminal - the player cancelled out of `Browsing`.
    Aborted,
}

/// Per-frame events the session emits. Engines react: play a UI blip,
/// log to the battle HUD, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryUseEvent {
    /// Cursor moved (item list or target list - the state field tells
    /// you which).
    CursorMoved,
    /// Confirm pressed but the action wasn't valid (no items / dead
    /// target / etc.). Engines play a buzz blip.
    InvalidConfirm,
    /// Successful confirm on an item row - entered target-select.
    EnteredTargetSelect,
    /// Successful confirm on a target row - completed with outcome.
    Used { slot: u8, outcome: ItemOutcome },
    /// Cancelled out (either back to browse, or out of session).
    Cancelled,
}

/// One inventory use session.
#[derive(Debug, Clone)]
pub struct InventoryUseSession {
    /// Items the player currently holds (count > 0 entries from the
    /// player's inventory). Order is the canonical "first picked up
    /// first shown" - engines pre-sort if they want a different order.
    pub items: Vec<u8>,
    /// Available targets (party in field; party + monsters in battle).
    pub targets: Vec<TargetRow>,
    /// Per-item filtered indices into `items` (only usable-in-context
    /// items show up).
    pub filtered_items: Vec<usize>,
    pub state: InventoryUseState,
    pub events: Vec<InventoryUseEvent>,
    pub context: InventoryContext,
    /// The catalog the session resolves item ids against.
    pub catalog: ItemCatalog,
}

impl InventoryUseSession {
    /// Construct a fresh session. The session enters [`InventoryUseState::Browsing`]
    /// at cursor 0 if there's at least one filtered item; otherwise it
    /// starts in `Aborted` state (engines should still render the empty
    /// menu, but Confirm will no-op).
    pub fn new(
        catalog: ItemCatalog,
        items: Vec<u8>,
        targets: Vec<TargetRow>,
        context: InventoryContext,
    ) -> Self {
        let filtered_items = filter_items(&items, &catalog, context);
        // Empty filter still starts at the browsing screen - engines need
        // to render the "no items" overlay rather than insta-abort.
        let state = InventoryUseState::Browsing { cursor: 0 };
        Self {
            items,
            targets,
            filtered_items,
            state,
            events: Vec::new(),
            context,
            catalog,
        }
    }

    /// True if this session is in a terminal state (`Done` / `Aborted`).
    pub fn is_done(&self) -> bool {
        matches!(
            self.state,
            InventoryUseState::Done(_) | InventoryUseState::Aborted
        )
    }

    /// The item row currently highlighted under the browsing cursor.
    /// Returns `None` if the session is in a non-browsing state, or if
    /// the filtered list is empty.
    pub fn current_item(&self) -> Option<&ItemEntry> {
        let cursor = match self.state {
            InventoryUseState::Browsing { cursor } => cursor,
            InventoryUseState::TargetSelect { item_cursor, .. } => item_cursor,
            _ => return None,
        };
        let item_idx = self.filtered_items.get(cursor).copied()?;
        let id = self.items.get(item_idx).copied()?;
        self.catalog.get(id)
    }

    /// Drain the event log. Engines call this once per frame to surface
    /// UI blips / log entries.
    pub fn drain_events(&mut self) -> Vec<InventoryUseEvent> {
        std::mem::take(&mut self.events)
    }

    /// Advance the session by one input event.
    pub fn input(&mut self, input: InventoryUseInput) {
        match self.state {
            InventoryUseState::Browsing { cursor } => self.input_browsing(cursor, input),
            InventoryUseState::TargetSelect {
                item_cursor,
                cursor,
            } => self.input_target_select(item_cursor, cursor, input),
            // Terminal states swallow input.
            _ => {}
        }
    }

    fn input_browsing(&mut self, cursor: usize, input: InventoryUseInput) {
        let n = self.filtered_items.len();
        match input {
            InventoryUseInput::Up => {
                if n == 0 {
                    return;
                }
                let new_cursor = if cursor == 0 { n - 1 } else { cursor - 1 };
                self.state = InventoryUseState::Browsing { cursor: new_cursor };
                self.events.push(InventoryUseEvent::CursorMoved);
            }
            InventoryUseInput::Down => {
                if n == 0 {
                    return;
                }
                let new_cursor = if cursor + 1 >= n { 0 } else { cursor + 1 };
                self.state = InventoryUseState::Browsing { cursor: new_cursor };
                self.events.push(InventoryUseEvent::CursorMoved);
            }
            InventoryUseInput::Confirm => {
                if n == 0 || self.targets.is_empty() {
                    self.events.push(InventoryUseEvent::InvalidConfirm);
                    return;
                }
                // Start the cursor on the first target that's valid for this
                // item's effect, so offensive items land on an enemy and heals
                // land on an ally without the player scrolling past the wrong
                // side. Falls back to row 0 when nothing matches.
                let start = self
                    .current_item()
                    .map(|e| e.effect)
                    .and_then(|eff| {
                        self.targets
                            .iter()
                            .position(|t| target_valid_for_effect(&eff, t))
                    })
                    .unwrap_or(0);
                self.state = InventoryUseState::TargetSelect {
                    item_cursor: cursor,
                    cursor: start,
                };
                self.events.push(InventoryUseEvent::EnteredTargetSelect);
            }
            InventoryUseInput::Cancel => {
                self.state = InventoryUseState::Aborted;
                self.events.push(InventoryUseEvent::Cancelled);
            }
        }
    }

    fn input_target_select(&mut self, item_cursor: usize, cursor: usize, input: InventoryUseInput) {
        let n = self.targets.len();
        match input {
            InventoryUseInput::Up => {
                if n == 0 {
                    return;
                }
                let new_cursor = if cursor == 0 { n - 1 } else { cursor - 1 };
                self.state = InventoryUseState::TargetSelect {
                    item_cursor,
                    cursor: new_cursor,
                };
                self.events.push(InventoryUseEvent::CursorMoved);
            }
            InventoryUseInput::Down => {
                if n == 0 {
                    return;
                }
                let new_cursor = if cursor + 1 >= n { 0 } else { cursor + 1 };
                self.state = InventoryUseState::TargetSelect {
                    item_cursor,
                    cursor: new_cursor,
                };
                self.events.push(InventoryUseEvent::CursorMoved);
            }
            InventoryUseInput::Confirm => {
                let Some(target) = self.targets.get(cursor).cloned() else {
                    self.events.push(InventoryUseEvent::InvalidConfirm);
                    return;
                };
                let Some(entry) = self.current_item().copied() else {
                    self.events.push(InventoryUseEvent::InvalidConfirm);
                    return;
                };
                if !self.context.allows(&entry) {
                    self.events.push(InventoryUseEvent::InvalidConfirm);
                    return;
                }
                if !target_valid_for_effect(&entry.effect, &target) {
                    self.events.push(InventoryUseEvent::InvalidConfirm);
                    return;
                }
                let outcome = crate::items::apply_effect(entry.effect, &target.snapshot());
                self.state = InventoryUseState::Done(outcome);
                self.events.push(InventoryUseEvent::Used {
                    slot: target.slot,
                    outcome,
                });
            }
            InventoryUseInput::Cancel => {
                self.state = InventoryUseState::Browsing {
                    cursor: item_cursor,
                };
                self.events.push(InventoryUseEvent::Cancelled);
            }
        }
    }
}

fn filter_items(items: &[u8], catalog: &ItemCatalog, context: InventoryContext) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, id) in items.iter().enumerate() {
        if let Some(entry) = catalog.get(*id)
            && context.allows(entry)
        {
            out.push(i);
        }
    }
    out
}

/// Validate that the picked target makes sense for the chosen effect, and
/// that it is on the right side of the field. Revive needs a dead ally;
/// offensive items (Damage / Capture) need a living enemy; Escape doesn't
/// care which side; key items are never usable here; everything else
/// (heals / cures / stat boosts / spirit) needs a living ally.
fn target_valid_for_effect(effect: &ItemEffect, target: &TargetRow) -> bool {
    match effect {
        ItemEffect::Revive { .. } => target.is_dead() && !target.is_enemy,
        ItemEffect::KeyItem => false,
        ItemEffect::Damage { .. } | ItemEffect::Capture { .. } => target.alive && target.is_enemy,
        ItemEffect::Escape => target.alive,
        _ => target.alive && !target.is_enemy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::ItemCatalog;

    fn party_targets() -> Vec<TargetRow> {
        vec![
            TargetRow::new(0, "Vahn").with_stats(100, 200, 10, 30),
            TargetRow::new(1, "Noa").with_stats(150, 200, 5, 20),
        ]
    }

    /// Small synthetic catalog with stable ids covering every session-logic
    /// case (single heal, revive, field-only, battle-only, offensive). These
    /// tests exercise the *session* state machine, so they use fixed test ids
    /// rather than [`ItemCatalog::vanilla`]'s real retail ids/amounts (those are
    /// validated in `items.rs` + the disc-gated `item_catalog_disc` test).
    fn test_catalog() -> ItemCatalog {
        use crate::items::{ItemEffect, ItemEntry};
        let mut c = ItemCatalog::new();
        let heal = |id, name| ItemEntry {
            id,
            name,
            effect: ItemEffect::Heal { amount: 100 },
            usable_in_battle: true,
            usable_in_field: true,
        };
        c.insert(heal(0x01, "Test Heal A"));
        c.insert(heal(0x02, "Test Heal B"));
        c.insert(heal(0x03, "Test Heal C"));
        c.insert(ItemEntry {
            id: 0x0C,
            name: "Test Revive",
            effect: ItemEffect::Revive { factor: 128 }, // 50%
            usable_in_battle: true,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x0E,
            name: "Test Field-Only",
            effect: ItemEffect::Heal { amount: 50 },
            usable_in_battle: false,
            usable_in_field: true,
        });
        c.insert(ItemEntry {
            id: 0x10,
            name: "Test Battle-Only",
            effect: ItemEffect::Spirit { amount: 5 },
            usable_in_battle: true,
            usable_in_field: false,
        });
        c.insert(ItemEntry {
            id: 0x13,
            name: "Test Offensive",
            effect: ItemEffect::Damage { amount: 200 },
            usable_in_battle: true,
            usable_in_field: false,
        });
        c
    }

    fn empty_session(items: Vec<u8>, ctx: InventoryContext) -> InventoryUseSession {
        InventoryUseSession::new(test_catalog(), items, party_targets(), ctx)
    }

    #[test]
    fn empty_inventory_starts_at_browsing_with_no_items() {
        let s = empty_session(vec![], InventoryContext::Battle);
        assert!(matches!(s.state, InventoryUseState::Browsing { cursor: 0 }));
        assert_eq!(s.filtered_items.len(), 0);
        assert!(s.current_item().is_none());
    }

    #[test]
    fn confirm_with_empty_inventory_logs_invalid() {
        let mut s = empty_session(vec![], InventoryContext::Battle);
        s.input(InventoryUseInput::Confirm);
        let evs = s.drain_events();
        assert_eq!(evs, vec![InventoryUseEvent::InvalidConfirm]);
    }

    #[test]
    fn cancel_from_browsing_aborts_session() {
        let mut s = empty_session(vec![0x01, 0x02], InventoryContext::Battle);
        s.input(InventoryUseInput::Cancel);
        assert!(matches!(s.state, InventoryUseState::Aborted));
        assert!(s.is_done());
    }

    #[test]
    fn confirm_browsing_enters_target_select() {
        let mut s = empty_session(vec![0x01], InventoryContext::Battle);
        s.input(InventoryUseInput::Confirm);
        assert!(matches!(
            s.state,
            InventoryUseState::TargetSelect {
                item_cursor: 0,
                cursor: 0
            }
        ));
        let evs = s.drain_events();
        assert_eq!(evs, vec![InventoryUseEvent::EnteredTargetSelect]);
    }

    fn mixed_targets() -> Vec<TargetRow> {
        vec![
            TargetRow::new(0, "Vahn").with_stats(100, 200, 10, 30),
            TargetRow::new(3, "Slug")
                .with_stats(60, 60, 0, 0)
                .with_enemy(true),
        ]
    }

    #[test]
    fn offensive_item_starts_cursor_on_the_enemy_row() {
        // Bomb (0x13) is offensive: confirming from browsing should skip the
        // ally row and land the target cursor on the enemy.
        let mut s = InventoryUseSession::new(
            test_catalog(),
            vec![0x13],
            mixed_targets(),
            InventoryContext::Battle,
        );
        s.input(InventoryUseInput::Confirm);
        match s.state {
            InventoryUseState::TargetSelect { cursor, .. } => assert_eq!(cursor, 1),
            other => panic!("expected TargetSelect, got {other:?}"),
        }
    }

    #[test]
    fn healing_item_buzzes_when_confirmed_on_an_enemy_row() {
        // Healing Leaf (0x01) is ally-only. Force the cursor onto the enemy
        // row and confirm - it must reject with InvalidConfirm.
        let mut s = InventoryUseSession::new(
            test_catalog(),
            vec![0x01],
            mixed_targets(),
            InventoryContext::Battle,
        );
        s.input(InventoryUseInput::Confirm); // -> TargetSelect, cursor on ally (0)
        s.input(InventoryUseInput::Down); // move onto the enemy row
        s.drain_events();
        s.input(InventoryUseInput::Confirm);
        assert!(
            matches!(s.state, InventoryUseState::TargetSelect { .. }),
            "still selecting - the enemy is not a valid heal target"
        );
        assert!(
            s.drain_events()
                .contains(&InventoryUseEvent::InvalidConfirm),
            "buzzed on the wrong-side target"
        );
    }

    #[test]
    fn down_in_browsing_wraps_around_at_end() {
        let mut s = empty_session(vec![0x01, 0x02], InventoryContext::Battle);
        s.input(InventoryUseInput::Down);
        assert!(matches!(s.state, InventoryUseState::Browsing { cursor: 1 }));
        s.input(InventoryUseInput::Down);
        assert!(matches!(s.state, InventoryUseState::Browsing { cursor: 0 }));
    }

    #[test]
    fn up_in_browsing_wraps_to_last() {
        let mut s = empty_session(vec![0x01, 0x02, 0x03], InventoryContext::Battle);
        s.input(InventoryUseInput::Up);
        assert!(matches!(s.state, InventoryUseState::Browsing { cursor: 2 }));
    }

    #[test]
    fn cancel_from_target_returns_to_browsing() {
        let mut s = empty_session(vec![0x01, 0x02], InventoryContext::Battle);
        // Move to second item, enter target select, then cancel.
        s.input(InventoryUseInput::Down);
        s.input(InventoryUseInput::Confirm);
        s.input(InventoryUseInput::Cancel);
        assert!(matches!(s.state, InventoryUseState::Browsing { cursor: 1 }));
    }

    #[test]
    fn confirm_target_completes_with_heal_outcome() {
        // Healing Leaf (id 0x01) heals 100 HP. Vahn at 100/200 -> +100.
        let mut s = empty_session(vec![0x01], InventoryContext::Battle);
        s.input(InventoryUseInput::Confirm);
        s.input(InventoryUseInput::Confirm);
        match s.state {
            InventoryUseState::Done(ItemOutcome::HealedHp { amount }) => {
                assert_eq!(amount, 100);
            }
            other => panic!("expected HealedHp, got {:?}", other),
        }
        // The Used event records the slot.
        let evs = s.drain_events();
        let used = evs
            .iter()
            .find(|e| matches!(e, InventoryUseEvent::Used { .. }));
        assert!(used.is_some());
    }

    #[test]
    fn down_in_target_select_wraps_around() {
        let mut s = empty_session(vec![0x01], InventoryContext::Battle);
        s.input(InventoryUseInput::Confirm);
        s.input(InventoryUseInput::Down);
        if let InventoryUseState::TargetSelect { cursor, .. } = s.state {
            assert_eq!(cursor, 1);
        }
        s.input(InventoryUseInput::Down);
        if let InventoryUseState::TargetSelect { cursor, .. } = s.state {
            assert_eq!(cursor, 0);
        }
    }

    #[test]
    fn revive_targeting_alive_actor_emits_invalid_confirm() {
        // Resurrection Leaf (id 0x0C) - only valid on dead targets.
        let mut s = empty_session(vec![0x0C], InventoryContext::Battle);
        s.input(InventoryUseInput::Confirm); // -> target select
        s.input(InventoryUseInput::Confirm); // confirm on Vahn (alive)
        // Should remain in target-select with InvalidConfirm event.
        assert!(matches!(s.state, InventoryUseState::TargetSelect { .. }));
        let evs = s.drain_events();
        let invalid = evs
            .iter()
            .find(|e| matches!(e, InventoryUseEvent::InvalidConfirm));
        assert!(invalid.is_some());
    }

    #[test]
    fn revive_targeting_dead_actor_completes() {
        let mut targets = party_targets();
        targets[0].alive = false;
        targets[0].hp = 0;
        let mut s = InventoryUseSession::new(
            test_catalog(),
            vec![0x0C],
            targets,
            InventoryContext::Battle,
        );
        s.input(InventoryUseInput::Confirm);
        s.input(InventoryUseInput::Confirm);
        match s.state {
            InventoryUseState::Done(ItemOutcome::Revived { hp_after }) => {
                // 50% of 200 = 100.
                assert_eq!(hp_after, 100);
            }
            other => panic!("expected Revived, got {:?}", other),
        }
    }

    #[test]
    fn field_only_item_filtered_out_of_battle_context() {
        // Power Tonic (0x0E) is field-only.
        let s = empty_session(vec![0x0E], InventoryContext::Battle);
        assert_eq!(s.filtered_items.len(), 0);
    }

    #[test]
    fn battle_only_item_filtered_out_of_field_context() {
        // Spirit Sphere (0x10) is battle-only.
        let s = empty_session(vec![0x10], InventoryContext::Field);
        assert_eq!(s.filtered_items.len(), 0);
    }

    #[test]
    fn current_item_after_navigation_returns_correct_entry() {
        let mut s = empty_session(vec![0x01, 0x02, 0x03], InventoryContext::Battle);
        s.input(InventoryUseInput::Down);
        s.input(InventoryUseInput::Down);
        let entry = s.current_item().unwrap();
        assert_eq!(entry.id, 0x03);
    }

    #[test]
    fn drain_events_returns_log_then_clears() {
        let mut s = empty_session(vec![0x01, 0x02], InventoryContext::Battle);
        s.input(InventoryUseInput::Down);
        s.input(InventoryUseInput::Down);
        let evs = s.drain_events();
        assert_eq!(evs.len(), 2);
        assert!(
            evs.iter()
                .all(|e| matches!(e, InventoryUseEvent::CursorMoved))
        );
        // Second drain returns empty.
        assert!(s.drain_events().is_empty());
    }

    #[test]
    fn session_swallows_input_in_done_state() {
        let mut s = empty_session(vec![0x01], InventoryContext::Battle);
        s.input(InventoryUseInput::Confirm);
        s.input(InventoryUseInput::Confirm);
        let prior_state = s.state;
        s.input(InventoryUseInput::Up);
        s.input(InventoryUseInput::Confirm);
        assert_eq!(s.state, prior_state);
    }

    #[test]
    fn target_row_snapshot_carries_dead_flag() {
        let mut t = TargetRow::new(0, "Vahn").with_stats(0, 200, 0, 0);
        t.alive = false;
        let snap = t.snapshot();
        assert!(snap.is_dead);
        assert_eq!(snap.hp_max, 200);
    }

    #[test]
    fn key_item_target_validation_always_false() {
        let target = TargetRow::new(0, "Vahn").with_stats(100, 100, 0, 0);
        assert!(!target_valid_for_effect(&ItemEffect::KeyItem, &target));
    }

    #[test]
    fn inventory_context_allows_field_or_battle_per_entry() {
        let cat = test_catalog();
        let healing_leaf = cat.get(0x01).unwrap();
        assert!(InventoryContext::Battle.allows(healing_leaf));
        assert!(InventoryContext::Field.allows(healing_leaf));

        let spirit = cat.get(0x10).unwrap();
        assert!(InventoryContext::Battle.allows(spirit));
        assert!(!InventoryContext::Field.allows(spirit));
    }
}
