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

#[test]
fn a_track_no_state_was_saved_under_keys_alike_when_seated() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        return;
    }
    // Tracks the library holds no state of, seated through retail's own VAB
    // open and `_SsSeqInit` on another state's libsnd: `2057` counts its loop
    // with the NRPN LSB (CC 98) and loops at about vsync 5950, `2072` keys a
    // note past the pitch table's front from vsync 1384, and `2075` is a
    // battle-bank theme.
    for (track, vsyncs) in [(2057, 6200), (2072, 1500), (2075, 3600)] {
        let Some(d) =
            crate::seq_differential::run_seated_track("mei_house_inside", track, vsyncs, false)
                .expect("differential")
        else {
            eprintln!("[skip] mei_house_inside not in the library");
            return;
        };
        assert_keyed_alike(&d, &format!("seated {track}"));
    }
}
