//! The casino coin counter `FUN_801F0ADC` (PROT 0897), driven frame by frame
//! with generated pad words against `submode_hub_actors::coin_exchange`: the
//! eight digit cells, the digit cursor, the Yes/No row, the hold timer, the
//! published ceiling, the purse and the coin bank, the stings, the system
//! flags it raises and clears, and the hand-back.
//!
//! Hooked and logged: the stings `FUN_80035B50` / `FUN_80035BD0`, the flag
//! bank's set and clear `FUN_8003CE08` / `FUN_8003CE34`, the panel installer
//! `FUN_801E9B3C`; the Yes/No picker `FUN_801E9DC8` answers from the script.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::submode_hub_actors::{
    CoinCounter, HubAction, HubActor, HubEnv, HubGrid, coin_exchange,
};
use psx_retail_exec::{A0, Stop, V0};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const COUNTER: u32 = 0x801F_0ADC;
const PUSH_CUE: u32 = 0x8003_5B50;
const REPLACE_CUE: u32 = 0x8003_5BD0;
const FLAG_SET: u32 = 0x8003_CE08;
const FLAG_CLEAR: u32 = 0x8003_CE34;
const INSTALL_PANEL: u32 = 0x801E_9B3C;
const PICKER: u32 = 0x801E_9DC8;
const DRAW_PUMP: u32 = 0x8003_1D00;

const GOLD: u32 = 0x8008_459C;
const BANK: u32 = 0x8008_45A4;
const CEILING: u32 = 0x8007_BB90;
const BLOCKED: u32 = 0x8007_BB80;
const PAD_EDGE: u32 = 0x8007_B874;
const ACCEPT_MASK: u32 = 0x8008_46D0;
const BACK_MASK: u32 = 0x8008_46D4;
const PAD_REPEAT: u32 = 0x8007_BB84;
const CURSOR: u32 = 0x8007_BB9C;
const YES_NO: u32 = 0x8007_BB98;
const ROW: u32 = 0x8007_BB88;
const HOLD: u32 = 0x8007_B458;
const DIGITS: u32 = 0x801F_35F0;
const OPERAND_PTR: u32 = 0x8007_B450;
const FRAME_STEP: u32 = 0x1F80_0393;
const GRID_PTR: u32 = 0x801C_6EA4;

const ACTOR: u32 = 0x8012_0000;
const GRID: u32 = 0x8012_1000;
const OPERAND: u32 = 0x8012_2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ev {
    Push(u8),
    Replace(u8),
    Set(u16),
    Clear(u16),
    Panel(u32),
}

#[test]
fn the_counter_sells_what_retail_sells() {
    differential!(prot(897, COUNTER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let log: Rc<RefCell<Vec<Ev>>> = Rc::new(RefCell::new(Vec::new()));
    for (at, make) in [
        (PUSH_CUE, (|a| Ev::Push(a as u8)) as fn(u32) -> Ev),
        (REPLACE_CUE, |a| Ev::Replace(a as u8)),
        (FLAG_SET, |a| Ev::Set(a as u16)),
        (FLAG_CLEAR, |a| Ev::Clear(a as u16)),
        (INSTALL_PANEL, |a| Ev::Panel(a)),
    ] {
        let l = log.clone();
        cpu.hook(at, move |r, _| l.borrow_mut().push(make(r.r[A0])));
    }
    let picker = Rc::new(Cell::new(0u32));
    let p = picker.clone();
    cpu.hook(PICKER, move |r, _| r.r[V0] = p.get());
    cpu.stub(DRAW_PUMP, 0);
    cpu.mem.wr32(GRID_PTR, GRID);
    cpu.mem.wr32(OPERAND_PTR, OPERAND);
    cpu.mem.wr32(ACCEPT_MASK, 0x20);
    cpu.mem.wr32(BACK_MASK, 0x40);

    let mut rng = Rng(0x434F_494E);
    let (mut sold, mut refused, mut failures) = (0, 0, Vec::new());
    for run in 0..1500 {
        let gold = match rng.below(4) {
            0 => rng.below(1000) as i32,
            1 => rng.below(100_000) as i32,
            2 => 1_000_000_000 + rng.below(1000) as i32,
            _ => rng.below(5_000_000) as i32,
        };
        let bank = match rng.below(3) {
            0 => 9_999_999 - rng.below(50) as i32,
            _ => rng.below(200_000) as i32,
        };
        let flag = rng.below(0x100) as i16;
        cpu.mem.wr32(GOLD, gold as u32);
        cpu.mem.wr32(BANK, bank as u32);
        cpu.mem.wr16(OPERAND + 2, flag as u16);
        for a in [CURSOR, YES_NO, ROW, HOLD, CEILING, BLOCKED] {
            cpu.mem.wr32(a, 0);
        }
        // A third of the runs open mid-entry, on cells the clamp never saw.
        let mut counter = CoinCounter::default();
        let sub = if rng.below(3) == 0 {
            for d in counter.digits.iter_mut() {
                *d = rng.below(10) as i8;
            }
            counter.cursor = rng.below(6) as i32;
            1
        } else {
            0
        };
        for (i, d) in counter.digits.iter().enumerate() {
            cpu.mem.wr8(DIGITS + i as u32, *d as u8);
        }
        cpu.mem.wr32(CURSOR, counter.cursor as u32);
        cpu.mem.wr16(ACTOR + 0x50, 0x25);
        cpu.mem.wr16(ACTOR + 0x54, sub as u16);
        cpu.mem.wr16(GRID + 0x2E, 0);
        cpu.mem.wr16(GRID + 0x40, 0);

        let mut actor = HubActor {
            state: 0x25,
            sub,
            ..Default::default()
        };
        let mut grid = HubGrid::default();
        let (mut p_gold, mut p_bank) = (gold, bank);
        for frame in 0..300 {
            let edge = match rng.below(12) {
                0 => 0x20,
                1 => 0x40,
                _ => 0,
            };
            let repeat = [0, 0, 0x1000, 0x4000, 0x2000, 0x8000, 0x5000][rng.below(7) as usize];
            let blocked = i32::from(rng.below(10) == 0);
            let pick = [0, 0, 0, 1, 2][rng.below(5) as usize];
            let yes_no = rng.below(2) as i32;
            let step = 1 + rng.below(3) as u8;
            // The picker owns the Yes/No row in retail; both sides see the same pick.
            if actor.sub == 2 {
                counter.yes_no = yes_no;
                cpu.mem.wr32(YES_NO, yes_no as u32);
            }
            cpu.mem.wr32(PAD_EDGE, edge);
            cpu.mem.wr32(PAD_REPEAT, repeat);
            cpu.mem.wr32(BLOCKED, blocked as u32);
            cpu.mem.wr8(FRAME_STEP, step);
            picker.set(pick);
            log.borrow_mut().clear();
            assert_eq!(cpu.call(COUNTER, &[ACTOR]), Stop::Returned);
            let digits: Vec<i8> = (0..8).map(|i| cpu.mem.rd8(DIGITS + i) as i8).collect();
            let retail = (
                cpu.mem.rd16(ACTOR + 0x50),
                cpu.mem.rd16(ACTOR + 0x54) as i16,
                digits,
                cpu.mem.rd32(CURSOR) as i32,
                cpu.mem.rd32(HOLD) as i32,
                cpu.mem.rd32(CEILING) as i32,
                cpu.mem.rd32(GOLD) as i32,
                cpu.mem.rd32(BANK) as i32,
                cpu.mem.rd16(GRID + 0x2E) as i16,
                log.borrow().clone(),
            );

            let env = HubEnv {
                gold: p_gold,
                coin_bank: p_bank,
                input_blocked: blocked,
                pad_edge: edge,
                accept_mask: 0x20,
                back_mask: 0x40,
                pad_repeat: repeat,
                picker_result: pick as i32,
                frame_delta: i32::from(step),
                script_flag: flag,
                ..Default::default()
            };
            let out = coin_exchange(&mut actor, &env, &mut counter, &mut grid);
            let mut events = Vec::new();
            for a in &out.actions {
                match *a {
                    HubAction::EntryCue(id) => events.push(Ev::Push(id)),
                    HubAction::ConfirmCue(id) => events.push(Ev::Replace(id)),
                    HubAction::SetSystemFlag(f) => events.push(Ev::Set(f)),
                    HubAction::ClearSystemFlag(f) => events.push(Ev::Clear(f)),
                    HubAction::InstallPanel(d) => events.push(Ev::Panel(d)),
                    HubAction::BuyCoins { coins, gold_cost } => {
                        p_bank = p_bank.wrapping_add(coins);
                        p_gold = p_gold.wrapping_sub(gold_cost);
                        sold += 1;
                    }
                    _ => {}
                }
            }
            refused += u32::from(events.contains(&Ev::Replace(0x23)));
            let port = (
                actor.state,
                actor.sub,
                counter.digits.to_vec(),
                counter.cursor,
                counter.hold,
                counter.ceiling,
                p_gold,
                p_bank,
                grid.handback,
                events,
            );
            if retail != port {
                failures.push(format!(
                    "run {run} frame {frame}: gold {gold} bank {bank} edge {edge:#x} repeat \
                     {repeat:#x} blocked {blocked} pick {pick} yes_no {yes_no}\n retail \
                     {retail:?}\n port   {port:?}"
                ));
                break;
            }
            if actor.state != 0x25 {
                break;
            }
        }
        if failures.len() >= 6 {
            break;
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(sold > 100 && refused > 15, "{sold} {refused}");
}
