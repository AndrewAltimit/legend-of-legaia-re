//! Disc-gated: the disc's own player pitch tween (`CC F8 43`) reaches
//! `World::player_draw_tilt`, the word both play hosts compose into the
//! player's draw matrix (the native window's `actor_model`, the browser
//! page's `player_transform` lanes 4 / 5).
//!
//! The census (`engine-scene`'s `height_tween_target_census_disc`) finds the
//! player's `+0x24` tweened at four sites: `rugi` `P2[72]` (to `900` over
//! `110`, then back to `0` over `90`), `uru2` `P2[11]` and `noaru` `P2[34]`.
//! This runs `rugi`'s record the way `play-window --play-record 2:72` does,
//! with the frame the play hosts run (the session tick, the world tail, the
//! effect scene-graph step - the record waits on a flag its op-`0x34`
//! stager's move-VM program clears), and samples the tilt each frame.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or the
//! extracted disc missing.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::scene_actors::bind_scene_actors;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    let env = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    let rel = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    [env, Some(rel)]
        .into_iter()
        .flatten()
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

#[test]
fn rugi_tilts_the_player_and_rights_it() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted disc not found (set LEGAIA_EXTRACTED_DIR)");
        return;
    };
    let cfg = BootConfig {
        scene: "rugi".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open session");
    session
        .enter_field_live("rugi", &FieldLiveOpts::default())
        .expect("enter rugi");
    let binding = bind_scene_actors(&mut session.host);
    let man = session
        .host
        .scene
        .as_ref()
        .and_then(|s| s.field_man_payload(&session.host.index).ok().flatten())
        .expect("rugi carries a MAN");
    let man_file = legaia_scene_formats::man_section::parse(&man).expect("MAN parses");
    assert!(
        session
            .host
            .world
            .install_cutscene_timeline_record(&man_file, &man, 2, 72, false),
        "P2[72] resolves"
    );
    let mut peak = 0u16;
    let mut first = None;
    let mut last = (0, 0);
    for f in 0..1500 {
        session.tick().expect("tick");
        let _ = session.host.world.step_world_frame_tail(
            binding.scene_anm.as_ref(),
            binding.locomotion_anm.as_ref(),
            |slot| binding.clip_targets.get(&slot).copied(),
        );
        session.step_effect_scene_graphs();
        let (pitch, roll) = session.host.world.player_draw_tilt();
        if pitch != 0 {
            first.get_or_insert(f);
        }
        // `900` is under the half turn, so the 12-bit word reads it directly.
        if pitch < 0x800 {
            peak = peak.max(pitch);
        }
        last = (pitch, roll);
    }
    eprintln!(
        "[player tilt] rugi P2[72]: first pitch at frame {first:?}, peak {peak}, final {last:?}"
    );
    assert!(first.is_some(), "the record never tilted the player");
    assert_eq!(peak, 900, "the tween reaches its authored 900");
    assert_eq!(last, (0, 0), "the second tween rights the player");
}
