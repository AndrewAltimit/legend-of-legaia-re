//! The action SM's anim-rate restore `FUN_801E93C8` against
//! `World::restore_battle_anim_rates`: every party size and formation, the
//! acting seat a party member on a plain or an art clip or a monster whose
//! staged record's `+0x87` is up or down, and each seat's `+0x21C` latch and
//! `+0x21D` rate, compared on retail's seven table seats and on a fifth
//! monster's seat `7` beyond them, with the context's `+0x243`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_actor_formats::monster_archive::MonsterAnimation;
use legaia_engine_battle_vm::battle_anim_rate::AnimRate;
use legaia_engine_battle_vm::battle_slot::SeatMap;
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::Stop;

const RESTORE: u32 = 0x801E_93C8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;
/// Monster records (clip pointers from `+0x4C`), monster `k` (seat `3 + k`) at `+k*4`.
const MONSTER_RECORDS: u32 = 0x801C_9348;
const CLIPS: u8 = 6;

fn actor(seat: u32) -> u32 {
    0x8013_0000 + seat * 0x400
}

fn clip(flag: u8) -> Option<MonsterAnimation> {
    let mut script = vec![0u8; 0x88];
    script[0x87] = flag;
    Some(MonsterAnimation {
        variant_track: Default::default(),
        action_id: 0,
        rate: 2,
        attach_key: 0,
        solo_flag: flag,
        impact_class: 0,
        effect_script: script,
        part_count: 1,
        frame_count: 1,
        frames: vec![vec![Default::default()]],
    })
}

#[test]
fn the_rate_restore_is_retails() {
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    // Eight seats in memory: the driver's seven and a fifth monster's seat 7.
    for seat in 0..8 {
        cpu.mem.wr32(ACTORS + seat * 4, actor(seat));
    }
    let mut rng = Rng(0x5241_5445);
    let (mut failures, mut ran, mut held) = (Vec::new(), 0, 0);
    for round in 0..20_000 {
        let party = 1 + rng.below(3) as u8;
        let monsters = 1 + rng.below(5) as u8;
        let seats = SeatMap::new(party);
        let mut w = World::new();
        w.load_party(Party::zeroed(3));
        w.enter_battle(party, monsters);
        let n = party + monsters;
        let active = rng.below(u32::from(n)) as u8;
        let retail_active = u32::from(seats.retail(active).get());
        cpu.mem.wr8(CTX + 0x13, retail_active as u8);
        let latch0 = rng.below(2) as u8;
        cpu.mem.wr8(CTX + 0x243, latch0);
        w.battle_ctx.active_actor = active;
        w.battle_ctx.gauge_rearm_latch = latch0;
        for seat in 0..8 {
            cpu.mem.wr8(actor(seat) + 0x21C, 0);
            cpu.mem.wr8(actor(seat) + 0x21D, 0);
            cpu.mem.wr8(actor(seat) + 0x1D9, 0);
        }
        for e in 0..n {
            let r = u32::from(seats.retail(e).get());
            let latch = [0u8, 1, 2, 0xFF][rng.below(4) as usize];
            let rate = [2u8, 4, 8, 0x10][rng.below(4) as usize];
            let id = if e < party {
                [0x0Cu8, 0x0F, 0x10, 0x20][rng.below(4) as usize]
            } else {
                rng.below(u32::from(CLIPS)) as u8
            };
            cpu.mem.wr8(actor(r) + 0x21C, latch);
            cpu.mem.wr8(actor(r) + 0x21D, rate);
            cpu.mem.wr8(actor(r) + 0x1D9, id);
            let b = &mut w.actors[usize::from(e)].battle;
            b.render_flag = latch;
            b.anim_rate = AnimRate(rate);
            b.current_anim = id;
            if e >= party {
                let k = r - 3;
                let table = 0x8014_0000 + k * 0x100;
                cpu.mem.wr32(MONSTER_RECORDS + k * 4, table);
                let mut clips = Vec::new();
                for c in 0..u32::from(CLIPS) {
                    let flag = [0u8, 0, 1, 0x80][rng.below(4) as usize];
                    let rec = 0x8015_0000 + k * 0x1000 + c * 0x100;
                    cpu.mem.wr32(table + c * 4 + 0x4C, rec);
                    cpu.mem.wr8(rec + 0x87, flag);
                    clips.push(clip(flag));
                }
                w.set_actor_battle_action_clips(usize::from(e), std::sync::Arc::new(clips));
            }
        }
        assert_eq!(cpu.call(RESTORE, &[]), Stop::Returned, "round {round}");
        w.restore_battle_anim_rates();
        let retail_ctx = cpu.mem.rd8(CTX + 0x243);
        if retail_ctx != w.battle_ctx.gauge_rearm_latch && failures.len() < 8 {
            failures.push(format!(
                "round {round} party {party} active {active}: ctx retail {retail_ctx} port {}",
                w.battle_ctx.gauge_rearm_latch
            ));
        }
        if latch0 == 1 {
            if retail_ctx == 0 {
                ran += 1;
            } else {
                held += 1;
            }
        }
        for e in 0..n {
            let r = u32::from(seats.retail(e).get());
            let retail = (cpu.mem.rd8(actor(r) + 0x21C), cpu.mem.rd8(actor(r) + 0x21D));
            let b = &w.actors[usize::from(e)].battle;
            let port = (b.render_flag, b.anim_rate.get());
            if retail != port && failures.len() < 8 {
                failures.push(format!(
                    "round {round} party {party} monsters {monsters} active {active} seat {e} (retail {r}): retail {retail:?} port {port:?}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(ran > 1_000 && held > 1_000, "{ran} {held}");
}
