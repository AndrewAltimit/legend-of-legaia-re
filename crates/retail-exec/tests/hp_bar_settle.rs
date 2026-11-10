//! The action SM's HP-bar settle check `FUN_801E7250` against
//! `World::battle_hp_bar_drain_pending`: every party size and formation, the
//! acting seat's target byte `+0x1DD` on a party seat, a monster seat, the
//! all-target `8` or past it, and each seat's live HP `+0x14C` against its
//! readout `+0x172`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_slot::SeatMap;
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::{Stop, V0};

const SETTLE: u32 = 0x801E_7250;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTORS: u32 = 0x801C_9370;

fn actor(seat: u32) -> u32 {
    0x8013_0000 + seat * 0x400
}

#[test]
fn the_bar_settle_check_is_retails() {
    differential!(prot(898, SETTLE));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    for seat in 0..8 {
        cpu.mem.wr32(ACTORS + seat * 4, actor(seat));
    }
    let mut rng = Rng(0x4842_4152);
    let (mut failures, mut pending, mut settled) = (Vec::new(), 0, 0);
    for round in 0..20_000 {
        let party = 1 + rng.below(3) as u8;
        let monsters = 1 + rng.below(5) as u8;
        let seats = SeatMap::new(party);
        let mut w = World::new();
        w.load_party(Party::zeroed(3));
        w.enter_battle(party, monsters);
        let n = party + monsters;
        cpu.mem.wr8(CTX, party);
        let active = rng.below(u32::from(n)) as u8;
        cpu.mem.wr8(CTX + 0x13, seats.retail(active).get());
        w.battle_ctx.active_actor = active;
        for seat in 0..8 {
            cpu.mem.wr16(actor(seat) + 0x14C, 0);
            cpu.mem.wr16(actor(seat) + 0x172, 0);
        }
        for e in 0..n {
            let r = u32::from(seats.retail(e).get());
            let hp = [0u16, 1, 50, 999][rng.below(4) as usize];
            // A third of the bars still moving.
            let shown = if rng.below(3) == 0 {
                [0u16, 1, 50, 999][rng.below(4) as usize]
            } else {
                hp
            };
            cpu.mem.wr16(actor(r) + 0x14C, hp);
            cpu.mem.wr16(actor(r) + 0x172, shown);
            let b = &mut w.actors[usize::from(e)].battle;
            b.hp = hp;
            b.liveness = hp;
            b.hp_display = (shown != hp || rng.below(2) == 0).then_some(shown);
        }
        let target = match rng.below(4) {
            0 => 8u8,
            1 => [9u8, 0x10, 0xFF][rng.below(3) as usize],
            _ => rng.below(u32::from(n)) as u8,
        };
        let retail_target = if target < 8 {
            seats.retail(target).get()
        } else {
            target
        };
        cpu.mem.wr8(
            actor(u32::from(seats.retail(active).get())) + 0x1DD,
            retail_target,
        );
        w.actors[usize::from(active)].battle.active_target = target;
        assert_eq!(cpu.call(SETTLE, &[]), Stop::Returned, "round {round}");
        let retail = cpu.regs.r[V0] != 0;
        let port = w.battle_hp_bar_drain_pending();
        if retail {
            pending += 1;
        } else {
            settled += 1;
        }
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round} party {party} monsters {monsters} active {active} target {target}: retail {retail} port {port}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(pending > 2_000 && settled > 2_000, "{pending} {settled}");
}
