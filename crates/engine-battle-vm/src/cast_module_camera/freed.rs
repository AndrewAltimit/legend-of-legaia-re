//! PROT 0912 (Freed)'s director, beside its ported tick body
//! (`crate::seru_magic_ticks::freed_tick`).

use super::body_paced::*;
use super::*;

/// The TR y global PROT 0912's arm 13 climbs to and holds at (`slti
/// v0,v0,0xc01` / `li v0,0xc00` at `0x801F7A60..0x801F7A6C`).
pub const FREED_RISE_CAP: i32 = 0xC00;

/// The TR y arm 11's cut leaves, where arm 13's climb starts.
const FREED_RISE_FROM: i32 = 0x600;

/// PROT 0912 (Freed). `c` is the creature seat `actor_table[7]` (`s2`) and
/// `P` the per-tick frame product. Arm 2 stores the creature's heading `0`
/// (`sh zero,0x46(s2)` at `0x801F6C54`) and nothing turns it after, so every
/// creature-relative yaw below is the literal less `0`.
///
/// | arm | when | camera |
/// |---:|---|---|
/// | 0 | its pass | cut: pitch `0x40`, TR `(0, 0x800, 0x1000)`, focus the caster (`0x801F6BD4`) |
/// | 2 | its pass, the creature seated | TR `(0, 0x600, 0x1200)`, focus `c`, over `0x80` frames (`0x801F6C78`) |
/// | 3 | its pass | TR `(0, 0x800, 0x800)`, focus `c`, over `0x40` frames (`0x801F6F24`) |
/// | 4 | every pass | yaw `+= 4P` (`sh v0,0x2(v1)` in the delay slot of the gate at `0x801F6F78`) |
/// | 4 | its pass | cut: pitch `-0x40`, TR `(0, 0x800, 0x800)`, focus `c` (`0x801F7004`) |
/// | 5, 6 | every pass | yaw `+= P`, TR z `+= 2P` |
/// | 7 | every pass | yaw `+= P`, TR y `-= P`, TR z `+= 8P` |
/// | 8 | every pass | yaw `+= P`, TR z `-= 4P` |
/// | 8 | its pass | the spell name; pitch `0x40`, yaw `0x840`, TR `(0x100, 0x300, 0xA00)`, focus `c`, over `0x30` frames (`0x801F76E0`) |
/// | 10 | its pass | the attack name; cut: pitch `0x100`, yaw `0x980`, TR `(0, 0x300, 0x800)`, focus `c` (`0x801F783C`) |
/// | 11 | its pass | cut: yaw `0x800`, TR `(0x140, 0x600, 0x600)`, focus `c` (`0x801F78B0`) |
/// | 13 | every pass | TR y `+= 4P`, held at [`FREED_RISE_CAP`] |
/// | 13 | its pass | cut: pitch `-0x200`, yaw `0xA00`, TR `(0x200, 0x800, 0x100)`, focus `c` (`0x801F7AEC`) |
/// | 14 | every pass | TR y `+= 2P` |
/// | 14 | its pass | cut: yaw `0x800`, TR `(0, 0x2000, 0x600)`, focus the origin (`0x801F7BCC`) |
/// | 15 | every pass | TR z `-= P / 2` |
/// | 15 | its pass | pitch `0x180`, yaw `0x800`, TR `(0, 0, 0x1000)`, focus the origin, over `0x40` frames (`0x801F7E00`) |
/// | 16 | every pass | yaw `+= 2P` |
/// | 17 | every pass | yaw `+= 2P`, TR z `+= 4P`, pitch `-= P / 2` |
/// | 18 | every pass | TR z `+= 4P`, pitch `+= P / 4` |
/// | 19 | every pass | TR z `-= P` |
///
/// Arm 0x0B's wait (the creature's clip cursor reaching `0x49`) and arm
/// 0x13's (every enemy seat idle) are the body's, not camera waits: their
/// length follows the creature's and the monsters' animation rates.
///
/// PORT: FUN_801F69D8 (PROT 0912; every camera arm, the drifts of arms 4..8 and 13..19, and the arm 8/10 captions)
pub fn freed_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let caster = seats.caster;
    let p = MODULE_DRAIN_PER_TICK;
    run_body_paced(
        st,
        phase,
        BodyPacedArms {
            open: |_| shot([0x40, 0], [0, 0x800, 0x1000], focus_on(caster), 1),
            pass: |st, arm| {
                let c = focus_on(st.creature_live.or(st.creature).unwrap_or_default());
                match arm {
                    2 => shot([0, 0], [0, 0x600, 0x1200], c, 0x80),
                    3 => shot([0, 0], [0, 0x800, 0x800], c, 0x40),
                    4 => shot([-0x40, 0], [0, 0x800, 0x800], c, 1),
                    8 => captioned(
                        shot([0x40, 0x840], [0x100, 0x300, 0xA00], c, 0x30),
                        ModuleCaption::SpellName,
                    ),
                    0x0A => captioned(
                        shot([0x100, 0x980], [0, 0x300, 0x800], c, 1),
                        ModuleCaption::AttackName,
                    ),
                    0x0B => shot([0, 0x800], [0x140, 0x600, 0x600], c, 1),
                    0x0D => shot([-0x200, 0xA00], [0x200, 0x800, 0x100], c, 1),
                    0x0E => shot([0, 0x800], [0, 0x2000, 0x600], [0; 3], 1),
                    0x0F => shot([0x180, 0x800], [0, 0, 0x1000], [0; 3], 0x40),
                    _ => ArmDirection::PASS,
                }
            },
            each: |_, arm, ticks| match arm {
                4 => drift(0, 4 * p, 0, 0),
                5 | 6 => drift(0, p, 0, 2 * p),
                7 => drift(0, p, -p, 8 * p),
                8 => drift(0, p, 0, -4 * p),
                0x0D => {
                    let at = FREED_RISE_FROM + 4 * p * ticks;
                    drift(0, 0, (FREED_RISE_CAP - at).clamp(0, 4 * p), 0)
                }
                0x0E => drift(0, 0, 2 * p, 0),
                0x0F => drift(0, 0, 0, -p / 2),
                0x10 => drift(0, 2 * p, 0, 0),
                0x11 => drift(-p / 2, 2 * p, 0, 4 * p),
                0x12 => drift(p / 4, 0, 0, 4 * p),
                0x13 => drift(0, 0, 0, -p),
                _ => ArmDirection::PASS,
            },
        },
    )
}

const fn spawn_at(va: u32, anchor: SpawnAnchor) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(va),
        anchor,
    }
}

const fn on_creature(va: u32, dx: i16, y: i16, dz: i16, rot: [i16; 3]) -> ModuleSpawn {
    spawn_at(va, SpawnAnchor::CreatureOffset { dx, y, dz, rot })
}

/// PROT 0912's three ice-crystal records, one triple per corner: arm 2
/// spawns them four times over and parks the twelve handles at `0x801F92B0`;
/// arm 4 turns them round the creature and kills them on its pass.
pub const FREED_CRYSTAL_RECORDS: [u32; 3] = [0x801F_852C, 0x801F_85A4, 0x801F_861C];

/// The corners arm 2 seats the crystal triples on, in handle order, as
/// `(dx, dz)` off the creature: the stack trio stepped `-0x200 / -0x200`,
/// then `x += 0x400`, `z += 0x400`, `x -= 0x400` between the triples
/// (`0x801F6D30..0x801F6E54`).
pub const FREED_CRYSTAL_CORNERS: [(i16, i16); 4] = [
    (-0x200, -0x200),
    (0x200, -0x200),
    (0x200, 0x200),
    (-0x200, 0x200),
];

const fn crystal(corner: usize, k: usize) -> ModuleSpawn {
    let (dx, dz) = FREED_CRYSTAL_CORNERS[corner];
    on_creature(FREED_CRYSTAL_RECORDS[k], dx, 0x200, dz, [0; 3])
}

/// PROT 0912 arm 2's sixteen spawns (`0x801F6C80..0x801F6E8C`), right behind
/// the seat installer, on the creature's `+0x34..+0x3B` with the zeroed
/// angle trio: three glows at `y = -0x200`, a ground ring at `y = 0x200`,
/// and the four crystal triples ([`FREED_CRYSTAL_CORNERS`]).
pub const FREED_CRYSTAL_SPAWNS: [ModuleSpawn; 16] = [
    on_creature(0x801F_8708, 0, -0x200, 0, [0; 3]),
    on_creature(0x801F_876C, 0, -0x200, 0, [0; 3]),
    on_creature(0x801F_87D0, 0, -0x200, 0, [0; 3]),
    on_creature(0x801F_8694, 0, 0x200, 0, [0; 3]),
    crystal(0, 0),
    crystal(0, 1),
    crystal(0, 2),
    crystal(1, 0),
    crystal(1, 1),
    crystal(1, 2),
    crystal(2, 0),
    crystal(2, 1),
    crystal(2, 2),
    crystal(3, 0),
    crystal(3, 1),
    crystal(3, 2),
];

const fn burst(va: u32, pitch: i16, roll: i16) -> ModuleSpawn {
    on_creature(va, 0, -0x200, 0, [pitch, 0, roll])
}

/// PROT 0912 arm 4's fifteen spawns on its pass (`0x801F700C..0x801F712C`),
/// on the creature at `y = -0x200`: the flash `0x801F8960`, four
/// `0x801F883C` blades rolled a quarter turn apart and two more pitched
/// `0x400` / `0xC00`, then four pairs of `0x801F889C` at roll `0x200 +
/// 0x400 k`, pitched `0x200` and `0x600` - the star the crystals burst into.
pub const FREED_BURST_SPAWNS: [ModuleSpawn; 15] = [
    burst(0x801F_8960, 0, 0),
    burst(0x801F_883C, 0, 0),
    burst(0x801F_883C, 0, 0x400),
    burst(0x801F_883C, 0, 0x800),
    burst(0x801F_883C, 0, 0xC00),
    burst(0x801F_883C, 0x400, 0),
    burst(0x801F_883C, 0xC00, 0),
    burst(0x801F_889C, 0x200, 0x200),
    burst(0x801F_889C, 0x600, 0x200),
    burst(0x801F_889C, 0x200, 0x600),
    burst(0x801F_889C, 0x600, 0x600),
    burst(0x801F_889C, 0x200, 0xA00),
    burst(0x801F_889C, 0x600, 0xA00),
    burst(0x801F_889C, 0x200, 0xE00),
    burst(0x801F_889C, 0x600, 0xE00),
];

/// PROT 0912 arm 12's eight spawns on its pass (`0x801F7910..0x801F7A10`):
/// the creature's trio at `x + 0x50`, `y = -0x200`, `z - 0x20`, zeroed
/// angles - the strike's frost.
pub const FREED_FROST_SPAWNS: [ModuleSpawn; 8] = [
    on_creature(0x801F_89C8, 0x50, -0x200, -0x20, [0; 3]),
    on_creature(0x801F_8A3C, 0x50, -0x200, -0x20, [0; 3]),
    on_creature(0x801F_8A88, 0x50, -0x200, -0x20, [0; 3]),
    on_creature(0x801F_8B04, 0x50, -0x200, -0x20, [0; 3]),
    on_creature(0x801F_8B80, 0x50, -0x200, -0x20, [0; 3]),
    on_creature(0x801F_8BFC, 0x50, -0x200, -0x20, [0; 3]),
    on_creature(0x801F_8C78, 0x50, -0x200, -0x20, [0; 3]),
    on_creature(0x801F_8D50, 0x50, -0x200, -0x20, [0; 3]),
];

const fn field(va: u32, y: i16) -> ModuleSpawn {
    spawn_at(
        va,
        SpawnAnchor::Literal {
            pos: [0, y, 0],
            rot: [0, 0x800, 0],
        },
    )
}

/// PROT 0912 arm 14's ten spawns on its pass (`0x801F7BD4..0x801F7CC4`), on
/// the stage origin with the arm's yaw `0x800`: three at the origin, the
/// rest `0x800` above it (`sh v0,0x32(sp)` with `-0x800` in the delay slot
/// at `0x801F7C38`) - the ice field and its shatter.
pub const FREED_FIELD_SPAWNS: [ModuleSpawn; 10] = [
    field(0x801F_90A0, 0),
    field(0x801F_91E0, 0),
    field(0x801F_9254, 0),
    field(0x801F_8E0C, -0x800),
    field(0x801F_8E90, -0x800),
    field(0x801F_8F14, -0x800),
    field(0x801F_8F98, -0x800),
    field(0x801F_901C, -0x800),
    field(0x801F_9110, -0x800),
    field(0x801F_9178, -0x800),
];

/// The spawn calls PROT 0912's tick makes on arm `arm`'s pass.
pub fn freed_arm_spawns(arm: u8) -> &'static [ModuleSpawn] {
    match arm {
        2 => &FREED_CRYSTAL_SPAWNS,
        4 => &FREED_BURST_SPAWNS,
        0x0C => &FREED_FROST_SPAWNS,
        0x0E => &FREED_FIELD_SPAWNS,
        _ => &[],
    }
}

/// The deformation row arm 7 hands the creature's node (`+0x6D = 1`) and
/// writes into `0x8007BE60` (`0x801F750C..0x801F7528`): offset `(0, 0, 0)`
/// with `+6 = -0x400`, flags `0xC`.
pub const FREED_SHATTER_ROW: u8 = 1;
/// See [`FREED_SHATTER_ROW`].
pub const FREED_SHATTER_ROW_OFFSET: [i16; 4] = [0, 0, 0, -0x400];
/// See [`FREED_SHATTER_ROW`].
pub const FREED_SHATTER_ROW_FLAGS: u32 = 0xC;

/// The module scalar `lbu 0x69(0x1F800314)` arm 4 divides its radius by.
const FREED_SCALAR: i32 = 8;

/// The step arm 4 takes off its turn word `0x801F92AC` a pass: `rate *
/// speed << 3` (`0x801F71B8`), per engine tick the scalar's `8 << 3`.
pub const FREED_CRYSTAL_TURN_STEP: i32 = FREED_SCALAR << 3;

/// Where arm 4's held pass puts the four crystal corners
/// (`0x801F7184..0x801F730C`), as `(dx, dz)` off the creature, from the turn
/// word `turn` (after this pass's step) and the module countdown `left`
/// (after this pass's drain). The radius is `((181 * left) >> 4) / scalar`,
/// `0x200 * sqrt 2` when the arm opens on `scalar << 6`, and corner `k`
/// stands at angle `turn + 0x200 + 0x400 k`, each axis `trunc(sin * r /
/// 4096)` off the SCUS tables `0x8007B81C` / `0x8007B7F8`. The arm writes
/// the corner's first handle's `+0x14` / `+0x18` and copies that trio over
/// the other two.
pub fn freed_crystal_corners(turn: i32, left: i32) -> [(i16, i16); 4] {
    let mut scaled = 181 * left;
    if scaled < 0 {
        scaled += 15;
    }
    let r = (scaled >> 4) / FREED_SCALAR;
    let mut out = [(0, 0); 4];
    for (k, o) in out.iter_mut().enumerate() {
        let a = (turn + 0x200 + 0x400 * k as i32) as u16 & 0xFFF;
        let (sin, cos) = trig12(a);
        *o = (
            (i32::from(sin) * r / 4096) as i16,
            (i32::from(cos) * r / 4096) as i16,
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four corners open where arm 2 seated the triples' radius
    /// (`0x200 * sqrt 2` off `scalar << 6`), a quarter turn apart, and close
    /// on the creature as the countdown drains.
    #[test]
    fn freed_crystals_turn_in_on_the_creature() {
        let open = freed_crystal_corners(0, 8 << 6);
        for (dx, dz) in open {
            let r = f64::from(dx).hypot(f64::from(dz));
            assert!((r - 724.0).abs() < 2.0, "radius {r}");
        }
        // Angle `0x200 + 0x400 k`: corner 0 on +x / +z, corner 2 opposite.
        assert!(open[0].0 > 0 && open[0].1 > 0);
        assert_eq!((open[2].0, open[2].1), (-open[0].0, -open[0].1));
        assert_eq!(freed_crystal_corners(-640, 0), [(0, 0); 4]);
        let half = freed_crystal_corners(0, 8 << 5);
        assert!(half[0].0 < open[0].0);
    }

    /// The pass spawns: sixteen in arm 2, fifteen in arm 4, eight in arm 12,
    /// ten in arm 14 and nothing elsewhere.
    #[test]
    fn freed_arm_spawn_counts() {
        let n: Vec<usize> = (0..=0x13).map(|a| freed_arm_spawns(a).len()).collect();
        assert_eq!(n[2], 16);
        assert_eq!(n[4], 15);
        assert_eq!(n[0x0C], 8);
        assert_eq!(n[0x0E], 10);
        assert_eq!(n.iter().sum::<usize>(), 49);
        let crystals = FREED_CRYSTAL_SPAWNS
            .iter()
            .filter(|s| s.record == SpawnRecord::Module(FREED_CRYSTAL_RECORDS[0]))
            .count();
        assert_eq!(crystals, 4);
        let c = ModuleSeat {
            x: 100,
            y: 0,
            z: -800,
            facing: 0,
        };
        let at = spawn_anchor_point(FREED_CRYSTAL_SPAWNS[4].anchor, Some(c), None, None);
        assert_eq!(at, Some(([100 - 0x200, 0x200, -800 - 0x200], [0; 3])));
    }

    /// Arm 13's climb stops at the cap however long the arm holds.
    #[test]
    fn freed_climb_holds_at_the_cap() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        freed_direct(&mut st, 0x0C, seats);
        let mut climbed = 0;
        for _ in 0..200 {
            climbed += i32::from(freed_direct(&mut st, 0x0D, seats).drift.unwrap().tr_y);
        }
        assert_eq!(climbed, FREED_RISE_CAP - FREED_RISE_FROM);
    }

    /// The pass shots land on the next arm's first tick, once.
    #[test]
    fn freed_pass_shots_land_once() {
        let mut st = ModuleCamState::default();
        let seats = ModuleCamSeats::default();
        assert!(freed_direct(&mut st, 0, seats).shot.is_some());
        freed_direct(&mut st, 8, seats);
        let d = freed_direct(&mut st, 9, seats);
        assert_eq!(d.shot.map(|s| s.frames), Some(0x30));
        assert_eq!(d.caption, Some(ModuleCaption::SpellName));
        assert!(freed_direct(&mut st, 9, seats).shot.is_none());
    }
}
