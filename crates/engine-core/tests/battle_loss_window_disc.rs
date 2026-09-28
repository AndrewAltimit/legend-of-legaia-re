//! Disc-gated: the loss window's two text pieces parse off the real PROT 0898
//! image, and the screen-element placement records `0x41` / `0x42` the
//! results frame opens are the same framed window.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset (disc-gated convention).
use legaia_engine_vm::battle_party_panel::{
    DEFEAT_WINDOW_ELEMENT, DefeatText, RESULT_WINDOW_ELEMENT, ResultSubject,
};
use legaia_patcher::disc::DiscPatcher;

#[test]
fn the_loss_window_text_and_seat_come_off_the_disc() {
    let Some(path) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let disc = std::fs::read(path).expect("read disc");
    let patcher = DiscPatcher::open(disc).expect("open disc");
    let overlay = patcher.read_entry(898).expect("read PROT 0898");
    let text = DefeatText::parse(&overlay).expect("defeat pieces parse");
    assert!(!text.solo_suffix.is_empty() && !text.team_tail.is_empty());
    let solo = text.compose(ResultSubject::Lead(1), "X");
    let team = text.compose(ResultSubject::LeadsTeam { escape_operand: 0 }, "X");
    assert_ne!(solo, team, "the two build arms word the loss differently");

    let scus = patcher.read_named_file("SCUS_942.54").expect("read SCUS");
    let table = legaia_asset::screen_elements::ScreenElementTable::from_scus(&scus)
        .expect("placement table");
    let win = table.get(RESULT_WINDOW_ELEMENT).expect("record 0x41");
    let loss = table.get(DEFEAT_WINDOW_ELEMENT).expect("record 0x42");
    assert_eq!(win, loss, "the loss window is the result window's twin");
    eprintln!(
        "[ok] loss window: {} + {} byte pieces; record 0x42 lands at {:?}",
        text.solo_suffix.len(),
        text.team_tail.len(),
        loss.alt_pen()
    );
}
