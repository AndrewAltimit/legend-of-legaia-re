//! PROT 0943's single-target Curse body `0x801F6EF4` (action `0x40`) run off
//! its image from arm 0 to its Done, pass by pass, against
//! `cast_arm_ticks::curse_single_tick` under the body's countdown table
//! (`capture_arm_countdowns(943, CURSE_SINGLE_TICK)`).
//!
//! Every call out of the image returns `0`. Each pass compares the countdown
//! word `0x801F7A04` with the table's, and a hold with a phase the body left
//! alone. A pass that leaves its arm compares the whole context and the
//! caster's and victim's records with the port's views written back, less the
//! presentation word `+0x04` the kernel does not carry (and the fade handle
//! `ctx[+0x102C]`), and every other seat's
//! record whole but for the hide / show bytes `+0x04` / `+0x21C`, which the
//! host's seat pass owns.

use crate::cast_view::{diffs, read_actor, read_ctx, write_actor, write_ctx};
use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_cast::cast_arm_ticks::{self as arms, CURSE_SINGLE_TICK};

/// The body, as `cast_arm_ticks::CURSE_SINGLE_TICK` names it.
const CURSE: u32 = 0x801F_6EF4;
use legaia_engine_cast::cast_module_camera::{
    self as cam, ModuleCountdown, capture_arm_countdowns,
};
use legaia_engine_cast::cast_module_ticks::CastTickStep;
use psx_retail_exec::{Stop, V0};

const BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const CTX: u32 = 0x8012_0000;
const CTX_LEN: usize = 0x1100;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: usize = 0x400;
const NODES: u32 = 0x8014_0000;
const SCALAR: u32 = 0x1F80_037D;
const STEP: u32 = 0x1F80_0393;
const ROSTER: u32 = 0x8007_BD10;
/// PROT 0943's countdown word (`sw v0,0x7a04(a0)` at `0x801F7054`).
const COUNTDOWN: u32 = 0x801F_7A04;

fn jal_targets(module: &[u8]) -> Vec<u32> {
    let mut out: Vec<u32> = module
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .filter(|w| w >> 26 == 3)
        .map(|w| ((w & 0x03FF_FFFF) << 2) | 0x8000_0000)
        .filter(|&t| !(BASE..BASE + 0x8000).contains(&t))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

#[test]
fn the_single_curse_walks_and_marks_as_retail_does() {
    differential!(prot(943, CURSE));
    assert_eq!(CURSE, CURSE_SINGLE_TICK);
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let module = disc.read_entry(943).expect("PROT 0943");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    for t in jal_targets(&module) {
        cpu.stub(t, 0);
    }
    let table = capture_arm_countdowns(943, CURSE_SINGLE_TICK).expect("the curse table");
    let mut rng = Rng(0x4355_5253);
    let mut failures = Vec::new();
    let (mut casts, mut marks, mut holds) = (0, 0, 0);
    for round in 0..400 {
        cpu.mem.load(BASE, &module);
        for b in 0..CTX_LEN as u32 {
            cpu.mem.wr8(CTX + b, rng.next() as u8);
        }
        let party = 1 + rng.below(3) as u8;
        let monsters = 1 + rng.below(5) as u8;
        let caster = 3 + rng.below(u32::from(monsters));
        let victim = rng.below(u32::from(party));
        cpu.mem.wr8(CTX, party);
        cpu.mem.wr8(CTX + 1, monsters);
        cpu.mem.wr8(CTX + 0x13, caster as u8);
        cpu.mem.wr8(CTX + 0x279, 0);
        for i in 0..3 {
            cpu.mem.wr8(ROSTER + i, u8::from(i < u32::from(party)));
        }
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE as u32;
            cpu.mem.wr32(ACTORS + s * 4, a);
            for o in (0..STRIDE as u32).step_by(2) {
                cpu.mem.wr16(a + o, rng.next() as u16);
            }
            let hp = if s == caster || rng.below(3) != 0 {
                1 + rng.below(999) as u16
            } else {
                0
            };
            cpu.mem.wr16(a + 0x14C, hp);
            cpu.mem.wr32(a + 0x22C, NODES + s * 0x100);
        }
        cpu.mem
            .wr8(ACTOR + caster * STRIDE as u32 + 0x1DD, victim as u8);
        cpu.mem.wr8(SCALAR, cam::SPEED_SCALAR as u8);
        cpu.mem.wr8(STEP, 1);
        let (ca, va) = (
            ACTOR + caster * STRIDE as u32,
            ACTOR + victim * STRIDE as u32,
        );
        let mut cd = ModuleCountdown::default();
        let mut done = false;
        for pass in 0..2_000 {
            let c = format!("round {round} pass {pass} caster {caster} victim {victim}");
            let mut ctx = read_ctx(&mut cpu.mem, CTX);
            let phase = ctx.phase;
            let mut k = read_actor(&mut cpu.mem, ca);
            let mut v = read_actor(&mut cpu.mem, va);
            let ctx_before = cpu.mem.read(CTX, CTX_LEN);
            let recs_before = cpu.mem.read(ACTOR, 8 * STRIDE);
            let gate = arms_gate(table, phase, &mut cd);
            assert_eq!(cpu.call(CURSE, &[]), Stop::Returned, "{c}");
            let busy = cpu.regs.r[V0] != 0;
            let after = cpu.mem.rd8(CTX + 0x279);
            let word = cpu.mem.rd32(COUNTDOWN) as i32;
            if gate {
                holds += 1;
                if !busy || after != phase {
                    failures.push(format!("{c}: port holds arm {phase}, retail moved"));
                    break;
                }
                if word != cd.0 {
                    failures.push(format!("{c}: held countdown retail {word} port {}", cd.0));
                    break;
                }
                continue;
            }
            if busy && after == phase {
                failures.push(format!("{c}: retail holds arm {phase}, port passes"));
                break;
            }
            let step = arms::curse_single_tick(&mut ctx, &mut k, &mut v);
            if let Some(a) = cam::arm_countdown(table, phase) {
                a.pass(&mut cd);
            }
            if (step == CastTickStep::Done) == busy {
                failures.push(format!("{c}: arm {phase} busy retail {busy} port {step:?}"));
                break;
            }
            if word != cd.0 {
                failures.push(format!(
                    "{c}: arm {phase} countdown retail {word} port {}",
                    cd.0
                ));
                cd.0 = word;
            }
            let mut want_ctx = ctx_before.clone();
            write_ctx(&mut want_ctx, &ctx);
            let mut got_ctx = cpu.mem.read(CTX, CTX_LEN);
            // Arm 1 parks its fade's handle at `ctx[+0x102C]` (`0x801F727C`),
            // which the host's presentation hook keeps.
            got_ctx[0x102C..0x1030].copy_from_slice(&want_ctx[0x102C..0x1030]);
            if got_ctx != want_ctx {
                failures.push(format!(
                    "{c}: arm {phase} ctx {:?}",
                    diffs(&got_ctx, &want_ctx)
                ));
            }
            let mut want = recs_before.clone();
            let got = cpu.mem.read(ACTOR, 8 * STRIDE);
            for (seat, view) in [(caster, &k), (victim, &v)] {
                let o = seat as usize * STRIDE;
                write_actor(&mut want[o..o + STRIDE], view);
            }
            for seat in 0..8usize {
                let o = seat * STRIDE;
                let mut mask = vec![0x04, 0x05, 0x06, 0x07];
                if seat as u32 != caster && seat as u32 != victim {
                    mask.push(0x21C);
                }
                let (mut g, w) = (got[o..o + STRIDE].to_vec(), &want[o..o + STRIDE]);
                for m in mask {
                    g[m] = w[m];
                }
                if g != w {
                    failures.push(format!("{c}: arm {phase} seat {seat} {:?}", diffs(&g, w)));
                }
            }
            if phase == 3 {
                marks += 1;
            }
            if !busy {
                done = true;
                break;
            }
            if failures.len() > 20 {
                break;
            }
        }
        if !done {
            failures.push(format!("round {round}: no Done"));
        }
        casts += 1;
        if failures.len() > 20 {
            break;
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        casts == 400 && marks == 400 && holds > 10_000,
        "{casts} {marks} {holds}"
    );
}

/// The table's gate for `phase`: whether the arm holds this pass.
fn arms_gate(table: &[cam::ArmCountdown], phase: u8, cd: &mut ModuleCountdown) -> bool {
    cam::arm_countdown(table, phase).is_some_and(|a| a.holds(cd))
}
