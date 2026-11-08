//! The per-art attack camera `FUN_801D71B8` against `attack_camera_framing`:
//! every character, every art id through and past the three jump tables,
//! the arm selector, the animation cursor across every threshold, both track
//! columns, the ramp, accumulator and latch. The tween builder `FUN_801D829C`
//! is hooked and its four arguments compared with the framing; compared
//! besides are the three context counters an arm can reset.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_attack_camera::{
    AttackCamActor, AttackCamCtx, attack_camera_framing, character_arm,
};
use legaia_game_tables::battle_attack_camera_table;
use psx_retail_exec::{A0, A1, A2, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const FRAMING: u32 = 0x801D_71B8;
const TWEEN: u32 = 0x801D_829C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const CHAR_IDS: u32 = 0x8007_BD10;
const ACTOR: u32 = 0x8013_0000;
const TARGET: u32 = 0x8013_1000;
const NODE: u32 = 0x8014_0000;
const FRAME_STEP: u32 = 0x1F80_0393;

type Tween = ([i16; 3], [i16; 3], [i16; 3], u32);

#[test]
fn the_attack_camera_is_retails() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let image = disc.read_entry(898).expect("PROT 0898");
    let table = battle_attack_camera_table::parse(&image).expect("track table");
    cpu.mem.load(OVERLAY_BASE, &image);
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr8(CTX + 6, 0xFF);
    cpu.mem.wr32(ACTORS, ACTOR);
    cpu.mem.wr32(ACTORS + 4, TARGET);
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
    let thresholds: [i16; 18] = [
        0, 0x3F, 0x40, 0x60, 0x61, 0x6F, 0x70, 0x71, 0x8F, 0x90, 0x9F, 0xA0, 0xAF, 0xB0, 0xBF,
        0xC0, 0xE0, 0x110,
    ];
    let mut rng = Rng(0x4154_4B43);
    let (mut failures, mut armed, mut ramps) = (Vec::new(), 0u32, 0u32);
    for round in 0..200_000 {
        let character = 1 + rng.below(3) as u8;
        let art = 0x18 + rng.below(0x18) as u8;
        let select = [0u8, 1, 2, 3, 5, 6][rng.below(6) as usize];
        let frame = match rng.below(3) {
            0 => thresholds[rng.below(18) as usize],
            1 => thresholds[rng.below(18) as usize].wrapping_sub(1),
            _ => rng.below(0x180) as i16,
        };
        let cursor = rng.below(2) as u8;
        let ramp = match rng.below(3) {
            0 => 0xC8,
            1 => 0,
            _ => rng.below(0xC9) as u8,
        };
        let accum = match rng.below(4) {
            0 => rng.below(0x400),
            1 => rng.below(0x1_0000),
            2 => rng.next(),
            _ => 8 * rng.below(0x100),
        };
        let latch = rng.below(4) as u8;
        let pos = [rng.next() as u16, rng.next() as u16, rng.next() as u16];
        let facing = rng.next() as u16;

        cpu.mem.wr8(CTX + 0x13, 0);
        cpu.mem.wr8(CHAR_IDS, character);
        cpu.mem.wr8(CTX + 0x26D, cursor);
        cpu.mem.wr8(CTX + 0x26E, ramp);
        cpu.mem.wr8(CTX + 0x26F, latch);
        cpu.mem.wr32(CTX + 0x87C, accum);
        cpu.mem.wr8(ACTOR + 0x1DD, 1);
        cpu.mem.wr8(ACTOR + 0x1DE, 3);
        cpu.mem.wr8(ACTOR + 0x1DB, art);
        cpu.mem.wr8(ACTOR + 0x21B, select);
        cpu.mem.wr16(ACTOR + 0x34, pos[0]);
        cpu.mem.wr16(ACTOR + 0x36, pos[1]);
        cpu.mem.wr16(ACTOR + 0x38, pos[2]);
        cpu.mem.wr16(ACTOR + 0x46, facing);
        cpu.mem.wr32(ACTOR + 0x22C, NODE);
        cpu.mem.wr16(NODE + 0x68, frame as u16);
        cpu.mem.wr16(TARGET + 0x14C, 1);
        let fade_before = match rng.below(3) {
            0 => rng.next() as u8,
            1 => 0x60,
            _ => rng.below(0x70) as u8,
        };
        let frame_step = 1 + rng.below(4) as u8;
        cpu.mem.wr8(TARGET + 0x226, fade_before);
        cpu.mem.wr8(FRAME_STEP, frame_step);
        shots.borrow_mut().clear();
        assert_eq!(cpu.call(FRAMING, &[]), Stop::Returned, "round {round}");
        let retail = shots.borrow().first().copied();
        assert!(shots.borrow().len() <= 1);

        let actor = AttackCamActor {
            character: character_arm(character).expect("1..=3"),
            art_id: art,
            arm_select: select,
            anim_frame: frame,
            pos,
            facing,
        };
        let mut ctx = AttackCamCtx {
            phase_cursor: cursor,
            ramp,
            death_ramp: 0,
            accum,
            latch,
        };
        let framing = attack_camera_framing(actor, &mut ctx, &table);
        let port = framing.map(|f| {
            (
                f.pose.rot,
                f.pose.dist,
                f.pose.look_at,
                u32::from(f.duration_frames),
            )
        });
        let port_fade = framing.map_or(fade_before, |f| {
            f.target_fade.apply(fade_before, frame_step)
        });
        let retail_fade = cpu.mem.rd8(TARGET + 0x226);
        ramps += u32::from(retail_fade != fade_before && retail_fade != 0);
        armed += u32::from(retail.is_some());
        let after = (
            cpu.mem.rd8(CTX + 0x26E),
            cpu.mem.rd8(CTX + 0x26F),
            cpu.mem.rd32(CTX + 0x87C),
        );
        if retail != port || after != (ctx.ramp, ctx.latch, ctx.accum) || retail_fade != port_fade {
            failures.push(format!(
                "char {character} art {art:#x} sel {select} frame {frame:#x} cur {cursor} \
                 ramp {ramp:#x} acc {accum:#x} latch {latch}: retail {retail:x?} {after:x?} fade {retail_fade:#x}, \
                 port {port:x?} {:x?} fade {port_fade:#x}",
                (ctx.ramp, ctx.latch, ctx.accum)
            ));
        }
    }
    assert!(armed > 10_000, "only {armed} rounds armed the tween");
    assert!(ramps > 500, "only {ramps} rounds ramped the fade");
    assert!(
        failures.is_empty(),
        "{} rounds differ:\n{}",
        failures.len(),
        failures[..failures.len().min(30)].join("\n")
    );
}
