//! The casino slot machine's rule kernels in PROT 0975: the LCG `FUN_801D30CC`,
//! the per-spin feature roll `FUN_801D258C`, the reel stop `FUN_801D2114` with its
//! landing search and the roll to the row `FUN_801D0554`, the win evaluation
//! `FUN_801D13E8`, the bonus-symbol scanner `FUN_801D1AF4`.

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
const SCAN: u32 = 0x801D_1AF4;
const ANTICIPATION: u32 = 0x801D_3CA4;

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

/// The bonus-symbol scanner `FUN_801D1AF4` over random strips with one, two
/// or three reels landed, against `SlotMachine::anticipation_scan`: the
/// latch `DAT_801D3CA4` it writes (`1` punch pair, `2` kick pair, untouched
/// otherwise).
#[test]
fn the_bonus_scanner_latches_what_retail_latches() {
    differential!(prot(975, SCAN));
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, payouts)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut m = SlotMachine::new(payouts, 1, 100);
    let mut rng = Rng(0x5343_414E);
    let mut seen = [0; 3];
    for round in 0..100_000 {
        // Punch `9` and kick `8` among few kinds, so pairs land often.
        let strips = gen_strips(&mut rng, 6, 4);
        let raw: [i32; 3] = std::array::from_fn(|_| rng.below(0x1400) as i32);
        let landed: [bool; 3] = std::array::from_fn(|_| rng.below(3) != 0);
        let seat = SlotSeat {
            strips,
            reel_pos: raw.map(|p| (p + 0x1000).rem_euclid(0x1400)),
            feature_mode: 0,
            bonus_spins: 0,
            net_take: 0,
            rng_state: rng.next(),
            jitter: 0,
            normal_target: 2,
        };
        seed_retail(&mut cpu, &seat, raw);
        for r in 0..3u32 {
            cpu.mem.wr32(LOCKED + r * 4, u32::from(landed[r as usize]));
        }
        cpu.mem.wr32(ANTICIPATION, 0);
        assert_eq!(cpu.call(SCAN, &[]), Stop::Returned);
        let retail = cpu.mem.rd32(ANTICIPATION) as i32;
        m.seat(seat);
        let port = m.scan_anticipation_landed(landed);
        assert_eq!(
            retail, port,
            "round {round} raw {raw:?} landed {landed:?} strips {strips:?}"
        );
        seen[retail as usize] += 1;
    }
    assert!(seen.iter().all(|&n| n > 5000), "{seen:?}");
}

/// The state machine `FUN_801CF0D8` whole, frame by frame from its state `0`
/// on a generated pad script, with its draws stubbed and `rand()` shared,
/// against `SlotMachine::frame`: the state word, the purse, the net take, the
/// feature mode, the reels and the display strip, the tally, both generators,
/// the two latches, the submenu cursor and the cue ring's two slots.
#[test]
fn whole_sessions_play_out_as_retail_plays_them() {
    differential!(prot(975, SM));
    const SM: u32 = 0x801C_F0D8;
    const STATE: u32 = 0x801D_3C84;
    const COINS: u32 = 0x801D_4114;
    const CURSOR: u32 = 0x801D_4110;
    const PAD_EDGE: u32 = 0x8007_B874;
    const RING: [u32; 2] = [0x8007_B6D8, 0x8007_B6DC];
    const ACTOR: u32 = 0x8014_0000;
    use legaia_engine_minigames::slot_machine::{SLOT_RNG_SEED, SlotRow};
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, payouts)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The marquee and HUD, the reels, the lamps, the paylines, the furniture,
    // the picker and the rules pages, the fades and the camera, the display
    // mode, the rect move, the primitive links, the text, the return warp.
    for va in [
        0x801C_FFF0,
        0x801D_0FA8,
        0x801D_0E1C,
        0x801D_3380,
        0x801D_08E4,
        0x801D_13C4,
        0x801D_317C,
        0x801D_2CC0,
        0x801D_2AA4,
        0x801D_30F8,
        0x8002_4EE4,
        0x8005_8068,
        0x8001_72C0,
        0x8005_8490,
        0x8001_DAF8,
        0x8003_D2C4,
        0x8005_9010,
        0x8002_C69C,
        0x8003_6888,
        0x8002_6018,
    ] {
        cpu.stub(va, 0);
    }
    let clean = cpu.mem.clone();
    let mut rng = Rng(0x534D_2020);
    let (mut spins, mut wins, mut quits, mut broke, mut rules) = (0, 0, 0, 0, 0);
    let mut modes = [0u32; 7];
    for scenario in 0..400 {
        cpu.mem = clean.clone();
        let coins = [2, 5, 30, 70, 400, 9_999_990][rng.below(6) as usize];
        let seed = rng.next();
        cpu.mem.wr32(LCG_WORD, SLOT_RNG_SEED);
        cpu.mem.wr32(COINS, coins as u32);
        cpu.mem.wr32(STATE, 0);
        cpu.mem.wr8(0x1F80_0393, 1);
        cpu.mem.wr32(PAD_EDGE, 0);
        state.set(seed);
        assert_eq!(cpu.call(SM, &[ACTOR]), Stop::Returned);
        let mut m = SlotMachine::new(payouts.clone(), SLOT_RNG_SEED, coins);
        m.seat_rand(state.get());
        // Half the sessions open inside a feature mode, or on a net take at
        // the roll's thresholds.
        if rng.below(2) == 0 {
            let mode = [0u8, 1, 2, 3, 6][rng.below(5) as usize];
            let bonus = if mode == 6 {
                1 + rng.below(4) as i32
            } else {
                0
            };
            let net = [0, 990, 1990, 2500][rng.below(4) as usize];
            cpu.mem.wr32(MODE, u32::from(mode));
            cpu.mem.wr32(BONUS_LEFT, bonus as u32);
            cpu.mem.wr32(IN_BONUS, u32::from(mode == 6));
            cpu.mem.wr32(NET_TAKE, net as u32);
            m.seat_mode(mode, bonus, net);
        }
        // The entry whiteout `DAT_801D3794` holds state 1 off the pad.
        let mut frame = 0;
        let mut last_state = 1;
        while frame < 6000 {
            let pool: &[u32] = match last_state {
                1 => &[0, 0, 0x40, 0x80, 0x20, 0x10, 0x100, 0x04, 0x1000],
                2 => &[0, 0, 0, 0, 0x40, 0x80, 0x10],
                3 => &[0, 0, 0, 0x80, 0x40, 0x20, 0xC0, 0xE0, 0x10],
                // The port's own Cross collect has no retail arm.
                4 => &[0, 0, 0x80, 0x20, 0x10, 0x100],
                0x32 => &[0, 0x1000, 0x4000, 0x4000, 0x20, 0x40, 0x04, 0x01],
                _ => &[0, 0, 0x40, 0x04, 0x80, 0x20, 0x10],
            };
            let edge = pool[rng.below(pool.len() as u32) as usize];
            cpu.mem.wr32(PAD_EDGE, edge);
            for a in RING {
                cpu.mem.wr16(a, 0);
            }
            assert_eq!(cpu.call(SM, &[ACTOR]), Stop::Returned, "frame {frame}");
            let raw: [i32; 3] =
                std::array::from_fn(|r| cpu.mem.rd32(REEL_POS + r as u32 * 4) as i32);
            let mut strips = [[0u8; 20]; 3];
            for (r, strip) in strips.iter_mut().enumerate() {
                for (i, cell) in strip.iter_mut().enumerate() {
                    *cell = cpu.mem.rd32(STRIPS + (r as u32 * 20 + i as u32) * 4) as u8;
                }
            }
            let retail = (
                SlotRow {
                    state: cpu.mem.rd32(STATE) as i32,
                    balance: cpu.mem.rd32(COINS) as i32,
                    net_take: cpu.mem.rd32(NET_TAKE) as i32,
                    mode: cpu.mem.rd32(MODE) as u8,
                    bonus_spins: cpu.mem.rd32(BONUS_LEFT) as i32,
                    reel_pos: raw.map(|p| (p + 0x1000).rem_euclid(0x1400)),
                    payout: cpu.mem.rd32(PAYOUT) as i32,
                    line: cpu.mem.rd32(LINE) as i32,
                    lcg: cpu.mem.rd32(LCG_WORD),
                    rand: state.get(),
                    latch: cpu.mem.rd32(SPIN_UP) != 0,
                    anticipation: cpu.mem.rd32(ANTICIPATION) as i32,
                    cursor: cpu.mem.rd32(CURSOR),
                    strips,
                },
                RING.map(|a| cpu.mem.rd16(a) as i16),
            );
            m.frame(edge);
            let sounds = m.take_sounds();
            let ring = [0u8, 2].map(|slot| {
                sounds
                    .ring
                    .iter()
                    .rev()
                    .find(|(s, _)| *s == slot)
                    .map_or(0, |&(_, id)| id)
            });
            let port = (m.frame_row(), ring);
            assert_eq!(
                retail, port,
                "scenario {scenario} coins {coins} frame {frame} edge {edge:#x} from state {last_state}"
            );
            spins += u32::from(last_state == 1 && retail.0.state == 2);
            wins += u32::from(last_state == 3 && retail.0.payout > 0);
            let m6 = usize::from(retail.0.mode.min(6));
            modes[m6] += u32::from(last_state == 3 && retail.0.state == 4);
            rules += u32::from(last_state == 0x32 && retail.0.state == 0x33);
            broke += u32::from(last_state == 1 && retail.0.state == 0x5A);
            last_state = retail.0.state;
            frame += 1;
            if last_state == 100 {
                quits += 1;
                break;
            }
        }
    }
    eprintln!(
        "spins {spins} wins {wins} quits {quits} broke {broke} rules {rules} resolved by mode {modes:?}"
    );
    assert!(spins > 2000 && wins > 100 && quits > 100 && broke > 20 && rules > 20);
    assert!([0, 1, 2, 3, 6].iter().all(|&i| modes[i] > 20), "{modes:?}");
}

/// The payline pass `FUN_801D3380` with its projection wrapper and primitive
/// link hooked, over every winning-line word and generated depths, against
/// `payline_prims` and `payline_ot_depth`: each packet's tag, colour and code,
/// the two endpoints it hands the projection and the bucket it links at.
#[test]
fn the_paylines_are_the_packets_retail_links() {
    differential!(prot(975, PAYLINES));
    const PAYLINES: u32 = 0x801D_3380;
    const PROJECT: u32 = 0x8003_D368;
    const LINK: u32 = 0x8003_D2C4;
    const CTX: u32 = 0x1F80_0314;
    const PACKETS: u32 = 0x8010_0000;
    const OT: u32 = 0x8018_0000;
    use legaia_engine_minigames::slot_machine::{payline_ot_depth, payline_prims};
    use legaia_minigame_formats::minigame_slot_scene::parse_paylines;
    use psx_retail_exec::{A0, A1};
    use std::cell::RefCell;
    let state = Rc::new(Cell::new(0));
    let (Some((mut cpu, _)), Some(d)) = (rig(&state), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d.read_entry(975).expect("PROT 0975");
    let lines = parse_paylines(&overlay).expect("paylines");
    // `(x, y, z)` of each endpoint projected, in call order, and the depths
    // the wrapper answers.
    let seen: Rc<RefCell<Vec<(i16, i16, i16)>>> = Rc::default();
    let depths: Rc<RefCell<Vec<i32>>> = Rc::default();
    let links: Rc<RefCell<Vec<(u32, u32)>>> = Rc::default();
    let (s, dq) = (seen.clone(), depths.clone());
    cpu.hook(PROJECT, move |r, m| {
        let p = r.r[A0];
        s.borrow_mut()
            .push((m.rd16(p) as i16, m.rd16(p + 2) as i16, m.rd16(p + 4) as i16));
        r.r[V0] = dq.borrow_mut().remove(0) as u32;
    });
    let l = links.clone();
    cpu.hook(LINK, move |r, _| l.borrow_mut().push((r.r[A0], r.r[A1])));
    let mut rng = Rng(0x4C49_4E45);
    for round in 0..2000 {
        let winning = rng.below(7) as i32 - 1;
        let shift = rng.below(4);
        let ds: Vec<i32> = (0..lines.len() * 2)
            .map(|_| rng.below(0x4000) as i32 - 0x400)
            .collect();
        *depths.borrow_mut() = ds.clone();
        seen.borrow_mut().clear();
        links.borrow_mut().clear();
        cpu.mem.wr32(CTX + 0x8C, PACKETS);
        cpu.mem.wr32(CTX + 0xE0, OT);
        cpu.mem.wr8(CTX + 0x90, shift as u8);
        cpu.mem.wr32(LINE, winning as u32);
        assert_eq!(cpu.call(PAYLINES, &[]), Stop::Returned);
        let prims = payline_prims(&lines, winning);
        assert_eq!(links.borrow().len(), prims.len(), "round {round}");
        for (i, prim) in prims.iter().enumerate() {
            let packet = PACKETS + i as u32 * 0x10;
            let bucket = payline_ot_depth(ds[i * 2 + 1], shift);
            assert_eq!(
                (
                    links.borrow()[i],
                    cpu.mem.rd32(packet),
                    cpu.mem.rd32(packet + 4),
                    seen.borrow()[i * 2],
                    seen.borrow()[i * 2 + 1],
                ),
                (
                    (OT.wrapping_add((bucket << 2) as u32), packet),
                    0x0300_0000,
                    u32::from_le_bytes([prim.color.0, prim.color.1, prim.color.2, prim.code]),
                    (prim.a.x, prim.a.y, prim.a.z),
                    (prim.b.x, prim.b.y, prim.b.z),
                ),
                "round {round} line {i} winning {winning} shift {shift}"
            );
        }
        assert_eq!(
            cpu.mem.rd32(CTX + 0x8C),
            PACKETS + 0x10 * prims.len() as u32
        );
    }
}
