//! The fishing venue's sky strip `FUN_801D24EC` in PROT 0972, whole, against
//! `sky_strip`: the six `POLY_FT4` packets it links behind the venue (their
//! corners, texels, CLUT and page words), the clear colour it stores into
//! both draw environments, and the yaw global it zeroes for its own view
//! rebuild and puts back. The horizon point's projection is scripted.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigame_scenes::fishing_scene::{SKY_CLEAR_RGB, sky_strip};
use legaia_minigame_formats::fishing_species as fs;
use psx_retail_exec::{A0, A1, Stop};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const SKY: u32 = 0x801D_24EC;
const VIEW_REBUILD: u32 = 0x8001_72C0;
const SET_TRANS: u32 = 0x8003_D1EC;
const PROJECT: u32 = 0x8003_D368;
const ADD_PRIM: u32 = 0x8003_D2C4;
const YAW: u32 = 0x8007_B792;
const FOCUS_X: u32 = 0x8008_9118;
const EYE: u32 = 0x8008_40B8;
/// The two draw environments' `r0`.
const CLEAR: [u32; 2] = [0x8007_BF44 + 0x19, 0x8007_BF44 + 0x74 + 0x19];
const CTX: u32 = 0x1F80_0314;
const PACKETS: u32 = 0x8010_0000;
const OT: u32 = 0x8018_0000;

#[test]
fn the_sky_strip_is_laid_as_retail_lays_it() {
    differential!(prot(972, SKY));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    let horizon = Rc::new(Cell::new((0i16, 0i16)));
    let yaw_seen = Rc::new(RefCell::new(Vec::<i16>::new()));
    let linked = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let y = yaw_seen.clone();
    cpu.hook(VIEW_REBUILD, move |_, m| {
        y.borrow_mut().push(m.rd16(YAW) as i16)
    });
    cpu.hook(SET_TRANS, |r, _| {
        assert_eq!(r.r[A0], EYE, "the eye trio as TR")
    });
    let h = horizon.clone();
    cpu.hook(PROJECT, move |r, m| {
        // The point straight ahead of the camera.
        let v = r.r[A0];
        assert_eq!(
            [m.rd16(v), m.rd16(v + 2), m.rd16(v + 4)],
            [0, 0, 0x1000],
            "the horizon point"
        );
        let (sx, sy) = h.get();
        m.wr16(r.r[A1], sx as u16);
        m.wr16(r.r[A1] + 2, sy as u16);
    });
    let l = linked.clone();
    cpu.hook(ADD_PRIM, move |r, _| {
        l.borrow_mut().push((r.r[A0], r.r[A1]))
    });
    cpu.mem.wr32(CTX + 0xE0, OT);
    let mut rng = Rng(0x534B_5921);
    for round in 0..5000 {
        let (sx, sy) = (rng.below(640) as i16 - 160, rng.below(480) as i16 - 120);
        let focus = rng.below(0x4000) as i32 - 0x2000;
        let yaw = -(rng.below(0x1000) as i16);
        horizon.set((sx, sy));
        cpu.mem.wr32(FOCUS_X, focus as u32);
        cpu.mem.wr16(YAW, yaw as u16);
        cpu.mem.wr32(CTX + 0x8C, PACKETS);
        for a in CLEAR {
            cpu.mem.load(a, &[0xEE; 3]);
        }
        yaw_seen.borrow_mut().clear();
        linked.borrow_mut().clear();

        assert_eq!(cpu.call(SKY, &[]), Stop::Returned);

        let ctx = format!("round {round} horizon ({sx}, {sy}) focus {focus} yaw {yaw}");
        let quads = sky_strip(i32::from(sx), i32::from(sy), focus, yaw);
        let links = linked.borrow().clone();
        assert_eq!(links.len(), quads.len(), "{ctx}");
        for (i, (q, (ot, p))) in quads.iter().zip(links).enumerate() {
            // Behind the whole venue: ordering-table word `0x400`.
            assert_eq!((ot, p), (OT + 0x1000, PACKETS + 0x28 * i as u32), "{ctx}");
            assert_eq!(
                (cpu.mem.rd32(p), cpu.mem.rd32(p + 4)),
                (0x0900_0000, 0x2C80_8080),
                "{ctx}: a neutral POLY_FT4"
            );
            for (v, (xy, uv)) in q.xy.iter().zip(q.uv).enumerate() {
                let at = p + 8 + 8 * v as u32;
                assert_eq!(
                    (
                        cpu.mem.rd16(at) as i16,
                        cpu.mem.rd16(at + 2) as i16,
                        cpu.mem.rd8(at + 4),
                        cpu.mem.rd8(at + 5)
                    ),
                    (xy[0] as i16, xy[1] as i16, uv[0], uv[1]),
                    "{ctx}: quad {i} corner {v}"
                );
            }
            assert_eq!(
                (cpu.mem.rd16(p + 0xE), cpu.mem.rd16(p + 0x16)),
                (q.clut, q.tpage),
                "{ctx}: quad {i} texture words"
            );
        }
        assert_eq!(cpu.mem.rd32(CTX + 0x8C), PACKETS + 0x28 * 6, "{ctx}");
        for a in CLEAR {
            assert_eq!(cpu.mem.read(a, 3), SKY_CLEAR_RGB, "{ctx}: the clear colour");
        }
        // Its own view is built with the yaw zeroed; the frame's is rebuilt
        // with it back.
        assert_eq!(*yaw_seen.borrow(), [0, yaw], "{ctx}: the view rebuilds");
        assert_eq!(cpu.mem.rd16(YAW) as i16, yaw, "{ctx}");
    }
}
