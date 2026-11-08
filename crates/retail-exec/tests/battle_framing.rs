//! `FUN_801D5854` cases 6, 7 and 8, the framing of every action, against
//! `BattleCamera::framing_case_pose`: both of case 6's arms with the win-pose
//! script, case 7's two-shot and pull-in, case 8's four dead and live arms over
//! every framing style, camera option, seat, target and battle-end state. The
//! tween builder `FUN_801D829C` is hooked and compared with the port's pose
//! and raw depth; the per-art tail `FUN_801D71B8` is stubbed.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_attack_camera::AttackCamCtx;
use legaia_engine_battle_vm::battle_cam_script::{
    ActionFraming, AttackCamChannels, BattleCamActor, BattleCamPhase, BattleCamera,
    PostActionTarget, prescale_tr_z,
};
use legaia_engine_camera::projection::BattleCamPose;
use psx_retail_exec::{A0, A1, A2, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const FRAMING: u32 = 0x801D_5854;
const ART_TAIL: u32 = 0x801D_71B8;
const TWEEN: u32 = 0x801D_829C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const CHAR_IDS: u32 = 0x8007_BD10;
const MONSTER_IDS: u32 = 0x8007_BD0C;
const OPTION: u32 = 0x8008_46C0;
const BATTLE_END: u32 = 0x8007_BD71;
const STAGED_FIGHT: u32 = 0x8007_BD2C;
const LONE_MONSTER: u32 = 0x8007_BD0D;
const SHIELD: u32 = 0x8007_BD84;
const LIVE_PITCH: u32 = 0x8007_B790;
const LIVE_YAW: u32 = 0x8007_B792;
const LIVE_TR_Y: u32 = 0x8008_40BC;
const HEIGHTS: u32 = 0x801F_4D2C;
const FRAME_STEP: u32 = 0x1F80_0393;

fn actor(seat: u32) -> u32 {
    0x8013_0000 + seat * 0x400
}
fn node(seat: u32) -> u32 {
    0x8014_0000 + seat * 0x100
}

type Tween = ([i16; 3], [i16; 3], [i16; 3], u32);

/// Halfwords a test can spread a position over without leaving `i16`.
fn s16(rng: &mut Rng, span: u32) -> i16 {
    (rng.below(2 * span + 1) as i32 - span as i32) as i16
}

#[test]
fn the_action_framings_are_retails() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.stub(ART_TAIL, 0);
    let shots: Rc<RefCell<Vec<Tween>>> = Rc::default();
    {
        let shots = shots.clone();
        cpu.hook(TWEEN, move |r, m| {
            let trio = |m: &mut psx_retail_exec::Memory, a: u32| {
                [m.rd16(a) as i16, m.rd16(a + 2) as i16, m.rd16(a + 4) as i16]
            };
            let t = (
                trio(m, r.r[A0]),
                trio(m, r.r[A1]),
                trio(m, r.r[A2]),
                r.r[A3],
            );
            shots.borrow_mut().push(t);
        });
    }
    for seat in 0..8 {
        cpu.mem.wr32(ACTORS + seat * 4, actor(seat));
        cpu.mem.wr32(actor(seat) + 0x22C, node(seat));
    }
    let mut rng = Rng(0x4652_414D);
    let mut failures: Vec<String> = Vec::new();
    let mut hits = [0u32; 3];
    for round in 0..120_000 {
        let case = 6 + rng.below(3);
        let slot = if rng.below(4) == 0 {
            3 + rng.below(4)
        } else {
            rng.below(3)
        };
        // A slot of `8` is case 7's fallback; retail reads past the table there.
        let target = loop {
            let t = match rng.below(12) {
                0 if case == 8 => 8,
                0..=6 => 3 + rng.below(4),
                _ => rng.below(3),
            };
            if t != slot {
                break t;
            }
        };
        let party_slot = slot < 3;
        let option = [0u8, 0, 1, 2][rng.below(4) as usize];
        let style = rng.below(5) as u8;
        let char_id = if party_slot {
            1 + rng.below(4) as u8
        } else {
            0
        };
        let battle_end = rng.below(4) == 0;
        let staged = u32::from(rng.below(4) == 0);
        let lone = rng.below(5) == 0;
        let flag_287 = u8::from(rng.below(3) == 0);
        let flag_288 = u8::from(rng.below(2) == 0);
        let (facing, depth, yaw_base, cursor) = (
            rng.next() as u16,
            0x800 + rng.below(0xC00) as u16,
            rng.below(0x1000) as u16,
            rng.below(2) as u8,
        );
        let (ramp, death_ramp, accum) = (
            rng.below(0xC9) as u8,
            rng.below(0xC9) as u8,
            8 * rng.below(0x4000),
        );
        let solo = u8::from(rng.below(2) == 0);
        let art = 0x11 + rng.below(0x20) as u8;
        let radius = 0x100 + rng.below(0x400) as i16;
        let frame_step = 1 + rng.below(3) as u8;
        let (live_pitch, live_yaw, live_tr_y) = (
            s16(&mut rng, 0x200),
            rng.below(0x2000) as i16,
            0x200 + rng.below(0x600) as i16,
        );

        // The acting seat.
        let a = actor(slot);
        cpu.mem.wr8(CTX + 0x13, slot as u8);
        let (ax, az, ay, ad, abx, abz) = (
            s16(&mut rng, 0x800),
            s16(&mut rng, 0x800),
            -(rng.below(0x200) as i16),
            -(rng.below(0x400) as i16),
            s16(&mut rng, 0x800),
            s16(&mut rng, 0x800),
        );
        cpu.mem.wr16(a + 0x34, ax as u16);
        cpu.mem.wr16(a + 0x36, ay as u16);
        cpu.mem.wr16(a + 0x38, az as u16);
        cpu.mem.wr16(a + 0x3C, abx as u16);
        cpu.mem.wr16(a + 0x3E, ad as u16);
        cpu.mem.wr16(a + 0x40, abz as u16);
        cpu.mem.wr16(a + 0x46, facing);
        cpu.mem.wr8(a + 0x1DD, target as u8);
        cpu.mem.wr8(a + 0x1DB, art);
        // The commit ladder materialises a staged art `>= 0x1A` into slot `0x11`.
        cpu.mem.wr8(a + 0x1D9, if art >= 0x1A { 0x11 } else { art });
        cpu.mem.wr16(node(slot) + 0x58, radius as u16);
        if party_slot {
            cpu.mem.wr8(CHAR_IDS + slot, char_id);
        }
        // Context and globals.
        cpu.mem.wr8(CTX + 0xD, style);
        cpu.mem.wr16(CTX + 0x6D0, depth);
        cpu.mem.wr16(CTX + 0x6DA, yaw_base);
        cpu.mem.wr8(CTX + 0x243, solo);
        cpu.mem.wr8(CTX + 0x26D, cursor);
        cpu.mem.wr8(CTX + 0x26E, ramp);
        cpu.mem.wr8(CTX + 0x270, death_ramp);
        cpu.mem.wr32(CTX + 0x87C, accum);
        cpu.mem.wr8(CTX + 0x287, flag_287);
        cpu.mem.wr8(CTX + 0x288, flag_288);
        cpu.mem.wr32(OPTION, u32::from(option));
        cpu.mem
            .wr8(BATTLE_END, if battle_end { 0xFE } else { 0xFF });
        cpu.mem.wr32(STAGED_FIGHT, staged);
        cpu.mem.wr8(LONE_MONSTER, u8::from(!lone));
        cpu.mem.wr32(SHIELD, 0);
        cpu.mem.wr16(LIVE_PITCH, live_pitch as u16);
        cpu.mem.wr16(LIVE_YAW, live_yaw as u16);
        cpu.mem.wr16(LIVE_TR_Y, live_tr_y as u16);
        cpu.mem.wr8(FRAME_STEP, frame_step);

        // The target seat, which can be the acting one.
        let mut post_target = None;
        if target < 8 {
            let t = actor(target);
            let (tx, tz, ty, td, tbx, tbz) = (
                s16(&mut rng, 0x800),
                s16(&mut rng, 0x800),
                if rng.below(3) == 0 {
                    0
                } else {
                    -(rng.below(0x100) as i16)
                },
                -(rng.below(0x500) as i16),
                s16(&mut rng, 0x800),
                s16(&mut rng, 0x800),
            );
            let dead = rng.below(3) == 0;
            let node_gone = dead && rng.below(2) == 0;
            let hp = if dead {
                0u16
            } else {
                1 + rng.below(999) as u16
            };
            let word = if node_gone {
                0
            } else {
                0x1000 + rng.below(0xFF_F000)
            };
            let animating = rng.below(2) == 0;
            let facing_t = rng.next() as u16;
            let id = [0x10u8, 0xB4, 0xA2, 0xA7, 0x20, 0x55][rng.below(6) as usize];
            let troster = if target < 3 {
                1 + rng.below(4) as u8
            } else {
                0
            };
            if target < 3 {
                cpu.mem.wr8(CHAR_IDS + target, troster);
            } else {
                cpu.mem.wr8(MONSTER_IDS + target - 3, id);
            }
            cpu.mem.wr16(t + 0x34, tx as u16);
            cpu.mem.wr16(t + 0x36, ty as u16);
            cpu.mem.wr16(t + 0x38, tz as u16);
            cpu.mem.wr16(t + 0x3C, tbx as u16);
            cpu.mem.wr16(t + 0x3E, td as u16);
            cpu.mem.wr16(t + 0x40, tbz as u16);
            cpu.mem.wr16(t + 0x46, facing_t);
            cpu.mem.wr16(t + 0x14C, hp);
            cpu.mem.wr32(t + 4, word);
            cpu.mem.wr8(t + 0x1D9, u8::from(animating));
            let height = (target < 3)
                .then(|| cpu.mem.rd16(HEIGHTS + (u32::from(troster) - 1) * 2) as i16 as f32);
            post_target = Some(PostActionTarget {
                world: [f32::from(tbx), f32::from(ty), f32::from(tbz)],
                display_y: f32::from(td),
                party: target < 3,
                height,
                monster_id: id,
                animating,
                live: !dead,
                facing: i32::from(facing_t & 0xFFF),
                node_gone,
                // `0x8007BD0D` is the formation's second monster id, which a
                // target on that seat carries.
                lone_defeat: flag_287 != 0 && flag_288 != 0 && lone && target != 4,
            });
        }

        shots.borrow_mut().clear();
        let stop = cpu.call(FRAMING, &[slot, case]);
        assert_eq!(stop, Stop::Returned, "round {round}");
        let Some(&(rot, tr, focus, dur)) = shots.borrow().first() else {
            continue;
        };

        // The port's camera in the same state.
        let takes_battle_over = case == 6 && party_slot && battle_end;
        let world = if takes_battle_over {
            [f32::from(abx), f32::from(ad), f32::from(abz)]
        } else {
            [f32::from(ax), f32::from(ay), f32::from(az)]
        };
        let bcam = BattleCamActor {
            facing: i32::from(facing as i16),
            world,
            height: None,
        };
        let mut cam = BattleCamera::new(BattleCamPhase::Action, 0);
        cam.stand_on_pose(BattleCamPose {
            pitch: f32::from(live_pitch),
            yaw: f32::from(live_yaw),
            tr: [0.0, f32::from(live_tr_y), 0.0],
            focus: [0.0; 3],
        });
        cam.set_actor(bcam);
        cam.set_post_action_target(post_target);
        let cam_action = ActionFraming {
            party_slot,
            battle_over: battle_end,
            depth_raw: i32::from(depth as i16),
            yaw_base: 0,
            style,
            char_id,
            anim_id: art,
            accum,
            ramp,
            body_radius: i32::from(radius),
            solo: solo != 0,
            scripted: flag_287 != 0,
            results_word: staged != 0,
        };
        cam.set_camera_option(option);
        cam.set_frame_step(frame_step);
        cam.set_attack_channels(
            party_slot
                .then_some(())
                .and(legaia_engine_battle_vm::battle_attack_camera::character_arm(1))
                .map(|character| AttackCamChannels {
                    character,
                    art_id: art,
                    arm_select: 0,
                    anim_frame: 0,
                    target_slot: target as u8,
                }),
            None,
        );
        // `FUN_801D5854`'s prologue advances the ramps ahead of every case.
        let mut counters = AttackCamCtx {
            phase_cursor: cursor,
            ramp,
            death_ramp,
            accum,
            latch: 0,
        };
        counters.advance(u32::from(frame_step));
        cam.set_action_framing(ActionFraming {
            accum: counters.accum,
            ramp: counters.ramp,
            ..cam_action
        });
        cam.seat_framing_counters(
            counters,
            i32::from(yaw_base as i16),
            Some([f32::from(abx), f32::from(abz)]),
        );
        let (pose, raw) = cam.framing_case_pose(case as u8).expect("case");
        hits[case as usize - 6] += 1;

        let ryaw = i32::from(rot[1] as u16 & 0xFFF);
        let pyaw = (pose.yaw as i32).rem_euclid(0x1000);
        let want = (
            i32::from(rot[0]),
            ryaw,
            i32::from(tr[1]),
            i32::from(tr[2]),
            [
                -i32::from(focus[0]),
                -i32::from(focus[1]),
                -i32::from(focus[2]),
            ],
            dur,
        );
        let got = (
            pose.pitch as i32,
            pyaw,
            pose.tr[1] as i32,
            raw,
            [
                pose.focus[0] as i32,
                pose.focus[1] as i32,
                pose.focus[2] as i32,
            ],
            0xC,
        );
        let z_agrees = prescale_tr_z(i32::from(tr[2])) == pose.tr[2];
        if want != got || !z_agrees {
            failures.push(format!(
                "round {round} case {case} seat {slot} target {target} opt {option} style {style} \
                 char {char_id} end {battle_end} staged {staged} 287 {flag_287}: \
                 retail {want:x?} port {got:x?}"
            ));
        }
    }
    eprintln!("{} of {:?} differ", failures.len(), hits);
    assert!(hits.iter().all(|h| *h > 10_000), "{hits:?}");
    assert!(
        failures.is_empty(),
        "{} rounds differ:\n{}",
        failures.len(),
        failures[..failures.len().min(25)].join("\n")
    );
}

/// Cases 0, 1, 3 and 9: the command close-up, the two cursor shots and the far
/// framing's formation box and yaw, against the pure framings the camera
/// steps through.
#[test]
fn the_menu_framings_are_retails() {
    use legaia_engine_battle_vm::battle_cam_script::{
        FormationBox, menu_framing, menu_raw_z, target_ally_pose, target_enemy_pose,
    };
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.stub(ART_TAIL, 0);
    cpu.stub(0x8003_5F04, 0);
    let shots: Rc<RefCell<Vec<Tween>>> = Rc::default();
    {
        let shots = shots.clone();
        cpu.hook(TWEEN, move |r, m| {
            let trio = |m: &mut psx_retail_exec::Memory, a: u32| {
                [m.rd16(a) as i16, m.rd16(a + 2) as i16, m.rd16(a + 4) as i16]
            };
            let t = (
                trio(m, r.r[A0]),
                trio(m, r.r[A1]),
                trio(m, r.r[A2]),
                r.r[A3],
            );
            shots.borrow_mut().push(t);
        });
    }
    for seat in 0..8 {
        cpu.mem.wr32(ACTORS + seat * 4, actor(seat));
        cpu.mem.wr32(actor(seat) + 0x22C, node(seat));
    }
    let mut rng = Rng(0x4D45_4E55);
    let mut failures: Vec<String> = Vec::new();
    for round in 0..60_000 {
        let case = [0u32, 1, 3, 9][rng.below(4) as usize];
        let party = 1 + rng.below(3);
        let monsters = 1 + rng.below(4);
        cpu.mem.wr8(CTX, party as u8);
        cpu.mem.wr8(CTX + 0x13, rng.below(party) as u8);
        let (live_yaw, focus) = (
            rng.below(0x1000) as i16,
            (s16(&mut rng, 0x800), s16(&mut rng, 0x800)),
        );
        cpu.mem.wr16(LIVE_YAW, live_yaw as u16);
        cpu.mem.wr32(0x8008_9118, (-(i32::from(focus.0))) as u32);
        cpu.mem.wr32(0x8008_9120, (-(i32::from(focus.1))) as u32);
        let mut seats = Vec::new();
        for s in 0..8u32 {
            let present = (s < party) || (3..3 + monsters).contains(&s);
            let hp = u16::from(present && rng.below(5) != 0);
            let (x, y, z) = (
                s16(&mut rng, 0x900),
                -(rng.below(0x400) as i16),
                s16(&mut rng, 0x900),
            );
            let f = rng.below(0x1000) as u16;
            let a = actor(s);
            cpu.mem.wr16(a + 0x34, x as u16);
            cpu.mem.wr16(a + 0x36, y as u16);
            cpu.mem.wr16(a + 0x38, z as u16);
            cpu.mem.wr16(a + 0x46, f);
            cpu.mem.wr16(a + 0x14C, hp);
            cpu.mem.wr8(CHAR_IDS + s, 1 + rng.below(3) as u8);
            cpu.mem.wr8(a + 0x1DD, (3 + rng.below(4)) as u8);
            seats.push((s, hp, x, y, z, f));
        }
        let selector = if case == 9 { rng.below(3) } else { 0 };
        let slot = if case == 9 {
            selector
        } else {
            rng.below(party)
        };
        shots.borrow_mut().clear();
        assert_eq!(cpu.call(FRAMING, &[slot, case]), Stop::Returned, "{round}");
        let Some(&(rot, tr, foc, dur)) = shots.borrow().first() else {
            continue;
        };
        let (_, _, ax, ay, az, facing) = seats[slot as usize];
        let char_id = u32::from(cpu.mem.rd8(CHAR_IDS + slot));
        let height = i32::from(cpu.mem.rd16(HEIGHTS + (char_id - 1) * 2));
        let bcam = BattleCamActor {
            facing: i32::from(facing as i16),
            world: [f32::from(ax), f32::from(ay), f32::from(az)],
            height: Some(height as f32),
        };
        let (pose, want_raw) = match case {
            0 => (bcam.submenu_pose(), 0x600),
            1 => {
                let t = seats[cpu.mem.rd8(actor(slot) + 0x1DD) as usize];
                (
                    target_enemy_pose(
                        bcam,
                        [f32::from(t.2), f32::from(t.4)],
                        [f32::from(focus.0), 0.0, f32::from(focus.1)],
                    ),
                    0x800,
                )
            }
            3 => (target_ally_pose(bcam), 0x600),
            _ => {
                let (lo, hi) = match selector {
                    1 => (3, 7),
                    2 => (0, 3),
                    _ => (0, 7),
                };
                let mut bx: Option<FormationBox> = None;
                let mut s = lo;
                while s < hi {
                    // `s0 == ctx[+0]` jumps to the first monster seat, before
                    // the loop test: a selector `2` party of fewer than three
                    // walks seat 3 once (`0x801D6FE4..0x801D6FF0`).
                    if s == party {
                        s = 3;
                    }
                    let t = seats[s as usize];
                    if t.1 != 0 {
                        FormationBox::extend(&mut bx, f32::from(t.2), f32::from(t.4));
                    }
                    s += 1;
                }
                (menu_framing(bx, f32::from(live_yaw)), menu_raw_z(bx))
            }
        };
        let raw = want_raw;
        let want = (
            i32::from(rot[0]),
            i32::from(rot[1] as u16 & 0xFFF),
            i32::from(tr[0]),
            i32::from(tr[1]),
            raw,
            [-i32::from(foc[0]), -i32::from(foc[1]), -i32::from(foc[2])],
        );
        let got = (
            pose.pitch as i32,
            (pose.yaw as i32).rem_euclid(0x1000),
            pose.tr[0] as i32,
            pose.tr[1] as i32,
            i32::from(tr[2]),
            [
                pose.focus[0] as i32,
                pose.focus[1] as i32,
                pose.focus[2] as i32,
            ],
        );
        let z_ok = prescale_tr_z(i32::from(tr[2])) == pose.tr[2];
        if want != got || !z_ok || dur != if case == 9 { 0xE } else { 0xC } {
            failures.push(format!(
                "round {round} case {case} slot {slot} party {party}: retail {want:x?} z {:x} port {got:x?} z {:?} dur {dur:#x}",
                tr[2], pose.tr[2]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(16)].join("\n")
    );
}
