//! PROT 0913 (Nova)'s director, beside its ported tick body
//! (`crate::seru_magic_ticks::nova_tick`).

use super::body_paced::*;
use super::*;
use legaia_engine_system::trig::trig12;

const fn nova_spawn(va: u32, anchor: SpawnAnchor) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(va),
        anchor,
    }
}

/// PROT 0913 arm 2's two spawns on its pass, on the creature the installer
/// has just stood on the caster, unturned (`jal 0x80021B04` at `0x801F6D3C`
/// / `0x801F6D54`): the purple vortex that opens on the ground. The retail
/// capture of the cast holds its rings at the caster's `(63, 0, -591)`.
pub const NOVA_VORTEX_SPAWNS: [ModuleSpawn; 2] = [
    nova_spawn(0x801F_889C, SpawnAnchor::CasterUnturned { y: 0 }),
    nova_spawn(0x801F_89AC, SpawnAnchor::CasterUnturned { y: 0 }),
];

const fn arm8_frame(div: i16, side: i16, y: i16, roll: i16) -> SpawnAnchor {
    SpawnAnchor::CreatureFrame {
        div,
        side,
        extra: 0,
        y,
        yaw: 0,
        roll,
    }
}

/// PROT 0913 arm 8's twelve spawns on its pass (`0x801F7310..0x801F773C`),
/// all on the creature `c` (heading `f`) and its own angle trio:
///
/// * `0x801F89FC` at `c - (sin f, cos f) / 26`, `y = -0x258`
///   (`* 0x4EC4EC4F >> 3` at `0x801F7360`);
/// * on each side `s = +-1`, the point `c - (sin f, cos f) / 12 + s (cos f,
///   -sin f) / 11` at `y = -0x280` (`* 0x2AAAAAAB >> 1`, `* 0x2E8BA2E9 >> 1`
///   of the table read at `f + 0x400`): `0x801F8A44`, then two of
///   `0x801F8A8C` (`s = +1`) or `0x801F8B04` (`s = -1`) with the trio's roll
///   stepped `-+0x80` and then `-+0x100` further (`sh v0,0x24(sp)` in each
///   call's delay slot);
/// * `0x801F8B7C`, `0x801F8BDC`, `0x801F8C60`, `0x801F8CE4`, `0x801F8D68`
///   on the creature seat itself (`a0 = c + 0x34`, `a1 = c + 0x44`).
///
/// The arm passes at once, so these land on the same tick as its framing.
pub const NOVA_ARM8_SPAWNS: [ModuleSpawn; 12] = [
    nova_spawn(0x801F_89FC, arm8_frame(26, 0, -0x258, 0)),
    nova_spawn(0x801F_8A44, arm8_frame(12, -11, -0x280, 0)),
    nova_spawn(0x801F_8A8C, arm8_frame(12, -11, -0x280, -0x80)),
    nova_spawn(0x801F_8A8C, arm8_frame(12, -11, -0x280, -0x180)),
    nova_spawn(0x801F_8A44, arm8_frame(12, 11, -0x280, 0)),
    nova_spawn(0x801F_8B04, arm8_frame(12, 11, -0x280, 0x80)),
    nova_spawn(0x801F_8B04, arm8_frame(12, 11, -0x280, 0x180)),
    nova_spawn(0x801F_8B7C, SpawnAnchor::Creature),
    nova_spawn(0x801F_8BDC, SpawnAnchor::Creature),
    nova_spawn(0x801F_8C60, SpawnAnchor::Creature),
    nova_spawn(0x801F_8CE4, SpawnAnchor::Creature),
    nova_spawn(0x801F_8D68, SpawnAnchor::Creature),
];

/// The two cloud-bank records, in the order arm 10 spawns them.
pub const NOVA_CLOUD_RECORDS: [u32; 2] = [0x801F_87F0, 0x801F_8790];

/// PROT 0913 arm 10's two spawns on its pass, on the creature (`0x801F7850`
/// turned `0x400` past its facing, `0x801F787C` on it): the two storm-cloud
/// banks, whose handles the module keeps at `0x801F9134` / `0x801F9138`.
pub const NOVA_CLOUD_SPAWNS: [ModuleSpawn; 2] = [
    nova_spawn(
        NOVA_CLOUD_RECORDS[0],
        SpawnAnchor::CreatureYaw { yaw: 0x400 },
    ),
    nova_spawn(NOVA_CLOUD_RECORDS[1], SpawnAnchor::CreatureYaw { yaw: 0 }),
];

/// The ring record PROT 0913 arm 13 stacks sixteen times into the
/// whirlwind (`addiu s5,v0,-0x7240` at `0x801F7B94`).
pub const NOVA_RING_RECORD: u32 = 0x801F_8DC0;

/// PROT 0913 arm 13's spawns on its pass: `0x801F8E28` on the victim at the
/// framing depth's scale (`0x801F7B88`), then the sixteen whirlwind rings
/// up the victim's column (`0x801F7C44`, the loop at `0x801F7BBC`).
pub const NOVA_WHIRLWIND_SPAWNS: [ModuleSpawn; 17] = {
    let mut out = [nova_spawn(0x801F_8E28, SpawnAnchor::VictimDepthScaled); 17];
    let mut i = 0;
    while i < 16 {
        out[i + 1] = nova_spawn(
            NOVA_RING_RECORD,
            SpawnAnchor::VictimColumn { index: i as u8 },
        );
        i += 1;
    }
    out
};

/// The three records arm 18 seats on the victim.
pub const NOVA_LIFT_RECORDS: [u32; 3] = [0x801F_8EDC, 0x801F_8F34, 0x801F_8F80];

/// PROT 0913 arm 18's three spawns on its pass, on the lifted victim at
/// the framing depth's scale (`0x801F8218..0x801F8260`); the module keeps
/// their handles and arm `0x13` drags them along with the victim.
pub const NOVA_LIFT_SPAWNS: [ModuleSpawn; 3] = [
    nova_spawn(NOVA_LIFT_RECORDS[0], SpawnAnchor::VictimDepthScaled),
    nova_spawn(NOVA_LIFT_RECORDS[1], SpawnAnchor::VictimDepthScaled),
    nova_spawn(NOVA_LIFT_RECORDS[2], SpawnAnchor::VictimDepthScaled),
];

/// PROT 0913 arm `0x13`'s two spawns on its pass, once the victim is back
/// on the floor facing the creature (`0x801F83B4` / `0x801F83D0`).
pub const NOVA_LANDING_SPAWNS: [ModuleSpawn; 2] = [
    nova_spawn(0x801F_9020, SpawnAnchor::VictimDepthScaled),
    nova_spawn(0x801F_90DC, SpawnAnchor::VictimDepthScaled),
];

/// PROT 0913 (Nova). `c` is the creature seat `actor_table[7]` (`s8`), `f`
/// its heading, `v` the victim `actor_table[caster[+0x1DD]]` (`s2`), `d` the
/// framing depth `ctx[+0x6D0]` and `P` the per-tick frame product.
///
/// | arm | when | camera |
/// |---:|---|---|
/// | 0 | its pass | cut: pitch `0x400`, yaw `0x900 - caster[+0x46]`, TR `(0, 0, 0x400)`, focus the caster (`0x801F6C50`) |
/// | 1, 2 | every pass | TR z `+= P / 4` |
/// | 2 | its pass | pitch `0x80`, yaw `0x900 - f`, TR `(0, 0x300, 0x800)`, focus `c`, over `0x40` frames (`0x801F6DB8`) |
/// | 3 | every pass | yaw `+= 8P` - one whole turn over the arm |
/// | 3 | its pass | yaw global `= 0x900 - f` (`0x801F6EB8`) |
/// | 4 | every pass | TR z `+= P / 4`, yaw `+= P / 4` |
/// | 4 | its pass | yaw global `= 0xA40 - f`, then pitch `-0x100`, yaw `0x940 - f`, TR `(0, 0x600, 0x400)`, focus `c`, over `0x40` frames (`0x801F6FE8`) |
/// | 5 | its pass | yaw `0x900 - f`, TR `(0, 0x800, 0x800)`, focus `c`, over `0x20` frames |
/// | 6 | its pass | the spell name; pitch `-0x80`, yaw `0x900 - f`, TR `(0, 0x400, 0x800)`, focus `c`, over `0x20` frames |
/// | 7 | its pass | the attack name; cut: pitch `-0x80`, yaw `0x700 - f`, TR `(0, 0x400, 0x600)`, focus `c` |
/// | 8 | its pass | pitch `-0x40`, yaw `0x780 - f`, TR `(0, 0x600, 0x400)`, focus `c`, over `0x80` frames |
/// | 9 | its pass | pitch `0x40`, yaw `0x800 - f`, TR `(0, 0x600, 0x800)`, focus `c`, over `0x80` frames |
/// | 11 | every pass | TR y `+= 4P`, TR z `+= 2P`, pitch `-= P / 4` |
/// | 12, 13 | its pass / every pass | pitch `-0x200`, yaw `0x800 - f`, TR `(0, 0x1000, 0x200)`, focus `c`, over `0x10` frames |
/// | 13 | its pass | the globals stored straight: pitch `-0x200`, yaw `0x800 - v[+0x46]`, TR y `0x1000`, TR z `0x200` (already scaled), focus `v` (`0x801F7B10..0x801F7B5C`) |
/// | 14 | its pass | pitch `0x40`, yaw `0x800 - v[+0x46]`, TR `(0, 0x600, d)`, focus `v`, over `0x40` frames |
/// | 15 | its pass | pitch `-0x300`, yaw `0x800 - v[+0x46]`, TR `(0, d / 2, -d / 2)`, focus `v`, over `0x100` frames |
/// | 16 | every pass | yaw `+= sin(n / 2) / 2`, `n` the climbing countdown (`0x801F8068..0x801F80A4`) |
/// | 17 | its pass | cut: pitch `-0x300`, yaw `-v[+0x46]`, TR `(0, 4d, 0)`, focus `v` |
/// | 18 | its pass | pitch `0x40`, yaw `0x800 - v[+0x46]`, TR `(0, 0x600, d)`, focus `v`, over `0x80` frames |
/// | 20 | every pass | `FUN_801D5854(caster, 8)`, the post-action framing |
///
/// Arm 0x10's yaw add is a fixed sine step a **pass**, not a multiple of the
/// frame product, so the engine's two ticks a battle frame each take half.
///
/// PORT: FUN_801F69F0 (PROT 0913; every camera arm, the drifts of arms 1..4, 11 and 16, the yaw stores of arms 3/4/13 and the arm 6/7 captions)
pub fn nova_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let caster = seats.caster;
    let v = seats.victim;
    let d = seats.depth_raw as i16;
    let p = MODULE_DRAIN_PER_TICK;
    run_body_paced(
        st,
        phase,
        BodyPacedArms {
            open: |_| {
                shot(
                    [0x400, yaw_from(0x900, caster.facing)],
                    [0, 0, 0x400],
                    focus_on(caster),
                    1,
                )
            },
            pass: |st, arm| {
                let cr = st.creature_live.or(st.creature).unwrap_or_default();
                let c = focus_on(cr);
                let f = cr.facing;
                // The victim turns under the climb and the landing (the
                // host runs arms `0x10` / `0x13`'s `+0x46` writes), so the
                // later framings read its live heading.
                let vf = v.facing;
                let on_v = |pitch: i16, k: i32, tr: [i16; 3], frames: u16| {
                    shot([pitch, yaw_from(k, vf)], tr, focus_on(v), frames)
                };
                match arm {
                    2 => ArmDirection {
                        spawns: &NOVA_VORTEX_SPAWNS,
                        ..shot([0x80, yaw_from(0x900, f)], [0, 0x300, 0x800], c, 0x40)
                    },
                    0x0A => ArmDirection {
                        spawns: &NOVA_CLOUD_SPAWNS,
                        ..ArmDirection::PASS
                    },
                    3 => set_yaw(st, i32::from(yaw_from(0x900, f))),
                    4 => ArmDirection {
                        drift: set_yaw(st, i32::from(yaw_from(0xA40, f))).drift,
                        ..shot([-0x100, yaw_from(0x940, f)], [0, 0x600, 0x400], c, 0x40)
                    },
                    5 => shot([0, yaw_from(0x900, f)], [0, 0x800, 0x800], c, 0x20),
                    6 => captioned(
                        shot([-0x80, yaw_from(0x900, f)], [0, 0x400, 0x800], c, 0x20),
                        ModuleCaption::SpellName,
                    ),
                    7 => captioned(
                        shot([-0x80, yaw_from(0x700, f)], [0, 0x400, 0x600], c, 1),
                        ModuleCaption::AttackName,
                    ),
                    8 => ArmDirection {
                        spawns: &NOVA_ARM8_SPAWNS,
                        ..shot([-0x40, yaw_from(0x780, f)], [0, 0x600, 0x400], c, 0x80)
                    },
                    9 => shot([0x40, yaw_from(0x800, f)], [0, 0x600, 0x800], c, 0x80),
                    0x0C => shot([-0x200, yaw_from(0x800, f)], [0, 0x1000, 0x200], c, 0x10),
                    // The stores land the scaled TR z `0x200` itself: the
                    // raw depth the tween builder scales to it.
                    0x0D => ArmDirection {
                        spawns: &NOVA_WHIRLWIND_SPAWNS,
                        ..on_v(-0x200, 0x800, [0, 0x1000, NOVA_SNAP_TR_Z_RAW], 1)
                    },
                    0x0E => on_v(0x40, 0x800, [0, 0x600, d], 0x40),
                    0x0F => on_v(-0x300, 0x800, [0, d / 2, d.wrapping_neg() / 2], 0x100),
                    0x11 => on_v(-0x300, 0, [0, d.wrapping_mul(4), 0], 1),
                    0x12 => ArmDirection {
                        spawns: &NOVA_LIFT_SPAWNS,
                        ..on_v(0x40, 0x800, [0, 0x600, d], 0x80)
                    },
                    // The hit's `jal 0x801F44A0` is followed by
                    // `FUN_800319A8(0)` (`0x801F8460`), inside the victim's
                    // targetable branch (`+0x16E & 4` clear, `0x801F8434`).
                    0x13 => ArmDirection {
                        spawns: &NOVA_LANDING_SPAWNS,
                        ..captioned(ArmDirection::PASS, ModuleCaption::Cleared)
                    },
                    _ => ArmDirection::PASS,
                }
            },
            each: |st, arm, ticks| match arm {
                1 | 2 => drift(0, 0, 0, p / 4),
                3 => drift(0, 8 * p, 0, 0),
                4 => drift(0, p / 4, 0, p / 4),
                0x0B => drift(-p / 4, 0, 4 * p, 2 * p),
                0x0D => {
                    let cr = st.creature_live.or(st.creature).unwrap_or_default();
                    shot(
                        [-0x200, yaw_from(0x800, cr.facing)],
                        [0, 0x1000, 0x200],
                        focus_on(cr),
                        0x10,
                    )
                }
                0x10 => {
                    // The body's countdown climbs `P / 2` a tick from the
                    // `0` arm 0x0F left.
                    let n = (p / 2) * (ticks + 1);
                    let (sin, _) = trig12(((n / 2) & 0xFFF) as u16);
                    drift(0, i32::from(sin) / 4, 0, 0)
                }
                0x14 => ArmDirection {
                    end_frame: true,
                    ..ArmDirection::PASS
                },
                _ => ArmDirection::PASS,
            },
        },
    )
}

/// The raw TR z that scales to the `0x200` arm 0x0D stores into the scaled
/// global: `(0x140 << 8) / 0xA0 = 0x200`.
const NOVA_SNAP_TR_Z_RAW: i16 = 0x140;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nova_snap_depth_scales_to_the_stored_global() {
        assert_eq!(
            legaia_engine_battle_vm::battle_cam_script::prescale_tr_z(i32::from(
                NOVA_SNAP_TR_Z_RAW
            )),
            512.0
        );
    }

    /// Arm 3's spin is one whole turn at the engine's arm length.
    #[test]
    fn nova_arm3_turns_once() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        nova_direct(&mut st, 2, seats);
        let mut yaw = 0;
        for _ in 0..64 {
            yaw += i32::from(nova_direct(&mut st, 3, seats).drift.unwrap().yaw);
        }
        assert_eq!(yaw, 0x1000);
    }

    /// The pass after arm 0x10 frames the victim on its live heading: the
    /// host turns the victim through the climb itself, so the framing reads
    /// whatever heading the climb left.
    #[test]
    fn nova_frames_the_victim_on_its_live_heading() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats {
            victim: ModuleSeat {
                facing: 0x345,
                ..Default::default()
            },
            ..Default::default()
        };
        nova_direct(&mut st, 0x11, seats);
        let s = nova_direct(&mut st, 0x12, seats)
            .shot
            .expect("arm 0x11's cut");
        assert_eq!(s.angles[1], yaw_from(0, 0x345));
    }

    /// Nova reports its spawn calls on the arms that make them, each on the
    /// next arm's first tick: the vortex (2), arm 8's twelve, the cloud banks (10), the
    /// whirlwind (13), the parts that ride the victim (18) and the landing
    /// flash (0x13).
    #[test]
    fn nova_reports_its_pass_spawns() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        let mut seen = Vec::new();
        for phase in 0u8..=0x14 {
            for _ in 0..2 {
                let d = nova_direct(&mut st, phase, seats);
                if !d.spawns.is_empty() {
                    seen.push((phase, d.spawns.len()));
                }
            }
        }
        assert_eq!(
            seen,
            vec![(3, 2), (9, 12), (0x0B, 2), (0x0E, 17), (0x13, 3), (0x14, 2)]
        );
        assert_eq!(
            NOVA_WHIRLWIND_SPAWNS[16].anchor,
            SpawnAnchor::VictimColumn { index: 15 }
        );
    }
}
