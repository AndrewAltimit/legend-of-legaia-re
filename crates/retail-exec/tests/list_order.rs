//! The menu overlay's list-order sub-screen `FUN_801DA2A0` (PROT 0899, the
//! pause menu's sub-screen `0x15`) against
//! `legaia_engine_menus::save_subscreen::{sub15_list_len, sub15_frame,
//! sub15_swap_rows}`: its list half (steps `2..=7`) over random records,
//! characters and picker verdicts, every call outside the routine stubbed
//! but the row picker `FUN_801D688C` (scripted, its arguments recorded) and
//! the two SFX cues (recorded). The step it leaves, whether it rejects,
//! settles or browses, and the record after a committed exchange are
//! compared.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_menus::save_subscreen::{
    Sub15Frame, sub15_frame, sub15_list_len, sub15_swap_rows,
};
use psx_retail_exec::{A0, A1, A2, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const SCREEN: u32 = 0x801D_A2A0;
const SCREEN_END: u32 = SCREEN + 1880;
const PICKER: u32 = 0x801D_688C;
/// The pause menu's root command picker (sub-screen `0x01`).
const ROOT_PICKER: u32 = 0x801D_6B20;
const SFX_A: u32 = 0x8003_5BD0;
const SFX_B: u32 = 0x8003_5B50;
const STEP: u32 = 0x801E_46AC;
const SUB_ROW: u32 = 0x801E_46C0;
const CHAR_CURSOR: u32 = 0x801E_46C4;
const ANCHOR: u32 = 0x801E_46D0;
const ROW_CURSOR: u32 = 0x8007_BB98;
const SCROLL: u32 = 0x8007_BB90;
const PAD: u32 = 0x8007_BB84;
const PARTY_ORDER: u32 = 0x8008_4598;
const RASERU_SLOTS: u32 = 0x8007_B424;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;

#[test]
fn the_list_order_screen_steps_as_retail_does() {
    differential!(prot(899, SCREEN));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &disc().expect("disc").read_entry(899).expect("PROT 0899"),
    );
    cpu.stub_outside = Some((SCREEN, SCREEN_END));
    let verdict = Rc::new(RefCell::new(0u32));
    let picks = Rc::new(RefCell::new(Vec::<(u32, u32, u32)>::new()));
    let (v, p) = (verdict.clone(), picks.clone());
    cpu.hook(PICKER, move |r, _| {
        p.borrow_mut().push((r.r[A0], r.r[A1], r.r[A2]));
        r.r[V0] = *v.borrow();
    });
    let cues = Rc::new(RefCell::new(Vec::<u32>::new()));
    for va in [SFX_A, SFX_B] {
        let c = cues.clone();
        cpu.hook(va, move |r, _| c.borrow_mut().push(r.r[A0]));
    }
    let raseru: Vec<usize> = (0..4)
        .map(|c| cpu.mem.rd16(RASERU_SLOTS + 2 * c) as usize)
        .collect();
    let mut rng = Rng(0x4C4F_5244);
    let mut seen = [0usize; 4];
    for case in 0..6000 {
        let step = 2 + rng.below(6);
        let slot = rng.below(3);
        let character = rng.below(3) as u8;
        let mut record: Vec<u8> = (0..STRIDE).map(|_| rng.next() as u8).collect();
        // Short lists and empty ones, and a Ra-Seru slot now and then empty.
        record[0x13C] = rng.below(12) as u8 * u8::from(rng.below(4) != 0);
        record[0x185] = rng.below(12) as u8 * u8::from(rng.below(4) != 0);
        if rng.below(4) == 0 {
            record[0x196 + raseru[usize::from(character)]] = 0;
        }
        if rng.below(4) == 0 {
            record[0xF4..0xFC].fill(0);
        }
        let rows = u32::from(record[0x13C].max(1));
        let cursor = rng.below(rows);
        let anchor = rng.below(rows) | if rng.below(2) == 0 { 0x1000 } else { 0 };
        *verdict.borrow_mut() = rng.below(4);
        cpu.mem
            .load(RECORDS + STRIDE * u32::from(character), &record);
        cpu.mem.wr8(PARTY_ORDER + slot, character);
        cpu.mem.wr32(STEP, step);
        cpu.mem.wr32(SUB_ROW, 0);
        cpu.mem.wr32(CHAR_CURSOR, slot);
        cpu.mem.wr32(ANCHOR, anchor);
        cpu.mem.wr32(ROW_CURSOR, cursor);
        cpu.mem.wr32(SCROLL, 0);
        cpu.mem.wr32(PAD, 0);
        picks.borrow_mut().clear();
        cues.borrow_mut().clear();
        assert_eq!(cpu.call(SCREEN, &[]), Stop::Returned, "case {case}");

        let len = sub15_list_len(step as u8, &record, raseru[usize::from(character)]);
        let ctx = format!("case {case}: step {step} char {character} len {len}");
        let got_step = cpu.mem.rd32(STEP);
        let mut want_record = record.clone();
        match sub15_frame(step as u8, len) {
            Sub15Frame::Reject => {
                assert_eq!(got_step, 1, "{ctx}: rejected");
                assert_eq!(*cues.borrow(), vec![0x23], "{ctx}: the buzz");
                assert!(picks.borrow().is_empty(), "{ctx}");
                seen[0] += 1;
            }
            Sub15Frame::Settle { next_step } => {
                assert_eq!(got_step, u32::from(next_step), "{ctx}: settled");
                assert!(picks.borrow().is_empty(), "{ctx}");
                seen[1] += 1;
            }
            Sub15Frame::Browse { rows } => {
                assert_eq!(
                    *picks.borrow(),
                    vec![(ROW_CURSOR, u32::from(rows), 0)],
                    "{ctx}: the picker"
                );
                seen[2] += 1;
                if step == 6 && *verdict.borrow() == 1 && anchor & 0x1000 == 0 {
                    sub15_swap_rows(&mut want_record, cursor as usize, anchor as usize);
                    seen[3] += 1;
                }
            }
        }
        let got = cpu
            .mem
            .read(RECORDS + STRIDE * u32::from(character), STRIDE as usize);
        assert!(
            got == want_record,
            "{ctx}: {:?}",
            diff_offsets(&got, &want_record)
        );
    }
    assert!(
        seen[..3].iter().all(|&n| n > 1000) && seen[3] > 40,
        "{seen:?}"
    );
}

/// Step 1 of `FUN_801DA2A0`, the Status pager (`0x801DA370..0x801DA530`),
/// against `StatusScreenSession::tick`: the party picker `FUN_801D688C`
/// scripted to each verdict (confirm, cancel, its no-turn answer `3`,
/// nothing), the page word `0x801E46C0` in either column, and Left / Right
/// on the repeat-pressed word, alone or together. The page is compared
/// folded onto one column, a confirm by where it goes (the Magic and Moves
/// lists, the Skills buzz, the Condition hop), a cancel by the close.
#[test]
fn the_status_pager_turns_and_confirms_as_retail_does() {
    differential!(prot(899, SCREEN));
    use legaia_engine_menus::status_screen::{
        StatusEvent, StatusInput, StatusMoveRow, StatusScreenSession, StatusSnapshot,
        StatusSpellRow,
    };
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    const PAGE: u32 = 0x801E_46C0;
    const EXIT: u32 = 0x801E_46A4;
    const LOCK: u32 = 0x8007_BB80;
    cpu.mem.load(
        OVERLAY_BASE,
        &disc().expect("disc").read_entry(899).expect("PROT 0899"),
    );
    cpu.stub_outside = Some((SCREEN, SCREEN_END));
    let verdict = Rc::new(RefCell::new(0u32));
    let v = verdict.clone();
    cpu.hook(PICKER, move |r, _| r.r[V0] = *v.borrow());
    let cues = Rc::new(RefCell::new(Vec::<u32>::new()));
    for va in [SFX_A, SFX_B] {
        let c = cues.clone();
        cpu.hook(va, move |r, _| c.borrow_mut().push(r.r[A0]));
    }
    let mut snap = StatusSnapshot::placeholder(0, "A");
    // Lists long enough to open (a short one is the later step's reject).
    snap.spells = (0x81..0x85)
        .map(|spell_id| StatusSpellRow {
            spell_id,
            ..Default::default()
        })
        .collect();
    snap.moves = (1..5)
        .map(|id| StatusMoveRow {
            id,
            ..Default::default()
        })
        .collect();
    // Character 0's record: four spells, four moves, a Ra-Seru on, so the
    // list step the confirm falls into settles in the same frame.
    let raseru = cpu.mem.rd16(RASERU_SLOTS) as u32;
    cpu.mem.wr8(RECORDS + 0x13C, 4);
    cpu.mem.wr8(RECORDS + 0x185, 4);
    cpu.mem.wr8(RECORDS + 0x196 + raseru, 1);
    let mut rng = Rng(0x5041_4745);
    let mut seen = [0usize; 4];
    for case in 0..6000 {
        let page = [0u32, 1, 2, 3, 5, 6, 7, 8][rng.below(8) as usize];
        let high = if rng.below(2) == 0 { 0x1000 } else { 0 };
        let v = [0u32, 0, 1, 2, 3][rng.below(5) as usize];
        let pad = [0u32, 0x8000, 0x2000, 0xA000, 0x1000][rng.below(5) as usize];
        *verdict.borrow_mut() = v;
        cpu.mem.wr32(STEP, 1);
        cpu.mem.wr32(PAGE, high | page);
        cpu.mem.wr32(EXIT, 0);
        cpu.mem.wr32(LOCK, 0);
        cpu.mem.wr32(PAD, pad);
        cpu.mem.wr32(CHAR_CURSOR, 0);
        cpu.mem.wr8(PARTY_ORDER, 0);
        cues.borrow_mut().clear();
        assert_eq!(cpu.call(SCREEN, &[]), Stop::Returned, "case {case}");

        let mut s = StatusScreenSession::new(vec![snap.clone()]).with_page((page % 5) as u8);
        let input = match v {
            1 => StatusInput::from_pad_edge(0x4000),
            2 => StatusInput::from_pad_edge(0x2000),
            3 => StatusInput::default(),
            _ => StatusInput {
                left: pad & 0x8000 != 0,
                right: pad & 0x2000 != 0,
                ..Default::default()
            },
        };
        let events = s.tick(input);
        let ctx = format!("case {case}: page {page} verdict {v} pad {pad:#x}");
        let got_page = cpu.mem.rd32(PAGE) & 0xFFF;
        let got_step = cpu.mem.rd32(STEP);
        match v {
            2 => {
                assert_eq!(cpu.mem.rd32(EXIT), 1, "{ctx}: closed");
                assert_eq!(events, vec![StatusEvent::Closed], "{ctx}");
            }
            1 => {
                match got_step {
                    // Steps 3 / 4 settle on to the lists' 6 / 7 at once.
                    6 | 7 => assert_eq!(events, vec![StatusEvent::ListOpened], "{ctx}"),
                    _ if cues.borrow().contains(&0x23) => {
                        assert_eq!(events, vec![StatusEvent::Rejected], "{ctx}")
                    }
                    // The Condition page's hop to the other column.
                    _ => {
                        assert_eq!(got_page % 5, 0, "{ctx}: the hop");
                        assert!(events.is_empty(), "{ctx}");
                    }
                }
                assert_eq!(got_step == 6, s.page() == 2, "{ctx}: Magic");
                seen[1] += 1;
            }
            _ => {
                assert_eq!(got_step, 1, "{ctx}");
                assert_eq!(got_page % 5, u32::from(s.page()), "{ctx}: the page");
                let turned = cues.borrow().contains(&0x21);
                assert_eq!(
                    turned,
                    events
                        .iter()
                        .any(|e| matches!(e, StatusEvent::PageTurned { .. })),
                    "{ctx}: the cue"
                );
                seen[if turned { 2 } else { 3 }] += 1;
            }
        }
        seen[0] += usize::from(v == 2);
    }
    assert!(seen.iter().all(|&n| n > 300), "{seen:?}");
}

/// What a reopened Status screen shows: the pause menu's close of Status
/// lands on the root picker, whose phase 0 (`FUN_801D6B20`, `0x801D6B54..
/// 0x801D6B78`) runs before Status' own step 0 (`0x801DA31C..0x801DA36C`)
/// on the way back in. Over random page words, party cursors and flag bits
/// left by the previous visit, retail's page comes back `0` (the root's
/// `sw 0x1000` at `0x801D6B6C`, masked by Status step 0) and the party
/// cursor comes back masked but kept - which is `StatusScreenSession::new`
/// with `with_cursor` on the cursor the last screen left.
#[test]
fn a_reopened_status_screen_keeps_the_member_and_not_the_page() {
    differential!(prot(899, SCREEN), prot(899, ROOT_PICKER));
    use legaia_engine_menus::status_screen::{StatusScreenSession, StatusSnapshot};
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    const PAGE: u32 = 0x801E_46C0;
    const ROOT_CURSOR: u32 = 0x801E_46BC;
    cpu.mem.load(
        OVERLAY_BASE,
        &disc().expect("disc").read_entry(899).expect("PROT 0899"),
    );
    cpu.stub(0x801D_6628, 0);
    cpu.stub(0x8003_1D00, 0);
    let snaps: Vec<StatusSnapshot> = (0..3)
        .map(|i| StatusSnapshot::placeholder(i, "A"))
        .collect();
    let mut rng = Rng(0x5245_4F50);
    for case in 0..4000 {
        let page = rng.next() & 0xF00F;
        let member = rng.below(3);
        let flags = rng.next() & 0xF000;
        cpu.mem.wr32(PAGE, page);
        cpu.mem.wr32(CHAR_CURSOR, flags | member);
        cpu.mem.wr32(ROOT_CURSOR, rng.next() & 0xF007);
        cpu.mem.wr8(PARTY_ORDER + member, member as u8);
        // The root picker's phase 0, then the Status screen's step 0 (the
        // driver zeroes the step on every change of sub-screen).
        cpu.mem.wr32(STEP, 0);
        assert_eq!(cpu.call(ROOT_PICKER, &[0]), Stop::Returned, "case {case}");
        cpu.mem.wr32(STEP, 0);
        assert_eq!(cpu.call(SCREEN, &[0]), Stop::Returned, "case {case}");
        let s = StatusScreenSession::new(snaps.clone()).with_cursor(member as u8);
        let ctx = format!("case {case}: page {page:#x} cursor {:#x}", flags | member);
        assert_eq!(cpu.mem.rd32(STEP), 1, "{ctx}: step 0 arms the pager");
        assert_eq!(
            cpu.mem.rd32(PAGE) & 0xFFF,
            u32::from(s.page()),
            "{ctx}: page"
        );
        assert_eq!(
            cpu.mem.rd32(CHAR_CURSOR),
            u32::from(s.cursor()),
            "{ctx}: the member"
        );
    }
}

/// The shared party cursor `DAT_801E46C4` through each party screen's step
/// 0 against `party_cursor_on_entry`: the Items single-target and
/// all-party applies (`FUN_801D8308`, `FUN_801D7FF8`), the Magic group and
/// single-target screens (`FUN_801D9280`, `FUN_801D9594`), the Equip
/// character picker (`FUN_801D98F0`) and Status (`FUN_801DA2A0`), over
/// random cursor and flag bits. Every call outside the routine is stubbed;
/// the low 12 bits it leaves are the position the screen opens on.
#[test]
fn each_party_screen_opens_on_the_shared_cursor_retail_leaves() {
    differential!(
        prot(899, 0x801D_8308),
        prot(899, 0x801D_7FF8),
        prot(899, 0x801D_9280),
        prot(899, 0x801D_9594),
        prot(899, 0x801D_98F0),
        prot(899, SCREEN)
    );
    use legaia_engine_menus::field_menu::{PartyCursorScreen, party_cursor_on_entry};
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &disc().expect("disc").read_entry(899).expect("PROT 0899"),
    );
    let screens = [
        (
            0x801D_8308u32,
            0x801D_8734u32,
            PartyCursorScreen::ItemTarget,
        ),
        (0x801D_7FF8, 0x801D_8308, PartyCursorScreen::ItemAllParty),
        (0x801D_9280, 0x801D_9594, PartyCursorScreen::SpellGroup),
        (0x801D_9594, 0x801D_98F0, PartyCursorScreen::SpellTarget),
        (0x801D_98F0, 0x801D_99F0, PartyCursorScreen::EquipCharacter),
        (SCREEN, SCREEN_END, PartyCursorScreen::Status),
    ];
    let mut rng = Rng(0x5041_5254);
    for case in 0..3000 {
        let (entry, end, screen) = screens[case % screens.len()];
        cpu.stub_outside = Some((entry, end));
        let cursor = rng.below(3);
        let flags = rng.next() & 0xF000;
        cpu.mem.wr32(CHAR_CURSOR, flags | cursor);
        cpu.mem.wr32(STEP, 0);
        assert_eq!(cpu.call(entry, &[0]), Stop::Returned, "case {case}");
        let got = cpu.mem.rd32(CHAR_CURSOR) & 0xFFF;
        let want = u32::from(party_cursor_on_entry(screen, cursor as u8));
        assert_eq!(
            got,
            want,
            "case {case}: {screen:?} from {:#x}",
            flags | cursor
        );
        assert_eq!(cpu.mem.rd32(STEP), 1, "case {case}: {screen:?} step 0 ran");
    }
}

/// The browse frame's page keys (`0x801DA8AC..0x801DA990`, the picker
/// answering nothing) against `ListOrderSession::tick`: Right (`0x2000`)
/// pages forward by seven, by the remainder, or drops the hand on the last
/// row; Left (`0x8000`) pages back or to the top; both at once run one after
/// the other. Lists of one to forty rows from the page's first row, the hand
/// and the window (`0x8007BB98` / `0x8007BB90`) and the move cue compared
/// after every frame.
#[test]
fn the_list_order_pages_as_retail_pages() {
    differential!(prot(899, SCREEN));
    use legaia_engine_menus::list_order::{ListOrderEvent, ListOrderRow, ListOrderSession};
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &disc().expect("disc").read_entry(899).expect("PROT 0899"),
    );
    cpu.stub_outside = Some((SCREEN, SCREEN_END));
    cpu.stub(PICKER, 0);
    let cues = Rc::new(RefCell::new(Vec::<u32>::new()));
    for va in [SFX_A, SFX_B] {
        let c = cues.clone();
        cpu.hook(va, move |r, _| c.borrow_mut().push(r.r[A0]));
    }
    let raseru: Vec<usize> = (0..4)
        .map(|c| cpu.mem.rd16(RASERU_SLOTS + 2 * c) as usize)
        .collect();
    let mut rng = Rng(0x5041_4745);
    let (mut forward, mut back, mut both) = (0u32, 0u32, 0u32);
    for case in 0..3000 {
        let step = 5 + rng.below(3);
        let character = rng.below(3) as u8;
        let mut record: Vec<u8> = (0..STRIDE).map(|_| rng.next() as u8).collect();
        record[0x13C] = 1 + rng.below(40) as u8;
        record[0x185] = 1 + rng.below(40) as u8;
        record[0x196 + raseru[usize::from(character)]] |= 1;
        let len = sub15_list_len(step as u8, &record, raseru[usize::from(character)]);
        if len == 0 {
            continue;
        }
        let rows = (0..len)
            .map(|id| ListOrderRow {
                id,
                label: String::new(),
            })
            .collect();
        let mut port = ListOrderSession::open(step as u8, 0, rows).expect("a browse page");
        cpu.mem
            .load(RECORDS + STRIDE * u32::from(character), &record);
        cpu.mem.wr8(PARTY_ORDER, character);
        cpu.mem.wr32(STEP, step);
        cpu.mem.wr32(SUB_ROW, 0);
        cpu.mem.wr32(CHAR_CURSOR, 0);
        cpu.mem.wr32(ANCHOR, 0x1000);
        cpu.mem.wr32(ROW_CURSOR, 0);
        cpu.mem.wr32(SCROLL, 0);
        for frame in 0..16 {
            let pad: u32 = [0x2000, 0x2000, 0x8000, 0xA000, 0][rng.below(5) as usize];
            cpu.mem.wr32(PAD, pad);
            cues.borrow_mut().clear();
            assert_eq!(cpu.call(SCREEN, &[]), Stop::Returned, "case {case}");
            // Retail's menu word carries the d-pad in its high byte; the
            // port's pad is the controller's own order.
            let event = port.tick((pad as u16).swap_bytes());
            let ctx = format!("case {case} frame {frame}: len {len} pad {pad:#x}");
            assert_eq!(
                (
                    cpu.mem.rd32(ROW_CURSOR) as usize,
                    cpu.mem.rd32(SCROLL) as usize
                ),
                (port.cursor(), port.scroll_top()),
                "{ctx}: hand and window"
            );
            let moves = cues.borrow().iter().filter(|&&c| c == 0x21).count();
            let want = match (pad, event) {
                (0xA000, _) => 2,
                (0, _) => 0,
                (_, ListOrderEvent::Moved) => 1,
                _ => 0,
            };
            assert_eq!(moves, want, "{ctx}: the move cue");
            forward += u32::from(pad == 0x2000);
            back += u32::from(pad == 0x8000);
            both += u32::from(pad == 0xA000);
        }
    }
    assert!(
        forward > 5000 && back > 5000 && both > 5000,
        "{forward} {back} {both}"
    );
}
