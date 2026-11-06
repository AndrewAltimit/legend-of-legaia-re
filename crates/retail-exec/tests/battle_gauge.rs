//! The party gauge colours of the battle frame `FUN_80046A20`
//! (`0x80046A80..0x80046D28`) against the HUD rows both hosts draw from
//! (`sync_battle_hud_rows` + `BattleHud::sync_status`, then
//! `BattleSlotHud::gauge_fill_indices`): readout HP at zero and at each band
//! edge, MP either side of its quarter and half, and a status word split
//! between the port's raw half and its tracker.

use crate::common::{Rng, scus_cpu};
use legaia_engine_battle::battle_hud_state::BattleHud;
use legaia_engine_battle_vm::status_effects::StatusKind;
use legaia_engine_core::world::World;
use legaia_retail_exec::Stop;
use legaia_save::Party;

const FRAME: u32 = 0x8004_6A20;
/// The `jal` past the gauge loop.
const LOOP_END: u32 = 0x8004_6D2C;
const GAUGES: u32 = 0x801C_8FA0;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8013_0000;
const ACTOR_STRIDE: u32 = 0x400;
const CELLS: u32 = 0x8014_0000;
/// `gp+0xA49`: the actor gauge `0` shows.
const SHOWN: u32 = 0x8007_BD61;

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
fn the_party_gauges_take_retail_colours() {
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.hook(LOOP_END, |_, _| {});
    for g in 0..4u32 {
        for c in 0..4u32 {
            cpu.mem
                .wr32(GAUGES + g * 16 + c * 4, CELLS + (g * 4 + c) * 2);
        }
    }
    for s in 0..3 {
        cpu.mem.wr32(ACTORS + s * 4, ACTOR + s * ACTOR_STRIDE);
    }
    let mut rng = Rng(0x4741_5547);
    let (mut grey, mut raw_only) = (0, 0);
    for round in 0..20000 {
        let mut w = World::new();
        w.load_party(Party::zeroed(3));
        w.enter_battle(3, 2);
        w.party.party_count = 3;
        w.tables.character_max_mp = vec![0; 3];
        let mut words = [0u16; 3];
        for s in 0..3u32 {
            let max_hp = [1u16, 4, 7, 100, 999, 9999][rng.below(6) as usize];
            let hp = match rng.below(5) {
                0 => 0,
                1 => max_hp >> 1,
                2 => (max_hp >> 2) + 1,
                3 => max_hp >> 2,
                _ => 1 + rng.below(u32::from(max_hp)) as u16,
            };
            let max_mp = [0u16, 1, 9, 40, 999][rng.below(5) as usize];
            let mp =
                [0, max_mp >> 1, (max_mp >> 1) + 1, max_mp >> 2, max_mp][rng.below(5) as usize];
            let word: u16 = match rng.below(4) {
                0 => 0,
                1 => {
                    [0x1000, 0x0001, 0x0008, 0x0010, 0x0020, 0x0400, 0x0004][rng.below(7) as usize]
                }
                _ => (rng.next() as u16) & 0x1C3F,
            };
            words[s as usize] = word;

            let actor = ACTOR + s * ACTOR_STRIDE;
            cpu.mem.wr16(actor + 0x172, hp);
            cpu.mem.wr16(actor + 0x14E, max_hp);
            cpu.mem.wr16(actor + 0x174, mp);
            cpu.mem.wr16(actor + 0x152, max_mp);
            cpu.mem.wr16(actor + 0x16E, word);

            let a = &mut w.actors[s as usize].battle;
            a.hp = hp;
            a.hp_display = Some(hp);
            a.max_hp = max_hp;
            a.liveness = hp;
            a.mp = mp;
            w.tables.character_max_mp[s as usize] = max_mp;
            // Half the time the tracked kinds sit in the tracker, as a hit's
            // status arm leaves them; the rest stays in the raw half.
            let mut raw = word;
            if rng.below(2) == 0 {
                for (kind, bit) in TRACKED {
                    if word & bit != 0 {
                        w.battle.status_effects.apply(s as u8, kind);
                        raw &= !bit;
                    }
                }
            }
            w.actors[s as usize].battle.field_flags = raw;
            if raw != 0 && raw == word {
                raw_only += 1;
            }
        }
        let shown = rng.below(3);
        cpu.mem.wr8(SHOWN, shown as u8);
        assert_eq!(cpu.call(FRAME, &[]), Stop::Returned, "round {round}");

        // What both hosts run each battle frame (`sync_battle_hud`).
        let mut hud = BattleHud::new();
        legaia_engine_battle_hud::sync_battle_hud_rows(&mut hud, &w);
        for slot in 0..hud.slots.len() as u8 {
            hud.sync_status(slot, &w.battle.status_effects);
        }
        let cell = |cpu: &mut legaia_retail_exec::Cpu, g: u32, c: u32| {
            cpu.mem.rd16(CELLS + (g * 4 + c) * 2) as u8
        };
        for g in 0..4u32 {
            let seat = if g == 0 { shown } else { g - 1 };
            let retail = (cell(&mut cpu, g, 0), cell(&mut cpu, g, 2));
            // Each colour fills two cells: the bar and its readout.
            assert_eq!(retail.0, cell(&mut cpu, g, 1));
            assert_eq!(retail.1, cell(&mut cpu, g, 3));
            let port = hud.slots[seat as usize].gauge_fill_indices();
            assert_eq!(
                retail, port,
                "round {round} gauge {g} seat {seat} word {:#06x}",
                words[seat as usize]
            );
            grey += i32::from(retail.0 == 3);
        }
    }
    assert!(grey > 5000 && raw_only > 3000, "{grey} {raw_only}");
}
