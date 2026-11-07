//! A spell cast from the pause Magic screen. Retail's two commits, the single
//! target `FUN_801D9594` (`0x801D9708..0x801D97B0`) and the group flow
//! `FUN_801D9280` (`0x801D939C..0x801D9444`), debit the caster's MP through the
//! cost kernel `FUN_80035394`, apply the spell record's `(+0, +1)` pair through
//! the shared applier `FUN_800402F4(class, sub, target, caster)` and run the
//! aggregator `FUN_80042558`. The port's side is the Magic sub-session the
//! hosts build, confirmed on the same caster, spell and target, and applied by
//! `apply_spell_outcome`.

use crate::common::{Rng, diff_offsets, scus, scus_cpu};
use legaia_engine_battle::accessory_passives::AccessoryPassives;
use legaia_engine_battle::battle_stats::EquipmentTable;
use legaia_engine_battle::tactical_arts_editor::ChainLibrary;
use legaia_engine_core::world::World;
use legaia_engine_menu_runtime::field_menu_dispatch::{FieldMenuSubsession, apply_spell_outcome};
use legaia_engine_menus::field_menu::FieldMenuRow;
use legaia_engine_menus::items::ItemCatalog;
use legaia_engine_menus::options::OptionsState;
use legaia_engine_menus::save_select::{SaveRack, SlotSnapshot};
use legaia_engine_menus::spell_menu::{SpellMenuInput, SpellMenuPhase};
use legaia_game_tables::accessory_passive::AccessoryPassiveTable;
use legaia_game_tables::item_effect::ItemEffectTable;
use legaia_save::{CharacterRecord, Party};
use psx_retail_exec::{Stop, V0};

const RECORDS: u32 = 0x8008_4708;
const STRIDE: u32 = 0x414;
const APPLIER: u32 = 0x8004_02F4;
const AGGREGATOR: u32 = 0x8004_2558;
const VALIDATOR: u32 = 0x8003_FB10;
const MP_COST: u32 = 0x8003_5394;
const RAND: u32 = 0x8005_6798;
const SPELL_STATS: u32 = 0x8007_54C8;

fn seed(rng: &mut Rng) -> Vec<u8> {
    let mut r = vec![0u8; STRIDE as usize];
    let mut w = |off: usize, v: u32| r[off..off + 2].copy_from_slice(&(v as u16).to_le_bytes());
    let hp = 1 + rng.below(9999);
    let mp = 50 + rng.below(950);
    w(0x11C, hp);
    w(0x11E, mp);
    w(0x120, 100);
    w(
        0x106,
        match rng.below(4) {
            0 => 0,
            1 => hp,
            _ => rng.below(hp),
        },
    );
    w(0x10A, 40 + rng.below(mp - 39));
    w(0x122, 1 + rng.below(280));
    for off in [0x124, 0x126, 0x128, 0x12A, 0x12C] {
        w(off, 1 + rng.below(1000));
    }
    r
}

#[test]
fn a_menu_cast_writes_the_records_retail_writes() {
    let (Some(mut cpu), Some(scus)) = (scus_cpu(), scus()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.stub(RAND, 0);
    cpu.mem.wr32(0x8008_4594, 3);
    cpu.mem.load(0x8008_4598, &[0, 1, 2]);
    let mut world = World::default();
    world.set_item_catalog(ItemCatalog::vanilla());
    world.set_item_effects(ItemEffectTable::from_scus(&scus).expect("item effects"));
    world.set_accessory_passives(AccessoryPassives::from_disc(
        &AccessoryPassiveTable::from_scus(&scus).expect("passives"),
    ));
    world.install_menu_text(&scus);
    world.set_spell_catalog(
        legaia_engine_battle::retail_magic::seru_magic_catalog_from_scus(&scus)
            .expect("spell catalog"),
    );
    world.tables.magic_xp_thresholds = legaia_engine_battle::magic_xp::thresholds_from_scus(&scus);
    assert!(world.tables.magic_xp_thresholds.is_some());
    let mut rng = Rng(0x4D41_4749);
    let mut leveled = 0u32;
    let mut failures = Vec::new();
    let mut cast = 0u32;
    let mut refused = 0u32;
    let mut per_spell = std::collections::BTreeMap::<u8, u32>::new();
    for spell in 0x81..=0xA0u8 {
        let at = SPELL_STATS + u32::from(spell) * 12;
        let (class, sub, cost) = (cpu.mem.rd8(at), cpu.mem.rd8(at + 1), cpu.mem.rd8(at + 3));
        for round in 0..48 {
            let caster = rng.below(3) as u8;
            let target_row = rng.below(3) as u8;
            let level = 1 + rng.below(9) as u8;
            let mut members: Vec<CharacterRecord> = (0..3)
                .map(|_| CharacterRecord::parse(&seed(&mut rng)).unwrap())
                .collect();
            for (i, m) in members.iter_mut().enumerate() {
                // Every member wears its Ra-Seru, which the caster confirm reads.
                let mut equip = m.equipment();
                equip.slots[[3, 2, 3][i]] = [0x01, 0x0A, 0x13][i];
                m.set_equipment(equip);
            }
            let mut list = members[usize::from(caster)].spell_list();
            list.count = 1;
            list.ids[0] = spell;
            list.levels[0] = level;
            members[usize::from(caster)].set_spell_list(list);
            // The slot's XP word sits anywhere up to the top threshold, so a
            // cast can cross one.
            let xp = rng.below(560);
            members[usize::from(caster)].raw[8..12].copy_from_slice(&xp.to_le_bytes());
            for (i, m) in members.iter().enumerate() {
                cpu.mem.load(RECORDS + i as u32 * STRIDE, &m.write());
            }
            assert_eq!(cpu.call(AGGREGATOR, &[]), Stop::Returned);

            // The port: the screen the hosts open, landed on the cast.
            let mut party = Party::zeroed(3);
            for i in 0..3 {
                party.members[i] = CharacterRecord::parse(
                    &cpu.mem.read(RECORDS + i as u32 * STRIDE, STRIDE as usize),
                )
                .unwrap();
            }
            world.party.roster = party;
            world.party.party_count = 3;
            world.party.party_leader_slot = Some(0);
            world.refresh_party_ability_bits();
            let catalog = world.tables.spell_catalog.clone();
            let mut sub_session = FieldMenuSubsession::build(
                FieldMenuRow::Magic,
                &world,
                &OptionsState::default(),
                &SaveRack::Blocks((0..3).map(SlotSnapshot::empty).collect()),
                &ChainLibrary::new(),
                &catalog,
                &EquipmentTable::new(),
            );
            let FieldMenuSubsession::Spells(s) = &mut sub_session else {
                panic!("Magic builds the spell session");
            };
            s.resume_cast(caster, spell, target_row);
            let group = match s.phase() {
                SpellMenuPhase::TargetSelect { .. } => false,
                SpellMenuPhase::GroupConfirm { .. } => true,
                _ => continue,
            };
            s.tick(SpellMenuInput {
                cross: true,
                ..Default::default()
            });
            let port_casts = matches!(s.phase(), SpellMenuPhase::Done(_));
            // Retail's commit. The single-target confirm asks the validator
            // about the picked member first (`0x801D96C0`) and buzzes on a
            // refusal, with nothing written.
            let retail_casts = group || {
                cpu.regs.r[V0] = 0;
                let stop = cpu.call(
                    VALIDATOR,
                    &[u32::from(class), u32::from(sub), u32::from(target_row), 0],
                );
                assert_eq!(stop, Stop::Returned);
                cpu.regs.r[V0] != 0
            };
            if retail_casts != port_casts {
                failures.push(format!(
                    "spell {spell:#04x} round {round} target {target_row}: retail casts \
                     {retail_casts}, port {port_casts}"
                ));
                continue;
            }
            if !retail_casts {
                refused += 1;
                continue;
            }
            apply_spell_outcome(s, &mut world);
            world.refresh_party_ability_bits();
            cpu.regs.r[V0] = 0;
            assert_eq!(
                cpu.call(MP_COST, &[u32::from(caster), u32::from(cost)]),
                Stop::Returned
            );
            let price = cpu.regs.r[V0];
            let mp_at = RECORDS + u32::from(caster) * STRIDE + 0x10A;
            let mp = cpu.mem.rd16(mp_at);
            cpu.mem.wr16(mp_at, mp.wrapping_sub(price as u16));
            let target = if group { 0 } else { u32::from(target_row) };
            let stop = cpu.call(
                APPLIER,
                &[u32::from(class), u32::from(sub), target, u32::from(caster)],
            );
            assert_eq!(stop, Stop::Returned, "spell {spell:#04x}");
            assert_eq!(cpu.call(AGGREGATOR, &[]), Stop::Returned);
            cast += 1;
            let lv_at = RECORDS + u32::from(caster) * STRIDE + 0x161;
            leveled += u32::from(cpu.mem.rd8(lv_at) != level);
            *per_spell.entry(spell).or_default() += 1;
            compare(&mut cpu, &world, &mut failures, || {
                format!(
                    "spell {spell:#04x} class {class:#x} sub {sub:#x} group {group} round {round} \
                     caster {caster} target {target_row} lv {level}"
                )
            });
        }
    }
    assert!(cast > 100, "{cast} casts over {per_spell:x?}");
    assert!(refused > 0, "no confirm met a refusal");
    assert!(leveled > 0, "no cast leveled its spell");
    let by_spell: std::collections::BTreeSet<&str> = failures.iter().map(|f| &f[..10]).collect();
    assert!(
        failures.is_empty(),
        "{cast} casts over {per_spell:x?}, {} differ over {} spells; first per spell:\n{}",
        failures.len(),
        by_spell.len(),
        by_spell
            .iter()
            .map(|p| failures.iter().find(|f| f.starts_with(p)).unwrap().as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Every record, retail against the port; the first differing one fails.
fn compare(
    cpu: &mut psx_retail_exec::Cpu,
    world: &World,
    failures: &mut Vec<String>,
    what: impl Fn() -> String,
) {
    for i in 0..3 {
        let retail = cpu.mem.read(RECORDS + i as u32 * STRIDE, STRIDE as usize);
        let port = world.party.roster.members[i].write();
        let d = diff_offsets(&retail, &port);
        if !d.is_empty() {
            failures.push(format!("{} rec {i}: {d:?}", what()));
            return;
        }
    }
}
