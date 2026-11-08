//! PROT 0950's **Shadow Break** body (`0x801F6A24`, action `0xAB`, Jette): the
//! camera arms, the party hide and the records, over
//! [`crate::cast_arm_ticks::rolling_flare_sweep_tick`].
//!
//! See `ghidra/scripts/funcs/overlay_cast_rolling_flare_0950_801f6a24.txt`.

use super::capture::{CaptureCamArm, CaptureDrift, CapturePartWrite};
use super::capture_countdown::{ROLLING_FLARE_SWEEP, arm_countdown};
use super::v_windhash::{HIDDEN, SHOWN};
use super::{
    ModuleCamSeats, ModuleCamState, ModuleShot, ModuleSpawn, SPEED_SCALAR, SpawnAnchor,
    SpawnRecord, focus_on, yaw_from,
};
use legaia_engine_system::trig::trig12;

/// PROT 0950's Shadow Break tick body.
pub const SHADOW_BREAK_BODY: u32 = crate::cast_arm_ticks::ROLLING_FLARE_SWEEP_TICK;

/// Arm 0's record on the caster (`0x801F6B28`).
pub const SHADOW_BREAK_OPENING_RECORD: u32 = 0x801F_85EC;
/// Arm 2's two records on the caster (`0x801F6CC4`, `0x801F6CEC`). Arm 9 halts
/// the first through the handle `0x801F86D0`.
pub const SHADOW_BREAK_BLADES: [u32; 2] = [0x801F_8240, 0x801F_82A0];
/// Arm 8's three records on the caster (`0x801F72F0`, `0x801F7334`, `0x801F734C`).
pub const SHADOW_BREAK_RISE: [u32; 3] = [0x801F_8430, 0x801F_848C, 0x801F_84E8];
/// Arm 9's record on the caster (`0x801F74AC`).
pub const SHADOW_BREAK_FALL_RECORD: u32 = 0x801F_8590;
/// Arm 10's record on the caster (`0x801F754C`).
pub const SHADOW_BREAK_BURST_RECORD: u32 = 0x801F_8654;
/// The three records arm 10 seats at each of its thirteen ring points
/// (`0x801F763C`, `0x801F7678`, `0x801F7690`).
const RING_RECORDS: [u32; 3] = [0x801F_8430, 0x801F_84E8, 0x801F_8544];
/// Every record the body seats: arm 11 stores `+0x90 = 0x200` through its whole
/// handle list (`0x801C90F0`, `0x801F78B4..0x801F78D4`).
static FADED_RECORDS: [u32; 9] = [
    SHADOW_BREAK_OPENING_RECORD,
    SHADOW_BREAK_BLADES[0],
    SHADOW_BREAK_BLADES[1],
    SHADOW_BREAK_RISE[0],
    SHADOW_BREAK_RISE[1],
    SHADOW_BREAK_RISE[2],
    SHADOW_BREAK_FALL_RECORD,
    SHADOW_BREAK_BURST_RECORD,
    RING_RECORDS[2],
];

/// Arm 10's ring (`0x801F759C..0x801F76A4`): thirteen points at
/// `facing + 0x9D + i * 0x1000 / 13`, `5 * sin / 12` round the caster on the
/// floor, three records each on the caster's rotation.
pub fn shadow_break_ring(ground: (i16, i16), facing: u16) -> Vec<ModuleSpawn> {
    let (x0, z0) = ground;
    (0..13i32)
        .flat_map(|i| {
            let angle = i32::from(facing) + 0x9D + i * 0x1000 / 13;
            let (sin, cos) = trig12((angle & 0xFFF) as u16);
            let at = |t: i16| ((i32::from(t) * 5) / 12) as i16;
            let pos = [x0.wrapping_add(at(sin)), 0, z0.wrapping_add(at(cos))];
            RING_RECORDS.map(|record| ModuleSpawn {
                record: SpawnRecord::Module(record),
                anchor: SpawnAnchor::LiteralScaled {
                    pos,
                    rot: [0, facing as i16, 0],
                    scale: 0x1000,
                },
            })
        })
        .collect()
}

/// **Shadow Break** (a `sltiu 0xE` jump table at the image head; caster
/// `actor_table[ctx + 0x13]`, word `0x801F86B0`):
///
/// | arm | drift a vsync | as it passes |
/// |---|---|---|
/// | 0 (`0x801F6AC8`) | - | pitch `0`, yaw `0x800 - facing`, TR `(0, 0x400, 0x600)`, `0xC` frames |
/// | 1 (`0x801F6B50`) | TR z `+s`; every party seat hidden | pitch `0x180`, TR `(0, -0x200, 0x600)`, `0x20` frames |
/// | 2 (`0x801F6C80`) | - | the party shown |
/// | 3 (`0x801F6DEC`) | TR z `+5s`, TR y `+s/8`, pitch `-1`, yaw `+2s` | - |
/// | 4 (`0x801F6F04`) | TR z `+5s` | pitch `-0x20`, yaw `0x600 - facing`, TR `(0, 0x5A0, 0x600)` |
/// | 5, 6 (`0x801F6FD4`, `0x801F7088`) | TR z `+s/4`, pitch `+1`, yaw `-s` | arm 6: pitch `-0x80`, yaw `0x700 - facing`, TR `(0, 0x700, 0x600)` |
/// | 7 (`0x801F71A0`) | pitch `-1`, TR y `+2s` | pitch `0x80`, TR `(0, 0x200, 0x600)` |
/// | 8 (`0x801F7280`) | TR z `+8s`, TR y `-s` | - |
/// | 9, 10, 11 | TR z `+8s` | arm 11 fades every record |
/// | 12, 13 | - | - |
///
/// The gates and re-arms are [`ROLLING_FLARE_SWEEP`]'s, which a director turns off for
/// its body, so this one runs them. Clips, rates and the sweep are the tick's;
/// the armed washes, the screen fades and the stage byte the presentation
/// decoder's; the records the spawn tables'.
///
/// PORT: overlay_cast_rolling_flare_0950_801f6a24 (PROT 0950 `0xAB`; the camera arms, the party hide and the records)
pub fn shadow_break_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let s = SPEED_SCALAR as i16;
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    let drift = |tr_z: i16, tr_y: i16, pitch: i16, yaw: i16| {
        Some(CaptureDrift {
            tr_z,
            tr_y,
            pitch,
            yaw,
            ..Default::default()
        })
    };
    let shot = |pitch: i16, yaw_base: i32, tr_y: i16, frames: u16| ModuleShot {
        angles: [pitch, yaw_from(yaw_base, c.facing), 0],
        tr: [0, tr_y, 0x600],
        focus: focus_on(c),
        frames,
    };
    arm.drift = match phase {
        1 => drift(s, 0, 0, 0),
        3 => drift(5 * s, s / 8, -1, 2 * s),
        4 => drift(5 * s, 0, 0, 0),
        5 | 6 => drift(s / 4, 0, 1, -s),
        7 => drift(0, 2 * s, -1, 0),
        8 => drift(8 * s, -s, 0, 0),
        9..=11 => drift(8 * s, 0, 0, 0),
        _ => None,
    };
    if phase == 1 {
        arm.party_write = HIDDEN;
    }
    if let Some(a) = arm_countdown(ROLLING_FLARE_SWEEP, phase) {
        if a.holds(&mut st.countdown) {
            arm.hold = true;
            return arm;
        }
        a.pass(&mut st.countdown);
    }
    match phase {
        0 => arm.shot = Some(shot(0, 0x800, 0x400, 0xC)),
        1 => {
            arm.shot = Some(shot(0x180, 0x800, -0x200, 0x20));
            arm.ctx_278 = Some(2);
        }
        2 => arm.party_write = SHOWN,
        4 => arm.shot = Some(shot(-0x20, 0x600, 0x5A0, 1)),
        6 => arm.shot = Some(shot(-0x80, 0x700, 0x700, 1)),
        7 => {
            arm.shot = Some(shot(0x80, 0x800, 0x200, 1));
            arm.ctx_278 = Some(3);
        }
        11 => {
            arm.part_write = Some(CapturePartWrite {
                records: &FADED_RECORDS,
                level_rate: Some(0x200),
                ..Default::default()
            });
        }
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::super::ModuleSeat;
    use super::*;

    fn seats() -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 665,
                y: 0,
                z: 969,
                facing: 2287,
            },
            depth_raw: 4915,
            ..Default::default()
        }
    }

    #[test]
    fn arm_zero_cuts_behind_the_caster_and_seeds_the_word() {
        let mut st = ModuleCamState::default();
        let a = shadow_break_camera(&mut st, 0, seats());
        let shot = a.shot.expect("a shot");
        assert_eq!(shot.tr, [0, 0x400, 0x600]);
        assert_eq!(shot.frames, 0xC);
        assert_eq!(st.countdown.0, SPEED_SCALAR << 8);
    }

    #[test]
    fn arm_one_drifts_and_hides_the_party_while_it_holds() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = 1 << 20;
        let a = shadow_break_camera(&mut st, 1, seats());
        assert!(a.hold);
        assert_eq!(a.drift.map(|d| d.tr_z), Some(8));
        assert_eq!(a.party_write.render_flag, Some(0xFF));
    }

    #[test]
    fn arm_three_walks_every_axis() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = 1 << 20;
        let d = shadow_break_camera(&mut st, 3, seats())
            .drift
            .expect("drift");
        assert_eq!((d.tr_z, d.tr_y, d.pitch, d.yaw), (40, 1, -1, 16));
    }

    #[test]
    fn the_ring_is_thirteen_triples_round_the_caster() {
        let ring = shadow_break_ring((100, -50), 0);
        assert_eq!(ring.len(), 39);
    }
}
