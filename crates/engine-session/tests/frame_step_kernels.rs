//! The frame steps both play hosts run through the session, at the cadence
//! retail runs them:
//!
//! * the NPC clip players step once per sim tick, under a gate asked on that
//!   tick (`step_npc_clip_players`), so a batch of ticks that crosses a mode
//!   edge advances the clips for the field's ticks only;
//! * the camera's half under the naming prompt runs once per sim step
//!   (`BootSession::pass_name_entry_frames`), as the per-tick arm
//!   (`step_name_entry_frame`) runs it.
//!
//! The clip test is synthetic; the naming-prompt test is disc-gated and
//! skip-passes without `extracted/`.

use std::path::PathBuf;

use legaia_engine_core::world::World;
use legaia_engine_field::field_anim::{FieldClipPlayer, synth_anm_bundle};
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts, step_npc_clip_players};

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Four ticks drained by one display frame, the third of which enters a
/// battle: the clips take the two field ticks and neither battle tick. Asked
/// once at the end of the batch the gate reads `Battle` and the clips take
/// none; asked once at its start they take all four.
#[test]
fn npc_clips_step_on_the_ticks_the_field_owns() {
    let bundle = synth_anm_bundle(&[(1, 8)]);
    let mut players = [(7u8, FieldClipPlayer::from_record(&bundle, 0).expect("clip"))];
    let step = players[0].1.step();
    let mut world = World::new();
    let modes = [
        SceneMode::Field,
        SceneMode::Field,
        SceneMode::Battle,
        SceneMode::Battle,
    ];
    let mut ran = Vec::new();
    for mode in modes {
        world.mode = mode;
        ran.push(step_npc_clip_players(
            &world,
            players.iter_mut().map(|(s, p)| (*s, p)),
        ));
    }
    assert_eq!(ran, [true, true, false, false]);
    assert_eq!(players[0].1.cursor(), 2 * step);

    // The overworld is the field for this purpose.
    world.mode = SceneMode::WorldMap;
    assert!(step_npc_clip_players(
        &world,
        players.iter_mut().map(|(s, p)| (*s, p)),
    ));
    assert_eq!(players[0].1.cursor(), 3 * step);
}

/// The world's display-frame clock and the camera's globals after `steps`
/// frames passed under the prompt.
fn prompt_state(session: &NativeSession) -> (u64, String) {
    (
        session.host.world.clock.display_frames,
        format!("{:?}", session.camera),
    )
}

fn session_on_prompt(extracted: &std::path::Path) -> NativeSession {
    let cfg = BootConfig {
        scene: "town01".to_string(),
        enable_audio: false,
    };
    let mut session = NativeSession::open(extracted, &cfg).expect("open extracted boot session");
    session
        .enter_field_live("town01", &FieldLiveOpts::default())
        .expect("enter town01 live");
    // A few field frames so the camera holds a real shot, then the prompt.
    for _ in 0..8 {
        session.tick().expect("tick");
    }
    session.host.world.open_name_entry(0);
    session
}

#[test]
fn the_naming_prompt_runs_the_camera_half_once_per_sim_step() {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };

    // A redraw that owes no step moves nothing.
    let mut idle = session_on_prompt(&extracted);
    let before = prompt_state(&idle);
    idle.pass_name_entry_frames(0);
    assert_eq!(prompt_state(&idle), before);

    // Three steps passed in one call are three per-tick frames with no edge.
    let mut batched = session_on_prompt(&extracted);
    let mut ticked = session_on_prompt(&extracted);
    assert_eq!(prompt_state(&batched), prompt_state(&ticked));
    batched.pass_name_entry_frames(3);
    for _ in 0..3 {
        assert!(ticked.step_name_entry_frame(0));
    }
    assert_eq!(prompt_state(&batched), prompt_state(&ticked));
    assert_eq!(batched.host.world.clock.display_frames, before.0 + 3);
    assert_eq!(
        batched.host.world.rng_state, ticked.host.world.rng_state,
        "the same draws of the shake stream"
    );

    // No prompt: the call owns nothing.
    let mut closed = session_on_prompt(&extracted);
    closed.host.world.party.name_entry = None;
    let before = prompt_state(&closed);
    closed.pass_name_entry_frames(4);
    assert_eq!(prompt_state(&closed), before);
}
