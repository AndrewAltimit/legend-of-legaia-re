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
