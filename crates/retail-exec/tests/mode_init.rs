//! The mode table's INIT handlers in `SCUS_942.54` against
//! `engine_mode::mode`'s staging plans: the shared core-state reset
//! `FUN_80025CB4` against `CORE_STATE_RESET`; the three staging wrappers
//! (CONFIG `FUN_80025C68`, MAIN `FUN_80025B64`, GAME OVER `FUN_80025B30`)
//! against `mode_init_stage`; the three bare ones (MONSTER TEST
//! `FUN_8002611C`, READ `FUN_8002612C`, BATTLE `FUN_800565D8`) against
//! `mode_init_bare`; and the OTHER INIT dispatcher `FUN_80025980` for every
//! warp sub-id against `other_warp_init_stage`.
//!
//! Every call out of a handler returns at once (`Cpu::stub_outside`) and is
//! recorded; the slot-A loader `FUN_8003EBE4` is hooked to record its
//! parameter. Each handler is reached through its mode-table row
//! (`0x8007078C`, `+0x10`).

use crate::common::{Rng, scus_cpu};
use legaia_engine_mode::mode::{
    CORE_STATE_RESET, GameMode, ModeInitBare, mode_init_bare, mode_init_stage,
    other_warp_init_stage,
};
use psx_retail_exec::{A0, Cpu, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const TABLE: u32 = 0x8007_078C;
const CORE_RESET: u32 = 0x8002_5CB4;
const CONFIG_INIT: u32 = 0x8002_5C68;
const MAIN_INIT: u32 = 0x8002_5B64;
const GAME_OVER_INIT: u32 = 0x8002_5B30;
const MONSTER_TEST_INIT: u32 = 0x8002_611C;
const READ_INIT: u32 = 0x8002_612C;
const BATTLE_INIT: u32 = 0x8005_65D8;
const OTHER_INIT: u32 = 0x8002_5980;
const SOUND_DETACH: u32 = 0x8002_689C;
const LOAD_A: u32 = 0x8003_EBE4;
const MODE_WORD: u32 = 0x8007_B83C;
const WARP_SUB: u32 = 0x8007_BA34;

fn handler(cpu: &mut Cpu, mode: GameMode) -> u32 {
    cpu.mem.rd32(TABLE + mode.as_index() as u32 * 24 + 0x10)
}

/// Run `entry` with every call out of `[entry, entry + 0x400)` stubbed:
/// the calls made and the loader parameters seen.
fn run(cpu: &mut Cpu, entry: u32, loads: &Rc<RefCell<Vec<u32>>>) -> Vec<u32> {
    loads.borrow_mut().clear();
    cpu.stub_outside = Some((entry, entry + 0x400));
    assert_eq!(cpu.call(entry, &[]), Stop::Returned);
    cpu.stub_outside = None;
    cpu.calls.clone()
}

fn rig() -> Option<(Cpu, Rc<RefCell<Vec<u32>>>)> {
    let mut cpu = scus_cpu()?;
    let loads = Rc::new(RefCell::new(Vec::new()));
    let l = loads.clone();
    cpu.hook(LOAD_A, move |r, _| l.borrow_mut().push(r.r[A0]));
    Some((cpu, loads))
}

#[test]
fn the_core_state_reset_matches_the_port() {
    differential!(scus(CORE_RESET));
    let Some((mut cpu, loads)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5245_5345);
    for _ in 0..50 {
        for a in (0x8007_B600..0x8007_BA60).step_by(4) {
            cpu.mem.wr32(a, rng.next());
        }
        // The retail leg: the debug word clear.
        cpu.mem.wr32(0x8007_B98C, 0);
        let calls = run(&mut cpu, CORE_RESET, &loads);
        assert_eq!(calls, [0x8001_D7F8], "the scene-name sync only");
        let r = CORE_STATE_RESET;
        let m = &mut cpu.mem;
        assert_eq!(m.rd16(0x8007_B718), r.brightness);
        assert_eq!(m.rd16(0x8007_B6F4), r.gte_h_zoom);
        assert_eq!(m.rd32(0x8007_B8B8), r.field_warm_entry);
        assert_eq!(m.rd8(0x8007_B648), r.b648);
        assert_eq!(m.rd16(MODE_WORD), r.game_mode);
        assert_eq!(m.rd32(0x8007_B874), r.pad_pressed_edge);
        assert_eq!(m.rd32(0x8007_B830), r.b830_b8c8);
        assert_eq!(m.rd32(0x8007_B8C8), r.b830_b8c8);
        assert_eq!(m.rd16(0x8007_B768), r.data_field_index);
        for a in [0x8007_B6FC, 0x8007_B6C8, 0x8007_B9C4] {
            assert_eq!(m.rd32(a), r.counters_cleared, "{a:#x}");
        }
        assert_eq!(m.rd16(0x8007_BA36), r.retail_ba36_b71c);
        assert_eq!(m.rd32(0x8007_B71C), u32::from(r.retail_ba36_b71c));
        assert_eq!(m.rd32(0x8007_B900), r.b900);
    }
}

#[test]
fn the_staging_inits_load_and_enter_what_the_plan_says() {
    differential!(scus(CONFIG_INIT), scus(MAIN_INIT), scus(GAME_OVER_INIT));
    let Some((mut cpu, loads)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    for mode in [
        GameMode::ConfigInit,
        GameMode::MainInit,
        GameMode::GameOverInit,
    ] {
        let entry = handler(&mut cpu, mode);
        let calls = run(&mut cpu, entry, &loads);
        let plan = mode_init_stage(mode).expect("a staging init");
        assert_eq!(*loads.borrow(), [plan.overlay_a_param as u32], "{mode:?}");
        assert_eq!(
            calls.last(),
            Some(&plan.overlay_entry),
            "{mode:?} {calls:x?}"
        );
        assert_eq!(
            calls.contains(&CORE_RESET) && calls.contains(&SOUND_DETACH),
            plan.runs_core_reset,
            "{mode:?} {calls:x?}"
        );
    }
}

#[test]
fn the_bare_inits_do_only_what_the_port_says() {
    differential!(scus(MONSTER_TEST_INIT), scus(READ_INIT), scus(BATTLE_INIT));
    let Some((mut cpu, loads)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    for mode in [
        GameMode::MonsterTest,
        GameMode::ReadInit,
        GameMode::BattleInit,
    ] {
        let entry = handler(&mut cpu, mode);
        cpu.mem.wr16(MODE_WORD, 0x7777);
        let calls = run(&mut cpu, entry, &loads);
        assert!(loads.borrow().is_empty(), "{mode:?} loads nothing");
        match mode_init_bare(mode).expect("a bare init") {
            ModeInitBare::SetsMode(m) => {
                assert!(calls.is_empty(), "{mode:?} {calls:x?}");
                assert_eq!(cpu.mem.rd16(MODE_WORD), m.as_index() as u16, "{mode:?}");
            }
            ModeInitBare::Calls(va) => {
                assert_eq!(calls, [va], "{mode:?}");
                assert_eq!(cpu.mem.rd16(MODE_WORD), 0x7777, "{mode:?}");
            }
        }
    }
}

#[test]
fn other_init_stages_every_warp_sub_id() {
    differential!(scus(OTHER_INIT));
    let Some((mut cpu, loads)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let entry = handler(&mut cpu, GameMode::OtherInit);
    for sub in 0..7i16 {
        cpu.mem.wr16(WARP_SUB, sub as u16);
        let calls = run(&mut cpu, entry, &loads);
        let plan = other_warp_init_stage(sub).expect("in range");
        assert_eq!(*loads.borrow(), [plan.overlay_a_param as u32], "sub {sub}");
        assert!(calls.contains(&plan.overlay_entry), "sub {sub}: {calls:x?}");
        assert!(!plan.runs_core_reset && !calls.contains(&CORE_RESET));
        // The hand-off to OTHER MODE.
        assert_eq!(
            cpu.mem.rd16(MODE_WORD),
            GameMode::OtherMode.as_index() as u16
        );
    }
}
