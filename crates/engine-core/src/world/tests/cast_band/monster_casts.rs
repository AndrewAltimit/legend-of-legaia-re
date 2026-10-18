//! Monster casts: the tag-0x23 entry, the capture charge branch and the element
//! change.

use super::*;
use legaia_engine_mode::mode::SceneMode;

/// A monster's cast clip is the tag-`0x23` entry its magic **slot** walks to
/// (`FUN_801E9FD4`, `0x801EA4A4..0x801EA548`), reversed against the slot
/// order - not an entry tagged with the spell id. Gimard's lone Tail Fire
/// `0x27` plays entry 8, his only tag-`0x23` entry, as the
/// `battle_gimard_tail_fire_a` capture reads (`+0x1E0 = 8`).
#[test]
fn a_monster_cast_plays_the_tag_0x23_entry_its_magic_slot_walks_to() {
    use legaia_actor_formats::monster_archive::MonsterAnimation;
    use std::sync::Arc;
    let clip = |tag: u8| {
        Some(MonsterAnimation {
            variant_track: Default::default(),
            action_id: tag,
            rate: 1,
            attach_key: 0,
            solo_flag: 0,
            impact_class: 0,
            effect_script: Vec::new(),
            part_count: 0,
            frame_count: 0,
            frames: Vec::new(),
        })
    };
    let mut world = World::default();
    world.party.party_count = 1;
    world.actors.push(Actor::default());
    world.actors.push(Actor::default());
    world.tables.monster_catalog.insert(
        legaia_engine_battle::monster_catalog::MonsterDef::new(10, "Gimard", 99, 60)
            .with_magic([0x27]),
    );
    world.actors[1].battle_monster_id = Some(10);
    let tags = [0x00, 0x01, 0x02, 0x04, 0x05, 0x0B, 0x0D, 0x0F, 0x23];
    world.actors[1].battle_action_clips = Some(Arc::new(tags.iter().map(|&t| clip(t)).collect()));
    assert_eq!(world.monster_cast_clip(1, 0x27), Some(8));

    // Two slots: the first tag-0x23 entry belongs to the LAST slot.
    world.tables.monster_catalog.insert(
        legaia_engine_battle::monster_catalog::MonsterDef::new(10, "Gimard", 99, 60)
            .with_magic([0x27, 0x28]),
    );
    let tags = [0x00, 0x01, 0x23, 0x04, 0x23];
    world.actors[1].battle_action_clips = Some(Arc::new(tags.iter().map(|&t| clip(t)).collect()));
    assert_eq!(world.monster_cast_clip(1, 0x28), Some(2));
    assert_eq!(world.monster_cast_clip(1, 0x27), Some(4));
}

/// PROT 0953's charge owes no fold: with the module's skip raised, the
/// pending cast is consumed and nobody's HP moves, and the skip is spent.
#[test]
fn a_capture_charge_branch_folds_nothing() {
    let mut world = seru_cast_world();
    let before: Vec<u16> = world.actors.iter().map(|a| a.battle.hp).collect();
    world.casting.pending_cast = Some(crate::world::PendingCast {
        caster: 1,
        spell_id: 0x81,
        targets: vec![0],
        on_flight: false,
    });
    world.casting.module_skips_fold = true;
    world.fold_pending_cast();
    let after: Vec<u16> = world.actors.iter().map(|a| a.battle.hp).collect();
    assert_eq!(before, after);
    assert!(world.casting.pending_cast.is_none());
    assert!(!world.casting.module_skips_fold);
}

/// PROT 0964's Element Change re-rolls against, and then rewrites, retail's
/// `0x801C8FE4` (`0x801F8A20` / `0x801F8A4C` / `0x801F8A90`) - the AI
/// counter Rogue's picker turns into its next attack (`counter - 0x50`). A
/// cast that left the counter alone kept Rogue on `0xB0` for the whole fight.
#[test]
fn element_change_writes_the_ai_counter_it_rerolls_against() {
    use legaia_game_tables::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};
    let mut world = module_code_world();
    let mut entries = vec![SpellEntry::default(); 0x100];
    entries[0xAF].class = CAPTURE_CLASS;
    // Sub-id 29 pages PROT 0964 (`935 + 29`).
    entries[0xAF].sub_class = 29;
    world.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
        spell_names: Some(SpellNameTable::from_entries(entries)),
        ..Default::default()
    });
    assert_eq!(world.cast_module_for(0xAF), Some(964));
    let mut seen = Vec::new();
    for _ in 0..6 {
        let before = world.battle.monster_ai_state.counter();
        world.casting.module_phase = 0;
        let mut done = false;
        for _ in 0..200 {
            let run = world.run_cast_module_code(0xAF, 3).expect("band entry");
            if !run.busy {
                done = true;
                break;
            }
        }
        assert!(done, "Element Change finishes");
        let after = world.battle.monster_ai_state.counter();
        assert!((0..3).contains(&after), "the counter holds the roll");
        assert_ne!(after, before, "the re-roll never repeats the counter");
        seen.push(after);
    }
    assert!(
        seen.iter().any(|&c| c != seen[0]),
        "Rogue's attack id never moved: {seen:?}"
    );
}

/// PROT 0964's Element Change rewrites the record copy retail's battle
/// loader made for the seat (`0x801C9348[0]`, `+0x1D`), not the archive: the
/// changed element is this fight's only, and the next battle with the same
/// monster starts on its disc element.
#[test]
fn element_change_stays_on_the_seat_and_ends_with_the_battle() {
    use legaia_engine_battle::monster_catalog::{FormationDef, FormationSlot, MonsterDef};
    let mut party = legaia_save::Party::zeroed(3);
    for member in &mut party.members {
        let mut hms = member.hp_mp_sp();
        hms.hp_cur = 100;
        hms.hp_max = 100;
        member.set_hp_mp_sp(hms);
    }
    let mut world = World::new();
    world.load_party(party);
    world.party.party_count = 3;
    let mut def = MonsterDef::new(0x40, "Rogue", 500, 10);
    def.element = 2;
    world.tables.monster_catalog.insert(def);
    let formation = FormationDef::new(7, vec![FormationSlot::new(0x40)]);
    let enter = |world: &mut World| {
        world.field_return = Some(crate::world::FieldReturnState {
            actors: world.actors.clone(),
            player_actor_slot: world.player_actor_slot,
            party_count: world.party.party_count,
        });
        world.battle.return_mode = SceneMode::Field;
        world.enter_battle_from_formation(&formation);
    };
    enter(&mut world);
    assert_eq!(world.monster_seat_element(3), Some(2));
    world.apply_cast_element_change(5);
    assert_eq!(world.monster_seat_element(3), Some(5), "the seat changed");
    assert_eq!(
        world.tables.monster_catalog.get(0x40).map(|d| d.element),
        Some(2),
        "the catalog record is untouched"
    );
    world.finish_battle();
    enter(&mut world);
    assert_eq!(
        world.monster_seat_element(3),
        Some(2),
        "the next fight starts on the disc element"
    );
}
