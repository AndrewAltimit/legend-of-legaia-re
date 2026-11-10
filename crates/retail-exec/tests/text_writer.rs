//! The line renderer `FUN_80036888` and its two wrappers `FUN_8003CC98` /
//! `FUN_8003CD00`, run live, against `legaia-font`'s layout.
//!
//! Every glyph retail draws is one `SPRT` it links into ordering-table slot
//! one: `(x, y)`, the cell `u = (c & 0xF) << 4`, `v = (c & 0xF0) - 0x20` on the
//! 16-px font page, a 14x15 rect, and the ink's CLUT `0x7F86 + ink`. The
//! packets are read back off the packet buffer and held, in order, to
//! `Font::layout_padded` over the expanded string: pen, cell and advance,
//! the `|` break, and a `0xCE` sprite escape's seat (the pictogram call
//! `FUN_8002C488`, hooked) and advance.
//!
//! What the renderer does that the layout leaves to its callers, and the
//! test holds apart: a space draws no sprite (the layout keeps its blank
//! cell); a `0xCF` operand becomes the ink of the glyphs after it; the glyph
//! pad `DAT_800740E8` is zeroed on the way out. Numeric escapes
//! (`0xCE 0x0B..=0x0E`, a script counter through `FUN_80034B78`) are not
//! generated: the dialog layer resolves them before layout.

use crate::common::{Rng, STACK_TOP, disc, scus, scus_cpu};
use legaia_font::Font;
use legaia_font::glyph_count::typewriter_glyph_count;
use psx_retail_exec::{A0, A1, A2, Cpu, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const WRITER: u32 = 0x8003_6888;
const DRAW_COUNTED: u32 = 0x8003_CC98;
const LABEL: u32 = 0x8003_CD00;
const ADD_PRIM: u32 = 0x8003_D2C4;
const SET_DRAW_MODE: u32 = 0x8005_9010;
const PICTO: u32 = 0x8002_C488;
const NUMBER: u32 = 0x8003_4B78;

const CURSOR: u32 = 0x1F80_03A0;
const OT_BASE: u32 = 0x1F80_03F4;
const PACKETS: u32 = 0x8010_0000;
const OT: u32 = 0x8018_0000;
const INK: u32 = 0x8007_B454;
const PAD: u32 = 0x8007_40E8;
const ESCAPES: u32 = 0x8007_4050;
const TEXT: u32 = 0x8014_0000;

/// One glyph sprite: pen, cell `(column, row)`, ink.
type Glyph = (i32, i32, u32, u32, u16);

struct Rig {
    cpu: Cpu,
    font: Font,
    prims: Rc<RefCell<Vec<(u32, u32)>>>,
    pictos: Rc<RefCell<Vec<(i32, i32, u32)>>>,
}

fn rig() -> Option<Rig> {
    let (mut cpu, d, scus) = (scus_cpu()?, disc()?, scus()?);
    let tim = d
        .read_prot_bytes(
            legaia_font::FONT_TIM_PROT_DAT_OFFSET,
            legaia_font::FONT_TIM_LEN,
        )
        .expect("font TIM");
    let head = d
        .read_prot_bytes(
            legaia_font::escape_icons::ICON_PROT_DAT_OFFSET,
            legaia_font::escape_icons::ICON_PROT_DAT_LEN,
        )
        .expect("escape icons");
    let font = Font::from_disc_tim_and_scus(&tim, &scus)
        .expect("font")
        .with_escape_icons_from_disc(&head, &scus);
    let prims: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    let p = prims.clone();
    cpu.hook(ADD_PRIM, move |r, _| {
        p.borrow_mut().push((r.r[A0], r.r[A1]))
    });
    // `SetDrawMode` fills a packet the test does not read; mark it so a
    // stale sprite under it is not taken for one.
    cpu.hook(SET_DRAW_MODE, |r, m| m.wr32(r.r[A0] + 4, 0xE100_0000));
    let pictos: Rc<RefCell<Vec<(i32, i32, u32)>>> = Rc::default();
    let p = pictos.clone();
    cpu.hook(PICTO, move |r, _| {
        p.borrow_mut()
            .push((r.r[A0] as i32, r.r[A1] as i32, r.r[A2]))
    });
    cpu.hook(NUMBER, |_, _| panic!("no numeric escape is generated"));
    // BIOS A(0x2B) `memset`.
    cpu.hook(0x8005_6788, |r, m| {
        for i in 0..r.r[A2] {
            m.wr8(r.r[A0] + i, r.r[A1] as u8);
        }
    });
    Some(Rig {
        cpu,
        font,
        prims,
        pictos,
    })
}

impl Rig {
    fn reset(&mut self) {
        self.cpu.mem.wr32(CURSOR, PACKETS);
        self.cpu.mem.wr32(OT_BASE, OT);
        self.prims.borrow_mut().clear();
        self.pictos.borrow_mut().clear();
    }

    /// The glyph sprites linked since [`Rig::reset`], in order.
    fn glyphs(&mut self) -> Vec<Glyph> {
        let prims = self.prims.borrow().clone();
        let mut out = Vec::new();
        for (ot, p) in prims {
            assert_eq!(ot, OT + 4, "every packet links into slot one");
            if self.cpu.mem.rd8(p + 7) != 0x64 {
                continue;
            }
            assert_eq!(self.cpu.mem.rd32(p + 4), 0x6480_8080, "a neutral SPRT");
            assert_eq!(self.cpu.mem.rd8(p + 3), 4, "four words");
            assert_eq!(
                (self.cpu.mem.rd16(p + 0x10), self.cpu.mem.rd16(p + 0x12)),
                (14, 15),
                "the glyph rect"
            );
            let (u, v) = (self.cpu.mem.rd8(p + 0xC), self.cpu.mem.rd8(p + 0xD));
            assert_eq!((u % 16, v % 16), (0, 0), "a cell origin");
            out.push((
                i32::from(self.cpu.mem.rd16(p + 8) as i16),
                i32::from(self.cpu.mem.rd16(p + 0xA) as i16),
                u32::from(u) / 16,
                u32::from(v) / 16,
                self.cpu.mem.rd16(p + 0xE).wrapping_sub(0x7F86),
            ));
        }
        out
    }
}

/// A generated line: glyphs, accent cells, breaks, colour pairs, the author
/// aliases, and sprite escapes the font carries a cell for.
fn gen_text(rng: &mut Rng, sprites: &[u8], aliases: bool) -> Vec<u8> {
    let mut s = Vec::new();
    for _ in 0..1 + rng.below(28) {
        let pick = match rng.below(18) {
            1 | 3 if !aliases => 8,
            n => n,
        };
        match pick {
            0 => s.push(0x7C),
            1 => s.extend([
                0x5E,
                0x2D + sprites[rng.below(sprites.len() as u32) as usize],
            ]),
            2 => s.extend([0xCE, sprites[rng.below(sprites.len() as u32) as usize]]),
            3 => s.extend([0xFF, rng.below(10) as u8]),
            4 => s.extend([0xCF, rng.below(10) as u8]),
            5 => s.push(0x80 + rng.below(0x40) as u8),
            6 | 7 => s.push(0x20),
            _ => s.push(match 0x21 + rng.below(0x5B) as u8 {
                0x5E => 0x5F,
                b => b,
            }),
        }
    }
    // The renderer draws nothing for a string that opens below `0x20`.
    if s[0] == 0x20 && rng.below(2) == 0 {
        s[0] = b'A';
    }
    s
}

/// What the port lays out for `text` at `(x, y)`: the glyph sprites retail
/// would draw (spaces dropped) with the ink each is drawn in, the sprite
/// escapes, and the glyph count the renderer returns.
#[allow(clippy::type_complexity)]
fn port_line(
    font: &Font,
    text: &[u8],
    pad: u32,
    (x, y): (i32, i32),
    ink0: u16,
) -> (Vec<Glyph>, Vec<(i32, i32)>, u32) {
    let (expanded, unresolved) =
        legaia_font::measure::expand_text(text, &legaia_font::MeasureOptions::default(), true);
    assert!(unresolved.is_empty());
    let layout = font.layout_padded(&expanded, pad);
    // The ink each laid glyph is drawn in: the last colour operand before it.
    let mut inks = Vec::new();
    let (mut ink, mut i) = (ink0, 0);
    while i < expanded.len() {
        match expanded[i] {
            0xCF => {
                ink = u16::from(expanded[i + 1]);
                i += 2;
            }
            0xCE => {
                inks.push(ink);
                i += 2;
            }
            0x7C => i += 1,
            _ => {
                inks.push(ink);
                i += 1;
            }
        }
    }
    assert_eq!(inks.len(), layout.glyphs.len(), "one ink a laid glyph");
    let (mut glyphs, mut escapes) = (Vec::new(), Vec::new());
    for (g, ink) in layout.glyphs.iter().zip(inks) {
        if g.byte == legaia_font::ESCAPE_GLYPH_BYTE {
            escapes.push((x + g.dst_x, y + g.dst_y));
        } else if g.byte != 0x20 {
            glyphs.push((
                x + g.dst_x,
                y + g.dst_y,
                g.atlas_x / 14,
                g.atlas_y / 15,
                ink,
            ));
        }
    }
    let breaks = expanded.iter().filter(|&&b| b == 0x7C).count();
    // A colour operand of `0x7C` is not a break; none is generated.
    (glyphs, escapes, (layout.glyphs.len() + breaks) as u32)
}

fn sprite_ops(rig: &mut Rig) -> Vec<u8> {
    let ops: Vec<u8> = (0..0x26u8)
        .filter(|&op| {
            rig.cpu.mem.rd16(ESCAPES + u32::from(op) * 4) != 0 && rig.font.escape_cell(op).is_some()
        })
        .collect();
    assert!(ops.len() > 20, "{} sprite escapes", ops.len());
    // The font's cells sit and advance as the table says.
    for &op in &ops {
        let e = ESCAPES + u32::from(op) * 4;
        let cell = rig.font.escape_cell(op).unwrap();
        assert_eq!(
            (u32::from(cell.advance), i32::from(cell.y_offset)),
            (
                u32::from(rig.cpu.mem.rd8(e + 2)),
                i32::from(rig.cpu.mem.rd8(e + 3) as i8)
            ),
            "escape {op:#x}"
        );
    }
    ops
}

#[test]
fn the_line_renderer_draws_the_glyphs_the_layout_places() {
    differential!(scus(WRITER));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let ops = sprite_ops(&mut rig);
    let mut rng = Rng(0x5752_4954);
    let (mut broken, mut escaped, mut inked, mut blank) = (0, 0, 0, 0);
    for round in 0..12_000 {
        let text = gen_text(&mut rng, &ops, true);
        let pad = rng.below(2);
        let (x, y) = (rng.below(300) as i32 - 20, rng.below(220) as i32 - 10);
        let ink0 = rng.below(10) as u16;
        let mut z = text.clone();
        z.extend([0u8; 4]);
        rig.cpu.mem.load(TEXT, &z);
        rig.cpu.mem.wr32(PAD, pad);
        rig.cpu.mem.wr32(INK, u32::from(ink0));
        rig.reset();
        rig.cpu.mem.wr32(STACK_TOP + 0x10, y as u32);
        assert_eq!(
            rig.cpu.call(WRITER, &[TEXT, 0, 0, x as u32]),
            Stop::Returned,
            "round {round}"
        );
        let count = rig.cpu.regs.r[V0];
        let what = format!("round {round}: {text:02x?} pad {pad} at ({x}, {y})");
        assert_eq!(rig.cpu.mem.rd32(PAD), 0, "{what}: the pad is spent");
        let got = rig.glyphs();
        if text[0] < 0x20 {
            assert!(got.is_empty() && count == 0, "{what}");
            continue;
        }
        let (glyphs, escapes, laid) = port_line(&rig.font, &text, pad, (x, y), ink0);
        assert_eq!(got, glyphs, "{what}: glyph sprites");
        let pictos = rig.pictos.borrow().clone();
        let seats: Vec<(i32, i32)> = pictos.iter().map(|p| (p.0, p.1)).collect();
        assert_eq!(seats, escapes, "{what}: sprite escapes");
        assert_eq!(count, laid, "{what}: the count returned");
        broken += u32::from(text.contains(&0x7C));
        escaped += u32::from(!pictos.is_empty());
        inked += u32::from(glyphs.iter().any(|g| g.4 != ink0));
        blank += u32::from(text.contains(&0x20));
    }
    assert!(
        broken > 3000 && escaped > 3000 && inked > 3000 && blank > 3000,
        "{broken} {escaped} {inked} {blank}"
    );
}

/// `FUN_8003CC98` draws one string and returns its typewriter count;
/// `FUN_8003CD00` draws a label, whose `|` lines the renderer itself steps
/// `0x0E` apart (`0x80036934`) - the wrapper's own `0x0D` step
/// (`0x8003CD84`) is for a string resumed past a control lead, which a label
/// is not.
#[test]
fn the_two_string_wrappers_draw_and_count_as_retail_does() {
    differential!(scus(DRAW_COUNTED), scus(LABEL));
    let Some(mut rig) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let ops = sprite_ops(&mut rig);
    let mut rng = Rng(0x4C41_424C);
    let mut multi = 0;
    for round in 0..6_000 {
        // No author alias: the count's pre-walk reads those as they lie.
        let mut text = gen_text(&mut rng, &ops, false);
        if text[0] < 0x21 || text[0] & 0xF0 == 0xC0 {
            text.insert(0, b'B');
        }
        let (x, y) = (rng.below(300) as i32, rng.below(200) as i32);
        let ink0 = rng.below(10) as u16;
        let mut z = text.clone();
        z.extend([0u8; 8]);
        let what = format!("round {round}: {text:02x?} at ({x}, {y})");

        rig.cpu.mem.load(TEXT, &z);
        rig.cpu.mem.wr32(PAD, 0);
        rig.cpu.mem.wr32(INK, u32::from(ink0));
        rig.reset();
        rig.cpu.mem.wr32(STACK_TOP + 0x10, y as u32);
        assert_eq!(
            rig.cpu.call(DRAW_COUNTED, &[TEXT, 0, 0, x as u32]),
            Stop::Returned
        );
        let (glyphs, _, _) = port_line(&rig.font, &text, 0, (x, y), ink0);
        assert_eq!(
            rig.cpu.regs.r[V0],
            typewriter_glyph_count(&z, None).count,
            "{what}: the count"
        );
        assert_eq!(rig.glyphs(), glyphs, "{what}: the counted draw");

        // The label painter: every `|` line one pitch down, in one ink run.
        let plain: Vec<u8> = {
            let mut p = Vec::new();
            let mut i = 0;
            while i < text.len() {
                match text[i] {
                    0xCF | 0xCE => i += 2,
                    b => {
                        p.push(b);
                        i += 1;
                    }
                }
            }
            p
        };
        let port = legaia_engine_ui::label_lines_draws(&rig.font, &plain, (x, y), [1.0; 4]);
        let want: Vec<(i32, i32, u32, u32)> = port
            .iter()
            // A space's blank cell is laid, not drawn.
            .filter(|d| (d.src.0, d.src.1) != (0, 0))
            .map(|d| (d.dst.0, d.dst.1, d.src.0 / 14, d.src.1 / 15))
            .collect();
        let (lines, _, laid) = port_line(&rig.font, &plain, 0, (x, y), ink0);
        let lines: Vec<(i32, i32, u32, u32)> = lines.iter().map(|g| (g.0, g.1, g.2, g.3)).collect();
        assert_eq!(want, lines, "{what}: the label painter against the layout");
        rig.cpu.mem.load(TEXT, &{
            let mut z = plain.clone();
            z.extend([0u8; 8]);
            z
        });
        rig.reset();
        assert_eq!(
            rig.cpu.call(LABEL, &[TEXT, x as u32, y as u32]),
            Stop::Returned
        );
        let got: Vec<(i32, i32, u32, u32)> =
            rig.glyphs().iter().map(|g| (g.0, g.1, g.2, g.3)).collect();
        assert_eq!(got, want, "{what}: the label");
        assert_eq!(rig.cpu.regs.r[V0], laid, "{what}: the label's count");
        multi += u32::from(plain.contains(&0x7C));
    }
    assert!(multi > 1500, "{multi}");
}
