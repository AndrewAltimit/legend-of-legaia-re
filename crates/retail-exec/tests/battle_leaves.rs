//! The action overlay's small pool kernels against theirs in
//! `battle_action::pool_ops`: the formation squash and recentres
//! `FUN_801DB318` / `FUN_801EFE44`, the commandable-seat scans `FUN_801DBA04` /
//! `FUN_801DB81C`, the flag-word scrub `FUN_801DB9C4`, the target ring
//! `FUN_801D8D00`, and the Seru list's prepend `FUN_801E92DC`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_action::{
    FormationPos, PoolActor, clear_pool_flag_words, first_selectable_target, next_selectable_actor,
    normalize_formation_span, seed_formation_recentre,
};
use psx_retail_exec::{Cpu, Stop, V0};

const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const FOCUS_X: u32 = 0x8008_9118;
const FOCUS_Z: u32 = 0x8008_9120;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;

fn rig() -> Option<Cpu> {
    let (mut cpu, disc) = (scus_cpu()?, disc()?);
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * STRIDE);
    }
    Some(cpu)
}

fn gen_pos(rng: &mut Rng, wide: bool) -> (Vec<FormationPos>, Vec<bool>) {
    let range = if wide { 0x1800 } else { 0x400 };
    let pos = (0..7)
        .map(|_| FormationPos {
            x: (rng.below(range * 2) as i32 - range as i32) as i16,
            z: (rng.below(range * 2) as i32 - range as i32) as i16,
        })
        .collect();
    let alive = (0..7).map(|_| rng.below(3) != 0).collect();
    (pos, alive)
}

fn seat(cpu: &mut Cpu, pos: &[FormationPos], alive: &[bool], fx: i32, fz: i32) {
    for s in 0..7u32 {
        let a = ACTOR + s * STRIDE;
        cpu.mem.wr16(a + 0x34, pos[s as usize].x as u16);
        cpu.mem.wr16(a + 0x38, pos[s as usize].z as u16);
        cpu.mem.wr16(a + 0x14C, u16::from(alive[s as usize]));
    }
    cpu.mem.wr32(FOCUS_X, fx as u32);
    cpu.mem.wr32(FOCUS_Z, fz as u32);
}

fn read_back(cpu: &mut Cpu) -> (Vec<FormationPos>, i32, i32) {
    let pos = (0..7u32)
        .map(|s| {
            let a = ACTOR + s * STRIDE;
            FormationPos {
                x: cpu.mem.rd16(a + 0x34) as i16,
                z: cpu.mem.rd16(a + 0x38) as i16,
            }
        })
        .collect();
    (
        pos,
        cpu.mem.rd32(FOCUS_X) as i32,
        cpu.mem.rd32(FOCUS_Z) as i32,
    )
}

#[test]
fn the_round_squash_matches() {
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x464F_524D);
    for round in 0..20000 {
        let (mut pos, alive) = gen_pos(&mut rng, round % 2 == 0);
        let (mut fx, mut fz) = (rng.below(0x2000) as i32 - 0x1000, rng.below(0x2000) as i32);
        seat(&mut cpu, &pos, &alive, fx, fz);
        let seed = pos.clone();
        assert_eq!(cpu.call(0x801D_B318, &[]), Stop::Returned);
        let retail = read_back(&mut cpu);
        normalize_formation_span(&mut pos, &alive, &mut fx, &mut fz);
        assert_eq!(retail, (pos, fx, fz), "round {round}: {seed:?} {alive:?}");
    }
}

#[test]
fn the_seed_recentre_matches() {
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5345_4544);
    for round in 0..20000 {
        let (mut pos, alive) = gen_pos(&mut rng, round % 2 == 0);
        let (mut fx, mut fz) = (rng.below(0x2000) as i32 - 0x1000, rng.below(0x2000) as i32);
        let actor = rng.below(7);
        let attack = rng.below(2) == 0;
        let target = rng.below(10) as u8;
        seat(&mut cpu, &pos, &alive, fx, fz);
        cpu.mem.wr8(CTX + 0x13, actor as u8);
        let a = ACTOR + actor * STRIDE;
        cpu.mem.wr8(a + 0x1DD, target);
        cpu.mem.wr8(a + 0x1DE, if attack { 3 } else { 2 });
        let seed = pos.clone();
        assert_eq!(cpu.call(0x801E_FE44, &[]), Stop::Returned);
        let retail = read_back(&mut cpu);
        seed_formation_recentre(&mut pos, &alive, attack, target, &mut fx, &mut fz);
        assert_eq!(
            retail,
            (pos, fx, fz),
            "round {round}: {seed:?} {alive:?} attack {attack} target {target}"
        );
    }
}

#[test]
fn the_selectable_scans_match() {
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5343_414E);
    for round in 0..20000 {
        let count = rng.below(4) as u8;
        let mut pool = Vec::new();
        let mut ids = Vec::new();
        for s in 0..4u32 {
            let id = 1 + rng.below(4) as u8;
            let status = if rng.below(3) == 0 {
                1u16 << rng.below(16)
            } else {
                0
            };
            let alive = rng.below(3) != 0;
            let a = ACTOR + s * STRIDE;
            cpu.mem.wr8(SEATS + s, id);
            cpu.mem.wr16(a + 0x14C, u16::from(alive));
            cpu.mem.wr16(a + 0x16E, status);
            pool.push(PoolActor {
                alive,
                status,
                ..PoolActor::default()
            });
            ids.push(id);
        }
        let cursor = rng.below(4) as u8;
        cpu.mem.wr8(CTX, count);
        cpu.mem.wr8(CTX + 0x13, cursor);
        assert_eq!(cpu.call(0x801D_BA04, &[]), Stop::Returned);
        assert_eq!(
            cpu.regs.r[V0] as u8,
            first_selectable_target(&pool, &ids, count),
            "first: round {round}"
        );
        assert_eq!(cpu.call(0x801D_B81C, &[]), Stop::Returned);
        assert_eq!(
            cpu.regs.r[V0] as u8,
            next_selectable_actor(&pool, &ids, count, cursor),
            "next: round {round}"
        );
    }
}

#[test]
fn the_flag_scrub_matches() {
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x464C_4147);
    let mut words: Vec<u32> = (0..8).map(|_| rng.next()).collect();
    for s in 0..8u32 {
        cpu.mem.wr32(ACTOR + s * STRIDE + 8, words[s as usize]);
    }
    assert_eq!(cpu.call(0x801D_B9C4, &[]), Stop::Returned);
    clear_pool_flag_words(&mut words);
    for s in 0..8u32 {
        assert_eq!(cpu.mem.rd32(ACTOR + s * STRIDE + 8), words[s as usize]);
    }
}

/// The attack ring's builder `FUN_801D8A88` against `build_attack_target_queue`,
/// then both arms of its step `FUN_801D8D00` over the ring retail built. The
/// seats crowd one another so ties and coincident seats come up; a sweep that
/// finds nothing leaves its context byte stale in retail, where the port
/// writes `0`.
#[test]
fn the_attack_ring_matches() {
    use legaia_engine_battle_vm::battle_action::{
        AttackTargetQueue, TargetCycle, bearing_12bit_approx, build_attack_target_queue,
        cycle_attack_target,
    };
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5249_4E47);
    let (mut full, mut short, mut stepped) = (0, 0, 0);
    for round in 0..30000 {
        let wide = rng.below(2) == 0;
        let (pos, mut alive) = gen_pos(&mut rng, wide);
        // Crowd a few seats onto one spot so the angle ties.
        if rng.below(4) == 0 {
            let s = 3 + rng.below(4) as usize;
            let t = 3 + rng.below(4) as usize;
            let mut p = pos.clone();
            p[s] = p[t];
            seat(&mut cpu, &p, &alive, 0, 0);
        }
        let acting = rng.below(3) as usize;
        alive[acting] = true;
        let target = 3 + rng.below(4) as u8;
        let mut posv = pos.clone();
        if rng.below(4) == 0 {
            let s = 3 + rng.below(4) as usize;
            let t = 3 + rng.below(4) as usize;
            posv[s] = posv[t];
        }
        seat(&mut cpu, &posv, &alive, 0, 0);
        cpu.mem.wr8(ACTOR + acting as u32 * STRIDE + 0x1DD, target);
        cpu.mem.wr8(CTX + 0x13, acting as u8);
        let stale = [
            3 + rng.below(4) as u8,
            3 + rng.below(4) as u8,
            3 + rng.below(4) as u8,
        ];
        for (i, b) in stale.iter().enumerate() {
            cpu.mem.wr8(CTX + 0x246 + i as u32, *b);
        }
        assert_eq!(cpu.call(0x801D_8A88, &[]), Stop::Returned);
        let retail: Vec<u8> = (0..5).map(|i| cpu.mem.rd8(CTX + 0x244 + i)).collect();

        let mut pool = [PoolActor::default(); 8];
        for s in 0..7 {
            pool[s] = PoolActor {
                alive: alive[s],
                x: posv[s].x,
                z: posv[s].z,
                target_slot: 0,
                status: 0,
            };
        }
        pool[acting].target_slot = target;
        let AttackTargetQueue {
            count,
            wrap_slot,
            ordered,
        } = build_attack_target_queue(&pool, acting, bearing_12bit_approx);
        let ctx = format!("round {round} acting {acting} target {target} alive {alive:?}");
        assert_eq!(retail[0], count, "count: {ctx}");
        assert_eq!(retail[1], wrap_slot, "wrap: {ctx}");
        for i in 0..3 {
            let want = if ordered[i] == 0 {
                stale[i]
            } else {
                ordered[i]
            };
            assert_eq!(retail[2 + i], want, "ordered[{i}]: {ctx} retail {retail:?}");
        }
        if ordered.iter().all(|&o| o != 0) {
            full += 1;
        } else {
            short += 1;
        }
        // The step, over the ring as retail left it.
        for (arg, mode) in [(0, TargetCycle::Next), (1, TargetCycle::Prev)] {
            if count == 0 {
                continue;
            }
            assert_eq!(cpu.call(0x801D_8D00, &[arg]), Stop::Returned);
            assert_eq!(
                cpu.regs.r[V0] as u8,
                cycle_attack_target(&retail, target, mode),
                "step {mode:?}: {ctx} ring {retail:?}"
            );
            stepped += 1;
        }
    }
    assert!(
        full > 1000 && short > 1000 && stepped > 20000,
        "{full} {short} {stepped}"
    );
}
