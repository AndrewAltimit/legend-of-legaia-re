//! The field player tick `FUN_801D1344` (PROT 0897), run whole with every
//! callee hooked and logged, against the slices the port keeps of it in
//! `field_warp_tile`: the post-warp hold's drain, the walk gate in front of
//! the walk-regen tick and the pad controller, and the order the frame's legs
//! run in. Beside it the horizon emitter's arming setter `FUN_801D8258`
//! (`EmitterGate::arm`), which the tick calls on the scene's three words.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::field_warp_tile::{
    MOVEMENT_LOCK, WarpTimer, drain_post_warp_hold, player_tick_walks,
};
use legaia_engine_field::world_map::EmitterGate;
use psx_retail_exec::{A0, A1, A2, A3, Cpu, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const TICK: u32 = 0x801D_1344;
const ARM: u32 = 0x801D_8258;

const HALT_LEG: u32 = 0x8001_96A4;
const HOOKED_ARM: u32 = ARM;
const SKIP_FADE: u32 = 0x801D_58F0;
const SKIP_MAP: u32 = 0x8001_FD44;
const PARTY_SYNC: u32 = 0x801C_F754;
const PARTY_HUD: u32 = 0x801D_0D38;
const WALK_REGEN: u32 = 0x801D_0B90;
const PAD_CONTROLLER: u32 = 0x801D_01B0;
const SETTLE: u32 = 0x801D_1BA0;
const CLIP_SET: u32 = 0x8002_4E08;
const ZONE_A: u32 = 0x801D_E234;
const ZONE_B: u32 = 0x801D_E3E0;
const CAMERA_EASE: u32 = 0x801D_B510;
const CAMERA_CLAMP: u32 = 0x801D_AA50;
const CAMERA_SHAKE: u32 = 0x801D_9D30;
const FRAME_END: u32 = 0x8001_72C0;
const TINT: u32 = 0x8004_695C;
const PRINT: u32 = 0x8001_A068;

const HALT: u32 = 0x8007_BAF4;
const PAD_EDGE_HI: u32 = 0x8007_B876;
const DEBUG: u32 = 0x8007_B6D0;
const TINT_WORD: u32 = 0x8007_3EE4;
const EMITTER: u32 = 0x8007_BCD0;
const PROGRESS: u32 = 0x8007_B868;
const SCRATCH: u32 = 0x1F80_0394;
const FRAME_STEP: u32 = 0x1F80_0393;
const PAD_HELD: u32 = 0x8007_B850;
const HOLD: u32 = 0x8007_B6B4;
const WARP_TIMER: u32 = 0x8007_B6B0;
const POOL: u32 = 0x8007_C348;
const GRID_PTR: u32 = 0x801C_6EA4;
const CLIP_WANT: u32 = 0x8007_B8F8;
const CLIP_HAVE: u32 = 0x8007_B8A2;
const CLIP_BASE: u32 = 0x8007_B824;
const GATE: u32 = 0x801F_351C;

const PLAYER: u32 = 0x8013_0000;
const GRID: u32 = 0x8013_1000;
const LIST_A: u32 = 0x8013_2000;
const LIST_B: u32 = 0x8013_2100;

fn cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    Some(cpu)
}

#[test]
fn the_emitter_arm_matches_retail() {
    differential!(prot(897, ARM));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4152_4D47);
    for run in 0..2000 {
        let args: [u32; 4] = std::array::from_fn(|_| rng.next());
        for i in 0..4 {
            cpu.mem.wr32(GATE + i * 4, 0xEEEE_EEEE);
        }
        assert_eq!(cpu.call(ARM, &args), Stop::Returned, "run {run}");
        let mut gate = EmitterGate::default();
        gate.arm(args[1], args[2], args[3]);
        let got: [u32; 4] = std::array::from_fn(|i| cpu.mem.rd32(GATE + i as u32 * 4));
        assert_eq!(
            got,
            [
                u32::from(gate.armed),
                gate.scale,
                gate.angle_step,
                gate.ot_layer
            ],
            "run {run}"
        );
    }
}

#[test]
fn the_player_tick_gates_its_legs_as_retail_does() {
    differential!(prot(897, TICK));
    let Some(mut cpu) = cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let log: Rc<RefCell<Vec<(u32, [u32; 4])>>> = Rc::new(RefCell::new(Vec::new()));
    for va in [
        HALT_LEG,
        HOOKED_ARM,
        SKIP_FADE,
        SKIP_MAP,
        PARTY_SYNC,
        PARTY_HUD,
        WALK_REGEN,
        PAD_CONTROLLER,
        SETTLE,
        CLIP_SET,
        ZONE_A,
        ZONE_B,
        CAMERA_EASE,
        CAMERA_CLAMP,
        CAMERA_SHAKE,
        FRAME_END,
        TINT,
        PRINT,
    ] {
        let l = log.clone();
        cpu.hook(va, move |r, _| {
            l.borrow_mut()
                .push((va, [r.r[A0], r.r[A1], r.r[A2], r.r[A3]]));
        });
    }
    cpu.mem.wr32(POOL + 0x1C, PLAYER);
    cpu.mem.wr32(POOL + 0x04, LIST_A);
    cpu.mem.wr32(POOL + 0x0C, LIST_B);
    cpu.mem.wr32(GRID_PTR, GRID);
    cpu.mem.wr32(LIST_A, 0);
    cpu.mem.wr32(LIST_B, 0);
    let mut rng = Rng(0x504C_5449);
    let (mut walked, mut held, mut engaged_n, mut halted, mut armed, mut zoned, mut clips) =
        (0, 0, 0, 0, 0, 0, 0);
    for run in 0..20000 {
        let halt = rng.below(16) == 0;
        let engaged = rng.below(3) == 0;
        let scratch = (rng.next() & 0x0041_0400 & [0, !0, !0][rng.below(3) as usize])
            | (rng.next() & 0x0000_00FF);
        let warp = WarpTimer {
            timer: match rng.below(4) {
                0 => 1 + rng.below(0x26) as i32,
                1 => -1000,
                _ => 0,
            },
            hold: match rng.below(3) {
                0 => rng.below(0x29) as i32,
                1 => 1 + rng.below(3) as i32,
                _ => 0,
            },
            dest: (0, 0),
        };
        let step = 1 + rng.below(3) as u8;
        let emitter: [u32; 3] = match rng.below(3) {
            0 => [0, rng.below(2) * 0x10, rng.next()],
            1 => [rng.next(), rng.next(), rng.next()],
            _ => [0, 0, rng.next()],
        };
        let flags = (rng.next() & !MOVEMENT_LOCK) | if engaged { MOVEMENT_LOCK } else { 0 };
        let bits62 = rng.next() as u16;
        let grid62 = rng.below(6) as u8;
        let (want, have) = (rng.below(3) as u16, rng.below(3) as u16);
        let clip_base = rng.below(0x40) as u16;
        cpu.mem.wr32(HALT, u32::from(halt));
        cpu.mem.wr16(PAD_EDGE_HI, 0);
        cpu.mem.wr32(DEBUG, 0);
        cpu.mem.wr32(TINT_WORD, 0x00FF_FFFF);
        for (i, w) in emitter.iter().enumerate() {
            cpu.mem.wr32(EMITTER + 4 * i as u32, *w);
        }
        // The prologue's skip packet needs a held Start under scratch bit 26;
        // the progress word keeps it shut here.
        cpu.mem.wr32(PROGRESS, 1);
        cpu.mem.wr32(PAD_HELD, rng.next() & 0xFFFF);
        cpu.mem.wr32(SCRATCH, scratch);
        cpu.mem.wr8(FRAME_STEP, step);
        cpu.mem.wr32(HOLD, warp.hold as u32);
        cpu.mem.wr32(WARP_TIMER, warp.timer as u32);
        cpu.mem.wr32(PLAYER + 0x10, flags);
        cpu.mem.wr16(PLAYER + 0x14, rng.next() as u16);
        cpu.mem.wr16(PLAYER + 0x18, rng.next() as u16);
        cpu.mem.wr16(PLAYER + 0x62, bits62);
        cpu.mem.wr8(GRID + 0x62, grid62);
        cpu.mem.wr16(CLIP_WANT, want);
        cpu.mem.wr16(CLIP_HAVE, have);
        cpu.mem.wr16(CLIP_BASE, clip_base);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(TICK, &[PLAYER]), Stop::Returned, "run {run}");
        let got: Vec<u32> = log.borrow().iter().map(|c| c.0).collect();
        let ctx = format!(
            "run {run} halt {halt} engaged {engaged} scratch {scratch:#x} {warp:?} step {step}"
        );
        if halt {
            // The halt word locks the player and runs one leg.
            assert_eq!(got, [HALT_LEG], "{ctx}");
            assert_eq!(cpu.mem.rd32(PLAYER + 0x10), flags | MOVEMENT_LOCK, "{ctx}");
            assert_eq!(cpu.mem.rd32(HOLD) as i32, warp.hold, "{ctx}");
            halted += 1;
            continue;
        }
        let mut port = warp;
        drain_post_warp_hold(&mut port, step);
        let scripted = scratch & 0x400 != 0;
        let walks = player_tick_walks(engaged, scripted, &port);
        let free = !engaged && !scripted;
        let follow = free || scratch & 0x1_0000 != 0;
        let mut legs = Vec::new();
        if emitter[0] != 0 || emitter[1] != 0 {
            legs.push(HOOKED_ARM);
            armed += 1;
        }
        legs.extend([PARTY_SYNC, PARTY_HUD]);
        if walks {
            legs.extend([WALK_REGEN, PAD_CONTROLLER]);
        }
        legs.push(SETTLE);
        if want != have {
            legs.push(CLIP_SET);
            clips += 1;
        }
        if follow {
            if scratch & 0x40_0000 != 0 {
                legs.extend([ZONE_A, ZONE_B]);
                zoned += 1;
            }
            legs.extend([CAMERA_EASE, CAMERA_CLAMP]);
        } else {
            legs.push(CAMERA_SHAKE);
        }
        legs.push(FRAME_END);
        assert_eq!(got, legs, "legs: {ctx}");
        assert_eq!(cpu.mem.rd32(HOLD) as i32, port.hold, "hold: {ctx}");
        assert_eq!(cpu.mem.rd32(WARP_TIMER) as i32, port.timer, "timer: {ctx}");
        assert_eq!(cpu.mem.rd32(PLAYER + 0x10), flags, "player flags: {ctx}");
        assert_eq!(
            cpu.mem.rd16(PLAYER + 0x62),
            if free { bits62 & 0xFFF7 } else { bits62 },
            "+0x62: {ctx}"
        );
        assert_eq!(
            cpu.mem.rd8(GRID + 0x62),
            grid62.saturating_sub(step),
            "the grid's countdown: {ctx}"
        );
        assert_eq!(cpu.mem.rd16(CLIP_HAVE), want, "clip latch: {ctx}");
        if let Some((_, a)) = log.borrow().iter().find(|c| c.0 == HOOKED_ARM) {
            assert_eq!(a, &[0, emitter[0], emitter[1], emitter[2]], "{ctx}");
        }
        if let Some((_, a)) = log.borrow().iter().find(|c| c.0 == CLIP_SET) {
            let clip = want.wrapping_add(clip_base) as i16;
            assert_eq!((a[0], a[1] as i32), (PLAYER, i32::from(clip)), "{ctx}");
        }
        walked += u32::from(walks);
        held += u32::from(free && !walks);
        engaged_n += u32::from(!free);
    }
    assert!(
        walked > 2000
            && held > 2000
            && engaged_n > 2000
            && halted > 500
            && armed > 2000
            && zoned > 500
            && clips > 2000,
        "{walked} {held} {engaged_n} {halted} {armed} {zoned} {clips}"
    );
}
