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
//! 2. The scene's region table is walked **in order**; the first region whose
//!    AABB contains the tile (`x_min <= tx <= x_max && z_min <= tz <= z_max`)
//!    is selected (`0x801d9fe8..0x801da050`).
//! 3. The region's per-step **rate increment** (`region[+4]`) is scaled by the
//!    user encounter-rate setting (`_DAT_8007B5F8`; `0x801da198..0x801da1b4`)
//!    and subtracted from the step counter (`_DAT_8007B5FC`,
//!    `0x801da20c..0x801da21c`). While the counter stays positive, nothing
//!    fires.
//! 4. When the counter drops to `<= 0`, a formation id is rolled uniformly from
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
//! setting scale (`FUN_800431D0(0x3b/0x3c)`, `FUN_8003CE64(0x1d/0x1e)`) have no
//! engine consumer yet and are deliberately not ported here (the additive
//! equipment bias lives on [`crate::encounter::EncounterTracker`]).
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

/// Per-scene region-keyed encounter table.
#[derive(Clone, Debug, Default)]
pub struct RegionEncounterTable {
    pub scene_label: String,
    pub regions: Vec<EncounterRegion>,
}

impl RegionEncounterTable {
    pub fn new(scene_label: impl Into<String>) -> Self {
        Self {
            scene_label: scene_label.into(),
            regions: Vec::new(),
        }
    }

    /// Reduce a world coordinate to its 128-unit tile (`coord >> 7`,
    /// arithmetic so negatives floor toward `-inf`, matching the retail
    /// `sra ...,0x17` on the sign-extended halfword).
    pub fn world_to_tile(coord: i16) -> i32 {
        (coord as i32) >> 7
    }

    /// The first region whose AABB contains tile `(tile_x, tile_z)`, or `None`
    /// when the player stands outside every region (the retail walk that runs
    /// off the end of the table without a hit).
    pub fn region_at_tile(&self, tile_x: i32, tile_z: i32) -> Option<&EncounterRegion> {
        self.regions
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
}

/// Build a [`RegionEncounterTable`] from a scene's decoded MAN bytes.
///
/// Returns `None` when the MAN header / encounter section fails to parse or the
/// section declares no regions. Companion to
/// [`crate::encounter_man::encounter_table_from_man`] (which aggregates the
/// same regions into a single weighted table); this one keeps the geometry so a
/// position-routed engine can roll against the active region.
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
}

impl RegionEncounterTracker {
    pub fn new(table: RegionEncounterTable) -> Self {
        Self {
            table,
            counter: ENCOUNTER_COUNTER_BASE,
            setting: EncounterRateSetting::default(),
            last_formation: None,
            suppressed: false,
        }
    }

    pub fn table(&self) -> &RegionEncounterTable {
        &self.table
    }

    pub fn setting(&self) -> EncounterRateSetting {
        self.setting
    }

    pub fn set_setting(&mut self, setting: EncounterRateSetting) {
        self.setting = setting;
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
        let inc = self.setting.scale(region.rate_increment) as i32;
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
