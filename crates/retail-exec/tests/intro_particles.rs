//! The field-to-battle transition's two particle fields off PROT 0979:
//! `FUN_801CFDA0` (the scatter) and `FUN_801D0370` (the spin-up, its ring
//! tail `FUN_801D1CFC` stubbed) against `battle_intro::emit_particle_field`
//! over the same `0x488`-record grid. Compared: every record byte after the
//! frame (the integration), the clock, and the `POLY_FT4` packets the routine
//! links into the ordering table (corners, texels, page, colour word and the
//! semi-transparency bit) against the quads the port emits, in link order.
//!
//! The view matrix `0x1F8003C8` is the identity rotation with zero
//! translation, which is what the transition holds from its second frame on
//! and what the port's projection assumes; the GTE's `OFX` / `OFY` are the
//! port's `INTRO_OFX` / `INTRO_OFY`.

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_render_kernels::battle_intro::{INTRO_OFX, INTRO_OFY, emit_particle_field};
use legaia_render_kernels::battle_intro_particles::IntroParticle;
use legaia_render_kernels::battle_intro_styles::{
    PARTICLE_TICK_A, PARTICLE_TICK_B, ParticleTickStyle,
};
use legaia_render_kernels::screen_prim::ScreenPrim;
use psx_retail_exec::{Cpu, Memory, Stop};

const OVERLAY_BASE: u32 = 0x801C_E818;
const SCATTER: u32 = 0x801C_FDA0;
const SPIN_UP: u32 = 0x801D_0370;
const RING: u32 = 0x801D_1CFC;
const ENTITY: u32 = 0x8010_0000;
const GRID: u32 = 0x8011_0000;
const PRIMS: u32 = 0x8014_0000;
const OT: u32 = 0x8013_0000;
const COUNT: usize = 0x488;
const STRIDE: u32 = 0x2C;
const VIEW: u32 = 0x1F80_03C8;
const PRIM_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const FRAME_STEP: u32 = 0x1F80_0393;
const OT_END: u32 = 0x00FF_FFFF;

fn intro_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    let image = crate::common::disc()?.read_entry(979).ok()?;
    cpu.mem.load(OVERLAY_BASE, &image);
    cpu.stub(RING, 0);
    // The sine and cosine LUT pointers the boot installs.
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    Some(cpu)
}

fn random_particle(rng: &mut Rng) -> IntroParticle {
    let s = |rng: &mut Rng, span: u32| (rng.below(2 * span) as i32 - span as i32) as i16;
    IntroParticle {
        tint: if rng.below(8) == 0 {
            rng.next()
        } else {
            rng.next() & 0x00FF_FFFF
        },
        trans: (rng.next() as i16, rng.next() as i16, rng.next() as i16),
        rot: (
            s(rng, 0x1400),
            s(rng, 0x1000),
            0x400 + rng.below(0x3000) as i16,
        ),
        field_16: rng.next() as i16,
        spin: (s(rng, 0x200), s(rng, 0x200), s(rng, 0x200)),
        delay: rng.below(0x3000) as i16,
        trans_vel: (s(rng, 0x100), s(rng, 0x100), s(rng, 0x100)),
        texel_page: rng.next() as i16,
        texel_v: rng.next() as i16,
    }
}

fn write_particle(mem: &mut Memory, at: u32, p: &IntroParticle) {
    mem.wr32(at + 4, p.tint);
    for (o, v) in [
        (0x08, p.trans.0),
        (0x0A, p.trans.1),
        (0x0C, p.trans.2),
        (0x10, p.rot.0),
        (0x12, p.rot.1),
        (0x14, p.rot.2),
        (0x16, p.field_16),
        (0x18, p.spin.0),
        (0x1A, p.spin.1),
        (0x1C, p.spin.2),
        (0x1E, p.delay),
        (0x20, p.trans_vel.0),
        (0x22, p.trans_vel.1),
        (0x24, p.trans_vel.2),
        (0x28, p.texel_page),
        (0x2A, p.texel_v),
    ] {
        mem.wr16(at + o, v as u16);
    }
}

/// The packets linked at OT word `word`, in draw order (the chain is
/// prepended, so the head is the last linked).
fn chain(mem: &mut Memory, word: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut p = mem.rd32(OT + 4 * word) & 0x00FF_FFFF;
    while p != OT_END && out.len() <= COUNT {
        let at = 0x8000_0000 | p;
        out.push(at);
        p = mem.rd32(at) & 0x00FF_FFFF;
    }
    out
}

/// A packet as `(xy, uv, tpage, colour word)`.
type Packet = ([(i16, i16); 4], [(u8, u8); 4], u16, u32);

fn read_packet(mem: &mut Memory, at: u32) -> Packet {
    let xy = [8u32, 0x10, 0x18, 0x20].map(|o| {
        let w = mem.rd32(at + o);
        (w as i16, (w >> 16) as i16)
    });
    let uv = [0xCu32, 0x14, 0x1C, 0x24].map(|o| (mem.rd8(at + o), mem.rd8(at + o + 1)));
    (xy, uv, mem.rd16(at + 0x16), mem.rd32(at + 4))
}

fn port_packet(p: &ScreenPrim) -> (u32, Packet) {
    let ScreenPrim::Textured(q) = p else {
        panic!("a particle is a textured quad");
    };
    let c = q.color;
    let word = (c >> 16 & 0xFF) | (c & 0xFF00) | (c & 0xFF) << 16 | 0x2C00_0000;
    let word = word | if q.semi_transparent { 0x0200_0000 } else { 0 };
    (q.ot_index, (q.xy, q.uv, q.tpage, word))
}

fn run(entry: u32, style: &ParticleTickStyle, seed: u64) {
    let Some(mut cpu) = intro_cpu() else {
        return;
    };
    let mut rng = Rng(seed);
    let (mut drawn, mut moved, mut masked) = (0usize, 0usize, 0usize);
    for round in 0..24 {
        let elapsed = if round == 0 {
            0
        } else {
            1 + rng.below(0x60) as i16
        };
        let step = 1 + rng.below(4) as u8;
        let mut grid: Vec<IntroParticle> = (0..COUNT).map(|_| random_particle(&mut rng)).collect();
        let filler: Vec<u8> = (0..COUNT * STRIDE as usize)
            .map(|_| rng.next() as u8)
            .collect();
        cpu.mem.load(GRID, &filler);
        for (i, p) in grid.iter().enumerate() {
            write_particle(&mut cpu.mem, GRID + STRIDE * i as u32, p);
        }
        cpu.mem.load(ENTITY, &[0u8; 0x60]);
        cpu.mem.wr16(ENTITY + 0x1A, elapsed as u16);
        cpu.mem.wr32(ENTITY + 0x48, GRID);
        // The view: identity rotation, zero translation.
        for k in 0..8 {
            cpu.mem.wr32(VIEW + 4 * k, 0);
        }
        cpu.mem.wr16(VIEW, 0x1000);
        cpu.mem.wr16(VIEW + 8, 0x1000);
        cpu.mem.wr16(VIEW + 0x10, 0x1000);
        cpu.gte.write_ctrl(24, (INTRO_OFX << 16) as u32);
        cpu.gte.write_ctrl(25, (INTRO_OFY << 16) as u32);
        cpu.mem.wr32(PRIM_PTR, PRIMS);
        cpu.mem.wr32(OT_PTR, OT);
        for w in 90..110 {
            cpu.mem.wr32(OT + 4 * w, OT_END);
        }
        cpu.mem.wr8(FRAME_STEP, step);
        assert_eq!(cpu.call(entry, &[ENTITY]), Stop::Returned);

        let mut clock = elapsed;
        let mut prims = Vec::new();
        let emit = elapsed != 0;
        emit_particle_field(&mut grid, style, &mut clock, step, emit, &mut prims);
        let ctx = format!("round {round} elapsed {elapsed} step {step}");
        assert_eq!(cpu.mem.rd16(ENTITY + 0x1A) as i16, clock, "clock, {ctx}");

        let mut port_mem = Memory::default();
        port_mem.load(GRID, &filler);
        for (i, p) in grid.iter().enumerate() {
            write_particle(&mut port_mem, GRID + STRIDE * i as u32, p);
        }
        let want = port_mem.read(GRID, filler.len());
        let got = cpu.mem.read(GRID, want.len());
        assert!(
            got == want,
            "records, {ctx}: {:?}",
            diff_offsets(&got, &want)
        );

        // The first frame projects through the field camera's stale view in
        // retail, which culls every quad; the port does not emit on it. This
        // harness's identity view cannot stand for that frame.
        if !emit {
            continue;
        }
        // Packets, in draw order: the port lists them in particle order and
        // each OT word's chain is prepended.
        let mut retail: Vec<(u32, Packet)> = Vec::new();
        for word in [99u32, 100] {
            for at in chain(&mut cpu.mem, word).into_iter().rev() {
                retail.push((word * 4, read_packet(&mut cpu.mem, at)));
            }
        }
        let mut port: Vec<(u32, Packet)> = prims.iter().map(port_packet).collect();
        // Retail's two buckets are walked whole, one after the other.
        port.sort_by_key(|(ot, _)| *ot);
        retail.sort_by_key(|(ot, _)| *ot);
        assert_eq!(retail.len(), port.len(), "packet count, {ctx}");
        for (k, (r, p)) in retail.iter().zip(&port).enumerate() {
            assert_eq!(r, p, "packet {k}, {ctx}");
        }
        drawn += port.len();
        moved += port.iter().filter(|(_, p)| p.3 & 0x0200_0000 != 0).count();
        masked += grid.iter().filter(|p| p.tint >> 24 != 0).count();
    }
    assert!(
        drawn > 2000 && moved > 500 && masked > 1000,
        "drawn {drawn} moved {moved} masked {masked}"
    );
}

#[test]
fn the_scatter_field_matches_its_emitter() {
    differential!(prot(979, SCATTER));
    run(SCATTER, &PARTICLE_TICK_A, 0x5343_4154);
}

#[test]
fn the_spin_up_field_matches_its_emitter() {
    differential!(prot(979, SPIN_UP));
    run(SPIN_UP, &PARTICLE_TICK_B, 0x5350_494E);
}
