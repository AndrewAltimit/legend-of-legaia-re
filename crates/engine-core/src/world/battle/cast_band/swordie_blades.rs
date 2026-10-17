//! PROT 0910 (Swordie)'s slash blades: the point arm 8 stores for the four
//! slashes to fly from, and the blade image the slash routine seats on every
//! call that leaves its slash travelling.

use crate::world::World;
use legaia_engine_cast::cast_module_camera::{ModuleSpawn, SpawnAnchor, SpawnRecord};
use legaia_engine_cast::seru_magic_ticks::{
    SWORDIE_BLADE_RECORD, SwordieSlashes, swordie_blade_origin, swordie_blade_pose,
};

/// The arm whose pass stores the slashes' origin (`0x801F77F4..0x801F789C`).
const SWORDIE_ORIGIN_ARM: u8 = 8;

/// The frame step retail runs the slash arms at: the cast-sequence oracle's
/// capture holds `*(0x1F800393) = 4` through arms 9 and 10, so a slash's
/// `64 * speed` progress takes sixteen passes and seats fifteen blades. The
/// engine steps the progress once a vsync at `1 * speed`, so a blade is
/// seated on each call that brings the progress to a multiple of
/// `4 * speed` - the calls retail makes.
const SWORDIE_SLASH_FRAME_STEP: u32 = 4;

impl World {
    /// PROT 0910's blade writes for the tick the body just ran (`phase_in`
    /// to `phase_out`).
    ///
    /// - Arm 8's pass stores the slashes' origin off the creature's seat
    ///   ([`swordie_blade_origin`]).
    /// - Each slash still travelling after its call seats one
    ///   [`SWORDIE_BLADE_RECORD`] at [`swordie_blade_pose`] off the victim's
    ///   live plane pair, on the calls retail makes
    ///   ([`SWORDIE_SLASH_FRAME_STEP`]).
    ///
    /// PORT: FUN_801F81DC (PROT 0910; the blade seat at `0x801F883C`, and arm 8's origin store in FUN_801F69EC)
    pub(in crate::world::battle::cast_band) fn swordie_blades(
        &mut self,
        phase_in: u8,
        phase_out: u8,
        slashes: &mut SwordieSlashes,
        speed: u8,
        summon_slot: u8,
        victim_slot: u8,
    ) {
        if phase_in == SWORDIE_ORIGIN_ARM
            && phase_out != phase_in
            && let Some(c) = self.actors.get(usize::from(summon_slot))
        {
            slashes.origin = swordie_blade_origin(
                c.move_state.world_x,
                c.move_state.world_z,
                c.battle.facing_angle,
            );
        }
        let Some(v) = self.actors.get(usize::from(victim_slot)) else {
            return;
        };
        let victim = (v.move_state.world_x, v.move_state.world_z);
        let pass = SWORDIE_SLASH_FRAME_STEP * u32::from(speed.max(1));
        let spawns: Vec<ModuleSpawn> = (0..slashes.draw.blades.len())
            .filter(|&s| slashes.draw.blades[s] && slashes.progress[s].is_multiple_of(pass))
            .map(|s| {
                let (pos, rot) =
                    swordie_blade_pose(s as u8, slashes.progress[s], speed, slashes.origin, victim);
                ModuleSpawn {
                    record: SpawnRecord::Module(SWORDIE_BLADE_RECORD),
                    anchor: SpawnAnchor::Literal { pos, rot },
                }
            })
            .collect();
        if !spawns.is_empty() {
            self.stage_module_arm_spawns(910, &spawns, None, None);
        }
    }
}
