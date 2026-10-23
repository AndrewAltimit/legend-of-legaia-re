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
pub(super) fn displayed_cam_live_steps(
    ram: &[u8],
    ctx: u32,
    steps: &[u8],
    shot_divisor: bool,
    prior_end: [Option<i32>; 9],
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
            let wide = g.abs() + moved;
            let step = (wide + (d - fs_n) - 1) / (d - fs_n);
            // `p` behind `pos` (further from the endpoint), and the step back
            // on this table would land behind `p` too.
            if let Some(p) = prior_end[k as usize]
                && (pos - p) * dir > 0
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

/// The live pose the displayed frame was drawn at under a **module shot**:
/// retail's live camera walked back along a tween table built once.
///
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
    let vsyncs: i32 = steps.iter().map(|&s| i32::from(s.max(1))).sum();
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
    let mut back = [0i32; 9];
    for k in (0..9u32).filter(|&k| k != 2) {
        let at = ctx + TWEEN_TABLE + k * 4;
        let inc = i32::from(game_anchors::u16_at(ram, at) as i16).abs();
        let end = game_anchors::u16_at(ram, at + 2) as i16;
        let gap = i32::from(end.wrapping_sub(raw_live[k as usize] as i16));
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
