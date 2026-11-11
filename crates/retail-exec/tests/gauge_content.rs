//! The AP gauge's content `FUN_8002C0B0(x, y, value)`, run live, against
//! `engine-ui`'s `ap_gauge_sprites`: the meter fill - two three-row Gouraud
//! quads, dark at the outer edges and gold on the shared middle one, `value
//! / 2` wide from `x + 0x1B` - and the value readout through the pictogram
//! call `FUN_8002C488` (the `100` glyph, or a tens and a ones cell).
//!
//! The port bakes the fill's ramp into an atlas column and stretches it; what
//! is held is the rect the two quads cover and the two colours the bake
//! reads. Values past `100`, which retail draws as a 255-px fill with no
//! readout, are not generated: the AP a caller hands over is capped there.

use crate::common::{Rng, scus_cpu};
use legaia_engine_ui::{SaveMenuAtlasRects, ap_gauge_sprites};
use legaia_ui_formats::title_pak;
use psx_retail_exec::{A0, A1, A2, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const GAUGE_CONTENT: u32 = 0x8002_C0B0;
const ADD_PRIM: u32 = 0x8003_D2C4;
const PICTO: u32 = 0x8002_C488;
const PACKET_CURSOR: u32 = 0x1F80_03A0;
const OT_BASE: u32 = 0x1F80_03F4;
const PACKETS: u32 = 0x8010_0000;
const OT: u32 = 0x8018_0000;

#[test]
fn the_gauge_content_is_the_fill_and_readout_retail_draws() {
    differential!(scus(GAUGE_CONTENT));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let prims: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    let p = prims.clone();
    cpu.hook(ADD_PRIM, move |r, _| {
        p.borrow_mut().push((r.r[A0], r.r[A1]))
    });
    let pictos: Rc<RefCell<Vec<(i32, i32, u32)>>> = Rc::default();
    let p = pictos.clone();
    cpu.hook(PICTO, move |r, _| {
        p.borrow_mut()
            .push((r.r[A0] as i32, r.r[A1] as i32, r.r[A2]))
    });
    cpu.mem.wr32(OT_BASE, OT);
    let tag = |n: u32| (n, 0, 1, 1);
    let rects = SaveMenuAtlasRects {
        gauge_cap: tag(0x200),
        gauge_trough: tag(0x201),
        gauge_box: tag(0x202),
        gauge_tip: tag(0x203),
        gauge_100: tag(0x6B),
        gauge_digits: (0x300, 0, 6, 6),
        gauge_fill: (0x400, 0, 1, 6),
        ..Default::default()
    };
    let (dark, gold) = (
        title_pak::OVERLAY_SYSTEM_UI_GAUGE_FILL_DARK_RGB,
        title_pak::OVERLAY_SYSTEM_UI_GAUGE_FILL_GOLD_RGB,
    );
    let (dark, gold) = ([dark.0, dark.1, dark.2], [gold.0, gold.1, gold.2]);
    let mut rng = Rng(0x4741_5547);
    let (mut empty, mut full, mut tens) = (0, 0, 0);
    for run in 0..6_000 {
        let ap = match rng.below(6) {
            0 => 0,
            1 => 100,
            _ => rng.below(101),
        };
        let (x, y) = (rng.below(280) as i32, rng.below(220) as i32);
        cpu.mem.wr32(PACKET_CURSOR, PACKETS);
        prims.borrow_mut().clear();
        pictos.borrow_mut().clear();
        assert_eq!(
            cpu.call(GAUGE_CONTENT, &[x as u32, y as u32, ap]),
            Stop::Returned
        );
        let what = format!("run {run}: ap {ap} at ({x}, {y})");
        let port = ap_gauge_sprites(&rects, (x, y), ap as u16, (0, 0), 1);

        // The fill: the rect the two quads cover, and their colours.
        let links = prims.borrow().clone();
        let fill: Vec<_> = port.iter().filter(|s| s.src.0 == 0x400).collect();
        if ap == 0 {
            assert!(links.is_empty() && fill.is_empty(), "{what}");
            empty += 1;
        } else {
            assert_eq!(links.len(), 2, "{what}");
            // At `1` the two quads are linked zero pixels wide; the port
            // draws nothing there.
            assert_eq!(fill.len(), usize::from(ap >= 2), "{what}");
            let mut rows = Vec::new();
            for &(ot, p) in &links {
                assert_eq!(ot, OT + 4, "{what}: slot one");
                assert_eq!(cpu.mem.rd8(p + 7), 0x39, "{what}: a Gouraud quad");
                let v = |o: u32, cpu: &mut psx_retail_exec::Cpu| {
                    (
                        i32::from(cpu.mem.rd16(p + o) as i16),
                        i32::from(cpu.mem.rd16(p + o + 2) as i16),
                    )
                };
                let c = |o: u32, cpu: &mut psx_retail_exec::Cpu| {
                    [
                        cpu.mem.rd8(p + o),
                        cpu.mem.rd8(p + o + 1),
                        cpu.mem.rd8(p + o + 2),
                    ]
                };
                let (tl, tr, bl, br) = (
                    v(8, &mut cpu),
                    v(0x10, &mut cpu),
                    v(0x18, &mut cpu),
                    v(0x20, &mut cpu),
                );
                assert_eq!((tl.1, bl.1, tl.0, tr.0), (tr.1, br.1, bl.0, br.0), "{what}");
                let (top, bottom) = (c(4, &mut cpu), c(0x14, &mut cpu));
                assert_eq!(
                    (top, bottom),
                    (c(0xC, &mut cpu), c(0x1C, &mut cpu)),
                    "{what}"
                );
                rows.push((tl.0, tl.1, tr.0 - tl.0, bl.1 - tl.1, top, bottom));
            }
            // Dark, gold; gold, dark - three rows each, one over the other.
            let (a, b) = (rows[0], rows[1]);
            assert_eq!((a.4, a.5, b.4, b.5), (dark, gold, gold, dark), "{what}");
            assert_eq!(
                (a.3, b.3, b.1, b.0, b.2),
                (3, 3, a.1 + 3, a.0, a.2),
                "{what}"
            );
            assert_eq!(a.2, (ap / 2) as i32, "{what}: the fill's width");
            if let Some(f) = fill.first() {
                assert_eq!(
                    (f.dst.0, f.dst.1, f.dst.2 as i32, f.dst.3 as i32),
                    (a.0, a.1, a.2, a.3 + b.3),
                    "{what}: the fill's rect"
                );
            }
        }
        // The readout.
        let want: Vec<(i32, i32, u32)> = port
            .iter()
            .filter_map(|s| match s.src.0 {
                0x6B => Some((s.dst.0, s.dst.1, 0x6B)),
                n @ 0x300..=0x33F => Some((s.dst.0, s.dst.1, 0x6C + (n - 0x300) / 6)),
                _ => None,
            })
            .collect();
        assert_eq!(*pictos.borrow(), want, "{what}: the readout");
        full += u32::from(ap == 100);
        tens += u32::from(want.len() == 2);
    }
    assert!(
        empty > 500 && full > 500 && tens > 2000,
        "{empty} {full} {tens}"
    );
}
