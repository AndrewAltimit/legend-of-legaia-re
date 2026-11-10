//! The Triangle arts list's renderer `FUN_80034358` against
//! `legaia_engine_ui::arts_input::arts_list_cells`: random learned-art lists,
//! scrolls, slides and AP-halving bits over the executable's own arts-name
//! table (`0x80075EC4`), with the text kernel `FUN_80036888`, the numeral
//! blit `FUN_8003C11C` and the arrow blit `FUN_8003C310` recorded with the
//! ink word `gp+0x13C` each ran under. The rows the port is handed are the
//! table records the routine's own scan matches, so this compares the
//! draw: the name's pen and ink, the AP cells and the arrows' cells and inks.

use crate::common::{Rng, scus_cpu};
use legaia_engine_ui::arts_input::{
    ArtsListCell, ArtsListRowView, LIST_NAME_INK, LIST_RECT, arts_list_cells, arts_list_window_x,
};
use psx_retail_exec::{A0, A1, A2, A3, Memory, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const LIST: u32 = 0x8003_4358;
const DRAW_TEXT: u32 = 0x8003_6888;
const BLIT_NUMERAL: u32 = 0x8003_C11C;
const BLIT_ARROW: u32 = 0x8003_C310;
const GP: u32 = 0x8007_B318;
const ARTS: u32 = 0x8007_5EC4;
const BLOCK: u32 = 0x8008_4140;
const NODE: u32 = 0x8013_0000;

/// One draw as recorded, owning its name bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Drawn {
    Name(Vec<u8>, i32, i32, u8),
    Digit(u8, i32, i32),
    Arrow(u8, u8, i32, i32),
}

fn raw_string(m: &mut Memory, mut at: u32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = m.rd8(at);
        match b {
            0 => return out,
            0xC1..=0xC7 | 0xCE | 0xCF => {
                out.extend([b, m.rd8(at + 1)]);
                at += 2;
            }
            _ => {
                out.push(b);
                at += 1;
            }
        }
    }
}

/// An arts-name table record: character, id, AP, name and command glyphs.
struct Art {
    character: u8,
    id: u8,
    ap: u8,
    name: Vec<u8>,
    glyphs: Vec<[u8; 2]>,
}

#[test]
fn the_arts_list_draws_what_retail_draws() {
    differential!(scus(LIST));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut table = Vec::new();
    let mut rec = ARTS;
    while cpu.mem.rd8(rec) < 0x63 {
        let (glyph_ptr, name_ptr) = (cpu.mem.rd32(rec + 8), cpu.mem.rd32(rec + 0xC));
        let n = cpu.mem.rd8(glyph_ptr);
        table.push(Art {
            character: cpu.mem.rd8(rec),
            id: cpu.mem.rd8(rec + 1),
            ap: cpu.mem.rd8(rec + 2),
            name: raw_string(&mut cpu.mem, name_ptr),
            glyphs: (0..u32::from(n))
                .map(|k| {
                    [
                        cpu.mem.rd8(glyph_ptr + 1 + 2 * k),
                        cpu.mem.rd8(glyph_ptr + 2 + 2 * k),
                    ]
                })
                .collect(),
        });
        rec += 0x14;
    }
    assert!(table.len() > 20, "the arts table");

    let log = Rc::new(RefCell::new(Vec::<Drawn>::new()));
    let l = log.clone();
    cpu.hook(DRAW_TEXT, move |r, m| {
        assert_eq!((r.r[A1], r.r[A2]), (0, 0));
        let name = raw_string(m, r.r[A0]);
        let y = m.rd32(r.r[SP] + 0x10) as i32;
        let ink = m.rd32(GP + 0x13C) as u8;
        l.borrow_mut()
            .push(Drawn::Name(name, r.r[A3] as i32, y, ink));
    });
    let l = log.clone();
    cpu.hook(BLIT_NUMERAL, move |r, m| {
        let (pos, rect) = (r.r[A0], r.r[A1]);
        assert_eq!(
            (m.rd16(rect + 2), m.rd16(rect + 4), m.rd16(rect + 6)),
            (0xD0, 8, 12)
        );
        assert_eq!(r.r[A2], 9 + 0x7F86, "the AP's ink");
        l.borrow_mut().push(Drawn::Digit(
            (m.rd16(rect) / 8) as u8,
            m.rd16(pos) as i16 as i32,
            m.rd16(pos + 2) as i16 as i32,
        ));
    });
    let l = log.clone();
    cpu.hook(BLIT_ARROW, move |r, m| {
        let ink = m.rd32(GP + 0x13C) as u8;
        l.borrow_mut().push(Drawn::Arrow(
            r.r[A0] as u8,
            ink,
            r.r[A1] as i32,
            r.r[A2] as i32,
        ));
    });

    let mut rng = Rng(0x4152_5453);
    let (mut drawn, mut halved, mut missing) = (0, 0, 0);
    for case in 0..3000 {
        let character = rng.below(3) as u8;
        let mine: Vec<&Art> = table.iter().filter(|a| a.character == character).collect();
        let count = rng.below(20) as u8;
        let ids: Vec<u8> = (0..count)
            .map(|_| {
                if rng.below(10) == 0 {
                    0xEE
                } else {
                    mine[rng.below(mine.len() as u32) as usize].id
                }
            })
            .collect();
        let scroll = rng.below(u32::from(count) + 2);
        let slide = 1 + rng.below(12) as u8;
        let half = rng.below(3) == 0;
        let at = BLOCK + 0x414 * u32::from(character);
        cpu.mem.wr8(at + 0x74D, count);
        for (k, id) in ids.iter().enumerate() {
            cpu.mem.wr8(at + 0x74E + k as u32, *id);
        }
        cpu.mem.wr32(at + 0x6C0, if half { 0x800 } else { 0 });
        cpu.mem.wr32(GP + 0x874, u32::from(character));
        cpu.mem.wr32(GP + 0x140, scroll);
        cpu.mem.wr32(GP + 0x15C, 0);
        let (_, y0, _, h) = LIST_RECT;
        cpu.mem.wr16(NODE + 0xA, arts_list_window_x(slide) as u16);
        cpu.mem.wr16(NODE + 0xC, y0 as u16);
        cpu.mem.wr16(NODE + 0x10, h as u16);
        log.borrow_mut().clear();
        assert_eq!(cpu.call(LIST, &[NODE]), Stop::Returned, "case {case}");

        // The rows the routine's scan resolves, from the scroll on.
        let rows: Vec<Option<ArtsListRowView>> = ids
            .iter()
            .skip(scroll as usize)
            .map(|&id| {
                let a = table
                    .iter()
                    .find(|a| a.character == character && a.id == id)?;
                Some(ArtsListRowView {
                    name: &a.name,
                    ap: if half { a.ap >> 1 } else { a.ap },
                    glyphs: &a.glyphs,
                })
            })
            .collect();
        let want: Vec<Drawn> = arts_list_cells(&rows, slide)
            .into_iter()
            .map(|c| match c {
                ArtsListCell::Name { name, x, y } => {
                    Drawn::Name(name.to_vec(), x, y, LIST_NAME_INK)
                }
                ArtsListCell::ApDigit { digit, x, y } => Drawn::Digit(digit, x, y),
                ArtsListCell::Arrow { dir, ink, x, y } => Drawn::Arrow(dir, ink, x, y),
            })
            .collect();
        let got = log.borrow().clone();
        assert_eq!(
            got, want,
            "case {case}: char {character} scroll {scroll} ids {ids:x?}"
        );
        drawn += got.len();
        halved += usize::from(half && !got.is_empty());
        missing += usize::from(rows.iter().take(4).any(Option::is_none));
    }
    assert!(
        drawn > 20_000 && halved > 500 && missing > 300,
        "{drawn} {halved} {missing}"
    );
    assert_eq!(cpu.mem.rd32(GP + 0x13C), 7, "the ink is left at 7");
}
