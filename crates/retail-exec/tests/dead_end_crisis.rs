//! PROT 0961's (Dead End Crisis / Final Crisis) seat line-up and restore,
//! run off the module image against
//! `cast_module_camera::dead_end_crisis_line_up` / `dead_end_crisis_restore`.
//!
//! Arm 0 saves `actor_table[0..4]`'s poses into the module's own words and
//! stands the seats; the `0xFF` arm puts them back. Each run compares all eight
//! actor records whole (only the four seats' `+0x34..+0x3B` and `+0x46` may
//! move), the module's saved words, and the context against the phase chain's
//! port (`DEAD_END_CRISIS_CHAIN`) with the director's `0xFF` store.

use crate::cast_view::{diffs, read_ctx, write_ctx};
use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_cast::cast_module_camera::{
    self as cam, DEAD_END_CRISIS_LINE_UP_SEATS as N, LineUpPose, ModuleCamSeats, ModuleCamState,
};
use legaia_engine_cast::cast_module_ticks as t;
use psx_retail_exec::{Cpu, Memory, Stop, V0};

const BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const RECORDS: u32 = 0x801C_9348;
const CTX: u32 = 0x8012_0000;
const CTX_LEN: usize = 0x1100;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;
const FORMATION: u32 = 0x8007_BD0C;
const RAND: u32 = 0x8005_6798;
/// The seat table's one-monster row (`addiu a3,v0,0x7628` at `0x801F6AEC`).
const SEAT_ROW: u32 = 0x8007_7628;
/// The module words arm 0 saves into (`-0x7D44` / `-0x7D24` off `0x8020`).
const SAVED_POS: u32 = 0x801F_82BC;
const SAVED_FACING: u32 = 0x801F_82DC;
/// The record arm `0xFF` flags in formation `0xB5`: `*(*(0x801C9348) + 0x64) + 0x87`.
const RECORD: u32 = 0x8015_8000;
const RECORD_SUB: u32 = 0x8015_9000;
const DEAD_END_CRISIS: u32 = 0x801F_69D8;

fn pose(m: &mut Memory, a: u32) -> LineUpPose {
    LineUpPose {
        pos: [0, 2, 4, 6].map(|o| m.rd16(a + 0x34 + o) as i16),
        facing: m.rd16(a + 0x46),
    }
}

fn put_pose(rec: &mut [u8], p: &LineUpPose) {
    for (i, v) in p.pos.iter().enumerate() {
        rec[0x34 + i * 2..0x36 + i * 2].copy_from_slice(&v.to_le_bytes());
    }
    rec[0x46..0x48].copy_from_slice(&p.facing.to_le_bytes());
}

fn saved(m: &mut Memory) -> [LineUpPose; N] {
    std::array::from_fn(|s| LineUpPose {
        pos: [0, 2, 4, 6].map(|o| m.rd16(SAVED_POS + s as u32 * 8 + o) as i16),
        facing: m.rd16(SAVED_FACING + s as u32 * 2),
    })
}

/// Every `jal` target the image's words encode, outside the image.
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

fn rig() -> Option<(Cpu, Vec<u8>)> {
    let (mut cpu, disc) = (scus_cpu()?, disc()?);
    cpu.mem.load(OVERLAY_BASE, &disc.read_entry(898).ok()?);
    let module = disc.read_entry(961).ok()?;
    cpu.mem.wr32(CTX_PTR, CTX);
    for target in jal_targets(&module) {
        if target != RAND {
            cpu.stub(target, 0);
        }
    }
    cpu.stub(RAND, 0);
    Some((cpu, module))
}

fn seed(cpu: &mut Cpu, rng: &mut Rng, module: &[u8], phase: u8, formation: u8) {
    cpu.mem.load(BASE, module);
    for b in 0..CTX_LEN as u32 {
        cpu.mem.wr8(CTX + b, rng.next() as u8);
    }
    cpu.mem.wr8(CTX + 0x13, 3);
    cpu.mem.wr8(CTX + 0x279, phase);
    for s in 0..8u32 {
        let a = ACTOR + s * STRIDE;
        cpu.mem.wr32(ACTORS + s * 4, a);
        for o in (0..STRIDE).step_by(2) {
            cpu.mem.wr16(a + o, rng.next() as u16);
        }
    }
    cpu.mem.wr8(FORMATION, formation);
    cpu.mem.wr32(RECORDS, RECORD);
    cpu.mem.wr32(RECORD + 0x64, RECORD_SUB);
    cpu.mem.wr8(RECORD_SUB + 0x87, 0);
}

#[test]
fn the_dead_end_crisis_line_up_and_restore_match_retail() {
    differential!(prot(961, DEAD_END_CRISIS));
    let Some((mut cpu, module)) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The row the port reads (`battle_seats`) is the executable's.
    let row_x: [i16; 3] = std::array::from_fn(|s| cpu.mem.rd16(SEAT_ROW + s as u32 * 8) as i16);
    for (s, x) in row_x.iter().enumerate() {
        assert_eq!(
            legaia_engine_battle::battle_seats::monster_seat(1, s, false).x,
            *x,
            "seat {s}"
        );
    }
    let chain = &t::DEAD_END_CRISIS_CHAIN;
    let mut rng = Rng(0x4445_4344);
    for round in 0..64 {
        let formation = if round % 2 == 0 { 0xB5 } else { 0x2A };

        // Arm 0: the line-up.
        seed(&mut cpu, &mut rng, &module, 0, formation);
        let (mut ctx, mut want) = (cpu.mem.read(CTX, CTX_LEN), cpu.mem.read(ACTOR, 0x2000));
        let mut poses: [LineUpPose; N] =
            std::array::from_fn(|s| pose(&mut cpu.mem, ACTOR + s as u32 * STRIDE));
        let saved_port = cam::dead_end_crisis_line_up(&mut poses, row_x);
        for (s, p) in poses.iter().enumerate() {
            put_pose(&mut want[s * STRIDE as usize..], p);
        }
        let mut view = read_ctx(&mut cpu.mem, CTX);
        let (mut c, mut v, mut st) = Default::default();
        t::run_chain_body(chain, &mut view, &mut c, &mut v, &mut st, 0);
        write_ctx(&mut ctx, &view);
        assert_eq!(cpu.call(DEAD_END_CRISIS, &[]), Stop::Returned);
        assert_ne!(cpu.regs.r[V0], 0, "round {round}: arm 0 is busy");
        let got = cpu.mem.read(ACTOR, 0x2000);
        assert!(got == want, "round {round} arm 0: {:?}", diffs(&got, &want));
        assert_eq!(
            saved(&mut cpu.mem),
            saved_port,
            "round {round}: saved words"
        );
        let got = cpu.mem.read(CTX, CTX_LEN);
        assert!(
            got == ctx,
            "round {round} arm 0 ctx: {:?}",
            diffs(&got, &ctx)
        );

        // Arm `0xFF`: the restore, off whatever the words hold.
        seed(&mut cpu, &mut rng, &module, 0xFF, formation);
        for b in 0..(N as u32 * 10) {
            cpu.mem.wr8(SAVED_POS + b, rng.next() as u8);
        }
        let held = saved(&mut cpu.mem);
        let (mut ctx, mut want) = (cpu.mem.read(CTX, CTX_LEN), cpu.mem.read(ACTOR, 0x2000));
        let mut poses: [LineUpPose; N] =
            std::array::from_fn(|s| pose(&mut cpu.mem, ACTOR + s as u32 * STRIDE));
        cam::dead_end_crisis_restore(&mut poses, &held);
        for (s, p) in poses.iter().enumerate() {
            put_pose(&mut want[s * STRIDE as usize..], p);
        }
        let mut view = read_ctx(&mut cpu.mem, CTX);
        let done = t::run_chain_body(chain, &mut view, &mut c, &mut v, &mut st, 0);
        assert_eq!(done.step, t::CastTickStep::Done);
        write_ctx(&mut ctx, &view);
        // The director's `0xFF` store, `ctx[+0x6DA] = 0x780`.
        let mut cam_st = ModuleCamState::default();
        cam::dead_end_crisis_camera(&mut cam_st, 0xFF, ModuleCamSeats::default());
        ctx[0x6DA..0x6DC].copy_from_slice(&(cam_st.yaw_base as u16).to_le_bytes());
        assert_eq!(cpu.call(DEAD_END_CRISIS, &[]), Stop::Returned);
        assert_eq!(cpu.regs.r[V0], 0, "round {round}: arm 0xFF ends the module");
        let got = cpu.mem.read(ACTOR, 0x2000);
        assert!(
            got == want,
            "round {round} arm 0xFF: {:?}",
            diffs(&got, &want)
        );
        let got = cpu.mem.read(CTX, CTX_LEN);
        assert!(
            got == ctx,
            "round {round} arm 0xFF ctx: {:?}",
            diffs(&got, &ctx)
        );
        // Not ported: the evolved Cort's flag the arm raises in formation
        // `0xB5` (`0x801F785C..0x801F786C`).
        assert_eq!(
            cpu.mem.rd8(RECORD_SUB + 0x87),
            u8::from(formation == 0xB5),
            "round {round}: the 0xB5 record flag"
        );
    }
}
