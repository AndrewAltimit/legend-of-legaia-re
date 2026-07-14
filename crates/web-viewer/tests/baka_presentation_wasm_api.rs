//! Disc-gated: the Baka Fighter duel-presentation WASM surface
//! (`minigames_baka.rs`) must decode the fighters, animation banks, HUD
//! widget table and stage set off a real disc through the same calls
//! `site/js/minigame-baka.js` makes.
//!
//! Structural facts only - no Sony bytes asserted. Skips + passes when
//! `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::minigames::LegaiaMinigames;

fn loaded() -> Option<LegaiaMinigames> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut mg = LegaiaMinigames::new();
    mg.load_disc(bytes).ok()?;
    Some(mg)
}

#[test]
fn duel_presentation_decodes_from_a_real_disc() {
    let Some(mg) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert!(mg.baka_presentation_ready(), "presentation assets resolve");

    // The 51-record HUD widget table, every record on a resolvable art page.
    let hud: serde_json::Value = serde_json::from_str(&mg.baka_hud_json()).unwrap();
    let widgets = hud.as_array().unwrap();
    assert_eq!(widgets.len(), 51);
    for (i, w) in widgets.iter().enumerate() {
        assert!(w["page"].is_u64(), "widget {i} resolves to an art page");
        assert!(w["w"].as_u64().unwrap() > 0);
    }
    // Widget 0 is the PRESS START strip cell traced from the title path.
    assert_eq!(widgets[0]["u"], 48);
    assert_eq!(widgets[0]["v"], 48);
    assert_eq!(widgets[0]["w"], 112);
    assert_eq!(widgets[0]["h"], 16);

    // Player side: the three party fighters, rigged by the PROT 1203 bank
    // (15 / 16 / 15 bones - the per-character bank split).
    for (ch, bones) in [(0u32, 15u32), (1, 16), (2, 15)] {
        let parts = mg.baka_fighter_part_count(0, ch);
        assert_eq!(parts, bones, "party fighter {ch} part count");
        let dims = mg.baka_anim_dims(0, ch, 0);
        assert_eq!(dims[0], bones, "party fighter {ch} idle rig");
        assert!(dims[1] > 0);
        let n = mg.baka_fighter_positions(0, ch).len() / 3;
        assert!(n > 500, "party fighter {ch} has geometry");
        assert_eq!(mg.baka_fighter_object_ids(0, ch).len(), n);
        let frames = mg.baka_anim_pose_frames(0, ch, 0, parts);
        assert_eq!(frames.len(), (dims[1] * parts * 6) as usize);
    }

    // Every ladder rung: mesh + own idle rig covering its TMD objects.
    for roster in 3u32..=16 {
        let parts = mg.baka_fighter_part_count(1, roster);
        assert!(parts > 0, "opponent {roster} mesh");
        let dims = mg.baka_anim_dims(1, roster, 0);
        assert_eq!(dims[0], parts, "opponent {roster} idle rig == nobj");
        assert!(mg.baka_anim_record_count(1, roster) >= 6);
    }

    // The stage set + the duel VRAM build.
    assert!(mg.baka_stage_positions(0).len() > 300, "arena wall mesh");
    assert_eq!(mg.baka_duel_vram(5).len(), 1024 * 512 * 2);
    // An art page decodes to RGBA through a widget's palette.
    let page = widgets[0]["page"].as_u64().unwrap() as usize;
    let palette = widgets[0]["palette"].as_u64().unwrap() as usize;
    let w = mg.baka_page_width(page);
    assert!(w > 0);
    assert_eq!(mg.baka_page_rgba(page, palette).len(), w * 256 * 4);
}
