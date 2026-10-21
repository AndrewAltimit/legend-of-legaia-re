//! The stagers of PROT 0918 (Kemaro), 0924 (Ultimate Rave), 0925
//! (Spikefish) and 0934 (Ozma): the routines their parts reach through op
//! `0x20` (`(*gp[0x714])(part, arm, arg)`), the siblings of
//! [`super::creature_stager_spawns`] whose arms do more than seat records
//! on the caller - PROT 0918's arm 0 keeps a handle its module lifts,
//! PROT 0925's arm 1 turns and moves its caller and keeps a handle table its
//! module halts, and PROT 0934's arm 0 seats on the caller's `+0x3C` trio.
//!
//! Pure: each arm returns what it seats ([`StagerSpawn`]); PROT 0925's
//! writes into its caller go through the [`StagerCaller`] it is handed.
//!
//! See `ghidra/scripts/funcs/overlay_summon_kemaro_0918_801f8ab0.txt`,
//! `overlay_stager_ultimate_rave_0924_801f7820.txt`,
//! `overlay_summon_spikefish_0925_801f7ae8.txt` and
//! `overlay_summon_ozma_0934_801f92ac.txt`.

use super::{StagerCaller, StagerSpawn};
use legaia_engine_system::trig::trig12;

fn spawn(record: u32, pos: [i16; 3], rot: [i16; 3]) -> StagerSpawn {
    StagerSpawn {
        record,
        pos,
        rot,
        pokes: Vec::new(),
        patches: Vec::new(),
        halve_speed: false,
        scale: 0x1000,
    }
}

/// A seat on the calling part's point and angles at `0x1000`.
fn on(record: u32, caller: &StagerCaller) -> StagerSpawn {
    spawn(record, caller.pos, caller.rot)
}

/// `rand() % n`: `rand()` (`FUN_80056798`) never returns a negative, so the
/// arms' `mult` / shift sequences compute the plain remainder.
fn rem(rand: &mut dyn FnMut() -> u32, n: u32) -> i32 {
    (rand() % n) as i32
}

/// The record PROT 0918's arm 0 seats, whose handle it keeps in
/// `0x801F92C4` for the module's arm 4 to lift.
pub const KEMARO_LIFTED_RECORD: u32 = 0x801F_8C40;
/// PROT 0918's arm-4 lift a battle frame, in units of `scalar * step`
/// (`0x801F7140..0x801F716C`: `+0x16 -= 6 * scalar * step`).
pub const KEMARO_LIFT_PER_SCALAR: i32 = 6;
/// The height PROT 0918's arm 4 stops the lifted part at
/// (`slti v0,v0,-0x800` at `0x801F7180`).
pub const KEMARO_LIFT_CEILING: i16 = -0x800;

/// PROT 0918 (Kemaro) - the stager `0x801F8AB0`, three arms, every seat by
/// `FUN_80021B04` on the caller at `0x1000`:
///
/// | arm | seats |
/// |---:|---|
/// | 0 | [`KEMARO_LIFTED_RECORD`], its handle stored to `0x801F92C4` (`0x801F8B00..0x801F8B1C`) |
/// | 1 | `0x801F8DF0` |
/// | 2 | sixteen `0x801F8E34`, the `k`-th with its roll `+0x28 = k << 8` (`0x801F8B40..0x801F8B68`) |
///
/// The handle is the module's: its arm 4 lifts that part
/// ([`KEMARO_LIFT_PER_SCALAR`]); the host finds it by its record.
///
/// PORT: FUN_801F8AB0 (PROT 0918 stager; every arm)
pub fn kemaro_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    match arm {
        0 => vec![on(KEMARO_LIFTED_RECORD, &caller)],
        1 => vec![on(0x801F_8DF0, &caller)],
        2 => (0..16u16)
            .map(|k| {
                let mut s = on(0x801F_8E34, &caller);
                s.pokes = vec![(0x28, k << 8)];
                s
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// PROT 0924 (Ultimate Rave) - the stager `0x801F7820`: arm 0 seats
/// `0x801F7898`, arm 1 `0x801F7910`, each on the caller at `0x1000` through
/// the pool wrapper `FUN_80050ED4`.
///
/// PORT: FUN_801F7820 (PROT 0924 stager; both arms)
pub fn ultimate_rave_stager_spawns(arm: u8, caller: StagerCaller) -> Vec<StagerSpawn> {
    match arm {
        0 => vec![on(0x801F_7898, &caller)],
        1 => vec![on(0x801F_7910, &caller)],
        _ => Vec::new(),
    }
}

/// PROT 0925's two stager words: the turn counter `0x801F8014` and the
/// ring count `0x801F8018` (the fill of the handle table `0x801F809C`),
/// both zeroed by the module's arm 0 (`0x801F6BEC`, `0x801F6BF8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpikefishStagerWords {
    pub turn: u32,
    pub ring: u32,
}

/// The quarter-turn table `0x801F8008` PROT 0925's arm 1 steps through by
/// its turn counter (`andi v1,t1,0x3`; `lbu t2,0x0(v1)`; `sll t2,t2,0xa`).
pub const SPIKEFISH_TURN_QUARTERS: [u8; 4] = [0, 2, 3, 1];
/// The size of PROT 0925's handle table `0x801F809C` (`slti v0,v0,0x10` at
/// `0x801F7BD8`).
pub const SPIKEFISH_RING_SLOTS: u32 = 16;
/// The two ring records PROT 0925's arm 1 seats, by the ring count's parity
/// (`andi v1,v1,0x1` at `0x801F7CC8`): odd, then even. The module's arm 7
/// halts every one the handle table holds (`0x801F7228..0x801F7254`).
pub const SPIKEFISH_RING_RECORDS: [u32; 2] = [0x801F_7F98, 0x801F_7FD0];
/// The arm whose pass halts PROT 0925's ring (`0x801F71F0`), and the phase
/// arm 1 seats it in (`li v0,0x7` at `0x801F7BC4`).
pub const SPIKEFISH_RING_ARM: u8 = 7;

/// PROT 0925 (Spikefish) - the stager `0x801F7AE8`, two arms, every seat
/// through the pool wrapper `FUN_80050ED4` at `0x1000`:
///
/// - arm 0 seats `0x801F7D58` on the caller.
/// - arm 1 turns the caller's roll `+0x28` to
///   `(rand % 1025 + (SPIKEFISH_TURN_QUARTERS[turn & 3] << 10)) & 0xFFF`,
///   steps the turn counter and seats `0x801F7F50` on the caller
///   (`0x801F7B38..0x801F7BB0`). While the module sits in arm 7 with fewer
///   than sixteen ring parts, it then moves the caller: `z = 0x200`, the roll
///   a half turn on, a radius `r = rand % 129 + 0x40`, `x = sin(roll) * r >> 12`,
///   `y = cos(roll) * r >> 12` (each rounded toward zero), and seats a ring
///   part on the caller's point with the angles `(3y, -3x, 0)` before its
///   `y` dropped by `0x20`, then the caller's `y` drops `0x20`
///   (`0x801F7BE4..0x801F7D20`). The ring part goes in the handle table.
///
/// The caller's writes land in `caller`, for the host to store back.
///
/// PORT: FUN_801F7AE8 (PROT 0925 stager; both arms)
pub fn spikefish_stager_spawns(
    arm: u8,
    caller: &mut StagerCaller,
    phase: u8,
    words: &mut SpikefishStagerWords,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    match arm {
        0 => vec![on(0x801F_7D58, caller)],
        1 => {
            let quarter = u16::from(SPIKEFISH_TURN_QUARTERS[(words.turn & 3) as usize]) << 10;
            words.turn = words.turn.wrapping_add(1);
            let roll = (rem(rand, 1025) as u16).wrapping_add(quarter) & 0xFFF;
            caller.rot[2] = roll as i16;
            let mut out = vec![on(0x801F_7F50, caller)];
            if phase == SPIKEFISH_RING_ARM && words.ring < SPIKEFISH_RING_SLOTS {
                caller.pos[2] = 0x200;
                let radius = rem(rand, 129) + 0x40;
                let roll = (caller.rot[2] as u16).wrapping_add(0x800) & 0xFFF;
                caller.rot[2] = roll as i16;
                let (sin, cos) = trig12(roll);
                let x = (i32::from(sin) * radius / 4096) as i16;
                let y = (i32::from(cos) * radius / 4096) as i16;
                caller.pos[0] = x;
                let angles = [(3 * i32::from(y)) as i16, (-3 * i32::from(x)) as i16, 0];
                caller.pos[1] = y.wrapping_sub(0x20);
                let record = SPIKEFISH_RING_RECORDS[usize::from(words.ring & 1 == 0)];
                out.push(spawn(record, caller.pos, angles));
                words.ring += 1;
            }
            out
        }
        _ => Vec::new(),
    }
}

/// PROT 0934's stager word `0x801FA410`: the volley count arms 1 and 2
/// step, zeroed by the module's arms 0 (`0x801F6C3C`) and 10
/// (`0x801F78F8`).
pub const OZMA_STAGER_WORD: u32 = 0x801F_A410;

/// PROT 0934's two volley records: the spark `0x801F9E28` and the bolt
/// `0x801F9F2C`.
pub const OZMA_SPARK_RECORD: u32 = 0x801F_9E28;
/// See [`OZMA_SPARK_RECORD`].
pub const OZMA_BOLT_RECORD: u32 = 0x801F_9F2C;

fn wait(mut s: StagerSpawn, w: i32) -> StagerSpawn {
    s.pokes = vec![(0x54, w as u16)];
    s
}

/// Arm 3's `x` push off the centre line: a value in `0..0x100` moves out
/// by `0x200`, then one in `-0xFF..=-1` moves out the other way
/// (`0x801F9A18..0x801F9A4C`, both `sltiu ..,0x100` on the halfword).
fn push_out(x: i16) -> i16 {
    let mut x = x;
    if (x as u16) < 0x100 {
        x = x.wrapping_add(0x200);
    }
    if (x as u16).wrapping_add(0xFF) < 0x100 {
        x = x.wrapping_sub(0x200);
    }
    x
}

/// PROT 0934 (Ozma) - the stager `0x801F92AC`, four arms. Every volley seat
/// is `FUN_80021B04` on the caller's angles `+0x24` at `0x1000` with the
/// seated part's wait `+0x54` drawn from `rand()` after it; sin / cos are
/// the `0x8007B81C` / `0x8007B7F8` tables and every division rounds toward
/// zero.
///
/// | arm | seats |
/// |---:|---|
/// | 0 | `0x801F9C78`, `0x801F9CC4`, `0x801F9DD0` on the caller's `+0x3C` trio and angles, through `FUN_80050ED4` |
/// | 1 | the word steps (held at `7` from `8`); for each of `(w + 2) / 3` rows `k`: four sparks at angle `j * 0x200 + rand % 512` (`x = cos / 6`, `y = -0x140 (k + 2) + rand % 161`, `z = sin / 12 - 0x500`, wait `rand % 256`), then one or two bolts at angle `rand % 2048` (`x = cos / 7`, `y = -0xC0 (k + 2) + rand % 97`, `z = sin / 13 - 0x500`, wait `rand % 256`; a set low bit of a third roll skips the second) |
/// | 2 | the word steps; eight sparks at angle `j * 0x200 + rand % 256` and two bolts at angle `rand % 4096`, `x = cos / 6`, `y = rand % 33 - 0x160 w - 0xD0`, `z = -0x4E0`, wait `rand % 64`; at word `4` two more sparks at `(0, -0x610, -0x4E0)` with waits `0x60` and `0xC0` |
/// | 3 | twenty-four sparks at angle `j * 4096 / 24 + rand % 85` and sixteen bolts at angle `rand % 4096`, `x = cos / 5` pushed out of `-0xFF..0x100` by `0x200`, `y = rand % 129 - 0x580` (sparks) / `- 0x500` (bolts), `z = -0x4E0`, wait `rand % 512` |
///
/// PORT: FUN_801F92AC (PROT 0934 stager; every arm)
pub fn ozma_stager_spawns(
    arm: u8,
    caller: StagerCaller,
    word: &mut i32,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<StagerSpawn> {
    let rot = caller.rot;
    let mut out = Vec::new();
    match arm {
        0 => {
            for record in [0x801F_9C78, 0x801F_9CC4, 0x801F_9DD0] {
                out.push(spawn(record, caller.disp, rot));
            }
        }
        1 => {
            *word += 1;
            if *word >= 8 {
                *word = 7;
            }
            let (mut row_y, mut bolt_y) = (-0x140i32, -0xC0i32);
            let mut k = 0;
            while k < (*word + 2) / 3 {
                for j in 0..4i32 {
                    let a = rem(rand, 512);
                    let y = row_y + rem(rand, 161) - 0x140;
                    let (sin, cos) = trig12((j * 0x200 + a) as u16);
                    let pos = [
                        (i32::from(cos) / 6) as i16,
                        y as i16,
                        (i32::from(sin) / 12 - 0x500) as i16,
                    ];
                    let s = spawn(OZMA_SPARK_RECORD, pos, rot);
                    out.push(wait(s, rem(rand, 256)));
                }
                let mut j = 0;
                while j < 2 {
                    let a = rem(rand, 2048);
                    let y = bolt_y + rem(rand, 97) - 0xC0;
                    let (sin, cos) = trig12(a as u16);
                    let pos = [
                        (i32::from(cos) / 7) as i16,
                        y as i16,
                        (i32::from(sin) / 13 - 0x500) as i16,
                    ];
                    let s = spawn(OZMA_BOLT_RECORD, pos, rot);
                    out.push(wait(s, rem(rand, 256)));
                    if rand() & 1 != 0 {
                        j += 1;
                    }
                    j += 1;
                }
                bolt_y -= 0xC0;
                row_y -= 0x140;
                k += 1;
            }
        }
        2 => {
            *word += 1;
            let w = *word;
            let mut volley =
                |n: i32, spread: Option<u32>, record: u32, out: &mut Vec<StagerSpawn>| {
                    for j in 0..n {
                        let angle = match spread {
                            Some(m) => j * 0x200 + rem(rand, m),
                            None => rem(rand, 4096),
                        };
                        let y = rem(rand, 33) - 0x160 * w - 0xD0;
                        let (_, cos) = trig12(angle as u16);
                        let pos = [(i32::from(cos) / 6) as i16, y as i16, -0x4E0];
                        let s = spawn(record, pos, rot);
                        out.push(wait(s, rem(rand, 64)));
                    }
                };
            volley(8, Some(256), OZMA_SPARK_RECORD, &mut out);
            volley(2, None, OZMA_BOLT_RECORD, &mut out);
            if w == 4 {
                for wt in [0x60, 0xC0] {
                    out.push(wait(spawn(OZMA_SPARK_RECORD, [0, -0x610, -0x4E0], rot), wt));
                }
            }
        }
        3 => {
            for j in 0..24i32 {
                let a = rem(rand, 85);
                let y = rem(rand, 129) - 0x580;
                let (_, cos) = trig12((j * 4096 / 24 + a) as u16);
                let pos = [push_out((i32::from(cos) / 5) as i16), y as i16, -0x4E0];
                let s = spawn(OZMA_SPARK_RECORD, pos, rot);
                out.push(wait(s, rem(rand, 512)));
            }
            for _ in 0..16 {
                let a = rem(rand, 4096);
                let y = rem(rand, 129) - 0x500;
                let (_, cos) = trig12(a as u16);
                let pos = [push_out((i32::from(cos) / 5) as i16), y as i16, -0x4E0];
                let s = spawn(OZMA_BOLT_RECORD, pos, rot);
                out.push(wait(s, rem(rand, 512)));
            }
        }
        _ => {}
    }
    out
}

/// The records the stagers of PROT 0918, 0924, 0925 and 0934 seat, which
/// leave their module's whole-set seat.
pub fn late_creature_stager_records(entry: u32) -> &'static [u32] {
    match entry {
        918 => &[KEMARO_LIFTED_RECORD, 0x801F_8DF0, 0x801F_8E34],
        924 => &[0x801F_7898, 0x801F_7910],
        925 => &[
            0x801F_7D58,
            0x801F_7F50,
            SPIKEFISH_RING_RECORDS[0],
            SPIKEFISH_RING_RECORDS[1],
        ],
        934 => &[
            0x801F_9C78,
            0x801F_9CC4,
            0x801F_9DD0,
            OZMA_SPARK_RECORD,
            OZMA_BOLT_RECORD,
        ],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counter() -> impl FnMut() -> u32 {
        let mut n = 0u32;
        move || {
            n = n.wrapping_add(0x1357);
            n & 0x7FFF
        }
    }

    fn caller() -> StagerCaller {
        StagerCaller {
            pos: [10, -20, 30],
            rot: [0, 0x7C0, 0x123],
            disp: [5, -6, 7],
            scale: 0x800,
            ..Default::default()
        }
    }

    #[test]
    fn kemaro_arm_two_rolls_sixteen_copies() {
        let s = kemaro_stager_spawns(2, caller());
        assert_eq!(s.len(), 16);
        assert_eq!(s[3].pokes, vec![(0x28, 0x300)]);
        assert!(
            s.iter()
                .all(|sp| sp.scale == 0x1000 && sp.pos == caller().pos)
        );
    }

    #[test]
    fn spikefish_rings_only_in_arm_seven_and_up_to_sixteen() {
        let mut r = counter();
        let mut w = SpikefishStagerWords::default();
        let mut c = caller();
        let s = spikefish_stager_spawns(1, &mut c, 6, &mut w, &mut r);
        assert_eq!(s.len(), 1);
        assert_eq!(c.pos, caller().pos);
        let quarter = i32::from(c.rot[2]) >> 10;
        assert_eq!(quarter, 0);
        for n in 0..20 {
            let mut c = caller();
            let s = spikefish_stager_spawns(1, &mut c, SPIKEFISH_RING_ARM, &mut w, &mut r);
            if n < 16 {
                assert_eq!(s.len(), 2);
                assert_eq!(c.pos[2], 0x200);
                assert_eq!(s[1].pos, c.pos);
                assert_eq!(s[1].record, SPIKEFISH_RING_RECORDS[usize::from(n % 2 == 0)]);
                let x = i32::from(c.pos[0]);
                let y = i32::from(c.pos[1]) + 0x20;
                assert!(x * x + y * y <= 0xC0 * 0xC0 + 4);
            } else {
                assert_eq!(s.len(), 1);
            }
        }
        assert_eq!(w.ring, 16);
    }

    #[test]
    fn ozma_word_holds_at_seven_and_rows_follow_it() {
        let mut r = counter();
        let mut w = 0;
        let s = ozma_stager_spawns(1, caller(), &mut w, &mut r);
        assert_eq!(w, 1);
        // One row: four sparks and one or two bolts.
        assert!((5..=6).contains(&s.len()));
        let mut w = 7;
        let s = ozma_stager_spawns(1, caller(), &mut w, &mut r);
        assert_eq!(w, 7);
        let sparks = s.iter().filter(|p| p.record == OZMA_SPARK_RECORD).count();
        assert_eq!(sparks, 12);
        let mut w = 3;
        let s = ozma_stager_spawns(2, caller(), &mut w, &mut r);
        assert_eq!(s.len(), 12);
        assert_eq!(s[10].pokes, vec![(0x54, 0x60)]);
        let s = ozma_stager_spawns(0, caller(), &mut w, &mut r);
        assert!(s.iter().all(|p| p.pos == caller().disp));
    }

    #[test]
    fn ozma_arm_three_keeps_off_the_centre_line() {
        let mut r = counter();
        let mut w = 0;
        let s = ozma_stager_spawns(3, caller(), &mut w, &mut r);
        assert_eq!(s.len(), 40);
        for sp in &s {
            assert!(!(-0xFF..0x100).contains(&sp.pos[0]), "{}", sp.pos[0]);
        }
        assert_eq!(push_out(0), 0x200);
        assert_eq!(push_out(-1), -0x201);
        assert_eq!(push_out(0x100), 0x100);
    }

    #[test]
    fn listed_records_cover_every_arm() {
        let mut r = counter();
        for arm in 0..4u8 {
            let mut w = 3;
            let mut sw = SpikefishStagerWords::default();
            let mut c = caller();
            let all = [
                (918, kemaro_stager_spawns(arm, caller())),
                (924, ultimate_rave_stager_spawns(arm, caller())),
                (
                    925,
                    spikefish_stager_spawns(arm, &mut c, SPIKEFISH_RING_ARM, &mut sw, &mut r),
                ),
                (934, ozma_stager_spawns(arm, caller(), &mut w, &mut r)),
            ];
            for (entry, spawns) in all {
                for sp in spawns {
                    assert!(
                        late_creature_stager_records(entry).contains(&sp.record),
                        "{entry} arm {arm}: {:x}",
                        sp.record
                    );
                }
            }
        }
    }
}
