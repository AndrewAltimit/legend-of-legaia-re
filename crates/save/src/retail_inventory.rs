//! Faithful, memory-safe model of the retail consumable-item inventory window.
//!
//! NOT WIRED: nothing outside this file constructs a [`RetailInventory`], and
//! that is deliberate - the engine's gameplay inventory is
//! `legaia_engine_core`'s typed item list, which is what the grant / consume /
//! shop kernels operate on. This module exists to answer questions *about*
//! retail's array (what the accessor family does to which slot, in what order,
//! and where the out-of-bounds add primitive lands), and it answers them by
//! being read and unit-tested, not by being on the frame path. Wiring it would
//! mean replacing the engine's inventory representation with the fixed
//! `(id, count)` window, which would trade a safe growable list for a
//! bug-compatible one; the ACE analysis below is the reason to keep the model,
//! not a reason to run it. Every `// PORT:` tag in this file is covered by
//! this note.
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
//! The item array is a fixed run of 2-byte `(id, count)` slots at
//! [`ITEM_WINDOW_BASE`] (`0x80085958` = SC+0x1818). Every accessor works
//! inside an **active window** `[gp[+0x2D2], gp[+0x2D4])` installed by
//! [`ItemWindow`] (`FUN_8004313C`, the sole `SCUS_942.54` writer of both
//! halfwords) - the window is context state, not a constant. Stacks cap at
//! [`STACK_CAP`] (99).
//!
//! ## Accessor family (`SCUS_942.54`)
//!
//! - `FUN_8004313C` - window selector: installs `gp[+0x2D2]` / `gp[+0x2D4]` /
//!   `gp[+0x2D6]` from the party-member count at SC+0x454 and story flag 20.
//!   See [`ItemWindow`].
//! - `FUN_80042EE0` - find-slot-by-id: linear scan `[start, end)`, returns the
//!   slot index or none. Bounded.
//! - `FUN_80042F4C` - find-count-by-id: find-slot then return the count byte
//!   (0 if absent). Bounded.
//! - `FUN_80042310` - consume-by-id: find-slot; `count = max(count - qty, 0)`;
//!   when it reaches 0 zero the **id byte in place**. It does **not** compact -
//!   the freed slot is left as a hole, exactly like consume-by-slot. Bounded.
//! - `FUN_80043048` - consume-by-slot: the same decrement addressed by
//!   **index** (not id). Bounded; no-ops (echoing its third argument) when the
//!   slot index is out of range or the slot is already empty.
//! - `FUN_800423E0` - **normalize**, not a plain compact: for each occupied
//!   slot it first merges every *later* stack of the same id into it (capped at
//!   99, zeroing the donor), then pulls occupied slots down into holes.
//!   Occupancy is keyed on `id != 0` **alone**, so a zero-count slot with a
//!   live id survives. Calls `FUN_8004313C` first, so it normalizes whichever
//!   window that installs.
//! - `FUN_800421D4` - ADD (the OOB primitive). MERGE pass first (existing id ->
//!   `count = min(count + qty, 99)`), then a FREE-SLOT pass (first `id == 0`).
//!
//! ## The out-of-bounds add primitive
//!
//! In `FUN_800421D4` the id store `sb t0,0x1818(a0)` at `0x800422BC` writes the
//! item id to `slot[i]` **before** the `slt` bound check that guards only the
//! *count* store. The free-slot scan runs once and exits either at the first
//! `id == 0` slot (the ordinary case - the store is in-window and the count
//! store follows) or, on a FULL window, at `i == end`. Only the second exit is
//! the primitive: it writes the id byte **one slot past the window** at
//! `ITEM_WINDOW_BASE + end * 2`, and the bound check then fails so the count is
//! never written.
//!
//! `end` is [`ItemWindow`]'s output, so the landing address is `0x80085A58`
//! (`end = 128`) or `0x80085B58` (`end = 256`) - **not** the `0x800859E8` an
//! earlier reading recorded. That address is slot 72, and the probe hits at
//! `pc = 0x800422BC` that produced it are the *ordinary* id store for a bag
//! whose first free slot happened to be 72: two different ids landing at slots
//! 72 then 73 is what a normal pair of adds looks like, not a repeated OOB.
//!
//! This model surfaces that primitive as the [`AddOutcome::OobIdWrite`] data
//! variant (carrying the would-be target address **and** the would-be written
//! id byte) and performs **no** write, leaving the modelled inventory
//! unchanged.
//!
//! ## Reachability: real-but-unreachable via the add path
//!
//! The primitive only fires if some path can reach `FUN_800421D4` with a window
//! *genuinely filled to `gp[+0x2D4]`* - i.e. the free-slot scan
//! (`0x80042254..0x8004229C`) must find **no** `id == 0` slot in `[start, end)`
//! and the merge scan (`0x800421FC..0x80042238`) must find **no** existing stack
//! of the incoming id, so `a2 == end` at the store. None of the add call sites
//! pre-check inventory room - they load an item id and `jal 0x800421D4`
//! directly (shop buy-confirm at `0x801C38A4` loads `a0 = rec+8`; battle-loot at
//! `0x8004F380` / `0x8004F608`) - so the only thing that can stop the OOB is the
//! scan itself running out of holes. It cannot, under normal play, for either
//! window class:
//!
//! - **Full window `[0, 256)`** (installed by [`ItemWindow`] for any party of
//!   `>= 2` members - the normal mid/late-game state, live-verified at 3
//!   members -> `(0, 256)`): the merge pass keys on the item id byte
//!   (`andi a3,t0,0xff` at `0x800421F4`, match at `0x80042214`), so a given
//!   non-zero id occupies **at most one** slot; the id byte is a `u8` and `0` is
//!   the empty sentinel (free-slot break at `0x80042284`), so under the
//!   add/consume/normalize accessors (none of which ever creates a duplicate
//!   *live* id) the window holds at most [`MAX_DISTINCT_ITEM_IDS`] `= 255`
//!   occupied slots. `255 < 256`, so a hole always remains -> `a2 < end` -> the
//!   store lands in-window and the guarded count store at `0x80042300` runs. The
//!   `a2 == end` exit is **mathematically unreachable** here.
//! - **Half windows `[0, 128)` / `[128, 256)`**: installed only in the single
//!   playable-member + story-flag-[`FULL_WINDOW_STORY_FLAG`]-clear state
//!   (`FUN_8004313C` `0x80043150..0x80043170`). 128 slots is `<= 255`, so the id
//!   ceiling alone does **not** forbid a fill - but that selector state is a
//!   transient early/solo phase, and the real disc item population is far below
//!   128 (the item-name table's live entries; the curated corpus is ~70), so the
//!   free-slot scan still terminates on a hole. No normal-play path presents a
//!   genuinely 128-full half-window.
//!
//! **Verdict: the OOB id store at `0x800422BC` is a real primitive but is
//! unreachable through the retail add call sites in normal play** - the add path
//! caps occupancy below `end`, so `i == end` never occurs. The written byte
//! would still be attacker-influenced (shop catalog id, drop id, captured-monster
//! id) *if* a non-add path (debug menu, cheat, or a crafted save that seeds
//! duplicate live ids, or a full 256-distinct-id window) forced the full-bag
//! exit; that is outside "normal play". The call sites are catalogued in
//! [`AddHelperCaller`] and the machine-checkable half of the verdict lives in
//! [`ItemWindow::oob_reachability`] / [`OobReachability`].
//!
//! See [`docs/reference/memory-map.md`](../../../docs/reference/memory-map.md).

// PORT: FUN_800421D4 (ADD) / FUN_80042EE0 (find-slot) / FUN_80042F4C (find-count)
// PORT: FUN_80042310 (consume) / FUN_800423E0 (normalize: merge + squeeze)
// PORT: FUN_8004313C (active-window selector)
// PORT: FUN_80043048 (consume-by-slot)
// REF: docs/reference/memory-map.md "Retail inventory accessors (SCUS_942.54)"

/// Base address of the consumable-item window (`= SC+0x1818`).
pub const ITEM_WINDOW_BASE: u32 = 0x8008_5958;

/// Slot span of the general-item **page** the `Have 99 Items` cheat writes
/// (`0x80085958..0x800859E8`).
///
/// This is a UI / cheat-coverage span, **not** the accessor window: retail's
/// `gp[+0x2D4]` is only ever 128 or 256 (see [`ItemWindow`]). Kept as the
/// convenience default for [`RetailInventory::new`] callers that just want a
/// small modelled bag; anything reasoning about retail bounds wants
/// [`ItemWindow::bounds`].
pub const GENERAL_ITEM_PAGE_SLOTS: usize = 72;

/// Deprecated alias for [`GENERAL_ITEM_PAGE_SLOTS`]. The old name claimed to
/// be `gp[+0x2D4]`, which it is not.
pub const ITEM_WINDOW_SLOTS: usize = GENERAL_ITEM_PAGE_SLOTS;

/// Total addressable item slots (`FUN_8004313C`'s widest window).
pub const ITEM_SLOTS_TOTAL: usize = 256;

/// Slots in one half of the item array - the boundary `FUN_8004313C` splits
/// on when the full window isn't installed.
pub const ITEM_SLOTS_HALF: usize = 128;

/// Story-flag index (`DAT_80085758` bit, via `FUN_8003CE64`) that unlocks the
/// full 256-slot item window; while it is clear the game addresses one
/// 128-slot half.
pub const FULL_WINDOW_STORY_FLAG: u32 = 20;

/// Per-stack count cap enforced by the retail add/merge paths.
pub const STACK_CAP: u8 = 99;

/// The maximum number of *distinct* occupiable item ids, and hence the maximum
/// number of live-occupied slots the add/consume/normalize accessors can ever
/// produce in a window.
///
/// The item id byte is a `u8` stored/loaded by `FUN_800421D4`
/// (`sb t0,0x1818(a0)` / `lbu ...`) and `0` is the empty-slot sentinel (the
/// free-slot pass breaks on `id == 0` at `0x80042284`). The add helper's merge
/// pass (`0x800421FC..0x80042238`) collapses a repeat id into its existing
/// stack, so a given non-zero id never occupies two slots; `FUN_800423E0`
/// (normalize) likewise merges duplicates, and `FUN_80042310` /
/// `FUN_80043048` (consume) only ever zero ids. So the count of live-occupied
/// slots equals the count of distinct non-zero ids held, which is at most
/// `255`. This is the ceiling that makes the full `[0, 256)` window's OOB exit
/// unreachable.
pub const MAX_DISTINCT_ITEM_IDS: u16 = 255;

/// The active accessor window `[gp[+0x2D2], gp[+0x2D4])`, as installed by
/// `FUN_8004313C` - the only `SCUS_942.54` writer of either halfword (11
/// callers, all of them the entry hop of an inventory operation; `FUN_800423E0`
/// calls it before normalizing).
///
/// The selector branches on the party-member count at `SC+0x454`
/// (`0x80084594`):
///
/// | members | window |
/// |---|---|
/// | `0` | unchanged - the previous window stays installed |
/// | `1` | story flag [`FULL_WINDOW_STORY_FLAG`] set gives [`Full`](Self::Full); clear falls to the half picked by `SC+0x458` (`0x80084598`) |
/// | `>= 2` | [`Full`](Self::Full), with no flag test at all |
///
/// The half for the solo-member case is [`High`](Self::High) when
/// `0x80084598` is nonzero, else [`Low`](Self::Low). The window *length* also
/// lands in `gp[+0x2D6]`.
///
/// Live cross-check: a mid-game battle state with a three-member party reads
/// `3` at `0x80084594` and `(start, end, len) = (0, 256, 256)` at `gp+0x2D2` -
/// the `>= 2` row - with 160 contiguous occupied slots, which a 72-slot model
/// would truncate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemWindow {
    /// `[0, 256)` - the whole array.
    Full,
    /// `[0, 128)` - the low half.
    Low,
    /// `[128, 256)` - the high half.
    High,
}

impl ItemWindow {
    /// `(start, end)` slot indices, i.e. `(gp[+0x2D2], gp[+0x2D4])`.
    #[must_use]
    pub fn bounds(self) -> (usize, usize) {
        match self {
            ItemWindow::Full => (0, ITEM_SLOTS_TOTAL),
            ItemWindow::Low => (0, ITEM_SLOTS_HALF),
            ItemWindow::High => (ITEM_SLOTS_HALF, ITEM_SLOTS_TOTAL),
        }
    }

    /// Window length (`gp[+0x2D6]`).
    #[must_use]
    pub fn len(self) -> usize {
        let (s, e) = self.bounds();
        e - s
    }

    /// Never empty - present only to satisfy the `len`-without-`is_empty` lint.
    #[must_use]
    pub fn is_empty(self) -> bool {
        false
    }

    /// Whether the full-bag OOB id store (`FUN_800421D4` at `0x800422BC`) can be
    /// reached through the retail *add* call sites while this window is
    /// installed - the machine-checkable half of the reachability verdict in the
    /// module docs.
    ///
    /// Reaching the OOB requires filling every slot of this window with a
    /// distinct live id (so the free-slot scan finds no `id == 0` hole). Since
    /// the accessors keep at most [`MAX_DISTINCT_ITEM_IDS`] distinct ids alive,
    /// a window wider than that ceiling can never be filled by the add path:
    ///
    /// - [`Full`](Self::Full) (`len == 256 > 255`): [`Unreachable`] - a hole
    ///   always remains.
    /// - [`Low`](Self::Low) / [`High`](Self::High) (`len == 128 <= 255`): not
    ///   forbidden by the ceiling, but the half window is only installed in the
    ///   transient single-member / story-flag-clear state and the real disc item
    ///   population is far below 128, so no normal-play path fills it -
    ///   [`GatedBySelectorState`].
    ///
    /// [`Unreachable`]: OobReachability::Unreachable
    /// [`GatedBySelectorState`]: OobReachability::GatedBySelectorState
    #[must_use]
    pub fn oob_reachability(self) -> OobReachability {
        if (self.len() as u16) > MAX_DISTINCT_ITEM_IDS {
            OobReachability::Unreachable
        } else {
            OobReachability::GatedBySelectorState
        }
    }

    /// Reproduce `FUN_8004313C`'s selection. `members` is the party-member
    /// count byte at `SC+0x454`, `full_window_flag` is story flag
    /// [`FULL_WINDOW_STORY_FLAG`], and `high_half` is the byte at `SC+0x458`
    /// being nonzero.
    ///
    /// `None` is the `members == 0` early return: retail leaves whatever
    /// window was already installed.
    #[must_use]
    pub fn select(members: u8, full_window_flag: bool, high_half: bool) -> Option<Self> {
        match members {
            0 => None,
            1 if !full_window_flag => Some(if high_half {
                ItemWindow::High
            } else {
                ItemWindow::Low
            }),
            _ => Some(ItemWindow::Full),
        }
    }
}

/// The reachability verdict for the full-bag OOB id store, per [`ItemWindow`].
///
/// See [`ItemWindow::oob_reachability`] and the module-level "Reachability"
/// section. Neither variant says the primitive is *fake* - the unconditional
/// store before the guard is confirmed from disassembly (`0x800422BC` vs the
/// `slt`/`beq` at `0x800422C8`/`0x800422CC`). They classify whether the retail
/// *add path* can ever present the required full window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OobReachability {
    /// The window is wider than [`MAX_DISTINCT_ITEM_IDS`], so the free-slot scan
    /// always finds a hole: the OOB exit is mathematically unreachable through
    /// the add path (the `[0, 256)` full window).
    Unreachable,
    /// The window is narrow enough to be filled in principle, but retail only
    /// installs it in a transient game state whose reachable inventory is far
    /// smaller than the window, so no normal-play path fills it (the `[0, 128)`
    /// / `[128, 256)` half windows).
    GatedBySelectorState,
}

/// The address one slot past a `(base, window)` window - the byte the retail
/// add primitive (`FUN_800421D4`) writes the id to when the free-slot scan
/// exhausts the window.
///
/// For retail's own windows this is `ITEM_WINDOW_BASE + end * 2`, i.e.
/// `0x80085A58` ([`ItemWindow::Low`]) or `0x80085B58` ([`ItemWindow::Full`] /
/// [`ItemWindow::High`]).
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
        /// Value the retail id store would have written: the added item's id.
        /// This is the attacker-influenced byte of the primitive (the `V` in
        /// "write value `V` to address `A`"); `qty` never reaches the OOB
        /// store because the count write is the guarded one.
        written_id: u8,
    },
}

/// The reverse-engineered call sites that invoke the unchecked add helper
/// `FUN_800421D4` without pre-checking inventory room. Each loads an item id and
/// `jal`s the helper directly, so the *only* backstop against the full-bag OOB
/// id store ([`AddOutcome::OobIdWrite`]) is the helper's own free-slot scan.
/// Per [`ItemWindow::oob_reachability`] that scan cannot exhaust the `[0, 256)`
/// full window (the [`MAX_DISTINCT_ITEM_IDS`] ceiling) and does not exhaust the
/// half windows in normal play, so these sites reach the helper but not the OOB
/// exit - the primitive is real-but-unreachable via this path. Each carries its
/// source function address for provenance.
///
/// See `docs/reference/functions.md` (`800421D4` caller list) and the per-site
/// entries (`8004E568`, `801C36B0`, `801F138C`, `801D0F60`, `8020E748`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddHelperCaller {
    /// Battle-end reward resolution (`FUN_8004E568`, add at `0x8004F380` /
    /// `0x8004F608`): awards the formation's drop items. Written id = drop id.
    BattleLoot,
    /// Shop / exchange buy-confirm (`FUN_801C36B0`, overlay 0971): a priced,
    /// **variable-quantity** give of the selected catalog record's item.
    /// Written id = the catalog record's item id (`rec+8`).
    ShopBuyConfirm,
    /// Captured-monster item pay (`FUN_801F138C`, overlay 0897): on a resolved
    /// capture, pays `actor[+0x1DF]` into the bag. Written id = captured id.
    CaptureItemPay,
    /// One-shot minigame completion reward (`FUN_801D0F60`, overlay 0977 at slot-A base `0x801CE818`; formerly mis-cited as `FUN_801C2748` off a `0x801C0000`-band import):
    /// awards a single fixed item `0xCD`. Written id = `0xCD` (fixed).
    MinigameReward,
    /// Equip swap-back refund (`FUN_8020E748` / `FUN_801E01F0`): refunds the
    /// displaced old equip/consumable id when swapping. Written id = old id.
    EquipSwapBackRefund,
}

impl AddHelperCaller {
    /// All known unchecked call sites, in `docs/reference/functions.md` order.
    pub const ALL: [AddHelperCaller; 5] = [
        AddHelperCaller::BattleLoot,
        AddHelperCaller::ShopBuyConfirm,
        AddHelperCaller::CaptureItemPay,
        AddHelperCaller::MinigameReward,
        AddHelperCaller::EquipSwapBackRefund,
    ];

    /// The `FUN_<addr>` entry point of the calling function (for the variable
    /// callers this is the function that performs the add).
    #[must_use]
    pub fn source_addr(self) -> u32 {
        match self {
            AddHelperCaller::BattleLoot => 0x8004_E568,
            AddHelperCaller::ShopBuyConfirm => 0x801C_36B0,
            AddHelperCaller::CaptureItemPay => 0x801F_138C,
            AddHelperCaller::MinigameReward => 0x801C_2748,
            AddHelperCaller::EquipSwapBackRefund => 0x8020_E748,
        }
    }
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
    /// decremented.
    ///
    /// Faithful to `FUN_80042310`: `count = max(count - qty, 0)`, and when the
    /// count reaches 0 the **id byte is zeroed in place**, leaving a hole. The
    /// function does **not** compact - `FUN_800423E0` is a separate entry
    /// point with no call from here, so any squeezing is the caller's choice.
    /// (An earlier reading of this port had `consume` compact inline, which
    /// silently renumbered every following slot.)
    // PORT: FUN_80042310
    pub fn consume(&mut self, id: u8, qty: u8) -> bool {
        let Some(i) = self.find_slot(id) else {
            return false;
        };
        let count = self.slots[i].1;
        let new_count = count.saturating_sub(qty);
        self.slots[i].1 = new_count;
        if new_count == 0 {
            self.slots[i].0 = 0;
        }
        true
    }

    /// [`consume`](Self::consume) followed by [`normalize`](Self::normalize) -
    /// the shape retail produces when an inventory operation is followed by
    /// its own `FUN_800423E0` hop. Convenience for callers that want a
    /// hole-free window.
    pub fn consume_and_normalize(&mut self, id: u8, qty: u8) -> bool {
        let hit = self.consume(id, qty);
        if hit {
            self.normalize();
        }
        hit
    }

    /// Consume `amount` from the slot at `slot` (a window **index**, not an
    /// item id) and return the slot's remaining count. Faithful to retail
    /// `FUN_80043048`: bounds-checks `slot < window`, acts only on an occupied
    /// slot (`id != 0`), clamps the new count at 0, and zeroes the id byte
    /// **in place** when the count reaches 0 - it does **not** compact the
    /// window (the freed slot stays as a hole, distinguishing it from the
    /// id-keyed [`consume`](Self::consume), which compacts). On the no-op paths
    /// (slot out of range, or already empty) retail echoes back its third
    /// argument unchanged; `echo` models that register.
    // PORT: FUN_80043048
    pub fn consume_slot(&mut self, slot: i16, amount: u8, echo: u8) -> u8 {
        if (slot as i32) >= self.slots.len() as i32 || slot < 0 {
            return echo;
        }
        let i = slot as usize;
        if self.slots[i].0 == 0 {
            return echo;
        }
        // count - amount, clamped at 0 (retail: `< 1 -> 0`).
        let remaining = self.slots[i].1.saturating_sub(amount);
        self.slots[i].1 = remaining;
        if remaining == 0 {
            // Zero the id byte in place - NO compaction (the hole remains).
            self.slots[i].0 = 0;
        }
        remaining
    }

    /// Normalize the window the way `FUN_800423E0` does: **merge duplicate
    /// stacks, then squeeze holes**.
    ///
    /// Retail walks the window with a write cursor. At each occupied slot it
    /// first scans the rest of the window for another stack of the same id and
    /// folds it in - `count += donor_count`, clamped to [`STACK_CAP`], donor
    /// slot fully zeroed - repeating until no duplicate remains. Only then
    /// does it advance, pulling the next occupied slot down into the hole it is
    /// sitting on.
    ///
    /// Two details the old "compact" model got wrong, both now reproduced:
    /// occupancy is keyed on `id != 0` **alone** (a live id with a zero count
    /// survives rather than being dropped), and duplicate ids are merged rather
    /// than left as two stacks.
    // PORT: FUN_800423E0
    pub fn normalize(&mut self) {
        let window = self.slots.len();
        let mut survivors: Vec<(u8, u8)> = Vec::with_capacity(window);
        for &(id, count) in &self.slots {
            if id == 0 {
                continue;
            }
            match survivors.iter_mut().find(|(sid, _)| *sid == id) {
                Some(slot) => {
                    slot.1 = slot.1.saturating_add(count).min(STACK_CAP);
                }
                None => survivors.push((id, count)),
            }
        }
        survivors.resize(window, (0, 0));
        self.slots = survivors;
    }

    /// Deprecated name for [`normalize`](Self::normalize). Retail's
    /// `FUN_800423E0` is not a plain compaction - see that method.
    pub fn compact(&mut self) {
        self.normalize();
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
            written_id: id,
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
        // Consuming slot 1 to zero leaves a HOLE (retail FUN_80042310 zeroes
        // the id in place and does not compact), so the next add reuses it.
        assert!(inv.consume(0x11, 2));
        assert_eq!(inv.slots()[1], (0, 0));
        assert_eq!(inv.slots()[2], (0x12, 3), "slot 2 must not renumber");
        assert_eq!(inv.add(0x13, 4), AddOutcome::Placed { slot: 1 });
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
                oob_target: 0x8008_59E8,
                written_id: 0xFE,
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
    fn item_window_selector_matches_fun_8004313c() {
        // 0 members: retail returns before touching gp[+0x2D2]/[+0x2D4].
        assert_eq!(ItemWindow::select(0, true, false), None);
        // A solo party gates on story flag 20; when clear, SC+0x458 picks the
        // half.
        assert_eq!(ItemWindow::select(1, true, true), Some(ItemWindow::Full));
        assert_eq!(ItemWindow::select(1, false, false), Some(ItemWindow::Low));
        assert_eq!(ItemWindow::select(1, false, true), Some(ItemWindow::High));
        // Two or more members skips the flag test entirely - the live
        // battle-state read (3 members, window (0, 256, 256)).
        assert_eq!(ItemWindow::select(2, false, true), Some(ItemWindow::Full));
        assert_eq!(ItemWindow::select(3, false, true), Some(ItemWindow::Full));

        assert_eq!(ItemWindow::Full.bounds(), (0, 256));
        assert_eq!(ItemWindow::Low.bounds(), (0, 128));
        assert_eq!(ItemWindow::High.bounds(), (128, 256));
        assert_eq!(ItemWindow::High.len(), 128);

        // The full-window OOB lands past slot 255, not at the old slot-72
        // address the earlier reading recorded.
        assert_eq!(
            oob_target(ITEM_WINDOW_BASE, ItemWindow::Full.len()),
            0x8008_5B58
        );
        assert_eq!(
            oob_target(ITEM_WINDOW_BASE, ItemWindow::Low.len()),
            0x8008_5A58
        );
    }

    #[test]
    fn normalize_merges_duplicate_stacks_and_squeezes() {
        // Two stacks of the same id plus a hole between them.
        let mut inv = RetailInventory::from_slots(
            ITEM_WINDOW_BASE,
            vec![(0x30, 40), (0, 0), (0x31, 2), (0x30, 5), (0, 0)],
        );
        inv.normalize();
        // The later 0x30 stack folds into the earlier one; survivors squeeze.
        assert_eq!(inv.slots()[0], (0x30, 45));
        assert_eq!(inv.slots()[1], (0x31, 2));
        assert_eq!(inv.slots()[2], (0, 0));
        assert_eq!(inv.slots()[3], (0, 0));
    }

    #[test]
    fn normalize_caps_merged_stacks_and_keeps_live_zero_counts() {
        let mut inv = RetailInventory::from_slots(
            ITEM_WINDOW_BASE,
            vec![(0x40, 90), (0x40, 30), (0x41, 0), (0, 0)],
        );
        inv.normalize();
        assert_eq!(inv.slots()[0], (0x40, STACK_CAP), "merge clamps at 99");
        // Retail's occupancy test is `id != 0` alone: a live id with a zero
        // count survives normalization.
        assert_eq!(inv.slots()[1], (0x41, 0));
        assert_eq!(inv.slots()[2], (0, 0));
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
    fn consume_to_zero_leaves_a_hole_in_place() {
        let mut inv = default_inv();
        inv.add(0xA0, 1);
        inv.add(0xA1, 4);
        inv.add(0xA2, 2);
        // Consume the middle stack to zero. Retail (FUN_80042310) zeroes the
        // id byte in place and returns - the survivors keep their slot indices.
        assert!(inv.consume(0xA1, 4));
        assert_eq!(inv.slots()[0], (0xA0, 1));
        assert_eq!(inv.slots()[1], (0, 0));
        assert_eq!(inv.slots()[2], (0xA2, 2));
        assert_eq!(inv.find_slot(0xA1), None);
        // The squeeze is a separate entry point (FUN_800423E0).
        let mut squeezed = inv.clone();
        squeezed.normalize();
        assert_eq!(squeezed.slots()[1], (0xA2, 2));
        // Partial consume leaves the stack in place.
        assert!(inv.consume(0xA2, 1));
        assert_eq!(inv.find_count(0xA2), 1);
        // Consuming an absent id is a no-op false.
        assert!(!inv.consume(0xFF, 1));
    }

    #[test]
    fn consume_by_slot_zeroes_in_place_without_compacting() {
        let mut inv = default_inv();
        inv.add(0xB0, 3);
        inv.add(0xB1, 5);
        inv.add(0xB2, 2);
        // Partial slot-consume leaves the stack and order intact.
        assert_eq!(inv.consume_slot(1, 2, 0), 3);
        assert_eq!(inv.slots()[1], (0xB1, 3));
        // Consume slot 1 to zero: the id byte is zeroed IN PLACE - unlike
        // consume-by-id, the window is NOT compacted, so the hole remains and
        // the trailing slot keeps its index.
        assert_eq!(inv.consume_slot(1, 3, 0), 0);
        assert_eq!(inv.slots()[0], (0xB0, 3));
        assert_eq!(inv.slots()[1], (0, 0));
        assert_eq!(inv.slots()[2], (0xB2, 2));
        // Over-consume clamps the count at 0 (and zeroes the id).
        assert_eq!(inv.consume_slot(0, 99, 0), 0);
        assert_eq!(inv.slots()[0], (0, 0));
    }

    #[test]
    fn consume_by_slot_noop_paths_echo_third_arg() {
        let mut inv = default_inv();
        inv.add(0xC0, 4);
        // Out-of-range slot index: no mutation, echoes the third argument.
        assert_eq!(inv.consume_slot(ITEM_WINDOW_SLOTS as i16, 1, 0x7A), 0x7A);
        assert_eq!(inv.consume_slot(-1, 1, 0x55), 0x55);
        // Already-empty slot (id == 0): same echo, no mutation.
        assert_eq!(inv.consume_slot(5, 1, 0x33), 0x33);
        assert_eq!(inv.find_count(0xC0), 4);
    }

    /// A full window with all-distinct ids: the next add can never merge or
    /// place, so it always surfaces the OOB primitive.
    fn full_distinct_bag() -> RetailInventory {
        // ids 1..=72 - all non-zero (no empty slot) and none equal to the test
        // ids below (0x80+), so neither the merge nor free-slot pass succeeds.
        let slots: Vec<(u8, u8)> = (0..ITEM_WINDOW_SLOTS)
            .map(|i| ((i as u8).wrapping_add(1), 5))
            .collect();
        RetailInventory::from_slots(ITEM_WINDOW_BASE, slots)
    }

    #[test]
    fn oob_write_carries_the_added_id_as_value() {
        // The written byte is the *added item's id*, not qty - this is the
        // attacker-influenced value of the primitive.
        for id in [0x80u8, 0xCD, 0xFE, 0xFF] {
            let mut inv = full_distinct_bag();
            assert_eq!(
                inv.add(id, 1),
                AddOutcome::OobIdWrite {
                    oob_target: 0x8008_59E8,
                    written_id: id,
                }
            );
        }
    }

    #[test]
    fn oob_write_is_independent_of_quantity() {
        // The shop buy-confirm path adds a variable quantity, but only the id
        // store is unguarded; qty never reaches the OOB store. So the outcome
        // is identical for qty = 1 and qty = 99.
        let mut inv1 = full_distinct_bag();
        let mut inv99 = full_distinct_bag();
        assert_eq!(
            inv1.add(0x90, 1),
            AddOutcome::OobIdWrite {
                oob_target: 0x8008_59E8,
                written_id: 0x90,
            }
        );
        assert_eq!(inv1.add(0x90, 1), inv99.add(0x90, 99));
    }

    #[test]
    fn add_helper_caller_catalogue_is_complete_and_distinct() {
        // The five reverse-engineered unchecked call sites, each with its
        // documented source address. Guards against silent drift if a site is
        // added/removed without updating ALL.
        assert_eq!(AddHelperCaller::ALL.len(), 5);
        let addrs: Vec<u32> = AddHelperCaller::ALL
            .iter()
            .map(|c| c.source_addr())
            .collect();
        assert_eq!(
            addrs,
            vec![
                0x8004_E568,
                0x801C_36B0,
                0x801F_138C,
                0x801C_2748,
                0x8020_E748
            ]
        );
        // All distinct.
        let mut sorted = addrs.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), addrs.len());
    }

    #[test]
    fn every_known_caller_reaches_the_oob_on_a_full_bag() {
        // Model each caller's full-bag add with a representative written id and
        // assert the primitive fires at the fixed key-item address. The value
        // column reflects each site's control over the written byte (per the
        // `AddHelperCaller` docs): shop = catalog id, loot = drop id, capture =
        // monster id, minigame = fixed 0xCD, refund = displaced equip id.
        // Representative ids chosen > ITEM_WINDOW_SLOTS (72) so they don't
        // collide with full_distinct_bag's 1..=72 fill (which would merge).
        let site_ids = [
            (AddHelperCaller::BattleLoot, 0x80u8),
            (AddHelperCaller::ShopBuyConfirm, 0x91),
            (AddHelperCaller::CaptureItemPay, 0xA5),
            (AddHelperCaller::MinigameReward, 0xCD),
            (AddHelperCaller::EquipSwapBackRefund, 0xB0),
        ];
        for (caller, id) in site_ids {
            let mut inv = full_distinct_bag();
            assert_eq!(
                inv.add(id, 1),
                AddOutcome::OobIdWrite {
                    oob_target: 0x8008_59E8,
                    written_id: id,
                },
                "caller {caller:?} (FUN_{:08X}) should reach the OOB on a full bag",
                caller.source_addr(),
            );
        }
    }

    #[test]
    fn full_window_oob_is_unreachable_by_id_ceiling() {
        // The full [0, 256) window has 256 slots but only 255 distinct non-zero
        // ids exist (id 0 is the empty sentinel), and FUN_800421D4's merge pass
        // keeps at most one slot per id - so a hole always remains and the OOB
        // exit (a2 == end at 0x800422BC) can never be taken by the add path.
        assert_eq!(ItemWindow::Full.len(), 256);
        assert!(ItemWindow::Full.len() as u16 > MAX_DISTINCT_ITEM_IDS);
        assert_eq!(
            ItemWindow::Full.oob_reachability(),
            OobReachability::Unreachable,
        );
    }

    #[test]
    fn half_windows_are_gated_by_selector_state_not_the_id_ceiling() {
        // 128 <= 255, so the id ceiling alone does not forbid a fill; retail
        // only installs these in the single-member / flag-20-clear state, whose
        // reachable inventory is far below 128.
        for w in [ItemWindow::Low, ItemWindow::High] {
            assert_eq!(w.len(), 128);
            assert!(w.len() as u16 <= MAX_DISTINCT_ITEM_IDS);
            assert_eq!(w.oob_reachability(), OobReachability::GatedBySelectorState);
        }
    }

    /// The reachability verdict, exercised as a data assertion: build the
    /// densest full `[0, 256)` window the add path could ever produce - every
    /// one of the 255 distinct non-zero ids present exactly once - and show that
    /// slot 255 is a forced hole, so no add of any real id `1..=255` reaches the
    /// OOB (each either merges or places into the hole). The mechanical
    /// counterpart to `ItemWindow::Full.oob_reachability() == Unreachable`.
    #[test]
    fn full_window_built_from_every_id_still_has_a_hole() {
        // ids 1..=255 in slots 0..=254; slot 255 is empty because there is no
        // 256th distinct non-zero id to occupy it.
        let slots: Vec<(u8, u8)> = (0..ItemWindow::Full.len())
            .map(|i| {
                let id = u8::try_from(i + 1).unwrap_or(0); // i == 255 -> id 0 (hole)
                (id, u8::from(id != 0))
            })
            .collect();
        assert_eq!(slots.len(), 256);
        assert_eq!(
            slots.iter().filter(|&&(id, _)| id == 0).count(),
            1,
            "exactly one forced hole"
        );
        assert_eq!(slots[255], (0, 0), "the hole is the last slot");

        let inv = RetailInventory::from_slots(ITEM_WINDOW_BASE, slots);
        for id in 1u8..=255 {
            let outcome = inv.clone().add(id, 1);
            assert!(
                !matches!(outcome, AddOutcome::OobIdWrite { .. }),
                "id {id:#04x} must never reach the OOB in a 256-window built from real ids: {outcome:?}",
            );
        }
    }
}
