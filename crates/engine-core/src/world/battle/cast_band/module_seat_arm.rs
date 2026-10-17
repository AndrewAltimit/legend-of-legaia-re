//! What each slot-B module's **seat arm** stores on its creature right after
//! the seat installer `FUN_801F19EC` returns, on the same pass.
//!
//! The installer stands the creature on the caster, wearing the caster's
//! facing, at animation rate `2` ([`World::seat_summon_actor`]). Most summon
//! modules then overwrite that seat with literals of their own: a ground
//! pair on the battle's own axes, a facing, a height and often a rate. Each
//! row below is read off the instructions between the module's
//! `jal 0x801F19EC` and the arm's exit; modules whose seat arm computes the
//! seat from a heading (PROT 0903, 0904, 0905, 0907, 0908, 0910, 0916) keep
//! it in their directors, which the seat adopts through `module_cam.creature`.

use super::*;

/// One coordinate a seat arm stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::world) enum SeatCoord {
    /// Not stored: the installer's (the caster's) stands.
    Keep,
    /// A literal (`li v0,K` / `sh zero`).
    Lit(i16),
    /// The installer's value plus a delta (`lhu` / `addiu` / `sh` on the
    /// same field).
    SeatPlus(i16),
    /// The victim's value plus a delta (PROT 0915 reads the caster's target
    /// seat, `caster[+0x1DD]`).
    VictimPlus(i16),
}

/// The stores one seat arm makes on its creature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::world) struct SeatArmWrite {
    /// `+0x34`.
    pub x: SeatCoord,
    /// `+0x36`, the height.
    pub y: SeatCoord,
    /// `+0x38`.
    pub z: SeatCoord,
    /// `+0x46`.
    pub facing: Option<u16>,
    /// `+0x21D`, the animation rate.
    pub rate: Option<u8>,
    /// `(+0x21B, +0x176)`: a loop counter and hold budget armed on the
    /// installer's clip, which the installer itself leaves unseeded.
    pub loop_hold: Option<(u8, u16)>,
}

const fn seat(x: SeatCoord, z: SeatCoord, facing: u16) -> SeatArmWrite {
    SeatArmWrite {
        x,
        y: SeatCoord::Keep,
        z,
        facing: Some(facing),
        rate: None,
        loop_hold: None,
    }
}

impl SeatArmWrite {
    const fn y(mut self, y: i16) -> Self {
        self.y = SeatCoord::Lit(y);
        self
    }

    const fn rate(mut self, rate: u8) -> Self {
        self.rate = Some(rate);
        self
    }
}

use SeatCoord::{Keep, Lit, SeatPlus, VictimPlus};

/// The seat-arm stores of PROT `prot_entry`, or `None` where the arm leaves
/// the installer's seat alone (or a director places the creature).
///
/// PROT 0929 (Mule) also stores `+0x38 = -(0x80089120)`, the battle's
/// world-anchor Z, which the engine keeps no twin of; its row keeps the
/// installer's Z.
pub(in crate::world) fn seat_arm_write(prot_entry: u32) -> Option<SeatArmWrite> {
    Some(match prot_entry {
        // `0x801F70E8..0x801F70F0`: the height only (`-0x320`); and
        // `+0x21B = 8` / `+0x176 = 0x80` (`0x801F70C8..0x801F70D4`), which
        // parks the installer's clip on its window frame `3` for eight
        // keyframes.
        911 => SeatArmWrite {
            x: Keep,
            y: Lit(-0x320),
            z: Keep,
            facing: None,
            rate: None,
            loop_hold: Some((8, 0x80)),
        },
        // `0x801F6C54`: the facing only.
        912 => SeatArmWrite {
            x: Keep,
            y: Keep,
            z: Keep,
            facing: Some(0),
            rate: None,
            loop_hold: None,
        },
        // `0x801F6CC0..0x801F6CE0`: `0x400` across and `0x267` back off
        // the installer's seat.
        914 => seat(SeatPlus(0x400), SeatPlus(-0x267), 0),
        // `0x801F6EA0..0x801F6EB8`: on the victim's line, `0xA00` toward the
        // party.
        915 => seat(VictimPlus(0), VictimPlus(-0xA00), 0),
        // `0x801F6E48..0x801F6E50`.
        917 => seat(Lit(0), Lit(0), 0),
        // `0x801F71EC..0x801F7200`.
        918 => seat(Lit(0), Lit(-0xC00), 0).y(-0x7F0),
        // `0x801F6D18..0x801F6D28`: the height only, `-(scalar << 8)`.
        920 => SeatArmWrite {
            x: Keep,
            y: Lit(-((legaia_engine_cast::cast_module_camera::SPEED_SCALAR << 8) as i16)),
            z: Keep,
            facing: None,
            rate: None,
            loop_hold: None,
        },
        // `0x801F6FC0..0x801F6FC8`.
        921 => seat(Lit(0), Lit(0), 0),
        // `0x801F6E04..0x801F6E14`: frozen (`+0x21D = 0`) until arm 5.
        922 => seat(Lit(0), Lit(-0x80), 0).rate(0),
        // `0x801F6DC0..0x801F6DD4`.
        923 => seat(Lit(0), Lit(-0x800), 0).y(-0x100),
        // `0x801F6D2C..0x801F6D54`.
        924 => seat(Lit(0), Lit(0x2000), 0x800).rate(8),
        // `0x801F6DC0..0x801F6DEC`.
        925 => seat(Lit(0), Lit(0), 0x800).rate(8),
        // `0x801F6DF4..0x801F6E0C`.
        927 => seat(Lit(0), Lit(0x1C0), 0x800).rate(0),
        // `0x801F746C..0x801F7498`: the origin, lifted `0x40`.
        928 => seat(Lit(0), Lit(0), 0).y(-0x40),
        // `0x801F70A0..0x801F70CC` (Z: see above).
        929 => seat(Lit(0), Keep, 0x800),
        // `0x801F6EA0..0x801F6EB4`.
        930 => seat(Lit(0), Lit(0x800), 0x800).rate(2),
        // `0x801F6E3C..0x801F6E60`.
        931 => seat(Lit(0), Lit(-0x800), 0).rate(4),
        // `0x801F6EC4..0x801F6EE8`.
        932 => seat(Lit(-0x96), Lit(-0x1C0), 0).rate(0x10),
        // `0x801F6D90..0x801F6DA8`.
        933 => seat(Lit(0), Lit(0), 0).rate(4),
        // `0x801F7048..0x801F7060`.
        934 => seat(Lit(0), Lit(-0x600), 0).rate(4),
        // `0x801F7134..0x801F7148` (`s1 = 0x800` at `0x801F710C`).
        966 => seat(Lit(0), Lit(0x1C0), 0x800).rate(0),
        _ => return None,
    })
}

/// PROT 0922's arm 4 (`0x801F6E34..0x801F6E7C`) slides its creature along Z
/// by `2 * step * scalar` a pass while the countdown arm 3 armed
/// (`scalar << 4`) drains by `step * scalar`: `32 * scalar` in all. At the
/// battle's scalar `8` that is `0x100`, from the seat's `-0x80` to `0x80`.
const PUERA_SLIDE_ARM: u8 = 4;
const PUERA_SLIDE_END_Z: i16 = 0x80;
/// The slide per engine tick: one vsync of `2 * step * scalar` at step `2`
/// and scalar `8`.
const PUERA_SLIDE_PER_TICK: i16 = 16;
/// PROT 0922's arm 5 (`0x801F6EE4..0x801F6EF4`): rate back to `8` and the
/// next clip staged.
const PUERA_WAKE_ARM: u8 = 5;

impl World {
    /// Apply PROT `prot_entry`'s seat-arm stores to the creature at `slot`.
    pub(in crate::world) fn apply_seat_arm_write(&mut self, prot_entry: u32, slot: usize) {
        let Some(w) = seat_arm_write(prot_entry) else {
            return;
        };
        let victim = self.casting.module_cam.victim_slot.or_else(|| {
            let caster = self.casting.summon_stager.as_ref()?.caster;
            self.actors
                .get(usize::from(caster))
                .map(|a| a.battle.active_target)
        });
        let victim_pos = victim
            .and_then(|v| self.actors.get(usize::from(v)))
            .map(|a| (a.move_state.world_x, a.move_state.world_z));
        let Some(a) = self.actors.get_mut(slot) else {
            return;
        };
        let resolve = |c: SeatCoord, cur: i16, victim: Option<i16>| match c {
            Keep => cur,
            Lit(v) => v,
            SeatPlus(d) => cur.wrapping_add(d),
            VictimPlus(d) => victim.map_or(cur, |v| v.wrapping_add(d)),
        };
        let ms = &mut a.move_state;
        ms.world_x = resolve(w.x, ms.world_x, victim_pos.map(|p| p.0));
        ms.world_y = resolve(w.y, ms.world_y, None);
        ms.world_z = resolve(w.z, ms.world_z, victim_pos.map(|p| p.1));
        if let Some(f) = w.facing {
            a.battle.facing_angle = f;
        }
        if let Some(r) = w.rate {
            a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(r);
        }
        if let Some((count, timer)) = w.loop_hold {
            a.battle.hit_count_bound = count;
            if let Some(p) = a.battle_animation.as_mut() {
                p.set_loop_timer(timer);
            }
        }
    }

    /// The creature stores of a module's later arms that the engine runs
    /// beside its director: PROT 0922's slide (arm 4) and wake (arm 5).
    pub(in crate::world::battle::cast_band) fn apply_module_seat_motion(
        &mut self,
        prot_entry: u32,
        phase: u8,
    ) {
        if prot_entry != 922 {
            return;
        }
        let Some(a) = self
            .casting
            .summon_actor_slot
            .and_then(|s| self.actors.get_mut(usize::from(s)))
            .filter(|a| a.active)
        else {
            return;
        };
        match phase {
            PUERA_SLIDE_ARM => {
                let z = &mut a.move_state.world_z;
                *z = (*z + PUERA_SLIDE_PER_TICK).min(PUERA_SLIDE_END_Z);
            }
            PUERA_WAKE_ARM
                if a.battle.anim_rate.get() == 0
                    && a.battle.current_anim == 0
                    && a.battle.queued_anim == 0 =>
            {
                a.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(8);
                a.battle.queued_anim = 1;
            }
            _ => {}
        }
    }
}

/// The modules whose seat arm drains a countdown and calls the installer
/// only on the pass that finds it spent (`bgez` / `bgtz` straight ahead of
/// `jal 0x801F19EC`), so the creature appears as the arm **leaves**, not as
/// it is entered: PROT 0907 (`0x801F76B8`), 0911 (`0x801F70A0`), 0923
/// (`0x801F6D9C`) and 0932 (`0x801F6EA4`). Their seat arm is
/// [`legaia_engine_cast::cast_module_camera::module_seat_arm`]; the
/// summon-hold modules already name the arm after theirs. PROT 0915
/// (`0x801F6E54`) and 0934 (`0x801F7028`) seat on the pass too, but their
/// camera-only directors park on that arm, so the port seats them as it is
/// entered.
const SEATS_ON_PASS: [u32; 4] = [907, 911, 923, 932];

/// Whether PROT `prot_entry`'s owed seat is due at module phase `phase`,
/// given its seat arm `arm`.
pub(in crate::world) fn seat_due(prot_entry: Option<u32>, arm: u8, phase: u8) -> bool {
    if prot_entry.is_some_and(|e| SEATS_ON_PASS.contains(&e)) {
        phase > arm
    } else {
        phase >= arm
    }
}
