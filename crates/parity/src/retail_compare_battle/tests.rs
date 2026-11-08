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
    use legaia_engine_battle_vm::battle_action::SUMMON_FLASH_IN;
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
        summon_fade_on(&ram, false),
        Some(RetailFade {
            to_white: true,
            age: 6,
            spawn_step: None,
        })
    );
    // Landed and 14 into the hold: 20 + 20 + 14 vsyncs, less the one the
    // landing frame counts twice.
    put16(&mut ram, b + 0x1C, 0);
    put16(&mut ram, b + 0x20, (-14i16) as u16);
    assert_eq!(summon_fade_on(&ram, false).map(|f| f.age), Some(53));
    // At frame step 3 a landing overshoots the delay to `-1`:
    // `nighto_summon_mid_cast`'s block (delay `-1`, duration `14`) is 21
    // vsyncs to the landing and two passes more, `27 - 3 = 24` on the
    // battle-frame clock, and the per-vsync reading's `25`.
    put16(&mut ram, STEP_MODE, 0x10);
    put16(&mut ram, FRAME_HISTORY, 0x200);
    assert_eq!(frame_step(&ram), 3);
    put16(&mut ram, b + 0x1C, (-1i16) as u16);
    put16(&mut ram, b + 0x20, 14);
    assert_eq!(summon_fade_on(&ram, true).map(|f| f.age), Some(24));
    assert_eq!(summon_fade_on(&ram, false).map(|f| f.age), Some(25));
    // A killed block is not the live flash.
    put32(&mut ram, actor + 0x10, ACTOR_DONE);
    assert_eq!(summon_fade(&ram), None);
}

/// A `0x35` flash-out's spawn step is its age less what the sustain word
/// drained: `zenoir_summon_mid_cast`'s block (`96` of `0x78` left, age `24`)
/// beside the word's `97` spawned on a step-`1` pass. A word a module
/// reseeded reads no step.
#[test]
fn reads_the_flash_outs_spawn_step_off_the_sustain_word() {
    use legaia_engine_battle_vm::battle_action::SUMMON_FLASH_OUT;
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800F_0000;
    put32(&mut ram, BATTLE_CTX, ctx);
    ram[(ctx + 7) as usize & 0x1F_FFFF] = 0x35;
    put16(&mut ram, ctx + 0x6D8, 97);
    let actor = 0x8008_2AEC;
    put32(&mut ram, actor + 0x0C, FADE_ACTOR_TICK);
    let b = actor + 0x7C;
    put16(&mut ram, b + 0x10, template_delta(&SUMMON_FLASH_OUT) as u16);
    put16(&mut ram, b + 0x18, 1);
    put16(&mut ram, b + 0x22, 1);
    put16(&mut ram, b + 0x20, 96);
    assert_eq!(
        summon_fade_on(&ram, true),
        Some(RetailFade {
            to_white: false,
            age: 24,
            spawn_step: Some(1),
        })
    );
    put16(&mut ram, ctx + 0x6D8, 140);
    assert_eq!(summon_fade_on(&ram, true).and_then(|f| f.spawn_step), None);
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

/// The fade's own table places the displayed fade: under the current
/// record the two-pass law, under the other record (the current one still
/// empty of it) one pass, and the law again when no table links one.
#[test]
fn the_fade_lag_follows_the_table_that_links_the_fade() {
    let mut ram = vec![0u8; 0x20_0000];
    put32(&mut ram, FORCED_STEP, 2);
    let ots = [0x8010_0000u32, 0x8011_0000];
    for (i, &ot) in ots.iter().enumerate() {
        put32(&mut ram, 0x8007_BF30 + i as u32 * 0x74 + 0x70, ot);
        // An empty bucket 1 links straight to bucket 0.
        put32(&mut ram, ot + 4, ot & 0x00FF_FFFF);
    }
    let link_fade = |ram: &mut Vec<u8>, ot: u32, packet: u32| {
        put32(ram, ot + 4, packet & 0x00FF_FFFF);
        put32(ram, packet, (5 << 24) | (ot & 0x00FF_FFFF));
        // POLY_F4, semi-transparent, raw: the fade's flash-out at 199.
        put32(ram, packet + 4, 0x2BC7_C7C7);
    };
    assert_eq!(fade_passes_back(&ram), None);
    assert_eq!(displayed_fade_lag_vsyncs(&ram), display_lag_vsyncs(&ram));
    put16(&mut ram, 0x8007_B74C, 0);
    link_fade(&mut ram, ots[1], 0x8012_0000);
    assert_eq!(fade_passes_back(&ram), Some(1));
    assert_eq!(displayed_fade_lag_vsyncs(&ram), 2);
    link_fade(&mut ram, ots[0], 0x8012_0100);
    assert_eq!(fade_passes_back(&ram), Some(2));
    assert_eq!(displayed_fade_lag_vsyncs(&ram), 4);
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
fn a_loading_fight_names_its_reason() {
    let ram = vec![0u8; 0x20_0000];
    let err = RetailBattle::from_ram(&ram).unwrap_err();
    assert!(err.contains("not resident"), "{err}");
}

#[test]
fn combatant_score_counts_a_zero_max_mp() {
    let r = Combatant {
        hp: 10,
        hp_max: 20,
        mp: 0,
        mp_max: 0,
    };
    let (s, _) = combatant_score(&[Some(r)], &[r], "p", &[]);
    assert_eq!(s, 1.0);
    let e = Combatant { mp_max: 602, ..r };
    let (s, d) = combatant_score(&[Some(r)], &[e], "p", &[]);
    assert_eq!(s, 0.75, "{d}");
    let (s, d) = combatant_score(&[Some(r)], &[], "p", &[]);
    assert_eq!(s, 0.0, "{d}");
}

/// The `Begin` chip's seat names the glide's `elapsed`: `koru_strip_forced`'s
/// older ordering table draws the left cap on `x = 101` (fourteen vsyncs in)
/// and its displayed frame on `x = 106` (twelve).
#[test]
fn begin_chip_seat_names_the_prompt_glide_elapsed() {
    assert_eq!(begin_chip_elapsed(142), Some(0));
    assert_eq!(begin_chip_elapsed(101 + 8), Some(14));
    assert_eq!(begin_chip_elapsed(106 + 8), Some(12));
    assert_eq!(begin_chip_elapsed(104), Some(16));
    assert_eq!(begin_chip_elapsed(103), None);
    // A RAM image with no ordering table yields no seat.
    assert_eq!(round_prompt_pool_seat(&vec![0u8; 0x20_0000], 2), None);
}

/// `nivora_duel_mid_blazing_slash`'s tween table: yaw `4640` closing on
/// `4051` by `59` a pass, TR z `4368` on `4915` by `55`, at frame step `2`.
/// Two passes back the walker stood one table step (`2 * 59`) plus one
/// re-arm step on the wider gap (`2 * ceil(707 / 10)`) further off.
#[test]
fn the_displayed_camera_stands_two_passes_short() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let rec = |k: u32| ctx + camera_align::TWEEN_TABLE + k * 4;
    put16(&mut ram, 0x8007_B792, 4640);
    put16(&mut ram, rec(1), 59);
    put16(&mut ram, rec(1) + 2, 4051);
    put32(&mut ram, 0x8008_40C0, 4368);
    put16(&mut ram, rec(5), 55);
    put16(&mut ram, rec(5) + 2, 4915);
    let p = camera_align::displayed_cam_live(&ram, ctx, 2, 2, [None; 9]);
    assert_eq!(p.yaw, (544 + 118 + 142) as f32);
    assert_eq!(p.tr[2], (4368 - 110 - 2 * 66) as f32);
    // No lag, no walk back.
    assert_eq!(
        camera_align::displayed_cam_live(&ram, ctx, 0, 2, [None; 9]).yaw,
        544.0
    );
}

/// `battle_garmel_zenoir_fire_claw_mid_cast`: the animation chain's
/// terminator pass armed no framing, so the RAM's yaw increment `12` with
/// `70` to go at step `3` is the pass before's re-arm (`ceil(142 / 12)`), not
/// this pass's (`ceil(106 / 12) = 9`), and both lag passes walked it.
#[test]
fn the_chain_terminator_pass_walks_back_on_the_table_it_found() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let rec = |k: u32| ctx + camera_align::TWEEN_TABLE + k * 4;
    put16(&mut ram, 0x8007_B792, 3311);
    put16(&mut ram, rec(1), 12);
    put16(&mut ram, rec(1) + 2, 3381);
    put32(&mut ram, 0x8008_40C0, 5619);
    put16(&mut ram, rec(5), 21);
    put16(&mut ram, rec(5) + 2, 5734);
    assert!(camera_align::chain_terminator_pass(&ram, ctx, 0x2B, 3));
    // Not outside `0x2B`, and not where this pass's own re-arm fits.
    assert!(!camera_align::chain_terminator_pass(&ram, ctx, 0x2A, 3));
    put16(&mut ram, rec(1), 9);
    put16(&mut ram, rec(5), 10);
    assert!(!camera_align::chain_terminator_pass(&ram, ctx, 0x2B, 3));
    put16(&mut ram, rec(1), 12);
    put16(&mut ram, rec(5), 21);
    let p = camera_align::displayed_module_cam_live(&ram, ctx, &[3, 3]);
    assert_eq!(p.yaw, 3239.0);
    assert_eq!(p.tr[2], 5493.0);
}

/// `battle_map01_frog_strike`: one pass past the strike commit its eye walks
/// the pull-in's first table, so the frame on screen still stands on the depth
/// the shot had landed on (`4915`), and walks no further back than that.
#[test]
fn a_monsters_pull_in_walks_back_no_further_than_the_landed_depth() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let rec = |k: u32| ctx + camera_align::TWEEN_TABLE + k * 4;
    put8(&mut ram, ctx + 7, 0x1E);
    put8(&mut ram, ctx + 0x13, 3);
    put8(&mut ram, ctx + 0x243, 1);
    put16(&mut ram, ctx + 0x6D0, 3072);
    put32(&mut ram, 0x8008_40BC, 1292);
    put16(&mut ram, rec(4), 6);
    put16(&mut ram, rec(4) + 2, 1344);
    put32(&mut ram, 0x8008_40C0, 4587);
    put16(&mut ram, rec(5), 164);
    put16(&mut ram, rec(5) + 2, 2948);
    let prior = camera_align::strike_pull_in_prior_end(&ram, ctx);
    assert_eq!(prior[5], Some(4915));
    assert_eq!(prior[4], Some(1280));
    let p = camera_align::displayed_cam_live(&ram, ctx, 2, 2, prior);
    assert_eq!(p.tr[2], 4915.0);
    assert_eq!(p.tr[1], 1280.0);
}

/// `battle_map01_gomboo_strike`'s words: the RAM holds the arrival's idle stage
/// one cursor past its commit, so the strike loop's first arm was one pass ago
/// and the frame on screen stands on the walk's last pass, under case 6's aim,
/// not `2 * 3` vsyncs back along case 7's table.
#[test]
fn a_frame_before_the_strike_loops_first_arm_stands_on_case_six() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let (attacker, target) = (0x800E_D264u32, 0x800E_C9E8u32);
    let rec = |k: u32| ctx + camera_align::TWEEN_TABLE + k * 4;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx + 7, 0x1E);
    put8(&mut ram, ctx + 0xD, 1);
    put8(&mut ram, ctx + 0x13, 4);
    put8(&mut ram, ctx + 0x15, 1);
    put16(&mut ram, ctx + 0x6DA, 578);
    put32(&mut ram, ACTOR_TABLE + 4 * 4, attacker);
    put32(&mut ram, ACTOR_TABLE, target);
    put8(&mut ram, attacker + 0x1DE, 3);
    for (k, (x, z)) in [
        (116i32, 266i32),
        (116, 266),
        (92, 234),
        (127, 324),
        (163, 414),
    ]
    .into_iter()
    .enumerate()
    {
        put32(&mut ram, attacker + POSITION_RING + k as u32 * 8, x as u32);
        put32(
            &mut ram,
            attacker + POSITION_RING + k as u32 * 8 + 4,
            z as u32,
        );
    }
    put16(&mut ram, attacker + 0x17A, 24);
    put16(&mut ram, attacker + 0x17C, 0);
    put32(&mut ram, attacker + 0x234, 0x800F_0000);
    put32(&mut ram, attacker + 0x238, 0x800F_0000);
    put32(&mut ram, attacker + 0x23C, 0x800F_0100);
    // Frame step `3`.
    put16(&mut ram, frame_step::STEP_MODE, 0x10);
    put16(&mut ram, frame_step::FRAME_HISTORY, 0x28E);
    assert_eq!(frame_step(&ram), 3);
    // Yaw, then the focus trio (stored negated).
    put16(&mut ram, 0x8007_B792, 3927);
    put16(&mut ram, rec(1), 157);
    put16(&mut ram, rec(1) + 2, 2523);
    put32(&mut ram, 0x8008_9118, (-155i32) as u32);
    put16(&mut ram, rec(6), 14);
    put16(&mut ram, rec(6) + 2, (-30i16) as u16);
    put32(&mut ram, 0x8008_9120, (-389i32) as u32);
    put16(&mut ram, rec(8), 30);
    put16(&mut ram, rec(8) + 2, (-128i16) as u16);

    let prior = camera_align::walk_arm_prior_end(&ram, ctx).expect("the second pass");
    assert_eq!(prior[6], Some(-92));
    assert_eq!(prior[8], Some(-234));
    let p = camera_align::displayed_cam_live_steps(&ram, ctx, &[3, 3], true, prior, [0; 9], true);
    assert_eq!((p.focus[0], p.focus[2]), (233.0, 563.0));
    assert!((280.0..=300.0).contains(&p.yaw), "yaw {}", p.yaw);

    // The commit a pass further on: the frame stands on case 7's own table.
    put16(&mut ram, attacker + 0x17C, 24);
    assert!(camera_align::walk_arm_prior_end(&ram, ctx).is_none());
}

/// `battle_map02_lip_kid_strike`'s words: the RAM's pass committed the
/// arrival's idle stage (record ring `A, B, B`, cursor ring `0, 408`) with no
/// swing staged (strike cursor `0`). Its table is case 6's on ring slot `1`,
/// and each pass before it chased the walker a ring stride further back.
#[test]
fn a_capture_on_the_arrivals_own_pass_walks_back_onto_case_six() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let (attacker, target) = (0x800E_D538u32, 0x800E_C9E8u32);
    let rec = |k: u32| ctx + camera_align::TWEEN_TABLE + k * 4;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx + 7, 0x1E);
    put8(&mut ram, ctx + 0xD, 1);
    put8(&mut ram, ctx + 0x13, 4);
    put8(&mut ram, ctx + 0x15, 0);
    put16(&mut ram, ctx + 0x6DA, 588);
    put32(&mut ram, ACTOR_TABLE + 4 * 4, attacker);
    put32(&mut ram, ACTOR_TABLE, target);
    put8(&mut ram, attacker + 0x1DE, 3);
    for (k, (x, z)) in [
        (93i32, 244i32),
        (83, 224),
        (117, 315),
        (151, 406),
        (185, 496),
    ]
    .into_iter()
    .enumerate()
    {
        put32(&mut ram, attacker + POSITION_RING + k as u32 * 8, x as u32);
        put32(
            &mut ram,
            attacker + POSITION_RING + k as u32 * 8 + 4,
            z as u32,
        );
    }
    put16(&mut ram, attacker + 0x17A, 0);
    put16(&mut ram, attacker + 0x17C, 408);
    put32(&mut ram, attacker + 0x234, 0x800F_0000);
    put32(&mut ram, attacker + 0x238, 0x800F_0100);
    put32(&mut ram, attacker + 0x23C, 0x800F_0100);
    // Frame step `3`.
    put16(&mut ram, frame_step::STEP_MODE, 0x10);
    put16(&mut ram, frame_step::FRAME_HISTORY, 0x273);
    assert_eq!(frame_step(&ram), 3);
    put16(&mut ram, 0x8007_B792, 353);
    put16(&mut ram, rec(1), 1);
    put16(&mut ram, rec(1) + 2, 357);
    put32(&mut ram, 0x8008_9118, (-177i32) as u32);
    put16(&mut ram, rec(6), 11);
    put16(&mut ram, rec(6) + 2, (-83i16) as u16);
    put32(&mut ram, 0x8008_9120, (-483i32) as u32);
    put16(&mut ram, rec(8), 29);
    put16(&mut ram, rec(8) + 2, (-224i16) as u16);

    let shift = camera_align::arrival_end_shift(&ram, ctx).expect("the arrival's pass");
    assert_eq!((shift[6], shift[8]), (-34, -91));
    let p =
        camera_align::displayed_cam_live_steps(&ram, ctx, &[3, 3], true, [None; 9], shift, false);
    assert_eq!((p.focus[0], p.focus[2]), (243.0, 657.0));
    // On the RAM's endpoint alone the walk back overshoots.
    let q =
        camera_align::displayed_cam_live_steps(&ram, ctx, &[3, 3], true, [None; 9], [0; 9], false);
    assert_eq!((q.focus[0], q.focus[2]), (255.0, 687.0));

    // A swing already staged on that pass is not the arrival's.
    put8(&mut ram, ctx + 0x15, 1);
    assert!(camera_align::arrival_end_shift(&ram, ctx).is_none());
}

/// `battle_korb2_warhalf_strike`'s words: the walk's last pass read the
/// attacker off ring slot `3`, heading `1836` onto the target's live pair, and
/// stood it on `(-96, 287)`, slot `2`. The loop's first arm read that heading,
/// so the endpoint that pass chased stood `112` short of the RAM's (the bearing
/// `1728` onto the body pair, a counter step at step `3`), and the walk back
/// lands on yaw `795` where the RAM's endpoint alone gives `759`.
#[test]
fn the_strike_loops_first_arm_chased_the_walks_heading() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let (attacker, target) = (0x800E_D264u32, 0x800E_C9E8u32);
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx + 7, 0x1E);
    put8(&mut ram, ctx + 0x13, 3);
    put8(&mut ram, ctx + 0x15, 1);
    put32(&mut ram, ACTOR_TABLE + 3 * 4, attacker);
    put32(&mut ram, ACTOR_TABLE, target);
    put16(&mut ram, attacker + 0x46, 1730);
    for (k, (x, z)) in [(-106i32, 309i32), (-101, 298), (-96, 287), (-112, 332)]
        .into_iter()
        .enumerate()
    {
        put32(&mut ram, attacker + POSITION_RING + k as u32 * 8, x as u32);
        put32(
            &mut ram,
            attacker + POSITION_RING + k as u32 * 8 + 4,
            z as u32,
        );
    }
    // Frame step `3`.
    put16(&mut ram, frame_step::STEP_MODE, 0x10);
    put16(&mut ram, frame_step::FRAME_HISTORY, 0x28E);
    assert_eq!(frame_step(&ram), 3);
    put16(&mut ram, target + 0x3C, 60);
    put16(&mut ram, target + 0x40, (-5i16) as u16);
    let h = camera_align::walk_heading(&ram, ctx).expect("second pass");
    assert_eq!(h.displayed, 1836);
    let shift = h.end_shift;
    assert_eq!(shift, -112);

    let rec = |k: u32| ctx + camera_align::TWEEN_TABLE + k * 4;
    put16(&mut ram, 0x8007_B792, 4135);
    put16(&mut ram, rec(1), 103);
    put16(&mut ram, rec(1) + 2, 3218);
    let mut end_shift = [0; 9];
    end_shift[1] = shift;
    let walked = camera_align::displayed_cam_live_steps(
        &ram,
        ctx,
        &[3, 3],
        false,
        [None; 9],
        end_shift,
        false,
    );
    assert_eq!(walked.yaw, 795.0);

    // A nearest slot `3` is the loop's third pass: the displayed frame's
    // arm already read the loop's own heading.
    put32(&mut ram, attacker + POSITION_RING + 3 * 8 + 4, 200);
    put32(&mut ram, attacker + POSITION_RING + 4 * 8 + 4, 400);
    assert!(camera_align::walk_heading(&ram, ctx).is_none());
}

/// `battle_geremi_ogre_strike`'s words: a walker the `+0x76` lock keeps on the
/// walk clip into the strike loop, nearest the target on ring slot `3` and
/// backing off `13` units a pass since. The displayed frame's pass stored the
/// walk's bearing onto the target's live pair (`2483`) and the loop's first arm
/// read it, so the table the RAM replaced chased an endpoint `67` further than
/// the RAM's (the loop's bearing `2420` onto the body pair, a counter step at
/// step `3`): the walk back lands on yaw `189`, where the frame reads best,
/// and the RAM's endpoint alone gives `168`.
#[test]
fn a_locked_walk_nearest_on_slot_three_chased_the_walks_heading() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let (attacker, target) = (0x800E_D538u32, 0x800E_CF90u32);
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx + 7, 0x1E);
    put8(&mut ram, ctx + 0x13, 4);
    put8(&mut ram, ctx + 0x15, 1);
    put32(&mut ram, ACTOR_TABLE + 4 * 4, attacker);
    put32(&mut ram, ACTOR_TABLE + 2 * 4, target);
    put8(&mut ram, attacker + 0x1DD, 2);
    put8(&mut ram, attacker + 0x1D9, 1);
    put16(&mut ram, attacker + 0x46, 2423);
    for (k, (x, z)) in [
        (210i32, 271i32),
        (203, 260),
        (196, 249),
        (188, 239),
        (226, 287),
    ]
    .into_iter()
    .enumerate()
    {
        put32(&mut ram, attacker + POSITION_RING + k as u32 * 8, x as u32);
        put32(
            &mut ram,
            attacker + POSITION_RING + k as u32 * 8 + 4,
            z as u32,
        );
    }
    put16(&mut ram, frame_step::STEP_MODE, 0x10);
    put16(&mut ram, frame_step::FRAME_HISTORY, 0x28E);
    assert_eq!(frame_step(&ram), 3);
    put16(&mut ram, target + 0x3C, 19);
    put16(&mut ram, target + 0x40, (-27i16) as u16);
    let h = camera_align::walk_heading(&ram, ctx).expect("locked walk, slot 3");
    assert_eq!(h.displayed, 2483);
    assert_eq!(h.end_shift, -67);

    let rec = |k: u32| ctx + camera_align::TWEEN_TABLE + k * 4;
    put16(&mut ram, 0x8007_B792, 3520);
    put16(&mut ram, rec(1), 106);
    put16(&mut ram, rec(1) + 2, 2568);
    let mut end_shift = [0; 9];
    end_shift[1] = h.end_shift;
    let walked = |end_shift| {
        camera_align::displayed_cam_live_steps(
            &ram,
            ctx,
            &[3, 3],
            false,
            [None; 9],
            end_shift,
            false,
        )
        .yaw
    };
    assert_eq!(walked(end_shift), 189.0);
    assert_eq!(walked([0; 9]), 168.0);

    // Off the walk clip the same ring is an arrival two passes into the loop,
    // which read the loop's own heading (`battle_noaru_gilium_lv2_strike`).
    put8(&mut ram, attacker + 0x1D9, 8);
    assert!(camera_align::walk_heading(&ram, ctx).is_none());

    // A swing committed on the RAM's pass alone, the walk's record held on
    // slots `1..=3` (`battle_map03_moldy_worm_strike`): the displayed frame's
    // pass still ran the walk.
    let record = |k: u32| attacker + ram_read::RING_RECORD + k * 4;
    put32(&mut ram, record(0), 0x800B_21E4);
    for k in 1..=4 {
        put32(&mut ram, record(k), 0x800A_DFD8);
    }
    let h = camera_align::walk_heading(&ram, ctx).expect("swing committed on the RAM's pass");
    assert_eq!(h.displayed, 2483);
    // ... but not when the record changed by slot `2`.
    put32(&mut ram, record(2), 0x800B_21E4);
    assert!(camera_align::walk_heading(&ram, ctx).is_none());
}

/// Case `0xA`'s yaw clock: the endpoint climbs twice the cursor's step a pass,
/// so the walk back from Poisonous Breath's `3350` under `3368` lands on the
/// `3320` its frame fits best, where the RAM's endpoint alone gives `3296`.
#[test]
fn a_clip_clock_endpoint_walks_back_against_the_cursor() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let rec = |k: u32| ctx + camera_align::TWEEN_TABLE + k * 4;
    let actor = 0x800F_0000;
    put8(&mut ram, ctx + 7, 0x2B);
    put8(&mut ram, ctx + 0x13, 4);
    put32(&mut ram, ACTOR_TABLE + 4 * 4, actor);
    put16(&mut ram, actor + 0x46, 2640);
    for (k, cursor) in [72u16, 60, 48].into_iter().enumerate() {
        put16(&mut ram, actor + 0x17A + k as u32 * 2, cursor);
    }
    put16(&mut ram, 0x8007_B792, 3350);
    put16(&mut ram, rec(1), 6);
    put16(&mut ram, rec(1) + 2, 3368);
    let shift = camera_align::clip_clock_end_shift(&ram, ctx).expect("the case's law");
    assert_eq!(shift, -24);
    let mut end_shift = [0; 9];
    end_shift[1] = shift;
    let walked = camera_align::displayed_cam_live_steps(
        &ram,
        ctx,
        &[3, 3],
        false,
        [None; 9],
        end_shift,
        false,
    );
    assert_eq!(walked.yaw, 3320.0);

    // Another shot's table: the endpoint is off the law.
    put16(&mut ram, rec(1) + 2, 3369);
    assert!(camera_align::clip_clock_end_shift(&ram, ctx).is_none());
}

/// A `0x32` capture on the summon route's first pass: the eye depth walks
/// back one pass along the close-up table (`2 * 183`) and then one `0x29`
/// step short of that table's endpoint `prescale(0xC00)`, not on past it.
/// The yaw, whose endpoint the arm did not move, walks back both passes.
#[test]
fn a_fresh_summon_route_table_walks_back_onto_the_seed_depth() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let rec = |k: u32| ctx + camera_align::TWEEN_TABLE + k * 4;
    put16(&mut ram, 0x8007_B792, 2104);
    put16(&mut ram, rec(1), 12);
    put16(&mut ram, rec(1) + 2, 1990);
    put32(&mut ram, 0x8008_40C0, 4279);
    put16(&mut ram, rec(5), 183);
    put16(&mut ram, rec(5) + 2, 2457);
    let mut prior = [None; 9];
    prior[5] = Some(4915);
    let p = camera_align::displayed_cam_live(&ram, ctx, 2, 2, prior);
    assert_eq!(p.tr[2], (4279 + 366 - 2 * 27) as f32);
    assert_eq!(p.yaw, (2104 + 24 + 28) as f32);
    // Without the prior endpoint both passes stay on the close-up table.
    let p = camera_align::displayed_cam_live(&ram, ctx, 2, 2, [None; 9]);
    assert_eq!(p.tr[2], (4279 + 366 + 2 * 219) as f32);
}

/// A spent combatant takes its captured heading into a running fight, and
/// the action's target does so only until the first strike commits (the
/// commit turns it onto the attacker, which the replay does itself).
#[test]
fn spent_combatants_keep_their_heading_until_the_strike_commits() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 1);
    put8(&mut ram, ctx + 1, 2);
    put8(&mut ram, ctx + 6, 0xFF);
    put8(&mut ram, ctx + 7, 0x1E);
    put8(&mut ram, FORMATION_CELL, 0x0D);
    put8(&mut ram, FORMATION_CELL + 1, 0x0D);
    for (slot, base) in [(0u32, 0x800E_C9E8u32), (3, 0x800E_D000), (4, 0x800E_D800)] {
        put32(&mut ram, ACTOR_TABLE + slot * 4, base);
        put16(&mut ram, base + 0x14C, 50);
        put16(&mut ram, base + 0x14E, 50);
    }
    let mut b = RetailBattle::from_ram(&ram).expect("seedable");
    b.active_actor = 0;
    b.target_code = 3;
    b.queued_category = 3;
    b.ground = vec![
        Some([-195, -214]),
        None,
        None,
        Some([5, 5]),
        Some([56, -622]),
    ];
    b.facing = vec![Some(484), None, None, Some(0x900), Some(2901)];
    b.init_keys = vec![Some(7), None, None, Some(0), Some(0)];
    // The first swing staged, not committed (`player_steal_skeleton_pre`).
    b.caster_clip = 0;
    b.strike_cursor = 1;
    let f = b.seeded_facing();
    assert_eq!(f[0], None, "the acting seat's heading is its own replay's");
    assert_eq!(f[1], Some(0x900), "the target before the strike lands");
    assert_eq!(f[2], Some(2901));
    // A committed swing: the target's heading is the commit's output.
    b.caster_clip = 0x0E;
    let f = b.seeded_facing();
    assert_eq!(f[1], None);
    assert_eq!(f[2], Some(2901));
    // A combatant whose turn is still to come keeps the engine's.
    b.init_keys[4] = Some(3);
    assert_eq!(b.seeded_facing()[2], None);
    // A Spirit turn writes no heading, so its Done tail keeps the acting
    // seat's (`nivora_duel_pre_megaton_press`: Gala at `3980`).
    b.queued_category = 4;
    b.action_state = 0x51;
    assert_eq!(b.seeded_facing()[0], Some(484));
    b.queued_category = 3;
    assert_eq!(
        b.seeded_facing()[0],
        None,
        "an attack's Done tail is the replay's"
    );
}

/// A monster's action capture replays the rest of its round on the
/// captured keys: the actor goes first, a combatant still to act keeps its
/// key, a spent monster sits out, and a spent party member replays its turn
/// first only under a cast, where the drive commits it Spirit.
#[test]
fn a_monster_capture_replays_its_round_on_the_captured_keys() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 1);
    put8(&mut ram, ctx + 1, 3);
    put8(&mut ram, ctx + 6, 0xFF);
    put8(&mut ram, ctx + 7, 0x2B);
    put8(&mut ram, ctx + 0x13, 4);
    for c in 0..3 {
        put8(&mut ram, FORMATION_CELL + c, 0x0D);
    }
    for (slot, base) in [
        (0u32, 0x800E_C9E8u32),
        (3, 0x800E_D000),
        (4, 0x800E_D800),
        (5, 0x800E_E000),
    ] {
        put32(&mut ram, ACTOR_TABLE + slot * 4, base);
        put16(&mut ram, base + 0x14C, 50);
        put16(&mut ram, base + 0x14E, 50);
    }
    let mut b = RetailBattle::from_ram(&ram).expect("seedable");
    b.active_actor = 4;
    b.queued_category = 2;
    b.init_keys = vec![Some(0), None, None, Some(40), Some(0), Some(0)];
    // Party seat 0 spent (replays Spirit first), monster 3 still to act,
    // the caster (retail 4, engine 2) above it, monster 5 spent.
    let k = b.engine_init_keys();
    assert_eq!(&k[..4], &[Some(42), Some(40), Some(41), Some(0)]);
    // Under a strike the member's replayed Attack would land twice.
    b.queued_category = 3;
    assert_eq!(b.engine_init_keys()[0], Some(0));
}

/// A counterattack capture - a party seat's strike loop inside a monster's
/// turn (`ctx[+0x274]`) - sits out every other living combatant, the live
/// keys included: they rank behind the turn the round has reached.
#[test]
fn a_counter_capture_sits_out_the_live_keys_too() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 1);
    put8(&mut ram, ctx + 1, 2);
    put8(&mut ram, ctx + 6, 0xFF);
    put8(&mut ram, ctx + 7, 0x1E);
    put8(&mut ram, ctx + 0x274, 3);
    put8(&mut ram, FORMATION_CELL, 0xA1);
    put8(&mut ram, FORMATION_CELL + 1, 0xA1);
    for (slot, base, key) in [
        (0u32, 0x800E_C9E8u32, 0u16),
        (3, 0x800E_D000, 0),
        (4, 0x800E_D800, 235),
    ] {
        put32(&mut ram, ACTOR_TABLE + slot * 4, base);
        put16(&mut ram, base + 0x14C, 50);
        put16(&mut ram, base + 0x14E, 50);
        put16(&mut ram, base + 0x16C, key);
        put8(&mut ram, base + 0x1DE, 3);
    }
    let mut b = RetailBattle::from_ram(&ram).expect("seedable");
    assert_eq!(b.turn_owner, 3);
    assert!(b.counter_capture());
    let spent = b.engine_spent_keys();
    assert!(!spent[0], "the counterer keeps its turn");
    assert!(
        spent[1] && spent[2],
        "the turn owner and the live key behind it"
    );
    // The member's own turn: a live key keeps its engine key.
    b.turn_owner = 0;
    assert!(!b.counter_capture());
    let spent = b.engine_spent_keys();
    assert!(spent[1] && !spent[2]);
}

/// The walk back to the displayed frame stops at the pass that armed the
/// live tween: a capture on `0x14` with the yaw counter still on the seed's
/// `0x200` rung is the walker pass right after the seed's own arm.
#[test]
fn the_displayed_camera_walks_back_no_further_than_the_seed_arm() {
    assert_eq!(tween_passes_back(0x14, 0x200, 2), 1);
    assert_eq!(tween_passes_back(0x14, 0x200, 0), 0);
    // A pass of `0x14` has drifted the counter: its own re-arms span the lag.
    assert_eq!(tween_passes_back(0x14, 0x202, 2), 2);
    assert_eq!(tween_passes_back(0x1E, 0x200, 2), 2);
}

/// The Done arm `0x50` seeds the tail countdown and stores `0x51` in one
/// pass, so a `0x51` capture on the seed with every HP bar settled is that
/// pass; a bar still draining freezes the countdown on later passes too.
#[test]
fn a_done_capture_on_the_seeded_countdown_is_the_arm_pass() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let gala = 0x800E_C9E8;
    put32(&mut ram, ACTOR_TABLE, gala);
    put16(&mut ram, gala + 0x14C, 1446);
    put16(&mut ram, gala + 0x172, 1446);
    put16(&mut ram, ctx + 0x6D8, 0x3C);
    assert!(done_arm_pass(&ram, ctx, 0x51));
    assert!(!done_arm_pass(&ram, ctx, 0x52), "only the fade-down band");
    put16(&mut ram, ctx + 0x6D8, 0x96);
    assert!(done_arm_pass(&ram, ctx, 0x51), "the Spirit-turn seed");
    put16(&mut ram, ctx + 0x6D8, 0x39);
    assert!(!done_arm_pass(&ram, ctx, 0x51), "a later pass stepped it");
    put16(&mut ram, ctx + 0x6D8, 0x3C);
    put16(&mut ram, gala + 0x172, 1500);
    assert!(!done_arm_pass(&ram, ctx, 0x51), "the settle gate holds it");
}

/// The pass before an Attack's Done arm is the return state's exit, which
/// re-arms nothing, so the screen two passes back stands one stale case-7 step
/// behind the Done arm's origin. `koru_pre_*`'s words (Gala on Koru, step 3):
/// one pass back on case 8 is yaw `3496`, depth `6730`, focus z `-342`; case 7
/// (yaw `3300`, depth `prescale(5120) = 8192`, focus the midpoint `-233`) takes
/// them to `3595`, `5998`, `-399`, and the pitch, which case 7 aims at `0`,
/// from `4073` to `4061` (a retail film of the tail reads `4058` on the held
/// pass: the walk's increment is the smaller of two the gap allows). `TR.y`
/// already stands on case 7's `0x500`.
#[test]
fn a_done_arm_capture_walks_the_strike_exit_on_case_7() {
    use legaia_engine_camera::projection::BattleCamPose;
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let (gala, koru) = (0x800E_C9E8u32, 0x800E_D264u32);
    put32(&mut ram, ACTOR_TABLE, gala);
    put32(&mut ram, ACTOR_TABLE + 3 * 4, koru);
    put8(&mut ram, ctx + 0x13, 0);
    put8(&mut ram, gala + 0x1DE, 3);
    put8(&mut ram, gala + 0x1DD, 3);
    put16(&mut ram, gala + 0x3C, 19);
    put16(&mut ram, gala + 0x40, (-792i16) as u16);
    put16(&mut ram, koru + 0x3C, 2);
    put16(&mut ram, koru + 0x40, 327);
    put8(&mut ram, koru + 0x1F1, 4);
    put16(&mut ram, ctx + 0x6D0, 5120);
    // The Done table's yaw endpoint.
    put16(&mut ram, ctx + TWEEN_TABLE + 4 + 2, 3300);
    let one_back = BattleCamPose {
        pitch: 4073.0,
        yaw: 3496.0,
        tr: [-74.0, 1280.0, 6730.0],
        focus: [10.0, 0.0, -342.0],
    };
    let p = done_arm_strike_walk_back(&ram, ctx, one_back, [3, 3]).expect("case 7");
    assert_eq!(p.yaw, 3595.0);
    assert_eq!(p.tr, [-113.0, 1280.0, 5998.0]);
    assert_eq!(p.focus, [10.0, 0.0, -399.0]);
    assert_eq!(p.pitch, 4061.0, "case 7 walked the pitch toward 0");
    // An eye height off case 7's `0x500` walks back away from it.
    let high = BattleCamPose {
        tr: [-74.0, 1290.0, 6730.0],
        ..one_back
    };
    let p = done_arm_strike_walk_back(&ram, ctx, high, [3, 3]).expect("case 7");
    assert_eq!(p.tr[1], 1296.0);
    // A target on its knockdown kept case 8 on the held passes.
    put8(&mut ram, koru + 0x1D9, 4);
    assert!(done_arm_strike_walk_back(&ram, ctx, one_back, [3, 3]).is_none());
    put8(&mut ram, koru + 0x1D9, 0);
    // A Spirit or a cast has no strike band.
    put8(&mut ram, gala + 0x1DE, 2);
    assert!(done_arm_strike_walk_back(&ram, ctx, one_back, [3, 3]).is_none());
}

/// A PROT 0962 charge body waiting in arm 3 walks the wind-up shot arm 2
/// built once over `0xC` units: `koru_blade_breath_mid_cast` holds it two
/// units in (yaw `99` a unit, `979` to go), so a walk back of two step-`2`
/// passes stops at the shot's origin, one pass back.
#[test]
fn a_charge_body_shot_walks_back_no_further_than_its_origin() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let koru = 0x800E_D264;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx + 7, 0x70);
    put8(&mut ram, ctx + 0x13, 3);
    put8(&mut ram, ctx + 0x279, 3);
    put32(&mut ram, ACTOR_TABLE + 3 * 4, koru);
    put8(&mut ram, koru + 0x1DF, 0xA2);
    // Live yaw `2366` walking to `3345` at `99` a unit; TR z `7285` to
    // `3276` at `402`.
    put16(&mut ram, 0x8007_B792, 2366);
    put32(&mut ram, 0x8008_40C0, 7285);
    put16(&mut ram, ctx + TWEEN_TABLE + 4, 99);
    put16(&mut ram, ctx + TWEEN_TABLE + 4 + 2, 3345);
    put16(&mut ram, ctx + TWEEN_TABLE + 5 * 4, 402);
    put16(&mut ram, ctx + TWEEN_TABLE + 5 * 4 + 2, 3276);
    assert_eq!(module_shot_frames(&ram, ctx), Some(0xC));
    let shown = displayed_module_cam_live(&ram, ctx, &[2, 2]);
    assert_eq!(shown.yaw, 2366.0 - 2.0 * 99.0);
    // Another module's table walks the whole lag back.
    put8(&mut ram, koru + 0x1DF, 0xA6);
    assert_eq!(module_shot_frames(&ram, ctx), None);
    let shown = displayed_module_cam_live(&ram, ctx, &[2, 2]);
    assert_eq!(shown.yaw, 2366.0 - 4.0 * 99.0);
}

/// The magic band rewrites a party-wide `8` to the lone member's slot
/// (`ctx[+0x24A] - 1`, `0x801E431C`); the seed read `8`, so the reader hands
/// the replay `8` back - only for an all-allies spell on that slot.
#[test]
fn a_lone_member_rewrite_reads_back_as_the_party_code() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_0000;
    let actor = 0x800E_8000;
    let holy_eyes = 0x91u8;
    let shape_at = 0x8007_54C8 + u32::from(holy_eyes) * 12 + 2;
    put8(&mut ram, actor + 0x1DE, 2);
    put8(&mut ram, actor + 0x1DF, holy_eyes);
    put8(&mut ram, actor + 0x1DD, 0);
    put8(&mut ram, ctx + 0x24A, 1);
    put8(&mut ram, shape_at, 0x26);
    assert_eq!(ram_read::seed_target_code(&ram, ctx, actor), 8);
    // A one-ally spell on the same member is the slot it was seeded on.
    put8(&mut ram, shape_at, 0x06);
    assert_eq!(ram_read::seed_target_code(&ram, ctx, actor), 0);
    // No lone-survivor latch: nothing rewrote the byte.
    put8(&mut ram, shape_at, 0x26);
    put8(&mut ram, ctx + 0x24A, 0);
    assert_eq!(ram_read::seed_target_code(&ram, ctx, actor), 0);
    // Not a magic action: the item band keeps its own byte.
    put8(&mut ram, ctx + 0x24A, 1);
    put8(&mut ram, actor + 0x1DE, 1);
    assert_eq!(ram_read::seed_target_code(&ram, ctx, actor), 0);
}

/// A downed party member sits out every replayed round, so its heading is
/// seeded like a spent one's; and the capture-class band seats an idle
/// caster and its idle target on the capture's loop cursor
/// (`zora_glare_petrify_*`, `zeto_big_wave_mid_cast`).
#[test]
fn a_downed_member_keeps_its_heading_and_the_capture_band_seats_idle_loops() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 2);
    put8(&mut ram, ctx + 1, 1);
    put8(&mut ram, ctx + 6, 0xFF);
    put8(&mut ram, ctx + 7, 0x6F);
    put8(&mut ram, FORMATION_CELL, 0xA9);
    for (slot, base, hp) in [
        (0u32, 0x800E_C9E8u32, 180u16),
        (1, 0x800E_CCBC, 0),
        (3, 0x800E_D264, 22422),
    ] {
        put32(&mut ram, ACTOR_TABLE + slot * 4, base);
        put16(&mut ram, base + 0x14C, hp);
        put16(&mut ram, base + 0x14E, hp.max(150));
        put16(&mut ram, base + 0x16C, 9);
    }
    let mut b = RetailBattle::from_ram(&ram).expect("seedable");
    b.active_actor = 3;
    b.target_code = 0;
    b.queued_category = 2;
    b.ground = vec![Some([-229, 269]), Some([830, -319]), None, Some([649, -47])];
    b.facing = vec![Some(0), Some(3723), None, Some(3297)];
    let f = b.seeded_facing();
    assert_eq!(f[1], Some(3723), "the downed member's heading is seeded");

    b.clip_cursors = vec![(0, 0, 84), (1, 8, 108), (3, 0, 52)];
    let seats =
        |b: &RetailBattle| -> Vec<u8> { b.engine_clip_cursors().iter().map(|s| s.slot).collect() };
    assert_eq!(
        seats(&b),
        vec![0, 1, 2],
        "every idle body in the capture band"
    );
    b.action_state = 0x20;
    assert_eq!(
        seats(&b),
        vec![1],
        "outside it the action's seats keep the replay's"
    );
    // A party target already on its reaction is seated on the displayed
    // frame; one whose displayed frame predates the hit is not.
    b.clip_cursors = vec![(0, 4, 56), (1, 8, 108), (3, 0, 52)];
    assert_eq!(seats(&b), vec![0, 1], "the struck member's knockdown");
    b.clip_cursors = vec![(0, 4, -8)];
    assert!(seats(&b).is_empty(), "before the hit");
}

/// A seated cast part is taken back over the display lag: its position by
/// its velocity, its roll by its bank rate, its channels by their rates in
/// render mode 2, its wait by the drain (`displayed_part_halfword`).
#[test]
fn cast_part_seed_walks_back_over_the_display_lag() {
    let mut ram = vec![0u8; 0x20_0000];
    let n = 0x8008_1F1C;
    put16(&mut ram, n + 0x5A, 2);
    put16(&mut ram, n + 0x14, (-741i16) as u16);
    put16(&mut ram, n + 0x3C, 152);
    put16(&mut ram, n + 0x28, 800);
    put16(&mut ram, n + 0x84, 0x100);
    put16(&mut ram, n + 0xB6, 300);
    put16(&mut ram, n + 0xC2, (-64i16) as u16);
    put16(&mut ram, n + 0x54, 304);
    let at = |off| ram_read::displayed_part_halfword(&ram, n, off, 6) as i16;
    assert_eq!(at(0x14), -741 - 6 * 19);
    assert_eq!(at(0x28), 800 - 6 * 32);
    assert_eq!(at(0xB6), 300 + 6 * 8);
    assert_eq!(at(0x54), 304 + 6 * 8);
    // Render mode 3 neither moves nor turns.
    put16(&mut ram, n + 0x5A, 3);
    let at = |off| ram_read::displayed_part_halfword(&ram, n, off, 6) as i16;
    assert_eq!(at(0x14), -741);
    assert_eq!(at(0x28), 800);
    assert_eq!(at(0xB6), 300);
}

/// A turning effect-script table part is seeded as its rotation trio on the
/// displayed frame - `freed_summon_mid_cast`'s ray fan: record `0x801F5A04`,
/// `+0x26 = 644` turning `-0x1C0 >> 3 = -56` a vsync, four vsyncs of lag at
/// step `2` - while a still part, a slot-B record and a killed node are not.
#[test]
fn action_fx_part_seed_is_the_displayed_rotation() {
    let mut ram = vec![0u8; 0x20_0000];
    put32(&mut ram, frame_step::FORCED_STEP, 2);
    let nodes = [0x8008_1F1C, 0x8008_2000, 0x8008_2100, 0x8008_2200];
    put32(&mut ram, 0x8007_C34C + 4, nodes[0]);
    for (i, &n) in nodes.iter().enumerate() {
        put32(&mut ram, n, nodes.get(i + 1).copied().unwrap_or(0));
        put32(&mut ram, n + 0x0C, 0x8002_1DF4);
        put32(&mut ram, n + 0x48, 0x801F_5A04);
        put16(&mut ram, n + 0x5A, 1);
        put16(&mut ram, n + 0x26, 644);
        put16(&mut ram, n + 0x82, 0xFE40);
    }
    put16(&mut ram, nodes[1] + 0x82, 0);
    put32(&mut ram, nodes[2] + 0x48, 0x801F_6A00);
    put32(&mut ram, nodes[3] + 0x10, 0x8);
    let seeds = ram_read::retail_action_fx_parts(&ram);
    assert_eq!(seeds.len(), 1);
    assert_eq!(seeds[0].tag, 0x801F_5A04);
    assert_eq!(
        seeds[0].pokes,
        vec![(0x24, 0), (0x26, 644 + 4 * 56), (0x28, 0)]
    );
}

/// The ray fan fading out at the end of the invoke clip is seeded with its
/// depth-cue level taken back the lag, and a fan its program killed on the
/// RAM's pass is still seated while the displayed frame drew it short of
/// black: `meta_summon_mid_cast`'s fan holds `0x132C` (the kill skipped
/// the clamp) at rate `0xCC8`, `0x199` a vsync, so four vsyncs back it drew
/// at `0xCC8`.
#[test]
fn a_fading_ray_fan_is_seeded_with_its_displayed_level() {
    let mut ram = vec![0u8; 0x20_0000];
    put32(&mut ram, frame_step::FORCED_STEP, 2);
    let nodes = [0x8008_2C9C, 0x8008_2D00];
    put32(&mut ram, 0x8007_C34C + 4, nodes[0]);
    for (i, &n) in nodes.iter().enumerate() {
        put32(&mut ram, n, nodes.get(i + 1).copied().unwrap_or(0));
        put32(&mut ram, n + 0x0C, 0x8002_1DF4);
        put32(&mut ram, n + 0x10, 0x8);
        put32(&mut ram, n + 0x48, 0x801F_5A04);
        put16(&mut ram, n + 0x5A, 1);
        put16(&mut ram, n + 0x26, 144);
        put16(&mut ram, n + 0x82, 0xFE40);
        put16(&mut ram, n + 0x90, 0xCC8);
    }
    put16(&mut ram, nodes[0] + 0x78, 0x132C);
    // Black on the displayed frame too: killed and gone from the screen.
    put16(&mut ram, nodes[1] + 0x78, 0x1A00);
    let seeds = ram_read::retail_action_fx_parts(&ram);
    assert_eq!(seeds.len(), 1);
    assert_eq!(
        seeds[0].pokes,
        vec![(0x24, 0), (0x26, 144 + 4 * 56), (0x28, 0), (0x78, 0xCC8)]
    );
}

/// A slot-B cast part under a depth-cue ramp is seeded with its level taken
/// back the lag, unless it stands on the clamp its rate runs toward - Horn's
/// sparkle (`0x801F85FC`, fading in at `-0x100` a vsync): the youngest
/// volley's `0xD00` reads black two step-`2` passes back, the oldest's `0`
/// is left to the replay.
#[test]
fn a_cast_part_level_ramp_is_seeded_off_its_clamp() {
    let mut ram = vec![0u8; 0x20_0000];
    put32(&mut ram, frame_step::FORCED_STEP, 2);
    let nodes = [0x8008_2000, 0x8008_2100, 0x8008_2200];
    put32(&mut ram, 0x8007_C34C + 4, nodes[0]);
    for (i, &n) in nodes.iter().enumerate() {
        put32(&mut ram, n, nodes.get(i + 1).copied().unwrap_or(0));
        put32(&mut ram, n + 0x0C, 0x8002_1DF4);
        put32(&mut ram, n + 0x48, 0x801F_85FC);
        put16(&mut ram, n + 0x5A, 2);
        put16(&mut ram, n + 0x90, 0xF800);
    }
    put16(&mut ram, nodes[0] + 0x78, 0xD00);
    put16(&mut ram, nodes[1] + 0x78, 0x100);
    put16(&mut ram, nodes[2] + 0x78, 0);
    let level = |s: &legaia_engine_effects::summon::CastPartSeed| {
        s.pokes.iter().find(|p| p.0 == 0x78).map(|p| p.1)
    };
    let seeds = ram_read::retail_cast_parts(&ram);
    assert_eq!(seeds.len(), 3);
    assert_eq!(level(&seeds[0]), Some(0x1000));
    assert_eq!(level(&seeds[1]), Some(0x500));
    assert_eq!(level(&seeds[2]), None);
}

/// A capture saved after the flip with the current table empty and the
/// fade linked under the other record shows the pass before the newest
/// table, so a cast part's seed is taken back one pass: Horn's capture.
#[test]
fn a_cast_part_is_taken_back_one_pass_when_the_newest_table_is_the_other() {
    let mut ram = vec![0u8; 0x20_0000];
    put32(&mut ram, frame_step::FORCED_STEP, 3);
    // Record 1 links a fade quad at bucket 1; record 0 is current and empty.
    let (ot0, ot1, pkt) = (0x800F_4B08_u32, 0x800F_099C_u32, 0x8014_0000_u32);
    put32(&mut ram, 0x8007_BF30 + 0x70, ot0);
    put32(&mut ram, 0x8007_BF30 + 0x74 + 0x70, ot1);
    for b in 0..0x1000u32 {
        let link = |ot: u32| {
            if b == 0 {
                0x00FF_FFFF
            } else {
                (ot + (b - 1) * 4) & 0xFF_FFFF
            }
        };
        put32(&mut ram, ot0 + b * 4, link(ot0));
        put32(&mut ram, ot1 + b * 4, link(ot1));
    }
    put32(&mut ram, ot1 + 4, pkt & 0xFF_FFFF);
    put32(&mut ram, pkt, 0x0500_0000 | (ot1 & 0xFF_FFFF));
    put32(&mut ram, pkt + 4, 0x2BFF_FFFF);
    let n = 0x8008_2000;
    put32(&mut ram, 0x8007_C34C + 4, n);
    put32(&mut ram, n + 0x0C, 0x8002_1DF4);
    put32(&mut ram, n + 0x48, 0x801F_85FC);
    put16(&mut ram, n + 0x5A, 2);
    put16(&mut ram, n + 0x54, 480);
    let seeds = ram_read::retail_cast_parts(&ram);
    let wait = seeds[0].pokes.iter().find(|p| p.0 == 0x54).map(|p| p.1);
    assert_eq!(wait, Some(480 + 8 * 3));
}

/// The ghosts on screen are the walk's slots read from the displayed frame,
/// two ring slots down: `battle_melee_hit_spark`'s Vahn (rate `4`, step `2`)
/// shows ring slots 4 and 6 - cursors `80` / `56` on `(-21, 116)` /
/// `(-21, 106)` - and a slot of another clip is left out.
#[test]
fn the_ghost_ring_seed_is_the_displayed_frames_walk() {
    let mut ram = vec![0u8; 0x20_0000];
    let p = 0x800E_C9E8;
    put32(&mut ram, ACTOR_TABLE, p);
    put8(&mut ram, p + 0x21D, 4);
    let cursors = [128u16, 116, 104, 92, 80, 68, 56, 44];
    for (i, &c) in cursors.iter().enumerate() {
        let i = i as u32;
        put8(&mut ram, p + 0x1FB + i, 0x11);
        put16(&mut ram, p + 0x17A + 2 * i, c);
        put32(
            &mut ram,
            p + POSITION_RING + 8 * i,
            (-21i32) as u32 & 0xFFFF,
        );
        put32(&mut ram, p + POSITION_RING + 8 * i + 4, 136 - 5 * i);
    }
    let seeds = ram_read::retail_ghost_ring(&ram, 1);
    let got: Vec<_> = seeds
        .iter()
        .map(|s| (s.slot, s.clip, s.depth, s.drawn, s.cursor, s.x, s.z))
        .collect();
    assert_eq!(
        got,
        vec![
            (0, 0x11, 2, true, 80, -21, 116),
            (0, 0x11, 4, true, 56, -21, 106)
        ]
    );
    // A slot the gate closes (ring id `0x10` or below) is seeded undrawn,
    // as `battle_vahn_tri_somersault_super`'s depth 4 ...
    put8(&mut ram, p + 0x1FB + 6, 0x00);
    let seeds = ram_read::retail_ghost_ring(&ram, 1);
    assert_eq!(seeds.len(), 2);
    assert!(seeds[0].drawn && !seeds[1].drawn);
    // ... and a slot of another art clip is left to the replay.
    put8(&mut ram, p + 0x1FB + 6, 0x12);
    assert_eq!(ram_read::retail_ghost_ring(&ram, 1).len(), 1);
}

/// A burst child (the narrow arm's record `0x801F5D0C`) is seeded whole,
/// turning or not: its `rand()`-rolled spread `+0x3E`, scale `+0x72`,
/// heading `+0x96` and tail `+0x98` ride beside the position and channels.
#[test]
fn a_burst_child_seeds_its_rolled_words() {
    let mut ram = vec![0u8; 0x20_0000];
    put32(&mut ram, frame_step::FORCED_STEP, 2);
    let n = 0x8008_1F1C;
    put32(&mut ram, 0x8007_C34C + 4, n);
    put32(&mut ram, n + 0x0C, 0x8002_1DF4);
    put32(&mut ram, n + 0x48, 0x801F_5D0C);
    put16(&mut ram, n + 0x5A, 2);
    put16(&mut ram, n + 0x3E, 44);
    put16(&mut ram, n + 0x72, 0x800);
    put16(&mut ram, n + 0x96, 64);
    put16(&mut ram, n + 0x98, 72);
    let seeds = ram_read::retail_action_fx_parts(&ram);
    assert_eq!(seeds.len(), 1);
    let poke = |o: u16| seeds[0].pokes.iter().find(|p| p.0 == o).map(|p| p.1);
    assert_eq!(poke(0x3E), Some(44));
    assert_eq!(poke(0x72), Some(0x800));
    assert_eq!(poke(0x96), Some(64));
    assert_eq!(poke(0x98), Some(72));
    assert!(poke(0x14).is_some() && poke(0xC8).is_some());
}

/// The effect pool's children are read back to the displayed frame: a
/// child holding its frame takes the lag back off its wait, one that just
/// entered a frame steps back onto the previous one, and one seeded inside
/// the lag is dropped. The batch index is the entry's place in the pack0
/// pointer table at the head's `+0x08`.
#[test]
fn effect_children_are_the_displayed_frames() {
    let mut ram = vec![0u8; 0x20_0000];
    put32(&mut ram, frame_step::FORCED_STEP, 1); // lag 2 sweeps
    let pool = 0x800E_6A5C;
    put32(&mut ram, 0x8007_BD30, pool);
    put16(&mut ram, pool, 0x1000);
    let table = 0x800E_46E8;
    put32(&mut ram, pool + 8, table);
    // Batch 1 at `entry`: three frames, delays 2 / 4 / 4, speed 1.
    let entry = 0x800E_4782;
    put32(&mut ram, table + 4, entry);
    for (k, d) in [2u8, 4, 4].iter().enumerate() {
        let f = entry + 2 + 6 * k as u32;
        ram[(f + 1 - 0x8000_0000) as usize] = *d;
        ram[(f + 2 - 0x8000_0000) as usize] = 1;
    }
    let child = |slot: u32, cursor: u8, wait: u8, vy: i16| {
        let c = pool + 0x10 + slot * 0x20;
        (c, cursor, wait, vy)
    };
    for (c, cursor, wait, vy) in [
        child(0, 2, 16, -4), // holding frame 2: wait 16 -> 32
        child(1, 1, 32, 0),  // just entered frame 1 -> frame 0, then +8
        child(2, 0, 8, 0),   // seeded inside the lag -> dropped
    ] {
        ram[(c - 0x8000_0000) as usize] = 3;
        ram[(c + 2 - 0x8000_0000) as usize] = cursor;
        ram[(c + 3 - 0x8000_0000) as usize] = wait;
        ram[(c + 1 - 0x8000_0000) as usize] = 2;
        put16(&mut ram, c + 6, vy as u16);
        put32(&mut ram, c + 0x10, (-300i32 << 8) as u32);
        put32(&mut ram, c + 0x1C, entry + 2 + 6 * u32::from(cursor));
    }
    let seeds = effect_children::retail_effect_children(&ram).expect("a pool");
    assert_eq!(seeds.len(), 2);
    assert_eq!(seeds[0].anim_id, 1);
    assert_eq!((seeds[0].frame_cursor, seeds[0].wait), (2, 32));
    // Two motion steps of `-4 * 1` taken back off Y.
    assert_eq!(seeds[0].pos[1], (-300 << 8) + 8);
    assert_eq!((seeds[1].frame_cursor, seeds[1].wait), (0, 8));
    assert_eq!(seeds[1].mirror, 2);
}

/// A readable pool with no live child reads as an empty seat, not as no
/// pool: the image child must retire the billboards its replay still
/// carries. A state with no pool pointer reads as `None`.
#[test]
fn an_empty_effect_pool_is_still_a_seat() {
    let mut ram = vec![0u8; 0x20_0000];
    assert!(effect_children::retail_effect_children(&ram).is_none());
    let pool = 0x800E_6A5C;
    put32(&mut ram, 0x8007_BD30, pool);
    put16(&mut ram, pool, 0x1000);
    put32(&mut ram, pool + 8, 0x800E_46E8);
    assert_eq!(
        effect_children::retail_effect_children(&ram),
        Some(Vec::new())
    );
}

/// On the action seed's own pass the image child stands each combatant on
/// the displayed frame's ring slot - the seed's formation recentre moved
/// them after that frame was built - and elsewhere on the RAM's live pair.
#[test]
fn the_seed_pass_image_ground_is_the_displayed_ring_slot() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 1);
    put8(&mut ram, ctx + 1, 1);
    put8(&mut ram, ctx + 6, 0xFF);
    put8(&mut ram, ctx + 7, 0x14);
    put16(&mut ram, ctx + 0x6DA, 0x200);
    put8(&mut ram, FORMATION_CELL, 0x04);
    for (slot, base, live, shown) in [
        (0u32, 0x800E_C9E8u32, [-7i16, -338i16], [-43i16, -383i16]),
        (3, 0x800E_D264, [0, 0], [-36, -52]),
    ] {
        put32(&mut ram, ACTOR_TABLE + slot * 4, base);
        put16(&mut ram, base + 0x14C, 50);
        put16(&mut ram, base + 0x14E, 50);
        put16(&mut ram, base + 0x34, live[0] as u16);
        put16(&mut ram, base + 0x38, live[1] as u16);
        put32(&mut ram, base + 0x4C, live[0] as u16 as u32);
        put32(&mut ram, base + 0x50, live[1] as u16 as u32);
        for k in 1..3u32 {
            put32(&mut ram, base + 0x4C + 8 * k, shown[0] as u16 as u32);
            put32(&mut ram, base + 0x50 + 8 * k, shown[1] as u16 as u32);
        }
    }
    let mut b = RetailBattle::from_ram(&ram).expect("seedable");
    let pairs = |s: Vec<legaia_engine_capture::ground_snap::GroundSnap>| {
        s.iter().map(|g| (g.slot, g.x, g.z)).collect::<Vec<_>>()
    };
    assert_eq!(
        pairs(b.engine_image_ground_snaps()),
        vec![(0, -43, -383), (1, -36, -52)]
    );
    assert_eq!(
        pairs(b.engine_ground_snaps()),
        vec![(0, -7, -338), (1, 0, 0)]
    );
    // Past the seed's pass the RAM's pairs stand.
    b.walk_yaw_base = 0x202;
    assert_eq!(
        pairs(b.engine_image_ground_snaps()),
        vec![(0, -7, -338), (1, 0, 0)]
    );
}

/// A struck monster stood on its body pair takes the displayed frame's
/// ground even past the seed's pass: the push carries it, so the body pair
/// the screen drew is the RAM's moved by the ring's delta
/// (`battle_melee_hit_spark`'s Gimard, `+11` a frame). The acting seat keeps
/// the RAM's.
#[test]
fn a_struck_monsters_image_ground_is_the_displayed_ring_slot() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 1);
    put8(&mut ram, ctx + 1, 1);
    put8(&mut ram, ctx + 6, 0xFF);
    put8(&mut ram, ctx + 7, 0x20);
    put16(&mut ram, ctx + 0x6DA, 0x298);
    put8(&mut ram, FORMATION_CELL, 0x04);
    for (slot, base, live, shown, body, clip) in [
        (
            0u32,
            0x800E_C9E8u32,
            [-21i16, 136i16],
            [-21i16, 126i16],
            [-24i16, 95i16],
            0x11u8,
        ),
        (3, 0x800E_D264, [-6, 405], [-6, 383], [-2, 492], 3),
    ] {
        put32(&mut ram, ACTOR_TABLE + slot * 4, base);
        put16(&mut ram, base + 0x14C, 50);
        put16(&mut ram, base + 0x14E, 50);
        put16(&mut ram, base + 0x34, live[0] as u16);
        put16(&mut ram, base + 0x38, live[1] as u16);
        put16(&mut ram, base + 0x3C, body[0] as u16);
        put16(&mut ram, base + 0x40, body[1] as u16);
        put8(&mut ram, base + 0x1D9, clip);
        put32(&mut ram, base + 0x4C, live[0] as u16 as u32);
        put32(&mut ram, base + 0x50, live[1] as u16 as u32);
        for k in 1..3u32 {
            put32(&mut ram, base + 0x4C + 8 * k, shown[0] as u16 as u32);
            put32(&mut ram, base + 0x50 + 8 * k, shown[1] as u16 as u32);
        }
    }
    let b = RetailBattle::from_ram(&ram).expect("seedable");
    let snap = |s: Vec<legaia_engine_capture::ground_snap::GroundSnap>, slot: u8| {
        s.into_iter()
            .find(|g| g.slot == slot)
            .map(|g| (g.x, g.z, g.body))
    };
    assert_eq!(
        snap(b.engine_image_ground_snaps(), 1),
        Some((-6, 383, Some([-2, 470])))
    );
    assert_eq!(
        snap(b.engine_ground_snaps(), 1),
        Some((-6, 405, Some([-2, 492])))
    );
    assert_eq!(
        snap(b.engine_image_ground_snaps(), 0).map(|(x, z, _)| (x, z)),
        Some((-21, 136))
    );
}

/// One cleared 64-bucket ordering table at `ot` whose head bucket chains
/// `SPRT` packets (CLUT `0x7FCC`, `16 x 20`) at `pool` drawn on `seats`.
fn put_sprite_table(ram: &mut [u8], ot: u32, pool: u32, seats: &[(i16, i16)], uvs: &[(u8, u8)]) {
    const N: u32 = 64;
    put32(ram, ot, 0x00FF_FFFF);
    for i in 1..N {
        put32(ram, ot + i * 4, (ot + (i - 1) * 4) & 0x00FF_FFFF);
    }
    let tail = (ot + (N - 2) * 4) & 0x00FF_FFFF;
    put32(ram, ot + (N - 1) * 4, pool & 0x00FF_FFFF);
    for (i, (&(x, y), &(u, v))) in seats.iter().zip(uvs).enumerate() {
        let p = pool + i as u32 * 20;
        let next = if i + 1 == seats.len() {
            tail
        } else {
            (p + 20) & 0x00FF_FFFF
        };
        put32(ram, p, (4 << 24) | next);
        put32(ram, p + 4, 0x6480_8080);
        put32(
            ram,
            p + 8,
            (u32::from(y as u16) << 16) | u32::from(x as u16),
        );
        put32(
            ram,
            p + 12,
            (0x7FCC << 16) | (u32::from(v) << 8) | u32::from(u),
        );
        put32(ram, p + 16, (20 << 16) | 16);
    }
}

/// `delilas_gi_spirit_artifact`'s actor plaque: the record retired on the
/// pass the capture froze (`total` cleared, `elapsed` left on its last
/// in-flight `13`), the table being built draws the plaque at rest and the
/// older one eight pixels above it - the offset the glide had at `13` of
/// `16` (`-26 + 38 * 13 / 16 = 4`, target `12`). The displayed frame is one
/// step of `3` older: `10`. A plaque the two tables draw on the same seat
/// landed earlier and yields no seat.
#[test]
fn a_glide_retired_on_the_frozen_pass_is_read_off_the_tables() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let rec = ctx + HUD_GLIDE_TABLE;
    put8(&mut ram, rec + 1, 13);
    put16(&mut ram, rec + 4, 16);
    put16(&mut ram, rec + 6, 12);
    put16(&mut ram, rec + 8, 16);
    put16(&mut ram, rec + 10, (-26i16) as u16);
    put32(&mut ram, ctx + 0x1074, 0x8009_E4C0);
    let uvs = [(0, 0), (16, 0)];
    put_sprite_table(
        &mut ram,
        0x800F_0000,
        0x8010_0000,
        &[(16, 8), (32, 8)],
        &uvs,
    );
    put_sprite_table(
        &mut ram,
        0x800F_1000,
        0x8010_1000,
        &[(16, 0), (32, 0)],
        &uvs,
    );
    assert_eq!(
        landed_glide_pool_seats(&ram, ctx, [3, 3]),
        vec![HudGlideSeat {
            target: [16, 12],
            elapsed: 10,
            total: 16
        }]
    );
    put_sprite_table(
        &mut ram,
        0x800F_1000,
        0x8010_1000,
        &[(16, 8), (32, 8)],
        &uvs,
    );
    assert_eq!(landed_glide_pool_seats(&ram, ctx, [3, 3]), vec![]);
}

/// A Spirit turn's bar and plate come off the widgets the glide records name:
/// the bar's width `+0x0E`, taken back across the display lag while it grows
/// past the AGL seed, and the plate's value `+0x10`.
#[test]
fn the_spirit_gauge_is_read_off_its_widgets() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let actor = 0x800E_CCBC;
    put8(&mut ram, ctx + 7, 0x47);
    put8(&mut ram, actor + 0x1DE, 4);
    put16(&mut ram, actor + 0x154, 194);
    let seat = |ram: &mut Vec<u8>, slot: u32, seats: [i16; 4], widget: u32| {
        let r = ctx + HUD_GLIDE_TABLE + slot * HUD_GLIDE_STRIDE;
        for (i, s) in seats.iter().enumerate() {
            put16(ram, r + 4 + i as u32 * 2, *s as u16);
        }
        put32(ram, ctx + 0x1074 + slot * 4, widget);
    };
    seat(&mut ram, 0, [16, 192, 16, 234], 0x8009_E540);
    seat(&mut ram, 1, [208, 172, 328, 172], 0x8009_E500);
    put16(&mut ram, 0x8009_E540 + 0x0E, 188);
    put16(&mut ram, 0x8009_E500 + 0x10, 100);
    assert_eq!(
        retail_spirit_gauge(&ram, ctx, Some(actor), 6),
        Some((188, 100))
    );
    put16(&mut ram, 0x8009_E540 + 0x0E, 200);
    assert_eq!(
        retail_spirit_gauge(&ram, ctx, Some(actor), 6),
        Some((194, 100))
    );
    put8(&mut ram, actor + 0x1DE, 1);
    assert_eq!(retail_spirit_gauge(&ram, ctx, Some(actor), 6), None);
}

/// A pass whose own history entry reads the `0x2BC` clamp stepped `4`
/// (the raw duration past `0x2D0` sets the step, `0x800170F8..0x80017104`),
/// whatever the history's largest entry says: `delilas_gi_spirit_artifact`'s
/// newest entry is the clamp and the one before it `0x2CF`, so its last two
/// passes stepped `4` and `3` while [`frame_step`] reads `3`.
#[test]
fn a_clamped_history_entry_is_a_step_four_pass() {
    let mut ram = vec![0u8; 0x20_0000];
    put16(&mut ram, STEP_MODE, 0x10);
    let hist = [
        566u16, 700, 660, 719, 700, 588, 574, 583, 560, 564, 567, 561, 567, 562, 559, 559,
    ];
    for (i, d) in hist.into_iter().enumerate() {
        put16(&mut ram, FRAME_HISTORY + i as u32 * 2, d);
    }
    put32(&mut ram, 0x8007_B758, 5);
    assert_eq!(frame_step(&ram), 3);
    assert_eq!(pass_steps(&ram), [4, 3]);
    put32(&mut ram, 0x8007_B758, 6);
    assert_eq!(pass_steps(&ram), [3, 4]);
    put32(&mut ram, 0x8007_B758, 4);
    assert_eq!(pass_steps(&ram), [3, 3]);
    put32(&mut ram, FORCED_STEP, 2);
    assert_eq!(pass_steps(&ram), [2, 2]);
}

/// Without two ordering tables drawing the battle floor the step stays
/// unconfirmed, and the walk-back keeps the history's single step.
#[test]
fn no_floor_tables_leave_the_pass_step_unconfirmed() {
    let ram = vec![0u8; 0x20_0000];
    assert_eq!(older_table_step(&ram, 0x800E_B654), None);
}

/// The history's per-pass steps are taken only where the battle floor
/// confirms them: a clamped newest entry with no ordering tables to check it
/// against leaves both passes on the history's largest entry, and the lag on
/// two of them.
#[test]
fn unconfirmed_pass_steps_keep_the_history_max() {
    let mut ram = vec![0u8; 0x20_0000];
    put16(&mut ram, STEP_MODE, 0x10);
    let hist = [
        566u16, 700, 660, 719, 700, 588, 574, 583, 560, 564, 567, 561, 567, 562, 559, 559,
    ];
    for (i, d) in hist.into_iter().enumerate() {
        put16(&mut ram, FRAME_HISTORY + i as u32 * 2, d);
    }
    put32(&mut ram, 0x8007_B758, 5);
    put32(&mut ram, BATTLE_CTX, 0x800E_B654);
    assert_eq!(pass_steps(&ram), [4, 3]);
    assert_eq!(display_pass_steps(&ram), [3, 3]);
    assert_eq!(display_lag_vsyncs(&ram), 6);
}

/// A synthetic pool with one live child and one dead slot, two ordering
/// tables and the double buffer's records: the newest table draws the live
/// child's current quad, the older one the dead slot's last frame. Mid-pass
/// (the newest table under the current record) the displayed frame is two
/// passes back and the dead slot comes back a sweep before its last frame's
/// end; after the flip (the newest table under the other record) the screen
/// shows the older table's pass itself.
#[test]
fn retired_effect_children_come_back_from_the_older_table() {
    const POOL: u32 = 0x800E_6A5C;
    const TABLE: u32 = 0x800E_46E8;
    const ATLAS: u32 = 0x800E_4264;
    const ENTRY: u32 = 0x800E_4782;
    const NEWER: u32 = 0x800F_0000;
    const OLDER: u32 = 0x800F_1000;
    let build = |cur: u16| {
        let mut ram = vec![0u8; 0x20_0000];
        put32(&mut ram, frame_step::FORCED_STEP, 1); // one sweep a pass
        put32(&mut ram, 0x8007_BD30, POOL);
        put16(&mut ram, POOL, 0x1000);
        put32(&mut ram, POOL + 4, ATLAS);
        put32(&mut ram, POOL + 8, TABLE);
        put32(&mut ram, TABLE + 4, ENTRY); // batch 1
        // Three frames, atlas 0 / 1 / 2, hold 2 each; the byte past the batch
        // (the overrun's stop) holds 1.
        put16(&mut ram, ENTRY, 3);
        for k in 0..4u32 {
            put8(&mut ram, ENTRY + 2 + 6 * k, k as u8);
            put8(&mut ram, ENTRY + 3 + 6 * k, if k < 3 { 2 } else { 1 });
        }
        for k in 0..3u32 {
            let a = ATLAS + 8 * k;
            put8(&mut ram, a, (k * 64) as u8);
            put8(&mut ram, a + 1, 128);
            put8(&mut ram, a + 2, 64);
            put8(&mut ram, a + 3, 64);
            put16(&mut ram, a + 4, 0x7683);
            put16(&mut ram, a + 6, 0x25);
        }
        // Slot 0 live, just entered frame 1 (wait 16); slot 1 dead, its
        // cursor run to 3.
        let live = POOL + 0x10;
        put8(&mut ram, live, 3);
        put8(&mut ram, live + 1, 3);
        put8(&mut ram, live + 2, 1);
        put8(&mut ram, live + 3, 16);
        put32(&mut ram, live + 0x1C, ENTRY + 2 + 6);
        let dead = POOL + 0x30;
        put8(&mut ram, dead + 1, 3);
        put8(&mut ram, dead + 2, 3);
        put8(&mut ram, dead + 3, 8);
        put32(&mut ram, dead + 0x10, (-200i32 << 8) as u32);
        put32(&mut ram, dead + 0x1C, ENTRY + 2 + 18);
        // Two 64-bucket reverse-cleared tables, each with its quads.
        let quad = |k: u32| {
            let b = legaia_engine_vm::effect_vm::pass2_brightness(3, k as u8);
            (b, (k * 64) as u8)
        };
        for (ot, quads) in [(NEWER, vec![quad(1)]), (OLDER, vec![quad(0), quad(2)])] {
            put32(&mut ram, ot, 0x00FF_FFFF);
            for i in 1..64u32 {
                put32(&mut ram, ot + 4 * i, (ot + 4 * (i - 1)) & 0xFF_FFFF);
            }
            for (j, (bright, u)) in quads.into_iter().enumerate() {
                let bucket = ot + 4 * (10 + j as u32);
                let pkt = ot + 0x400 + 0x40 * j as u32;
                put32(&mut ram, pkt, (9 << 24) | ((bucket - 4) & 0xFF_FFFF));
                put32(&mut ram, bucket, pkt & 0xFF_FFFF);
                put32(
                    &mut ram,
                    pkt + 4,
                    (0x2E << 24) | (u32::from(bright) * 0x01_0101),
                );
                let (u0, u1, v0, v1) = (u, u + 63, 128u8, 191u8);
                for (c, (uu, vv)) in [(u0, v0), (u1, v0), (u0, v1), (u1, v1)]
                    .into_iter()
                    .enumerate()
                {
                    let extra = match c {
                        0 => 0x7683u32 << 16,
                        1 => 0x25 << 16,
                        _ => 0,
                    };
                    put32(
                        &mut ram,
                        pkt + 12 + 8 * c as u32,
                        extra | u32::from(uu) | (u32::from(vv) << 8),
                    );
                }
            }
        }
        // Record 0 holds the newer table, record 1 the older.
        put32(&mut ram, 0x8007_BF30 + 0x70, NEWER);
        put32(&mut ram, 0x8007_BF30 + 0x74 + 0x70, OLDER);
        put16(&mut ram, 0x8007_B74C, cur);
        ram
    };
    let pick = |seeds: &[legaia_engine_core::world::EffectChildSeed]| {
        let mut v: Vec<_> = seeds
            .iter()
            .map(|s| (s.frame_cursor, s.wait, s.pos[1] >> 8))
            .collect();
        v.sort();
        v
    };
    // Mid-pass: two passes back. The live child steps back onto frame 0;
    // the dead slot stood on its last frame at wait 0 one pass ago (it
    // retired on the next sweep), wait 8 the pass before.
    let mid = effect_children::retail_effect_children(&build(0)).expect("a pool");
    assert_eq!(pick(&mid), vec![(0, 8, 0), (2, 8, -200)]);
    // After the flip: one pass back, the older table's pass itself.
    let after = effect_children::retail_effect_children(&build(1)).expect("a pool");
    assert_eq!(pick(&after), vec![(0, 0, 0), (2, 0, -200)]);
}

/// A retired combo-cluster record is seated off the newest numeral's ring
/// timer: `battle_gimard_tail_fire_b`'s timer `0x160` (22 vsyncs) at step 2
/// puts the cluster `14` of sixteen in on screen; a numeral old enough for
/// the cluster to have landed seats nothing, and so does a live record.
#[test]
fn a_retired_combo_cluster_is_timed_off_the_numeral_ring() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let rec = ctx + HUD_GLIDE_TABLE + 3 * HUD_GLIDE_STRIDE;
    put8(&mut ram, rec + 1, 14);
    put16(&mut ram, rec + 4, 168);
    put16(&mut ram, rec + 6, 168);
    put16(&mut ram, rec + 8, 328);
    put16(&mut ram, rec + 10, 168);
    put8(&mut ram, ctx + 0x262, 1);
    put32(&mut ram, ctx + 0x83C, 16);
    put32(&mut ram, ctx + 0x85C, 0x160);
    assert_eq!(
        combo_cluster_timer_seat(&ram, ctx, 4),
        Some(HudGlideSeat {
            target: [168, 168],
            elapsed: 14,
            total: 16
        })
    );
    put32(&mut ram, ctx + 0x85C, 0x200);
    assert_eq!(combo_cluster_timer_seat(&ram, ctx, 4), None, "landed");
    put32(&mut ram, ctx + 0x85C, 0x160);
    put8(&mut ram, rec, 16);
    assert_eq!(combo_cluster_timer_seat(&ram, ctx, 4), None, "live record");
}

/// The newest numeral's displayed age: `battle_melee_hit_spark`'s ring
/// entry 2 (`15` on pool slot 3, timer `288`) at step 3 and a six-vsync lag
/// rose on the timer `144` - age 9. A zeroed value or an out-of-range seat
/// seats nothing.
#[test]
fn the_newest_numeral_is_aged_to_the_displayed_readout() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put8(&mut ram, ctx + 0x262, 3);
    put32(&mut ram, ctx + 0x83C + 2 * 4, 15);
    put32(&mut ram, ctx + 0x85C + 2 * 4, 288);
    put16(&mut ram, ctx + 0x318 + 2 * 2, 3);
    assert_eq!(popup_age_seat(&ram, ctx, 6, 3), Some((3, 9)));
    put16(&mut ram, ctx + 0x318 + 2 * 2, 9);
    assert_eq!(popup_age_seat(&ram, ctx, 6, 3), None, "no seat");
    put16(&mut ram, ctx + 0x318 + 2 * 2, 3);
    put32(&mut ram, ctx + 0x83C + 2 * 4, 0);
    assert_eq!(popup_age_seat(&ram, ctx, 6, 3), None, "expired");
}

/// A ray burst's level and lane weight taken back over the display lag:
/// `rim_elm_gimard_victory`'s RAM level `0x400` (rate `0x800`, `0x100` a
/// vsync) and lane `0xFFC` (`0x155` a vsync) read `0` and `2728` four
/// vsyncs earlier; a level taken back past zero stays on the clamp.
#[test]
fn a_ray_burst_level_and_lane_come_back_over_the_lag() {
    let mut ram = vec![0u8; 0x20_0000];
    let node = 0x8008_30D4;
    put16(&mut ram, node + 0x78, 0x400);
    put16(&mut ram, node + 0x90, 0x800);
    put16(&mut ram, node + 0xA0, 0xFFC);
    put16(&mut ram, node + 0xB8, 0x155);
    assert_eq!(ram_read::displayed_morph_halfword(&ram, node, 0x78, 4), 0);
    assert_eq!(
        ram_read::displayed_morph_halfword(&ram, node, 0xA0, 4),
        2728
    );
    assert_eq!(ram_read::displayed_morph_halfword(&ram, node, 0x78, 6), 0);
}

/// A ray burst's position is the screen's only when its level ramp began
/// before the displayed frame: `battle_melee_hit_spark`'s level `0x900`
/// (`0x100` a vsync) is nine vsyncs old against a six-vsync lag;
/// `rim_elm_gimard_victory`'s `0x400` began on its four-vsync lag's
/// displayed frame itself, and a level on its clamp names no age.
#[test]
fn a_ray_burst_position_stands_only_through_the_lag() {
    let mut ram = vec![0u8; 0x20_0000];
    let node = 0x8008_227C;
    put16(&mut ram, node + 0x90, 0x800);
    put16(&mut ram, node + 0x78, 0x900);
    assert!(ram_read::ray_burst_stood_through_lag(&ram, node, 6));
    put16(&mut ram, node + 0x78, 0x400);
    assert!(!ram_read::ray_burst_stood_through_lag(&ram, node, 4));
    put16(&mut ram, node + 0x78, 0x1000);
    assert!(!ram_read::ray_burst_stood_through_lag(&ram, node, 4));
}

/// A numeral's digit quads are read off both packet arenas, and a run two
/// passes behind the newest is the older run stepped back by its own move:
/// `rim_elm_gimard_victory`'s `6` at `(117, 81)` 12 px in the newer arena
/// and `(124, 92)` 10 px in the older puts the frame on screen at
/// `(131, 103)` 8 px.
#[test]
fn numeral_cells_come_off_the_arenas_and_step_back_a_pass() {
    let mut ram = vec![0u8; 0x20_0000];
    let quad = |ram: &mut Vec<u8>, at: u32, x: i16, y: i16, w: i16, digit: u8| {
        let u = ((digit + 9) % 10) * 24;
        put32(ram, at, 0x0900_0000);
        put32(ram, at + 4, 0x2C80_8080);
        let corners = [
            (x, y),
            (x + w - 1, y),
            (x, y + w - 1),
            (x + w - 1, y + w - 1),
        ];
        for (k, (cx, cy)) in corners.into_iter().enumerate() {
            let o = at + 8 + 8 * k as u32;
            put16(ram, o, cx as u16);
            put16(ram, o + 2, cy as u16);
            put8(ram, o + 4, u + if k % 2 == 1 { 23 } else { 0 });
            put8(ram, o + 5, 64 + if k >= 2 { 23 } else { 0 });
        }
    };
    quad(&mut ram, 0x800F_EF80, 117, 81, 12, 6);
    quad(&mut ram, 0x8013_0EB0, 124, 92, 10, 6);
    let quads = popup_cells::digit_quads(&ram);
    assert_eq!(quads.len(), 2);
    assert_eq!(
        (quads[0].x, quads[0].y, quads[0].w, quads[0].digit),
        (117, 81, 12, 6)
    );
    let back = popup_cells::step_back(&quads[1..], &quads[..1]);
    assert_eq!(
        (back[0].x, back[0].y, back[0].w, back[0].digit),
        (131, 103, 8, 6)
    );
}

/// A strike-loop swing committed between the displayed frame and the RAM's:
/// the ring ids and the cursor ring say so, and the image child stands the
/// acting seat on the ring's displayed pair (`rim_elm_gimard_victory`'s
/// Vahn). A swing that did not recommit keeps the RAM's ground.
#[test]
fn a_recommitted_swing_stands_on_the_displayed_pair() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 1);
    put8(&mut ram, ctx + 1, 1);
    put8(&mut ram, ctx + 6, 0xFF);
    put8(&mut ram, ctx + 7, 0x1E);
    put8(&mut ram, FORMATION_CELL, 0x0A);
    let vahn = 0x800E_C9E8u32;
    for (slot, base) in [(0u32, vahn), (3, 0x800E_D000)] {
        put32(&mut ram, ACTOR_TABLE + slot * 4, base);
        put16(&mut ram, base + 0x14C, 50);
        put16(&mut ram, base + 0x14E, 60);
    }
    for (k, (id, cur, x, z)) in [
        (0x0Eu8, 16u16, 3i32, -154i32),
        (0x0E, 0, 4, -153),
        (0x0F, 144, 12, -217),
    ]
    .into_iter()
    .enumerate()
    {
        let k = k as u32;
        put8(&mut ram, vahn + 0x1FB + k, id);
        put16(&mut ram, vahn + 0x17A + 2 * k, cur);
        put32(&mut ram, vahn + POSITION_RING + 8 * k, x as u32);
        put32(&mut ram, vahn + POSITION_RING + 8 * k + 4, z as u32);
    }
    let mut b = RetailBattle::from_ram(&ram).expect("seedable");
    assert_eq!(b.displayed_recommit.first(), Some(&true));
    b.active_actor = 0;
    b.action_state = 0x1E;
    let seats = b.engine_image_displayed_seats();
    assert_eq!(seats.len(), 1);
    assert_eq!((seats[0].slot, seats[0].x, seats[0].z), (0, 12, -217));
    b.action_state = 0x14;
    assert!(
        b.engine_image_displayed_seats().is_empty(),
        "not in the strike loop"
    );
    b.action_state = 0x1E;
    b.displayed_recommit[0] = false;
    assert!(
        b.engine_image_displayed_seats().is_empty(),
        "no commit in the window"
    );
}

/// A monster strike captured on the strike loop's first pass: the RAM holds
/// the strike entry at cursor `0`, the rings the arrival's idle committed a
/// pass before it, which is what the displayed frame shows
/// (`battle_noaru_gilium_lv2_strike`). The image child poses the attacker on
/// the idle at the ring's cursor and stands it on the ring's pair; off a
/// clip-age gate, or where the rings show the RAM's own clip, it does neither.
#[test]
fn a_first_pass_monster_strike_shows_the_clip_before_its_commit() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 1);
    put8(&mut ram, ctx + 1, 1);
    put8(&mut ram, ctx + 6, 0xFF);
    put8(&mut ram, ctx + 7, 0x1E);
    put8(&mut ram, FORMATION_CELL, 0x0A);
    let (vahn, gilium, record) = (0x800E_C9E8u32, 0x800E_D000u32, 0x800A_8158u32);
    for (slot, base) in [(0u32, vahn), (3, gilium)] {
        put32(&mut ram, ACTOR_TABLE + slot * 4, base);
        put16(&mut ram, base + 0x14C, 50);
        put16(&mut ram, base + 0x14E, 60);
    }
    put32(&mut ram, 0x801C_9348, record);
    let (idle, walk, strike) = (0x800A_C698u32, 0x800A_DF44u32, 0x800B_5754u32);
    for (i, e) in [idle, walk].into_iter().enumerate() {
        put32(&mut ram, record + 0x4C + 4 * i as u32, e);
    }
    put32(&mut ram, record + 0x4C + 4 * 8, strike);
    for (k, (e, cur, x, z)) in [
        (strike, 0u16, -39i32, 287i32),
        (idle, 24, -39, 291),
        (idle, 0, -39, 291),
        (walk, 448, -39, 267),
    ]
    .into_iter()
    .enumerate()
    {
        let k = k as u32;
        put32(&mut ram, gilium + 0x234 + 4 * k, e);
        put16(&mut ram, gilium + 0x17A + 2 * k, cur);
        put32(&mut ram, gilium + POSITION_RING + 8 * k, x as u32);
        put32(&mut ram, gilium + POSITION_RING + 8 * k + 4, z as u32);
    }
    let mut b = RetailBattle::from_ram(&ram).expect("seedable");
    assert_eq!(b.displayed_clips.get(3), Some(&Some((0, 0))));
    assert_eq!(b.displayed_clips.first(), Some(&None), "a party seat");
    b.active_actor = 3;
    b.action_state = 0x1E;
    b.queued_category = 3;
    b.caster_clip = 8;
    b.span_gate = SpanGate::Age { accum: 0 };
    let pc = b.party_count.clamp(1, 3);
    let clips = b.engine_image_displayed_clips();
    assert_eq!(clips.len(), 1);
    assert_eq!((clips[0].slot, clips[0].clip, clips[0].cursor), (pc, 0, 0));
    let seats = b.engine_image_displayed_seats();
    assert_eq!(seats.len(), 1);
    assert_eq!((seats[0].slot, seats[0].x, seats[0].z), (pc, -39, 291));
    b.span_gate = SpanGate::None;
    assert!(
        b.engine_image_displayed_clips().is_empty(),
        "not a clip-age gate"
    );
    b.span_gate = SpanGate::Age { accum: 0 };
    b.caster_clip = 0;
    assert!(
        b.engine_image_displayed_clips().is_empty(),
        "no commit in the window"
    );
    // A monster cast's accumulator gate poses the caster on the displayed
    // slot whether or not a commit stands in the window: the gate's own frame
    // carries the RAM's keyframe (`battle_gimard_tail_fire_a`: `224` against
    // the displayed `208`).
    b.action_state = 0x2B;
    b.queued_category = 2;
    let clips = b.engine_image_displayed_clips();
    assert_eq!(clips.len(), 1, "the magic band");
    assert_eq!((clips[0].slot, clips[0].clip, clips[0].cursor), (pc, 0, 0));
    // So does a monster's Spirit / Item band (a class-0 Heal), whose cast
    // clip plays through `0x3E` on the same accumulator.
    b.action_state = 0x3E;
    let clips = b.engine_image_displayed_clips();
    assert_eq!(clips.len(), 1, "the Spirit / Item band");
    assert_eq!((clips[0].slot, clips[0].clip, clips[0].cursor), (pc, 0, 0));
    b.action_state = 0x1E;
    b.queued_category = 3;

    // A walked-in charge cast past its walk (`koru_blade_breath_mid_cast`):
    // the wind-up committed inside the lag, the display shows the run.
    {
        let mut c = b.clone();
        c.action_state = 0x70;
        c.module_phase = 3;
        c.cam_landed = false;
        c.queued_action = 0xA2;
        c.queued_category = 2;
        c.span_gate = SpanGate::None;
        c.caster_clip = 5;
        c.displayed_clips[3] = Some((1, 136));
        assert!(c.walk_in_shot_aligned());
        let clips = c.engine_image_displayed_clips();
        assert_eq!(clips.len(), 1, "the walked cast's run clip");
        assert_eq!((clips[0].clip, clips[0].cursor), (1, 136));
        c.caster_clip = 1;
        assert!(
            c.engine_image_displayed_clips().is_empty(),
            "still on the run"
        );
    }

    // A walk-in body one pass into its walk arm whose walk clip committed
    // inside the lag (`battle_tunnelc_xain_bloody_horns_mid_cast`: the rings
    // hold the idle at `0` on the displayed slot, the walk `1` at `16` live).
    {
        let mut c = b.clone();
        c.action_state = 0x70;
        c.module_phase = 1;
        c.cam_landed = false;
        c.queued_action = 0x5C;
        c.queued_category = 2;
        c.span_gate = SpanGate::None;
        c.caster_clip = 1;
        assert!(c.walk_arm_follow_aligned());
        let clips = c.engine_image_displayed_clips();
        assert_eq!(clips.len(), 1, "the idle before the walk's commit");
        assert_eq!((clips[0].clip, clips[0].cursor), (0, 0));
        let seats = c.engine_image_displayed_seats();
        assert_eq!(seats.len(), 1);
        assert_eq!((seats[0].x, seats[0].z), (-39, 291), "the ring's pair");
        c.caster_clip = 0;
        assert!(
            c.engine_image_displayed_clips().is_empty(),
            "no commit in the window"
        );
        // Past the walk, a module arm poses the caster on the displayed slot
        // with no commit in the window (`cort_evolved_card_doomsday_mid_cast`).
        c.module_phase = 4;
        let clips = c.engine_image_displayed_clips();
        assert_eq!(clips.len(), 1, "a capture-band module arm");
        assert_eq!((clips[0].clip, clips[0].cursor), (0, 0));
        let seats = c.engine_image_displayed_seats();
        assert_eq!(
            (seats[0].x, seats[0].z),
            (-39, 291),
            "on the ring's pair, as a magic band's caster"
        );
    }

    // The strike's target still idles: its loop is seated, as no hit has
    // staged its reaction yet; a cast's target keeps the replay's clock.
    b.span_gate = SpanGate::Age { accum: 0 };
    b.target_code = 0;
    b.clip_cursors = vec![(0, 0, 12)];
    let seats: Vec<u8> = b.engine_clip_cursors().iter().map(|s| s.slot).collect();
    assert_eq!(seats, vec![0], "the melee target's idle");
    b.queued_category = 2;
    assert!(b.engine_clip_cursors().is_empty(), "a cast's target");
    b.queued_category = 3;

    // Every party seat sat the round out on Spirit: the drive's party does.
    b.party_categories = vec![4];
    assert!(b.party_spirit_round());
    b.party_categories = vec![0];
    assert!(!b.party_spirit_round(), "a member swung or acted otherwise");
}

/// A monster strike whose displayed frame shows the RAM's own clip but whose
/// live pair moved inside the lag (`battle_jou_cort_181_strike`: the arrival's
/// idle committed on the displayed pass, and the loop's passes since backed
/// Cort off his target, `(-201, 530)` displayed against `(-206, 546)` live):
/// the image child stands the attacker on the displayed frame's ring pair.
#[test]
fn a_monster_strike_that_moved_inside_the_lag_stands_on_the_displayed_pair() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 1);
    put8(&mut ram, ctx + 1, 1);
    put8(&mut ram, ctx + 6, 0xFF);
    put8(&mut ram, ctx + 7, 0x1E);
    put8(&mut ram, FORMATION_CELL, 0x0A);
    let (vahn, cort, record) = (0x800E_C9E8u32, 0x800E_D000u32, 0x800A_8158u32);
    for (slot, base) in [(0u32, vahn), (3, cort)] {
        put32(&mut ram, ACTOR_TABLE + slot * 4, base);
        put16(&mut ram, base + 0x14C, 50);
        put16(&mut ram, base + 0x14E, 60);
    }
    put32(&mut ram, 0x801C_9348, record);
    let (idle, walk) = (0x800B_0AE8u32, 0x800B_1C2Cu32);
    for (i, e) in [idle, walk].into_iter().enumerate() {
        put32(&mut ram, record + 0x4C + 4 * i as u32, e);
    }
    put16(&mut ram, cort + 0x34, (-206i16) as u16);
    put16(&mut ram, cort + 0x38, 546);
    for (k, (e, cur, x, z)) in [
        (idle, 32u16, -206i32, 546i32),
        (idle, 16, -206, 546),
        (idle, 0, -201, 530),
        (walk, 188, -185, 466),
    ]
    .into_iter()
    .enumerate()
    {
        let k = k as u32;
        put32(&mut ram, cort + 0x234 + 4 * k, e);
        put16(&mut ram, cort + 0x17A + 2 * k, cur);
        put32(&mut ram, cort + POSITION_RING + 8 * k, x as u32);
        put32(&mut ram, cort + POSITION_RING + 8 * k + 4, z as u32);
    }
    let mut b = RetailBattle::from_ram(&ram).expect("seedable");
    b.active_actor = 3;
    b.action_state = 0x1E;
    b.queued_category = 3;
    b.caster_clip = 0;
    b.span_gate = SpanGate::Age { accum: 64 };
    let pc = b.party_count.clamp(1, 3);
    assert!(
        b.engine_image_displayed_clips().is_empty(),
        "no commit in the window"
    );
    let seats = b.engine_image_displayed_seats();
    assert_eq!(seats.len(), 1);
    assert_eq!((seats[0].slot, seats[0].x, seats[0].z), (pc, -201, 530));
    // Standing where it stood, it takes no seat.
    b.displayed_ground[3] = Some([-206, 546]);
    assert!(b.engine_image_displayed_seats().is_empty());
}

/// A round prompt caught on its far framing's arm: `gaza2_card_first_menu`'s
/// table (TR z `353` a frame with `1050` to go, TR y `19` with `47`, pitch
/// landed) has three frames left, and the displayed frame six vsyncs more.
/// Off the case-9 surfaces, or landed, there is no seat.
#[test]
fn a_prompt_on_its_far_framing_arm_names_the_frames_left() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put8(&mut ram, ctx + 6, 0x1E);
    let rec = |k: u32| ctx + camera_align::TWEEN_TABLE + k * 4;
    put16(&mut ram, rec(0), 3);
    put16(&mut ram, rec(0) + 2, 32);
    put16(&mut ram, 0x8007_B790, 32);
    put16(&mut ram, rec(4), 19);
    put16(&mut ram, rec(4) + 2, 1280);
    put32(&mut ram, 0x8008_40BC, 1327);
    put16(&mut ram, rec(5), 353);
    put16(&mut ram, rec(5) + 2, 7800);
    put32(&mut ram, 0x8008_40C0, 6750);
    let left = camera_align::displayed_menu_glide_left(&ram, ctx, 6).expect("walking");
    assert!((left - (6.0 + 1050.0 / 353.0)).abs() < 1e-3, "{left}");
    put8(&mut ram, ctx + 6, 0x28);
    assert_eq!(camera_align::displayed_menu_glide_left(&ram, ctx, 6), None);
    put8(&mut ram, ctx + 6, 0x1E);
    // Retail's own table: the walkers less six increments, a landed pitch
    // left to the arm.
    let shown = camera_align::displayed_menu_glide_pose(&ram, ctx, 6).expect("walking");
    assert_eq!(shown.tr[1], 1327.0 + 6.0 * 19.0);
    assert_eq!(shown.tr[2], 6750.0 - 6.0 * 353.0);
    assert!(shown.pitch.is_nan());
    assert_eq!(shown.focus[2], 0.0);
    put32(&mut ram, 0x8008_40BC, 1280);
    put32(&mut ram, 0x8008_40C0, 7800);
    assert_eq!(camera_align::displayed_menu_glide_left(&ram, ctx, 6), None);
}

/// A party Attack capture's yaw counter climbs from the Attack arm's
/// `0x200` unless one of the acting seat's action entries carries the
/// `+0x87 == 2` byte the swing re-seed fires on (`FUN_8004E13C`); the drive
/// pins the counter only on a rung both sides stand on.
#[test]
fn an_attacks_yaw_rung_holds_without_a_reseed_entry() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 1);
    put8(&mut ram, ctx + 1, 1);
    put8(&mut ram, ctx + 6, 0xFF);
    put8(&mut ram, ctx + 7, 0x51);
    put8(&mut ram, ctx + 0x13, 0);
    put16(&mut ram, ctx + 0x6DA, 1114);
    put8(&mut ram, FORMATION_CELL, 0xB5);
    let actor = 0x800E_C9E8;
    put32(&mut ram, ACTOR_TABLE, actor);
    put8(&mut ram, actor + 0x1DE, 3);
    let table = 0x8018_7A14;
    put32(&mut ram, PARTY_ENTRY_TABLES, table);
    for slot in 0..PARTY_ENTRY_SLOTS {
        put32(&mut ram, table + slot * 4, 0x8018_8000 + slot * 0x100);
    }
    let rung = |ram: &[u8]| {
        let b = RetailBattle::from_ram(ram).expect("seedable");
        match b.battle_drive() {
            Some(BattleDrive::Action { steer, .. }) => steer.rung,
            other => panic!("not an action drive: {other:?}"),
        }
    };
    assert_eq!(rung(&ram), Some(0x200));
    // An art entry that re-seeds the counter leaves the rung unknown.
    put8(&mut ram, 0x8018_8000 + 0x10 * 0x100 + 0x87, 2);
    assert_eq!(rung(&ram), None);
}

/// A party arts capture on a dynamic art slot names the latched raw id
/// `+0x1DB` its last commit installed: the SpecialStarter and the Hyper Art
/// behind it both play in slot `0x11`, and once the strike loop has staged
/// the art the cursor names both, so the drive's age gate also waits for the
/// engine to latch the capture's id.
#[test]
fn an_arts_capture_on_the_art_slot_gates_on_the_latched_id() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 1);
    put8(&mut ram, ctx + 1, 1);
    put8(&mut ram, ctx + 6, 0xFF);
    put8(&mut ram, ctx + 7, 0x1F);
    put8(&mut ram, ctx + 0x13, 0);
    put8(&mut ram, ctx + 0x15, 5);
    put8(&mut ram, FORMATION_CELL, 0x0D);
    let actor = 0x800E_C9E8;
    put32(&mut ram, ACTOR_TABLE, actor);
    put8(&mut ram, actor + 0x1DD, 3);
    put8(&mut ram, actor + 0x1DE, 3);
    for (i, b) in [0x0C, 0x0F, 0x0F, 0x1A, 0x1E].into_iter().enumerate() {
        put8(&mut ram, actor + 0x1DF + i as u32, b);
    }
    put8(&mut ram, actor + 0x1D9, 0x11);
    put8(&mut ram, actor + 0x1DA, 0x11);
    put8(&mut ram, actor + 0x1DB, 0x1E);
    let steer = |ram: &[u8]| {
        let b = RetailBattle::from_ram(ram).expect("seedable");
        match b.battle_drive() {
            Some(BattleDrive::Action { steer, .. }) => steer,
            other => panic!("not an action drive: {other:?}"),
        }
    };
    let s = steer(&ram);
    assert_eq!((s.clip, s.cursor, s.latched), (None, Some(5), Some(0x1E)));
    // A swing clip is placed by the clip gate and the age alone.
    put8(&mut ram, actor + 0x1D9, 0x0F);
    put8(&mut ram, actor + 0x1DB, 0x0F);
    assert_eq!(steer(&ram).latched, None);
}

/// A party Attack's Done-band capture places its walk once a first run has
/// measured the string: the counter's span from the Attack arm's `0x200`,
/// turned into vsyncs at the ring's rate, less the replay's string, is the
/// walk (`RetailBattle::party_approach`).
#[test]
fn a_party_done_capture_places_its_walk_from_the_measured_string() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 1);
    put8(&mut ram, ctx + 1, 1);
    put8(&mut ram, ctx + 6, 0xFF);
    put8(&mut ram, ctx + 7, 0x51);
    put8(&mut ram, ctx + 0x13, 0);
    put8(&mut ram, FORMATION_CELL, 0xB5);
    let actor = 0x800E_C9E8;
    put32(&mut ram, ACTOR_TABLE, actor);
    put8(&mut ram, actor + 0x1DD, 3);
    put8(&mut ram, actor + 0x1DE, 3);
    let mut b = RetailBattle::from_ram(&ram).expect("seedable");
    b.ring_steps = [3; 16];
    // 60 vsyncs of walk and a 400-vsync string at step 3: `4 / 3` a vsync.
    b.walk_yaw_base = 0x200 + 460 * 4 / 3;
    b.captured = vec![None; 8];
    b.captured[0] = Some(([-125, -254], [0, 0], 0, 0));
    b.captured[3] = Some(([170, 375], [0, 0], 0, 0));
    assert!(b.wants_party_combo());
    assert_eq!(b.capture_approach_yaw(), None, "unmeasured");
    let app = b
        .with_party_combo(400)
        .capture_approach_yaw()
        .expect("a walk to place");
    assert_eq!(app.arrive, 0x200 + 60);
    assert_eq!((app.strike, app.target_ground), ([-125, -254], [170, 375]));
    // A string as long as the whole span leaves no walk to place.
    assert_eq!(b.with_party_combo(458).capture_approach_yaw(), None);
}

/// A string longer than the damage ring has wrapped it: the newest eight
/// pushes sit at `i mod 8`, each older one is an unknown amount
/// (`ram_read::action_hit_sequence`).
#[test]
fn a_wrapped_damage_ring_reads_its_newest_eight_pushes() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put8(&mut ram, ctx + 6, 0xFF);
    put8(&mut ram, ctx + 7, 0x51);
    put8(&mut ram, ctx + 0x273, 10);
    for e in 0..8u32 {
        put16(&mut ram, ctx + 0x318 + e * 2, 3);
        put16(&mut ram, ctx + 0x83C + e * 4, 100 + e as u16);
    }
    let seq = ram_read::action_hit_sequence(&ram, ctx);
    assert_eq!(seq.len(), 10);
    assert_eq!(
        &seq[..2],
        &[(3, 0), (3, 0)],
        "overwritten by pushes 8 and 9"
    );
    assert_eq!(seq[2], (3, 102));
    assert_eq!(&seq[8..], &[(3, 100), (3, 101)]);
}

/// A monster's turning case `0xE` shot: three step-`3` passes past the commit
/// read accumulator `48`, two of them on the shot, so its yaw walks back
/// `2 * 3 * 4` onto the case-6 yaw (`battle_tower_guardian_windhash_mid_cast`
/// `1321` to `1297`). Two step-`4` passes read `32`, one on the shot
/// (`battle_korb2_aluru_cannon_mid_cast` `3841` to `3825`).
#[test]
fn a_turning_shot_walks_its_yaw_back_onto_the_framing_it_replaced() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 3);
    put8(&mut ram, ctx + 1, 2);
    put8(&mut ram, ctx + 6, 0x1E);
    put8(&mut ram, FORMATION_CELL, 0x4F);
    put8(&mut ram, SEAT_CHARS, 1);
    put32(&mut ram, ACTOR_TABLE, 0x800E_C9E8);
    put16(&mut ram, 0x800E_C9E8 + 0x14C, 99);
    put16(&mut ram, 0x800E_C9E8 + 0x14E, 99);
    let mut b = RetailBattle::from_ram(&ram).expect("seedable");
    b.flow = 0xFF;
    b.active_actor = 4;
    b.action_state = 0x2B;
    b.queued_category = 2;
    b.queued_shot = 0x0E;
    b.span_gate = SpanGate::Age { accum: 48 };
    b.step_run = (3, 3);
    b.cam_tween.0.yaw = 1321.0;
    assert_eq!(b.retake_turning_yaw(), Some(1297));
    b.span_gate = SpanGate::Age { accum: 32 };
    b.step_run = (4, 3);
    b.cam_tween.0.yaw = 3841.0;
    assert_eq!(b.retake_turning_yaw(), Some(3825));
    b.queued_shot = 0x07;
    assert_eq!(b.retake_turning_yaw(), None, "not a turning shot");
    // The whole pose: `battle_korb2_aluru_cannon_mid_cast`'s table and steps.
    b.queued_shot = 0x0E;
    b.step_run = (4, 3);
    b.recent_steps[..4].copy_from_slice(&[4, 4, 4, 3]);
    b.walk_yaw_base = 2088;
    b.cam_style = 0;
    b.cam_tween.0.focus = [474.0, 0.0, 978.0];
    b.cam_tween.1.pitch = 4080.0;
    b.cam_tween.1.tr = [0.0, 768.0, 4915.0];
    b.cam_tween_steps = [19, 16, 0, 0, 507, 79, 10, 0, 96];
    b.party_count = 3;
    b.monster_count = 2;
    b.active_actor = 4;
    b.turn_cursor = 3;
    b.init_keys = [144, 0, 0, 0, 0, 0, 0, 0].map(Some).to_vec();
    b.ground = [
        [-154, -831],
        [755, -978],
        [-754, -781],
        [558, -652],
        [474, 978],
    ]
    .map(Some)
    .to_vec();
    b.facing = vec![None, None, None, None, Some(2265)];
    let p = b.retake_turning_pose().expect("a pose");
    assert_eq!(
        (p.pitch, p.yaw, p.tr, p.focus),
        (7.0, 3757.0, [0.0, 1271.0, 4772.0], [492.0, 0.0, 802.0])
    );
    // A round's first action walks off the far framing: no earlier seat.
    b.turn_cursor = 0;
    assert!(b.retake_turning_pose().is_none());
}

#[test]
fn only_a_monsters_strike_stands_on_the_walked_back_pose() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 3);
    put8(&mut ram, ctx + 1, 2);
    put8(&mut ram, ctx + 6, 0x1E);
    put8(&mut ram, FORMATION_CELL, 0x4F);
    put8(&mut ram, SEAT_CHARS, 1);
    put32(&mut ram, ACTOR_TABLE, 0x800E_C9E8);
    put16(&mut ram, 0x800E_C9E8 + 0x14C, 99);
    put16(&mut ram, 0x800E_C9E8 + 0x14E, 99);
    let mut b = RetailBattle::from_ram(&ram).expect("seedable");
    b.flow = 0xFF;
    b.active_actor = 4;
    b.action_state = 0x1E;
    b.queued_category = 3;
    b.span_gate = SpanGate::Age { accum: 0 };
    assert!(b.stand_on_displayed_pose());
    b.active_actor = 0;
    assert!(!b.stand_on_displayed_pose(), "a party seat");
    b.active_actor = 4;
    b.action_state = 0x2B;
    assert!(!b.stand_on_displayed_pose(), "a cast");
    b.action_state = 0x70;
    b.queued_action = 0x5B;
    b.module_phase = 3;
    assert!(b.stand_on_displayed_pose(), "Scythe Wind's sweep");
    b.module_phase = 2;
    assert!(!b.stand_on_displayed_pose(), "the arm that arms once");
}

/// The recorded case-6 tail of Aluru Cannon: TR z `4772` walks `+36` on the
/// armed pass (step `3`) and `+48` on the terminator's (step `4`), its stale
/// increment `12` (`ceil(143 / 12)`), and the shot closes the last `59`.
#[test]
fn a_stale_pass_walks_back_on_the_armed_increment() {
    assert_eq!(stale_pass_gap(59, 4, 3), 107);
    assert_eq!(stale_pass_gap(19, 4, 3), 35);
    assert_eq!(stale_pass_gap(0, 4, 3), 0);
}

/// The art arm's one-frame shot leaves `ceil(gap / 2)` in the table, so a
/// capture on the commit pass names the depth the swing walked from.
#[test]
fn a_commit_capture_stands_two_steps_behind_the_tables_depth() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 3);
    put8(&mut ram, ctx + 1, 2);
    put8(&mut ram, ctx + 6, 0x1E);
    put8(&mut ram, FORMATION_CELL, 0x4F);
    put8(&mut ram, SEAT_CHARS, 1);
    put32(&mut ram, ACTOR_TABLE, 0x800E_C9E8);
    put16(&mut ram, 0x800E_C9E8 + 0x14C, 99);
    put16(&mut ram, 0x800E_C9E8 + 0x14E, 99);
    let mut b = RetailBattle::from_ram(&ram).expect("seedable");
    b.flow = 0xFF;
    b.active_actor = 0;
    b.action_state = 0x1F;
    b.queued_category = 3;
    b.caster_latched = 0x1E;
    b.committed_queue[..5].copy_from_slice(&[0x0C, 0x0F, 0x0F, 0x1A, 0x1E]);
    b.display_phase_lag = 2;
    b.cam_accum = 0;
    b.cam_tween.0.tr[2] = 1945.0;
    b.cam_tween.1.tr[2] = 1945.0;
    b.displayed_cam_live.tr[2] = 1945.0;
    b.cam_tween_steps[5] = 103;
    assert!(b.camera_behind_commit());
    assert_eq!(b.displayed_commit_live().tr[2], 2151.0);
    b.cam_accum = 8;
    assert_eq!(b.displayed_commit_live().tr[2], 1945.0);
}

/// Case 7 re-arms `ceil(gap / 0xC)` from `0x700` under the approach's case 6,
/// so the table's increment names the pass: the first at `150` on any step,
/// and the ladder below it falls by the step.
#[test]
fn a_walk_clock_strike_counts_its_passes_off_the_table() {
    // `battle_nilboa_warfish_strike`: step 2, gap 1490, increment 150, on
    // accumulator 288 the walk's loop left.
    assert_eq!(strike_passes_off_table(150, 2), 1);
    // `battle_map02_poisonous_worm_strike`: step 3, gap 998, increment 111, on
    // accumulator 384.
    assert_eq!(strike_passes_off_table(111, 3), 2);
    // `battle_rikuroa_worm_strike`: step 2, gap 879, increment 88.
    assert_eq!(strike_passes_off_table(88, 2), 4);
    // The ladder ends: a landed table does not count forever.
    assert!(strike_passes_off_table(0, 2) < 64);
}

/// Two retail recordings of a monster's cast, pass by pass from the seed
/// `0x0C` to the `0x2A` terminator: `retona_viguro_v2_pre_thunder_break` ran
/// fourteen step-`2` passes and `noaru_slippery_v2_pre_poisonous_breath` ten
/// step-`3` passes, and every component lands on the terminator row's value.
#[test]
fn a_case_6_walk_lands_on_the_recorded_terminator_pose() {
    let retona = [2; 14];
    assert_eq!(capture_drive::case6_walk(128, 0, &retona), 6);
    assert_eq!(capture_drive::case6_walk(1024, 1280, &retona), 1264);
    assert_eq!(capture_drive::case6_walk(3276, 5324, &retona), 5176);
    assert_eq!(capture_drive::case6_walk(48, 943, &retona), 880);
    assert_eq!(capture_drive::case6_walk(-296, 187, &retona), 158);
    let slippery = [3; 10];
    assert_eq!(capture_drive::case6_walk(3276, 4915, &slippery), 4839);
    assert_eq!(capture_drive::case6_walk(821, 1024, &slippery), 1019);
    assert_eq!(capture_drive::case6_walk(-525, 525, &slippery), 480);
}

/// `0x29` leaves on the pass its `0x14` timer reads negative, so a walk runs
/// `0x14 / step + 1` wait passes behind the seed and `0x28`, then the
/// terminator: fourteen passes at step `2`, ten at step `3`.
#[test]
fn a_case_6_walk_runs_the_cast_waits_passes() {
    assert_eq!(capture_drive::case6_walk_passes(&[2; 16], 2).len(), 14);
    assert_eq!(capture_drive::case6_walk_passes(&[3; 16], 2).len(), 10);
    // The ring's slot `3` is the terminator, the last pass walked.
    let mut ring = [3; 16];
    ring[3] = 4;
    assert_eq!(capture_drive::case6_walk_passes(&ring, 2).last(), Some(&4));
    // One `0x2B` pass in, the display is `0x29`'s exit: the terminator takes
    // no step, so the exit is an armed pass like the rest.
    let one_in = capture_drive::case6_walk_passes(&[3; 16], 1);
    assert_eq!(one_in.len(), 10);
    assert_eq!(one_in.last(), Some(&0));
}

/// A magic-band step run that began inside `0x2B` holds its step back until
/// the accumulator reaches the passes before it: Caruban's Fire Breath on a
/// full party (`battle_rugi_caruban_3_fire_breath_mid_cast`) reads
/// accumulator `64` over steps `3`, `3`, `2` (newest first), so the run's two
/// step-`3` passes open once the first `0x2B` pass has added its `16`.
#[test]
fn a_magic_band_step_run_waits_for_the_passes_before_it() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 3);
    put8(&mut ram, ctx + 1, 2);
    put8(&mut ram, ctx + 6, 0x1E);
    put8(&mut ram, FORMATION_CELL, 0x4F);
    put8(&mut ram, SEAT_CHARS, 1);
    put32(&mut ram, ACTOR_TABLE, 0x800E_C9E8);
    put16(&mut ram, 0x800E_C9E8 + 0x14C, 99);
    put16(&mut ram, 0x800E_C9E8 + 0x14E, 99);
    let mut b = RetailBattle::from_ram(&ram).expect("seedable");
    b.span_gate = SpanGate::Age { accum: 64 };
    b.step_run = (3, 2);
    b.recent_steps = [3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2];
    assert_eq!(b.step_run_accum_hold(), Some(16));
    // A run over every `0x2B` pass waits for nothing (Windhash's
    // accumulator `48` over two step-`3` passes of a three-pass run).
    b.span_gate = SpanGate::Age { accum: 48 };
    b.step_run = (3, 3);
    b.recent_steps[..3].copy_from_slice(&[3, 3, 3]);
    assert_eq!(b.step_run_accum_hold(), None);
    // Passes before the run on another step than the replay's ordinary one
    // cannot be counted by it (`battle_deene_ironman_jagged_earth_mid_cast`:
    // `136` over `4`, `4`, `3`, `3`, `3`).
    b.span_gate = SpanGate::Age { accum: 136 };
    b.step_run = (4, 2);
    b.recent_steps[..5].copy_from_slice(&[4, 4, 3, 3, 3]);
    assert_eq!(b.step_run_accum_hold(), None);
    // A history that does not add up to the accumulator names no pass.
    b.span_gate = SpanGate::Age { accum: 60 };
    b.step_run = (3, 2);
    b.recent_steps[..3].copy_from_slice(&[3, 3, 2]);
    assert_eq!(b.step_run_accum_hold(), None);
}

/// A `0x6F` capture walks back on the re-arm's widening share, with the TR z
/// endpoint the depth ramp moved: `battle_map01_gomboo_steal_mid_cast`'s
/// words (depth `3024` after one step-`3` ramp, so the pass before chased
/// `prescale(3072) = 4915`, `77` past the RAM's `4838`). Its yaw stands
/// `63 + 84` back - one pass on the table's `21`, one on the re-arm's
/// `ceil(246 / 9) = 28` - where the table's own increment put it `126` back,
/// a yaw increment short of the frame on screen.
#[test]
fn a_capture_fade_walks_back_on_the_rearm_with_the_ramped_endpoint() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let rec = |k: u32| ctx + camera_align::TWEEN_TABLE + k * 4;
    put8(&mut ram, ctx + 7, 0x6F);
    put16(&mut ram, ctx + 0x6D0, 3024);
    put16(&mut ram, 0x8007_B790, 10);
    put16(&mut ram, 0x8007_B792, 3757);
    put32(&mut ram, 0x8008_40BC, 1257);
    put32(&mut ram, 0x8008_40C0, 4717);
    put32(&mut ram, 0x8008_9118, (-364i32) as u32);
    put32(&mut ram, 0x8008_9120, (-754i32) as u32);
    for (k, inc, end) in [
        (0u32, 2u16, 0i16),
        (1, 21, 3940),
        (4, 3, 1280),
        (5, 14, 4838),
        (6, 6, -417),
        (8, 22, -941),
    ] {
        put16(&mut ram, rec(k), inc);
        put16(&mut ram, rec(k) + 2, end as u16);
    }
    let shift = camera_align::capture_fade_end_shift(&ram, ctx, &[3, 3]);
    assert_eq!(shift[5], 77);
    assert_eq!(shift.iter().filter(|&&s| s != 0).count(), 1);
    let p =
        camera_align::displayed_cam_live_steps(&ram, ctx, &[3, 3], false, [None; 9], shift, false);
    assert_eq!(p.yaw, 3610.0);
    let fixed =
        camera_align::displayed_cam_live_steps(&ram, ctx, &[3, 3], false, [None; 9], [0; 9], false);
    assert!(
        p.tr[2] < fixed.tr[2],
        "the ramped endpoint widens the gap: {} vs {}",
        p.tr[2],
        fixed.tr[2]
    );
}

/// The walk-in bodies whose arm 0 stores the yaw counter's `0x200` rung (PROT
/// 0951 / 0952) leave no seed clock behind; the others keep theirs.
#[test]
fn only_the_counter_storing_walk_ins_drop_the_yaw_clock() {
    use super::seed_plan::yaw_counter_reseeded_by;
    assert!(yaw_counter_reseeded_by(0x5C), "Bloody Horns");
    assert!(yaw_counter_reseeded_by(0x5B), "Scythe Wind");
    assert!(yaw_counter_reseeded_by(0xB8), "Astral Slash");
    assert!(
        !yaw_counter_reseeded_by(0x6F),
        "Melt Spray walks on the seed's counter"
    );
    assert!(!yaw_counter_reseeded_by(0x60), "White Shield does not walk");
    assert!(
        !yaw_counter_reseeded_by(0x81),
        "a summon is no capture body"
    );
}

/// A bar and plaque the Attack arm raised, retired in the RAM, are seated on
/// the elapsed the yaw counter's passes since the arm name, less the newest
/// two (`battle_nilboa_aluru_v2_strike`: counter `0x212` at step `2`, both
/// retired at `14`, the screen at `14`); past their landing on screen, or off
/// the arm's rung, nothing is seated.
#[test]
fn attack_arm_glides_are_seated_off_the_yaw_counter() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put16(&mut ram, STEP_MODE, 0x10);
    for i in 0..16 {
        put16(&mut ram, FRAME_HISTORY + i * 2, 0x1C0);
    }
    put8(&mut ram, ctx + 7, 0x1E);
    for (slot, target, start) in [(0u32, [16i16, 192], [16i16, 234]), (1, [16, 12], [16, -26])] {
        let r = ctx + HUD_GLIDE_TABLE + slot * HUD_GLIDE_STRIDE;
        put8(&mut ram, r + 1, 14);
        put16(&mut ram, r + 4, target[0] as u16);
        put16(&mut ram, r + 6, target[1] as u16);
        put16(&mut ram, r + 8, start[0] as u16);
        put16(&mut ram, r + 10, start[1] as u16);
    }
    put16(&mut ram, ctx + 0x6DA, 0x212);
    let seats = attack_arm_glide_seats(&ram, ctx);
    assert_eq!(
        seats,
        vec![
            HudGlideSeat {
                target: [16, 192],
                elapsed: 14,
                total: 16
            },
            HudGlideSeat {
                target: [16, 12],
                elapsed: 14,
                total: 16
            },
        ]
    );
    // Two passes later the screen shows them landed.
    put16(&mut ram, ctx + 0x6DA, 0x216);
    assert!(attack_arm_glide_seats(&ram, ctx).is_empty());
    // A counter on another rung names no arm.
    put16(&mut ram, ctx + 0x6DA, 0x812);
    assert!(attack_arm_glide_seats(&ram, ctx).is_empty());
}

/// A Spirit / Item band's plaques, retired in the RAM, are seated on the
/// backdrop weight's age less the display lag
/// (`battle_deene_garnet_heal_mid_cast`: weight `0x500`, both retired at
/// `13`, a seven-vsync lag, the screen at `13`); a weight on a ceiling, a
/// rise already landed on screen or a fallen latch seats nothing.
#[test]
fn spirit_band_plaques_are_seated_off_the_backdrop_weight() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    let backdrop = 0x800F_0000;
    put8(&mut ram, ctx + 7, 0x3E);
    put8(&mut ram, ctx + 0x243, 1);
    put32(&mut ram, ctx + 0x106C, backdrop);
    put16(&mut ram, backdrop + 0x78, 0x500);
    for (slot, target, start) in [
        (0u32, [16i16, 12], [16i16, -26]),
        (1, [251, 192], [251, 234]),
        (2, [-304, 192], [16, 192]),
    ] {
        let r = ctx + HUD_GLIDE_TABLE + slot * HUD_GLIDE_STRIDE;
        put8(&mut ram, r + 1, 13);
        put16(&mut ram, r + 4, target[0] as u16);
        put16(&mut ram, r + 6, target[1] as u16);
        put16(&mut ram, r + 8, start[0] as u16);
        put16(&mut ram, r + 10, start[1] as u16);
    }
    let seat = |target, elapsed| HudGlideSeat {
        target,
        elapsed,
        total: 16,
    };
    assert_eq!(
        spirit_band_glide_seats(&ram, ctx, 7),
        vec![seat([16, 12], 13), seat([251, 192], 13)],
        "the plaques, not the roster panel leaving"
    );
    assert!(
        spirit_band_glide_seats(&ram, ctx, 2).is_empty(),
        "landed on screen"
    );
    put16(&mut ram, backdrop + 0x78, 0x800);
    assert!(
        spirit_band_glide_seats(&ram, ctx, 7).is_empty(),
        "a ceiling"
    );
    put16(&mut ram, backdrop + 0x78, 0x500);
    put8(&mut ram, ctx + 0x243, 0);
    assert!(spirit_band_glide_seats(&ram, ctx, 7).is_empty(), "no latch");
}

/// A part whose program switched a rate inside the display lag is taken back
/// on the old rate before the switch: PROT 0964's ray (`0x801F9008`) on the
/// pass its length turned from `+96` to `-96`, length `384`, reads `288`
/// eight vsyncs back, and one twelve vsyncs into the shrink reads `336`.
#[test]
fn a_rate_switched_inside_the_lag_rewinds_on_the_old_rate() {
    let mut ram = vec![0u8; 0x20_0000];
    let rec = 0x801F_9008u32;
    // `13 -128`, `28 12`, `9 31`, `28 -12`, `13 128`, `9 31`, `8` after the
    // record's two-halfword head and a `9 0`.
    let prog: [i16; 16] = [
        -1, 0, 9, 0, 13, -128, 28, 12, 9, 31, 28, -12, 13, 128, 9, 31,
    ];
    for (i, &v) in prog.iter().enumerate() {
        put16(&mut ram, rec + 2 * i as u32, v as u16);
    }
    put16(&mut ram, rec + 32, 8);
    let n = 0x8008_1AE4;
    put32(&mut ram, n + 0x48, rec);
    put16(&mut ram, n + 0x70, 16);
    put16(&mut ram, n + 0x5A, 2);
    put16(&mut ram, n + 0xC8, 384);
    put16(&mut ram, n + 0xCA, (-96i16) as u16);
    put16(&mut ram, n + 0x54, 248);
    put16(&mut ram, n + 0x78, 0);
    put16(&mut ram, n + 0x90, 1024);
    assert_eq!(ram_read::displayed_part_halfword(&ram, n, 0xC8, 8), 288);
    // Its level was fading in at `-128` a vsync until the switch.
    assert_eq!(ram_read::displayed_morph_halfword(&ram, n, 0x78, 8), 1024);
    put16(&mut ram, n + 0x54, 152);
    put16(&mut ram, n + 0xC8, 240);
    assert_eq!(ram_read::displayed_part_halfword(&ram, n, 0xC8, 8), 336);
}

/// A magic-band capture whose accumulator adds up only on the history past
/// its newest entry: `battle_garmel_songi_hyper_wave_mid_cast` holds `48`
/// over steps `3, 2, 2, 2` (newest first), three step-`2` passes, so the
/// display's two passes ran on step `2`. One that adds up on the newest
/// reading keeps it.
#[test]
fn a_magic_band_accumulator_reads_the_older_steps_where_they_add_up() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx + 7, 0x2B);
    put16(&mut ram, frame_step::STEP_MODE, 0x10);
    // The newest entry (cursor 4, so slot 3) is the dear frame.
    put32(&mut ram, frame_step::FRAME_HISTORY_CURSOR, 4);
    for k in 0..16u32 {
        put16(&mut ram, frame_step::FRAME_HISTORY + k * 2, 0x1C0);
    }
    put16(&mut ram, frame_step::FRAME_HISTORY + 3 * 2, 534);
    assert_eq!(frame_step::recent_pass_steps(&ram)[..3], [3, 2, 2]);
    put32(&mut ram, ctx + 0x87C, 48);
    assert_eq!(frame_step::magic_band_older_steps(&ram), Some([2, 2]));
    // `24 + 16 + 16`: the newest reading adds up, so it stands.
    put32(&mut ram, ctx + 0x87C, 56);
    assert_eq!(frame_step::magic_band_older_steps(&ram), None);
    // Outside `0x2B` the accumulator counts other passes.
    put32(&mut ram, ctx + 0x87C, 48);
    put8(&mut ram, ctx + 7, 0x70);
    assert_eq!(frame_step::magic_band_older_steps(&ram), None);
}

/// A monster strike saved on its arrival pass (`0x1E` just stored by the
/// poll `0x19`, cursor `ctx[+0x15]` still `0`, the walk clip on) holds the
/// approach's case-6 table, not case 7's ladder: its walk ended on the RAM's
/// counter, so the approach start reads that counter as the arrival
/// (`battle_nilboa_lucifer_strike`, `568`; read off the ladder it was `530`
/// and the replay walked four passes where retail's ring shows 27).
#[test]
fn an_arrival_capture_places_its_walk_on_the_rams_counter() {
    let mut ram = vec![0u8; 0x20_0000];
    let ctx = 0x800E_B654;
    put32(&mut ram, BATTLE_CTX, ctx);
    put8(&mut ram, ctx, 1);
    put8(&mut ram, ctx + 1, 1);
    put8(&mut ram, ctx + 6, 0xFF);
    put8(&mut ram, ctx + 7, 0x1E);
    put8(&mut ram, FORMATION_CELL, 0x4F);
    put32(&mut ram, ACTOR_TABLE, 0x800E_C9E8);
    let mut b = RetailBattle::from_ram(&ram).expect("seedable");
    b.flow = 0xFF;
    b.active_actor = 3;
    b.action_state = 0x1E;
    b.queued_category = 3;
    b.caster_clip = 1;
    b.strike_cursor = 0;
    b.target_code = 0;
    b.party_count = 1;
    b.span_gate = SpanGate::Age { accum: 432 };
    b.step_run = (2, 16);
    b.walk_yaw_base = 568;
    // The case-6 table the arrival armed: the yaw has landed.
    b.cam_tween_steps = [0; 9];
    b.captured = vec![None; 8];
    b.captured[0] = Some(([0, 0], [0, 0], 0, 0));
    b.captured[3] = Some(([0, 312], [0, 0], 0, 0));
    assert!(b.walk_clock_arrival());
    let app = b.capture_approach_yaw().expect("a walk to place");
    assert_eq!(app.arrive, 568);
    assert_eq!((app.strike, app.target_ground), ([0, 312], [0, 0]));
    // The same pass with the staged idle committed (clip `0`, accumulator
    // `0`): still the walk's last reading (`battle_dolk_gimard_strike`).
    b.caster_clip = 0;
    b.span_gate = SpanGate::Age { accum: 0 };
    assert_eq!(b.capture_approach_yaw().map(|a| a.arrive), Some(568));
    // Its framing endpoint was armed on the pass's starting pair (ring slot
    // `1`) and the heading the pass before left: counter `568`, style `3`,
    // table yaw `568` name heading `0x800`.
    b.arm_ground = Some([0, 304]);
    b.cam_style = 3;
    b.cam_tween.1.yaw = 568.0;
    b.monster_count = 1;
    b.facing = vec![None; 8];
    b.facing[3] = Some(2100);
    let arm = b.arrival_arm_snaps().expect("an arm seat");
    let s = arm.iter().find(|s| s.slot == 1).expect("the attacker");
    assert_eq!((s.x, s.z, s.facing), (0, 304, Some(0x800)));
    b.caster_clip = 1;
    b.span_gate = SpanGate::Age { accum: 432 };
    // One strike-loop pass later the table is case 7's, read off its ladder.
    b.strike_cursor = 1;
    assert!(!b.walk_clock_arrival());
    assert!(b.capture_approach_yaw().is_none_or(|a| a.arrive < 568));
    assert!(b.arrival_arm_snaps().is_none());
}
