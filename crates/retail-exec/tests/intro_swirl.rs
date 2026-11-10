//! The field-to-battle transition's swirl off PROT 0979: the band submit
//! `FUN_801D1A20` before the late phase (clock below `0x5A`, through the SCUS
//! dispatcher `FUN_80043390`) against `emit_swirl_band`, and the tick `FUN_801D1888` with the submit hooked
//! against `tick_swirl`. The mesh is the port's own build over the PSX sine
//! table; the band pose is what the tick loads, identity rotation with the
//! band's depth as the translation. Compared: the packets linked into the
//! ordering table, bucket by bucket in link order; the band table and the
//! clock; the submit calls the tick makes.
//!
//! The late phase is left out: there the submit hands the band to the
//! shatter renderer `FUN_80029888`, which draws each quad as two `POLY_FT3`
//! triangles spun about their own centroids and pushed apart, where
//! `emit_swirl_band` draws the whole quads rotated about the origin.

use crate::common::{Rng, scus_cpu};
use legaia_render_kernels::battle_intro::{
    INTRO_NEAR_OTZ, INTRO_OFX, INTRO_OFY, emit_swirl_band, gpu_span_cut,
};
use legaia_render_kernels::battle_intro_swirl::{
    BANDS, SwirlBuildOutcome, SwirlHalf, SwirlMesh, SwirlTrig, build_swirl_mesh, swirl_band_draw,
    tick_swirl,
};
use legaia_render_kernels::gte::{psx_cos, psx_sin};
use legaia_render_kernels::screen_prim::ScreenPrim;
use psx_retail_exec::{Cpu, Memory, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const TICK: u32 = 0x801D_1888;
const SUBMIT: u32 = 0x801D_1A20;
const CLOCK: u32 = 0x801D_2470;
const VERTS_PTR: u32 = 0x801D_2474;
const TEXELS_PTR: u32 = 0x801D_2478;
const BANDS_PTR: u32 = 0x801D_247C;
const SCRATCH_BASE_PTR: u32 = 0x8007_B85C;
const SCRATCH_BASE: u32 = 0x8010_1000;
const ENTITY: u32 = 0x8010_0000;
const VERTS: u32 = 0x8017_0000;
const TEXELS: u32 = 0x8017_8000;
const BAND_TABLE: u32 = 0x8017_A000;
const PRIMS: u32 = 0x8018_0000;
const OT: u32 = 0x801A_0000;
const OT_WORDS: u32 = 0x4000;
const PRIM_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const OT_SHIFT: u32 = 0x1F80_03A4;
const NEAR_OTZ: u32 = 0x1F80_037E;
const SYSTEM_FLAGS: u32 = 0x1F80_0394;
const LINK_MASK: u32 = 0x1F80_03FC;
const FRAME_STEP: u32 = 0x1F80_0393;
const VIEW: u32 = 0x1F80_03C8;
const OT_END: u32 = 0x00FF_FFFF;

struct PsxTables;

impl SwirlTrig for PsxTables {
    fn table_x(&mut self, entry: i32) -> i16 {
        psx_sin((entry & 0xFFF) as u16) as i16
    }
    fn table_y(&mut self, entry: i32) -> i16 {
        psx_cos((entry & 0xFFF) as u16) as i16
    }
}

fn mesh() -> SwirlMesh {
    match build_swirl_mesh(true, &mut PsxTables) {
        SwirlBuildOutcome::Built(m) => *m,
        other => panic!("swirl mesh: {other:?}"),
    }
}

fn intro_cpu(mesh: &SwirlMesh) -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    let image = crate::common::disc()?.read_entry(979).ok()?;
    cpu.mem.load(OVERLAY_BASE, &image);
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    for (i, v) in mesh.vertices.iter().enumerate() {
        let at = VERTS + 8 * i as u32;
        cpu.mem.wr16(at, v.x as u16);
        cpu.mem.wr16(at + 2, v.y as u16);
        cpu.mem.wr16(at + 4, v.z as u16);
        cpu.mem.wr16(at + 6, 0);
    }
    for (i, t) in mesh.texels.iter().enumerate() {
        cpu.mem.wr8(TEXELS + 2 * i as u32, t.0 as u8);
        cpu.mem.wr8(TEXELS + 2 * i as u32 + 1, t.1 as u8);
    }
    cpu.mem.wr32(VERTS_PTR, VERTS);
    cpu.mem.wr32(TEXELS_PTR, TEXELS);
    Some(cpu)
}

fn seat_render_state(cpu: &mut Cpu) {
    cpu.mem.wr32(SCRATCH_BASE_PTR, SCRATCH_BASE);
    cpu.gte.write_ctrl(24, (INTRO_OFX << 16) as u32);
    cpu.gte.write_ctrl(25, (INTRO_OFY << 16) as u32);
    cpu.gte.write_ctrl(26, 0x80);
    cpu.mem.wr32(PRIM_PTR, PRIMS);
    cpu.mem.wr32(OT_PTR, OT);
    cpu.mem.wr8(OT_SHIFT, 0);
    cpu.mem.wr16(NEAR_OTZ, INTRO_NEAR_OTZ as u16);
    cpu.mem.wr32(SYSTEM_FLAGS, 0);
    cpu.mem.wr32(LINK_MASK, 0x00FF_FFFF);
    for w in 0..OT_WORDS {
        cpu.mem.wr32(OT + 4 * w, OT_END);
    }
}

/// A packet as `(xy, uv, clut, tpage, colour word)`.
type Packet = ([(i16, i16); 4], [(u8, u8); 4], u16, u16, u32);

fn read_packet(mem: &mut Memory, at: u32) -> Packet {
    let xy = [8u32, 0x10, 0x18, 0x20].map(|o| {
        let w = mem.rd32(at + o);
        (w as i16, (w >> 16) as i16)
    });
    let uv = [0xCu32, 0x14, 0x1C, 0x24].map(|o| (mem.rd8(at + o), mem.rd8(at + o + 1)));
    (
        xy,
        uv,
        mem.rd16(at + 0xE),
        mem.rd16(at + 0x16),
        mem.rd32(at + 4),
    )
}

fn port_packet(p: &ScreenPrim) -> (u32, Packet) {
    let ScreenPrim::Textured(q) = p else {
        panic!("a swirl quad is a textured quad");
    };
    let c = q.color;
    let word = (c >> 16 & 0xFF) | (c & 0xFF00) | (c & 0xFF) << 16;
    let word = word
        | if q.semi_transparent {
            0x2E00_0000
        } else {
            0x2C00_0000
        };
    (q.ot_index >> 2, (q.xy, q.uv, q.clut, q.tpage, word))
}

fn compare_packets(mem: &mut Memory, prims: &[ScreenPrim], ctx: &str) -> usize {
    let mut retail: Vec<(u32, Packet)> = Vec::new();
    for word in 0..OT_WORDS {
        let mut chain = Vec::new();
        let mut p = mem.rd32(OT + 4 * word) & 0x00FF_FFFF;
        while p != OT_END && chain.len() < 4096 {
            chain.push(0x8000_0000 | p);
            p = mem.rd32(0x8000_0000 | p) & 0x00FF_FFFF;
        }
        for at in chain.into_iter().rev() {
            // The GPU's span limit, which the port applies at emission
            // (`gpu_span_cut`): retail links the packet and the GPU skips it.
            let (xy, uv, clut, tpage, word_c) = read_packet(mem, at);
            if let Some(xy) = gpu_span_cut(xy) {
                retail.push((word, (xy, uv, clut, tpage, word_c)));
            }
        }
    }
    let mut port: Vec<(u32, Packet)> = prims.iter().map(port_packet).collect();
    port.sort_by_key(|(ot, _)| *ot);
    assert_eq!(retail.len(), port.len(), "packet count, {ctx}");
    for (k, (r, p)) in retail.iter().zip(&port).enumerate() {
        assert_eq!(r, p, "packet {k}, {ctx}");
    }
    port.len()
}

#[test]
fn the_swirl_band_submit_matches_emit_swirl_band() {
    differential!(prot(979, SUBMIT));
    let mesh = mesh();
    let Some(mut cpu) = intro_cpu(&mesh) else {
        return;
    };
    let mut rng = Rng(0x5357_524C);
    let (mut drawn, mut mirrored) = (0usize, 0usize);
    for round in 0..400 {
        let band = rng.below(BANDS as u32) as usize;
        let half = if rng.below(2) == 0 {
            SwirlHalf::Primary
        } else {
            SwirlHalf::Mirrored
        };
        let clock = 1 + rng.below(0x59) as i32;
        let band_z = 0x81 + rng.below(0x2000) as i32;
        seat_render_state(&mut cpu);
        cpu.mem.wr32(CLOCK, clock as u32);
        cpu.gte.write_ctrl(0, 0x1000);
        cpu.gte.write_ctrl(1, 0);
        cpu.gte.write_ctrl(2, 0x1000);
        cpu.gte.write_ctrl(3, 0);
        cpu.gte.write_ctrl(4, 0x1000);
        cpu.gte.write_ctrl(5, 0);
        cpu.gte.write_ctrl(6, 0);
        cpu.gte.write_ctrl(7, band_z as u32);
        let a0 = u32::from(half == SwirlHalf::Mirrored);
        assert_eq!(cpu.call(SUBMIT, &[a0, band as u32]), Stop::Returned);
        let draw = swirl_band_draw(half, band, clock);
        let mut prims = Vec::new();
        emit_swirl_band(&mesh, band_z, &draw, &mut prims);
        let ctx = format!("round {round} band {band} {half:?} clock {clock:#x} z {band_z:#x}");
        drawn += compare_packets(&mut cpu.mem, &prims, &ctx);
        mirrored += usize::from(half == SwirlHalf::Mirrored);
    }
    assert!(
        drawn > 5000 && mirrored > 100,
        "drawn {drawn} mirrored {mirrored}"
    );
}

#[test]
fn the_swirl_tick_matches_tick_swirl() {
    differential!(prot(979, TICK));
    let mut mesh = mesh();
    let Some(mut cpu) = intro_cpu(&mesh) else {
        return;
    };
    let calls: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    let log = calls.clone();
    cpu.hook(SUBMIT, move |r, _| {
        log.borrow_mut()
            .push((r.r[psx_retail_exec::A0], r.r[psx_retail_exec::A0 + 1]));
    });
    let mut rng = Rng(0x5357_544B);
    let mut elapsed: i16 = 0;
    let mut prev_clock = 0i32;
    cpu.mem.wr32(BANDS_PTR, BAND_TABLE);
    for (i, b) in mesh.bands.iter().enumerate() {
        let at = BAND_TABLE + 0x10 * i as u32;
        cpu.mem.wr32(at, b.phase as u32);
        cpu.mem.wr32(at + 4, b.width as u32);
        cpu.mem.wr32(at + 8, b.angle as u32);
        cpu.mem.wr32(at + 0xC, b.rate as u32);
    }
    cpu.mem.wr32(CLOCK, 0);
    cpu.mem.load(ENTITY, &[0u8; 0x60]);
    for k in 0..8 {
        cpu.mem.wr32(VIEW + 4 * k, 0);
    }
    cpu.mem.wr16(VIEW, 0x1000);
    cpu.mem.wr16(VIEW + 8, 0x1000);
    cpu.mem.wr16(VIEW + 0x10, 0x1000);
    let mut total = 0;
    for frame in 0..80 {
        let step = 1 + rng.below(3) as u8;
        cpu.mem.wr8(FRAME_STEP, step);
        cpu.mem.wr16(ENTITY + 0x1A, elapsed as u16);
        calls.borrow_mut().clear();
        assert_eq!(cpu.call(TICK, &[ENTITY]), Stop::Returned);
        let tick = tick_swirl(&mut mesh, &mut elapsed, step, &mut prev_clock);
        let ctx = format!("frame {frame} step {step}");
        assert_eq!(cpu.mem.rd16(ENTITY + 0x1A) as i16, elapsed, "clock, {ctx}");
        assert_eq!(
            cpu.mem.rd32(CLOCK) as i32,
            prev_clock,
            "submit clock, {ctx}"
        );
        for (i, b) in mesh.bands.iter().enumerate() {
            let at = BAND_TABLE + 0x10 * i as u32;
            assert_eq!(
                cpu.mem.rd32(at + 8) as i32,
                b.angle,
                "band {i} scalar, {ctx}"
            );
        }
        let got: Vec<(u32, u32)> = calls.borrow().clone();
        let want: Vec<(u32, u32)> = tick
            .draws
            .iter()
            .map(|d| {
                let band = d.first_vertex / (2 * 99);
                let half = u32::from(d.first_vertex % (2 * 99) != 0);
                (half, band as u32)
            })
            .collect();
        assert_eq!(got, want, "submits, {ctx}");
        total += got.len();
    }
    assert!(total > 200, "submits {total}");
}
