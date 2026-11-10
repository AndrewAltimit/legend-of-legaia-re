//! The per-frame window walk `FUN_80031D00` and three of the arms it
//! dispatches to, against `legaia_engine_menus::menu_list_rows`:
//!
//! - the walk's per-node dispatch (jump table `0x80010DC0`) over every kind
//!   byte, with and without a record string, against `text_actor_arm` - the
//!   callee each node reaches, every call outside the walk stubbed;
//! - the list-kind caption `FUN_80035CB8` against `caption_window_pen` and
//!   `caption_window_cancels` (the pen, the ink, the cancel cue it queues and
//!   the list mode it sets);
//! - the spell caption `FUN_80035DA0` against `spell_caption_desc` over the
//!   executable's spell table;
//! - the item caption `FUN_80035E44` against `item_caption_item`, the item
//!   detail printer `FUN_800337B0` it hands to recorded.

use crate::common::{Rng, scus_cpu};
use legaia_engine_menus::menu_list_rows::{
    TextActorArm, caption_window_cancels, caption_window_pen, item_caption_item,
    spell_caption_desc, text_actor_arm,
};
use psx_retail_exec::{A0, A1, A2, A3, SP, Stop};
use std::cell::RefCell;
use std::rc::Rc;

const WALK: u32 = 0x8003_1D00;
const WALK_END: u32 = 0x8003_2434;
const CAPTION: u32 = 0x8003_5CB8;
const SPELL_CAPTION: u32 = 0x8003_5DA0;
const ITEM_CAPTION: u32 = 0x8003_5E44;
const DRAW_TEXT: u32 = 0x8003_6888;
const DRAW_COUNTED: u32 = 0x8003_CC98;
const LIST_KERNEL: u32 = 0x8003_2A44;
const ROW_DESCRIPTION: u32 = 0x8003_4250;
const ARTS_LIST: u32 = 0x8003_4358;
const ITEM_DETAIL: u32 = 0x8003_37B0;
const GP: u32 = 0x8007_B318;
const GAME_MODE: u32 = 0x8007_B83C;
const PRESS: u32 = 0x8007_B874;
const CANCEL_MASKS: u32 = 0x8008_4140 + 0x590;
const CUE_RING: u32 = 0x8007_B6D8;
const SPELLS: u32 = 0x8007_54C8;
const SPELL_DESCS: u32 = 0x8007_5DB0;
const ITEMS: u32 = 0x8007_4368;
const BLOCK: u32 = 0x8008_4140;
const HEAD: u32 = 0x8013_0000;
const NODE: u32 = 0x8013_0100;
const RECORD: u32 = 0x8014_0000;

fn seat_node(cpu: &mut psx_retail_exec::Cpu, kind: u8, glyphs: u16) {
    cpu.mem.load(NODE, &[0u8; 0x40]);
    cpu.mem.wr32(GP + 0x148, HEAD);
    cpu.mem.wr32(HEAD, NODE);
    cpu.mem.wr32(HEAD + 4, NODE);
    cpu.mem.wr32(NODE, HEAD);
    cpu.mem.wr32(NODE + 4, HEAD);
    cpu.mem.wr16(NODE + 8, 5);
    cpu.mem.wr8(NODE + 0x1C, kind);
    cpu.mem.wr16(NODE + 0x14, glyphs);
    cpu.mem.wr32(NODE + 0x18, RECORD);
}

#[test]
fn the_window_walk_dispatches_each_kind_where_the_port_does() {
    differential!(scus(WALK));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.stub_outside = Some((WALK, WALK_END));
    // A record string of two segments, so the typewriter has a glyph to show.
    cpu.mem.load(RECORD, &[2, 0x41, 0, 0x42, 0, 1, 0x43, 0, 0]);
    let mut reached = 0;
    for kind in 0..=0x30u8 {
        for glyphs in [0u16, 3] {
            seat_node(&mut cpu, kind, glyphs);
            assert_eq!(cpu.call(WALK, &[]), Stop::Returned, "kind {kind:#x}");
            let drawn: Vec<u32> = cpu
                .calls
                .iter()
                .copied()
                .filter(|&c| c != 0x8003_1AE4)
                .collect();
            let want: Vec<u32> = match text_actor_arm(kind) {
                TextActorArm::Text | TextActorArm::Typewriter if glyphs != 0 => vec![DRAW_TEXT],
                TextActorArm::StaticText if glyphs != 0 => vec![DRAW_COUNTED],
                TextActorArm::ListOrCaption if glyphs == 0 => vec![LIST_KERNEL],
                TextActorArm::ListOrCaption => vec![CAPTION],
                TextActorArm::RowDescription => vec![ROW_DESCRIPTION],
                TextActorArm::SpellCaption => vec![SPELL_CAPTION],
                TextActorArm::ArtsList => vec![ARTS_LIST],
                TextActorArm::ItemCaption => vec![ITEM_CAPTION],
                _ => vec![],
            };
            assert_eq!(drawn, want, "kind {kind:#x} glyphs {glyphs}");
            reached += usize::from(!want.is_empty());
        }
    }
    assert!(reached > 60, "{reached}");
}

#[test]
fn the_list_caption_draws_and_cancels_as_retail_does() {
    differential!(scus(CAPTION));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let drawn = Rc::new(RefCell::new(Vec::<(u32, i16, i16, u32)>::new()));
    let d = drawn.clone();
    cpu.hook(DRAW_COUNTED, move |r, m| {
        assert_eq!((r.r[A1], r.r[A2]), (0, 0));
        let y = m.rd32(r.r[SP] + 0x10) as i16;
        d.borrow_mut()
            .push((r.r[A0], r.r[A3] as i16, y, m.rd32(GP + 0x13C)));
    });
    let mut rng = Rng(0x4341_5054);
    let mut cancelled = 0;
    for case in 0..4000 {
        let (x, y, h) = (rng.next() as i16, rng.next() as i16, rng.next() as i16);
        let mode = [0x15i16, 0x15, 0x3, 0x11][rng.below(4) as usize];
        let (press, masks) = (rng.next() & 0xFFFF, rng.next() & 0xFFFF);
        let (m0, m1) = (masks & 0xFF, masks & 0xFF00);
        let ring = rng.below(4);
        cpu.mem.wr16(GAME_MODE, mode as u16);
        cpu.mem.wr32(PRESS, press);
        cpu.mem.wr32(CANCEL_MASKS, m0);
        cpu.mem.wr32(CANCEL_MASKS + 4, m1);
        cpu.mem.wr16(GP + 0x158, ring as u16);
        cpu.mem.wr32(GP + 0x87C, 0x55);
        cpu.mem.wr16(CUE_RING + 2 * ring, 0xAAAA);
        cpu.mem.wr16(NODE + 0xA, x as u16);
        cpu.mem.wr16(NODE + 0xC, y as u16);
        cpu.mem.wr16(NODE + 0x10, h as u16);
        cpu.mem.wr32(NODE + 0x18, RECORD);
        drawn.borrow_mut().clear();
        assert_eq!(cpu.call(CAPTION, &[NODE]), Stop::Returned, "case {case}");
        let (px, py) = caption_window_pen(x, y, h);
        assert_eq!(*drawn.borrow(), vec![(RECORD, px, py, 7)], "case {case}");
        let cancels = caption_window_cancels(mode, press, m0 | m1);
        assert_eq!(
            cpu.mem.rd32(GP + 0x87C) == 3,
            cancels,
            "case {case}: the list mode"
        );
        assert_eq!(
            cpu.mem.rd16(CUE_RING + 2 * ring) == 0x37,
            cancels,
            "case {case}: the cancel cue"
        );
        cancelled += usize::from(cancels);
    }
    assert!(cancelled > 500, "{cancelled}");
}

#[test]
fn the_spell_caption_names_the_description_the_port_names() {
    differential!(scus(SPELL_CAPTION));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let drawn = Rc::new(RefCell::new(Vec::<(u32, i16, i16)>::new()));
    let d = drawn.clone();
    cpu.hook(DRAW_COUNTED, move |r, m| {
        let y = m.rd32(r.r[SP] + 0x10) as i16;
        d.borrow_mut().push((r.r[A0], r.r[A3] as i16, y));
    });
    let mut rng = Rng(0x5350_4C43);
    let mut shown = 0;
    for case in 0..4000 {
        let character = rng.below(4);
        let count = [0u8, 1, 7][rng.below(3) as usize];
        let spell = rng.below(0xC0);
        let (x, y) = (rng.below(300) as i16, rng.below(240) as i16);
        cpu.mem.wr8(BLOCK + 0x414 * character + 0x704, count);
        cpu.mem.wr32(GP + 0x874, character);
        cpu.mem.wr32(GP + 0x870, spell);
        cpu.mem.wr16(NODE + 0xA, x as u16);
        cpu.mem.wr16(NODE + 0xC, y as u16);
        drawn.borrow_mut().clear();
        assert_eq!(
            cpu.call(SPELL_CAPTION, &[NODE]),
            Stop::Returned,
            "case {case}"
        );
        let desc = cpu.mem.rd8(SPELLS + 12 * spell + 4);
        let want: Vec<(u32, i16, i16)> = spell_caption_desc(count, spell, desc)
            .map(|k| (cpu.mem.rd32(SPELL_DESCS + 4 * u32::from(k)), x, y))
            .into_iter()
            .collect();
        shown += want.len();
        assert_eq!(
            *drawn.borrow(),
            want,
            "case {case}: spell {spell:#x} count {count}"
        );
    }
    assert!(shown > 600, "{shown}");
}

#[test]
fn the_item_caption_hands_the_item_the_port_hands() {
    differential!(scus(ITEM_CAPTION));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let drawn = Rc::new(RefCell::new(Vec::<(u32, u32, u32, u32)>::new()));
    let d = drawn.clone();
    cpu.hook(ITEM_DETAIL, move |r, _| {
        d.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2], r.r[A3]));
    });
    let mut rng = Rng(0x4954_4D43);
    let mut shown = 0;
    for case in 0..4000 {
        let class = [0x3000u32, 0x1000, 0x5000, 0x3000][rng.below(4) as usize];
        let mode = rng.below(5);
        let item = rng.below(0x100);
        let (x, y) = (rng.next() as i16, rng.next() as i16);
        cpu.mem.wr32(GP + 0x884, class);
        cpu.mem.wr32(GP + 0x87C, mode);
        cpu.mem.wr32(GP + 0x870, item);
        cpu.mem.wr16(NODE + 0xA, x as u16);
        cpu.mem.wr16(NODE + 0xC, y as u16);
        drawn.borrow_mut().clear();
        assert_eq!(
            cpu.call(ITEM_CAPTION, &[NODE]),
            Stop::Returned,
            "case {case}"
        );
        let want: Vec<(u32, u32, u32, u32)> = item_caption_item(class, mode, item)
            .map(|id| {
                (
                    cpu.mem.rd32(ITEMS + 12 * id + 8),
                    id,
                    x as i32 as u32,
                    y as i32 as u32,
                )
            })
            .into_iter()
            .collect();
        shown += want.len();
        assert_eq!(*drawn.borrow(), want, "case {case}");
    }
    assert!(shown > 1000, "{shown}");
}
