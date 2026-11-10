//! Four small `SCUS_942.54` leaves against the `World` and menu kernels that
//! port them: the party-ability bit test `FUN_800431D0` against
//! `World::party_has_ability`, the timed sound-release arm `FUN_800267A8`
//! against `World::arm_sound_release` (its `gp` cells and the volume-shim
//! arguments), the one-shot sound setup `FUN_8002689C` against
//! `World::detach_sound`, and the cold reset `FUN_8001FFA4` against
//! `GAME_STATE_COLD_RESET` over the whole live game-state block.

use crate::common::{Rng, scus_cpu};
use legaia_engine_core::world::World;
use legaia_engine_menus::new_game::GAME_STATE_COLD_RESET;
use psx_retail_exec::{A0, A1, A2, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const HAS_ABILITY: u32 = 0x8004_31D0;
const ARM: u32 = 0x8002_67A8;
const DETACH: u32 = 0x8002_689C;
const COLD_RESET: u32 = 0x8001_FFA4;
const GP: u32 = 0x8007_B318;
const MASK: u32 = 0x8007_4358;
const AUDIO_LEVEL: u32 = 0x8007_B910;
const SHIM: u32 = 0x8006_2004;
const SHIM_VOICE: u32 = 0x8007_0536;
const SPU_CMD: u32 = 0x8006_5440;
const MASTER_VOLUME: u32 = 0x8006_2AA0;
const STATE: u32 = 0x8008_4140;
const STATE_LEN: u32 = 0x1A18;

#[test]
fn the_party_ability_test_reads_the_mask_as_retail_reads_it() {
    differential!(scus(HAS_ABILITY));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut w = World::new();
    let mut rng = Rng(0x4142_494C);
    let mut set = 0;
    for case in 0..200 {
        for (i, word) in w.party.party_ability_mask.iter_mut().enumerate() {
            *word = match rng.below(3) {
                0 => 0,
                1 => 1 << rng.below(32),
                _ => rng.next(),
            };
            cpu.mem.wr32(MASK + 4 * i as u32, *word);
        }
        let words = w.party.party_ability_mask.len() as u32;
        for index in 0..32 * words {
            assert_eq!(cpu.call(HAS_ABILITY, &[index]), Stop::Returned);
            let r = cpu.regs.r[V0] != 0;
            assert_eq!(
                r,
                w.party_has_ability(index as u8),
                "case {case} bit {index}"
            );
            set += u32::from(r);
        }
    }
    assert!(set > 1000, "{set}");
}

#[test]
fn the_sound_release_arm_latches_what_retail_latches() {
    differential!(scus(ARM));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let shim = Rc::new(RefCell::new(None));
    let s = shim.clone();
    cpu.hook(SHIM, move |r, _| {
        *s.borrow_mut() = Some((r.r[A0], r.r[A1], r.r[A2]));
    });
    let mut rng = Rng(0x534E_4452);
    for case in 0..2000 {
        let mut w = World::new();
        let level = match rng.below(4) {
            0 => 0xD7,
            1 => 0,
            2 => rng.below(0x100) as i32,
            _ => rng.next() as i32,
        };
        w.audio.levels.configured_level = level;
        cpu.mem.wr32(AUDIO_LEVEL, level as u32);
        let voice = rng.next() as u16;
        cpu.mem.wr16(SHIM_VOICE, voice);
        // The field op's operand is an `s16`.
        let deadline = i32::from(rng.next() as i16);
        for off in [0x808, 0x80C, 0x810, 0x814, 0x81C] {
            cpu.mem.wr32(GP + off, 0xA5A5_A5A5);
        }
        *shim.borrow_mut() = None;
        // The one caller passes tag 0 (`0x801E01B4` in the field overlay).
        assert_eq!(cpu.call(ARM, &[0, deadline as u32]), Stop::Returned);
        w.arm_sound_release(deadline);
        let arm = w.audio.sound_arm.expect("armed");
        let what = format!("case {case} level {level:#x} deadline {deadline}");
        assert_eq!(cpu.mem.rd32(GP + 0x808), 1, "{what}");
        assert_eq!(cpu.mem.rd32(GP + 0x810), arm.tag, "{what}");
        assert_eq!(cpu.mem.rd32(GP + 0x814), arm.deadline, "{what}");
        assert_eq!(cpu.mem.rd32(GP + 0x80C) as i32, arm.latched_level, "{what}");
        assert_eq!(cpu.mem.rd32(GP + 0x81C), 0, "{what}");
        let (a0, a1, a2) = shim.borrow().expect("shim called");
        assert_eq!(a0, voice as i16 as i32 as u32, "{what}");
        assert_eq!(a1, arm.shim_level() as i32 as u32, "{what}");
        assert_eq!(a2, arm.shim_deadline(), "{what}");
    }
}

#[test]
fn the_sound_setup_latches_once_as_retail_does() {
    differential!(scus(DETACH));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let calls = Rc::new(RefCell::new(Vec::new()));
    for f in [SPU_CMD, MASTER_VOLUME] {
        let c = calls.clone();
        cpu.hook(f, move |r, _| c.borrow_mut().push((f, r.r[A0], r.r[A1])));
    }
    let mut w = World::new();
    cpu.mem.wr32(GP + 0x804, 0);
    for round in 0..4 {
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(DETACH, &[]), Stop::Returned);
        let first = w.detach_sound();
        assert_eq!(cpu.mem.rd32(GP + 0x804), 1, "round {round}");
        let want = if first {
            vec![(SPU_CMD, 0x32, 0x32), (MASTER_VOLUME, 0x7F, 0x7F)]
        } else {
            Vec::new()
        };
        assert_eq!(*calls.borrow(), want, "round {round}");
        assert_eq!(first, round == 0);
    }
}

#[test]
fn the_cold_reset_leaves_the_game_state_as_retail_leaves_it() {
    differential!(scus(COLD_RESET));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let chained = Rc::new(RefCell::new(Vec::new()));
    for f in [0x8003_4A6C, 0x8002_614C] {
        let c = chained.clone();
        cpu.hook(f, move |r, _| c.borrow_mut().push((f, r.r[A0])));
    }
    let mut rng = Rng(0x434F_4C44);
    for i in 0..STATE_LEN + 0x40 {
        cpu.mem.wr8(STATE + i, rng.next() as u8 | 1);
    }
    for a in [0x8007_B910, 0x8007_B6EC, 0x8007_B750, 0x8007_BAD0] {
        cpu.mem.wr32(a, 0x5A5A_5A5A);
    }
    assert_eq!(cpu.call(COLD_RESET, &[]), Stop::Returned);
    let c = GAME_STATE_COLD_RESET;
    let mut want = vec![0u8; STATE_LEN as usize];
    for (off, v) in [
        (0x434, c.flag_at_0x434),
        (0x43C, c.brightness_ref as u32),
        (0x440, c.voice_volume as u32),
    ] {
        want[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    let got = cpu.mem.read(STATE, STATE_LEN as usize);
    assert_eq!(got, want, "{:?}", crate::common::diff_offsets(&got, &want));
    // The block ends where the zero fill does.
    assert_ne!(cpu.mem.rd8(STATE + STATE_LEN), 0);
    assert_eq!(cpu.mem.rd32(0x8007_B910) as i32, c.audio_level);
    assert_eq!(cpu.mem.rd32(0x8007_B6EC) as i32, c.bgm_volume_raw);
    assert_eq!(cpu.mem.rd32(0x8007_B750), 0);
    assert_eq!(cpu.mem.rd32(0x8007_BAD0), 0);
    assert_eq!(
        *chained.borrow(),
        [(0x8003_4A6C, chained.borrow()[0].1), (0x8002_614C, 0)]
    );
}
