//! The party cast trigger `FUN_801DBF9C` against
//! `World::stage_spell_anim_trigger`: every spell id on every party seat,
//! over the anim-pair lists PROT 0898 carries (`0x801F4E63` / `0x801F4EDC`).
//! Compared: the whole action parameter stream `+0x1DF..+0x1F2` over
//! garbage: the pairs copied from `+0x1E0`, the terminator behind them, the
//! summon arm's three bytes, and every byte neither arm names.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_action::ACTION_PARAM_BYTES;
use legaia_engine_core::world::World;
use legaia_game_tables::spell_anim_pairs::SpellAnimPairs;
use legaia_save::Party;
use psx_retail_exec::Stop;

const TRIGGER: u32 = 0x801D_BF9C;
const OVERLAY_BASE: u32 = 0x801C_E818;
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8012_0000;

#[test]
fn the_cast_trigger_stages_what_retail_stages() {
    differential!(prot(898, TRIGGER));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let overlay = disc.read_entry(898).expect("PROT 0898");
    cpu.mem.load(OVERLAY_BASE, &overlay);
    for k in 0..8u32 {
        cpu.mem.wr32(ACTORS + 4 * k, ACTOR + k * 0x400);
    }
    let pairs = SpellAnimPairs::parse(&overlay, OVERLAY_BASE);
    assert!(!pairs.is_empty());
    let mut rng = Rng(0x5350_4C54);
    let (mut listed, mut summons, mut longest) = (0, 0, 0);
    for round in 0..8 {
        for id in 0..=0xFFu32 {
            let slot = rng.below(3);
            let seed: [u8; ACTION_PARAM_BYTES] = std::array::from_fn(|_| rng.next() as u8);
            cpu.mem.load(ACTOR + slot * 0x400 + 0x1DF, &seed);
            assert_eq!(cpu.call(TRIGGER, &[slot, id]), Stop::Returned);
            let retail = cpu
                .mem
                .read(ACTOR + slot * 0x400 + 0x1DF, ACTION_PARAM_BYTES);

            let mut w = World::new();
            w.load_party(Party::zeroed(3));
            w.enter_battle(3, 2);
            w.battle.spell_anim_pairs = pairs.clone();
            w.actors[slot as usize].battle.params = seed;
            w.stage_spell_anim_trigger(slot as u8, id as u8);
            let port = w.actors[slot as usize].battle.params;
            assert_eq!(
                retail,
                port,
                "round {round} id {id:#x} slot {slot}: {:?}",
                crate::common::diff_offsets(&retail, &port)
            );
            if id < 0x25 {
                let n = pairs.pairs(id as u8).map_or(0, <[_]>::len);
                longest = longest.max(n);
                listed += usize::from(n > 0);
            } else {
                summons += 1;
            }
        }
    }
    assert!(
        listed > 100 && summons > 1000 && longest >= 2,
        "{listed} {summons} {longest}"
    );
}
