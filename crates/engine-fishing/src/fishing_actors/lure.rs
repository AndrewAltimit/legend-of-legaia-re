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
    /// `0x801D917C` - the `z` accumulator.
    pub z_fixed: i32,
    /// `0x801D9178` - the height accumulator.
    pub y_fixed: i32,
    /// The tracked point `0x801D9184..0x801D9188` as last frame's tail left it:
    /// the bearing and both tile probes read this, not the accumulators.
    pub tracked: (i16, i16, i16),
    /// The spawn point `0x801D918C` / `0x801D9190` the record measures from.
    pub spawn: (i16, i16),
    /// The cast power `0x801D9274`, spent as the lure flies.
    pub power: i32,
    /// The lure actor's `+0x3E`: the fall speed, `0x40` at the release.
    pub fall: i16,
    /// The reel pull `0x801D9148`.
    pub pull: i32,
    /// The rod sway `0x801D9140` and its target `0x801D9144`.
    pub sway: (i32, i32),
}

/// `(sin, cos) * radius * scale >> 12`, `FUN_801D7BB8`.
fn lure_polar(angle: i32, radius: i32, scale: i32) -> (i32, i32) {
    let (sin, cos) = legaia_engine_field::minigame_floor::polar_tables();
    legaia_engine_field::minigame_floor::polar_offset(angle as u32, radius, scale, sin, cos)
        .unwrap_or((0, 0))
}

/// `FUN_8005AF0C`: the SCUS root, normalised onto `FUN_8005ADB8`'s range, whose
/// hyperbolic rotation runs six shifts with `4` taken twice. The result carries
/// six fractional bits; the rotation's rounding is what the record's last unit
/// rests on, so a float root will not do.
///
/// PORT: FUN_8005AF0C
pub fn scus_sqrt(n: i32) -> i32 {
    if n == 0 {
        return 0;
    }
    // `FUN_8005BA1C` reads the GTE's `LZCR`: the run of leading bits equal
    // to the sign, so a negative input counts its leading ones.
    let lead = if n < 0 {
        (!n).leading_zeros()
    } else {
        n.leading_zeros()
    };
    let e = 8 - lead as i32;
    let (scale, arg) = if e >= 0 {
        let s = e >> 1;
        (s, n >> (s * 2))
    } else {
        let s = (e >> 1) + 1;
        (s, n.wrapping_shl((-2 * s) as u32))
    };
    let x = hyperbolic_root(arg);
    let shift = scale - 6;
    if shift >= 0 { x << shift } else { x >> -shift }
}

/// `FUN_8005ADB8`: the hyperbolic rotation [`scus_sqrt`] runs on its
/// normalised argument, from `(arg + 0x5D50AD, arg - 0x5D50AD)` through
/// shifts `1..=6` with `4` taken twice. Returns the `x` it leaves.
///
/// PORT: FUN_8005ADB8
pub fn hyperbolic_root(arg: i32) -> i32 {
    let (mut x, mut y) = (arg.wrapping_add(0x5D_50AD), arg.wrapping_sub(0x5D_50AD));
    let mut turn = |shift: i32| {
        let (dx, dy) = (y >> shift, x >> shift);
        if y < 0 {
            x = x.wrapping_add(dx);
            y = y.wrapping_add(dy);
        } else {
            x = x.wrapping_sub(dx);
            y = y.wrapping_sub(dy);
        }
    };
    for shift in 1..=6 {
        turn(shift);
        if shift == 4 {
            turn(shift);
        }
    }
    x
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
    pub fn cast(
        anchor_x: i16,
        anchor_y: i16,
        anchor_z: i16,
        facing: i16,
        power: i32,
        frame_step: i32,
    ) -> Option<Self> {
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
        let height = anchor_y.wrapping_sub(0x80);
        Some(Self {
            x_fixed: (x as i32) << 8,
            z,
            z_fixed: (z as i32) << 8,
            y_fixed: (height as i32) << 8,
            tracked: (x, height, z),
            spawn: (x, z),
            power,
            fall: 0x40,
            pull: 0,
            sway: (0, 0),
        })
    }

    /// One frame of flight: the lure goes out along the facing on what is left
    /// of the power, and falls. A lure below the water plane spends the power
    /// faster, and one reaching height `0xC8` spends it all. Returns `true` on
    /// the frame the power runs out, the landing.
    ///
    /// PORT: FUN_801d26cc (`0x801D279C..0x801D2944`)
    pub fn flight_step(&mut self, facing: i16, fs: i32) -> bool {
        // `power * 5 << 7`, truncated to `/ 0x1000`, negated.
        let radius = -(self.power.wrapping_mul(5).wrapping_shl(7) / 0x1000);
        let (dx, dz) = lure_polar(i32::from(facing), radius, fs);
        self.x_fixed = self.x_fixed.wrapping_add(dx.wrapping_mul(26));
        self.z_fixed = self.z_fixed.wrapping_add(dz.wrapping_mul(26));
        self.y_fixed = self
            .y_fixed
            .wrapping_add((i32::from(self.fall) / 10).wrapping_mul(fs).wrapping_shl(8));
        self.fall = self.fall.wrapping_add((fs * 3) as i16);
        if self.tracked.1 > 0 {
            self.power -= fs * 120;
            if self.y_fixed > 0xC800 {
                self.y_fixed = 0xC800;
                self.power = 0;
            }
        }
        self.power -= fs * 50;
        if self.power < 0 {
            self.power = 0;
            return true;
        }
        false
    }

    /// One in-water frame's move: the pull along `heading`, the sway across the
    /// facing while pulling, and (hooked) half the fish's swim. Close in, the
    /// sway and the swim fade with the record: `(record - 300) * 8` and
    /// `* 16`, from `0x1000`.
    ///
    /// PORT: FUN_801d26cc (`0x801D2C3C..0x801D2DC8`)
    pub fn water_step(
        &mut self,
        heading: u16,
        facing: i16,
        record: i32,
        swim: Option<(i32, i32)>,
        fs: i32,
    ) {
        let (px, pz) = lure_polar(i32::from(heading), -0x900, fs);
        let mut near = 0x1000;
        if record < 0x32C {
            near = (record - 300) * 8;
        }
        near = near.max(0);
        let radius = (-self.sway.0).wrapping_mul(near) / 0x1000;
        let (mut ax, mut az) = lure_polar(i32::from(facing) + 0x400, radius, fs);
        if record < 0x22C {
            near = (record - 300) * 16;
        }
        near = near.max(0);
        if self.pull == 0 {
            (ax, az) = (0, 0);
        }
        let (sx, sz) = swim.unwrap_or((0, 0));
        self.x_fixed = self.x_fixed.wrapping_add(
            px.wrapping_mul(self.pull) / 0x1000 + ax + ((sx / 2).wrapping_mul(near) / 0x1000),
        );
        self.z_fixed = self.z_fixed.wrapping_add(
            pz.wrapping_mul(self.pull) / 0x1000 + az + ((sz / 2).wrapping_mul(near) / 0x1000),
        );
    }

    /// The reel pull: a press on an idle reel starts it at `0xA00` (Cross) or
    /// `0xC00` (Square, or both); it falls `0x80` a frame-step and drops to
    /// nothing under `0x600`, so line comes in by the press.
    ///
    /// PORT: FUN_801d26cc (`0x801D2E60..0x801D2F40`)
    pub fn pull_step(&mut self, edge: u32, fs: i32) {
        if edge & 0xC0 != 0 && self.pull == 0 {
            self.pull = if edge & 0x80 != 0 { 0xC00 } else { 0xA00 };
        }
        self.pull -= fs << 7;
        if self.pull < 0x600 {
            self.pull = 0;
        }
    }

    /// The lure's sink, after the reel pull: close in the height is capped by
    /// the record, further out a Light or Normal lure rises under the pull and
    /// snaps back to its depth when it reaches the surface band; a Heavy lure
    /// holds. The height never goes negative. `lure` is `_DAT_80084450`.
    ///
    /// PORT: FUN_801d26cc (`0x801D2F44..0x801D3080`)
    pub fn sink_step(&mut self, record: i32, lure: u32, fs: i32) {
        if record < 0x190 {
            self.y_fixed = self.y_fixed.min((record - 0xFA) << 8);
        } else {
            let (factor, floor, reset) = match lure {
                0 => (0x600, 0x1400, 0x1900),
                1 => (0x500, 0x5000, 0x5A00),
                _ => (0, i32::MIN, 0),
            };
            if factor != 0 {
                let mut scaled = self.pull.wrapping_mul(factor);
                if scaled < 0 {
                    scaled += 0x1FFF;
                }
                self.y_fixed -= (scaled >> 13).wrapping_mul(fs);
                if self.y_fixed < floor {
                    self.y_fixed = reset;
                }
            }
        }
        self.y_fixed = self.y_fixed.max(0);
    }

    /// The rod sway: the D-pad sets the target to `+-0x100`, which otherwise
    /// falls back `4` a frame-step; the sway follows it at `0x10`.
    ///
    /// PORT: FUN_801d26cc (`0x801D3830..0x801D3958`)
    pub fn sway_step(&mut self, held: u32, fs: i32) {
        let (ref mut sway, ref mut target) = self.sway;
        if *target > 0 {
            *target = (*target - (fs << 2)).max(0);
        } else if *target < 0 {
            *target = (*target + (fs << 2)).min(0);
        }
        if held & 0x2000 != 0 {
            *target = 0x100;
        }
        if held & 0x8000 != 0 {
            *target = -0x100;
        }
        if *sway < *target {
            *sway = (*sway + (fs << 4)).min(*target);
        } else if *target < *sway {
            *sway = (*sway - (fs << 4)).max(*target);
        }
    }

    /// The frame's tail: the tracked point takes the accumulators, and the
    /// record is its distance from the spawn point.
    ///
    /// PORT: FUN_801d26cc (`0x801D39C8..0x801D3A40`)
    pub fn tail(&mut self) -> i32 {
        self.tracked = (
            (self.x_fixed >> 8) as i16,
            (self.y_fixed >> 8) as i16,
            (self.z_fixed >> 8) as i16,
        );
        self.z = self.tracked.2;
        super::tracked_point_separation((self.tracked.0, self.tracked.2), self.spawn, scus_sqrt)
    }

    /// The bearing from the spawn point to the lure (`FUN_80019B28`, `0x801D2A08`).
    pub fn heading(&self) -> u16 {
        legaia_engine_field::camera_zone::bearing(
            i32::from(self.spawn.1),
            i32::from(self.spawn.0),
            i32::from(self.tracked.2),
            i32::from(self.tracked.0),
        )
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
        let x = self.tracked.0;
        let z = self.tracked.2;
        let mut out = LureProbe {
            weight: WATER_TILE_DEFAULT.1,
            ..Default::default()
        };
        // `walk_grid_overhead` indexes from the grid's own base, so it takes
        // the `+0x4000` block rather than the whole map.
        let walk = map
            .get(legaia_engine_field::field_regions::MAP_WALK_GRID_OFFSET..)
            .unwrap_or(&[]);
        if walk_grid_overhead(walk, x as i32, z as i32) {
            let push = fs << LURE_DRIFT_SHIFT;
            let signed = if cast_counter & 1 != 0 { push } else { -push };
            self.x_fixed = self.x_fixed.saturating_add(signed);
            out.drift = signed >> 8;
        }
        let tile_x = (x as i32) >> LURE_TILE_SHIFT;
        let tile_z = (z as i32) >> LURE_TILE_SHIFT;
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
