//! HUD widget glides that landed on the pass the capture froze, recovered
//! from the two ordering tables.
//!
//! `FUN_801D9BBC` snaps a record whose remaining `total - elapsed` is within
//! the frame step onto its target and retires it by clearing `total` alone
//! (`0x801D9C20` branches to the snap, `0x801D9C3C` jumps to the
//! `sb zero,0x0(a1)` at `0x801D9D1C`): the landing pass never writes
//! `elapsed`, so the byte keeps the last in-flight value, and the record
//! keeps its target (`+0x04` / `+0x06`) and start (`+0x08` / `+0x0A`) too. A
//! capture saved on the landing pass therefore reads a landed table while the
//! frame on screen is two passes older and shows the widget mid-glide
//! (`delilas_gi_spirit_artifact`: actor plaque, readout bar and AP plate
//! thirteen of sixteen vsyncs in in the older table, ten on screen).
//!
//! The RAM alone cannot tell that capture from one whose widget landed long
//! before: the retired record reads the same either way. The ordering tables
//! can. The table being built draws the widget at rest; the older one, built
//! on the record's last in-flight pass, draws it displaced by exactly the
//! glide's offset at that `elapsed`. A retired record whose offset the two
//! tables show is one that landed this pass, and the displayed frame is one
//! frame step before the older table (the older pass's own step,
//! `pass_steps`).

use std::collections::HashSet;

use super::*;
use legaia_ram_image::extract::PSX_RAM_KSEG0;
use legaia_ram_image::prim_pool::{self, Prim};

/// The handle table `FUN_801D9BBC` writes a record's seat through
/// (`ctx[+0x1074 + slot * 4]`, `0x801D9BF0`); a slot with no widget is
/// skipped (`0x801D9BF8`).
const HUD_GLIDE_HANDLES: u32 = 0x1074;

/// One screen-space sprite, keyed by everything but its position.
type SpriteKey = (u8, u16, (u8, u8), (u16, u16));

/// Every sprite one ordering table draws: `(key, position)`.
fn table_sprites(ram: &[u8], head: u32) -> Vec<(SpriteKey, (i16, i16))> {
    let Some(head) = head.checked_sub(PSX_RAM_KSEG0) else {
        return Vec::new();
    };
    prim_pool::chain_walk(ram, PSX_RAM_KSEG0, head as usize)
        .into_iter()
        .filter_map(|c| match c.prim {
            Prim::Sprt {
                pos,
                uv,
                clut,
                size,
                ..
            } => Some(((0, clut, uv, size), pos)),
            Prim::Sprt8 { pos, uv, clut, .. } => Some(((8, clut, uv, (8, 8)), pos)),
            Prim::Sprt16 { pos, uv, clut, .. } => Some(((16, clut, uv, (16, 16)), pos)),
            _ => None,
        })
        .collect()
}

/// The box a widget's sprites sit in around its seat: the plaque's left cap
/// eight pixels left of it, the bar's tiles from `x = 0` (seat `16`) out to
/// its arrow, the AP plate's pieces a hundred pixels right, each row within
/// a sprite's height below the seat.
const WIDGET_BOX_X: std::ops::RangeInclusive<i32> = -16..=256;
const WIDGET_BOX_Y: std::ops::RangeInclusive<i32> = -8..=24;

/// How many sprites `older` draws displaced by `d` from a sprite `newer`
/// draws at rest inside the widget's box around `target`, counting only
/// sprites that moved (a sprite `newer` also draws on the same spot is part
/// of a still widget, whatever repeats it holds).
fn moved_by(
    older: &[(SpriteKey, (i16, i16))],
    newer: &HashSet<(SpriteKey, (i16, i16))>,
    target: [i16; 2],
    d: (i32, i32),
) -> usize {
    older
        .iter()
        .filter(|(k, p)| {
            let rest = (i32::from(p.0) - d.0, i32::from(p.1) - d.1);
            WIDGET_BOX_X.contains(&(rest.0 - i32::from(target[0])))
                && WIDGET_BOX_Y.contains(&(rest.1 - i32::from(target[1])))
                && !newer.contains(&(*k, *p))
                && newer.contains(&(*k, (rest.0 as i16, rest.1 as i16)))
        })
        .count()
}

/// `FUN_801D9BBC`'s seat along one axis: `start + (target - start) *
/// elapsed / total`, the `div` truncating toward zero.
fn glide_axis(start: i16, target: i16, elapsed: u8, total: u8) -> i32 {
    i32::from(start)
        + (i32::from(target) - i32::from(start)) * i32::from(elapsed) / i32::from(total)
}

/// The retired HUD glide records that landed on the pass the capture froze,
/// each seated on the `elapsed` the displayed frame shows.
///
/// For each retired record with a widget and a last `elapsed`, `total` is
/// bounded by the snap rule: the landing pass found `total - elapsed <=
/// step` (`0x801D9C20`), so `total` is in `elapsed + 1 ..= elapsed + step`
/// on the landing pass's step (`steps[0]`).
/// A total whose offset at `elapsed` the older table shows (at least two
/// sprites moved by it onto their rest seats in the other table) names the
/// record; its displayed `elapsed` is less the older pass's step
/// (`steps[1]`).
pub(super) fn landed_glide_pool_seats(ram: &[u8], ctx: u32, steps: [u8; 2]) -> Vec<HudGlideSeat> {
    let retired: Vec<u32> = (0..HUD_GLIDE_SLOTS)
        .filter(|&s| {
            let r = ctx + HUD_GLIDE_TABLE + s * HUD_GLIDE_STRIDE;
            game_anchors::u8_at(ram, r) == 0
                && game_anchors::u8_at(ram, r + 1) != 0
                && game_anchors::u32_at(ram, ctx + HUD_GLIDE_HANDLES + s * 4) != 0
        })
        .map(|s| ctx + HUD_GLIDE_TABLE + s * HUD_GLIDE_STRIDE)
        .collect();
    if retired.is_empty() {
        return Vec::new();
    }
    let tables: Vec<Vec<(SpriteKey, (i16, i16))>> =
        prim_pool::find_ot_arrays(ram, PSX_RAM_KSEG0, 64)
            .iter()
            .map(|ot| table_sprites(ram, ot.head))
            .filter(|s| !s.is_empty())
            .collect();
    if tables.len() < 2 {
        return Vec::new();
    }
    let sets: Vec<HashSet<(SpriteKey, (i16, i16))>> =
        tables.iter().map(|t| t.iter().copied().collect()).collect();
    let step = steps[0].max(1);
    let mut out = Vec::new();
    for r in retired {
        let last = game_anchors::u8_at(ram, r + 1);
        let target = [
            game_anchors::i16_at(ram, r + 4),
            game_anchors::i16_at(ram, r + 6),
        ];
        let start = [
            game_anchors::i16_at(ram, r + 8),
            game_anchors::i16_at(ram, r + 10),
        ];
        let best = (1..=step)
            .filter_map(|k| last.checked_add(k))
            .filter_map(|total| {
                let d = (
                    glide_axis(start[0], target[0], last, total) - i32::from(target[0]),
                    glide_axis(start[1], target[1], last, total) - i32::from(target[1]),
                );
                if d == (0, 0) {
                    return None;
                }
                let n = (0..tables.len())
                    .flat_map(|a| {
                        (0..tables.len())
                            .filter(move |&b| b != a)
                            .map(move |b| (a, b))
                    })
                    .map(|(a, b)| moved_by(&tables[a], &sets[b], target, d))
                    .max()
                    .unwrap_or(0);
                (n >= 2).then_some((n, total))
            })
            .max_by_key(|&(n, total)| (n, std::cmp::Reverse(total)));
        if let Some((_, total)) = best {
            out.push(HudGlideSeat {
                target,
                elapsed: last.saturating_sub(steps[1]),
                total,
            });
        }
    }
    out
}
