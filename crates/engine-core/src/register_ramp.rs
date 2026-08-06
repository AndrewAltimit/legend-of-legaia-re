//! The field-VM op `0x43` sub-3..6 **camera-register zone ramp**: a camera
//! parameter that interpolates on the player's own position as he crosses an
//! authored tile rectangle.
//!
//! PORT: FUN_8003C6A4
//! REF: FUN_80020DE0 (pool allocator the retail spawn goes through),
//!      FUN_80037018 (the per-frame tick, ported as
//!      `legaia_engine_vm::ambient_motion::zone_ramp_tick`),
//!      FUN_801DE840 (the op-0x43 sub-3..6 caller)
//!
//! ## "Sound register ramp" is a misnomer
//!
//! The op used to be documented as a *sound* register ramp with four target
//! values, a duration in ticks and a curve parameter. None of those four
//! readings survives the disassembly. Every one of the four destinations is a
//! **field camera-configuration register**, and the two trailing halfwords are
//! the ramp's endpoints, not a duration and a shape:
//!
//! | destination | camera role | scene default |
//! |---|---|---|
//! | `DAT_8007B60C` | pitch (`_DAT_8007B790`) | `0x1B8` |
//! | `DAT_8007B610` | yaw (`_DAT_8007B792`) | `0` |
//! | `DAT_8007B614` | eye-space Z, the eye-back depth; its **sign** picks the orbit side | `0x4000` |
//! | `DAT_8007B618` | GTE `H`, the projection register (`_DAT_8007B6F4`) | `0x300` |
//!
//! All four are read by the field-overlay camera composer into the same
//! descriptor whose ten fields are [`crate::camera::RetailCamGlobals`] - see
//! [`RampSlot::camera_axis`] for the per-register store sites. The pitch
//! default is the same `0x1B8` that `FUN_80025C24` seeds into
//! `RetailCamGlobals::FIELD_RESET`, which is what identifies the register.
//! The whole block's seed is [`crate::camera::CAMERA_ZONE_DEFAULTS`].
//!
//! ## The retail spawn
//!
//! `FUN_8003C6A4(dest, width, tile_x_lo, tile_z_lo, tile_x_hi, tile_z_hi,
//! start, end)` allocates an actor from the descriptor pool at
//! `&DAT_80074304` on the effect-actor list (`_DAT_8007C34C`) and fills it in:
//!
//! | Actor field | Value |
//! |---|---|
//! | `+0x94` | `dest` (the destination register) |
//! | `+0x8C` | `width` - destination store width (the field VM always passes `4` = `sw`) |
//! | `+0x88` / `+0x8A` | zone `x_lo` / `z_lo`, each `tile * 0x80 + 0x40` (tile centre in world units) |
//! | `+0xC8` / `+0xCA` | zone `x_hi` / `z_hi`, same scaling |
//! | `+0x80` / `+0x84` | ramp value at the low / high edge of the Z window |
//!
//! The descriptor's `+8` word is `0x80037018`, which `FUN_80020DE0` copies to
//! the new actor's `+0x0C` - i.e. the per-frame handler. That handler is
//! [`legaia_engine_vm::ambient_motion::zone_ramp_tick`]: it gates on the
//! player-engaged flag and the scratch system lock, AABB-tests the player
//! against the zone, and stores
//! `start + (end - start) * (player.z - z_lo) / (z_hi - z_lo)` through `+0x94`.
//! X is a gate only; Z is the ramp parameter.
//!
//! The field-VM caller picks the destination by sub-op: sub-3 ->
//! `DAT_8007B618`, sub-4 -> `DAT_8007B614`, sub-5 -> `DAT_8007B60C`, sub-6 ->
//! `DAT_8007B610`. Both endpoints come through `FUN_8003CE9C`, which
//! **sign-extends** its halfword, so a ramp may run downward.
//!
//! Clean-room boundary: `ghidra/scripts/funcs/8003c6a4.txt` plus the
//! disassembly of `0x80037018` and of the op-0x43 arm at `0x801DF628` are the
//! spec; no Sony bytes live here.

use legaia_engine_vm::ambient_motion::{ZoneRamp, ZoneRampTick, zone_ramp_tick};

/// Destination store width the field VM always requests (retail `a1 = 4`,
/// which `FUN_80037018`'s `slti 5` arm turns into a `sw`).
pub const RAMP_WIDTH: u8 = 4;

/// World-unit scaling the spawner applies to each tile coordinate:
/// `tile * TILE_UNITS + TILE_CENTRE`.
pub const TILE_UNITS: i16 = 0x80;
/// Half-tile centre offset (`addiu v0,v0,0x40` after each `sll v0,_,0x7`).
pub const TILE_CENTRE: i16 = 0x40;

/// Destination camera-configuration register, keyed by the op-0x43 sub-op.
/// Variant names carry the retail global each sub-op targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RampSlot {
    /// Sub-op 3 -> `DAT_8007B618`.
    Dat8007B618,
    /// Sub-op 4 -> `DAT_8007B614` (camera eye-back depth).
    Dat8007B614,
    /// Sub-op 5 -> `DAT_8007B60C` (camera pitch).
    Dat8007B60C,
    /// Sub-op 6 -> `DAT_8007B610`.
    Dat8007B610,
}

impl RampSlot {
    /// Every slot, in register-address order - the index order
    /// [`CameraRegisterFile`] stores them in.
    pub const ALL: [RampSlot; 4] = [
        Self::Dat8007B60C,
        Self::Dat8007B610,
        Self::Dat8007B614,
        Self::Dat8007B618,
    ];

    /// Map an op-0x43 sub-op byte to its destination register. `None`
    /// outside the ramp family (3..=6).
    pub fn from_sub_op(sub_op: u8) -> Option<Self> {
        match sub_op {
            3 => Some(Self::Dat8007B618),
            4 => Some(Self::Dat8007B614),
            5 => Some(Self::Dat8007B60C),
            6 => Some(Self::Dat8007B610),
            _ => None,
        }
    }

    /// The retail RAM address of the destination register (the `dest` the
    /// spawn stores at actor `+0x94`).
    pub fn retail_addr(self) -> u32 {
        match self {
            Self::Dat8007B618 => 0x8007_B618,
            Self::Dat8007B614 => 0x8007_B614,
            Self::Dat8007B60C => 0x8007_B60C,
            Self::Dat8007B610 => 0x8007_B610,
        }
    }

    /// Index into [`CameraRegisterFile`].
    pub fn index(self) -> usize {
        match self {
            Self::Dat8007B60C => 0,
            Self::Dat8007B610 => 1,
            Self::Dat8007B614 => 2,
            Self::Dat8007B618 => 3,
        }
    }

    /// Which of the ten [`crate::camera::RetailCamGlobals`] axes this
    /// register drives.
    ///
    /// Every one of the four is read by the field-overlay camera composer
    /// (`0x801DABA4` in PROT 0897 - **not** the battle overlay's routine at
    /// the same VA) straight into the camera descriptor whose ten fields are
    /// those globals: `+0x02` pitch, `+0x06` yaw, `+0x0E`/`+0x12`/`+0x16`
    /// eye-space translation, `+0x1A`/`+0x1E`/`+0x22` focus, `+0x26` GTE `H`.
    ///
    /// | register | store site | descriptor field | axis |
    /// |---|---|---|---|
    /// | `B60C` | `0x801DAF28` | `+0x02` pitch | 0 |
    /// | `B610` | `0x801DAF4C` | `+0x06` yaw | 1 |
    /// | `B614` | `0x801DAF3C` | `+0x16` eye-space Z | 5 |
    /// | `B618` | `0x801DAFEC` / `0x801DB4B8` | `+0x26` GTE `H` | 9 |
    ///
    /// (`B60C` and `B614` also appear in the mode-4 arm at `0x801DACE4` /
    /// `0x801DACE8` with the same roles.)
    // REF: FUN_801DABA4
    pub fn camera_axis(self) -> usize {
        match self {
            Self::Dat8007B60C => 0,
            Self::Dat8007B610 => 1,
            Self::Dat8007B614 => 5,
            Self::Dat8007B618 => 9,
        }
    }
}

/// The four camera-configuration registers the ramps write, in
/// register-address order (`0x8007B60C`, `B610`, `B614`, `B618`).
///
/// Seeded to the same values `FUN_801DBE9C`'s zone-miss arm installs
/// ([`crate::camera::CAMERA_ZONE_DEFAULTS`]), so a scene with no ramp reads
/// exactly what retail's default zone config carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CameraRegisterFile {
    values: [i32; 4],
    /// Set once any ramp has written a register this scene. Hosts use it to
    /// keep a ramp-free scene's framing untouched.
    written: bool,
}

impl CameraRegisterFile {
    /// The zone-miss defaults, in [`RampSlot::ALL`] order.
    pub const DEFAULTS: [i32; 4] = [0x1B8, 0, 0x4000, 0x300];

    /// Read one register.
    pub fn get(&self, slot: RampSlot) -> i32 {
        self.values[slot.index()]
    }

    /// Write one register (what the handler's `sw` through `+0x94` does).
    pub fn set(&mut self, slot: RampSlot, value: i32) {
        self.values[slot.index()] = value;
        self.written = true;
    }

    /// Camera pitch (`DAT_8007B60C`), 12-bit angle units.
    pub fn pitch(&self) -> i32 {
        self.get(RampSlot::Dat8007B60C)
    }

    /// Camera eye-back depth (`DAT_8007B614`). Signed in retail - the sign
    /// selects which side of the player the orbit sits on.
    pub fn eye_back(&self) -> i32 {
        self.get(RampSlot::Dat8007B614)
    }

    /// Whether any ramp has written a register since the last scene entry.
    pub fn written(&self) -> bool {
        self.written
    }
}

impl Default for CameraRegisterFile {
    fn default() -> Self {
        Self {
            values: Self::DEFAULTS,
            written: false,
        }
    }
}

/// One spawned zone-ramp record - the engine mirror of the actor fields
/// `FUN_8003C6A4` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisterRamp {
    /// Destination camera register (retail actor `+0x94`).
    pub slot: RampSlot,
    /// Destination store width (retail actor `+0x8C`; always [`RAMP_WIDTH`]
    /// from the field VM). Kept raw so the out-of-range retire arm of
    /// [`zone_ramp_tick`] stays reachable.
    pub width: u8,
    /// Zone low corner in world units (retail actor `+0x88` / `+0x8A`).
    pub x_lo: i16,
    pub z_lo: i16,
    /// Zone high corner in world units (retail actor `+0xC8` / `+0xCA`).
    pub x_hi: i16,
    pub z_hi: i16,
    /// Register value at the low Z edge (retail actor `+0x80`).
    pub start: i32,
    /// Register value at the high Z edge (retail actor `+0x84`).
    pub end: i32,
}

impl RegisterRamp {
    /// The record in the shape the ported handler reads.
    pub fn zone_ramp(&self) -> ZoneRamp {
        ZoneRamp {
            start: self.start,
            end: self.end,
            x_lo: self.x_lo,
            x_hi: self.x_hi,
            z_lo: self.z_lo,
            z_hi: self.z_hi,
            kind: i32::from(self.width),
        }
    }

    /// One handler tick against a player position - the port of
    /// `FUN_80037018` for this record. `player_engaged` stands in for
    /// `_DAT_8007C364[+0x10] & 0x80000`; the scratch system lock
    /// (`_DAT_1F800394 & 0x400`) has no engine counterpart and is passed
    /// clear.
    // REF: FUN_80037018
    pub fn tick(&self, player_x: i16, player_z: i16, player_engaged: bool) -> ZoneRampTick {
        zone_ramp_tick(&self.zone_ramp(), player_x, player_z, player_engaged, false)
    }
}

/// Scale one tile coordinate into the world units the spawner stores
/// (`tile * 0x80 + 0x40`).
pub fn tile_to_world(tile: u8) -> i16 {
    i16::from(tile) * TILE_UNITS + TILE_CENTRE
}

/// Spawn a ramp record for an op-0x43 sub-3..6 instruction: resolve the
/// destination register from the sub-op and scale the four tile coordinates
/// into world units. `zone` is the operand's `[x_lo, z_lo, x_hi, z_hi]`;
/// `start` / `end` are the two sign-extended halfwords. `None` when the
/// sub-op is outside the ramp family.
// PORT: FUN_8003C6A4
pub fn spawn_register_ramp(
    sub_op: u8,
    zone: [u8; 4],
    start: i16,
    end: i16,
) -> Option<RegisterRamp> {
    let slot = RampSlot::from_sub_op(sub_op)?;
    Some(RegisterRamp {
        slot,
        width: RAMP_WIDTH,
        x_lo: tile_to_world(zone[0]),
        z_lo: tile_to_world(zone[1]),
        x_hi: tile_to_world(zone[2]),
        z_hi: tile_to_world(zone[3]),
        start: i32::from(start),
        end: i32::from(end),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_op_slot_mapping_matches_retail() {
        assert_eq!(
            RampSlot::from_sub_op(3).map(RampSlot::retail_addr),
            Some(0x8007_B618)
        );
        assert_eq!(
            RampSlot::from_sub_op(4).map(RampSlot::retail_addr),
            Some(0x8007_B614)
        );
        assert_eq!(
            RampSlot::from_sub_op(5).map(RampSlot::retail_addr),
            Some(0x8007_B60C)
        );
        assert_eq!(
            RampSlot::from_sub_op(6).map(RampSlot::retail_addr),
            Some(0x8007_B610)
        );
        assert_eq!(RampSlot::from_sub_op(2), None);
        assert_eq!(RampSlot::from_sub_op(7), None);
    }

    #[test]
    fn zone_corners_scale_from_tiles_to_world_units() {
        let r = spawn_register_ramp(3, [0, 1, 0x80, 0xFF], 30, -2).unwrap();
        // tile * 0x80 + 0x40 - the tile-centre convention the movement ops
        // use, as the retail spawn stores at +0x88/+0x8A/+0xC8/+0xCA.
        assert_eq!(
            (r.x_lo, r.z_lo, r.x_hi, r.z_hi),
            (0x40, 0xC0, 0x4040, 0x7FC0)
        );
        assert_eq!(r.width, RAMP_WIDTH);
        // Both endpoints are sign-extended halfwords (`FUN_8003CE9C`).
        assert_eq!(r.start, 30);
        assert_eq!(r.end, -2);
    }

    #[test]
    fn max_tile_corner_does_not_overflow_i16() {
        let r = spawn_register_ramp(6, [0xFF; 4], 0, 0).unwrap();
        assert_eq!(r.x_lo, 0x7FC0);
        assert_eq!(r.z_hi, 0x7FC0);
    }

    /// The whole point of the record: walking the Z window moves the
    /// register from `start` to `end`, and X only gates.
    #[test]
    fn tick_lerps_on_player_z_and_gates_on_x() {
        // Zone tiles x 0..2, z 0..4 -> world x 0x40..0x140, z 0x40..0x240.
        let r = spawn_register_ramp(5, [0, 0, 2, 4], 0x100, 0x200).unwrap();
        let mid_z = (r.z_lo + r.z_hi) / 2;
        assert_eq!(
            r.tick(0x40, r.z_lo, false),
            ZoneRampTick::Write {
                value: 0x100,
                width: legaia_engine_vm::ambient_motion::ZoneRampWidth::U32
            }
        );
        assert_eq!(
            r.tick(0x40, r.z_hi, false),
            ZoneRampTick::Write {
                value: 0x200,
                width: legaia_engine_vm::ambient_motion::ZoneRampWidth::U32
            }
        );
        let ZoneRampTick::Write { value, .. } = r.tick(0x40, mid_z, false) else {
            panic!("mid-zone must write");
        };
        assert!(
            value > 0x100 && value < 0x200,
            "midpoint must be strictly between the endpoints, got {value:#x}"
        );
        // Outside the X gate: nothing written.
        assert_eq!(r.tick(0x400, mid_z, false), ZoneRampTick::Idle);
        // Engaged player: the handler's first gate rejects.
        assert_eq!(r.tick(0x40, mid_z, true), ZoneRampTick::Idle);
    }

    #[test]
    fn register_file_seeds_the_zone_miss_defaults() {
        let f = CameraRegisterFile::default();
        assert_eq!(f.pitch(), 0x1B8, "pitch default is the FIELD_RESET pitch");
        assert_eq!(f.eye_back(), 0x4000);
        assert_eq!(f.get(RampSlot::Dat8007B610), 0);
        assert_eq!(f.get(RampSlot::Dat8007B618), 0x300);
        assert!(!f.written());
    }

    #[test]
    fn register_file_indexes_are_distinct_and_in_address_order() {
        let mut addrs: Vec<u32> = RampSlot::ALL.iter().map(|s| s.retail_addr()).collect();
        let sorted = {
            let mut a = addrs.clone();
            a.sort_unstable();
            a
        };
        assert_eq!(addrs, sorted, "ALL must be in register-address order");
        addrs.dedup();
        assert_eq!(addrs.len(), 4);
        let mut f = CameraRegisterFile::default();
        for (i, slot) in RampSlot::ALL.iter().enumerate() {
            f.set(*slot, 1000 + i as i32);
        }
        for (i, slot) in RampSlot::ALL.iter().enumerate() {
            assert_eq!(f.get(*slot), 1000 + i as i32);
        }
        assert!(f.written());
    }
}
