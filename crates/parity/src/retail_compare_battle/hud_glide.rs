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
    use legaia_ram_image::extract::PSX_RAM_KSEG0;
    use legaia_ram_image::prim_pool::{self, Prim};
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
