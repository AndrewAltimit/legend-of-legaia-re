//! The cast camera script `FUN_801DC0A0` against `spell_cam_inputs` +
//! `spell_cam_case`, the pair the action SM's host runs for it: every case
//! but the summon close-up `0x12`, over every party size and formation, a
//! caster on any seat aiming at a seat or a group code, the move-FX block,
//! the effect-child count and the frame depth. The tween builder
//! `FUN_801D829C` is hooked and its four arguments compared with the shot;
//! the group aim `FUN_801DCEAC` runs as it ships. Compared besides: the queue
//! byte the case hands on through, the hit bound and `+0x176`, `ctx[+0x24C]`.
//! The live yaw and `ctx[+0x87C]` are the camera's own and are handed over;
//! the frame depth `FUN_801F0348` leaves at `ctx[+0x6D0]` is seeded as the
//! port's `camera_frame_height`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_cam_script::spell_cam_case;
use legaia_engine_battle_vm::battle_slot::SeatMap;
use legaia_engine_core::battle_cam_inputs::spell_cam_inputs;
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::{A0, A1, A2, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const CAM: u32 = 0x801D_C0A0;
const TWEEN: u32 = 0x801D_829C;
const DEPTH: u32 = 0x801F_0348;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const RECORDS: u32 = 0x801C_9348;
const CHAR_IDS: u32 = 0x8007_BD10;
const LIVE_YAW: u32 = 0x8007_B792;
const FRAME_STEP: u32 = 0x1F80_0393;
/// Retail's resting colour word, which the port holds as `0` under flag `0`.
const NEUTRAL_WORD: u32 = 0x2008_0200;

fn actor(seat: u32) -> u32 {
    0x8013_0000 + seat * 0x400
}

fn node(seat: u32) -> u32 {
    0x8014_0000 + seat * 0x100
}

/// The tween builder's arguments: rotation, TR, focus, frames.
type Tween = ([i16; 3], [i16; 3], [i16; 3], u32);

#[test]
fn the_cast_camera_is_retails() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
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
    cpu.stub(DEPTH, 0);
    for seat in 0..8 {
        cpu.mem.wr32(ACTORS + seat * 4, actor(seat));
        cpu.mem.wr32(actor(seat) + 0x22C, node(seat));
    }
    let mut rng = Rng(0x5350_4341);
    let (mut failures, mut armed, mut handed, mut trapped) = (Vec::new(), 0, 0, 0);
    for round in 0..40_000 {
        let party = 1 + rng.below(3) as u8;
        let monsters = 1 + rng.below(5) as u8;
        let seats = SeatMap::new(party);
        let mut w = World::new();
        w.load_party(Party::zeroed(3));
        w.enter_battle(party, monsters);
        w.party.active_party = if rng.below(4) == 0 {
            vec![0, 3, 1][..usize::from(party)].to_vec()
        } else {
            vec![0, 1, 2][..usize::from(party)].to_vec()
        };
        let n = party + monsters;
        for seat in 0..8 {
            cpu.mem.wr32(actor(seat) + 4, 0);
            cpu.mem.wr8(CHAR_IDS + seat, 0);
        }
        for e in 0..n {
            let r = u32::from(seats.retail(e).get());
            let mut c = |s: u32| (rng.below(2 * s + 1) as i32 - s as i32) as i16;
            let (x, z) = (c(0x800), c(0x800));
            let (colour, flag) = [
                (0u32, 0u8),
                (0x0040_1004, 0),
                (0, 0xFF),
                (0, 2),
                (0x1008_0100, 2),
            ][rng.below(5) as usize];
            let word = if colour == 0 && flag == 0 {
                NEUTRAL_WORD
            } else {
                colour
            };
            let radius = if e < party {
                if w.party.active_party[usize::from(e)] == 3 {
                    0x400
                } else {
                    0x280
                }
            } else {
                [0x20i16, 0x40, 0x80, 0xC0, 0x100][rng.below(5) as usize]
            };
            let a = actor(r);
            cpu.mem.wr16(a + 0x34, x as u16);
            cpu.mem.wr16(a + 0x38, z as u16);
            cpu.mem.wr32(a + 4, word);
            cpu.mem.wr16(node(r) + 0x58, radius as u16);
            cpu.mem.wr16(node(r) + 0x68, 0);
            if e < party {
                cpu.mem
                    .wr8(CHAR_IDS + r, w.party.active_party[usize::from(e)] + 1);
            } else {
                cpu.mem.wr32(RECORDS + (r - 3) * 4, 0x8016_0000);
            }
            let m = &mut w.actors[usize::from(e)];
            m.move_state.world_x = x;
            m.move_state.world_z = z;
            m.battle.render_color = colour;
            m.battle.render_flag = flag;
            if e >= party {
                m.battle_monster_id = Some(0x20);
                m.battle.render_radius = Some(radius);
            }
        }
        cpu.mem.wr8(0x8016_0021, 0);

        let caster = rng.below(u32::from(n)) as u8;
        let rc = u32::from(seats.retail(caster).get());
        let case = [
            0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0xA, 0xB, 0xC, 0xD, 0xE, 0xF, 0x10, 0x11, 0x13, 0x14,
        ][rng.below(20) as usize];
        let target = if rng.below(3) == 0 {
            8 + rng.below(3) as u8
        } else {
            rng.below(u32::from(n)) as u8
        };
        let retail_target = if target < 8 {
            seats.retail(target).get()
        } else {
            target
        };
        let facing = rng.below(0x1000) as u16;
        let hit_bound = [0u8, 1, 0xFD, 0xFE, 0xFF][rng.below(5) as usize];
        let current = rng.below(3) as u8;
        let a = actor(rc);
        cpu.mem.wr16(a + 0x46, facing);
        cpu.mem.wr8(a + 0x1DD, retail_target);
        cpu.mem.wr8(a + 0x21B, hit_bound);
        cpu.mem.wr16(a + 0x176, 0x55);
        cpu.mem.wr8(a + 0x1D9, current);
        cpu.mem.wr8(a + 0x1DF, case);
        let b = &mut w.actors[usize::from(caster)].battle;
        b.facing_angle = facing;
        b.active_target = target;
        b.hit_count_bound = hit_bound;
        b.current_anim = current;

        let fx = (
            rng.below(0x1001) as i32 - 0x800,
            rng.below(0x1001) as i32 - 0x800,
        );
        let timer = [0i16, 1, 0x20, 0x21, 0x100, 0x1FF, 0x200, 0x3C0][rng.below(8) as usize];
        let children = rng.below(2) as u8;
        let phase = rng.below(4) as u8;
        let depth = [0x400i16, 0x900, 0xC00, 0x1400][rng.below(4) as usize];
        let live_yaw = rng.below(0x1000) as u16;
        let accum = rng.below(0x400);
        cpu.mem.wr8(CTX + 0x15, 0);
        cpu.mem.wr32(CTX + 0x87C, accum.wrapping_sub(16));
        cpu.mem.wr16(CTX + 0x1144, fx.0 as u16);
        cpu.mem.wr16(CTX + 0x1148, fx.1 as u16);
        cpu.mem.wr16(CTX + 0x6C6, timer as u16);
        cpu.mem.wr8(CTX + 0x24D, children);
        cpu.mem.wr8(CTX + 0x24E, phase);
        cpu.mem.wr16(CTX + 0x6D0, depth as u16);
        cpu.mem.wr8(CTX + 0x24C, 0);
        cpu.mem.wr16(LIVE_YAW, live_yaw);
        cpu.mem.wr8(FRAME_STEP, 2);
        w.casting.move_fx_streak.launch = Some((fx.0, 0, fx.1));
        w.casting.move_fx_streak.counter_word = timer as u16;
        w.casting.move_fx_streak.phase = phase;
        w.battle_ctx.magic_recovery_gate = children;
        w.battle.camera_frame_height = depth;

        shots.borrow_mut().clear();
        match cpu.call(CAM, &[rc, u32::from(case)]) {
            Stop::Returned => {}
            // A group with nobody drawn in it, or a slot code on cases `9` /
            // `0xB` naming a seat not drawn, reaches `FUN_801DCEAC`'s `break`:
            // retail stops there, so there is nothing to compare.
            Stop::DivideByZero { .. } => {
                trapped += 1;
                continue;
            }
            stop => panic!("round {round}: {stop:?}"),
        }
        let retail_shot = shots.borrow().first().copied();
        let retail = (
            retail_shot,
            cpu.mem.rd8(a + 0x1DF),
            cpu.mem.rd8(a + 0x21B),
            cpu.mem.rd16(a + 0x176),
            cpu.mem.rd8(CTX + 0x24C),
        );

        let mut inputs = spell_cam_inputs(&w, caster, case);
        inputs.live_yaw = f32::from(live_yaw);
        inputs.accum = accum;
        let step = spell_cam_case(&inputs);
        let port_shot = step.shot.map(|s| {
            let p = s.pose;
            (
                [p.pitch as i16, p.yaw as i16, 0],
                [p.tr[0] as i16, p.tr[1] as i16, s.raw_z as i16],
                [-(p.focus[0] as i32) as i16, 0, -(p.focus[2] as i32) as i16],
                s.frames,
            )
        });
        let port = (
            port_shot,
            step.next_case.unwrap_or(case),
            if step.clear_hit_bound { 0 } else { hit_bound },
            if step.clear_hit_bound { 0 } else { 0x55 },
            if step.hit_counter_fd { 0xFD } else { 0 },
        );
        // Retail's yaw is the halfword the arm stores; the shot holds it
        // wrapped into a turn.
        let retail = (
            retail
                .0
                .map(|(r, t, f, n)| ([r[0], r[1] & 0xFFF, r[2]], t, f, n)),
            retail.1,
            retail.2,
            retail.3,
            retail.4,
        );
        armed += i32::from(retail.0.is_some());
        handed += i32::from(retail.1 != case);
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round} party {party} monsters {monsters} caster {caster} case {case:#x} target {target}: retail {retail:?} port {port:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        armed > 20_000 && handed > 3_000,
        "{armed} {handed} {trapped}"
    );
}
