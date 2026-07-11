//! 4-byte register-ramp actor parameterization (field-VM op `0x43`
//! sub-3..6, the "sound register ramp" family).
//!
//! PORT: FUN_8003C6A4
//! REF: FUN_80020DE0 (pool allocator the retail spawn goes through),
//!      FUN_801DE840 (the op-0x43 sub-3..6 caller)
//!
//! Retail `FUN_8003C6A4(slot_ptr, count, b1, b2, b3, b4, ticks, curve)`
//! allocates an actor from the descriptor pool at `&DAT_80074304` on the
//! effect-actor list (`_DAT_8007C34C`) and parameterizes a ramp of `count`
//! byte-wide registers at `slot_ptr` toward the four target values:
//!
//! | Actor field | Value |
//! |---|---|
//! | `+0x94` | `slot_ptr` (the destination register block) |
//! | `+0x8C` | `count` (the field VM always passes 4) |
//! | `+0x88` / `+0x8A` / `+0xC8` / `+0xCA` | `b1..b4` scaled `* 0x80 + 0x40` (9.7 fixed point, the same tile-centre convention the movement ops use) |
//! | `+0x80` | `ticks` (ramp duration in frame ticks) |
//! | `+0x84` | `curve` (interpolation-curve parameter) |
//!
//! The field-VM caller picks the destination block by sub-op:
//! sub-3 -> `DAT_8007B618`, sub-4 -> `DAT_8007B614`, sub-5 -> `DAT_8007B60C`,
//! sub-6 -> `DAT_8007B610`.
//!
//! The engine models the parameterization (what the dump shows); the
//! per-frame interpolation handler bound from the pool descriptor
//! (`DAT_80074304 + 8`) is not yet traced, so consumers hold the spawned
//! records ([`crate::world::World::register_ramps`]) without ticking them.
//!
//! Clean-room boundary: `ghidra/scripts/funcs/8003c6a4.txt` is the spec; no
//! Sony bytes live here.

/// Register-block width the field VM always requests (retail `a1 = 4`).
pub const RAMP_WIDTH: u8 = 4;

/// Destination register block, keyed by the op-0x43 sub-op. Variant names
/// carry the retail global each sub-op targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RampSlot {
    /// Sub-op 3 -> `DAT_8007B618`.
    Dat8007B618,
    /// Sub-op 4 -> `DAT_8007B614`.
    Dat8007B614,
    /// Sub-op 5 -> `DAT_8007B60C`.
    Dat8007B60C,
    /// Sub-op 6 -> `DAT_8007B610`.
    Dat8007B610,
}

impl RampSlot {
    /// Map an op-0x43 sub-op byte to its destination block. `None` outside
    /// the ramp family (3..=6).
    pub fn from_sub_op(sub_op: u8) -> Option<Self> {
        match sub_op {
            3 => Some(Self::Dat8007B618),
            4 => Some(Self::Dat8007B614),
            5 => Some(Self::Dat8007B60C),
            6 => Some(Self::Dat8007B610),
            _ => None,
        }
    }

    /// The retail RAM address of the destination block (the `slot_ptr` the
    /// spawn stores at actor `+0x94`).
    pub fn retail_addr(self) -> u32 {
        match self {
            Self::Dat8007B618 => 0x8007_B618,
            Self::Dat8007B614 => 0x8007_B614,
            Self::Dat8007B60C => 0x8007_B60C,
            Self::Dat8007B610 => 0x8007_B610,
        }
    }
}

/// One spawned ramp record - the engine mirror of the actor fields
/// `FUN_8003C6A4` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisterRamp {
    /// Destination register block (retail actor `+0x94`).
    pub slot: RampSlot,
    /// Register count (retail actor `+0x8C`; always [`RAMP_WIDTH`] from the
    /// field VM).
    pub width: u8,
    /// The four target values in 9.7 fixed point - `value * 0x80 + 0x40`
    /// (retail actor `+0x88` / `+0x8A` / `+0xC8` / `+0xCA`).
    pub targets_fp: [i16; 4],
    /// Ramp duration in frame ticks (retail actor `+0x80`).
    pub ticks: u16,
    /// Interpolation-curve parameter (retail actor `+0x84`).
    pub curve: u16,
}

/// Spawn a ramp record for an op-0x43 sub-3..6 instruction: resolve the
/// destination block from the sub-op and scale the four byte targets into
/// the 9.7 fixed-point form the retail interpolator consumes. `None` when
/// the sub-op is outside the ramp family.
// PORT: FUN_8003C6A4
pub fn spawn_register_ramp(
    sub_op: u8,
    targets: [u8; 4],
    ticks: u16,
    curve: u16,
) -> Option<RegisterRamp> {
    let slot = RampSlot::from_sub_op(sub_op)?;
    Some(RegisterRamp {
        slot,
        width: RAMP_WIDTH,
        targets_fp: targets.map(|b| i16::from(b) * 0x80 + 0x40),
        ticks,
        curve,
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
    fn targets_scale_to_9_7_fixed_point() {
        let r = spawn_register_ramp(3, [0, 1, 0x80, 0xFF], 30, 2).unwrap();
        // value * 0x80 + 0x40 - the +0x40 half-step centre, as the retail
        // stores at +0x88/+0x8A/+0xC8/+0xCA.
        assert_eq!(r.targets_fp, [0x40, 0xC0, 0x4040, 0x7FC0]);
        assert_eq!(r.width, RAMP_WIDTH);
        assert_eq!(r.ticks, 30);
        assert_eq!(r.curve, 2);
    }

    #[test]
    fn max_byte_target_does_not_overflow_i16() {
        let r = spawn_register_ramp(6, [0xFF; 4], 0, 0).unwrap();
        assert!(r.targets_fp.iter().all(|&v| v == 0x7FC0));
    }
}
