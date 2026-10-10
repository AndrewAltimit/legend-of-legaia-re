use super::*;

fn put16(ram: &mut [u8], va: u32, v: u16) {
    let o = (va & 0x1F_FFFF) as usize;
    ram[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(ram: &mut [u8], va: u32, v: u32) {
    let o = (va & 0x1F_FFFF) as usize;
    ram[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn put8(ram: &mut [u8], va: u32, v: u8) {
    ram[(va & 0x1F_FFFF) as usize] = v;
}

/// A synthetic two-on-one fight: the reader takes the counts off the
/// context, the ids off the cell and the combatants off the fixed pool
/// slots (monster 0 is slot 3 whatever the party size).
#[test]
fn reads_a_fight_out_of_ram() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 2);
    put8(&mut ram, ctx + 1, 1);
    put8(&mut ram, ctx + 6, 0x1E);
    put8(&mut ram, FORMATION_CELL, 0x4F);
    put8(&mut ram, SEAT_CHARS, 1);
    put8(&mut ram, SEAT_CHARS + 1, 2);
    for (slot, base, hp) in [(0u32, 0x800E_C9E8u32, 180u16), (3, 0x800E_D000, 999)] {
        put32(&mut ram, ACTOR_TABLE + slot * 4, base);
        put16(&mut ram, base + 0x14C, hp - 1);
        put16(&mut ram, base + 0x14E, hp);
    }
    let b = RetailBattle::from_ram(&ram).expect("seedable");
    assert_eq!(b.monster_ids, vec![0x4F]);
    assert_eq!(b.seat_chars, vec![1, 2]);
    assert_eq!(b.party.len(), 2);
    assert_eq!(b.party[0].map(|c| (c.hp, c.hp_max)), Some((179, 180)));
    assert_eq!(b.party[1], None, "an empty pool slot reads as no combatant");
    assert_eq!(b.monsters[0].map(|c| c.hp_max), Some(999));
    assert_eq!(
        BattleFlowState::from_raw(b.flow),
        BattleFlowState::TurnPrompt
    );
}

/// A live summon flash-in block, as `FUN_80024E80` leaves it in the
/// actor pool: the reader finds it by tick word, kind, id and delta, and
/// takes its age off the two countdowns.
#[test]
fn reads_the_summon_flash_age_off_the_fade_block() {
    use legaia_engine_vm::battle_action::SUMMON_FLASH_IN;
    let mut ram = vec![0u8; 0x20_0000];
    let actor = 0x8008_2BC4;
    put32(&mut ram, actor + 0x0C, FADE_ACTOR_TICK);
    let b = actor + 0x7C;
    put16(&mut ram, b + 0x10, template_delta(&SUMMON_FLASH_IN) as u16);
    put16(&mut ram, b + 0x18, 1);
    put16(&mut ram, b + 0x22, 1);
    // Still in the start delay: 14 of 20 left -> 6 vsyncs in.
    put16(&mut ram, b + 0x1C, 14);
    put16(&mut ram, b + 0x20, 20);
    assert_eq!(
        summon_fade(&ram),
        Some(RetailFade {
            to_white: true,
            age: 6
        })
    );
    // Landed and 14 into the hold: 20 + 20 + 14 vsyncs, less the one the
    // landing frame counts twice.
    put16(&mut ram, b + 0x1C, 0);
    put16(&mut ram, b + 0x20, (-14i16) as u16);
    assert_eq!(summon_fade(&ram).map(|f| f.age), Some(53));
    // A killed block is not the live flash.
    put32(&mut ram, actor + 0x10, ACTOR_DONE);
    assert_eq!(summon_fade(&ram), None);
}

/// The frame step comes off the duration history's longest entry, and
/// the displayed frame is two steps behind the RAM.
#[test]
fn the_frame_step_and_display_lag_come_off_the_history() {
    let mut ram = vec![0u8; 0x20_0000];
    put16(&mut ram, STEP_MODE, 0x10);
    for (i, d) in [296u16, 310, 0x136].into_iter().enumerate() {
        put16(&mut ram, FRAME_HISTORY + i as u32 * 2, d);
    }
    assert_eq!(frame_step(&ram), 2);
    assert_eq!(display_lag_vsyncs(&ram), 4);
    put16(&mut ram, FRAME_HISTORY + 6, 0x210);
    assert_eq!(frame_step(&ram), 3);
    put16(&mut ram, STEP_MODE, 0);
    assert_eq!(frame_step(&ram), 1, "a non-adaptive mode steps one vsync");
    put32(&mut ram, FORCED_STEP, 2);
    assert_eq!(frame_step(&ram), 2, "a forced step skips the history");
}

/// The glide table's in-flight records come out with the display lag
/// taken off `elapsed` (`nivora_duel_mid_blazing_slash`'s plaque and bar
/// read ten of sixteen, six on screen at step 2), a landed record
/// (`total == 0`) is skipped, and the list survives its env form.
#[test]
fn hud_glides_come_out_lag_corrected_and_round_trip() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let rec = |slot: u32| ctx + HUD_GLIDE_TABLE + slot * HUD_GLIDE_STRIDE;
    // slot 0: the readout bar, (16, 234) -> (16, 192), ten in.
    ram[(rec(0) & 0x1F_FFFF) as usize] = 16;
    ram[(rec(0) & 0x1F_FFFF) as usize + 1] = 10;
    put16(&mut ram, rec(0) + 4, 16);
    put16(&mut ram, rec(0) + 6, 192);
    // slot 1: the actor plaque, three in - younger than the lag.
    ram[(rec(1) & 0x1F_FFFF) as usize] = 16;
    ram[(rec(1) & 0x1F_FFFF) as usize + 1] = 3;
    put16(&mut ram, rec(1) + 4, 16);
    put16(&mut ram, rec(1) + 6, 12);
    let seats = hud_glide_seats(&ram, ctx, 4);
    assert_eq!(
        seats,
        vec![
            HudGlideSeat {
                target: [16, 192],
                elapsed: 6,
                total: 16
            },
            HudGlideSeat {
                target: [16, 12],
                elapsed: 0,
                total: 16
            },
        ]
    );
    assert_eq!(
        HudGlideSeat::list_from_env(&HudGlideSeat::to_env(&seats)),
        seats
    );
}

#[test]
fn the_phase_gate_round_trips_through_its_env_form() {
    for g in [
        PhaseGate {
            action_state: 0x33,
            fade: None,
            module_phase: None,
            cam_accum: None,
            walk_yaw: None,
            done_hold: None,
            module_countdown: None,
        },
        PhaseGate {
            action_state: 0x35,
            fade: Some(RetailFade {
                to_white: false,
                age: 24,
            }),
            module_phase: None,
            cam_accum: None,
            walk_yaw: None,
            done_hold: None,
            module_countdown: None,
        },
        PhaseGate {
            action_state: 0x36,
            fade: None,
            module_phase: Some(6),
            cam_accum: Some(72),
            walk_yaw: Some(1497),
            done_hold: None,
            module_countdown: Some(-16),
        },
        PhaseGate {
            action_state: 0x51,
            fade: None,
            module_phase: None,
            cam_accum: None,
            walk_yaw: None,
            done_hold: Some(104),
            module_countdown: None,
        },
    ] {
        assert_eq!(PhaseGate::from_env(&g.to_env()), Some(g));
    }
}

#[test]
fn bar_seeds_round_trip_through_the_child_env() {
    let seeds = vec![
        BarSeed {
            slot: 0,
            hp: 412,
            mp: 37,
            ground: None,
            facing: None,
            defeat_lanes: None,
            status: 0,
        },
        BarSeed {
            slot: 4,
            hp: 1,
            mp: 0,
            ground: Some([-4, -707]),
            facing: Some(0x9F0),
            defeat_lanes: Some(0),
            status: 0,
        },
        BarSeed {
            slot: 2,
            hp: 330,
            mp: 0,
            ground: Some([10, 20]),
            facing: None,
            defeat_lanes: None,
            status: 0x1,
        },
        BarSeed {
            slot: 3,
            hp: 9,
            mp: 1,
            ground: None,
            facing: None,
            defeat_lanes: Some(0xd0d),
            status: 0x1C10,
        },
    ];
    assert_eq!(bar_seeds_from_env(&bar_seeds_to_env(&seeds)), seeds);
    assert!(bar_seeds_from_env("").is_empty());
}

#[test]
fn only_the_tracked_ailments_seed() {
    // Rage's delegation group stays behind; Venom and a Rot limb pass.
    assert_eq!(seedable_status_bits(0x0380 | 0x0001 | 0x0010), 0x0011);
    assert_eq!(seedable_status_bits(0x0380), 0);
}

#[test]
fn the_battle_drive_round_trips_through_its_env_form() {
    for d in [
        BattleDrive::Opening {
            swept: true,
            entry: 0xAF,
        },
        BattleDrive::Menu {
            flow: BattleFlowState::ArtsCommandEntry,
            seat: 2,
        },
        BattleDrive::Menu {
            flow: BattleFlowState::CommitBegin,
            seat: 1,
        },
        BattleDrive::Action {
            seat: 3,
            state: 0x6F,
            category: 2,
            queued: 0x7A,
            spare: false,
            absorbed: 0,
            end: SpanGate::Exit { phase: 3 },
            style: None,
            steer: ActionSteer::default(),
        },
        BattleDrive::Action {
            seat: 0,
            state: 0x52,
            category: 3,
            queued: 0x0F,
            spare: true,
            absorbed: 1,
            end: SpanGate::DoneHold { timer: -1 },
            style: None,
            steer: ActionSteer::default(),
        },
        BattleDrive::Action {
            seat: 0,
            state: 0x5A,
            category: 3,
            queued: 0x0D,
            spare: true,
            absorbed: 0,
            end: SpanGate::Results { hold: 80 },
            style: None,
            steer: ActionSteer::default(),
        },
        BattleDrive::Action {
            seat: 3,
            state: 0x6F,
            category: 2,
            queued: 0xAD,
            spare: false,
            absorbed: 0,
            end: SpanGate::CaptureFade {
                height: 0xFF40,
                accum: 344,
                arm: Some((1, 496)),
                yaw: Some(0x83C),
            },
            style: None,
            steer: ActionSteer::default(),
        },
        BattleDrive::Action {
            seat: 3,
            state: 0x19,
            category: 3,
            queued: 0x08,
            spare: false,
            absorbed: 0,
            end: SpanGate::Age { accum: 552 },
            style: Some(3),
            steer: ActionSteer {
                target: Some(4),
                yaw: Some(0xA98),
                message: Some((0x801C_ED18, 25)),
                plate_cleared: true,
                ..ActionSteer::default()
            },
        },
        BattleDrive::Action {
            seat: 0,
            state: 0x20,
            category: 3,
            queued: 0x0F,
            spare: false,
            absorbed: 0,
            end: SpanGate::Age { accum: 176 },
            style: Some(0),
            steer: ActionSteer {
                yaw: Some(0x280),
                arts: true,
                gauge: Some(153),
                clip: Some(0x11),
                aim: Some(1),
                queue: Some([
                    0x0D, 0x0F, 0x0E, 0x19, 0x27, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ]),
                cursor: Some(5),
                fading: Some((3, 0xF0)),
                spoils: true,
                ..ActionSteer::default()
            },
        },
    ] {
        assert_eq!(BattleDrive::from_env(&d.to_env()), Some(d));
    }
    assert_eq!(BattleDrive::from_env("menu,40"), None);
}

/// A committed arts queue reads back as the arrows that were entered:
/// swings as they stand, each starter + art pair as its art's last arrow.
/// `battle_melee_hit_spark`'s `0D 0F 0E 19 27` is Right Up Down Up when
/// art `0x27` is Up Down Up; a constant with no known combo (a Super
/// tail) or a stray byte leaves it unrecoverable.
#[test]
fn entered_arrows_undo_the_queue_builder() {
    use legaia_art::Command::{Down, Up};
    let combo = |a: u8| (a == 0x27).then(|| vec![Up, Down, Up]);
    let mut q = [0u8; 16];
    q[..5].copy_from_slice(&[0x0D, 0x0F, 0x0E, 0x19, 0x27]);
    let mut want = [0u8; 16];
    want[..4].copy_from_slice(&[0x0D, 0x0F, 0x0E, 0x0F]);
    assert_eq!(entered_arrows(&q, combo), Some(want));
    q[3] = 0x1A; // a newly-learned starter reads the same
    assert_eq!(entered_arrows(&q, combo), Some(want));
    q[4] = 0x2B;
    assert_eq!(entered_arrows(&q, combo), None, "unknown art");
    assert_eq!(entered_arrows(&[0u8; 16], combo), None, "empty queue");
    let mut stray = [0u8; 16];
    stray[0] = 0x2B;
    assert_eq!(entered_arrows(&stray, combo), None, "bare constant");
}

/// Taking an absorbed Seru back off a list undoes the Done band's
/// prepend exactly - ids, levels and the XP words - and leaves a list
/// without it alone.
#[test]
fn unlearning_undoes_the_absorb_grant() {
    use legaia_engine_core::magic_xp::learn_spell_prepend;
    let mut rec = legaia_save::CharacterRecord::zeroed();
    learn_spell_prepend(&mut rec, 0x83);
    learn_spell_prepend(&mut rec, 0x85);
    rec.raw[0x8..0xC].copy_from_slice(&7u32.to_le_bytes());
    rec.raw[0xC..0x10].copy_from_slice(&9u32.to_le_bytes());
    let before = rec.raw.clone();
    learn_spell_prepend(&mut rec, 0x81);
    unlearn_spell(&mut rec, 0x81);
    assert_eq!(rec.raw, before);
    unlearn_spell(&mut rec, 0x8A);
    assert_eq!(rec.raw, before);
}

/// The seed plan follows the flow byte: the entry band opens, a cast in
/// the summon band replays, any other `0xFF` action is driven, the
/// selection band above the prompt is driven, the prompt parks.
#[test]
fn the_seed_plan_follows_the_flow_byte() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 1);
    put8(&mut ram, ctx + 1, 1);
    put8(&mut ram, FORMATION_CELL, 0x4F);
    let plan = |ram: &mut Vec<u8>, flow: u8, state: u8| {
        put8(ram, ctx + 6, flow);
        put8(ram, ctx + 7, state);
        RetailBattle::from_ram(ram).expect("seedable").seed_plan()
    };
    for flow in OPENING_FLOWS {
        assert_eq!(plan(&mut ram, flow, 0), SeedPlan::Opening);
    }
    assert_eq!(plan(&mut ram, 0x1E, 0), SeedPlan::Prompt);
    assert_eq!(
        plan(&mut ram, 0x50, 0),
        SeedPlan::Menu {
            flow: BattleFlowState::ArtsCommandEntry,
            seat: 0
        }
    );
    assert_eq!(
        plan(&mut ram, 0xFF, 0x1E),
        SeedPlan::Action {
            seat: 0,
            state: 0x1E
        }
    );
}

#[test]
fn a_monster_pool_slot_maps_onto_the_engine_seating() {
    assert_eq!(engine_seat(1, 2), 1);
    assert_eq!(engine_seat(3, 1), 1);
    assert_eq!(engine_seat(4, 2), 3);
    assert_eq!(engine_seat(3, 3), 3);
}

#[test]
fn a_loading_fight_names_its_reason() {
    let ram = vec![0u8; 0x20_0000];
    let err = RetailBattle::from_ram(&ram).unwrap_err();
    assert!(err.contains("not resident"), "{err}");
}

#[test]
fn combatant_score_skips_a_missing_engine_max_mp() {
    let r = Combatant {
        hp: 10,
        hp_max: 20,
        mp: 5,
        mp_max: 9,
    };
    let e = Combatant { mp_max: 0, ..r };
    let (s, _) = combatant_score(&[Some(r)], &[e], "m", &[]);
    assert_eq!(s, 1.0);
    let (s, d) = combatant_score(&[Some(r)], &[], "p", &[]);
    assert_eq!(s, 0.0, "{d}");
}

/// The image child's glide-origin pair survives its env round trip.
#[test]
fn cam_align_env_round_trips() {
    use legaia_engine_vm::battle_cam_script::BattleCamPose;
    let live = BattleCamPose {
        pitch: 128.0,
        yaw: 3205.0,
        tr: [0.0, 972.0, 2867.0],
        focus: [-11.0, 0.0, -381.0],
    };
    let end = BattleCamPose {
        pitch: 128.0,
        yaw: 4062.0,
        tr: [0.0, 1024.0, 4915.0],
        focus: [-7.0, 0.0, -331.0],
    };
    let env = cam_align_to_env(&(live, end));
    assert_eq!(cam_align_from_env(&env), Some((live, end)));
    assert_eq!(cam_align_from_env("1,2,3"), None);
}
