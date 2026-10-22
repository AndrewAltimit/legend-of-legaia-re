//! Disc-gated regression: **the overworld camera is back on the player on
//! the first frame after a battle, whether the party won or ran.**
//!
//! Retail leaves a battle through MAIN INIT back into field mode 3, and the
//! field init `FUN_801D6704` runs the camera reset `FUN_80025C24`
//! unconditionally (`0x801D698C`) and, on the back-from-battle word
//! `_DAT_8007B8B8 == 2`, re-centres the window on the saved player tile
//! (`0x801D6E0C..0x801D6ECC`, `FUN_80017DD4`). The port keeps the scene
//! resident across the fight and reported no scene entry, so the follow
//! camera kept the focus it had pinned onto the party's battle seat through
//! the fight - clamped by the region box to the corner of the Drake Kingdom
//! map - and the overworld's standing-player ease held it there until the
//! player moved.
//!
//! Both tests resume the playthrough card's `PRO-04` save onto `map01`, roll
//! a fight one step east, and either run (the latched pre-emptive advantage
//! makes the escape roll certain) or wipe the monsters.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` / the save library.

use legaia_engine_save_state::WorldSaveState;
use std::path::{Path, PathBuf};

use legaia_engine_core::world::world_map_camera_relative_bits;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

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

fn player_tile(session: &NativeSession) -> (i32, i32) {
    let w = &session.host.world;
    let a = &w.actors[w.player_actor_slot.expect("player slot") as usize];
    (
        i32::from(a.move_state.world_x) >> 7,
        i32::from(a.move_state.world_z) >> 7,
    )
}

/// The pad direction that walks world `(dx, dz)` (one axis) under the live
/// camera.
fn pad_toward(session: &NativeSession, dx: i32, dz: i32) -> u16 {
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
fn seated(seat: (i32, i32)) -> Option<NativeSession> {
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
    let mut session = NativeSession::open(&extracted, &cfg).expect("open boot session");
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

/// Walk east from the seat with the region counter at 1 until the fight
/// opens, then wait for the round prompt.
fn into_battle(session: &mut NativeSession) {
    for _ in 0..600 {
        if let Some(t) = session.host.world.world_map.region_tracker.as_mut() {
            t.set_counter(1);
        }
        let pad = pad_toward(session, 1, 0);
        session.host.world.set_pad(pad);
        let _ = session.tick();
        if session.host.world.mode == SceneMode::Battle {
            break;
        }
    }
    assert_eq!(
        session.host.world.mode,
        SceneMode::Battle,
        "no fight opened"
    );
    for _ in 0..2000 {
        if session.host.world.battle.command.is_some() {
            return;
        }
        session.host.world.set_pad(0);
        let _ = session.tick();
    }
    panic!("no round prompt");
}

fn idle(session: &mut NativeSession) {
    session.host.world.set_pad(0);
    let _ = session.tick();
}

/// The player's world `(x, z)`.
fn player_xz(session: &NativeSession) -> (i32, i32) {
    let w = &session.host.world;
    let a = &w.actors[w.player_actor_slot.expect("player slot") as usize];
    (
        i32::from(a.move_state.world_x),
        i32::from(a.move_state.world_z),
    )
}

/// The camera focus as a world `(x, z)`.
fn focus_xz(session: &NativeSession) -> (i32, i32) {
    let f = session.camera.globals.focus_world();
    (f[0], f[2])
}

/// Run `step` once per tick until the fight is over, then assert the camera
/// targets the standing player on the return frame and the frames after.
fn assert_return_reseats(
    session: &mut NativeSession,
    label: &str,
    mut step: impl FnMut(&mut NativeSession, u32),
) {
    let mut f = 0;
    while session.host.world.mode == SceneMode::Battle {
        assert!(f < 6000, "{label}: the fight never ended");
        step(session, f);
        f += 1;
    }
    assert_eq!(session.host.world.mode, SceneMode::WorldMap, "{label}");
    for frame in 0..5 {
        let (p, c) = (player_xz(session), focus_xz(session));
        eprintln!("[{label}] return frame {frame}: player {p:?} focus {c:?}");
        assert_eq!(
            c, p,
            "{label}: return frame {frame} - the camera is not on the standing player"
        );
        idle(session);
    }
}

#[test]
fn escaping_an_overworld_fight_reseats_the_camera_on_the_player() {
    let Some(mut session) = seated((DOOR_TILE.0 - 1, DOOR_TILE.1 + 1)) else {
        return;
    };
    assert_eq!(focus_xz(&session), player_xz(&session), "seated focus");
    into_battle(&mut session);
    session.host.world.set_battle_formation_latched(
        legaia_engine_battle_vm::battle_formulas::FormationAdvantage::Preemptive,
    );
    for _ in 0..64 {
        if !session.host.world.battle_command_pad_held() {
            break;
        }
        idle(&mut session);
    }
    // Round prompt: Circle takes Run without a confirm (retail state 0x1E).
    session.host.world.set_pad(PadButton::Circle.mask());
    let _ = session.tick();
    assert_return_reseats(&mut session, "escape", |s, _| idle(s));
}

#[test]
fn winning_an_overworld_fight_reseats_the_camera_on_the_player() {
    let Some(mut session) = seated((DOOR_TILE.0 - 1, DOOR_TILE.1 + 1)) else {
        return;
    };
    into_battle(&mut session);
    // Hold every monster at zero HP and mash Circle through the round prompt
    // until the wipe scan ends the fight.
    assert_return_reseats(&mut session, "victory", |s, f| {
        let w = &mut s.host.world;
        let n = usize::from(w.party.party_count);
        for a in w.actors.iter_mut().skip(n) {
            if a.battle_monster_id.is_some() {
                a.battle.hp = 0;
                a.battle.liveness = 0;
            }
        }
        let pad = if f % 2 == 0 {
            PadButton::Circle.mask()
        } else {
            0
        };
        w.set_pad(pad);
        let _ = s.tick();
    });
    assert!(
        session.host.world.battle.last_rewards.is_some(),
        "the fight ended in a win"
    );
}
