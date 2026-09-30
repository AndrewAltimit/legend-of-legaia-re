//! Disc-gated: the host keeps retail's track-select word `_DAT_8007BAC8`.
//!
//! Op `0x35` sub-ops `1` and `9` store their id into the word before
//! anything resolves it, so the park sentinel `0x1000` - which starts no
//! track (the resolver at `0x8002454C` leaves the slot parked) - still
//! lands there. A director only ever sees tracks that start, so the word is
//! the host's to keep: it is what a save state's BGM word is compared with.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` / the extracted disc are
//! missing (`LEGAIA_EXTRACTED_DIR` first, then repo-relative).

use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::scene::{BgmDirector, SceneHost};
use std::path::PathBuf;

#[derive(Default)]
struct Starts(Vec<u16>);

impl BgmDirector for Starts {
    fn start(&mut self, id: u16, _bytes: &[u8]) {
        self.0.push(id);
    }
    fn start_owned_vab(&mut self, id: u16, _bytes: &[u8]) {
        self.0.push(id);
    }
}

fn extracted() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let env = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    let rel = ["extracted", "../extracted", "../../extracted"].map(PathBuf::from);
    let found = env
        .into_iter()
        .chain(rel)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists());
    if found.is_none() {
        eprintln!("[skip] extracted/ missing");
    }
    found
}

#[test]
fn a_park_sentinel_start_selects_the_word_and_starts_nothing() {
    let Some(extracted) = extracted() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    assert_eq!(host.bgm_track_word, None);
    let mut starts = Starts::default();

    host.world.pending_field_events.push(FieldEvent::Bgm {
        text_id: 2016,
        sub_op: 1,
    });
    host.route_bgm_events(&mut starts).expect("route");
    assert_eq!(starts.0, vec![2016], "a pool id starts its track");
    assert_eq!(host.bgm_track_word, Some(2016));

    host.world.pending_field_events.push(FieldEvent::Bgm {
        text_id: 0x1000,
        sub_op: 9,
    });
    host.route_bgm_events(&mut starts).expect("route");
    eprintln!("[ok] starts {:?}, word {:?}", starts.0, host.bgm_track_word);
    assert_eq!(starts.0, vec![2016], "the park sentinel starts no track");
    assert_eq!(
        host.bgm_track_word,
        Some(0x1000),
        "but it is the id the start arm stored"
    );
}
