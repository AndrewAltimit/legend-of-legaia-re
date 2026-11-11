//! The Muscle Dome hub's backdrop emitter `FUN_801D00F8` and its shade
//! `FUN_801D1610` (PROT 0977), run live, against `engine-ui`'s
//! `ringside_backdrop`: the still arm's two `POLY_FT4` packets, the
//! first-visit arm's six tile calls and its shade, and the shade packet on
//! its own - each read back off the packet buffer with the ordering-table
//! slot it was linked at.

use crate::common::{Rng, STACK_TOP, disc, scus_cpu};
use legaia_engine_ui::muscle_dome_hud::HubAnchor;
use legaia_engine_ui::ringside_backdrop::{
    GP0_POLY_FT4, GP0_POLY_G4_SEMI, SHADE_DRAW_MODE_TPAGE, SHADE_OT, TILE_OT, backdrop_shade,
    first_visit_shade, first_visit_tile_draws, ringside_still_quads,
};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const BACKDROP: u32 = 0x801D_00F8;
const SHADE: u32 = 0x801D_1610;
const TILE: u32 = 0x801D_08EC;
const ADD_PRIM: u32 = 0x8003_D2C4;
const SET_DRAW_MODE: u32 = 0x8005_9010;

const PACKET_CURSOR: u32 = 0x1F80_03A0;
const OT_BASE: u32 = 0x1F80_03F4;
const PACKETS: u32 = 0x8010_0000;
const OT: u32 = 0x8018_0000;
/// The re-entry latch and the tile emitter's ordering-table word.
const LATCH: u32 = 0x801D_1AE0;
const TILE_OT_WORD: u32 = 0x801D_1AA8;

/// One tile call: `(x, y, record, level, scale, ordering-table word)`.
type TileCall = (i32, i32, i32, i32, i32, u32);

struct Rig {
    cpu: Cpu,
    prims: Rc<RefCell<Vec<(u32, u32)>>>,
    modes: Rc<RefCell<Vec<(u32, u32)>>>,
    tiles: Rc<RefCell<Vec<TileCall>>>,
}

fn rig() -> Option<Rig> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(977).expect("PROT 0977"),
    );
    let prims: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    let p = prims.clone();
    cpu.hook(ADD_PRIM, move |r, _| {
        p.borrow_mut().push((r.r[A0], r.r[A1]))
    });
    let modes: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    let m = modes.clone();
    cpu.hook(SET_DRAW_MODE, move |r, _| {
        m.borrow_mut().push((r.r[A0], r.r[A3]))
    });
    let tiles: Rc<RefCell<Vec<TileCall>>> = Rc::default();
    let t = tiles.clone();
    cpu.hook(TILE, move |r, m| {
        let scale = m.rd32(r.r[SP] + 0x10) as i32;
        let ot = m.rd32(TILE_OT_WORD);
        t.borrow_mut().push((
            r.r[A0] as i32,
            r.r[A1] as i32,
            r.r[A2] as i32,
            r.r[A3] as i32,
            scale,
            ot,
        ))
    });
    Some(Rig {
        cpu,
        prims,
        modes,
        tiles,
    })
}

impl Rig {
    fn reset(&mut self) {
        self.cpu.mem.wr32(PACKET_CURSOR, PACKETS);
        self.cpu.mem.wr32(OT_BASE, OT);
        self.prims.borrow_mut().clear();
        self.modes.borrow_mut().clear();
        self.tiles.borrow_mut().clear();
    }

    fn xy(&mut self, p: u32, o: u32) -> (i16, i16) {
        (
            self.cpu.mem.rd16(p + o) as i16,
            self.cpu.mem.rd16(p + o + 2) as i16,
        )
    }

    /// The shade packet at `p` and its draw mode, against the port's.
    fn check_shade(
        &mut self,
        links: &[(u32, u32)],
        want: &legaia_engine_ui::ringside_backdrop::BackdropShade,
        what: &str,
    ) {
        assert_eq!(links.len(), 2, "{what}: the quad and its draw mode");
        let slot = OT + want.ot * 4;
        assert!(
            links.iter().all(|l| l.0 == slot),
            "{what}: slot {:#x}",
            want.ot
        );
        let p = links[0].1;
        assert_eq!(self.cpu.mem.rd8(p + 3), 8, "{what}: eight words");
        assert_eq!(self.cpu.mem.rd8(p + 7), GP0_POLY_G4_SEMI, "{what}");
        let xy = [
            self.xy(p, 8),
            self.xy(p, 0x10),
            self.xy(p, 0x18),
            self.xy(p, 0x20),
        ];
        assert_eq!(xy, want.xy, "{what}: corners");
        let rgb = [4u32, 0xC, 0x14, 0x1C].map(|o| {
            [
                self.cpu.mem.rd8(p + o),
                self.cpu.mem.rd8(p + o + 1),
                self.cpu.mem.rd8(p + o + 2),
            ]
        });
        assert_eq!(rgb, want.rgb, "{what}: colours");
        let modes = self.modes.borrow().clone();
        assert_eq!(
            modes,
            vec![(links[1].1, u32::from(SHADE_DRAW_MODE_TPAGE))],
            "{what}: the draw mode"
        );
        assert_eq!(((modes[0].1 >> 5) & 3) as u8, want.abr, "{what}: the blend");
    }
}

#[test]
fn the_backdrop_emitter_draws_both_arms_as_retail_does() {
    differential!(prot(977, BACKDROP));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5249_4E47);
    let (mut stills, mut firsts, mut clamped) = (0, 0, 0);
    for run in 0..4_000 {
        let level = match rng.below(4) {
            0 => [-5i32, 0, 0x100, 0x1FF, 0xFF, -1][rng.below(6) as usize],
            _ => rng.below(0x100) as i32,
        };
        let latch = rng.below(2) * (1 + rng.below(3));
        rig.cpu.mem.wr32(LATCH, latch);
        rig.cpu.mem.wr32(TILE_OT_WORD, 0);
        rig.reset();
        assert_eq!(rig.cpu.call(BACKDROP, &[level as u32]), Stop::Returned);
        let what = format!("run {run}: level {level} latch {latch}");
        let links = rig.prims.borrow().clone();
        clamped += u32::from(!(0..0x100).contains(&level));
        if latch != 0 {
            stills += 1;
            assert!(rig.tiles.borrow().is_empty(), "{what}");
            assert_eq!(links.len(), 2, "{what}");
            let quads = ringside_still_quads(level);
            for (q, &(ot, p)) in quads.iter().zip(&links) {
                assert_eq!(ot, OT + TILE_OT * 4, "{what}: the wall's slot");
                assert_eq!(rig.cpu.mem.rd8(p + 3), 9, "{what}: nine words");
                assert_eq!(rig.cpu.mem.rd8(p + 7), GP0_POLY_FT4, "{what}");
                let rgb = [
                    rig.cpu.mem.rd8(p + 4),
                    rig.cpu.mem.rd8(p + 5),
                    rig.cpu.mem.rd8(p + 6),
                ];
                assert_eq!(rgb, q.rgb[0], "{what}: the fade level");
                let xy = [
                    rig.xy(p, 8),
                    rig.xy(p, 0x10),
                    rig.xy(p, 0x18),
                    rig.xy(p, 0x20),
                ];
                assert_eq!(xy, q.xy, "{what}: corners");
                let uv = [0xCu32, 0x14, 0x1C, 0x24]
                    .map(|o| (rig.cpu.mem.rd8(p + o), rig.cpu.mem.rd8(p + o + 1)));
                assert_eq!(uv, q.uv, "{what}: texels");
                assert_eq!(rig.cpu.mem.rd16(p + 0x16), q.tpage, "{what}: the page");
            }
            continue;
        }
        firsts += 1;
        let c = level.clamp(0, 0xFF);
        let want: Vec<TileCall> = first_visit_tile_draws()
            .iter()
            .map(|d| {
                assert_eq!(d.anchor, HubAnchor::Corner);
                (i32::from(d.x), i32::from(d.y), d.sel, c, d.scale, TILE_OT)
            })
            .collect();
        assert_eq!(*rig.tiles.borrow(), want, "{what}: the wall tiles");
        rig.check_shade(&links, &first_visit_shade(), &what);
        assert_eq!(first_visit_shade().ot, SHADE_OT);
    }
    assert!(
        stills > 1000 && firsts > 1000 && clamped > 300,
        "{stills} {firsts} {clamped}"
    );
}

#[test]
fn the_shade_packet_is_the_one_retail_links() {
    differential!(prot(977, SHADE));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5348_4144);
    for run in 0..4_000 {
        let (x, y) = (rng.below(400) as i16 - 40, rng.below(300) as i16 - 30);
        let (w, h) = (rng.below(0x180) as i16, rng.below(0x100) as i16);
        let ot = rng.below(0x400);
        rig.reset();
        rig.cpu.mem.wr32(STACK_TOP + 0x10, ot);
        assert_eq!(
            rig.cpu
                .call(SHADE, &[x as u32, y as u32, w as u32, h as u32]),
            Stop::Returned
        );
        let links = rig.prims.borrow().clone();
        let what = format!("run {run}: ({x}, {y}, {w}, {h}) at slot {ot:#x}");
        rig.check_shade(&links, &backdrop_shade(x, y, w, h, ot), &what);
    }
}
