//! The battle magic window (`0x46`) draws each spell's element plate.
//!
//! Retail's list draws a spell row as the spell's raw display name, whose
//! leading `0xCE op 0x20` escape emits the element plate at the row pen
//! (`docs/formats/spell-table.md`). The rung opens the window by pad - a
//! field encounter, Begin, the ring's Magic arm - over a caster that knows
//! Gimard, then composes it through the shared `engine-screens` frame both
//! play hosts draw, with a font carrying the disc's escape sprites.
//!
//! Disc-gated (`LEGAIA_DISC_BIN` plus an extracted tree, found through
//! `LEGAIA_EXTRACTED_DIR` before the repo-relative fallbacks); skips and
//! passes without them, printing why.

use std::path::PathBuf;

use legaia_engine_battle::monster_catalog::{vanilla_formation_table, vanilla_monster_catalog};
use legaia_engine_core::world::{Actor, World};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene::vfs::Vfs;
use legaia_engine_system::input::{InputState, PadButton};

/// Gimard, a player Seru spell whose name opens with the fire plate.
const GIMARD: u8 = 0x81;

fn extracted_dir() -> Option<PathBuf> {
    let over = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    over.into_iter()
        .chain(
            ["extracted", "../extracted", "../../extracted"]
                .into_iter()
                .map(PathBuf::from),
        )
        .find(|d| d.join("PROT.DAT").exists())
}

fn disc_scus() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    legaia_engine_scene::vfs::DiscVfs::open(&path)
        .ok()?
        .read("SCUS_942.54")
        .ok()
}

fn press(w: &mut World, b: PadButton) {
    for _ in 0..64 {
        if !w.battle_command_pad_held() {
            break;
        }
        w.set_pad(0);
        let _ = w.tick();
    }
    w.set_pad(InputState::mask_of([b]));
    let _ = w.tick();
    w.set_pad(0);
    let _ = w.tick();
}

#[test]
fn the_battle_magic_window_draws_each_spells_element_plate() {
    let Some(scus) = disc_scus() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset / unreadable");
        return;
    };
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing (set LEGAIA_EXTRACTED_DIR or run legaia-extract)");
        return;
    };
    let prot = std::fs::read(extracted.join("PROT.DAT")).expect("PROT.DAT");
    let head = &prot[legaia_font::escape_icons::ICON_PROT_DAT_OFFSET as usize..];
    let font = legaia_font::Font::load_from_extracted(&extracted)
        .expect("extracted dialog font (run legaia-extract)")
        .with_escape_icons_from_disc(head, &scus);
    assert!(
        font.escape_cell(0x14).is_some(),
        "no escape sprites decoded"
    );

    let mut w = World::new();
    while w.actors.len() < 8 {
        w.actors.push(Actor::default());
    }
    w.party.party_count = 3;
    w.load_party(legaia_save::Party::zeroed(3));
    for i in 0..3 {
        w.actors[i].active = true;
        w.actors[i].battle.hp = 100;
        w.actors[i].battle.max_hp = 100;
        w.actors[i].battle.liveness = 1;
    }
    w.set_formation_table(vanilla_formation_table(), vanilla_monster_catalog());
    w.install_menu_text(&scus);
    w.tables.spell_catalog = legaia_engine_battle::retail_magic::retail_seru_magic_catalog();
    {
        let rec = &mut w.party.roster.members[0];
        let mut list = rec.spell_list();
        list.count = 1;
        list.ids[0] = GIMARD;
        list.levels[0] = 1;
        rec.set_spell_list(list);
    }
    w.player_actor_slot = Some(0);
    w.actors[0].move_state.world_x = 300;
    w.actors[0].move_state.world_z = 300;
    w.actors[0].move_state.field_72 = 4096;
    {
        use legaia_engine_battle::encounter::{
            EncounterEntry, EncounterSession, EncounterTable, EncounterTracker,
        };
        let mut table = EncounterTable::new("battle_spell_plate");
        table.set_trigger_rate(0xFF);
        table.push(EncounterEntry::new(1, 1));
        let mut session = EncounterSession::new(EncounterTracker::new(table));
        session.transition_frames = 2;
        session.grace_frames = 2;
        w.set_encounter_session(Some(session));
    }
    w.mode = SceneMode::Field;
    w.toggles.live_gameplay_loop = true;
    w.battle.player_driven = true;

    let up = InputState::mask_of([PadButton::Up]);
    for _ in 0..6000 {
        w.set_pad(up);
        let _ = w.tick();
        if w.mode == SceneMode::Battle {
            break;
        }
    }
    assert_eq!(w.mode, SceneMode::Battle, "no encounter triggered");
    for i in 0..3 {
        w.actors[i].battle.mp = 99;
    }
    for _ in 0..0x400 {
        if w.battle.command.is_some() && w.battle_ctx.active_actor == 0 {
            break;
        }
        w.set_pad(0);
        let _ = w.tick();
    }
    if matches!(
        w.battle.command.as_ref().map(|s| &s.phase),
        Some(legaia_engine_battle_menus::battle_input::CommandPhase::RoundPrompt { .. })
    ) {
        press(&mut w, PadButton::Cross); // Begin -> the ring
    }
    press(&mut w, PadButton::Right); // ring: Magic arm
    let model = w
        .battle_spell_menu_model()
        .expect("the Magic arm opened the spell window");
    assert_eq!(
        model.icons,
        [Some(0x14)],
        "Gimard's name opens with the fire plate"
    );

    let text = legaia_engine_screens::with_battle_spell_frame(&model, |f| {
        legaia_engine_ui::battle_item_ui::battle_spell_window_text(&font, f, (0, 0), 1)
    });
    let pen = legaia_engine_ui::battle_item_ui::SPELL_ICON_PEN_X;
    let plate = text
        .iter()
        .find(|t| t.dst.0 == pen && t.dst.2 == 20 && t.dst.3 == 12)
        .expect("the fire plate draws at the row pen");
    assert_eq!(
        plate.color[..3],
        [1.0, 1.0, 1.0],
        "a plate keeps its own colours"
    );
    assert!(
        text.iter()
            .any(|t| t.dst.1 == plate.dst.1 && t.dst.0 == pen + 25),
        "the name follows the plate one width plus a space on"
    );
    eprintln!("[battle magic] plate at ({}, {})", plate.dst.0, plate.dst.1);
}
