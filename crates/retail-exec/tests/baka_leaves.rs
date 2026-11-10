//! Baka Fighter's leaves in PROT 0976 outside the duel's frame: the end-of-match
//! tally `FUN_801D239C` with its drain rate `FUN_801D6710`, and the action
//! table's keyframe lookup `FUN_801D6E5C`, against `BakaTally`,
//! `tally_drain_step` and `keyframe_in_range`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::baka_fighter::{
    BAKA_CUE_CURSOR, BakaTally, keyframe_in_range, tally_drain_step,
};
use psx_retail_exec::{Cpu, Stop, V0};

const OVERLAY_BASE: u32 = 0x801C_E818;
const TALLY: u32 = 0x801D_239C;
const DRAIN: u32 = 0x801D_6710;
const LOOKUP: u32 = 0x801D_6E5C;
const SKIP: u32 = 0x801D_BF00;
/// The four counters in `BakaTally::new` order: the three score rows, then
/// the coin prize.
const COUNTERS: [u32; 4] = [0x801D_BEE0, 0x801D_BED8, 0x801D_BEDC, 0x801D_BEE8];
const FADES: [u32; 4] = [0x801D_BEF4, 0x801D_BEEC, 0x801D_BEF0, 0x801D_BEFC];
const TOTAL: u32 = 0x801D_BEE4;
const GOLD: u32 = 0x8008_4440;
const CUE: u32 = 0x8007_B6D8;
const PAD_EDGE: u32 = 0x8007_B874;
const FRAME_STEP: u32 = 0x1F80_0393;
const ACTION_TABLES: u32 = 0x801D_B8B8;

fn rig() -> Option<Cpu> {
    let (mut cpu, d) = (scus_cpu()?, disc()?);
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(976).expect("PROT 0976"));
    // The HUD quad and the number drawer.
    cpu.stub(0x801D_5ED0, 0);
    cpu.stub(0x801D_6A18, 0);
    Some(cpu)
}

#[test]
fn the_tally_drain_rate_is_retails() {
    differential!(prot(976, DRAIN));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4452_4149);
    for round in 0..20_000 {
        let left = match rng.below(3) {
            0 => rng.below(40) as i32 - 20,
            1 => rng.below(100_000) as i32,
            _ => rng.next() as i32,
        };
        let skip = rng.below(4) == 0;
        cpu.mem.wr32(SKIP, u32::from(skip));
        assert_eq!(cpu.call(DRAIN, &[left as u32]), Stop::Returned);
        assert_eq!(
            cpu.regs.r[V0] as i32,
            tally_drain_step(left, skip),
            "round {round} left {left} skip {skip}"
        );
    }
}

/// Whole tallies, frame by frame, with zero rows mixed in and the fast-forward
/// pressed at random, against `BakaTally::tick`.
#[test]
fn the_end_of_match_tally_drains_as_retail_drains_it() {
    differential!(prot(976, TALLY));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5441_4C4C);
    let (mut done, mut early) = (0, 0);
    for tally in 0..3000 {
        let counters: [i32; 4] = std::array::from_fn(|_| match rng.below(4) {
            0 => 0,
            1 => 1 + rng.below(6) as i32,
            2 => rng.below(2000) as i32,
            _ => rng.below(60_000) as i32,
        });
        let total = rng.below(100_000) as i32;
        let gold = rng.below(1_000_000) as i32;
        let mut port = BakaTally::new(counters).with_total(total);
        for i in 0..4 {
            cpu.mem.wr32(COUNTERS[i], counters[i] as u32);
            cpu.mem.wr32(FADES[i], 0);
        }
        cpu.mem.wr32(TOTAL, total as u32);
        cpu.mem.wr32(GOLD, gold as u32);
        cpu.mem.wr32(SKIP, 0);
        let mut paid = gold;
        let press_rate = [0, 30, 500][rng.below(3) as usize];
        for frame in 0..5000 {
            let dt = 1 + rng.below(3) as i32;
            let press = press_rate > 0 && rng.below(press_rate) == 0;
            cpu.mem.wr8(FRAME_STEP, dt as u8);
            cpu.mem
                .wr32(PAD_EDGE, if press { 0x10 << rng.below(4) } else { 0 });
            cpu.mem.wr16(CUE, 0);
            assert_eq!(cpu.call(TALLY, &[]), Stop::Returned);
            port.tick(dt, press);
            paid += port.take_gold();
            let cues = port.take_cues();
            let c = port.counters();
            let retail = (
                cpu.regs.r[V0] != 0,
                COUNTERS.map(|a| cpu.mem.rd32(a) as i32),
                FADES.map(|a| cpu.mem.rd32(a) as i32),
                cpu.mem.rd32(TOTAL) as i32,
                cpu.mem.rd32(GOLD) as i32,
                cpu.mem.rd16(CUE) != 0,
            );
            let ported = (
                !port.done(),
                c,
                port.fades(),
                port.total(),
                paid,
                cues.contains(&BAKA_CUE_CURSOR),
            );
            assert_eq!(
                retail, ported,
                "tally {tally} frame {frame} counters {counters:?}"
            );
            // A later row's fade running while an earlier row still drains.
            early += u32::from(c[0] != 0 && retail.2[2] != 0);
            if port.done() {
                done += 1;
                break;
            }
        }
    }
    assert!(done > 2900 && early > 0, "{done} {early}");
}

#[test]
fn the_keyframe_lookup_finds_what_retail_finds() {
    differential!(prot(976, LOOKUP));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const TABLE: u32 = 0x8015_0000;
    let mut rng = Rng(0x4B45_5946);
    let (mut hits, mut misses) = (0, 0);
    for round in 0..50_000 {
        let fighter = rng.below(17);
        let action = rng.below(9);
        let count = [0, 1, 2, 5, rng.below(12)][rng.below(5) as usize];
        let frames: Vec<i16> = (0..count)
            .map(|_| match rng.below(4) {
                0 => -(rng.below(40) as i16),
                _ => rng.below(80) as i16,
            })
            .collect();
        cpu.mem.wr32(ACTION_TABLES + fighter * 4, TABLE);
        let rec = TABLE + action * 0x60;
        cpu.mem.wr32(rec + 0x1C, count);
        for (i, &f) in frames.iter().enumerate() {
            cpu.mem.wr16(rec + 0x26 + i as u32 * 8, f as u16);
        }
        let from = rng.below(1600) as i32 - 400;
        let to = if rng.below(8) == 0 {
            from - 1 - rng.below(40) as i32
        } else {
            from + rng.below(200) as i32
        };
        let args = [fighter, action, from as u32, to as u32];
        assert_eq!(cpu.call(LOOKUP, &args), Stop::Returned);
        let retail = cpu.regs.r[V0] as i32;
        let port = keyframe_in_range(&frames, from, to).map_or(-1, |i| i as i32);
        assert_eq!(
            retail, port,
            "round {round} frames {frames:?} from {from} to {to}"
        );
        hits += u32::from(retail >= 0);
        misses += u32::from(retail < 0);
    }
    assert!(hits > 5000 && misses > 5000, "{hits} {misses}");
}

/// The duel's exchange leaves, each called on its own over seated fighter
/// blocks, against the `BakaFight` kernels the whole-round differential
/// (`baka_fight`) only reaches through the round SM: the resolver
/// `FUN_801D3A14`, the CPU picker `FUN_801D487C` and the comeback critical
/// `FUN_801D6660`. Both fighters are real roster records; the `rand()`
/// stream is shared.
mod exchange {
    use super::*;
    use legaia_engine_minigames::baka_fighter::{BakaFight, ExchangeSeat};
    use legaia_minigame_formats::baka_opponents as bo;
    use std::cell::Cell;
    use std::rc::Rc;

    const RESOLVE: u32 = 0x801D_3A14;
    const PICK: u32 = 0x801D_487C;
    const CRIT: u32 = 0x801D_6660;
    const BLOCK: u32 = 0x801D_BFBC;
    const STRIDE: u32 = 0xA8;
    const ROSTER: u32 = 0x801D_769C;
    const SETTLE: u32 = 0x801D_BF54;
    const RAND: u32 = 0x8005_6798;

    struct Rig {
        cpu: Cpu,
        state: Rc<Cell<u32>>,
        opponents: Vec<bo::BakaOpponent>,
        actions: Vec<bo::BakaActionSet>,
    }

    fn rig() -> Option<Rig> {
        let mut cpu = super::rig()?;
        let overlay = disc()?.read_entry(976).expect("PROT 0976");
        let state = Rc::new(Cell::new(0u32));
        let stream = state.clone();
        cpu.hook(RAND, move |r, _| {
            let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
            stream.set(next);
            r.r[V0] = (next >> 16) & 0x7FFF;
        });
        Some(Rig {
            cpu,
            state,
            opponents: bo::parse(&overlay).expect("roster"),
            actions: bo::parse_actions(&overlay).expect("actions"),
        })
    }

    fn seat(cpu: &mut Cpu, slot: u32, roster: usize, s: ExchangeSeat) {
        let b = BLOCK + slot * STRIDE;
        cpu.mem.wr32(b + 0x08, s.hp as u32);
        cpu.mem.wr32(b + 0x24, u32::from(s.chosen));
        cpu.mem.wr32(b + 0x2C, u32::from(s.committed));
        cpu.mem.wr32(b + 0x30, s.combo as u32);
        cpu.mem.wr32(b + 0x88, s.ai_cursor as u32);
        cpu.mem.wr32(b + 0x94, roster as u32);
        cpu.mem.wr32(b + 0xA0, u32::from(s.crit_pending));
        cpu.mem.wr32(b + 0xA4, ROSTER + roster as u32 * 0x6C);
    }

    fn read(cpu: &mut Cpu, slot: u32) -> ExchangeSeat {
        let b = BLOCK + slot * STRIDE;
        ExchangeSeat {
            hp: cpu.mem.rd32(b + 0x08) as i32,
            chosen: cpu.mem.rd32(b + 0x24) as u8,
            committed: cpu.mem.rd32(b + 0x2C) != 0,
            combo: cpu.mem.rd32(b + 0x30) as i32,
            ai_cursor: cpu.mem.rd32(b + 0x88) as usize,
            crit_pending: cpu.mem.rd32(b + 0xA0) != 0,
        }
    }

    fn random_seats(
        rng: &mut Rng,
        opponents: &[bo::BakaOpponent],
        rosters: [usize; 2],
    ) -> [ExchangeSeat; 2] {
        std::array::from_fn(|s| ExchangeSeat {
            hp: [0, 1, 0x27F, 0x280, 0xC80, rng.below(0xC81) as i32][rng.below(6) as usize],
            chosen: [0, 1, 2, 3, 4, rng.below(4) as u8][rng.below(6) as usize],
            committed: rng.below(4) == 0,
            combo: 1 + rng.below(5) as i32,
            ai_cursor: rng.below(opponents[rosters[s]].ai_pattern.len() as u32 + 1) as usize,
            crit_pending: rng.below(4) == 0,
        })
    }

    /// Run `leaf` on both sides from one seated state; `retail` / `port`
    /// return what the leaf answers, the seats are compared after.
    fn each_round(
        seed: u64,
        rounds: u32,
        mut retail: impl FnMut(&mut Cpu, &mut Rng) -> (i32, [usize; 2]),
        mut port: impl FnMut(&mut BakaFight, i32) -> i32,
        mut tally: impl FnMut(i32),
    ) {
        let Some(mut r) = rig() else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        let mut rng = Rng(seed);
        for round in 0..rounds {
            let rosters = [
                rng.below(bo::OPPONENT_COUNT as u32) as usize,
                rng.below(bo::OPPONENT_COUNT as u32) as usize,
            ];
            let seats = random_seats(&mut rng, &r.opponents, rosters);
            let settle = [-5, 0, 1, 2, 30][rng.below(5) as usize];
            let seed = rng.next();
            let mut f = BakaFight::from_tables(&r.opponents, &r.actions, rosters[0], rosters[1], 0)
                .expect("fight");
            f.seat_exchange(seats, settle, seed);
            for s in 0..2u32 {
                seat(&mut r.cpu, s, rosters[s as usize], seats[s as usize]);
            }
            r.cpu.mem.wr32(SETTLE, settle as u32);
            r.state.set(seed);
            let (answer, _) = retail(&mut r.cpu, &mut rng);
            let ported = port(&mut f, answer);
            let after = [read(&mut r.cpu, 0), read(&mut r.cpu, 1)];
            assert_eq!(
                (answer, after, r.cpu.mem.rd32(SETTLE) as i32, r.state.get()),
                (ported, f.exchange_seats(), f.settle_timer(), f.rng_state()),
                "round {round} rosters {rosters:?} seats {seats:?} settle {settle}"
            );
            tally(answer);
        }
    }

    #[test]
    fn the_exchange_resolves_as_retail_resolves_it() {
        differential!(prot(976, RESOLVE));
        let step = Rc::new(Cell::new(1));
        let s = step.clone();
        let mut seen = [0u32; 5];
        // The call site leaves `a0` at `0x801E0000` and `a1` at a debug
        // print's return; neither is a special's `4`.
        each_round(
            0x5245_534F,
            100_000,
            |cpu, rng| {
                s.set(1 + rng.below(3) as i32);
                cpu.mem.wr8(FRAME_STEP, s.get() as u8);
                assert_eq!(cpu.call(RESOLVE, &[0x801E_0000, 0]), Stop::Returned);
                (cpu.regs.r[V0] as i32, [0, 0])
            },
            |f, _| f.resolve_exchange(step.get()),
            |a| seen[(a + 1) as usize] += 1,
        );
        // Open, seat 0, seat 1, (no `2`), draw.
        if seen != [0; 5] {
            assert!([0, 1, 2, 4].iter().all(|&i| seen[i] > 1000), "{seen:?}");
        }
    }

    #[test]
    fn the_cpu_picks_the_attack_retail_picks() {
        differential!(prot(976, PICK));
        let slot = Rc::new(Cell::new(0usize));
        let s = slot.clone();
        let mut kinds = [0u32; 3];
        each_round(
            0x5049_434B,
            100_000,
            |cpu, rng| {
                s.set(rng.below(2) as usize);
                assert_eq!(cpu.call(PICK, &[s.get() as u32]), Stop::Returned);
                (cpu.regs.r[V0] as i32, [0, 0])
            },
            |f, _| i32::from(f.pick_ai_attack(slot.get())) - 1,
            |a| kinds[a as usize] += 1,
        );
        if kinds != [0; 3] {
            assert!(kinds.iter().all(|&n| n > 10_000), "{kinds:?}");
        }
    }

    #[test]
    fn the_comeback_critical_rolls_as_retail_rolls_it() {
        differential!(prot(976, CRIT));
        let slot = Rc::new(Cell::new(0usize));
        let s = slot.clone();
        each_round(
            0x4352_4954,
            100_000,
            |cpu, rng| {
                s.set(rng.below(2) as usize);
                assert_eq!(cpu.call(CRIT, &[s.get() as u32]), Stop::Returned);
                (0, [0, 0])
            },
            |f, _| {
                f.roll_crit(slot.get());
                0
            },
            |_| {},
        );
    }

    /// The damage kernel `FUN_801D3B18` called on its own over seated blocks
    /// and actors, every attack type on either seat, the special's last
    /// sub-keyframe included, against `BakaFight::apply_damage`.
    #[test]
    fn a_blow_lands_as_retail_lands_it() {
        differential!(prot(976, DAMAGE));
        const DAMAGE: u32 = 0x801D_3B18;
        const ACTOR: [u32; 2] = [0x8014_0000, 0x8014_0100];
        const CLIP_BASE: [u32; 2] = [0, 0x400];
        const ACTION_TABLES: u32 = 0x801D_B8B8;
        const PUSH: u32 = 0x801D_BEC0;
        let Some(mut r) = rig() else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        r.cpu.stub(0x8005_67A8, 0);
        let mut rng = Rng(0x424C_4F57);
        let (mut finishers, mut crits, mut specials) = (0, 0, 0);
        for round in 0..100_000 {
            let rosters = [
                rng.below(bo::OPPONENT_COUNT as u32) as usize,
                rng.below(bo::OPPONENT_COUNT as u32) as usize,
            ];
            let mut seats = random_seats(&mut rng, &r.opponents, rosters);
            let loser = rng.below(2) as usize;
            let winner = loser ^ 1;
            // The winner holds an attack: the caller books a blow only for one.
            seats[winner].chosen = 1 + rng.below(4) as u8;
            // A round opens on a streak of 0, where the streak term is `-0x40`.
            seats[loser].combo = rng.below(6) as i32;
            let count = r.actions[rosters[winner]].keyframes[bo::ACTION_SPECIAL];
            let landed = match rng.below(3) {
                0 => None,
                1 => Some((count - 1).max(0) as usize),
                _ => Some(rng.below(count.max(1) as u32) as usize),
            };
            let mut f = BakaFight::from_tables(&r.opponents, &r.actions, rosters[0], rosters[1], 0)
                .expect("fight");
            f.seat_exchange(seats, 0, 0);
            for s in 0..2usize {
                seat(&mut r.cpu, s as u32, rosters[s], seats[s]);
                let b = BLOCK + s as u32 * STRIDE;
                let rec = ROSTER + rosters[s] as u32 * 0x6C;
                let damage_mod = r.cpu.mem.rd32(rec + 0x24);
                let table = r.cpu.mem.rd32(ACTION_TABLES + rosters[s] as u32 * 4);
                r.cpu.mem.wr32(b + 0x0C, 1);
                r.cpu.mem.wr32(b + 0x10, u32::from(seats[s].chosen));
                r.cpu.mem.wr32(b + 0x14, CLIP_BASE[s]);
                r.cpu.mem.wr32(b + 0x18, damage_mod);
                r.cpu.mem.wr32(b + 0x20, 0);
                r.cpu.mem.wr32(b + 0x34, 0);
                r.cpu.mem.wr32(b + 0x38, 0);
                r.cpu.mem.wr32(b + 0x98, u32::MAX);
                for o in (0..0x100).step_by(4) {
                    r.cpu.mem.wr32(ACTOR[s] + o, 0);
                }
                r.cpu.mem.wr32(ACTOR[s] + 0x48, table);
                r.cpu.mem.wr32(0x801D_BFA0 + s as u32 * 4, ACTOR[s]);
                r.cpu.mem.wr32(0x801D_BFAC + s as u32 * 4, ACTOR[s]);
            }
            r.cpu.mem.wr32(
                BLOCK + winner as u32 * STRIDE + 0x98,
                landed.map_or(u32::MAX, |i| i as u32),
            );
            r.cpu.mem.wr32(PUSH, 0);
            assert_eq!(r.cpu.call(DAMAGE, &[loser as u32]), Stop::Returned);
            f.land_blow(loser, landed);

            let word =
                |cpu: &mut Cpu, s: usize, o: u32| cpu.mem.rd32(BLOCK + s as u32 * STRIDE + o);
            let retail = (
                [read(&mut r.cpu, 0), read(&mut r.cpu, 1)],
                [0, 1].map(|s| word(&mut r.cpu, s, 0x34)),
                [0, 1].map(|s| word(&mut r.cpu, s, 0x38)),
                [0, 1].map(|s| word(&mut r.cpu, s, 0x20) != 0),
                word(&mut r.cpu, winner, 0x0C),
                i32::from(r.cpu.mem.rd16(ACTOR[loser] + 0x5C) as i16) - CLIP_BASE[loser] as i32 - 1,
                (
                    i32::from(r.cpu.mem.rd16(ACTOR[0] + 0x14) as i16),
                    r.cpu.mem.rd32(PUSH) as i32,
                ),
                i32::from(r.cpu.mem.rd16(ACTOR[1] + 0x14) as i16),
            );
            let strike = match f.strike_clock(winner).state {
                legaia_engine_minigames::baka_fighter::StrikeState::Armed => 0,
                legaia_engine_minigames::baka_fighter::StrikeState::Landed => 1,
                legaia_engine_minigames::baka_fighter::StrikeState::Consumed => 2,
            };
            let port = (
                f.exchange_seats(),
                [f.round_wins(0), f.round_wins(1)],
                [f.hits_taken(0), f.hits_taken(1)],
                [f.locked(0), f.locked(1)],
                strike,
                f.motion(loser).record as i32,
                f.push_back(),
                f.push_back().0,
            );
            assert_eq!(
                retail, port,
                "round {round} rosters {rosters:?} seats {seats:?} loser {loser} landed {landed:?}"
            );
            finishers += u32::from(retail.1 != [0, 0]);
            crits += u32::from(seats[winner].crit_pending);
            specials += u32::from(seats[winner].chosen == 4 && seats[loser].hp > 0);
        }
        assert!(
            finishers > 1000 && crits > 1000 && specials > 1000,
            "{finishers} {crits} {specials}"
        );
    }
}

/// The HUD's number drawers: the right-aligned 8px field `FUN_801D6A18`, the
/// coin strip `FUN_801D6F44` and the single digit `FUN_801D69E4`, each quad
/// they hand the widget emitter `FUN_801D5ED0` with the glyph column they
/// patch into its descriptor first, against `right_aligned_number_cells`,
/// `coin_digit_cells` and `single_digit_cell`.
mod digits {
    use super::*;
    use legaia_engine_minigames::baka_fighter::{
        DigitCell, coin_digit_cells, right_aligned_number_cells, single_digit_cell,
    };
    use psx_retail_exec::{A1, A2, A3};
    use std::cell::RefCell;
    use std::rc::Rc;

    const FIELD: u32 = 0x801D_6A18;
    const COIN: u32 = 0x801D_6F44;
    const SINGLE: u32 = 0x801D_69E4;
    const EMIT: u32 = 0x801D_5ED0;
    const WIDGETS: u32 = 0x801D_7160;
    const STACK_TOP: u32 = 0x801F_FF00;

    /// `(x, y, widget, brightness, u)` per emitted quad.
    type Quads = Rc<RefCell<Vec<(i32, i32, u32, u32, u8)>>>;

    fn rig() -> Option<(Cpu, Quads)> {
        let (mut cpu, d) = (scus_cpu()?, disc()?);
        cpu.mem
            .load(OVERLAY_BASE, &d.read_entry(976).expect("PROT 0976"));
        let quads: Quads = Rc::default();
        let sink = quads.clone();
        cpu.hook(EMIT, move |r, m| {
            let widget = r.r[A2];
            let u = m.rd8(WIDGETS + widget * 0x14 + 8);
            sink.borrow_mut().push((
                r.r[psx_retail_exec::A0] as i32,
                r.r[A1] as i32,
                widget,
                r.r[A3],
                u,
            ));
        });
        Some((cpu, quads))
    }

    fn expect(cells: &[DigitCell], x: i32, y: i32, b: u32) -> Vec<(i32, i32, u32, u32, u8)> {
        cells
            .iter()
            .map(|c| (x + i32::from(c.x_offset), y, u32::from(c.widget), b, c.u))
            .collect()
    }

    #[test]
    fn the_number_drawers_draw_the_glyphs_retail_draws() {
        differential!(prot(976, FIELD), prot(976, COIN), prot(976, SINGLE));
        let Some((mut cpu, quads)) = rig() else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        let mut rng = Rng(0x4449_4749);
        for round in 0..30_000 {
            let value = match rng.below(4) {
                0 => rng.below(10) as i32,
                1 => rng.below(100_000) as i32,
                2 => rng.below(100_000_000) as i32,
                _ => (rng.next() & 0x7FFF_FFFF) as i32,
            };
            let (x, y, b) = (
                rng.below(320) as i32,
                rng.below(240) as i32,
                rng.below(0x100),
            );
            cpu.mem.wr32(STACK_TOP + 0x10, rng.below(4));
            for (entry, cells) in [
                (FIELD, right_aligned_number_cells(value)),
                (COIN, coin_digit_cells(value)),
            ] {
                quads.borrow_mut().clear();
                let args = [x as u32, y as u32, value as u32, b];
                assert_eq!(cpu.call(entry, &args), Stop::Returned);
                assert_eq!(
                    *quads.borrow(),
                    expect(&cells, x, y, b),
                    "round {round} entry {entry:#x} value {value}"
                );
            }
            let digit = rng.below(10) as u8;
            quads.borrow_mut().clear();
            let args = [x as u32, y as u32, u32::from(digit), b];
            assert_eq!(cpu.call(SINGLE, &args), Stop::Returned);
            assert_eq!(
                *quads.borrow(),
                expect(&[single_digit_cell(digit)], x, y, b),
                "round {round} digit {digit}"
            );
        }
    }
}

/// The HUD quad emitter `FUN_801D5ED0`, decoded off the POLY_GT4 packet it
/// writes, against `hud_widget_quad` over the disc's 51 widget records; and
/// the centre effect spawn `FUN_801D6E04` against `center_effect_spawn`.
mod widgets {
    use super::*;
    use legaia_engine_minigames::baka_fighter::{
        HudWidgetQuad, center_effect_spawn, hud_widget_quad,
    };
    use legaia_minigame_formats::baka_opponents::parse_baka_hud;
    use psx_retail_exec::{A0, A1, A2, A3};
    use std::cell::RefCell;
    use std::rc::Rc;

    const EMIT: u32 = 0x801D_5ED0;
    const SPAWN_CENTRE: u32 = 0x801D_6E04;
    const MIRROR: u32 = 0x801D_BE98;
    const BUCKET: u32 = 0x801D_BEBC;
    const PACKETS: u32 = 0x1F80_03A0;
    const PACKET: u32 = 0x8018_0000;
    const STACK_TOP: u32 = 0x801F_FF00;

    fn decode(cpu: &mut Cpu) -> HudWidgetQuad {
        let m = &mut cpu.mem;
        let xy = |m: &mut psx_retail_exec::Memory, o: u32| {
            (m.rd16(PACKET + o) as i16, m.rd16(PACKET + o + 2) as i16)
        };
        let uv =
            |m: &mut psx_retail_exec::Memory, o: u32| (m.rd8(PACKET + o), m.rd8(PACKET + o + 1));
        let rgb = |m: &mut psx_retail_exec::Memory, o: u32| {
            [
                m.rd8(PACKET + o),
                m.rd8(PACKET + o + 1),
                m.rd8(PACKET + o + 2),
            ]
        };
        let (x0, y0) = xy(m, 0x08);
        let (x1, _) = xy(m, 0x14);
        let (_, y1) = xy(m, 0x20);
        HudWidgetQuad {
            poly_code: m.rd8(PACKET + 0x07),
            x0,
            y0,
            x1,
            y1,
            uv: [uv(m, 0x0C), uv(m, 0x18), uv(m, 0x24), uv(m, 0x30)],
            rgb_top: rgb(m, 0x04),
            rgb_bottom: rgb(m, 0x1C),
            clut: m.rd16(PACKET + 0x0E),
            tpage_attr: m.rd16(PACKET + 0x1A),
        }
    }

    #[test]
    fn a_hud_widget_emits_the_quad_retail_emits() {
        differential!(prot(976, EMIT));
        let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        let overlay = d.read_entry(976).expect("PROT 0976");
        cpu.mem.load(OVERLAY_BASE, &overlay);
        cpu.stub(0x8003_D2C4, 0);
        let hud = parse_baka_hud(&overlay).expect("widgets");
        let mut rng = Rng(0x5749_4447);
        let mut mirrored = 0;
        for round in 0..50_000 {
            let id = rng.below(hud.len() as u32);
            let (x, y) = (rng.below(400) as i16 - 40, rng.below(300) as i16 - 30);
            let brightness = [0, 0x80, 0xFF, 0x100, rng.below(0x200) as i32][rng.below(5) as usize];
            let size = [0x1000, 0x800, 0x2000, rng.below(0x3000) as i32][rng.below(4) as usize];
            let mirror = rng.below(4) == 0;
            cpu.mem.wr32(PACKETS, PACKET);
            cpu.mem.wr32(MIRROR, u32::from(mirror));
            cpu.mem.wr32(BUCKET, rng.below(8));
            cpu.mem.wr32(STACK_TOP + 0x10, size as u32);
            let args = [x as i32 as u32, y as i32 as u32, id, brightness as u32];
            assert_eq!(cpu.call(EMIT, &args), Stop::Returned);
            let retail = decode(&mut cpu);
            let port = hud_widget_quad(&hud[id as usize], x, y, brightness, size, mirror);
            assert_eq!(
                retail, port,
                "round {round} widget {id} {:?} x {x} y {y} brightness {brightness} size {size:#x} mirror {mirror}",
                hud[id as usize]
            );
            // The mirror flag is one-shot, and the bucket is left at 3.
            assert_eq!((cpu.mem.rd32(MIRROR), cpu.mem.rd32(BUCKET)), (0, 3));
            assert_eq!(cpu.mem.rd32(PACKETS), PACKET + 0x34);
            mirrored += u32::from(mirror);
        }
        assert!(mirrored > 5000, "{mirrored}");
    }

    #[test]
    fn the_centre_effect_spawns_where_retail_spawns_it() {
        differential!(prot(976, SPAWN_CENTRE));
        let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        cpu.mem
            .load(OVERLAY_BASE, &d.read_entry(976).expect("PROT 0976"));
        const PART: u32 = 0x8015_0000;
        let spawned = Rc::new(RefCell::new(Vec::new()));
        let sink = spawned.clone();
        cpu.hook(0x8002_1B04, move |r, m| {
            let h = |m: &mut psx_retail_exec::Memory, a: u32| m.rd16(a) as i16;
            let (p, q) = (r.r[A0], r.r[A1]);
            sink.borrow_mut().push((
                [h(m, p), h(m, p + 2), h(m, p + 4)],
                [h(m, q), h(m, q + 2), h(m, q + 4)],
                r.r[A2],
                r.r[A3],
            ));
            r.r[V0] = PART;
        });
        let mut rng = Rng(0x4345_4E54);
        for _ in 0..2000 {
            let sprite = rng.next() as u16;
            let descriptor = 0x801D_0000 + rng.below(0x8000);
            spawned.borrow_mut().clear();
            cpu.mem.wr16(PART + 0x50, 0);
            assert_eq!(
                cpu.call(SPAWN_CENTRE, &[u32::from(sprite), descriptor]),
                Stop::Returned
            );
            let p = center_effect_spawn(sprite);
            assert_eq!(
                (spawned.borrow().clone(), cpu.mem.rd16(PART + 0x50)),
                (
                    vec![([p.x, p.y, 0], [0, 0, 0], descriptor, p.scale as u32)],
                    p.sprite_id
                )
            );
        }
    }
}

/// Chrome leaves: the cameo's sprite blit `FUN_801D65F8` (the `MoveImage`
/// rect it builds), the glyph wrapper `FUN_801D69A8`, and the developer
/// editor's animation-slot installer `FUN_801D57BC` and remover
/// `FUN_801D58E0` over the action records' slot blocks.
mod chrome {
    use super::*;
    use legaia_engine_minigames::baka_fighter_chrome::{
        anim_slot_delete, anim_slot_install, glyph_draw, glyph_u, sprite_blit,
    };
    use psx_retail_exec::{A0, A1, A2, A3};
    use std::cell::RefCell;
    use std::rc::Rc;

    const BLIT: u32 = 0x801D_65F8;
    const GLYPH: u32 = 0x801D_69A8;
    const INSTALL: u32 = 0x801D_57BC;
    const REMOVE: u32 = 0x801D_58E0;
    const BLIT_TABLE: u32 = 0x801D_BE84;
    const STACK_TOP: u32 = 0x801F_FF00;

    #[test]
    fn the_cameo_blit_moves_the_rect_retail_moves() {
        differential!(prot(976, BLIT));
        let Some(mut cpu) = super::rig() else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        let moved = Rc::new(RefCell::new(Vec::new()));
        let sink = moved.clone();
        cpu.hook(0x8005_8490, move |r, m| {
            let rect = r.r[A0];
            sink.borrow_mut().push((
                [0, 2, 4, 6].map(|o| m.rd16(rect + o) as i16),
                r.r[A1] as i32,
                r.r[A2] as i32,
            ));
        });
        let mut rng = Rng(0x424C_4954);
        for round in 0..5000 {
            let index = rng.below(8);
            let entry = [rng.next() as u8, rng.next() as u8];
            cpu.mem.wr8(BLIT_TABLE + index * 4, entry[0]);
            cpu.mem.wr8(BLIT_TABLE + index * 4 + 1, entry[1]);
            moved.borrow_mut().clear();
            assert_eq!(cpu.call(BLIT, &[0, index]), Stop::Returned);
            let b = sprite_blit(0, entry).expect("mode 0");
            assert_eq!(
                *moved.borrow(),
                vec![([b.src_x, b.src_y, b.size.0, b.size.1], b.dst_x, b.dst_y)],
                "round {round}"
            );
        }
    }

    #[test]
    fn the_glyph_wrapper_pages_and_draws_as_retail_does() {
        differential!(prot(976, GLYPH));
        let Some(mut cpu) = super::rig() else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        use legaia_engine_minigames::baka_fighter_chrome::GLYPH_U_VA;
        let drawn = Rc::new(RefCell::new(Vec::new()));
        let sink = drawn.clone();
        cpu.hook(0x801D_5ED0, move |r, m| {
            sink.borrow_mut().push((
                r.r[A0] as i16,
                r.r[A1] as i16,
                r.r[A2] as u8,
                r.r[A3] as i32,
                m.rd32(r.r[psx_retail_exec::SP] + 0x10) as i32,
                m.rd8(GLYPH_U_VA),
            ));
        });
        let mut rng = Rng(0x474C_5950);
        for round in 0..5000 {
            let (x, y) = (rng.below(320) as i16, rng.below(240) as i16);
            let index = rng.below(40) as i32 - 5;
            let (b, size) = (rng.below(0x100) as i32, rng.below(0x2000) as i32);
            cpu.mem.wr32(STACK_TOP + 0x10, size as u32);
            drawn.borrow_mut().clear();
            let args = [x as u32, y as u32, index as u32, b as u32];
            assert_eq!(cpu.call(GLYPH, &args), Stop::Returned);
            let d = glyph_draw(x, y, index, b, size);
            assert_eq!(
                *drawn.borrow(),
                vec![(d.x, d.y, d.widget, d.brightness, d.size, glyph_u(index))],
                "round {round}"
            );
        }
    }

    #[test]
    fn the_editor_slots_install_and_remove_as_retail_does() {
        differential!(prot(976, INSTALL), prot(976, REMOVE));
        let Some(mut cpu) = super::rig() else {
            eprintln!("[skip] LEGAIA_DISC_BIN unset");
            return;
        };
        cpu.stub(0x8005_67A8, 0);
        const TABLE: u32 = 0x8015_0000;
        let mut rng = Rng(0x534C_4F54);
        let (mut appended, mut reused, mut removed) = (0, 0, 0);
        for round in 0..20_000 {
            let fighter = rng.below(17);
            let action = rng.below(9);
            cpu.mem.wr32(ACTION_TABLES + fighter * 4, TABLE);
            let rec = TABLE + action * 0x60;
            let count = rng.below(9) as usize;
            let mut keys: Vec<i16> = (0..count).map(|_| rng.below(24) as i16 - 4).collect();
            cpu.mem.wr32(rec + 0x1C, count as u32);
            for (i, &k) in keys.iter().enumerate() {
                let s = rec + 0x20 + i as u32 * 8;
                cpu.mem.wr16(s, rng.next() as u16);
                cpu.mem.wr16(s + 2, rng.next() as u16);
                cpu.mem.wr16(s + 4, rng.next() as u16);
                cpu.mem.wr16(s + 6, k as u16);
            }
            let key_raw = rng.below(24 * 16) as i32 - 4 * 16 - rng.below(16) as i32;
            let remove = rng.below(2) == 1;
            let read_keys = |cpu: &mut Cpu| {
                let n = cpu.mem.rd32(rec + 0x1C) as usize;
                (0..n.min(8))
                    .map(|i| cpu.mem.rd16(rec + 0x26 + i as u32 * 8) as i16)
                    .collect::<Vec<_>>()
            };
            if remove {
                // The remover takes the table in `t1` and the key in `a3`.
                cpu.regs.r[9] = TABLE;
                assert_eq!(
                    cpu.call(REMOVE, &[0, action, 0, key_raw as u32]),
                    Stop::Returned
                );
                let hit = cpu.regs.r[V0] as i32 != -1;
                let d = anim_slot_delete(&mut keys, key_raw);
                assert_eq!(
                    (read_keys(&mut cpu), hit),
                    (keys.clone(), d.slot.is_some()),
                    "round {round} remove {key_raw}"
                );
                removed += u32::from(hit);
            } else {
                assert_eq!(
                    cpu.call(INSTALL, &[fighter, action, key_raw as u32]),
                    Stop::Returned
                );
                let i = anim_slot_install(&mut keys, key_raw);
                let retail = read_keys(&mut cpu);
                assert_eq!(retail, keys, "round {round} install {key_raw}");
                if !i.full {
                    // The landed slot's three accumulators are zeroed.
                    let s = rec + 0x20 + i.slot as u32 * 8;
                    assert_eq!([0, 2, 4].map(|o| cpu.mem.rd16(s + o)), [0; 3]);
                }
                appended += u32::from(!i.full && !i.reused);
                reused += u32::from(i.reused);
            }
        }
        assert!(
            appended > 1000 && reused > 1000 && removed > 1000,
            "{appended} {reused} {removed}"
        );
    }
}

/// The per-fighter mesh installer `FUN_801D4C50` on its retail arm (the
/// streaming flag `_DAT_8007B8C2` set), over generated packs: the PROT entry it
/// reads (`FUN_8003EB98`'s first argument) against `fighter_pack_entry`, and
/// every chunk it hands the dispatcher `FUN_8001F05C` (payload address, header
/// word, the decompressed flag) against `fighter_pack_chunks`. The allocator
/// returns the scratch pack and the CD calls are stood in for.
#[test]
fn the_fighter_pack_walk_matches_retail() {
    use legaia_engine_minigames::baka_fighter::{fighter_pack_chunks, fighter_pack_entry};
    use psx_retail_exec::{A1, A2, A3};
    use std::cell::RefCell;
    use std::rc::Rc;
    const INSTALL: u32 = 0x801D_4C50;
    const ALLOC: u32 = 0x8001_7888;
    const FREE: u32 = 0x8001_7B94;
    const READ_ENTRY: u32 = 0x8003_EB98;
    const SYNC: u32 = 0x8003_DE7C;
    const DISPATCH: u32 = 0x8001_F05C;
    const STREAMING: u32 = 0x8007_B8C2;
    const MESH_INDEX: u32 = 0x801D_C114;
    const BUF: u32 = 0x8014_0000;
    differential!(prot(976, INSTALL));
    let Some(mut cpu) = rig() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(0x8005_8104, 0);
    cpu.stub(FREE, 0);
    cpu.stub(SYNC, 0);
    cpu.stub(ALLOC, BUF);
    let reads = Rc::new(RefCell::new(Vec::<(u32, u32, u32)>::new()));
    let r2 = reads.clone();
    cpu.hook(READ_ENTRY, move |r, _| {
        r2.borrow_mut()
            .push((r.r[psx_retail_exec::A0], r.r[A1], r.r[A2]))
    });
    let chunks = Rc::new(RefCell::new(Vec::<(u32, u32, u32, u32)>::new()));
    let c2 = chunks.clone();
    cpu.hook(DISPATCH, move |r, _| {
        c2.borrow_mut()
            .push((r.r[psx_retail_exec::A0], r.r[A1], r.r[A2], r.r[A3]))
    });
    cpu.mem.wr16(STREAMING, 1);
    cpu.mem.wr32(MESH_INDEX, 0);
    cpu.mem.wr32(0x8007_C018, 0);
    let mut rng = Rng(0x4655_4B50);
    let mut walked = 0usize;
    for case in 0..2000 {
        let roster = rng.below(6);
        let mut pack = Vec::new();
        for _ in 0..rng.below(12) {
            let span = if rng.below(4) == 0 { 0x40 } else { 0x2000 };
            let size = 1 + rng.below(span);
            let kind = rng.next() & 0xFF;
            pack.extend_from_slice(&((kind << 24) | size).to_le_bytes());
            let body = ((size / 4) * 4) as usize;
            pack.extend((0..body).map(|_| rng.next() as u8));
        }
        // The terminator: a type byte over a zero size.
        pack.extend_from_slice(&((rng.next() & 0xFF) << 24).to_le_bytes());
        cpu.mem.load(BUF, &pack);
        reads.borrow_mut().clear();
        chunks.borrow_mut().clear();
        assert_eq!(cpu.call(INSTALL, &[roster]), Stop::Returned, "case {case}");
        let (_, entry) = fighter_pack_entry(roster as usize);
        assert_eq!(
            *reads.borrow(),
            [(entry, BUF, 1)],
            "case {case}: roster {roster}"
        );
        let want: Vec<_> = fighter_pack_chunks(&pack)
            .iter()
            .map(|c| {
                let hdr = u32::from_le_bytes(pack[c.offset - 4..c.offset].try_into().unwrap());
                assert_eq!((c.kind, c.len as u32), ((hdr >> 24) as u8, hdr & 0xFF_FFFF));
                (BUF + c.offset as u32, hdr, 0, 1)
            })
            .collect();
        assert_eq!(*chunks.borrow(), want, "case {case}");
        walked += want.len();
    }
    assert!(walked > 5000, "only {walked} chunks walked");
}
