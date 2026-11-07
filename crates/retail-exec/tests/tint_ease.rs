//! The packed-colour ease `FUN_80050F30` against `packed3_approach_target`
//! (the live tint machine's step) and `ease_actor_state`: words with the top
//! two bits set and clear, lanes on, under and over their targets, every
//! step scale and frame step the tint arms pass.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::battle_formulas::packed3_approach_target;
use legaia_engine_battle_vm::battle_impact_fx::ease_actor_state;
use psx_retail_exec::{SP, Stop, V0};

const EASE: u32 = 0x8005_0F30;
const FRAME_STEP: u32 = 0x1F80_0393;

#[test]
fn the_colour_ease_is_the_retail_ease() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5449_4E54);
    let mut top_cleared = 0;
    for n in 0..60_000 {
        let word = match n % 3 {
            0 => rng.next(),
            1 => rng.next() | 0xC000_0000,
            _ => 0x2008_0200 ^ (rng.below(0x40) << (10 * rng.below(3))),
        };
        let t = [rng.next() as u8, rng.next() as u8, rng.next() as u8];
        let t = if n % 5 == 0 { [0x80; 3] } else { t };
        let step = [1u8, 2, 4, 8, 0x10, 0xFF][rng.below(6) as usize];
        let dt = [1u8, 2, 3][rng.below(3) as usize];
        cpu.mem.wr8(FRAME_STEP, dt);
        // The fifth argument rides the caller's stack at `sp + 0x10`.
        cpu.mem.wr32(0x801F_FF00 + 0x10, u32::from(step));
        let stop = cpu.call(
            EASE,
            &[word, u32::from(t[0]), u32::from(t[1]), u32::from(t[2])],
        );
        assert_eq!(stop, Stop::Returned);
        assert_eq!(cpu.regs.r[SP], 0x801F_FF00);
        let retail = cpu.regs.r[V0];
        let port = packed3_approach_target(word, t[0], t[1], t[2], step, dt);
        assert_eq!(
            retail, port,
            "word {word:#010x} target {t:?} step {step} dt {dt}"
        );
        let lanes = u32::from(step) * u32::from(dt) * 8;
        assert_eq!(
            ease_actor_state(word, t, lanes),
            retail,
            "ease_actor_state, word {word:#010x} target {t:?} lanes {lanes}"
        );
        if word & 0xC000_0000 != 0 && retail & 0xC000_0000 == 0 {
            top_cleared += 1;
        }
    }
    assert!(top_cleared > 1_000, "{top_cleared}");
}
