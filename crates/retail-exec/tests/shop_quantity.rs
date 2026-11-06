//! The shop's two quantity steppers, buy `FUN_801DB7F4` and sell
//! `FUN_801DBD94` (PROT 0899), against `BuyQuantitySession` /
//! `SellQuantitySession` and the purse and bag writes the menu runtime folds
//! their commits into: generated pad scripts over real item prices, purses at
//! and around each bound, stacks from one to ninety-nine, with and without the
//! Point Card.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_menus::shop::{
    BuyQtyEvent, BuyQuantitySession, SellQtyEvent, SellQuantitySession, apply_point_card,
    apply_sale_gold,
};
use legaia_engine_system::input::PadButton;
use legaia_retail_exec::{Cpu, Stop};

const OVERLAY_BASE: u32 = 0x801C_E818;
const BUY: u32 = 0x801D_B7F4;
const SELL: u32 = 0x801D_BD94;
const PHASE: u32 = 0x801E_46AC;
const SUBMENU: u32 = 0x801E_46A4;
const ITEM: u32 = 0x801E_46B0;
const QTY: u32 = 0x801E_46B4;
const MAX: u32 = 0x801E_46B8;
const STAGED: u32 = 0x8007_BB88;
const BUSY: u32 = 0x8007_BB80;
const EDGE: u32 = 0x8007_B874;
const REPEAT: u32 = 0x8007_BB84;
const CONFIRM_MASK: u32 = 0x8008_46D0;
const CANCEL_MASK: u32 = 0x8008_46D4;
const GOLD: u32 = 0x8008_459C;
const POINTS: u32 = 0x8008_45B4;
const BAG: u32 = 0x8008_5958;
const WINDOW_LO: u32 = 0x8007_B5EA;
const WINDOW_HI: u32 = 0x8007_B5EC;
const ITEM_TABLE: u32 = 0x8007_4368;
const POINT_CARD: u8 = 0xFE;

/// Retail pad bits and the port's for the same button.
const BUTTONS: [(u32, PadButton); 6] = [
    (0x1000, PadButton::Up),
    (0x2000, PadButton::Right),
    (0x4000, PadButton::Down),
    (0x8000, PadButton::Left),
    (0x0040, PadButton::Cross),
    (0x0020, PadButton::Circle),
];

fn rig() -> Option<Cpu> {
    let (mut cpu, disc) = (scus_cpu()?, disc()?);
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(899).expect("PROT 0899"));
    for stub in [
        0x801D_6628u32,
        0x8003_5BD0,
        0x8003_5B50,
        0x8003_1D00,
        0x8003_5334,
    ] {
        cpu.stub(stub, 0);
    }
    Some(cpu)
}

fn price(cpu: &mut Cpu, id: u8) -> u16 {
    cpu.mem.rd16(ITEM_TABLE + u32::from(id) * 12 + 2)
}

fn press(rng: &mut Rng) -> u32 {
    match rng.below(12) {
        0 => 0x40,
        1 => 0x20,
        2 => 0,
        3 => BUTTONS[rng.below(4) as usize].0 | BUTTONS[rng.below(4) as usize].0,
        _ => BUTTONS[rng.below(4) as usize].0,
    }
}

fn port_pad(retail: u32) -> u16 {
    BUTTONS
        .iter()
        .filter(|(r, _)| retail & r != 0)
        .fold(0, |w, (_, b)| w | b.mask())
}

/// One stack per id; `(slot, id, count)`.
fn seat_bag(cpu: &mut Cpu, stacks: &[(u32, u8, u8)]) {
    for i in 0..0x100 {
        cpu.mem.wr16(BAG + i * 2, 0);
    }
    for &(slot, id, n) in stacks {
        cpu.mem.wr8(BAG + slot * 2, id);
        cpu.mem.wr8(BAG + slot * 2 + 1, n);
    }
    cpu.mem.wr16(WINDOW_LO, 0);
    cpu.mem.wr16(WINDOW_HI, 0x100);
}

fn frame(cpu: &mut Cpu, entry: u32, pad: u32) {
    cpu.mem.wr32(EDGE, pad);
    cpu.mem.wr32(REPEAT, pad);
    assert_eq!(cpu.call(entry, &[]), Stop::Returned);
}

fn count_of(cpu: &mut Cpu, id: u8) -> u8 {
    (0..0x100)
        .find(|&i| cpu.mem.rd8(BAG + i * 2) == id)
        .map_or(0, |i| cpu.mem.rd8(BAG + i * 2 + 1))
}

#[test]
fn the_buy_stepper_buys_what_retail_buys() {
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let ids: Vec<u8> = (1..=0xFDu8)
        .filter(|&id| price(&mut cpu, id) != 0)
        .collect();
    let mut rng = Rng(0x5348_4F50);
    let (mut bought, mut capped, mut cancelled, mut at_cap) = (0, 0, 0, 0);
    for round in 0..6000 {
        let id = ids[rng.below(ids.len() as u32) as usize];
        let p = price(&mut cpu, id);
        let gold: u32 = match rng.below(4) {
            0 => u32::from(p) * (1 + rng.below(3)),
            1 => u32::from(p) * 99 + rng.below(u32::from(p)),
            2 => 9_999_999,
            _ => u32::from(p) + rng.below(200_000),
        };
        let held: Option<u8> = match rng.below(5) {
            0 => Some(99),
            1 => Some(1 + rng.below(98) as u8),
            _ => None,
        };
        let card = rng.below(3) == 0;
        let points = rng.below(9_999_999);
        let mut stacks = Vec::new();
        if let Some(n) = held {
            stacks.push((rng.below(40), id, n));
        }
        if card {
            stacks.push((40 + rng.below(40), POINT_CARD, 1));
        }
        seat_bag(&mut cpu, &stacks);
        cpu.mem.wr32(PHASE, 0);
        cpu.mem.wr32(SUBMENU, 0x1D);
        cpu.mem.wr32(STAGED, u32::from(id));
        cpu.mem.wr32(ITEM, u32::from(id));
        cpu.mem.wr32(QTY, 1);
        cpu.mem.wr32(BUSY, 0);
        cpu.mem.wr32(CONFIRM_MASK, 0x40);
        cpu.mem.wr32(CANCEL_MASK, 0x20);
        cpu.mem.wr32(GOLD, gold);
        cpu.mem.wr32(POINTS, points);

        let mut s = BuyQuantitySession::new(id, p, gold as i32, held, card);
        let (mut port_gold, mut port_count, mut port_points) =
            (gold as i32, held.unwrap_or(0), points as i32);
        let mut port_done = false;
        let mut committed = false;
        // Phase 0 on both sides.
        frame(&mut cpu, BUY, 0);
        assert_eq!(s.tick(0), BuyQtyEvent::None);
        assert_eq!(cpu.mem.rd32(MAX) as i32, s.max, "round {round} max");
        for step in 0..40 {
            let pad = if step < 30 { press(&mut rng) } else { 0x40 };
            frame(&mut cpu, BUY, pad);
            if !port_done {
                match s.tick(port_pad(pad)) {
                    BuyQtyEvent::Bought {
                        qty,
                        cost,
                        point_credit,
                        ..
                    } => {
                        // `MenuRuntime::apply_quantity_buy`: refused past the
                        // stack cap.
                        if u32::from(port_count) + u32::from(qty) <= 99 {
                            port_gold -= cost;
                            port_count += qty;
                            if point_credit > 0 {
                                port_points = apply_point_card(port_points, point_credit);
                            }
                        }
                        committed = true;
                    }
                    BuyQtyEvent::Cancelled | BuyQtyEvent::ExitToBuyList => port_done = true,
                    _ => {}
                }
            }
            let phase = cpu.mem.rd32(PHASE);
            if phase == 1 && !committed {
                assert_eq!(
                    cpu.mem.rd32(QTY) as i32,
                    s.qty,
                    "round {round} step {step} pad {pad:#x} max {}",
                    s.max
                );
            }
            if cpu.mem.rd32(SUBMENU) != 0x1D {
                break;
            }
            // The Point Card note waits for a press; the port's toast takes
            // the next one too.
            if phase == 4 && !port_done {
                frame(&mut cpu, BUY, 0x40);
                s.tick(PadButton::Cross.mask());
                break;
            }
        }
        let retail = (
            cpu.mem.rd32(GOLD) as i32,
            count_of(&mut cpu, id),
            cpu.mem.rd32(POINTS) as i32,
        );
        if held == Some(99) && committed {
            // Retail opens the stepper on a full stack with a bound of `0`,
            // buys the one copy the stepper starts on, debits it and lets the
            // add trim it away; the port's runtime refuses that buy.
            assert_eq!(retail.0, gold as i32 - i32::from(p), "round {round}");
            assert_eq!(retail.1, 99);
            assert_eq!((port_gold, port_count), (gold as i32, 99));
            at_cap += 1;
            continue;
        }
        assert_eq!(
            retail,
            (port_gold, port_count, port_points),
            "round {round}: id {id:#x} price {p} gold {gold} held {held:?} card {card}"
        );
        if committed {
            bought += 1;
            capped += i32::from(held.is_some() && s.max < 99);
        } else {
            cancelled += 1;
        }
    }
    assert!(
        bought > 1000 && cancelled > 500 && capped > 100 && at_cap > 100,
        "{bought} {cancelled} {capped} {at_cap}"
    );
}

#[test]
fn the_sell_stepper_sells_what_retail_sells() {
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let ids: Vec<u8> = (1..=0xFDu8)
        .filter(|&id| price(&mut cpu, id) != 0)
        .collect();
    let mut rng = Rng(0x5345_4C4C);
    let (mut sold, mut whole, mut cancelled) = (0, 0, 0);
    for round in 0..6000 {
        let id = ids[rng.below(ids.len() as u32) as usize];
        let p = price(&mut cpu, id);
        let held = [1u8, 2, 9, 10, 11, 50, 98, 99][rng.below(8) as usize];
        let gold = [0u32, 1000, 9_999_000, 9_999_999][rng.below(4) as usize];
        let slot = rng.below(64);
        let mut stacks = vec![(slot, id, held)];
        if rng.below(2) == 0 {
            stacks.push((64 + rng.below(64), if id == 1 { 2 } else { 1 }, 3));
        }
        seat_bag(&mut cpu, &stacks);
        cpu.mem.wr32(PHASE, 0);
        cpu.mem.wr32(SUBMENU, 0x1F);
        cpu.mem.wr32(STAGED, slot);
        cpu.mem.wr32(ITEM, u32::from(id));
        cpu.mem.wr32(BUSY, 0);
        cpu.mem.wr32(CONFIRM_MASK, 0x40);
        cpu.mem.wr32(CANCEL_MASK, 0x20);
        cpu.mem.wr32(GOLD, gold);

        let mut s = SellQuantitySession::new(id, p, held);
        let (mut port_gold, mut port_count) = (gold as i32, held);
        let mut port_sold = false;
        let mut port_cancel = false;
        frame(&mut cpu, SELL, 0);
        s.tick(0, 1);
        assert_eq!(cpu.mem.rd32(MAX) as i32, s.max);
        for step in 0..40 {
            let pad = if step < 30 { press(&mut rng) } else { 0x40 };
            frame(&mut cpu, SELL, pad);
            if !port_sold && !port_cancel {
                match s.tick(port_pad(pad), 1) {
                    SellQtyEvent::Sold { qty, credit, .. } => {
                        port_gold = apply_sale_gold(port_gold, credit);
                        port_count -= qty;
                        port_sold = true;
                    }
                    SellQtyEvent::Cancelled => port_cancel = true,
                    _ => {}
                }
                if !port_sold && !port_cancel {
                    assert_eq!(cpu.mem.rd32(QTY) as i32, s.qty, "round {round} step {step}");
                }
            }
            if cpu.mem.rd32(SUBMENU) != 0x1F || cpu.mem.rd32(PHASE) == 2 {
                break;
            }
        }
        let retail = (cpu.mem.rd32(GOLD) as i32, count_of(&mut cpu, id));
        assert_eq!(
            retail,
            (port_gold, port_count),
            "round {round}: id {id:#x} price {p} gold {gold} held {held}"
        );
        assert_eq!(
            cpu.mem.rd32(SUBMENU) == 0x1E || cpu.mem.rd32(PHASE) == 2,
            port_sold || port_cancel
        );
        if port_sold {
            sold += 1;
            whole += i32::from(port_count == 0);
        } else {
            cancelled += 1;
        }
    }
    assert!(
        sold > 1000 && whole > 200 && cancelled > 500,
        "{sold} {whole} {cancelled}"
    );
}
