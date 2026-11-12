//! The HUD element spawner `FUN_801D8DE8` (PROT 0898) raising the message
//! bar, element `0x5B`, in mode `0` - what a cast module arm calls - with
//! the text-actor register `FUN_8003541C` and the glide leaf `FUN_801DB7B0`
//! hooked and every other call stubbed, against the port's raise
//! (`World`'s message-bar glide, read through
//! `legaia_engine_battle_hud::battle_message_bar_dy`): the plate is
//! registered at placement record 91's seat A and glides to its seat B over
//! the round's `ctx[+0x1C]` frames, so the bar's first-frame offset below
//! its rest seat is seat A less seat B. Every free handle slot is tried, and
//! the slot the spawner takes is the first free one.

use crate::common::{Rng, STACK_TOP, disc, scus_cpu};
use legaia_engine_battle_vm::battle_commit_log::{LAUNCH_GLIDE_FRAMES, LogLaunch};
use legaia_engine_core::world::World;
use psx_retail_exec::{A0, A1, A2, A3, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const SPAWN: u32 = 0x801D_8DE8;
const REGISTER: u32 = 0x8003_541C;
const GLIDE: u32 = 0x801D_B7B0;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const MESSAGE_BAR: u32 = 0x5B;

#[test]
fn the_message_bar_rises_from_where_retail_registers_it() {
    differential!(prot(898, SPAWN));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(898).expect("PROT 0898"));
    cpu.stub_outside = Some((OVERLAY_BASE, 0x8020_0000));
    let registered = Rc::new(RefCell::new(Vec::<(i32, i32)>::new()));
    let r2 = registered.clone();
    cpu.hook(REGISTER, move |r, m| {
        let y = m.rd32(r.r[SP] + 0x10) as i32;
        r2.borrow_mut().push((r.r[A3] as i32, y));
        r.r[V0] = 0x8015_0000;
    });
    let glides = Rc::new(RefCell::new(Vec::<(u32, u32, u32, i32, i32)>::new()));
    let g2 = glides.clone();
    cpu.hook(GLIDE, move |r, m| {
        let y = m.rd32(r.r[SP] + 0x10) as i16 as i32;
        g2.borrow_mut()
            .push((r.r[A0], r.r[A1], r.r[A2], r.r[A3] as i16 as i32, y));
    });
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x4D42_4152);
    for round in 0..200 {
        cpu.mem.load(CTX, &[0u8; 0x1400]);
        // The round reset's glide length, a glide flag, and handles taken.
        cpu.mem.wr8(CTX + 0x1C, LAUNCH_GLIDE_FRAMES);
        cpu.mem.wr8(CTX + 0x1B, rng.below(2) as u8);
        let taken = rng.below(0x28);
        for s in 0..taken {
            cpu.mem.wr32(CTX + 0x1074 + 4 * s, 0x8016_0000 + s);
        }
        cpu.mem.wr32(STACK_TOP + 0x10, 0);
        registered.borrow_mut().clear();
        glides.borrow_mut().clear();
        assert_eq!(
            cpu.call(SPAWN, &[MESSAGE_BAR, 0]),
            Stop::Returned,
            "round {round}"
        );
        let label = format!("round {round}");
        let reg = registered.borrow().clone();
        let gl = glides.borrow().clone();
        assert_eq!(reg.len(), 1, "{label}");
        assert_eq!(gl.len(), 1, "{label}");
        let (slot, total, flag, _, target_y) = gl[0];
        assert_eq!(slot, taken, "{label}: the first free handle");
        assert_eq!(
            cpu.mem.rd32(CTX + 0x1074 + 4 * slot),
            0x8015_0000,
            "{label}"
        );
        assert_eq!(total, u32::from(LAUNCH_GLIDE_FRAMES), "{label}");
        assert_eq!(flag, u32::from(cpu.mem.rd8(CTX + 0x1B)), "{label}");
        let (_, seat_a) = reg[0];

        // The port's raise, on its first frame.
        let mut w = World::new();
        w.mode = legaia_engine_mode::mode::SceneMode::Battle;
        w.battle.steal_caption = Some(legaia_engine_battle::battle_steal::StealCaption {
            text: "line".into(),
            owner: 0,
            subject: None,
        });
        let glide = LogLaunch::new(false);
        assert_eq!(u32::from(glide.total), total, "{label}");
        w.battle.message_bar_glide = Some(glide);
        assert_eq!(
            legaia_engine_battle_hud::battle_message_bar_dy(&w),
            seat_a - target_y,
            "{label}: seat A {seat_a}, seat B {target_y}"
        );
    }
}
