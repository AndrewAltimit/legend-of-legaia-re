//! The spawn arms of the slot-B stagers that a module's own **parts** call.
//!
//! A part's move program reaches its module's stager through op `0x20`
//! (`(*gp[0x714])(part, op[1], op[2])`, the module's `0x801F6734` row), and
//! the stager's arm seats more records off the calling part: its position
//! `+0x14` and rotation `+0x24` (`a0 = part + 0x14`, `a1 = part + 0x24`), or
//! a seat the arm reads. Some arms write into the part they just seated
//! (`FUN_80021B04` returns it in `v0`), after the seater's own move-VM run
//! has set the part up; one rewrites a record's bytecode before seating it.
//!
//! These are pure: each arm returns what it seats ([`StagerSpawn`]) and the
//! host seats it.

use legaia_engine_system::trig::trig12;

/// What a stager arm reads off the part that called it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StagerCaller {
    /// `+0x14..+0x18`.
    pub pos: [i16; 3],
    /// `+0x24..+0x28`.
    pub rot: [i16; 3],
    /// `+0xB4`, the caller's inner radius word (op `0x13`'s `v10`, ramped by
    /// the part tick).
    pub radius_b4: i16,
    /// Op `0x20`'s second operand, the stager's third argument (`a2`).
    pub arg: i16,
}

/// One `FUN_80021B04(pos, rot, record, 0x1000)` call out of a stager arm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagerSpawn {
    /// The record, by slot-B VA.
    pub record: u32,
    pub pos: [i16; 3],
    pub rot: [i16; 3],
    /// Halfword stores into the seated part after its seat run, as
    /// `(actor offset, value)`.
    pub pokes: Vec<(u16, u16)>,
    /// Halfword stores into the record's own bytes before it is seated, as
    /// `(VA, value)`.
    pub patches: Vec<(u32, u16)>,
    /// Halve the seated part's own speed `+0x98` after its seat run
    /// (`lhu; sll 0x10; sra 0x10; srl 0x1F; addu; sra 1`: a signed halving
    /// rounding toward zero).
    pub halve_speed: bool,
}

impl StagerSpawn {
    fn at(record: u32, pos: [i16; 3], rot: [i16; 3]) -> Self {
        Self {
            record,
            pos,
            rot,
            pokes: Vec::new(),
            patches: Vec::new(),
            halve_speed: false,
        }
    }
}

/// `sin` / `cos` of a 12-bit angle divided by 32, truncating toward zero
/// (`bgez; addiu 0x1F; sra 5`).
fn trunc32(v: i16) -> i16 {
    (i32::from(v) / 32) as i16
}

/// One ray of a radial burst - the eight-iteration loop both stagers run
/// (PROT 0907 `0x801F8080..0x801F8154`, PROT 0908 `0x801F83D4..0x801F84A0`):
/// the ray seats on the shared anchor, then the arm stores its wait
/// `+0x54 = i`, a random spin `+0x84`, a length `+0xBA`, two random radii
/// `+0xB4` / `+0xB6`, and its roll `+0x28 = i * 0x200 + (rand & 0x1FF)` -
/// eight rays around the full turn, each jittered inside its eighth.
#[allow(clippy::too_many_arguments)]
fn ray(
    record: u32,
    pos: [i16; 3],
    rot: [i16; 3],
    i: u16,
    spin: u16,
    length: i16,
    b4: (i32, i32),
    rand: &mut dyn FnMut() -> u32,
) -> StagerSpawn {
    let mut s = StagerSpawn::at(record, pos, rot);
    let r_b4 = rand() as i32;
    let r_b6 = rand() as i32;
    let r_28 = rand();
    s.pokes = vec![
        (0x54, i),
        (0x84, spin),
        (0xBA, length as u16),
        (0xB4, (r_b4 % b4.0 + b4.1) as u16),
        (0xB6, (r_b6 % 0xFF + 0x200) as u16),
        (0x28, i * 0x200 + (r_28 & 0x1FF) as u16),
    ];
    s
}

/// PROT 0907 (Nighto) - the stager `0x801F7FA8`, its four spawn arms.
///
/// | arm | caller | seats |
/// |---:|---|---|
/// | 0 | the crystal `0x801F82C0` | eight rays `0x801F8314` at `trunc(sin / 32)` / `trunc(cos / 32)` back along the caller's `+0x26`, `y = -0x15E` (`0x801F8004..0x801F8154`) |
/// | 1 | the summoning disc `0x801F81D0` | the ring `0x801F8370` on the caller, after storing the caller's `+0xB4 - 0x20` over the ring's two radius words `0x801F8388` / `0x801F838A` (`0x801F8160..0x801F8184`) |
/// | 2 | `0x801F83BC` | the ring `0x801F8414` on the caller |
/// | 3 | `0x801F845C` | the ring `0x801F84B8` on the caller |
///
/// A ray's spin is `((rand & 0xFF) - 0x80) / 2` (`srl 0x1F; addu; sra 1`),
/// its length `-0x160`, its radii `rand % 0x3F + 0x40` and
/// `rand % 0xFF + 0x200`.
///
/// PORT: FUN_801F7FA8 (PROT 0907 stager; every arm)
pub fn nighto_stager_spawns(
    arm: u8,
    caller: StagerCaller,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    match arm {
        0 => {
            let (sin, cos) = trig12(caller.rot[1] as u16 & 0xFFF);
            let pos = [
                caller.pos[0].wrapping_sub(trunc32(sin)),
                -0x15E,
                caller.pos[2].wrapping_sub(trunc32(cos)),
            ];
            (0..8u16)
                .map(|i| {
                    let spin = ((rand() & 0xFF) as i32 - 0x80) / 2;
                    ray(
                        0x801F_8314,
                        pos,
                        caller.rot,
                        i,
                        spin as u16,
                        -0x160,
                        (0x3F, 0x40),
                        rand,
                    )
                })
                .collect()
        }
        1 => {
            let r = caller.radius_b4.wrapping_sub(0x20) as u16;
            let mut s = StagerSpawn::at(0x801F_8370, caller.pos, caller.rot);
            s.patches = vec![(0x801F_838A, r), (0x801F_8388, r)];
            vec![s]
        }
        2 => vec![StagerSpawn::at(0x801F_8414, caller.pos, caller.rot)],
        3 => vec![StagerSpawn::at(0x801F_84B8, caller.pos, caller.rot)],
        _ => Vec::new(),
    }
}

/// PROT 0908 (Zenoir) - the stager `0x801F8310`, its three spawn arms.
///
/// | arm | caller | seats |
/// |---:|---|---|
/// | 0 | the summoning flash `0x801F8584` | the floor ring `0x801F869C` on the caller (`0x801F8368..0x801F8378`) |
/// | 1 | `0x801F880C` | the flash `0x801F8880` on the creature's body point `+0x3C..+0x40` (an unaligned `lwl` / `lwr` copy to the stack), then eight rays `0x801F8828` on the same point (`a0 = creature + 0x3C`), all at the caller's rotation (`0x801F8384..0x801F84A0`) |
/// | 2 | the slam `0x801F8AA0` | two blasts `0x801F8ABC` on the caller; the second's speed `+0x98` halved (`sra 1` at `0x801F84FC`) |
///
/// A ray's spin is `(rand & 0xFF) - 0x80`, its length `-0x80`, its radii
/// `rand % 0x1F + 0x20` and `rand % 0xFF + 0x200`.
///
/// PORT: FUN_801F8310 (PROT 0908 stager; every arm)
pub fn zenoir_stager_spawns(
    arm: u8,
    caller: StagerCaller,
    creature_body: [i16; 3],
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    match arm {
        0 => vec![StagerSpawn::at(0x801F_869C, caller.pos, caller.rot)],
        1 => {
            let mut out = vec![StagerSpawn::at(0x801F_8880, creature_body, caller.rot)];
            out.extend((0..8u16).map(|i| {
                let spin = (rand() & 0xFF) as i32 - 0x80;
                ray(
                    0x801F_8828,
                    creature_body,
                    caller.rot,
                    i,
                    spin as u16,
                    -0x80,
                    (0x1F, 0x20),
                    rand,
                )
            }));
            out
        }
        2 => {
            let first = StagerSpawn::at(0x801F_8ABC, caller.pos, caller.rot);
            let mut second = first.clone();
            second.halve_speed = true;
            vec![first, second]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counter() -> impl FnMut() -> u32 {
        let mut n = 0u32;
        move || {
            n += 1;
            n * 0x123
        }
    }

    /// Arm 0's eight rays share one anchor, `/ 32` back along the caller's
    /// heading at `y = -0x15E`, and fan their rolls one eighth apart.
    #[test]
    fn nighto_rays_fan_round_the_crystal() {
        let caller = StagerCaller {
            pos: [100, 0x200, -300],
            rot: [0, 0x400, 0],
            radius_b4: 0,
            arg: 0,
        };
        let mut r = counter();
        let s = nighto_stager_spawns(0, caller, &mut r);
        assert_eq!(s.len(), 8);
        let (sin, cos) = trig12(0x400);
        for (i, sp) in s.iter().enumerate() {
            assert_eq!(sp.record, 0x801F_8314);
            assert_eq!(
                sp.pos,
                [
                    100 - (sin as i32 / 32) as i16,
                    -0x15E,
                    -300 - (cos as i32 / 32) as i16
                ]
            );
            let roll = sp.pokes.iter().find(|p| p.0 == 0x28).unwrap().1;
            assert_eq!(roll >> 9, i as u16);
            let wait = sp.pokes.iter().find(|p| p.0 == 0x54).unwrap().1;
            assert_eq!(wait, i as u16);
            let b4 = sp.pokes.iter().find(|p| p.0 == 0xB4).unwrap().1;
            assert!((0x40..0x40 + 0x3F).contains(&b4));
            let spin = sp.pokes.iter().find(|p| p.0 == 0x84).unwrap().1 as i16;
            assert!((-0x40..0x40).contains(&spin));
        }
    }

    /// Arm 1 sizes the ring off the calling disc's live inner radius.
    #[test]
    fn nighto_ring_takes_the_disc_radius() {
        let caller = StagerCaller {
            radius_b4: 0x90,
            ..StagerCaller::default()
        };
        let s = nighto_stager_spawns(1, caller, &mut counter());
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].record, 0x801F_8370);
        assert!(s[0].patches.contains(&(0x801F_8388, 0x70)));
        assert!(s[0].patches.contains(&(0x801F_838A, 0x70)));
        assert!(nighto_stager_spawns(4, caller, &mut counter()).is_empty());
    }

    /// Arm 1 seats the flash and eight rays on the creature, arm 2 two
    /// blasts with the second slowed.
    #[test]
    fn zenoir_arms_seat_their_records() {
        let caller = StagerCaller {
            pos: [1, 2, 3],
            rot: [0, 0x100, 0],
            radius_b4: 0,
            arg: 0,
        };
        let body = [10, -20, 30];
        let s = zenoir_stager_spawns(1, caller, body, &mut counter());
        assert_eq!(s.len(), 9);
        assert_eq!(s[0].record, 0x801F_8880);
        assert!(s.iter().all(|x| x.pos == body && x.rot == caller.rot));
        assert!(s[1..].iter().all(|x| x.record == 0x801F_8828));
        let s = zenoir_stager_spawns(2, caller, body, &mut counter());
        assert_eq!(s.len(), 2);
        assert!(
            s.iter()
                .all(|x| x.record == 0x801F_8ABC && x.pos == caller.pos)
        );
        assert!(!s[0].halve_speed);
        assert!(s[1].halve_speed);
        let s = zenoir_stager_spawns(0, caller, body, &mut counter());
        assert_eq!(s[0].record, 0x801F_869C);
    }
}
