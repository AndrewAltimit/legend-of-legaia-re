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
