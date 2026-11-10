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
