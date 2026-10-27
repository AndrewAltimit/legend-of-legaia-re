//! Disc-gated: the disc's own player pitch tween (`CC F8 43`) reaches
//! `World::player_draw_tilt`, the word both play hosts compose into the
//! player's draw matrix (the native window's `actor_model`, the browser
//! page's `player_transform` lanes 4 / 5).
//!
//! The census (`engine-scene`'s `height_tween_target_census_disc`) finds the
//! player's `+0x24` tweened at four sites: `rugi` `P2[72]` (to `900` over
//! `110`, then back to `0` over `90`), `uru2` `P2[11]` and `noaru` `P2[34]`.
//! This runs `rugi`'s record the way `play-window --play-record 2:72` does
//! and samples the tilt each frame, through two drivers that must agree: the
//! session tick (`BootSession::tick`, which runs the world frame tail for a
//! caller that does not), and the scene host's tick followed by its headless
//! tail (`SceneHost::step_headless_frame_tail`).
//!
//! The tail is load-bearing. The record sets system flag `0x0A` (`50 0A` at
//! pc `258`) and parks on `4C C6 0A 00` at pc `260` until a part staged by
//! its op-`0x34` records (`B4 F8` / `B4 0D` / `B4 0E`) clears it. Those parts
//! tick in the tail's VRAM-effects step (`World::step_field_vram_effects` ->
//! `step_ambient_fx`); dropping that step parks the record at pc `260` for
//! good, and dropping the effect scene-graph step does not. `SceneHost::tick`
//! alone runs neither, by design - the same shape as `garmel` P2[12], which
//! `step_headless_frame_tail` documents.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or the
//! extracted disc missing.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    let env = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    let rel = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    [env, Some(rel)]
        .into_iter()
        .flatten()
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

/// The tilt trace, one `(pitch, roll)` per frame, of `rugi` `P2[72]` driven
/// by `session_tick` (`true`) or by `SceneHost::tick` and its headless tail.
fn tilt_trace(extracted: &std::path::Path, session_tick: bool) -> Vec<(u16, u16)> {
    let cfg = BootConfig {
        scene: "rugi".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(extracted, &cfg).expect("open session");
    session
        .enter_field_live("rugi", &FieldLiveOpts::default())
        .expect("enter rugi");
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
    (0..1500)
        .map(|_| {
            if session_tick {
                session.tick().expect("tick");
            } else {
                session.host.tick().expect("tick");
                let _ = session.host.step_headless_frame_tail();
            }
            session.host.world.player_draw_tilt()
        })
        .collect()
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
    let trace = tilt_trace(&extracted, true);
    let first = trace.iter().position(|&(pitch, _)| pitch != 0);
    // `900` is under the half turn, so the 12-bit word reads it directly.
    let peak = trace
        .iter()
        .map(|&(pitch, _)| pitch)
        .filter(|&p| p < 0x800)
        .max()
        .unwrap_or(0);
    let last = *trace.last().unwrap();
    eprintln!(
        "[player tilt] rugi P2[72]: first pitch at frame {first:?}, peak {peak}, final {last:?}"
    );
    assert!(first.is_some(), "the record never tilted the player");
    assert_eq!(peak, 900, "the tween reaches its authored 900");
    assert_eq!(last, (0, 0), "the second tween rights the player");
    assert!(
        tilt_trace(&extracted, false) == trace,
        "SceneHost::tick plus its headless tail runs the record as the session does"
    );
}
