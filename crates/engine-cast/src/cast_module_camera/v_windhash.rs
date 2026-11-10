//! PROT 0947's body (`0x801F69F0`, V-Windhash `0x57` and Neo Windhash `0xA7`):
//! the camera arms, the seat hides and the blade's per-pass trail.
//!
//! See `ghidra/scripts/funcs/overlay_cast_v_windhash_0947_801f69f0.txt`.

use super::{
    CaptureCamArm, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleShot, ModuleSpawn,
    SPEED_SCALAR, SpawnAnchor, SpawnRecord, bearing_12bit_approx, focus_on,
};
use legaia_engine_system::trig::trig12;

/// The module's countdown word (`lui 0x801F` / `0x7B40`).
pub const V_WINDHASH_COUNTDOWN: u32 = 0x801F_7B40;

/// Neo Windhash: its arm 0 cue differs and its arm 2 seats one more record
/// (`0x801F7090..0x801F70B0`).
const NEO_WINDHASH: u8 = 0xA7;

/// The two records arm 1 seats at `(0, 0, 0x100)` on the zeroed angles
/// (`0x801F6D40`, `0x801F6D60`).
const ARM1_RECORDS: [u32; 2] = [0x801F_7900, 0x801F_795C];
/// The pair seated on the blade's one side, and on its other
/// (`0x801F6F90`, `0x801F6FB0`; `0x801F7038`, `0x801F7060`).
const SIDE_A: [u32; 2] = [0x801F_79B8, 0x801F_7A00];
const SIDE_B: [u32; 2] = [0x801F_79DC, 0x801F_7A44];
/// Neo Windhash's extra record on arm 2's second side (`0x801F70A8`).
const NEO_RECORD: u32 = 0x801F_7A88;
/// Height of the blade's two sides (`li v0,-0x12C` / `sh v0,0x22(sp)`).
const BLADE_Y: i16 = -0x12C;
/// `+0x04` a shown seat holds (`lui 0x2008` / `ori 0x200`).
const SHOWN_COLOUR: u32 = 0x2008_0200;
pub(super) const SHOWN: CaptureSeatWrite = CaptureSeatWrite {
    colour: Some(SHOWN_COLOUR),
    render_flag: Some(0),
    ..EMPTY_WRITE
};
pub(super) const HIDDEN: CaptureSeatWrite = CaptureSeatWrite {
    colour: Some(0),
    render_flag: Some(0xFF),
    ..EMPTY_WRITE
};
const EMPTY_WRITE: CaptureSeatWrite = CaptureSeatWrite {
    rate: None,
    stage: None,
    restage: false,
    loop_release: false,
    x: None,
    z: None,
    dz: 0,
    y: None,
    dy: 0,
    facing: None,
    colour: None,
    blend: None,
    render_flag: None,
    cursor_reset: false,
    seek: None,
    seek_floor: None,
    stage_knockdown: false,
    ghost_ring_clear: false,
    alive_only: false,
};

pub(super) const fn seat(record: u32, pos: [i16; 3], rot: [i16; 3]) -> ModuleSpawn {
    ModuleSpawn {
        record: SpawnRecord::Module(record),
        anchor: SpawnAnchor::LiteralScaled {
            pos,
            rot,
            scale: 0x1000,
        },
    }
}

/// `(v * 96) >> 12` as `sll` / `addu` / `sll 5`, `addiu 0xFFF` and `sra 0xC`
/// take it: rounded toward zero.
fn blade_half(v: i16) -> i32 {
    (i32::from(v) * 96) / 0x1000
}

/// The two sides of the blade around the point `(x, z)`: `+-96` units across
/// the heading `yaw`, on the angle trio `(0, yaw, 0)`.
fn trail(x: i16, z: i16, yaw: i16, extra: Option<u32>) -> [Option<ModuleSpawn>; 5] {
    let (s, c) = trig12((yaw as u16).wrapping_add(0x400) & 0xFFF);
    let (dx, dz) = (blade_half(s), blade_half(c));
    let rot = [0, yaw, 0];
    let a = [
        (i32::from(x) + dx) as i16,
        BLADE_Y,
        (i32::from(z) + dz) as i16,
    ];
    let b = [
        (i32::from(x) - dx) as i16,
        BLADE_Y,
        (i32::from(z) - dz) as i16,
    ];
    [
        Some(seat(SIDE_A[0], a, rot)),
        Some(seat(SIDE_A[1], a, rot)),
        Some(seat(SIDE_B[0], b, rot)),
        Some(seat(SIDE_B[1], b, rot)),
        extra.map(|r| seat(r, b, rot)),
    ]
}

/// **V-Windhash / Neo Windhash** (`sltiu 6` through the table at
/// `0x801F69D8`; caster `actor_table[ctx + 0x13]`, victim
/// `actor_table[caster + 0x1DD]`):
///
/// | arm | camera and seats | countdown |
/// |---|---|---|
/// | 0 (`0x801F6AB4`) | the caster turns on the victim; every party seat and live monster hidden, the caster shown; shot on the caster: pitch `-0x20`, yaw `0x750 - facing`, TR `(0, 0x500, ctx[+0x6D0] / 3)`, `0x20` frames | seed `scalar << 5` |
/// | 1 (`0x801F6CFC`) | once the gate passes, two records, then shot on the caster: pitch `0`, the same yaw, TR `(0, 0x500, ctx[+0x6D0] / 2)`, `0x80` frames | `+= scalar << 7` |
/// | 2 (`0x801F6DEC`) | once the gate passes, the victim shown and the caster hidden; a cut `4096` units behind the victim along the caster's bearing: pitch `-0x40`, yaw `-facing`, TR `(0, 0x300, 0x800)`; the blade's first trail there | `+= scalar << 6` |
/// | 3 (`0x801F70BC`) | every pass, held or not: a cut closing on the victim as the word falls, the trail at its point | `+= scalar << 6` |
/// | 4 (`0x801F74EC`) | every pass, held or not: the trail runs on along the bearing at `sin * delta >> 6`; no shot | `+= scalar * 12` |
/// | 5 (`0x801F773C`) | once the victim settles, every present party seat and live monster shown | - |
///
/// The phase chain - clips, the hit of arm 3, arm 5's settle - is ported
/// ([`crate::cast_module_ticks::V_WINDHASH_CHAIN`]); this director owns the
/// gates, so `next` stays `None`. Not ported: arm 0's cue
/// (`FUN_8004FCC8(0x19B)`, `0x19E` for `0xA7`).
///
/// PORT: overlay_cast_v_windhash_0947_801f69f0 (PROT 0947; the camera arms, the seat hides and the trail)
pub fn v_windhash_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (c, v) = (seats.caster, seats.victim);
    let mut arm = CaptureCamArm::default();
    // `0x801F7B48`: the blade's point, in the module's words.
    let blade = &mut st.blade;
    match phase {
        0 => {
            let facing = bearing_12bit_approx(v.z, v.x, c.z, c.x).wrapping_add(0x800) & 0xFFF;
            arm.caster_write = CaptureSeatWrite {
                facing: Some(facing),
                ..SHOWN
            };
            arm.others_write = HIDDEN;
            // `0x801F6CB4..0x801F6CF0`.
            arm.vram_move = Some(super::GIMARD_BREATH_CLUT_MOVE);
            arm.shot = Some(ModuleShot {
                angles: [-0x20, 0x750_i16.wrapping_sub(facing as i16), 0],
                tr: [0, 0x500, (i32::from(seats.depth_raw as i16) / 3) as i16],
                focus: focus_on(c),
                frames: 0x20,
            });
            st.countdown.0 = SPEED_SCALAR << 5;
        }
        1 => {
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            arm.pass_spawns[0] = Some(seat(ARM1_RECORDS[0], [0, 0, 0x100], [0; 3]));
            arm.pass_spawns[1] = Some(seat(ARM1_RECORDS[1], [0, 0, 0x100], [0; 3]));
            arm.shot = Some(ModuleShot {
                angles: [0, 0x750_i16.wrapping_sub(c.facing as i16), 0],
                tr: [0, 0x500, (i32::from(seats.depth_raw as i16) / 2) as i16],
                focus: focus_on(c),
                frames: 0x80,
            });
            st.countdown.add(1 << 7);
        }
        2 => {
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            arm.victim_write = SHOWN;
            arm.caster_write = HIDDEN;
            let bearing = bearing_12bit_approx(v.z, v.x, c.z, c.x).wrapping_add(0x800) & 0xFFF;
            let (s, co) = trig12(bearing);
            let focus = [s.wrapping_sub(v.x), 0, co.wrapping_sub(v.z)];
            arm.shot = Some(ModuleShot {
                angles: [-0x40, (c.facing as i16).wrapping_neg(), 0],
                tr: [0, 0x300, 0x800],
                focus,
                frames: 1,
            });
            *blade = [focus[0].wrapping_neg(), focus[2].wrapping_neg()];
            let extra = (seats.action == NEO_WINDHASH).then_some(NEO_RECORD);
            arm.pass_spawns = trail(blade[0], blade[1], c.facing as i16, extra);
            st.countdown.add(1 << 6);
        }
        3 => {
            let held = st.countdown.drain_above(0);
            let cd = st.countdown.0;
            let bearing =
                bearing_12bit_approx(v.z, v.x, blade[1], blade[0]).wrapping_add(0x800) & 0xFFF;
            let (s, co) = trig12(bearing);
            // `sra 9` after `addiu 0x1FF` on a negative product.
            let across = |t: i16| (i32::from(t) * cd / 0x200) as i16;
            let focus = [
                v.x.wrapping_neg().wrapping_add(across(s)),
                0,
                v.z.wrapping_neg().wrapping_add(across(co)),
            ];
            arm.shot = Some(ModuleShot {
                angles: [
                    (-cd / SPEED_SCALAR) as i16,
                    (c.facing as i16).wrapping_neg(),
                    0,
                ],
                tr: [0, (0x400 - cd / 2) as i16, (cd + 0x600) as i16],
                focus,
                frames: 1,
            });
            *blade = [focus[0].wrapping_neg(), focus[2].wrapping_neg()];
            arm.pass_spawns = trail(blade[0], blade[1], c.facing as i16, None);
            if held {
                arm.hold = true;
            } else {
                st.countdown.add(1 << 6);
                // `0x801F74D4..0x801F74E8`: the victim turns on the caster.
                let facing = bearing_12bit_approx(c.z, c.x, v.z, v.x).wrapping_add(0x800) & 0xFFF;
                arm.victim_write.facing = Some(facing);
            }
        }
        4 => {
            let bearing = bearing_12bit_approx(v.z, v.x, c.z, c.x).wrapping_add(0x800) & 0xFFF;
            let (s, co) = trig12(bearing);
            // `sra 6` after `addiu 0x3F` on a negative product: one tick is
            // one frame delta here.
            blade[0] = blade[0].wrapping_add(s / 64);
            blade[1] = blade[1].wrapping_add(co / 64);
            arm.pass_spawns = trail(blade[0], blade[1], bearing as i16, None);
            if st.countdown.drain_above(0) {
                arm.hold = true;
            } else {
                st.countdown.0 += SPEED_SCALAR * 12;
            }
        }
        5 => {
            // `blez` at `0x801F7748`: an arm entered on a spent word drains
            // nothing. The settle is the chain's, and `others_write` waits
            // for it.
            if st.countdown.0 > 0 && st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
            arm.others_write = SHOWN;
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
                x: 928,
                y: 0,
                z: 81,
                facing: 0x400,
            },
            victim: ModuleSeat {
                x: 38,
                y: 0,
                z: 711,
                facing: 0,
            },
            depth_raw: 3000,
            action: 0x57,
            ..Default::default()
        }
    }

    #[test]
    fn arm_zero_hides_the_field_and_frames_the_caster_on_a_third_of_the_depth() {
        let mut st = ModuleCamState::default();
        let a = v_windhash_camera(&mut st, 0, seats());
        assert_eq!(a.others_write.render_flag, Some(0xFF));
        assert_eq!(a.caster_write.render_flag, Some(0));
        let shot = a.shot.unwrap();
        assert_eq!((shot.tr, shot.frames), ([0, 0x500, 1000], 0x20));
        assert_eq!(shot.focus, [-928, 0, -81]);
        assert_eq!(st.countdown.0, SPEED_SCALAR << 5);
    }

    #[test]
    fn arm_two_swaps_the_seats_and_lays_the_blade_on_both_sides() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = 1;
        let a = v_windhash_camera(&mut st, 2, seats());
        assert_eq!(a.caster_write.render_flag, Some(0xFF));
        assert_eq!(a.victim_write.render_flag, Some(0));
        assert_eq!(a.pass_spawns.iter().flatten().count(), 4);
        assert_eq!(a.shot.unwrap().frames, 1);
    }

    #[test]
    fn neo_windhash_seats_one_more_record_on_arm_two() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = 1;
        let mut s = seats();
        s.action = NEO_WINDHASH;
        let a = v_windhash_camera(&mut st, 2, s);
        assert_eq!(a.pass_spawns.iter().flatten().count(), 5);
    }

    #[test]
    fn arm_three_trails_on_held_passes_and_closes_on_the_victim() {
        let mut st = ModuleCamState::default();
        st.countdown.0 = SPEED_SCALAR << 6;
        st.blade = [100, 100];
        let held = v_windhash_camera(&mut st, 3, seats());
        assert!(held.hold);
        assert!(held.shot.is_some());
        assert_eq!(held.pass_spawns.iter().flatten().count(), 4);
        st.countdown.0 = 1;
        let pass = v_windhash_camera(&mut st, 3, seats());
        assert!(!pass.hold);
        assert!(pass.victim_write.facing.is_some());
    }

    #[test]
    fn arm_five_shows_the_field_again() {
        let mut st = ModuleCamState::default();
        let a = v_windhash_camera(&mut st, 5, seats());
        assert!(!a.hold);
        assert_eq!(a.others_write.colour, Some(SHOWN_COLOUR));
    }
}
