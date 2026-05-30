//! Faithful, memory-safe model of the retail consumable-item inventory window.
//!
//! This is a *reverse-engineering / preservation* model of the fixed-window
//! item inventory used by `SCUS_942.54`, not the engine's gameplay inventory.
//! It reproduces the retail accessor family's exact slot order and stack-cap
//! arithmetic so the behaviour (including the well-known out-of-bounds add
//! primitive) can be reasoned about as data without ever performing an unsafe
//! write.
//!
//! ## Retail layout
//!
//! The item window is a fixed array of 2-byte `(id, count)` slots at
//! [`ITEM_WINDOW_BASE`] (`0x80085958` = SC+0x1818). The window length is
//! `gp[+0x2D4]` = [`ITEM_WINDOW_SLOTS`] (72 slots for the consumable-item
//! page). Stacks cap at [`STACK_CAP`] (99).
//!
//! ## Accessor family (`SCUS_942.54`)
//!
//! - `FUN_80042EE0` — find-slot-by-id: linear scan `[0, window)`, returns the
//!   slot index or none. Bounded.
//! - `FUN_80042F4C` — find-count-by-id: find-slot then return the count byte
//!   (0 if absent). Bounded.
//! - `FUN_80042310` — consume-by-id: find-slot; decrement count; when it hits
//!   0, compact via `FUN_800423E0`. Bounded.
//! - `FUN_800423E0` — compact/merge: shift slots down to fill a freed gap and
//!   zero the tail. Stack cap 99.
//! - `FUN_800421D4` — ADD (the OOB primitive). MERGE pass first (existing id →
//!   `count = min(count + qty, 99)`), then a FREE-SLOT pass (first `id == 0`).
//!
//! ## The out-of-bounds add primitive
//!
//! In `FUN_800421D4` the id store `sb t0,0x1818(a0)` at `0x800422BC` writes the
//! item id to `slot[i]` **before** the `slt` bound check that guards only the
//! *count* store. On a FULL bag the free-slot scan reaches `i == window` with
//! no empty slot, so step 3 writes the id byte **one slot past the window** at
//! `ITEM_WINDOW_BASE + window * 2 = 0x800859E8`, and the bound check then fails
//! so the count is never written. `0x800859E8` = SC+0x18A8 = the first byte of
//! the KEY-ITEM list immediately following the consumable-item window.
//!
//! This model surfaces that primitive as the [`AddOutcome::OobIdWrite`] data
//! variant (carrying the would-be target address) and performs **no** write,
//! leaving the modelled inventory unchanged.
//!
//! See [`docs/reference/memory-map.md`](../../../docs/reference/memory-map.md).

// PORT: FUN_800421D4 (ADD) / FUN_80042EE0 (find-slot) / FUN_80042F4C (find-count)
// PORT: FUN_80042310 (consume) / FUN_800423E0 (compact)
// REF: docs/reference/memory-map.md "Retail inventory accessors (SCUS_942.54)"

/// Base address of the consumable-item window (`= SC+0x1818`).
pub const ITEM_WINDOW_BASE: u32 = 0x8008_5958;

/// Number of slots in the consumable-item page (`gp[+0x2D4]`).
pub const ITEM_WINDOW_SLOTS: usize = 72;

/// Per-stack count cap enforced by the retail add/merge paths.
pub const STACK_CAP: u8 = 99;

/// The address one slot past a `(base, window)` window — the byte the retail
/// add primitive (`FUN_800421D4`) writes the id to on a full bag.
///
/// For the default consumable window this is `0x800859E8` (= SC+0x18A8).
#[must_use]
pub fn oob_target(base: u32, window_slots: usize) -> u32 {
    base + (window_slots as u32) * 2
}

/// Outcome of an [`RetailInventory::add`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddOutcome {
    /// The id already had a stack; its count was raised (capped at
    /// [`STACK_CAP`]).
    Merged {
        /// Slot index whose count was updated.
        slot: usize,
        /// Count after the merge (`<= STACK_CAP`).
        new_count: u8,
    },
    /// The id was placed into the lowest empty slot.
    Placed {
        /// Slot index that received the new stack.
        slot: usize,
    },
    /// The bag was full: the retail code would have written the id byte one
    /// slot past the window before its (failing) bound check. No write is
    /// performed by this model; the count is never applied on this path.
    OobIdWrite {
        /// Address the retail id store would have hit (`= base + window * 2`).
        oob_target: u32,
    },
}

/// A faithful, memory-safe model of the retail fixed-window item inventory.
///
/// `slots.len()` is always the window length; each slot is a `(id, count)`
/// pair. An `id == 0` slot is empty (the retail empty sentinel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetailInventory {
    slots: Vec<(u8, u8)>,
    base: u32,
}

impl RetailInventory {
    /// Create an empty inventory with `window_slots` `(0, 0)` slots at `base`.
    #[must_use]
    pub fn new(base: u32, window_slots: usize) -> Self {
        Self {
            slots: vec![(0, 0); window_slots],
            base,
        }
    }

    /// Create an inventory from explicit slot contents.
    ///
    /// The window length becomes `slots.len()`.
    #[must_use]
    pub fn from_slots(base: u32, slots: Vec<(u8, u8)>) -> Self {
        Self { slots, base }
    }

    /// Base address of the window.
    #[must_use]
    pub fn base(&self) -> u32 {
        self.base
    }

    /// Number of slots in the window.
    #[must_use]
    pub fn window_slots(&self) -> usize {
        self.slots.len()
    }

    /// The window's slots as `(id, count)` pairs.
    #[must_use]
    pub fn slots(&self) -> &[(u8, u8)] {
        &self.slots
    }

    /// The address one slot past this window (the full-bag OOB id target).
    #[must_use]
    pub fn oob_target(&self) -> u32 {
        oob_target(self.base, self.slots.len())
    }

    /// Find the slot holding `id`, scanning `[0, window)`.
    ///
    /// `id == 0` is the empty sentinel and is never matched.
    // PORT: FUN_80042EE0
    #[must_use]
    pub fn find_slot(&self, id: u8) -> Option<usize> {
        if id == 0 {
            return None;
        }
        self.slots.iter().position(|&(sid, _)| sid == id)
    }

    /// Return the count of `id`, or 0 if it is absent.
    // PORT: FUN_80042F4C
    #[must_use]
    pub fn find_count(&self, id: u8) -> u8 {
        match self.find_slot(id) {
            Some(i) => self.slots[i].1,
            None => 0,
        }
    }

    /// Consume `qty` of `id`. Returns `true` if the item was present and
    /// decremented; when a stack hits 0 the window is compacted.
    // PORT: FUN_80042310 (+ compact via FUN_800423E0)
    pub fn consume(&mut self, id: u8, qty: u8) -> bool {
        let Some(i) = self.find_slot(id) else {
            return false;
        };
        let count = self.slots[i].1;
        let new_count = count.saturating_sub(qty);
        self.slots[i].1 = new_count;
        if new_count == 0 {
            self.compact();
        }
        true
    }

    /// Compact the window: drop emptied slots (`id == 0` *or* `count == 0`),
    /// shift the survivors down, and zero the tail.
    // PORT: FUN_800423E0
    pub fn compact(&mut self) {
        let window = self.slots.len();
        let mut survivors: Vec<(u8, u8)> = self
            .slots
            .iter()
            .copied()
            .filter(|&(id, count)| id != 0 && count != 0)
            .collect();
        survivors.resize(window, (0, 0));
        self.slots = survivors;
    }

    /// Add `qty` of `id`, reproducing the retail order: merge into an existing
    /// stack first (capped at [`STACK_CAP`]), else place into the lowest empty
    /// slot, else surface the full-bag OOB id-store primitive as
    /// [`AddOutcome::OobIdWrite`] **without** writing anything.
    // PORT: FUN_800421D4
    pub fn add(&mut self, id: u8, qty: u8) -> AddOutcome {
        // (1) MERGE pass: existing stack of the same id.
        if let Some(i) = self.find_slot(id) {
            let new_count = self.slots[i].1.saturating_add(qty).min(STACK_CAP);
            self.slots[i].1 = new_count;
            return AddOutcome::Merged { slot: i, new_count };
        }
        // (2) FREE-SLOT pass: first empty slot (id == 0).
        if let Some(i) = self.slots.iter().position(|&(sid, _)| sid == 0) {
            self.slots[i] = (id, qty.min(STACK_CAP));
            return AddOutcome::Placed { slot: i };
        }
        // (3) FULL bag: retail would store the id one slot past the window
        // before its (failing) bound check. Surface as data; perform no write.
        AddOutcome::OobIdWrite {
            oob_target: self.oob_target(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_inv() -> RetailInventory {
        RetailInventory::new(ITEM_WINDOW_BASE, ITEM_WINDOW_SLOTS)
    }

    #[test]
    fn oob_target_is_sc_plus_0x18a8() {
        assert_eq!(oob_target(ITEM_WINDOW_BASE, ITEM_WINDOW_SLOTS), 0x8008_59E8);
        // SC base = ITEM_WINDOW_BASE - 0x1818; OOB target = SC + 0x18A8.
        let sc = ITEM_WINDOW_BASE - 0x1818;
        assert_eq!(0x8008_59E8, sc + 0x18A8);
    }

    #[test]
    fn merge_caps_at_99() {
        let mut inv = default_inv();
        assert_eq!(inv.add(0x10, 50), AddOutcome::Placed { slot: 0 });
        assert_eq!(
            inv.add(0x10, 80),
            AddOutcome::Merged {
                slot: 0,
                new_count: STACK_CAP
            }
        );
        assert_eq!(inv.find_count(0x10), 99);
    }

    #[test]
    fn add_places_in_lowest_empty_slot() {
        let mut inv = default_inv();
        assert_eq!(inv.add(0x10, 1), AddOutcome::Placed { slot: 0 });
        assert_eq!(inv.add(0x11, 2), AddOutcome::Placed { slot: 1 });
        assert_eq!(inv.add(0x12, 3), AddOutcome::Placed { slot: 2 });
        // Consuming slot 1 to zero compacts; the next add reuses the lowest gap.
        assert!(inv.consume(0x11, 2));
        // After compaction: [0x10,0x12, empty...] — new id goes to slot 2.
        assert_eq!(inv.add(0x13, 4), AddOutcome::Placed { slot: 2 });
    }

    #[test]
    fn full_bag_add_returns_oob_and_does_not_mutate() {
        // Fill every slot with distinct non-zero ids.
        let slots: Vec<(u8, u8)> = (0..ITEM_WINDOW_SLOTS)
            .map(|i| ((i as u8).wrapping_add(1), 5))
            .collect();
        let mut inv = RetailInventory::from_slots(ITEM_WINDOW_BASE, slots.clone());
        let before = inv.clone();
        let outcome = inv.add(0xFE, 7);
        assert_eq!(
            outcome,
            AddOutcome::OobIdWrite {
                oob_target: 0x8008_59E8
            }
        );
        // No count applied, no slot mutated, no panic.
        assert_eq!(inv, before);
        assert_eq!(inv.slots(), &slots[..]);
    }

    #[test]
    fn full_bag_with_matching_id_still_merges() {
        // A full bag whose ids include the target id merges (no OOB).
        let mut slots: Vec<(u8, u8)> = (0..ITEM_WINDOW_SLOTS)
            .map(|i| ((i as u8).wrapping_add(1), 5))
            .collect();
        slots[3] = (0x40, 10);
        let mut inv = RetailInventory::from_slots(ITEM_WINDOW_BASE, slots);
        assert_eq!(
            inv.add(0x40, 3),
            AddOutcome::Merged {
                slot: 3,
                new_count: 13
            }
        );
    }

    #[test]
    fn find_slot_and_count_bounded() {
        let mut inv = default_inv();
        assert_eq!(inv.find_slot(0x20), None);
        assert_eq!(inv.find_count(0x20), 0);
        // id 0 is the empty sentinel: never matched.
        assert_eq!(inv.find_slot(0), None);
        inv.add(0x20, 9);
        assert_eq!(inv.find_slot(0x20), Some(0));
        assert_eq!(inv.find_count(0x20), 9);
    }

    #[test]
    fn consume_to_zero_compacts() {
        let mut inv = default_inv();
        inv.add(0xA0, 1);
        inv.add(0xA1, 4);
        inv.add(0xA2, 2);
        // Consume the middle stack to zero; survivors shift down.
        assert!(inv.consume(0xA1, 4));
        assert_eq!(inv.slots()[0], (0xA0, 1));
        assert_eq!(inv.slots()[1], (0xA2, 2));
        assert_eq!(inv.slots()[2], (0, 0));
        assert_eq!(inv.find_slot(0xA1), None);
        // Partial consume leaves the stack in place.
        assert!(inv.consume(0xA2, 1));
        assert_eq!(inv.find_count(0xA2), 1);
        // Consuming an absent id is a no-op false.
        assert!(!inv.consume(0xFF, 1));
    }
}
