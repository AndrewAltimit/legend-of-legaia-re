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
            age: 6
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
    let seeds = ram_read::retail_effect_children(&ram);
    assert_eq!(seeds.len(), 2);
    assert_eq!(seeds[0].anim_id, 1);
    assert_eq!((seeds[0].frame_cursor, seeds[0].wait), (2, 32));
    // Two motion steps of `-4 * 1` taken back off Y.
    assert_eq!(seeds[0].pos[1], (-300 << 8) + 8);
    assert_eq!((seeds[1].frame_cursor, seeds[1].wait), (0, 8));
    assert_eq!(seeds[1].mirror, 2);
}
