//! The casino slot machine's rule kernels in PROT 0975: the LCG `FUN_801D30CC`,
//! the per-spin feature roll `FUN_801D258C`, the reel stop `FUN_801D2114` with its
//! landing search and the roll to the row `FUN_801D0554`, the win evaluation
//! `FUN_801D13E8`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::slot_machine::{
    SlotMachine, SlotRng, SlotSeat, feature_roll, land_row,
};
use legaia_engine_system::rand::BiosRand;
use legaia_minigame_formats::minigame_slot_scene::{LANDING_LINE_BY_JITTER, PAYLINE_ROW_OFFSETS};
use legaia_minigame_formats::slot_payout;
use psx_retail_exec::{Cpu, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const RAND: u32 = 0x8005_6798;
const LCG: u32 = 0x801D_30CC;
const LAND: u32 = 0x801D_2440;
const LCG_WORD: u32 = 0x801D_3C80;
const JITTER: u32 = 0x801D_4134;
const NORMAL_TARGET: u32 = 0x801D_3CB8;
const SPIN_UP: u32 = 0x801D_3790;
const MODE: u32 = 0x801D_3CAC;
const NET_TAKE: u32 = 0x801D_3D40;
const BONUS_LEFT: u32 = 0x801D_3CB0;
const IN_BONUS: u32 = 0x801D_3D30;
const REEL_POS: u32 = 0x801D_3CC0;
const STRIPS: u32 = 0x801D_3D50;
const SYMBOL: u32 = 0x801D_3D34;
const PAYOUT: u32 = 0x801D_3D38;
const LINE: u32 = 0x801D_3C8C;
const JUST_ENDED: u32 = 0x801D_3798;
const STOP_TARGET: u32 = 0x801D_3CE0;
const BUDGET: u32 = 0x801D_3CF0;
const SPINNING: u32 = 0x801D_3D00;
const LOCKED: u32 = 0x801D_3D10;
const SPIN_TIMER: u32 = 0x801D_3C90;

fn rig(state: &Rc<Cell<u32>>) -> Option<(Cpu, slot_payout::SlotPayoutTable)> {
    let (mut cpu, disc) = (scus_cpu()?, disc()?);
    let overlay = disc.read_entry(975).expect("PROT 0975");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    // The jackpot's two voice calls.
    cpu.stub(0x8006_53C8, 0);
    cpu.stub(0x8006_5034, 0);
    Some((cpu, slot_payout::parse(&overlay).expect("payouts")))
}

#[test]
fn the_feature_roll_rolls_what_retail_rolls() {
    differential!(prot(975, 0x801D_258C));
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, _)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x534C_4F54);
    let mut entered = 0;
    for round in 0..200_000 {
        let net: i32 = match rng.below(6) {
            0 => 999 + rng.below(3) as i32,
            1 => 1999 + rng.below(3) as i32,
            2 => -(rng.below(500) as i32),
            3 => rng.below(1000) as i32,
            4 => 1000 + rng.below(1000) as i32,
            _ => 2000 + rng.below(4000) as i32,
        };
        let mode = if rng.below(4) == 0 {
            rng.below(7) as u8
        } else {
            0
        };
        let press = rng.below(3) == 0;
        let seed = rng.next();
        cpu.mem.wr32(NET_TAKE, net as u32);
        cpu.mem.wr32(MODE, u32::from(mode));
        cpu.mem.wr32(SPIN_UP, u32::from(press));
        state.set(seed);
        assert_eq!(cpu.call(0x801D_258C, &[]), Stop::Returned);
        let retail = (
            cpu.mem.rd32(JITTER) as i32,
            cpu.mem.rd32(NORMAL_TARGET) as u8,
            cpu.mem.rd32(MODE) as u8,
            state.get(),
        );
        let mut bios = BiosRand::new(seed);
        let roll = feature_roll(&mut bios, net, mode, press);
        let port = (
            roll.jitter,
            roll.normal_target,
            roll.entered_mode.unwrap_or(mode),
            bios.state(),
        );
        assert_eq!(
            retail, port,
            "round {round} net {net} mode {mode} press {press}"
        );
        entered += u32::from(mode == 0 && retail.2 != 0);
    }
    assert!(entered > 100, "{entered}");
}

fn gen_strips(rng: &mut Rng, band: u8, kinds: u32) -> [[u8; 20]; 3] {
    let mut strips = [[0u8; 20]; 3];
    for reel in &mut strips {
        for cell in reel.iter_mut() {
            *cell = band + rng.below(kinds) as u8;
        }
    }
    strips
}

fn seed_retail(cpu: &mut Cpu, seat: &SlotSeat, raw: [i32; 3]) {
    cpu.mem.wr32(MODE, u32::from(seat.feature_mode));
    cpu.mem.wr32(BONUS_LEFT, seat.bonus_spins as u32);
    cpu.mem.wr32(IN_BONUS, u32::from(seat.feature_mode == 6));
    cpu.mem.wr32(NET_TAKE, seat.net_take as u32);
    cpu.mem.wr32(LCG_WORD, seat.rng_state);
    cpu.mem.wr32(JITTER, seat.jitter as u32);
    cpu.mem.wr32(NORMAL_TARGET, u32::from(seat.normal_target));
    cpu.mem.wr32(JUST_ENDED, 0);
    cpu.mem.wr32(SPIN_TIMER, 0xFFFF_FFFF);
    for r in 0..3u32 {
        cpu.mem.wr32(REEL_POS + r * 4, raw[r as usize] as u32);
        cpu.mem.wr32(SPINNING + r * 4, 1);
        cpu.mem.wr32(LOCKED + r * 4, 0);
        for row in 0..20u32 {
            cpu.mem.wr32(
                STRIPS + r * 0x50 + row * 4,
                u32::from(seat.strips[r as usize][row as usize]),
            );
        }
    }
}

#[test]
fn a_spin_pays_what_retail_pays() {
    differential!(prot(975, 0x801D_13E8));
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, payouts)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut m = SlotMachine::new(payouts, 1, 100);
    let mut rng = Rng(0x5041_5953);
    let (mut wins, mut jackpots, mut failures) = (0, 0, Vec::new());
    for round in 0..200_000 {
        let mode = [0u8, 0, 1, 2, 3, 3, 6][rng.below(7) as usize];
        let bonus = mode == 6;
        let strips = if bonus {
            gen_strips(&mut rng, 0x10, 10)
        } else {
            let kinds = 1 + rng.below(10);
            gen_strips(&mut rng, 0, kinds)
        };
        let raw: [i32; 3] = std::array::from_fn(|_| rng.below(20) as i32 * 0x100);
        let seat = SlotSeat {
            strips,
            reel_pos: raw.map(|p| (p + 0x1000).rem_euclid(0x1400)),
            feature_mode: mode,
            bonus_spins: if bonus { 1 + rng.below(3) as i32 } else { 0 },
            net_take: rng.below(3000) as i32,
            rng_state: rng.next(),
            jitter: rng.below(5) as i32,
            normal_target: 2 + rng.below(6) as u8,
        };
        seed_retail(&mut cpu, &seat, raw);
        assert_eq!(cpu.call(0x801D_13E8, &[]), Stop::Returned);
        let symbol = cpu.mem.rd32(SYMBOL) as i32;
        let retail = (
            cpu.mem.rd32(PAYOUT) as i32,
            (cpu.mem.rd32(LINE) as i32, symbol),
            cpu.mem.rd32(MODE) as u8,
            cpu.mem.rd32(BONUS_LEFT) as i32,
            cpu.mem.rd32(NET_TAKE) as i32,
            cpu.mem.rd32(LCG_WORD),
        );
        m.seat(seat);
        let result = m.evaluate_seated();
        // The line and symbol are latches on a spin that paid nothing, and a
        // bonus spin's symbol names no line.
        let port = (
            result.payout,
            if bonus || symbol < 0 {
                (retail.1.0, symbol)
            } else {
                (
                    result.line.map_or(-1, |l| l as i32),
                    result.symbol.map_or(-1, i32::from),
                )
            },
            m.feature_mode(),
            m.bonus_spins(),
            m.net_take(),
            m.rng_state(),
        );
        wins += u32::from(symbol >= 0);
        jackpots += u32::from(retail.2 == 6 && !bonus);
        if retail != port && failures.len() < 6 {
            failures.push(format!(
                "round {round} mode {mode}:\n retail {retail:?}\n port   {port:?}\n raw {raw:?} {:?}",
                seat.strips
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(wins > 1000 && jackpots > 10, "{wins} {jackpots}");
}

#[test]
fn a_reel_stops_where_retail_stops() {
    differential!(prot(975, 0x801D_0554), prot(975, 0x801D_2114));
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, payouts)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut m = SlotMachine::new(payouts, 1, 100);
    let mut rng = Rng(0x5354_4F50);
    let mut failures = Vec::new();
    for round in 0..100_000 {
        let mode = [0u8, 0, 1, 2, 3, 6][rng.below(6) as usize];
        let reel = rng.below(3) as usize;
        let raw: [i32; 3] = std::array::from_fn(|_| rng.below(0x1400) as i32);
        let seat = SlotSeat {
            strips: gen_strips(&mut rng, 0, 10),
            reel_pos: raw.map(|p| (p + 0x1000).rem_euclid(0x1400)),
            feature_mode: mode,
            bonus_spins: 0,
            net_take: 0,
            rng_state: rng.next(),
            jitter: rng.below(5) as i32,
            normal_target: 2 + rng.below(6) as u8,
        };
        seed_retail(&mut cpu, &seat, raw);
        assert_eq!(cpu.call(0x801D_2114, &[reel as u32]), Stop::Returned);
        let target = cpu.mem.rd32(STOP_TARGET + reel as u32 * 4);
        let budget = cpu.mem.rd32(BUDGET + reel as u32 * 4) as i32;
        // The other two reels keep turning in the routine; only this one locks.
        let mut frames = 0;
        while cpu.mem.rd32(LOCKED + reel as u32 * 4) == 0 {
            // The roll runs after the press frame's stop.
            assert_eq!(cpu.call(0x801D_0554, &[]), Stop::Returned);
            frames += 1;
            assert!(frames < 200, "round {round}");
        }
        let row = (cpu.mem.rd32(REEL_POS + reel as u32 * 4) as i32 >> 8) as usize;
        let retail = ((row + 0x10) % 20, cpu.mem.rd32(LCG_WORD));

        m.seat(seat);
        assert!(m.stop_reel(reel));
        let port = (m.payline_row(reel), m.rng_state());
        if retail != port && failures.len() < 6 {
            failures.push(format!(
                "round {round} mode {mode} reel {reel}: raw {:#x} target {target} budget {budget} frames {frames}\n retail {retail:?}\n port   {port:?}",
                raw[reel]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_lcg_steps_as_retail_steps() {
    differential!(prot(975, LCG));
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, _)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4C43_4720);
    for chain in 0..2000 {
        let seed = [0, 1, u32::MAX, 0x3333_3333, 0xFFFF_0000, rng.next()][rng.below(6) as usize];
        cpu.mem.wr32(LCG_WORD, seed);
        let mut port = SlotRng::new(seed);
        for step in 0..50 {
            assert_eq!(cpu.call(LCG, &[]), Stop::Returned);
            let p = port.next_u32();
            assert_eq!(
                cpu.mem.rd32(LCG_WORD),
                p,
                "chain {chain} step {step} seed {seed:#x}"
            );
        }
    }
}

#[test]
fn the_landing_search_lands_where_retail_lands() {
    differential!(prot(975, LAND));
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, _)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4C41_4E44);
    let (mut found, mut missed, mut failures) = (0, 0, Vec::new());
    for round in 0..200_000 {
        let kinds = [3, 6, 10][rng.below(3) as usize];
        let strips = gen_strips(&mut rng, 0, kinds);
        let reel = rng.below(3) as usize;
        let raw = rng.below(0x1400) as i32;
        let jitter = rng.below(5) as i32;
        let depth = [0, 1, 3, 6, 20, 25, rng.below(8) as usize][rng.below(7) as usize];
        let target = rng.below(11) as u8;
        for (r, strip) in strips.iter().enumerate() {
            for (i, &cell) in strip.iter().enumerate() {
                cpu.mem
                    .wr32(STRIPS + (r as u32 * 20 + i as u32) * 4, u32::from(cell));
            }
        }
        cpu.mem.wr32(REEL_POS + reel as u32 * 4, raw as u32);
        cpu.mem.wr32(JITTER, jitter as u32);
        let args = [reel as u32, depth as u32, u32::from(target)];
        assert_eq!(cpu.call(LAND, &args), Stop::Returned);
        let landed = cpu.regs.r[V0] as i32;
        let retail = (landed + 0x10).rem_euclid(20) as usize;

        let line = LANDING_LINE_BY_JITTER[jitter as usize];
        let from_row = ((raw >> 8) + 0x10).rem_euclid(20) as usize;
        let port = land_row(
            &strips[reel],
            from_row,
            depth,
            Some(target),
            PAYLINE_ROW_OFFSETS[line][reel],
        );
        let hit =
            (1..=depth.min(20)).any(|t| strips[reel][((raw >> 8) as usize + t) % 20] == target);
        found += u32::from(hit);
        missed += u32::from(!hit);
        if retail != port && failures.len() < 6 {
            failures.push(format!(
                "round {round} reel {reel} raw {raw:#x} jitter {jitter} depth {depth} target \
                 {target}: retail {landed} ({retail}) port {port} strip {:?}",
                strips[reel]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(found > 50_000 && missed > 50_000, "{found} {missed}");
}
