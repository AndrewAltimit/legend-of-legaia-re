//! The arm clock the body-paced directors (PROT 0910..0913) share.
//!
//! A body-paced director runs before its module's ported tick body and sees
//! only the phase the body is about to run; whether that pass lets the arm
//! through is the body's own countdown's call. What a retail arm does on its
//! **pass** - the `FUN_801D829C` call after its gate, a caption - therefore
//! lands on the first tick of the arm the pass moved to, one engine tick (half
//! a retail battle frame) after retail's. Since retail's tween only starts
//! stepping on the frame after the arm armed it, the shot is not visibly late.
//!
//! What an arm does on **every** pass - the drifts it adds to the camera
//! globals ahead of its gate, a shot it re-arms before the gate - runs on
//! every tick the body spends in the arm.
//!
//! The clock lives in the module countdown word, which none of these
//! directors uses as a countdown: `(phase + 1) << 16 | ticks in the arm`.

use super::*;

/// Where the director stands this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ArmTick {
    /// The director's first call this cast.
    First,
    /// The first tick of this arm: arm `prev` passed on the last one.
    From(u8),
    /// A later tick in the same arm, counted from `1`.
    Within(i32),
}

/// Read and advance the arm clock for `phase`.
pub(super) fn arm_tick(st: &mut ModuleCamState, phase: u8) -> ArmTick {
    let word = st.countdown.0;
    let memo = word >> 16;
    let here = i32::from(phase) + 1;
    let tick = if word == 0 || memo == 0 {
        ArmTick::First
    } else if memo != here {
        ArmTick::From((memo - 1) as u8)
    } else {
        ArmTick::Within((word & 0xFFFF) + 1)
    };
    let ticks = match tick {
        ArmTick::Within(n) => n,
        _ => 0,
    };
    st.countdown.0 = (here << 16) | (ticks & 0xFFFF);
    tick
}

/// One body-paced director's arms: `open` is arm 0's own shot, `pass` what
/// an arm does on the pass that leaves it, `each` what the arm does on every
/// pass (given the ticks it has spent there, `0` on its first).
pub(super) struct BodyPacedArms<O, P, E>
where
    O: Fn(&mut ModuleCamState) -> ArmDirection,
    P: Fn(&mut ModuleCamState, u8) -> ArmDirection,
    E: Fn(&mut ModuleCamState, u8, i32) -> ArmDirection,
{
    pub open: O,
    pub pass: P,
    pub each: E,
}

/// Run one tick of a body-paced director. On an arm's first tick the arm
/// that just passed contributes its pass shot and caption; the arm's own
/// every-pass drift runs as well, and its every-pass shot only where the
/// pass armed none. Drifts sum.
pub(super) fn run_body_paced<O, P, E>(
    st: &mut ModuleCamState,
    phase: u8,
    arms: BodyPacedArms<O, P, E>,
) -> ArmDirection
where
    O: Fn(&mut ModuleCamState) -> ArmDirection,
    P: Fn(&mut ModuleCamState, u8) -> ArmDirection,
    E: Fn(&mut ModuleCamState, u8, i32) -> ArmDirection,
{
    match arm_tick(st, phase) {
        ArmTick::First if phase == 0 => (arms.open)(st),
        ArmTick::First => (arms.each)(st, phase, 0),
        ArmTick::Within(n) => (arms.each)(st, phase, n),
        ArmTick::From(prev) => {
            let passed = (arms.pass)(st, prev);
            let own = (arms.each)(st, phase, 0);
            // The pass's `FUN_80021B04` / `FUN_80050ED4` calls, fades and
            // VRAM moves land with its shot, on the next arm's first tick.
            ArmDirection {
                shot: passed.shot.or(own.shot),
                caption: passed.caption.or(own.caption),
                drift: sum_drift(passed.drift, own.drift),
                end_frame: passed.end_frame || own.end_frame,
                spawns: if passed.spawns.is_empty() {
                    own.spawns
                } else {
                    passed.spawns
                },
                fades: if passed.fades.is_empty() {
                    own.fades
                } else {
                    passed.fades
                },
                kills_fades: passed.kills_fades || own.kills_fades,
                vram_move: passed.vram_move.or(own.vram_move),
                ..ArmDirection::PASS
            }
        }
    }
}

fn sum_drift(a: Option<CaptureDrift>, b: Option<CaptureDrift>) -> Option<CaptureDrift> {
    match (a, b) {
        (Some(a), Some(b)) => Some(CaptureDrift {
            pitch: a.pitch.wrapping_add(b.pitch),
            yaw: a.yaw.wrapping_add(b.yaw),
            tr_y: a.tr_y.wrapping_add(b.tr_y),
            tr_z: a.tr_z.wrapping_add(b.tr_z),
            tr_x: a.tr_x.wrapping_add(b.tr_x),
        }),
        (a, b) => a.or(b),
    }
}

/// A drift of the camera globals by these per-tick amounts.
pub(super) fn drift(pitch: i32, yaw: i32, tr_y: i32, tr_z: i32) -> ArmDirection {
    ArmDirection::PASS.drifted(CaptureDrift {
        pitch: pitch as i16,
        yaw: yaw as i16,
        tr_y: tr_y as i16,
        tr_z: tr_z as i16,
        tr_x: 0,
    })
}

/// A store of `yaw` straight into the yaw global `0x8007B792`, as the drift
/// from the live yaw that lands it (shortest way round). With no live yaw
/// the store is dropped.
pub(super) fn set_yaw(st: &ModuleCamState, yaw: i32) -> ArmDirection {
    match st.live_yaw {
        Some(live) => {
            let d = ((yaw - i32::from(live)) & 0xFFF) as i16;
            let d = if d >= 0x800 { d - 0x1000 } else { d };
            drift(0, i32::from(d), 0, 0)
        }
        None => ArmDirection::PASS,
    }
}

/// One `FUN_801D829C` call.
pub(super) fn shot(angles: [i16; 2], tr: [i16; 3], focus: [i16; 3], frames: u16) -> ArmDirection {
    ArmDirection::shot(ModuleShot {
        angles: [angles[0], angles[1], 0],
        tr,
        focus,
        frames,
    })
}

/// The same call with a caption put up beside it.
pub(super) fn captioned(d: ArmDirection, caption: ModuleCaption) -> ArmDirection {
    ArmDirection {
        caption: Some(caption),
        ..d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_reports_entries_and_dwell() {
        let mut st = ModuleCamState::default();
        assert_eq!(arm_tick(&mut st, 0), ArmTick::First);
        assert_eq!(arm_tick(&mut st, 0), ArmTick::Within(1));
        assert_eq!(arm_tick(&mut st, 0), ArmTick::Within(2));
        assert_eq!(arm_tick(&mut st, 1), ArmTick::From(0));
        assert_eq!(arm_tick(&mut st, 5), ArmTick::From(1));
        assert_eq!(arm_tick(&mut st, 5), ArmTick::Within(1));
    }

    #[test]
    fn a_yaw_store_takes_the_short_way_round() {
        let st = ModuleCamState {
            live_yaw: Some(0xF00),
            ..Default::default()
        };
        assert_eq!(set_yaw(&st, 0x100).drift.map(|d| d.yaw), Some(0x200));
        let st = ModuleCamState {
            live_yaw: Some(0x100),
            ..Default::default()
        };
        assert_eq!(set_yaw(&st, 0xF00).drift.map(|d| d.yaw), Some(-0x200));
    }
}
