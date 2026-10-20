//! Disc-gated: field-VM op `4C 51` (the NPC run / seat) binds its byte-`+4`
//! move clip on every arm, not only on a live channel stepping itself.
//!
//! Retail's NPC arm stores the byte into `+0x5C`, forces `+0x5E = -2` and
//! binds it through `FUN_800204F8` (`0x801E1A8C..0x801E1A9C` in
//! `FUN_801DE840`), after clearing hold / clamp / reverse from `+0x62` and
//! setting the party-bank bit from the depth byte's bit 7. Two arms of the
//! port seated the actor and dropped the clip:
//!
//! - the scene-entry install pre-run: `teien`'s Berserker (`P1[6]`, model
//!   `0x14`) seats itself beside its tree with `4C 51 AA 30 00 2A` on the
//!   pre-fight arm and should loop clip `42`, its tree-eating cycle - it
//!   stood in its header clip `39` instead;
//! - a cutscene's cross-context poke: `korout` `P2[3]` seats Noa and Gala
//!   with `CC 09 51 .. 0A` / `CC 0A 51 .. 0F` and then walks them up the
//!   bridge with `C1` legs - they glided in their idle clips.
//!
//! Skip-pass (CLAUDE.md disc-gated convention): `LEGAIA_DISC_BIN` unset or the
//! extracted disc missing.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::scene_actors::{SceneActorBinding, bind_scene_actors};
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};

fn extracted_dir() -> Option<PathBuf> {
    let env = std::env::var_os("LEGAIA_EXTRACTED_DIR").map(PathBuf::from);
    let rel = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extracted");
    [env, Some(rel)]
        .into_iter()
        .flatten()
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

fn enter(scene: &str) -> Option<(NativeSession, SceneActorBinding)> {
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
    session
        .enter_field_live(scene, &FieldLiveOpts::default())
        .expect("enter scene");
    let binding = bind_scene_actors(&mut session.host);
    Some((session, binding))
}

#[test]
fn teien_berserker_binds_its_seat_clip() {
    let Some((session, binding)) = enter("teien") else {
        return;
    };
    const BERSERKER: u8 = 6;
    let cat = binding.catalog.as_ref().expect("teien catalog");
    let entry = cat
        .entries
        .iter()
        .find(|e| e.placement.index == usize::from(BERSERKER))
        .expect("P1[6] is catalogued");
    assert_eq!(entry.placement.model_index, 0x14, "P1[6] is the Berserker");
    assert_eq!(
        entry.placement.anim_id, 42,
        "the pre-fight seat's 4C 51 names clip 42 (the tree-eating loop)"
    );
    assert!(binding.clip_players.contains_key(&BERSERKER));
    let cursor = session.host.world.npcs.clip_cursors[&BERSERKER];
    assert_eq!(cursor.anim_id, 42);
}

#[test]
fn korout_bridge_walk_plays_the_followers_walk_clips() {
    let Some((mut session, binding)) = enter("korout") else {
        return;
    };
    // Drive the frame tail the way the play hosts do, with the banks and
    // the clip targets the binding resolved.
    session.set_host_drains_queues(true);
    let (noa, gala) = (2u8, 3u8);
    let mut walked = [0u32; 2];
    let mut prev = session.host.world.npcs.positions.clone();
    for _ in 0..150 {
        session.tick().expect("tick");
        let w = &mut session.host.world;
        let _ = w.step_world_frame_tail(
            binding.scene_anm.as_ref(),
            binding.locomotion_anm.as_ref(),
            |slot| binding.clip_targets.get(&slot).copied(),
        );
        for (i, (slot, walk)) in [(noa, 10u8), (gala, 15u8)].into_iter().enumerate() {
            let moved = prev.get(&slot) != w.npcs.positions.get(&slot);
            // A seat is a teleport; a step of a few units is the walk leg.
            let stepped = moved
                && prev
                    .get(&slot)
                    .zip(w.npcs.positions.get(&slot))
                    .is_some_and(|(a, b)| (a.0 - b.0).abs() + (a.1 - b.1).abs() < 64);
            if stepped {
                let anim = w.npcs.clip_cursors.get(&slot).map(|c| c.anim_id);
                assert_eq!(
                    anim,
                    Some(walk),
                    "slot {slot} walks the bridge on the clip its 4C 51 seat named"
                );
                walked[i] += 1;
            }
        }
        prev = w.npcs.positions.clone();
    }
    assert!(
        walked.iter().all(|&n| n > 0),
        "both followers walk during the entry beat ({walked:?})"
    );
}
