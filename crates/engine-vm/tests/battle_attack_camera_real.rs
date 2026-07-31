//! Disc-gated: the per-art attack camera (`FUN_801D71B8`) driven against the
//! **real** `0x801F4E10` track table, and through the shared battle camera
//! the two hosts run.
//!
//! Three things a synthetic table cannot check:
//!
//! * that the disc rows the arms fold are real camera offsets rather than
//!   whatever happened to sit at those addresses (a probe table proves the
//!   addressing, not the data);
//! * that the arms disagree with `FUN_801D5854` case 6's own framing - if the
//!   per-art override landed on the same pose it would be unobservable, and
//!   the whole channel could be silently inert;
//! * that the override actually reaches [`BattleCamera`]'s live pose when the
//!   Action phase runs, which is the seam the port was missing.
//!
//! Skips and passes when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::PathBuf;

use legaia_asset::battle_attack_camera_table::AttackCameraTracks;
use legaia_engine_vm::battle_attack_camera::{
    AttackCamActor, AttackCamCtx, CharacterArm, FIRST_ART, art_arm, attack_camera_framing,
};
use legaia_engine_vm::battle_cam_script::{
    ActionFraming, AttackCamChannels, BattleCamActor, BattleCamInputs, BattleCamPhase,
    BattleCamera, action_framing, drive, phase_for,
};

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() {
            return Some(p);
        }
    }
    eprintln!("[skip] extracted/PROT.DAT missing");
    None
}

fn real_tracks() -> Option<AttackCameraTracks> {
    let prot = extracted_dir()?.join("PROT.DAT");
    let mut archive = legaia_prot::archive::Archive::open(&prot).expect("open PROT.DAT");
    let entry = archive
        .entries
        .get(legaia_asset::battle_camera_table::BATTLE_ACTION_OVERLAY_PROT_INDEX)
        .cloned()
        .expect("PROT 0898 entry exists");
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .expect("read PROT 0898");
    legaia_asset::battle_attack_camera_table::parse(&bytes)
}

const CHARACTERS: [CharacterArm; 3] = [CharacterArm::One, CharacterArm::Two, CharacterArm::Three];
/// The animation cursors that walk every multi-window arm through its windows.
const CURSORS: [i16; 6] = [0x00, 0x50, 0x80, 0xB0, 0xF8, 0x120];

fn actor(character: CharacterArm, art_id: u8) -> AttackCamActor {
    AttackCamActor {
        character,
        art_id,
        arm_select: 0,
        anim_frame: 0,
        // The retail solo-Vahn seat, so the framing is comparable with the
        // shared camera's own traced case.
        pos: [0, 0, (-800i16) as u16],
        facing: 0,
    }
}

/// Every arm body frames on the real table, and every pose it hands back is a
/// plausible camera rather than a wild value: the depth stays positive (a
/// camera behind its own focus would put the actor off the screen) and the
/// look-at is the actor.
#[test]
fn every_arm_frames_on_the_real_table() {
    let Some(table) = real_tracks() else { return };
    let mut framed = std::collections::BTreeSet::new();
    for (ci, c) in CHARACTERS.into_iter().enumerate() {
        for art in FIRST_ART..=0x2Du8 {
            let Some(va) = art_arm(c, art) else { continue };
            for anim in CURSORS {
                for arm_select in [0u8, 2, 5] {
                    let mut ctx = AttackCamCtx {
                        phase_cursor: (ci % 2) as u8,
                        ramp: 24,
                        accum: 240,
                        latch: 0,
                    };
                    let mut a = actor(c, art);
                    a.anim_frame = anim;
                    a.arm_select = arm_select;
                    let Some(f) = attack_camera_framing(a, &mut ctx, &table) else {
                        continue;
                    };
                    framed.insert(va);
                    assert!(
                        f.pose.dist[2] > 0,
                        "{c:?} art {art:#x} anim {anim:#x} arm {arm_select}: depth {} is not in front of the eye",
                        f.pose.dist[2]
                    );
                    assert_eq!(f.pose.look_at, [0, 0, 800], "orbits the acting actor");
                }
            }
        }
    }
    assert_eq!(framed.len(), 13, "all thirteen arm bodies framed");
}

/// **The non-vacuity check.** Every arm's framing differs from the case-6
/// party pose it overrides - so wiring the channel is observable, and a
/// regression that silently stopped calling it would move the camera.
#[test]
fn the_real_arms_all_reframe_away_from_the_case_six_pose() {
    let Some(table) = real_tracks() else { return };
    let base = action_framing(
        BattleCamActor {
            facing: 0,
            world: [0.0, 0.0, -800.0],
            height: None,
        },
        ActionFraming::default(),
    );
    for (ci, c) in CHARACTERS.into_iter().enumerate() {
        for art in FIRST_ART..=0x2Du8 {
            if art_arm(c, art).is_none() {
                continue;
            }
            let mut ctx = AttackCamCtx {
                phase_cursor: (ci % 2) as u8,
                ramp: 24,
                accum: 240,
                latch: 0,
            };
            let mut a = actor(c, art);
            a.anim_frame = 0x50;
            let Some(f) = attack_camera_framing(a, &mut ctx, &table) else {
                continue;
            };
            let same_yaw = f64::from(f.pose.rot[1]).rem_euclid(4096.0) == f64::from(base.yaw);
            let same_depth = f64::from(f.pose.dist[2]) == f64::from(base.tr[2]);
            assert!(
                !(same_yaw && same_depth),
                "{c:?} art {art:#x} reproduces the case-6 pose exactly"
            );
        }
    }
}

/// The two columns of the real table are genuinely different framings, which
/// is what makes `ctx[+0x26D]`'s coin flip visible in play: at least one arm
/// per character frames differently for cursor `0` and cursor `1`.
#[test]
fn the_two_real_columns_frame_differently() {
    let Some(table) = real_tracks() else { return };
    for c in CHARACTERS {
        let mut differing = 0;
        for art in FIRST_ART..=0x2Du8 {
            if art_arm(c, art).is_none() {
                continue;
            }
            let pose = |cursor: u8| {
                let mut ctx = AttackCamCtx {
                    phase_cursor: cursor,
                    ramp: 24,
                    accum: 240,
                    latch: 0,
                };
                let mut a = actor(c, art);
                a.anim_frame = 0x50;
                attack_camera_framing(a, &mut ctx, &table).map(|f| f.pose)
            };
            if pose(0) != pose(1) {
                differing += 1;
            }
        }
        assert!(differing > 0, "{c:?} has no column-sensitive arm");
    }
}

/// **The seam.** Driven through the shared battle camera with the real table,
/// an Action phase whose actor carries a live art id settles on the per-art
/// framing, not on case 6's - and the same drive with the channel unarmed
/// settles on case 6's. Proving both directions is what makes this a wiring
/// test rather than a "it changed" test.
#[test]
fn the_shared_camera_settles_on_the_per_art_framing() {
    let Some(table) = real_tracks() else { return };
    let cam_actor = BattleCamActor {
        facing: 0,
        world: [0.0, 0.0, -800.0],
        height: None,
    };
    let run = |attack: Option<AttackCamChannels>| {
        let mut slot: Option<BattleCamera> = None;
        let mut frames = 0u64;
        // A few frames of Menu, then a long Action phase - the shape a real
        // turn takes.
        for phase in [false, false, true, true] {
            for _ in 0..40 {
                frames += 2;
                let inputs = BattleCamInputs {
                    target: None,
                    entry_yaw: 0.0,
                    phase: phase_for(false, false, phase),
                    acting: Some(cam_actor),
                    formation: None,
                    action: ActionFraming::default(),
                    shake_amplitude: 0,
                    attack: if phase { attack } else { None },
                };
                drive(&mut slot, true, inputs, frames, Some(&table));
            }
        }
        let cam = slot.expect("camera created");
        assert_eq!(cam.phase(), BattleCamPhase::Action);
        cam.framing_pose()
    };

    let unarmed = run(None);
    let armed = run(Some(AttackCamChannels {
        character: CharacterArm::One,
        art_id: 0x1A,
        arm_select: 5,
        anim_frame: 0x80,
    }));
    assert_ne!(
        unarmed, armed,
        "the per-art override never reached the live pose"
    );
    // The unarmed run must be case 6's own party framing.
    let case6 = action_framing(cam_actor, ActionFraming::default());
    assert_eq!(
        unarmed.tr[2], case6.tr[2],
        "case 6 owns the unarmed framing"
    );
    // The armed run must be in front of the actor it frames.
    assert!(armed.tr[2] > 0.0, "armed depth {}", armed.tr[2]);
}

/// An art id with no arm leaves case 6's framing standing - the "no arm" path
/// must not snap the camera to a default pose.
#[test]
fn an_unarmed_art_id_leaves_case_six_standing() {
    let Some(table) = real_tracks() else { return };
    let cam_actor = BattleCamActor {
        facing: 0,
        world: [0.0, 0.0, -800.0],
        height: None,
    };
    let mut slot: Option<BattleCamera> = None;
    let mut frames = 0u64;
    for _ in 0..80 {
        frames += 2;
        let inputs = BattleCamInputs {
            target: None,
            entry_yaw: 0.0,
            phase: BattleCamPhase::Action,
            acting: Some(cam_actor),
            formation: None,
            action: ActionFraming::default(),
            shake_amplitude: 0,
            attack: Some(AttackCamChannels {
                character: CharacterArm::One,
                // `0x1B` is an epilogue slot inside character 1's own bound.
                art_id: 0x1B,
                arm_select: 0,
                anim_frame: 0x40,
            }),
        };
        drive(&mut slot, true, inputs, frames, Some(&table));
    }
    let pose = slot.expect("camera created").framing_pose();
    let case6 = action_framing(cam_actor, ActionFraming::default());
    assert_eq!(pose.tr, case6.tr);
    assert_eq!(pose.pitch, case6.pitch);
}
