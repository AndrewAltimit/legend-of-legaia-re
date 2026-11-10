//! PROT 0904's (Theeder) four beam packet builders against
//! `legaia_engine_ui::cast_theeder`: the prong `FUN_801F815C`, the charge
//! beam `FUN_801F83A4`, the sweep `FUN_801F8634` and the retract
//! `FUN_801F8B84`. The billboard projector `FUN_800195A8` is hooked to hand
//! back a generated screen point and depth per world point (the port takes
//! projected points as input), `rand()` is scripted, and every packet the
//! builder writes is decoded and held to the port's primitive: code,
//! corners, colours, texels, CLUT and page. The sweep's ring shift is held
//! to `engine-cast`'s `TheederTrail::sample` on a pass boundary.
//!
//! The ordering-table bucket is not compared: the port draws the overlay's
//! packets over the finished scene at fixed entries (disclosed in
//! `cast_theeder`), where retail links the depth-averaged ones at their
//! ends' mean depth.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_cast::seru_magic_ticks::TheederTrail;
use legaia_engine_ui::cast_theeder::{
    Xy, charge_prims, prong_prim_widened, prong_widen, theeder_prims,
};
use legaia_render_kernels::cast_theeder_packet::TheederPacket;
use legaia_render_kernels::screen_prim::ScreenPrim;
use psx_retail_exec::{A0, Cpu, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const MODULE_BASE: u32 = 0x801F_69D8;
const PRONG: u32 = 0x801F_815C;
const CHARGE: u32 = 0x801F_83A4;
const SWEEP: u32 = 0x801F_8634;
const RETRACT: u32 = 0x801F_8B84;
const PROJECT: u32 = 0x8001_95A8;
const RAND: u32 = 0x8005_6798;
const POOL_PTR: u32 = 0x1F80_03A0;
const OT_PTR: u32 = 0x1F80_03F4;
const POOL: u32 = 0x8012_0000;
const OT: u32 = 0x8014_0000;
const OT_LEN: u32 = 0x1400;
/// The ring of trail tips (`hist[0]` newest) and its count word.
const RING: u32 = 0x801F_90FC;
const COUNT: u32 = 0x801F_91FC;
const POINT_A: u32 = 0x8011_0000;
const POINT_B: u32 = 0x8011_0010;

/// The screen point and depth the hooked projector gives a world point.
fn project(p: [i16; 3]) -> (Xy, u32) {
    let xy = (p[0] >> 3, p[2] >> 4);
    // Mostly far (no prong widening), now and then within `0x100`.
    let z = (p[1] as u16 as u32) & 0x3FF;
    (xy, z)
}

fn module_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem.load(
        MODULE_BASE,
        &disc().expect("disc").read_entry(904).expect("PROT 0904"),
    );
    cpu.hook(PROJECT, |r, m| {
        let a = r.r[A0];
        let p = [m.rd16(a) as i16, m.rd16(a + 2) as i16, m.rd16(a + 4) as i16];
        let ((x, y), z) = project(p);
        let out = m.rd32(r.r[SP] + 0x10);
        m.wr16(out, x as u16);
        m.wr16(out + 2, y as u16);
        r.r[V0] = z;
    });
    Some(cpu)
}

fn reset_pool(cpu: &mut Cpu) {
    cpu.mem.wr32(POOL_PTR, POOL);
    cpu.mem.wr32(OT_PTR, OT);
    cpu.mem.load(POOL, &[0xEE; 0x400]);
    for i in 0..OT_LEN {
        cpu.mem.wr32(OT + 4 * i, 0x00FF_FFFF);
    }
}

fn point(rng: &mut Rng) -> [i16; 3] {
    [
        (rng.next() as i16) >> 3,
        rng.next() as i16,
        (rng.next() as i16) >> 3,
    ]
}

fn put(cpu: &mut Cpu, at: u32, p: [i16; 3]) {
    for (k, v) in p.iter().enumerate() {
        cpu.mem.wr16(at + 2 * k as u32, *v as u16);
    }
    cpu.mem.wr16(at + 6, 0xA5A5);
}

/// A packet as decoded, for both sides.
#[derive(Debug, PartialEq, Eq)]
struct Packet {
    code: u8,
    xy: [(i16, i16); 4],
    rgb: [[u8; 3]; 4],
    uv: Option<([(u8, u8); 4], u16, u16)>,
}

/// Every packet written since [`reset_pool`]: `POLY_FT4` (`0x2C..0x2F`, ten
/// words) or `POLY_G4` (`0x38..0x3B`, nine).
fn retail_packets(cpu: &mut Cpu) -> Vec<Packet> {
    let end = cpu.mem.rd32(POOL_PTR);
    let mut p = POOL;
    let mut out = Vec::new();
    while p < end {
        let code = cpu.mem.rd8(p + 7);
        let words = cpu.mem.rd8(p + 3);
        let xy = |m: &mut psx_retail_exec::Memory, o: u32| (m.rd16(o) as i16, m.rd16(o + 2) as i16);
        let rgb = |m: &mut psx_retail_exec::Memory, o: u32| [m.rd8(o), m.rd8(o + 1), m.rd8(o + 2)];
        if code & 0xFC == 0x2C {
            assert_eq!(words, 9, "an FT4 is nine words");
            let c = rgb(&mut cpu.mem, p + 4);
            out.push(Packet {
                code,
                xy: std::array::from_fn(|v| xy(&mut cpu.mem, p + 8 + 8 * v as u32)),
                rgb: [c; 4],
                uv: Some((
                    std::array::from_fn(|v| {
                        let o = p + 0xC + 8 * v as u32;
                        (cpu.mem.rd8(o), cpu.mem.rd8(o + 1))
                    }),
                    cpu.mem.rd16(p + 0xE),
                    cpu.mem.rd16(p + 0x16),
                )),
            });
            p += 0x28;
        } else {
            assert_eq!(code & 0xFC, 0x38, "packet code {code:#x}");
            assert_eq!(words, 8, "a G4 is eight words");
            out.push(Packet {
                code,
                xy: std::array::from_fn(|v| xy(&mut cpu.mem, p + 8 + 8 * v as u32)),
                rgb: std::array::from_fn(|v| rgb(&mut cpu.mem, p + 4 + 8 * v as u32)),
                uv: None,
            });
            p += 0x24;
        }
    }
    assert_eq!(p, end, "the pool ends on a packet");
    out
}

fn port_packet(prim: &ScreenPrim) -> Packet {
    match prim {
        ScreenPrim::Textured(q) => {
            assert!(q.gouraud.is_none());
            let c = q.color.to_le_bytes();
            // `0x2F`: textured, semi-transparent, raw texture (`0x808080`
            // modulation is the identity either way).
            assert_eq!(q.color, 0x80_80_80);
            Packet {
                code: 0x2C | u8::from(q.semi_transparent) << 1 | 1,
                xy: q.xy,
                rgb: [[c[2], c[1], c[0]]; 4],
                uv: Some((q.uv, q.clut, q.tpage)),
            }
        }
        ScreenPrim::Flat(f) => {
            let g = f.gouraud.expect("every Theeder G4 is gouraud");
            // `0x3B`: retail sets the raw-texture bit on these untextured
            // quads too, where the GPU ignores it.
            Packet {
                code: 0x38 | u8::from(f.semi_transparent) << 1 | 1,
                xy: f.xy,
                rgb: g.map(|c| [c[0], c[1], c[2]]),
                uv: None,
            }
        }
    }
}

fn scripted_rand(cpu: &mut Cpu, seed: u64) -> Rc<RefCell<(Rng, Vec<u32>)>> {
    let draws: Rc<RefCell<(Rng, Vec<u32>)>> = Rc::new(RefCell::new((Rng(seed), Vec::new())));
    let d = draws.clone();
    cpu.hook(RAND, move |r, _| {
        let mut g = d.borrow_mut();
        let v = g.0.next() & 0x7FFF;
        g.1.push(v);
        r.r[V0] = v;
    });
    draws
}

#[test]
fn the_prong_matches_the_module() {
    differential!(prot(904, PRONG));
    let Some(mut cpu) = module_cpu() else {
        return;
    };
    let draws = scripted_rand(&mut cpu, 0x5052_4F4E);
    let mut rng = Rng(0x7468_6565);
    let mut widened = 0;
    for case in 0..3000 {
        let (a, b) = (point(&mut rng), point(&mut rng));
        put(&mut cpu, POINT_A, a);
        put(&mut cpu, POINT_B, b);
        reset_pool(&mut cpu);
        draws.borrow_mut().1.clear();
        assert_eq!(
            cpu.call(PRONG, &[POINT_A, POINT_B]),
            Stop::Returned,
            "case {case}"
        );
        let drawn = std::mem::take(&mut draws.borrow_mut().1);
        assert_eq!(drawn.len(), 2, "two draws, the second discarded");
        let ((pa, za), (pb, zb)) = (project(a), project(b));
        let (wa, wb) = (prong_widen(za), prong_widen(zb));
        widened += usize::from(wa != 0 || wb != 0);
        let want = port_packet(&prong_prim_widened(pa, wa, pb, wb, (drawn[0] & 3) as u8));
        let got = retail_packets(&mut cpu);
        assert_eq!(got, vec![want], "case {case}: a {a:?} b {b:?}");
    }
    assert!(widened > 100, "the near-camera arm ran {widened} times");
}

#[test]
fn the_charge_beam_matches_the_module() {
    differential!(prot(904, CHARGE));
    let Some(mut cpu) = module_cpu() else {
        return;
    };
    let mut rng = Rng(0x6368_6172);
    for case in 0..3000 {
        let (a, b) = (point(&mut rng), point(&mut rng));
        let level = rng.below(0x200) as u16;
        put(&mut cpu, POINT_A, a);
        put(&mut cpu, POINT_B, b);
        reset_pool(&mut cpu);
        assert_eq!(
            cpu.call(CHARGE, &[POINT_A, POINT_B, u32::from(level)]),
            Stop::Returned,
            "case {case}"
        );
        let want: Vec<Packet> = charge_prims(project(a).0, project(b).0, level)
            .iter()
            .map(port_packet)
            .collect();
        assert_eq!(
            retail_packets(&mut cpu),
            want,
            "case {case}: level {level:#x}"
        );
    }
}

fn seed_ring(cpu: &mut Cpu, rng: &mut Rng, n: u32) -> [[i16; 3]; 16] {
    let ring: [[i16; 3]; 16] = std::array::from_fn(|_| point(rng));
    for (i, p) in ring.iter().enumerate() {
        put(cpu, RING + 8 * i as u32, *p);
    }
    cpu.mem.wr32(COUNT, n);
    ring
}

fn read_ring(cpu: &mut Cpu) -> Vec<[i16; 3]> {
    (0..16u32)
        .map(|i| {
            let at = RING + 8 * i;
            [
                cpu.mem.rd16(at) as i16,
                cpu.mem.rd16(at + 2) as i16,
                cpu.mem.rd16(at + 4) as i16,
            ]
        })
        .collect()
}

fn port_fan(packet: &TheederPacket, trail: &[[i16; 3]]) -> Vec<Packet> {
    theeder_prims(packet, trail, |p| Some(project(p).0))
        .iter()
        .map(port_packet)
        .collect()
}

#[test]
fn the_sweep_matches_the_module() {
    differential!(prot(904, SWEEP));
    let Some(mut cpu) = module_cpu() else {
        return;
    };
    let mut rng = Rng(0x7377_6565);
    let mut full = 0;
    for case in 0..2000 {
        let (a, b) = (point(&mut rng), point(&mut rng));
        let n = rng.below(16);
        let before = seed_ring(&mut cpu, &mut rng, n);
        put(&mut cpu, POINT_A, a);
        put(&mut cpu, POINT_B, b);
        reset_pool(&mut cpu);
        assert_eq!(
            cpu.call(SWEEP, &[POINT_A, POINT_B]),
            Stop::Returned,
            "case {case}"
        );

        // The ring half, as the port's trail takes it on a pass boundary.
        let mut trail = TheederTrail::default();
        trail.hist = before;
        trail.n = n as usize;
        let drawn = trail.sample(b);
        let ring = read_ring(&mut cpu);
        assert_eq!(&ring[..], &trail.hist[..], "case {case}: the ring");
        assert_eq!(
            cpu.mem.rd32(COUNT),
            trail.n as u32,
            "case {case}: the count"
        );
        full += usize::from(n == 15);

        let want = port_fan(
            &TheederPacket::Sweep {
                mouth: a,
                tip: b,
                drawn,
            },
            &ring,
        );
        assert_eq!(retail_packets(&mut cpu), want, "case {case}: n {n}");
    }
    assert!(full > 50, "the capped count ran {full} times");
}

#[test]
fn the_retract_matches_the_module() {
    differential!(prot(904, RETRACT));
    let Some(mut cpu) = module_cpu() else {
        return;
    };
    let mut rng = Rng(0x7265_7472);
    for case in 0..2000 {
        let a = point(&mut rng);
        // Arm 13 runs while the count it returns is non-zero
        // (`bne v0,zero` at `0x801F7FBC`), and the sweep before it leaves at
        // least one, so a zero count never reaches it (retail would wrap it).
        let n = 1 + rng.below(15);
        let before = seed_ring(&mut cpu, &mut rng, n);
        put(&mut cpu, POINT_A, a);
        reset_pool(&mut cpu);
        assert_eq!(cpu.call(RETRACT, &[POINT_A]), Stop::Returned, "case {case}");
        let left = cpu.regs.r[V0];

        let mut trail = TheederTrail::default();
        trail.hist = before;
        trail.n = n as usize;
        let drawn = trail.drawn();
        trail.retract();
        assert_eq!(
            read_ring(&mut cpu),
            before.to_vec(),
            "case {case}: the ring stands"
        );
        assert_eq!(
            cpu.mem.rd32(COUNT),
            trail.n as u32,
            "case {case}: the count"
        );
        assert_eq!(left, trail.n as u32, "case {case}: what is left");

        let want = port_fan(&TheederPacket::Retract { mouth: a, drawn }, &before);
        assert_eq!(retail_packets(&mut cpu), want, "case {case}: n {n}");
    }
}
