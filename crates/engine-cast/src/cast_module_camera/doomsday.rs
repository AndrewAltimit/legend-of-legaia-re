//! PROT 0965 (**Doomsday**, `0xB6`, the evolved Cort): the tick body
//! `FUN_801F69D8`'s camera arms, countdown gates, seat stores and part
//! spawns - the half the phase port
//! ([`crate::cast_module_ticks::doomsday_tick`], the party sweep) does not
//! carry. The fades and the colour-wash ramps (`ctx[+0x27A..]`, `+0x286`)
//! are the presentation decoder's.
//!
//! See `ghidra/scripts/funcs/overlay_cast_doomsday_0965_801f69d8.txt`.

use super::capture::{CaptureCamArm, CaptureDrift, CaptureSeatWrite};
use super::*;

/// PROT 0965's tick body (its load base).
pub const DOOMSDAY_BODY: u32 = 0x801F_69D8;

/// The x a party seat is lined up at for arm `0x0B`'s sweep: half of
/// `*(0x800775E0 + (ctx[+0] * 3 + seat) * 8)` (`0x801F773C..0x801F7778`),
/// by party count and seat. The seat's z goes to `0` and its heading to
/// `0x800`.
pub const DOOMSDAY_LINE_UP_X: [[i16; 3]; 3] = [[150, 0, 0], [0, 300, 0], [0, -300, 300]];

/// The countdown word `0x801F8370` counts **vsyncs**: each arm drains it by
/// the frame step alone (`lbu 0x393` / `subu`), not by `scalar * delta`. The
/// director keeps it scaled into [`ModuleCountdown`]'s unit.
fn vsyncs(n: i32) -> i32 {
    SPEED_SCALAR * n
}

/// The camera, countdown and seat half of **Doomsday** (PROT 0965, body
/// `0x801F69D8`, a `beq` / `slti` tree over `0..=0x0C` and `0xFF`), by the
/// module phase the body is about to run. `d` is the frame step (one engine
/// tick); `w` the countdown, in vsyncs:
///
/// | arm | every pass | gate | as it passes |
/// |---|---|---|---|
/// | 0 (`0x801F6B0C`) | - | - | cue `0x1C2`; caster clip `7` with the bump; shot pitch `0x100`, yaw `0x800 - facing`, TR `(0, 0x200, 0x1800)` on the caster, `0x20` frames; `w = 0x20` |
/// | 1 (`0x801F6B90`) | caster stage `0` | `w` | `+ 0x40`; the lift word `0x801F8374 = 0` |
/// | 2 (`0x801F6BC8`) | pitch `-12d`, TR y `+192d`, TR z `-224d`; the lift word `+ 16d`, caster height `-sin(lift) / 2` | `w` | `+ 0x40`; (the held fade to white) |
/// | 3 (`0x801F6CD4`) | the same drift | `w` | `+ 0x40`; pool flush; stage byte `3`, the backdrop off; the party and the caster hidden; three records at the origin |
/// | 4, 5 (`0x801F6ED0`, `0x801F6F4C`) | (the wash ramps) | `w` | `+ 0x100` / `+ 0xC0` |
/// | 6 (`0x801F6FE0`) | - | `w` | two records at the origin; `+ 0x100` |
/// | 7 (`0x801F70A8`) | - | `w` | `+ 0x100`; stage byte `0`, the backdrop back (`0x800`); the party shown; cut pitch `0x100`, yaw `-facing`, TR `(0, 0, 0x4650)` on the origin |
/// | 8 (`0x801F7250`) | yaw `+2d` | `w` | pool flush; the party hidden, the caster shown; cut pitch `-0x200`, yaw `0x800 - facing`, TR `(0, 0x2C00, 0x1000)` on the caster; two records and the ten-ring loop on the cut's trios ([`doomsday_ring_part_words`]); stage byte `3`, the backdrop off; `+ 0x100` |
/// | 9 (`0x801F7408`) | - | `w` | pool flush; the caster hidden; four records at the origin; cut TR `(-0x100, 0x100, -0x1964)` on the origin; `+ 0x80` |
/// | `0x0A` (`0x801F74EC`) | a `0x20`-frame shot TR `(0x200, 0, -0xA8C)` on the origin | `w` | seven records at the origin; `+ 0x40` |
/// | `0x0B` (`0x801F7648`) | - | `w` | pool flush; the party shown and lined up ([`DOOMSDAY_LINE_UP_X`], the host); (the `0x600` sweep: the phase port); cut TR `(0, 0x300, 0xC00)` on the origin; stage byte `0`, the backdrop back; `+ 0x80` |
/// | `0x0C` (`0x801F78E0`) | TR z `+4d` | `w`, then the target row's settle | (`0xFF`) |
/// | `0xFF` (`0x801F7A08`) | - | - | the party's seats restored (the host); caster height `0`; cut pitch `0`, yaw `0x800 - facing`, TR `(0, 0x800, 0x800)` on the caster |
///
/// The part-flag stores on held handles are not carried.
///
/// PORT: FUN_801F69D8 (PROT 0965; the camera arms, the countdown and settle
/// gates, the hides, the stage byte and backdrop stores, the spawns)
pub fn doomsday_camera(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> CaptureCamArm {
    let c = seats.caster;
    let origin = [0i16; 3];
    let shot = |pitch: i16, yaw: i16, tr: [i16; 3], focus: [i16; 3], frames: u16| ModuleShot {
        angles: [pitch, yaw, 0],
        tr,
        focus,
        frames,
    };
    let hidden = CaptureSeatWrite {
        colour: Some(0),
        render_flag: Some(0xFF),
        ..Default::default()
    };
    let shown = CaptureSeatWrite {
        colour: Some(0x2008_0200),
        render_flag: Some(0),
        ..Default::default()
    };
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    if phase == 0 {
        st.countdown.0 = vsyncs(0x20);
        arm.caster_write.stage = Some(7);
        arm.caster_write.restage = true;
        arm.shot = Some(shot(
            0x100,
            yaw_from(0x800, c.facing),
            [0, 0x200, 0x1800],
            focus_on(c),
            0x20,
        ));
        return arm;
    }
    if phase == 0xFF {
        arm.caster_write.y = Some(0);
        arm.shot = Some(shot(
            0,
            yaw_from(0x800, c.facing),
            [0, 0x800, 0x800],
            focus_on(c),
            1,
        ));
        return arm;
    }
    if !(1..=0x0C).contains(&phase) {
        return arm;
    }
    let [mut lift, _] = st.module_words;
    match phase {
        1 => arm.caster_write.stage = Some(0),
        2 | 3 => {
            arm.drift = Some(CaptureDrift {
                pitch: -12,
                tr_y: 192,
                tr_z: -224,
                ..Default::default()
            });
            if phase == 2 {
                lift += 16;
                let s = i32::from(legaia_engine_system::trig::sin12(lift as u16));
                arm.caster_write.y = Some(((-s) / 2) as i16);
            }
        }
        8 => {
            arm.drift = Some(CaptureDrift {
                yaw: 2,
                ..Default::default()
            })
        }
        0x0A => {
            arm.shot = Some(shot(0, 0, [0x200, 0, -0xA8C], origin, 0x20));
        }
        0x0C => {
            arm.drift = Some(CaptureDrift {
                tr_z: 4,
                ..Default::default()
            })
        }
        _ => {}
    }
    // Arm `0x0C` drains only while the word is positive (`blez` at
    // `0x801F7908`), then waits on the row.
    arm.hold = if phase == 0x0C {
        (st.countdown.0 > 0 && st.countdown.drain_above(0)) || !st.party_settled
    } else {
        st.countdown.drain_above(0)
    };
    if arm.hold {
        st.module_words[0] = lift;
        return arm;
    }
    let re_arm = |st: &mut ModuleCamState, n: i32| st.countdown.0 += vsyncs(n);
    match phase {
        1 => {
            re_arm(st, 0x40);
            lift = 0;
        }
        2 => re_arm(st, 0x40),
        3 => {
            re_arm(st, 0x40);
            arm.ctx_278 = Some(3);
            arm.backdrop_off = true;
            arm.party_write = hidden;
            arm.caster_write = CaptureSeatWrite {
                stage: arm.caster_write.stage,
                ..hidden
            };
        }
        4 | 6 | 7 => re_arm(st, 0x100),
        5 => re_arm(st, 0xC0),
        8 => re_arm(st, 0x100),
        9 | 0x0B => re_arm(st, 0x80),
        0x0A => re_arm(st, 0x40),
        _ => {}
    }
    match phase {
        7 => {
            arm.ctx_278 = Some(0);
            arm.backdrop_on = true;
            arm.party_write = shown;
            arm.caster_write = hidden;
            arm.shot = Some(shot(
                0x100,
                yaw_from(0, c.facing),
                [0, 0, 0x4650],
                origin,
                1,
            ));
        }
        8 => {
            arm.party_write = hidden;
            arm.caster_write = shown;
            arm.shot = Some(shot(
                -0x200,
                yaw_from(0x800, c.facing),
                [0, 0x2C00, 0x1000],
                focus_on(c),
                1,
            ));
            arm.ctx_278 = Some(3);
            arm.backdrop_off = true;
        }
        9 => {
            arm.caster_write = hidden;
            arm.shot = Some(shot(0, 0, [-0x100, 0x100, -0x1964], origin, 1));
        }
        0x0B => {
            arm.party_write = shown;
            arm.party_line_up = true;
            arm.shot = Some(shot(0, 0, [0, 0x300, 0xC00], origin, 1));
            arm.ctx_278 = Some(0);
            arm.backdrop_on = true;
        }
        _ => {}
    }
    st.module_words[0] = lift;
    arm
}

const fn at_origin(record: u32) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor: SpawnAnchor::Literal {
            pos: [0; 3],
            rot: [0; 3],
        },
    }
}

/// `0x801F6E24..0x801F6EC4`, arm 3: three records at the origin (the middle
/// one through `FUN_80021B04`).
pub(super) const E965_3: [ModuleSpawn; 3] = [
    at_origin(0x801F_7C2C),
    at_origin(0x801F_7C8C),
    at_origin(0x801F_7CA8),
];
/// `0x801F7014..0x801F704C`, arm 6.
pub(super) const E965_6: [ModuleSpawn; 2] = [at_origin(0x801F_7D74), at_origin(0x801F_7DF0)];
const fn on_shot(record: u32) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor: SpawnAnchor::ShotFocus,
    }
}

/// The record arm 8's loop seats ten times (`addiu a2,s3,0x7ee4` at
/// `0x801F73CC`).
pub const DOOMSDAY_RING_RECORD: u32 = 0x801F_7EE4;
/// How many times arm 8's loop seats [`DOOMSDAY_RING_RECORD`]: `s2` runs
/// `0xC, 0x18, .. 0x78` (`sltiu v0,s2,0x80` at `0x801F73F4`).
pub const DOOMSDAY_RING_COUNT: usize = 10;

/// `0x801F7364..0x801F7400`, arm 8: two records, then the loop's ten, every
/// one on the trios the arm's cut was built from - the caster's negated
/// pair `(-x, 0, -z)` at `sp+0x20` and the angles `(-0x200, 0x800 -
/// facing, 0)` at `sp+0x10` ([`SpawnAnchor::ShotFocus`]), not the origin:
/// arm 8 is the one arm whose stack holds its shot when it spawns.
pub(super) const E965_8: [ModuleSpawn; 12] = [
    on_shot(0x801F_7E58),
    on_shot(0x801F_7EC8),
    on_shot(DOOMSDAY_RING_RECORD),
    on_shot(DOOMSDAY_RING_RECORD),
    on_shot(DOOMSDAY_RING_RECORD),
    on_shot(DOOMSDAY_RING_RECORD),
    on_shot(DOOMSDAY_RING_RECORD),
    on_shot(DOOMSDAY_RING_RECORD),
    on_shot(DOOMSDAY_RING_RECORD),
    on_shot(DOOMSDAY_RING_RECORD),
    on_shot(DOOMSDAY_RING_RECORD),
    on_shot(DOOMSDAY_RING_RECORD),
];

/// The part words arm 8 stores into the `n`-th part its loop seats
/// (`0x801F73D8..0x801F73F0`, `k = n + 1`): `+0xB6 = 0x240 k` and
/// `+0xB4 = 0x300 k` (`sh` through `a1 = part + 0x80`, `+0x36` / `+0x34`)
/// and the level `+0x78 = 0x180 k` - ten nested rings, each wider and
/// brighter than the last. `None` past the loop or for any other record.
pub fn doomsday_ring_part_words(record: u32, n: usize) -> Option<[(u16, u16); 3]> {
    if record != DOOMSDAY_RING_RECORD || n >= DOOMSDAY_RING_COUNT {
        return None;
    }
    let k = n as u16 + 1;
    Some([(0xB6, 0x240 * k), (0xB4, 0x300 * k), (0x78, 0x180 * k)])
}
/// `0x801F7430..0x801F74A8`, arm 9.
pub(super) const E965_9: [ModuleSpawn; 4] = [
    at_origin(0x801F_7F3C),
    at_origin(0x801F_7FAC),
    at_origin(0x801F_7FF8),
    at_origin(0x801F_81D4),
];
/// `0x801F7534..0x801F75E0`, arm `0x0A`.
pub(super) const E965_A: [ModuleSpawn; 7] = [
    at_origin(0x801F_8044),
    at_origin(0x801F_80A8),
    at_origin(0x801F_810C),
    at_origin(0x801F_8170),
    at_origin(0x801F_8238),
    at_origin(0x801F_82A0),
    at_origin(0x801F_8308),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_eight_seats_ten_nested_rings_on_its_cut() {
        // Every arm-8 record sits on the cut's trios, not the origin.
        assert!(E965_8.iter().all(|s| s.anchor == SpawnAnchor::ShotFocus));
        let rings = E965_8
            .iter()
            .filter(|s| s.record == SpawnRecord::Module(DOOMSDAY_RING_RECORD))
            .count();
        assert_eq!(rings, DOOMSDAY_RING_COUNT);
        assert_eq!(
            doomsday_ring_part_words(DOOMSDAY_RING_RECORD, 0),
            Some([(0xB6, 0x240), (0xB4, 0x300), (0x78, 0x180)])
        );
        assert_eq!(
            doomsday_ring_part_words(DOOMSDAY_RING_RECORD, 9),
            Some([(0xB6, 0x1680), (0xB4, 0x1E00), (0x78, 0xF00)])
        );
        assert_eq!(doomsday_ring_part_words(DOOMSDAY_RING_RECORD, 10), None);
        assert_eq!(doomsday_ring_part_words(0x801F_7E58, 0), None);
    }

    #[test]
    fn the_word_counts_vsyncs_and_the_last_arm_waits_on_the_row() {
        let seats = ModuleCamSeats::default();
        let mut st = ModuleCamState::default();
        doomsday_camera(&mut st, 0, seats);
        let dwell = |st: &mut ModuleCamState, phase: u8| {
            (1..)
                .find(|_| !doomsday_camera(st, phase, seats).hold)
                .unwrap()
        };
        assert_eq!(dwell(&mut st, 1), 0x20);
        assert_eq!(dwell(&mut st, 2), 0x40);
        let a = (0..)
            .map(|_| doomsday_camera(&mut st, 3, seats))
            .find(|a| !a.hold)
            .unwrap();
        assert_eq!(a.party_write.render_flag, Some(0xFF));
        assert_eq!(dwell(&mut st, 4), 0x40);
        assert_eq!(dwell(&mut st, 5), 0x100);
        // Arm `0x0C` holds on an unsettled row once its word is spent.
        st.countdown.0 = 0;
        assert!(doomsday_camera(&mut st, 0x0C, seats).hold);
        st.party_settled = true;
        assert!(!doomsday_camera(&mut st, 0x0C, seats).hold);
    }
}
