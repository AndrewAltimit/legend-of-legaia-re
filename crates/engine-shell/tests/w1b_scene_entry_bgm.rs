//! Disc-gated: a cold scene entry sounds the scene's own track.
//!
//! The audio-trace oracle's `.mc` comparand cannot decide this (see
//! `docs/subsystems/audio.md`, "Why `converged` is not the audio oracle's
//! fidelity measure"), and its convergence rule reports the same
//! `NoFrameMatched` row whether the engine played the right track or played
//! nothing at all. This test asks the question the rule cannot: entering a
//! scene from cold the way the playable hosts enter one, does a track start
//! and does it keep playing?
//!
//! Three separate legs have each, on their own, left the engine silent here,
//! and each reads identically from outside:
//!
//! 1. **No field-live entry.** `BootSession::open` only calls `load_scene`,
//!    which installs no field record - the field VM steps nothing and op
//!    `0x35` never executes. Pinned by `voice_mask`: no start, no voices.
//! 2. **No global-pool start hook.** Every real music cue is a global id
//!    (`>= 2000`), so a `BgmDirector` that leaves
//!    [`legaia_engine_core::scene::BgmDirector::start_owned_vab`] on the
//!    trait's no-op default drops the whole field corpus while looking
//!    wired. Pinned by asserting the started id is `>= 2000`.
//! 3. **No free-roam story staging.** `town01`'s entry script starts its
//!    track and then pauses it while flag `0x225` is clear (the opening's
//!    silent dawn); a picker visit never runs the opening records that
//!    repair it, so the track parks. Pinned by `playhead_advanced`: without
//!    the staging call a sequencer attaches and its playhead stays at tick
//!    0 forever, which is why a start-only assertion is not enough.
//!
//! Track identity is checked too, against retail rather than against the
//! port: `town01` must select global `2016`, the id retail's live
//! `_DAT_8007BAC8` holds in the catalogued town01 field save states, and the
//! same id the disc-wide `bgm_scene_resolution` sweep resolves.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or
//! `extracted/` missing.

use std::path::PathBuf;

use legaia_engine_core::scene::BgmDirector;
use legaia_engine_shell::audio_trace_oracle::{AudioTraceBuildOptions, build_engine_audio_trace};

/// One retail second at 60 Hz - the same window the audio-trace oracle uses.
const FRAMES: u64 = 60;

/// The Rim Elm theme: global-pool id `2000 + 16`, sound-test slot 16.
const TOWN01_BGM_ID: u16 = 2016;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn cold_scene_entry_starts_and_keeps_playing_the_scene_track() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    // Every scene here is one a catalogued field save state sits in, so the
    // set is the same corpus the audio-trace oracle walks - minus the save
    // states, which this test does not need.
    for scene in ["town01", "town0c", "keikoku", "map01", "map02", "map03"] {
        let opts = AudioTraceBuildOptions {
            scene: scene.to_string(),
            frames: FRAMES,
            ..Default::default()
        };
        let trace = build_engine_audio_trace(&extracted, None, &opts)
            .unwrap_or_else(|e| panic!("scene {scene:?}: build engine audio trace: {e:#}"));

        let union = trace.iter().fold(0u32, |a, f| a | f.active_voice_mask);
        let peak = trace
            .iter()
            .map(|f| f.active_voice_mask.count_ones())
            .max()
            .unwrap_or(0);
        let playhead = trace
            .iter()
            .filter_map(|f| f.sequencer_playhead_ticks)
            .max()
            .unwrap_or(0);

        eprintln!(
            "[entry-bgm] {scene:<8} voices=0b{union:024b} peak={peak} playhead={playhead} ticks"
        );

        assert!(
            trace.iter().any(|f| f.sequencer_playhead_ticks.is_some()),
            "scene {scene:?}: no sequencer ever attached in {FRAMES} frames - the field VM never \
             reached op 0x35 (is the cold entry running `enter_field_live`?)"
        );
        assert!(
            playhead > 0,
            "scene {scene:?}: a sequencer attached but its playhead never left tick 0 in \
             {FRAMES} frames - the track is parked, which is what an unstaged entry-script pause \
             does (is the cold entry running `seed_free_roam_story_baseline`?)"
        );
        assert!(
            union != 0,
            "scene {scene:?}: the sequencer advanced but keyed no SPU voice - the bank is missing \
             (a global-pool track carries its own VAB through `start_owned_vab`)"
        );
    }
}

/// `town01` selects the track retail selects, not merely *a* track.
///
/// The id is retail's, read off the live BGM-select word in the catalogued
/// town01 field states; the resolver then maps it through the piecewise
/// `music_01` bank map. Asserting the id rather than the PROT entry keeps
/// this a statement about the script's choice, which is what op `0x35`
/// carries.
#[test]
fn town01_cold_entry_selects_the_retail_track() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    /// Records every id that reaches a start hook, and which of the two it
    /// took. Both exist: a director that implements only the first is the
    /// defect this file's leg 2 describes.
    #[derive(Default)]
    struct RecordingDirector {
        /// `(bgm_id, took_the_global_pool_path)`.
        starts: Vec<(u16, bool)>,
    }
    impl BgmDirector for RecordingDirector {
        fn start(&mut self, bgm_id: u16, _seq: &[u8]) {
            self.starts.push((bgm_id, false));
        }
        fn start_owned_vab(&mut self, bgm_id: u16, _entry: &[u8]) {
            self.starts.push((bgm_id, true));
        }
    }

    let cfg = legaia_engine_shell::boot::BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session =
        legaia_engine_shell::boot::BootSession::open(&extracted, &cfg).expect("open town01");
    session.host.world.seed_free_roam_story_baseline("town01");
    session
        .enter_field_live(
            "town01",
            &legaia_engine_shell::boot::FieldLiveOpts::default(),
        )
        .expect("enter town01 live");

    let mut director = RecordingDirector::default();
    for _ in 0..FRAMES {
        session.tick().expect("tick");
        session
            .host
            .route_bgm_events(&mut director)
            .expect("route BGM events");
    }

    eprintln!("[entry-bgm] town01 starts: {:?}", director.starts);
    assert!(
        director.starts.iter().any(|&(id, _)| id == TOWN01_BGM_ID),
        "town01's entry script should select global BGM {TOWN01_BGM_ID} (retail's live \
         BGM-select word in the catalogued town01 states); got {:?}",
        director.starts,
    );
    assert!(
        director
            .starts
            .iter()
            .all(|&(id, owned)| id < 2000 || owned),
        "every id >= 2000 is a global-pool track and must resolve through the owned-VAB hook; \
         got {:?}",
        director.starts,
    );
}
