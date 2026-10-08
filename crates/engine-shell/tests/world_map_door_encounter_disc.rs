//! Disc-gated regression: **a step onto an overworld door does not also roll
//! a random encounter.**
//!
//! Retail's region roll `FUN_801D9E1C` sits out a step while the player
//! carries the engaged bit (`*(0x8007C364)+0x10 & 0x80000`), while the
//! dialogue-pacing countdown `_DAT_8007B6B4` runs, and while the kind-0 warp
//! timer `_DAT_8007B6B0` is up (`0x801DA130..0x801DA164`) - after the region's
//! battle-setup half, before the rate scale, so the step counter does not
//! drain either. A door crossing raises the engaged bit (the script runner
//! `FUN_80039B7C` raises it on every frame it steps the door's record) or arms
//! the warp timer, so the step that enters a door never starts a fight.
//!
//! The reader also raises the engaged bit on the trigger itself
//! (`0x801DA2C0..0x801DA2D8`), so a step that rolls a fight stops the
//! player where it stands for the battle intro.
//!
//! The port did neither on the overworld: a step that rolled a fight one
//! tile short of `map01`'s door to `dolk2` kept walking through the intro's
//! 132 frames, onto the door, and the scene change the door queued either
//! dropped the fight or tore it down a few dozen ticks in - no victory, no
//! flee, the party standing in `dolk2`. The full-game ladder's pad tier
//! read that as a detour on the way to `jou`.
//!
//! Both tests resume the playthrough card's `PRO-04` save onto `map01`
//! beside the door with the region counter about to run out.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` / the save library.

use std::path::{Path, PathBuf};

use legaia_engine_core::input::PadButton;
use legaia_engine_core::world::{SceneMode, world_map_camera_relative_bits};
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};

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

/// The `map01` door to `dolk2` the ladder walks onto.
const DOOR_TILE: (i32, i32) = (54, 71);

fn player_tile(session: &BootSession) -> (i32, i32) {
    let w = &session.host.world;
    let a = &w.actors[w.player_actor_slot.expect("player slot") as usize];
    (
        i32::from(a.move_state.world_x) >> 7,
        i32::from(a.move_state.world_z) >> 7,
    )
}

/// The pad direction that walks world `(dx, dz)` (one axis) under the live
/// camera.
fn pad_toward(session: &BootSession, dx: i32, dz: i32) -> u16 {
    let want = match (dx.signum(), dz.signum()) {
        (1, _) => 0x2000,
        (-1, _) => 0x8000,
        (_, 1) => 0x1000,
        _ => 0x4000,
    };
    let az = session
        .host
        .world
        .world_map
        .ctrl
        .as_ref()
        .map_or(0, |c| c.azimuth);
    [
        (PadButton::Up, 0, 1),
        (PadButton::Down, 0, -1),
        (PadButton::Right, 1, 0),
        (PadButton::Left, -1, 0),
    ]
    .into_iter()
    .find(|&(_, sx, sy)| world_map_camera_relative_bits(az, sx, sy) == want)
    .map_or(PadButton::Up.mask(), |(b, _, _)| b.mask())
}

/// Resume `PRO-04` onto `map01` with the player seated on `seat`.
fn seated(seat: (i32, i32)) -> Option<BootSession> {
    let extracted = extracted_dir()?;
    let lib = std::env::var_os("LEGAIA_SAVES_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("saves/library"));
    let Some(sc) = card_sc(&lib, "playthrough-endgame-7saves.mcr", "PRO-04") else {
        eprintln!("[skip] save library card PRO-04 missing");
        return None;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut session = BootSession::open(&extracted, &cfg).expect("open boot session");
    let sf = legaia_save::SaveFile::from_retail_sc_block(&sc, legaia_save::RETAIL_SC_PARTY_RECORDS)
        .expect("lift SC block");
    session.host.world.load_full(sf.clone());
    session
        .host
        .set_entry_seat((seat.0 * 128 + 64) as i16, (seat.1 * 128 + 64) as i16);
    let opts = FieldLiveOpts {
        live_loop: true,
        player_battle: true,
        battle_bgm: None,
    };
    let landing = session.resume_save(sf, "map01", &opts);
    assert!(landing.entered_scene(), "resume onto map01: {landing:?}");
    for _ in 0..60 {
        session.host.world.set_pad(0);
        let _ = session.tick();
    }
    assert_eq!(player_tile(&session), seat, "seated beside the door");
    Some(session)
}

/// A step that rolls a fight one tile short of the door holds the player
/// there, and the fight opens and stays open in `map01`.
#[test]
fn a_rolled_step_beside_an_overworld_door_stops_the_walk() {
    let Some(mut session) = seated((DOOR_TILE.0 - 1, DOOR_TILE.1 + 1)) else {
        return;
    };
    let below = (DOOR_TILE.0, DOOR_TILE.1 + 1);
    let mut rolled_at = None;
    for f in 0..600 {
        if rolled_at.is_none()
            && let Some(t) = session.host.world.world_map.region_tracker.as_mut()
        {
            t.set_counter(1);
        }
        // East onto the tile below the door, then north into it.
        let t = player_tile(&session);
        let pad = if t.0 < DOOR_TILE.0 {
            pad_toward(&session, 1, 0)
        } else {
            pad_toward(&session, 0, -1)
        };
        session.host.world.set_pad(pad);
        let _ = session.tick();
        if rolled_at.is_none() && session.host.world.encounter_owns_player() {
            rolled_at = Some(f);
            assert_eq!(
                player_tile(&session),
                below,
                "the roll is the step below the door"
            );
        }
        if session.host.world.mode == SceneMode::Battle {
            break;
        }
        assert_eq!(
            session.host.world.active_scene_label, "map01",
            "the walk left map01 at frame {f} before its rolled fight opened"
        );
        if rolled_at.is_some() {
            assert_eq!(
                player_tile(&session),
                below,
                "the player walked on through the intro"
            );
        }
    }
    assert!(rolled_at.is_some(), "the armed counter rolled no fight");
    assert_eq!(
        session.host.world.mode,
        SceneMode::Battle,
        "the rolled fight opened"
    );
    for t in 0..300 {
        session.host.world.set_pad(0);
        let _ = session.tick();
        assert_eq!(
            session.host.world.mode,
            SceneMode::Battle,
            "the fight was torn down {t} ticks in"
        );
    }
    assert_eq!(session.host.world.active_scene_label, "map01");
}

/// The step onto the door itself rolls nothing, however spent the counter:
/// the door's record holds the player and the party lands in `dolk2`.
#[test]
fn stepping_onto_an_overworld_door_rolls_no_encounter() {
    let Some(mut session) = seated((DOOR_TILE.0, DOOR_TILE.1 + 1)) else {
        return;
    };
    let mut entered = None;
    for f in 0..600 {
        if let Some(t) = session.host.world.world_map.region_tracker.as_mut() {
            t.set_counter(1);
        }
        let pad = pad_toward(&session, 0, -1);
        session.host.world.set_pad(pad);
        let _ = session.tick();
        assert!(
            !session.host.world.encounter_owns_player()
                && session.host.world.mode != SceneMode::Battle,
            "frame {f}: the door step rolled a fight at {:?}",
            player_tile(&session)
        );
        if session.host.world.active_scene_label != "map01" {
            entered = Some(session.host.world.active_scene_label.clone());
            break;
        }
    }
    assert_eq!(
        entered.as_deref(),
        Some("dolk2"),
        "walking north onto {DOOR_TILE:?} enters dolk2"
    );
}
