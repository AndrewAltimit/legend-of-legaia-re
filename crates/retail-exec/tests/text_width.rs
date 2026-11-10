//! The text-width measurer `FUN_80035F04` (with its expander `FUN_80036514`)
//! against the two port measures that stand in for it: `Font::measure`, the
//! port of the expander and the line walk, and `Font::layout_padded`, which
//! callers read `advance_x` off. Generated strings over the printable glyphs,
//! the accent cells, newlines, the author escapes `0x5E X` / `0xFF X` and the
//! expanded `0xCE` / `0xCF` forms, at both glyph pads the game stores.

use crate::common::{Rng, disc, scus, scus_cpu};
use legaia_font::{Font, MeasureOptions};
use psx_retail_exec::{Stop, V0};

const MEASURE: u32 = 0x8003_5F04;
const PAD: u32 = 0x8007_40E8;
const TEXT: u32 = 0x8014_0000;

fn gen_text(rng: &mut Rng) -> Vec<u8> {
    let n = 1 + rng.below(24);
    let mut s = Vec::new();
    for _ in 0..n {
        match rng.below(20) {
            0 => s.push(0x7C),
            1 => s.extend([0x5E, 0x2D + rng.below(0x26) as u8]),
            2 => s.extend([0xCE, rng.below(0x26) as u8]),
            3 => s.extend([0xFF, 1 + rng.below(8) as u8]),
            4 => s.extend([0xCF, 1 + rng.below(8) as u8]),
            5 => s.push(0x80 + rng.below(0x40) as u8),
            // `0x5E` is the author escape's lead, never a glyph.
            _ => s.push(match 0x20 + rng.below(0x5C) as u8 {
                0x5E => 0x5F,
                b => b,
            }),
        }
    }
    s
}

#[test]
fn a_measured_width_is_the_retail_width() {
    differential!(scus(MEASURE));
    let (Some(mut cpu), Some(disc), Some(scus)) = (scus_cpu(), disc(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let tim = disc
        .read_prot_bytes(
            legaia_font::FONT_TIM_PROT_DAT_OFFSET,
            legaia_font::FONT_TIM_LEN,
        )
        .expect("font TIM");
    let font = Font::from_disc_tim_and_scus(&tim, &scus).expect("font");
    // BIOS A(0x2B) `memset`, the scratch line's clear.
    cpu.hook(0x8005_6788, |r, m| {
        for i in 0..r.r[psx_retail_exec::A2] {
            m.wr8(r.r[psx_retail_exec::A0] + i, r.r[psx_retail_exec::A1] as u8);
        }
    });
    let escapes = legaia_font::escape_icons::escape_table_from_scus(&scus).expect("escapes");
    let mut rng = Rng(0x5749_4454);
    let (mut measure_off, mut layout_off, mut escaped) = (0, 0, 0);
    let mut first = Vec::new();
    for round in 0..20_000 {
        let text = gen_text(&mut rng);
        let pad = rng.below(2);
        cpu.mem.wr32(PAD, pad);
        let mut z = text.clone();
        z.push(0);
        cpu.mem.load(TEXT, &z);
        assert_eq!(cpu.call(MEASURE, &[TEXT]), Stop::Returned, "round {round}");
        let retail = cpu.regs.r[V0];
        let opts = MeasureOptions {
            glyph_pad: pad,
            escapes: Some(&escapes),
            ..MeasureOptions::default()
        };
        let measured = font.measure(&text, &opts).max_px;
        // The default options carry the retail escape advances themselves.
        assert_eq!(
            font.measure(
                &text,
                &MeasureOptions {
                    glyph_pad: pad,
                    ..MeasureOptions::default()
                }
            )
            .max_px,
            measured,
            "{text:02x?}"
        );
        let laid = font.layout_padded(&text, pad).advance_x;
        escaped += i32::from(text.iter().any(|&b| b == 0x5E || b == 0xCE));
        if measured != retail {
            measure_off += 1;
            if first.len() < 6 {
                first.push(format!(
                    "measure {text:02x?} pad {pad}: retail {retail} port {measured}"
                ));
            }
        }
        layout_off += i32::from(laid != retail);
    }
    assert!(escaped > 5000, "{escaped}");
    assert!(first.is_empty(), "{measure_off} off:\n{}", first.join("\n"));
    // `layout_padded` lays out glyphs, not escapes: it is no width measure
    // for a string carrying `0x5E X` / `0xFF X` / `0xCE X`.
    assert!(layout_off > 5000, "{layout_off}");
}

const EXPAND: u32 = 0x8003_6514;
const OUT: u32 = 0x8015_0000;
/// The name tables the substitution tokens read.
const RECORD_NAMES: u32 = 0x8008_49AF;
const LEADER: u32 = 0x8008_4597;
const ITEMS: u32 = 0x8007_4368;
const MAGIC: u32 = 0x8007_54C8;
const SPELLS_2D: u32 = 0x8007_5EC4;
const INLINE_NAMES: u32 = 0x8007_3F24;

/// The substitution expander alone, on a string carrying every token it has
/// an arm for. The name tables are seeded with generated strings (a member
/// name token inside some, which is expanded once more), and the port's
/// resolver hook reads the same tables.
#[test]
fn the_expander_writes_the_bytes_retail_writes() {
    differential!(scus(EXPAND));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4558_5044);
    let word = |rng: &mut Rng, nest: bool| -> Vec<u8> {
        let mut s: Vec<u8> = (0..1 + rng.below(6))
            .map(|_| b'A' + rng.below(26) as u8)
            .collect();
        if nest && rng.below(3) == 0 {
            s.extend([0xC1, [0u8, 1, 2, 0x63][rng.below(4) as usize]]);
            s.push(b'z');
        }
        s
    };
    let put = |cpu: &mut psx_retail_exec::Cpu, at: u32, s: &[u8]| {
        for (i, c) in s.iter().chain(&[0]).enumerate() {
            cpu.mem.wr8(at + i as u32, *c);
        }
    };
    // Three member names, eight item, magic and spell names, eight inline.
    for id in 0..3u32 {
        let n = word(&mut rng, false);
        put(&mut cpu, RECORD_NAMES + id * 0x414, &n);
    }
    let pool = 0x8016_0000;
    for k in 0..8u32 {
        let (a, b, c) = (
            pool + k * 0x40,
            pool + 0x400 + k * 0x40,
            pool + 0x800 + k * 0x40,
        );
        let w = word(&mut rng, true);
        put(&mut cpu, a, &w);
        let w = word(&mut rng, true);
        put(&mut cpu, b, &w);
        let w = word(&mut rng, true);
        put(&mut cpu, c, &w);
        cpu.mem.wr32(ITEMS + k * 12 + 4, a);
        cpu.mem.wr32(MAGIC + k * 12 + 8, b);
        // The spell table's rows are `[hi][lo]`-keyed, ascending.
        cpu.mem.wr8(SPELLS_2D + k * 20, (k / 4) as u8);
        cpu.mem.wr8(SPELLS_2D + k * 20 + 1, (k % 4) as u8);
        cpu.mem.wr32(SPELLS_2D + k * 20 + 0xC, c);
        let w = word(&mut rng, false);
        assert!(w.len() < 8);
        put(&mut cpu, INLINE_NAMES + k * 8, &w);
    }
    let (mut nested, mut dropped, mut subs) = (0, 0, 0);
    for round in 0..20_000 {
        cpu.mem.wr8(LEADER, rng.below(3) as u8);
        let mut text = Vec::new();
        for _ in 0..1 + rng.below(16) {
            match rng.below(14) {
                0 => text.extend([0x5E, 0x2D + rng.below(0x26) as u8]),
                1 => text.extend([0xCE, rng.below(0x26) as u8]),
                2 => text.extend([0xFF, 1 + rng.below(8) as u8]),
                3 => text.extend([0xCF, 1 + rng.below(8) as u8]),
                4 => text.extend([0xC1, [0u8, 1, 2, 0x63][rng.below(4) as usize]]),
                5 => text.extend([[0xC2u8, 0xC4][rng.below(2) as usize], rng.below(8) as u8]),
                6 => text.extend([0xC3, rng.below(8) as u8]),
                7 => text.extend([
                    0xC5,
                    [0u8, 1, 2, 3, 0x40, 0x41, 0x42, 0x43][rng.below(8) as usize],
                ]),
                8 => text.extend([0xC7, rng.below(8) as u8]),
                // A lead the copy pass has no arm for: it goes through
                // alone, and its operand is read as the next token.
                9 => text.extend([
                    0xC8 + rng.below(6) as u8,
                    [b'q', 0x5E, 0xFF, 0xCE, 0xCF, 0xC7][rng.below(6) as usize],
                    1 + rng.below(7) as u8,
                ]),
                _ => text.push(match 0x20 + rng.below(0x5C) as u8 {
                    0x5E => 0x5F,
                    b => b,
                }),
            }
        }
        let keep = rng.below(2);
        let mut z = text.clone();
        z.extend([0u8; 4]);
        cpu.mem.load(TEXT, &z);
        cpu.mem.load(OUT, &[0xEEu8; 0x200]);
        assert_eq!(
            cpu.call(EXPAND, &[OUT, TEXT, keep]),
            Stop::Returned,
            "round {round}"
        );
        // Everything written, less the closing NUL: a `0xCE 00` pair in
        // the middle is not the end. `0xEE` is never generated.
        let mut retail = cpu.mem.read(OUT, 0x200);
        let end = retail
            .iter()
            .rposition(|&b| b != 0xEE)
            .expect("a terminator");
        assert_eq!(retail[end], 0, "round {round}: the closing NUL");
        retail.truncate(end);

        let mem = std::cell::RefCell::new(&mut cpu.mem);
        let expand = |op: u8, arg: u8| -> Option<Vec<u8>> {
            let mut m = mem.borrow_mut();
            let arg32 = u32::from(arg);
            let at = match op {
                0xC1 => {
                    let id = if arg == 0x63 {
                        u32::from(m.rd8(LEADER))
                    } else {
                        arg32
                    };
                    RECORD_NAMES + id * 0x414
                }
                0xC2 | 0xC4 => m.rd32(ITEMS + arg32 * 12 + 4),
                0xC3 => m.rd32(MAGIC + arg32 * 12 + 8),
                0xC5 => {
                    // `0x8003672C..0x800367B0`: the first row whose key is
                    // `(arg / 0x40, arg % 0x40)`, among rows up to that `hi`.
                    let (hi, lo) = (arg32 / 0x40, arg32 % 0x40);
                    let mut row = 0;
                    while u32::from(m.rd8(SPELLS_2D + row * 20)) <= hi {
                        if u32::from(m.rd8(SPELLS_2D + row * 20)) == hi
                            && u32::from(m.rd8(SPELLS_2D + row * 20 + 1)) == lo
                        {
                            break;
                        }
                        row += 1;
                    }
                    m.rd32(SPELLS_2D + row * 20 + 0xC)
                }
                0xC7 => INLINE_NAMES + arg32 * 8,
                _ => return None,
            };
            // A name's bytes up to its NUL - a nested member token's
            // operand is not a terminator, `0` (the first member) included.
            let mut name = Vec::new();
            let mut a = at;
            loop {
                let b = m.rd8(a);
                if b == 0 {
                    break;
                }
                name.push(b);
                a += 1;
                if b == 0xC1 {
                    name.push(m.rd8(a));
                    a += 1;
                }
            }
            Some(name)
        };
        let opts = MeasureOptions {
            expand: Some(&expand),
            ..MeasureOptions::default()
        };
        let (port, unresolved) = legaia_font::measure::expand_text(&text, &opts, keep != 0);
        assert!(unresolved.is_empty(), "round {round}: {unresolved:02x?}");
        dropped += u32::from(keep == 0 && text.iter().any(|&b| b == 0xFF || b == 0xCF));
        nested += u32::from(retail.windows(1).any(|w| w[0] == b'z'));
        subs += u32::from(text.iter().any(|b| (0xC1..=0xC7).contains(b)));
        assert_eq!(retail, port, "round {round}: {text:02x?} keep {keep}");
    }
    assert!(
        nested > 500 && dropped > 3000 && subs > 5000,
        "{nested} {dropped} {subs}"
    );
}
