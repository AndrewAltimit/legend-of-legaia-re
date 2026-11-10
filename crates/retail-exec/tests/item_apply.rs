//! `FUN_800402F4`, the item applier, on its field path, against
//! `World::use_item`: every item the field menu can use, then the aggregator
//! the menu's use routine runs after it (`FUN_801D8308`).

use crate::common::{Rng, diff_offsets, scus, scus_cpu};
use legaia_engine_battle::accessory_passives::AccessoryPassives;
use legaia_engine_core::world::World;
use legaia_engine_menus::items::ItemCatalog;
use legaia_game_tables::accessory_passive::AccessoryPassiveTable;
use legaia_game_tables::item_effect::ItemEffectTable;
use legaia_save::{CharacterRecord, Party};
use psx_retail_exec::{Stop, V0};

const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const APPLIER: u32 = 0x8004_02F4;
const AGGREGATOR: u32 = 0x8004_2558;
const VALIDATOR: u32 = 0x8003_FB10;
const RAND: u32 = 0x8005_6798;

fn seed(rng: &mut Rng) -> Vec<u8> {
    let mut r = vec![0u8; STRIDE as usize];
    let mut w = |off: usize, v: u32| r[off..off + 2].copy_from_slice(&(v as u16).to_le_bytes());
    let hp = 1 + rng.below(9999);
    let mp = 1 + rng.below(999);
    w(0x11C, hp);
    w(0x11E, mp);
    w(0x120, 100);
    // Down, hurt or whole, so the zero-HP gates and the restore clamps both run.
    w(
        0x106,
        match rng.below(3) {
            0 => 0,
            1 => hp,
            _ => rng.below(hp),
        },
    );
    w(0x10A, rng.below(mp + 1));
    w(0x122, 1 + rng.below(280));
    for off in [0x124, 0x126, 0x128, 0x12A, 0x12C] {
        w(off, 1 + rng.below(1000));
    }
    r
}

#[test]
fn every_field_item_writes_the_records_retail_writes() {
    differential!(scus(VALIDATOR), scus(APPLIER), scus(AGGREGATOR));
    let (Some(mut cpu), Some(scus)) = (scus_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(RAND, 0);
    // The present party: three members, in order.
    cpu.mem.wr32(0x8008_4594, 3);
    cpu.mem.load(0x8008_4598, &[0, 1, 2]);
    let table = ItemEffectTable::from_scus(&scus).expect("item effects");
    let mut world = World::default();
    world.set_item_catalog(ItemCatalog::vanilla());
    world.set_item_effects(table.clone());
    world.set_accessory_passives(AccessoryPassives::from_disc(
        &AccessoryPassiveTable::from_scus(&scus).expect("passives"),
    ));
    let mut rng = Rng(0x4954_454D);
    let mut failures = Vec::new();
    let mut tried = 0;
    let mut per_class = [0u32; 256];
    for id in 0..=255u8 {
        let Some(e) = table.effect(id) else { continue };
        // Kind `2` is a consumable; the `+1` byte of any other kind indexes a
        // different table. The flag is the field menu's.
        if table.kind(id) != 2 || e.flags & 0x02 == 0 {
            continue;
        }
        for round in 0..64 {
            let records: Vec<Vec<u8>> = (0..3).map(|_| seed(&mut rng)).collect();
            let target = rng.below(3);
            for (i, rec) in records.iter().enumerate() {
                cpu.mem.load(RECORDS + i as u32 * STRIDE, rec);
            }
            // A record in play has been through the aggregator.
            assert_eq!(cpu.call(AGGREGATOR, &[]), Stop::Returned);
            // The menu offers a use only when the validator takes it.
            cpu.regs.r[V0] = 0;
            let stop = cpu.call(
                VALIDATOR,
                &[u32::from(e.class), u32::from(e.tier), target, 0],
            );
            assert_eq!(stop, Stop::Returned, "validator, item {id:#04x}");
            if cpu.regs.r[V0] == 0 {
                continue;
            }
            let stop = cpu.call(APPLIER, &[u32::from(e.class), u32::from(e.tier), target, 0]);
            assert_eq!(stop, Stop::Returned, "item {id:#04x} class {}", e.class);
            assert_eq!(cpu.call(AGGREGATOR, &[]), Stop::Returned);

            let mut party = Party::zeroed(3);
            for (i, rec) in records.iter().enumerate() {
                party.members[i] = CharacterRecord::parse(rec).unwrap();
            }
            world.party.roster = party;
            world.party.party_count = 3;
            world.refresh_party_ability_bits();
            // An all-party item's session fans out across the living allies.
            if e.flags & 0x20 != 0 {
                for member in 0..3u8 {
                    if world.party.roster.members[usize::from(member)]
                        .hp_mp_sp()
                        .hp_cur
                        != 0
                    {
                        world.use_item(id, member);
                    }
                }
            } else {
                world.use_item(id, target as u8);
            }
            world.refresh_party_ability_bits();
            tried += 1;
            per_class[usize::from(e.class)] += 1;
            for i in 0..3 {
                let retail = cpu.mem.read(RECORDS + i as u32 * STRIDE, STRIDE as usize);
                let port = world.party.roster.members[i].write();
                let d = diff_offsets(&retail, &port);
                if !d.is_empty() {
                    failures.push(format!(
                        "item {id:#04x} class {} tier {} round {round} target {target} rec {i}: {d:?}",
                        e.class, e.tier
                    ));
                    break;
                }
            }
        }
    }
    for class in [0, 1, 2, 6, 11, 12, 13] {
        assert!(per_class[class] > 20, "class {class}: {}", per_class[class]);
    }
    let by_item: std::collections::BTreeSet<&str> = failures.iter().map(|f| &f[..9]).collect();
    assert!(
        failures.is_empty(),
        "{tried} uses, {} differ over {} items; first per item:\n{}",
        failures.len(),
        by_item.len(),
        by_item
            .iter()
            .map(|p| failures.iter().find(|f| f.starts_with(p)).unwrap().as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
