//! Starting-inventory randomization: replace the new game's fixed Healing Leaf
//! with up to five random consumables.
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
//! item, and the reclaimable region is ten instructions, so the seed holds at
//! most [`MAX_STARTING_ITEMS`] slots. The patch is the same size as the
//! original code (no executable growth or relocation) and is applied through
//! [`crate::disc::DiscPatcher::patch_named_file`] like the steal table.
//!
//! Because the write lands **directly** in the consumable inventory page
//! (bypassing the engine's id-routing add primitive), the random pool is
//! restricted to the contiguous block of genuine consumables
//! ([`STARTING_ITEM_POOL`], Healing Leaf .. Wonder Elixir) so a starting item
//! is always something that belongs on that page.

use legaia_asset::new_game::{
    DOOR_OF_WIND_ITEM, INCENSE_ITEM, INVENTORY_SC_OFFSET, STARTING_INV_SEED_LEN, WARP_ALL_FLAGS_HI,
    WARP_ALL_FLAGS_LO, WARP_FLAGS_SC_OFFSET, WARP_SEED_LEN,
};

use crate::rng::SplitMix64;

/// Number of MIPS instructions the reclaimable inventory-seed region holds
/// (40 bytes / 4).
const SEED_INSTRS: usize = STARTING_INV_SEED_LEN / 4;

/// Instructions one inventory slot costs: one `addiu $v0,(count<<8)|id` + one
/// `sh $v0,off($s0)`.
const INSTRS_PER_ITEM: usize = 2;

/// Most starting-item slots the reclaimable seed region can hold: ten
/// instructions / two per item (`addiu` + `sh`).
pub const MAX_STARTING_ITEMS: usize = SEED_INSTRS / INSTRS_PER_ITEM;

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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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
    /// Preset the all-towns visited-towns bitmask so Door of Wind can warp to
    /// every destination from the start.
    pub all_warps: bool,
}

impl StartingSeedOptions {
    /// `true` when the seed should be rewritten at all (any toggle is set).
    pub fn is_active(&self) -> bool {
        self.random_items > 0 || self.door_of_wind > 0 || self.incense > 0 || self.all_warps
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
/// is clamped to the (filtered) pool size. Deterministic in `(seed, n, exclude)`.
fn plan_random_items(seed: u64, n: usize, exclude: &[u8]) -> Vec<(u8, u8)> {
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
        .take(n.min(MAX_STARTING_ITEMS))
        .map(|id| (id, lo + rng.below(span) as u8))
        .collect()
}

/// Resolve [`StartingSeedOptions`] into a concrete [`SeedPlan`] for `seed`.
///
/// Composition, in slot order:
/// 1. **Forced convenience items** — Door of Wind then Incense (each `count`× ,
///    clamped to [`MAX_ITEM_STACK`]) for whichever has a non-zero count, written
///    first so they always survive the capacity clamp.
/// 2. **Base / reroll**: with `random_items == 0` the vanilla Healing Leaf base
///    is kept (the convenience toggles stay additive to a normal new game);
///    with `random_items > 0` the bag is rerolled to that many random
///    consumables instead (the existing `--starting-items` behaviour), drawn
///    excluding any forced item so it isn't duplicated.
///
/// The item list is clamped to [`MAX_STARTING_ITEMS`]. The all-warps preset
/// lives in its **own** code region (it doesn't share the inventory budget), so
/// it never reduces how many items fit. Deterministic in `(seed, opts)`.
pub fn plan_seed(seed: u64, opts: &StartingSeedOptions) -> SeedPlan {
    let cap = MAX_STARTING_ITEMS;
    let mut items: Vec<(u8, u8)> = Vec::new();
    // Forced convenience items, in a stable slot order. Each is seeded first so
    // it always survives the capacity clamp; its id is excluded from the reroll
    // below so a random fill can't deal a duplicate slot.
    let mut forced_ids: Vec<u8> = Vec::new();
    for (id, count) in [
        (DOOR_OF_WIND_ID, opts.door_of_wind),
        (INCENSE_ID, opts.incense),
    ] {
        let count = count.min(MAX_ITEM_STACK);
        if count > 0 {
            items.push((id, count));
            forced_ids.push(id);
        }
    }

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

/// Encode a [`SeedPlan`]'s inventory slots into the 40-byte inventory-seed patch
/// (the region at [`legaia_asset::new_game::STARTING_INV_SEED_VA`]).
///
/// Emits one `addiu $v0, $zero, (count << 8) | id` + `sh $v0, (0x1818 + 2k)($s0)`
/// pair per inventory slot, padded to [`STARTING_INV_SEED_LEN`] with `nop` (which
/// also overwrites the redundant zero-loop — required for the warp preset, see
/// [`build_warp_patch`]). Panics if the plan exceeds the `SEED_INSTRS`-instruction
/// budget (callers clamp via [`plan_seed`]). The `all_warps` flag is **not**
/// encoded here — the warp preset is a separate region ([`build_warp_patch`]) so
/// it never reduces the item capacity. The inventory base offset comes from
/// [`INVENTORY_SC_OFFSET`].
pub fn build_seed_patch_for(plan: &SeedPlan) -> [u8; STARTING_INV_SEED_LEN] {
    let mut words: Vec<u32> = Vec::with_capacity(SEED_INSTRS);
    for (slot, &(id, count)) in plan.items.iter().enumerate() {
        let off = (INVENTORY_SC_OFFSET as usize + slot * 2) as u16;
        words.push(addiu_v0(((count as u16) << 8) | id as u16));
        words.push(sh_v0_s0(off));
    }
    assert!(
        words.len() <= SEED_INSTRS,
        "seed plan needs {} instructions but only {SEED_INSTRS} fit the region",
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
    fn max_items_is_five() {
        assert_eq!(MAX_STARTING_ITEMS, 5);
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
    fn warps_do_not_reduce_the_item_capacity() {
        // The warp preset lives in its own region, so enabling it leaves the
        // full 5-slot item capacity intact.
        let with = plan_seed(
            7,
            &StartingSeedOptions {
                random_items: 5,
                door_of_wind: 0,
                incense: 0,
                all_warps: true,
            },
        );
        assert_eq!(with.items.len(), MAX_STARTING_ITEMS);
        assert_eq!(with.items.len(), 5);
        let without = plan_seed(
            7,
            &StartingSeedOptions {
                random_items: 5,
                door_of_wind: 0,
                incense: 0,
                all_warps: false,
            },
        );
        // Same items either way — warps don't perturb the inventory plan.
        assert_eq!(with.items, without.items);
    }

    #[test]
    fn door_of_wind_only_is_additive_to_the_vanilla_base() {
        // No reroll, no warps: keep Healing Leaf AND add Door of Wind x10.
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                random_items: 0,
                door_of_wind: DOOR_OF_WIND_COUNT,
                incense: 0,
                all_warps: false,
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
                random_items: 0,
                door_of_wind: 25,
                incense: 0,
                all_warps: false,
            },
        );
        assert_eq!(plan.items[0], (DOOR_OF_WIND_ID, 25));
        // A count above the stack cap clamps to MAX_ITEM_STACK.
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                random_items: 0,
                door_of_wind: 250,
                incense: 0,
                all_warps: false,
            },
        );
        assert_eq!(plan.items[0], (DOOR_OF_WIND_ID, MAX_ITEM_STACK));
        // Count 0 means off: no Door of Wind, just the vanilla base.
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                random_items: 0,
                door_of_wind: 0,
                incense: 0,
                all_warps: false,
            },
        );
        assert_eq!(plan.items, vec![VANILLA_STARTING_ITEM]);
        assert!(!plan.items.iter().any(|(id, _)| *id == DOOR_OF_WIND_ID));
    }

    #[test]
    fn all_warps_emits_the_bitmask_in_its_own_region() {
        let plan = plan_seed(
            1,
            &StartingSeedOptions {
                random_items: 0,
                door_of_wind: 0,
                incense: 0,
                all_warps: true,
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
        // Request 5 random + door of wind + warps: warps no longer steal item
        // budget, so all 5 slots fill (door takes one, 4 random fills).
        let opts = StartingSeedOptions {
            random_items: 5,
            door_of_wind: DOOR_OF_WIND_COUNT,
            incense: 0,
            all_warps: true,
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
                door_of_wind: 0,
                incense: 0,
                all_warps: false,
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
            incense: 0,
            all_warps: true,
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
    fn reroll_excludes_both_forced_items() {
        // A reroll alongside both forced items must not deal a duplicate of
        // either; all five slots fill (2 forced + 3 random).
        let plan = plan_seed(
            99,
            &StartingSeedOptions {
                random_items: 5,
                door_of_wind: DOOR_OF_WIND_COUNT,
                incense: INCENSE_COUNT,
                all_warps: false,
            },
        );
        assert_eq!(plan.items.len(), 5);
        assert_eq!(plan.items[0], (DOOR_OF_WIND_ID, DOOR_OF_WIND_COUNT));
        assert_eq!(plan.items[1], (INCENSE_ID, INCENSE_COUNT));
        for (id, _) in &plan.items[2..] {
            assert!(STARTING_ITEM_POOL.contains(id));
            assert_ne!(*id, DOOR_OF_WIND_ID, "reroll excludes Door of Wind");
            assert_ne!(*id, INCENSE_ID, "reroll excludes Incense");
        }
    }
}
