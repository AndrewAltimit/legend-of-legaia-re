//! Retail-side seed readers: the facts each comparison seeds the engine with,
//! read out of a state's RAM / VRAM (ambient headings and stream turns, the
//! clear colour, scroll rects and panels, object models, walkers, morph
//! weights, fog, CLUT cell FX, the pause menu and the camera block), each with
//! its environment hand-off.

use super::*;

/// Motion-VM ops that write `+0x26`: the directional steps `0x03` / `0x19`
/// / `0x20`, the ramps `0x04` / `0x0D`, the home-relative step `0x06` and
/// the AABB wander `0x18` (`docs/subsystems/motion-vm.md`).
pub(super) const AMBIENT_HEADING_OPS: [u8; 7] = [0x03, 0x04, 0x06, 0x0D, 0x18, 0x19, 0x20];

/// Whether actor node `n`'s heading is the ambient motion VM's: the VM is
/// dispatched on it (`+0x10 & 0x80`), no script or pursue context holds it
/// (`+0x10 & 0x500`, the busy test the interpreter defers to), and the
/// variant its PC sits in (`*(+0x80) + *(+0x84)`, the variant table
/// `[u16 selector][s16 delta]` the preamble walks) carries a heading op
/// before its loop-back `0x01`.
pub fn retail_ambient_heading(ram: &[u8], n: u32) -> bool {
    game_anchors::u32_at(ram, n + 0x10) & 0x500 == 0 && retail_stream_turns(ram, n)
}

/// Whether node `n` runs an ambient motion stream that walks or turns it
/// (`+0x10 & 0x80`, a heading op in the variant its PC sits in), whatever
/// holds it now: an engaged wanderer stopped where its walks left it.
pub fn retail_stream_turns(ram: &[u8], n: u32) -> bool {
    use legaia_scene_formats::man_motion::op_width;
    let flags = game_anchors::u32_at(ram, n + 0x10);
    if flags & 0x80 == 0 {
        return false;
    }
    let stream = game_anchors::u32_at(ram, n + 0x80);
    if !(0x8000_0000..0x8020_0000).contains(&stream) {
        return false;
    }
    let pc = stream + u32::from(game_anchors::u16_at(ram, n + 0x84));
    // The variant whose code holds the PC.
    let mut header = stream;
    let mut code = None;
    for _ in 0..64 {
        let selector = game_anchors::u16_at(ram, header);
        let delta = game_anchors::i16_at(ram, header + 2);
        let end = if selector == 0xFFFF || delta <= 0 {
            u32::MAX
        } else {
            header + delta as u32
        };
        if (header + 4..end).contains(&pc) {
            code = Some(header + 4);
            break;
        }
        if end == u32::MAX {
            break;
        }
        header = end;
    }
    let Some(mut at) = code else {
        return false;
    };
    for _ in 0..256 {
        if at >= 0x8020_0000 {
            break;
        }
        let op = game_anchors::u8_at(ram, at);
        if AMBIENT_HEADING_OPS.contains(&op) {
            return true;
        }
        match (op, op_width(op)) {
            (0x01, _) | (_, None) => break,
            (_, Some(w)) => at += w as u32,
        }
    }
    false
}

/// The heading `+0x26` of every actor the field actor tick (`FUN_8003BC08`)
/// runs, keyed by its flat MAN index `+0x50` (first node per index).
pub fn retail_actor_facings(ram: &[u8]) -> Vec<ActorFacing> {
    let player = game_anchors::player_ptr(ram);
    let mut seen = std::collections::BTreeSet::new();
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| Some(n) != player && game_anchors::u32_at(ram, n + 0x0C) == 0x8003_BC08)
        .filter_map(|n| {
            let flat = game_anchors::u16_at(ram, n + 0x50);
            seen.insert(flat).then(|| ActorFacing {
                flat,
                facing: game_anchors::i16_at(ram, n + 0x26),
                x: game_anchors::i16_at(ram, n + 0x14),
                z: game_anchors::i16_at(ram, n + 0x18),
                model: game_anchors::i16_at(ram, n + 0x64),
                flags: game_anchors::u32_at(ram, n + 0x10),
                ambient: retail_ambient_heading(ram, n),
            })
        })
        .collect()
}

/// The draw environment's clear colour bytes (`r0 / g0 / b0`).
pub(super) const DRAW_ENV_CLEAR: u32 = 0x8007_BF5D;

/// The clear colour a field state's frame is filled with wherever no
/// primitive lands: the draw environment's `r0 / g0 / b0` (`0x8007BF5D..5F`),
/// which op `4C 13` writes and the MAN loader zeroes. It is the system
/// script's history - `town01`'s entry loop sets cave brown inside its cliff
/// box only on a pass the player is free for, and the opening holds the
/// player from the install pass on (the `rim_elm_zoom_intro` capture's system
/// context is still parked on its install-pass PC, the colour black) - which
/// a seed that runs the loop before the resume cannot reproduce. The image
/// child writes it over the engine's on the frame it captures.
pub fn retail_clear_rgb(ram: &[u8]) -> [u8; 3] {
    [0, 1, 2].map(|i| game_anchors::u8_at(ram, DRAW_ENV_CLEAR + i))
}

/// The pointer to the overworld curvature table (`FUN_800271A8`'s first
/// buffer).
const CURVATURE_TABLE_PTR: u32 = 0x8007_BB04;

/// The build offset of a state's overworld curvature table, in screen rows:
/// its entry `0`, the zero vector's `SY - 0x78`, which is the `OFY` the
/// table was built under less `120`. `0` after a scene load; `-6` in a state
/// whose player has opened the map display since
/// (`legaia_render_kernels::overworld_curvature::REBUILD_BIAS`,
/// `sebucus_overworld_resident`). That is pad history the seat does not
/// replay, so the image child takes it as `LEGAIA_SEAT_CURVE_BIAS`. `None`
/// when no table is allocated.
pub fn retail_curve_bias(ram: &[u8]) -> Option<i16> {
    let ptr = game_anchors::u32_at(ram, CURVATURE_TABLE_PTR);
    let end = 0x8000_0000u32 + ram.len() as u32;
    if !(0x8001_0000..end.saturating_sub(2)).contains(&ptr) {
        return None;
    }
    let bias = game_anchors::i16_at(ram, ptr);
    (-16..=16).contains(&bias).then_some(bias)
}

/// The field pager's state word `_DAT_801F2734` (`0x19` = a page waits).
const PAGER_STATE: u32 = 0x801F_2734;
/// The pager's automatic-press countdown `_DAT_80073F00`.
const PAGER_AUTO_PRESS: u32 = 0x8007_3F00;
/// The cursor sprite primitive's kind-1 frame index / timer
/// (`0x801C6000 + 4`, `0x801C6010 + 4`; `FUN_8002B994`).
const PAGE_MARK_FRAME: u32 = 0x801C_6004;
const PAGE_MARK_TIMER: u32 = 0x801C_6014;

/// Which frame of the dialogue page mark's two-frame strip a field state's
/// **displayed** frame shows, or `None` when the pager draws no mark (no
/// page waits, or an automatic press is counting down -
/// `0x801D9804..0x801D9828`).
///
/// The strip flips every sixteen vsyncs of the kind's timer
/// (`legaia_engine_dialog::cursor_sprite`), which counts from the first call
/// at the box's height - time the seed does not replay. The frame on screen
/// is two game frames older than the RAM, so the timer is taken back by the
/// lag and the frame with it when that crosses a flip.
pub fn retail_page_mark(ram: &[u8]) -> Option<u8> {
    if game_anchors::u32_at(ram, PAGER_STATE) != 0x19
        || game_anchors::i16_at(ram, PAGER_AUTO_PRESS) > 0
    {
        return None;
    }
    let frame = game_anchors::u32_at(ram, PAGE_MARK_FRAME);
    let timer = game_anchors::u32_at(ram, PAGE_MARK_TIMER) as i32;
    if frame > 1 || !(0..16).contains(&timer) {
        return None;
    }
    let lag = 2 * i32::from(crate::retail_compare_battle::frame_step(ram).max(1));
    Some(if timer < lag {
        frame as u8 ^ 1
    } else {
        frame as u8
    })
}

/// The actor tick that runs a move-VM part (`FUN_80021DF4`).
pub(super) const PART_TICK: u32 = 0x8002_1DF4;

/// Every live mode-4 VRAM scroller on a retail state's actor lists - a
/// `FUN_80021DF4` part with `+0x5A = 4`, rect `+0xD0..+0xD6`
/// (`legaia_engine_field::vram_scroll`) - with the
/// texels the state's VRAM (`1024 x 512` BGR555 LE) holds there.
pub fn retail_scroll_rects(ram: &[u8], vram: &[u8]) -> Vec<SeededVramRect> {
    if vram.len() != 1024 * 512 * 2 {
        return Vec::new();
    }
    let mut out: Vec<SeededVramRect> = Vec::new();
    let step = i16::from(crate::retail_compare_battle::frame_step(ram));
    for n in crate::retail_compare_script::actor_nodes(ram) {
        if game_anchors::u32_at(ram, n + 0x0C) != PART_TICK
            || game_anchors::i16_at(ram, n + 0x5A) != 4
        {
            continue;
        }
        let rect = (
            game_anchors::u16_at(ram, n + 0xD0),
            game_anchors::u16_at(ram, n + 0xD2),
            game_anchors::u16_at(ram, n + 0xD4),
            game_anchors::u16_at(ram, n + 0xD6),
        );
        let (x, y, w, h) = rect;
        if w == 0 || h == 0 || w > 1024 || h > 512 || out.iter().any(|(r, _)| *r == rect) {
            continue;
        }
        let texels = (0..h)
            .flat_map(|row| (0..w).map(move |col| (row, col)))
            .map(|(row, col)| {
                let o =
                    (((usize::from(y + row) & 0x1FF) * 1024) + (usize::from(x + col) & 0x3FF)) * 2;
                u16::from_le_bytes([vram[o], vram[o + 1]])
            })
            .collect::<Vec<u16>>();
        // The displayed frame is two game frames older than the VRAM: take
        // back the rotations the scroller fired in between.
        let fires = scroll_fires_within(
            game_anchors::i16_at(ram, n + 0xC4),
            game_anchors::i16_at(ram, n + 0xC6),
            step,
            DISPLAY_LAG_FRAMES,
        );
        let back = |per_tick: i16, extent: u16| -> usize {
            let e = i32::from(extent).max(1);
            (i32::from(per_tick) * i32::from(step) * fires).rem_euclid(e) as usize
        };
        let (bw, bh) = (
            back(game_anchors::i16_at(ram, n + 0xCC), w),
            back(game_anchors::i16_at(ram, n + 0xCE), h),
        );
        out.push((
            rect,
            unrotate_rect(&texels, usize::from(w), usize::from(h), bw, bh),
        ));
    }
    out
}

/// Game frames the displayed frame lags the RAM by (the double-buffer law of
/// [`crate::retail_compare_battle::display_lag_vsyncs`], in frames).
pub(super) const DISPLAY_LAG_FRAMES: i32 = 2;

/// How many times a mode-4 scroller fired over its last `lag` game ticks,
/// from its live countdown `+0xC6` and reload `+0xC4`: the countdown drains
/// `step` a tick and fires the tick it goes negative, reloading to the
/// period (`vram_scroll::mode4_integrate`), so it fires every
/// `period / step + 1` ticks and a countdown equal to the period fired on the
/// current tick.
pub(super) fn scroll_fires_within(period: i16, countdown: i16, step: i16, lag: i32) -> i32 {
    let (p, c, s) = (
        i32::from(period),
        i32::from(countdown),
        i32::from(step.max(1)),
    );
    if p < 0 || c > p {
        return 0;
    }
    let cycle = p / s + 1;
    let since = (p - c) / s;
    if since > lag - 1 {
        0
    } else {
        1 + (lag - 1 - since) / cycle
    }
}

/// Rotate a `w x h` rect **right** by `dx` and **down** by `dy` - the inverse
/// of the scroller's left / up rotation.
pub(super) fn unrotate_rect(texels: &[u16], w: usize, h: usize, dx: usize, dy: usize) -> Vec<u16> {
    if w == 0 || h == 0 || texels.len() < w * h {
        return texels.to_vec();
    }
    (0..h)
        .flat_map(|row| (0..w).map(move |col| (row, col)))
        .map(|(row, col)| texels[((row + h - dy % h) % h) * w + (col + w - dx % w) % w])
        .collect()
}

/// The image-panel widget's handler (`FUN_801F849C`, PROT 0900).
pub(super) const PANEL_TICK: u32 = 0x801F_849C;

/// The first live image-panel widget on a retail state's actor lists
/// ([`legaia_engine_effects::screen_fx::PanelWidget`]'s field map: current
/// `+0x14..+0x1A` / `+0x24`, targets `+0x3C..+0x42` / `+0x26`, base sizes
/// `+0xB8..+0xBC`, tween `+0x9C` / `+0x9E`, spawn size `+0xAA` / `+0xAC`,
/// texel origin `+0xA4` / `+0xA8`, pages `+0xA0` / `+0xA2`).
///
/// The ending vignettes spawn the panel from the vignette's own record
/// (`43 12` grabs the drawn frame into `(512, 0)`, `43 13` shows it), and a
/// capture is usually parked in the credits record that runs after it -
/// `ending_panel_corner` holds record 13, the panel already shrunk to the
/// corner - so a seed that resumes the running record never spawns it.
pub fn retail_panel(ram: &[u8]) -> Option<legaia_engine_effects::screen_fx::PanelWidget> {
    let n = crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .find(|&n| game_anchors::u32_at(ram, n + 0x0C) == PANEL_TICK)?;
    let h = |o: u32| game_anchors::i16_at(ram, n + o);
    Some(legaia_engine_effects::screen_fx::PanelWidget {
        cur: [h(0x14), h(0x16), h(0x18), h(0x1A), h(0x24)],
        target: [h(0x3C), h(0x3E), h(0x40), h(0x42), h(0x26)],
        base: [h(0xB8), h(0xBA), h(0xBC)],
        t: h(0x9C),
        dur: h(0x9E),
        w0: h(0xAA),
        h0: h(0xAC),
        u: game_anchors::u8_at(ram, n + 0xA4),
        v: game_anchors::u8_at(ram, n + 0xA8),
        texpage: game_anchors::u16_at(ram, n + 0xA0),
        texpage2: game_anchors::u16_at(ram, n + 0xA2),
    })
}

/// The VRAM the panel samples, with the state's texels there: page 0 from
/// the texel origin over the spawn size (one page at most), and - for a
/// panel wider than a page - page 1 out to the far edge its quad's `u`
/// reaches. That edge is not the image's: `FUN_801F849C` starts the second
/// quad at `u + 0x100 + 0xE` and ends it at `u + w0 + 0x10`
/// (`0x801F8838..0x801F88B0`, byte-wrapped), past the `320`-wide grab, and
/// the `43 12` split copies `0x60` columns from source `+0xF0` to match
/// (`legaia_engine_vm::vram_rect_copy::op43_sub12_calls`) - so a seed cut at
/// the image's own width left the strip's last texels unseeded.
pub(super) fn panel_source_rects(
    p: &legaia_engine_effects::screen_fx::PanelWidget,
    vram: &[u8],
) -> Vec<SeededVramRect> {
    let (w0, h) = (p.w0.clamp(0, 1024) as u16, p.h0.clamp(0, 512) as u16);
    if w0 == 0 || h == 0 || vram.len() != 1024 * 512 * 2 {
        return Vec::new();
    }
    let page = |tp: u16| ((tp & 0xF) * 64, ((tp >> 4) & 1) * 256 + u16::from(p.v));
    let grab = |x: u16, y: u16, w: u16| -> SeededVramRect {
        let texels = (0..h)
            .flat_map(|row| (0..w).map(move |col| (row, col)))
            .map(|(row, col)| {
                let o =
                    (((usize::from(y + row) & 0x1FF) * 1024) + (usize::from(x + col) & 0x3FF)) * 2;
                u16::from_le_bytes([vram[o], vram[o + 1]])
            })
            .collect();
        ((x, y, w, h), texels)
    };
    let (x0, y0) = page(p.texpage);
    let mut out = vec![grab(x0 + u16::from(p.u), y0, w0.min(0x100))];
    if p.texpage2 != 0 {
        let (x1, y1) = page(p.texpage2);
        let far = u16::from(p.u.wrapping_add(p.w0 as u8).wrapping_add(0x10)) + 1;
        out.push(grab(x1, y1, far));
    }
    out
}

/// The scene model bank's pool base (`*(u16*)0x8007B6F8`): a field actor's
/// `+0x64` is this plus its scene-bank model id (`FUN_8003A1E4`,
/// `FUN_80024E08`).
pub(super) const MODEL_BANK_BASE: u32 = 0x8007_B6F8;

/// Each bind record's `(record +0x50, scene-bank model id)`, off the
/// **first** field actor in list order that carries it: the one a motion
/// stream is bound to (`FUN_8003A9D4`, [`legaia_engine_scene::field_env::stream_bound_draws`]),
/// whose `+0x64` less the bank base is the model the stream's op `0x0E` last
/// swapped in.
pub fn retail_object_models(ram: &[u8]) -> Vec<(u16, i16)> {
    let base = i32::from(game_anchors::u16_at(ram, MODEL_BANK_BASE));
    let mut seen = std::collections::BTreeSet::new();
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| game_anchors::u32_at(ram, n + 0x0C) == 0x8003_BC08)
        .filter_map(|n| {
            let record = game_anchors::u16_at(ram, n + 0x50);
            if !seen.insert(record) {
                return None;
            }
            let id = i32::from(game_anchors::i16_at(ram, n + 0x64)) - base;
            (0..0xF0).contains(&id).then_some((record, id as i16))
        })
        .collect()
}

/// Every field actor with a clip bound, first in list order per record
/// (as [`retail_object_models`] takes them). A placed object's clip is touch
/// and walk history - a door the player has just pushed open, a lid mid
/// swing - which a seat does not replay; the image child writes the cursor
/// and control word over the matching prop's own on the frame it captures
/// (`World::seed_object_prop_clip`, which leaves a prop on another clip
/// alone). The cursor is the **displayed** frame's: the RAM's taken back by
/// the display lag's game frames ([`rewind_clip_cursor`]).
pub fn retail_object_clips(ram: &[u8]) -> Vec<ObjectClipSeed> {
    let step = crate::retail_compare_battle::frame_step(ram).max(1);
    let lag_frames = crate::retail_compare_battle::display_lag_vsyncs(ram) / u16::from(step);
    let mut seen = std::collections::BTreeSet::new();
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| game_anchors::u32_at(ram, n + 0x0C) == 0x8003_BC08)
        .filter_map(|n| {
            let record = game_anchors::u16_at(ram, n + 0x50);
            if !seen.insert(record) {
                return None;
            }
            let clip = game_anchors::u16_at(ram, n + 0x5C);
            let flags = game_anchors::u16_at(ram, n + 0x62);
            let rate = game_anchors::i16_at(ram, n + 0x6A);
            // The bound record's header (`+0x4C`): frame count at `+2`, the
            // blend gate at `+1` bit 0 and its divisor at `+6`.
            let rec = game_anchors::u32_at(ram, n + 0x4C);
            let frames = game_anchors::u16_at(ram, rec + 2);
            let gated = game_anchors::u8_at(ram, rec + 1) & 1 != 0;
            let div = game_anchors::u8_at(ram, rec + 6);
            let cursor = rewind_clip_cursor(
                game_anchors::i16_at(ram, n + 0x68),
                flags,
                frames,
                clip_display_lag_units(
                    legaia_engine_vm::actor_tick::clip_step(rate as u16, gated, div),
                    step,
                    lag_frames,
                ),
            );
            (1..0x100).contains(&clip).then_some(ObjectClipSeed {
                record,
                clip: clip as u8,
                cursor,
                flags,
                rate,
            })
        })
        .collect()
}

/// How far a clip cursor moved over the display lag: one anim tick
/// (`FUN_800204F8`) adds the clip step times the frame-step scalar
/// `DAT_1F800393` (`mult a0,v0` at `0x80020660` / `0x80020680`), so each of
/// the `lag_frames` game frames moves it `clip_step * frame_step` units.
/// `nilboa`'s Noa runs a gated clip (divisor `4`, step `4`) on a step-`2`
/// frame: `8` a game frame, `16` over the two-frame lag.
pub fn clip_display_lag_units(clip_step: u16, frame_step: u8, lag_frames: u16) -> u16 {
    clip_step
        .saturating_mul(u16::from(frame_step))
        .saturating_mul(lag_frames)
}

/// A clip cursor `back` units before the capture, as the anim tick
/// (`FUN_800204F8`) would have stood it: a held clip (`+0x62 & 2`) does not
/// move; otherwise it steps against its direction (`& 0x80` reverses), and
/// a cursor taken past the start clamps to it for a one-shot clip
/// (`& 8`) and wraps by the clip's span (`frames * 16`) for a looping one.
/// `nilboa`'s Noa loops her clip `45` (eight frames at `8` a game frame) and
/// the state holds cursor `48` on a step-`2` frame: the displayed frame is
/// two game frames back, at `32`, and her arms are down there where the
/// RAM's `48` has them raised.
///
/// A one-shot clip standing on the limit it clamps to (`frames * 16 - 1`
/// forward, `0` reversed) with the end latch `0x100` up is parked there and
/// is not taken back. The tick clears the latch and raises it again on every
/// pass that finds the cursor at the limit (`0x80020638`, `0x800206D4` /
/// `0x80020728`), so the state cannot say how long ago the clip arrived; a
/// one-shot spends every tick after its swing on that limit, which makes
/// "parked" the reading the frame bears out. `mei_house_door_pcsx`'s door
/// (`town01` record 1, clip 1, cursor `479` of a 30-frame clip) shows the
/// open leaf of the end pose, not the leaf two game frames short of it.
pub fn rewind_clip_cursor(cursor: i16, flags: u16, frames: u16, back: u16) -> i16 {
    use legaia_engine_scene::field_env::{ANIM_CLAMP, ANIM_END, ANIM_HOLD, ANIM_REVERSE};
    if flags & ANIM_HOLD != 0 || back == 0 || frames == 0 {
        return cursor;
    }
    let span = i32::from(frames) * 16;
    let limit = if flags & ANIM_REVERSE == 0 {
        span - 1
    } else {
        0
    };
    if flags & ANIM_CLAMP != 0 && flags & ANIM_END != 0 && i32::from(cursor) == limit {
        return cursor;
    }
    let back = i32::from(back);
    let c = i32::from(cursor);
    let c = if flags & ANIM_REVERSE == 0 {
        c - back
    } else {
        c + back
    };
    let c = if flags & ANIM_CLAMP != 0 {
        c.clamp(0, span - 1)
    } else {
        c.rem_euclid(span)
    };
    c as i16
}

/// Every placement the ambient motion VM is walking or turning
/// ([`retail_ambient_heading`]), at its **displayed** seat - the `rand()`
/// history the facing channel does not score, which the image child stands
/// where retail's frame shows it (`World::seed_ambient_walker`).
///
/// The RAM's `+0x14` / `+0x18` is the newest tick's; the frame on the TV is
/// [`DISPLAY_LAG_FRAMES`] older ([`walker_displayed_seat`]).
pub fn retail_walkers(ram: &[u8]) -> Vec<WalkerSeed> {
    let player = game_anchors::player_ptr(ram);
    let mut seen = std::collections::BTreeSet::new();
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| Some(n) != player && game_anchors::u32_at(ram, n + 0x0C) == 0x8003_BC08)
        .filter(|&n| retail_stream_turns(ram, n))
        .filter_map(|n| {
            let flat = game_anchors::u16_at(ram, n + 0x50);
            let (x, z) = walker_displayed_seat(
                (
                    game_anchors::i16_at(ram, n + 0x14),
                    game_anchors::i16_at(ram, n + 0x18),
                ),
                (
                    game_anchors::i16_at(ram, n + 0x1C),
                    game_anchors::i16_at(ram, n + 0x20),
                ),
                DISPLAY_LAG_FRAMES,
            );
            seen.insert(flat).then(|| WalkerSeed {
                flat,
                x,
                z,
                heading: game_anchors::u16_at(ram, n + 0x26) & 0x0FFF,
            })
        })
        .collect()
}

/// Where a walker stood `lag` game frames before the RAM's tick.
///
/// The actor-list tick iterator `FUN_8002519C` copies each node's `+0x14`
/// word (X and Y) to `+0x1C` and its `+0x18` (Z) to `+0x20` before it calls
/// the node's tick (`0x800251D4..0x800251F0`), so `+0x1C` / `+0x20` is where
/// the walker stood when this tick began - the seat the previous frame drew
/// it at - and `pos - prev` is one tick's step. The frame before that is
/// taken back by the same step, which holds for a walk at constant speed
/// and is the one extrapolation here. A walker standing still has
/// `prev == pos` and keeps its seat.
pub fn walker_displayed_seat(pos: (i16, i16), prev: (i16, i16), lag: i32) -> (i16, i16) {
    let back = |p: i16, q: i16| {
        let step = i32::from(p) - i32::from(q);
        (i32::from(p) - step * lag).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
    };
    (back(pos.0, prev.0), back(pos.1, prev.1))
}

/// Every field-actor-ticked node whose envelope is up (`+0x10 & 0x1000`)
/// with armed lanes - where its morph stands is time since the arm
/// (`town01`'s shoreline tide), so the image child writes it over the
/// engine's on the frame it captures (`World::seed_field_morph`). The
/// weights are the **displayed** frame's: the RAM's taken back by the
/// display lag ([`rewind_morph_weights`]).
pub fn retail_morphs(ram: &[u8]) -> Vec<MorphSeed> {
    let step = crate::retail_compare_battle::frame_step(ram).max(1);
    let lag_frames = crate::retail_compare_battle::display_lag_vsyncs(ram) / u16::from(step);
    let mut seen = std::collections::BTreeSet::new();
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| game_anchors::u32_at(ram, n + 0x0C) == 0x8003_BC08)
        .filter(|&n| game_anchors::u32_at(ram, n + 0x10) & 0x1000 != 0)
        .filter_map(|n| {
            let flat = game_anchors::u16_at(ram, n + 0x50);
            let lanes = u32::from(game_anchors::u8_at(ram, n + 0x6C)).min(8);
            (lanes > 0 && seen.insert(flat)).then(|| {
                let mut weights: Vec<u16> = (0..lanes)
                    .map(|i| game_anchors::u16_at(ram, n + 0xA0 + i * 2))
                    .collect();
                let up: Vec<i16> = (0..lanes)
                    .map(|i| game_anchors::i16_at(ram, n + 0xB8 + i * 2))
                    .collect();
                let down: Vec<i16> = (0..lanes)
                    .map(|i| game_anchors::i16_at(ram, n + 0xC8 + i * 2))
                    .collect();
                let done_mask = game_anchors::u32_at(ram, n + 0x7C);
                let env = game_anchors::u16_at(ram, n + 0x62);
                rewind_morph_weights(&mut weights, &up, &down, done_mask, env, step, lag_frames);
                MorphSeed {
                    flat,
                    weights,
                    done_mask,
                    env,
                }
            })
        })
        .collect()
}

/// Take a morph envelope's lane weights back `frames` game frames of
/// `step` vsyncs each - the displayed frame's weights, not the RAM's.
///
/// The envelope (`FUN_80020740`, `legaia_engine_vm::move_buffer::envelope_tick`)
/// moves a lane that has not peaked up by its `+0xB8` velocity times the
/// frame step while the finishing bit (`done_mask` bit 31) is clear, and a
/// peaked lane down by its `+0xC8` velocity once it is set (unless HOLD
/// `0x0400` or FROZEN `0x8000` stops it). Run backwards over the lag, each
/// lane retraces its own ramp, clamped to `0 ..= 0x1000`; a phase change
/// inside the lag is not undone. Gated `jouine` (`cort_evolved_pre_battle`)
/// is parked six vsyncs ahead of its displayed frame with its flesh-wall
/// lanes rising `51` / `81` a vsync: seeded on the RAM's weights the wall
/// was drawn further swollen than the frame on the TV.
pub fn rewind_morph_weights(
    weights: &mut [u16],
    up: &[i16],
    down: &[i16],
    done_mask: u32,
    env: u16,
    step: u8,
    frames: u16,
) {
    const HOLD: u16 = 0x0400;
    const FROZEN: u16 = 0x8000;
    const PEAK: i32 = 0x1000;
    if env & FROZEN != 0 || frames == 0 {
        return;
    }
    let finishing = done_mask & 0x8000_0000 != 0;
    let n = weights.len();
    let per = i32::from(step) * i32::from(frames);
    for (lane, slot) in weights.iter_mut().enumerate() {
        let bit = 1u32 << (lane & 0x1F);
        let peaked = done_mask & bit != 0;
        let w = i32::from(*slot as i16);
        let rewound = if !peaked && !finishing {
            let ramping = lane == 0 || done_mask & (1u32 << ((lane - 1) & 0x1F)) != 0;
            if !ramping {
                continue;
            }
            w - i32::from(up.get(lane).copied().unwrap_or(0)) * per
        } else if finishing && peaked && env & HOLD == 0 {
            let next_drained = lane + 1 == n || done_mask & (1u32 << ((lane + 1) & 0x1F)) == 0;
            if !next_drained {
                continue;
            }
            w + i32::from(down.get(lane).copied().unwrap_or(0)) * per
        } else {
            continue;
        };
        *slot = rewound.clamp(0, PEAK) as u16;
    }
}

/// The fog pool pointer (`_DAT_8007B7E0`, [`legaia_engine_field::fog_particles`]).
pub(super) const FOG_POOL_PTR: u32 = 0x8007_B7E0;

/// Every live record of a retail state's fog pool: 80 `0x18`-byte records
/// from pool `+0xA4`, alive byte `+0x05`
/// ([`legaia_engine_field::fog_particles`] has the layout).
pub fn retail_fog(ram: &[u8]) -> Vec<legaia_engine_field::fog_particles::FogParticle> {
    let pool = game_anchors::u32_at(ram, FOG_POOL_PTR);
    if (pool & 0xFFE0_0000) != 0x8000_0000 {
        return Vec::new();
    }
    (0..legaia_engine_field::fog_particles::FOG_POOL_SLOTS as u32)
        .map(|i| pool + 0xA4 + i * 0x18)
        .filter(|&r| game_anchors::u8_at(ram, r + 5) != 0)
        .map(|r| legaia_engine_field::fog_particles::FogParticle {
            age: game_anchors::u16_at(ram, r),
            rate: game_anchors::u16_at(ram, r + 2),
            slot: game_anchors::u8_at(ram, r + 4),
            alive: true,
            vx: game_anchors::u8_at(ram, r + 6) as i8,
            vz: game_anchors::u8_at(ram, r + 7) as i8,
            x: game_anchors::u32_at(ram, r + 8) as i32,
            z: game_anchors::u32_at(ram, r + 0xC) as i32,
            y: game_anchors::i16_at(ram, r + 0x10),
            grey: game_anchors::u8_at(ram, r + 0x14),
        })
        .collect()
}

/// The stager bundle the ambient parts run out of (`_DAT_8007B8D0`); a
/// part's `+0x48` points at its record inside it.
pub(super) const STAGER_BUNDLE_PTR: u32 = 0x8007_B8D0;

/// Every live draw-kind-4 sprite-arm sheet on a retail state's actor lists,
/// in list order: a part ticked by `FUN_80021DF4` with `+0x56 == 4` and the
/// sprite-arm bit `+0x9E & 0x4000`, not halted (`+0x10 & 0x8`)
/// ([`legaia_engine_core::world::ambient::SpriteArmSeed`] names the fields).
pub fn retail_sprite_arms(ram: &[u8]) -> Vec<legaia_engine_core::world::ambient::SpriteArmSeed> {
    let base = game_anchors::u32_at(ram, STAGER_BUNDLE_PTR);
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| {
            game_anchors::u32_at(ram, n + 0x0C) == PART_TICK
                && game_anchors::u16_at(ram, n + 0x56) == 4
                && game_anchors::u16_at(ram, n + 0x9E) & 0x4000 != 0
                && game_anchors::u32_at(ram, n + 0x10) & 0x8 == 0
        })
        .filter_map(|n| {
            let rec = game_anchors::u32_at(ram, n + 0x48).checked_sub(base)?;
            Some(legaia_engine_core::world::ambient::SpriteArmSeed {
                record_off: rec,
                pos: [0x14, 0x16, 0x18].map(|o| game_anchors::i16_at(ram, n + o)),
                rot: [0x24, 0x26, 0x28].map(|o| game_anchors::i16_at(ram, n + o)),
                scale: game_anchors::u16_at(ram, n + 0x72),
                colour: game_anchors::u32_at(ram, n + 0x74),
                level: game_anchors::u16_at(ram, n + 0x78),
                pose: retail_pose_entries(ram, n),
            })
        })
        .collect()
}

/// A mode-6 node's packed pose (`+0x4C` block: part count byte at `+0`,
/// 8-byte entries from `+8`), or empty for any other node.
pub(super) fn retail_pose_entries(ram: &[u8], n: u32) -> Vec<[u8; 8]> {
    if game_anchors::i16_at(ram, n + 0x5A) != 6 {
        return Vec::new();
    }
    let block = game_anchors::u32_at(ram, n + 0x4C);
    if (block & 0xFFE0_0000) != 0x8000_0000 {
        return Vec::new();
    }
    let count = u32::from(game_anchors::u8_at(ram, block));
    (0..count.min(32))
        .map(|i| std::array::from_fn(|b| game_anchors::u8_at(ram, block + 8 + i * 8 + b as u32)))
        .collect()
}

/// Every live mode-3 CLUT-cell cycler on a retail state's actor lists, in
/// list order: a part ticked by `FUN_80021DF4` with render mode `+0x5A = 3`
/// past its first armed frame (`+0x9C > 1`), as the snapshot its next
/// `FUN_80019D50` write uses - rect `+0xA0..+0xA6`, adds `+0x90/92/94`, mode
/// `+0x9E`, white amount `+0x68` ([`legaia_engine_field::clut_cell_fx`]).
pub fn retail_cell_fx(ram: &[u8]) -> Vec<legaia_engine_field::clut_cell_fx::ClutCellFx> {
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| {
            game_anchors::u32_at(ram, n + 0x0C) == PART_TICK
                && game_anchors::i16_at(ram, n + 0x5A) == 3
                && game_anchors::i16_at(ram, n + 0x9C) > 1
        })
        .map(|n| legaia_engine_field::clut_cell_fx::ClutCellFx {
            rect: (
                game_anchors::u16_at(ram, n + 0xA0),
                game_anchors::u16_at(ram, n + 0xA2),
                game_anchors::u16_at(ram, n + 0xA4),
                game_anchors::u16_at(ram, n + 0xA6),
            ),
            h_add: game_anchors::i16_at(ram, n + 0x90),
            s_add: game_anchors::i16_at(ram, n + 0x92),
            v_add: game_anchors::i16_at(ram, n + 0x94),
            mode: game_anchors::i16_at(ram, n + 0x9E),
            white: game_anchors::i16_at(ram, n + 0x68),
        })
        .collect()
}

/// The GPU packet buffers' bases, `DAT_8007B728[2]`, indexed by the buffer
/// index `gp+0x434` (`FUN_80016444` prints the bytes used as
/// `_DAT_1F8003A0 - DAT_8007B728[gp+0x434]`, `0x80016890..0x800168B4`).
const PACKET_BUFFER_BASES: u32 = 0x8007_B728;
/// The buffer index `gp+0x434` (`gp = 0x8007B318`): the buffer the frame
/// last built allocated from.
const PACKET_BUFFER_INDEX: u32 = 0x8007_B74C;
/// The packet buffer's size `_DAT_8007B9CC` (the bound the same print
/// subtracts the used bytes from).
const PACKET_BUFFER_SIZE: u32 = 0x8007_B9CC;

/// The CLUT-cell texels of the **older** frame a state still holds, one rect
/// per live mode-3 cycler ([`retail_cell_fx`]): one game frame newer than
/// the frame on screen, and the nearest record of it left in RAM.
///
/// `FUN_80019D50` does not write VRAM directly: it builds the repacked row as
/// a `[x, y, w, h]` rect followed by the texels at the packet cursor
/// `*(0x1F8003A0)`, bumps the cursor past them (`0x80019DA0..0x80019DD4`)
/// and hands the block to `LoadImage` (`0x8001A004..0x8001A030`). The cursor
/// allocates from the frame's own packet buffer, so the two built frames a
/// state holds keep their cell uploads beside their primitives: the newest
/// in buffer `gp+0x434`, which VRAM matches, and the one before it in the
/// other buffer, the first copy of each rect from that buffer's base (that
/// frame allocated contiguously from there and overwrote anything older
/// below its end). The frame on screen is one older again - the summon law
/// of [`crate::retail_compare_battle::display_lag_vsyncs`] - and its buffer
/// has been reused, so its own uploads are gone.
///
/// The RAM's adds are the newest frame's or, where the move program re-keys
/// them after the upload, the next one's: `koin3`'s dance-floor and cone rows
/// re-key `+0x90..+0x94` between frames, so seeding the RAM's adds draws a
/// phase two or three frames from the one on screen. A cycler is taken only
/// when it had armed (`+0x9C > 1`) by the older frame.
pub fn retail_older_buffer_cell_rects(ram: &[u8]) -> Vec<SeededVramRect> {
    let index = game_anchors::i16_at(ram, PACKET_BUFFER_INDEX);
    if !(0..=1).contains(&index) {
        return Vec::new();
    }
    let base = game_anchors::u32_at(ram, PACKET_BUFFER_BASES + 4 * (index as u32 ^ 1));
    let size = game_anchors::u32_at(ram, PACKET_BUFFER_SIZE).min(0x8_0000);
    if !(0x8000_0000..0x8020_0000).contains(&base) {
        return Vec::new();
    }
    let lo = (base & 0x1F_FFFF) as usize;
    let hi = (lo + size as usize).min(ram.len());
    let window = &ram[lo.min(hi)..hi];
    let mut out: Vec<SeededVramRect> = Vec::new();
    for n in crate::retail_compare_script::actor_nodes(ram) {
        if game_anchors::u32_at(ram, n + 0x0C) != PART_TICK
            || game_anchors::i16_at(ram, n + 0x5A) != 3
            || i32::from(game_anchors::i16_at(ram, n + 0x9C)) <= 1 + DISPLAY_LAG_FRAMES
        {
            continue;
        }
        let rect = (
            game_anchors::u16_at(ram, n + 0xA0),
            game_anchors::u16_at(ram, n + 0xA2),
            game_anchors::u16_at(ram, n + 0xA4),
            game_anchors::u16_at(ram, n + 0xA6),
        );
        let (_, _, w, h) = rect;
        if w == 0 || h == 0 || w > 256 || h > 64 || out.iter().any(|(r, _)| *r == rect) {
            continue;
        }
        let header: Vec<u8> = [rect.0, rect.1, rect.2, rect.3]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let count = usize::from(w) * usize::from(h);
        let Some(at) = window
            .windows(header.len())
            .enumerate()
            .find(|(i, win)| i % 2 == 0 && *win == header.as_slice())
            .map(|(i, _)| i)
        else {
            continue;
        };
        let Some(bytes) = window.get(at + 8..at + 8 + count * 2) else {
            continue;
        };
        let texels = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        out.push((rect, texels));
    }
    out
}

/// A menu-class capture the seed can reproduce: a pause-menu screen, named
/// by the root row whose confirm opens it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailMenu {
    /// `DAT_801E46A4`.
    pub subscreen: u8,
    /// The root row whose confirm reaches it.
    pub row: legaia_engine_menus::field_menu::FieldMenuRow,
    /// For an Equip-row screen, how many confirms past the root row it sits
    /// (`0` character picker, `1` slot browse, `2` candidate list).
    pub equip_depth: u8,
}

impl RetailMenu {
    /// Classify a menu-class capture's sub-screen id. The pause menu runs
    /// over a walkable scene with the id set; the title / boot family (the
    /// attract loop, the title picker, the card-boot save select) holds it
    /// clear, and a script-entered screen (the casino prize exchange,
    /// `0x20`) is no root row's.
    pub(super) fn from_ram(ram: &[u8], scene: &str) -> std::result::Result<Self, String> {
        use legaia_engine_menus::field_menu::FieldMenuRow;
        let subscreen = game_anchors::u8_at(ram, MENU_SUBSCREEN);
        if subscreen == 0 {
            return Err(format!(
                "title / boot screen on {scene} (sub-screen 0x00); no title seeding path"
            ));
        }
        let (row, equip_depth) = match subscreen {
            MENU_EQUIP_PICK => (FieldMenuRow::Equip, 0),
            MENU_EQUIP_SLOTS => (FieldMenuRow::Equip, 1),
            MENU_EQUIP_CANDIDATES => (FieldMenuRow::Equip, 2),
            _ => {
                let row = FieldMenuRow::from_retail_subscreen(subscreen).ok_or_else(|| {
                    format!("sub-screen 0x{subscreen:02X} is no pause-menu row's (script-entered)")
                })?;
                (row, 0)
            }
        };
        Ok(Self {
            subscreen,
            row,
            equip_depth,
        })
    }
}

/// Prefix of the reason a menu-class state carries when it is not a
/// pause-menu screen the seed can drive to.
pub const MENU_NOT_SEEDABLE: &str = "menu not seedable: ";

/// The two staging descriptors the composer `FUN_801DAB90` writes: the
/// follow ease's (`FUN_801DB510`) and op `0x45` APPLY's.
pub(super) const CAMERA_STAGINGS: [u32; 2] = [0x801F_3580, 0x801C_6EA8];

/// Whether retail's camera has composed from `block` since it was loaded:
/// a staging descriptor carries the block's `H` (`+0x26`, copied in every
/// mode). A loader that ran after the last compose - a walk-on band the
/// player arrived on and has not moved from (`kor5_field_card_boot`
/// stands on `P2[0]`'s tile) - leaves the live camera on the previous
/// block, which the seat's tile re-query reproduces.
pub(super) fn camera_block_composed(
    ram: &[u8],
    block: &legaia_engine_field::camera_zone::CameraZoneConfig,
) -> bool {
    CAMERA_STAGINGS
        .iter()
        .any(|&st| i32::from(rd16(ram, st + 0x26)) == block.h)
}

/// The 40 bytes from [`CAMERA_BLOCK`].
pub(super) fn camera_block_bytes(ram: &[u8]) -> Option<Vec<u8>> {
    let lo = (CAMERA_BLOCK & 0x1F_FFFF) as usize;
    ram.get(lo..lo + 0x28).map(<[u8]>::to_vec)
}

pub(super) fn rd16(ram: &[u8], va: u32) -> i16 {
    game_anchors::i16_at(ram, va)
}

pub(super) fn rd32(ram: &[u8], va: u32) -> i32 {
    game_anchors::u32_at(ram, va) as i32
}
