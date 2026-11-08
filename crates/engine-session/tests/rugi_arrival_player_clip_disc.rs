//! Disc-gated: a cutscene's `4C 51` seat on the **player** binds the scene
//! clip its depth byte names, and the player then plays it - not the
//! locomotion pair.
//!
//! The arm writes the party-bank bit from the depth byte's bit 7 on whichever
//! actor it resolved (`0x801E1828..0x801E1858`) before the identity compare
//! that splits the player arm off (`0x801E1954`), so a player seat with the
//! bit clear picks scene record `move_id - 1`. `rugi` `P2[72]`, the party
//! flying into Rogue's Tower, seats Vahn with `CC F8 51 22 52 04 07` beside
//! Noa (`CC 0D 51 .. 04 21`) and Gala (`CC 0E 51 .. 04 10`), and restores the
//! bit with `B1 F8 18` before `A2 F8 02` lands him on idle. The port wrote the
//! bit on the companions only, so Vahn's clip 7 strode into his locomotion
//! bank, and the landing drift (a `C1 F8` leg while his height is held off
//! the floor) started the port's motion-derived walk.
//!
//! The sweep runs every shipped player seat whose depth byte clears the bit
//! (the `4C 51` census rows on target `0xF8`) and checks each binds its scene
//! record with a rig the player's skeleton takes.
//!
//! Driven through a `NativeSession` with the per-frame tail the play hosts
//! run, the same player clip path the native window and the browser page
//! share. Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN`
//! unset or the extracted disc missing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::scene_actors::{SceneActorBinding, bind_scene_actors};
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

fn extracted_dir() -> Option<PathBuf> {
    let env = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    let rel = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    [env, Some(rel)]
        .into_iter()
        .flatten()
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

/// What the player's clip is doing after a frame.
#[derive(Debug, Clone)]
struct Sample {
    source: &'static str,
    id: usize,
    frames: usize,
    clip: i16,
    party_bank: bool,
    moved: bool,
}

/// Run `scene`'s partition-2 record `record` from scene entry for `frames`
/// host frames (Cross tapped to page any dialogue), sampling the player's
/// clip each frame. `None` on a skip.
fn run_record(scene: &str, record: usize, frames: u32) -> Option<Vec<Sample>> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted disc not found (set LEGAIA_EXTRACTED_DIR)");
        return None;
    };
    let cfg = BootConfig {
        scene: scene.into(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(&extracted, &cfg).expect("open session");
    session.host.world.set_active_party(vec![0, 1, 2]);
    session
        .enter_field_live(scene, &FieldLiveOpts::default())
        .expect("enter scene");
    let mut binding = bind_scene_actors(&mut session.host);
    session.set_host_drains_queues(true);
    let man = session
        .host
        .scene
        .as_ref()
        .and_then(|s| s.field_man_payload(&session.host.index).ok().flatten())
        .expect("the scene carries a MAN");
    let man_file = legaia_scene_formats::man_section::parse(&man).expect("MAN parses");
    assert!(
        session
            .host
            .world
            .install_cutscene_timeline_record(&man_file, &man, 2, record, false),
        "{scene} P2[{record}] resolves"
    );
    let mut out = Vec::new();
    let mut last_pos = None;
    for f in 0..frames {
        if session.host.world.take_player_rig_change() {
            binding = bind_scene_actors(&mut session.host);
        }
        session.host.world.set_pad(if f % 8 < 2 {
            PadButton::Cross.mask()
        } else {
            0
        });
        frame(&mut session, &binding);
        let w = &session.host.world;
        let pos = w
            .player_actor_slot
            .and_then(|s| w.actors.get(s as usize))
            .map(|a| (a.move_state.world_x, a.move_state.world_z));
        let (source, id, _, frames) = w
            .locomotion
            .player_anim
            .as_ref()
            .expect("the binding installed the player's clips")
            .playing_clip();
        out.push(Sample {
            source,
            id,
            frames,
            clip: w.locomotion.player_clip,
            party_bank: w.locomotion.player_party_bank,
            moved: last_pos.is_some() && last_pos != pos,
        });
        last_pos = pos;
    }
    Some(out)
}

/// One host frame: the session tick, then the world tail with the banks and
/// the VRAM-effects step whose move-VM parts clear the waits `rugi`'s record
/// parks on (`4C C6 0A 00`).
fn frame(session: &mut NativeSession, binding: &SceneActorBinding) {
    session.tick().expect("tick");
    let _ = session.host.world.step_world_frame_tail(
        binding.scene_anm.as_ref(),
        binding.locomotion_anm.as_ref(),
        |slot| binding.clip_targets.get(&slot).copied(),
    );
    if let Some(res) = session.host.resources.as_mut() {
        let _ = session
            .host
            .world
            .step_field_vram_effects(&mut res.vram, false);
    }
}

/// `rugi` `P2[72]`: Vahn flies in on scene clip 7 (record 6) for the whole
/// flight, then lands on the leader's idle - never the locomotion bank's
/// landing slot and never the motion-derived walk.
#[test]
fn rugi_flies_vahn_in_on_his_scene_clip() {
    let Some(run) = run_record("rugi", 72, 600) else {
        return;
    };
    let flight: Vec<&Sample> = run
        .iter()
        .filter(|s| s.clip == 7)
        .skip(1) // the bind tick, before the host resolves the scene clip
        .collect();
    assert!(
        flight.len() > 300,
        "the seat picked clip 7 ({} frames)",
        flight.len()
    );
    assert!(
        flight
            .iter()
            .all(|s| !s.party_bank && s.source == "scene" && s.id == 6 && s.frames == 15),
        "the flight plays scene record 6: {:?}",
        flight.iter().find(|s| s.source != "scene" || s.id != 6)
    );
    assert!(
        flight.iter().any(|s| s.moved),
        "the record carries the player during the flight"
    );
    let landing: Vec<&Sample> = run
        .iter()
        .skip_while(|s| s.clip != 7)
        .skip_while(|s| s.clip == 7)
        .collect();
    assert!(
        landing.iter().any(|s| s.moved),
        "the landing drifts the party forward"
    );
    assert!(
        landing
            .iter()
            .all(|s| s.party_bank && s.source == "bank" && s.id == 1),
        "the landing stands on the idle the script names: {:?}",
        landing.iter().find(|s| s.source != "bank" || s.id != 1)
    );
}

/// Every shipped player seat with the depth byte's bit 7 clear binds scene
/// record `move_id - 1`, and the player's rig takes it (a record of another
/// skeleton is refused and the motion pair plays instead).
#[test]
fn every_scene_bank_player_seat_plays_its_scene_record() {
    // (scene, P2 record, move id) of every census row the record reaches
    // from a bare install.
    const SITES: &[(&str, usize, i16)] = &[
        ("chitei2", 12, 0x0D),
        ("chitei2", 14, 0x0D),
        ("deroa", 9, 0x11),
        ("dohaty", 12, 0x05),
        ("garmel", 14, 0x0E),
        ("jagaroom", 9, 0x02),
        ("koin1", 10, 0x03),
        ("map03", 14, 0x0C),
        ("nilboa", 23, 0x03),
        ("nilboa", 24, 0x03),
        ("nilboa", 25, 0x03),
        ("noaru", 34, 0x0B),
        ("rugi", 72, 0x07),
        ("son", 5, 0x04),
        ("son", 7, 0x04),
        ("taiku", 12, 0x12),
        ("taiku", 13, 0x16),
        ("taiku", 14, 0x16),
        ("taiku", 17, 0x16),
        ("taiku", 18, 0x16),
        ("teien", 2, 0x01),
        ("town0b", 23, 0x3B),
        ("town0b", 30, 0x3E),
        ("town0c", 29, 0x3C),
        ("town0d", 32, 0x0B),
    ];
    for &(scene, record, move_id) in SITES {
        let Some(run) = run_record(scene, record, 3000) else {
            return;
        };
        let mut seen: BTreeMap<(&str, usize), u32> = BTreeMap::new();
        // The bind tick and the host's resolve window: a seat made on the
        // record's first frames (`teien`) has no earlier pose to hold.
        for s in run
            .iter()
            .filter(|s| s.clip == move_id && !s.party_bank)
            .skip(1 + usize::from(legaia_engine_field::field_anim::SCENE_RESOLVE_HOLD))
        {
            *seen.entry((s.source, s.id)).or_default() += 1;
        }
        eprintln!("{scene} P2[{record}] clip {move_id}: {seen:?}");
        assert!(!seen.is_empty(), "{scene} P2[{record}] reaches its seat");
        let want = (move_id - 1) as usize;
        assert!(
            seen.keys()
                .all(|&(src, id)| (src == "scene" || src == "held") && id == want),
            "{scene} P2[{record}] plays scene record {want}: {seen:?}"
        );
    }
}
