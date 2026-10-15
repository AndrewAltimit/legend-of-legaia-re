//! The shake channel and the `drive` entry points.

use super::*;

/// The shake rides on top of the framing pose: `pose()` carries it,
/// `framing_pose()` does not, and amplitude 0 keeps them equal.
#[test]
fn shake_offsets_the_pose_only_while_the_amplitude_is_live() {
    let mut cam = traced_cam(BattleCamPhase::Menu);
    steps(&mut cam, 4);
    assert_eq!(cam.pose(), cam.framing_pose(), "no amplitude, no jitter");
    assert_eq!(cam.shake_offset(), [0, 0]);
    cam.set_shake_amplitude(8);
    let mut moved = false;
    for _ in 0..8 {
        steps(&mut cam, 1);
        let (shaken, framing) = (cam.pose(), cam.framing_pose());
        assert_eq!(shaken.pitch, framing.pitch, "shake is translation-only");
        assert_eq!(shaken.tr[2], framing.tr[2]);
        assert_eq!(
            [shaken.tr[0] - framing.tr[0], shaken.tr[1] - framing.tr[1]],
            [cam.shake_offset()[0] as f32, cam.shake_offset()[1] as f32]
        );
        moved |= shaken.tr[0] != framing.tr[0] || shaken.tr[1] != framing.tr[1];
    }
    assert!(moved, "amplitude 8 must actually jitter the translation");
    // Dropping the amplitude backs the last offset out on the next step.
    cam.set_shake_amplitude(0);
    steps(&mut cam, 1);
    assert_eq!(cam.shake_offset(), [0, 0]);
    assert_eq!(cam.pose(), cam.framing_pose());
}

/// A live shake must not stall a glide: the framing still lands exactly
/// on its endpoint (which is why the offset is held beside the pose).
#[test]
fn shake_does_not_prevent_a_glide_from_completing() {
    let mut cam = traced_cam(BattleCamPhase::Dialogue);
    cam.set_shake_amplitude(0x10);
    cam.set_phase(BattleCamPhase::Menu);
    steps(&mut cam, 20);
    assert_eq!(cam.framing_pose().tr, traced_menu_tr());
}

/// `drive` owns the whole per-frame ordering: entry snap, retarget,
/// phase change, step - and drops the state when the battle ends.
#[test]
fn drive_creates_steps_and_drops() {
    let at = |phase, formation| BattleCamInputs {
        target: None,
        entry_yaw: 0.0,
        phase,
        formation,
        ..Default::default()
    };
    let mut slot: Option<BattleCamera> = None;
    // Inactive: stays empty.
    drive(&mut slot, false, at(BattleCamPhase::Menu, None), 0, None);
    assert!(slot.is_none());
    // First active frame in the Menu phase: entry snap to the far
    // framing (BOOT depth - no formation installed yet on frame 0).
    drive(&mut slot, true, at(BattleCamPhase::Menu, None), 0, None);
    let p0 = slot.as_ref().unwrap().pose();
    assert_eq!((p0.pitch, p0.tr), (BOOT_POSE.pitch, BOOT_POSE.tr));
    // Formation + 2 frames: the framing resizes and the orbit runs.
    let formation = Some(FormationBox {
        min: [-800.0, -800.0],
        max: [800.0, 800.0],
    });
    drive(
        &mut slot,
        true,
        at(BattleCamPhase::Menu, formation),
        2,
        None,
    );
    let p1 = slot.as_ref().unwrap().pose();
    assert_eq!(p1.yaw, 4092.0, "one orbit step");
    // Submenu opens on the traced default seat: 6 steps arrive on the
    // close-up.
    for f in 2..8 {
        drive(
            &mut slot,
            true,
            at(BattleCamPhase::Submenu, formation),
            f * 2,
            None,
        );
    }
    let p2 = slot.as_ref().unwrap().pose();
    assert_eq!(p2.tr, BattleCamActor::default().submenu_pose().tr);
    // Battle ends: the state drops so the next battle re-snaps.
    drive(&mut slot, false, at(BattleCamPhase::Menu, None), 16, None);
    assert!(slot.is_none());
}

/// `drive` carries the action framing and the shake amplitude through to
/// the camera - the two channels a host could otherwise wire on one side
/// only.
#[test]
fn drive_carries_the_action_and_shake_channels() {
    let inputs = BattleCamInputs {
        target: None,
        entry_yaw: 0.0,
        phase: BattleCamPhase::Action,
        acting: Some(BattleCamActor::default()),
        action: ActionFraming {
            party_slot: false,
            depth_raw: 0x1400,
            ..Default::default()
        },
        shake_amplitude: 6,
        ..Default::default()
    };
    let mut slot: Option<BattleCamera> = None;
    for f in 0..=CHASE_SETTLE_STEPS {
        drive(&mut slot, true, inputs, f * 2, None);
    }
    let cam = slot.as_ref().unwrap();
    let want = action_framing(BattleCamActor::default(), inputs.action);
    assert_eq!(cam.framing_pose().tr, want.tr, "case-6 fallback depth");
    assert_ne!(cam.shake_offset(), [0, 0], "amplitude reached the kernel");
}

/// A Menu-phase entry with the formation already live snaps straight to
/// the case-9 formation-sized framing - no degenerate minimum-depth
/// frame while waiting for the first phase transition.
#[test]
fn drive_entry_sizes_to_the_live_formation() {
    let formation = Some(FormationBox {
        min: [-800.0, -800.0],
        max: [800.0, 800.0],
    });
    let mut slot: Option<BattleCamera> = None;
    drive(
        &mut slot,
        true,
        BattleCamInputs {
            phase: BattleCamPhase::Menu,
            formation,
            ..Default::default()
        },
        0,
        None,
    );
    let p = slot.as_ref().unwrap().pose();
    assert_eq!(p.tr, [0.0, 1280.0, 7680.0], "the traced far framing");
    assert_eq!(p.focus, [0.0; 3]);
}

/// A battle that opens on dialogue snaps to the held close-up; any other
/// entry snaps to the far framing.
#[test]
fn drive_entry_snap_follows_the_opening_phase() {
    let mut slot: Option<BattleCamera> = None;
    drive(
        &mut slot,
        true,
        BattleCamInputs {
            phase: BattleCamPhase::Dialogue,
            ..Default::default()
        },
        0,
        None,
    );
    assert_eq!(slot.as_ref().unwrap().pose(), dialogue_pose(None));
    let mut slot: Option<BattleCamera> = None;
    // Opening straight into a submenu still enters at the menu framing
    // and glides in (retail's loading pose resolves at the far framing).
    drive(
        &mut slot,
        true,
        BattleCamInputs {
            phase: BattleCamPhase::Submenu,
            ..Default::default()
        },
        0,
        None,
    );
    let p = slot.as_ref().unwrap().pose();
    assert_ne!(p.tr, BattleCamActor::default().submenu_pose().tr);
}

/// The camera's draws land on the stream the caller lends it: a live shake
/// takes exactly two `world_rand` draws per camera step off `rng`, and a
/// shake at rest takes none. Retail draws them from the one BIOS `rand()`
/// seed (`FUN_801D9D30`'s two `jal 0x80056798`), so a private camera stream
/// would interleave them differently from every other draw in the tick.
#[test]
fn drive_on_stream_draws_the_shake_off_the_lent_stream() {
    let at = |amp| BattleCamInputs {
        phase: BattleCamPhase::Menu,
        shake_amplitude: amp,
        ..Default::default()
    };
    let mut slot: Option<BattleCamera> = None;
    let mut rng = 0x1357_9BDFu32;
    drive_on_stream(&mut slot, true, at(0), 0, None, &mut rng);
    let rest = rng;
    // At rest, steps draw nothing.
    drive_on_stream(&mut slot, true, at(0), 8, None, &mut rng);
    assert_eq!(rng, rest, "a shake at rest never moves the stream");
    // Live: one camera step (two display frames) = two draws.
    drive_on_stream(&mut slot, true, at(4), 10, None, &mut rng);
    let mut check = rest;
    legaia_engine_system::rand::world_rand(&mut check);
    legaia_engine_system::rand::world_rand(&mut check);
    assert_eq!(rng, check, "one live step = two world draws");
}
