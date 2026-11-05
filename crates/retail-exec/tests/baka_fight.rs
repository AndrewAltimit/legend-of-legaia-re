//! Baka Fighter's duel in PROT 0976: the combat tick `FUN_801D3F44` and the round
//! SM `FUN_801D3468` with its resolver, damage kernel, critical roll and CPU
//! picker, frame by frame in the pool walk's order (player, round SM, opponent:
//! the actor list of `minigame_baka_fighter_retail_fight`), against
//! `BakaFight::frame` on one pad script and one `rand()` stream. The clip records
//! both sides read are the same disc headers; only their timing words are used.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_minigames::baka_fighter::{
    BakaAttack, BakaFight, ClipHeaders, MatchPhase, StrikeState, roster_clip_headers,
};
use legaia_minigame_formats::baka_opponents as bo;
use legaia_retail_exec::{Cpu, Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const OVERLAY_BASE: u32 = 0x801C_E818;
const RAND: u32 = 0x8005_6798;
const BLOCK: u32 = 0x801D_BFBC;
const STRIDE: u32 = 0xA8;
const ACTOR: [u32; 2] = [0x8014_0000, 0x8014_0100];
const SM_ACTOR: u32 = 0x8014_0200;
const SPARE: u32 = 0x8014_0300;
const BANK: [u32; 2] = [0x8015_0000, 0x8016_0000];
const CLIP_BASE: [u32; 2] = [0, 0x400];
const COMBAT: u32 = 0x801D_3F44;
const ROUND_SM: u32 = 0x801D_3468;
const ROSTER: u32 = 0x801D_769C;
const ACTION_TABLES: u32 = 0x801D_B8B8;
const LATCH: u32 = 0x801D_BF50;
const PLAYER_COMMIT: u32 = 0x801D_C124;
const RUMBLE: [u32; 2] = [0x801D_BEA0, 0x801D_BEA4];
const PAD_EDGE: u32 = 0x8007_B874;

fn seat(cpu: &mut Cpu, slot: u32, roster: u32, clips: &ClipHeaders) {
    let a = ACTOR[slot as usize];
    let b = BLOCK + slot * STRIDE;
    let base = CLIP_BASE[slot as usize];
    for o in (0..0xA8).step_by(4) {
        cpu.mem.wr32(b + o, 0);
    }
    for o in (0..0x100).step_by(4) {
        cpu.mem.wr32(a + o, 0);
    }
    let rec = ROSTER + roster * 0x6C;
    let table = cpu.mem.rd32(ACTION_TABLES + roster * 4);
    cpu.mem.wr32(b + 0x8, 0xC80);
    cpu.mem.wr32(b + 0x14, base);
    let damage_mod = cpu.mem.rd32(rec + 0x24);
    cpu.mem.wr32(b + 0x18, damage_mod);
    cpu.mem.wr32(b + 0x90, u32::MAX);
    cpu.mem.wr32(b + 0x94, roster);
    cpu.mem.wr32(b + 0x98, u32::MAX);
    cpu.mem.wr32(b + 0xA4, rec);
    cpu.mem.wr32(a + 0x10, 0x0822_0880);
    cpu.mem.wr32(a + 0x48, table);
    cpu.mem.wr16(a + 0x50, slot as u16);
    cpu.mem.wr16(a + 0x5C, (base + 1) as u16);
    cpu.mem.wr16(a + 0x5E, 0xFFFF);
    // The clip bank: an offset table, then each record's three timing fields.
    let bank = BANK[slot as usize];
    for action in 0..bo::ACTIONS_PER_FIGHTER as u32 {
        let id = action + 1;
        let off = 0x1000 + action * 0x10;
        cpu.mem.wr32(bank + id * 4, off);
        // A ladder fighter's bank stops short of the win flourish no duel plays.
        let Some(h) = clips[action as usize] else {
            continue;
        };
        cpu.mem.wr8(bank + off + 1, u8::from(h.double_step));
        cpu.mem.wr16(bank + off + 2, h.frames);
        cpu.mem.wr8(bank + off + 6, h.n);
    }
    cpu.mem.wr32(0x801D_BFA0 + slot * 4, a);
    cpu.mem.wr32(0x801D_BFAC + slot * 4, a);
}

/// The whole of what the two sides have to agree on, in one comparable row.
#[derive(Debug, PartialEq, Eq, Clone)]
struct Row {
    hp: [i32; 2],
    chosen: [u32; 2],
    strike: [u32; 2],
    landed: [i32; 2],
    locked: [bool; 2],
    combo: [i32; 2],
    wins: [u32; 2],
    down: [bool; 2],
    record: [i32; 2],
    cursor: [i32; 2],
    latch: bool,
    answer: i32,
    rumble: [i32; 2],
    rand: u32,
}

fn retail_row(cpu: &mut Cpu, rand: u32) -> Row {
    let word = |cpu: &mut Cpu, slot: usize, o: u32| cpu.mem.rd32(BLOCK + slot as u32 * STRIDE + o);
    let pair = |cpu: &mut Cpu, o: u32| [word(cpu, 0, o), word(cpu, 1, o)];
    let i32s = |v: [u32; 2]| v.map(|x| x as i32);
    Row {
        hp: i32s(pair(cpu, 0x8)),
        chosen: pair(cpu, 0x24),
        strike: pair(cpu, 0xC),
        landed: i32s(pair(cpu, 0x98)),
        locked: pair(cpu, 0x20).map(|x| x != 0),
        combo: i32s(pair(cpu, 0x30)),
        wins: pair(cpu, 0x34),
        down: pair(cpu, 0x2C).map(|x| x != 0),
        record: [0, 1]
            .map(|s| i32::from(cpu.mem.rd16(ACTOR[s] + 0x5C) as i16) - CLIP_BASE[s] as i32 - 1),
        cursor: [0, 1].map(|s| i32::from(cpu.mem.rd16(ACTOR[s] + 0x68) as i16)),
        latch: cpu.mem.rd32(LATCH) != 0,
        answer: cpu.mem.rd32(PLAYER_COMMIT) as i32,
        rumble: RUMBLE.map(|a| cpu.mem.rd32(a) as i32),
        rand,
    }
}

fn port_row(f: &BakaFight, rand: u32) -> Row {
    let strike = |s: usize| match f.strike_clock(s).state {
        StrikeState::Armed => 0,
        StrikeState::Landed => 1,
        StrikeState::Consumed => 2,
    };
    Row {
        hp: [f.hp(0), f.hp(1)],
        chosen: [0, 1].map(|s| {
            f.chosen(s)
                .map_or(0, |a: BakaAttack| u32::from(a.type_id()))
        }),
        strike: [strike(0), strike(1)],
        landed: [0, 1].map(|s| f.strike_clock(s).landed.map_or(-1, |i| i as i32)),
        locked: [f.locked(0), f.locked(1)],
        combo: [f.combo(0), f.combo(1)],
        wins: [f.round_wins(0), f.round_wins(1)],
        down: [f.knocked_down(0), f.knocked_down(1)],
        record: [0, 1].map(|s| f.motion(s).record as i32),
        cursor: [0, 1].map(|s| f.motion(s).cursor),
        latch: f.rate_divisor() != 8,
        answer: f.player_commit(),
        rumble: f.rumble(),
        rand,
    }
}

#[test]
fn whole_rounds_play_out_as_retail_plays_them() {
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d.read_entry(976).expect("PROT 0976");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    let opponents = bo::parse(&overlay).expect("roster");
    let actions = bo::parse_actions(&overlay).expect("actions");
    let clips = roster_clip_headers(|i| d.read_entry(i).ok());

    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });
    // Draws, the impact pair, the banners, the camera glides, the voice lines.
    for va in [
        0x801D_6BB8,
        0x801D_4DF8,
        0x8005_67A8,
        0x801D_6910,
        0x801D_693C,
        0x801D_6968,
        0x801D_6994,
        0x8002_1248,
        0x8003_D53C,
        0x8001_A068,
    ] {
        cpu.stub(va, 0);
    }
    cpu.stub(0x8002_0DE0, SPARE);
    cpu.stub(0x801D_6E04, SPARE);
    let clean = cpu.mem.clone();
    let gp = cpu.regs.r[legaia_retail_exec::GP];

    let mut rng = Rng(0x4241_4B41);
    let (mut rounds, mut finishers, mut chains, mut crits, mut ko_draws) = (0, 0, 0, 0, 0);
    for scenario in 0..1500 {
        let player = rng.below(3) as usize;
        let opponent = 3 + rng.below(bo::OPPONENT_COUNT as u32 - 3) as usize;
        let seed = rng.next();
        let density = [2, 6, 12, 25, 60][rng.below(5) as usize];

        cpu.mem = clean.clone();
        cpu.regs.r[legaia_retail_exec::GP] = gp;
        seat(&mut cpu, 0, player as u32, &clips[player]);
        seat(&mut cpu, 1, opponent as u32, &clips[opponent]);
        cpu.mem.wr32(0x8007_B888, BANK[0]);
        cpu.mem.wr32(0x8007_B840, BANK[1]);
        cpu.mem.wr32(0x801D_BF44, 100);
        cpu.mem.wr32(0x801D_BF78, 2);
        cpu.mem.wr32(PLAYER_COMMIT, u32::MAX);
        cpu.mem.wr8(0x1F80_0393, 1);
        cpu.mem.wr8(0x1F80_037D, 8);
        state.set(seed);

        let mut f = BakaFight::from_tables(&opponents, &actions, player, opponent, seed)
            .expect("fight")
            .with_roster_clip_headers(clips.clone());

        let mut last_hits = [0, 0];
        for frame in 0..2500 {
            let edge: u16 = if rng.below(density) == 0 {
                [0x80, 0x20, 0x40][rng.below(3) as usize]
            } else {
                0
            };
            cpu.mem.wr32(PAD_EDGE, u32::from(edge));
            for (entry, arg) in [(COMBAT, ACTOR[0]), (ROUND_SM, SM_ACTOR), (COMBAT, ACTOR[1])] {
                let stop = cpu.call(entry, &[arg]);
                assert_eq!(stop, Stop::Returned, "frame {frame} entry {entry:#x}");
            }
            let retail = retail_row(&mut cpu, state.get());

            let outcome = f.frame(edge, 0, 0);
            assert!(!outcome.exit);
            let port = port_row(&f, f.rng_state());
            assert_eq!(
                retail, port,
                "scenario {scenario} player {player} opponent {opponent} density {density} frame {frame} edge {edge:#x}"
            );
            chains +=
                u32::from(retail.combo[0] > 1 || retail.combo[1] > 1 && last_hits != retail.combo);
            last_hits = retail.combo;
            crits += u32::from(cpu.mem.rd32(BLOCK + 0xA0) != 0);
            if retail.down[0] || retail.down[1] {
                finishers += 1;
            }
            if retail.hp == [0, 0] {
                ko_draws += 1;
            }
            if !matches!(f.phase(), MatchPhase::Fighting) {
                rounds += 1;
                break;
            }
        }
    }
    assert!(rounds > 1000, "{rounds}");
    assert!(finishers > 1000, "{finishers}");
    assert!(chains > 0, "{chains}");
    assert!(crits > 0, "{crits}");
    eprintln!(
        "rounds {rounds} finishers {finishers} chains {chains} crits {crits} double KO frames {ko_draws}"
    );
}

/// The round score `FUN_801D2A28` over the disc's two bonus tables, against
/// `baka_round_score`, at every combo and health the HUD can show.
#[test]
fn a_round_win_scores_as_retail_scores() {
    use legaia_engine_minigames::baka_fighter::{BakaScoreTables, baka_round_score};
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = d.read_entry(976).expect("PROT 0976");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    cpu.stub(0x8005_67A8, 0);
    let t = BakaScoreTables::from_overlay(&overlay).expect("tables");
    const COMBO: u32 = 0x801D_BEC8;
    const ROW_COMBO: u32 = 0x801D_BED8;
    const ROW_BONUS: u32 = 0x801D_BEDC;
    const HP: u32 = 0x801D_BFC4;
    for combo in 0..40 {
        for hp in (0..=0xC80).step_by(7).chain([0xC80]) {
            cpu.mem.wr32(COMBO, combo as u32);
            cpu.mem.wr32(HP, hp as u32);
            cpu.mem.wr32(ROW_COMBO, 0);
            cpu.mem.wr32(ROW_BONUS, 0);
            assert_eq!(cpu.call(0x801D_2A28, &[]), Stop::Returned);
            let g = baka_round_score(combo, &t.combo_bonus, hp, &t.health_bonus);
            assert_eq!(
                (cpu.mem.rd32(ROW_COMBO) as i32, cpu.mem.rd32(ROW_BONUS) as i32),
                (g.combo_gain, g.bonus_gain),
                "combo {combo} hp {hp}"
            );
        }
    }
}
