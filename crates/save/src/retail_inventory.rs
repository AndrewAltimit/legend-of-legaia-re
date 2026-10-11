//! Faithful, memory-safe model of the retail consumable-item inventory array.
//!
//! This is **the engine's bag**, not a model beside it.
//! `legaia_engine_core`'s `ItemBag` holds a [`RetailInventory`] of
//! [`ITEM_SLOTS_TOTAL`] slots and exposes a map-shaped adapter over it, so the
//! grant / consume / shop kernels read as they always did while the slot
//! order, the holes and the active window underneath them are retail's. One
//! consumer indexes the bag by slot - PROT 0941's Steal is a rejection sampler
//! over the physical array - and that is what a growable list could not
//! express at all.
//!
//! The earlier reading, that wiring this model into the engine would "trade a
//! safe growable list for a bug-compatible fixed window", had the trade
//! backwards: the window is *behaviour*, not a defect, and the model performs
//! no unsafe write in any case - the out-of-bounds add arm is surfaced as the
//! [`AddOutcome::OobIdWrite`] data variant and stores nothing.
//!
//! It keeps its *preservation* host too: `save-tool items` reads the item
//! window straight out of a real SC block and runs the accessor family over it
//! without writing anything back - which window the selector installs for that
//! party, which slot a consume empties, and whether it leaves a hole - asked
//! against real save data rather than against a fixture. One accessor stays
//! inert even there and says so at its own tag.
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

// Every `// PORT:` tag in this file sits on the item that implements the
// routine - `ItemWindow::select`, `RetailInventory::{find_slot, find_count,
// consume, consume_slot, normalize, add}`. A file-scope duplicate of them was
// removed: a tag with no item under it is resolved against the *module*, and a
// module anchor counts as live as soon as any function in the file is reachable
// by name, which reported this deliberately-inert model as wired.
// REF: docs/reference/memory-map.md "Retail inventory accessors (SCUS_942.54)"

/// Base address of the consumable-item window (`= SC+0x1818`).
pub const ITEM_WINDOW_BASE: u32 = 0x8008_5958;

/// The value the consume-by-id helper returns when the id is not in the
/// active window (`li a2,0x100` at `0x80042374`).
///
/// It is deliberately one past the widest window, so the guard that follows
/// it (`slot < gp[+0x2D4]`, `0x80042384`) fails for every window retail
/// installs and the helper returns without a store.
pub const NOT_IN_WINDOW: u16 = 0x100;

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
    ///   population is 250 named ids, so the id space does not forbid a fill; what
    ///   bounds it is how much of that is obtainable while a character travels
    ///   alone (a progress bound, unmeasured) - so no normal-play path fills it -
    ///   [`GatedBySelectorState`].
    ///
    /// [`Unreachable`]: OobReachability::Unreachable
    /// [`GatedBySelectorState`]: OobReachability::GatedBySelectorState
    ///
    /// REF: FUN_8004313C (the selector whose two window widths this ranks)
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
    ///
    /// PORT: FUN_8004313C (active-window selector - the sole `SCUS_942.54`
    /// writer of `gp[+0x2D2]` / `gp[+0x2D4]`)
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
/// entries (`8004E568`, `801C36B0`, `801DABA4`, `801D0F60`, `8020E748`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddHelperCaller {
    /// Battle-end reward resolution (`FUN_8004E568`, add at `0x8004F380` /
    /// `0x8004F608`): awards the formation's drop items. Written id = drop id.
    BattleLoot,
    /// Shop / exchange buy-confirm (`FUN_801C36B0`, overlay 0971): a priced,
    /// **variable-quantity** give of the selected catalog record's item.
    /// Written id = the catalog record's item id (`rec+8`).
    ShopBuyConfirm,
    /// Dead-slot Item refund in the battle-order rebuild `FUN_801DABA4`
    /// (PROT 0898, `jal 0x800421d4` at `0x801DAC58`): a fallen fighter still
    /// holding a committed Item command (`actor[+0x1DE] == 1`) gets its item
    /// `actor[+0x1DF]` back, quantity `1`, and the category byte is cleared.
    /// Written id = the committed item's id. No capture is involved.
    DeadSlotItemRefund,
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
        AddHelperCaller::DeadSlotItemRefund,
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
            AddHelperCaller::DeadSlotItemRefund => 0x801D_ABA4,
            AddHelperCaller::MinigameReward => 0x801D_0F60,
            AddHelperCaller::EquipSwapBackRefund => 0x8020_E748,
        }
    }
}

/// A faithful, memory-safe model of the retail fixed-window item inventory.
///
/// Each slot is a `(id, count)` pair; an `id == 0` slot is empty (the retail
/// empty sentinel).
///
/// **Storage and window are two different lengths.** `slots.len()` is the
/// physical array - [`ITEM_SLOTS_TOTAL`] for the retail bag - and the
/// accessors below scan only `[start, end)`, the active window
/// `gp[+0x2D2] / gp[+0x2D4]` that [`ItemWindow`] installs. A model built for
/// one window's worth of slots gets the two lengths in one number, which is
/// correct only while the window is the whole array; [`Self::set_window`]
/// is what makes a solo character's half-bag expressible. Constructors
/// default the window to the whole array, so a caller that never sets one
/// behaves exactly as before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetailInventory {
    slots: Vec<(u8, u8)>,
    base: u32,
    /// `(gp[+0x2D2], gp[+0x2D4])` - the active window, in slot indices.
    window: (usize, usize),
}

impl RetailInventory {
    /// Create an empty inventory with `window_slots` `(0, 0)` slots at `base`.
    #[must_use]
    pub fn new(base: u32, window_slots: usize) -> Self {
        Self {
            slots: vec![(0, 0); window_slots],
            base,
            window: (0, window_slots),
        }
    }

    /// Create an inventory from explicit slot contents.
    ///
    /// The window length becomes `slots.len()`.
    #[must_use]
    pub fn from_slots(base: u32, slots: Vec<(u8, u8)>) -> Self {
        let window = (0, slots.len());
        Self {
            slots,
            base,
            window,
        }
    }

    /// Install the active window every accessor below is bounded by - the
    /// three `gp`-relative halfwords `FUN_8004313C` writes
    /// ([`ItemWindow::select`] picks which).
    ///
    /// Clamped to the physical array, so a [`ItemWindow::Full`] window over a
    /// short model window stays in bounds.
    pub fn set_window(&mut self, window: ItemWindow) {
        let (s, e) = window.bounds();
        let len = self.slots.len();
        self.window = (s.min(len), e.min(len));
    }

    /// The installed window as `(start, end)` slot indices.
    #[must_use]
    pub fn window_bounds(&self) -> (usize, usize) {
        self.window
    }

    /// The whole array, mutably - for a host that owns the bag and assigns a
    /// slot outright (the engine's map-shaped adapter does, where retail's own
    /// helpers have no equivalent call).
    pub fn slots_mut(&mut self) -> &mut [(u8, u8)] {
        &mut self.slots
    }

    /// The window's slots - what the pause menu's pages and every accessor
    /// see, as against [`Self::slots`], which is the whole array.
    #[must_use]
    pub fn window_slots_view(&self) -> &[(u8, u8)] {
        let (s, e) = self.window;
        &self.slots[s.min(self.slots.len())..e.min(self.slots.len())]
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
    ///
    /// Keyed on the window's `end`, which is the halfword the add helper's
    /// failed bound check reads (`0x800422C0`), not on the physical array.
    #[must_use]
    pub fn oob_target(&self) -> u32 {
        oob_target(self.base, self.window.1)
    }

    /// Find the slot holding `id`, scanning the active window
    /// `[gp[+0x2D2], gp[+0x2D4])` (`0x80042EE0`/`0x80042EE4` load both bounds
    /// before the scan).
    ///
    /// `id == 0` is the empty sentinel and is never matched. An id held in a
    /// slot **outside** the window is not found - that is the split bag, not a
    /// miss.
    // PORT: FUN_80042EE0
    #[must_use]
    pub fn find_slot(&self, id: u8) -> Option<usize> {
        if id == 0 {
            return None;
        }
        let (s, e) = self.window;
        self.slots
            .get(s..e.min(self.slots.len()))?
            .iter()
            .position(|&(sid, _)| sid == id)
            .map(|i| i + s)
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
        self.consume_returning_slot(id, qty) != NOT_IN_WINDOW
    }

    /// [`consume`](Self::consume) with retail's own return value: the slot the
    /// id was found in, or [`NOT_IN_WINDOW`] (`0x100`).
    ///
    /// The sentinel is the whole reason a caller would want this. The search
    /// is window-bounded (`0x80042310`/`0x80042314` load `gp[+0x2D2]` and
    /// `gp[+0x2D4]`) and the miss arm at `0x80042374` loads `0x100`, which the
    /// guard at `0x80042384` (`slot < gp[+0x2D4]`) then fails - so the
    /// function returns having touched no memory at all. A consumer that
    /// picked its id from **outside** the window therefore removes nothing,
    /// and retail's Steal is exactly such a consumer: its draw is over the
    /// whole 256-slot array while its removal is window-bounded.
    pub fn consume_returning_slot(&mut self, id: u8, qty: u8) -> u16 {
        let Some(i) = self.find_slot(id) else {
            return NOT_IN_WINDOW;
        };
        // Retail re-checks the found slot against `end` before it dereferences
        // (the same guard the sentinel fails), and reads the id byte again: an
        // empty slot returns the index without a store.
        if i >= self.window.1 || self.slots[i].0 == 0 {
            return i as u16;
        }
        let count = self.slots[i].1;
        let new_count = count.saturating_sub(qty);
        self.slots[i].1 = new_count;
        if new_count == 0 {
            self.slots[i].0 = 0;
        }
        i as u16
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
    /// id-keyed [`consume`](Self::consume), which leaves one too). On the
    /// no-op paths (slot out of range, or already empty) retail echoes back its
    /// third argument unchanged; `echo` models that register.
    ///
    /// The bound is the window's `end` alone (`lh v0,0x2d4(gp)` at
    /// `0x8004304C`, with no `start` test): this entry point trusts its
    /// caller's index below the window and only refuses one past it.
    // PORT: FUN_80043048
    pub fn consume_slot(&mut self, slot: i16, amount: u8, echo: u8) -> u8 {
        let end = self.window.1.min(self.slots.len());
        if (slot as i32) >= end as i32 || slot < 0 {
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
    /// The pass runs over the **active window** only (`0x800423F0` /
    /// `0x800423F4` load both bounds first): slots outside it keep their
    /// contents and their order, so normalizing a half-bag never renumbers the
    /// other character's half.
    // PORT: FUN_800423E0
    pub fn normalize(&mut self) {
        let (s, e) = self.window;
        let e = e.min(self.slots.len());
        if s >= e {
            return;
        }
        let window = e - s;
        let mut survivors: Vec<(u8, u8)> = Vec::with_capacity(window);
        for &(id, count) in &self.slots[s..e] {
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
        self.slots[s..e].copy_from_slice(&survivors);
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
        // (2) FREE-SLOT pass: first empty slot (id == 0) **in the window** -
        // `0x80042254` / `0x80042258` reload both bounds for this second scan.
        let (s, e) = self.window;
        let e = e.min(self.slots.len());
        if s < e
            && let Some(i) = self.slots[s..e].iter().position(|&(sid, _)| sid == 0)
        {
            self.slots[s + i] = (id, qty.min(STACK_CAP));
            return AddOutcome::Placed { slot: s + i };
        }
        // (3) FULL bag: retail would store the id one slot past the window
        // before its (failing) bound check. Surface as data; perform no write.
        AddOutcome::OobIdWrite {
            oob_target: self.oob_target(),
            written_id: id,
        }
    }

    /// Raise slot `slot`'s count by `delta`, saturating at [`STACK_CAP`], and
    /// return the new count. `None` is retail's two rejection paths: `slot`
    /// outside the active window, or an empty slot (id byte zero), both of
    /// which return 0 without storing.
    ///
    /// This is the cap in its **primitive** form, addressed by slot rather
    /// than by id: it saturates rather than overflowing or refusing, and it is
    /// the same clamp [`add`](Self::add)'s merge pass applies after finding a
    /// slot itself. The two are separate retail entry points, not one function
    /// reached two ways.
    ///
    /// PORT: FUN_80042FE8
    /// REPLACED-BY: nothing is owed a port - no reference of any form reaches
    /// `FUN_80042FE8` in any image on the disc (not a word, `jal`, `j`, branch
    /// or `lui`/`addiu` pair), so retail never calls it either and no host can
    /// be found by looking harder. `legaia_engine_core`'s bounded list is what
    /// grants items in the port, and it has no slot-addressed add; this stays
    /// because it is where retail writes the 99 cap most plainly, and because
    /// the byte-denominated coverage sweep surfaced it. See
    /// `docs/tooling/port-catalog.md` - What may carry it, fourth shape.
    pub fn add_to_slot(&mut self, slot: usize, delta: u8) -> Option<u8> {
        let cell = self.slots.get_mut(slot)?;
        if cell.0 == 0 {
            return None;
        }
        cell.1 = cell.1.saturating_add(delta).min(STACK_CAP);
        Some(cell.1)
    }
}

/// What a [`compose_window`] pass could not fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefusedGrant {
    /// The item id that had nowhere to go.
    pub id: u8,
    /// How many of it were being placed.
    pub qty: u8,
}

/// Lay an engine-side `(id, qty)` list into a retail item window through
/// retail's **own** add path, then normalize it.
///
/// The engine keeps its bag as a map and hands out a compact list; a retail
/// save block holds a fixed slot array with rules the map does not have. This
/// is the bridge, and it exists because writing the list into the array
/// positionally produces blocks retail cannot represent:
///
/// - **the 99 cap.** The engine's battle-drop and steal grants are
///   `saturating_add(1)` against a `u8` with no cap of their own, so a long
///   session can bank more than [`STACK_CAP`] of an item. Retail's accessors
///   clamp there and its UI draws two digits. [`RetailInventory::add`]'s merge
///   pass applies the clamp.
/// - **one stack per id.** Two entries with the same id are a shape retail's
///   add path never produces and its `find_count` under-reports.
///   [`RetailInventory::normalize`] folds them.
/// - **the full bag.** Grants past the window's capacity are returned as
///   [`RefusedGrant`]s rather than silently dropped - that is retail's
///   full-bag arm surfaced as data instead of performed (it would store the id
///   one slot past the window).
///
/// Returns the composed window and everything that did not fit, in input
/// order.
pub fn compose_window(
    base: u32,
    window_slots: usize,
    grants: &[(u8, u8)],
) -> (RetailInventory, Vec<RefusedGrant>) {
    let mut inv = RetailInventory::new(base, window_slots);
    let mut refused = Vec::new();
    for &(id, qty) in grants {
        if id == 0 {
            // Id 0 is the empty-slot sentinel, not an item: retail's own
            // occupancy test is `id != 0`. Placing one would write a slot
            // every accessor reads as free.
            continue;
        }
        if let AddOutcome::OobIdWrite { .. } = inv.add(id, qty) {
            refused.push(RefusedGrant { id, qty });
        }
    }
    inv.normalize();
    (inv, refused)
}

#[cfg(test)]
mod tests;
