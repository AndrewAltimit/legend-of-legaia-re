//! Disc-gated: the pause-menu Items / Magic screen sessions resolve their
//! window text - names, real bag counts, info-window descriptions,
//! per-caster MP maxima, learned spell levels - from the user's
//! `SCUS_942.54` via [`legaia_engine_core::pause_screens::MenuTextTables`].
//! Structural assertions only (the description strings are Sony text and
//! stay uncommitted). Skips without `LEGAIA_DISC_BIN`.

use legaia_engine_core::Vfs;
use legaia_engine_core::field_menu::FieldMenuRow;
use legaia_engine_core::field_menu_dispatch::{FieldMenuSubsession, build_pause_items_session};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::pause_screens::{items_screen_model, magic_screen_model};
use legaia_engine_core::world::World;
use std::path::PathBuf;

fn disc_scus() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    if !path.is_file() {
        return None;
    }
    legaia_engine_core::DiscVfs::open(&path)
        .ok()?
        .read("SCUS_942.54")
        .ok()
}

fn world_with_disc_text(scus: &[u8]) -> World {
    let mut world = World::new();
    world.roster = legaia_save::Party::zeroed(1);
    let member = &mut world.roster.members[0];
    let mut hms = member.hp_mp_sp();
    hms.hp_max = 100;
    hms.hp_cur = 100;
    hms.mp_max = 120;
    hms.mp_cur = 60;
    member.set_hp_mp_sp(hms);
    member.set_magic_rank(7);
    world.install_menu_text(scus);
    world.set_item_catalog(legaia_engine_core::items::ItemCatalog::vanilla());
    world
}

#[test]
fn items_screen_resolves_disc_names_counts_and_descriptions() {
    let Some(scus) = disc_scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset / unreadable");
        return;
    };
    let mut world = world_with_disc_text(&scus);
    // Healing Berry x3 + Healing Leaf x9 (real consumable ids).
    world.inventory.insert(0x79, 3);
    world.inventory.insert(0x77, 9);

    let mut s = build_pause_items_session(&world);
    // Rows are id-sorted: 0x77 first.
    assert_eq!(s.rows.len(), 2);
    assert_eq!(s.rows[0].name, "Healing Leaf");
    assert_eq!(s.rows[0].count, 9);
    assert_eq!(s.rows[1].name, "Healing Berry");
    assert_eq!(s.rows[1].count, 3);
    // Both consumables carry a single-line disc description naming HP.
    for row in &s.rows {
        assert!(
            row.desc.contains("HP") && !row.desc.contains('\n'),
            "{}: {:?}",
            row.name,
            row.desc
        );
    }

    // Cross on "Use" enters the list; the model stages the hovered row
    // into the info window with the real count + description.
    s.input_pad_edge(PadButton::Cross.mask());
    let m = items_screen_model(&s);
    assert!(m.focus_list);
    assert_eq!(m.pages, 6, "retail 72-slot bag = 6 list pages");
    let info = m.info.expect("hovered row staged");
    assert_eq!(info.name, "Healing Leaf");
    assert_eq!(info.count, 9);
    assert!(info.desc.contains("HP"));
}

#[test]
fn magic_screen_resolves_disc_descriptions_levels_and_mp_max() {
    let Some(scus) = disc_scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset / unreadable");
        return;
    };
    let mut world = world_with_disc_text(&scus);
    // Learn Gimard (0x81), Vera (0x83) and the Ra-Seru Horn (0x9c) at
    // levels 2 / 1 / 1.
    {
        let member = &mut world.roster.members[0];
        let mut list = member.spell_list();
        list.count = 3;
        list.ids[..3].copy_from_slice(&[0x81, 0x83, 0x9c]);
        list.levels[..3].copy_from_slice(&[2, 1, 1]);
        member.set_spell_list(list);
    }
    world.spell_catalog = legaia_engine_core::retail_magic::seru_magic_catalog_from_scus(&scus)
        .expect("disc spell catalog");

    let sub = FieldMenuSubsession::build(
        FieldMenuRow::Magic,
        &world,
        &legaia_engine_core::options::OptionsState::default(),
        &[],
        &legaia_engine_core::tactical_arts_editor::ChainLibrary::new(),
        &world.spell_catalog,
        &legaia_engine_core::battle_stats::EquipmentTable::new(),
    );
    let FieldMenuSubsession::Spells(mut s) = sub else {
        panic!("expected Spells subsession");
    };

    // Caster focus: level + mp/mp_max plumb from the record.
    let m = magic_screen_model(&s, world.menu_text.as_ref());
    assert!(!m.focus_list);
    assert_eq!(m.casters.len(), 1);
    let (_, level, mp, mp_max) = &m.casters[0];
    assert_eq!((*level, *mp, *mp_max), (7, 60, 120));
    // The hovered caster's list previews all three learned spells.
    assert_eq!(m.page_rows.len(), 3);
    assert_eq!(m.page_rows[0].0, "Gimard");
    assert!(m.page_rows[2].1, "Horn is in the Ra-Seru icon block");

    // Enter the list: the staged spell's info carries the learned level,
    // the disc MP cost, and the two-line disc description.
    let _ = s.tick(legaia_engine_core::spell_menu::SpellMenuInput {
        cross: true,
        ..Default::default()
    });
    let m = magic_screen_model(&s, world.menu_text.as_ref());
    assert!(m.focus_list);
    let info = m.info.expect("hovered spell staged");
    assert_eq!(info.name, "Gimard");
    assert_eq!(info.level, 2);
    assert_eq!(info.mp_cost, 10, "Gimard MP from the disc table");
    let lines: Vec<&str> = info.desc.split('\n').collect();
    assert_eq!(lines.len(), 2, "retail desc shape: title | effect line");
    assert!(lines.iter().all(|l| !l.is_empty()));
}
