//! The action effect script stepper `FUN_801DEA50` against `step_effect_script`:
//! generated records at every frame gate, with the direct and table forms, the
//! offset scale and facing rotation, the terminator's move-power pick and the
//! launch point. The two spawners are hooked and their positions compared in
//! order; an actor mid-tween is out of scope.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_effects::action_effect_script::{
    EffectScriptActor, RECORD_BASE, RECORD_STRIDE, step_effect_script,
};
use legaia_engine_system::rotation_lut::retail_rotation_lut;
use psx_retail_exec::{A0, A1, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const STEP: u32 = 0x801D_EA50;
const DIRECT: u32 = 0x801D_FDF0;
const TABLE: u32 = 0x8005_0ED4;
const APPROACH: u32 = 0x801D_F570;
const BEARING: u32 = 0x8001_9B28;
const MOVE_IMAGE: u32 = 0x8005_8490;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MOVE_MAP: u32 = 0x801F_4E64;
const POWER_TABLE: u32 = 0x801F_4F5C;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const NODE: u32 = 0x8014_0000;
const SCRIPT: u32 = 0x8015_0000;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const SIN: u32 = 0x8016_0000;
const COS: u32 = 0x8016_4000;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Call {
    Direct(i16, i16, i16),
    Table(i16, i16, i16),
}

#[test]
fn the_effect_stepper_is_retails() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let lut = retail_rotation_lut();
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let map: Vec<u8> = cpu.mem.read(MOVE_MAP, 0x100);
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(SIN_PTR, SIN);
    cpu.mem.wr32(COS_PTR, COS);
    use legaia_engine_system::rotation_lut::RotationLut;
    for a in 0..0x1000 {
        cpu.mem.wr16(SIN + a as u32 * 2, lut.b(a) as u16);
        cpu.mem.wr16(COS + a as u32 * 2, lut.a(a) as u16);
    }
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * 0x400);
        cpu.mem.wr32(ACTOR + s * 0x400 + 0x22C, NODE + s * 0x100);
        cpu.mem.wr16(ACTOR + s * 0x400 + 0x14C, 0);
    }
    cpu.stub(APPROACH, 0);
    cpu.stub(BEARING, 0);
    cpu.stub(MOVE_IMAGE, 0);
    let calls: Rc<RefCell<Vec<Call>>> = Rc::default();
    {
        let c = calls.clone();
        cpu.hook(DIRECT, move |r, m| {
            let a = r.r[A1];
            c.borrow_mut().push(Call::Direct(
                m.rd16(a) as i16,
                m.rd16(a + 2) as i16,
                m.rd16(a + 4) as i16,
            ));
        });
        let c = calls.clone();
        cpu.hook(TABLE, move |r, m| {
            let a = r.r[A0];
            c.borrow_mut().push(Call::Table(
                m.rd16(a) as i16,
                m.rd16(a + 2) as i16,
                m.rd16(a + 4) as i16,
            ));
            r.r[2] = 0;
        });
    }
    let mut rng = Rng(0x4546_4658);
    let (mut spawned, mut terminators, mut failures) = (0u32, 0u32, Vec::new());
    for round in 0..50_000 {
        let n = 1 + rng.below(8) as usize;
        let mut block = vec![0u8; RECORD_BASE + 8 * RECORD_STRIDE];
        for i in 0..n {
            let o = RECORD_BASE + i * RECORD_STRIDE;
            let frame = 1 + rng.below(14) as u8;
            let effect = match rng.below(8) {
                0 => 0x7F,
                1 => 0xFF,
                2 => 0x80 | rng.below(0x70) as u8,
                _ => rng.below(0x40) as u8,
            };
            let (effect, frame) = if matches!(effect, 0x84 | 0x93) {
                (0x85, frame)
            } else {
                (effect, frame)
            };
            let mut c = || (rng.below(0x801) as i32 - 0x400) as i16;
            let (x, y, z) = (c(), c(), c());
            block[o] = frame;
            block[o + 1] = effect;
            block[o + 2..o + 4].copy_from_slice(&x.to_le_bytes());
            block[o + 4..o + 6].copy_from_slice(&y.to_le_bytes());
            block[o + 6..o + 8].copy_from_slice(&z.to_le_bytes());
        }
        let cursor = rng.below(n as u32 + 1) as u8;
        let frame = rng.below(0x12) as u8;
        let slot = rng.below(4);
        let scale = 0x800 + rng.below(0x1000) as u16;
        let facing = rng.below(0x1000) as u16;
        let world = (
            rng.below(0x801) as i16 - 0x400,
            -(rng.below(0x100) as i16),
            rng.below(0x801) as i16 - 0x400,
        );
        let action = 1 + rng.below(0x60) as u8;
        let scope = [3u8, 8, 9, 0xFF][rng.below(4) as usize];
        cpu.mem.wr8(CTX + 0x13, slot as u8);
        cpu.mem.wr32(CTX + 0x1028, 0);
        let a = ACTOR + slot * 0x400;
        cpu.mem.wr8(a + 0x1F5, cursor);
        cpu.mem.wr8(a + 0x1DC, 0);
        cpu.mem.wr8(a + 0x1D9, 1);
        cpu.mem.wr16(a + 0x34, world.0 as u16);
        cpu.mem.wr16(a + 0x36, world.1 as u16);
        cpu.mem.wr16(a + 0x38, world.2 as u16);
        cpu.mem.wr16(a + 0x46, facing);
        cpu.mem.wr8(a + 0x1DF, action);
        cpu.mem.wr8(a + 0x1DD, scope);
        cpu.mem.wr16(NODE + slot * 0x100 + 0x72, scale);
        cpu.mem.wr16(NODE + slot * 0x100 + 0x58, 0x80);
        cpu.mem.load(SCRIPT, &block);
        calls.borrow_mut().clear();
        cpu.mem.wr32(CTX + 0x1014, POWER_TABLE);
        assert_eq!(
            cpu.call(STEP, &[slot, SCRIPT, u32::from(frame)]),
            Stop::Returned,
            "round {round}"
        );

        let actor = EffectScriptActor {
            cursor,
            facing,
            world: (i32::from(world.0), i32::from(world.1), i32::from(world.2)),
            scale,
            scope,
            action,
            suppressed: false,
            approach: None,
        };
        let step = step_effect_script(lut, &block, actor, frame, &map);
        let retail_cursor = cpu.mem.rd8(a + 0x1F5);
        let mut got = calls.borrow().clone().into_iter();
        let mut wrong = None;
        if retail_cursor != step.cursor {
            wrong = Some(format!(
                "cursor retail {retail_cursor} port {}",
                step.cursor
            ));
        }
        let mut spawns = step.spawns.iter();
        for i in usize::from(cursor)..usize::from(step.cursor) {
            let effect = block[RECORD_BASE + i * RECORD_STRIDE + 1];
            if effect & 0x7F == 0x7F {
                // A table-form terminator walks its move-power record's
                // zero-ended effect list at `+0xE`, one spawn each.
                if effect == 0x7F
                    && let Some(off) = step.move_power_offset
                {
                    let mut k = POWER_TABLE + off as u32 + 0xE;
                    while cpu.mem.rd8(k) != 0 {
                        got.next();
                        k += 1;
                    }
                }
                continue;
            }
            let Some(s) = spawns.next() else { break };
            spawned += 1;
            let want = (s.at.0 as i16, s.at.1 as i16, s.at.2 as i16);
            let next = got.next();
            let (kind_ok, pos) = match next {
                Some(Call::Direct(x, y, z)) => (s.direct, (x, y, z)),
                Some(Call::Table(x, y, z)) => (!s.direct, (x, y, z)),
                None => (false, (0, 0, 0)),
            };
            if !kind_ok || pos != want {
                wrong = Some(format!(
                    "spawn at cursor {}: retail {next:?} port {:?} direct {}",
                    s.cursor, want, s.direct
                ));
                break;
            }
            // Floor part, scale-arm twin and flame calls each take their own
            // spawn on retail's side: skip what the same record adds.
            let e = s.effect & 0x7F;
            if s.direct && (0x81..=0x83).contains(&(0x80 | e)) {
                got.next();
            }
            if !s.direct && (4..=6).contains(&e) {
                got.next();
                got.next();
            }
        }
        if let Some(launch) = step.launch {
            terminators += 1;
            let retail_launch = (
                cpu.mem.rd16(CTX + 0x1144) as i16,
                cpu.mem.rd16(CTX + 0x1146) as i16,
                cpu.mem.rd16(CTX + 0x1148) as i16,
            );
            let want = (launch.0 as i16, launch.1 as i16, launch.2 as i16);
            let retail_off = cpu.mem.rd32(CTX + 0x1014).wrapping_sub(POWER_TABLE) as usize;
            if retail_launch != want || Some(retail_off) != step.move_power_offset {
                wrong = Some(format!(
                    "terminator: retail {retail_launch:?} off {retail_off:#x} \
                     port {want:?} {:x?}",
                    step.move_power_offset
                ));
            }
        }
        if let Some(w) = wrong
            && failures.len() < 14
        {
            failures.push(format!(
                "round {round} cursor {cursor} frame {frame} action {action:#x} n {n}: {w}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        spawned > 30_000 && terminators > 3000,
        "{spawned} {terminators}"
    );
}
