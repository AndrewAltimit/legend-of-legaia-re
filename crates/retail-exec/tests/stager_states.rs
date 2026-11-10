//! The slot-B stager arms that move battle state (rather than seat parts, which
//! `stager_spawns` holds) run off their module images against
//! `cast_module_ticks::*_stager`. Each run compares the whole context and all
//! eight actor records the arm could reach, read back through the kernels'
//! views (`cast_view`), so a store the kernel does not make fails the test.

use crate::cast_view::{diffs, read_actor, read_ctx, write_actor, write_ctx};
use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_cast::cast_module_ticks::{self as t, CastActorState};
use psx_retail_exec::{A0, A1, A2, Cpu, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const CTX: u32 = 0x8012_0000;
const CTX_LEN: usize = 0x1100;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;
const CALLER: u32 = 0x8014_0000;
const FADE: u32 = 0x8002_4E80;
const FADE_HANDLE: u32 = 0x8015_F000;
const SEAT: u32 = 0x8002_1B04;
const SEAT_POOLED: u32 = 0x8005_0ED4;
const POPUP: u32 = 0x801F_44A0;
const SUMMON_ROLL: u32 = 0x801D_D0AC;
const RESPECTING_ROLL: u32 = 0x801D_D4B0;
const INSTALL: u32 = 0x801F_19EC;

const WATER_CRYSTALS: u32 = 0x801F_75BC;
/// PROT 0949's seven ramp leaves, the stager's table arms 1..7.
const RAMP_1: u32 = 0x801F_7630;
const RAMP_2: u32 = 0x801F_7644;
const RAMP_3: u32 = 0x801F_7658;
const RAMP_4: u32 = 0x801F_766C;
const RAMP_5: u32 = 0x801F_7680;
const RAMP_6: u32 = 0x801F_7694;
const RAMP_7: u32 = 0x801F_76A8;
const PUERA: u32 = 0x801F_90E4;
const GILIUM: u32 = 0x801F_8B90;
const GIZAM: u32 = 0x801F_7740;
const VIGURO: u32 = 0x801F_7AF4;
const JUGGERNAUT: u32 = 0x801F_85A8;
const EVIL_SERU_MAGIC: u32 = 0x801F_8D64;

/// PROT 0909's word arm 0 parks the creature's displaced `+0x1DD` in
/// (`sw a0,-0x7CA4(v0)` at `0x801F7B94`).
const VIGURO_STASH: u32 = 0x801F_835C;
/// PROT 0927's shake word (`0x801F9C90`); arms 3 and 4 add to it.
const JUGGERNAUT_SHAKE: u32 = 0x801F_9C90;
/// PROT 0966's counterpart (`lw v1,-0x5B98(a2)` at `0x801F8E64`).
const ESM_SHAKE: u32 = 0x801F_A468;

struct Rig {
    cpu: Cpu,
    rng: Rng,
}

impl Rig {
    fn new(entry: usize, seed: u64) -> Option<(Self, Vec<u8>)> {
        let (mut cpu, disc) = (scus_cpu()?, disc()?);
        cpu.mem.load(OVERLAY_BASE, &disc.read_entry(898).ok()?);
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
        cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
        for va in [SEAT, SEAT_POOLED, INSTALL] {
            cpu.stub(va, 0x8016_0000);
        }
        cpu.stub(FADE, FADE_HANDLE);
        let module = disc.read_entry(entry).ok()?;
        Some((
            Self {
                cpu,
                rng: Rng(seed),
            },
            module,
        ))
    }

    /// Seed the context and the eight records with noise, the table with the
    /// records, and return the context and records as bytes.
    fn seed(&mut self, module: &[u8]) {
        let (cpu, rng) = (&mut self.cpu, &mut self.rng);
        cpu.mem.load(BASE, module);
        for b in 0..CTX_LEN as u32 {
            cpu.mem.wr8(CTX + b, rng.next() as u8);
        }
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE;
            cpu.mem.wr32(ACTORS + s * 4, a);
            for o in (0..STRIDE).step_by(2) {
                cpu.mem.wr16(a + o, (rng.next() % 0x1000) as u16);
            }
        }
    }

    fn ctx(&mut self) -> Vec<u8> {
        self.cpu.mem.read(CTX, CTX_LEN)
    }

    fn actors(&mut self) -> Vec<u8> {
        self.cpu.mem.read(ACTOR, (8 * STRIDE) as usize)
    }

    fn views(&mut self) -> Vec<CastActorState> {
        (0..8)
            .map(|s| read_actor(&mut self.cpu.mem, ACTOR + s * STRIDE))
            .collect()
    }
}

/// The record bytes with every view written back over them.
fn expected_actors(mut bytes: Vec<u8>, views: &[CastActorState]) -> Vec<u8> {
    for (s, v) in views.iter().enumerate() {
        let o = s * STRIDE as usize;
        write_actor(&mut bytes[o..o + STRIDE as usize], v);
    }
    bytes
}

#[test]
fn the_water_crystals_ramp_matches_retail() {
    differential!(
        prot(949, WATER_CRYSTALS),
        prot(949, RAMP_1),
        prot(949, RAMP_2),
        prot(949, RAMP_3),
        prot(949, RAMP_4),
        prot(949, RAMP_5),
        prot(949, RAMP_6),
        prot(949, RAMP_7)
    );
    let Some((mut rig, module)) = Rig::new(949, 0x5752_4D50) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let leaves = [RAMP_1, RAMP_2, RAMP_3, RAMP_4, RAMP_5, RAMP_6, RAMP_7];
    for round in 0..64 {
        for arm in 0..10u8 {
            // Through the dispatch: the victim is the caster's `+0x1DD` seat.
            rig.seed(&module);
            let caster = rig.rng.below(8);
            let victim = rig.rng.below(8);
            rig.cpu.mem.wr8(CTX + 0x13, caster as u8);
            rig.cpu
                .mem
                .wr8(ACTOR + caster * STRIDE + 0x1DD, victim as u8);
            let (ctx, before) = (rig.ctx(), rig.actors());
            let mut views = rig.views();
            assert_eq!(
                rig.cpu.call(WATER_CRYSTALS, &[CALLER, u32::from(arm)]),
                Stop::Returned
            );
            t::water_crystals_stager(&mut views[victim as usize], arm);
            let (got, want) = (rig.actors(), expected_actors(before, &views));
            assert!(
                got == want,
                "round {round} arm {arm}: {:?}",
                diffs(&got, &want)
            );
            assert!(rig.ctx() == ctx, "round {round} arm {arm}: ctx");

            // Each leaf on its own, as the table enters it.
            if let Some(&leaf) = leaves.get(usize::from(arm).wrapping_sub(1)) {
                rig.seed(&module);
                let before = rig.actors();
                let mut views = rig.views();
                let a = ACTOR + victim * STRIDE;
                assert_eq!(rig.cpu.call(leaf, &[a]), Stop::Returned);
                t::water_crystals_stager(&mut views[victim as usize], arm);
                let (got, want) = (rig.actors(), expected_actors(before, &views));
                assert!(got == want, "leaf {leaf:#x}: {:?}", diffs(&got, &want));
            }
        }
    }
}

#[test]
fn the_puera_and_gilium_stage_bytes_match_retail() {
    differential!(prot(922, PUERA), prot(923, GILIUM));
    for (entry, va) in [(922usize, PUERA), (923, GILIUM)] {
        let Some((mut rig, module)) = Rig::new(entry, 0x5055_4552) else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        for round in 0..64 {
            for arm in 0..6u8 {
                rig.seed(&module);
                // PROT 0923's arm 1 halts three parked handles.
                for (k, &h) in t::GILIUM_STAGER_HALTS.iter().enumerate() {
                    rig.cpu.mem.wr32(h, 0x8017_0000 + k as u32 * 0x100);
                    rig.cpu
                        .mem
                        .wr32(0x8017_0010 + k as u32 * 0x100, rig.rng.next() & !8);
                }
                let (mut ctx, actors) = (rig.ctx(), rig.actors());
                let mut view = read_ctx(&mut rig.cpu.mem, CTX);
                let flags: Vec<u32> = (0..3)
                    .map(|k| rig.cpu.mem.rd32(0x8017_0010 + k * 0x100))
                    .collect();
                assert_eq!(rig.cpu.call(va, &[CALLER, u32::from(arm)]), Stop::Returned);
                if entry == 922 {
                    t::puera_stager(&mut view, arm);
                } else {
                    t::gilium_stager(&mut view, arm);
                }
                write_ctx(&mut ctx, &view);
                let c = format!("PROT {entry:04} round {round} arm {arm}");
                let got = rig.ctx();
                assert!(got == ctx, "{c}: {:?}", diffs(&got, &ctx));
                assert!(rig.actors() == actors, "{c}: actors");
                for (k, f) in flags.iter().enumerate() {
                    let halted = entry == 923 && arm == 1;
                    let want = if halted { f | 8 } else { *f };
                    assert_eq!(
                        rig.cpu.mem.rd32(0x8017_0010 + k as u32 * 0x100),
                        want,
                        "{c}: handle {k}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_gizam_and_viguro_seat_poses_match_retail() {
    differential!(prot(906, GIZAM), prot(909, VIGURO));
    for (entry, va) in [(906usize, GIZAM), (909, VIGURO)] {
        let Some((mut rig, module)) = Rig::new(entry, 0x4749_5A4D) else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        for round in 0..64 {
            for arm in 0..3u8 {
                rig.seed(&module);
                let (mut ctx, before) = (rig.ctx(), rig.actors());
                let mut cview = read_ctx(&mut rig.cpu.mem, CTX);
                let mut views = rig.views();
                let stash_before = rig.cpu.mem.rd32(VIGURO_STASH);
                assert_eq!(
                    rig.cpu.call(va, &[CALLER, u32::from(arm), 0]),
                    Stop::Returned
                );
                let mut stashed = None;
                if entry == 906 {
                    t::gizam_stager(&mut cview, &mut views[7], arm);
                } else {
                    stashed = t::viguro_stager(&mut cview, &mut views[7], arm);
                }
                write_ctx(&mut ctx, &cview);
                let mut want = expected_actors(before, &views);
                let c = format!("PROT {entry:04} round {round} arm {arm}");
                if entry == 909 && arm == 0 {
                    // The pose stores: the host stands the seat itself
                    // (`run_viguro_stager_calls`, `VIGURO_SEAT_Z`).
                    let o = 7 * STRIDE as usize;
                    for (off, v) in [(0x34usize, 0u16), (0x38, 0xC00), (0x46, 0x800)] {
                        want[o + off..o + off + 2].copy_from_slice(&v.to_le_bytes());
                    }
                    assert_eq!(
                        Some(rig.cpu.mem.rd32(VIGURO_STASH) as u8),
                        stashed,
                        "{c}: the stashed +0x1DD"
                    );
                } else if entry == 909 {
                    assert_eq!(rig.cpu.mem.rd32(VIGURO_STASH), stash_before, "{c}");
                }
                if entry == 909 && arm == 1 {
                    // The held fade's handle, which the host parks
                    // (`push_stager_fade` at `HandleSlot::Ctx(0x102C)`).
                    ctx[0x102C..0x1030].copy_from_slice(&FADE_HANDLE.to_le_bytes());
                }
                let got = rig.actors();
                assert!(got == want, "{c}: {:?}", diffs(&got, &want));
                let got = rig.ctx();
                assert!(got == ctx, "{c}: ctx {:?}", diffs(&got, &ctx));
            }
        }
    }
}

/// One wrapper roll a sweep draws, and the popup it pushes.
#[derive(Default)]
struct Sweep {
    rolls: Vec<(u8, i32)>,
    popups: Vec<(i16, u8)>,
}

#[test]
fn the_juggernaut_and_esm_sweeps_match_retail() {
    differential!(prot(927, JUGGERNAUT), prot(966, EVIL_SERU_MAGIC));
    for (entry, va, roll_va, shake) in [
        (927usize, JUGGERNAUT, SUMMON_ROLL, JUGGERNAUT_SHAKE),
        (966, EVIL_SERU_MAGIC, RESPECTING_ROLL, ESM_SHAKE),
    ] {
        let Some((mut rig, module)) = Rig::new(entry, 0x4A55_4747) else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        let log = Rc::new(RefCell::new(Sweep::default()));
        let script = Rc::new(RefCell::new(Vec::<i32>::new()));
        {
            let (log, script) = (log.clone(), script.clone());
            rig.cpu.hook(roll_va, move |r, _| {
                let v = script.borrow_mut().remove(0);
                log.borrow_mut().rolls.push((r.r[A2] as u8, v));
                r.r[V0] = v as u32;
            });
        }
        {
            let log = log.clone();
            rig.cpu.hook(POPUP, move |r, _| {
                log.borrow_mut()
                    .popups
                    .push((r.r[A0] as i16, r.r[A1] as u8));
            });
        }
        let mut landed = 0;
        for round in 0..200 {
            for arm in [3u8, 4] {
                rig.seed(&module);
                let (party, monsters) = (1 + rig.rng.below(3) as u8, 1 + rig.rng.below(4) as u8);
                rig.cpu.mem.wr8(CTX, party);
                rig.cpu.mem.wr8(CTX + 1, monsters);
                for s in 0..8u32 {
                    let a = ACTOR + s * STRIDE;
                    let hp = [0, 1, 2, 50, 999][rig.rng.below(5) as usize];
                    rig.cpu.mem.wr16(a + 0x14C, hp);
                    let flags = if rig.rng.below(4) == 0 { 4 } else { 0 };
                    rig.cpu.mem.wr16(a + 0x16E, flags);
                }
                let shake0 = rig.rng.below(0x40);
                rig.cpu.mem.wr32(shake, shake0);
                *script.borrow_mut() = (0..8).map(|_| rig.rng.below(1200) as i32 - 100).collect();
                let rolls = script.borrow().clone();
                *log.borrow_mut() = Sweep::default();
                let (ctx, before) = (rig.ctx(), rig.actors());
                let cview = read_ctx(&mut rig.cpu.mem, CTX);
                let mut views = rig.views();
                assert_eq!(rig.cpu.call(va, &[CALLER, u32::from(arm)]), Stop::Returned);

                let mut k = 0;
                let mut port_rolls = Vec::new();
                let hits = {
                    let take = |seat: u8| {
                        let v = rolls[k];
                        k += 1;
                        port_rolls.push((seat, v));
                        v
                    };
                    if entry == 927 {
                        t::juggernaut_stager(&cview, &mut views, arm, take)
                    } else {
                        t::evil_seru_magic_stager(&cview, &mut views, arm, take)
                    }
                };
                let c = format!("PROT {entry:04} round {round} arm {arm}");
                let l = log.borrow();
                assert_eq!(l.rolls, port_rolls, "{c}: rolls");
                // The popup is pushed with the roll, ahead of the clamp.
                let pops: Vec<(i16, u8)> = l.rolls.iter().map(|&(s, v)| (v as i16, s)).collect();
                assert_eq!(l.popups, pops, "{c}: popups");
                assert_eq!(hits.len(), l.rolls.len(), "{c}");
                let want = expected_actors(before, &views);
                let got = rig.actors();
                assert!(got == want, "{c}: {:?}", diffs(&got, &want));
                let got = rig.ctx();
                assert!(got == ctx, "{c}: ctx {:?}", diffs(&got, &ctx));
                let shake_want = shake0 as i32 + t::juggernaut_stager_shake(arm);
                assert_eq!(rig.cpu.mem.rd32(shake) as i32, shake_want, "{c}: shake");
                landed += hits.len();
            }
        }
        assert!(landed > 100, "PROT {entry:04}: {landed} hits");
    }
}

#[test]
fn the_juggernaut_caller_jitter_matches_retail() {
    differential!(prot(927, JUGGERNAUT));
    let Some((mut rig, module)) = Rig::new(927, 0x4A49_5454) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let stream = Rc::new(std::cell::Cell::new(1u32));
    {
        let stream = stream.clone();
        rig.cpu.hook(0x8005_6798, move |r, _| {
            let n = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
            stream.set(n);
            r.r[V0] = (n >> 16) & 0x7FFF;
        });
    }
    let (mut odd, mut even) = (0, 0);
    for round in 0..256 {
        rig.seed(&module);
        for o in (0..0x100u32).step_by(4) {
            let w = rig.rng.next();
            rig.cpu.mem.wr32(CALLER + o, w);
        }
        let arg = rig.rng.next() as i16;
        let seed = rig.rng.next();
        stream.set(seed);
        let (ctx, actors) = (rig.ctx(), rig.actors());
        let mut want = rig.cpu.mem.read(CALLER, 0x100);
        assert_eq!(
            rig.cpu.call(
                JUGGERNAUT,
                &[
                    CALLER,
                    u32::from(t::JUGGERNAUT_STAGER_JITTER_ARM),
                    arg as i32 as u32
                ]
            ),
            Stop::Returned
        );
        let mut n = seed;
        let mut draws = 0;
        let (v84, z) = t::juggernaut_stager_jitter(arg, || {
            n = n.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
            draws += 1;
            (n >> 16) & 0x7FFF
        });
        want[0x84..0x86].copy_from_slice(&v84.to_le_bytes());
        want[0x28..0x2A].copy_from_slice(&z.to_le_bytes());
        let c = format!("round {round} arg {arg}");
        let got = rig.cpu.mem.read(CALLER, 0x100);
        assert!(got == want, "{c}: {:?}", diffs(&got, &want));
        assert_eq!(stream.get(), n, "{c}: draws ({draws})");
        assert!(rig.ctx() == ctx, "{c}: ctx");
        assert!(rig.actors() == actors, "{c}: actors");
        if v84 > 0 { odd += 1 } else { even += 1 }
    }
    assert!(odd > 50 && even > 50, "{odd} up, {even} down");
}
