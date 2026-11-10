//! The field-to-battle transition's tile shatter off PROT 0979: the tick
//! `FUN_801D0D24` and its per-tile emitter `FUN_801D0E54`, which builds a
//! ten-primitive object and hands it to the SCUS dispatcher `FUN_80043390`
//! and its `FT4` leaf, against `tick_tile_grid_emit` with `emit_tile` as the
//! hook. Compared: every record byte after the frame, the clock, and the
//! packets linked into the ordering table (corners, texels, CLUT, page, colour
//! word) against the port's quads, bucket by bucket in link order.
//!
//! The view `0x1F8003C8` is the identity with zero translation (the
//! transition's view from its second frame on), the GTE screen centre the
//! port's `INTRO_OFX` / `INTRO_OFY`, the dispatcher's OT shift `0` and its
//! near cutoff `0x1F80037E` the port's `INTRO_NEAR_OTZ`. The records' `v`
//! bytes stay off the bottom row's wrapped `0`, where the port departs from
//! the stored bytes by choice (`unwrap_bottom_edge_v`).

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_render_kernels::battle_intro::{INTRO_NEAR_OTZ, INTRO_OFX, INTRO_OFY, emit_tile};
use legaia_render_kernels::battle_intro_tiles::{
    TileCorner, TileGrid, TileRecord, tick_tile_grid_emit,
};
use legaia_render_kernels::screen_prim::ScreenPrim;
use psx_retail_exec::{Cpu, Memory, Stop};

const OVERLAY_BASE: u32 = 0x801C_E818;
const TICK: u32 = 0x801D_0D24;
const EMIT: u32 = 0x801D_0E54;
const DISPATCH: u32 = 0x8004_3390;
const TILES_PTR: u32 = 0x801D_2468;
const SCRATCH_BASE_PTR: u32 = 0x8007_B85C;
const ENTITY: u32 = 0x8010_0000;
const SCRATCH_BASE: u32 = 0x8010_1000;
const TILES: u32 = 0x8017_0000;
const PRIMS: u32 = 0x8018_0000;
const OT: u32 = 0x801A_0000;
const OT_WORDS: u32 = 0x4000;
const COUNT: usize = 0x100;
const STRIDE: u32 = 0x5C;
const VIEW: u32 = 0x1F80_03C8;
const PRIM_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const OT_SHIFT: u32 = 0x1F80_03A4;
const NEAR_OTZ: u32 = 0x1F80_037E;
const SYSTEM_FLAGS: u32 = 0x1F80_0394;
const FRAME_STEP: u32 = 0x1F80_0393;
const OT_END: u32 = 0x00FF_FFFF;
/// The address mask the leaves link with (`lw a0,0xe8(t2)`).
const LINK_MASK: u32 = 0x1F80_03FC;

fn intro_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    let image = crate::common::disc()?.read_entry(979).ok()?;
    cpu.mem.load(OVERLAY_BASE, &image);
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    Some(cpu)
}

fn random_tile(rng: &mut Rng) -> TileRecord {
    let s = |rng: &mut Rng, span: u32| (rng.below(2 * span) as i32 - span as i32) as i16;
    let corner = |rng: &mut Rng, x: i16, y: i16, z: i16| TileCorner {
        x,
        y,
        z,
        pad: s(rng, 0x40),
        uv: (rng.next() as i8, (1 + rng.below(0xFF)) as u8 as i8),
    };
    let h = 0x40 + rng.below(0x40) as i16;
    let d = 8 + rng.below(0x20) as i16;
    let mut front = [(-h, -h), (h, -h), (-h, h), (h, h)].map(|(x, y)| corner(rng, x, y, -d));
    let mut back = [(-h, -h), (h, -h), (-h, h), (h, h)].map(|(x, y)| corner(rng, x, y, d));
    for k in 0..4 {
        back[k].uv = front[k].uv;
    }
    // The spin rates stay small: they double every frame.
    back[0].pad = s(rng, 0x20);
    back[1].pad = s(rng, 0x20);
    front[2].pad = s(rng, 0x20);
    TileRecord {
        progress: if rng.below(8) == 0 {
            0x1000 + rng.below(0x100) as i16
        } else {
            rng.below(0x1000) as i16
        },
        tpage: rng.next() as i16,
        pos: (
            s(rng, 0x800),
            s(rng, 0x600),
            0x800 + rng.below(0x1800) as i16,
        ),
        delay: rng.below(0x1000) as i16,
        angles: (rng.next() as i16, rng.next() as i16, rng.next() as i16),
        front,
        back,
    }
}

fn write_tile(mem: &mut Memory, at: u32, t: &TileRecord) {
    let mut h = |o: u32, v: i16| mem.wr16(at + o, v as u16);
    h(0, t.progress);
    h(2, t.tpage);
    h(4, t.pos.0);
    h(6, t.pos.1);
    h(8, t.pos.2);
    h(0xA, t.delay);
    h(0xC, t.angles.0);
    h(0xE, t.angles.1);
    h(0x10, t.angles.2);
    for (k, c) in t.front.iter().chain(&t.back).enumerate() {
        let o = 0x14 + 8 * k as u32;
        h(o, c.x);
        h(o + 2, c.y);
        h(o + 4, c.z);
        h(o + 6, c.pad);
    }
    for (k, c) in t.front.iter().enumerate() {
        mem.wr8(at + 0x54 + 2 * k as u32, c.uv.0 as u8);
        mem.wr8(at + 0x55 + 2 * k as u32, c.uv.1 as u8);
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
        panic!("a tile face is a textured quad");
    };
    let c = q.color;
    let word = (c >> 16 & 0xFF) | (c & 0xFF00) | (c & 0xFF) << 16;
    let word = word
        | if q.semi_transparent {
            0x2E00_0000
        } else {
            0x2C00_0000
        };
    // The leaf links at OT byte `otz & !3`, word `otz >> 2`.
    (q.ot_index >> 2, (q.xy, q.uv, q.clut, q.tpage, word))
}

/// Every packet linked into the OT against the port's quads, bucket by
/// bucket in link order; the count compared.
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
            retail.push((word, read_packet(mem, at)));
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

/// The render state the tick and the emitter read.
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

/// The emitter and the dispatcher called on their own, one record at a time,
/// with the record's matrix loaded the way the tick loads it: the rotation
/// `RotMatrix(+0x0C)` (`FUN_80026988`, differential-tested on its own) and the
/// translation the identity view leaves, `+0x04`. The dispatcher then re-runs
/// over the object the emitter built, at the record's progress.
#[test]
fn the_tile_emitter_and_dispatcher_match_emit_tile() {
    differential!(prot(979, EMIT), scus(DISPATCH));
    let Some(mut cpu) = intro_cpu() else {
        return;
    };
    let mut rng = Rng(0x454D_4954);
    let (mut drawn, mut faded) = (0usize, 0usize);
    for round in 0..3000 {
        let mut rec = random_tile(&mut rng);
        if round % 4 == 0 {
            rec.progress = 0;
        }
        let load_matrix = |cpu: &mut Cpu| {
            let m = legaia_render_kernels::battle_intro::euler_rot_psx(rec.angles);
            let e: Vec<i16> = m.m.iter().flatten().copied().collect();
            let p = |a: i16, b: i16| (a as u16 as u32) | (b as u16 as u32) << 16;
            for (k, w) in [
                p(e[0], e[1]),
                p(e[2], e[3]),
                p(e[4], e[5]),
                p(e[6], e[7]),
                e[8] as u16 as u32,
                rec.pos.0 as i32 as u32,
                rec.pos.1 as i32 as u32,
                rec.pos.2 as i32 as u32,
            ]
            .iter()
            .enumerate()
            {
                cpu.gte.write_ctrl(k, *w);
            }
        };
        write_tile(&mut cpu.mem, TILES, &rec);
        seat_render_state(&mut cpu);
        load_matrix(&mut cpu);
        cpu.mem.wr8(FRAME_STEP, 1);
        // A scaled clock at zero holds the record: only the packet runs.
        assert_eq!(cpu.call(EMIT, &[TILES, 0]), Stop::Returned);
        let mut prims = Vec::new();
        emit_tile(&rec, &mut prims);
        let ctx = format!("round {round} record {rec:?}");
        drawn += compare_packets(&mut cpu.mem, &prims, &format!("emitter, {ctx}"));

        if rec.progress < 0x1000 {
            seat_render_state(&mut cpu);
            load_matrix(&mut cpu);
            let object = SCRATCH_BASE + 0x5_DC00 + 0xC;
            let args = [object, 0, rec.progress as i32 as u32];
            assert_eq!(cpu.call(DISPATCH, &args), Stop::Returned);
            compare_packets(&mut cpu.mem, &prims, &format!("dispatcher, {ctx}"));
            faded += usize::from(rec.progress != 0 && !prims.is_empty());
        }
    }
    assert!(drawn > 5000 && faded > 500, "drawn {drawn} faded {faded}");
}

#[test]
fn the_tile_shatter_matches_its_emitter() {
    differential!(prot(979, TICK));
    let Some(mut cpu) = intro_cpu() else {
        return;
    };
    let mut rng = Rng(0x5449_4C45);
    let (mut drawn, mut retired, mut moved) = (0usize, 0usize, 0usize);
    for round in 0..16 {
        let elapsed = 1 + rng.below(0x40) as i16;
        let step = 1 + rng.below(4) as u8;
        let mut grid = TileGrid {
            vertices: Vec::new(),
            tiles: (0..COUNT).map(|_| random_tile(&mut rng)).collect(),
        };
        let filler: Vec<u8> = (0..COUNT * STRIDE as usize)
            .map(|_| rng.next() as u8)
            .collect();
        cpu.mem.load(TILES, &filler);
        for (i, t) in grid.tiles.iter().enumerate() {
            write_tile(&mut cpu.mem, TILES + STRIDE * i as u32, t);
        }
        cpu.mem.wr32(TILES_PTR, TILES);
        seat_render_state(&mut cpu);
        cpu.mem.load(ENTITY, &[0u8; 0x60]);
        cpu.mem.wr16(ENTITY + 0x1A, elapsed as u16);
        for k in 0..8 {
            cpu.mem.wr32(VIEW + 4 * k, 0);
        }
        cpu.mem.wr16(VIEW, 0x1000);
        cpu.mem.wr16(VIEW + 8, 0x1000);
        cpu.mem.wr16(VIEW + 0x10, 0x1000);
        cpu.mem.wr8(FRAME_STEP, step);
        assert_eq!(cpu.call(TICK, &[ENTITY]), Stop::Returned);
        assert!(cpu.calls.contains(&EMIT) && cpu.calls.contains(&DISPATCH));

        let mut clock = elapsed;
        let mut prims = Vec::new();
        tick_tile_grid_emit(&mut grid, &mut clock, step, |rec| {
            emit_tile(rec, &mut prims)
        });
        let ctx = format!("round {round} elapsed {elapsed} step {step}");
        assert_eq!(cpu.mem.rd16(ENTITY + 0x1A) as i16, clock, "clock, {ctx}");

        let mut port_mem = Memory::default();
        port_mem.load(TILES, &filler);
        for (i, t) in grid.tiles.iter().enumerate() {
            write_tile(&mut port_mem, TILES + STRIDE * i as u32, t);
        }
        let want = port_mem.read(TILES, filler.len());
        let got = cpu.mem.read(TILES, want.len());
        assert!(
            got == want,
            "records, {ctx}: {:?}",
            diff_offsets(&got, &want)
        );

        drawn += compare_packets(&mut cpu.mem, &prims, &ctx);
        retired += grid.tiles.iter().filter(|t| t.progress >= 0x1000).count();
        moved += grid
            .tiles
            .iter()
            .filter(|t| i32::from(t.delay) < i32::from(elapsed) * 0x3C)
            .count();
    }
    assert!(
        drawn > 5000 && retired > 100 && moved > 1000,
        "drawn {drawn} retired {retired} moved {moved}"
    );
}
