//! The save UI's dimmed backdrop `FUN_801E02A4` (PROT 0899) against
//! `engine-ui`'s `backdrop_dim_sprites`: the two `SPRT` packets it links,
//! each behind a draw-mode packet naming its texture page, at every
//! brightness.
//!
//! The title art sits at the head of texture page 8 in 8-bit texels, so a
//! texel column is `(page - 8) * 128 + u`, which is what the port's one
//! atlas rect is split by.

use crate::common::{disc, scus_cpu};
use legaia_engine_ui::backdrop_dim_sprites;
use psx_retail_exec::{A0, A1, A3, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const BACKDROP: u32 = 0x801E_02A4;
const OVERLAY_BASE: u32 = 0x801C_E818;
const ADD_PRIM: u32 = 0x8003_D2C4;
const SET_DRAW_TPAGE: u32 = 0x8005_9010;
const CURSOR: u32 = 0x1F80_03A0;
const OT_BASE: u32 = 0x1F80_03F4;
const PACKETS: u32 = 0x8010_0000;
const OT: u32 = 0x8018_0000;

#[test]
fn the_dimmed_backdrop_is_the_two_sprites_retail_links() {
    differential!(prot(899, BACKDROP));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    // `(ordering-table entry, packet)` per link, and each draw-mode packet's
    // page word.
    let links: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    let pages: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    let l = links.clone();
    cpu.hook(ADD_PRIM, move |r, _| {
        l.borrow_mut().push((r.r[A0], r.r[A1]))
    });
    let p = pages.clone();
    cpu.hook(SET_DRAW_TPAGE, move |r, _| {
        p.borrow_mut().push((r.r[A0], r.r[A3]))
    });
    for brightness in 0..=0xFFu32 {
        cpu.mem.wr32(CURSOR, PACKETS);
        cpu.mem.wr32(OT_BASE, OT);
        links.borrow_mut().clear();
        pages.borrow_mut().clear();
        assert_eq!(cpu.call(BACKDROP, &[brightness]), Stop::Returned);
        // Sprite, its page, sprite, its page - all into one bucket.
        let links = links.borrow().clone();
        let pages = pages.borrow().clone();
        assert_eq!(links.len(), 4, "{links:x?}");
        assert!(links.iter().all(|&(ot, _)| ot == OT + 0x50), "{links:x?}");
        assert_eq!(
            [links[1].1, links[3].1],
            [pages[0].0, pages[1].0],
            "each sprite is followed by its page packet"
        );
        let port = backdrop_dim_sprites((0, 0, 320, 240), brightness as u8, (0, 0), 1);
        assert_eq!(port.len(), 2);
        for (i, sprite) in port.iter().enumerate() {
            let at = links[i * 2].1;
            let m = &mut cpu.mem;
            assert_eq!(m.rd8(at + 7), 0x64, "a textured sprite");
            assert_eq!(m.rd8(at + 3), 4, "four words");
            let rgb = [m.rd8(at + 4), m.rd8(at + 5), m.rd8(at + 6)];
            assert_eq!(rgb, [brightness as u8; 3], "the dim on all three");
            let (x, y) = (m.rd16(at + 8) as i16, m.rd16(at + 0xA) as i16);
            let (u, v) = (u32::from(m.rd8(at + 0xC)), u32::from(m.rd8(at + 0xD)));
            let (w, h) = (u32::from(m.rd16(at + 0x10)), u32::from(m.rd16(at + 0x12)));
            assert_eq!(m.rd16(at + 0xE), 0x7A80, "the title CLUT");
            let page = pages[i].1;
            assert_eq!(page & !1, 0x88, "an 8-bit page at the art's head");
            let column = (page - 0x88) * 128 + u;
            // The screen ends the right sprite's 255 rows at 240.
            let rows = h.min(240);
            assert_eq!(
                (sprite.dst, sprite.src),
                ((i32::from(x), i32::from(y), w, rows), (column, v, w, rows)),
                "brightness {brightness:#x} sprite {i}"
            );
            let tint = brightness as f32 / 128.0;
            assert_eq!(sprite.color, [tint, tint, tint, 1.0]);
        }
    }
}
