//! Disc-free: the minigames page fights a Muscle Dome contest **as the
//! ordinary battle**, on the play page's runtime, by default.
//!
//! Retail's fight start `FUN_801D1510` hands each round to the shared battle
//! (formation cell, map id `0x4C6`, game mode `0x14`), and both play hosts do
//! the same (`World::start_muscle_battle_leg`). The page has no `World` of its
//! own, so it opens the contest in an embedded `play.html` launched straight
//! into the arena; the dome session's own panel stays only as the free-play
//! "one foe" host. Read off the repo's own HTML, as the host-drift gates do.
//!
//! No Sony bytes.

#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

fn repo(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn the_contest_defaults_to_the_battle_host() {
    let page = repo("site/_content/minigames.html");
    let sel = page.find("id=\"muscle-host\"").expect("the host picker");
    let body = &page[sel..sel + 600];
    let battle = body
        .find("value=\"battle\" selected")
        .expect("battle is the default");
    let panel = body
        .find("value=\"panel\"")
        .expect("the panel stays as an option");
    assert!(battle < panel);
    assert!(
        page.contains("id=\"muscle-battle\""),
        "the embedded runtime frame"
    );
    let start = page
        .find("function muscleBattleStart()")
        .expect("the launcher");
    let launch = &page[start..start + 700];
    assert!(
        launch.contains("minigame: '5'"),
        "launches the arena sub-id"
    );
    assert!(
        launch.contains("'play.html?'"),
        "on the play page's runtime"
    );
}

#[test]
fn the_play_page_opens_a_minigame_off_its_query() {
    let play = repo("site/_content/play.html");
    let at = play
        .find("function startPendingMinigame()")
        .expect("the query launch");
    let body = &play[at..at + 900];
    assert!(body.contains("enter('koin1')"), "from the casino floor");
    assert!(
        body.contains("play_mg_choose_muscle_course"),
        "the course menu's pick"
    );
    assert!(
        body.contains("play_mg_debug_warp(req.sub)"),
        "through the door warp"
    );
    let rs = repo("crates/web-play/src/play_minigames.rs");
    assert!(rs.contains("pub fn play_mg_choose_muscle_course"));
}
