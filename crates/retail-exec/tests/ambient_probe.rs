//! The player box the ambient motion VM steps against, run on its own: the
//! actor-vs-player probe `FUN_801CF8AC` (PROT 0897) as the directional steps
//! call it, one compass point of `DAT_801F2254` per heading index
//! (`0x80038498..0x800384C0`), and the wander fan `FUN_801D5A68`, three points
//! of `DAT_801F21B4` or'd, against `AmbientPlayerProbe`'s `step_blocked` and
//! `wander_blocked`. Walkers carry a placement's class bit (`+0x10 & 0x20000`
//! or the party-bank `0x01000000`) and random collision-exempt bits; positions
//! cover the whole halfword plane and the box's edges.
//!
//! Both callers test the result's bit `1` alone, so that is what is compared.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_core::world::AmbientPlayerProbe;
use legaia_engine_motion_vm::ambient_motion::AmbientBlocking;
use psx_retail_exec::{Cpu, Stop, V0};

const PROBE: u32 = 0x801C_F8AC;
const WANDER: u32 = 0x801D_5A68;
const OVERLAY_BASE: u32 = 0x801C_E818;
const FACING_TABLE: u32 = 0x801F_2254;
const PLAYER_PTR: u32 = 0x8007_C364;
const PLAYER: u32 = 0x8013_0000;
const ACTOR: u32 = 0x8013_1000;

fn retail_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(897).expect("PROT 0897"));
    Some(cpu)
}

#[test]
fn the_ambient_player_probes_match_retail() {
    differential!(prot(897, PROBE), prot(897, WANDER));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    let mut rng = Rng(0x414D_4250);
    let (mut step_hits, mut wander_hits, mut exempt_runs, mut misses) = (0, 0, 0, 0);
    for run in 0..60_000 {
        let (px, pz) = (rng.next() as i16, rng.next() as i16);
        // Near the player most of the time, across the plane the rest.
        let (x, z) = if rng.below(4) == 0 {
            (rng.next() as i16, rng.next() as i16)
        } else {
            let r = [48, 80, 128][rng.below(3) as usize];
            (
                px.wrapping_add((rng.below(2 * r) as i32 - r as i32) as i16),
                pz.wrapping_add((rng.below(2 * r) as i32 - r as i32) as i16),
            )
        };
        // The MAN spawner ORs `0x20000` into every placement (`0x8003A3A8`),
        // the party-bank bit beside it for a special model.
        let class = [0x0002_0000u32, 0x0102_0000][rng.below(2) as usize];
        let exempt_bits = if rng.below(6) == 0 {
            1 + rng.below(3)
        } else {
            0
        };
        let flags = rng.next() & !0x0102_0003 | class | exempt_bits;
        cpu.mem.wr16(PLAYER + 0x14, px as u16);
        cpu.mem.wr16(PLAYER + 0x18, pz as u16);
        cpu.mem.wr32(PLAYER + 0x98, 0);
        cpu.mem.wr32(ACTOR + 0x10, flags);
        cpu.mem.wr16(ACTOR + 0x14, x as u16);
        cpu.mem.wr16(ACTOR + 0x18, z as u16);
        let port = AmbientPlayerProbe {
            player: Some((px, pz)),
            exempt: flags & 3 != 0,
        };
        exempt_runs += u32::from(port.exempt);
        let label = format!("run {run}: player ({px}, {pz}) actor ({x}, {z}) flags {flags:#010x}");

        let lut = rng.below(8) as u8;
        let dx = cpu.mem.rd16(FACING_TABLE + u32::from(lut) * 4);
        let dz = cpu.mem.rd16(FACING_TABLE + u32::from(lut) * 4 + 2);
        cpu.regs.r[V0] = 0xDEAD_BEEF;
        assert_eq!(
            cpu.call(
                PROBE,
                &[
                    ACTOR,
                    i32::from(dx as i16) as u32,
                    i32::from(dz as i16) as u32
                ]
            ),
            Stop::Returned
        );
        let retail = cpu.regs.r[V0] & 1 != 0;
        assert_eq!(
            retail,
            port.step_blocked(x, z, lut),
            "step lut {lut}, {label}"
        );
        step_hits += u32::from(retail);

        let dir = rng.below(4) as u8;
        cpu.regs.r[V0] = 0xDEAD_BEEF;
        assert_eq!(cpu.call(WANDER, &[ACTOR, u32::from(dir)]), Stop::Returned);
        let retail = cpu.regs.r[V0] & 1 != 0;
        assert_eq!(
            retail,
            port.wander_blocked(x, z, dir),
            "wander dir {dir}, {label}"
        );
        wander_hits += u32::from(retail);
        misses += u32::from(!retail);
    }
    assert!(
        step_hits > 3000 && wander_hits > 5000 && exempt_runs > 5000 && misses > 10_000,
        "{step_hits} {wander_hits} {exempt_runs} {misses}"
    );
}
