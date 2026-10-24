//! Disc-gated: a clip retail plays **once** on the player plays once and
//! holds its last frame in the port - it does not loop.
//!
//! Retail's clip tick `FUN_800204F8` reads the actor's `+0x62` control word:
//! with the clamp bit `0x08` up the cursor stops on the clip's last position
//! and the end latch `0x100` stays raised (`0x80020700..0x8002072C`); clear,
//! it wraps to frame 0. Two shipped paths raise the clamp on the player:
//!
//! - **a cutscene's one-shot gesture**: `cave01` `P2[2]`, Noa's introduction
//!   beside Terra, runs `AB F8 03` (clamp), `A2 F8 1D` (scene clip 29, the
//!   120-frame getting-up), dialogue, then `AC F8 08` / `AD F8 08` (wait for
//!   the latch) and `A2 F8 03`. The port played the clip once from a queue
//!   and then restarted it from the scene pick, so Noa got up again and again
//!   for as long as the dialogue stayed open;
//! - **the ledge hop**: `FUN_801d2298` raises the clamp at take-off and at the
//!   landing crossing and clears it at tear-down, with clip bases `6` / `7`
//!   (bank slots 5 / 6). The port ignored the clamp, and the hop's arc-made
//!   moves sent the body to the motion-derived walk loop for the whole flight.
//!
//! The tick a script makes the scene pick, the clip is not loaded yet (the
//! host resolves it in the frame tail): the body holds its last pose for
//! that tick instead of flashing the motion-derived pair.
//!
//! Both are driven here through a `NativeSession` with the per-frame tail the
//! play hosts run (`step_world_frame_tail` over the binding's banks), the
//! same player clip path the native window and the browser page share.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or the
//! extracted disc missing.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::scene_actors::{SceneActorBinding, bind_scene_actors};
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;
use legaia_engine_vm::actor_tick::{CLIP_CLAMP, CLIP_END};

fn extracted_dir() -> Option<PathBuf> {
    let env = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    let rel = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    [env, Some(rel)]
        .into_iter()
        .flatten()
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

fn open(scene: &str, seed: impl FnOnce(&mut NativeSession)) -> Option<NativeSession> {
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
    seed(&mut session);
    session
        .enter_field_live(scene, &FieldLiveOpts::default())
        .expect("enter scene");
    Some(session)
}

/// One host frame: the session tick, then the world tail with the banks.
fn frame(session: &mut NativeSession, binding: &SceneActorBinding) {
    session.tick().expect("tick");
    let _ = session.host.world.step_world_frame_tail(
        binding.scene_anm.as_ref(),
        binding.locomotion_anm.as_ref(),
        |slot| binding.clip_targets.get(&slot).copied(),
    );
}

/// What the player's clip is doing after a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    source: &'static str,
    id: usize,
    cursor: u32,
    frames: usize,
    clip: i16,
    flags: u16,
}

fn sample(session: &NativeSession) -> Sample {
    let w = &session.host.world;
    let flags = w
        .player_actor_slot
        .and_then(|s| w.actors.get(s as usize))
        .map_or(0, |a| a.move_state.local_flags);
    let (source, id, cursor, frames) = w
        .locomotion
        .player_anim
        .as_ref()
        .expect("the binding installed the player's clips")
        .playing_clip();
    Sample {
        source,
        id,
        cursor,
        frames,
        clip: w.locomotion.player_clip,
        flags,
    }
}

/// `cave01`'s Noa introduction: the clamped getting-up clip (scene clip 29,
/// record 28) plays one pass and rests on its last frame, latched, until the
/// script's next pick - it never restarts.
#[test]
fn noas_getting_up_plays_once_and_holds() {
    const GETTING_UP: i16 = 29;
    let Some(mut session) = open("cave01", |s| {
        s.host.world.set_active_party(vec![0, 1, 2]);
        // The beat that empties the party and spawns Noa's introduction.
        s.host.world.system_flag_set(0x156);
    }) else {
        return;
    };
    let mut binding = bind_scene_actors(&mut session.host);
    session.set_host_drains_queues(true);
    let mut naming_step = 0u32;
    let mut run: Vec<Sample> = Vec::new();
    let mut left_at = None;
    for f in 0..3000u32 {
        // The roster change to Noa alone rebuilds the rig, as both hosts do.
        if session.host.world.take_player_rig_change() {
            binding = bind_scene_actors(&mut session.host);
        }
        if session.host.world.name_entry_active() {
            let edge = if f % 8 >= 2 {
                0
            } else {
                let p = [PadButton::Cross, PadButton::Up, PadButton::Cross]
                    [(naming_step as usize).min(2)];
                naming_step += 1;
                p.mask()
            };
            session.step_name_entry_frame(edge);
            continue;
        }
        // Cross through the dialogue.
        session.host.world.set_pad(if f % 8 < 2 {
            PadButton::Cross.mask()
        } else {
            0
        });
        frame(&mut session, &binding);
        let s = sample(&session);
        if s.clip == GETTING_UP {
            run.push(s);
        } else if !run.is_empty() {
            left_at = Some(s);
            break;
        }
    }
    let left_at = left_at.expect("the cutscene picked the getting-up clip and moved on");
    // The tick of the pick holds the previous pose while the host resolves
    // the clip; it never flashes the motion pair.
    assert!(
        run.iter().all(|s| s.source != "idle" && s.source != "walk"),
        "the motion pair flashed under the scene pick: {:?}",
        &run[..run.len().min(3)]
    );
    let scene: Vec<&Sample> = run.iter().filter(|s| s.source == "scene").collect();
    let first = scene
        .first()
        .expect("the getting-up clip played as the scene pick");
    assert_eq!((first.id, first.frames), (28, 120), "{first:?}");
    let last_pos = (first.frames * 16 - 1) as u32;
    assert!(
        scene.iter().all(|s| s.flags & CLIP_CLAMP != 0),
        "the script's AB F8 03 reached the player's +0x62"
    );
    // The cursor only ever moves forward: no wrap, no restart.
    for w in scene.windows(2) {
        assert!(
            w[1].cursor >= w[0].cursor,
            "the one-shot restarted: {:?} -> {:?}",
            w[0],
            w[1]
        );
    }
    let held: Vec<&&Sample> = scene.iter().filter(|s| s.cursor == last_pos).collect();
    assert!(
        held.len() >= 2,
        "the clip reached its last frame and rested there ({} samples)",
        held.len()
    );
    assert!(
        held.iter().all(|s| s.flags & CLIP_END != 0),
        "latched at rest"
    );
    eprintln!(
        "getting-up: {} frames on the scene pick, {} held on frame {}; next pick clip {}",
        scene.len(),
        held.len(),
        first.frames - 1,
        left_at.clip
    );
}

/// A `town01` ledge hop: the take-off clip (bank slot 5) and the landing clip
/// (slot 6) each play once and hold under the hop's clamp, and the pad's walk
/// returns at tear-down.
#[test]
fn the_ledge_hop_plays_its_clips_once() {
    let Some(mut session) = open("town01", |_| {}) else {
        return;
    };
    let binding = bind_scene_actors(&mut session.host);
    session.set_host_drains_queues(true);
    for _ in 0..600 {
        session.host.world.set_pad(0);
        frame(&mut session, &binding);
    }
    let slot = session.host.world.player_actor_slot.expect("player") as usize;
    // The retail capture's ledge: Up from (5152, 32) hops at (5152, 96).
    {
        let w = &mut session.host.world;
        w.actors[slot].move_state.world_x = 5152;
        w.actors[slot].move_state.world_z = 32;
        w.actors[slot].move_state.flags &= !0x0008_0000;
        w.locomotion.ledge_hop = None;
    }
    let mut flight = Vec::new();
    let mut hopped = false;
    for t in 0..90u32 {
        let pad = if t < 30 { PadButton::Up.mask() } else { 0 };
        session.host.world.set_pad(pad);
        frame(&mut session, &binding);
        let in_hop = session.host.world.locomotion.ledge_hop.is_some();
        hopped |= in_hop;
        if in_hop {
            flight.push(sample(&session));
        } else if hopped {
            break;
        }
    }
    assert!(hopped, "the ledge at (5152, 96) starts a hop");
    for (bank_slot, name) in [(5usize, "take-off"), (6, "landing")] {
        let clip: Vec<&Sample> = flight
            .iter()
            .filter(|s| s.source == "bank" && s.id == bank_slot)
            .collect();
        assert!(!clip.is_empty(), "the {name} clip plays: {flight:?}");
        for w in clip.windows(2) {
            assert!(
                w[1].cursor >= w[0].cursor,
                "the {name} clip looped: {clip:?}"
            );
        }
        let last_pos = (clip[0].frames * 16 - 1) as u32;
        assert_eq!(
            clip.last().map(|s| s.cursor),
            Some(last_pos),
            "the {name} clip ends on its last frame: {clip:?}"
        );
    }
    assert!(
        !flight.iter().any(|s| s.source == "walk"),
        "no motion-derived walk in the air: {flight:?}"
    );
}
