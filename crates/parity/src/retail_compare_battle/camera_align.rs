//! The battle camera's tween at the capture instant: whether it has landed,
//! the tween read out of the context, and its environment hand-off.

use super::*;

/// The tween table `FUN_801D829C` builds (`ctx[+0x118C]`, nine
/// `{u16 step, u16 endpoint}` records: pitch / yaw / roll, the translation
/// trio, the focus trio).
pub(super) const TWEEN_TABLE: u32 = 0x118C;

/// Whether retail's camera sits on its framing: every tweened component with
/// a step reads its endpoint. The yaw is left out - the idle orbit writes it
/// beside the walker. The rotation pair and the translation / focus words
/// compare as the low halfwords the walker steps.
pub(super) fn camera_landed(ram: &[u8], ctx: u32) -> bool {
    let live = [
        game_anchors::u16_at(ram, 0x8007_B790),
        0,
        game_anchors::u16_at(ram, 0x8007_B794),
        game_anchors::u16_at(ram, 0x8008_40B8),
        game_anchors::u16_at(ram, 0x8008_40BC),
        game_anchors::u16_at(ram, 0x8008_40C0),
        game_anchors::u16_at(ram, 0x8008_9118),
        game_anchors::u16_at(ram, 0x8008_911C),
        game_anchors::u16_at(ram, 0x8008_9120),
    ];
    (0..9u32).filter(|&k| k != 1).all(|k| {
        let rec = ctx + TWEEN_TABLE + k * 4;
        let end = game_anchors::u16_at(ram, rec + 2);
        let mask = if k < 3 { 0xFFF } else { 0xFFFF };
        live[k as usize] & mask == end & mask
    })
}

/// The actor whose framing a capture ahead of the seed pass holds: the seat
/// `ctx[+0x13]` that ran last, when its initiative key reads spent. The action
/// seed zeroes the key (`sh zero,0x16c(s3)` at `0x801E2CDC`) and the round's
/// seeder rolls a new one, so a live key means this actor belongs to the last
/// round, and the prompt's case `9` has framed since (`taiku_topaz_pre_hyper_heal`).
/// A spent key means no framing call ran since `0x5A` armed that actor's shot
/// (`0x801E6428..0x801E6470`).
pub(super) fn prior_framing(
    ram: &[u8],
    ctx: u32,
) -> Option<legaia_engine_capture::battle_drive::PriorFraming> {
    if game_anchors::u8_at(ram, ctx + 6) != 0xFF
        || !PRE_SEED_STATES.contains(&game_anchors::u8_at(ram, ctx + 7))
        || !camera_landed(ram, ctx)
    {
        return None;
    }
    let seat = game_anchors::u8_at(ram, ctx + 0x13);
    let actor = game_anchors::u32_at(ram, ACTOR_TABLE + u32::from(seat.min(7)) * 4);
    if !in_ram(actor) || game_anchors::u16_at(ram, actor + 0x16C) != 0 {
        return None;
    }
    Some(legaia_engine_capture::battle_drive::PriorFraming {
        seat,
        category: game_anchors::u8_at(ram, actor + 0x1DE),
        target: game_anchors::u8_at(ram, actor + 0x1DD),
        style: game_anchors::u8_at(ram, ctx + 0xD),
        depth: game_anchors::u16_at(ram, ctx + 0x6D0),
    })
}

/// Retail's live camera pose and its tween endpoints, in the engine's pose
/// space: the 12-bit angles, the eye translation `0x800840B8/BC/C0`, and the
/// focus `0x80089118/1C/20` un-negated (retail stores it negated, and so do
/// the focus records of the table). The walker steps the low halfwords, so
/// the endpoints are halfwords too.
pub(super) fn retail_cam_tween(
    ram: &[u8],
    ctx: u32,
) -> (
    legaia_engine_camera::projection::BattleCamPose,
    legaia_engine_camera::projection::BattleCamPose,
) {
    use legaia_engine_camera::projection::BattleCamPose;
    let word = |va: u32| game_anchors::u32_at(ram, va) as i32 as f32;
    let live = BattleCamPose {
        pitch: f32::from(game_anchors::u16_at(ram, 0x8007_B790) & 0xFFF),
        yaw: f32::from(game_anchors::u16_at(ram, 0x8007_B792) & 0xFFF),
        tr: [word(0x8008_40B8), word(0x8008_40BC), word(0x8008_40C0)],
        focus: [-word(0x8008_9118), -word(0x8008_911C), -word(0x8008_9120)],
    };
    let end_at = |k: u32| game_anchors::u16_at(ram, ctx + TWEEN_TABLE + k * 4 + 2) as i16;
    let end = BattleCamPose {
        pitch: f32::from(end_at(0) as u16 & 0xFFF),
        yaw: f32::from(end_at(1) as u16 & 0xFFF),
        tr: [3, 4, 5].map(|k| f32::from(end_at(k))),
        focus: [6, 7, 8].map(|k| -f32::from(end_at(k))),
    };
    (live, end)
}

/// The display frames a round prompt's far framing had left on the frame
/// the display showed, while it was still walking; `None` once it has
/// landed, or off the two case-9 surfaces (`ctx[+0x06]` `0x1E` / `0x6E`).
///
/// Case 9 arms its table once (`FUN_801D829C` with `a3 = 0xE`,
/// `0x801D712C`): each component's increment is `ceil(|gap| / 0xE)` per
/// frame, and the walker `FUN_8002149C` adds it once a frame, so a
/// component still short of its endpoint has `gap / inc` frames to go.
/// `gaza2_card_first_menu` reads TR z `6750` on `7800` at `353` a frame
/// (TR y `1327` on `1280` at `19`, pitch landed at `3`): eleven frames of
/// an arm that set out from the case-2 close-up the entry sweep ended on,
/// three to go. The display shows the frame `lag` vsyncs older
/// ([`pass_steps`]), which adds those frames back.
pub(super) fn displayed_menu_glide_left(ram: &[u8], ctx: u32, lag: u16) -> Option<f32> {
    if !matches!(game_anchors::u8_at(ram, ctx + 6), 0x1E | 0x6E) {
        return None;
    }
    let live: [i32; 9] = [
        i32::from(game_anchors::u16_at(ram, 0x8007_B790) as i16),
        0,
        0,
        game_anchors::u32_at(ram, 0x8008_40B8) as i32,
        game_anchors::u32_at(ram, 0x8008_40BC) as i32,
        game_anchors::u32_at(ram, 0x8008_40C0) as i32,
        game_anchors::u32_at(ram, 0x8008_9118) as i32,
        game_anchors::u32_at(ram, 0x8008_911C) as i32,
        game_anchors::u32_at(ram, 0x8008_9120) as i32,
    ];
    // Yaw belongs to the idle orbit and roll is never driven.
    let left = [0u32, 3, 4, 5, 6, 7, 8]
        .into_iter()
        .filter_map(|k| {
            let at = ctx + TWEEN_TABLE + k * 4;
            let inc = i32::from(game_anchors::u16_at(ram, at) as i16).abs();
            let end = game_anchors::u16_at(ram, at + 2) as i16;
            let gap = i32::from(end.wrapping_sub(live[k as usize] as i16)).abs();
            (inc > 0 && gap > 0).then(|| gap as f32 / inc as f32)
        })
        .fold(None, |m: Option<f32>, l| Some(m.map_or(l, |m| m.max(l))))?;
    Some(left + f32::from(lag))
}

/// The tween divisor of the action shots (`nivora_duel_mid_blazing_slash`
/// reads yaw `59` with `589` to go at step `2`: `(589 + 2 * 59) / 59 = 12`).
const SHOT_TWEEN_DIVISOR: i32 = 0xC;

/// How many passes back along the live tween table the displayed frame
/// stands: the display lag's `passes`, but never past the pass that armed
/// the table. Before that the camera was walking the previous framing's
/// table, which the RAM no longer holds.
///
/// The action seed `0x0C` arms case 6 on the yaw counter's `0x800` rung
/// (`0x801E2CF8`, `jal 0x801D5854` at `0x801E2D50`) and then, on the Attack
/// arm, stores state `0x14` and the `0x200` rung (`0x801E2F20`); every SM pass
/// after it drifts the counter in the prologue (`0x801E29E4..0x801E2A24`)
/// before `0x14` re-arms. A capture reading `0x14` with the counter still on
/// `0x200` is the walker pass right after the seed's arm
/// (`super_queue_replace_*`: yaw `3205` walking to `4062`, the `0x800`-rung
/// endpoint, not the `0x200` one `0x14` aims at), so the frame two passes
/// back was the commit's framing, one pass behind the arm.
pub(super) fn tween_passes_back(action_state: u8, yaw_counter: u16, passes: u16) -> u16 {
    if action_state == 0x14 && yaw_counter == 0x200 {
        passes.min(1)
    } else {
        passes
    }
}

/// What a turning shot adds to the live yaw per vsync of frame step
/// (`FUN_801DC0A0` cases `3`, `5`, `0xE`, `0x10`; `0x801DCB18` for `0xE`).
pub(super) fn turning_shot_rate(case: u8) -> Option<i32> {
    match case {
        0x03 => Some(-8),
        0x05 => Some(16),
        0x0E => Some(4),
        0x10 => Some(-16),
        _ => None,
    }
}

/// The Done tail's countdown `ctx[+0x6D8]` and the two values the Done arm
/// `0x50` seeds it with (`li v0,0x3c` / `sh v0,0x2(s7)` at `0x801E5F24..28`,
/// `0x96` when `ctx[+0x15]` is set, `0x801E5F38..3C`).
const DONE_TIMER: u32 = 0x6D8;

/// Case 7's eye height `TR.y` (`li v0,0x500` / `sh v0,0x1a(sp)` at
/// `0x801D65E4..0x801D65E8`).
const CASE7_TR_Y: f32 = 1280.0;
const DONE_TIMER_SEEDS: [u16; 2] = [0x3C, 0x96];

/// Whether a `0x51` capture holds the Done arm's own pass: the arm `0x50`
/// frames the tail (`jal 0x801D5854` at `0x801E5EDC` / `0x801E5EF0`), seeds
/// the countdown and stores `0x51` in the same pass (`0x801E5F5C`), and every
/// `0x51` pass after it steps the countdown down by the frame step unless
/// the HP-bar settle gate `FUN_801E7250` holds it (`0x801E604C`; a live HP
/// `+0x14C` still apart from its bar `+0x172`). A countdown on its seed with
/// every bar settled is therefore the arm's pass, the first that walked the
/// Done table: the pass before walked the strike band's own framing, so the
/// displayed frame stands one pass back along the table and no further
/// ([`tween_passes_back`]'s rule). `koru_pre_*` (Gala's Lightning Storm
/// tail, countdown `60`) reads `image` `.667` two passes back and `.740` one.
pub(super) fn done_arm_pass(ram: &[u8], ctx: u32, action_state: u8) -> bool {
    action_state == 0x51
        && DONE_TIMER_SEEDS.contains(&game_anchors::u16_at(ram, ctx + DONE_TIMER))
        && (0..8u32).all(|slot| {
            let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
            !in_ram(p)
                || game_anchors::u16_at(ram, p + 0x14C) == game_anchors::u16_at(ram, p + 0x172)
        })
}

/// The displayed pose of a Done-arm capture ([`done_arm_pass`]) after an
/// Attack: `one_back` (the pose the Done arm set out from, one pass back along
/// its own table) walked one more pass back along the **case-7** table the
/// strike band left.
///
/// The return state `0x20` arms case 7 on each held pass (`li a1,0x7` at
/// `0x801E56BC`), case 8 instead when the target's anim `+0x1D9` is its
/// knockdown `+0x1F1` or non-zero get-up `+0x1F2`, or a party seat faces a
/// target on anim `7` / `8` (`0x801E5660..0x801E56B4`). Its exit pass stores
/// `0x50` (`0x801E5590`) and jumps to the state tail at `0x801E5F64` without
/// calling `FUN_801D5854`, so that pass re-arms nothing: the walker task
/// `FUN_8002149C` steps the case-7 increments the last held pass armed, and
/// the Done arm re-arms case 8 one pass later. The frame on screen two passes
/// back is therefore the held pass's pose, and the exit pass moved it by the
/// same increment `ceil(G / 0xC)` that pass armed off its gap `G`, which two
/// walks of it narrowed to the gap the Done arm saw:
/// `G - (fs1 + fs2) * ceil(G / 0xC) = |end7 - one_back|`.
///
/// Case 7's endpoints are its base (`0x801D65DC..0x801D6694`): the yaw is case
/// 8's (both reduce to `ctx[+0x6DA] - actor[+0x46] - 0x700`, `0x801D6708` /
/// `0x801D67EC` + `0x801D693C`), `TR = (0, 0x500, prescale(ctx[+0x6D0]))`, the
/// focus the midpoint `(a + b) >> 1` of the actor's and the target's body
/// pairs `+0x3C` / `+0x40` (live `+0x36` for Y), and the pitch `0`: the
/// prologue zeroes the pose (`sh zero,0x10(sp)` at `0x801D58B4`) and the case-7
/// base below a style `2` never writes it. Case 8 on an idle monster then
/// holds the pitch and `TR.y` the exit pass left (`0x801D6BFC`), so the
/// Done-arm RAM reads them on the held pass's table one exit step on, and the
/// walk takes that step back as it does the rest. A retail film from
/// `koru_strip_forced` through Gala's Lightning Storm reads the held pass at
/// pitch `4058` on case 7's table `{5, 0}`, and the exit pass at `4073`; the
/// image child seats a held component whatever the two holds read
/// (`BattleCamera::framing_held_components`).
///
/// The increment the walk recovers is not always unique: `G - 6 * ceil(G / 12)`
/// takes most values twice, `6` units of gap apart, and the walk takes the
/// smaller (`koru_pre_*`: pitch `4061` beside the film's `4058`, depth `5998`
/// beside `5995`; TR x `-113` on the film's own value).
///
/// `None` (the one-pass pose stands) off an Attack, on case 8, on a framing
/// style `2` / `3` and on a monster's pull-in (`ctx[+0x243]`), none of which
/// this walk models. `koru_pre_*` (Gala's art tail, step `3`): the RAM's yaw
/// `3445` walks back to `3496` on case 8 and to `3595` on case 7, depth `6730`
/// to `5998`, focus z `-342` to `-399`; `image` `.735` one pass back and
/// `.904` two. `cort_evolved_card_pre_doomsday` (Gala's ten-hit art on the
/// evolved Cort): depth `6270` to `5922`, `.820` to `.965`.
pub(super) fn done_arm_strike_walk_back(
    ram: &[u8],
    ctx: u32,
    one_back: legaia_engine_camera::projection::BattleCamPose,
    steps: [u8; 2],
) -> Option<legaia_engine_camera::projection::BattleCamPose> {
    let seat = u32::from(game_anchors::u8_at(ram, ctx + 0x13));
    let actor = game_anchors::u32_at(ram, ACTOR_TABLE + seat.min(7) * 4);
    if seat > 7 || !in_ram(actor) || game_anchors::u8_at(ram, actor + 0x1DE) != 3 {
        return None;
    }
    let style = game_anchors::u8_at(ram, ctx + 0xD);
    if style >= 2 || (seat >= 3 && game_anchors::u8_at(ram, ctx + 0x243) != 0) {
        return None;
    }
    let target_slot = u32::from(game_anchors::u8_at(ram, actor + 0x1DD));
    let target = game_anchors::u32_at(ram, ACTOR_TABLE + target_slot.min(7) * 4);
    if target_slot > 7 || !in_ram(target) {
        return None;
    }
    // `0x801E5660..0x801E56B4`: the fork to case 8.
    let anim = game_anchors::u8_at(ram, target + 0x1D9);
    let getup = game_anchors::u8_at(ram, target + 0x1F2);
    if anim == game_anchors::u8_at(ram, target + 0x1F1)
        || (anim == getup && anim != 0)
        || (seat < 3 && matches!(anim, 7 | 8))
    {
        return None;
    }
    let (_, end8) = retail_cam_tween(ram, ctx);
    let body = |p: u32| {
        [
            i32::from(game_anchors::u16_at(ram, p + 0x3C) as i16),
            i32::from(game_anchors::u16_at(ram, p + 0x36) as i16),
            i32::from(game_anchors::u16_at(ram, p + 0x40) as i16),
        ]
    };
    let (a, t) = (body(actor), body(target));
    let mid = |k: usize| ((a[k] + t[k]) >> 1) as f32;
    let depth = i32::from(game_anchors::u16_at(ram, ctx + 0x6D0) as i16);
    let tr_z = legaia_engine_battle_vm::battle_cam_script::prescale_tr_z(depth);
    let (fs1, fs2) = (i32::from(steps[0].max(1)), i32::from(steps[1].max(1)));
    let d = SHOT_TWEEN_DIVISOR;
    if d <= fs1 + fs2 {
        return None;
    }
    // One component: `v` stood `gap` short of `end` after the exit pass.
    let back = |v: f32, gap: f32| -> f32 {
        let g1 = gap.round() as i32;
        if g1 == 0 {
            return v;
        }
        let mut big = g1.abs();
        loop {
            let next = g1.abs() + (fs1 + fs2) * ((big + d - 1) / d);
            if next == big {
                break;
            }
            big = next;
        }
        v - (g1.signum() * fs1 * ((big + d - 1) / d)) as f32
    };
    let wrapped = |gap: f32| (gap + 2048.0).rem_euclid(4096.0) - 2048.0;
    Some(legaia_engine_camera::projection::BattleCamPose {
        pitch: back(one_back.pitch, wrapped(-one_back.pitch)).rem_euclid(4096.0),
        yaw: back(one_back.yaw, wrapped(end8.yaw - one_back.yaw)).rem_euclid(4096.0),
        tr: [
            back(one_back.tr[0], -one_back.tr[0]),
            back(one_back.tr[1], CASE7_TR_Y - one_back.tr[1]),
            back(one_back.tr[2], tr_z - one_back.tr[2]),
        ],
        focus: [
            back(one_back.focus[0], mid(0) - one_back.focus[0]),
            back(one_back.focus[1], mid(1) - one_back.focus[1]),
            back(one_back.focus[2], mid(2) - one_back.focus[2]),
        ],
    })
}

/// The eye depth the case-6 table chased before a `0x32` capture's table was
/// armed, as [`displayed_cam_live`]'s `prior_end` (component 5, the eye
/// translation's Z).
///
/// The summon route arm stores the close-up depth `ctx[+0x6D0] = 0x600`
/// (`0x801E4604`) and state `0x32` on the pass it leaves `0x29`, and every
/// `0x32` pass re-arms case 6 on it. Until that pass case 6 framed the cast on
/// the depth the action seed's `FUN_801F0348` stored (`0x801E2D2C`), so a
/// capture on the route's first pass holds a table one pass old: the frame on
/// screen two passes back was still closing on the seed's depth.
/// `flute_spikefish_midcast` holds TR z `4279` walking to `prescale(0x600)` at
/// `183` a vsync; two passes back along that table read `5083`, beyond the
/// `0x29` endpoint `prescale(0xC00) = 4915` the camera climbed toward, and its
/// frame matches at about `4600`, one pass back and one `0x29` step short.
pub(super) fn summon_route_prior_end(ram: &[u8], ctx: u32, action_state: u8) -> [Option<i32>; 9] {
    let mut prior = [None; 9];
    if action_state != 0x32 {
        return prior;
    }
    let seat = game_anchors::u8_at(ram, ctx + 0x13);
    let actor = game_anchors::u32_at(ram, ACTOR_TABLE + u32::from(seat.min(7)) * 4);
    if !in_ram(actor) {
        return prior;
    }
    let target = game_anchors::u8_at(ram, actor + 0x1DD);
    let depth = legaia_engine_battle_vm::battle_formulas::camera_height_for_frame(
        seat,
        target,
        legaia_engine_battle_vm::battle_formulas::RETAIL_MONSTER_SLOT_BASE,
        |slot| {
            let rec =
                game_anchors::u32_at(ram, 0x801C_9348 + u32::from(slot.saturating_sub(3)) * 4);
            if in_ram(rec) {
                game_anchors::u8_at(ram, rec + 0x1F)
            } else {
                0
            }
        },
    );
    prior[5] =
        Some(legaia_engine_battle_vm::battle_cam_script::prescale_tr_z(i32::from(depth)) as i32);
    prior
}

/// Where case 7 held the eye before its pull-in, as [`displayed_cam_live`]'s
/// `prior_end` (the translation's Y and Z).
///
/// The pull-in (`0x801D6780`: `TR.y += 0x40`, `TR.z = 3z/5`) is gated for a
/// monster on `ctx[+0x243]`, which the strike clip's commit stores after that
/// pass's arm, so a capture one pass past the commit holds the pull-in's first
/// table: the pass before it stood on the depth the shot had already landed on.
/// `battle_map01_frog_strike` reads TR z `4587` walking to `2948` by `164`; one
/// pass back is `4915`, `prescale(ctx[+0x6D0])`, and the frame on screen is
/// still there.
pub(super) fn strike_pull_in_prior_end(ram: &[u8], ctx: u32) -> [Option<i32>; 9] {
    let mut prior = [None; 9];
    let seat = game_anchors::u8_at(ram, ctx + 0x13);
    if game_anchors::u8_at(ram, ctx + 7) != STRIKE_LOOP_STATE
        || seat < 3
        || game_anchors::u8_at(ram, ctx + 0x243) == 0
    {
        return prior;
    }
    let depth = i32::from(game_anchors::u16_at(ram, ctx + 0x6D0) as i16);
    let far = legaia_engine_battle_vm::battle_cam_script::prescale_tr_z(depth) as i32;
    let near = legaia_engine_battle_vm::battle_cam_script::prescale_tr_z(depth * 3 / 5) as i32;
    let end_z = i32::from(game_anchors::u16_at(ram, ctx + TWEEN_TABLE + 5 * 4 + 2) as i16);
    if (end_z - near).abs() <= 2 {
        prior[5] = Some(far);
        let end_y = i32::from(game_anchors::u16_at(ram, ctx + TWEEN_TABLE + 4 * 4 + 2) as i16);
        prior[4] = Some(end_y - 0x40);
    }
    prior
}

/// Where case 6 aimed on the pass before the strike loop's first, as
/// [`displayed_cam_live_steps`]'s `prior_end`, when that first pass is the one
/// before the RAM's. The walk's arm chased the walker, and the displayed frame
/// is that pass.
///
/// A monster's walk ends in `0x19`. The next pass's anim tick commits the
/// arrival's idle stage at cursor `0` and its walker step still runs on case
/// 6's table, then its SM pass arms case 7 for the first time
/// (`jal 0x801D5854` with `a1 = 7` at `0x801E36E8`). So a capture holding
/// that stage on the commit's next cursor (clip-record ring slots `0` and `1`
/// alike, slot `2` the walk) walked one pass on case 7's table and no more:
/// the frame on screen, two passes back, was drawn before case 7 was armed,
/// on the pose case 6's last table carried on from.
///
/// Case 6 arms ahead of the pass's own re-face, so its yaw subtracts the
/// heading the pass before stored (position-ring slot `4`'s bearing onto the
/// target's live pair, [`walk_heading`]'s rule) from the counter as the walk's
/// last pass left it, with the style byte's half turn; its focus is the live
/// pair of that pass (slot `2`), negated (`0x801D64C4`).
/// `battle_map01_gomboo_strike` reads yaw `3927` under endpoint `2523`: one
/// pass back is `302`, case 6 aimed at `326`, and a sweep of the child reads
/// `.965` at yaw `280..300` with the focus at `(232, 561)` (`.563` walked back
/// along case 7's table to yaw `896`).
///
/// `None` elsewhere: on any later pass the two passes back stay on case 7's
/// table, which [`displayed_cam_live_steps`] walks by its own re-arm law.
pub(super) fn walk_arm_prior_end(ram: &[u8], ctx: u32) -> Option<[Option<i32>; 9]> {
    if game_anchors::u8_at(ram, ctx + 7) != STRIKE_LOOP_STATE
        || game_anchors::u8_at(ram, ctx + 0x15) != 1
    {
        return None;
    }
    let seat = u32::from(game_anchors::u8_at(ram, ctx + 0x13));
    let actor = game_anchors::u32_at(ram, ACTOR_TABLE + seat.min(7) * 4);
    if !(3..=7).contains(&seat) || !in_ram(actor) || game_anchors::u8_at(ram, actor + 0x1DE) != 3 {
        return None;
    }
    let record = |k: u32| game_anchors::u32_at(ram, actor + RING_RECORD + k * 4);
    let on_idle_stage = record(0) == record(1)
        && record(1) != record(2)
        && game_anchors::u16_at(ram, actor + CURSOR_RING + 2) == 0;
    // A walk the `+0x76` lock keeps running has no such commit; the arrival is
    // the walker's nearest slot instead ([`walk_heading`]), and that alone
    // also holds a pass later (`battle_korb2_warhalf_strike`), checked below.
    let on_walk =
        game_anchors::u8_at(ram, actor + 0x1D9) == WALK_CLIP && walk_heading(ram, ctx).is_some();
    if !on_idle_stage && !on_walk {
        return None;
    }
    let target_slot = u32::from(game_anchors::u8_at(ram, actor + 0x1DD));
    let target = game_anchors::u32_at(ram, ACTOR_TABLE + target_slot.min(7) * 4);
    if target_slot > 7 || !in_ram(target) {
        return None;
    }
    let ring = |a: u32, k: u32| {
        let at = a + POSITION_RING + k * 8;
        [
            i32::from(game_anchors::u32_at(ram, at) as i32 as i16),
            i32::from(game_anchors::u32_at(ram, at + 4) as i32 as i16),
        ]
    };
    let (a, t) = (ring(actor, 4), ring(target, 4));
    let heading = (f64::from(t[0] - a[0]).atan2(f64::from(t[1] - a[1])) * 4096.0
        / std::f64::consts::TAU)
        .round() as i32;
    // The counter climbs ahead of the arm, so the walk's last arm read it
    // short of the RAM's by this pass's rung and the one before.
    let rungs: i32 = pass_steps(ram)
        .iter()
        .map(|&s| {
            i32::from(legaia_engine_battle_vm::battle_cam_script::action_yaw_pass_increment(s))
        })
        .sum();
    let counter = i32::from(game_anchors::u16_at(ram, ctx + 0x6DA)) - rungs;
    let half_turn = if game_anchors::u8_at(ram, ctx + 0xD) & 1 != 0 {
        0x800
    } else {
        0
    };
    let aim = (counter - heading + half_turn).rem_euclid(4096);
    if !on_idle_stage {
        // One pass back along case 7's table, the camera still stands on case
        // 6's aim if that pass was the first arm; had it armed earlier, that
        // pass would already be a table step or more off the aim.
        // `battle_nilboa_warfish_strike` stands `2` off with a step of `300`,
        // `battle_korb2_warhalf_strike` about `566` off with one of `309`.
        let inc = i32::from(game_anchors::u16_at(ram, ctx + TWEEN_TABLE + 4) as i16);
        let end = i32::from(game_anchors::u16_at(ram, ctx + TWEEN_TABLE + 6) as i16);
        let live = i32::from(game_anchors::u16_at(ram, 0x8007_B792) as i16);
        let step = i32::from(pass_steps(ram)[0].max(1)) * inc.abs();
        let one_back = live - (end - live).signum() * step;
        if ((one_back - aim + 2048).rem_euclid(4096) - 2048).abs() >= step {
            return None;
        }
    }
    let mut prior = [None; 9];
    prior[1] = Some(aim);
    let walker = ring(actor, 2);
    prior[6] = Some(-walker[0]);
    prior[8] = Some(-walker[1]);
    Some(prior)
}

/// The live pose the **displayed** frame was drawn at: retail's live camera
/// walked back `passes` passes along its tween table.
///
/// The walker `FUN_8002149C` steps each component's low halfword toward its
/// endpoint by `increment * frame_step` a pass, and every pass re-arms the
/// table with `ceil((gap + frame_step * increment) / d)` (`d = 0xC` for the
/// action shots: `nivora_duel_mid_blazing_slash` reads yaw `59` with `589`
/// to go, `(589 + 2 * 59) / 59 = 12`). So the pass before stood
/// `frame_step * increment` further back, and each earlier pass a step of
/// `gap / (d - frame_step)` further again. The display shows the frame two
/// passes older than the RAM ([`display_lag_vsyncs`]), so a camera still
/// closing on its framing - the ease is fast at its start - draws that frame
/// well short of the RAM's pose: a far backdrop object swings across the
/// screen while the framed bodies hold still.
///
/// `d` is read off the table's largest increment, `0xC` when every
/// component has landed.
///
/// `prior_end` names, per component, the endpoint the walker chased before
/// the table the RAM holds was armed, where that arm moved it. A component
/// walked back past it was on the earlier table by then, and walks back from
/// there along that table instead ([`summon_route_prior_end`]).
#[cfg(test)]
pub(super) fn displayed_cam_live(
    ram: &[u8],
    ctx: u32,
    passes: u16,
    frame_step: u8,
    prior_end: [Option<i32>; 9],
) -> legaia_engine_camera::projection::BattleCamPose {
    displayed_cam_live_steps(
        ram,
        ctx,
        &vec![frame_step; usize::from(passes)],
        false,
        prior_end,
        [0; 9],
        false,
    )
}

/// [`displayed_cam_live`] with each pass walked back on its own frame step,
/// newest first ([`pass_steps`]): the pass the RAM holds moved its
/// components `step * increment`, and each pass before it the re-arm's share
/// of the wider gap on that pass's own step.
///
/// `shot_divisor` takes the re-arm divisor as the action shots' `0xC`
/// wherever every moving component admits it, which a small increment pins
/// only loosely off its own gap (yaw `2` with `5` to go at step `4` admits
/// `7..=12`; `nivora_duel_pre_megaton_press` reads `0.922` on `7` and `0.954`
/// on `12`). It goes with steps the floor confirmed
/// ([`older_table_step`]); the history-max steps keep the largest
/// increment's own estimate, which they were measured with.
///
/// `end_shift` is how far each component's endpoint stood from the RAM's on
/// the pass before it, in the walker's raw space ([`walker_end_shift`]): a
/// framing re-armed on a body still walking chased an endpoint that moved
/// with it. Pass `n` back walks against `n` such shifts.
///
/// `armed_last_pass` says the RAM's table was first armed on the pass before
/// the RAM's, so every pass further back ran on the earlier table, whatever
/// its distance from `prior_end` ([`walk_arm_prior_end`]).
pub(super) fn displayed_cam_live_steps(
    ram: &[u8],
    ctx: u32,
    steps: &[u8],
    shot_divisor: bool,
    prior_end: [Option<i32>; 9],
    end_shift: [i32; 9],
    armed_last_pass: bool,
) -> legaia_engine_camera::projection::BattleCamPose {
    let (live, _) = retail_cam_tween(ram, ctx);
    let passes = steps.len();
    let step_of = |n: usize| i32::from(steps.get(n).copied().unwrap_or(1).max(1));
    let fs = step_of(0);
    // The raw words the walker steps: pitch / yaw (12-bit), the eye
    // translation, the focus (stored negated).
    let raw_live: [i32; 9] = [
        i32::from(game_anchors::u16_at(ram, 0x8007_B790) as i16),
        i32::from(game_anchors::u16_at(ram, 0x8007_B792) as i16),
        0,
        game_anchors::u32_at(ram, 0x8008_40B8) as i32,
        game_anchors::u32_at(ram, 0x8008_40BC) as i32,
        game_anchors::u32_at(ram, 0x8008_40C0) as i32,
        game_anchors::u32_at(ram, 0x8008_9118) as i32,
        game_anchors::u32_at(ram, 0x8008_911C) as i32,
        game_anchors::u32_at(ram, 0x8008_9120) as i32,
    ];
    let rec = |k: u32| {
        let at = ctx + TWEEN_TABLE + k * 4;
        (
            i32::from(game_anchors::u16_at(ram, at) as i16),
            i32::from(game_anchors::u16_at(ram, at + 2) as i16),
        )
    };
    // The gap to the endpoint in the walker's halfword space.
    let gap = |k: u32| {
        let (_, end) = rec(k);
        i32::from((end as i16).wrapping_sub(raw_live[k as usize] as i16))
    };
    // The divisor the walker re-arms by: `inc = ceil(gap_before / d)` with
    // `gap_before = gap + fs * inc`, so each moving component admits
    // `ceil(gap_before / inc) ..= ceil(gap_before / (inc - 1)) - 1`.
    let admits = |d: i32| {
        (0..9u32).filter(|&k| k != 2).all(|k| {
            let inc = rec(k).0.abs();
            let g = gap(k).abs();
            if inc == 0 || g == 0 {
                return true;
            }
            let before = g + fs * inc;
            (inc - 1) * d < before && before <= inc * d
        })
    };
    let d = if shot_divisor && admits(SHOT_TWEEN_DIVISOR) {
        SHOT_TWEEN_DIVISOR
    } else {
        (0..9u32)
            .filter(|&k| k != 2)
            .map(|k| (rec(k).0.abs(), gap(k).abs()))
            .max()
            .filter(|&(inc, _)| inc > 0)
            .map_or(SHOT_TWEEN_DIVISOR, |(inc, g)| {
                ((g + fs * inc + inc / 2) / inc).clamp(fs + 1, 0x40)
            })
    };
    let mut back = [0i32; 9];
    for k in (0..9u32).filter(|&k| k != 2) {
        let (inc, _) = rec(k);
        let g = gap(k);
        if passes == 0 || inc == 0 || g == 0 {
            continue;
        }
        let dir = g.signum();
        // One pass back is the table's own increment...
        let mut moved = fs * inc.abs();
        // ...and each pass before it the re-arm's share of the wider gap.
        // Once a pass back would carry the component past the endpoint the
        // earlier table chased, that pass ran on the earlier table: the
        // walker closed on that endpoint and never stood beyond it, so the
        // pass before it stood the earlier re-arm's step short of it.
        let mut prior = None;
        for n in 1..passes {
            let fs_n = step_of(n).min(d - 1);
            let pos = raw_live[k as usize] - dir * moved;
            // A pass back on the earlier table steps away from its endpoint.
            let earlier = |p: i32| {
                let to = p - pos;
                to.signum() * dir * fs_n * ((to.abs() + (d - fs_n) - 1) / (d - fs_n))
            };
            if let Some(p) = prior {
                moved += earlier(p);
                continue;
            }
            if armed_last_pass && let Some(p) = prior_end[k as usize] {
                // The yaw is 12-bit: the endpoint nearest this pass's.
                let p = if k == 1 {
                    pos + ((p - pos + 2048).rem_euclid(4096) - 2048)
                } else {
                    p
                };
                prior = Some(p);
                moved += earlier(p);
                continue;
            }
            // The gap pass `n` armed, to the endpoint it chased.
            let wide = (g.abs() + moved + dir * end_shift[k as usize] * n as i32).max(0);
            let step = (wide + (d - fs_n) - 1) / (d - fs_n);
            // `p` behind `pos` (further from the endpoint), and the step back
            // on this table would land behind `p` too.
            if let Some(p) = prior_end[k as usize]
                && (pos - p) * dir >= 0
                && (pos - dir * fs_n * step - p) * dir < 0
            {
                prior = Some(p);
                moved += earlier(p);
                continue;
            }
            moved += fs_n * step;
        }
        back[k as usize] = dir * moved;
    }
    let ang = |v: f32, k: usize| (v - back[k] as f32).rem_euclid(4096.0);
    legaia_engine_camera::projection::BattleCamPose {
        pitch: ang(live.pitch, 0),
        yaw: ang(live.yaw, 1),
        tr: [
            live.tr[0] - back[3] as f32,
            live.tr[1] - back[4] as f32,
            live.tr[2] - back[5] as f32,
        ],
        // Engine focus is the stored word negated.
        focus: [
            live.focus[0] + back[6] as f32,
            live.focus[1] + back[7] as f32,
            live.focus[2] + back[8] as f32,
        ],
    }
}

/// The pose the near-camera ghost pass read for the displayed frame: one pass
/// behind `shown`, the pose that frame stands on
/// ([`World::tick_battle_camera_ghost`](legaia_engine_core::world::World)).
/// `further` is the same walk with one pass more.
///
/// Case 7's yaw sits `0x700` past its endpoint on the first pass that armed it
/// (`0x801D6700`), so a walk back beyond that is a pass of the earlier framing
/// the re-arm law does not describe, and the earlier framing had all but
/// landed: `battle_korb2_aluru_strike` stands on yaw `2188` under an endpoint
/// of `395`, one unit from where case 6 was resting. Such a pass holds `shown`.
pub(super) fn displayed_ghost_pose(
    ram: &[u8],
    ctx: u32,
    action_state: u8,
    shown: legaia_engine_camera::projection::BattleCamPose,
    further: legaia_engine_camera::projection::BattleCamPose,
) -> legaia_engine_camera::projection::BattleCamPose {
    const CASE_7_UNWRAP: f32 = 0x700 as f32;
    if action_state == STRIKE_LOOP_STATE {
        let (live, end) = retail_cam_tween(ram, ctx);
        let dir = (end.yaw - live.yaw + 2048.0).rem_euclid(4096.0) - 2048.0;
        // Round the way the walk goes back, away from the endpoint.
        let moved = if dir < 0.0 {
            (further.yaw - end.yaw).rem_euclid(4096.0)
        } else {
            (end.yaw - further.yaw).rem_euclid(4096.0)
        };
        if moved > CASE_7_UNWRAP {
            return shown;
        }
    }
    further
}

/// The pose the displayed frame of a walking round prompt stands on, off
/// retail's own table: the arm's table is linear, so `lag` frames back is the
/// live word less `lag` increments, on each component still short of its
/// endpoint ([`displayed_menu_glide_left`]). A component that has landed with
/// an increment still in the table reads `NaN`: the table no longer says when
/// it landed, so the engine's arm keeps it (pitch `3` a frame reaches `32`
/// well ahead of the translation). The yaw is the orbit's.
pub(super) fn displayed_menu_glide_pose(
    ram: &[u8],
    ctx: u32,
    lag: u16,
) -> Option<legaia_engine_camera::projection::BattleCamPose> {
    displayed_menu_glide_left(ram, ctx, lag)?;
    let (live, end) = retail_cam_tween(ram, ctx);
    let inc = |k: u32| f32::from(game_anchors::u16_at(ram, ctx + TWEEN_TABLE + k * 4) as i16).abs();
    let back = |l: f32, e: f32, k: u32| {
        let gap = e - l;
        if gap == 0.0 {
            if inc(k) == 0.0 { l } else { f32::NAN }
        } else {
            l - gap.signum() * inc(k) * f32::from(lag)
        }
    };
    Some(legaia_engine_camera::projection::BattleCamPose {
        pitch: back(live.pitch, end.pitch, 0),
        yaw: live.yaw,
        tr: [3u32, 4, 5].map(|k| {
            let i = (k - 3) as usize;
            back(live.tr[i], end.tr[i], k)
        }),
        focus: [6u32, 7, 8].map(|k| {
            let i = (k - 6) as usize;
            back(live.focus[i], end.focus[i], k)
        }),
    })
}

/// The live pose the displayed frame was drawn at under a **module shot**:
/// retail's live camera walked back along a tween table built once.
///
/// Whether retail's camera is still walking a table on its angles or focus
/// (`0x8007B790` / `0x8007B792`, `0x80089118..0x80089120` off their tween
/// endpoints). A module's drifts move TR alone (PROT 0946's arms add to TR
/// z after their shot has landed), so the translation is left out: a table
/// whose angles and focus are home has finished, whatever a drift did to
/// the eye since.
pub(super) fn module_shot_walking(ram: &[u8], ctx: u32) -> bool {
    let live = [
        (0u32, game_anchors::u16_at(ram, 0x8007_B790), 0xFFFu16),
        (1, game_anchors::u16_at(ram, 0x8007_B792), 0xFFF),
        (6, game_anchors::u16_at(ram, 0x8008_9118), 0xFFFF),
        (7, game_anchors::u16_at(ram, 0x8008_911C), 0xFFFF),
        (8, game_anchors::u16_at(ram, 0x8008_9120), 0xFFFF),
    ];
    live.iter().any(|&(k, v, mask)| {
        let end = game_anchors::u16_at(ram, ctx + TWEEN_TABLE + k * 4 + 2);
        v & mask != end & mask
    })
}

/// Whether a magic-band capture holds the animation chain's **terminator
/// pass**: the `0x2A` pass that meets the queue's `0xFF` stores `0x2B` and
/// calls neither `FUN_801DC0A0` nor `FUN_801D5854` (`0x801E4774..0x801E479C`),
/// so it re-arms no framing while the walker `FUN_8002149C` still steps the
/// table the pass before armed. Both passes of the display lag therefore
/// moved `increment * step` on the one table the RAM holds, not the widening
/// share [`displayed_cam_live_steps`] walks for a table re-armed every pass.
///
/// The RAM says which pass it is through its own table: the largest moving
/// increment `inc` is not what a re-arm on the RAM's pass would have stored
/// (`ceil((gap + step * inc) / 0xC) != inc`), and is what one stored a pass
/// earlier (`ceil((gap + 2 * step * inc) / 0xC) == inc`).
/// `battle_garmel_zenoir_fire_claw_mid_cast` holds yaw `3311` on increment
/// `12` with `70` to go at step `3`: a re-arm there stores `9`, one a pass
/// back stores `12`. Walked back on the table the frame draws at yaw `3239`
/// (`image` `.995`); the re-arm law's `3209` reads `.797`.
pub(super) fn chain_terminator_pass(ram: &[u8], ctx: u32, action_state: u8, step: u8) -> bool {
    if action_state != 0x2B {
        return false;
    }
    let raw_live = [
        i32::from(game_anchors::u16_at(ram, 0x8007_B790) as i16),
        i32::from(game_anchors::u16_at(ram, 0x8007_B792) as i16),
        0,
        game_anchors::u32_at(ram, 0x8008_40B8) as i32,
        game_anchors::u32_at(ram, 0x8008_40BC) as i32,
        game_anchors::u32_at(ram, 0x8008_40C0) as i32,
        game_anchors::u32_at(ram, 0x8008_9118) as i32,
        game_anchors::u32_at(ram, 0x8008_911C) as i32,
        game_anchors::u32_at(ram, 0x8008_9120) as i32,
    ];
    let Some((inc, gap)) = (0..9u32)
        .filter(|&k| k != 2)
        .map(|k| {
            let at = ctx + TWEEN_TABLE + k * 4;
            let inc = i32::from(game_anchors::u16_at(ram, at) as i16).abs();
            let end = game_anchors::u16_at(ram, at + 2) as i16;
            (
                inc,
                i32::from(end.wrapping_sub(raw_live[k as usize] as i16)).abs(),
            )
        })
        .filter(|&(inc, gap)| inc > 0 && gap > 0)
        .max_by_key(|&(inc, _)| inc)
    else {
        return false;
    };
    let fs = i32::from(step.max(1));
    let rearm = |before: i32| (before + SHOT_TWEEN_DIVISOR - 1) / SHOT_TWEEN_DIVISOR;
    rearm(gap + fs * inc) != inc && rearm(gap + 2 * fs * inc) == inc
}

/// The tween length of the module shot a `0x70` capture's table holds, where
/// a module arm built one shot of a known length: PROT 0962's charge bodies
/// (`0xA2..=0xA4`) waiting in arm 3, whose table is the wind-up shot arm 2
/// built ([`legaia_engine_cast::cast_module_camera::WIND_UP_SHOT_FRAMES`]);
/// arm 3 only drifts it until its own pass frames through case 8. `None`
/// elsewhere: the walk back is not bounded.
///
/// `koru_blade_breath_mid_cast` holds that table two frame-step units in
/// (yaw `99` a unit with `979` to go, TR z `402` with `4009`): one pass at
/// step `2`. Walked back two passes (yaw `1970`) its frame read `image`
/// `.823`; at the shot's origin (yaw `2168`, where the doc's walk-in film
/// swings the framing) `.943`.
pub(super) fn module_shot_frames(ram: &[u8], ctx: u32) -> Option<i32> {
    if game_anchors::u8_at(ram, ctx + 7) != 0x70 || game_anchors::u8_at(ram, ctx + 0x279) != 3 {
        return None;
    }
    let seat = game_anchors::u8_at(ram, ctx + 0x13);
    let actor = game_anchors::u32_at(ram, ACTOR_TABLE + u32::from(seat.min(7)) * 4);
    if !in_ram(actor) {
        return None;
    }
    let action = game_anchors::u8_at(ram, actor + 0x1DF);
    (0xA2..=0xA4).contains(&action).then_some(i32::from(
        legaia_engine_cast::cast_module_camera::WIND_UP_SHOT_FRAMES,
    ))
}

/// A slot-B arm calls the builder `FUN_801D829C` on the pass it runs and
/// never again, so the table the RAM holds is the one every pass since has
/// walked: each pass moved a component `increment * frame_step` toward its
/// endpoint (`FUN_8002149C`), and the pass before stood exactly that much
/// further back - not the widening share [`displayed_cam_live_steps`] walks
/// for a table re-armed every pass. A component already on its endpoint is
/// left there: when it landed is not in the table.
///
/// `steps` are the passes to walk back, each on its own frame step, newest
/// first.
pub(super) fn displayed_module_cam_live(
    ram: &[u8],
    ctx: u32,
    steps: &[u8],
) -> legaia_engine_camera::projection::BattleCamPose {
    let (live, _) = retail_cam_tween(ram, ctx);
    let mut vsyncs: i32 = steps.iter().map(|&s| i32::from(s.max(1))).sum();
    let raw_live: [i32; 9] = [
        i32::from(game_anchors::u16_at(ram, 0x8007_B790) as i16),
        i32::from(game_anchors::u16_at(ram, 0x8007_B792) as i16),
        0,
        game_anchors::u32_at(ram, 0x8008_40B8) as i32,
        game_anchors::u32_at(ram, 0x8008_40BC) as i32,
        game_anchors::u32_at(ram, 0x8008_40C0) as i32,
        game_anchors::u32_at(ram, 0x8008_9118) as i32,
        game_anchors::u32_at(ram, 0x8008_911C) as i32,
        game_anchors::u32_at(ram, 0x8008_9120) as i32,
    ];
    let table = |k: u32| {
        let at = ctx + TWEEN_TABLE + k * 4;
        let inc = i32::from(game_anchors::u16_at(ram, at) as i16).abs();
        let end = game_anchors::u16_at(ram, at + 2) as i16;
        (
            inc,
            i32::from(end.wrapping_sub(raw_live[k as usize] as i16)),
        )
    };
    // A shot whose length is known has walked `frames - ceil(gap / inc)`
    // frame-step units since its arm built it (the builder's `inc =
    // ceil(gap / frames)`, read on the component that moves furthest a
    // unit); the frame before the arm stood on the framing the shot set out
    // from, which is the table's own origin, so the walk back stops there.
    if let Some(frames) = module_shot_frames(ram, ctx)
        && let Some((inc, gap)) = (0..9u32)
            .filter(|&k| k != 2)
            .map(table)
            .filter(|&(inc, gap)| inc > 0 && gap != 0)
            .max_by_key(|&(inc, _)| inc)
    {
        let left = (gap.abs() + inc - 1) / inc;
        vsyncs = vsyncs.min((frames - left).max(0));
    }
    let mut back = [0i32; 9];
    for k in (0..9u32).filter(|&k| k != 2) {
        let (inc, gap) = table(k);
        if inc == 0 || gap == 0 {
            continue;
        }
        back[k as usize] = gap.signum() * inc * vsyncs;
    }
    let ang = |v: f32, k: usize| (v - back[k] as f32).rem_euclid(4096.0);
    legaia_engine_camera::projection::BattleCamPose {
        pitch: ang(live.pitch, 0),
        yaw: ang(live.yaw, 1),
        tr: [
            live.tr[0] - back[3] as f32,
            live.tr[1] - back[4] as f32,
            live.tr[2] - back[5] as f32,
        ],
        focus: [
            live.focus[0] + back[6] as f32,
            live.focus[1] + back[7] as f32,
            live.focus[2] + back[8] as f32,
        ],
    }
}

/// The strike loop's framing case (`jal 0x801D5854` with `a1 = 7` at
/// `0x801E36E8`), armed on every `0x1E` pass.
const STRIKE_LOOP_STATE: u8 = 0x1E;
/// A monster's walk clip `+0x1D9` (the tag-`1` entry, `ActionSteer::walk_clock`).
const WALK_CLIP: u8 = 1;
/// The anim-cursor ring (`+0x17A`, one halfword a pass, slot `0` live).
const CURSOR_RING: u32 = 0x17A;
/// Case 7's focus endpoint off the midpoint the RAM's arm read, within which
/// the walker's body pair is taken as the one the framing read.
const STRIKE_FOCUS_FIT: i32 = 4;

/// How far the strike loop's focus endpoint stood, on the pass before the
/// RAM's, from the RAM's own - [`displayed_cam_live_steps`]'s `end_shift`,
/// in the walker's raw space - while the acting seat still walks.
///
/// State `0x1E` re-arms case 7 every pass, its focus the midpoint of the
/// attacker's and the target's body pairs (`post_action::recover_framing`).
/// A pass frames the body pair the previous pass's decode left
/// ([`super::framing_body`]), so the RAM's arm read the attacker on the
/// position ring's slot `1` with the pose at the cursor ring's slot `1`,
/// and the pass before it slot `2`. A monster captured still on its walk
/// clip moves between the two, and the endpoint that pass chased stood half
/// that move from the RAM's: `battle_retona_king_frog_strike`'s Frog walked
/// `32` units of z between them, its focus endpoint `16` off.
///
/// The yaw endpoint moves too (the counter's drift, the heading the walker
/// re-faced onto), but only by a few units, and is left on the RAM's: the
/// image child tests its yaw on the gate tick's framing, which is the RAM's.
///
/// `None` off the strike loop, off a monster's walk clip, on a seat that has not moved over those
/// slots, or when the RAM's focus endpoint is not the midpoint of the body
/// pair slot `1` names and the target's (another framing owns the table).
pub(super) fn walker_end_shift(ram: &[u8], ctx: u32) -> Option<[i32; 9]> {
    if game_anchors::u8_at(ram, ctx + 7) != STRIKE_LOOP_STATE {
        return None;
    }
    let seat = u32::from(game_anchors::u8_at(ram, ctx + 0x13));
    let actor = game_anchors::u32_at(ram, ACTOR_TABLE + seat.min(7) * 4);
    // A monster on its walk clip: a swing's own root motion moves the body
    // pair too, but the frame of a seat on its strike clip is drawn from
    // the replay's pose (`battle_map03_grude_strike` read `.982` to `.980`
    // walked back against it).
    if !(3..=7).contains(&seat)
        || !in_ram(actor)
        || game_anchors::u8_at(ram, actor + 0x1D9) != WALK_CLIP
    {
        return None;
    }
    let target_slot = u32::from(game_anchors::u8_at(ram, actor + 0x1DD));
    let target = game_anchors::u32_at(ram, ACTOR_TABLE + target_slot.min(7) * 4);
    if target_slot > 7 || !in_ram(target) {
        return None;
    }
    let ring = |k: u32| {
        let at = actor + POSITION_RING + k * 8;
        [
            game_anchors::u32_at(ram, at) as i32 as i16 as i32,
            game_anchors::u32_at(ram, at + 4) as i32 as i16 as i32,
        ]
    };
    if ring(1) == ring(2) {
        return None;
    }
    let live = [
        i32::from(game_anchors::i16_at(ram, actor + 0x34)),
        i32::from(game_anchors::i16_at(ram, actor + 0x38)),
    ];
    // The body pair the decode of cursor-ring slot `k` left, on the live
    // pair position-ring slot `k` holds.
    let body = |k: u32| {
        let cursor = game_anchors::u16_at(ram, actor + CURSOR_RING + k * 2);
        let b = super::framing_body::body_pair_at(ram, actor, cursor)?;
        let p = ring(k);
        Some([
            i32::from(b[0]) - live[0] + p[0],
            i32::from(b[1]) - live[1] + p[1],
        ])
    };
    let target_body = [
        i32::from(game_anchors::i16_at(ram, target + 0x3C)),
        i32::from(game_anchors::i16_at(ram, target + 0x40)),
    ];
    let (b1, b2) = (body(1)?, body(2)?);
    // The focus records hold the midpoint negated.
    let end = |k: u32| i32::from(game_anchors::u16_at(ram, ctx + TWEEN_TABLE + k * 4 + 2) as i16);
    let fits = [(6u32, 0usize), (8, 1)]
        .iter()
        .all(|&(k, i)| (-end(k) - (b1[i] + target_body[i]) / 2).abs() <= STRIKE_FOCUS_FIT);
    if !fits {
        return None;
    }
    let mut shift = [0i32; 9];
    shift[6] = -((b2[0] - b1[0]) / 2);
    shift[8] = -((b2[1] - b1[1]) / 2);
    Some(shift)
}

/// How far the focus endpoint stood, on each pass before the RAM's, from the
/// RAM's own - [`displayed_cam_live_steps`]'s `end_shift` - on a monster's
/// arrival pass.
///
/// The walk runs in `0x19`, which re-arms case 6 on every pass with the focus
/// on the walker's live pair (`0x801D64C4`) ahead of the pass's anim tick, so
/// the arm of pass `k` back read position-ring slot `k + 1`. The pass that
/// finds the walker in range arms case 6 one last time and moves on to the
/// strike loop, whose first case-7 arm comes a pass later; its anim tick
/// commits the arrival's idle stage. A capture on that pass - the clip-record
/// ring's slot `0` on the stage, slots `1` and `2` on the walk, the cursor
/// ring's slot `0` on the commit's `0` and no swing staged (strike cursor
/// `ctx[+0x15]` still `0`) - holds case 6's table on slot `1`'s pair
/// (`battle_map02_lip_kid_strike`: endpoint `(83, 224)`), and every pass
/// before chased the walker a ring stride further back. The walk's stride is
/// even, so pass `n` back stood `n` strides off.
///
/// Walked back against the RAM's endpoint alone, the displayed frame stands
/// short of the camera that was chasing the walk: `battle_map02_lip_kid_strike`
/// draws focus `(255, 687)` and reads `image` `.890`; on the walk's endpoints
/// it draws `(243, 657)` and reads `.980`.
///
/// `None` off that pass, on a party seat, or when the RAM's focus endpoint is
/// not slot `1`'s pair.
/// [`arrival_end_shift`] where the older ordering table's floor confirms the
/// frame step the history names for the RAM's pass
/// ([`super::camera_pool::older_table_step`]). The shift walks the passes
/// back on that step, and a floor that fits another step says the passes
/// back did not run as the history reads them: `battle_noaru_songi_strike`
/// (history `3`, floor `4`) reads `image` `.966` on the RAM's endpoint and
/// `.945` on the walker's, `battle_cave01_theeder_strike` (history `2`)
/// `.977` and `.935` with the floor's step taken instead. Without a floor
/// fit the history stands.
pub(super) fn arrival_end_shift_confirmed(ram: &[u8], ctx: u32) -> Option<[i32; 9]> {
    let shift = arrival_end_shift(ram, ctx)?;
    match super::camera_pool::older_table_step(ram, ctx) {
        Some(s) if s != frame_step(ram) => None,
        _ => Some(shift),
    }
}

pub(super) fn arrival_end_shift(ram: &[u8], ctx: u32) -> Option<[i32; 9]> {
    if game_anchors::u8_at(ram, ctx + 7) != STRIKE_LOOP_STATE
        || game_anchors::u8_at(ram, ctx + 0x15) != 0
    {
        return None;
    }
    let seat = u32::from(game_anchors::u8_at(ram, ctx + 0x13));
    let actor = game_anchors::u32_at(ram, ACTOR_TABLE + seat.min(7) * 4);
    if !(3..=7).contains(&seat) || !in_ram(actor) || game_anchors::u8_at(ram, actor + 0x1DE) != 3 {
        return None;
    }
    let record = |k: u32| game_anchors::u32_at(ram, actor + RING_RECORD + k * 4);
    if record(0) == record(1)
        || record(1) != record(2)
        || game_anchors::u16_at(ram, actor + CURSOR_RING) != 0
    {
        return None;
    }
    let ring = |k: u32| {
        let at = actor + POSITION_RING + k * 8;
        [
            game_anchors::u32_at(ram, at) as i32 as i16 as i32,
            game_anchors::u32_at(ram, at + 4) as i32 as i16 as i32,
        ]
    };
    let (s1, s2) = (ring(1), ring(2));
    // The focus records hold the pair negated.
    let end = |k: u32| i32::from(game_anchors::u16_at(ram, ctx + TWEEN_TABLE + k * 4 + 2) as i16);
    if end(6) != -s1[0] || end(8) != -s1[1] {
        return None;
    }
    let mut shift = [0i32; 9];
    shift[6] = s1[0] - s2[0];
    shift[8] = s1[1] - s2[1];
    Some(shift)
}

/// How far case `0xA`'s yaw endpoint stood, on the pass before the RAM's, from
/// the RAM's own - [`displayed_cam_live_steps`]'s `end_shift` for the yaw.
///
/// `FUN_801DC0A0`'s case `0xA` re-arms every pass with a yaw of
/// `node[+0x68] * 2 + 0x700 - facing` (`0x801DC8B8..0x801DC8CC`), so its
/// endpoint climbs with the clip. The RAM's arm read the cursor ring's slot `1`
/// (the anim tick runs after the arm) and the pass before it slot `2`.
/// `battle_noaru_slippery_v2_poisonous_breath_mid_cast` holds yaw `3350` under
/// an endpoint of `3368` on slot `1`'s `60`, and the walk back against the
/// `24` a pass lands on `3332` then `3320`, the yaw its frame fits best.
///
/// `None` off the cast band, or when the RAM's yaw endpoint is not the case's
/// law over slot `1` (another shot owns the table).
pub(super) fn clip_clock_end_shift(ram: &[u8], ctx: u32) -> Option<i32> {
    if !(0x2A..=0x2D).contains(&game_anchors::u8_at(ram, ctx + 7)) {
        return None;
    }
    let seat = u32::from(game_anchors::u8_at(ram, ctx + 0x13));
    let actor = game_anchors::u32_at(ram, ACTOR_TABLE + seat.min(7) * 4);
    if !in_ram(actor) {
        return None;
    }
    let cursor = |k: u32| i32::from(game_anchors::u16_at(ram, actor + CURSOR_RING + k * 2) as i16);
    let facing = i32::from(game_anchors::u16_at(ram, actor + 0x46));
    let end = i32::from(game_anchors::u16_at(ram, ctx + TWEEN_TABLE + 4 + 2));
    if (cursor(1) * 2 + 0x700 - facing - end) & 0xFFF != 0 {
        return None;
    }
    // A clip younger than the walk back: the pass before read another clip.
    let step = cursor(1) - cursor(2);
    (step > 0).then_some(-2 * step)
}

/// The walk's last heading on a monster strike whose displayed frame **is** that
/// walk pass, read off the attacker's position ring.
///
/// Each pass of the walk or the strike loop arms its framing and then stores
/// `+0x46` from the live pair the pass before left, so ring slot `k + 1`
/// names what pass `k` back stored. The strike loop bears onto the target's
/// **body** pair `+0x3C` / `+0x40` (`0x801E36F0..0x801E371C`), the walk states
/// onto its **live** pair `+0x34` / `+0x38` (`0x801E3568..0x801E3588`).
pub(super) struct WalkHeading {
    /// What the displayed frame's pass stored: the body is drawn on it.
    pub displayed: u16,
    /// Case 7's yaw endpoint on the pass before the RAM's, off the RAM's -
    /// [`displayed_cam_live_steps`]'s `end_shift` for the yaw. Case 7
    /// subtracts the heading (`0x801D65DC..`) ahead of the pass's own re-face,
    /// so that table read what the displayed frame's pass stored.
    pub end_shift: i32,
}

/// [`WalkHeading`] on a monster's first swing: the walk's last pass stands the
/// attacker nearest the target and the loop's first moves it away again, so
/// nearest on slot `2` the displayed frame is that pass, and it drew and handed
/// on the walk's bearing. `battle_korb2_warhalf_strike`'s body stands on `1836`
/// where the RAM holds `1730`, the table the RAM's replaced chased an endpoint
/// `112` further, and the walk back lands on yaw `795` where the RAM's endpoint
/// alone gives `759`.
///
/// A walker the `+0x76` lock keeps on the walk clip into the loop is read the
/// same way when it stands nearest on slot `3`: its loop passes back it off
/// the target a ring step at a time, and the displayed frame is still the
/// walk's last pass (`battle_geremi_ogre_strike` yaw `168` to `189`, `image`
/// `.878` to `.999`; `battle_vozz_twin_tomb_strike` `.957` to `.997`). So is
/// a walker whose RAM pass has just committed its swing, when the clip-record
/// ring (`+0x234`) holds the walk's record on slots `1..=3`: the displayed
/// frame's pass still ran the walk (`battle_map03_moldy_worm_strike`, swing
/// clip `5` committed on the RAM's pass, yaw `1429` to `1438`, `image` `.941`
/// to `.999`).
///
/// `None` otherwise. An arrival off the walk clip nearest on slot `3`, its
/// record changed by slot `2`, is a pass later and the bearing is the loop's own: `12` units of this shift read
/// `battle_noaru_gilium_lv2_strike` `.904` to `.585`. Nor is a party attacker's (its first swing commit re-seeds
/// the yaw counter, `FUN_8004E13C`) or a later swing's table read this way.
pub(super) fn walk_heading(ram: &[u8], ctx: u32) -> Option<WalkHeading> {
    if game_anchors::u8_at(ram, ctx + 7) != STRIKE_LOOP_STATE
        || game_anchors::u8_at(ram, ctx + 0x15) != 1
    {
        return None;
    }
    let seat = u32::from(game_anchors::u8_at(ram, ctx + 0x13));
    let actor = game_anchors::u32_at(ram, ACTOR_TABLE + seat.min(7) * 4);
    if !(3..=7).contains(&seat) || !in_ram(actor) {
        return None;
    }
    let target_slot = u32::from(game_anchors::u8_at(ram, actor + 0x1DD));
    let target = game_anchors::u32_at(ram, ACTOR_TABLE + target_slot.min(7) * 4);
    if target_slot > 7 || !in_ram(target) {
        return None;
    }
    let ring = |a: u32, k: u32| {
        let at = a + POSITION_RING + k * 8;
        [
            f64::from(game_anchors::u32_at(ram, at) as i32 as i16),
            f64::from(game_anchors::u32_at(ram, at + 4) as i32 as i16),
        ]
    };
    let dist = |k: u32| {
        let (a, t) = (ring(actor, k), ring(target, k));
        (a[0] - t[0]).hypot(a[1] - t[1])
    };
    let nearest_two = dist(2) < dist(1) && dist(2) < dist(3);
    // The walk clip's record held from slot `3` through slot `1` and the RAM's
    // pass committed the swing: the displayed frame's pass still ran the
    // walk, as a locked walker's does.
    let record = |k: u32| game_anchors::u32_at(ram, actor + RING_RECORD + k * 4);
    let walk_held = record(1) == record(2) && record(2) == record(3) && record(0) != record(1);
    let locked_walk_nearest_three = (game_anchors::u8_at(ram, actor + 0x1D9) == WALK_CLIP
        || walk_held)
        && dist(3) < dist(1)
        && dist(3) < dist(2)
        && dist(3) < dist(4);
    if !nearest_two && !locked_walk_nearest_three {
        return None;
    }
    // How far the target's body stood off its live pair.
    let body_off = [
        f64::from(game_anchors::i16_at(ram, target + 0x3C))
            - f64::from(game_anchors::i16_at(ram, target + 0x34)),
        f64::from(game_anchors::i16_at(ram, target + 0x40))
            - f64::from(game_anchors::i16_at(ram, target + 0x38)),
    ];
    // `(FUN_80019B28(..) + 0x800) & 0xFFF`: slot `k`'s live pair onto the
    // target's, shifted by `off`.
    let bearing = |k: u32, off: [f64; 2]| {
        let (a, t) = (ring(actor, k), ring(target, k));
        (f64::atan2(t[0] + off[0] - a[0], t[1] + off[1] - a[1]) * 4096.0 / std::f64::consts::TAU)
            .round() as i32
    };
    let displayed = bearing(3, [0.0, 0.0]);
    let turn = (bearing(2, body_off) - displayed + 2048).rem_euclid(4096) - 2048;
    // The counter the endpoint adds climbs `max(1, 4 * step / 3)` a pass.
    let step = i32::from(frame_step(ram));
    Some(WalkHeading {
        displayed: displayed.rem_euclid(4096) as u16,
        end_shift: turn - (4 * step / 3).max(1),
    })
}

/// How far the TR z endpoint stood, on the pass before a `0x6F` capture's,
/// from the RAM's own - [`displayed_cam_live_steps`]'s `end_shift`, in the
/// walker's raw space.
///
/// The magic-capture fade ramps the framing depth `ctx[+0x6D0]` down by
/// `16 * step` (`lbu v1,0x393` / `sll v1,v1,0x4` / `subu` at
/// `0x801E5000..0x801E5014`) and then re-arms case 6 on the new depth
/// (`jal 0x801d5854`, `a1 = 6` at `0x801E501C`), whose TR z endpoint is the
/// depth prescaled. The pass before stood on the depth before that ramp,
/// whether it was a `0x6F` pass (which ramped from there) or the `0x6E`
/// branch the fade left (which holds the depth the ramp starts from). The
/// other components' endpoints do not move with the depth.
pub(super) fn capture_fade_end_shift(ram: &[u8], ctx: u32, steps: &[u8]) -> [i32; 9] {
    use legaia_engine_battle_vm::battle_cam_script::prescale_tr_z;
    let depth = i32::from(game_anchors::u16_at(ram, ctx + 0x6D0) as i16);
    let ramp = 16 * i32::from(steps.first().copied().unwrap_or(1).max(1));
    let mut shift = [0; 9];
    shift[5] = (prescale_tr_z(depth + ramp) - prescale_tr_z(depth)) as i32;
    shift
}
