//! Disc-gated: the kingdom overworld's camera vertical offset lands on
//! retail's value.
//!
//! `_DAT_8007BCAC` (the eased offset every fog particle height is measured
//! against) walks toward `scene_ctrl[+0x4A] - player[+0x16]`. On the
//! overworld the scene control word is written once per entry by `map01`'s
//! own entry script: `P1[0]`'s park loop opens on `CD F8 00 00 7E 7E`, a
//! whole-map box test on the player, and its first pass reaches
//! `2E 18 / 4C 49 3C 00 00 00 / 2F 18` at `+0x1B0`. The entry prologue's
//! `2E 19` has already raised `_DAT_1F800394` bit 25, so the op takes the
//! delta arm (`0x801E1488` tests `0x02000000` before `0x01000000`):
//! `+0x4A = 60` (`sh s0,0x4a(v1)` at `0x801E14BC`) and
//! `_DAT_8007BCAC = 60 - player[+0x16]` (`0x801E14D4`).
//!
//! Retail, both ways: the mednafen `keikoku_chest_preload` state (`map01`,
//! player footing `-192`) holds `+0x4A = 60` and `_DAT_8007BCAC = 252`; a
//! PCSX-Redux run of `drake_castle_to_worldmap` across the Drake Castle ->
//! `map01` entry hits `0x801E14BC` once, on the second mode-3 frame, from
//! `P1[0]` `+0x1B2`, and nothing else stores the word.
//!
//! The port used to read `0 / 192` here: the system context's position
//! anchor was re-seated on the player only in field mode, so on the
//! overworld the box test read tile `(-1, -1)`, failed, and the loop never
//! reached the op.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn gated() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })
}

#[test]
fn map01_entry_script_writes_retails_camera_offset() {
    let Some(extracted) = gated() else { return };
    let scene = "map01".to_string();
    let cfg = BootConfig {
        scene: scene.clone(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("boot session");
    let mode = session
        .enter_world_map_live(&scene, &FieldLiveOpts::default())
        .expect("enter map01");
    assert_eq!(mode, SceneMode::WorldMap);
    // The `keikoku_chest_preload` seat.
    assert!(session.host.world.debug_seat_player(8266, 8700));
    session.camera.zone.arm_arrival();
    let mut written_at = None;
    for f in 0..120 {
        session.host.world.set_pad(0);
        let _ = session.tick();
        if written_at.is_none() && session.host.world.camera.scene_offset != 0 {
            written_at = Some(f);
        }
    }
    let w = &session.host.world;
    let footing = w.camera_ease_player_footing();
    eprintln!(
        "[ok] map01: scene ctrl +0x4A = {} (written at tick {written_at:?}), \
         _DAT_8007BCAC = {}, footing {footing}",
        w.camera.scene_offset, w.camera.offset_ease
    );
    assert!(
        written_at.is_some(),
        "map01 P1[0] never reached its `4C 49` - the park loop's `CD F8` box test is failing"
    );
    assert_eq!(
        w.camera.scene_offset, 60,
        "retail keikoku_chest_preload: scene ctrl +0x4A = 60"
    );
    assert_eq!(footing, -192, "the keikoku seat stands on the -192 floor");
    assert_eq!(
        w.camera.offset_ease, 252,
        "retail keikoku_chest_preload: _DAT_8007BCAC = 252 (60 - -192)"
    );
}
