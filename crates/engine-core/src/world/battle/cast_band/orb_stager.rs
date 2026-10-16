//! PROT 0911 (Orb)'s part-pool flush. Its stager is
//! `cast_module_ticks::orb_stager_spawns`, run with the other part-called
//! stagers.

use crate::world::World;
use legaia_engine_battle_vm::cast_module_ticks::ORB_STAGER_ARM0_RECORD;

/// The records PROT 0911 seats through the part-pool wrapper `FUN_80050ED4`
/// that a pool flush can reach: arm 5's four (`0x801F7704..0x801F774C`) and
/// the stager's arm-`0` record. Arm 2's flash goes through `FUN_80021B04`
/// and stays out of the pool.
pub const ORB_POOL_RECORDS: [u32; 5] = [
    0x801F_81DC,
    0x801F_823C,
    0x801F_8288,
    0x801F_82D4,
    ORB_STAGER_ARM0_RECORD,
];

/// The arm whose pass flushes the part pool (`jal 0x80050E74` at
/// `0x801F77D0`, ahead of arm 9's own spawns): the light column, ray fan
/// and star arm 5 seated go before the heal's rings come up. The capture
/// of the cast holds them halted (`+0x10 & 8`) at the start of arm 10.
pub const ORB_POOL_FLUSH_ARM: u8 = 9;

impl World {
    /// PROT 0911's pool flush (`FUN_80050E74`): every part the module seated
    /// through `FUN_80050ED4` gets its halt bit when arm 9 passes. Arm 3's
    /// flush (`0x801F70A8`) reaches only the opening's star swarm, which the
    /// engine does not seat.
    ///
    /// REF: FUN_80050E74
    pub(in crate::world::battle) fn orb_pool_flush(&mut self, passed_arm: u8) {
        if passed_arm != ORB_POOL_FLUSH_ARM {
            return;
        }
        if let Some(scene) = self.casting.active_summon.as_mut() {
            for part in &mut scene.parts {
                if part.tag.is_some_and(|t| ORB_POOL_RECORDS.contains(&t)) {
                    part.finished = true;
                }
            }
        }
    }
}
