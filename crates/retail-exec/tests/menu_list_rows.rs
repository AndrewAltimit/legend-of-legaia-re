//! The list-row kernels of `SCUS_942.54` the menus share: the row-name
//! resolver `FUN_8002FF8C` against `row_name_source` over every class nibble
//! and payload (the source resolved to the pointer it names), the list-node
//! allocator `FUN_80030104` against `list_alloc` (the persisted selection it
//! clamps in place, the node header and the mirrors it resets), and the equip
//! stat-field accessor `FUN_800302E4` against `equip_stat_field` over every
//! id-space tag and field, with the favour term `FUN_801DD0C0` hooked; and
//! the highlighted-row description dispatcher `FUN_80034250` against
//! `row_description_source`, the text drawer `FUN_800337B0` hooked.

use crate::common::{Rng, scus_cpu};
use legaia_engine_menus::equipment::equip_stat_field;
use legaia_engine_menus::menu_list_rows::{
    DescriptionSource, ListAlloc, ListSelection, RowNameSource, list_alloc, row_description_source,
    row_name_source,
};
use legaia_game_tables::equip_stats::EquipBonus;
use psx_retail_exec::{A0, A1, A2, A3, Cpu, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const ROW_NAME: u32 = 0x8002_FF8C;
const LIST_ALLOC: u32 = 0x8003_0104;
const STAT_FIELD: u32 = 0x8003_02E4;
const DESCRIBE: u32 = 0x8003_4250;
const DRAW_TEXT: u32 = 0x8003_37B0;
const EFFECTS: u32 = 0x8007_52C0;
const PASSIVES: u32 = 0x8007_625C;
const GP: u32 = 0x8007_B318;
const ITEMS: u32 = 0x8007_4368;
const SPELLS: u32 = 0x8007_54C8;
const PLACES: u32 = 0x8007_3A98;
const PLACE_NAMES: u32 = 0x8007_3B18;
const VERBS: u32 = 0x8007_329C;
const BAG: u32 = 0x8008_5958;
const EQUIP: u32 = 0x8007_4F68;
const MALLOC: u32 = 0x8001_7888;
const FREE: u32 = 0x8001_7B94;
const TEXT_WIDTH: u32 = 0x8003_CC90;
const FAVOUR: u32 = 0x801D_D0C0;
const WINDOW: u32 = 0x8012_0000;
const NODE: u32 = 0x8013_0000;
const OUT: u32 = 0x8012_1000;
const TEXT: u32 = 0x8012_2000;

fn resolve(cpu: &mut Cpu, src: RowNameSource) -> u32 {
    let m = &mut cpu.mem;
    match src {
        RowNameSource::Empty => m.rd32(GP + 0x168),
        RowNameSource::Spell(id) => m.rd32(SPELLS + u32::from(id) * 12 + 8),
        RowNameSource::Item(id) => m.rd32(ITEMS + u32::from(id) * 12 + 4),
        RowNameSource::BagSlot(slot) => {
            let id = u32::from(m.rd8(BAG + 2 * u32::from(slot)));
            m.rd32(ITEMS + id * 12 + 4)
        }
        RowNameSource::Landmark(i) => {
            PLACE_NAMES + u32::from(m.rd8(PLACES + 6 * u32::from(i))) * 0x20
        }
        RowNameSource::Verb(i) => m.rd32(VERBS + 4 * u32::from(i)),
    }
}

#[test]
fn the_row_name_resolver_names_what_retail_names() {
    differential!(scus(ROW_NAME));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x524F_574E);
    for i in 0..0x200u32 {
        cpu.mem.wr8(BAG + 2 * i, rng.next() as u8);
    }
    cpu.mem.wr32(GP + 0x168, 0x8007_0000 | rng.below(0x1000));
    let mut classes = [0u32; 16];
    for case in 0..40000 {
        // The tables hold 256-ish records; the payload is masked to 10 bits,
        // so keep it inside RAM the tables can index.
        let payload = rng.below(0x100) | ((rng.below(4) << 10) * u32::from(case % 7 == 0));
        let entry = (rng.below(16) << 12) | payload | (rng.below(4) << 10);
        assert_eq!(cpu.call(ROW_NAME, &[entry]), Stop::Returned);
        let src = row_name_source(entry as u16);
        let want = resolve(&mut cpu, src);
        assert_eq!(cpu.regs.r[V0], want, "entry {entry:#06x} -> {src:?}");
        classes[(entry >> 12) as usize] += 1;
    }
    assert!(classes.iter().all(|&n| n > 1000), "{classes:?}");
}

#[test]
fn the_list_node_allocator_seeds_what_retail_seeds() {
    differential!(scus(LIST_ALLOC));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let frees = Rc::new(RefCell::new(0));
    cpu.hook(MALLOC, |r, _| r.r[V0] = NODE);
    let f = frees.clone();
    cpu.hook(FREE, move |_, _| *f.borrow_mut() += 1);
    cpu.hook(TEXT_WIDTH, |r, m| {
        let mut n = 0;
        while m.rd8(r.r[A0] + n) != 0 {
            n += 1;
        }
        r.r[V0] = n * 7;
    });
    let mut rng = Rng(0x4C49_5354);
    let (mut lists, mut texts, mut clamped) = (0, 0, 0);
    for case in 0..20000 {
        let count = match rng.below(4) {
            0 => 0,
            1 => 1,
            _ => 1 + rng.below(80) as i32,
        };
        let mut persisted = ListSelection {
            scroll_top: rng.below(90) as i32 - 2,
            selected: rng.below(90) as i32 - 2,
        };
        let content_h =
            [0i16, 4, 14, 18, 100, 150, 200, -20][rng.below(8) as usize] + rng.below(14) as i16;
        cpu.mem.wr32(GP + 0x878, persisted.scroll_top as u32);
        cpu.mem.wr32(GP + 0x880, persisted.selected as u32);
        for off in [0x848, 0x870, 0x884, 0x888] {
            cpu.mem.wr32(GP + off, 0x5A5A_5A5A);
        }
        for i in 0..0x30 {
            cpu.mem.wr8(NODE + i, 0xEE);
            cpu.mem.wr8(WINDOW + i, 0);
        }
        cpu.mem.wr16(WINDOW + 0x10, content_h as u16);
        let text: Vec<u8> = (0..rng.below(20))
            .map(|_| b'a' + rng.below(26) as u8)
            .collect();
        cpu.mem.load(TEXT, &text);
        cpu.mem.wr8(TEXT + text.len() as u32, 0);
        cpu.mem.wr32(OUT, 0xDEAD);
        *frees.borrow_mut() = 0;
        assert_eq!(
            cpu.call(LIST_ALLOC, &[OUT, WINDOW, count as u32, TEXT]),
            Stop::Returned
        );
        let before = persisted;
        let what = format!("case {case} count {count} h {content_h} {before:?}");
        match list_alloc(count, content_h, &mut persisted) {
            ListAlloc::EmptyText => {
                texts += 1;
                assert_eq!(cpu.mem.rd32(OUT), 0, "{what}");
                assert_eq!(cpu.mem.rd32(WINDOW + 0x18), NODE, "{what}");
                assert_eq!(
                    cpu.mem.read(NODE, text.len() + 1)[..text.len()],
                    text[..],
                    "{what}"
                );
                assert_eq!(cpu.mem.rd16(WINDOW + 0x14), 0x80, "{what}");
                assert_eq!(cpu.mem.rd16(WINDOW + 0x12), 7 * text.len() as u16, "{what}");
                assert_eq!(cpu.mem.rd32(GP + 0x888), 0, "{what}");
            }
            ListAlloc::Node(n) => {
                lists += 1;
                assert_eq!(cpu.mem.rd32(OUT), NODE, "{what}");
                assert_eq!(cpu.mem.rd32(WINDOW + 0x18), NODE, "{what}");
                let header = [
                    cpu.mem.rd16(NODE),
                    cpu.mem.rd16(NODE + 2),
                    cpu.mem.rd16(NODE + 4),
                    cpu.mem.rd16(NODE + 6),
                ];
                assert_eq!(
                    header,
                    [n.scroll_top, n.visible_rows, n.count, n.selected],
                    "{what}"
                );
                assert_eq!(cpu.mem.rd16(NODE + 0x28), 0, "{what}");
                assert_eq!(cpu.mem.rd32(GP + 0x888) as i32, count, "{what}");
                assert_eq!(cpu.mem.rd32(GP + 0x884), 0, "{what}");
                assert_eq!(cpu.mem.rd32(GP + 0x870), 0, "{what}");
                assert_eq!(cpu.mem.rd32(GP + 0x848), u32::MAX, "{what}");
            }
        }
        assert_eq!(
            cpu.mem.rd32(GP + 0x878) as i32,
            persisted.scroll_top,
            "{what}"
        );
        assert_eq!(
            cpu.mem.rd32(GP + 0x880) as i32,
            persisted.selected,
            "{what}"
        );
        clamped += u32::from(persisted != before);
    }
    assert!(
        lists > 5000 && texts > 2000 && clamped > 2000,
        "{lists} {texts} {clamped}"
    );
}

#[test]
fn the_equip_stat_field_reads_what_retail_reads() {
    differential!(scus(STAT_FIELD));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The favour term is overlay-resident; a function of its arguments here.
    let favour = |id: u32| (id * 7 + 3) & 0x1F;
    cpu.hook(FAVOUR, move |r, _| {
        assert_eq!(r.r[A2], 0);
        r.r[V0] = favour(r.r[A1]);
    });
    let mut rng = Rng(0x4551_5550);
    for i in 0..0x400u32 {
        cpu.mem.wr8(BAG + 2 * i, rng.next() as u8);
    }
    let mut snapshot = |a: u32| cpu.mem.rd8(a);
    let bag: Vec<u8> = (0..0x400u32).map(|i| snapshot(BAG + 2 * i)).collect();
    // A direct id reaches 10 bits, past the 256-record table, as in retail.
    let items: Vec<u8> = (0..0x400u32)
        .map(|i| snapshot(ITEMS + i * 12 + 1))
        .collect();
    let records: Vec<[u8; 8]> = (0..256u32)
        .map(|n| std::array::from_fn(|k| snapshot(EQUIP + n * 8 + k as u32)))
        .collect();
    let mut nonzero = 0;
    for case in 0..40000 {
        let id = (rng.below(16) << 12) | rng.below(0x400);
        let field = rng.below(6);
        assert_eq!(cpu.call(STAT_FIELD, &[0x55, id, field]), Stop::Returned);
        let r = cpu.regs.r[V0];
        let p = equip_stat_field(
            id as u16,
            field as u8,
            |slot| bag[usize::from(slot)],
            // Retail's lookup is unconditional: item record `+1` names the
            // equip record whatever the item is.
            |item| {
                Some(EquipBonus {
                    raw: records[usize::from(items[usize::from(item)])],
                })
            },
            |item| favour(u32::from(item)),
        );
        assert_eq!(r, p, "case {case} id {id:#06x} field {field}");
        nonzero += u32::from(r != 0);
    }
    assert!(nonzero > 2000, "{nonzero}");
}

#[test]
fn the_description_dispatcher_draws_what_retail_draws() {
    differential!(scus(DESCRIBE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let drawn = Rc::new(RefCell::new(None));
    let d = drawn.clone();
    cpu.hook(DRAW_TEXT, move |r, _| {
        *d.borrow_mut() = Some((r.r[A0], r.r[A1], r.r[A2], r.r[A3]));
    });
    let mut rng = Rng(0x4445_5343);
    for i in 0..0x100u32 {
        let id = if rng.below(5) == 0 {
            0
        } else {
            rng.next() as u8
        };
        cpu.mem.wr8(BAG + 2 * i, id);
    }
    let mut seen = [0u32; 3];
    for case in 0..20000 {
        let mode = [0u32, 1, 2, 3, 4, 4][rng.below(6) as usize];
        let class = (rng.below(16) << 12)
            | if rng.below(8) == 0 {
                rng.below(0x1000)
            } else {
                0
            };
        let payload = rng.below(0x100);
        let (wx, wy) = (rng.next() as i16, rng.next() as i16);
        cpu.mem.wr32(GP + 0x87C, mode);
        cpu.mem.wr32(GP + 0x884, class);
        cpu.mem.wr32(GP + 0x870, payload);
        cpu.mem.wr16(WINDOW + 0xA, wx as u16);
        cpu.mem.wr16(WINDOW + 0xC, wy as u16);
        *drawn.borrow_mut() = None;
        assert_eq!(cpu.call(DESCRIBE, &[WINDOW]), Stop::Returned);
        let bag: Vec<u8> = (0..0x100u32).map(|i| cpu.mem.rd8(BAG + 2 * i)).collect();
        let src = row_description_source(mode as i32, class as u16, payload as u16, |s| {
            bag[usize::from(s)]
        });
        let what = format!("case {case} mode {mode} class {class:#x} payload {payload}");
        let (k, want) = match src {
            DescriptionSource::None => (0, None),
            DescriptionSource::ItemDesc(id) => {
                let text = cpu.mem.rd32(ITEMS + u32::from(id) * 12 + 8);
                (1, Some((text, u32::from(id))))
            }
            DescriptionSource::PassiveDesc(id) => {
                let sub = u32::from(cpu.mem.rd8(ITEMS + u32::from(id) * 12 + 1));
                let passive = u32::from(cpu.mem.rd8(EFFECTS + sub * 4 + 3));
                (
                    2,
                    Some((cpu.mem.rd32(PASSIVES + passive * 12 + 8), u32::from(id))),
                )
            }
        };
        let got = drawn.borrow().map(|(t, id, x, y)| {
            assert_eq!((x as i16, y as i16), (wx, wy), "{what}");
            (t, id)
        });
        assert_eq!(got, want, "{what}: {src:?}");
        seen[k] += 1;
    }
    assert!(seen.iter().all(|&n| n > 300), "{seen:?}");
}

const WINDOW_UPSERT: u32 = 0x8003_2434;
const HEAP_ALLOC: u32 = 0x8001_7888;
const WINDOW_CONTENT: u32 = 0x8003_0628;
const WINDOW_LIST: u32 = GP + 0x148;

/// The live-window register `FUN_80032434` against `LiveWindowSet::upsert`
/// and `record_string_glyph_count`: a run of registrations over a few ids,
/// the heap allocator `FUN_80017888` a bump allocator and the content
/// rebuild `FUN_80030628` recorded, the whole circular list walked after
/// each and held to the port's set in order, field by field.
#[test]
fn the_window_register_keeps_the_list_retail_keeps() {
    differential!(scus(WINDOW_UPSERT));
    use legaia_engine_menus::menu_list_rows::{
        LiveWindow, LiveWindowSet, record_string_glyph_count,
    };
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    const HEAP: u32 = 0x8015_0000;
    const CONFIG: u32 = 0x8014_0000;
    const RECORDS: u32 = 0x8014_1000;
    let next = Rc::new(RefCell::new(HEAP));
    let n = next.clone();
    cpu.hook(HEAP_ALLOC, move |r, _| {
        let mut p = n.borrow_mut();
        r.r[V0] = *p;
        *p += (r.r[A1] + 3) & !3;
    });
    cpu.stub(0x8003_C110, 0);
    let rebuilt = Rc::new(RefCell::new(Vec::new()));
    let b = rebuilt.clone();
    cpu.hook(WINDOW_CONTENT, move |r, m| {
        b.borrow_mut().push(m.rd16(r.r[A0] + 8))
    });
    let mut rng = Rng(0x5749_4E44);
    let mut reused = 0;
    for run in 0..40 {
        cpu.mem.wr32(WINDOW_LIST, 0);
        let mut port = LiveWindowSet::new();
        for step in 0..40 {
            let id = rng.below(24) as u16;
            // A record string: `[len][len * 2 bytes]` segments, zero-ended;
            // now and then none at all.
            let record = if rng.below(8) == 0 {
                0
            } else {
                let at = RECORDS + 0x100 * (step % 16);
                let mut o = at;
                for _ in 0..rng.below(5) {
                    let len = rng.below(12) as u8;
                    if len == 0 {
                        break;
                    }
                    cpu.mem.wr8(o, len);
                    for k in 0..u32::from(len) * 2 {
                        cpu.mem.wr8(o + 1 + k, 1 + rng.below(0xFE) as u8);
                    }
                    o += 1 + u32::from(len) * 2;
                }
                cpu.mem.wr8(o, 0);
                at
            };
            let half = |rng: &mut Rng| rng.next() as u16;
            let cfg: [u16; 12] = std::array::from_fn(|_| half(&mut rng));
            cpu.mem.wr32(CONFIG, record);
            for (k, v) in cfg.iter().enumerate().skip(2) {
                cpu.mem.wr16(CONFIG + 2 * k as u32, *v);
            }
            rebuilt.borrow_mut().clear();
            let node = {
                assert_eq!(
                    cpu.call(WINDOW_UPSERT, &[u32::from(id), CONFIG]),
                    Stop::Returned
                );
                cpu.regs.r[V0]
            };
            assert_eq!(
                *rebuilt.borrow(),
                vec![id],
                "run {run} step {step}: one rebuild"
            );
            reused += usize::from(port.get(id).is_some());
            let glyphs = if record == 0 {
                0
            } else {
                let bytes = cpu.mem.read(record, 0x100);
                record_string_glyph_count(&bytes)
            };
            port.upsert(LiveWindow {
                id,
                x: cfg[8] as i16,
                y: cfg[9] as i16,
                w: cfg[6] as i16,
                h: cfg[7] as i16,
                content_id: cfg[2] as u8,
                sub_kind: cfg[3] as u8,
                glyph_count: glyphs,
                slide_active: true,
            });
            // The slide record the node carries: current and target both
            // the rect origin, the start the config's `+0x08` / `+0x0A`.
            let slide = cpu.mem.rd32(node + 0x24);
            let s: [u16; 7] = std::array::from_fn(|k| cpu.mem.rd16(slide + 2 * k as u32));
            assert_eq!(
                s,
                [cfg[10], cfg[8], cfg[9], cfg[8], cfg[9], cfg[4], cfg[5]],
                "run {run} step {step}: the slide record"
            );
            // Walk the circular list from the sentinel.
            let head = cpu.mem.rd32(WINDOW_LIST);
            let mut got = Vec::new();
            let mut p = cpu.mem.rd32(head);
            while p != head {
                let m = &mut cpu.mem;
                got.push(LiveWindow {
                    id: m.rd16(p + 8),
                    x: m.rd16(p + 0xA) as i16,
                    y: m.rd16(p + 0xC) as i16,
                    w: m.rd16(p + 0xE) as i16,
                    h: m.rd16(p + 0x10) as i16,
                    content_id: m.rd8(p + 0x1C),
                    sub_kind: m.rd8(p + 0x1D),
                    glyph_count: m.rd16(p + 0x14),
                    slide_active: m.rd16(p + 0x20) == 1,
                });
                let fwd = m.rd32(p);
                assert_eq!(m.rd32(fwd + 4), p, "the back link");
                p = m.rd32(p);
                assert!(got.len() <= 64, "a loop in the list");
            }
            let want: Vec<LiveWindow> = port.iter().cloned().collect();
            assert_eq!(got, want, "run {run} step {step}");
        }
    }
    assert!(reused > 400, "re-registrations ran {reused} times");
}

const ITEM_DETAIL: u32 = 0x8003_37B0;
const DRAW_LINES: u32 = 0x8003_CD00;
const DRAW_RAW: u32 = 0x8003_6888;
const BLIT_CELL: u32 = 0x8003_C11C;

/// One draw the item-detail printer makes: a string (which drawer, the raw
/// bytes, the pen) or a numeral cell (the digit, the pen).
#[derive(Debug, Clone, PartialEq, Eq)]
enum DetailDraw {
    Lines(Vec<u8>, i16, i16),
    Raw(Vec<u8>, i16, i16),
    Digit(u8, i16, i16),
}

/// A menu string in RAM, raw: two-byte escapes kept whole even when their
/// operand is `0` (`ItemDetailTables`' reader).
fn raw_string(m: &mut psx_retail_exec::Memory, mut at: u32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = m.rd8(at);
        match b {
            0 => break,
            0xC1..=0xC7 | 0xCE | 0xCF => {
                out.extend([b, m.rd8(at + 1)]);
                at += 2;
            }
            _ => {
                out.push(b);
                at += 1;
            }
        }
        assert!(out.len() < 0x200, "an unterminated string");
    }
    out
}

/// The item-detail printer `FUN_800337B0` against `item_detail_line`, over
/// every item id with its own description and, for every equipment id, with
/// none (the slot-template arm): the string drawers `FUN_8003CD00` /
/// `FUN_80036888` and the numeral blit `FUN_8003C11C` recorded. The text
/// is the line's, at the pen; each stat's label at its `label_dx`, one row
/// down; its value in three 8-px cells from `value_dx`, right-aligned with
/// leading zeros blank.
#[test]
fn the_item_detail_printer_draws_what_retail_draws() {
    differential!(scus(ITEM_DETAIL));
    use legaia_engine_menus::shop::{DETAIL_STAT_DY, ItemDetailTables, item_detail_line};
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let tables = ItemDetailTables::from_scus(&crate::common::scus().unwrap()).unwrap();
    let draws = Rc::new(RefCell::new(Vec::<DetailDraw>::new()));
    let d = draws.clone();
    cpu.hook(DRAW_LINES, move |r, m| {
        let s = raw_string(m, r.r[A0]);
        d.borrow_mut()
            .push(DetailDraw::Lines(s, r.r[A1] as i16, r.r[A2] as i16));
    });
    let d = draws.clone();
    cpu.hook(DRAW_RAW, move |r, m| {
        assert_eq!((r.r[A1], r.r[A2]), (0, 0), "a raw draw from the start");
        let s = raw_string(m, r.r[A0]);
        let y = m.rd32(r.r[29] + 0x10);
        d.borrow_mut()
            .push(DetailDraw::Raw(s, r.r[A3] as i16, y as i16));
    });
    let d = draws.clone();
    cpu.hook(BLIT_CELL, move |r, m| {
        let (pos, rect) = (r.r[A0], r.r[A1]);
        assert_eq!(
            (m.rd16(rect + 2), m.rd16(rect + 4), m.rd16(rect + 6)),
            (0xD0, 8, 12),
            "a numeral cell"
        );
        let u = m.rd16(rect);
        assert_eq!(u % 8, 0);
        d.borrow_mut().push(DetailDraw::Digit(
            (u / 8) as u8,
            m.rd16(pos) as i16,
            m.rd16(pos + 2) as i16,
        ));
    });
    const EMPTY: u32 = 0x8014_2000;
    cpu.mem.wr8(EMPTY, 0);
    let mut rng = Rng(0x4954_454D);
    let (mut templated, mut equipped) = (0, 0);
    for pass in 0..2 {
        for id in 0..=255u8 {
            let is_equip = tables.kind[usize::from(id)] == 1;
            if pass == 1 && !is_equip {
                continue;
            }
            let desc = if pass == 0 {
                cpu.mem.rd32(ITEMS + 12 * u32::from(id) + 8)
            } else {
                EMPTY
            };
            if desc == 0 {
                continue;
            }
            let (x, y) = (rng.below(200) as i16, rng.below(200) as i16);
            cpu.mem.wr32(GP + 0x15C, 0);
            draws.borrow_mut().clear();
            let args = [desc, u32::from(id), x as u32, y as u32];
            assert_eq!(cpu.call(ITEM_DETAIL, &args), Stop::Returned, "id {id:#x}");

            let mut t = tables.clone();
            if pass == 1 {
                t.desc[usize::from(id)].clear();
            }
            let line = item_detail_line(&t, id);
            let mut want = Vec::new();
            let own = !t.desc[usize::from(id)].is_empty();
            if own {
                want.push(DetailDraw::Lines(line.text.clone(), x, y));
            } else if is_equip {
                templated += 1;
                want.push(DetailDraw::Raw(line.text.clone(), x, y));
            }
            let sy = y + DETAIL_STAT_DY as i16;
            for st in &line.stats {
                let label: Vec<u8> = t.stat_labels[st.label].bytes().collect();
                want.push(DetailDraw::Raw(label, x + st.label_dx as i16, sy));
                let digits = [st.value / 100, st.value / 10 % 10, st.value % 10];
                let mut lead = true;
                for (k, dg) in digits.into_iter().enumerate() {
                    lead &= dg == 0 && k < 2;
                    if !lead {
                        want.push(DetailDraw::Digit(
                            dg,
                            x + st.value_dx as i16 + 8 * k as i16,
                            sy,
                        ));
                    }
                }
            }
            equipped += usize::from(!line.stats.is_empty());
            assert_eq!(*draws.borrow(), want, "pass {pass} id {id:#x}");
        }
    }
    assert!(
        templated > 50 && equipped > 100,
        "templated {templated} equipped {equipped}"
    );
}

const WINDOW_BUILD: u32 = 0x8003_0628;
/// The list memory the hooked allocator grants, and the word saying whether
/// it grants it.
const LIST_BUF: u32 = 0x8013_4000;
const GRANT: u32 = 0x8013_3FF0;

/// The rows the builder copied into a granted list (`+0x28`), as many as
/// `want` holds and with the word past them untouched; none when refused.
fn list_rows(cpu: &mut Cpu, grant: u32, want: &[u16]) -> Vec<u16> {
    if grant == 0 {
        assert_eq!(
            cpu.mem.rd16(LIST_BUF + 0x28),
            0xDEAD,
            "a refused list is not written"
        );
        return want.to_vec();
    }
    let n = want.len() as u32;
    assert_eq!(
        cpu.mem.rd16(LIST_BUF + 0x28 + 2 * n),
        0xDEAD,
        "no row past the list"
    );
    (0..n)
        .map(|k| cpu.mem.rd16(LIST_BUF + 0x28 + 2 * k))
        .collect()
}
const APPLICABLE: u32 = 0x8003_043C;
const BAG_WINDOW: u32 = 0x8004_313C;
const STAGED_ROWS: u32 = 0x801C_6020;
const ROW_FLAGS: u32 = 0x1F80_0394;

/// The item tables the bag builders read, as written into RAM for one case.
struct RowTables {
    items: Vec<[u8; 4]>,
    effects: Vec<[u8; 4]>,
    equip_flags: Vec<u8>,
}

impl legaia_engine_menus::menu_list_rows::ItemRowTables for RowTables {
    fn kind(&self, id: u8) -> u8 {
        self.items[usize::from(id)][0]
    }
    fn subtype(&self, id: u8) -> u8 {
        self.items[usize::from(id)][1]
    }
    fn price(&self, id: u8) -> u16 {
        let r = self.items[usize::from(id)];
        u16::from_le_bytes([r[2], r[3]])
    }
    fn effect_flags(&self, subtype: u8) -> u8 {
        self.effects[usize::from(subtype)][2]
    }
    fn effect_marker(&self, subtype: u8) -> u8 {
        self.effects[usize::from(subtype)][3]
    }
    fn equip_flags(&self, subtype: u8) -> u8 {
        self.equip_flags[usize::from(subtype)]
    }
}

/// Whether the hooked applicability probe answers yes for `id`.
fn applicable(id: u8) -> bool {
    (u32::from(id).wrapping_mul(0x9E37_79B9) >> 7) & 3 != 0
}

/// The window-content dispatcher `FUN_80030628`'s three bag-list arms -
/// content id `2` (the price-gated sell list), `3` (the Items Use list, in
/// the field and in battle, with both door gates) and `0x22` (Throw Out) -
/// against `build_price_gated_rows` / `build_use_list_rows` /
/// `build_throw_out_rows`: random item, effect and equipment tables written
/// over the executable's, random bags and window ranges, the bag-window
/// helper `FUN_8004313C` stubbed (the range is seeded), the applicability
/// probe `FUN_8003043C` scripted, and the list allocator `FUN_80030104`
/// hooked to read the staged row words it is handed.
#[test]
fn the_bag_list_arms_build_the_rows_retail_builds() {
    differential!(scus(WINDOW_BUILD));
    use legaia_engine_menus::menu_list_rows::{
        UseListCtx, build_price_gated_rows, build_throw_out_rows, build_use_list_rows,
    };
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.stub(BAG_WINDOW, 0);
    cpu.hook(APPLICABLE, |r, _| {
        r.r[V0] = u32::from(applicable(r.r[A0] as u8))
    });
    let built = Rc::new(RefCell::new(None::<Vec<u16>>));
    let b = built.clone();
    cpu.hook(LIST_ALLOC, move |r, m| {
        let n = r.r[A2];
        *b.borrow_mut() = Some((0..n).map(|k| m.rd16(STAGED_ROWS + 2 * k)).collect());
        // Grant the list (now and then refuse it), so the copy into the
        // node's row buffer runs too.
        let grant = m.rd32(GRANT);
        m.wr32(r.r[A0], LIST_BUF);
        r.r[V0] = grant;
    });
    let mut rng = Rng(0x4241_4752);
    let mut seen = [0usize; 4];
    for case in 0..3000 {
        let tables = RowTables {
            items: (0..256)
                .map(|_| {
                    let kind = [0u8, 1, 2, 2, 3][rng.below(5) as usize];
                    let price = if rng.below(3) == 0 {
                        0
                    } else {
                        rng.next() as u16
                    };
                    let p = price.to_le_bytes();
                    // The subtype indexes the equipment table (107 records)
                    // for kind 1 and the effect table (130) otherwise.
                    let sub = rng.below(if kind == 1 { 107 } else { 130 }) as u8;
                    [kind, sub, p[0], p[1]]
                })
                .collect(),
            effects: (0..130).map(|_| (rng.next()).to_le_bytes()).collect(),
            equip_flags: (0..107).map(|_| rng.next() as u8).collect(),
        };
        for id in 0..256u32 {
            let r = tables.items[id as usize];
            for k in 0..4 {
                cpu.mem.wr8(ITEMS + 12 * id + k, r[k as usize]);
            }
        }
        for (id, e) in tables.effects.iter().enumerate() {
            for (k, b) in e.iter().enumerate() {
                cpu.mem.wr8(EFFECTS + 4 * id as u32 + k as u32, *b);
            }
        }
        for (id, f) in tables.equip_flags.iter().enumerate() {
            cpu.mem.wr8(EQUIP + 8 * id as u32 + 7, *f);
        }
        let bag: Vec<u8> = (0..256)
            .map(|_| match rng.below(4) {
                0 => 0,
                1 => [0x88, 0x89][rng.below(2) as usize],
                _ => rng.next() as u8,
            })
            .collect();
        for (k, id) in bag.iter().enumerate() {
            cpu.mem.wr16(BAG + 2 * k as u32, u16::from(*id));
        }
        let start = rng.below(128) as u16;
        let end = start + rng.below(129) as u16;
        cpu.mem.wr16(GP + 0x2D2, start);
        cpu.mem.wr16(GP + 0x2D4, end);
        let content = [2u8, 3, 3, 0x22][rng.below(4) as usize];
        // The context word is the field (`0`) or battle (`1`) menu; the
        // port's `UseListCtx::battle` carries those two.
        let context = rng.below(2);
        let flags = rng.next() & 0x0030_0000;
        cpu.mem.wr32(GP + 0x85C, context);
        cpu.mem.wr32(ROW_FLAGS, flags);
        cpu.mem.wr8(NODE + 0x1C, content);
        *built.borrow_mut() = None;
        let grant = u32::from(rng.below(8) != 0);
        cpu.mem.wr32(GRANT, grant);
        for k in 0..0x120 {
            cpu.mem.wr16(LIST_BUF + 0x28 + 2 * k, 0xDEAD);
        }
        assert_eq!(
            cpu.call(WINDOW_BUILD, &[NODE]),
            Stop::Returned,
            "case {case}"
        );

        let window = &bag[usize::from(start)..usize::from(end)];
        let want = match content {
            2 => build_price_gated_rows(window, start, &tables),
            0x22 => build_throw_out_rows(window, start, &tables),
            _ => build_use_list_rows(
                window,
                start,
                &tables,
                &UseListCtx {
                    battle: context == 1,
                    door_light_blocked: flags & 0x0010_0000 != 0,
                    door_wind_blocked: flags & 0x0020_0000 != 0,
                    applicable: &applicable,
                },
            ),
        };
        let got = built.borrow().clone().unwrap_or_default();
        let ctx = format!("case {case}: content {content:#x} context {context} flags {flags:#x}");
        seen[3] += usize::from(content == 3 && context == 1);
        assert_eq!(got, want, "{ctx}");
        assert_eq!(
            list_rows(&mut cpu, grant, &want),
            want,
            "{ctx}: the copied rows"
        );
        seen[usize::from(content == 3) + usize::from(content == 0x22) * 2] += 1;
    }
    assert!(seen.iter().all(|&n| n > 200), "{seen:?}");
}

const LIST_DRAW: u32 = 0x8003_2A44;
const DRAW_STRING: u32 = 0x8003_6888;

/// The list kernel `FUN_80032A44`'s row pass against `list_row_ink`: random
/// rows of every class in a list node, the kernel run in each list mode
/// (`gp+0x87C`; `1`, the pad-driven mode, is left out - its input half is
/// the cursor step the list tests cover), every call outside it stubbed but
/// the row-name resolver `FUN_8002FF8C` (recorded) and the string drawer
/// `FUN_80036888` (recorded with the ink word `gp+0x13C` it runs under).
/// Each bag and spell row's name must draw in the ink the port stages.
#[test]
fn the_list_rows_draw_in_the_ink_the_port_stages() {
    differential!(scus(LIST_DRAW));
    use legaia_engine_menus::menu_list_rows::list_row_ink;
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.stub_outside = Some((LIST_DRAW, 0x8003_37B0));
    let current = Rc::new(RefCell::new(0u16));
    let c = current.clone();
    cpu.hook(ROW_NAME, move |r, _| {
        *c.borrow_mut() = r.r[A0] as u16;
        r.r[V0] = TEXT;
    });
    let drawn = Rc::new(RefCell::new(Vec::<(u16, u8)>::new()));
    let (c, d) = (current.clone(), drawn.clone());
    // The name column is the window's `x + 0xC`; the bag arm's quantity
    // draws through the same routine further right.
    cpu.hook(DRAW_STRING, move |r, m| {
        if r.r[A3] == 40 + 0xC {
            d.borrow_mut().push((*c.borrow(), m.rd32(GP + 0x13C) as u8));
        }
    });
    const LIST: u32 = 0x8013_1000;
    let mut rng = Rng(0x494E_4B53);
    let mut checked = [0usize; 2];
    for case in 0..2000 {
        let count = 1 + rng.below(40) as u16;
        let visible = 1 + rng.below(12) as u16;
        let top = rng.below(u32::from(count)) as u16;
        let rows: Vec<u16> = (0..count)
            .map(|_| {
                let class =
                    [0x1000u16, 0x1000, 0x2000, 0x5000, 0x3000, 0x4000][rng.below(6) as usize];
                class | (rng.next() as u16 & 0x0FFF)
            })
            .collect();
        let mode = [0u32, 2, 4][rng.below(3) as usize];
        cpu.mem.wr32(GP + 0x87C, mode);
        cpu.mem.wr32(NODE + 0x18, LIST);
        cpu.mem.wr8(NODE + 0x1C, 3);
        for (o, v) in [(0xA, 40u16), (0xC, 30), (0xE, 200), (0x10, 160)] {
            cpu.mem.wr16(NODE + o, v);
        }
        cpu.mem.wr16(LIST, top);
        cpu.mem.wr16(LIST + 2, visible);
        cpu.mem.wr16(LIST + 4, count);
        cpu.mem.wr16(LIST + 6, top);
        for (k, r) in rows.iter().enumerate() {
            cpu.mem.wr16(LIST + 0x28 + 2 * k as u32, *r);
        }
        drawn.borrow_mut().clear();
        assert_eq!(cpu.call(LIST_DRAW, &[NODE]), Stop::Returned, "case {case}");
        let parked = mode == 4;
        for (entry, ink) in drawn.borrow().iter() {
            let class = entry & 0xF000;
            if matches!(class, 0x1000 | 0x2000 | 0x5000) {
                assert_eq!(
                    *ink,
                    list_row_ink(*entry, parked),
                    "case {case}: row {entry:#06x} mode {mode}"
                );
                checked[usize::from(class != 0x1000)] += 1;
            }
        }
        let shown = usize::from(visible.min(count - top));
        let names = rows[usize::from(top)..usize::from(top) + shown]
            .iter()
            .filter(|r| matches!(*r & 0xF000, 0x1000 | 0x2000 | 0x5000))
            .count();
        let got = drawn
            .borrow()
            .iter()
            .filter(|(e, _)| matches!(e & 0xF000, 0x1000 | 0x2000 | 0x5000))
            .count();
        assert_eq!(
            got, names,
            "case {case}: every visible bag and spell row draws"
        );
    }
    assert!(checked.iter().all(|&n| n > 2000), "{checked:?}");
}

const HELD_COUNT: u32 = 0x8004_2F4C;
const BAG_SLOT_OF: u32 = 0x8004_2EE0;
const PARTY_COUNT: u32 = 0x8008_4594;
const PARTY_IDS: u32 = 0x8008_4598;
const PURSE: u32 = 0x8008_459C;
/// A character record's eight equipment bytes, `0x80084140 + 0x414 * id +
/// 0x75E`.
const fn equipment(id: u32) -> u32 {
    0x8008_4140 + 0x414 * id + 0x75E
}

/// A list allocator hook that grants `LIST_BUF` when `GRANT` says so.
fn grant_lists(cpu: &mut Cpu) {
    cpu.hook(LIST_ALLOC, |r, m| {
        m.wr32(r.r[A0], LIST_BUF);
        r.r[V0] = m.rd32(GRANT);
    });
}

/// `FUN_80030628`'s content-id `0x0B` arm, the shop buy list, against
/// `shop_tail_rows_allowed` and `build_shop_buy_rows`: random stock records
/// (`gp+0x138`, template ids under `0x1A` among them), prices, purse, held
/// counts (`FUN_80042EE0` answers a bag slot or `0x100`), the Platinum Card
/// probe `FUN_80042F4C(0xFF)` and the party's equipment bytes. The rows the
/// arm copies into the granted list are compared whole.
#[test]
fn the_shop_buy_list_builds_the_rows_retail_builds() {
    differential!(scus(WINDOW_BUILD));
    use legaia_engine_menus::menu_list_rows::{build_shop_buy_rows, shop_tail_rows_allowed};
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    const RECORD: u32 = 0x8013_6000;
    const CARDS: u32 = 0x8013_3FE0;
    grant_lists(&mut cpu);
    cpu.hook(HELD_COUNT, |r, m| {
        assert_eq!(r.r[A0], 0xFF);
        r.r[V0] = m.rd32(CARDS);
    });
    // Held ids sit in the bag slot of their own number; the rest answer
    // `0x100`, retail's "not held".
    cpu.hook(BAG_SLOT_OF, |r, m| {
        let id = r.r[A0] & 0xFF;
        r.r[V0] = if m.rd8(BAG + 2 * id) == id as u8 {
            id
        } else {
            0x100
        };
    });
    let mut rng = Rng(0x5348_4F50);
    let mut seen = [0usize; 3];
    for case in 0..3000 {
        let count = rng.below(24) as usize;
        let pad = rng.below(4) as usize;
        let mut ids: Vec<u8> = (0..count)
            .map(|_| match rng.below(10) {
                0 => rng.below(0x1A) as u8,
                1 => 0xFF,
                _ => 0x1A + rng.below(0xE6) as u8,
            })
            .collect();
        for k in 0..pad.min(count) {
            ids[count - 1 - k] = rng.below(0x1A) as u8;
        }
        cpu.mem.wr8(RECORD + 2, count as u8);
        for (k, id) in ids.iter().enumerate() {
            cpu.mem.wr8(RECORD + 3 + k as u32, *id);
        }
        cpu.mem.wr32(GP + 0x138, RECORD);
        let cards = [0u32, 0, 1, 0x10000][rng.below(4) as usize];
        cpu.mem.wr32(CARDS, cards);
        let members = rng.below(4);
        cpu.mem.wr8(PARTY_COUNT, members as u8);
        let mut gear = Vec::new();
        for i in 0..members {
            let who = rng.below(4);
            cpu.mem.wr8(PARTY_IDS + i, who as u8);
            let mut slots = [0u8; 8];
            for (j, b) in slots.iter_mut().enumerate() {
                *b = if rng.below(40) == 0 {
                    0xFF
                } else {
                    rng.next() as u8 % 0xFF
                };
                cpu.mem.wr8(equipment(who) + j as u32, *b);
            }
            gear.push((who, slots));
        }
        // A later member sharing an earlier one's record overwrote it.
        let gear: Vec<[u8; 8]> = gear
            .iter()
            .map(|(who, _)| {
                let mut s = [0u8; 8];
                for (j, b) in s.iter_mut().enumerate() {
                    *b = cpu.mem.rd8(equipment(*who) + j as u32);
                }
                s
            })
            .collect();
        let purse = rng.next() >> (1 + rng.below(24));
        cpu.mem.wr32(PURSE, purse);
        for id in 0..256u32 {
            cpu.mem
                .wr16(ITEMS + 12 * id + 2, (rng.next() >> rng.below(16)) as u16);
            let held = rng.below(3) == 0;
            cpu.mem.wr8(
                BAG + 2 * id,
                if held {
                    id as u8
                } else {
                    (id as u8).wrapping_add(1)
                },
            );
            cpu.mem.wr8(
                BAG + 2 * id + 1,
                [1u8, 50, 98, 99, 200][rng.below(5) as usize],
            );
        }
        cpu.mem.wr8(NODE + 0x1C, 0x0B);
        let grant = u32::from(rng.below(8) != 0);
        cpu.mem.wr32(GRANT, grant);
        for k in 0..0x40 {
            cpu.mem.wr16(LIST_BUF + 0x28 + 2 * k, 0xDEAD);
        }
        assert_eq!(
            cpu.call(WINDOW_BUILD, &[NODE]),
            Stop::Returned,
            "case {case}"
        );

        let tail = shop_tail_rows_allowed(cards & 0xFFFF != 0, gear.iter().copied());
        let prices: Vec<u16> = (0..256)
            .map(|id| cpu.mem.rd16(ITEMS + 12 * id + 2))
            .collect();
        let held: Vec<u8> = (0..256u32)
            .map(|id| {
                if cpu.mem.rd8(BAG + 2 * id) == id as u8 {
                    cpu.mem.rd8(BAG + 2 * id + 1)
                } else {
                    0
                }
            })
            .collect();
        let want = build_shop_buy_rows(
            &ids,
            tail,
            purse,
            |id| prices[usize::from(id)],
            |id| held[usize::from(id)],
        );
        let ctx = format!("case {case}: ids {ids:02x?} tail {tail} purse {purse}");
        assert_eq!(list_rows(&mut cpu, grant, &want), want, "{ctx}");
        seen[usize::from(tail)] += 1;
        seen[2] += usize::from(want.iter().any(|w| w & 0x0800 != 0));
    }
    assert!(seen.iter().all(|&n| n > 300), "{seen:?}");
}

/// `FUN_80030628`'s Goods arms (content ids `0xE`..`0x10` and `0x1C`..`0x1E`)
/// against `build_goods_candidate_rows`: the equipped accessory read off the
/// character record (`+0x763..+0x765` for the character `gp+0x874`), random
/// bags, item kinds and passive markers, the bag window seeded and
/// `FUN_8004313C` stubbed. The copied rows are compared whole.
#[test]
fn the_goods_candidate_lists_build_the_rows_retail_builds() {
    differential!(scus(WINDOW_BUILD));
    use legaia_engine_menus::menu_list_rows::build_goods_candidate_rows;
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.stub(BAG_WINDOW, 0);
    grant_lists(&mut cpu);
    let mut rng = Rng(0x474F_4F44);
    for case in 0..3000 {
        let tables = RowTables {
            items: (0..256)
                .map(|_| {
                    let kind = [1u8, 2, 2, 3][rng.below(4) as usize];
                    [kind, rng.below(130) as u8, 0, 0]
                })
                .collect(),
            effects: (0..130)
                .map(|_| {
                    let marker = if rng.below(2) == 0 {
                        0x41
                    } else {
                        rng.below(0x42) as u8
                    };
                    [0, 0, 0, marker]
                })
                .collect(),
            equip_flags: Vec::new(),
        };
        for id in 0..256u32 {
            let r = tables.items[id as usize];
            cpu.mem.wr8(ITEMS + 12 * id, r[0]);
            cpu.mem.wr8(ITEMS + 12 * id + 1, r[1]);
        }
        for (id, e) in tables.effects.iter().enumerate() {
            cpu.mem.wr8(EFFECTS + 4 * id as u32 + 3, e[3]);
        }
        let bag: Vec<u8> = (0..256)
            .map(|_| {
                if rng.below(4) == 0 {
                    0
                } else {
                    rng.next() as u8
                }
            })
            .collect();
        for (k, id) in bag.iter().enumerate() {
            cpu.mem.wr16(BAG + 2 * k as u32, u16::from(*id));
        }
        let start = rng.below(128) as u16;
        let end = start + rng.below(129) as u16;
        cpu.mem.wr16(GP + 0x2D2, start);
        cpu.mem.wr16(GP + 0x2D4, end);
        let who = rng.below(4);
        cpu.mem.wr32(GP + 0x874, who);
        let content = [0x0Eu8, 0x0F, 0x10, 0x1C, 0x1D, 0x1E][rng.below(6) as usize];
        let slot = u32::from((content - 0x0E) % 0x0E);
        let equipped = match rng.below(3) {
            0 => 0,
            1 => bag[usize::from(start) + rng.below(u32::from(end - start).max(1)) as usize],
            _ => rng.next() as u8,
        };
        cpu.mem.wr8(equipment(who) + 5 + slot, equipped);
        cpu.mem.wr8(NODE + 0x1C, content);
        let grant = u32::from(rng.below(8) != 0);
        cpu.mem.wr32(GRANT, grant);
        for k in 0..0x120 {
            cpu.mem.wr16(LIST_BUF + 0x28 + 2 * k, 0xDEAD);
        }
        assert_eq!(
            cpu.call(WINDOW_BUILD, &[NODE]),
            Stop::Returned,
            "case {case}"
        );
        let window = &bag[usize::from(start)..usize::from(end)];
        let want = build_goods_candidate_rows(window, start, &tables, equipped);
        let ctx = format!("case {case}: content {content:#x} equipped {equipped:#x}");
        assert_eq!(list_rows(&mut cpu, grant, &want), want, "{ctx}");
    }
}

/// `FUN_80030628`'s content-id `0x19` arm, the landmark list, against
/// `landmark_list_rows`: random placement tables written over `0x80073A98`
/// (repeated names, the `0xFF` terminator anywhere), the system-flag test
/// `FUN_8003CE64` answering from a random flag set.
#[test]
fn the_landmark_list_builds_the_rows_retail_builds() {
    differential!(scus(WINDOW_BUILD));
    use legaia_engine_menus::menu_list_rows::landmark_list_rows;
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    const FLAG_TEST: u32 = 0x8003_CE64;
    const FLAGS: u32 = 0x8013_7000;
    grant_lists(&mut cpu);
    cpu.hook(FLAG_TEST, |r, m| {
        let f = r.r[A0] & 0x1FF;
        r.r[V0] = u32::from(m.rd8(FLAGS + f) != 0);
    });
    let mut rng = Rng(0x4C41_4E44);
    for case in 0..3000 {
        let n = rng.below(24);
        let records: Vec<(u32, u8, u8)> = (0..n)
            .map(|k| {
                let name = if rng.below(12) == 0 {
                    0xFF
                } else {
                    rng.below(6) as u8
                };
                (k, name, rng.next() as u8)
            })
            .collect();
        for &(k, name, flag) in &records {
            cpu.mem.wr8(PLACES + 6 * k, name);
            cpu.mem.wr8(PLACES + 6 * k + 1, flag);
        }
        cpu.mem.wr8(PLACES + 6 * n, 0xFF);
        let flags: Vec<bool> = (0..0x120).map(|_| rng.below(3) != 0).collect();
        for (f, on) in flags.iter().enumerate() {
            cpu.mem.wr8(FLAGS + f as u32, u8::from(*on));
        }
        cpu.mem.wr8(NODE + 0x1C, 0x19);
        let grant = u32::from(rng.below(8) != 0);
        cpu.mem.wr32(GRANT, grant);
        for k in 0..0x40 {
            cpu.mem.wr16(LIST_BUF + 0x28 + 2 * k, 0xDEAD);
        }
        assert_eq!(
            cpu.call(WINDOW_BUILD, &[NODE]),
            Stop::Returned,
            "case {case}"
        );
        let want = landmark_list_rows(records.iter().copied(), |f| flags[usize::from(f)]);
        assert_eq!(
            list_rows(&mut cpu, grant, &want),
            want,
            "case {case}: {records:?}"
        );
    }
}

/// The list kernel `FUN_80032A44`'s navigation phase (list mode `gp+0x87C`
/// = 1, `0x80032A88..0x80032C74`) against `list_kernel_navigate_rows`:
/// random counts, page sizes (`list+2`), page-aligned tops and selections,
/// and every combination of the four direction bits in the repeat-pressed
/// word `gp+0x86C`, with the content id drawn from the kernel's own
/// navigable-id table `0x80073E1C` or from outside it (no navigation).
#[test]
fn the_list_kernel_navigates_as_the_port_does() {
    differential!(scus(LIST_DRAW));
    use legaia_engine_menus::pause_screens::list_kernel_navigate_rows;
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    const LIST: u32 = 0x8013_1000;
    const NAV_IDS: u32 = 0x8007_3E1C;
    cpu.stub_outside = Some((LIST_DRAW, 0x8003_37B0));
    let ids: Vec<u8> = (0..64)
        .map(|k| cpu.mem.rd8(NAV_IDS + k))
        .take_while(|&b| b != 0x23)
        .collect();
    assert!(!ids.is_empty());
    let mut rng = Rng(0x4E41_5649);
    let mut moved = 0;
    for case in 0..6000 {
        let count = rng.below(40) as usize;
        let visible = 1 + rng.below(12) as usize;
        let sel = if count == 0 {
            0
        } else {
            rng.below(count as u32) as usize
        };
        let top = sel - sel % visible;
        let pad = (rng.below(16) as u16) << 12;
        let navigable = rng.below(6) != 0;
        let content = if navigable {
            ids[rng.below(ids.len() as u32) as usize]
        } else {
            0x24
        };
        cpu.mem.wr32(GP + 0x87C, 1);
        cpu.mem.wr32(GP + 0x86C, u32::from(pad));
        cpu.mem.wr32(0x8007_B874, 0);
        cpu.mem.wr32(NODE + 0x18, LIST);
        cpu.mem.wr8(NODE + 0x1C, content);
        for (o, v) in [(0xA, 40u16), (0xC, 30), (0xE, 200), (0x10, 160)] {
            cpu.mem.wr16(NODE + o, v);
        }
        cpu.mem.wr16(LIST, top as u16);
        cpu.mem.wr16(LIST + 2, visible as u16);
        cpu.mem.wr16(LIST + 4, count as u16);
        cpu.mem.wr16(LIST + 6, sel as u16);
        for k in 0..count as u32 {
            cpu.mem.wr16(LIST + 0x28 + 2 * k, 0x1000 | k as u16);
        }
        assert_eq!(cpu.call(LIST_DRAW, &[NODE]), Stop::Returned, "case {case}");
        let want = if navigable && count > 0 {
            // The port takes the raw pad layout (Up `0x10` .. Left `0x80`);
            // the kernel's word is the packed one, the same bits `<< 8`.
            list_kernel_navigate_rows(sel, count, pad >> 8, visible)
        } else {
            sel
        };
        let what = format!(
            "case {case}: count {count} visible {visible} sel {sel} pad {pad:#06x} content {content:#x}"
        );
        assert_eq!(
            cpu.mem.rd16(LIST + 6) as usize,
            want,
            "{what}: the selection"
        );
        assert_eq!(
            cpu.mem.rd16(LIST) as usize,
            want - want % visible,
            "{what}: the page"
        );
        moved += usize::from(want != sel);
    }
    assert!(moved > 1500, "{moved}");
}

/// `FUN_80030628`'s armament arms (content ids `7`..`0xA` and `0x15`..`0x18`)
/// against `build_armament_candidate_rows` with `armament_candidate_accepts`
/// as its filter and `equip_stat_field` as its sort key: random item, equip
/// and bag tables, characters and window ranges, the equipped byte placed
/// where each content id reads it, the stat accessor `FUN_800302E4` live
/// with its overlay favour term `FUN_801DD0C0` answering a function of its
/// arguments. The copied rows - filter, order and sort - are compared whole.
#[test]
fn the_armament_candidate_lists_build_the_rows_retail_builds() {
    differential!(scus(WINDOW_BUILD));
    use legaia_engine_menus::menu_list_rows::{
        armament_candidate_accepts, build_armament_candidate_rows,
    };
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    const CATEGORY: u32 = 0x8007_B484;
    const CHAR_BITS: u32 = 0x8007_B48C;
    const WEAPON_SLOT: u32 = 0x8007_B42C;
    cpu.stub(BAG_WINDOW, 0);
    grant_lists(&mut cpu);
    let favour = |id: u32| (id * 5 + 1) & 0x7;
    cpu.hook(FAVOUR, move |r, _| r.r[V0] = favour(r.r[A1]));
    let categories: Vec<u8> = (0..4)
        .map(|k| cpu.mem.rd16(CATEGORY + 2 * k) as u8)
        .collect();
    let mut rng = Rng(0x4152_4D53);
    let mut sorted = 0;
    for case in 0..3000 {
        let items: Vec<[u8; 2]> = (0..256)
            .map(|_| {
                [
                    [1u8, 1, 1, 2, 0][rng.below(5) as usize],
                    rng.below(64) as u8,
                ]
            })
            .collect();
        let records: Vec<[u8; 8]> = (0..64)
            .map(|_| {
                let mut r: [u8; 8] = std::array::from_fn(|_| rng.below(12) as u8);
                r[6] = rng.next() as u8;
                r[7] = rng.next() as u8;
                r
            })
            .collect();
        for (id, r) in items.iter().enumerate() {
            cpu.mem.wr8(ITEMS + 12 * id as u32, r[0]);
            cpu.mem.wr8(ITEMS + 12 * id as u32 + 1, r[1]);
        }
        for (k, r) in records.iter().enumerate() {
            cpu.mem.load(EQUIP + 8 * k as u32, r);
        }
        let bag: Vec<u8> = (0..256)
            .map(|_| {
                if rng.below(5) == 0 {
                    0
                } else {
                    rng.next() as u8
                }
            })
            .collect();
        for (k, id) in bag.iter().enumerate() {
            cpu.mem.wr16(BAG + 2 * k as u32, u16::from(*id));
        }
        let start = rng.below(128) as u16;
        let end = start + rng.below(64) as u16;
        cpu.mem.wr16(GP + 0x2D2, start);
        cpu.mem.wr16(GP + 0x2D4, end);
        let who = rng.below(3);
        cpu.mem.wr32(GP + 0x874, who);
        let char_bit = 1u8 << rng.below(3);
        cpu.mem.wr8(CHAR_BITS + who, char_bit);
        let weapon_slot = rng.below(4) as u16;
        cpu.mem.wr16(WEAPON_SLOT + 2 * who, weapon_slot);
        let content = [7u8, 8, 9, 0xA, 0x15, 0x16, 0x17, 0x18][rng.below(8) as usize];
        let row = usize::from(if content >= 0x15 {
            content - 0x15
        } else {
            content - 7
        });
        let category = categories[row];
        let at = match row {
            0 => 0x75F,
            1 => 0x75E,
            2 => 0x75E + u32::from(weapon_slot),
            _ => 0x762,
        };
        let equipped = match rng.below(3) {
            0 => 0,
            1 => bag[usize::from(start) + rng.below(u32::from(end - start).max(1)) as usize],
            _ => rng.next() as u8,
        };
        let record = 0x8008_4140 + 0x414 * who;
        for k in 0..8 {
            cpu.mem.wr8(record + 0x75E + k, rng.next() as u8);
        }
        cpu.mem.wr8(record + at, equipped);
        cpu.mem.wr8(NODE + 0x1C, content);
        let grant = u32::from(rng.below(8) != 0);
        cpu.mem.wr32(GRANT, grant);
        for k in 0..0x60 {
            cpu.mem.wr16(LIST_BUF + 0x28 + 2 * k, 0xDEAD);
        }
        assert_eq!(
            cpu.call(WINDOW_BUILD, &[NODE]),
            Stop::Returned,
            "case {case}"
        );
        let window = &bag[usize::from(start)..usize::from(end)];
        let accepts = |id: u8| {
            let [kind, sub] = items[usize::from(id)];
            let r = records[usize::from(sub)];
            armament_candidate_accepts(kind, r[7], r[6], category, char_bit)
        };
        let stat = |word: u16| {
            equip_stat_field(
                word,
                category,
                |slot| bag[usize::from(slot)],
                |item| {
                    let sub = items.get(usize::from(item)).map_or(0, |r| r[1]);
                    Some(EquipBonus {
                        raw: records[usize::from(sub) % 64],
                    })
                },
                |item| favour(u32::from(item)),
            )
        };
        let want = build_armament_candidate_rows(window, start, equipped, accepts, stat);
        let ctx =
            format!("case {case}: content {content:#x} category {category} equipped {equipped:#x}");
        assert_eq!(list_rows(&mut cpu, grant, &want), want, "{ctx}");
        sorted += usize::from(want.len() > 3);
    }
    assert!(sorted > 1000, "{sorted}");
}

/// `FUN_80030628`'s spell list (content ids `5` / `0x20`): in the field
/// (`gp+0x85C` = 0, `0x80031118..0x80031260`) against the Magic screen's
/// `SpellMenuSession::current_spell_rows`, in battle (`gp+0x85C` = 1,
/// `0x80031008..0x80031110`) against `BattleSpellSession`'s rows, over the
/// disc's own
/// Seru-magic records (`seru_magic_catalog_from_scus`): random learned
/// lists, MP, MP-saver ability words (`+0x6BC`) and Ra-Seru bytes, the
/// relevance broadcast `FUN_8003053C` answering from a random set the
/// session gets as its unaffected list. A row is lit exactly when the port
/// admits it, in the same order; an empty Ra-Seru byte builds no list, which
/// the port carries as the caster's `ra_seru_missing`.
#[test]
fn the_spell_list_lights_the_rows_the_magic_screens_admit() {
    differential!(scus(WINDOW_BUILD));
    use legaia_engine_menus::spell_menu::{CasterSlot, SpellMenuSession};
    let (Some(mut cpu), Some(exe)) = (scus_cpu(), crate::common::scus()) else {
        return;
    };
    let catalog = legaia_engine_battle::retail_magic::seru_magic_catalog_from_scus(&exe)
        .expect("Seru magic table");
    const RELEVANT: u32 = 0x8013_7000;
    /// The per-character Ra-Seru slot table `0x8007B424`.
    const RASERU_SLOT: u32 = 0x8007_B424;
    grant_lists(&mut cpu);
    cpu.hook(0x8003_053C, |r, m| {
        r.r[V0] = u32::from(m.rd8(RELEVANT + (r.r[A0] & 0xFF)))
    });
    let ids: Vec<u8> = (0x81..=0x8B).collect();
    let mut rng = Rng(0x5350_4C4C);
    let (mut lit, mut dim) = (0, 0);
    for case in 0..4000 {
        let who = rng.below(3);
        cpu.mem.wr32(GP + 0x874, who);
        let battle = rng.below(2) == 1;
        cpu.mem.wr32(GP + 0x85C, u32::from(battle));
        let record = 0x8008_4140 + 0x414 * who;
        let n = rng.below(12) as usize;
        let mut learned = ids.clone();
        for k in (1..learned.len()).rev() {
            learned.swap(k, rng.below(k as u32 + 1) as usize);
        }
        learned.truncate(n);
        cpu.mem.wr8(record + 0x704, n as u8);
        for (k, id) in learned.iter().enumerate() {
            cpu.mem.wr8(record + 0x705 + k as u32, *id);
        }
        let ability = [0u32, 0x10, 0x20, 0x30][rng.below(4) as usize] | (rng.next() & !0x30);
        cpu.mem.wr32(record + 0x6BC, ability);
        let mp = rng.below(120) as u16;
        cpu.mem.wr16(record + 0x6D2, mp);
        let raseru_slot = rng.below(4) as u16;
        cpu.mem.wr16(RASERU_SLOT + 2 * who, raseru_slot);
        let raseru = if rng.below(8) == 0 {
            0
        } else {
            1 + rng.below(0xFF) as u8
        };
        cpu.mem.wr8(record + 0x75E + u32::from(raseru_slot), raseru);
        let relevant: Vec<bool> = (0..256).map(|_| rng.below(4) != 0).collect();
        for (k, r) in relevant.iter().enumerate() {
            cpu.mem.wr8(RELEVANT + k as u32, u8::from(*r));
        }
        cpu.mem.wr8(NODE + 0x1C, [5u8, 0x20][rng.below(2) as usize]);
        let grant = u32::from(rng.below(8) != 0);
        cpu.mem.wr32(GRANT, grant);
        for k in 0..0x20 {
            cpu.mem.wr16(LIST_BUF + 0x28 + 2 * k, 0xDEAD);
        }
        assert_eq!(
            cpu.call(WINDOW_BUILD, &[NODE]),
            Stop::Returned,
            "case {case}"
        );

        let caster = CasterSlot {
            slot: 0,
            mp,
            hp: 1,
            spells: learned.clone(),
            ability_bits: ability,
            ra_seru_missing: raseru == 0,
            ..Default::default()
        };
        let unaffected: Vec<u8> = ids
            .iter()
            .copied()
            .filter(|&id| !relevant[usize::from(id)])
            .collect();
        let mut s = SpellMenuSession::new(vec![caster], Vec::new(), catalog.clone())
            .with_unaffected_spells(unaffected);
        s.resume_cast(0, 0xFF, 0);
        let ctx = format!("case {case}: learned {learned:02x?} mp {mp} ability {ability:#x}");
        if raseru == 0 {
            assert_eq!(
                cpu.mem.rd16(LIST_BUF + 0x28),
                0xDEAD,
                "{ctx}: no Ra-Seru, no list"
            );
            continue;
        }
        let rows: Vec<(u8, bool)> = if battle {
            legaia_engine_battle::battle_magic::BattleSpellSession::new(
                0, 0, &learned, &catalog, mp, ability,
            )
            .spells
            .iter()
            .map(|r| (r.id, r.affordable))
            .collect()
        } else {
            s.current_spell_rows()
                .iter()
                .map(|v| (v.spell_id, v.admissible))
                .collect()
        };
        let want: Vec<u16> = rows
            .iter()
            .map(|&(id, ok)| u16::from(id) | if ok { 0x5000 } else { 0x5800 })
            .collect();
        assert_eq!(
            list_rows(&mut cpu, grant, &want),
            want,
            "{ctx} battle {battle}"
        );
        lit += rows.iter().filter(|r| r.1).count();
        dim += rows.iter().filter(|r| !r.1).count();
    }
    assert!(lit > 2000 && dim > 2000, "{lit} {dim}");
}

/// The list kernel's confirm and cancel (`0x80032CCC..0x80032DD0`) against
/// `InventoryUseSession` under `with_list_kernel_rows`: a confirm on a row
/// whose word carries `0x800` buzzes (cue `0x23`, the list stays), on any
/// other row the list hands over (mode `2`, cue `0x20`) as the session
/// enters its target select; a cancel leaves (mode `3`, cue `0x37`) as the
/// session cancels. The masks are the record's `0x800846D0` / `0x800846D4`.
#[test]
fn the_list_kernel_confirms_and_cancels_as_the_items_screen_does() {
    differential!(scus(LIST_DRAW));
    use legaia_engine_menus::inventory_use::{
        InventoryContext, InventoryUseEvent, InventoryUseInput, InventoryUseSession, TargetRow,
    };
    use legaia_engine_menus::items::ItemCatalog;
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    const LIST: u32 = 0x8013_1000;
    const NAV_IDS: u32 = 0x8007_3E1C;
    const CONFIRM_MASK: u32 = 0x8008_46D0;
    const CANCEL_MASK: u32 = 0x8008_46D4;
    const CUES: u32 = 0x8007_B6D8;
    cpu.stub_outside = Some((LIST_DRAW, 0x8003_37B0));
    let content = cpu.mem.rd8(NAV_IDS);
    let mut rng = Rng(0x434F_4E46);
    let mut seen = [0usize; 4];
    for case in 0..4000 {
        let count = 1 + rng.below(6) as usize;
        let disabled: Vec<bool> = (0..count).map(|_| rng.below(2) == 0).collect();
        let press = [0u32, 0x40, 0x40, 0x20][rng.below(4) as usize];
        cpu.mem.wr32(GP + 0x87C, 1);
        cpu.mem.wr32(GP + 0x86C, 0);
        cpu.mem.wr32(0x8007_B874, press);
        cpu.mem.wr32(CONFIRM_MASK, 0x40);
        cpu.mem.wr32(CANCEL_MASK, 0x20);
        // The four-slot cue ring: a cancel queues at the write index
        // `gp+0x158` and advances it; a confirm overwrites the slot the last
        // cue went to, `gp+0x15A` (`0x80032D14` / `0x80032D2C`).
        let (slot, last) = (rng.below(4), rng.below(4));
        cpu.mem.wr16(GP + 0x158, slot as u16);
        cpu.mem.wr16(GP + 0x15A, last as u16);
        for k in 0..4 {
            cpu.mem.wr16(CUES + 2 * k, 0xAAAA);
        }
        cpu.mem.wr32(NODE + 0x18, LIST);
        cpu.mem.wr8(NODE + 0x1C, content);
        for (o, v) in [(0xA, 40u16), (0xC, 30), (0xE, 200), (0x10, 160)] {
            cpu.mem.wr16(NODE + o, v);
        }
        cpu.mem.wr16(LIST, 0);
        cpu.mem.wr16(LIST + 2, 8);
        cpu.mem.wr16(LIST + 4, count as u16);
        cpu.mem.wr16(LIST + 6, 0);
        for (k, d) in disabled.iter().enumerate() {
            cpu.mem.wr16(
                LIST + 0x28 + 2 * k as u32,
                0x1000 | k as u16 | if *d { 0x800 } else { 0 },
            );
        }
        assert_eq!(cpu.call(LIST_DRAW, &[NODE]), Stop::Returned, "case {case}");

        let targets = vec![TargetRow::new(0, "A").with_stats(10, 100, 1, 10)];
        let mut s = InventoryUseSession::new(
            ItemCatalog::vanilla(),
            vec![0x01; count],
            targets,
            InventoryContext::Battle,
        )
        .with_list_kernel_rows(disabled.clone(), 8);
        match press {
            0x40 => s.input(InventoryUseInput::Confirm),
            0x20 => s.input(InventoryUseInput::Cancel),
            _ => {}
        }
        let events = s.drain_events();
        let mode = cpu.mem.rd32(GP + 0x87C);
        let cue = cpu
            .mem
            .rd16(CUES + 2 * if press == 0x40 { last } else { slot });
        let ctx = format!("case {case}: press {press:#x} disabled {disabled:?}");
        let (want_events, want_mode, want_cue) = match (press, disabled[0]) {
            (0x40, true) => (vec![InventoryUseEvent::InvalidConfirm], 1, 0x23),
            (0x40, false) => (vec![InventoryUseEvent::EnteredTargetSelect], 2, 0x20),
            (0x20, _) => (vec![InventoryUseEvent::Cancelled], 3, 0x37),
            _ => (vec![], 1, 0xAAAA),
        };
        assert_eq!(events, want_events, "{ctx}: the port");
        assert_eq!((mode, cue), (want_mode, want_cue), "{ctx}: retail");
        seen[match (press, disabled[0]) {
            (0x40, true) => 0,
            (0x40, false) => 1,
            (0x20, _) => 2,
            _ => 3,
        }] += 1;
    }
    assert!(seen.iter().all(|&n| n > 300), "{seen:?}");
}
