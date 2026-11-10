//! The fishing hub's tackle screen `FUN_801D0F5C` in PROT 0972, whole: its
//! input and equip half against `RodLureSelect::tick` (the cursor, the cue,
//! the persistent lure and rod words, the hub's state), and its draw half
//! against `FishingHub::lines` (each row's item, seat and ink, the counts,
//! the cursor).

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_fishing::fishing::RodLureSelect;
use legaia_engine_fishing::fishing_hub::{FishingHub, FishingHubText, HubScreen};
use legaia_minigame_formats::fishing_species as fs;
use psx_retail_exec::{A0, A1, A2, A3, SP, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const TACKLE: u32 = 0x801D_0F5C;
const BAG_COUNT: u32 = 0x8004_2F4C;
const PRINT: u32 = 0x8003_6888;
const NUMBER: u32 = 0x8003_4B78;
const CURSOR_DRAW: u32 = 0x8002_C488;
const FRAME: u32 = 0x801D_74B0;
const CURSOR: u32 = 0x801D_90DC;
const PAD_MOVE: u32 = 0x801D_90D8;
const PAD_EDGE: u32 = 0x8007_B874;
const STATE: u32 = 0x801D_926C;
const LURE: u32 = 0x8008_4450;
const ROD: u32 = 0x8008_4454;
const CUE: u32 = 0x8007_B6D8;
const INK: u32 = 0x8007_B454;
const DEV_FLAG: u32 = 0x8007_B9B0;

/// One drawn thing: its text (an item id for a name row, the digits for a
/// count, `>` for the cursor), its seat, and whether the ink word was the
/// highlight's.
type Row = (Vec<u8>, i32, i32, bool);

#[test]
fn the_tackle_screen_runs_as_retail_runs_it() {
    differential!(prot(972, TACKLE));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d
        .read_entry(fs::FISHING_OVERLAY_PROT_INDEX)
        .expect("PROT 0972");
    cpu.mem.load(fs::FISHING_OVERLAY_BASE_VA, &overlay);
    let text = FishingHubText::from_overlay(&overlay).expect("the hub's text");
    let bag = Rc::new(RefCell::new([0i32; 6]));
    let b = bag.clone();
    cpu.hook(BAG_COUNT, move |r, _| {
        let id = r.r[A0];
        assert!((0x9D..=0xA2).contains(&id), "item {id:#x}");
        r.r[V0] = b.borrow()[(id - 0x9D) as usize] as u32;
    });
    let rows = Rc::new(RefCell::new(Vec::<Row>::new()));
    let frames = Rc::new(RefCell::new(Vec::<[u32; 4]>::new()));
    let o = rows.clone();
    cpu.hook(PRINT, move |r, m| {
        // A row's name is the item-name escape `0xC2 id`.
        let s = r.r[A0];
        assert_eq!((m.rd8(s), m.rd8(s + 2)), (0xC2, 0), "an item-name line");
        let y = m.rd32(r.r[SP] + 0x10) as i32;
        o.borrow_mut()
            .push((vec![m.rd8(s + 1)], r.r[A3] as i32, y, m.rd32(INK) == 7));
    });
    let o = rows.clone();
    cpu.hook(NUMBER, move |r, m| {
        assert_eq!(r.r[A1], 4, "a four-cell field");
        o.borrow_mut().push((
            format!("{:>4}", r.r[A0] as i32).into_bytes(),
            r.r[A2] as i32,
            r.r[A3] as i32,
            m.rd32(INK) == 7,
        ));
    });
    let o = rows.clone();
    cpu.hook(CURSOR_DRAW, move |r, m| {
        assert_eq!((r.r[A2], m.rd32(INK)), (0x4E, 7));
        o.borrow_mut()
            .push((b">".to_vec(), r.r[A0] as i32, r.r[A1] as i32, false));
    });
    let f = frames.clone();
    cpu.hook(FRAME, move |r, _| {
        f.borrow_mut().push([r.r[A0], r.r[A1], r.r[A2], r.r[A3]])
    });
    cpu.mem.wr32(DEV_FLAG, 0);

    let mut rng = Rng(0x5441_434B);
    let (mut lures, mut rods, mut refused, mut left, mut wraps) = (0, 0, 0, 0, 0);
    for round in 0..40_000 {
        for c in bag.borrow_mut().iter_mut() {
            *c = if rng.below(3) == 0 {
                0
            } else {
                1 + rng.below(99) as i32
            };
        }
        let owned_rods = bag.borrow()[3..].iter().filter(|&&c| c != 0).count() as i32;
        // The cursor a frame can open on: a row of the list, or one step
        // past either end (which only a frame with no input can leave).
        let interactive = rng.below(8) != 0;
        let cursor = if interactive {
            rng.below(owned_rods as u32 + 3) as i32
        } else {
            rng.below(owned_rods as u32 + 5) as i32 - 1
        };
        let pad_edge = [0u32, 0x40, 0x04, 0x20, 0x01, 0x44, 0x61][rng.below(7) as usize];
        let pad_move = [0u32, 0, 0x1000, 0x4000, 0x5000][rng.below(5) as usize];
        let (lure, rod) = (rng.below(3), rng.below(3));
        let state = 0x6E;
        cpu.mem.wr32(CURSOR, cursor as u32);
        cpu.mem.wr32(PAD_EDGE, pad_edge);
        cpu.mem.wr32(PAD_MOVE, pad_move);
        cpu.mem.wr32(LURE, lure);
        cpu.mem.wr32(ROD, rod);
        cpu.mem.wr32(STATE, state);
        cpu.mem.wr16(CUE, 0);
        rows.borrow_mut().clear();
        frames.borrow_mut().clear();

        assert_eq!(cpu.call(TACKLE, &[u32::from(interactive)]), Stop::Returned);

        let counts = *bag.borrow();
        let count_of = |id: u32| counts[(id - 0x9D) as usize];
        let mut port = RodLureSelect { cursor };
        let t = port.tick(pad_edge, pad_move, interactive, count_of);
        let lure = t.equip_lure.unwrap_or(lure);
        let rod = t.equip_rod.map_or(rod, |r| r as u32);
        let ctx = format!(
            "round {round} cursor {cursor} edge {pad_edge:#x} move {pad_move:#x} bag {counts:?}"
        );
        assert_eq!(
            (
                cpu.mem.rd32(CURSOR) as i32,
                cpu.mem.rd16(CUE),
                cpu.mem.rd32(LURE),
                cpu.mem.rd32(ROD),
                cpu.mem.rd32(STATE),
            ),
            (
                port.cursor,
                t.sfx.unwrap_or(0),
                lure,
                rod,
                if t.leave { 100 } else { state },
            ),
            "{ctx}"
        );

        // The draw half reads the words the equip just stored.
        let hub = FishingHub {
            screen: HubScreen::Tackle,
            tackle: port,
            ..FishingHub::open()
        };
        let lines = hub.lines(&text, lure, rod as i32, |id| (vec![id as u8], count_of(id)));
        let want: Vec<Row> = lines[text.menu_rows.len()..]
            .iter()
            .map(|l| (l.text.clone(), i32::from(l.x), i32::from(l.y), l.marked))
            .collect();
        assert_eq!(*rows.borrow(), want, "{ctx}: the rows");
        assert_eq!(frames.borrow().len(), 1, "{ctx}: one frame");

        lures += u32::from(t.equip_lure.is_some());
        rods += u32::from(t.equip_rod.is_some());
        refused += u32::from(t.sfx == Some(0x22));
        left += u32::from(t.leave);
        wraps += u32::from(port.cursor != cursor && (port.cursor - cursor).abs() > 1);
    }
    assert!(
        lures > 500 && rods > 500 && refused > 100 && left > 1000 && wraps > 500,
        "lures {lures} rods {rods} refused {refused} left {left} wraps {wraps}"
    );
}
