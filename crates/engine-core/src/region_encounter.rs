//! Region-keyed random-encounter trigger - clean-room port of `FUN_801D9E1C`.
//!
//! This is the faithful overworld / field random-encounter model, distinct
//! from the aggregated weighted-row [`crate::encounter::EncounterTable`]: it
//! preserves the per-region geometry so a moving player rolls against the
//! region it is actually standing in.
//!
//! ## Mechanism (from the disassembly)
//!
//! The reader runs once per movement update:
//!
//! 1. The player's world `(x, z)` is reduced to a **128-unit tile** by an
//!    arithmetic `>> 7` (`worldX >> 7`, `worldZ >> 7`) - region AABBs are in
//!    tile units (`0x801d9e94..0x801d9ec0`).
//! 2. The scene's **condition** table is walked to pick which slice of the
//!    region array is live (`0x801d9f30..0x801d9fd8`). See [`RegionGroup`].
//!    Only that slice is searched next; the rest of the region array belongs
//!    to other story states and is invisible.
//! 3. The selected group is walked **in order**; the first region whose
//!    AABB contains the tile (`x_min <= tx <= x_max && z_min <= tz <= z_max`)
//!    is selected (`0x801d9fe8..0x801da050`).
//! 4. The region's per-step **rate increment** (`region[+4]`) is scaled by the
//!    user encounter-rate setting (`_DAT_8007B5F8`; `0x801da198..0x801da1b4`)
//!    and subtracted from the step counter (`_DAT_8007B5FC`,
//!    `0x801da20c..0x801da21c`). While the counter stays positive, nothing
//!    fires.
//! 5. When the counter drops to `<= 0`, a formation id is rolled uniformly from
//!    the region's `[base, base + count)` slice (`base = region[+6]`,
//!    `count = region[+7]`; `0x801da228..0x801da268`) with a one-step
//!    anti-repeat (if the pick equals the previous formation, advance one and
//!    wrap; `0x801da26c..0x801da290`), then the counter resets to
//!    `0x3ce + (rng_a % 0x1e7) - (rng_b % 0x1e7)` (range `[0x1e8, 0x5b4]`;
//!    `0x801da2dc..0x801da358`).
//!
//! Two RNG draws drive the counter reset and one drives the formation pick;
//! the no-trigger path consumes **zero** RNG (matching retail, which only calls
//! the RNG advance `FUN_80056798` on the trigger branch), so feeding this from
//! the world's shared deterministic RNG keeps replays bit-identical.
//!
//! The accessory / status multiplicative modifiers retail layers on top of the
//! setting scale are ported as [`EncounterRateModifiers`] (`FUN_800431D0`
//! ability bits `0x3B`/`0x3C` and `FUN_8003CE64` system flags `0x1D`/`0x1E`,
//! shifts `<<2`/`>>1`/`<<1`/`>>1` in retail order) and refreshed from the
//! party ability mask + flag bank each step.
//!
//! ## Consumers
//!
//! Both scene modes route this tracker off the scene's MAN encounter section
//! ([`region_encounter_table_from_man`]):
//!
//! - **Overworld** - [`crate::world::World::set_world_map_regions`]; the per-tile
//!   roll lives in `World::live_world_map_tick`, latching
//!   `World::pending_world_map_encounter`.
//! - **Field** - [`crate::world::World::set_field_regions`]; the roll lives in
//!   [`crate::world::World::on_field_step`], which drives a trigger through the
//!   mean-rate [`crate::encounter::EncounterSession`]'s transition / grace SM via
//!   [`crate::encounter::EncounterSession::trigger_with`]. A field scene whose MAN
//!   has no encounter-region section keeps the aggregated mean-rate session.
//!
//! Source: `ghidra/scripts/funcs/overlay_world_map_walk_801d9e1c.txt` +
//! [`docs/formats/encounter.md`](../../../docs/formats/encounter.md).
//!
//! PORT: FUN_801D9E1C
//! REF: FUN_80056798, FUN_800431D0, FUN_8003CE64

use legaia_asset::man_section;

/// Counter base term (`0x3ce` = 974) - the reset's centre value.
pub const ENCOUNTER_COUNTER_BASE: i32 = 0x3ce;
/// Counter reset modulus (`0x1e7` = 487) applied to each of the two RNG draws.
pub const ENCOUNTER_COUNTER_MOD: u32 = 0x1e7;

/// One scene encounter region: an AABB in 128-unit tiles, a per-step rate
/// increment, and the formation slice it rolls into.
///
/// Mirrors [`man_section::RegionRecord`]'s `+0x00..+0x08` prefix; the region's
/// `y` AABB fields are tested against the player's **Z** tile (`worldZ >> 7`),
/// matching the disassembly's `s3` register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncounterRegion {
    pub tile_x_min: u8,
    pub tile_z_min: u8,
    pub tile_x_max: u8,
    pub tile_z_max: u8,
    /// Per-step rate increment (`region[+4]`).
    pub rate_increment: u8,
    /// First formation index this region rolls into (`region[+6]`).
    pub formation_base: u8,
    /// Number of formations in the roll range (`region[+7]`).
    pub formation_count: u8,
}

impl EncounterRegion {
    /// `true` if the 128-unit tile `(tile_x, tile_z)` is inside this region's
    /// AABB. Tiles are signed (`worldX >> 7` can be negative on the overworld);
    /// the bounds are unsigned bytes, so the comparison widens to `i32` exactly
    /// as the retail `slt` does against the byte-loaded bounds.
    pub fn contains_tile(&self, tile_x: i32, tile_z: i32) -> bool {
        tile_x >= self.tile_x_min as i32
            && tile_x <= self.tile_x_max as i32
            && tile_z >= self.tile_z_min as i32
            && tile_z <= self.tile_z_max as i32
    }
}

/// User encounter-rate setting (`_DAT_8007B5F8`; the world-map debug `ENCOUNT`
/// row cycles it). The numeric value is the retail global; [`Self::scale`]
/// ports the exact per-setting arithmetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EncounterRateSetting {
    /// `0` - encounters off.
    Off,
    /// `1` - rate increment used as-is.
    Low,
    /// `2` - rate increment `<< 2` (the shipped default).
    #[default]
    Normal,
    /// `3` - rate increment `>> 2`.
    High,
}

impl EncounterRateSetting {
    /// The retail global value (`_DAT_8007B5F8`).
    pub fn as_u8(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Low => 1,
            Self::Normal => 2,
            Self::High => 3,
        }
    }

    /// Build from the retail global value; out-of-range falls back to `Normal`.
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Off,
            1 => Self::Low,
            3 => Self::High,
            _ => Self::Normal,
        }
    }

    /// Scale a region's per-step rate increment, porting
    /// `0x801da198..0x801da1b4`: setting `2` shifts left 2 (`× 4`), setting `3`
    /// shifts right 2 (`÷ 4`), settings `0`/`1` leave it unchanged. `Off`
    /// zeroes the increment so the counter never advances.
    pub fn scale(self, increment: u8) -> u32 {
        let inc = increment as u32;
        match self {
            Self::Off => 0,
            Self::Low => inc,
            Self::Normal => inc << 2,
            Self::High => inc >> 2,
        }
    }
}

/// Accessory / status encounter-rate modifiers, applied to the setting-scaled
/// per-step rate increment in retail order (`FUN_801D9E1C`
/// `0x801da1b8..0x801da200` - four sequential shifts on the same value):
///
/// | Source | Retail test | Effect |
/// |---|---|---|
/// | High Encounter passive (Bad Luck Bell / Nemesis Gem) | `FUN_800431D0(0x3B)` | `<< 2` |
/// | Low Encounter passive (Good Luck Bell / Evil Talisman) | `FUN_800431D0(0x3C)` | `>> 1` |
/// | System flag `0x1D` | `FUN_8003CE64(0x1D)` | `<< 1` |
/// | System flag `0x1E` | `FUN_8003CE64(0x1E)` | `>> 1` |
///
/// The magnitudes are statically pinned in `overlay_world_map_801d9e1c.txt`
/// (no capture needed). The engine refreshes these from the party ability
/// mask + system-flag bank each step
/// ([`crate::world::World::encounter_rate_modifiers`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EncounterRateModifiers {
    /// Ability bit `0x3B` set on any member - rate `<< 2`.
    pub high_encounter: bool,
    /// Ability bit `0x3C` set on any member - rate `>> 1`.
    pub low_encounter: bool,
    /// System flag `0x1D` set - rate `<< 1`.
    pub flag_high: bool,
    /// System flag `0x1E` set - rate `>> 1`.
    pub flag_low: bool,
}

impl EncounterRateModifiers {
    /// Apply the four shifts in retail order to a setting-scaled rate.
    // PORT: FUN_801D9E1C (accessory/status rate shifts, 0x801da1b8..0x801da200)
    pub fn apply(self, rate: u32) -> u32 {
        let mut r = rate;
        if self.high_encounter {
            r <<= 2;
        }
        if self.low_encounter {
            r >>= 1;
        }
        if self.flag_high {
            r <<= 1;
        }
        if self.flag_low {
            r >>= 1;
        }
        r
    }

    /// `true` when no modifier is active (the default state).
    pub fn is_neutral(self) -> bool {
        self == Self::default()
    }
}

/// One story-flag-gated slice of a scene's region array.
///
/// A scene's regions are not one list - they are several **variants of the
/// same scene**, one per story state, laid end to end, and the MAN's
/// condition array says where each starts. `FUN_801D9E1C` walks the
/// conditions in order and takes the first whose flag is set (or the
/// `0xFFFF` unconditional tail); the regions of every other group are
/// unreachable in that state.
///
/// Reading the array flat instead is what makes most scenes look
/// encounter-free: a group's rows commonly begin with (or consist of) a
/// whole-map `rate 0` row, and a flat first-match lookup stops there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionGroup {
    /// Story flag gating this group, or [`DEFAULT_GROUP_FLAG`] for the
    /// unconditional tail every retail scene ends with.
    pub flag_id: u16,
    /// Index of the group's first region in [`RegionEncounterTable::regions`].
    pub start: usize,
    /// Number of regions in the group.
    pub len: usize,
}

/// The condition flag id that means "no flag test - this group always wins"
/// (`FUN_801D9E1C` `0x801d9f50`).
pub const DEFAULT_GROUP_FLAG: u16 = legaia_asset::man_section::CONDITION_DEFAULT_FLAG;

/// Per-scene region-keyed encounter table.
///
/// `regions` holds every authored region across every story variant;
/// `groups` says which slice belongs to which story flag, and `active` is
/// the slice the roll currently searches. Refresh `active` from the live
/// flag bank with [`Self::select_group`] before each step - retail
/// re-evaluates the condition walk every step, so a flag set mid-scene
/// changes the region set immediately.
#[derive(Clone, Debug, Default)]
pub struct RegionEncounterTable {
    pub scene_label: String,
    pub regions: Vec<EncounterRegion>,
    /// The condition partition. Empty for hand-built (synthetic) tables,
    /// which behave as one implicit group covering every region.
    pub groups: Vec<RegionGroup>,
    /// Currently selected `(start, len)` slice of `regions`. `None` means
    /// the condition walk found no group - retail's "return without
    /// rolling" exit, which is emphatically not the same as "search
    /// everything".
    active: Option<(usize, usize)>,
}

impl RegionEncounterTable {
    pub fn new(scene_label: impl Into<String>) -> Self {
        Self {
            scene_label: scene_label.into(),
            regions: Vec::new(),
            groups: Vec::new(),
            active: None,
        }
    }

    /// Re-run the condition walk against the live story-flag bank and latch
    /// the group it selects.
    ///
    /// `flag_test(flag_id)` answers the `FUN_8003CE64` question. A table with
    /// no `groups` (a synthetic one) keeps every region active.
    // PORT: FUN_801D9E1C (condition walk, 0x801d9f30..0x801d9fd8)
    pub fn select_group(&mut self, mut flag_test: impl FnMut(u16) -> bool) {
        if self.groups.is_empty() {
            self.active = Some((0, self.regions.len()));
            return;
        }
        self.active = self
            .groups
            .iter()
            .find(|g| g.flag_id == DEFAULT_GROUP_FLAG || flag_test(g.flag_id))
            .map(|g| (g.start, g.len));
    }

    /// The region slice the roll currently searches (see [`Self::select_group`]).
    pub fn active_regions(&self) -> &[EncounterRegion] {
        match self.active {
            Some((start, len)) => {
                let end = start.saturating_add(len).min(self.regions.len());
                let start = start.min(end);
                &self.regions[start..end]
            }
            // A table built by hand (tests, synthetic worlds) never had a
            // condition walk run over it; treat it as all-active rather than
            // silently dead.
            None if self.groups.is_empty() => &self.regions,
            None => &[],
        }
    }

    /// The selected group's descriptor, when the table carries a condition
    /// partition and the walk matched one.
    pub fn active_group(&self) -> Option<RegionGroup> {
        let (start, len) = self.active?;
        self.groups
            .iter()
            .copied()
            .find(|g| g.start == start && g.len == len)
    }

    /// Reduce a world coordinate to its 128-unit tile (`coord >> 7`,
    /// arithmetic so negatives floor toward `-inf`, matching the retail
    /// `sra ...,0x17` on the sign-extended halfword).
    pub fn world_to_tile(coord: i16) -> i32 {
        (coord as i32) >> 7
    }

    /// The first region **of the active group** whose AABB contains tile
    /// `(tile_x, tile_z)`, or `None` when the player stands outside every one
    /// of them (the retail walk that runs off the end without a hit).
    pub fn region_at_tile(&self, tile_x: i32, tile_z: i32) -> Option<&EncounterRegion> {
        self.active_regions()
            .iter()
            .find(|r| r.contains_tile(tile_x, tile_z))
    }

    /// [`Self::region_at_tile`] from a world `(x, z)`.
    pub fn region_at_world(&self, world_x: i16, world_z: i16) -> Option<&EncounterRegion> {
        self.region_at_tile(Self::world_to_tile(world_x), Self::world_to_tile(world_z))
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// `true` when at least one region **of the active group** that can
    /// actually produce a battle (non-zero rate increment **and** a non-empty
    /// formation range) owns at least one tile it is the *first* match for.
    ///
    /// Two qualifiers, both load-bearing:
    ///
    /// - *Active group*: rollability is a property of the (scene, story
    ///   state) pair, not of the scene. A town is silent until its "under
    ///   attack" flag flips the region set; a dungeon can be silent before
    ///   the story opens it. Call [`Self::select_group`] first.
    /// - *First match*: [`Self::region_at_tile`] stops at the first containing
    ///   region, exactly like retail's walk, so a rollable region whose every
    ///   tile is already covered by an earlier rate-0 row in the same group is
    ///   unreachable - the scan below shadows regions in group order the same
    ///   way the lookup does. Groups routinely end with a whole-map rate-0
    ///   catch-all, which is the "outside every named region, roll nothing"
    ///   row and correctly shadows nothing before it.
    ///
    /// Cost is one pass over the union of the group's region AABBs against a
    /// 256x256 claim map, so call it on scene entry (or cache it), not per
    /// frame.
    pub fn any_rollable(&self) -> bool {
        let mut claimed = vec![false; 256 * 256];
        for r in self.active_regions() {
            let mut owns_a_tile = false;
            for tz in r.tile_z_min..=r.tile_z_max {
                for tx in r.tile_x_min..=r.tile_x_max {
                    let idx = tz as usize * 256 + tx as usize;
                    if !claimed[idx] {
                        claimed[idx] = true;
                        owns_a_tile = true;
                    }
                }
            }
            if owns_a_tile && r.rate_increment > 0 && r.formation_count > 0 {
                return true;
            }
        }
        false
    }
}

/// Build a [`RegionEncounterTable`] from a scene's decoded MAN bytes.
///
/// Returns `None` when the MAN header / encounter section fails to parse or the
/// section declares no regions. Companion to
/// [`crate::encounter_man::encounter_table_from_man`] (which aggregates the
/// same regions into a single weighted table); this one keeps the geometry so a
/// position-routed engine can roll against the active region.
///
/// Every region variant the scene authors is kept, and the condition array is
/// carried alongside as [`RegionEncounterTable::groups`]. The table is left
/// resolved for the **all-flags-clear** state; a host with a live flag bank
/// should call [`RegionEncounterTable::select_group`] (or
/// [`RegionEncounterTracker::select_group`]) per step.
pub fn region_encounter_table_from_man(
    scene_label: &str,
    man_bytes: &[u8],
) -> Option<RegionEncounterTable> {
    let man = man_section::parse(man_bytes).ok()?;
    let body = man.encounter_section_body(man_bytes)?;
    let es = man_section::parse_encounter_section(body).ok()?;

    let mut table = RegionEncounterTable::new(scene_label);
    for region in man_section::region_records(body, &es).flatten() {
        table.regions.push(EncounterRegion {
            tile_x_min: region.aabb_x_min,
            tile_z_min: region.aabb_y_min,
            tile_x_max: region.aabb_x_max,
            tile_z_max: region.aabb_y_max,
            rate_increment: region.rate_increment,
            formation_base: region.formation_range_base,
            formation_count: region.formation_range_count,
        });
    }

    if table.regions.is_empty() {
        return None;
    }

    let mut start = 0usize;
    for cond in man_section::condition_records(body, &es).flatten() {
        let len = cond.region_count as usize;
        table.groups.push(RegionGroup {
            flag_id: cond.flag_id,
            start,
            len,
        });
        start = start.saturating_add(len);
    }
    // Resolve for a cleared flag bank so a host that never refreshes still
    // gets the unconditional tail rather than group 0.
    table.select_group(|_| false);
    Some(table)
}

/// A successful region-encounter roll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionEncounterRoll {
    /// Picked formation index (a row in the scene's formation list).
    pub formation_id: u8,
}

/// Per-scene region-keyed encounter state (the `FUN_801D9E1C` step counter +
/// anti-repeat latch).
#[derive(Clone, Debug)]
pub struct RegionEncounterTracker {
    table: RegionEncounterTable,
    /// `_DAT_8007B5FC` - the step counter; a trigger fires when it reaches
    /// `<= 0`. Seeded to [`ENCOUNTER_COUNTER_BASE`] so the first encounter
    /// takes a believable number of steps even before the first reset.
    counter: i32,
    /// `_DAT_8007B5F8` user setting.
    setting: EncounterRateSetting,
    /// `_DAT_8007B605` - the previous formation id, for the one-step
    /// anti-repeat. `None` until the first trigger.
    last_formation: Option<u8>,
    /// Master suppression (post-battle grace / cutscene). When set, steps never
    /// advance the counter.
    suppressed: bool,
    /// Accessory / status rate modifiers, refreshed by the host each step.
    modifiers: EncounterRateModifiers,
}

impl RegionEncounterTracker {
    pub fn new(table: RegionEncounterTable) -> Self {
        Self {
            table,
            counter: ENCOUNTER_COUNTER_BASE,
            setting: EncounterRateSetting::default(),
            last_formation: None,
            suppressed: false,
            modifiers: EncounterRateModifiers::default(),
        }
    }

    pub fn table(&self) -> &RegionEncounterTable {
        &self.table
    }

    /// Re-run the scene's condition walk against the live story-flag bank.
    ///
    /// Retail evaluates this every step, so hosts refresh it alongside
    /// [`Self::set_modifiers`]: a flag set mid-scene swaps the region set
    /// (and therefore the rates, formations and battle backdrop) on the very
    /// next step.
    pub fn select_group(&mut self, flag_test: impl FnMut(u16) -> bool) {
        self.table.select_group(flag_test);
    }

    pub fn setting(&self) -> EncounterRateSetting {
        self.setting
    }

    pub fn set_setting(&mut self, setting: EncounterRateSetting) {
        self.setting = setting;
    }

    pub fn modifiers(&self) -> EncounterRateModifiers {
        self.modifiers
    }

    /// Install the current accessory / status rate modifiers (see
    /// [`EncounterRateModifiers`]); the host refreshes these before each
    /// step roll.
    pub fn set_modifiers(&mut self, modifiers: EncounterRateModifiers) {
        self.modifiers = modifiers;
    }

    pub fn suppress(&mut self) {
        self.suppressed = true;
    }

    pub fn clear_suppression(&mut self) {
        self.suppressed = false;
    }

    pub fn is_suppressed(&self) -> bool {
        self.suppressed
    }

    pub fn counter(&self) -> i32 {
        self.counter
    }

    /// Reset per-scene state (scene change). Re-seeds the counter and clears
    /// the anti-repeat latch.
    pub fn reset(&mut self) {
        self.counter = ENCOUNTER_COUNTER_BASE;
        self.last_formation = None;
        self.suppressed = false;
    }

    /// Advance one movement step at world `(world_x, world_z)`.
    ///
    /// `rng` is pulled only on the trigger branch (formation pick + the two
    /// counter-reset draws), so a non-triggering step consumes no RNG - the
    /// same property the retail roll has. Returns `Some` when a battle should
    /// start.
    pub fn on_step(
        &mut self,
        world_x: i16,
        world_z: i16,
        mut rng: impl FnMut() -> u32,
    ) -> Option<RegionEncounterRoll> {
        if self.suppressed || self.setting == EncounterRateSetting::Off {
            return None;
        }
        let region = *self.table.region_at_world(world_x, world_z)?;
        if region.formation_count == 0 {
            return None;
        }
        let inc = self
            .modifiers
            .apply(self.setting.scale(region.rate_increment)) as i32;
        // While the counter stays positive, the step does not fire and no RNG
        // is consumed (retail `bgtz v1, ...; sw v1, _DAT_8007B5FC`).
        if self.counter - inc > 0 {
            self.counter -= inc;
            return None;
        }

        // Trigger: pick a formation uniformly from the region's slice, with the
        // one-step anti-repeat.
        let count = region.formation_count;
        let pick = (rng() % count as u32) as u8;
        let mut formation_id = region.formation_base.wrapping_add(pick);
        if Some(formation_id) == self.last_formation {
            // Advance one and wrap to base at the slice end.
            let end = region.formation_base.wrapping_add(count);
            formation_id = formation_id.wrapping_add(1);
            if formation_id == end {
                formation_id = region.formation_base;
            }
        }
        self.last_formation = Some(formation_id);

        // Counter reset: 0x3ce + (rng_a % 0x1e7) - (rng_b % 0x1e7).
        let ra = (rng() % ENCOUNTER_COUNTER_MOD) as i32;
        let rb = (rng() % ENCOUNTER_COUNTER_MOD) as i32;
        self.counter = ENCOUNTER_COUNTER_BASE + ra - rb;

        Some(RegionEncounterRoll { formation_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(x0: u8, z0: u8, x1: u8, z1: u8, rate: u8, base: u8, count: u8) -> EncounterRegion {
        EncounterRegion {
            tile_x_min: x0,
            tile_z_min: z0,
            tile_x_max: x1,
            tile_z_max: z1,
            rate_increment: rate,
            formation_base: base,
            formation_count: count,
        }
    }

    #[test]
    fn world_to_tile_is_arithmetic_shift() {
        assert_eq!(RegionEncounterTable::world_to_tile(0), 0);
        assert_eq!(RegionEncounterTable::world_to_tile(127), 0);
        assert_eq!(RegionEncounterTable::world_to_tile(128), 1);
        assert_eq!(RegionEncounterTable::world_to_tile(256), 2);
        // Negative floors toward -inf (arithmetic shift), matching `sra`.
        assert_eq!(RegionEncounterTable::world_to_tile(-1), -1);
        assert_eq!(RegionEncounterTable::world_to_tile(-128), -1);
        assert_eq!(RegionEncounterTable::world_to_tile(-129), -2);
    }

    #[test]
    fn region_at_tile_first_match_wins() {
        let mut t = RegionEncounterTable::new("s");
        t.regions.push(region(0, 0, 4, 4, 8, 0, 2));
        t.regions.push(region(2, 2, 6, 6, 16, 2, 3)); // overlaps the first
        // (3,3) is in both; the walk takes the first.
        let r = t.region_at_tile(3, 3).unwrap();
        assert_eq!(r.rate_increment, 8);
        // (5,5) is only in the second.
        assert_eq!(t.region_at_tile(5, 5).unwrap().rate_increment, 16);
        // (9,9) is in neither.
        assert!(t.region_at_tile(9, 9).is_none());
    }

    /// A MAN whose section 0 carries `conditions` over `regions`, in the
    /// retail layout. `regions` are `(rate, x1, z1)` whole-corner boxes from
    /// the origin so the caller can tell them apart by rate alone.
    fn man_with_groups(conditions: &[(u16, u16)], regions: &[(u8, u8, u8)]) -> Vec<u8> {
        let mut body = vec![8u8, 4, 12, 1];
        body.extend_from_slice(&[0, 0, 0, 1, 4, 0, 0, 0]); // formation 0
        body.push(conditions.len() as u8);
        for (flag, n) in conditions {
            body.extend_from_slice(&flag.to_le_bytes());
            body.extend_from_slice(&n.to_le_bytes());
        }
        body.push(regions.len() as u8);
        for &(rate, x1, z1) in regions {
            body.extend_from_slice(&[0, 0, x1, z1, rate, 0, 0, 1, 0, 0, 0, 0]);
        }

        let mut buf = vec![0u8; 0x2B];
        let ln = body.len() as u32;
        buf.extend_from_slice(&[(ln & 0xFF) as u8, (ln >> 8) as u8, (ln >> 16) as u8]);
        buf.extend_from_slice(&body);
        for _ in 0..5 {
            buf.extend_from_slice(&[0, 0, 0]);
        }
        buf
    }

    #[test]
    fn man_table_carries_the_condition_partition() {
        let man = man_with_groups(
            &[(0x0141, 2), (0xFFFF, 1)],
            &[(0, 128, 128), (24, 40, 40), (18, 60, 60)],
        );
        let t = region_encounter_table_from_man("s", &man).expect("table");
        assert_eq!(t.regions.len(), 3, "every variant is preserved");
        assert_eq!(
            t.groups,
            vec![
                RegionGroup {
                    flag_id: 0x0141,
                    start: 0,
                    len: 2
                },
                RegionGroup {
                    flag_id: DEFAULT_GROUP_FLAG,
                    start: 2,
                    len: 1
                },
            ]
        );
        // Built resolved for a cleared flag bank: the default tail, not
        // group 0 - so the leading whole-map rate-0 row is invisible.
        assert_eq!(t.active_regions().len(), 1);
        assert_eq!(t.active_regions()[0].rate_increment, 18);
        assert!(t.any_rollable());
    }

    #[test]
    fn selecting_a_gated_group_swaps_the_live_region_set() {
        let man = man_with_groups(
            &[(0x0141, 2), (0xFFFF, 1)],
            &[(0, 128, 128), (24, 40, 40), (18, 60, 60)],
        );
        let mut t = region_encounter_table_from_man("s", &man).expect("table");
        // Default state: tile (10,10) rolls the trailing group's rate-18 row.
        assert_eq!(t.region_at_tile(10, 10).unwrap().rate_increment, 18);
        // Flag 0x0141 set: the gated group wins, and *its* first row is the
        // whole-map rate-0 one, which shadows the rate-24 row behind it.
        t.select_group(|f| f == 0x0141);
        assert_eq!(t.region_at_tile(10, 10).unwrap().rate_increment, 0);
        assert!(!t.any_rollable(), "the group's rate-0 row shadows the rest");
        assert_eq!(t.active_group().unwrap().flag_id, 0x0141);
    }

    #[test]
    fn a_walk_with_no_match_leaves_no_active_regions() {
        // No default sentinel: retail returns without rolling rather than
        // falling back to region 0.
        let man = man_with_groups(&[(0x0141, 1)], &[(24, 40, 40)]);
        let mut t = region_encounter_table_from_man("s", &man).expect("table");
        assert!(t.active_regions().is_empty());
        assert!(t.region_at_tile(10, 10).is_none());
        assert!(!t.any_rollable());
        t.select_group(|f| f == 0x0141);
        assert_eq!(t.active_regions().len(), 1);
        assert!(t.any_rollable());
    }

    #[test]
    fn a_hand_built_table_keeps_every_region_active() {
        // Synthetic tables (tests, synthetic worlds) never ran a condition
        // walk; they must not read as "all groups dead".
        let mut t = RegionEncounterTable::new("s");
        t.regions.push(region(0, 0, 8, 8, 16, 0, 2));
        assert_eq!(t.active_regions().len(), 1);
        assert!(t.region_at_tile(1, 1).is_some());
        t.select_group(|_| false);
        assert_eq!(t.active_regions().len(), 1);
    }

    #[test]
    fn rate_setting_scale_matches_disasm() {
        assert_eq!(EncounterRateSetting::Off.scale(10), 0);
        assert_eq!(EncounterRateSetting::Low.scale(10), 10);
        assert_eq!(EncounterRateSetting::Normal.scale(10), 40); // << 2
        assert_eq!(EncounterRateSetting::High.scale(10), 2); // >> 2
        assert_eq!(
            EncounterRateSetting::from_u8(2),
            EncounterRateSetting::Normal
        );
        assert_eq!(
            EncounterRateSetting::from_u8(99),
            EncounterRateSetting::Normal
        );
    }

    #[test]
    fn no_region_no_trigger_and_no_rng() {
        let mut t = RegionEncounterTable::new("s");
        t.regions.push(region(0, 0, 1, 1, 255, 0, 2));
        let mut tracker = RegionEncounterTracker::new(t);
        let mut draws = 0u32;
        // World (1000, 1000) -> tile (7, 7), outside the region.
        for _ in 0..100 {
            let r = tracker.on_step(1000, 1000, || {
                draws += 1;
                0
            });
            assert!(r.is_none());
        }
        assert_eq!(draws, 0, "no RNG consumed off-region");
    }

    #[test]
    fn counter_depletes_then_triggers() {
        let mut t = RegionEncounterTable::new("s");
        // One region covering tile (0,0), big rate so it depletes fast.
        t.regions.push(region(0, 0, 1, 1, 255, 5, 3));
        let mut tracker = RegionEncounterTracker::new(t);
        tracker.set_setting(EncounterRateSetting::Normal); // 255<<2 = 1020/step
        // Counter starts at 0x3ce (974); 974 - 1020 <= 0 -> first step triggers.
        let mut seq = [7u32, 100, 50].into_iter().cycle();
        let roll = tracker.on_step(0, 0, || seq.next().unwrap());
        let roll = roll.expect("triggers on the first step");
        // formation_id = base(5) + 7 % 3 = 5 + 1 = 6.
        assert_eq!(roll.formation_id, 6);
        // Counter reset to 0x3ce + 100%487 - 50%487 = 974 + 100 - 50 = 1024.
        assert_eq!(tracker.counter(), 974 + 100 - 50);
    }

    #[test]
    fn off_setting_never_triggers() {
        let mut t = RegionEncounterTable::new("s");
        t.regions.push(region(0, 0, 8, 8, 255, 0, 4));
        let mut tracker = RegionEncounterTracker::new(t);
        tracker.set_setting(EncounterRateSetting::Off);
        for _ in 0..10_000 {
            assert!(tracker.on_step(64, 64, || 0).is_none());
        }
    }

    #[test]
    fn suppression_blocks_trigger() {
        let mut t = RegionEncounterTable::new("s");
        t.regions.push(region(0, 0, 8, 8, 255, 0, 4));
        let mut tracker = RegionEncounterTracker::new(t);
        tracker.set_setting(EncounterRateSetting::Normal);
        tracker.suppress();
        for _ in 0..10_000 {
            assert!(tracker.on_step(64, 64, || 0).is_none());
        }
        tracker.clear_suppression();
        // Now a step in-region eventually fires.
        let mut fired = false;
        for _ in 0..10_000 {
            if tracker.on_step(64, 64, || 7).is_some() {
                fired = true;
                break;
            }
        }
        assert!(fired);
    }

    #[test]
    fn anti_repeat_advances_on_duplicate_pick() {
        let mut t = RegionEncounterTable::new("s");
        // base 10, count 4 -> ids 10..14. Big rate so every step triggers.
        t.regions.push(region(0, 0, 1, 1, 255, 10, 4));
        let mut tracker = RegionEncounterTracker::new(t);
        tracker.set_setting(EncounterRateSetting::Normal);
        // Force pick == 0 every time -> base 10. The reset draws don't matter
        // for the pick; keep them tiny so the counter stays <= 0 next step.
        // First trigger: 10. Second: pick 0 -> 10 == last -> bump to 11.
        let mut draws = [0u32, 0, 1000].into_iter().cycle();
        let first = tracker.on_step(0, 0, || draws.next().unwrap()).unwrap();
        assert_eq!(first.formation_id, 10);
        // Counter is now 974 + 0 - (1000%487=26) ... keep it triggering: set
        // counter low directly.
        tracker.counter = -1;
        let mut draws2 = [0u32, 0, 0].into_iter().cycle();
        let second = tracker.on_step(0, 0, || draws2.next().unwrap()).unwrap();
        assert_eq!(second.formation_id, 11, "duplicate pick advanced by one");
    }
}
