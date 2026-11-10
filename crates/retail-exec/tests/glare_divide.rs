//! PROT 0940's split (`FUN_801F78B8`, actions `0x50` / `0xAE`) run through its
//! arm 2 off the disc against `glare_divide_split_tick`: the `rand()` draws it
//! spends, the clone's HP and the stats of whichever half the coin weakens.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_cast::cast_arm_ticks::{CastArmExtState, SplitWeakened, glare_divide_split_tick};
use legaia_engine_cast::cast_module_ticks::{CastActorState, CastModuleCtx};
use psx_retail_exec::{Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const TICK: u32 = 0x801F_78B8;
const MODULE_BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const RAND: u32 = 0x8005_6798;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;
const REC: u32 = 0x8015_0000;
const OBJ: u32 = 0x8016_0000;

fn lcg(s: u32) -> u32 {
    s.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039)
}

#[test]
fn the_split_weakens_the_half_retail_weakens() {
    differential!(prot(940, TICK));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let module = disc.read_entry(940).expect("PROT 0940");
    let stream = Rc::new(Cell::new(0u32));
    let draws = Rc::new(Cell::new(0u32));
    {
        let (stream, draws) = (stream.clone(), draws.clone());
        cpu.hook(RAND, move |r, _| {
            let next = lcg(stream.get());
            stream.set(next);
            draws.set(draws.get() + 1);
            r.r[V0] = (next >> 16) & 0x7FFF;
        });
    }
    for va in [
        0x8004_FCC8,
        0x801D_829C,
        0x8005_4CB0,
        0x8003_CA38,
        0x8003_CA78,
    ] {
        cpu.stub(va, 0);
    }
    cpu.stub(0x8002_4C88, OBJ);
    // The sine / cosine table pointers as `FUN_80026BE0` installs them.
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x4449_5649);
    let (mut plain, mut caster_weak, mut clone_weak) = (0, 0, 0);
    for round in 0..400 {
        cpu.mem.load(MODULE_BASE, &module);
        for s in 0..8 {
            let a = ACTOR + s * STRIDE;
            cpu.mem.wr32(ACTORS + s * 4, a);
            cpu.mem.wr32(a + 0x22C, REC + s * 0x100);
            for o in (0x14C..0x160).step_by(2) {
                cpu.mem.wr16(a + o, 0);
            }
        }
        let caster_slot = 3 + rng.below(2);
        let count = rng.below(3) as u8 + (caster_slot - 2) as u8;
        let action = if rng.below(2) == 0 { 0x50u8 } else { 0xAE };
        let a = ACTOR + caster_slot * STRIDE;
        let hp = 1 + rng.below(9000) as u16;
        let (mp, atk, agl) = (
            rng.below(500) as u16,
            1 + rng.below(400) as u16,
            rng.below(0x200) as u16,
        );
        cpu.mem.wr16(a + 0x14C, hp);
        cpu.mem.wr16(a + 0x150, mp);
        cpu.mem.wr16(a + 0x158, atk);
        cpu.mem.wr16(a + 0x154, agl);
        cpu.mem.wr16(a + 0x156, agl);
        cpu.mem.wr8(a + 0x1DF, action);
        cpu.mem.wr8(a + 0x1DD, 1);
        cpu.mem.wr8(CTX + 0x1, count);
        cpu.mem.wr8(CTX + 0x13, caster_slot as u8);
        cpu.mem.wr8(CTX + 0x279, 2);
        // The arm's countdown, run down already.
        cpu.mem.wr32(0x801F_864C, 0);
        let seed = rng.next();
        stream.set(seed);
        draws.set(0);
        let mut calls = 0;
        while cpu.mem.rd8(CTX + 0x279) == 2 {
            let stop = cpu.call(TICK, &[]);
            assert_eq!(stop, Stop::Returned, "round {round}");
            calls += 1;
            assert!(calls < 400, "round {round}: arm 2 never advanced");
        }
        let clone = cpu.mem.rd32(ACTORS + (3 + u32::from(count)) * 4);
        let rd = |cpu: &mut psx_retail_exec::Cpu, p: u32| {
            [
                cpu.mem.rd16(p + 0x14C),
                cpu.mem.rd16(p + 0x150),
                cpu.mem.rd16(p + 0x158),
                cpu.mem.rd16(p + 0x154),
                cpu.mem.rd16(p + 0x156),
            ]
        };
        let retail_caster = rd(&mut cpu, a);
        let retail_clone = rd(&mut cpu, clone);

        // The port: the kernel on the caster's view, the World's clone rule.
        let mut ctx = CastModuleCtx {
            phase: 2,
            monster_count: count,
            ..CastModuleCtx::default()
        };
        let mut caster = CastActorState {
            hp,
            atk,
            agl,
            agl_base: agl,
            target_code: 1,
            ..CastActorState::default()
        };
        let mut s = seed;
        let mut n = 0u32;
        let (_, split) = glare_divide_split_tick(
            &mut ctx,
            &mut caster,
            &CastArmExtState::default(),
            action,
            0,
            || {
                n += 1;
                s = lcg(s);
                (s >> 16) & 0x7FFF
            },
        );
        let split = split.expect("arm 2 splits");
        let mut port_caster = [caster.hp, mp, caster.atk, caster.agl, caster.agl_base];
        let mut port_clone = [split.clone_hp, mp, atk, agl, agl];
        match split.weakened {
            Some(SplitWeakened::Caster) => {
                port_caster[1] = 0;
                caster_weak += 1;
            }
            Some(SplitWeakened::Clone) => {
                port_clone = [1, 0, 1, 0x20, 0x20];
                clone_weak += 1;
            }
            None => plain += 1,
        }
        let ctx = format!("round {round} action {action:#x} {:?}", split.weakened);
        assert_eq!(draws.get(), n, "draws: {ctx}");
        assert_eq!(retail_caster, port_caster, "caster: {ctx}");
        // The clone's record copy is the World's; its HP and the weakening are
        // the arm's.
        assert_eq!(retail_clone[0], port_clone[0], "clone hp: {ctx}");
        if split.weakened == Some(SplitWeakened::Clone) {
            assert_eq!(retail_clone, port_clone, "clone: {ctx}");
        }
        assert_eq!(
            cpu.mem.rd16(clone + 0x172),
            split.clone_hp,
            "clone max: {ctx}"
        );
    }
    assert!(
        plain > 50 && caster_weak > 30 && clone_weak > 30,
        "{plain} {caster_weak} {clone_weak}"
    );
}
