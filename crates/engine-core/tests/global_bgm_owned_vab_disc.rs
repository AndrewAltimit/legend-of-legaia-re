//! Disc-gated: the global-pool BGM path (`bgm_id >= 2000`) resolves a
//! `music_01` bank entry and routes it through the director's owned-VAB hook.
//!
//! Every real music cue in the game is a global-pool id (field / battle /
//! minigame); the scene-local resolver [`SceneHost::bgm_seq_bytes`] returns
//! `None` for those, so before this path they never reached the director. Here
//! we pin: (1) `music_bank_entry_bytes` resolves a global id to its own
//! `[VAB][SEQ]` bank entry, (2) `route_bgm_events` dispatches a global start
//! through `start_owned_vab` (not the scene-local `start`), and (3) scene-local
//! and out-of-bank ids still resolve as before.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset (CI has no Sony disc).

use legaia_engine_core::field_events::FieldEvent;
use legaia_engine_core::scene::{BgmDirector, SceneHost};

/// Records which director hook each dispatched event hit.
#[derive(Default)]
struct RecordingBgm {
    log: Vec<String>,
}
impl BgmDirector for RecordingBgm {
    fn start(&mut self, id: u16, bytes: &[u8]) {
        self.log.push(format!("start({id},{})", bytes.len()));
    }
    fn start_owned_vab(&mut self, id: u16, bytes: &[u8]) {
        self.log.push(format!("owned({id},{})", bytes.len()));
    }
    fn stop(&mut self) {
        self.log.push("stop".into());
    }
}

fn host() -> Option<SceneHost> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    SceneHost::open_disc(&disc).ok()
}

#[test]
fn global_bgm_id_resolves_its_own_music_bank_entry() {
    let Some(host) = host() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // 2016 = music_01 slot 16 = M14B (the Rim Elm theme town01 starts).
    let entry = host
        .music_bank_entry_bytes(2016)
        .expect("read music bank entry")
        .expect("global id 2016 resolves");
    assert!(
        entry.windows(4).any(|w| w == b"pBAV"),
        "the bank entry carries its own VAB"
    );
    assert!(
        entry.windows(4).any(|w| w == b"pQES"),
        "the bank entry carries a SEQ score"
    );

    // Scene-local ids and out-of-bank ids don't resolve through the global path.
    assert!(host.music_bank_entry_bytes(5).unwrap().is_none());
    assert!(host.music_bank_entry_bytes(1999).unwrap().is_none());
    // Past the 82-slot bank (2000..=2081): none.
    assert!(host.music_bank_entry_bytes(2082).unwrap().is_none());
}

#[test]
fn route_bgm_events_dispatches_global_start_through_owned_vab() {
    let Some(mut host) = host() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    host.load_scene("town01").expect("load town01");

    // A global start (sub-op 1) must reach the owned-VAB hook with the real
    // bank-entry bytes, not the scene-local `start`.
    host.world.pending_field_events.push(FieldEvent::Bgm {
        text_id: 2016,
        sub_op: 1,
    });
    let mut rec = RecordingBgm::default();
    let acted = host.route_bgm_events(&mut rec).expect("route");
    assert_eq!(acted, 1, "the global start was acted on");
    assert_eq!(rec.log.len(), 1, "exactly one dispatch: {:?}", rec.log);
    assert!(
        rec.log[0].starts_with("owned(2016,"),
        "global start routed through start_owned_vab, got {:?}",
        rec.log
    );
    // The owned entry bytes are non-trivial (the whole [VAB][SEQ] pair).
    let len: usize = rec.log[0]
        .trim_start_matches("owned(2016,")
        .trim_end_matches(')')
        .parse()
        .unwrap();
    assert!(len > 1000, "owned entry is the full bank record, got {len}");

    // A control sub-op still routes normally.
    host.world.pending_field_events.push(FieldEvent::Bgm {
        text_id: 2016,
        sub_op: 4,
    });
    let mut rec2 = RecordingBgm::default();
    host.route_bgm_events(&mut rec2).expect("route stop");
    assert_eq!(rec2.log, vec!["stop".to_string()]);
}
