//! The Muscle Dome ladder in PROT 0977: the arena's re-entry `FUN_801CEA6C`, the
//! leg's rows `FUN_801D1184` and the settlement `FUN_801D0F60`, whole contests at
//! a time, against `DomeContest`. Unlock flags, gates and outcomes are random.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::muscle_dome::{
    COURSE_COUNT, ContestFlags, ContestState, DomeContest, LegReport,
};
use legaia_retail_exec::{Stop, V0};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const OVERLAY_END: u32 = 0x801D_2018;
const ENTRY: u32 = 0x801C_EA6C;
const ROWS: u32 = 0x801D_1184;
const SETTLE: u32 = 0x801D_0F60;
const WORD: u32 = 0x8007_BAC0;
const STANDING: u32 = 0x8007_BD60;
const STATE: u32 = 0x801D_1A78;
const GAVE_UP: u32 = 0x801D_1A74;
const LATCH: u32 = 0x801D_1ADC;
const TALLY: u32 = 0x8008_4440;
const TURNS: u32 = 0x8008_4444;
const OUTCOME: u32 = 0x8008_4448;
const HP_MAX: u32 = 0x8008_480C;
const ROW_LANES: [u32; 4] = [0x801D_1ACC, 0x801D_1AD0, 0x801D_1AD4, 0x801D_1AAC];

#[test]
fn a_contest_runs_and_settles_as_retails_does() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(977).expect("PROT 0977");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    cpu.stub_outside = Some((OVERLAY_BASE, OVERLAY_END));
    let flags = Rc::new(RefCell::new(BTreeSet::<u32>::new()));
    let items = Rc::new(RefCell::new(Vec::<u32>::new()));
    let f = flags.clone();
    cpu.hook(0x8003_CE64, move |r, _| {
        r.r[V0] = u32::from(f.borrow().contains(&r.r[legaia_retail_exec::A0]));
    });
    let f = flags.clone();
    cpu.hook(0x8003_CE08, move |r, _| {
        f.borrow_mut().insert(r.r[legaia_retail_exec::A0]);
    });
    let f = flags.clone();
    cpu.hook(0x8003_CE34, move |r, _| {
        f.borrow_mut().remove(&r.r[legaia_retail_exec::A0]);
    });
    let i = items.clone();
    cpu.hook(0x8004_21D4, move |r, _| {
        i.borrow_mut().push(r.r[legaia_retail_exec::A0]);
    });
    // The two spin waits on the vsync counter.
    cpu.hook(0x8005_FB84, |_, m| m.wr32(0x8007_B8AC, 0x100));
    cpu.hook(0x8001_7888, |r, _| r.r[V0] = 0x8010_0000);

    let mut rng = Rng(0x444F_4D45);
    let (mut settled, mut prizes, mut ran) = (0, 0, 0);
    for contest in 0..4000 {
        let unlock = [rng.below(2) == 0, rng.below(2) == 0, rng.below(2) == 0];
        let gates = [rng.below(2) == 0, rng.below(2) == 0, rng.below(2) == 0];
        let prize = rng.below(4) == 0;
        flags.borrow_mut().clear();
        items.borrow_mut().clear();
        for (i, on) in unlock.iter().enumerate() {
            if *on {
                flags.borrow_mut().insert(0x536 + i as u32);
            }
        }
        for (id, on) in [0x378, 0x382, 0x471].iter().zip(gates) {
            if on {
                flags.borrow_mut().insert(*id);
            }
        }
        if prize {
            flags.borrow_mut().insert(0x6CB);
        }
        let cf = ContestFlags {
            course_unlock: unlock,
            master_gates: gates,
            prize_awarded: prize,
        };
        let mut port = DomeContest::from_overlay(&overlay, &cf).expect("contest");
        let hp_max = 1 + rng.below(999) as u16;
        cpu.mem.wr16(HP_MAX, hp_max);
        cpu.mem.wr32(WORD, 0);
        cpu.mem.wr32(TALLY, 0x7777);
        assert_eq!(cpu.call(ENTRY, &[]), Stop::Returned, "{:x?}", cpu.calls);
        assert_eq!(
            cpu.mem.rd32(WORD),
            port.word(),
            "contest {contest}: entry word"
        );
        assert_eq!(cpu.mem.rd32(TALLY) as i32, port.tally(), "entry tally");
        assert_eq!(cpu.mem.rd32(STATE), 0, "first visit");

        let survive_odds = 2 + rng.below(30);
        for leg in 0..40 {
            let outcome = [0u32, 1, 2, 3, 4, 7][rng.below(if leg == 0 { 6 } else { 5 }) as usize];
            let survived = rng.below(survive_odds) != 0;
            let turns = rng.below(14);
            cpu.mem.wr8(STANDING, if survived { 0x80 } else { 0 });
            cpu.mem.wr32(OUTCOME, outcome);
            cpu.mem.wr32(TURNS, turns);
            assert_eq!(cpu.call(ENTRY, &[]), Stop::Returned, "{:x?}", cpu.calls);
            port.finish_leg(
                LegReport {
                    survived,
                    outcome,
                    turns_taken: turns,
                },
                hp_max,
                &cf,
            );
            let at = format!("contest {contest} leg {leg} outcome {outcome} survived {survived}");
            assert_eq!(cpu.mem.rd32(WORD), port.word(), "{at}: word");
            assert_eq!(cpu.mem.rd32(STATE), port.state() as u32, "{at}: state");
            assert_eq!(
                cpu.mem.rd32(LATCH) != 0,
                port.continue_latch(),
                "{at}: latch"
            );
            assert_eq!(cpu.mem.rd32(GAVE_UP) != 0, port.gave_up(), "{at}: gave up");
            assert_eq!(cpu.call(ROWS, &[]), Stop::Returned);
            let rows = port.rows();
            assert_eq!(
                ROW_LANES.map(|a| cpu.mem.rd32(a) as i32),
                [
                    rows.round_lane,
                    rows.turns_lane,
                    rows.outcome_lane,
                    rows.score_cell
                ],
                "{at}: rows"
            );
            if port.state() == ContestState::LegScore {
                // The screen's drain: the score cell into the tally.
                let tally = cpu.mem.rd32(TALLY) as i32;
                cpu.mem.wr32(TALLY, (tally + rows.score_cell) as u32);
                port.advance();
                port.advance();
                port.advance();
                assert_eq!(port.state(), ContestState::Fight);
                assert_eq!(cpu.mem.rd32(TALLY) as i32, port.tally(), "{at}: tally");
                continue;
            }
            assert!(port.over(), "{at}");
            let out = port.settle(&cf);
            assert_eq!(cpu.call(SETTLE, &[]), Stop::Returned, "{:x?}", cpu.calls);
            assert_eq!(cpu.mem.rd32(TALLY) as i32, out.score, "{at}: settled tally");
            assert_eq!(
                cpu.mem.rd32(LATCH) != 0,
                out.continuing,
                "{at}: settled latch"
            );
            let f = flags.borrow();
            assert_eq!(f.contains(&0x50A), out.set_continue_flag, "{at}: 0x50A");
            assert_eq!(f.contains(&0x35), out.set_gave_up_flag, "{at}: 0x35");
            for c in 0..COURSE_COUNT as u32 {
                assert_eq!(
                    f.contains(&(0x130 + c)),
                    out.set_ran_first_flag == Some(0x130 + c as u16),
                    "{at}: ran-first {c}"
                );
            }
            assert_eq!(
                items.borrow().as_slice(),
                if out.award_prize {
                    &[0xCD][..]
                } else {
                    &[][..]
                },
                "{at}: prize"
            );
            settled += 1;
            prizes += usize::from(out.award_prize);
            ran += usize::from(out.set_gave_up_flag);
            break;
        }
    }
    assert!(
        settled > 3000 && prizes > 0 && ran > 100,
        "{settled} {prizes} {ran}"
    );
}
