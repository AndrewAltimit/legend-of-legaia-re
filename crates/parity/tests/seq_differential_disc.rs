//! Retail's libsnd, run off the state's RAM in the interpreter, against the port's
//! sequencer: every note, release and voice register over a stretch of score, with
//! and without cues contending for the voices. Skips without the disc and library.

use crate::seq_differential::{Differential, run_label};

fn assert_keyed_alike(d: &Differential, what: &str) {
    assert!(d.retail_keyons > 100, "{what}: nothing keyed: {d:?}");
    assert_eq!(d.retail_keyons, d.port_keyons, "{what}: {:#?}", d.first);
    assert_eq!(d.key_diffs, 0, "{what}: {:#?}", d.first);
    assert_eq!(d.release_diffs, 0, "{what}: {:#?}", d.first);
    assert_eq!(d.sound_diffs, 0, "{what}: {:#?}", d.first);
}

#[test]
fn the_score_keys_as_retails_libsnd_keys_it() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        return;
    }
    // The Baka Fighter theme's pan moves swing tails still ringing.
    for label in ["minigame_baka_fighter", "title_screen_new_game"] {
        let Some(d) = run_label(label, 3600, false).expect("differential") else {
            eprintln!("[skip] {label} not in the library");
            continue;
        };
        assert_keyed_alike(&d, label);
    }
}

#[test]
fn cues_contend_for_voices_as_they_do_in_retail() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        return;
    }
    let Some(d) = run_label("title_screen_new_game", 3600, true).expect("differential") else {
        eprintln!("[skip] title_screen_new_game not in the library");
        return;
    };
    assert_keyed_alike(&d, "cues");
}
