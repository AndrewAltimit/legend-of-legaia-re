//! The cast lure actor and its water probe.

use super::*;

// --- The cast lure (FUN_801CF3BC case 0x14 + FUN_801D26CC's probe pair) ----

/// Radius the cast lure spawns at, ahead of the angler along its facing
/// (`li a1,0xc8` feeding the polar helper at `0x801CFC50`).
pub const LURE_CAST_RADIUS: i32 = 200;

/// The venue anchor both fishing hosts stand the angler on.
///
/// This is port glue, not a retail constant: retail reads the angler actor's
/// own `+0x14`/`+0x18`, and neither host spawns a field actor for the angler.
/// The play window already seeds its wander actor and its tracked-point
/// readout from this pair, so the lure casts from the same place the rest of
/// the venue is measured against.
pub const VENUE_ANCHOR: (i16, i16) = (0x400, 0x400);

/// Shift the walk-grid probe pushes the lure's `x` accumulator by per frame
/// delta (`sll v0,v0,0xb` at `0x801D2E3C` / `0x801D2E50`), in the
/// accumulator's 24.8 fixed point - eight world units per frame delta.
pub const LURE_DRIFT_SHIFT: u32 = 11;

/// World units per `.MAP` tile - the shift both of the lure's probes take
/// their tile coordinate with (`sra a2,v1,0x17` over a sign-extended `i16`).
pub const LURE_TILE_SHIFT: u32 = 7;

/// The `+0x8000` cell word's **water** bit (`andi v1,v1,0x4000` at
/// `0x801D3374`).
///
/// This is a bit of the per-tile object-index halfword, not an offset: the
/// same word's low nine bits are the object index the bite tick keeps
/// (`andi s3,v0,0x1ff` at `0x801D2E14`). It is not the `+0x4000` walk grid
/// [`walk_grid_overhead`] reads - the two share only the hex digits.
pub const CELL_WATER_BIT: u16 = 0x4000;

/// What one lure frame resolved out of the venue map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LureProbe {
    /// The `+0x8000` cell word said this tile is water.
    pub water: bool,
    /// Strike-credit addend from the tile's water class (`s2`; `0` outside
    /// every class).
    pub countdown_bonus: i32,
    /// The class's fish weight (`s4`; [`WATER_TILE_DEFAULT`]'s `10` outside
    /// every class).
    pub weight: i32,
    /// This frame's walk-grid drift of the lure's `x`, in world units
    /// (signed; `0` when the probe found no overhead bit).
    pub drift: i32,
}

/// The cast lure as an advancing object.
///
/// Retail spawns it in the cast arm of the fishing SM (`FUN_801CF3BC` case
/// `0x14`): the angler actor's `xz` minus the polar offset of its `+0x26`
/// facing at radius [`LURE_CAST_RADIUS`], latched into the tracked-point
/// pair at `0x801D918C` and into the 24.8 `x` accumulator at `0x801D9174`.
/// The bite tick `FUN_801D26CC` then probes the tile under it every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LureActor {
    /// `0x801D9174` - the lure's world `x` in 24.8 fixed point, the form the
    /// drift is applied in.
    pub x_fixed: i32,
    /// `0x801D9190` - the lure's world `z`, the tracked triple's third
    /// halfword. `0x801D918E`, between it and the `x`, is the lure's
    /// **height** (`actor + 0x16` less `0x80`, stored by `sh $a0, 2($a2)`
    /// at `0x801CFCEC`) - the same `+0x14 / +0x16 / +0x18` = x / footing /
    /// z layout the field player record uses. The 24.8 accumulator for this
    /// `z` is `0x801D917C`, the third of the `<< 8` triple.
    pub z: i16,
}

impl LureActor {
    /// Spawn the lure for a cast from `(anchor_x, anchor_z)` facing `facing`.
    ///
    /// `frame_step` is the scratchpad frame delta (`0x1F800393`), which retail
    /// passes as the polar helper's **scale** on this arm as well as on the
    /// per-frame ones - so a 60 Hz frame (`1`) puts the lure exactly
    /// [`LURE_CAST_RADIUS`] units out - and the spawn is a subtraction:
    /// `lure.xz = anchor.xz - polar(facing, 200)` (`subu v1,v1,v0` at
    /// `0x801CFCD4`).
    ///
    /// Returns `None` only when the quadrature tables are too short for the
    /// masked angle, which the materialised pair never is.
    ///
    /// PORT: FUN_801cf3bc (case 0x14 - the cast lure spawn)
    pub fn cast(anchor_x: i16, anchor_z: i16, facing: i16, frame_step: i32) -> Option<Self> {
        let (sin, cos) = legaia_engine_field::minigame_floor::polar_tables();
        let (dx, dz) = legaia_engine_field::minigame_floor::polar_offset(
            facing as u32,
            LURE_CAST_RADIUS,
            frame_step.max(1),
            sin,
            cos,
        )?;
        let x = (anchor_x as i32 - dx) as i16;
        let z = (anchor_z as i32 - dz) as i16;
        Some(Self {
            x_fixed: (x as i32) << 8,
            z,
        })
    }

    /// The lure's world `x` (the accumulator's integer part).
    pub fn x(&self) -> i16 {
        (self.x_fixed >> 8) as i16
    }

    /// Probe the venue map under the lure and apply this frame's drift.
    ///
    /// `map` is the scene's `.MAP` buffer (`*_DAT_1F8003EC`), `region` its
    /// parsed `+0x10000` region table, `cast_counter` the persistent lifetime
    /// cast count (`_DAT_80084460`, whose low bit picks the drift's sign at
    /// `0x801D2E28`) and `frame_step` the scratchpad frame delta.
    ///
    /// Two independent reads, in retail's order:
    ///
    /// 1. [`walk_grid_overhead`] over the `+0x4000` grid's **high** nibble -
    ///    a hit drifts the `x` accumulator by `frame_step << 11`.
    /// 2. the `+0x8000` cell word's [`CELL_WATER_BIT`]; on water, the region
    ///    walk ([`legaia_engine_field::field_regions::refresh_region_attributes`] - retail's
    ///    `FUN_800180EC`, called at `0x801D3384`, writing `_DAT_8007B8F4`)
    ///    yields the region-kind mask [`water_tile_class`] classifies.
    pub fn probe(
        &mut self,
        map: &[u8],
        region: Option<&legaia_engine_field::field_regions::RegionTable<'_>>,
        cast_counter: i32,
        frame_step: i32,
    ) -> LureProbe {
        let fs = frame_step.max(1);
        let x = self.x();
        let mut out = LureProbe {
            weight: WATER_TILE_DEFAULT.1,
            ..Default::default()
        };
        // `walk_grid_overhead` indexes from the grid's own base, so it takes
        // the `+0x4000` block rather than the whole map.
        let walk = map
            .get(legaia_engine_field::field_regions::MAP_WALK_GRID_OFFSET..)
            .unwrap_or(&[]);
        if walk_grid_overhead(walk, x as i32, self.z as i32) {
            let push = fs << LURE_DRIFT_SHIFT;
            let signed = if cast_counter & 1 != 0 { push } else { -push };
            self.x_fixed = self.x_fixed.saturating_add(signed);
            out.drift = signed >> 8;
        }
        let tile_x = (x as i32) >> LURE_TILE_SHIFT;
        let tile_z = (self.z as i32) >> LURE_TILE_SHIFT;
        let grid = legaia_engine_field::minigame_floor::FloorGrid::new(map);
        if grid.cell(tile_x, tile_z) & CELL_WATER_BIT == 0 {
            return out;
        }
        out.water = true;
        let (mask, _attrs) = legaia_engine_field::field_regions::refresh_region_attributes(
            region, tile_x, tile_z, false,
        );
        if let Some((bonus, weight)) = water_tile_class(mask) {
            out.countdown_bonus = bonus;
            out.weight = weight;
        }
        out
    }
}
