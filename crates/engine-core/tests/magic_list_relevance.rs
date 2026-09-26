//! The pause Magic screen's relevance gate: retail's spell-record broadcast
//! `FUN_8003053C`, run through the action validator over the roster records.
//!
//! Every `jal 0x8003053c` on the disc is menu code - the list builder
//! (`0x80031210`) and the two cast confirms (`0x801D954C` / `0x801D98B4`) -
//! so a spell that would affect nobody greys in the list and refuses its
//! confirm. The spell table here is synthetic (the record geometry, no disc
//! text); each assertion has a contrast a gate-less screen fails.

use legaia_asset::spell_names::{SpellEntry, SpellNameTable};
use legaia_engine_core::battle_stats::EquipmentTable;
use legaia_engine_core::field_menu::FieldMenuRow;
use legaia_engine_core::field_menu_dispatch::FieldMenuSubsession;
use legaia_engine_core::input::PadButton;
use legaia_engine_core::items::ItemCatalog;
use legaia_engine_core::menu_validator::spell_affects_anyone;
use legaia_engine_core::options::OptionsState;
use legaia_engine_core::pause_screens::MenuTextTables;
use legaia_engine_core::save_select::{SaveRack, SlotSnapshot};
use legaia_engine_core::spell_menu::{InvalidReason, SpellMenuEvent, SpellMenuPhase};
use legaia_engine_core::spells::SpellCatalog;
use legaia_engine_core::tactical_arts_editor::ChainLibrary;
use legaia_engine_core::world::World;

/// The vanilla catalog's Heal - a field-usable single-ally heal.
const HEAL: u8 = 0x10;

/// A three-member party at `hp` / 100, with `HEAL` learned by member 0 and
/// the static spell record for it installed as `(class, sub, flags)`.
fn world(hp: u16, record: Option<(u8, u8, u8)>) -> World {
    let mut world = World::new();
    world.party.roster = legaia_save::Party::zeroed(3);
    for member in &mut world.party.roster.members {
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = hp;
        hms.hp_max = 100;
        hms.mp_cur = 30;
        hms.mp_max = 30;
        member.set_hp_mp_sp(hms);
    }
    world.party.party_count = 3;
    world.party.party_leader_slot = Some(0);
    world.set_item_catalog(ItemCatalog::vanilla());
    let mut spells = world.party.roster.members[0].spell_list();
    spells.count = 1;
    spells.ids[0] = HEAL;
    spells.levels[0] = 1;
    world.party.roster.members[0].set_spell_list(spells);
    if let Some((class, sub_class, target)) = record {
        let mut entries: Vec<SpellEntry> = (0..=HEAL)
            .map(|_| SpellEntry {
                class: 0x14,
                sub_class: 0,
                name: None,
                mp: 0,
                target: 0,
                desc: None,
            })
            .collect();
        entries[HEAL as usize] = SpellEntry {
            class,
            sub_class,
            name: None,
            mp: 5,
            target,
            desc: None,
        };
        world.menu.text = Some(MenuTextTables {
            spell_names: Some(SpellNameTable::from_entries(entries)),
            ..Default::default()
        });
    }
    world
}

fn magic(world: &World) -> FieldMenuSubsession {
    FieldMenuSubsession::build(
        FieldMenuRow::Magic,
        world,
        &OptionsState::default(),
        &SaveRack::Blocks((0..3).map(SlotSnapshot::empty).collect()),
        &ChainLibrary::new(),
        &SpellCatalog::vanilla(),
        &EquipmentTable::new(),
    )
}

/// Pick caster 0 and return whether the Heal row is admissible, then press
/// Cross on it and return whether the confirm opened a target flow.
fn heal_row(world: &World) -> (bool, bool) {
    let mut sub = magic(world);
    sub.tick_pad_edge(PadButton::Cross.mask());
    let FieldMenuSubsession::Spells(s) = &mut sub else {
        panic!("Magic builds the spell session");
    };
    let admissible = s.current_spell_rows()[0].admissible;
    let events = s.tick(legaia_engine_core::spell_menu::SpellMenuInput {
        cross: true,
        ..Default::default()
    });
    let refused = events.contains(&SpellMenuEvent::InvalidConfirm {
        reason: InvalidReason::NobodyAffected,
    });
    let opened = !matches!(s.phase(), SpellMenuPhase::SpellSelect { .. });
    assert!(!(refused && opened));
    (admissible, opened)
}

#[test]
fn a_single_target_heal_greys_when_the_whole_party_is_at_full_hp() {
    // Class 0 = validator arm 0x00 (alive and below max HP), flags 0x06 =
    // one ally: one validator call per present member.
    let rec = Some((0x00, 0x00, 0x06));
    assert_eq!(spell_affects_anyone(&world(50, rec), HEAL), Some(true));
    assert_eq!(heal_row(&world(50, rec)), (true, true));

    assert_eq!(spell_affects_anyone(&world(100, rec), HEAL), Some(false));
    assert_eq!(
        heal_row(&world(100, rec)),
        (false, false),
        "nobody to heal: the row greys and the confirm is refused"
    );

    // One wounded member is enough.
    let mut w = world(100, rec);
    let mut hms = w.party.roster.members[2].hp_mp_sp();
    hms.hp_cur = 99;
    w.party.roster.members[2].set_hp_mp_sp(hms);
    assert_eq!(heal_row(&w), (true, true));
}

#[test]
fn the_all_flag_asks_the_validator_once_on_slot_zero() {
    // Flags bit 0x20: one call with target 0. Arm 0x01 (party walk) then
    // walks the present party itself.
    let rec = Some((0x01, 0x00, 0x26));
    assert_eq!(spell_affects_anyone(&world(50, rec), HEAL), Some(true));
    assert_eq!(spell_affects_anyone(&world(100, rec), HEAL), Some(false));
    // Arm 0x00 on slot 0 only: slot 0 at full HP refuses even with others
    // wounded - the single call never looks past it.
    let mut w = world(100, Some((0x00, 0x00, 0x26)));
    let mut hms = w.party.roster.members[1].hp_mp_sp();
    hms.hp_cur = 1;
    w.party.roster.members[1].set_hp_mp_sp(hms);
    assert_eq!(spell_affects_anyone(&w, HEAL), Some(false));
}

#[test]
fn without_the_disc_spell_table_the_gate_stays_open() {
    assert_eq!(spell_affects_anyone(&world(100, None), HEAL), None);
    assert_eq!(heal_row(&world(100, None)), (true, true));
}
