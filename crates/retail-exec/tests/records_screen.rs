//! The battle-records page `FUN_801ED710` (PROT 0897) against
//! `legaia_engine_dev_menu::dev_menu_leaves::records_screen` (the clamps,
//! the play-time split, the treasure percentage) and
//! `legaia_engine_ui::records_screen_fields` (the layout, emit order and
//! inks): random tallies, play clocks, character counters and treasure
//! pairs, with the label drawer `FUN_8003CC98`, the two numeral fields
//! `FUN_80034B78` / `FUN_80034E4C` and the glyph `FUN_8003C1F8` recorded
//! with the ink word `_DAT_8007B454`. The portrait icon `FUN_8002C488` is a
//! host seam the port leaves out; its calls are counted, not compared.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_dev_menu::dev_menu_leaves::{CharRecordStats, records_screen};
use legaia_engine_ui::{RecordsField, RecordsLabels, RecordsScreenView, records_screen_fields};
use psx_retail_exec::{A0, A1, A2, A3, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const RECORDS: u32 = 0x801E_D710;
const LABEL: u32 = 0x8003_CC98;
const BLANK_FIELD: u32 = 0x8003_4B78;
const ZERO_FIELD: u32 = 0x8003_4E4C;
const GLYPH: u32 = 0x8003_C1F8;
const ICON: u32 = 0x8002_C488;
const INK: u32 = 0x8007_B454;
const BLOCK: u32 = 0x8008_4140;
const TREASURE: u32 = 0x801C_6460;
/// The ten label strings, in emit order (`lui`/`addiu` pairs ahead of each
/// `jal 0x8003CC98`).
const LABEL_VAS: [u32; 10] = [
    0x801C_F51C,
    0x801C_F52C,
    0x801C_F53C,
    0x801C_F54C,
    0x801C_F55C,
    0x801C_F568,
    0x801C_F57C,
    0x801C_F588,
    0x801C_F590,
    0x801C_F59C,
];

fn ascii(m: &mut psx_retail_exec::Memory, mut at: u32) -> String {
    let mut s = String::new();
    loop {
        let b = m.rd8(at);
        if b == 0 {
            return s;
        }
        s.push(char::from(b));
        at += 1;
    }
}

#[test]
fn the_records_page_draws_what_retail_draws() {
    differential!(prot(897, RECORDS));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &disc().expect("disc").read_entry(897).expect("PROT 0897"),
    );
    let log = Rc::new(RefCell::new(Vec::<RecordsField>::new()));
    let icons = Rc::new(RefCell::new(0usize));
    let l = log.clone();
    cpu.hook(LABEL, move |r, m| {
        assert_eq!((r.r[A1], r.r[A2]), (0, 0));
        let y = m.rd32(r.r[SP] + 0x10) as i32;
        let ink = m.rd32(INK) as u8;
        let text = ascii(m, r.r[A0]);
        l.borrow_mut().push(RecordsField::Label {
            x: r.r[A3] as i32,
            y,
            text,
            ink,
        });
    });
    for (va, zero_pad) in [(BLANK_FIELD, false), (ZERO_FIELD, true)] {
        let l = log.clone();
        cpu.hook(va, move |r, m| {
            let ink = m.rd32(INK) as u8;
            l.borrow_mut().push(RecordsField::Number {
                x: r.r[A2] as i32,
                y: r.r[A3] as i32,
                value: i64::from(r.r[A0] as i32),
                digits: r.r[A1] as u8,
                zero_pad,
                ink,
            });
        });
    }
    let l = log.clone();
    cpu.hook(GLYPH, move |r, m| {
        let ink = m.rd32(INK) as u8;
        let ch = match r.r[A0] {
            9 => ':',
            6 => '/',
            0xD => '.',
            g => panic!("glyph {g}"),
        };
        l.borrow_mut().push(RecordsField::Symbol {
            x: r.r[A1] as i32,
            y: r.r[A2] as i32,
            ch,
            ink,
        });
    });
    let n = icons.clone();
    cpu.hook(ICON, move |_, _| *n.borrow_mut() += 1);

    let text: Vec<String> = LABEL_VAS
        .iter()
        .map(|&va| ascii(&mut cpu.mem, va))
        .collect();
    let labels = RecordsLabels {
        battles: &text[0],
        escapes: &text[1],
        max_hits: &text[2],
        max_damage: &text[3],
        knockouts: &text[4],
        monsters_defeated: &text[5],
        hyper_arts: &text[6],
        magic: &text[7],
        treasure: &text[8],
        percent: &text[9],
    };

    let mut rng = Rng(0x5245_4344);
    let (mut capped, mut shown) = (0, 0);
    for case in 0..4000 {
        // Each counter now and then past its display cap.
        let big = |rng: &mut Rng, cap: u32| {
            if rng.below(4) == 0 {
                cap + 1 + rng.below(cap)
            } else {
                rng.below(cap + 1)
            }
        };
        let battles = big(&mut rng, 99_999);
        let escapes = big(&mut rng, 99_999);
        let frames = if rng.below(4) == 0 {
            rng.next()
        } else {
            rng.below(60 * 60 * 60 * 120)
        };
        let chars: [CharRecordStats; 3] = std::array::from_fn(|_| CharRecordStats {
            max_hits: big(&mut rng, 999),
            max_damage: big(&mut rng, 9_999_999),
            knockouts: big(&mut rng, 999),
            monsters_defeated: big(&mut rng, 999_999),
            hyper_arts: rng.below(0x20) as u8,
            magic: rng.below(0x20) as u8,
        });
        let total = [0, -3, 1 + rng.below(400) as i32][rng.below(3) as usize];
        let found = if total > 0 {
            rng.below(total as u32 + 1) as i32
        } else {
            rng.below(5) as i32
        };
        let (x, y) = (rng.below(64) as i32, rng.below(48) as i32);

        cpu.mem.wr32(BLOCK + 0x564, battles);
        cpu.mem.wr32(BLOCK + 0x568, escapes);
        cpu.mem.wr32(BLOCK + 0x49C, frames);
        for (c, s) in chars.iter().enumerate() {
            let at = BLOCK + 0x414 * c as u32;
            cpu.mem.wr32(at + 0x6B4, s.max_hits);
            cpu.mem.wr32(at + 0x6B0, s.max_damage);
            cpu.mem.wr32(at + 0x660, s.knockouts);
            cpu.mem.wr32(at + 0x664, s.monsters_defeated);
            cpu.mem.wr8(at + 0x74D, s.hyper_arts);
            cpu.mem.wr8(at + 0x704, s.magic);
        }
        cpu.mem.wr16(TREASURE, found as u16);
        cpu.mem.wr16(TREASURE + 2, total as u16);
        log.borrow_mut().clear();
        *icons.borrow_mut() = 0;
        assert_eq!(
            cpu.call(RECORDS, &[x as u32, y as u32]),
            Stop::Returned,
            "case {case}"
        );

        let m = records_screen(battles, escapes, frames, &chars, found, total);
        let view = RecordsScreenView {
            battles: m.battles,
            escapes: m.escapes,
            play_hours: m.play_hours,
            play_minutes: m.play_minutes,
            play_seconds: m.play_seconds,
            max_hits: m.max_hits,
            max_damage: m.max_damage,
            knockouts: m.knockouts,
            monsters_defeated: m.monsters_defeated,
            hyper_arts: m.hyper_arts,
            magic: m.magic,
            treasure_found: m.treasure_found,
            treasure_total: m.treasure_total,
            treasure_percent: m.treasure_percent,
            treasure_fraction: m.treasure_fraction,
            treasure_percent_shown: m.treasure_percent_shown,
            labels,
        };
        let want = records_screen_fields(&view, (x, y));
        capped += usize::from(m.play_hours == 99 || battles > 99_999);
        shown += usize::from(m.treasure_percent_shown);
        let got = log.borrow().clone();
        for (k, (g, w)) in got.iter().zip(&want).enumerate() {
            assert_eq!(g, w, "case {case} draw {k}");
        }
        assert_eq!(
            got.len(),
            want.len(),
            "case {case}: found {found} total {total} tail {:?}",
            &got[want.len().min(got.len())..]
        );
        assert!(*icons.borrow() > 0, "case {case}: the portraits draw");
    }
    assert!(
        capped > 500 && shown > 1000,
        "capped {capped} shown {shown}"
    );
}
