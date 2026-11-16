//! The screen wipe's curtain emitter `FUN_8003479C` against
//! `legaia_render_kernels::wipe_curtain::curtain_packets`, at every level
//! the pause session can hand it and past both ends: each packet builder it
//! calls is hooked and recorded - the flat quad `FUN_8003C510`, the gouraud
//! quad `FUN_8003C43C`, the draw mode `FUN_80059010` - with its rectangle,
//! colours and flags, and the draw-mode links `FUN_8003D2C4` with the
//! ordering-table entry they take.

use crate::common::scus_cpu;
use legaia_render_kernels::wipe_curtain::{CURTAIN_OT, CURTAIN_W, CurtainPacket, curtain_packets};
use psx_retail_exec::{A0, A1, A2, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const EMIT: u32 = 0x8003_479C;
const FLAT: u32 = 0x8003_C510;
const GOURAUD: u32 = 0x8003_C43C;
const DRAW_MODE: u32 = 0x8005_9010;
const LINK: u32 = 0x8003_D2C4;
const PACKET_CURSOR: u32 = 0x1F80_03A0;
const OT_BASE: u32 = 0x1F80_03F4;
const PACKETS: u32 = 0x8012_0000;
const OT: u32 = 0x8013_0000;

#[derive(Debug, PartialEq, Eq)]
enum Seen {
    Flat {
        xy: [i16; 8],
        colour: u32,
        semi: u32,
        raw: u32,
    },
    Gouraud {
        xy: [i16; 8],
        colours: [u32; 4],
        semi: u32,
        raw: u32,
    },
    DrawMode {
        dfe: u32,
        dtd: u32,
        tpage: u32,
    },
    Link {
        ot: u32,
    },
}

fn rect(y0: i16, y1: i16) -> [i16; 8] {
    [0, y0, CURTAIN_W, y0, 0, y1, CURTAIN_W, y1]
}

#[test]
fn the_curtain_emits_as_retail_does() {
    differential!(scus(EMIT));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let seen = Rc::new(RefCell::new(Vec::<Seen>::new()));
    let read_xy = |m: &mut psx_retail_exec::Memory, a: u32| -> [i16; 8] {
        std::array::from_fn(|i| m.rd16(a + 2 * i as u32) as i16)
    };
    let s = seen.clone();
    cpu.hook(FLAT, move |r, m| {
        let xy = read_xy(m, r.r[A0]);
        let colour = m.rd32(r.r[A1]);
        s.borrow_mut().push(Seen::Flat {
            xy,
            colour,
            semi: r.r[A2],
            raw: r.r[A3],
        });
    });
    let s = seen.clone();
    cpu.hook(GOURAUD, move |r, m| {
        let xy = read_xy(m, r.r[A0]);
        let colours = std::array::from_fn(|i| m.rd32(r.r[A1] + 4 * i as u32));
        s.borrow_mut().push(Seen::Gouraud {
            xy,
            colours,
            semi: r.r[A2],
            raw: r.r[A3],
        });
    });
    let s = seen.clone();
    cpu.hook(DRAW_MODE, move |r, _| {
        s.borrow_mut().push(Seen::DrawMode {
            dfe: r.r[A1],
            dtd: r.r[A2],
            tpage: r.r[A3],
        })
    });
    let s = seen.clone();
    cpu.hook(LINK, move |r, _| {
        s.borrow_mut().push(Seen::Link { ot: r.r[A0] })
    });
    let mut drawn = 0;
    for level in (-0x20..=0x120).chain([0x1000, -0x1000]) {
        cpu.mem.wr32(PACKET_CURSOR, PACKETS);
        cpu.mem.wr32(OT_BASE, OT);
        seen.borrow_mut().clear();
        assert_eq!(cpu.call(EMIT, &[level as u32]), Stop::Returned);
        let want: Vec<Seen> = curtain_packets(level)
            .into_iter()
            .flat_map(|p| match p {
                CurtainPacket::Bar { y0, y1 } => vec![Seen::Flat {
                    xy: rect(y0, y1),
                    colour: 0,
                    semi: 0,
                    raw: 0,
                }],
                CurtainPacket::Band {
                    y0,
                    y1,
                    top,
                    bottom,
                } => vec![Seen::Gouraud {
                    xy: rect(y0, y1),
                    colours: [top, top, bottom, bottom],
                    semi: 1,
                    raw: 1,
                }],
                CurtainPacket::DrawMode { tpage } => vec![
                    Seen::DrawMode {
                        dfe: 0,
                        dtd: 0,
                        tpage: u32::from(tpage),
                    },
                    Seen::Link {
                        ot: OT + 4 * CURTAIN_OT,
                    },
                ],
            })
            .collect();
        drawn += want.len();
        assert_eq!(*seen.borrow(), want, "level {level:#x}");
    }
    assert!(drawn > 1000, "{drawn}");
}
