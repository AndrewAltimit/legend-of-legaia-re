//! PROT 0898's chained streak ribbon `FUN_801E1D98` against
//! `legaia_render_kernels::afterimage::build_streak_ribbon`: the corner
//! builder `FUN_800195A8` hooked to hand back generated bottom corners,
//! `rand()` recorded and replayed, `AddPrim` `FUN_8003D2C4` hooked to record
//! each segment linked. Compared: the segments linked, in order, each
//! `POLY_FT4` whole - jittered corners, texels, CLUT, page, code and colour.

use crate::common::{Rng, disc, scus_cpu};
use legaia_render_kernels::afterimage::{AfterimageQuad, build_streak_ribbon};
use psx_retail_exec::{A1, Memory, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BASE: u32 = 0x801C_E818;
const RIBBON: u32 = 0x801E_1D98;
const CORNERS: u32 = 0x8001_95A8;
const RAND: u32 = 0x8005_6798;
const ADD_PRIM: u32 = 0x8003_D2C4;
const POOL_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const RECT: u32 = 0x8010_0000;
const POOL: u32 = 0x8012_0000;
const OT: u32 = 0x8013_0000;

/// A `POLY_FT4` as compared: code+colour, per corner `xy` and `uv`, the CLUT
/// and the page.
type Ft4 = (u32, [(i16, i16); 4], [(u8, u8); 4], u16, u16);

/// A packet read off the pool.
fn packet(m: &mut Memory, p: u32) -> Ft4 {
    let xy = std::array::from_fn(|k| {
        let at = p + 8 + 8 * k as u32;
        (m.rd16(at) as i16, m.rd16(at + 2) as i16)
    });
    let uv = std::array::from_fn(|k| {
        let at = p + 12 + 8 * k as u32;
        (m.rd8(at), m.rd8(at + 1))
    });
    (m.rd32(p + 4), xy, uv, m.rd16(p + 0xE), m.rd16(p + 0x16))
}

fn port_packet(q: &AfterimageQuad) -> Ft4 {
    let code = 0x2C | u32::from(q.semi_transparent) << 1;
    (q.color | code << 24, q.xy, q.uv, q.clut, q.tpage)
}

#[test]
fn the_streak_ribbon_matches_the_overlay() {
    differential!(prot(898, RIBBON));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        BASE,
        &disc().expect("disc").read_entry(898).expect("PROT 0898"),
    );
    let corners: Rc<RefCell<[(i16, i16); 4]>> = Rc::default();
    let c = corners.clone();
    cpu.hook(CORNERS, move |r, m| {
        for (k, (x, y)) in c.borrow().iter().enumerate() {
            let out = m.rd32(r.r[SP] + 0x10 + 4 * k as u32);
            m.wr16(out, *x as u16);
            m.wr16(out + 2, *y as u16);
        }
        r.r[V0] = 3;
    });
    let draws: Rc<RefCell<(Rng, Vec<u32>)>> = Rc::new(RefCell::new((Rng(0x5249_4242), Vec::new())));
    let d = draws.clone();
    cpu.hook(RAND, move |r, _| {
        let mut g = d.borrow_mut();
        let v = g.0.next() & 0x7FFF;
        g.1.push(v);
        r.r[V0] = v;
    });
    let linked: Rc<RefCell<Vec<u32>>> = Rc::default();
    let l = linked.clone();
    cpu.hook(ADD_PRIM, move |r, _| l.borrow_mut().push(r.r[A1]));
    let mut rng = Rng(0x5354_5245);
    let (mut chains, mut suppressed, mut segments) = (0usize, 0usize, 0usize);
    for case in 0..3000 {
        // A bottom billboard: a top edge, a height, a little skew.
        let x0 = rng.below(0x140) as i16 - 0x20;
        let w = if rng.below(6) == 0 {
            0x41 + rng.below(0x80) as i16
        } else {
            rng.below(0x48) as i16
        };
        let y0 = rng.below(0xF0) as i16 - 0x20;
        let h = rng.below(0xA0) as i16 - 0x10;
        let quad = [
            (x0, y0),
            (x0 + w, y0 + rng.below(5) as i16 - 2),
            (x0 + rng.below(9) as i16 - 4, y0 + h),
            (x0 + w + rng.below(9) as i16 - 4, y0 + h),
        ];
        *corners.borrow_mut() = quad;
        let trail = rng.next() as u8;
        cpu.mem.load(RECT, &[0; 8]);
        cpu.mem.wr32(POOL_PTR, POOL);
        cpu.mem.wr32(OT_PTR, OT);
        draws.borrow_mut().1.clear();
        linked.borrow_mut().clear();
        assert_eq!(
            cpu.call(RIBBON, &[RECT, u32::from(trail)]),
            Stop::Returned,
            "case {case}"
        );
        let recorded = std::mem::take(&mut draws.borrow_mut().1);
        let n = recorded.len();
        let mut it = recorded.into_iter();
        let port = build_streak_ribbon(quad, trail, || it.next().expect("the port drew more"));
        assert_eq!(
            it.next(),
            None,
            "case {case}: retail drew {n}, the port fewer"
        );
        let retail: Vec<_> = linked
            .borrow()
            .iter()
            .map(|&p| packet(&mut cpu.mem, p))
            .collect();
        let want: Vec<_> = port.iter().map(port_packet).collect();
        assert_eq!(retail, want, "case {case}: {quad:?}");
        if port.is_empty() {
            suppressed += 1;
        } else {
            chains += 1;
            segments += port.len();
        }
    }
    assert!(
        chains > 1000 && suppressed > 100 && segments > chains,
        "{chains} {suppressed} {segments}"
    );
}
