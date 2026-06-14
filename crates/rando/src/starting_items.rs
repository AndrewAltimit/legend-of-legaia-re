//! Starting-inventory randomization: replace the new game's fixed Healing Leaf
//! with random consumables and/or forced convenience items.
//!
//! A vanilla New Game seeds exactly one inventory slot — Healing Leaf (item id
//! `0x77`) ×5 — written in code by `FUN_80034A6C`
//! (see [`legaia_asset::new_game::StartingInventory`] and
//! `docs/formats/new-game-table.md`). There is no static starting-inventory
//! table to edit, so this randomizer rewrites the **seed code** itself: the
//! 40-byte reclaimable region at [`legaia_asset::new_game::STARTING_INV_SEED_VA`]
//! (the original `li`/`sb` seed plus the redundant inline zero-loop the callers
//! already cover with their `SC`-block `memset`).
//!
//! Each item is written with a single packed **halfword store** — an inventory
//! slot is two contiguous bytes `[id][count]`, so `sh $v0` writes both at once
//! after `addiu $v0, $zero, (count << 8) | id`. That is two instructions per
//! item, and the reclaimable inventory region is ten instructions, so it holds
//! [`INV_REGION_SLOTS`] slots. When the all-warps preset is off, the seed also
//! borrows the adjacent warp-preset region (four more instructions, using `$v1`)
//! for [`WARP_REGION_SLOTS`] more slots that continue the same inventory array —
//! a combined [`MAX_STARTING_ITEMS`]. This keeps a full random fill *additive*
//! to the forced convenience items rather than crowding it out. The patch is the
//! same size as the original code (no executable growth or relocation) and is
//! applied through [`crate::disc::DiscPatcher::patch_named_file`] like the steal
//! table.
//!
//! The write lands **directly** in the new game's owned-item list — the single
//! ordered `(id, count)` array the inventory menu later filters into its Items /
//! Goods / Key tabs by item category — bypassing the engine's id-routing add
//! primitive. Because every category shares this one list (verified against a
//! real end-game save: consumables, equipment, and accessories all sit in it as
//! plain `(id, count)` pairs), an explicit convenience toggle can seed an
//! accessory ("Goods") id just as easily as a consumable. The **random** pool,
//! by contrast, is restricted to the contiguous block of genuine consumables
//! ([`STARTING_ITEM_POOL`], Healing Leaf .. Wonder Elixir) so a *random* start
//! stays sensible rather than handing out arbitrary equipment.

use legaia_asset::new_game::{
    DOOR_OF_WIND_ITEM, INCENSE_ITEM, INVENTORY_SC_OFFSET, STARTING_INV_SEED_LEN, WARP_ALL_FLAGS_HI,
    WARP_ALL_FLAGS_LO, WARP_FLAGS_SC_OFFSET, WARP_SEED_LEN,
};

use crate::rng::SplitMix64;

/// Number of MIPS instructions the reclaimable inventory-seed region holds
/// (40 bytes / 4).
const SEED_INSTRS: usize = STARTING_INV_SEED_LEN / 4;

/// Instructions one inventory slot costs: one `addiu,(count<<8)|id` + one
/// `sh,off($s0)`.
const INSTRS_PER_ITEM: usize = 2;

/// Item slots the inventory-seed region holds on its own: ten instructions /
/// two per item (`addiu` + `sh`).
pub const INV_REGION_SLOTS: usize = SEED_INSTRS / INSTRS_PER_ITEM;

/// Extra item slots the warp-preset region ([`legaia_asset::new_game::WARP_SEED_VA`])
/// can hold **when the all-warps preset is not requested**. Its four
/// instructions otherwise carry the visited-towns bitmask; when that preset is
/// off they are free, so the seed borrows them for two more `(id, count)` slots
/// (using `$v1` so they never clobber the live constant the surrounding code
/// keeps in `$v0`). The slots they write continue the inventory array, so a
/// decode that replays both regions reads one contiguous run.
pub const WARP_REGION_SLOTS: usize = WARP_SEED_LEN / 4 / INSTRS_PER_ITEM;

/// Most starting-item slots a seed can hold: the inventory region plus the
/// warp-preset region's borrowed slots. The warp region is only available for
/// items when the all-warps preset is off; with it on the cap is
/// [`INV_REGION_SLOTS`] (see [`plan_seed`]).
pub const MAX_STARTING_ITEMS: usize = INV_REGION_SLOTS + WARP_REGION_SLOTS;

/// Item id of Door of Wind (the warp consumable), re-exported for callers.
pub const DOOR_OF_WIND_ID: u8 = DOOR_OF_WIND_ITEM;

/// Default Door of Wind stack seeded when the toggle is enabled without an
/// explicit count. Door of Wind is consumed per warp, so a small stack keeps it
/// useful for a while without the GameShark "99 in one slot" overkill; the user
/// can override it.
pub const DOOR_OF_WIND_COUNT: u8 = 10;

/// Item id of Incense (the encounter-rate consumable), re-exported for callers.
pub const INCENSE_ID: u8 = INCENSE_ITEM;

/// Default Incense stack seeded when the toggle is enabled without an explicit
/// count. Like Door of Wind, Incense is consumed per use, so a modest stack is a
/// convenience without trivializing exploration; the user can override it.
pub const INCENSE_COUNT: u8 = 10;

/// Item id of the Speed Chain accessory ("Wearer always gets the first turn").
/// An accessory ("Goods"), not a consumable, but the owned-item list is a single
/// ordered `(id, count)` array shared by every category (see the module docs), so
/// it seeds exactly like Door of Wind.
pub const SPEED_CHAIN_ID: u8 = 0xD1;

/// Item id of the Chicken Heart accessory ("Increases the successful-escape
/// rate"). Not to be confused with the Chicken King, which guarantees escape.
pub const CHICKEN_HEART_ID: u8 = 0xF4;

/// Item id of the Good Luck Bell accessory ("Raises the item-drop rate").
pub const GOOD_LUCK_BELL_ID: u8 = 0xFC;

/// Default stack seeded for each accessory convenience toggle when it is enabled
/// without an explicit count. Accessories are equip-once goods (not consumed), so
/// one is the natural default.
pub const ACCESSORY_SEED_COUNT: u8 = 1;

/// Maximum stack the game holds in one inventory slot. A seeded Door-of-Wind
/// count is clamped to this (the count byte is written raw, so a larger value
/// would either overflow the display or be capped by the bag anyway).
pub const MAX_ITEM_STACK: u8 = 99;

/// The vanilla New Game starting item: Healing Leaf (`0x77`) ×5. Preserved as
/// the base slot when a convenience toggle is on but no random reroll was
/// requested, so the toggles read as *additional* to a normal new game.
pub const VANILLA_STARTING_ITEM: (u8, u8) = (0x77, 5);

/// Default `(min, max)` random count for each seeded item (inclusive). Modest,
/// so a random start is helpful without trivializing the early game; vanilla
/// seeds five Healing Leaves.
pub const DEFAULT_COUNT_RANGE: (u8, u8) = (1, 5);

/// What the New Game starting seed should set, beyond the vanilla Healing Leaf.
///
/// Not `Copy`: [`extra_items`] is a `Vec`. Pass by reference (the plan / apply
/// API takes `&StartingSeedOptions`).
///
/// [`extra_items`]: Self::extra_items
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StartingSeedOptions {
    /// Number of random consumables to reroll the starting bag to. `0` keeps
    /// the vanilla Healing Leaf base (so the convenience toggles stay additive).
    pub random_items: usize,
    /// How many Door of Wind to seed into the starting bag. `0` = off; any
    /// positive count seeds one slot of Door of Wind, clamped to
    /// [`MAX_ITEM_STACK`]. (The CLI / web default when the toggle is enabled is
    /// [`DOOR_OF_WIND_COUNT`].)
    pub door_of_wind: u8,
    /// How many Incense to seed into the starting bag. `0` = off; any positive
    /// count seeds one slot of Incense, clamped to [`MAX_ITEM_STACK`]. (The CLI /
    /// web default when the toggle is enabled is [`INCENSE_COUNT`].)
    pub incense: u8,
    /// How many Speed Chain accessories to seed into the starting bag. `0` = off;
    /// clamped to [`MAX_ITEM_STACK`]. (CLI / web default when enabled:
    /// [`ACCESSORY_SEED_COUNT`].)
    pub speed_chain: u8,
    /// How many Chicken Heart accessories to seed. `0` = off (see [`speed_chain`]).
    ///
    /// [`speed_chain`]: Self::speed_chain
    pub chicken_heart: u8,
    /// How many Good Luck Bell accessories to seed. `0` = off (see [`speed_chain`]).
    ///
    /// [`speed_chain`]: Self::speed_chain
    pub good_luck_bell: u8,
    /// Preset the all-towns visited-towns bitmask so Door of Wind can warp to
    /// every destination from the start.
    pub all_warps: bool,
    /// Explicit `(item_id, count)` slots to seed into the starting bag, on top
    /// of the convenience toggles. Additive — like the toggles, these are
    /// seeded into the "forced" prefix that always survives the capacity clamp,
    /// and their ids are excluded from the random reroll so they're never
    /// duplicated. Each `count` is clamped to [`MAX_ITEM_STACK`]; an entry with
    /// id `0` (the no-item sentinel) or count `0` is dropped, and a duplicate id
    /// (already a convenience item or an earlier `extra_items` entry) is skipped
    /// so every slot is distinct. The id space is the full 256-id item table
    /// (consumables, equipment, AND accessories all live in the one owned-item
    /// list — see the module docs), so any item or accessory can be requested.
    /// Slots beyond the direct-seed capacity overflow into the script-injection
    /// path ([`crate::starting_bag`]) like any other bag item.
    pub extra_items: Vec<(u8, u8)>,
}

impl StartingSeedOptions {
    /// `true` when the seed should be rewritten at all (any toggle is set).
    pub fn is_active(&self) -> bool {
        self.random_items > 0
            || self.door_of_wind > 0
            || self.incense > 0
            || self.speed_chain > 0
            || self.chicken_heart > 0
            || self.good_luck_bell > 0
            || self.all_warps
            || self
                .extra_items
                .iter()
                .any(|&(id, count)| id != 0 && count > 0)
    }
}

/// A resolved starting-seed plan: the warp preset plus the concrete inventory
/// slots to write, already clamped to the region's capacity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeedPlan {
    /// Whether to write the all-warps bitmask preset.
    pub all_warps: bool,
    /// The `(id, count)` inventory slots in slot order.
    pub items: Vec<(u8, u8)>,
}

/// The consumable item-id pool starting items are drawn from: the contiguous
/// block from Healing Leaf (`0x77`) through Wonder Elixir (`0x8e`) in the
/// retail item-name table — restoratives, cures, Phoenix, Waters, Doors,
/// Incense, and Elixirs. Everything below `0x77` is equipment and everything
/// above `0x8e` is magic books / quest / key items, none of which belong on the
/// consumable inventory page this seed writes to directly.
pub const STARTING_ITEM_POOL: &[u8] = &[
    0x77, 0x78, 0x79, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e, 0x7f, 0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86,
    0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e,
];

// MIPS encodings for the three instructions the seed uses; see
// `docs/formats/new-game-table.md`. `$v0` = rt 2, `$s0` (= SC base) = base 16.
const NOP: u32 = 0x0000_0000;
/// `addiu $v0, $zero, imm16`.
fn addiu_v0(imm: u16) -> u32 {
    0x2402_0000 | imm as u32
}
/// `sh $v0, off($s0)`.
fn sh_v0_s0(off: u16) -> u32 {
    0xA602_0000 | off as u32
}
/// `addiu $v1, $zero, imm16` — the warp preset uses `$v1` so it never clobbers
/// `$v0`, which carries a live constant through its region (see
/// [`legaia_asset::new_game::WARP_SEED_VA`]).
fn addiu_v1(imm: u16) -> u32 {
    0x2403_0000 | imm as u32
}
/// `sh $v1, off($s0)`.
fn sh_v1_s0(off: u16) -> u32 {
    0xA603_0000 | off as u32
}

/// Plan `n` random starting items from `seed`: `n` distinct ids drawn from
/// [`STARTING_ITEM_POOL`], each with a random count in [`DEFAULT_COUNT_RANGE`].
/// `n` is clamped to `0..=MAX_STARTING_ITEMS` and to the pool size. Deterministic
/// in `(seed, n)`.
pub fn plan_starting_items(seed: u64, n: usize) -> Vec<(u8, u8)> {
    plan_random_items(seed, n.min(MAX_STARTING_ITEMS), &[])
}

/// Plan `n` random starting items, excluding any id in `exclude` from the draw
/// (so a forced item like Door of Wind isn't also dealt a duplicate slot). `n`
/// is clamped to [`MAX_STARTING_ITEMS`] and the (filtered) pool size. Deterministic
/// in `(seed, n, exclude)`.
fn plan_random_items(seed: u64, n: usize, exclude: &[u8]) -> Vec<(u8, u8)> {
    plan_random_items_capped(seed, n, exclude, MAX_STARTING_ITEMS)
}

/// Like [`plan_random_items`] but clamped to `cap` (and the pool size) instead of
/// [`MAX_STARTING_ITEMS`]. The shuffle + per-item count RNG draws are identical
/// regardless of `cap`, so a larger `cap` simply extends the same sequence — the
/// first `min(small_cap, …)` items match a smaller-`cap` call exactly. This lets
/// the script-injection path ([`plan_full_bag`]) plan a bag beyond the 7-slot
/// direct-seed cap whose prefix still equals what [`plan_seed`] seeds directly.
fn plan_random_items_capped(seed: u64, n: usize, exclude: &[u8], cap: usize) -> Vec<(u8, u8)> {
    let mut rng = SplitMix64::new(seed ^ 0x5247_4E49_5453_5452); // "RGNITSTR"-ish salt
    let mut pool: Vec<u8> = STARTING_ITEM_POOL
        .iter()
        .copied()
        .filter(|id| !exclude.contains(id))
        .collect();
    rng.shuffle(&mut pool);
    let (lo, hi) = DEFAULT_COUNT_RANGE;
    let span = (hi - lo) as usize + 1;
    pool.into_iter()
        .take(n.min(cap))
        .map(|id| (id, lo + rng.below(span) as u8))
        .collect()
}

/// Capacity of the direct new-game seed given the warp preset: [`INV_REGION_SLOTS`]
/// when `all_warps` claims the warp region, else [`MAX_STARTING_ITEMS`]. Items
/// beyond this are granted by the script-injection path ([`overflow_bag`]).
pub fn direct_cap(all_warps: bool) -> usize {
    if all_warps {
        INV_REGION_SLOTS
    } else {
        MAX_STARTING_ITEMS
    }
}

/// The **forced** starting-bag items in slot order: the enabled convenience
/// toggles (Door of Wind, Incense, then the Speed Chain / Chicken Heart / Good
/// Luck Bell accessories) followed by the user's explicit [`StartingSeedOptions::extra_items`].
///
/// Each entry's count is clamped to [`MAX_ITEM_STACK`]; entries with id `0` (the
/// no-item sentinel) or count `0` are dropped, and a duplicate id is skipped so
/// every returned slot is distinct. These items are seeded first by both
/// [`plan_seed`] and [`plan_full_bag`] so they survive the capacity clamp, and
/// their ids are excluded from the random reroll. Factoring this into one helper
/// keeps the direct seed an exact prefix of the full bag (the invariant
/// [`overflow_bag`] relies on).
fn forced_items(opts: &StartingSeedOptions) -> Vec<(u8, u8)> {
    let convenience = [
        (DOOR_OF_WIND_ID, opts.door_of_wind),
        (INCENSE_ID, opts.incense),
        (SPEED_CHAIN_ID, opts.speed_chain),
        (CHICKEN_HEART_ID, opts.chicken_heart),
        (GOOD_LUCK_BELL_ID, opts.good_luck_bell),
    ];
    let mut items: Vec<(u8, u8)> = Vec::new();
    for &(id, count) in convenience.iter().chain(opts.extra_items.iter()) {
        let count = count.min(MAX_ITEM_STACK);
        if id != 0 && count > 0 && !items.iter().any(|&(seen, _)| seen == id) {
            items.push((id, count));
        }
    }
    items
}

/// The **full**, uncapped starting bag for `opts` — the forced convenience items
/// then the full requested random fill (or the vanilla Healing Leaf base when no
/// reroll), in the exact order [`plan_seed`] composes. So `plan_seed`'s output is
/// this list truncated to [`direct_cap`], and the remainder is the [`overflow_bag`]
/// the script path grants. Deterministic in `(seed, opts)`.
pub fn plan_full_bag(seed: u64, opts: &StartingSeedOptions) -> Vec<(u8, u8)> {
    let cap = direct_cap(opts.all_warps);
    let mut items = forced_items(opts);
    let forced_ids: Vec<u8> = items.iter().map(|&(id, _)| id).collect();
    if opts.random_items > 0 {
        // Uncapped random fill (cap = pool size); the first `cap - forced` of these
        // match what `plan_seed` seeds directly, the rest are the overflow.
        items.extend(plan_random_items_capped(
            seed,
            opts.random_items,
            &forced_ids,
            STARTING_ITEM_POOL.len(),
        ));
    } else if items.len() < cap {
        items.push(VANILLA_STARTING_ITEM);
    }
    items
}

/// The starting-bag slots beyond the direct seed's [`direct_cap`] — the items the
/// script-injection path ([`crate::starting_bag`]) grants on top of what
/// [`plan_seed`] writes directly. Empty when the whole bag fits the direct seed.
/// Deterministic in `(seed, opts)`.
pub fn overflow_bag(seed: u64, opts: &StartingSeedOptions) -> Vec<(u8, u8)> {
    let full = plan_full_bag(seed, opts);
    let cap = direct_cap(opts.all_warps);
    if full.len() > cap {
        full[cap..].to_vec()
    } else {
        Vec::new()
    }
}

/// Resolve [`StartingSeedOptions`] into a concrete [`SeedPlan`] for `seed`.
///
/// Composition, in slot order:
/// 1. **Forced items** — the enabled convenience toggles (Door of Wind, Incense,
///    then the Speed Chain / Chicken Heart / Good Luck Bell accessories, each
///    `count`× clamped to [`MAX_ITEM_STACK`]) followed by the user's explicit
///    [`StartingSeedOptions::extra_items`], written first so they always survive
///    the capacity clamp (see [`forced_items`]).
/// 2. **Base / reroll**: with `random_items == 0` the vanilla Healing Leaf base
///    is kept (the convenience toggles stay additive to a normal new game);
///    with `random_items > 0` the bag is rerolled to that many random
///    consumables instead (the existing `--starting-items` behaviour), drawn
///    excluding any forced item so it isn't duplicated.
///
/// The item list is clamped to the available capacity: [`MAX_STARTING_ITEMS`]
/// normally, or [`INV_REGION_SLOTS`] when the all-warps preset is on (it then
/// claims the warp-preset region that would otherwise hold the last item
/// slots). Convenience items are seeded first so they survive the clamp; the
/// random fill takes whatever capacity is left — so the requested random count
/// is preserved as long as it fits *on top of* the convenience items rather
/// than being displaced by them. Deterministic in `(seed, opts)`.
pub fn plan_seed(seed: u64, opts: &StartingSeedOptions) -> SeedPlan {
    // The warp-preset region doubles as the last two item slots, but only when
    // the all-warps bitmask isn't using it.
    let cap = if opts.all_warps {
        INV_REGION_SLOTS
    } else {
        MAX_STARTING_ITEMS
    };
    // Forced items (convenience toggles + explicit extras), in a stable slot
    // order. Each is seeded first so it always survives the capacity clamp; its
    // id is excluded from the reroll below so a random fill can't deal a
    // duplicate slot.
    let mut items = forced_items(opts);
    let forced_ids: Vec<u8> = items.iter().map(|&(id, _)| id).collect();

    if opts.random_items > 0 {
        let room = cap.saturating_sub(items.len());
        let n = opts.random_items.min(room);
        items.extend(plan_random_items(seed, n, &forced_ids));
    } else if items.len() < cap {
        // No reroll requested: keep the vanilla Healing Leaf base so the
        // toggles are purely additive.
        items.push(VANILLA_STARTING_ITEM);
    }

    items.truncate(cap);
    SeedPlan {
        all_warps: opts.all_warps,
        items,
    }
}

/// Encode a list of `(id, count)` starting items into the 40-byte inventory
/// seed patch. Convenience wrapper over [`build_seed_patch_for`].
pub fn build_seed_patch(items: &[(u8, u8)]) -> [u8; STARTING_INV_SEED_LEN] {
    build_seed_patch_for(&SeedPlan {
        all_warps: false,
        items: items.to_vec(),
    })
}

/// Encode a [`SeedPlan`]'s **inventory-region** slots into the 40-byte
/// inventory-seed patch (the region at
/// [`legaia_asset::new_game::STARTING_INV_SEED_VA`]).
///
/// Only the first [`INV_REGION_SLOTS`] slots land here; any overflow is encoded
/// into the warp-preset region by [`build_warp_items_patch`] (see
/// [`overflow_items`]). Thin wrapper over [`build_inv_patch`].
pub fn build_seed_patch_for(plan: &SeedPlan) -> [u8; STARTING_INV_SEED_LEN] {
    let n = plan.items.len().min(INV_REGION_SLOTS);
    build_inv_patch(&plan.items[..n])
}

/// Encode up to [`INV_REGION_SLOTS`] `(id, count)` slots into the 40-byte
/// inventory-seed region.
///
/// Emits one `addiu $v0, $zero, (count << 8) | id` + `sh $v0, (0x1818 + 2k)($s0)`
/// pair per slot (slot `k` at `INVENTORY_SC_OFFSET + 2k`), padded to
/// [`STARTING_INV_SEED_LEN`] with `nop` (which also overwrites the redundant
/// zero-loop — required for the warp preset, see [`build_warp_patch`]). Panics
/// if more than [`INV_REGION_SLOTS`] slots are given (callers clamp via
/// [`plan_seed`]). The inventory base offset comes from [`INVENTORY_SC_OFFSET`].
pub fn build_inv_patch(items: &[(u8, u8)]) -> [u8; STARTING_INV_SEED_LEN] {
    let mut words: Vec<u32> = Vec::with_capacity(SEED_INSTRS);
    for (slot, &(id, count)) in items.iter().enumerate() {
        let off = (INVENTORY_SC_OFFSET as usize + slot * 2) as u16;
        words.push(addiu_v0(((count as u16) << 8) | id as u16));
        words.push(sh_v0_s0(off));
    }
    assert!(
        words.len() <= SEED_INSTRS,
        "{} item slots need {} instructions but only {SEED_INSTRS} fit the inventory region",
        items.len(),
        words.len()
    );
    while words.len() < SEED_INSTRS {
        words.push(NOP);
    }
    let mut out = [0u8; STARTING_INV_SEED_LEN];
    for (i, w) in words.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

/// The item slots that overflow past the inventory region (slots
/// [`INV_REGION_SLOTS`]..), which the warp-preset region carries when all-warps
/// is off. Empty when the plan fits the inventory region alone, or when
/// `all_warps` reserves the warp region for the bitmask. Slice into
/// [`build_warp_items_patch`].
pub fn overflow_items(plan: &SeedPlan) -> &[(u8, u8)] {
    if plan.all_warps || plan.items.len() <= INV_REGION_SLOTS {
        &[]
    } else {
        &plan.items[INV_REGION_SLOTS..]
    }
}

/// Encode the overflow item slots into the 16-byte warp-preset region as
/// `addiu $v1, $zero, (count << 8) | id` + `sh $v1, off($s0)` pairs that
/// **continue** the inventory array at slot [`INV_REGION_SLOTS`]+, padded with
/// `nop`. Uses `$v1` (not `$v0`) for the same reason [`build_warp_patch`] does:
/// the surrounding code keeps a live `0x2dc0` constant in `$v0`. Only used when
/// the all-warps preset is off (otherwise that preset owns this region). Panics
/// if more than [`WARP_REGION_SLOTS`] slots are given (callers clamp via
/// [`plan_seed`] / [`overflow_items`]).
pub fn build_warp_items_patch(items: &[(u8, u8)]) -> [u8; WARP_SEED_LEN] {
    assert!(
        items.len() <= WARP_REGION_SLOTS,
        "{} overflow slots exceed the {WARP_REGION_SLOTS}-slot warp region",
        items.len()
    );
    let mut words: Vec<u32> = Vec::with_capacity(WARP_SEED_LEN / 4);
    for (i, &(id, count)) in items.iter().enumerate() {
        let slot = INV_REGION_SLOTS + i;
        let off = (INVENTORY_SC_OFFSET as usize + slot * 2) as u16;
        words.push(addiu_v1(((count as u16) << 8) | id as u16));
        words.push(sh_v1_s0(off));
    }
    while words.len() < WARP_SEED_LEN / 4 {
        words.push(NOP);
    }
    let mut out = [0u8; WARP_SEED_LEN];
    for (i, w) in words.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

/// Encode the all-warps preset into the 16-byte warp-seed patch (the separate
/// region at [`legaia_asset::new_game::WARP_SEED_VA`]).
///
/// Two `addiu $v1, $zero, imm` + `sh $v1, off($s0)` pairs that write the
/// visited-towns bitmask ([`WARP_FLAGS_SC_OFFSET`]). It uses `$v1` (not `$v0`)
/// to avoid clobbering the live `0x2dc0` constant the surrounding code carries
/// in `$v0`. Only applied when `all_warps` is set; otherwise the region keeps
/// its original (redundant) bytes. Because this region runs *before* the
/// inventory seed's zero-loop, the caller must also rewrite the inventory region
/// (dropping that loop) for the preset to survive — which is always the case
/// when any seed toggle is active.
pub fn build_warp_patch() -> [u8; WARP_SEED_LEN] {
    let words = [
        addiu_v1(WARP_ALL_FLAGS_LO),
        sh_v1_s0(WARP_FLAGS_SC_OFFSET as u16),
        addiu_v1(WARP_ALL_FLAGS_HI),
        sh_v1_s0(WARP_FLAGS_SC_OFFSET as u16 + 2),
    ];
    let mut out = [0u8; WARP_SEED_LEN];
    for (i, w) in words.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::new_game::StartingInventory;

    #[test]
    fn pool_is_the_consumable_block() {
        assert_eq!(STARTING_ITEM_POOL.first(), Some(&0x77)); // Healing Leaf
        assert_eq!(STARTING_ITEM_POOL.last(), Some(&0x8e)); // Wonder Elixir
        assert_eq!(STARTING_ITEM_POOL.len(), 0x8e - 0x77 + 1);
        // Contiguous, ascending, no equipment / key items.
        for w in STARTING_ITEM_POOL.windows(2) {
            assert_eq!(w[1], w[0] + 1);
        }
    }

    #[test]
    fn capacity_is_inventory_plus_warp_region() {
        // Inventory region (10 instrs / 2) plus the warp region's borrowed
        // slots (16 bytes / 4 / 2) when all-warps is off.
        assert_eq!(INV_REGION_SLOTS, 5);
        assert_eq!(WARP_REGION_SLOTS, 2);
        assert_eq!(MAX_STARTING_ITEMS, 7);
    }

    #[test]
    fn plan_gives_distinct_ids_in_pool_with_valid_counts() {
        let items = plan_starting_items(0xC0FFEE, 5);
        assert_eq!(items.len(), 5);
        let mut ids: Vec<u8> = items.iter().map(|i| i.0).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 5, "ids are distinct");
        for (id, count) in items {
            assert!(STARTING_ITEM_POOL.contains(&id));
            assert!((DEFAULT_COUNT_RANGE.0..=DEFAULT_COUNT_RANGE.1).contains(&count));
        }
    }

    #[test]
    fn plan_clamps_to_max_and_is_deterministic() {
        let a = plan_starting_items(42, 99);
        let b = plan_starting_items(42, 99);
        assert_eq!(a, b, "deterministic for a fixed seed");
        assert_eq!(
            a.len(),
            MAX_STARTING_ITEMS,
            "clamped to the region capacity"
        );
        assert!(plan_starting_items(42, 0).is_empty());
    }

    #[test]
    fn build_patch_round_trips_through_the_decoder() {
        // The encoder + the asset-side decoder must agree: build a patch, decode
        // it back, get the same slots. This is the contract the disc round-trip
        // and the runtime oracle both rely on.
        let items = vec![(0x80u8, 2u8), (0x7eu8, 1u8), (0x8au8, 4u8)];
        let patch = build_seed_patch(&items);
        let decoded = StartingInventory::decode_region(&patch);
        assert_eq!(decoded.items(), items.as_slice());
    }

    #[test]
    fn empty_patch_is_all_nops_and_decodes_to_nothing() {
        let patch = build_seed_patch(&[]);
        assert!(patch.iter().all(|&b| b == 0));
        assert!(StartingInventory::decode_region(&patch).is_empty());
    }

    #[test]
    fn five_random_items_round_trip() {
        let items = plan_starting_items(7, 5);
        let patch = build_seed_patch(&items);
        assert_eq!(StartingInventory::decode_region(&patch).items(), &items[..]);
    }

    #[test]
    fn all_warps_trades_the_two_overflow_slots_for_the_bitmask() {
        // The warp-preset region carries EITHER the all-warps bitmask OR the two
        // item slots that overflow the inventory region. So enabling all-warps
        // lowers the item cap to the inventory region alone; with it off the bag
        // can use the full capacity.
        let many = |all_warps| StartingSeedOptions {
            random_items: MAX_STARTING_ITEMS, // ask for more than the inv region holds
            all_warps,
            ..Default::default()
        };
        let with = plan_seed(7, &many(true));
        assert_eq!(with.items.len(), INV_REGION_SLOTS);
        assert!(
            overflow_items(&with).is_empty(),
            "warp region is reserved for the bitmask"
        );
        let without = plan_seed(7, &many(false));
        assert_eq!(without.items.len(), MAX_STARTING_ITEMS);
        assert_eq!(overflow_items(&without).len(), WARP_REGION_SLOTS);
    }

    #[test]
    fn random_fill_is_additive_to_convenience_items() {
        // The reported case: enable convenience items AND ask for a full random
        // bag. The convenience items must all survive, and the random fill takes
        // the REMAINING capacity rather than the convenience items eating into
        // the random count. With two accessories + five random (= the combined
        // capacity) everything fits, so the requested five random are preserved.
        let opts = StartingSeedOptions {
            random_items: 5,
            speed_chain: ACCESSORY_SEED_COUNT,
            chicken_heart: ACCESSORY_SEED_COUNT,
            ..Default::default()
        };
        let plan = plan_seed(7, &opts);
        assert!(plan.items.contains(&(SPEED_CHAIN_ID, ACCESSORY_SEED_COUNT)));
        assert!(
            plan.items
                .contains(&(CHICKEN_HEART_ID, ACCESSORY_SEED_COUNT))
        );
        assert_eq!(plan.items.len(), MAX_STARTING_ITEMS);
        // The non-forced slots are the five random consumables, intact.
        let randoms = plan
            .items
            .iter()
            .filter(|(id, _)| STARTING_ITEM_POOL.contains(id))
            .count();
        assert_eq!(
            randoms, 5,
            "all five random items survive the convenience picks"
        );
    }

    #[test]
    fn dual_region_round_trip_reads_all_slots() {
        // A bag larger than the inventory region (all-warps off): the first
        // INV_REGION_SLOTS land in the inventory region and the rest in the warp
        // region; decoding BOTH yields the original slots in order.
        let plan = plan_seed(
            0xD15C,
            &StartingSeedOptions {
                random_items: MAX_STARTING_ITEMS,
                ..Default::default()
            },
        );
        assert_eq!(plan.items.len(), MAX_STARTING_ITEMS);
        let overflow = overflow_items(&plan);
        assert_eq!(overflow.len(), WARP_REGION_SLOTS);
        let inv = build_seed_patch_for(&plan);
        let warp = build_warp_items_patch(overflow);
        let decoded = StartingInventory::decode_regions(&inv, &warp);
        assert_eq!(decoded.items(), &plan.items[..]);
    }

    #[test]
    fn door_of_wind_only_is_additive_to_the_vanilla_base() {
        // No reroll, no warps: keep Healing Leaf AND add Door of Wind x10.
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                door_of_wind: DOOR_OF_WIND_COUNT,
                ..Default::default()
            },
        );
        assert!(!plan.all_warps);
        assert_eq!(
            plan.items,
            vec![(DOOR_OF_WIND_ID, DOOR_OF_WIND_COUNT), VANILLA_STARTING_ITEM]
        );
        let patch = build_seed_patch_for(&plan);
        assert_eq!(
            StartingInventory::decode_region(&patch).items(),
            &[(DOOR_OF_WIND_ID, DOOR_OF_WIND_COUNT), VANILLA_STARTING_ITEM]
        );
    }

    #[test]
    fn door_of_wind_count_is_user_settable_and_clamped() {
        // An explicit count is seeded verbatim.
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                door_of_wind: 25,
                ..Default::default()
            },
        );
        assert_eq!(plan.items[0], (DOOR_OF_WIND_ID, 25));
        // A count above the stack cap clamps to MAX_ITEM_STACK.
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                door_of_wind: 250,
                ..Default::default()
            },
        );
        assert_eq!(plan.items[0], (DOOR_OF_WIND_ID, MAX_ITEM_STACK));
        // Count 0 means off: no Door of Wind, just the vanilla base.
        let plan = plan_seed(1, &StartingSeedOptions::default());
        assert_eq!(plan.items, vec![VANILLA_STARTING_ITEM]);
        assert!(!plan.items.iter().any(|(id, _)| *id == DOOR_OF_WIND_ID));
    }

    #[test]
    fn all_warps_emits_the_bitmask_in_its_own_region() {
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                all_warps: true,
                ..Default::default()
            },
        );
        assert!(plan.all_warps);
        assert_eq!(plan.items, vec![VANILLA_STARTING_ITEM]);
        // The warp preset is a SEPARATE region; the inventory patch carries no
        // warp stores.
        let inv = build_seed_patch_for(&plan);
        assert!(!legaia_asset::new_game::region_unlocks_all_warps(&inv));
        assert_eq!(
            StartingInventory::decode_region(&inv).items(),
            &[VANILLA_STARTING_ITEM]
        );
        // The dedicated warp patch sets the bitmask.
        let warp = build_warp_patch();
        assert!(legaia_asset::new_game::region_unlocks_all_warps(&warp));
    }

    #[test]
    fn door_plus_warps_plus_reroll_keeps_five_item_slots() {
        // Request 5 random + door of wind + warps: with all-warps on the bag is
        // capped at the inventory region (5 slots), so door takes one and 4
        // random fills — the door is never crowded out.
        let opts = StartingSeedOptions {
            random_items: 5,
            door_of_wind: DOOR_OF_WIND_COUNT,
            all_warps: true,
            ..Default::default()
        };
        let plan = plan_seed(99, &opts);
        assert!(plan.all_warps);
        assert_eq!(plan.items.len(), 5, "all five item slots stay available");
        assert_eq!(plan.items[0], (DOOR_OF_WIND_ID, DOOR_OF_WIND_COUNT));
        // The random fills are distinct consumables, never Door of Wind.
        for (id, _) in &plan.items[1..] {
            assert!(STARTING_ITEM_POOL.contains(id));
            assert_ne!(*id, DOOR_OF_WIND_ID, "reroll excludes the forced item");
        }
        let inv = build_seed_patch_for(&plan);
        assert_eq!(
            StartingInventory::decode_region(&inv).items(),
            &plan.items[..]
        );
    }

    #[test]
    fn reroll_replaces_the_vanilla_base() {
        // With a reroll requested, the Healing Leaf base is dropped.
        let plan = plan_seed(
            3,
            &StartingSeedOptions {
                random_items: 3,
                ..Default::default()
            },
        );
        assert_eq!(plan.items.len(), 3);
        assert_eq!(plan.items, plan_starting_items(3, 3));
    }

    #[test]
    fn plan_seed_is_deterministic() {
        let opts = StartingSeedOptions {
            random_items: 4,
            door_of_wind: DOOR_OF_WIND_COUNT,
            all_warps: true,
            ..Default::default()
        };
        assert_eq!(plan_seed(0xABCD, &opts), plan_seed(0xABCD, &opts));
    }

    #[test]
    fn inactive_options_are_detected() {
        assert!(!StartingSeedOptions::default().is_active());
        assert!(
            StartingSeedOptions {
                all_warps: true,
                ..Default::default()
            }
            .is_active()
        );
        // Incense alone activates the seed rewrite.
        assert!(
            StartingSeedOptions {
                incense: INCENSE_COUNT,
                ..Default::default()
            }
            .is_active()
        );
    }

    #[test]
    fn incense_only_is_additive_to_the_vanilla_base() {
        // No reroll, no warps: keep Healing Leaf AND add Incense x10.
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                incense: INCENSE_COUNT,
                ..Default::default()
            },
        );
        assert!(!plan.all_warps);
        assert_eq!(
            plan.items,
            vec![(INCENSE_ID, INCENSE_COUNT), VANILLA_STARTING_ITEM]
        );
        let patch = build_seed_patch_for(&plan);
        assert_eq!(
            StartingInventory::decode_region(&patch).items(),
            &[(INCENSE_ID, INCENSE_COUNT), VANILLA_STARTING_ITEM]
        );
    }

    #[test]
    fn incense_count_is_user_settable_and_clamped() {
        // An explicit count is seeded verbatim.
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                incense: 30,
                ..Default::default()
            },
        );
        assert_eq!(plan.items[0], (INCENSE_ID, 30));
        // A count above the stack cap clamps to MAX_ITEM_STACK.
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                incense: 250,
                ..Default::default()
            },
        );
        assert_eq!(plan.items[0], (INCENSE_ID, MAX_ITEM_STACK));
        // Count 0 means off: no Incense, just the vanilla base.
        let plan = plan_seed(1, &StartingSeedOptions::default());
        assert!(!plan.items.iter().any(|(id, _)| *id == INCENSE_ID));
    }

    #[test]
    fn door_of_wind_and_incense_seed_distinct_slots() {
        // Both convenience items seeded: Door of Wind first, then Incense, then
        // the vanilla Healing Leaf base — three distinct slots.
        let plan = plan_seed(
            5,
            &StartingSeedOptions {
                door_of_wind: DOOR_OF_WIND_COUNT,
                incense: INCENSE_COUNT,
                ..Default::default()
            },
        );
        assert_eq!(
            plan.items,
            vec![
                (DOOR_OF_WIND_ID, DOOR_OF_WIND_COUNT),
                (INCENSE_ID, INCENSE_COUNT),
                VANILLA_STARTING_ITEM,
            ]
        );
        let patch = build_seed_patch_for(&plan);
        assert_eq!(
            StartingInventory::decode_region(&patch).items(),
            &plan.items[..]
        );
    }

    #[test]
    fn accessory_toggles_are_additive_and_default_to_one() {
        // Each accessory toggle alone seeds one of that accessory plus the
        // vanilla Healing Leaf base. Build the three option sets explicitly.
        let cases = [
            (
                StartingSeedOptions {
                    speed_chain: ACCESSORY_SEED_COUNT,
                    ..Default::default()
                },
                SPEED_CHAIN_ID,
            ),
            (
                StartingSeedOptions {
                    chicken_heart: ACCESSORY_SEED_COUNT,
                    ..Default::default()
                },
                CHICKEN_HEART_ID,
            ),
            (
                StartingSeedOptions {
                    good_luck_bell: ACCESSORY_SEED_COUNT,
                    ..Default::default()
                },
                GOOD_LUCK_BELL_ID,
            ),
        ];
        for (opts, id) in cases {
            assert!(opts.is_active());
            let plan = plan_seed(1, &opts);
            assert_eq!(
                plan.items,
                vec![(id, ACCESSORY_SEED_COUNT), VANILLA_STARTING_ITEM],
                "accessory {id:#04x} seeded additively, count 1"
            );
            // Round-trips through the encoder/decoder.
            let patch = build_seed_patch_for(&plan);
            assert_eq!(
                StartingInventory::decode_region(&patch).items(),
                &plan.items[..]
            );
        }
    }

    #[test]
    fn all_convenience_items_seed_alongside_the_vanilla_base() {
        // Door of Wind, Incense, then the three accessories — five forced items.
        // With the inventory + warp capacity there is still room for the vanilla
        // Healing Leaf, so it stays (the toggles are additive to a normal new
        // game) and spills into the warp region as the sixth slot.
        let plan = plan_seed(
            7,
            &StartingSeedOptions {
                door_of_wind: DOOR_OF_WIND_COUNT,
                incense: INCENSE_COUNT,
                speed_chain: ACCESSORY_SEED_COUNT,
                chicken_heart: ACCESSORY_SEED_COUNT,
                good_luck_bell: ACCESSORY_SEED_COUNT,
                ..Default::default()
            },
        );
        assert_eq!(
            plan.items,
            vec![
                (DOOR_OF_WIND_ID, DOOR_OF_WIND_COUNT),
                (INCENSE_ID, INCENSE_COUNT),
                (SPEED_CHAIN_ID, ACCESSORY_SEED_COUNT),
                (CHICKEN_HEART_ID, ACCESSORY_SEED_COUNT),
                (GOOD_LUCK_BELL_ID, ACCESSORY_SEED_COUNT),
                VANILLA_STARTING_ITEM,
            ],
            "five forced items in order, Healing Leaf kept"
        );
        // Decoding both regions (the Healing Leaf overflows into the warp region)
        // reads every slot back.
        let inv = build_seed_patch_for(&plan);
        let warp = build_warp_items_patch(overflow_items(&plan));
        assert_eq!(
            StartingInventory::decode_regions(&inv, &warp).items(),
            &plan.items[..]
        );
    }

    #[test]
    fn reroll_excludes_both_forced_items() {
        // A reroll alongside both forced items must not deal a duplicate of
        // either; with all-warps off all seven slots fill (2 forced + 5 random).
        let plan = plan_seed(
            99,
            &StartingSeedOptions {
                random_items: 5,
                door_of_wind: DOOR_OF_WIND_COUNT,
                incense: INCENSE_COUNT,
                ..Default::default()
            },
        );
        assert_eq!(plan.items.len(), MAX_STARTING_ITEMS);
        assert_eq!(plan.items[0], (DOOR_OF_WIND_ID, DOOR_OF_WIND_COUNT));
        assert_eq!(plan.items[1], (INCENSE_ID, INCENSE_COUNT));
        for (id, _) in &plan.items[2..] {
            assert!(STARTING_ITEM_POOL.contains(id));
            assert_ne!(*id, DOOR_OF_WIND_ID, "reroll excludes Door of Wind");
            assert_ne!(*id, INCENSE_ID, "reroll excludes Incense");
        }
    }

    #[test]
    fn direct_seed_is_the_full_bag_prefix_and_overflow_is_the_rest() {
        // A bag past the direct cap (2 convenience + a big random fill), with and
        // without all-warps (which lowers the cap). The direct seed must equal the
        // full bag's prefix and `direct ++ overflow` must reconstruct it exactly —
        // so the script path grants precisely the items the direct seed dropped, no
        // duplicate, no gap.
        for all_warps in [false, true] {
            let opts = StartingSeedOptions {
                random_items: 12,
                door_of_wind: DOOR_OF_WIND_COUNT,
                incense: INCENSE_COUNT,
                all_warps,
                ..Default::default()
            };
            for seed in [1u64, 42, 0xC0FFEE, 1781435615857] {
                let direct = plan_seed(seed, &opts).items;
                let full = plan_full_bag(seed, &opts);
                let overflow = overflow_bag(seed, &opts);
                let cap = direct_cap(all_warps);
                assert!(direct.len() <= cap, "direct seed within its cap");
                assert_eq!(
                    direct,
                    full[..direct.len()],
                    "direct seed is the full bag's prefix (all_warps={all_warps}, seed={seed})"
                );
                let mut combined = direct.clone();
                combined.extend(overflow.iter().copied());
                assert_eq!(
                    combined, full,
                    "direct + overflow reconstructs the full bag (no dup, no gap)"
                );
                // The bag exceeds the cap, so there IS overflow for the script path.
                assert!(!overflow.is_empty(), "this bag overflows the direct cap");
            }
        }
    }

    #[test]
    fn small_bag_has_no_overflow() {
        // A bag that fits the direct seed produces no script-path overflow.
        let opts = StartingSeedOptions {
            door_of_wind: DOOR_OF_WIND_COUNT,
            incense: INCENSE_COUNT,
            ..Default::default()
        };
        assert!(overflow_bag(7, &opts).is_empty());
    }

    #[test]
    fn explicit_extra_items_activate_and_seed_additively() {
        // An explicit item alone activates the seed and is added on top of the
        // vanilla Healing Leaf base (no reroll requested).
        let opts = StartingSeedOptions {
            extra_items: vec![(0xD1, 1)], // Speed Chain accessory
            ..Default::default()
        };
        assert!(opts.is_active());
        let plan = plan_seed(1, &opts);
        assert_eq!(plan.items, vec![(0xD1, 1), VANILLA_STARTING_ITEM]);
        let patch = build_seed_patch_for(&plan);
        assert_eq!(
            StartingInventory::decode_region(&patch).items(),
            &[(0xD1, 1), VANILLA_STARTING_ITEM]
        );
    }

    #[test]
    fn extra_items_follow_convenience_items_in_slot_order() {
        // Convenience toggles seed first, then the explicit extras, then the
        // vanilla base — a stable, predictable slot order.
        let opts = StartingSeedOptions {
            door_of_wind: DOOR_OF_WIND_COUNT,
            extra_items: vec![(0x30, 2), (0x42, 9)], // arbitrary equipment ids
            ..Default::default()
        };
        let plan = plan_seed(1, &opts);
        assert_eq!(
            plan.items,
            vec![
                (DOOR_OF_WIND_ID, DOOR_OF_WIND_COUNT),
                (0x30, 2),
                (0x42, 9),
                VANILLA_STARTING_ITEM,
            ]
        );
    }

    #[test]
    fn extra_items_clamp_count_and_drop_invalid_or_duplicate_entries() {
        let opts = StartingSeedOptions {
            door_of_wind: DOOR_OF_WIND_COUNT,
            extra_items: vec![
                (0x42, 250),          // count clamps to the stack cap
                (0x00, 5),            // id 0 is the no-item sentinel: dropped
                (0x43, 0),            // count 0: dropped
                (0x42, 1),            // duplicate id: skipped (first wins)
                (DOOR_OF_WIND_ID, 1), // already a convenience item: skipped
            ],
            ..Default::default()
        };
        let plan = plan_seed(1, &opts);
        assert_eq!(
            plan.items,
            vec![
                (DOOR_OF_WIND_ID, DOOR_OF_WIND_COUNT),
                (0x42, MAX_ITEM_STACK),
                VANILLA_STARTING_ITEM,
            ]
        );
    }

    #[test]
    fn random_reroll_excludes_explicit_extra_items() {
        // A consumable id requested explicitly must not also be dealt by the
        // random fill (no duplicate slot).
        let extra_id = STARTING_ITEM_POOL[3];
        let opts = StartingSeedOptions {
            random_items: MAX_STARTING_ITEMS,
            extra_items: vec![(extra_id, 7)],
            ..Default::default()
        };
        let plan = plan_seed(123, &opts);
        assert_eq!(plan.items[0], (extra_id, 7), "explicit item seeded first");
        let dupes = plan.items[1..]
            .iter()
            .filter(|&&(id, _)| id == extra_id)
            .count();
        assert_eq!(dupes, 0, "the random fill never repeats the explicit id");
    }

    #[test]
    fn extra_items_beyond_the_cap_become_overflow() {
        // More explicit items than the direct seed holds: the first `cap` land in
        // the direct seed and the rest are the script-path overflow. `direct ++
        // overflow` reconstructs the full forced list (the same prefix invariant
        // the convenience/random path relies on).
        let extras: Vec<(u8, u8)> = (0x20u8..0x2Cu8).map(|id| (id, 1)).collect(); // 12 items
        let opts = StartingSeedOptions {
            extra_items: extras.clone(),
            ..Default::default()
        };
        let direct = plan_seed(1, &opts).items;
        let overflow = overflow_bag(1, &opts);
        assert_eq!(
            direct.len(),
            MAX_STARTING_ITEMS,
            "direct seed fills its cap"
        );
        assert!(!overflow.is_empty(), "the rest overflow to the script path");
        let mut combined = direct;
        combined.extend(overflow);
        assert_eq!(combined, extras, "direct + overflow reconstructs the bag");
    }
}
