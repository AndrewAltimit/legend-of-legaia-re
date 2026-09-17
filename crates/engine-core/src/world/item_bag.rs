//! The party's item bag: retail's 256-slot array, with a map-shaped adapter
//! over it.
//!
//! Retail keeps one flat array of 2-byte `[id][count]` slots at `0x80085958`
//! and every accessor is bounded by an **active window**
//! (`gp[+0x2D2] / gp[+0x2D4]`, see [`docs/subsystems/inventory.md`]). The
//! engine used to keep a `HashMap<u8, u8>` instead, which is a different
//! *shape* and not just a different container: a map has no slot coordinate,
//! so it cannot express a hole, cannot express the same id in two slots before
//! a normalize merges them, and cannot be indexed by a number.
//!
//! One consumer indexes the bag by slot - PROT 0941's Steal, a rejection
//! sampler over the physical array - and that is what this type exists for.
//! Every other consumer addresses the bag by id, so they keep the map-shaped
//! calls ([`Self::get`], [`Self::insert`], [`Self::entry`], ...); those are an
//! adapter over the array rather than a second store, and their iteration
//! order is **slot order**, which is the order retail's pause-menu pages and
//! shop sell list walk.
//!
//! The slot arithmetic itself is not re-implemented here: it lives in
//! [`legaia_save::retail_inventory`], where the retail accessor family
//! (`FUN_800421D4` add, `FUN_80042310` consume, `FUN_800423E0` normalize,
//! `FUN_8004313C` window select) is ported once. This type is the engine's
//! seat at that model.
//!
//! [`docs/subsystems/inventory.md`]: ../../../../docs/subsystems/inventory.md

use legaia_save::retail_inventory::{
    AddOutcome, ITEM_SLOTS_TOTAL, ItemWindow, RetailInventory, STACK_CAP,
};

/// The party's bag: 256 physical `(id, count)` slots plus the active window.
///
/// `Default` is an empty bag with the full window installed - the state a
/// disc-free host and a fresh world both start in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemBag {
    inv: RetailInventory,
    /// Scratch count for a [`BagEntry::or_insert`] whose claim failed on a
    /// full window: the caller's `+= 1` lands here and is dropped, where
    /// retail's add helper would have written one byte past the window.
    overflow: u8,
}

impl Default for ItemBag {
    fn default() -> Self {
        Self::new()
    }
}

impl ItemBag {
    /// An empty 256-slot bag with [`ItemWindow::Full`] installed.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inv: RetailInventory::new(
                legaia_save::retail_inventory::ITEM_WINDOW_BASE,
                ITEM_SLOTS_TOTAL,
            ),
            overflow: 0,
        }
    }

    /// Build a bag from the physical slot array of a save (SC `+0x1818`), in
    /// slot order, holes included. Shorter input is zero-padded, longer is
    /// truncated.
    #[must_use]
    pub fn from_slots(slots: &[(u8, u8)]) -> Self {
        let mut v = vec![(0u8, 0u8); ITEM_SLOTS_TOTAL];
        for (i, s) in slots.iter().take(ITEM_SLOTS_TOTAL).enumerate() {
            v[i] = *s;
        }
        Self {
            inv: RetailInventory::from_slots(legaia_save::retail_inventory::ITEM_WINDOW_BASE, v),
            overflow: 0,
        }
    }

    /// The physical array, slot order, holes included - what a save block
    /// carries and what the Steal sampler draws over.
    #[must_use]
    pub fn slots(&self) -> &[(u8, u8)] {
        self.inv.slots()
    }

    /// The active window as `(start, end)` slot indices.
    #[must_use]
    pub fn window_bounds(&self) -> (usize, usize) {
        self.inv.window_bounds()
    }

    /// The installed window, as the selector's own three-way answer.
    #[must_use]
    pub fn window(&self) -> ItemWindow {
        match self.inv.window_bounds() {
            (0, e) if e <= ITEM_SLOTS_TOTAL / 2 => ItemWindow::Low,
            (s, _) if s > 0 => ItemWindow::High,
            _ => ItemWindow::Full,
        }
    }

    /// Install an explicit window (a host that knows the answer, and the
    /// tests).
    pub fn set_window(&mut self, window: ItemWindow) {
        self.inv.set_window(window);
    }

    /// Install the window `FUN_8004313C` would from the live party: the member
    /// count, story flag 20, and whether the lone member is someone other than
    /// Vahn.
    ///
    /// `members == 0` leaves the installed window alone, which is the
    /// selector's own early return.
    ///
    /// REF: FUN_8004313C (ported as `ItemWindow::select`)
    pub fn install_window_for_party(&mut self, members: u8, full_window_flag: bool, solo_id: u8) {
        if let Some(w) = ItemWindow::select(members, full_window_flag, solo_id != 0) {
            self.inv.set_window(w);
        }
    }

    // ------------------------------------------------------------------
    // Slot-addressed API (the half a map cannot express)
    // ------------------------------------------------------------------

    /// The `(id, count)` pair at a physical slot, window or no window - the
    /// read PROT 0941's draw makes.
    #[must_use]
    pub fn slot(&self, slot: u8) -> (u8, u8) {
        self.inv
            .slots()
            .get(slot as usize)
            .copied()
            .unwrap_or((0, 0))
    }

    /// Take `amount` off a physical slot through retail's own **by-slot**
    /// consume helper, returning the count left in it.
    ///
    /// This is the accessor a *row payload* needs: retail's list entries carry
    /// a bag slot, so a Use or a Throw Out acts on the slot the player pointed
    /// at rather than on whichever slot a window scan finds the id in. The
    /// helper zeroes the id byte in place when the stack empties and leaves the
    /// hole - it does not compact.
    ///
    /// REF: FUN_80043048 (ported as `RetailInventory::consume_slot`)
    pub fn consume_slot(&mut self, slot: u8, amount: u8) -> u8 {
        self.inv.consume_slot(i16::from(slot), amount, 0)
    }

    /// Remove one of `id` through retail's window-bounded consume helper,
    /// returning the slot it came out of or [`legaia_save::retail_inventory::NOT_IN_WINDOW`].
    ///
    /// The sentinel is not an error path: a consumer that picked its id from
    /// outside the active window (Steal does) gets it, and nothing is removed.
    pub fn consume_returning_slot(&mut self, id: u8, qty: u8) -> u16 {
        self.inv.consume_returning_slot(id, qty)
    }

    /// Merge duplicate stacks and squeeze holes inside the window
    /// (`FUN_800423E0`).
    pub fn normalize(&mut self) {
        self.inv.normalize();
    }

    /// Place `qty` of `id` through retail's add helper: merge into an existing
    /// stack (capped at 99), else the window's first free slot. `false` when
    /// the window is full (retail's OOB arm, which this model surfaces rather
    /// than performs).
    pub fn add(&mut self, id: u8, qty: u8) -> bool {
        if id == 0 {
            return false;
        }
        !matches!(self.inv.add(id, qty), AddOutcome::OobIdWrite { .. })
    }

    // ------------------------------------------------------------------
    // Map-shaped adapter
    // ------------------------------------------------------------------

    /// Count of `id`, or `None` when no slot **in the window** holds it.
    #[must_use]
    pub fn get(&self, id: &u8) -> Option<&u8> {
        let slot = self.inv.find_slot(*id)?;
        Some(&self.inv.slots()[slot].1)
    }

    /// Mutable count of `id`, or `None` when no slot in the window holds it.
    pub fn get_mut(&mut self, id: &u8) -> Option<&mut u8> {
        let slot = self.inv.find_slot(*id)?;
        Some(&mut self.inv.slots_mut()[slot].1)
    }

    /// Set `id`'s count, claiming a free slot when it is not held yet.
    /// Returns the previous count the way `HashMap::insert` does.
    ///
    /// A count of `0` is kept rather than dropped: retail's occupancy test is
    /// `id != 0` alone, so a live id with a zero count is a state the array
    /// has and the map did not.
    pub fn insert(&mut self, id: u8, count: u8) -> Option<u8> {
        if id == 0 {
            return None;
        }
        if let Some(slot) = self.inv.find_slot(id) {
            let prev = self.inv.slots()[slot].1;
            self.inv.slots_mut()[slot].1 = count;
            return Some(prev);
        }
        // Not held: claim the window's first free slot. `add` is the retail
        // path to a free slot; the count is then set outright because this is
        // an assignment, not a grant.
        if self.add(id, count.min(STACK_CAP))
            && let Some(slot) = self.inv.find_slot(id)
        {
            self.inv.slots_mut()[slot].1 = count;
        }
        None
    }

    /// Drop `id` entirely (id byte and count), returning its count.
    ///
    /// This is the *map* removal, not `FUN_80042310`: a consume leaves the
    /// count it decremented behind and only zeroes the id when it reaches
    /// zero. Use [`Self::consume_returning_slot`] where retail's helper is
    /// what is being modelled.
    pub fn remove(&mut self, id: &u8) -> Option<u8> {
        let slot = self.inv.find_slot(*id)?;
        let prev = self.inv.slots()[slot].1;
        self.inv.slots_mut()[slot] = (0, 0);
        Some(prev)
    }

    /// Whether a slot in the window holds `id`.
    #[must_use]
    pub fn contains_key(&self, id: &u8) -> bool {
        self.inv.find_slot(*id).is_some()
    }

    /// Occupied slots in the window.
    #[must_use]
    pub fn len(&self) -> usize {
        self.window_view().iter().filter(|(id, _)| *id != 0).count()
    }

    /// Whether the window holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Zero every physical slot - the pre-zero both new-game seed callers run
    /// over the whole range before `FUN_80034A6C` writes slot 0.
    pub fn clear(&mut self) {
        for s in self.inv.slots_mut().iter_mut() {
            *s = (0, 0);
        }
    }

    /// Occupied `(id, count)` pairs in **slot order**, inside the window.
    ///
    /// Slot order is the order retail's menu pages and sell list walk, so a
    /// caller that renders this list gets retail's ordering for free; the map
    /// this replaced had no order at all and its consumers sorted to cope.
    pub fn iter(&self) -> impl Iterator<Item = (&u8, &u8)> {
        self.window_view()
            .iter()
            .filter(|(id, _)| *id != 0)
            .map(|(id, count)| (id, count))
    }

    /// Counts of the occupied slots in the window, slot order.
    pub fn values(&self) -> impl Iterator<Item = &u8> {
        self.iter().map(|(_, count)| count)
    }

    /// Ids of the occupied slots in the window, slot order.
    pub fn keys(&self) -> impl Iterator<Item = &u8> {
        self.iter().map(|(id, _)| id)
    }

    /// `HashMap::entry`'s shape: the slot for `id`, claimed if free.
    pub fn entry(&mut self, id: u8) -> BagEntry<'_> {
        BagEntry { bag: self, id }
    }

    fn window_view(&self) -> &[(u8, u8)] {
        self.inv.window_slots_view()
    }
}

impl<'a> IntoIterator for &'a ItemBag {
    type Item = (&'a u8, &'a u8);
    type IntoIter = Box<dyn Iterator<Item = (&'a u8, &'a u8)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

impl FromIterator<(u8, u8)> for ItemBag {
    fn from_iter<T: IntoIterator<Item = (u8, u8)>>(iter: T) -> Self {
        let mut bag = ItemBag::new();
        for (id, count) in iter {
            bag.insert(id, count);
        }
        bag
    }
}

/// The handle [`ItemBag::entry`] returns - the `or_insert` half of
/// `HashMap`'s entry API, which is the only half the engine's call sites use.
pub struct BagEntry<'a> {
    bag: &'a mut ItemBag,
    id: u8,
}

impl<'a> BagEntry<'a> {
    /// The count for this id, claiming a free slot seeded with `default` when
    /// the window holds none.
    ///
    /// When the window is full the claim fails and this hands back a count
    /// that belongs to no slot, so a caller's `+= 1` is dropped instead of
    /// corrupting a neighbour - retail's own full-bag arm writes one byte past
    /// the window, which this model surfaces rather than performs
    /// (`AddOutcome::OobIdWrite`).
    pub fn or_insert(self, default: u8) -> &'a mut u8 {
        if self.bag.inv.find_slot(self.id).is_none() {
            self.bag.insert(self.id, default);
        }
        match self.bag.inv.find_slot(self.id) {
            Some(slot) => &mut self.bag.inv.slots_mut()[slot].1,
            None => {
                self.bag.overflow = default;
                &mut self.bag.overflow
            }
        }
    }

    /// `or_insert(0)` - the shape most call sites want.
    pub fn or_default(self) -> &'a mut u8 {
        self.or_insert(0)
    }
}
