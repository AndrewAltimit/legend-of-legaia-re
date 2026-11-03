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

/// The Done tail's countdown `ctx[+0x6D8]` and the two values the Done arm
/// `0x50` seeds it with (`li v0,0x3c` / `sh v0,0x2(s7)` at `0x801E5F24..28`,
/// `0x96` when `ctx[+0x15]` is set, `0x801E5F38..3C`).
const DONE_TIMER: u32 = 0x6D8;
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
pub(super) fn displayed_cam_live_steps(
    ram: &[u8],
    ctx: u32,
    steps: &[u8],
    shot_divisor: bool,
    prior_end: [Option<i32>; 9],
    end_shift: [i32; 9],
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
/// `None` otherwise. Nearest on slot `3` is a pass later and the bearing is the
/// loop's own: `12` units of this shift read `battle_noaru_gilium_lv2_strike`
/// `.904` to `.585`. Nor is a party attacker's (its first swing commit re-seeds
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
    if dist(2) >= dist(1) || dist(2) >= dist(3) {
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
