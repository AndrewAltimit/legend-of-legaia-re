//! The HUD glide table's seats at the capture instant, with their
//! environment hand-off.

use super::*;

/// The HUD's tracked widget glides `FUN_801D9BBC` walks: `ctx[+0x11B4]`,
/// `0xC`-byte records `[total][elapsed] .. [target x, y][start x, y]`, one
/// per handle slot `ctx[+0x1074]` (forty).
pub(super) const HUD_GLIDE_TABLE: u32 = 0x11B4;
pub(super) const HUD_GLIDE_STRIDE: u32 = 0xC;
pub(super) const HUD_GLIDE_SLOTS: u32 = 40;

/// Every in-flight record of the HUD glide table, lag-corrected
/// ([`HudGlideSeat`]).
pub(super) fn hud_glide_seats(ram: &[u8], ctx: u32, lag: u16) -> Vec<HudGlideSeat> {
    (0..HUD_GLIDE_SLOTS)
        .filter_map(|s| {
            let r = ctx + HUD_GLIDE_TABLE + s * HUD_GLIDE_STRIDE;
            let total = game_anchors::u8_at(ram, r);
            (total != 0).then(|| HudGlideSeat {
                target: [
                    game_anchors::i16_at(ram, r + 4),
                    game_anchors::i16_at(ram, r + 6),
                ],
                elapsed: u16::from(game_anchors::u8_at(ram, r + 1)).saturating_sub(lag) as u8,
                total,
            })
        })
        .collect()
}

/// The combo cluster's anchor (placement record 80): seat A `(328, 168)`,
/// seat B `(168, 168)`, over the round's `ctx[+0x1C] = 0x10` vsyncs.
const COMBO_START: [i16; 2] = [328, 168];
const COMBO_TARGET: [i16; 2] = [168, 168];
const COMBO_TOTAL: u8 = 16;

/// The combo cluster's glide on the **displayed** frame, timed off the
/// damage numeral's ring timer when its record has already retired.
///
/// A landed hit's damage write raises `DAT_8007B64C` and pushes the numeral
/// (`FUN_801F44A0`, ring timer `ctx[+0x85C]` zeroed, stepped `0x10` a vsync by
/// the readout `FUN_801DF6B8`); the readout pass `FUN_801E805C` at the head
/// of the next action-SM pass answers the flag by opening record 80
/// (`FUN_801D8DE8(0x50, 0)`). The cluster's `elapsed` therefore trails the
/// newest numeral's age by two passes: `battle_melee_hit_spark` holds the
/// live record at `12` beside timer `0x120` (18 vsyncs, step `3`).
///
/// The record retires on the pass that snaps it and keeps its last `elapsed`
/// byte, so a table read on a later pass cannot say whether the frame on
/// screen still showed it gliding; the numeral's timer can.
/// `battle_gimard_tail_fire_b` (step `2`) reads a retired cluster beside
/// timer `0x160`: 22 vsyncs, the cluster `18` in the RAM's pass and `14`
/// on screen, its label at `x = 228` where a landed one stands on `208`.
pub(super) fn combo_cluster_timer_seat(ram: &[u8], ctx: u32, lag: u16) -> Option<HudGlideSeat> {
    let retired = (0..HUD_GLIDE_SLOTS).any(|s| {
        let r = ctx + HUD_GLIDE_TABLE + s * HUD_GLIDE_STRIDE;
        game_anchors::u8_at(ram, r) == 0
            && game_anchors::i16_at(ram, r + 4) == COMBO_TARGET[0]
            && game_anchors::i16_at(ram, r + 6) == COMBO_TARGET[1]
            && game_anchors::i16_at(ram, r + 8) == COMBO_START[0]
            && game_anchors::i16_at(ram, r + 10) == COMBO_START[1]
    });
    if !retired {
        return None;
    }
    let newest = u32::from(game_anchors::u8_at(ram, ctx + 0x262).wrapping_sub(1) & 7);
    if game_anchors::u32_at(ram, ctx + 0x83C + newest * 4) == 0 {
        return None;
    }
    let age = game_anchors::u32_at(ram, ctx + 0x85C + newest * 4) >> 4;
    // Two passes of spawn delay, then the display lag's two.
    let elapsed = age.checked_sub(2 * u32::from(lag))?;
    (elapsed < u32::from(COMBO_TOTAL)).then_some(HudGlideSeat {
        target: COMBO_TARGET,
        elapsed: elapsed as u8,
        total: COMBO_TOTAL,
    })
}

/// The newest damage numeral as the **displayed** frame drew it: the struck
/// pool slot and the engine popup age
/// (`legaia_engine_battle_vm::battle_value_readout::popup_anchor`, which
/// rises on `16 * age`) whose rise is the one retail drew.
///
/// The readout `FUN_801DF6B8` runs once a pass, in the first body's draw
/// tick ahead of every pose decode: it lifts the numeral by `3/2` of the
/// ring timer as it stood **before** the pass's step
/// (`0x801DF784..0x801DF7B4`), then steps the timer by `0x10 * step`
/// (`0x801DF7B8..0x801DF7C8`). A capture saved after its pass's readout
/// holds the stepped timer, so the frame on screen - drawn the display lag
/// earlier - rose on that timer less the lag's vsyncs and its own pass's
/// step. A PCSX-Redux film of `rim_elm_gimard_victory`
/// (`autorun_drawn_pose_film.lua`) logs the read, the stepped timer and the
/// trio of the previous pass's decode on every pass.
///
/// The replay lands the hit on its own clock, so its popup's age on the
/// captured frame need not be retail's: `battle_melee_hit_spark`'s RAM
/// timer `288` at step `3` puts the frame's rise on `144` (age `9`), where
/// the replay had aged its `15` to `11`, eighteen pixels higher. Only the
/// age is seated: the renderer reads the struck body's trio on every draw,
/// so the anchor is the captured frame's own readout trio.
pub(super) fn popup_age_seat(ram: &[u8], ctx: u32, lag: u16, step: u8) -> Option<(u8, u16)> {
    let newest = u32::from(game_anchors::u8_at(ram, ctx + 0x262).wrapping_sub(1) & 7);
    if game_anchors::u32_at(ram, ctx + 0x83C + newest * 4) == 0 {
        return None;
    }
    let seat = game_anchors::u16_at(ram, ctx + 0x318 + newest * 2);
    if seat >= 8 {
        return None;
    }
    let vsyncs = game_anchors::u32_at(ram, ctx + 0x85C + newest * 4) >> 4;
    let age = vsyncs.checked_sub(u32::from(lag) + u32::from(step.max(1)))?;
    Some((seat as u8, age.min(u32::from(u16::MAX)) as u16))
}

/// The round prompt's `Begin` chip (placement record 0): it spawns on
/// `x = 142` and lands on `x = 104` over [`ROUND_PROMPT_TOTAL`] vsyncs, and
/// its left cap is an `8`-wide `SPRT` (CLUT `0x7FC4`, texel `(208, 0)`)
/// drawn eight pixels left of the record's seat.
const BEGIN_START_X: i32 = 142;
const BEGIN_SEAT: [i16; 2] = [104, 86];
const BEGIN_CAP_DX: i32 = 8;
const BEGIN_CAP_CLUT: u16 = 0x7FC4;
const BEGIN_CAP_UV: (u8, u8) = (208, 0);
const ROUND_PROMPT_TOTAL: u8 = 16;

/// The `elapsed` at which `FUN_801D9BBC` seats the `Begin` chip on `x`
/// (`start + (target - start) * elapsed / total`); `None` for an `x` no step
/// of the glide produces.
pub(super) fn begin_chip_elapsed(x: i32) -> Option<u8> {
    (0..=ROUND_PROMPT_TOTAL).find(|&e| {
        BEGIN_START_X
            + (i32::from(BEGIN_SEAT[0]) - BEGIN_START_X) * i32::from(e)
                / i32::from(ROUND_PROMPT_TOTAL)
            == x
    })
}

/// The round prompt's open glide as the **displayed** frame shows it, read
/// off the packet pools when the glide table no longer holds it.
///
/// A glide record is cleared on the pass that snaps it, so a capture taken
/// on that pass or the one after reads an empty table while the frame on
/// screen still shows the prompt unfolding. The two ordering tables hold the
/// frame being built and the one the GPU drew before it, and the displayed
/// frame is one frame step older than the older of the two
/// ([`display_lag_vsyncs`]). So the older table's `Begin` left cap names the
/// glide's `elapsed` there, and one step less is the frame on screen
/// (`koru_strip_forced`: the tables hold the cap at rest and five pixels
/// short, the display shows it ten short).
pub(super) fn round_prompt_pool_seat(ram: &[u8], step: u8) -> Option<HudGlideSeat> {
    use psx_ram_image::extract::PSX_RAM_KSEG0;
    use psx_ram_image::prim_pool::{self, Prim};
    let oldest = prim_pool::find_ot_arrays(ram, PSX_RAM_KSEG0, 64)
        .iter()
        .filter_map(|ot| {
            let head = ot.head.checked_sub(PSX_RAM_KSEG0)? as usize;
            prim_pool::chain_walk(ram, PSX_RAM_KSEG0, head)
                .iter()
                .find_map(|c| match c.prim {
                    Prim::Sprt { pos, uv, clut, .. }
                        if clut == BEGIN_CAP_CLUT && uv == BEGIN_CAP_UV =>
                    {
                        begin_chip_elapsed(i32::from(pos.0) + BEGIN_CAP_DX)
                    }
                    _ => None,
                })
        })
        .min()?;
    (oldest < ROUND_PROMPT_TOTAL).then(|| HudGlideSeat {
        target: BEGIN_SEAT,
        elapsed: oldest.saturating_sub(step),
        total: ROUND_PROMPT_TOTAL,
    })
}

/// The yaw counter `ctx[+0x6DA]`.
const YAW_COUNTER: u32 = 0x6DA;
/// The rung the action seed's Attack arm stands the counter on
/// (`li v0,0x200; sh v0,0x4(s7)` at `0x801E2F1C..0x801E2F20`).
const ATTACK_ARM_RUNG: u16 = 0x200;
/// The strike states that run on the Attack arm's rung: the approach `0x14`
/// / `0x19` and the strike `0x1E`.
const ATTACK_RUNG_STATES: [u8; 3] = [0x14, 0x19, 0x1E];
/// The widgets the action seed raises on the Attack arm's pass, each as its
/// `(target, start)`: the readout bar (record 7, `FUN_801D8DE8(7, 0)` at
/// `0x801E2F40`, a party target only) and the actor plaque.
const ATTACK_ARM_WIDGETS: [([i16; 2], [i16; 2]); 2] =
    [([16, 192], [16, 234]), ([16, 12], [16, -26])];

/// What the prologue adds to the yaw counter on a pass of frame step `step`:
/// `(step * 4) / 3`, or `1` where that truncates to zero
/// (`0x801E29E4..0x801E2A24`).
fn counter_drift(step: u8) -> u16 {
    match (u16::from(step) * 4) / 3 {
        0 => 1,
        d => d,
    }
}

/// The readout bar and actor plaque the action seed raised on the Attack
/// arm's pass, retired in the RAM but still gliding on the displayed frame.
///
/// The seed stores state `0x14` and the counter's `0x200` rung, then raises
/// the bar for a party target on the same pass (`0x801E2F14..0x801E2F44`);
/// the plaque opens on that pass too. Each later pass drifts the counter
/// ([`counter_drift`]) and adds its step to every glide's `elapsed`, so the
/// counter's distance from the rung counts the passes since the raise, and
/// the frame on screen, two passes older than the RAM, drew the glides at
/// the elapsed those passes less the newest two had built. A record the
/// table retired one pass before the older ordering table keeps no trace in
/// either table ([`landed_glide_pool_seats`] cannot see it):
/// `battle_nilboa_aluru_v2_strike` reads the counter at `0x212` (nine
/// step-`2` passes), both glides retired at `14`, both tables at rest, and
/// the screen shows bar and plaque six and five pixels short of their seats.
pub(super) fn attack_arm_glide_seats(ram: &[u8], ctx: u32) -> Vec<HudGlideSeat> {
    if !ATTACK_RUNG_STATES.contains(&game_anchors::u8_at(ram, ctx + 7)) {
        return Vec::new();
    }
    let Some(mut left) = game_anchors::u16_at(ram, ctx + YAW_COUNTER).checked_sub(ATTACK_ARM_RUNG)
    else {
        return Vec::new();
    };
    // Walk the passes back to the arm's: the counter's drift over them adds
    // up to its distance from the rung exactly, or the rung is not the arm's.
    let steps = recent_pass_steps(ram);
    let mut passes = 0usize;
    while left > 0 && passes < steps.len() {
        let d = counter_drift(steps[passes]);
        if d > left {
            return Vec::new();
        }
        left -= d;
        passes += 1;
    }
    if left != 0 || passes < 2 {
        return Vec::new();
    }
    let displayed: u16 = steps[2..passes].iter().map(|&s| u16::from(s)).sum();
    (0..HUD_GLIDE_SLOTS)
        .filter_map(|s| {
            let r = ctx + HUD_GLIDE_TABLE + s * HUD_GLIDE_STRIDE;
            let at = |o: u32| {
                [
                    game_anchors::i16_at(ram, r + o),
                    game_anchors::i16_at(ram, r + o + 2),
                ]
            };
            let last = u16::from(game_anchors::u8_at(ram, r + 1));
            let total = 16u16;
            (game_anchors::u8_at(ram, r) == 0
                && ATTACK_ARM_WIDGETS.contains(&(at(4), at(8)))
                && displayed <= last
                && total - last.min(total) <= 4)
                .then(|| HudGlideSeat {
                    target: at(4),
                    elapsed: displayed as u8,
                    total: total as u8,
                })
        })
        .collect()
}

/// The battle-frame depth-cue pass `FUN_80050120` adds `step << 6` to the
/// backdrop pair's weight (`+0x78`) a pass while the action latch
/// `ctx[+0x243]` is up (`0x80050600..0x8005065C`), and takes it back to `0`
/// at the same rate once the latch falls.
const BACKDROP_CUE_PER_VSYNC: u16 = 0x40;
/// The weight's ceilings (`0x800` outdoors, `0xC00` / `0x1000` under the
/// module stage bytes): a weight on one may have sat there for any time.
const BACKDROP_CUE_CEILINGS: [u16; 3] = [0x800, 0xC00, 0x1000];
/// The Spirit / Item band past its pre-arm.
const SPIRIT_BAND: std::ops::RangeInclusive<u8> = 0x3D..=0x40;
/// The capture-class band (`0x6E..=0x71`), whose cast begin raises the same
/// latch behind its `0x6E` store (`0x801E44FC`).
const CAPTURE_BAND: std::ops::RangeInclusive<u8> = 0x6E..=0x71;

/// The actor plaque and target plaque of a Spirit / Item band, retired in the
/// RAM but still rising on the displayed frame, timed off the backdrop weight.
///
/// The band's pre-arm `0x3C` raises the latch (`0x801E3B6C`), and the
/// plaques' sixteen-vsync rise and the weight's ramp count the same vsyncs
/// from there: `battle_jou_vera_v3_heal_mid_cast` holds both plaques in
/// flight at `15` beside weight `960` (`15 * 0x40`), and
/// `battle_deene_garnet_heal_mid_cast` holds them retired at `13` beside
/// weight `0x500` (twenty vsyncs: `13`, then its step-`3` landing pass and
/// the step-`4` pass the capture froze in). A retired record cannot say when
/// it landed ([`landed_glide_pool_seats`] needs the older table to show it
/// moving), and in Garnet's capture both tables draw the plaques at rest
/// while the screen, two passes back, shows them eight pixels short. The
/// weight on the displayed frame, `lag` vsyncs less, is the rise's age
/// there. A weight on a ceiling says nothing and seats nothing.
pub(super) fn spirit_band_glide_seats(ram: &[u8], ctx: u32, lag: u16) -> Vec<HudGlideSeat> {
    let state = game_anchors::u8_at(ram, ctx + 7);
    if !(SPIRIT_BAND.contains(&state) || CAPTURE_BAND.contains(&state))
        || game_anchors::u8_at(ram, ctx + 0x243) == 0
    {
        return Vec::new();
    }
    let rec = game_anchors::u32_at(ram, ctx + 0x106C);
    if !in_ram(rec) {
        return Vec::new();
    }
    let cue = game_anchors::u16_at(ram, rec + 0x78);
    if BACKDROP_CUE_CEILINGS.contains(&cue) {
        return Vec::new();
    }
    let total = 16u16;
    let Some(shown) = (cue / BACKDROP_CUE_PER_VSYNC).checked_sub(lag) else {
        return Vec::new();
    };
    if shown >= total {
        return Vec::new();
    }
    (0..HUD_GLIDE_SLOTS)
        .filter_map(|s| {
            let r = ctx + HUD_GLIDE_TABLE + s * HUD_GLIDE_STRIDE;
            let at = |o: u32| {
                [
                    game_anchors::i16_at(ram, r + o),
                    game_anchors::i16_at(ram, r + o + 2),
                ]
            };
            let last = u16::from(game_anchors::u8_at(ram, r + 1));
            let (target, start) = (at(4), at(8));
            // The actor plaque (record 68) from above the screen, the target
            // plaque (record 81) from below it onto the bar's row, and the
            // roster panels (records 6 / `0x4E` / `0x4F`) a monster's
            // party-wide seed raises beside the actor plaque on one pass
            // (`FUN_801E6D84`, `0x801E7058..0x801E7078`).
            let plaque = (target, start) == ([16, 12], [16, -26])
                || (target[1] == 192 && start == [target[0], 234])
                || (target[1] == 168 && start == [target[0], 234]);
            (game_anchors::u8_at(ram, r) == 0 && plaque && shown <= last).then_some(HudGlideSeat {
                target,
                elapsed: shown as u8,
                total: total as u8,
            })
        })
        .collect()
}
