//! The battle arts entry's Triangle list on the live World: Triangle opens
//! the pager, the window slides in by the battle frame step
//! (`FUN_801D3444`), and the open page's rows come out of the arts-name
//! table for the character's learned ids (`FUN_80034358`), names spliced
//! and AP halved under the record's `+0xF8` bit `0x800`.

use super::*;
use legaia_art::arts_table::ArtTableEntry;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_system::input::PadButton;

fn art(index: u8, name: &[u8], ap: u8) -> ArtTableEntry {
    ArtTableEntry {
        character: legaia_art::Character::Vahn,
        index,
        name: String::new(),
        ap,
        commands: Vec::new(),
        is_miracle: index == 0,
        name_bytes: name.to_vec(),
        glyphs: vec![[0xFF, 0x06], [0x81, 0xAA]],
    }
}

fn list_world(learned: &[u8], halved: bool) -> World {
    let mut world = World {
        party: legaia_engine_menus::party_state::PartyState {
            party_count: 1,
            ..Default::default()
        },
        ..World::default()
    };
    world.mode = SceneMode::Battle;
    world.party.party_names = vec!["Vahn".into()];
    let mut lead = legaia_save::CharacterRecord::zeroed();
    let mut skills = lead.displayed_skills();
    skills.count = learned.len() as u8;
    skills.ids[..learned.len()].copy_from_slice(learned);
    lead.set_displayed_skills(skills);
    if halved {
        let mut bits = lead.ability_bits();
        bits[5] |= 0x08;
        lead.set_ability_bits(bits);
    }
    world.party.roster = legaia_save::Party {
        members: vec![lead],
    };
    world.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
        arts: Some(
            (0..8)
                .map(|i| art(i, &[b'A' + i], 10 + i))
                .chain([art(9, b"\xC1\x00!", 40)])
                .collect(),
        ),
        ..Default::default()
    });
    world.actors[0].battle.max_hp = 100;
    world.actors[0].battle.hp = 100;
    world.actors[0].battle.liveness = 1;
    world.open_arts_command_input(0);
    world
}

fn tick(world: &mut World, button: Option<PadButton>) {
    world.input.set_pad(0);
    if let Some(b) = button {
        world.input.set_pad(b.mask());
    }
    world.battle.anim_pass_vsyncs = 2;
    world.tick_battle_arts_input();
}

#[test]
fn triangle_opens_the_list_and_the_window_slides_in() {
    let mut w = list_world(&[1, 2, 3, 4, 5, 6, 7], false);
    assert_eq!(w.arts_list_rows(), None, "closed: no rows");
    tick(&mut w, Some(PadButton::Triangle));
    let s = w.battle.arts_input.as_ref().unwrap();
    assert_eq!((s.list_page, s.list_slide), (Some(0), 2));
    for _ in 0..8 {
        tick(&mut w, None);
    }
    assert_eq!(w.arts_input_view().unwrap().list_slide, 12);
    let rows = w.arts_list_rows().unwrap();
    assert_eq!(rows.len(), 5);
    let first = rows[0].as_ref().unwrap();
    assert_eq!((first.name.as_slice(), first.ap), (&b"B"[..], 11));
    // The second page holds the last two learned ids.
    tick(&mut w, Some(PadButton::Triangle));
    let rows = w.arts_list_rows().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].as_ref().unwrap().name, b"H");
}

#[test]
fn a_missing_id_keeps_its_row_and_names_splice_and_ap_halves() {
    let mut w = list_world(&[9, 8, 1], true);
    tick(&mut w, Some(PadButton::Triangle));
    let rows = w.arts_list_rows().unwrap();
    assert_eq!(rows.len(), 3);
    let spliced = rows[0].as_ref().unwrap();
    assert_eq!((spliced.name.as_slice(), spliced.ap), (&b"Vahn!"[..], 20));
    assert!(rows[1].is_none(), "id 8 is not in the table");
    assert_eq!(rows[2].as_ref().unwrap().ap, 11 >> 1);
}

#[test]
fn a_seated_list_stands_on_the_captures_page() {
    let mut w = list_world(&[1, 2, 3, 4, 5, 6, 7], false);
    w.seat_arts_list(1, 12);
    let view = w.arts_input_view().unwrap();
    assert_eq!((view.list_page, view.list_slide), (Some(1), 12));
    w.seat_arts_list(2, 12);
    assert_eq!(
        w.arts_input_view().unwrap().list_page,
        Some(1),
        "no third page"
    );
}
