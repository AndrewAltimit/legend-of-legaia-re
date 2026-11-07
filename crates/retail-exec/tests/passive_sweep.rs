//! The party walk of the per-frame actor pass `FUN_8004CE2C`
//! (`0x8004CE6C..0x8004D018`) against `World::tick_battle_passive_sweep`: the
//! Maximum AP pin and the guard accessories' clear, over every guard
//! combination, a status word split between the port's raw half and its
//! tracker, and fallen members.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle_vm::status_effects::StatusKind;
use legaia_engine_core::world::World;
use legaia_save::Party;
use psx_retail_exec::Stop;

const ENTRY: u32 = 0x8004_CE2C;
/// The first instruction past the party walk.
const WALK_END: u32 = 0x8004_D01C;
const CTX_PTR: u32 = 0x8007_BD24;
const CTX: u32 = 0x8012_0000;
const ACTOR_TABLE: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const SEATS: u32 = 0x8007_BD10;
const RECORDS: u32 = 0x8008_4140;
const STRIDE: u32 = 0x414;

/// Tracked kinds and the word bit each packs to, for splitting a word.
const TRACKED: [(StatusKind, u16); 6] = [
    (StatusKind::Venom, 0x0001),
    (StatusKind::Toxic, 0x0002),
    (StatusKind::Stone, 0x0004),
    (StatusKind::Numb, 0x0400),
    (StatusKind::Sleep, 0x0800),
    (StatusKind::Curse, 0x1000),
];

#[test]
fn the_passive_sweep_matches_retail() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.hook(WALK_END, |_, _| {});
    cpu.mem.wr32(CTX_PTR, CTX);
    let mut rng = Rng(0x4755_4152);
    let (mut cleared, mut pinned, mut fallen_cleared) = (0, 0, 0);
    for round in 0..20000 {
        let n = 1 + rng.below(3);
        let mut w = World::new();
        w.load_party(Party::zeroed(3));
        w.enter_battle(3, 2);
        w.party.party_count = n as u8;
        cpu.mem.wr8(CTX, n as u8);
        let mut want = Vec::new();
        for s in 0..n {
            let guards = (rng.next() & 0x1FC0_0000) & [0, 0xFFFF_FFFF][rng.below(2) as usize];
            let word0 = guards | (rng.next() & 0x003F_FFFF);
            let word1 = rng.next() & [0x400, 0, 0xFFFF_FBFF, 0xFFFF_FFFF][rng.below(4) as usize];
            let status = (rng.next() as u16) & [0x1C7F, 0x1FFF, 0xFFFF, 0][rng.below(4) as usize];
            let hp = [0u16, 1, 250][rng.below(3) as usize];
            let gauge = [0u16, 37, 100][rng.below(3) as usize];

            let actor = ACTOR + s * ACTOR_STRIDE;
            cpu.mem.wr32(ACTOR_TABLE + s * 4, actor);
            cpu.mem.wr8(SEATS + s, s as u8 + 1);
            cpu.mem.wr32(RECORDS + s * STRIDE + 0x6BC, word0);
            cpu.mem.wr32(RECORDS + s * STRIDE + 0x6C0, word1);
            cpu.mem.wr16(actor + 0x14C, hp);
            cpu.mem.wr16(actor + 0x16E, status);
            cpu.mem.wr16(actor + 0x170, gauge);

            let mut bits = [0u8; legaia_save::ABILITY_BITS_LEN];
            bits[0..4].copy_from_slice(&word0.to_le_bytes());
            bits[4..8].copy_from_slice(&word1.to_le_bytes());
            w.party.roster.members[s as usize].set_ability_bits(bits);
            let a = &mut w.actors[s as usize].battle;
            a.hp = hp;
            a.liveness = hp;
            a.spirit_gauge = gauge;
            // Half the time the tracked kinds sit in the tracker, as a hit's
            // status arm leaves them; the rest stays in the raw half.
            let mut raw = status;
            if rng.below(2) == 0 {
                for (kind, bit) in TRACKED {
                    if status & bit != 0 {
                        w.battle.status_effects.apply(s as u8, kind);
                        raw &= !bit;
                    }
                }
            }
            w.actors[s as usize].battle.field_flags = raw;
            want.push((status, gauge, hp, word0));
        }

        assert_eq!(cpu.call(ENTRY, &[]), Stop::Returned, "round {round}");
        w.tick_battle_passive_sweep();

        for s in 0..n {
            let actor = ACTOR + s * ACTOR_STRIDE;
            let retail = (cpu.mem.rd16(actor + 0x16E), cpu.mem.rd16(actor + 0x170));
            let port = (
                w.raw_status_word(s as u8),
                w.actors[s as usize].battle.spirit_gauge,
            );
            let (status, gauge, hp, word0) = want[s as usize];
            assert_eq!(
                retail, port,
                "round {round} seat {s}: status {status:#06x} gauge {gauge} hp {hp} word0 {word0:#x}"
            );
            if retail.0 != status {
                cleared += 1;
                fallen_cleared += i32::from(hp == 0);
            }
            pinned += i32::from(retail.1 == 100 && gauge != 100);
        }
    }
    assert!(
        cleared > 3000 && pinned > 1000 && fallen_cleared > 500,
        "{cleared} {pinned} {fallen_cleared}"
    );
}
