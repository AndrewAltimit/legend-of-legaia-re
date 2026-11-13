//! Disc-gated: the art-learned text a field script prints, resolved off the
//! user's disc (issue #603).
//!
//! - The dialog `0xC5` arts-name token: Drake Castle's book of Biron arts
//!   (`dolk2` P1[49]) closes each chapter on "so you already know the
//!   `0xC5 XX`." with `XX` = `char * 0x40 + art` (`0x0D`, `0x49`, `0x8E`).
//!   Each must resolve to the arts-name table's own row for that pair.
//! - The prompt screen op `49 09 00 <char> <art>` opens: window record `9`
//!   (`FUN_801E733C`) prints the field overlay's two strings with the
//!   character and the art patched in. Both lines must come off PROT 0897,
//!   name the character, and carry the art's table name.
//!
//! Only structural facts are asserted - every expected name is read back
//! out of the same disc tables, never spelled here. Skips and passes without
//! `LEGAIA_DISC_BIN`.

use legaia_engine_core::world::World;
use legaia_engine_field::submode_hub_actors::slot;
use legaia_engine_scene::vfs::Vfs;
use legaia_engine_scene_host::scene_host::SceneHost;

fn booted() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return None;
    };
    let path = std::path::PathBuf::from(&disc);
    if !path.is_file() {
        eprintln!("[skip] LEGAIA_DISC_BIN is not a file");
        return None;
    }
    let mut host = SceneHost::open_disc(&path).ok()?;
    let scus = legaia_engine_scene::vfs::DiscVfs::open(&path)
        .ok()?
        .read("SCUS_942.54")
        .ok()?;
    host.world.install_menu_text(&scus);
    host.world.party.party_names = vec!["Vahn".into(), "Noa".into(), "Gala".into()];
    Some(host)
}

/// The table's own name bytes for `[character, art]`.
fn table_name(world: &World, character: u8, art: u8) -> Vec<u8> {
    world
        .menu
        .text
        .as_ref()
        .and_then(|t| t.arts.as_ref())
        .and_then(|a| {
            a.iter()
                .find(|e| e.character as u8 == character && e.index == art)
        })
        .map(|e| e.name_bytes.clone())
        .expect("the arts-name table holds the row")
}

#[test]
fn the_book_chapters_arts_tokens_resolve_off_the_disc() {
    let Some(host) = booted() else { return };
    let w = &host.world;
    for (operand, character, art) in [(0x0Du8, 0u8, 0x0Du8), (0x49, 1, 9), (0x8E, 2, 0x0E)] {
        let record = [
            0x1F, b't', 0xCF, 0x06, 0xC5, operand, 0xCF, 0x07, b'.', 0x00,
        ];
        let subs = w.dialog_substitutions(&record).expect("a substitution");
        let got = subs.get(&(5, operand)).expect("the 0xC5 token resolves");
        let want = table_name(w, character, art);
        assert!(!got.is_empty(), "operand {operand:#04x} names an art");
        assert_eq!(got, &want, "operand {operand:#04x}");
    }
}

#[test]
fn the_learned_prompt_prints_both_lines_off_the_field_overlay() {
    let Some(mut host) = booted() else { return };
    let w = &mut host.world;
    let _ = w.man_load_actor_reset();
    w.open_field_submode_screen(slot::PROMPT, None);
    w.set_submode_board_entries(&[0x49, 0x09, 0x00, 0x01, 0x09]);
    w.tick_submode_screen(1);
    assert_eq!(w.art_notice_shown(), Some((1, 9)));
    let want_art = table_name(w, 1, 9);
    let [first, second] = host.art_notice_lines().expect("the notice lines");
    let has = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).any(|x| x == needle);
    assert!(has(&first, b"Noa"), "the first line names the character");
    assert!(first.len() > 3, "and carries the overlay's own text");
    assert!(has(&second, &want_art), "the second line names the art");
    assert!(
        !first
            .iter()
            .chain(second.iter())
            .any(|&b| b == 0xC1 || b == 0xC5),
        "no token is left unexpanded"
    );
}
