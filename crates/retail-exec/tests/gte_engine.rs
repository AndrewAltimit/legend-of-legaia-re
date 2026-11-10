#![allow(clippy::needless_range_loop)] // register indices are the subject here
//! The engine's GTE against the interpreter's, which is written from the
//! hardware documentation alone (`psx_retail_exec::gte`). Random register
//! files, both shapes a game reaches (vertices, matrices and colours in their
//! usual ranges) and the extremes that saturate, are written through each
//! side's `ctc2` / `mtc2`, one command runs on each, and the whole register
//! file is read back through `cfc2` / `mfc2`. Needs no disc.

use crate::common::Rng;
use legaia_render_kernels::gte as eg;
use psx_retail_exec::gte::{Gte, unr_divide};

/// The live perspective divide every integer projection kernel shares.
#[test]
fn engine_divide_matches_the_spec_divider() {
    not_a_differential!(
        "holds render-kernels' gte_divide against the spec GTE divider; runs no routine"
    );
    let mut bad = Vec::new();
    for sz in 0..=0xFFFFu32 {
        for h in [
            0u32, 1, 0x80, 0xFF, 0x100, 0x200, 0x260, 0x400, 0x7FFF, 0x8000, 0xFFFF,
        ]
        .into_iter()
        .chain([
            sz.wrapping_mul(2) & 0xFFFF,
            (sz * 2).saturating_sub(1) & 0xFFFF,
        ]) {
            let (q, o) = unr_divide(h as u16, sz as u16);
            let (eq, eo) = eg::gte_divide(h as u16, sz as u16);
            if (i64::from(q), o) != (eq, eo) && bad.len() < 16 {
                bad.push(format!(
                    "h {h:#x} sz {sz:#x}: spec {q:#x}/{o} engine {eq:#x}/{eo}"
                ));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

fn random_word(rng: &mut Rng, wild: bool) -> u32 {
    if wild {
        return rng.next();
    }
    // A halfword pair in a game's ranges: vertices, 4.12 matrix entries.
    let h = |rng: &mut Rng| (rng.below(0x2000) as i32 - 0x1000) as u16 as u32;
    h(rng) | h(rng) << 16
}

fn seed(rng: &mut Rng, wild: bool) -> ([u32; 32], [u32; 32]) {
    let mut d = [0u32; 32];
    let mut c = [0u32; 32];
    for w in d.iter_mut().chain(c.iter_mut()) {
        *w = random_word(rng, wild);
    }
    if !wild {
        // Colours, depths, FIFOs and the projection block in game ranges.
        d[6] = rng.next();
        d[8] = rng.below(0x1001);
        for r in 16..=19 {
            d[r] = rng.below(0x8000);
        }
        for r in [5usize, 6, 7] {
            c[r] = (rng.below(0x4000) as i32 - 0x1000) as u32;
        }
        for r in [13usize, 14, 15, 21, 22, 23] {
            c[r] = rng.below(0x1000);
        }
        c[24] = rng.below(320) << 16;
        c[25] = rng.below(240) << 16;
        c[26] = 0x100 + rng.below(0x300);
        c[27] = (rng.below(0x200) as i32 - 0x100) as u32;
        c[28] = rng.below(0x0200_0000);
        c[29] = rng.below(0x800);
        c[30] = rng.below(0x800);
    }
    c[31] = 0;
    (d, c)
}

/// Run one command both ways from the same register file and list the
/// registers whose reads differ.
fn run(d: &[u32; 32], c: &[u32; 32], cmd: u32, engine: &dyn Fn(&mut eg::Gte)) -> Vec<String> {
    let mut s = Gte::default();
    let mut e = eg::Gte::new();
    for r in 0..32 {
        s.write_ctrl(r, c[r]);
        e.ctc2(r as u8, c[r]);
    }
    for r in 0..32 {
        // SXYP pushes; write the three slots directly instead.
        if r == 15 || r == 28 || r == 29 || r == 31 {
            continue;
        }
        s.write_data(r, d[r]);
        e.mtc2(r as u8, d[r]);
    }
    assert!(s.execute(cmd));
    engine(&mut e);
    let mut out = Vec::new();
    for r in 0..32 {
        let (a, b) = (s.read_data(r), e.mfc2(r as u8));
        if a != b {
            out.push(format!("d{r} spec {a:#x} engine {b:#x}"));
        }
    }
    for r in 0..32 {
        let (a, b) = (s.read_ctrl(r), e.cfc2(r as u8));
        if a != b {
            out.push(format!("c{r} spec {a:#x} engine {b:#x}"));
        }
    }
    out
}

#[test]
fn engine_gte_matches_the_spec_gte() {
    not_a_differential!(
        "holds render-kernels' register-file Gte against the spec GTE; runs no routine"
    );
    type Op = (&'static str, u32, Box<dyn Fn(&mut eg::Gte)>);
    let ops: Vec<Op> = vec![
        (
            "RTPS",
            0x0018_0001,
            Box::new(|g| {
                g.rtps();
            }),
        ),
        (
            "RTPT",
            0x0028_0030,
            Box::new(|g| {
                g.rtpt();
            }),
        ),
        (
            "NCLIP",
            0x0140_0006,
            Box::new(|g| {
                g.nclip();
            }),
        ),
        (
            "AVSZ3",
            0x0158_002D,
            Box::new(|g| {
                g.avsz3();
            }),
        ),
        (
            "AVSZ4",
            0x0168_002E,
            Box::new(|g| {
                g.avsz4();
            }),
        ),
        (
            "NCDS",
            0x00E8_0413,
            Box::new(|g| {
                g.ncds();
            }),
        ),
        (
            "NCDT",
            0x00F8_0416,
            Box::new(|g| {
                g.ncdt();
            }),
        ),
        (
            "NCCS",
            0x0108_041B,
            Box::new(|g| {
                g.nccs();
            }),
        ),
        (
            "NCCT",
            0x0118_043F,
            Box::new(|g| {
                g.ncct();
            }),
        ),
        (
            "NCS",
            0x00C8_041E,
            Box::new(|g| {
                g.ncs();
            }),
        ),
        (
            "NCT",
            0x00D8_0420,
            Box::new(|g| {
                g.nct();
            }),
        ),
        (
            "CDP",
            0x0128_0414,
            Box::new(|g| {
                g.cdp();
            }),
        ),
        (
            "CC",
            0x0138_041C,
            Box::new(|g| {
                g.cc();
            }),
        ),
        (
            "DPCS",
            0x0078_0010,
            Box::new(|g| {
                g.dpcs();
            }),
        ),
        (
            "DPCT",
            0x00F8_002A,
            Box::new(|g| {
                g.dpct();
            }),
        ),
        (
            "DCPL",
            0x0068_0029,
            Box::new(|g| {
                g.dcpl();
            }),
        ),
        ("INTPL", 0x0098_0011, Box::new(|g| g.intpl())),
        ("SQR sf", 0x00A8_0428, Box::new(|g| g.sqr(true))),
        ("OP sf", 0x0178_000C, Box::new(|g| g.op(true))),
        ("GPF sf", 0x0198_003D, Box::new(|g| g.gpf(true))),
        ("GPL sf", 0x01A8_003E, Box::new(|g| g.gpl(true))),
        (
            "MVMVA rt*v0+tr",
            0x0048_0012,
            Box::new(|g| {
                let (m, v, t) = (g.rot, g.v[0], g.trans);
                g.mvmva(&m, v, t, true, false)
            }),
        ),
    ];
    // The register-file `Gte` keeps RTPS / RTPT in its own q19.12 scale
    // (vertices and TR pre-multiplied by 4096, OFX in whole pixels) and its
    // colour ops do not follow the hardware's MAC / IR / FIFO steps; only
    // the `gte-replay` tool runs it. These commands must agree exactly; the
    // rest are reported.
    const EXACT: [&str; 6] = [
        "NCLIP",
        "AVSZ3",
        "AVSZ4",
        "SQR sf",
        "OP sf",
        "MVMVA rt*v0+tr",
    ];
    let mut rng = Rng(0x6754_4521);
    let mut report = Vec::new();
    let mut known = Vec::new();
    for (name, cmd, f) in &ops {
        let mut fails = 0;
        let mut first = None;
        for i in 0..400 {
            let (d, c) = seed(&mut rng, i % 4 == 3);
            let diff = run(&d, &c, *cmd, f.as_ref());
            if !diff.is_empty() {
                fails += 1;
                first.get_or_insert(diff);
            }
        }
        if fails == 0 {
            continue;
        }
        let line = format!("{name}: {fails}/400, e.g. {:?}", first.unwrap());
        if EXACT.contains(name) {
            report.push(line);
        } else {
            known.push(*name);
        }
    }
    eprintln!("register-file Gte off the spec GTE on: {known:?}");
    assert!(report.is_empty(), "{}", report.join("\n"));
}

/// The light-source rows' shading kernel `field_light::nccs` (live on both
/// hosts) against the spec GTE running `NCCS` (`cop2 0x108041B`) on the same
/// light matrix, colour matrix, back colour, primary colour and normal.
#[test]
fn field_light_nccs_matches_the_spec_gte() {
    not_a_differential!(
        "holds render-kernels' field_light::nccs against the spec GTE's NCCS; runs no routine"
    );
    use legaia_render_kernels::field_light::{LIGHT_COLOUR_MATRIX, nccs};
    let mut rng = Rng(0x4E43_4353);
    let pack = |a: i32, b: i32| (a as u16 as u32) | (b as u16 as u32) << 16;
    let mut bad = Vec::new();
    for i in 0..20_000 {
        let any = |rng: &mut Rng| rng.next() as i16 as i32;
        let unit = |rng: &mut Rng| rng.below(0x2001) as i32 - 0x1000;
        let light: [[i32; 3]; 3] = std::array::from_fn(|_| {
            std::array::from_fn(|_| {
                if i % 5 == 0 {
                    any(&mut rng)
                } else {
                    unit(&mut rng)
                }
            })
        });
        let colour = if i % 2 == 0 {
            LIGHT_COLOUR_MATRIX
        } else {
            std::array::from_fn(|_| std::array::from_fn(|_| unit(&mut rng)))
        };
        let back = [0, 1, 2].map(|_| rng.next() as u8);
        let rgbc = [0, 1, 2].map(|_| rng.next() as u8);
        let normal = [0, 1, 2].map(|_| {
            if i % 5 == 1 {
                rng.next() as i16
            } else {
                unit(&mut rng) as i16
            }
        });

        let mut g = Gte::default();
        let m = |g: &mut Gte, base: usize, m: &[[i32; 3]; 3]| {
            let e: Vec<i32> = m.iter().flatten().copied().collect();
            for k in 0..4 {
                g.write_ctrl(base + k, pack(e[2 * k], e[2 * k + 1]));
            }
            g.write_ctrl(base + 4, e[8] as u16 as u32);
        };
        m(&mut g, 8, &light);
        m(&mut g, 16, &colour);
        for (k, b) in back.iter().enumerate() {
            g.write_ctrl(13 + k, u32::from(*b) << 4);
        }
        g.write_data(0, pack(i32::from(normal[0]), i32::from(normal[1])));
        g.write_data(1, normal[2] as u16 as u32);
        g.write_data(
            6,
            u32::from(rgbc[0]) | u32::from(rgbc[1]) << 8 | u32::from(rgbc[2]) << 16,
        );
        assert!(g.execute(0x0108_041B));
        let w = g.read_data(22);
        let spec = [w as u8, (w >> 8) as u8, (w >> 16) as u8];
        let port = nccs(&light, &colour, back, rgbc, normal);
        if spec != port && bad.len() < 8 {
            bad.push(format!("light {light:?} colour {colour:?} back {back:?} rgbc {rgbc:?} n {normal:?}: spec {spec:?} port {port:?}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
