//! `FUN_80053CB8`, a party actor's battle-load stat block, against
//! `World::seed_party_battle_stats`. Both run over a record the aggregator has
//! just rebuilt, as a battle entry does.

use crate::common::{Rng, scus, scus_cpu};
use legaia_engine_battle::accessory_passives::AccessoryPassives;
use legaia_engine_core::world::World;
use legaia_engine_menus::equipment::equip_modifier_table_from_disc;
use legaia_game_tables::accessory_passive::AccessoryPassiveTable;
use legaia_game_tables::equip_stats::EquipStatTable;
use legaia_save::{CharacterRecord, Party};
use psx_retail_exec::Stop;

const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const AGGREGATOR: u32 = 0x8004_2558;
const SEED: u32 = 0x8005_3CB8;
/// `0x8007BD10 + slot`: the character seated in a battle slot, 1-based.
const SEATS: u32 = 0x8007_BD10;
/// `0x801C9370 + slot*4`: the actor pointer table.
const ACTORS: u32 = 0x801C_9370;
const ACTOR: u32 = 0x8010_0000;
const MODEL: u32 = 0x8011_0000;
const CTX: u32 = 0x8012_0000;

fn seed(rng: &mut Rng, top: u32, gear: &[u8]) -> Vec<u8> {
    let mut r = vec![0u8; STRIDE as usize];
    let mut w = |off: usize, v: u32| r[off..off + 2].copy_from_slice(&(v as u16).to_le_bytes());
    w(0x11C, 1 + rng.below(9999));
    w(0x11E, rng.below(999));
    w(0x120, 100);
    w(0x106, rng.below(9999));
    w(0x10A, rng.below(999));
    w(0x122, 1 + rng.below(280));
    // `top` walks the stats up to the caps, where the clamp order shows.
    for off in [0x124, 0x126, 0x128, 0x12A, 0x12C] {
        w(off, 1 + rng.below(top));
    }
    // The five gear slots hold equipment, as every path to them enforces; the
    // accessory bytes are read by the passive walk alone.
    for slot in 0..5 {
        r[0x196 + slot] = gear[rng.below(gear.len() as u32) as usize];
    }
    for slot in 5..8 {
        r[0x196 + slot] = 0xC0 + rng.below(0x30) as u8;
    }
    r
}

#[test]
fn a_party_actor_opens_a_fight_on_retails_numbers() {
    differential!(scus(AGGREGATOR), scus(SEED));
    let (Some(mut cpu), Some(scus)) = (scus_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut world = World::default();
    world.set_accessory_passives(AccessoryPassives::from_disc(
        &AccessoryPassiveTable::from_scus(&scus).expect("passives"),
    ));
    let table =
        equip_modifier_table_from_disc(&EquipStatTable::from_scus(&scus).expect("equipment"));
    let gear: Vec<u8> = (1..=255u8).filter(|id| table.get(*id).is_some()).collect();
    world.set_equipment_table(table);
    cpu.mem.wr32(ACTORS, ACTOR);
    cpu.mem.wr32(MODEL + 0x50, 0x8013_0000);
    cpu.mem.wr32(0x8007_BD24, CTX);
    cpu.mem.wr8(SEATS, 1);

    let mut rng = Rng(0x5345_4544);
    for round in 0..3000 {
        let top = if round % 2 == 0 { 400 } else { 1000 };
        let raw = seed(&mut rng, top, &gear);
        cpu.mem.load(RECORDS, &raw);
        assert_eq!(cpu.call(AGGREGATOR, &[]), Stop::Returned);
        assert_eq!(cpu.call(SEED, &[MODEL, 0]), Stop::Returned);
        let mut rd = |off: u32| cpu.mem.rd16(ACTOR + off);
        let retail = [
            ("atk", rd(0x15A)),
            ("udf", rd(0x15C)),
            ("ldf", rd(0x160)),
            ("spd", rd(0x164)),
            ("int", rd(0x168)),
            ("agl", rd(0x156)),
        ];

        let mut party = Party::zeroed(1);
        party.members[0] = CharacterRecord::parse(&raw).unwrap();
        world.party.roster = party;
        world.party.party_count = 1;
        world.seed_party_battle_stats();
        let (udf, ldf) = world.battle.defense_split[0].expect("defense split");
        let port = [
            ("atk", world.battle.attack[0]),
            ("udf", udf),
            ("ldf", ldf),
            ("spd", world.battle.speed[0]),
            ("int", world.battle.accuracy[0]),
            ("agl", world.actors[0].battle.agl_base),
        ];
        assert_eq!(
            retail,
            port,
            "round {round}: equip {:02x?}",
            &raw[0x196..0x19E]
        );
    }
}
