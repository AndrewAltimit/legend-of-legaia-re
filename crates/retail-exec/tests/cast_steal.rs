//! PROT 0941's Steal (`FUN_801F730C`, action `0x51`) run through its phase-1
//! arm, the walk's arrival test answered at once, against the port's
//! resolution: `steal_party_draw` and the bag's consume on a party victim, the
//! steal table's roll on a monster one, the thief's two cells
//! `0x801C8FE0 + (seat - 3) * 4` / `(seat + 5) * 4`, the `rand()` draws spent,
//! and the message bar's text `steal_caption`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_cast::cast_arm_ticks::{StealOutcome, steal_caption, steal_party_draw};
use legaia_engine_menus::item_bag::ItemBag;
use legaia_save::retail_inventory::ItemWindow;
use psx_retail_exec::{GP, Stop};
use std::cell::Cell;
use std::rc::Rc;

const TICK: u32 = 0x801F_730C;
const MODULE_BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const RAND: u32 = 0x8005_6798;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const SEATS: u32 = 0x8007_BD10;
const MONSTER_IDS: u32 = 0x8007_BD0C;
const BAG: u32 = 0x8008_5958;
const ITEMS: u32 = 0x8007_4368;
const STEAL_TABLE: u32 = 0x8007_7828;
const CELLS: u32 = 0x801C_8FE0;
const CAPTION: u32 = 0x8007_74AC;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;
const REC: u32 = 0x8014_0000;

fn lcg(s: u32) -> u32 {
    s.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039)
}

/// A string as the text helpers walk it: a `0xC0..=0xCF` byte takes the next
/// byte with it, zero or not (a name escape such as the hero's).
fn cstr(cpu: &mut psx_retail_exec::Cpu, mut a: u32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = cpu.mem.rd8(a);
        if b == 0 || out.len() > 200 {
            return out;
        }
        out.push(b);
        if b & 0xF0 == 0xC0 {
            out.push(cpu.mem.rd8(a + 1));
            a += 1;
        }
        a += 1;
    }
}

#[test]
fn the_steal_takes_what_retail_takes() {
    differential!(prot(941, TICK));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem
        .load(MODULE_BASE, &disc.read_entry(941).expect("PROT 0941"));
    let gp = cpu.regs.r[GP];
    let stream = Rc::new(Cell::new(0u32));
    let draws = Rc::new(Cell::new(0u32));
    {
        let (stream, draws) = (stream.clone(), draws.clone());
        cpu.hook(RAND, move |r, _| {
            let next = lcg(stream.get());
            stream.set(next);
            draws.set(draws.get() + 1);
            r.r[psx_retail_exec::V0] = (next >> 16) & 0x7FFF;
        });
    }
    // The walk's arrival test: arrived on the loop's own call.
    let arrivals = Rc::new(Cell::new(0u32));
    {
        let arrivals = arrivals.clone();
        cpu.hook(0x8004_E2F0, move |r, _| {
            let n = arrivals.get();
            arrivals.set(n + 1);
            r.r[psx_retail_exec::V0] = u32::from(n % 2 == 1);
        });
    }
    for va in [0x801D_5854, 0x8004_FE5C, 0x801D_8DE8, 0x8005_0E2C] {
        cpu.stub(va, 0);
    }
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(CTX_PTR, CTX);
    for s in 0..8 {
        let a = ACTOR + s * STRIDE;
        cpu.mem.wr32(ACTORS + s * 4, a);
        cpu.mem.wr32(a + 0x22C, REC);
    }
    cpu.mem.wr32(REC + 0x4C, REC + 0x100);
    cpu.mem.wr16(REC + 0x10C, 0x10);

    let names: Vec<String> = (0..256u32)
        .map(|id| {
            let p = cpu.mem.rd32(ITEMS + id * 12 + 4);
            cstr(&mut cpu, p).iter().map(|&b| b as char).collect()
        })
        .collect();
    let mut rng = Rng(0x5354_454C);
    let (mut took, mut empty, mut gated, mut hits, mut misses) = (0, 0, 0, 0, 0);
    for round in 0..6000 {
        // A bag from sparse to full, half of it unpriced quest ids.
        let mut raw = vec![(0u8, 0u8); 256];
        let density = [0u32, 1, 4, 20, 200][rng.below(5) as usize];
        for s in raw.iter_mut() {
            if density > 0 && rng.below(density) == 0 {
                *s = (rng.below(256) as u8, rng.below(3) as u8);
            }
        }
        for (i, (id, n)) in raw.iter().enumerate() {
            cpu.mem.wr8(BAG + i as u32 * 2, *id);
            cpu.mem.wr8(BAG + i as u32 * 2 + 1, *n);
        }
        let window = [ItemWindow::Full, ItemWindow::Low, ItemWindow::High][rng.below(3) as usize];
        let mut bag = ItemBag::from_slots(&raw);
        bag.set_window(window);
        let (ws, we) = bag.window_bounds();
        cpu.mem.wr16(gp + 0x2D2, ws as u16);
        cpu.mem.wr16(gp + 0x2D4, we as u16);
        cpu.mem.wr16(gp + 0x2D6, we as u16);
        let seat1 = [1u8, 2, 3, 4][rng.below(4) as usize];
        cpu.mem.wr8(SEATS + 1, seat1);
        let caster = 3 + rng.below(4) as u8;
        let victim = loop {
            let v = rng.below(7) as u8;
            if v != caster {
                break v;
            }
        };
        let mut ids = [0u8; 4];
        for (i, id) in ids.iter_mut().enumerate() {
            *id = 1 + rng.below(0xD0) as u8;
            cpu.mem.wr8(MONSTER_IDS + i as u32, *id);
        }
        cpu.mem.wr8(CTX + 0x13, caster);
        cpu.mem.wr8(CTX + 0x279, 1);
        cpu.mem
            .wr8(ACTOR + u32::from(caster) * STRIDE + 0x1DD, victim);
        for c in 0..16 {
            cpu.mem.wr32(CELLS + c * 4, 0);
        }
        cpu.mem.wr32(CAPTION, 0);
        let seed = rng.next();
        // A stream whose next draw is zero now and then, for the gate.
        let seed = if rng.below(8) == 0 {
            (0..)
                .map(|k| seed.wrapping_add(k))
                .find(|&s| lcg(s) >> 16 & 0x7FFF == 0)
                .unwrap()
        } else {
            seed
        };
        stream.set(seed);
        draws.set(0);
        arrivals.set(0);
        assert_eq!(cpu.call(TICK, &[]), Stop::Returned, "round {round}");
        let retail_draws = draws.get();
        let retail_caption = {
            let p = cpu.mem.rd32(CAPTION);
            cstr(&mut cpu, p)
        };
        let retail_bag: Vec<(u8, u8)> = (0..256u32)
            .map(|i| (cpu.mem.rd8(BAG + i * 2), cpu.mem.rd8(BAG + i * 2 + 1)))
            .collect();

        // The port, on the same stream.
        let mut s = seed;
        let mut n = 0u32;
        let mut rand = || {
            s = lcg(s);
            n += 1;
            (s >> 16) & 0x7FFF
        };
        let outcome = if victim < 3 {
            let prices: Vec<bool> = (0..256u32)
                .map(|id| cpu.mem.rd16(ITEMS + id * 12 + 2) != 0)
                .collect();
            let floor = (seat1 == 4).then_some(ws as u8);
            match steal_party_draw(bag.slots(), floor, &mut rand, |id| prices[id as usize]) {
                Some(slot) => StealOutcome::FromBag {
                    item: bag.slots()[slot as usize].0,
                },
                None => StealOutcome::BagEmpty,
            }
        } else {
            let id = ids[usize::from(victim - 3)];
            let chance = cpu.mem.rd8(STEAL_TABLE + u32::from(id) * 2);
            let item = cpu.mem.rd8(STEAL_TABLE + u32::from(id) * 2 + 1);
            let roll = (rand() % 100) as u8;
            StealOutcome::FromMonster {
                chance,
                item,
                roll,
                hit: roll < chance,
            }
        };
        let ctx = format!("round {round} caster {caster} victim {victim} {outcome:?}");
        assert_eq!(retail_draws, n, "draws: {ctx}");
        let mut cells = [0u32; 16];
        match outcome {
            StealOutcome::FromBag { item } => {
                bag.consume_returning_slot(item, 1);
                cells[usize::from(caster - 3)] = u32::from(item);
                took += 1;
            }
            StealOutcome::BagEmpty => {
                if n == 1 {
                    gated += 1;
                }
                empty += 1;
            }
            StealOutcome::FromMonster {
                item, hit: true, ..
            } => {
                cells[usize::from(caster - 3)] = u32::from(item);
                cells[usize::from(caster + 5)] = 1;
                hits += 1;
            }
            StealOutcome::FromMonster { .. } => misses += 1,
        }
        assert_eq!(retail_bag, bag.slots(), "bag: {ctx}");
        for (c, want) in cells.iter().enumerate() {
            assert_eq!(cpu.mem.rd32(CELLS + c as u32 * 4), *want, "cell {c}: {ctx}");
        }
        let port_caption = steal_caption(outcome, |id| names[usize::from(id)].clone());
        let port_bytes: Vec<u8> = port_caption.chars().map(|c| c as u8).collect();
        assert_eq!(
            String::from_utf8_lossy(&retail_caption),
            String::from_utf8_lossy(&port_bytes),
            "caption: {ctx}"
        );
        assert_eq!(cpu.mem.rd8(CTX + 0x279), 2, "phase: {ctx}");
    }
    assert!(
        took > 500 && empty > 300 && gated > 50 && hits > 100 && misses > 300,
        "{took} {empty} {gated} {hits} {misses}"
    );
}
