//! `FUN_801E9504`, the level-up applier, against `World::apply_battle_xp`, on
//! one `rand()` stream. The comparison runs after the aggregator's rebuild,
//! because the port rebuilds at once and retail on the way back to the field,
//! and it covers the whole record.

use crate::common::{Rng, diff_offsets, disc, scus, scus_cpu};
use legaia_engine_battle::accessory_passives::AccessoryPassives;
use legaia_engine_core::world::World;
use legaia_game_tables::accessory_passive::AccessoryPassiveTable;
use legaia_save::{CharacterRecord, Party};
use psx_retail_exec::{Stop, V0};
use std::cell::Cell;
use std::rc::Rc;

const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const APPLIER: u32 = 0x801E_9504;
const AGGREGATOR: u32 = 0x8004_2558;
const RAND: u32 = 0x8005_6798;
/// PROT 0898's load base (`crates/asset/data/static-overlays.toml`).
const BATTLE_OVERLAY_BASE: u32 = 0x801C_E818;

#[test]
fn a_level_up_grows_the_record_retail_grows() {
    let (Some(mut cpu), Some(scus), Some(disc)) = (scus_cpu(), scus(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.load(
        BATTLE_OVERLAY_BASE,
        &disc.read_entry(898).expect("PROT 0898"),
    );
    // The correction divisors' pointer, constant across the save corpus.
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    let state = Rc::new(Cell::new(0u32));
    let stream = state.clone();
    cpu.hook(RAND, move |r, _| {
        let next = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        stream.set(next);
        r.r[V0] = (next >> 16) & 0x7FFF;
    });

    let mut world = World::default();
    world.install_retail_progression_tables(&scus);
    world.set_accessory_passives(AccessoryPassives::from_disc(
        &AccessoryPassiveTable::from_scus(&scus).expect("passives"),
    ));
    let table: Vec<u32> = world.party.level_up_tracker.xp_table.clone();

    let mut rng = Rng(0x4C56_4C55);
    let mut levelled = 0;
    for round in 0..1500 {
        let who = rng.below(3) as usize;
        let level = 1 + rng.below(98) as u8;
        // Most rounds cross a threshold or several; the stats reach the caps.
        let xp_base = table[level as usize - 1];
        let mut records: Vec<Vec<u8>> = vec![vec![0u8; STRIDE as usize]; 3];
        let r = &mut records[who];
        let mut w = |off: usize, v: u32| r[off..off + 2].copy_from_slice(&(v as u16).to_le_bytes());
        w(0x11C, 1 + rng.below(9999));
        w(0x11E, rng.below(999));
        w(0x120, 100);
        w(0x106, rng.below(9999));
        w(0x10A, rng.below(999));
        w(0x122, 1 + rng.below(280));
        for off in [0x124, 0x126, 0x128, 0x12A, 0x12C] {
            w(off, 1 + rng.below(999));
        }
        r[0x130] = level;
        // The summed reward; `FUN_8004E568` hands a lone survivor three quarters.
        let reward = rng.below(1 + xp_base / 3 + 4000);
        let share = reward - (reward >> 2);
        let start_xp = xp_base.saturating_sub(rng.below(2000));
        r[0..4].copy_from_slice(&start_xp.to_le_bytes());

        // Retail: `FUN_8004E568` adds the share, then calls the applier, then
        // the scene setup reruns the aggregator.
        for (i, rec) in records.iter().enumerate() {
            cpu.mem.load(RECORDS + i as u32 * STRIDE, rec);
        }
        // ... capped there at 9,999,999 (`0x8004F2C4`).
        cpu.mem.wr32(
            RECORDS + who as u32 * STRIDE,
            (start_xp + share).min(9_999_999),
        );
        let seed = rng.next();
        state.set(seed);
        assert_eq!(cpu.call(APPLIER, &[who as u32]), Stop::Returned);
        assert_eq!(cpu.call(AGGREGATOR, &[]), Stop::Returned);
        let retail = cpu.mem.read(RECORDS + who as u32 * STRIDE, STRIDE as usize);

        // The port.
        let mut party = Party::zeroed(3);
        for (i, rec) in records.iter().enumerate() {
            party.members[i] = CharacterRecord::parse(rec).unwrap();
        }
        world.party.roster = party;
        world.party.party_count = 3;
        world.party.level_up_tracker.level = [1; 4].map(|_| level);
        world.party.level_up_tracker.xp[who] = start_xp;
        world.rng_state = seed;
        for (i, a) in world.actors.iter_mut().take(3).enumerate() {
            a.battle.hp = u16::from(i == who);
        }
        world.apply_battle_xp(reward);
        world.refresh_party_ability_bits();
        let port = world.party.roster.members[who].write();

        if retail[0x130] != level {
            levelled += 1;
        }
        let d = diff_offsets(&retail, &port);
        assert!(
            d.is_empty(),
            "round {round}: char {who} level {level} start {start_xp} share {share}: {d:?}"
        );
        assert_eq!(world.rng_state, state.get(), "round {round}: rand draws");
    }
    assert!(levelled > 300, "{levelled} rounds levelled");
}
