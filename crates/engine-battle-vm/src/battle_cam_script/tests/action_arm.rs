//! The phase map and the action-band arms: fork, yaw counter, fallback tweaks, park stepping.

use super::*;

/// `phase_for` is the shared boolean mapping: dialogue outranks the
/// submenu (retail's tutorial text draws over an open menu), and an
/// executing action outranks only the idle far framing.
#[test]
fn phase_for_maps_the_battle_state() {
    assert_eq!(phase_for(true, false, false), BattleCamPhase::Dialogue);
    assert_eq!(phase_for(true, true, true), BattleCamPhase::Dialogue);
    assert_eq!(phase_for(false, true, false), BattleCamPhase::Submenu);
    assert_eq!(phase_for(false, true, true), BattleCamPhase::Submenu);
    assert_eq!(phase_for(false, false, true), BattleCamPhase::Action);
    assert_eq!(phase_for(false, false, false), BattleCamPhase::Menu);
}

/// Case 6's arm fork: the battle-end signal `DAT_8007BD71 == 0xFE`
/// **and** a party slot take the battle-over arm; a running fight
/// (`0xFF`) takes the in-fight arm for party and monster alike.
#[test]
fn action_arm_fork_needs_both_conditions() {
    let f = ActionFraming::default();
    assert!(
        !f.takes_party_arm(),
        "an ordinary party attack in a running fight is the in-fight arm"
    );
    let over = ActionFraming {
        battle_over: true,
        ..f
    };
    assert!(over.takes_party_arm(), "battle over, party seat");
    assert!(
        !ActionFraming {
            party_slot: false,
            ..over
        }
        .takes_party_arm(),
        "monster slot"
    );
}

/// **The in-fight arm, pinned against retail RAM.** Three PCSX-Redux
/// captures parked in `ctx[7] == 0x19` with Gaza (seat 3) acting read the
/// rotation / translation / focus trios directly; each is reproduced here
/// from the context bytes the same captures carry (`ctx[+0x6DA]`,
/// `ctx[+0x6D0]`, `ctx[+0xD]`, `actor[+0x46]`, `actor[+0x34/+0x38]`).
/// The live yaw trails the counter by the tween's lag, so the yaw is
/// checked against the counter the arm computes from, which is what the
/// walker converges on.
#[test]
fn in_fight_arm_reproduces_the_gaza_captures() {
    // gaza2_park_0x19: rot (0, 1657), TR (0, 1280, 5324), focus (-433, 0,
    // -291) stored negated; 6DA 4352, 6D0 3328, D 0, facing 2687.
    let a = action_framing(
        BattleCamActor {
            facing: 2687,
            world: [433.0, -410.0, 291.0],
            height: None,
        },
        ActionFraming {
            party_slot: false,
            yaw_base: 4352,
            depth_raw: 3328,
            ..Default::default()
        },
    );
    assert_eq!(a.pitch, 0.0);
    assert_eq!(a.yaw, ((4352 - 2687) & 0xFFF) as f32);
    assert_eq!(a.tr, [0.0, 1280.0, 5324.0]);
    assert_eq!(a.focus, [433.0, 0.0, 291.0], "focus height is the floor");
    // gaza2_park_0x19_summon_melee: rot (0, 1412), same TR, focus (785,
    // 0, -39); 6DA 2476, facing 1056.
    let b = action_framing(
        BattleCamActor {
            facing: 1056,
            world: [-785.0, 0.0, 39.0],
            height: None,
        },
        ActionFraming {
            party_slot: false,
            yaw_base: 2476,
            depth_raw: 3328,
            ..Default::default()
        },
    );
    assert_eq!(b.yaw, 1420.0);
    assert_eq!(b.focus, [-785.0, 0.0, 39.0]);
    // gaza2_park_0x19_target_vahn: rot (128, 574), TR (0, 1024, 5324),
    // focus (0, 0, -1490); 6DA 6726, D 2, facing 2048.
    let c = action_framing(
        BattleCamActor {
            facing: 2048,
            world: [0.0, 0.0, 1490.0],
            height: None,
        },
        ActionFraming {
            party_slot: false,
            yaw_base: 6726,
            depth_raw: 3328,
            style: 2,
            ..Default::default()
        },
    );
    assert_eq!(c.pitch, 128.0);
    assert_eq!(c.yaw, ((6726 - 2048) & 0xFFF) as f32);
    assert_eq!(c.tr, [0.0, 1024.0, 5324.0]);
    assert_eq!(c.focus, [0.0, 0.0, 1490.0]);
}

/// The reading this replaces put a party attacker through the
/// battle-over arm: eye `prescale(0x500)` = 2048 projection units behind
/// the actor, which parked the camera inside whichever combatant stood
/// there. A party seat in a running fight frames at the framed
/// monster's depth like everyone else, and the old value must not come
/// back.
#[test]
fn a_party_attack_in_a_running_fight_is_not_the_battle_over_close_up() {
    let vahn = BattleCamActor {
        facing: 0,
        world: [0.0, 0.0, -800.0],
        height: None,
    };
    let live = action_framing(
        vahn,
        ActionFraming {
            yaw_base: 0x280,
            depth_raw: 0xC00,
            ..Default::default()
        },
    );
    assert_eq!(
        live.tr[2],
        prescale_tr_z(0xC00),
        "the framed monster's depth"
    );
    assert_ne!(live.tr[2], prescale_tr_z(ACTION_PARTY_TR_Z_RAW));
    assert_eq!(live.tr[1], ACTION_TR_Y);
    assert_eq!(
        live.yaw, 0x280 as f32,
        "counter minus facing, not 0x800 - facing"
    );
    // The same seat once the battle-end signal is up is the close-up.
    let over = action_framing(
        vahn,
        ActionFraming {
            battle_over: true,
            ..Default::default()
        },
    );
    assert_eq!(over.tr[2], prescale_tr_z(ACTION_PARTY_TR_Z_RAW));
    assert_eq!(over.yaw, 0x800 as f32);
}

/// **The screen-space consequence, pinned through the retail projection.**
/// A party melee at the range retail's own mid-art capture measures
/// (`battle_melee_hit_spark`: attacker `(-21, 136)`, target `(-6, 405)`,
/// 270 apart) is filmed by the in-fight arm at the framed monster's
/// depth (`ctx[+0x6D0] = 0xC00` in that fight) from the party seed
/// `0x280`; both combatants must land inside the 320x240 frame, feet
/// and head. The arm the port shipped before - the battle-over arm with
/// its `prescale(0x500)` eye behind the attacker - puts the eye 968
/// projection units short of the target, whose body then spans more
/// than the whole frame ("223 of 240 scanlines covered, ndc.y = -1.63"
/// was the earlier measurement). The two arms are projected through the
/// same [`battle_vp`] so the assertion is on the picture, not the pose.
#[test]
fn a_party_melee_keeps_both_combatants_in_frame() {
    // Retail party model height stand-in: ~200 stage units (the display
    // trio reads `-215` for a seated party member's origin), Y-down.
    const HEAD: f32 = -200.0;
    let attacker = BattleCamActor {
        facing: 0,
        world: [0.0, 0.0, 530.0],
        height: None,
    };
    let target = [0.0, 0.0, 800.0];
    let live = action_framing(
        attacker,
        ActionFraming {
            yaw_base: 0x280,
            depth_raw: 0xC00,
            ..Default::default()
        },
    );
    // Actor draw class: the hosts compose `scale(4) * FLIP` under the
    // camera (see `battle_vp_matches_the_handrolled_retail_projection`).
    let scale4: [f32; 16] = [
        4.0, 0.0, 0.0, 0.0, //
        0.0, 4.0, 0.0, 0.0, //
        0.0, 0.0, 4.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
    ];
    let model = mat_mul(&scale4, &FLIP);
    let vp = battle_vp(&live, 4.0, 4.0 / 3.0);
    let inside = |v: [f32; 3]| -> (f32, f32) {
        let (x, y) = project(&vp, &model, v).expect("in front of the eye");
        assert!(
            (0.0..=320.0).contains(&x) && (0.0..=240.0).contains(&y),
            "{v:?} projects off-frame at ({x:.1}, {y:.1})"
        );
        (x, y)
    };
    let (_, a_feet) = inside(attacker.world);
    let (_, a_head) = inside([attacker.world[0], HEAD, attacker.world[2]]);
    let (_, t_feet) = inside(target);
    let (_, t_head) = inside([target[0], HEAD, target[2]]);
    assert!(a_head < a_feet && t_head < t_feet, "upright");
    // Neither body dominates the frame: retail's mid-art frame shows both
    // fighters at well under half the frame height.
    assert!(a_feet - a_head < 120.0, "attacker {}", a_feet - a_head);
    assert!(t_feet - t_head < 120.0, "target {}", t_feet - t_head);

    // The arm the port used to take for the same swing.
    let over = action_framing(
        attacker,
        ActionFraming {
            battle_over: true,
            ..Default::default()
        },
    );
    let vp = battle_vp(&over, 4.0, 4.0 / 3.0);
    let (_, t_feet) = project(&vp, &model, target).expect("target in front of the eye");
    let t_head = project(&vp, &model, [target[0], HEAD, target[2]]).map(|p| p.1);
    // Feet below the frame or head above it: the target's body spans
    // more than the whole frame - the belly close-up.
    let spans_frame = t_feet > 240.0 || t_head.is_none_or(|y| y < 0.0);
    assert!(
        spans_frame,
        "the battle-over arm must reproduce the old close-up (feet {t_feet:.1}, head {t_head:?})"
    );
}

/// The yaw counter's per-action ladder, driven through the shared entry
/// on the action-state edges: round begin `0`, seed `0x800`, Attack entry
/// `0x200`, and a party swing's commit re-seed `0x280` / `0xA80`. The
/// strike loop's own entry stores nothing.
#[test]
fn the_yaw_counter_is_reseeded_on_the_action_state_edges() {
    let mut slot: Option<BattleCamera> = None;
    let mut frames = 0u64;
    let mut feed = |slot: &mut Option<BattleCamera>, state: u8, party: bool| {
        frames += 2;
        drive(
            slot,
            true,
            BattleCamInputs {
                phase: phase_for_state(false, false, state, DoneBandInputs::default()),
                acting: Some(BattleCamActor::default()),
                action: ActionFraming {
                    party_slot: party,
                    ..Default::default()
                },
                action_state: state,
                ..Default::default()
            },
            frames,
            None,
        );
    };
    // Retail's counter advances one unit per display frame, so each fed
    // frame pair adds 2 on top of the seed (the creating frame elapses
    // nothing).
    feed(&mut slot, 0x00, true);
    assert_eq!(slot.as_ref().unwrap().action_yaw_base(), 0, "round begin");
    feed(&mut slot, 0x0C, true);
    assert_eq!(slot.as_ref().unwrap().action_yaw_base(), 0x802);
    feed(&mut slot, 0x14, true);
    assert_eq!(slot.as_ref().unwrap().action_yaw_base(), 0x202);
    feed(&mut slot, 0x16, true);
    feed(&mut slot, 0x16, true);
    assert_eq!(
        slot.as_ref().unwrap().action_yaw_base(),
        0x206,
        "no edge, drift only"
    );
    feed(&mut slot, 0x1E, true);
    assert_eq!(
        slot.as_ref().unwrap().action_yaw_base(),
        0x208,
        "the strike loop's entry stores nothing"
    );
    // The swing commit's re-seed (`FUN_8004E13C`'s value-2 arm) does.
    let cam = slot.as_mut().unwrap();
    cam.observe_swing_reseed((3, 1));
    cam.observe_swing_reseed((4, 1));
    assert_eq!(cam.action_yaw_base(), 0xA80);
    cam.observe_swing_reseed((4, 0));
    assert_eq!(cam.action_yaw_base(), 0xA80, "only a new re-seed applies");
    // A fresh camera, monster attacker: the strike loop leaves 0x200.
    let mut monster: Option<BattleCamera> = None;
    feed(&mut monster, 0x0C, false);
    assert_eq!(monster.as_ref().unwrap().action_yaw_base(), 0x800);
    feed(&mut monster, 0x14, false);
    feed(&mut monster, 0x1E, false);
    assert_eq!(monster.as_ref().unwrap().action_yaw_base(), 0x204);
    // Round begin zeroes it again.
    feed(&mut monster, 0x00, false);
    assert_eq!(monster.as_ref().unwrap().action_yaw_base(), 2);
}

/// The seed pass and the band entry can land inside one host tick, so
/// the observer keys on **band entry**: a monster attack seen as
/// `0x00 -> 0x15` and a party attack seen as `0x5A -> 0x0A -> 0x14` both
/// land on the `0x200` base (the `0x801E2F20` store is in the `0x0C`
/// pass), a spell band entered the same way keeps `0x800`, and an
/// attack-band entry from another action band - no seed pass between -
/// stores nothing.
#[test]
fn the_yaw_counter_seeds_on_band_entry_when_states_are_skipped() {
    let mut frames = 0u64;
    let mut feed = |slot: &mut Option<BattleCamera>, state: u8, party: bool| {
        frames += 2;
        drive(
            slot,
            true,
            BattleCamInputs {
                phase: phase_for_state(false, false, state, DoneBandInputs::default()),
                acting: Some(BattleCamActor::default()),
                action: ActionFraming {
                    party_slot: party,
                    ..Default::default()
                },
                action_state: state,
                ..Default::default()
            },
            frames,
            None,
        );
    };
    // Monster: the engine's SM runs `0x0A -> 0x0C -> 0x14` inside the
    // tick, and the camera first sees the walk state.
    let mut monster: Option<BattleCamera> = None;
    feed(&mut monster, 0x00, false);
    feed(&mut monster, 0x15, false);
    assert_eq!(monster.as_ref().unwrap().action_yaw_base(), 0x202);
    feed(&mut monster, 0x1E, false);
    assert_eq!(
        monster.as_ref().unwrap().action_yaw_base(),
        0x204,
        "monster: no coin"
    );
    // Party, overworld shape: `0x5A -> 0x0A -> 0x14`.
    let mut party: Option<BattleCamera> = None;
    feed(&mut party, 0x5A, true);
    feed(&mut party, 0x0A, true);
    assert_eq!(
        party.as_ref().unwrap().action_yaw_base(),
        // (the creating feed elapses nothing; the second adds 2)
        2,
        "setup band: untouched"
    );
    feed(&mut party, 0x14, true);
    assert_eq!(party.as_ref().unwrap().action_yaw_base(), 0x202);
    // A spell band entered from the setup band keeps the seed's 0x800.
    let mut caster: Option<BattleCamera> = None;
    feed(&mut caster, 0x0A, true);
    feed(&mut caster, 0x28, true);
    assert_eq!(caster.as_ref().unwrap().action_yaw_base(), 0x802);
    // ... and crossing into the attack band from there is not a seed.
    feed(&mut caster, 0x14, true);
    assert_eq!(caster.as_ref().unwrap().action_yaw_base(), 0x804);
}

/// The battle-over arm frames from behind the actor at a constant depth,
/// and the height floor tilts the pitch instead of sinking the camera.
#[test]
fn party_action_arm_floors_the_height_and_tilts_the_pitch() {
    let ground = BattleCamActor {
        facing: 0x200,
        world: [100.0, 0.0, -800.0],
        height: None,
    };
    let over = ActionFraming {
        battle_over: true,
        ..Default::default()
    };
    let p = action_framing(ground, over);
    assert_eq!(p.yaw, (0x800 - 0x200) as f32, "0x800 - facing");
    assert_eq!(p.tr[0], 0.0);
    assert_eq!(p.tr[2], prescale_tr_z(ACTION_PARTY_TR_Z_RAW));
    assert_eq!(p.focus, [100.0, 0.0, -800.0], "orbits the acting actor");
    // TR.y would be -5 * 0 = 0, under the floor: raised to 0x280 with a
    // quarter of the shortfall added to the pitch.
    assert_eq!(p.tr[1], ACTION_HEIGHT_FLOOR);
    assert_eq!(p.pitch, (0x280 / 4) as f32);
    // An actor lifted off the ground (retail Y is down-positive, so a
    // negative Y is airborne) frames higher and stops tilting once the
    // scaled height clears the floor.
    let airborne = BattleCamActor {
        world: [0.0, -200.0, 0.0],
        ..ground
    };
    let q = action_framing(airborne, over);
    assert_eq!(q.tr[1], 1000.0, "-5 * -200");
    assert_eq!(q.pitch, 0.0, "clear of the floor, no compensation");
}

/// The in-fight arm is the one that reads `ctx[+0x6D0]` - the depth
/// `camera_height_for_frame` derives from the framed monster's size.
#[test]
fn fallback_action_arm_reads_the_computed_depth() {
    let monster = ActionFraming {
        party_slot: false,
        depth_raw: 0x1400,
        ..Default::default()
    };
    let actor = BattleCamActor {
        facing: 0x100,
        world: [0.0, 0.0, 800.0],
        height: None,
    };
    let p = action_framing(actor, monster);
    assert_eq!(p.pitch, 0.0);
    assert_eq!(p.yaw, (4096 - 0x100) as f32, "yaw_base 0 minus the facing");
    assert_eq!(p.tr, [0.0, ACTION_TR_Y, prescale_tr_z(0x1400)]);
    assert_eq!(monster.raw_z(), 0x1400);
    // A bulkier monster pulls the camera back; the size class is the only
    // thing that moves between these two.
    let small = ActionFraming {
        depth_raw: crate::battle_formulas::CAMERA_HEIGHT_MIN as i32,
        ..monster
    };
    assert!(action_framing(actor, small).tr[2] < p.tr[2]);
}

/// `ctx[+0xD]` styles 1/3 add the half turn; 2/3 share the height +
/// pitch body (retail reaches it by falling out of the `== 3` arm).
#[test]
fn fallback_style_byte_selects_the_three_tweaks() {
    let base = ActionFraming {
        party_slot: false,
        yaw_base: 0x400,
        ..Default::default()
    };
    let actor = BattleCamActor::default();
    let at = |style| action_framing(actor, ActionFraming { style, ..base });
    assert_eq!(at(0).yaw, 0x400 as f32);
    assert_eq!(at(0).tr[1], ACTION_TR_Y);
    assert_eq!(at(1).yaw, 0xC00 as f32);
    assert_eq!(at(1).tr[1], ACTION_TR_Y, "style 1 leaves the height");
    assert_eq!(at(2).yaw, 0x400 as f32, "style 2 leaves the yaw");
    assert_eq!(at(2).tr[1], ACTION_STYLE_TR_Y);
    assert_eq!(at(2).pitch, ACTION_STYLE_PITCH);
    // Style 3 is both.
    assert_eq!(at(3).yaw, 0xC00 as f32);
    assert_eq!(at(3).tr[1], ACTION_STYLE_TR_Y);
    assert_eq!(at(3).pitch, ACTION_STYLE_PITCH);
    assert_eq!(at(4).yaw, at(0).yaw, "no arm for any other style");
    assert_eq!(at(4).tr, at(0).tr);
}

/// Character `4` in a party seat replaces the fallback translation
/// wholesale, depth included.
#[test]
fn fallback_character_four_override_replaces_the_translation() {
    let f = ActionFraming {
        char_id: ACTION_OVERRIDE_CHAR_ID, // party slot, fight running
        depth_raw: 0x1400,
        style: 2,
        ..Default::default()
    };
    let p = action_framing(BattleCamActor::default(), f);
    assert_eq!(p.pitch, 0x80 as f32, "overrides the style tilt too");
    assert_eq!(p.tr[1], 0x300 as f32);
    assert_eq!(p.tr[2], prescale_tr_z(0xC00));
    assert_eq!(f.raw_z(), 0xC00);
    // A monster slot with the same id keeps the computed depth - retail
    // gates the override on `ctx[+0x13] < 3`.
    let monster = ActionFraming {
        party_slot: false,
        ..f
    };
    assert_eq!(monster.raw_z(), 0x1400);
    assert_eq!(
        action_framing(BattleCamActor::default(), monster).tr[1],
        ACTION_STYLE_TR_Y
    );
}

/// Entering the action phase eases into the case-6 framing: retail re-arms
/// the `a3 = 0xC` tween every pass, so each step covers about a sixth of
/// what remains rather than landing after 6 steps. Leaving it returns to
/// the far framing over case 9's `a3 = 0xE` (7 steps) with the orbit
/// running.
#[test]
fn action_phase_eases_in_on_the_re_armed_tween_and_out_over_seven() {
    let mut cam = traced_cam(BattleCamPhase::Menu);
    cam.set_actor(BattleCamActor {
        facing: 0,
        height: None,
        world: [0.0, 0.0, -800.0],
    });
    let want = action_framing(
        BattleCamActor {
            facing: 0,
            height: None,
            world: [0.0, 0.0, -800.0],
        },
        ActionFraming::default(),
    );
    let start_gap = (cam.framing_pose().tr[2] - want.tr[2]).abs();
    cam.set_phase(BattleCamPhase::Action);
    steps(&mut cam, u64::from(ACTION_STEPS));
    let gap = (cam.framing_pose().tr[2] - want.tr[2]).abs();
    assert!(gap > 0.0, "an ease-out has not landed at step 6");
    assert!(
        gap < start_gap / 2.0,
        "most of the way in: {gap} of {start_gap}"
    );
    steps(&mut cam, CHASE_SETTLE_STEPS);
    let p = cam.framing_pose();
    assert_eq!(p.pitch, want.pitch);
    assert_eq!(p.tr, want.tr);
    assert_eq!(p.focus, want.focus);
    // The in-fight arm's yaw is `ctx[+0x6DA] - facing`, and the counter
    // advances one unit per display frame while the framing is re-armed
    // every pass - so the yaw chases the counter and trails it: once the
    // gap is inside `0xC` the walker's increment is the counter's own two
    // units a step, and the lag holds. The three retail `0x19` parks read
    // it eight units behind.
    let lag =
        |c: &BattleCamera| c.action_yaw_base() as f32 - c.framing_pose().yaw.rem_euclid(4096.0);
    assert!((0.0..12.0).contains(&lag(&cam)), "lag {}", lag(&cam));
    // Held while the action runs - no idle orbit in the Action phase.
    steps(&mut cam, 20);
    let held = cam.framing_pose();
    assert_eq!((held.pitch, held.tr, held.focus), (p.pitch, p.tr, p.focus));
    assert!((0.0..12.0).contains(&lag(&cam)), "lag {}", lag(&cam));
    // End of action: back to the far framing over 7 steps.
    cam.set_phase(BattleCamPhase::Menu);
    steps(&mut cam, SWING_RETURN_STEPS as u64);
    assert_eq!(cam.framing_pose().tr, traced_menu_tr());
    assert_eq!(cam.framing_pose().pitch, MENU_PITCH);
}

/// The action framing pulls in on the actor - the whole point of the
/// phase - and the in-fight arm's depth is the one thing that can push
/// it back out again.
#[test]
fn action_framing_pulls_in_except_for_the_bulkiest_monsters() {
    let actor = BattleCamActor::default();
    let depth = |d| {
        action_framing(
            actor,
            ActionFraming {
                party_slot: false,
                depth_raw: d,
                ..Default::default()
            },
        )
        .tr[2]
    };
    let far = menu_framing(Some(traced_formation()), 0.0).tr[2];
    let party = action_framing(actor, ActionFraming::default());
    assert!(party.tr[2] < far, "party close-up: {party:?} vs {far}");
    assert_eq!(party.focus, actor.world, "framed on the acting actor");
    // The size-class floor still frames closer than the traced far
    // framing; the ceiling does not - a maximum-bulk monster is framed
    // from further out than the solo tutorial formation was.
    assert!(depth(crate::battle_formulas::CAMERA_HEIGHT_MIN as i32) < far);
    assert!(depth(crate::battle_formulas::CAMERA_HEIGHT_MAX as i32) > far);
}

/// The idle bands - stated over the real [`ActionState`] enum rather
/// than over raw bytes, so a state added to the SM lands on one side of
/// the line deliberately.
///
/// The **Done band** is on the framed side: its arms re-arm case 6 / 8
/// every pass under the `ctx[+0x6D8]` tail timer, and the end-of-action
/// gate `0x5A` is where the far framing takes over.
#[test]
fn the_idle_states_leave_the_action_framing() {
    use crate::battle_action::ActionState;
    for s in [
        ActionState::Begin,
        ActionState::PreActionWait,
        ActionState::QueuedFromMenu,
        ActionState::EndOfAction,
        ActionState::RunBegin,
    ] {
        assert!(
            !action_state_frames_the_action(s.as_byte()),
            "{s:?} is idle - it must leave the far framing and its orbit"
        );
    }
    for s in [
        ActionState::ActionSeed,
        ActionState::AttackFace,
        ActionState::AttackAdvance,
        ActionState::AttackStrike,
        ActionState::MagicHitLoop,
        ActionState::SummonSustain,
        ActionState::DoneCleanup,
        ActionState::DoneFadeDown,
        ActionState::DoneSeruAbsorb,
    ] {
        assert!(
            action_state_frames_the_action(s.as_byte()),
            "{s:?} frames the action"
        );
    }
    // Retail's own pair is a subset, kept visible rather than folded in.
    for s in RETAIL_ORBIT_STATES {
        assert!(!action_state_frames_the_action(s));
    }
}

/// The in-fight arm's yaw drifts with the action SM's own counter, so
/// successive enemy actions do not all frame from the same angle.
#[test]
fn action_yaw_counter_drifts_one_unit_per_display_frame() {
    let inputs = BattleCamInputs {
        target: None,
        entry_yaw: 0.0,
        phase: BattleCamPhase::Menu,
        action: ActionFraming {
            party_slot: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut slot: Option<BattleCamera> = None;
    drive(&mut slot, true, inputs, 0, None);
    // 200 display frames of idling, then the action opens.
    drive(&mut slot, true, inputs, 200, None);
    let action = BattleCamInputs {
        target: None,
        entry_yaw: 0.0,
        phase: BattleCamPhase::Action,
        ..inputs
    };
    for f in 0..=CHASE_SETTLE_STEPS {
        drive(&mut slot, true, action, 200 + f * 2, None);
    }
    let first = slot.as_ref().unwrap().framing_pose().yaw;
    // Case 6 is re-armed on every action-SM pass, so the framing chases
    // the counter rather than freezing on its value at the phase change:
    // 200 at entry plus the display frames since, less the chase's lag.
    let counter = 200.0 + (CHASE_SETTLE_STEPS as f32) * 2.0;
    let lag = counter - first.rem_euclid(4096.0);
    assert!(
        (0.0..12.0).contains(&lag),
        "yaw_base chases the live counter: lag {lag}"
    );
    // A later action frames from a different angle.
    drive(&mut slot, true, inputs, 400, None);
    for f in 0..=CHASE_SETTLE_STEPS {
        drive(&mut slot, true, action, 400 + f * 2, None);
    }
    assert_ne!(slot.as_ref().unwrap().framing_pose().yaw, first);
}

/// The active actor's clip commit re-zeroes the ramp, the accumulator and
/// the latch (`FUN_8004AD80` `0x8004BF50..0x8004BF78`) - the death ramp is
/// left alone - so the summon close-up swings from the cast clip's start.
#[test]
fn an_active_clip_commit_rezeroes_the_camera_counters() {
    let mut cam = BattleCamera::new(BattleCamPhase::Action, 0);
    cam.observe_active_commits(4);
    for f in 1..=40u64 {
        cam.advance_to(f);
    }
    assert_eq!(
        cam.attack.ctx.ramp,
        crate::battle_attack_camera::AttackCamCtx::RAMP_CAP
    );
    let death = cam.attack.ctx.death_ramp;
    cam.attack.ctx.latch = 3;
    // The same count again is no commit.
    cam.observe_active_commits(4);
    assert_eq!(
        cam.attack.ctx.ramp,
        crate::battle_attack_camera::AttackCamCtx::RAMP_CAP
    );
    cam.observe_active_commits(5);
    let c = cam.attack.ctx;
    assert_eq!((c.ramp, c.accum, c.latch), (0, 0, 0));
    assert_eq!(c.death_ramp, death);
    cam.advance_to(43);
    assert_eq!(
        cam.attack.ctx.accum,
        3 * crate::battle_attack_camera::AttackCamCtx::RAMP_SCALE
    );
}

/// `battle_gaza2_park_0x19_summon_melee`: the in-fight case-6 arm on
/// retail's own inputs reproduces the step table's targets exactly. Gaza
/// (seat 3) sits at `(-785, 39)` facing `1056`, `ctx[+0x6DA] = 2476`,
/// `ctx[+0x6D0] = 3328`, style `0`; the table holds yaw `1420`, TR
/// `(0, 0x500, ..)` and focus `(785, 0, -39)` stored negated. So the
/// capture's camera gap is its inputs - Gaza parked mid-approach by the
/// retail softlock the state records, and the yaw base's drift - not the
/// framing.
#[test]
fn gaza2_park_action_framing_matches_the_retail_step_targets() {
    let actor = BattleCamActor {
        facing: 1056,
        world: [-785.0, 0.0, 39.0],
        height: None,
    };
    let f = ActionFraming {
        party_slot: false,
        depth_raw: 3328,
        yaw_base: 2476,
        ..ActionFraming::default()
    };
    let pose = action_framing(actor, f);
    assert_eq!(pose.yaw, 1420.0);
    assert_eq!(pose.pitch, 0.0);
    assert_eq!(pose.tr[1], 1280.0);
    assert_eq!(pose.tr[2], 5324.0, "the live TR z");
    assert_eq!(pose.focus, [-785.0, 0.0, 39.0]);
}

/// A camera on a frame step of three steps once every three display frames
/// and scales the walker by three: the summon close-up's three-frame tween
/// (`a3 = 3`) then lands every step, where at the default step of two it
/// covers two thirds of the gap and trails a moving target.
#[test]
fn a_frame_step_of_three_steps_every_third_frame_and_lands_a_three_frame_tween() {
    let mut cam = BattleCamera::new(BattleCamPhase::Menu, 0);
    assert_eq!(cam.frame_step(), DEFAULT_CAMERA_FRAME_STEP);
    cam.set_frame_step(3);
    let start = cam.last_frames;
    cam.advance_to(start + 2);
    assert_eq!(cam.frame_accum, 2, "two frames bank, no step yet");
    cam.advance_to(start + 3);
    assert_eq!(cam.frame_accum, 0, "the third frame fires the step");
    // A glide authored in default-step steps keeps its display frames.
    assert_eq!(cam.steps_of(ACTION_STEPS), 4, "12 frames in steps of 3");
    assert_eq!(cam.frames_to_steps(0x30), 16);

    let pose = |yaw: f32| BattleCamPose {
        pitch: 0.0,
        yaw,
        tr: [0.0, 0.0, 0.0],
        focus: [0.0; 3],
    };
    for (step, landed) in [(3.0, 300.0), (2.0, 200.0)] {
        let mut from = pose(0.0);
        let g = Glide::chase(&mut from, pose(300.0), 0, 3, step);
        cam.pose = from;
        cam.step_components(&g);
        assert_eq!(cam.pose.yaw, landed, "step {step}");
    }
}
