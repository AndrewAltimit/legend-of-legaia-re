//! The presentation tint machine: the commit's bystander sweep `FUN_8004E13C`
//! against `solo_commit_state`, and the per-actor arms of `FUN_80050120` against
//! `tint_sm_step` over every state, lane shape and fade gate.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::battle_formulas::{
    FadeInputs, TintWords, solo_commit_state, tint_sm_step,
};
use psx_retail_exec::Stop;

const SWEEP: u32 = 0x8004_E13C;
const SM: u32 = 0x8005_0120;
const RAND: u32 = 0x8005_6798;
const CUE_STOP: u32 = 0x8001_7910;
const GRID_HOOK: u32 = 0x8004_695C;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
const RECORDS: u32 = 0x801C_9348;
const FRAME_STEP: u32 = 0x1F80_0393;

fn actor(s: u32) -> u32 {
    0x8013_0000 + s * 0x400
}

#[test]
fn the_solo_sweep_is_retails() {
    differential!(scus(SWEEP));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(RAND, 1);
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x534F_4C4F);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, actor(s));
    }
    for round in 0..40_000 {
        let acting = rng.below(7);
        let target = match rng.below(6) {
            0 => 8,
            1 => 9,
            2 => 0xFF,
            _ => rng.below(7),
        };
        cpu.mem.wr8(CTX + 0x13, acting as u8);
        cpu.mem.wr8(actor(acting) + 0x1DD, target as u8);
        let mut seeded = Vec::new();
        for s in 0..7 {
            let hp = if rng.below(4) == 0 {
                0
            } else {
                1 + rng.below(99) as u16
            };
            let state = [0u8, 0, 1, 2, 3, 0xC8][rng.below(6) as usize];
            cpu.mem.wr16(actor(s) + 0x14C, hp);
            cpu.mem.wr8(actor(s) + 0x21C, state);
            seeded.push((hp, state));
        }
        assert_eq!(cpu.call(SWEEP, &[1]), Stop::Returned, "{round}");
        for s in 0..7u32 {
            let (hp, state) = seeded[s as usize];
            let port = solo_commit_state(s as u8, hp, state, acting as u8, target as u8);
            let retail = cpu.mem.rd8(actor(s) + 0x21C);
            assert_eq!(
                retail, port,
                "round {round} seat {s} hp {hp} state {state:#x} acting {acting} target {target:#x}"
            );
        }
    }
}

#[test]
fn the_tint_arms_are_retails() {
    differential!(scus(SM));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(CUE_STOP, 0);
    cpu.stub(GRID_HOOK, 0);
    cpu.mem.wr32(CTX_PTR, CTX);
    // The fading monster's bookkeeping path never takes the capture arms.
    cpu.mem.wr8(CTX + 0x269, 1);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, actor(s));
        cpu.mem.wr32(actor(s) + 0x22C, 0x8014_0000 + s * 0x100);
    }
    for i in 0..4 {
        cpu.mem.wr32(RECORDS + i * 4, 0x8015_0000 + i * 0x100);
    }
    let mut rng = Rng(0x5449_4E54);
    let mut failures = Vec::new();
    let mut faded = 0u32;
    for round in 0..60_000 {
        let dt = 1 + rng.below(4) as u8;
        cpu.mem.wr8(FRAME_STEP, dt);
        let mut seeded = Vec::new();
        for s in 0..8u32 {
            let a = actor(s);
            let lane = |rng: &mut Rng| match rng.below(4) {
                0 => 0,
                1 => 0x80,
                2 => rng.below(0x10),
                _ => rng.below(0x400),
            };
            let color =
                lane(&mut rng) | lane(&mut rng) << 10 | lane(&mut rng) << 20 | rng.below(4) << 30;
            let state = rng.below(0x0D) as u8;
            let blend = [0u32, 0x10, 0x800, 0x1000, 0x2000][rng.below(5) as usize];
            let selector = rng.below(4) as u8;
            let committed = [0u8, 8, 3, 0x20][rng.below(4) as usize];
            let knockdown = [3u8, 0x20, 9][rng.below(3) as usize];
            let capture = u8::from(rng.below(4) == 0);
            let mode = rng.next() & 0x00FF_FFFF;
            cpu.mem.wr32(a + 4, color);
            cpu.mem.wr32(a + 8, mode);
            cpu.mem.wr32(a + 0xC, blend);
            cpu.mem.wr8(a + 0x21C, state);
            cpu.mem.wr8(a + 0x21F, selector);
            cpu.mem.wr8(a + 0x1D9, committed);
            cpu.mem.wr8(a + 0x1F1, knockdown);
            cpu.mem.wr8(a + 0x225, capture);
            cpu.mem.wr8(a + 0x1DA, 0x33);
            cpu.mem.wr8(a + 0x1DC, 0x22);
            seeded.push((
                state, color, blend, selector, committed, knockdown, capture, mode,
            ));
        }
        assert_eq!(cpu.call(SM, &[]), Stop::Returned, "{round}");
        for s in 0..8u32 {
            let a = actor(s);
            let (state, color, blend, selector, committed, knockdown, capture, mode) =
                seeded[s as usize];
            let (w, fx) = tint_sm_step(
                state,
                TintWords {
                    color,
                    blend,
                    selector,
                },
                FadeInputs {
                    party: s < 3,
                    committed_anim: committed,
                    knockdown_entry: knockdown,
                    captured: capture != 0,
                },
                dt,
            );
            faded += u32::from(state == 2);
            let want_state = if fx.party_fade_done { 0 } else { state };
            let want_mode = if fx.mode_semi_transparent {
                mode | 0x8100_0000
            } else {
                mode
            };
            let want = (w.color, w.blend, w.selector, want_state, want_mode);
            let got = (
                cpu.mem.rd32(a + 4),
                cpu.mem.rd32(a + 0xC),
                cpu.mem.rd8(a + 0x21F),
                cpu.mem.rd8(a + 0x21C),
                cpu.mem.rd32(a + 8),
            );
            if want != got && failures.len() < 16 {
                failures.push(format!(
                    "round {round} seat {s} state {state} dt {dt} colour {color:#x} blend {blend:#x}: \
                     retail {got:x?} port {want:x?}"
                ));
            }
            if fx.party_fade_done {
                assert_eq!(cpu.mem.rd8(a + 0x1DA), 0, "staged clears");
                assert_eq!(cpu.mem.rd8(a + 0x1DC), 1, "commit flags");
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(faded > 20_000, "{faded}");
}
