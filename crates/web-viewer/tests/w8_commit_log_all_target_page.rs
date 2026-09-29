//! Disc-gated page ladder: an **all-target** spell committed on the browser
//! play page, so the battle commit log lands its target plaque by copying the
//! whole-row label's placement - `FUN_801D57E8`
//! (`battle_cursor_pose::element_placement_copy`), reached from
//! `battle_commit_log::stage_commit_row`'s `AllEnemies` / `AllAllies` arms.
//!
//! No other ladder commits an all-target command, so the row was never
//! entered although it is drawn on both hosts. Two preconditions stand in
//! front of it and the fixture seeds each the way a player would arrive with
//! it: a party that has a Ra-Seru (a cold start's ring refuses the Magic arm,
//! status bit `0x1000`), which is a played-through card from the user's save
//! library loaded through the page's own import; and an all-target spell at
//! the top of the lead's list (the page's `learn_spell`, the browser twin of
//! the native `--learn-spell`). The pad does the rest - Begin, the Magic arm,
//! the spell row - and the page composes the frame the committed row shows
//! on. The spell id
//! is picked off the disc's own spell table by its target byte (the cheapest
//! whole-row spell, so a cold-start lead can pay for it) rather than named, so
//! the choice cannot drift from what makes it all-target.
//!
//! Structural assertions only: the commit-log row draws glyphs from its name
//! pen. Skips + passes when `LEGAIA_DISC_BIN` or the library card is
//! missing.

#![cfg(not(target_arch = "wasm32"))]

use legaia_asset::spell_names::{SpellNameTable, SpellTargetShape};
use legaia_engine_core::input::PadButton;
use legaia_engine_vm::battle_commit_log::{NAME_X, ROW_Y_LOWER};
use legaia_web_viewer::runtime::LegaiaRuntime;

/// Stage scale at the 960x720 surface (origin `(0, 0)`).
const SCALE: i64 = 3;

fn extracted_scus() -> Option<Vec<u8>> {
    let dirs = [
        std::env::var("LEGAIA_EXTRACTED_DIR").ok(),
        Some("extracted".into()),
        Some("../extracted".into()),
        Some("../../extracted".into()),
    ];
    dirs.into_iter()
        .flatten()
        .find_map(|d| std::fs::read(std::path::Path::new(&d).join("SCUS_942.54")).ok())
}

/// A played-through card from the user's save library (gitignored).
fn card_path() -> Option<std::path::PathBuf> {
    let lib = std::env::var("LEGAIA_SAVES_LIBRARY")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            ["saves/library", "../saves/library", "../../saves/library"]
                .iter()
                .map(std::path::PathBuf::from)
                .find(|p| p.is_dir())
        })?;
    let p = lib.join("cards").join("playthrough-endgame-7saves.mcr");
    p.is_file().then_some(p)
}

/// The cheapest player Seru spell whose target byte names a whole row.
fn all_target_spell(scus: &[u8]) -> Option<(u8, u8, SpellTargetShape)> {
    let table = SpellNameTable::from_scus(scus)?;
    (0x81u8..=0x95)
        .filter_map(|id| {
            let e = table.entry(id)?;
            let shape = e.target_shape();
            matches!(
                shape,
                SpellTargetShape::AllEnemies | SpellTargetShape::AllAllies
            )
            .then_some((id, e.mp, shape))
        })
        .min_by_key(|&(_, mp, _)| mp)
}

/// Glyph quads on the commit log's first row, provided one starts on the
/// row's name pen (`NAME_X`) - the name / command / target triple's seat.
fn commit_row_glyphs(rt: &mut LegaiaRuntime) -> usize {
    let v: serde_json::Value =
        serde_json::from_str(&rt.play_overlay_draws_json(960, 720)).expect("overlay json");
    let pen_y = (i64::from(ROW_Y_LOWER) - 2) * SCALE;
    let pen_x = i64::from(NAME_X) * SCALE;
    let row: Vec<i64> = v["texts"]
        .as_array()
        .map(|t| {
            t.iter()
                .filter(|q| {
                    let y = q["dst"][1].as_i64().unwrap_or(-1);
                    (pen_y - 3 * SCALE..=pen_y + 3 * SCALE).contains(&y)
                })
                .filter_map(|q| q["dst"][0].as_i64())
                .collect()
        })
        .unwrap_or_default();
    if row.iter().any(|&x| (pen_x..pen_x + 2 * SCALE).contains(&x)) {
        row.len()
    } else {
        0
    }
}

#[test]
fn an_all_target_spell_commit_lands_the_row_label_on_the_page() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Some(scus) = extracted_scus() else {
        eprintln!("[skip] extracted/SCUS_942.54 missing");
        return;
    };
    let (spell, mp, shape) = all_target_spell(&scus).expect("a player spell targets a whole row");
    eprintln!("[ran] all-target spell {spell:#04x} ({mp} MP, {shape:?})");

    let mut rt = LegaiaRuntime::new();
    rt.load_disc(std::fs::read(&disc).expect("read disc"), String::new())
        .expect("load disc");
    // A played-through party: a cold start has no Ra-Seru, and the ring
    // refuses the Magic arm (status bit `0x1000`) until one is joined. The
    // card is the user's own library save, loaded through the page's import.
    let card = card_path().and_then(|p| std::fs::read(p).ok());
    let Some(card) = card else {
        eprintln!("[skip] saves library card missing");
        return;
    };
    rt.import_card_save(card, 1).expect("import the card save");
    rt.play_resume_save().expect("land the save");
    for _ in 0..8 {
        rt.tick_frame().expect("tick");
    }
    rt.enter_field("map01").expect("enter map01");
    for _ in 0..5 {
        rt.tick_frame().expect("tick");
    }
    assert!(rt.learn_spell(0, spell), "the lead has a roster record");
    assert!(rt.debug_force_battle(-1), "map01 arms a fight");

    // Walk the command session with the pad: Begin at the round prompt
    // (Left), the Magic arm on the ring (Right), and row 0 of the spell list
    // (the spell just learned is prepended).
    let mut best = 0usize;
    let mut g = 0u32;
    for _ in 0..4800u32 {
        let active = rt.play_battle_active();
        if active {
            g += 1;
        }
        // Frames since the fight opened: Begin, the Magic arm (the press
        // commits the arm), then Cross on the spell row - a whole-row spell
        // opens no target cursor, so the commit lands at once and the log
        // draws the row while the next member's ring is up. No row may draw
        // before that commit (the probe runs from the Magic press on).
        let pad = match g {
            40 => PadButton::Left.mask(),
            60 => PadButton::Right.mask(),
            80 | 100 | 120 => PadButton::Cross.mask(),
            _ => 0,
        };
        rt.set_pad(pad);
        rt.tick_frame().expect("tick");
        if active && g > 60 {
            let n = commit_row_glyphs(&mut rt);
            if std::env::var_os("W8_TRACE").is_some() {
                eprintln!("g={g} row glyphs={n}");
            }
            best = best.max(n);
        }
        if best > 0 || g > 140 {
            break;
        }
    }
    assert!(
        best > 0,
        "no commit-log row drew for the all-target spell {spell:#04x}"
    );
    eprintln!("[ok] commit log drew {best} glyph quad(s) on its first row");
}
