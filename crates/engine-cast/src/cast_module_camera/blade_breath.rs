//! PROT 0962's three charge bodies, all Koru's: **Blade Breath** (`0xA2`,
//! `FUN_801F7AE4`), **Thunder Needle** (`0xA3`, `FUN_801F74A0`) and
//! **Gigaton Press** (`0xA4`, `FUN_801F6D54`) - their camera arms, gates,
//! seat stores and part spawns, the half the phase port
//! ([`crate::cast_arm_ticks::blade_breath_tick`]) does not carry.
//!
//! The three share arms `0` and `1` (the walk-in) and the shape of arm `2`
//! (cue, wind-up clip, one `0xC`-frame shot on the victim); they differ from
//! the strike on.
//!
//! See `ghidra/scripts/funcs/overlay_cast_blade_breath_0962_801f7ae4.txt`,
//! `..._801f74a0.txt` and `..._801f6d54.txt`.

use super::capture::{CaptureCamArm, CaptureDrift, CaptureSeatWrite, CastRibbonRing};
use super::*;

/// PROT 0962's `0xA2` tick body.
pub const BLADE_BREATH_BODY: u32 = 0x801F_7AE4;
/// PROT 0962's `0xA3` tick body.
pub const THUNDER_NEEDLE_BODY: u32 = 0x801F_74A0;
/// PROT 0962's `0xA4` tick body.
pub const GIGATON_PRESS_BODY: u32 = 0x801F_6D54;

/// The cursor `0xA3`'s arm 3 waits for (`slti v0,v0,0xf0` at `0x801F781C`).
pub const THUNDER_NEEDLE_STRIKE_CURSOR: u16 = 0xF0;
/// The cursor `0xA3`'s arm 4 waits for (`slti v0,v0,0x1c0` at `0x801F7A7C`).
pub const THUNDER_NEEDLE_TAIL_CURSOR: u16 = 0x1C0;
/// The span of `0xA3`'s arm-4 streak ring: the ring word `0x801F89B0`
/// draws while it is under `0x41` (`slti v0,v0,0x41` at `0x801F7980`).
pub const THUNDER_NEEDLE_RING_SPAN: i32 = 0x40;
/// The streaks `0xA3`'s arm 4 draws a pass (`sltiu v0,s0,0x10` at
/// `0x801F7A20`).
pub const THUNDER_NEEDLE_RING_STREAKS: usize = 16;
/// The trail id each streak passes the ribbon (`li a1,0xc` at `0x801F7A04`).
pub const THUNDER_NEEDLE_RING_TRAIL: u8 = 0xC;

/// The sixteen ground points `0xA3`'s arm 4 hands the chained streak ribbon
/// `FUN_801E1D98` on a pass whose ring word is `w` (`0x801F798C..0x801F7A24`):
/// streak `i` sits at angle `(i << 8) + (w & 0xFF)` and radius `w` (each lane
/// `table[angle] * w >> 7`, rounded toward zero, the sine view `*0x8007B81C`
/// on `x` and the cosine view `*0x8007B7F8` on `z`) about the **caster's**
/// live pair `+0x34` / `+0x38`, at height `0` (the stack vector's `y` is the
/// prologue's zero). The ring widens and turns as the word climbs.
pub fn thunder_needle_ring(
    caster_x: i16,
    caster_z: i16,
    w: i32,
) -> [[i16; 3]; THUNDER_NEEDLE_RING_STREAKS] {
    let lane = |t: i16| (i32::from(t) * w) / 128;
    std::array::from_fn(|i| {
        let angle = ((i as i32) << 8) + (w & 0xFF);
        let (sin, cos) = legaia_engine_system::trig::trig12(angle as u16);
        [
            (i32::from(caster_x) + lane(sin)) as i16,
            0,
            (i32::from(caster_z) + lane(cos)) as i16,
        ]
    })
}

/// The cursor `0xA4`'s arm 3 waits for (`slti v0,v0,0x100` at `0x801F70EC`).
pub const GIGATON_PRESS_STRIKE_CURSOR: u16 = 0x100;

/// The walk-in arms `0` and `1` all three bodies share: the facings
/// `FUN_80019B28` gives off the victim's body pair and case 6 on the caster.
fn walk_in(arm: &mut CaptureCamArm, phase: u8, caster_facing: u16) {
    arm.caster_write.facing = Some(caster_facing);
    if phase == 0 {
        arm.victim_write.facing = Some((caster_facing + 0x800) & 0xFFF);
    }
    arm.follow_caster = true;
}

/// Arm 2's shot, the one the three bodies make (`a3 = 0xC`): pitch `pitch`,
/// yaw `k - facing`, TR `(0, 0x400, 0x800)`, focus the victim's live pair.
fn wind_up_shot(pitch: i16, k: i32, caster_facing: u16, victim: ModuleSeat) -> ModuleShot {
    ModuleShot {
        angles: [pitch, yaw_from(k, caster_facing), 0],
        tr: [0, 0x400, 0x800],
        focus: focus_on(victim),
        frames: 0xC,
    }
}

/// The node cursor (12.4) arm 3 waits for on the caster's wind-up clip
/// (`lh v0,0x68(v0)` / `slti v0,v0,0x300` at `0x801F7E88..0x801F7E90`).
pub const BLADE_BREATH_STRIKE_CURSOR: u16 = 0x300;

/// The camera, gate and seat half of **Blade Breath** (PROT 0962, body
/// `0x801F7AE4`, a `beq` / `slti` chain over `0..=4` and `0xFF`), by the
/// module phase the body is about to run. `s` is the speed scalar, `h` the
/// heading `FUN_80019B28` takes from the victim's body pair `+0x3C` / `+0x40`
/// to the caster's live pair:
///
/// | arm | every pass | gate | as it passes |
/// |---|---|---|---|
/// | 0 (`0x801F7BC8`) | caster faces `h + 0x800`, the victim `h` (`0x801F7BEC` / `0x801F7BFC`); case 6 on the caster | - | (the arrival test, the phase port) |
/// | 1 (`0x801F7CC0`) | caster faces `h + 0x800`; case 6 on the caster | - | (the arrival test) |
/// | 2 (`0x801F7D50`) | - | - | cue `0x18B`; caster faces `h + 0x800`; shot pitch `-0x20`, yaw `0x500 - facing`, TR `(0, 0x400, 0x800)`, focus the victim's live pair, `0xC` frames; the word `0x801F89AC = s << 7` |
/// | 3 (`0x801F7DFC`) | TR z `-s/2`, pitch `-s/8`, yaw `-s/4` | caster cursor `0x300` | pool flush; four records on the victim; the `0x200` hit; victim colour `0x3FF`, rate `4`; case 8 on the caster |
/// | 4 (`0x801F8000`) | victim rate `4`; case 8 on the caster | the victim's `+0x1D9` at `0` or `8` | - |
/// | `0xFF` (`0x801F803C`) | case 8 on the caster | - | - |
///
/// The three drifts are one product `s * delta` shifted (`sra 1` for TR z,
/// `/ 8` and `/ 4` rounded toward zero for the angles), spent ahead of the
/// gate on every pass. A clip gate whose input the host cannot answer
/// (`None`) passes.
///
/// PORT: FUN_801F7AE4 (PROT 0962 `0xA2`; the camera arms, the cursor gate,
/// the facings and the victim's colour / rate stores)
pub fn blade_breath_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let s = SPEED_SCALAR as i16;
    let c = seats.caster;
    let vb = st.victim_body.unwrap_or(seats.victim);
    // `FUN_80019B28(victim+0x40, victim+0x3C, caster+0x38, caster+0x34)`.
    let h = heading(vb, c);
    let caster_facing = (h.wrapping_add(0x800)) & 0xFFF;
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    match phase {
        0 | 1 => walk_in(&mut arm, phase, caster_facing),
        2 => {
            arm.caster_write.facing = Some(caster_facing);
            st.countdown.0 = SPEED_SCALAR << 7;
            arm.shot = Some(wind_up_shot(-0x20, 0x500, caster_facing, seats.victim));
        }
        3 => {
            arm.drift = Some(CaptureDrift {
                pitch: -(s / 8),
                yaw: -(s / 4),
                tr_y: 0,
                tr_z: -(s >> 1),
                tr_x: 0,
            });
            arm.hold = st
                .caster_frame
                .is_some_and(|f| f < BLADE_BREATH_STRIKE_CURSOR);
            if !arm.hold {
                // `li 0x3FF; sw 0x4(s2)` / `li 0x1000; sw 0xc(s2)` and
                // `sb s6,0x21d(s2)` with `s6 = 4` (`0x801F7F50..0x801F7F6C`).
                arm.victim_write = CaptureSeatWrite {
                    colour: Some(0x3FF),
                    rate: Some(4),
                    ..Default::default()
                };
                arm.end_frame = true;
                // `FUN_801DD4B0(0x200, ...)` at `0x801F7F18`.
                arm.folds_hit = true;
            }
        }
        4 => {
            arm.victim_write.rate = Some(4);
            arm.end_frame = true;
            // `lbu s0,0x1d9(s2)` / `beq s0,zero` / `bne s0,8` at
            // `0x801F8018..0x801F8028`: the reaction the hit staged plays out.
            arm.hold = !st.victim_settled;
        }
        0xFF => arm.end_frame = true,
        _ => {}
    }
    arm
}

/// **Thunder Needle** (PROT 0962, body `0x801F74A0`, the same chain as
/// [`blade_breath_camera`]):
///
/// | arm | every pass | gate | as it passes |
/// |---|---|---|---|
/// | 0, 1 | the walk-in ([`blade_breath_camera`]) | - | - |
/// | 2 (`0x801F7710`) | - | - | cue `0x18C`; caster faces `h + 0x800`, clip `6` staged with the bump, rate `3`; shot pitch `0x20`, yaw `0x800 - facing`, TR `(0, 0x400, 0x800)` on the victim; the words `0x801F89B0 = 0`, `0x801F89AC = s << 7` |
/// | 3 (`0x801F77D4`) | caster stage `0`, rate `3`; TR z `+s` | caster cursor `0xF0` | pool flush; three records on the victim; the `0x200` hit; victim colour `0x3FF00000`, rate `4` |
/// | 4 (`0x801F7964`) | TR z `+s`; caster rate `2`, victim `4` | caster cursor `0x1C0` | the caster's `+0x1DC` bump |
///
/// Arm 4 also widens a ring of sixteen streaks about the caster every pass,
/// ahead of its gate, while its word (stepped by the frame step from arm 2's
/// `0`) is under `0x41` ([`thunder_needle_ring`], [`CaptureCamArm::ribbons`]).
/// Unlike the other two bodies it frames nothing through case 8.
///
/// PORT: FUN_801F74A0 (PROT 0962 `0xA3`; the camera arms, the two cursor
/// gates, the facings, the seat colour / rate stores and the streak ring)
pub fn thunder_needle_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let s = SPEED_SCALAR as i16;
    let vb = st.victim_body.unwrap_or(seats.victim);
    let caster_facing = (heading(vb, seats.caster).wrapping_add(0x800)) & 0xFFF;
    let tr_z = CaptureDrift {
        tr_z: s,
        ..Default::default()
    };
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    match phase {
        0 | 1 => walk_in(&mut arm, phase, caster_facing),
        2 => {
            arm.caster_write.facing = Some(caster_facing);
            st.countdown.0 = SPEED_SCALAR << 7;
            // `sw zero,-0x7650(v0)` at `0x801F77B8`: the ring word.
            st.module_words[0] = 0;
            arm.shot = Some(wind_up_shot(0x20, 0x800, caster_facing, seats.victim));
        }
        3 => {
            arm.drift = Some(tr_z);
            arm.caster_write.rate = Some(3);
            arm.hold = st
                .caster_frame
                .is_some_and(|f| f < THUNDER_NEEDLE_STRIKE_CURSOR);
            if !arm.hold {
                // `lui 0x3ff0; sw 0x4(s3)` and `sb 4,0x21d(s3)`
                // (`0x801F78CC..0x801F78F0`).
                arm.victim_write.colour = Some(0x3FF0_0000);
                arm.victim_write.rate = Some(4);
                // `FUN_801DD4B0(0x200, ...)` at `0x801F788C`.
                arm.folds_hit = true;
            }
        }
        4 => {
            // The word climbs by the frame step a pass (`lbu 0x393` / `addu`
            // at `0x801F796C..0x801F7978`), one a tick on the engine's
            // vsync clock, and the ring draws ahead of the cursor gate.
            let w = st.module_words[0] + 1;
            st.module_words[0] = w;
            if w <= THUNDER_NEEDLE_RING_SPAN {
                arm.ribbons = Some(CastRibbonRing {
                    points: thunder_needle_ring(seats.caster.x, seats.caster.z, w),
                    trail_id: THUNDER_NEEDLE_RING_TRAIL,
                    count: THUNDER_NEEDLE_RING_STREAKS as u8,
                });
            }
            arm.drift = Some(tr_z);
            arm.caster_write.rate = Some(2);
            arm.victim_write.rate = Some(4);
            arm.hold = st
                .caster_frame
                .is_some_and(|f| f < THUNDER_NEEDLE_TAIL_CURSOR);
        }
        _ => {}
    }
    arm
}

/// The caster's height on `0xA4`'s arm 3, off its jump word `w`:
/// `-sin(16 w) / 8`, toward zero (`sll v0,a0,0x5` into the sine table
/// `*0x8007B81C`, `subu`, `+7`, `sra 3` at `0x801F70B8..0x801F70E0`).
pub fn gigaton_press_jump_height(w: i32) -> i16 {
    let s = i32::from(legaia_engine_system::trig::sin12((w * 16) as u16));
    ((-s) / 8) as i16
}

/// **Gigaton Press** (PROT 0962, body `0x801F6D54`, the same chain as
/// [`blade_breath_camera`]):
///
/// | arm | every pass | gate | as it passes |
/// |---|---|---|---|
/// | 0, 1 | the walk-in ([`blade_breath_camera`]) | - | - |
/// | 2 (`0x801F6FA0`) | - | - | cue `0x18D`; caster faces `h + 0x800`, clip `7` staged with the bump, rate `4`; shot pitch `0x20`, yaw `0x800 - facing`, TR `(0, 0x400, 0x800)` on the victim; the jump word `0x801F89B0 = 0` |
/// | 3 (`0x801F706C`) | caster rate `4`; TR z `+2s`; the jump word `+ d`, caster height `-sin(16 w) / 8` | caster cursor `0x100` | pool flush; five records on the caster; caster height `0`; the `0x200` hit; victim rate `4`; the shake word `0x801F89B0 = 0x100` |
/// | 4 (`0x801F738C`) | TR z `+2s`; TR y `+-` the shake word, the sign flipping a pass; the word `- s d / 2` (floor `0`); victim rate `4`; case 8 is **not** called | the victim's `+0x1D9` at `0` or `8` | pool flush |
///
/// Arm 3's sixteen debris records (`rand() % 4` of four, on a ring of
/// radius `0x400` about the victim) are [`gigaton_press_debris`], which the
/// host seats on the arm's pass.
///
/// PORT: FUN_801F6D54 (PROT 0962 `0xA4`; the camera arms, the cursor and
/// settle gates, the jump arc, the shake and the rate stores)
pub fn gigaton_press_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let s = SPEED_SCALAR as i16;
    let vb = st.victim_body.unwrap_or(seats.victim);
    let caster_facing = (heading(vb, seats.caster).wrapping_add(0x800)) & 0xFFF;
    let [mut w, mut toggle] = st.module_words;
    let mut arm = CaptureCamArm {
        drift_first: true,
        ..Default::default()
    };
    match phase {
        0 | 1 => walk_in(&mut arm, phase, caster_facing),
        2 => {
            arm.caster_write.facing = Some(caster_facing);
            st.countdown.0 = SPEED_SCALAR << 7;
            w = 0;
            arm.shot = Some(wind_up_shot(0x20, 0x800, caster_facing, seats.victim));
        }
        3 => {
            arm.caster_write.rate = Some(4);
            arm.drift = Some(CaptureDrift {
                tr_z: 2 * s,
                ..Default::default()
            });
            w += 1;
            arm.caster_write.y = Some(gigaton_press_jump_height(w));
            arm.hold = st
                .caster_frame
                .is_some_and(|f| f < GIGATON_PRESS_STRIKE_CURSOR);
            if !arm.hold {
                arm.caster_write.y = Some(0);
                arm.victim_write.rate = Some(4);
                arm.folds_hit = true;
                // `li 0x100; sw -0x7650` / `sw zero,-0x764c`
                // (`0x801F7328..0x801F7338`).
                w = 0x100;
                toggle = 0;
            }
        }
        4 => {
            let mut drift = CaptureDrift {
                tr_z: 2 * s,
                ..Default::default()
            };
            // The shake steps once a battle frame: its sign flips a pass.
            if st.opens_pass {
                let up = toggle & 1 != 0;
                toggle += 1;
                drift.tr_y = if up { w as i16 } else { -(w as i16) };
                w = (w - ((SPEED_SCALAR * i32::from(st.frame_step.max(1))) >> 1)).max(0);
            }
            arm.drift = Some(drift);
            arm.victim_write.rate = Some(4);
            arm.hold = !st.victim_settled;
        }
        _ => {}
    }
    st.module_words = [w, toggle];
    arm
}

/// Whether a capture body's walk-in arms frame the caster through the
/// action SM's case 6 on every pass of the approach (PROT 0962 `0xA2`'s arms
/// 0 and 1). The host's approach leg holds the module while the caster
/// walks, so it makes the call on the body's behalf.
pub fn capture_approach_follows(entry: u32, body: Option<u32>) -> bool {
    entry == 962
        && matches!(
            body,
            Some(BLADE_BREATH_BODY | THUNDER_NEEDLE_BODY | GIGATON_PRESS_BODY)
        )
}

/// `0x801F7EA4..0x801F7F04`, arm 3: four records on the victim's `+0x34` /
/// `+0x44`, scale `0x1000`.
pub(super) const E962_A2_3: [ModuleSpawn; 4] = [
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8378),
        anchor: SpawnAnchor::Victim,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_83D0),
        anchor: SpawnAnchor::Victim,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8428),
        anchor: SpawnAnchor::Victim,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8480),
        anchor: SpawnAnchor::Victim,
    },
];

/// `0x801F7830..0x801F7878`, `0xA3`'s arm 3: three records on the victim.
pub(super) const E962_A3_3: [ModuleSpawn; 3] = [
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_84D8),
        anchor: SpawnAnchor::Victim,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_852C),
        anchor: SpawnAnchor::Victim,
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8940),
        anchor: SpawnAnchor::Victim,
    },
];

/// `0x801F7100..0x801F7178`, `0xA4`'s arm 3: five records on the caster's
/// `+0x34` / `+0x44`.
pub(super) const E962_A4_3: [ModuleSpawn; 5] = [
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_81E8),
        anchor: SpawnAnchor::CasterShifted { dy: 0, dz: 0 },
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_823C),
        anchor: SpawnAnchor::CasterShifted { dy: 0, dz: 0 },
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8290),
        anchor: SpawnAnchor::CasterShifted { dy: 0, dz: 0 },
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_82F0),
        anchor: SpawnAnchor::CasterShifted { dy: 0, dz: 0 },
    },
    ModuleSpawn {
        record: SpawnRecord::Module(0x801F_8334),
        anchor: SpawnAnchor::CasterShifted { dy: 0, dz: 0 },
    },
];

/// `0xA4` arm 3's debris, the four records `rand() % 4` picks from
/// (`0x801F7240..0x801F7280`).
pub const GIGATON_PRESS_DEBRIS: [u32; 4] = [0x801F_8588, 0x801F_85D4, 0x801F_8620, 0x801F_866C];

/// `0xA4` arm 3's sixteen debris spawns (`0x801F717C..0x801F7294`), in call
/// order: point `i` at the victim's plane pair plus `trunc(sin(a) / 4)` on X
/// and `trunc(cos(a) / 4)` on Z for `a = i << 8` (the SCUS tables
/// `*0x8007B81C` / `*0x8007B7F8` read at `i << 9` bytes), on the floor with no
/// rotation, each record picked by its own `rand() % 4` (`FUN_80056798`).
pub fn gigaton_press_debris(victim: ModuleSeat, mut rand: impl FnMut() -> u32) -> Vec<ModuleSpawn> {
    (0..16u16)
        .map(|i| {
            let (sin, cos) = trig12(i << 8);
            let pos = [
                (i32::from(victim.x) + i32::from(sin) / 4) as i16,
                0,
                (i32::from(victim.z) + i32::from(cos) / 4) as i16,
            ];
            ModuleSpawn {
                record: SpawnRecord::Module(GIGATON_PRESS_DEBRIS[(rand() % 4) as usize]),
                anchor: SpawnAnchor::Literal { pos, rot: [0; 3] },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {

    #[test]
    fn thunder_needle_ring_widens_and_turns_about_the_caster() {
        // Word 64: radius `4096 * 64 / 128 = 2048`, streak 0 at angle 64.
        let r = thunder_needle_ring(100, -200, 0x40);
        let (s0, c0) = legaia_engine_system::trig::trig12(0x40);
        assert_eq!(
            r[0],
            [
                100 + (i32::from(s0) * 64 / 128) as i16,
                0,
                -200 + (i32::from(c0) * 64 / 128) as i16
            ]
        );
        // Streak 4 sits a quarter turn on: `x` from the cosine side.
        let (s4, c4) = legaia_engine_system::trig::trig12(0x440);
        assert_eq!(
            r[4],
            [
                100 + (i32::from(s4) * 64 / 128) as i16,
                0,
                -200 + (i32::from(c4) * 64 / 128) as i16
            ]
        );
        // Word 0: every streak on the caster.
        assert!(thunder_needle_ring(5, 6, 0).iter().all(|p| *p == [5, 0, 6]));
    }

    #[test]
    fn thunder_needle_arm_four_draws_the_ring_for_its_span_only() {
        let seats = ModuleCamSeats::default();
        let mut st = ModuleCamState::default();
        thunder_needle_camera(&mut st, 2, seats);
        let drawn = (0..100)
            .filter(|_| thunder_needle_camera(&mut st, 4, seats).ribbons.is_some())
            .count();
        assert_eq!(drawn, THUNDER_NEEDLE_RING_SPAN as usize);
    }

    use super::*;

    #[test]
    fn arm_three_drifts_every_pass_and_waits_on_the_wind_up() {
        let seats = ModuleCamSeats::default();
        let mut st = ModuleCamState::default();
        let a = blade_breath_camera(&mut st, 2, seats);
        assert_eq!(a.shot.unwrap().tr, [0, 0x400, 0x800]);
        assert_eq!(st.countdown.0, SPEED_SCALAR << 7);
        st.caster_frame = Some(0x2F0);
        let a = blade_breath_camera(&mut st, 3, seats);
        assert!(a.hold);
        let d = a.drift.unwrap();
        assert_eq!((d.pitch, d.yaw, d.tr_z), (-1, -2, -4));
        st.caster_frame = Some(0x300);
        let a = blade_breath_camera(&mut st, 3, seats);
        assert!(!a.hold && a.end_frame);
        assert_eq!(a.victim_write.colour, Some(0x3FF));
    }

    #[test]
    fn thunder_needle_gates_both_arms_on_the_caster_cursor() {
        let seats = ModuleCamSeats::default();
        let mut st = ModuleCamState {
            caster_frame: Some(0xE0),
            ..Default::default()
        };
        assert!(thunder_needle_camera(&mut st, 3, seats).hold);
        st.caster_frame = Some(0xF0);
        let a = thunder_needle_camera(&mut st, 3, seats);
        assert!(!a.hold && a.folds_hit);
        assert_eq!(a.drift.unwrap().tr_z, SPEED_SCALAR as i16);
        st.caster_frame = Some(0x1B0);
        assert!(thunder_needle_camera(&mut st, 4, seats).hold);
        st.caster_frame = Some(0x1C0);
        assert!(!thunder_needle_camera(&mut st, 4, seats).hold);
    }

    #[test]
    fn gigaton_press_jumps_then_shakes_down_to_rest() {
        assert_eq!(gigaton_press_jump_height(0), 0);
        assert!(gigaton_press_jump_height(0x40) < -0x100);
        let seats = ModuleCamSeats::default();
        let mut st = ModuleCamState {
            caster_frame: Some(0x100),
            victim_settled: false,
            opens_pass: true,
            frame_step: 1,
            ..Default::default()
        };
        let a = gigaton_press_camera(&mut st, 3, seats);
        assert!(!a.hold && a.folds_hit);
        assert_eq!(a.caster_write.y, Some(0));
        assert_eq!(st.module_words, [0x100, 0]);
        let a = gigaton_press_camera(&mut st, 4, seats);
        assert!(a.hold);
        assert_eq!(a.drift.unwrap().tr_y, -0x100);
        let a = gigaton_press_camera(&mut st, 4, seats);
        assert_eq!(a.drift.unwrap().tr_y, 0x100 - 4);
    }

    /// The sixteen rocks sit on a `0x400` ring round the victim, one record
    /// per `rand() % 4` in call order. The picks are a retail RAM read of a
    /// `koru_strip_forced` cast's rocks on the arm's pass, the victim's
    /// ground pair `(-192, -564)`: point 0 straight down `+z`, point 4 on
    /// `+x`.
    #[test]
    fn gigaton_debris_rings_the_victim_one_pick_per_rock() {
        let victim = ModuleSeat {
            x: -192,
            y: 0,
            z: -564,
            facing: 0,
        };
        let picks = [0u32, 2, 2, 0, 2, 2, 1, 3, 0, 3, 2, 3, 1, 1, 0, 0];
        let mut k = 0;
        let rocks = gigaton_press_debris(victim, || {
            k += 1;
            picks[k - 1] + 4 * k as u32
        });
        assert_eq!(rocks.len(), 16);
        for (i, r) in rocks.iter().enumerate() {
            assert_eq!(
                r.record,
                SpawnRecord::Module(GIGATON_PRESS_DEBRIS[picks[i] as usize])
            );
        }
        let at = |i: usize| match rocks[i].anchor {
            SpawnAnchor::Literal { pos, rot } => (pos, rot),
            _ => panic!("literal"),
        };
        assert_eq!(at(0), ([-192, 0, 460], [0; 3]));
        assert_eq!(at(4), ([-192 + 1024, 0, -564], [0; 3]));
        assert_eq!(at(8), ([-192, 0, -564 - 1024], [0; 3]));
    }
}
