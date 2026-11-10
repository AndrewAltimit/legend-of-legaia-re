//! The casino prize counter `FUN_801DC1CC` (PROT 0899, sub-screen `0x20`)
//! frame by frame with the list kernel `FUN_801D688C` live beneath it,
//! against `PrizeExchangeSession` and the commit its host applies
//! (`apply_redeem`, then `rebuild`): every block of the disc's prize table,
//! coin banks short of and past each price, held counts at and under the
//! cap, one-shot gates raised and clear, and generated pad words.
//!
//! The flag bank, the bag's count and grant, the window VM, both cue
//! enqueues and the window tick are hooked; the cue ring is kept as retail
//! keeps it (`FUN_80035B50` pushes a slot, `FUN_80035BD0` rewrites the last
//! one), so the cue a frame leaves is compared with the port's event cue.
//! Each frame compares the step, the exit, both cursors, the visible rows,
//! the coin bank, the bag, the flags and that cue.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::prize_exchange::{
    PRIZE_EXCHANGE_VISITED_FLAG, PrizeEvent, PrizeExchangeSession, RedeemBag, apply_redeem,
    parse_blocks,
};
use legaia_engine_system::menu_input::NavButtons;
use psx_retail_exec::{A0, A1, Stop, V0};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const COUNTER: u32 = 0x801D_C1CC;
const LIST: u32 = 0x801D_688C;
const VM: u32 = 0x801D_6628;
const FLAG_SET: u32 = 0x8003_CE08;
const FLAG_TEST: u32 = 0x8003_CE64;
const HELD: u32 = 0x8004_2F4C;
const GRANT: u32 = 0x8004_21D4;
const PUSH: u32 = 0x8003_5B50;
const REWRITE: u32 = 0x8003_5BD0;
const WINDOW_TICK: u32 = 0x8003_1D00;

const CONTEXT: u32 = 0x8007_B450;
const CONTEXT_BYTES: u32 = 0x8013_0000;
const SCREEN: u32 = 0x801E_46A4;
const STEP: u32 = 0x801E_46AC;
const CONFIRM: u32 = 0x801E_46D0;
const CURSOR: u32 = 0x8007_BB98;
const ROW_COUNT: u32 = 0x801E_F0D0;
const ROWS: u32 = 0x801E_F0E0;
const COINS: u32 = 0x8008_45A4;
const BUSY: u32 = 0x8007_BB80;
const EDGE: u32 = 0x8007_B874;
const REPEAT: u32 = 0x8007_BB84;
const LIST_CONFIRM_MASK: u32 = 0x801E_F0F0;
const LIST_CANCEL_MASK: u32 = 0x801E_F0F4;

#[derive(Default)]
struct World {
    flags: BTreeSet<u16>,
    bag: BTreeMap<u8, u8>,
    ring: Vec<u32>,
    scripts: usize,
}

struct Bag<'a>(&'a mut BTreeMap<u8, u8>);

impl RedeemBag for Bag<'_> {
    fn held(&self, id: u8) -> u8 {
        self.0.get(&id).copied().unwrap_or(0)
    }
    fn grant_one(&mut self, id: u8) {
        let n = self.0.entry(id).or_insert(0);
        *n = n.saturating_add(1);
    }
}

fn edge_word(rng: &mut Rng) -> (u32, u32) {
    let edge = match rng.below(10) {
        0..=2 => 0,
        3 | 4 => 0x40,
        5 => 0x20,
        6 => 0x60,
        _ => [0x1000, 0x4000, 0x2000, 0x8000][rng.below(4) as usize],
    };
    (edge, edge & 0xF000)
}

#[test]
fn the_prize_counter_steps_as_retail_steps() {
    differential!(prot(899, COUNTER));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d.read_entry_footprint(899).expect("PROT 0899");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let blocks = parse_blocks(&overlay).expect("prize table");
    let world: Rc<RefCell<World>> = Rc::default();
    let w = world.clone();
    cpu.hook(FLAG_SET, move |r, _| {
        w.borrow_mut().flags.insert(r.r[A0] as u16);
    });
    let w = world.clone();
    cpu.hook(FLAG_TEST, move |r, _| {
        r.r[V0] = u32::from(w.borrow().flags.contains(&(r.r[A0] as u16)));
    });
    let w = world.clone();
    cpu.hook(HELD, move |r, _| {
        r.r[V0] = u32::from(w.borrow().bag.get(&(r.r[A0] as u8)).copied().unwrap_or(0));
    });
    let w = world.clone();
    cpu.hook(GRANT, move |r, _| {
        let mut w = w.borrow_mut();
        let n = w.bag.entry(r.r[A0] as u8).or_insert(0);
        *n = n.saturating_add(r.r[A1] as u8);
    });
    let w = world.clone();
    cpu.hook(PUSH, move |r, _| w.borrow_mut().ring.push(r.r[A0]));
    let w = world.clone();
    cpu.hook(REWRITE, move |r, _| {
        let mut w = w.borrow_mut();
        match w.ring.last_mut() {
            Some(c) => *c = r.r[A0],
            None => w.ring.push(r.r[A0]),
        }
    });
    let w = world.clone();
    cpu.hook(VM, move |_, _| w.borrow_mut().scripts += 1);
    cpu.stub(WINDOW_TICK, 0);
    cpu.mem.wr32(LIST_CONFIRM_MASK, 0x40);
    cpu.mem.wr32(LIST_CANCEL_MASK, 0x20);
    cpu.mem.wr32(BUSY, 0);
    cpu.mem.wr32(CONTEXT, CONTEXT_BYTES);

    let mut rng = Rng(0x5052_495A);
    let (mut redeemed, mut refused, mut gated) = (0, 0, 0);
    for episode in 0..3000 {
        let block = rng.below(4) as usize;
        cpu.mem.wr8(CONTEXT_BYTES, 7);
        cpu.mem.wr8(CONTEXT_BYTES + 1, block as u8);
        // The port's view of the same flags and bag, kept by its host.
        let mut flags = BTreeSet::new();
        let mut bag = BTreeMap::new();
        for rec in &blocks[block] {
            if rec.gate != 0 && rng.below(3) == 0 {
                flags.insert(rec.gate);
            }
            if rng.below(3) == 0 {
                bag.insert(
                    rec.item_id,
                    [98u8, 99, rng.below(99) as u8][rng.below(3) as usize],
                );
            }
        }
        let coins = [0, rng.below(1200), rng.below(12_000), 9_999_999][rng.below(4) as usize];
        {
            let mut w = world.borrow_mut();
            w.flags = flags.clone();
            w.bag = bag.clone();
            w.ring.clear();
            w.scripts = 0;
        }
        cpu.mem.wr32(COINS, coins);
        let mut port_coins = coins;
        cpu.mem.wr32(STEP, 0);
        cpu.mem.wr32(SCREEN, 0x20);
        cpu.mem.wr32(CURSOR, 0xFFFF_FFFF);
        cpu.mem.wr32(CONFIRM, 1);
        assert_eq!(cpu.call(COUNTER, &[]), Stop::Returned);
        // The port's host raises the entry flag and builds the session.
        flags.insert(PRIZE_EXCHANGE_VISITED_FLAG);
        let mut s = PrizeExchangeSession::new(blocks[block].clone(), |g| flags.contains(&g));
        for frame in 0..60 {
            let what = format!("episode {episode} block {block} frame {frame}");
            {
                let w = world.borrow();
                assert_eq!(w.flags, flags, "{what}: flags");
                assert_eq!(w.bag, bag, "{what}: bag");
            }
            assert_eq!(cpu.mem.rd32(COINS), port_coins, "{what}: coins");
            assert_eq!(cpu.mem.rd32(SCREEN) == 0, s.is_done(), "{what}: exit");
            if s.is_done() {
                break;
            }
            assert_eq!(
                cpu.mem.rd32(STEP),
                u32::from(s.retail_step()),
                "{what}: step"
            );
            assert_eq!(cpu.mem.rd32(CURSOR), s.cursor_word(), "{what}: cursor");
            if s.retail_step() >= 2 {
                assert_eq!(
                    cpu.mem.rd32(CONFIRM),
                    u32::from(s.confirm_cursor()),
                    "{what}: prompt"
                );
            }
            let n = cpu.mem.rd8(ROW_COUNT) as usize;
            let retail_rows: Vec<usize> = (0..n)
                .map(|i| cpu.mem.rd8(ROWS + i as u32) as usize)
                .collect();
            assert_eq!(retail_rows, s.row_indices(), "{what}: rows");

            let (edge, repeat) = edge_word(&mut rng);
            cpu.mem.wr32(EDGE, edge);
            cpu.mem.wr32(REPEAT, repeat);
            let before = world.borrow().ring.len();
            assert_eq!(cpu.call(COUNTER, &[]), Stop::Returned, "{what}");
            let retail_cue = {
                let w = world.borrow();
                (w.ring.len() > before)
                    .then(|| *w.ring.last().unwrap())
                    .filter(|&c| c != 0)
                    .map(|c| c as u8)
            };
            let ev = s.tick(
                NavButtons::new(
                    edge & 0x40 != 0,
                    edge & 0x20 != 0,
                    repeat & 0x1000 != 0,
                    repeat & 0x4000 != 0,
                ),
                port_coins,
                |id| bag.get(&id).copied().unwrap_or(0),
            );
            assert_eq!(retail_cue, ev.cue(), "{what}: cue of {ev:?}");
            match ev {
                PrizeEvent::Redeemed {
                    item_id,
                    price,
                    gate,
                } => {
                    assert!(apply_redeem(
                        &mut port_coins,
                        &mut Bag(&mut bag),
                        &mut |g| {
                            flags.insert(g);
                        },
                        item_id,
                        price,
                        gate,
                    ));
                    s.rebuild(|g| flags.contains(&g));
                    redeemed += 1;
                    gated += u32::from(gate != 0);
                }
                PrizeEvent::Refused(_) => refused += 1,
                _ => {}
            }
        }
    }
    assert!(
        redeemed > 300 && refused > 300 && gated > 30,
        "{redeemed} {refused} {gated}"
    );
}

/// The list kernel's own bytes on the ledger: one direct call, beside the
/// counter that enters it every frame.
#[test]
fn the_list_kernel_matches_its_port() {
    differential!(prot(899, LIST));
    use legaia_engine_system::menu_input::{CursorNav, menu_cursor_nav};
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        OVERLAY_BASE,
        &d.read_entry_footprint(899).expect("PROT 0899"),
    );
    let cues: Rc<RefCell<Vec<u32>>> = Rc::default();
    let c = cues.clone();
    cpu.hook(PUSH, move |r, _| c.borrow_mut().push(r.r[A0]));
    cpu.mem.wr32(LIST_CONFIRM_MASK, 0x40);
    cpu.mem.wr32(LIST_CANCEL_MASK, 0x20);
    let mut rng = Rng(0x4C49_5354);
    let mut moved = 0;
    for run in 0..50_000 {
        let count = [0, 1, 2, rng.below(40)][rng.below(4) as usize];
        let wrap = rng.below(2);
        let mut cell = rng.below(count.max(1)) | [0, 0x1000, 0x8000][rng.below(3) as usize];
        let (edge, repeat) = edge_word(&mut rng);
        cpu.mem.wr32(EDGE, edge);
        cpu.mem.wr32(REPEAT, repeat);
        cpu.mem.wr32(CURSOR, cell);
        cues.borrow_mut().clear();
        assert_eq!(cpu.call(LIST, &[CURSOR, count, wrap]), Stop::Returned);
        let r = cpu.regs.r[V0];
        let nav = menu_cursor_nav(
            &mut cell,
            count,
            wrap != 0,
            NavButtons::new(
                edge & 0x40 != 0,
                edge & 0x20 != 0,
                repeat & 0x1000 != 0,
                repeat & 0x4000 != 0,
            ),
        );
        let p = match nav {
            CursorNav::None => 0,
            CursorNav::Confirm => 1,
            CursorNav::Cancel => 2,
            CursorNav::Moved => 3,
        };
        let what = format!("run {run}: count {count} wrap {wrap} edge {edge:#x}");
        assert_eq!(r, p, "{what}");
        assert_eq!(cpu.mem.rd32(CURSOR), cell, "{what}");
        assert_eq!(
            *cues.borrow(),
            nav.sfx_cue().map(u32::from).into_iter().collect::<Vec<_>>(),
            "{what}"
        );
        moved += u32::from(p == 3);
    }
    assert!(moved > 3000, "{moved}");
}
