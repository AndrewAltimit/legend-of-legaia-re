//! PROT 0910's (Swordie) screen-space builders off the disc, against
//! `legaia_render_kernels::cast_swordie`: the split panels `FUN_801F7C18`
//! over random panel tables, and the slash streak `FUN_801F7DD0` over every
//! split counter. The packets are read back off the ordering table, in
//! `AddPrim` order per entry: the code byte (and its semi-transparency
//! bit), every corner and every corner colour.

use crate::common::{Rng, disc, scus_cpu};
use legaia_render_kernels::cast_swordie::{
    SWORDIE_SPLIT_OT, SWORDIE_STREAK_OT, swordie_split_prims, swordie_streak_prims,
};
use legaia_render_kernels::screen_prim::ScreenPrim;
use psx_retail_exec::{Cpu, Memory, Stop};

const SLOT_B: u32 = 0x801F_69D8;
const SPLIT: u32 = 0x801F_7C18;
const STREAK: u32 = 0x801F_7DD0;
const PANELS: u32 = 0x801F_8D6C;
const COUNTER: u32 = 0x801F_8D8C;
const POOL_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const POOL: u32 = 0x8011_0000;
const OT: u32 = 0x8014_0000;
const END: u32 = 0x00FF_FFFF;

/// One packet as the port describes it: semi-transparent, four corners
/// (a triangle's last repeated), four colours.
type Packet = (bool, [(i16, i16); 4], [[u8; 3]; 4]);

fn swordie_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem.load(SLOT_B, &disc()?.read_entry(910).ok()?);
    Some(cpu)
}

fn reset(cpu: &mut Cpu) {
    cpu.mem.wr32(POOL_PTR, POOL);
    cpu.mem.wr32(OT_PTR, OT);
    for e in 0..8 {
        cpu.mem.wr32(OT + 4 * e, END);
    }
}

/// The packets hanging off OT entry `e`, oldest link first.
fn linked(m: &mut Memory, e: u32) -> Vec<Packet> {
    let mut out = Vec::new();
    let mut at = m.rd32(OT + 4 * e) & 0x00FF_FFFF;
    while at != END {
        let p = 0x8000_0000 | at;
        let code = m.rd8(p + 7);
        let tri = code & 0xFC == 0x30;
        assert!(
            tri || code & 0xFC == 0x38,
            "a gouraud polygon, code {code:#x}"
        );
        let n = if tri { 3 } else { 4 };
        let corner = |m: &mut Memory, v: u32| -> ((i16, i16), [u8; 3]) {
            let c = m.read(p + 4 + 8 * v, 3);
            let xy = (m.rd16(p + 8 + 8 * v) as i16, m.rd16(p + 10 + 8 * v) as i16);
            (xy, [c[0], c[1], c[2]])
        };
        let mut xy = [(0, 0); 4];
        let mut rgb = [[0u8; 3]; 4];
        for v in 0..4 {
            let (a, b) = corner(m, v.min(n - 1));
            xy[v as usize] = a;
            rgb[v as usize] = b;
        }
        out.push((code & 2 != 0, xy, rgb));
        at = m.rd32(p) & 0x00FF_FFFF;
    }
    out.reverse();
    out
}

fn port_packets(prims: &[ScreenPrim], ot: u32) -> Vec<Packet> {
    prims
        .iter()
        .map(|p| {
            let ScreenPrim::Flat(q) = p else {
                panic!("a gouraud polygon")
            };
            assert_eq!(q.ot_index, ot);
            let g = q.gouraud.expect("gouraud");
            (q.semi_transparent, q.xy, g.map(|c| [c[0], c[1], c[2]]))
        })
        .collect()
}

#[test]
fn the_swordie_split_panels_match_the_module() {
    differential!(prot(910, SPLIT));
    let Some(mut cpu) = swordie_cpu() else {
        return;
    };
    let mut rng = Rng(0x5350_4C54);
    for case in 0..500 {
        let panels: [[i16; 8]; 2] =
            std::array::from_fn(|_| std::array::from_fn(|_| rng.next() as i16 >> 6));
        for (k, t) in panels.iter().enumerate() {
            for (i, v) in t.iter().enumerate() {
                cpu.mem
                    .wr16(PANELS + 0x10 * k as u32 + 2 * i as u32, *v as u16);
            }
        }
        reset(&mut cpu);
        assert_eq!(cpu.call(SPLIT, &[]), Stop::Returned);
        let want = port_packets(&swordie_split_prims(&panels), SWORDIE_SPLIT_OT);
        assert_eq!(linked(&mut cpu.mem, SWORDIE_SPLIT_OT), want, "case {case}");
        assert_eq!(cpu.mem.rd32(POOL_PTR) - POOL, 0x24 * 2);
    }
}

#[test]
fn the_swordie_slash_streak_matches_the_module() {
    differential!(prot(910, STREAK));
    let Some(mut cpu) = swordie_cpu() else {
        return;
    };
    for c in 0..0x200u32 {
        cpu.mem.wr32(COUNTER, c);
        reset(&mut cpu);
        assert_eq!(cpu.call(STREAK, &[]), Stop::Returned);
        let port = swordie_streak_prims(c);
        let want = port_packets(&port, SWORDIE_STREAK_OT);
        assert_eq!(
            linked(&mut cpu.mem, SWORDIE_STREAK_OT),
            want,
            "counter {c:#x}"
        );
        // Two quads always; two triangles while the head runs.
        let tris = if c < 0xE { 2 } else { 0 };
        assert_eq!(
            cpu.mem.rd32(POOL_PTR) - POOL,
            0x24 * 2 + 0x1C * tris,
            "counter {c:#x}"
        );
    }
}
