//! Disc-gated regression: **the camera goes back to the party after Zora's
//! Floating Castle's security doors slam shut, and stays there.**
//!
//! `taiku` P2[4] is the emergency-bulkhead beat: the walk-on band
//! `(115..=117, 35)` spawns it (C1 `[0x384, 0x389, 0x53B]`). It stages the
//! door shot with one op-`0x45` CONFIGURE (`apply 40`), closes the three
//! bulkheads, and releases the shot with `45 C0 5A 00` - APPLY with a
//! 90-frame glide - then waits the glide out (`4C CD`) and ends.
//!
//! Retail's APPLY arm composes the player's zone shot into the very staging
//! block the CONFIGURE wrote (`jal 0x801DAB90`, `a1 = 0x801C6EA8`, at
//! `0x801DF228`) before committing it (`jal 0x801DE084` at `0x801DF27C`), so
//! the scripted shot is gone the moment APPLY runs. The port kept its mirror
//! of the staged slots (`World::camera.state.params`) until the next scene
//! entry, and `camera_view::cutscene_owns_camera` went on handing the frame
//! to the frozen door shot: through the release glide, and then again on
//! every later record with no camera beat at all - the bulkhead toggles
//! P2[0] / P2[1] a few tiles west among them.
//!
//! The test resumes the ladder's Zora Castle card save (`PRO-14`, whose
//! `0x389` is cleared so the beat can fire again) onto `taiku`.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` / the save library.

use legaia_engine_save_state::WorldSaveState;
use std::path::{Path, PathBuf};

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;
use legaia_engine_world_views::camera_view::cutscene_owns_camera;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let d = std::env::var_os("LEGAIA_EXTRACTED_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("extracted"));
    if d.join("PROT.DAT").is_file() {
        Some(d)
    } else {
        eprintln!("[skip] extracted/ missing - run legaia-extract first");
        None
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The SC block of the save named `save` on the library card `card`.
fn card_sc(lib: &Path, card: &str, save: &str) -> Option<Vec<u8>> {
    let mounted = legaia_save::emu::MountedCard::open(&lib.join("cards").join(card)).ok()?;
    (1..=15u8).find_map(|block| {
        let frame = mounted.dir_frame(block)?;
        let name: String = frame[0x0A..0x0A + 20]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();
        if !name.ends_with(save) {
            return None;
        }
        mounted.sc_block(block).map(<[u8]>::to_vec)
    })
}

fn player_xz(session: &NativeSession) -> (i32, i32) {
    let w = &session.host.world;
    let a = &w.actors[w.player_actor_slot.expect("player slot") as usize];
    (
        i32::from(a.move_state.world_x),
        i32::from(a.move_state.world_z),
    )
}

fn focus_xz(session: &NativeSession) -> (i32, i32) {
    let f = session.camera.globals.focus_world();
    (f[0], f[2])
}

fn seat(session: &mut NativeSession, tx: i32, tz: i32) {
    let w = &mut session.host.world;
    let slot = w.player_actor_slot.expect("player slot") as usize;
    w.actors[slot].move_state.world_x = (tx * 128 + 64) as i16;
    w.actors[slot].move_state.world_z = (tz * 128 + 64) as i16;
}

fn step(session: &mut NativeSession, pad: u16) {
    session.host.world.set_pad(pad);
    let _ = session.tick();
}

/// `PRO-14` resumed onto `taiku`, the player two tiles south of the beat's
/// walk-on band.
fn at_the_bulkheads() -> Option<NativeSession> {
    let extracted = extracted_dir()?;
    let lib = std::env::var_os("LEGAIA_SAVES_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("saves/library"));
    let Some(sc) = card_sc(&lib, "playthrough-ladder-pro00-14.mcr", "PRO-14") else {
        eprintln!("[skip] save library card PRO-14 missing");
        return None;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let sf = legaia_save::SaveFile::from_retail_sc_block(&sc, legaia_save::RETAIL_SC_PARTY_RECORDS)
        .expect("lift SC block");
    session.host.world.load_full(sf.clone());
    let opts = FieldLiveOpts {
        live_loop: true,
        player_battle: false,
        battle_bgm: None,
    };
    let landing = session.resume_save(sf, "taiku", &opts);
    assert!(landing.entered_scene(), "resume onto taiku: {landing:?}");
    for _ in 0..60 {
        step(&mut session, 0);
    }
    // The card is past the beat; drop the one C1 flag it carries.
    session.host.world.system_flag_clear(0x389);
    assert!(!session.host.world.system_flag_test(0x384));
    assert!(!session.host.world.system_flag_test(0x53B));
    seat(&mut session, 116, 37);
    for _ in 0..30 {
        step(&mut session, 0);
    }
    assert_eq!(focus_xz(&session), player_xz(&session), "settled follow");
    Some(session)
}

/// Walk north onto the band and play the beat out.
fn play_the_beat(session: &mut NativeSession) {
    let mut entered = false;
    for _ in 0..120 {
        step(session, PadButton::Down.mask());
        if session.host.world.cutscene_timeline_active() {
            entered = true;
            break;
        }
    }
    assert!(entered, "walking onto (116, 35) spawns P2[4]");
    let mut owned = false;
    let mut frames = 0;
    while session.host.world.cutscene_timeline_active() {
        assert!(frames < 2000, "P2[4] never ended");
        owned |= cutscene_owns_camera(&session.host.world);
        step(session, 0);
        frames += 1;
    }
    assert!(owned, "the door shot owns the frame during the beat");
    assert!(session.host.world.system_flag_test(0x384), "the beat ran");
}

#[test]
fn the_security_door_beat_hands_the_camera_back_to_the_party() {
    let Some(mut session) = at_the_bulkheads() else {
        return;
    };
    play_the_beat(&mut session);
    // The release left no scripted shot behind.
    assert!(
        session.host.world.camera.state.params.is_empty(),
        "APPLY dropped the staged door shot"
    );
    assert_eq!(
        focus_xz(&session),
        player_xz(&session),
        "the release glide landed on the player"
    );
    // The camera follows a walk.
    for _ in 0..40 {
        step(&mut session, PadButton::Down.mask());
        assert!(!cutscene_owns_camera(&session.host.world));
        assert_eq!(focus_xz(&session), player_xz(&session), "follows the walk");
    }
    eprintln!("[ok] taiku P2[4] hands the camera back");
}

/// A camera-less record after the beat - the P2[0] / P2[1] bulkhead toggles
/// at `(110, 32)` / `(110, 34)` - keeps the follow camera: retail frames it
/// on the globals, which the field camera put on the player.
#[test]
fn a_camera_less_record_after_the_beat_keeps_the_follow_camera() {
    let Some(mut session) = at_the_bulkheads() else {
        return;
    };
    play_the_beat(&mut session);
    let mut fired = 0;
    for (tx, tz) in [(111, 34), (110, 34), (111, 34), (111, 32), (110, 32)] {
        seat(&mut session, tx, tz);
        for _ in 0..20 {
            step(&mut session, 0);
            let w = &session.host.world;
            if w.cutscene_timeline_active() {
                fired += 1;
            }
            assert!(
                !cutscene_owns_camera(w),
                "({tx}, {tz}): a record with no camera beat must not frame the door shot"
            );
        }
    }
    assert!(fired > 0, "the bulkhead toggles ran");
    eprintln!("[ok] taiku P2[0] / P2[1] keep the follow camera");
}
