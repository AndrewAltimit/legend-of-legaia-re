//! The field walk-regen tick `FUN_801D0B90` (PROT 0897) against
//! `legaia_engine_field::walk_regen::tick_walk_regen` over the present party's
//! records: the step counter `0x801F2274`, the HP / MP / AP Walk bumps and their
//! clamps, and the Incense window `0x8007B600` with its wear-off edge.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::walk_regen::{WalkGauge, WalkRegenMember, tick_walk_regen};
use legaia_retail_exec::Stop;

const TICK: u32 = 0x801D_0B90;
const OVERLAY_BASE: u32 = 0x801C_E818;
const COUNTER: u32 = 0x801F_2274;
const WINDOW: u32 = 0x8007_B600;
const EVENT_PTR: u32 = 0x8007_B450;
const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;

#[test]
fn a_walk_tick_restores_what_retail_restores() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    // The wear-off registration; the player actor it flags is seeded below.
    cpu.stub(0x8002_0DE0, 0);
    let player = 0x8013_0000u32;
    let mut rng = Rng(0x5741_4C4B);
    let (mut ran, mut bumped, mut clamped, mut edges) = (0, 0, 0, 0);
    for round in 0..4000 {
        let count = 1 + rng.below(3);
        let mut ids: Vec<u32> = (0..4).collect();
        for i in (1..4).rev() {
            ids.swap(i, rng.below(i as u32 + 1) as usize);
        }
        ids.truncate(count as usize);
        cpu.mem.wr8(0x8008_4594, count as u8);
        for (i, id) in ids.iter().enumerate() {
            cpu.mem.wr8(0x8008_4598 + i as u32, *id as u8);
        }
        let mut members = Vec::new();
        for id in 0..4u32 {
            let r = RECORDS + id * STRIDE;
            let bits = [0u32, 0x0100_0000, 0x0200_0000, 0x0400_0000, 0x0700_0000]
                [rng.below(5) as usize]
                | (rng.below(2) * 0x10);
            cpu.mem.wr32(r + 0xF8, bits);
            let mut gauge = |max_off: u32| {
                let max = [0u16, 5, 99, 999, 9999, 0xFFFF][rng.below(6) as usize];
                let cur = match rng.below(4) {
                    0 => max,
                    1 => max.wrapping_sub(rng.below(9) as u16),
                    2 => max.wrapping_add(rng.below(9) as u16),
                    _ => rng.below(u32::from(max) + 1) as u16,
                };
                cpu.mem.wr16(r + max_off, max);
                cpu.mem.wr16(r + max_off + 2, cur);
                WalkGauge {
                    value: cur,
                    cap: max,
                }
            };
            let hp = gauge(0x104);
            let mp = gauge(0x108);
            let ap = gauge(0x10C);
            members.push(WalkRegenMember {
                ability_hi: bits,
                hp,
                mp,
                ap,
            });
        }
        let mut counter = [0i32, 0x1F, 0x20, 0x21, 0x40, 0x7FFF, -5][rng.below(7) as usize];
        let mut window = [0i32, 1, 2, 0x40, -1][rng.below(5) as usize];
        cpu.mem.wr32(COUNTER, counter as u32);
        cpu.mem.wr32(WINDOW, window as u32);
        cpu.mem.wr32(EVENT_PTR, 0);
        cpu.mem.wr32(0x8007_C348 + 0x1C, player);
        cpu.mem.wr32(player + 0x10, 0);
        assert_eq!(cpu.call(TICK, &[]), Stop::Returned, "round {round}");

        // The port: the party's records in list order, the way the World
        // projects them.
        let mut party: Vec<WalkRegenMember> = ids.iter().map(|&id| members[id as usize]).collect();
        let before = party.clone();
        let edge = tick_walk_regen(&mut counter, &mut party, &mut window);
        let ctx = format!("round {round} ids {ids:?}");
        assert_eq!(cpu.mem.rd32(COUNTER) as i32, counter, "counter: {ctx}");
        assert_eq!(cpu.mem.rd32(WINDOW) as i32, window, "window: {ctx}");
        assert_eq!(cpu.mem.rd32(EVENT_PTR) == 0x801F_2278, edge, "edge: {ctx}");
        assert_eq!(
            cpu.mem.rd32(player + 0x10) & 0x8_0000 != 0,
            edge,
            "flag: {ctx}"
        );
        for (i, &id) in ids.iter().enumerate() {
            let r = RECORDS + id * STRIDE;
            let m = party[i];
            let retail = [
                cpu.mem.rd16(r + 0x106),
                cpu.mem.rd16(r + 0x10A),
                cpu.mem.rd16(r + 0x10E),
            ];
            assert_eq!(
                retail,
                [m.hp.value, m.mp.value, m.ap.value],
                "member {id}: {ctx}"
            );
            for (b, a) in [
                (before[i].hp, m.hp),
                (before[i].mp, m.mp),
                (before[i].ap, m.ap),
            ] {
                bumped += u32::from(a.value != b.value);
                clamped += u32::from(a.value == a.cap && b.value != a.cap);
            }
        }
        ran += u32::from(counter != cpu.mem.rd32(COUNTER) as i32 || edge || counter >= 0);
        edges += u32::from(edge);
    }
    assert!(
        ran > 0 && bumped > 500 && clamped > 100 && edges > 20,
        "{ran} {bumped} {clamped} {edges}"
    );
}
