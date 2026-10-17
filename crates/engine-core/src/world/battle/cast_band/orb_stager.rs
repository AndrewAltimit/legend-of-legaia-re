//! PROT 0911 (Orb)'s writes outside its tick kernel: the opening star
//! swarm arm 2 seats and arm 3 draws in, the part-pool flushes, the roster
//! panels arm 9's pass raises and the creature seat arm 5's pass moves. Its
//! stager is `cast_module_ticks::orb_stager_spawns`, run with the other
//! part-called stagers.

use crate::world::World;
use legaia_engine_cast::cast_module_ticks::ORB_STAGER_ARM0_RECORD;

/// The star record arm 2 seats sixteen times through the part-pool wrapper
/// `FUN_80050ED4` (`a2 = 0x801F8190` at `0x801F6E90` / `0x801F703C`): a
/// billboard (`+0x52 = 0x380`) on draw kind 4 that waits out the cast.
pub const ORB_STAR_RECORD: u32 = 0x801F_8190;

/// Stars in the swarm: eight passes of the loop at `0x801F6CDC`, two seated
/// a pass (`sltiu v0,s2,0x10` at `0x801F7068`).
pub const ORB_STAR_COUNT: usize = 16;

/// The height every star converges on - the flash point arm 2's discs sit
/// at (`li v0,-0x3e8` at `0x801F71FC` / `0x801F7370`).
pub const ORB_STAR_GOAL_Y: i16 = -0x3E8;

/// The records PROT 0911 seats through the part-pool wrapper `FUN_80050ED4`
/// that a pool flush can reach: the opening's stars, arm 5's four
/// (`0x801F7704..0x801F774C`) and the stager's arm-`0` record. Arm 2's flash
/// goes through `FUN_80021B04` and stays out of the pool.
pub const ORB_POOL_RECORDS: [u32; 6] = [
    ORB_STAR_RECORD,
    0x801F_81DC,
    0x801F_823C,
    0x801F_8288,
    0x801F_82D4,
    ORB_STAGER_ARM0_RECORD,
];

/// The arms whose pass flushes the part pool (`jal 0x80050E74`): arm 3's at
/// `0x801F70A8`, when only the stars are seated, and arm 9's at
/// `0x801F77D0`, ahead of its own spawns - the light column, ray fan and
/// star arm 5 seated go before the heal's rings come up. The capture of the
/// cast holds them halted (`+0x10 & 8`) at the start of arm 10.
pub const ORB_POOL_FLUSH_ARMS: [u8; 2] = [3, 9];

/// The arm whose pass raises the roster panels (`FUN_801D8DE8(6, 0)` at
/// `0x801F7E68`, then `(0x4E + i, 0)` at `0x801F7E98`).
pub const ORB_PANEL_ARM: u8 = 9;

/// The depth-cue weight arm 9's pass stores into the backdrop pair's
/// `+0x78` (`0x801F7854`).
pub const ORB_HEAL_BACKDROP_CUE: u16 = 0x800;

/// The arm whose pass steps the creature forward and lifts it, jumping to
/// [`ORB_PANEL_ARM`] (`0x801F75E4..0x801F7780`).
pub const ORB_LIFT_ARM: u8 = 5;

/// The speed scalar `*(0x1F80037D)` arm 3 divides the countdown by.
const SCALAR: i32 = legaia_engine_battle_vm::battle_anim_rate::RATE_NORMAL as i32;

/// `(a * b) >> 12` rounded toward zero, the `bgez; addiu 0xfff; sra 0xc`
/// idiom.
fn mul12(a: i16, b: i16) -> i32 {
    (i32::from(a) * i32::from(b)) / 4096
}

/// The sine and cosine table reads `*0x8007B81C` / `*0x8007B7F8` at a
/// 12-bit index.
fn sin(i: u16) -> i16 {
    legaia_engine_system::trig::sin12(i)
}
fn cos(i: u16) -> i16 {
    legaia_engine_system::trig::trig12(i).1
}

/// One pass of arm 2's star loop (`0x801F6CDC..0x801F7070`): five `rand()`
/// draws build the offset of a star and of its mirror, and each is seated
/// round the caster `(cx, cz)` facing `f`.
///
/// ```text
/// a.y = -(r0 % 511); a.x = r1 % 511 + 0x100; a.z = r2 % 255 + 0x300
/// if r3 & 1 { a.x = -a.x }
/// A at (cx + sin f * a.z + cos f * a.x,        a.y, cz + sin f * a.x + cos f * a.z)
/// b = a; b.x = -a.x; b.y = -0x200 - r4 % 511
/// B at (cx + sin f * b.z + cos(0xFFF-f) * b.x, b.y, cz + sin(0xFFF-f) * b.x + cos f * b.z)
/// ```
///
/// Returns `[(offset, seat)]` for the pair, each `>> 12` rounded toward
/// zero and the sums stored as halfwords.
///
/// PORT: FUN_801F69D8 (PROT 0911 arm 2's star loop, `0x801F6CDC..0x801F7070`)
pub fn orb_star_pair(
    cx: i16,
    cz: i16,
    f: u16,
    mut rand: impl FnMut() -> u32,
) -> [([i16; 3], [i16; 3]); 2] {
    let f = f & 0xFFF;
    let mut draw = || rand() as i32;
    let y = -(draw() % 511);
    let mut x = draw() % 511 + 0x100;
    let z = draw() % 255 + 0x300;
    if draw() & 1 != 0 {
        x = -x;
    }
    let a = [x as i16, y as i16, z as i16];
    let seat_a = [
        cx.wrapping_add((mul12(sin(f), a[2]) + mul12(cos(f), a[0])) as i16),
        a[1],
        cz.wrapping_add((mul12(sin(f), a[0]) + mul12(cos(f), a[2])) as i16),
    ];
    let by = -0x200 - draw() % 511;
    let b = [a[0].wrapping_neg(), by as i16, a[2]];
    let m = 0xFFF - f;
    let seat_b = [
        cx.wrapping_add((mul12(sin(f), b[2]) + mul12(cos(m), b[0])) as i16),
        b[1],
        cz.wrapping_add((mul12(sin(m), b[0]) + mul12(cos(f), b[2])) as i16),
    ];
    [(a, seat_a), (b, seat_b)]
}

/// Where arm 3 holds star `offset` this pass (`0x801F7140..0x801F73D0`):
/// `k = 0x80 - countdown / scalar`, floored at `0`; under `0x80` the star
/// sits `k / 128` of the way from its offset height to [`ORB_STAR_GOAL_Y`]
/// and its plane offset, turned by the caster's mirrored facing, shrinks by
/// `(0x80 - k) / 128` round `(cx, cz)`; from `0x80` it sits on the goal.
///
/// PORT: FUN_801F69D8 (PROT 0911 arm 3's per-pass star placement)
pub fn orb_star_place(offset: [i16; 3], cx: i16, cz: i16, f: u16, countdown: i32) -> [i16; 3] {
    let f = f & 0xFFF;
    let k = (0x80 - countdown / SCALAR).max(0);
    if k >= 0x80 {
        return [cx, ORB_STAR_GOAL_Y, cz];
    }
    let [px, py, pz] = offset;
    let y = i32::from(py) + (i32::from(ORB_STAR_GOAL_Y) - i32::from(py)) * k / 128;
    let left = 0x80 - k;
    let m = 0xFFF - f;
    let dz = (mul12(sin(m), px) + mul12(cos(f), pz)) * left / 128;
    let dx = (mul12(sin(f), pz) + mul12(cos(m), px)) * left / 128;
    [
        cx.wrapping_add(dx as i16),
        y as i16,
        cz.wrapping_add(dz as i16),
    ]
}

impl World {
    /// PROT 0911's writes around its tick, every tick of the module:
    ///
    /// - arm 2's pass seats the sixteen stars ([`orb_star_pair`]) on the
    ///   caster's facing, on arm 3's first tick with the pass's other calls;
    /// - while arm 3 holds, each star moves to [`orb_star_place`] of the
    ///   countdown the pass left;
    /// - arms 3 and 9 flush the part pool on their pass (`FUN_80050E74`):
    ///   every part the module seated through `FUN_80050ED4` gets its halt
    ///   bit;
    /// - arm 9's pass raises the roster panels
    ///   ([`World::raise_module_roster_panels`]) and half-cues the backdrop
    ///   pair ([`ORB_HEAL_BACKDROP_CUE`]).
    ///
    /// REF: FUN_80050E74, FUN_801D8DE8
    pub(in crate::world::battle) fn orb_arm_writes(&mut self, phase_in: u8, phase_out: u8) {
        let passed = phase_out != phase_in;
        // Arm 2's pass seats the swarm beside its flash discs; the
        // body-paced director hands that pass's calls over on arm 3's first
        // tick, and the stars go with them.
        if phase_in == 2 && passed {
            self.casting.module_orb.stars_owed = true;
        }
        if phase_in == 3 && std::mem::take(&mut self.casting.module_orb.stars_owed) {
            self.orb_seat_stars();
        }
        if phase_in == 3 && !passed {
            self.orb_place_stars();
        }
        if passed && ORB_POOL_FLUSH_ARMS.contains(&phase_in) {
            self.orb_pool_flush();
        }
        if passed && phase_in == ORB_PANEL_ARM {
            self.raise_module_roster_panels();
            // `li v0,0x800` / `sh v0,0x78(..)` into both backdrop records
            // (`ctx[+0x106C]` / `ctx[+0x1070]`, `0x801F7848..0x801F7860`):
            // the stage the heal stands on comes back half-cued, and the
            // ambient pass ramps it from there.
            self.battle.backdrop_cue = ORB_HEAL_BACKDROP_CUE;
        }
    }

    /// The caster's plane pair and facing, as arms 2 and 3 copy them off
    /// `actor_table[ctx[+0x13]]`.
    fn orb_caster_seat(&self) -> Option<(i16, i16, u16)> {
        let a = self.actors.get(usize::from(self.battle_ctx.active_actor))?;
        Some((
            a.move_state.world_x,
            a.move_state.world_z,
            a.battle.facing_angle & 0xFFF,
        ))
    }

    fn orb_seat_stars(&mut self) {
        use legaia_engine_cast::cast_module_camera::{ModuleSpawn, SpawnAnchor, SpawnRecord};
        let Some((cx, cz, f)) = self.orb_caster_seat() else {
            return;
        };
        let mut offsets = Vec::with_capacity(ORB_STAR_COUNT);
        let mut spawns = Vec::with_capacity(ORB_STAR_COUNT);
        for _ in 0..ORB_STAR_COUNT / 2 {
            for (offset, pos) in orb_star_pair(cx, cz, f, || self.next_rand()) {
                offsets.push(offset);
                spawns.push(ModuleSpawn {
                    record: SpawnRecord::Module(ORB_STAR_RECORD),
                    anchor: SpawnAnchor::Literal {
                        pos,
                        rot: [0, f as i16, 0],
                    },
                });
            }
        }
        self.casting.module_orb.star_offsets = offsets;
        self.stage_module_arm_spawns(911, &spawns, None, None);
    }

    fn orb_place_stars(&mut self) {
        let Some((cx, cz, f)) = self.orb_caster_seat() else {
            return;
        };
        let countdown = self.casting.module_settle_countdown;
        let offsets = &self.casting.module_orb.star_offsets;
        let Some(scene) = self.casting.active_summon.as_mut() else {
            return;
        };
        let stars = scene
            .parts
            .iter_mut()
            .filter(|p| p.tag == Some(ORB_STAR_RECORD));
        for (part, offset) in stars.zip(offsets) {
            let [x, y, z] = orb_star_place(*offset, cx, cz, f, countdown);
            part.state.world_x = x;
            part.state.world_y = y;
            part.state.world_z = z;
        }
    }

    /// PROT 0911's pool flush (`FUN_80050E74`).
    fn orb_pool_flush(&mut self) {
        if let Some(scene) = self.casting.active_summon.as_mut() {
            for part in &mut scene.parts {
                if part.tag.is_some_and(|t| ORB_POOL_RECORDS.contains(&t)) {
                    part.finished = true;
                }
            }
        }
    }

    /// Hold the seat arm 5's pass wrote (`0x801F75E4..0x801F7600`) for the
    /// next tick. Retail steps and lifts the creature in the pass that cuts
    /// to it (`FUN_801D829C` at `0x801F7634`), so no frame shows the old shot
    /// with the creature gone; the body-paced director lands that cut on the
    /// next tick ([`ORB_LIFT_ARM`]'s pass moves to arm 9), and the seat moves
    /// with it. Returns whether the seat was held.
    pub(in crate::world::battle) fn orb_defer_seat(
        &mut self,
        phase_in: u8,
        phase_out: u8,
        seat: &legaia_engine_cast::seru_magic_ticks::OrbSeatPos,
    ) -> bool {
        if phase_in != ORB_LIFT_ARM || phase_out == ORB_LIFT_ARM {
            return false;
        }
        self.casting.module_orb.deferred_seat = Some([seat.x, seat.y, seat.z]);
        true
    }

    /// Put a held arm-5 seat on the creature, ahead of the tick's camera
    /// feed.
    pub(in crate::world::battle) fn orb_land_deferred_seat(&mut self) {
        let Some([x, y, z]) = self.casting.module_orb.deferred_seat.take() else {
            return;
        };
        if let Some(a) = self
            .casting
            .summon_actor_slot
            .and_then(|s| self.actors.get_mut(usize::from(s)))
        {
            a.move_state.world_x = x;
            a.move_state.world_y = y;
            a.move_state.world_z = z;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A facing of zero leaves the offsets unturned: star A at the caster
    /// plus its offset, its mirror at minus its x.
    #[test]
    fn a_star_pair_mirrors_across_the_caster() {
        let mut draws = [10u32, 20, 30, 1, 40].into_iter();
        let [(a, sa), (b, sb)] = orb_star_pair(100, -200, 0, || draws.next().unwrap());
        assert_eq!(a, [-(20 + 0x100), -10, 30 + 0x300]);
        assert_eq!(sa, [100 - (20 + 0x100), -10, -200 + 30 + 0x300]);
        assert_eq!(b, [20 + 0x100, -0x200 - 40, 30 + 0x300]);
        // cos(0xFFF) truncates to 4095, so the mirror's x term loses a unit.
        assert_eq!(sb[1], -0x200 - 40);
        assert_eq!(sb[2], -200 + 30 + 0x300 + mul12(sin(0xFFF), b[0]) as i16);
    }

    /// The swarm starts on its offsets and closes on the flash point as the
    /// countdown runs out.
    #[test]
    fn stars_converge_on_the_flash_point() {
        let off = [0x200, -0x100, 0x300];
        let start = orb_star_place(off, 0, 0, 0, SCALAR << 7);
        assert_eq!(start[1], -0x100);
        let half = orb_star_place(off, 0, 0, 0, SCALAR << 6);
        assert_eq!(half[1], (-0x100 + (-0x3E8 + 0x100) / 2) as i16);
        assert_eq!(orb_star_place(off, 5, 6, 0, 0), [5, ORB_STAR_GOAL_Y, 6]);
    }
}
