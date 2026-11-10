//! PROT 0898's single-quad afterimage streak `FUN_801E1AB0` against
//! `legaia_render_kernels::afterimage::build_afterimage_quad`: the corner
//! builder `FUN_800195A8` is hooked to hand back generated corners (the
//! port takes them as input), `rand()` is recorded and replayed, and the
//! whole `POLY_FT4` is compared - jittered corners, texels, CLUT, page, code
//! and colour word.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_render_kernels::afterimage::build_afterimage_quad;
use psx_retail_exec::{SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BASE: u32 = 0x801C_E818;
const QUAD: u32 = 0x801E_1AB0;
const CORNERS: u32 = 0x8001_95A8;
const RAND: u32 = 0x8005_6798;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8010_0000;
const POOL_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const POOL: u32 = 0x8012_0000;
const OT: u32 = 0x8013_0000;

#[test]
fn the_afterimage_quad_matches_the_overlay() {
    differential!(prot(898, QUAD));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        BASE,
        &disc().expect("disc").read_entry(898).expect("PROT 0898"),
    );
    cpu.mem.wr32(CTX_PTR, CTX);
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
    let draws: Rc<RefCell<(Rng, Vec<u32>)>> = Rc::new(RefCell::new((Rng(0x4146_5452), Vec::new())));
    let d = draws.clone();
    cpu.hook(RAND, move |r, _| {
        let mut g = d.borrow_mut();
        let v = g.0.next() & 0x7FFF;
        g.1.push(v);
        r.r[V0] = v;
    });
    let mut rng = Rng(0x5354_524B);
    for case in 0..3000 {
        let quad: [(i16, i16); 4] =
            std::array::from_fn(|_| (rng.next() as i16 >> 5, rng.next() as i16 >> 5));
        *corners.borrow_mut() = quad;
        let trail = rng.next();
        cpu.mem.wr32(POOL_PTR, POOL);
        cpu.mem.wr32(OT_PTR, OT);
        cpu.mem.load(POOL, &[0xEE; 0x30]);
        draws.borrow_mut().1.clear();
        assert_eq!(cpu.call(QUAD, &[trail]), Stop::Returned, "case {case}");
        let recorded = std::mem::take(&mut draws.borrow_mut().1);
        assert_eq!(recorded.len(), 9, "nine draws");
        let mut it = recorded.into_iter();
        let q = build_afterimage_quad(quad, trail as u8, || it.next().unwrap());

        let mut want = cpu.mem.read(POOL, 0x30);
        let code = 0x2C | u32::from(q.semi_transparent) << 1;
        want[4..8].copy_from_slice(&(q.color | code << 24).to_le_bytes());
        for (k, ((x, y), (u, v))) in q.xy.iter().zip(q.uv).enumerate() {
            let at = 8 + 8 * k;
            want[at..at + 2].copy_from_slice(&x.to_le_bytes());
            want[at + 2..at + 4].copy_from_slice(&y.to_le_bytes());
            want[at + 4] = u;
            want[at + 5] = v;
        }
        want[0xE..0x10].copy_from_slice(&q.clut.to_le_bytes());
        want[0x16..0x18].copy_from_slice(&q.tpage.to_le_bytes());
        let mut retail = cpu.mem.read(POOL, 0x30);
        // The tag word is the ordering table's.
        retail[..4].copy_from_slice(&want[..4]);
        assert!(
            retail == want,
            "case {case}: {:?}",
            diff_offsets(&retail, &want)
        );
        assert_eq!(cpu.mem.rd32(POOL_PTR), POOL + 0x28);
    }
}
