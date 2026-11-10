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
        r.r[V0] = 0;
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
