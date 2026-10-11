//! The post-strike, death-reframe, recover and done-tail framings.

use super::*;

/// The two post-strike states hand the camera to case 7, the Seru-absorb
/// banner hold / idle hold to case 8, and the Done-cleanup pair to
/// whichever case its category fork picks - `FUN_801E295C`'s own arms.
#[test]
fn the_post_strike_states_arm_the_two_shot() {
    let none = DoneBandInputs::default();
    for s in RECOVER_STATES {
        assert_eq!(
            phase_for_state(false, false, s, none),
            BattleCamPhase::Recover,
            "state 0x{s:02X} arms FUN_801D5854 case 7"
        );
    }
    for s in ACTION_END_STATES {
        assert_eq!(
            phase_for_state(false, false, s, none),
            BattleCamPhase::ActionEnd,
            "state 0x{s:02X} arms FUN_801D5854 case 8"
        );
    }
    // The Done-cleanup pair: `0x801E5EA0..0x801E5EF4`'s ladder.
    let attack = DoneBandInputs {
        category: DONE_CATEGORY_ATTACK,
        target_party_seat: true,
        target_dead: false,
        ..DoneBandInputs::default()
    };
    let run = DoneBandInputs {
        category: DONE_CATEGORY_RUN,
        ..attack
    };
    let monster_spell = DoneBandInputs {
        category: 2,
        target_party_seat: false,
        target_dead: false,
        ..DoneBandInputs::default()
    };
    let party_target_corpse = DoneBandInputs {
        category: 1,
        target_party_seat: true,
        target_dead: true,
        ..DoneBandInputs::default()
    };
    let monster_target_corpse = DoneBandInputs {
        target_party_seat: false,
        ..party_target_corpse
    };
    for s in DONE_STATES {
        assert_eq!(
            phase_for_state(false, false, s, attack),
            BattleCamPhase::ActionEnd
        );
        assert_eq!(phase_for_state(false, false, s, run), BattleCamPhase::Menu);
        assert_eq!(
            phase_for_state(false, false, s, monster_spell),
            BattleCamPhase::Action
        );
        assert_eq!(
            phase_for_state(false, false, s, party_target_corpse),
            BattleCamPhase::ActionEnd
        );
        assert_eq!(
            phase_for_state(false, false, s, monster_target_corpse),
            BattleCamPhase::Action,
            "the dead-target arm is gated on actor[+0x1DD] < 3"
        );
    }
    // The far framing still owns the end-of-action gate.
    assert_eq!(
        phase_for_state(false, false, 0x5A, attack),
        BattleCamPhase::Menu
    );
}

/// **Capture pin for the Done band.** `zora_glare_petrify_post` is a
/// retail save parked in `ctx[7] == 0x51` after a monster's spell (slot
/// 3, `ctx[+0xD] = 0`, `ctx[+0x6D0] = 0xC00`, `ctx[+0x6DA] = 1966`,
/// Zora at `+0x34/+0x38 = (649, -47)` facing `3297`). Its camera trio
/// reads pitch `0`, yaw `2735`, `TR (0, 1275, 4820)`, focus the negated
/// `(624, 0, -40)` - a tween one step short of case 6's in-fight pose,
/// nowhere near the far framing (pitch `32`, `TR.z` sized to the
/// formation, focus at the bbox centre; the two share `TR.y = 0x500`).
/// The Done band with a non-attack category is therefore
/// case 6 over the caster, and this is the pose the port targets.
#[test]
fn a_monster_spell_done_tail_reads_the_zora_capture() {
    let done = DoneBandInputs {
        category: 2,
        target_party_seat: false,
        target_dead: false,
        ..DoneBandInputs::default()
    };
    assert_eq!(
        phase_for_state(false, false, 0x51, done),
        BattleCamPhase::Action
    );
    let zora = BattleCamActor {
        facing: 3297,
        world: [649.0, 0.0, -47.0],
        height: None,
    };
    let f = ActionFraming {
        party_slot: false,
        depth_raw: 0xC00,
        yaw_base: 1966,
        style: 0,
        char_id: 0,
        ..Default::default()
    };
    let pose = action_framing(zora, f);
    assert_eq!(pose.pitch, 0.0);
    assert_eq!(
        pose.yaw,
        ((1966 - 3297) & 0xFFF) as f32,
        "2765, the live 2735 chasing it"
    );
    assert_eq!(pose.tr, [0.0, 0x500 as f32, prescale_tr_z(0xC00)]);
    assert!(
        (pose.tr[2] - 4915.0).abs() < 1.0,
        "4820 in the capture, one step short"
    );
    assert_eq!(pose.focus, [649.0, 0.0, -47.0]);
    // The far framing this band used to take is a different pose on
    // every axis the capture reads.
    let far = menu_framing(
        Some(FormationBox {
            min: [-829.0, -319.0],
            max: [830.0, 319.0],
        }),
        2735.0,
    );
    assert_ne!(far.pitch, pose.pitch, "32 vs 0");
    assert_ne!(far.tr[2], pose.tr[2], "formation-sized vs ctx[+0x6D0]");
    assert_ne!(far.focus, pose.focus, "bbox centre vs the caster");
}

/// The case-8 **death re-frame** is a different pose on every component,
/// and the `ctx[+0x270]` ramp is what separates its two ends.
///
/// Retail literals: `TR.y = 0x300`, `pitch = 0x140`, `TR.z = ctx[+0x6D0]`
/// on the floor arm (`0x801D6B00..0x801D6B14`); `TR.y = 0x300 - r`,
/// `pitch = 0x180 - 3r/2`, `TR.z = ctx[+0x6D0] - 4r` on the ramped one
/// (`0x801D6B50..0x801D6B98`).
#[test]
fn the_death_reframe_ramp_drops_levels_and_pushes_in() {
    let base = BattleCamPose {
        pitch: 0.0,
        yaw: 100.0,
        tr: [0.0, POST_TR_Y, prescale_tr_z(0xC00)],
        focus: [1.0, 2.0, 3.0],
    };

    // Body on the stage floor: the flat pose, and the caller is told to
    // clear the ramp.
    let mut floor = base;
    assert!(apply_death_reframe(&mut floor, 0xC00, 0xC8, 0.0));
    assert_eq!(floor.pitch, DEATH_PITCH_FLAT as f32);
    assert_eq!(floor.tr[1], DEATH_TR_Y as f32);
    assert_eq!(floor.tr[2], prescale_tr_z(0xC00));
    // Yaw and focus are the base pose's - the tail rewrites neither.
    assert_eq!((floor.yaw, floor.focus), (base.yaw, base.focus));

    // Body still falling, ramp at zero: the pitch seed is the *other*
    // constant. `0x180`, not `0x140` - the one place the two arms of the
    // fork disagree at r = 0.
    let mut fresh = base;
    assert!(!apply_death_reframe(&mut fresh, 0xC00, 0, 1.0));
    assert_eq!(fresh.pitch, DEATH_PITCH_BASE as f32);
    assert_eq!(fresh.tr[1], DEATH_TR_Y as f32);
    assert_eq!(fresh.tr[2], prescale_tr_z(0xC00));

    // Saturated ramp: all three tighten together.
    let mut deep = base;
    assert!(!apply_death_reframe(&mut deep, 0xC00, 0xC8, 1.0));
    assert_eq!(deep.pitch, 0x54 as f32, "0x180 - (3 * 0xC8 >> 1)");
    assert_eq!(deep.tr[1], 0x238 as f32, "0x300 - 0xC8");
    assert_eq!(deep.tr[2], prescale_tr_z(0xC00 - 4 * 0xC8));
    assert!(deep.tr[2] < fresh.tr[2], "the camera pushes in");
    assert!(deep.tr[1] < fresh.tr[1], "and drops");
    assert!(deep.pitch < fresh.pitch, "and levels off");
}

/// The ramp reaches the camera only through a **dead** target, and taking
/// it zeroes the yaw ladder - so a death shot does not inherit the
/// swing's accumulated orbit (`sh zero,0x4(t0)`, `t0 = ctx + 0x6D6`).
#[test]
fn only_a_dead_target_reaches_the_death_reframe() {
    let mut cam = BattleCamera::new(BattleCamPhase::ActionEnd, 0);
    cam.set_actor(BattleCamActor::default());
    cam.action_yaw = 0x321;
    cam.attack.ctx.death_ramp = 0xC8;

    // A live target keeps the base framing and the yaw ladder.
    cam.target = Some(PostActionTarget {
        world: [200.0, 400.0, 600.0],
        live: true,
        ..Default::default()
    });
    let (live_pose, live_z) = cam.action_end_pose();
    assert_eq!(cam.action_yaw, 0x321, "the ladder survives a live target");
    // ...and takes the live-target arm, whose monster depth is `7z/10`.
    assert_eq!(
        live_z,
        live_target_depth(0, cam.live_action_framing().depth_raw)
    );
    assert_ne!(live_pose.tr[1], (DEATH_TR_Y - 0xC8) as f32);

    // The same target dead, still above the floor: the ramped pose, and
    // the glide's raw depth follows the `4 * r` the pose took off.
    cam.target = Some(PostActionTarget {
        world: [200.0, 400.0, 600.0],
        live: false,
        ..Default::default()
    });
    let raw = cam.live_action_framing().depth_raw;
    let (dead_pose, dead_z) = cam.action_end_pose();
    assert_eq!(cam.action_yaw, 0, "the death re-frame zeroes the ladder");
    assert_eq!(dead_pose.tr[1], (DEATH_TR_Y - 0xC8) as f32);
    assert_eq!(dead_z, raw - 4 * 0xC8, "glide target follows the pose");
    assert_eq!(cam.attack.ctx.death_ramp, 0xC8, "still falling, not reset");

    // Dropped to the floor: the flat pose, and the ramp is re-zeroed.
    cam.target = Some(PostActionTarget {
        world: [200.0, 0.0, 600.0],
        live: false,
        ..Default::default()
    });
    let (floor_pose, floor_z) = cam.action_end_pose();
    assert_eq!(floor_pose.pitch, DEATH_PITCH_FLAT as f32);
    assert_eq!(floor_z, raw);
    assert_eq!(cam.attack.ctx.death_ramp, 0);
}

/// **Case 7 orbits the midpoint, and that is what keeps both combatants
/// on screen.** The property no other framing in the set has: case 6
/// focuses one actor, case 8 the target, case 9 the formation centre.
#[test]
fn the_recover_framing_orbits_the_pair_not_one_actor() {
    let actor = BattleCamActor {
        facing: 0,
        world: [0.0, 0.0, -800.0],
        height: None,
    };
    let target = PostActionTarget {
        world: [200.0, 0.0, 600.0],
        live: true,
        ..Default::default()
    };
    let f = ActionFraming::default();
    let pose = recover_framing(actor, Some(target), f, 0.0, false);
    assert_eq!(pose.focus, [100.0, 0.0, -100.0], "midpoint of the pair");
    // Case 8 frames the target alone, on the stage floor.
    let end = action_end_framing(actor, Some(target), f, 0.0);
    assert_eq!(end.focus, [200.0, 0.0, 600.0]);
    // A target whose node is gone falls back to the acting actor (retail's
    // `0x801D6870` arm). The fork reads the node word, not the HP: a target
    // killed but still drawn stays framed (`player_steal_skeleton_banner`).
    let gone = action_end_framing(
        actor,
        Some(PostActionTarget {
            live: false,
            node_gone: true,
            ..target
        }),
        f,
        0.0,
    );
    assert_eq!(gone.focus, [0.0, 0.0, -800.0]);
    let killed = action_end_framing(
        actor,
        Some(PostActionTarget {
            live: false,
            ..target
        }),
        f,
        0.0,
    );
    assert_eq!(killed.focus, [200.0, 0.0, 600.0]);
    // No target at all degenerates case 7 to the acting actor.
    assert_eq!(
        recover_framing(actor, None, f, 0.0, false).focus,
        actor.world
    );
}

/// Case 7's style fork and pull-in tweak, against the arms at
/// `0x801D6698` and `0x801D6780`.
#[test]
fn the_recover_framing_style_and_pull_in_match_the_arms() {
    let actor = BattleCamActor::default();
    let base = ActionFraming {
        depth_raw: 0xC00,
        ..Default::default()
    };
    let plain = recover_framing(actor, None, base, 0.0, false);
    assert_eq!(plain.pitch, 0.0);
    assert_eq!(plain.tr[1], 0x500 as f32);
    assert_eq!(plain.tr[2], prescale_tr_z(0xC00));
    // Style 2: `TR.y = 0x400`, `pitch += 0x80` (`0x801D66E8`).
    let s2 = recover_framing(actor, None, ActionFraming { style: 2, ..base }, 0.0, false);
    assert_eq!(s2.pitch, 0x80 as f32);
    assert_eq!(s2.tr[1], 0x400 as f32);
    // Style 1 adds the half turn to the yaw and leaves the rest.
    let s1 = recover_framing(actor, None, ActionFraming { style: 1, ..base }, 0.0, false);
    assert_eq!(s1.tr[1], 0x500 as f32);
    assert_eq!(
        s1.yaw,
        (plain.yaw as i32 + 0x800).rem_euclid(4096) as f32,
        "style 1 is a half turn"
    );
    // Pull-in (`0x801D6780`): pitch levelled, `TR.y += 0x40`,
    // `TR.z = 3z/5`.
    let pull = recover_framing(actor, None, ActionFraming { style: 2, ..base }, 0.0, true);
    assert_eq!(pull.pitch, 0.0);
    assert_eq!(pull.tr[1], 0x400 as f32 + 0x40 as f32);
    assert_eq!(pull.tr[2], prescale_tr_z(0xC00 * 3 / 5));
}

/// The one-way yaw unwrap both post-action cases run (`0x801D6700` /
/// `0x801D6930`): wrap into 12 bits, then add a full turn when the result
/// would make the tween rotate backwards past the live camera yaw.
#[test]
fn the_post_action_yaw_unwraps_forward_only() {
    // `0 - 0x700` wraps to `0x900`; above a camera yaw of 0, so it stands.
    assert_eq!(unwrap_forward(-0x700, 0.0), 0x900 as f32);
    // Below the live yaw -> a full turn is added rather than rotating back.
    assert_eq!(unwrap_forward(-0x700, 4000.0), (0x900 + 0x1000) as f32);
}

/// The action framing owns the camera **per band**, and the Done band is
/// one of them.
///
/// The band boundaries are `FUN_801E295C`'s own (see
/// [`action_state_frames_the_action`]). `DoneFadeDown` (`0x51`) is
/// retail's bounded action tail (`ctx[+0x6D8]`, seeded `0x3C` at `0x50`),
/// re-armed on case 6 / 8 every pass; the far framing takes over at the
/// end-of-action gate `0x5A`, not before.
#[test]
fn the_done_band_owns_the_action_framing_until_end_of_action() {
    for idle in [0x00u8, 0x0A, 0x0B, 0x5A, 0xFD, 0xFE, 0xFF] {
        assert!(
            !action_state_frames_the_action(idle),
            "state 0x{idle:02X} is idle - the far framing owns it"
        );
    }
    for run in 0x64u8..=0x67 {
        assert!(
            !action_state_frames_the_action(run),
            "Run band 0x{run:02X}: retail arms case 9 + the orbit itself"
        );
    }
    // The action bands DO own it - the seed arm and every attack state
    // re-arm case 6 (`0x801E6464`, `0x801E32E4`).
    for act in [
        0x0Cu8, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1E, 0x1F, 0x28, 0x3C, 0x46, 0x50, 0x51, 0x52,
    ] {
        assert!(
            action_state_frames_the_action(act),
            "state 0x{act:02X} is an action in flight"
        );
    }
}

/// **The residency law the phase script exists to satisfy.** A turn's
/// Done tail is filmed by the per-action framing (case 8 for an attack)
/// and hands back to the far framing at the end-of-action gate, where the
/// formation is on screen and the idle orbit lives.
///
/// The sequence below is a measured `--no-player-battle` turn from the
/// native play-window (`LEGAIA_DIAG_BATCAM`): seed, the attack band, the
/// `DoneFadeDown` tail, then `0x5A` and the next `Begin`. An earlier
/// reading kept the tail on the far framing so that it owned most of the
/// fight's frames; retail's `0x51` arm re-arms case 6 / 8 every pass
/// (`0x801E5FC0..0x801E6018`), and with the in-fight arm right that is a
/// two-shot, not a close-up. What must hold: no tail frame is on the far
/// framing, and every frame from `0x5A` on is.
#[test]
fn a_real_turn_films_its_done_tail_and_hands_back_at_end_of_action() {
    // (state byte, display frames spent there) - one measured turn.
    let turn: [(u8, u32); 10] = [
        (0x00, 2),  // Begin
        (0x0C, 1),  // ActionSeed
        (0x14, 1),  // AttackFace
        (0x16, 24), // AttackAdvance
        (0x17, 1),  // AttackCloseRange
        (0x18, 1),  // AttackStrike
        (0x1E, 2),  // AttackChain
        (0x50, 1),  // DoneCleanup
        (0x51, 48), // DoneFadeDown - the bounded settle
        (0x5A, 2),  // EndOfAction
    ];
    let done = DoneBandInputs {
        category: DONE_CATEGORY_ATTACK,
        target_party_seat: true,
        target_dead: false,
        ..DoneBandInputs::default()
    };
    let mut slot: Option<BattleCamera> = None;
    let mut frames = 0u64;
    let (mut tail_far, mut tail, mut gate_far, mut gate) = (0u32, 0u32, 0u32, 0u32);
    for _round in 0..2 {
        for (state, dwell) in turn {
            for _ in 0..dwell {
                frames += 1;
                let inputs = BattleCamInputs {
                    target: None,
                    frame_step: 0,
                    entry_yaw: 0.0,
                    phase: phase_for_state(false, false, state, done),
                    acting: Some(BattleCamActor::default()),
                    formation: Some(traced_formation()),
                    action: ActionFraming::default(),
                    shake_amplitude: 0,
                    attack: None,
                    action_state: state,
                    active_commits: 0,
                    active_installs: 0,
                    swing_reseed: (0, 0),
                    camera_option: 0,
                    acting_body: None,
                    cursor: None,
                    spell_cam: None,
                    entry_sweep: false,
                };
                drive(&mut slot, true, inputs, frames, None);
                let far = slot.as_ref().map(|c| c.phase()) == Some(BattleCamPhase::Menu);
                if DONE_STATES.contains(&state) {
                    tail += 1;
                    tail_far += u32::from(far);
                } else if state == 0x5A || state == 0x00 {
                    gate += 1;
                    gate_far += u32::from(far);
                }
            }
        }
    }
    assert_eq!(
        tail_far, 0,
        "the Done tail is a per-action framing ({tail} frames)"
    );
    assert_eq!(
        gate_far, gate,
        "end of action and Begin are the far framing"
    );
    assert_eq!(
        slot.as_ref().unwrap().phase(),
        BattleCamPhase::Menu,
        "the round closes on the far framing"
    );
}

/// The strike loop `0x1E` arms case 7 on every pass with no fork
/// (`0x801E36E4..0x801E36EC`); `0x1F` / `0x20` fork to case 8 when the
/// target is on its knockdown / get-up, and `0x20` alone also when the
/// target is a party seat in a death clip.
#[test]
fn the_post_strike_band_forks_on_the_target_reaction() {
    let plain = DoneBandInputs {
        category: DONE_CATEGORY_ATTACK,
        target_party_seat: true,
        ..DoneBandInputs::default()
    };
    let knocked = DoneBandInputs {
        target_knocked: true,
        ..plain
    };
    let dying = DoneBandInputs {
        target_death_clip: true,
        ..plain
    };
    let phase = |s, d| phase_for_state(false, false, s, d);
    assert_eq!(phase(0x1E, knocked), BattleCamPhase::Recover);
    assert_eq!(phase(0x1E, dying), BattleCamPhase::Recover);
    for s in [0x1F, 0x20] {
        assert_eq!(phase(s, plain), BattleCamPhase::Recover);
        assert_eq!(phase(s, knocked), BattleCamPhase::ActionEnd);
    }
    assert_eq!(phase(0x1F, dying), BattleCamPhase::Recover);
    assert_eq!(phase(0x20, dying), BattleCamPhase::ActionEnd);
    let monster_dying = DoneBandInputs {
        target_party_seat: false,
        ..dying
    };
    assert_eq!(phase(0x20, monster_dying), BattleCamPhase::Recover);
}

/// **Capture pin for case 8's live-target arm.** `battle_melee_hit_spark`
/// (`ctx[7] == 0x20`, style `0`, `ctx[+0x6D0] = 0xC00`) holds Vahn's swing on
/// monster `0x0A`, which is on its knockdown (`+0x1D9 = +0x1F1 = 3`) at
/// display height `+0x3E = -461`. Its tween table targets `TR.z 3440` -
/// `prescale(7 * 0xC00 / 10)` exactly - and `TR.y 1550`, the arm's
/// `-7y/2` over the height the table was built from a few frames earlier
/// (`-443`); the capture's own height gives `1613`.
#[test]
fn the_live_target_arm_reads_the_melee_hit_spark_capture() {
    let target = PostActionTarget {
        live: true,
        monster_id: 0x0A,
        animating: true,
        display_y: -461.0,
        ..PostActionTarget::default()
    };
    let mut pose = BattleCamPose {
        pitch: 0.0,
        ..BOOT_POSE
    };
    let raw_z = apply_live_target_reframe(&mut pose, target, 0xC00, BOOT_POSE);
    assert_eq!(raw_z, 0x866);
    assert_eq!(pose.tr[2], 3440.0);
    assert_eq!(pose.tr[1], 1613.0);
    assert_eq!(pose.pitch, 0.0);
    let built = PostActionTarget {
        display_y: -443.0,
        ..target
    };
    let mut pose = BattleCamPose {
        pitch: 0.0,
        ..BOOT_POSE
    };
    apply_live_target_reframe(&mut pose, built, 0xC00, BOOT_POSE);
    assert_eq!(pose.tr[1], 1550.0);
    // A grounded target floors at `0x300` and tilts the pitch by a quarter
    // of the shortfall; an idle one holds the live camera's TR.y / pitch.
    let mut low = BattleCamPose {
        pitch: 0.0,
        ..BOOT_POSE
    };
    let grounded = PostActionTarget {
        display_y: -100.0,
        ..target
    };
    apply_live_target_reframe(&mut low, grounded, 0xC00, BOOT_POSE);
    assert_eq!(low.tr[1], 0x300 as f32);
    assert_eq!(low.pitch, ((0x300 - 350) >> 2) as f32);
    let mut held = BattleCamPose {
        pitch: 0.0,
        ..BOOT_POSE
    };
    let idle = PostActionTarget {
        animating: false,
        ..target
    };
    let live = BattleCamPose {
        pitch: 77.0,
        tr: [0.0, 999.0, 1.0],
        ..BOOT_POSE
    };
    apply_live_target_reframe(&mut held, idle, 0xC00, live);
    assert_eq!((held.pitch, held.tr[1]), (77.0, 999.0));
    // The monster-id ladder.
    assert_eq!(live_target_depth(0xB4, 0xC00), 9 * 0xC00 / 10);
    assert_eq!(live_target_depth(0xA7, 0xC00), 0xC00);
    assert_eq!(live_target_depth(0x20, 0xC00), 8 * 0xC00 / 10);
}

/// **Capture pin for the post-strike death re-frame.**
/// `player_steal_skeleton_banner` (`ctx[7] == 0x20`, style `2`, death ramp
/// `ctx[+0x270]` at its `0xC8` cap) holds Vahn over a skeleton that is on its
/// knockdown (`+0x1D9 = +0x1F1 = 5`) at HP `0`, still airborne (`+0x36 =
/// 183`), facing `2595`, with the per-action coin `ctx[+0x26D] = 0`. Its tween
/// table targets pitch `84`, yaw `1757`, `TR (0, 568, 3635)` - case 8's death
/// re-frame at the cap - and `ctx[+0x6DA]` reads the `0` the arm stores.
#[test]
fn a_kill_on_the_return_reads_the_steal_banner_capture() {
    let done = DoneBandInputs {
        category: DONE_CATEGORY_ATTACK,
        target_party_seat: true,
        target_dead: true,
        target_knocked: true,
        ..DoneBandInputs::default()
    };
    assert_eq!(
        phase_for_state(false, false, 0x20, done),
        BattleCamPhase::ActionEnd
    );
    let mut cam = BattleCamera::new(BattleCamPhase::ActionEnd, 0);
    cam.set_actor(BattleCamActor {
        facing: 548,
        world: [279.0, 0.0, 245.0],
        height: None,
    });
    cam.action.style = 2;
    cam.action.depth_raw = 0xC00;
    cam.action_yaw = 0x218;
    cam.attack.ctx.death_ramp = 0xC8;
    cam.target = Some(PostActionTarget {
        world: [957.0, 183.0, 857.0],
        live: false,
        facing: 2595,
        ..PostActionTarget::default()
    });
    let (pose, raw_z) = cam.action_end_pose();
    assert_eq!(pose.pitch, 84.0);
    assert_eq!(pose.yaw, 1757.0);
    assert_eq!(pose.tr, [0.0, 568.0, 3635.0]);
    assert_eq!(raw_z, 0xC00 - 0x320);
    assert_eq!(pose.focus, [957.0, 0.0, 857.0]);
    assert_eq!(cam.action_yaw, 0);
}
