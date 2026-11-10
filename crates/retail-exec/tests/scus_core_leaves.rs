//! Three small `SCUS_942.54` routines the scene and sound hosts stand on.
//!
//! - The scene control block's allocate-and-reset `FUN_8003A024` against
//!   `SceneControlBlockReset::install`: the whole `0x64`-byte block over
//!   garbage, the two pointer globals, the two cleared globals and the
//!   failed-allocation code.
//! - The timed sound release `FUN_800267FC` against `SoundReleaseTimer::tick`,
//!   call by call over armed and idle timers, every frame step and both
//!   gates, with the teardown pair hooked.
//! - The shared minigame exit `FUN_80026018` against
//!   `World::minigame_return_warp` and `credit_casino_coins`: the coin bank
//!   and its ceiling, the party-standing bit and the scene-name restore.

use crate::common::{Rng, scus_cpu};
use legaia_engine_core::world::World;
use legaia_engine_minigames::muscle_dome::{PARTY_STANDING_BIT, credit_casino_coins};
use legaia_engine_system::scus_leaf_kernels::{
    SCENE_CONTROL_BLOCK_ALLOC_FAIL_CODE, SCENE_CONTROL_BLOCK_RESET, SCENE_CONTROL_BLOCK_SIZE,
    SCENE_RESET_CLEARED_GLOBALS,
};
use legaia_engine_system::sound_state::{SoundReleaseTick, SoundReleaseTimer};
use psx_retail_exec::{A0, A1, A2, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const BLOCK_RESET: u32 = 0x8003_A024;
const ALLOC: u32 = 0x8001_7888;
const BLOCK_PTR: u32 = 0x801C_6EA4;
const TABLE_PTR: u32 = 0x801C_6EA0;
const ERROR_CODE: u32 = 0x8007_B828;
const BLOCK: u32 = 0x8012_0000;

#[test]
fn the_scene_control_block_reset_is_retails() {
    differential!(scus(BLOCK_RESET));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let asked = Rc::new(RefCell::new(Vec::new()));
    let a = asked.clone();
    cpu.hook(ALLOC, move |r, _| {
        a.borrow_mut().push((r.r[A0], r.r[A1]));
        r.r[psx_retail_exec::V0] = BLOCK;
    });
    let mut rng = Rng(0x5343_4252);
    for _ in 0..500 {
        let mut port = [0u8; SCENE_CONTROL_BLOCK_SIZE];
        for b in port.iter_mut() {
            *b = rng.next() as u8;
        }
        cpu.mem.load(BLOCK, &port);
        for g in SCENE_RESET_CLEARED_GLOBALS {
            cpu.mem.wr32(g, rng.next() | 1);
        }
        cpu.mem.wr32(ERROR_CODE, 0);
        asked.borrow_mut().clear();
        assert_eq!(cpu.call(BLOCK_RESET, &[]), Stop::Returned);
        SCENE_CONTROL_BLOCK_RESET.install(&mut port);
        let retail = cpu.mem.read(BLOCK, SCENE_CONTROL_BLOCK_SIZE);
        assert_eq!(
            retail,
            port,
            "{:?}",
            crate::common::diff_offsets(&retail, &port)
        );
        assert_eq!(*asked.borrow(), vec![(0, SCENE_CONTROL_BLOCK_SIZE as u32)]);
        assert_eq!(cpu.mem.rd32(BLOCK_PTR), BLOCK);
        assert_eq!(
            cpu.mem.rd32(TABLE_PTR),
            SCENE_CONTROL_BLOCK_RESET.fallback_table
        );
        for g in SCENE_RESET_CLEARED_GLOBALS {
            assert_eq!(cpu.mem.rd32(g), 0, "{g:#x}");
        }
        assert_eq!(cpu.mem.rd32(ERROR_CODE), 0);
    }
    // A failed allocation parks its code and carries on through the null.
    cpu.stub(ALLOC, 0);
    assert_eq!(cpu.call(BLOCK_RESET, &[]), Stop::Returned);
    assert_eq!(
        cpu.mem.rd32(ERROR_CODE),
        SCENE_CONTROL_BLOCK_ALLOC_FAIL_CODE
    );
    assert_eq!(cpu.mem.rd32(BLOCK_PTR), 0);
}

const RELEASE_TICK: u32 = 0x8002_67FC;
const TEARDOWN: u32 = 0x8002_657C;
const SEQ_STOP: u32 = 0x8006_4370;
const ARMED: u32 = 0x8007_BB20;
const DEADLINE: u32 = 0x8007_BB2C;
const ELAPSED: u32 = 0x8007_BB34;
const SOURCE: u32 = 0x8007_052C;
const DUAL_GATE: u32 = 0x8007_B868;
const FRAME_STEP: u32 = 0x1F80_0393;

#[test]
fn the_timed_sound_release_is_retails() {
    differential!(scus(RELEASE_TICK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let torn = Rc::new(RefCell::new(Vec::new()));
    let t = torn.clone();
    cpu.hook(TEARDOWN, move |r, _| {
        t.borrow_mut().push((0, r.r[A0], r.r[A1]))
    });
    let t = torn.clone();
    cpu.hook(SEQ_STOP, move |r, _| t.borrow_mut().push((1, r.r[A0], 0)));
    let mut rng = Rng(0x5352_454C);
    let (mut fired, mut released) = (0, 0);
    for case in 0..3000 {
        let mut timer = SoundReleaseTimer {
            armed: rng.below(8) != 0,
            deadline: match rng.below(8) {
                0 => rng.next() as i32,
                _ => rng.below(40) as i32,
            },
            elapsed: rng.below(4) as i32,
        };
        let voice = rng.next() as i16;
        cpu.mem.wr32(ARMED, u32::from(timer.armed));
        cpu.mem.wr32(DEADLINE, timer.deadline as u32);
        cpu.mem.wr32(ELAPSED, timer.elapsed as u32);
        cpu.mem.wr16(SOURCE + 0xA, voice as u16);
        for call in 0..64 {
            let step = 1 + rng.below(4) as u8;
            let active = rng.below(4) != 0;
            let gate = rng.below(4) == 0;
            cpu.mem.wr8(FRAME_STEP, step);
            cpu.mem.wr16(SOURCE + 8, u16::from(active));
            cpu.mem.wr32(DUAL_GATE, u32::from(gate));
            torn.borrow_mut().clear();
            assert_eq!(cpu.call(RELEASE_TICK, &[]), Stop::Returned);
            let tick = timer.tick(step, active, gate);
            let ctx = format!("case {case} call {call}: {timer:?} {tick:?}");
            assert_eq!(cpu.mem.rd32(ARMED) != 0, timer.armed, "{ctx}");
            assert_eq!(cpu.mem.rd32(ELAPSED) as i32, timer.elapsed, "{ctx}");
            let want = match tick {
                SoundReleaseTick::Fired {
                    release_voice: true,
                } => {
                    released += 1;
                    vec![(0, 0, SOURCE), (1, i32::from(voice) as u32, 0)]
                }
                _ => vec![],
            };
            assert_eq!(*torn.borrow(), want, "{ctx}");
            fired += usize::from(matches!(tick, SoundReleaseTick::Fired { .. }));
        }
    }
    assert!(fired > 1500 && released > 800, "{fired} {released}");
}

const MINIGAME_EXIT: u32 = 0x8002_6018;
const COPY: u32 = 0x8001_A8B0;
const COINS: u32 = 0x8008_45A4;
const WINNINGS: u32 = 0x8008_4440;
const STANDING: u32 = 0x8007_BD60;
const SCENE_NAME: u32 = 0x8008_4548;
const SCENE_BACKUP: u32 = 0x8007_BAE8;

#[test]
fn the_minigame_exit_banks_what_retail_banks() {
    differential!(scus(MINIGAME_EXIT));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let copies = Rc::new(RefCell::new(Vec::new()));
    let c = copies.clone();
    cpu.hook(COPY, move |r, _| {
        c.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2]))
    });
    let mut rng = Rng(0x4D47_4558);
    let mut capped = 0;
    for case in 0..20_000 {
        let coins = match rng.below(4) {
            0 => 9_999_999 - rng.below(2000),
            _ => rng.below(10_000_000),
        };
        let winnings = match rng.below(4) {
            0 => rng.below(0x4000_0000),
            _ => rng.below(5000),
        };
        let standing = rng.next() as u8;
        cpu.mem.wr32(COINS, coins);
        cpu.mem.wr32(WINNINGS, winnings);
        cpu.mem.wr8(STANDING, standing);
        copies.borrow_mut().clear();
        assert_eq!(cpu.call(MINIGAME_EXIT, &[]), Stop::Returned);
        let retail = cpu.mem.rd32(COINS);

        let mut w = World::new();
        w.minigames.casino_coins = coins;
        w.minigames.winnings = winnings;
        w.minigames.scene_backup = Some("town01".into());
        w.minigame_return_warp();
        assert_eq!(
            retail, w.minigames.casino_coins,
            "case {case}: {coins} + {winnings}"
        );
        assert_eq!(retail, credit_casino_coins(coins, winnings as i32));
        assert_eq!(w.active_scene_label, "town01");
        // The scene name comes back out of its backup, eight bytes.
        assert_eq!(*copies.borrow(), vec![(SCENE_NAME, SCENE_BACKUP, 8)]);
        assert_eq!(cpu.mem.rd8(STANDING), standing | PARTY_STANDING_BIT);
        capped += u32::from(retail == 9_999_999);
    }
    assert!(capped > 4000, "{capped}");
}
