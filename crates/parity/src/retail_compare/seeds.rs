//! Retail-side seed readers: the facts each comparison seeds the engine with,
//! read out of a state's RAM / VRAM (ambient headings and stream turns, the
//! clear colour, scroll rects and panels, object models, walkers, morph
//! weights, fog, CLUT cell FX, the pause menu and the camera block), each with
//! its environment hand-off. Split out of `retail_compare.rs`; no logic change.

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
    use legaia_asset::man_motion::op_width;
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
/// (`legaia_engine_core::cursor_sprite`), which counts from the first call
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
/// ([`legaia_engine_core::world::ambient`]'s `vram_scroll`) - with the
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
/// ([`legaia_engine_core::screen_fx::PanelWidget`]'s field map: current
/// `+0x14..+0x1A` / `+0x24`, targets `+0x3C..+0x42` / `+0x26`, base sizes
/// `+0xB8..+0xBC`, tween `+0x9C` / `+0x9E`, spawn size `+0xAA` / `+0xAC`,
/// texel origin `+0xA4` / `+0xA8`, pages `+0xA0` / `+0xA2`).
///
/// The ending vignettes spawn the panel from the vignette's own record
/// (`43 12` grabs the drawn frame into `(512, 0)`, `43 13` shows it), and a
/// capture is usually parked in the credits record that runs after it -
/// `ending_panel_corner` holds record 13, the panel already shrunk to the
/// corner - so a seed that resumes the running record never spawns it.
pub fn retail_panel(ram: &[u8]) -> Option<legaia_engine_core::screen_fx::PanelWidget> {
    let n = crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .find(|&n| game_anchors::u32_at(ram, n + 0x0C) == PANEL_TICK)?;
    let h = |o: u32| game_anchors::i16_at(ram, n + o);
    Some(legaia_engine_core::screen_fx::PanelWidget {
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
    p: &legaia_engine_core::screen_fx::PanelWidget,
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

/// [`retail_panel`] as `LEGAIA_SEAT_PANEL`: the widget's fields as
/// comma-separated integers in [`panel_from_env`]'s order.
pub fn panel_env(p: &legaia_engine_core::screen_fx::PanelWidget) -> String {
    let mut v: Vec<i32> = Vec::new();
    v.extend(p.cur.iter().map(|&x| i32::from(x)));
    v.extend(p.target.iter().map(|&x| i32::from(x)));
    v.extend(p.base.iter().map(|&x| i32::from(x)));
    v.extend([
        i32::from(p.t),
        i32::from(p.dur),
        i32::from(p.w0),
        i32::from(p.h0),
        i32::from(p.u),
        i32::from(p.v),
        i32::from(p.texpage),
        i32::from(p.texpage2),
    ]);
    v.iter()
        .map(|x| x.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Inverse of [`panel_env`].
pub fn panel_from_env(s: &str) -> Option<legaia_engine_core::screen_fx::PanelWidget> {
    let v: Vec<i32> = s
        .split(',')
        .map(|t| t.trim().parse().ok())
        .collect::<Option<_>>()?;
    if v.len() != 21 {
        return None;
    }
    let h = |i: usize| v[i] as i16;
    Some(legaia_engine_core::screen_fx::PanelWidget {
        cur: [h(0), h(1), h(2), h(3), h(4)],
        target: [h(5), h(6), h(7), h(8), h(9)],
        base: [h(10), h(11), h(12)],
        t: h(13),
        dur: h(14),
        w0: h(15),
        h0: h(16),
        u: v[17] as u8,
        v: v[18] as u8,
        texpage: v[19] as u16,
        texpage2: v[20] as u16,
    })
}

/// [`retail_scroll_rects`] as the bytes of a `LEGAIA_SEAT_VRAM_RECTS` file:
/// per rect `x, y, w, h` (`u16` LE) then its `w * h` texels.
pub fn vram_rects_file(rects: &[SeededVramRect]) -> Vec<u8> {
    let mut out = Vec::new();
    for ((x, y, w, h), texels) in rects {
        for v in [*x, *y, *w, *h].iter().chain(texels) {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

/// Inverse of [`vram_rects_file`]; a truncated tail is dropped.
pub fn vram_rects_from_file(bytes: &[u8]) -> Vec<SeededVramRect> {
    let words: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= words.len() {
        let (x, y, w, h) = (words[i], words[i + 1], words[i + 2], words[i + 3]);
        let n = usize::from(w) * usize::from(h);
        let Some(texels) = words.get(i + 4..i + 4 + n) else {
            break;
        };
        out.push(((x, y, w, h), texels.to_vec()));
        i += 4 + n;
    }
    out
}

/// The scene model bank's pool base (`*(u16*)0x8007B6F8`): a field actor's
/// `+0x64` is this plus its scene-bank model id (`FUN_8003A1E4`,
/// `FUN_80024E08`).
pub(super) const MODEL_BANK_BASE: u32 = 0x8007_B6F8;

/// Each bind record's `(record +0x50, scene-bank model id)`, off the
/// **first** field actor in list order that carries it: the one a motion
/// stream is bound to (`FUN_8003A9D4`, [`legaia_engine_core::field_env::stream_bound_draws`]),
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

/// One drawn field actor's clip words: record `+0x50`, clip id `+0x5C`,
/// cursor `+0x68`, control word `+0x62` and cursor step `+0x6A` - what the
/// anim tick `FUN_800204F8` steps and the draw walker poses from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectClipSeed {
    pub record: u16,
    pub clip: u8,
    pub cursor: i16,
    pub flags: u16,
    pub rate: i16,
}

/// Every field actor with a clip bound, first in list order per record
/// (as [`retail_object_models`] takes them). A placed object's clip is touch
/// and walk history - a door the player has just pushed open, a lid mid
/// swing - which a seat does not replay; the image child writes the cursor
/// and control word over the matching prop's own on the frame it captures
/// (`World::seed_object_prop_clip`, which leaves a prop on another clip
/// alone).
pub fn retail_object_clips(ram: &[u8]) -> Vec<ObjectClipSeed> {
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
            (1..0x100).contains(&clip).then(|| ObjectClipSeed {
                record,
                clip: clip as u8,
                cursor: game_anchors::i16_at(ram, n + 0x68),
                flags: game_anchors::u16_at(ram, n + 0x62),
                rate: game_anchors::i16_at(ram, n + 0x6A),
            })
        })
        .collect()
}

/// `LEGAIA_SEAT_OBJECT_CLIPS`: `record:clip:cursor:flags:rate`, `;`-joined.
pub fn object_clips_env(clips: &[ObjectClipSeed]) -> String {
    clips
        .iter()
        .map(|c| {
            format!(
                "{}:{}:{}:{}:{}",
                c.record, c.clip, c.cursor, c.flags, c.rate
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Inverse of [`object_clips_env`]; a malformed entry is dropped.
pub fn object_clips_from_env(v: &str) -> Vec<ObjectClipSeed> {
    v.split(';')
        .filter_map(|e| {
            let mut f = e.split(':').map(str::trim);
            Some(ObjectClipSeed {
                record: f.next()?.parse().ok()?,
                clip: f.next()?.parse().ok()?,
                cursor: f.next()?.parse().ok()?,
                flags: f.next()?.parse().ok()?,
                rate: f.next()?.parse().ok()?,
            })
        })
        .collect()
}

/// One ambient walker's live seat: flat MAN index `+0x50`, `+0x14` /
/// `+0x18`, and the retail-space heading `+0x26`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkerSeed {
    pub flat: u16,
    pub x: i16,
    pub z: i16,
    pub heading: u16,
}

/// Every placement the ambient motion VM is walking or turning
/// ([`retail_ambient_heading`]), at its live seat - the `rand()` history
/// the facing channel does not score, which the image child stands where
/// retail's frame shows it (`World::seed_ambient_walker`).
pub fn retail_walkers(ram: &[u8]) -> Vec<WalkerSeed> {
    let player = game_anchors::player_ptr(ram);
    let mut seen = std::collections::BTreeSet::new();
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| Some(n) != player && game_anchors::u32_at(ram, n + 0x0C) == 0x8003_BC08)
        .filter(|&n| retail_stream_turns(ram, n))
        .filter_map(|n| {
            let flat = game_anchors::u16_at(ram, n + 0x50);
            seen.insert(flat).then(|| WalkerSeed {
                flat,
                x: game_anchors::i16_at(ram, n + 0x14),
                z: game_anchors::i16_at(ram, n + 0x18),
                heading: game_anchors::u16_at(ram, n + 0x26) & 0x0FFF,
            })
        })
        .collect()
}

/// [`retail_walkers`] as `LEGAIA_SEAT_WALKERS`: `flat:x:z:heading` per
/// walker, `;`-joined, decimal.
pub fn walkers_env(w: &[WalkerSeed]) -> String {
    w.iter()
        .map(|s| format!("{}:{}:{}:{}", s.flat, s.x, s.z, s.heading))
        .collect::<Vec<_>>()
        .join(";")
}

/// Parse [`walkers_env`].
pub fn walkers_from_env(v: &str) -> Vec<WalkerSeed> {
    v.split(';')
        .filter_map(|e| {
            let mut f = e.trim().split(':');
            Some(WalkerSeed {
                flat: f.next()?.parse().ok()?,
                x: f.next()?.parse().ok()?,
                z: f.next()?.parse().ok()?,
                heading: f.next()?.parse().ok()?,
            })
        })
        .collect()
}

/// One field actor's live VDF morph envelope (op `0x4B`,
/// `legaia_engine_core::world::npc_morph`): its flat MAN index `+0x50`, the
/// lane weights `+0xA0 + i*2` over its `+0x6C` lanes, the lane-done mask
/// `+0x7C` and the envelope control word `+0x62`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorphSeed {
    pub flat: u16,
    pub weights: Vec<u16>,
    pub done_mask: u32,
    pub env: u16,
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

/// [`retail_morphs`] as `LEGAIA_SEAT_MORPHS`:
/// `flat:w0/w1/..:done:env` per actor, `;`-joined, numbers in hex.
pub fn morphs_env(m: &[MorphSeed]) -> String {
    m.iter()
        .map(|s| {
            format!(
                "{:x}:{}:{:x}:{:x}",
                s.flat,
                s.weights
                    .iter()
                    .map(|w| format!("{w:x}"))
                    .collect::<Vec<_>>()
                    .join("/"),
                s.done_mask,
                s.env
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Parse [`morphs_env`].
pub fn morphs_from_env(v: &str) -> Vec<MorphSeed> {
    v.split(';')
        .filter_map(|e| {
            let mut f = e.trim().split(':');
            let flat = u16::from_str_radix(f.next()?, 16).ok()?;
            let weights = f
                .next()?
                .split('/')
                .map(|w| u16::from_str_radix(w, 16).ok())
                .collect::<Option<Vec<u16>>>()?;
            let done_mask = u32::from_str_radix(f.next()?, 16).ok()?;
            let env = u16::from_str_radix(f.next()?, 16).ok()?;
            Some(MorphSeed {
                flat,
                weights,
                done_mask,
                env,
            })
        })
        .collect()
}

/// The fog pool pointer (`_DAT_8007B7E0`, [`legaia_engine_core::fog_particles`]).
pub(super) const FOG_POOL_PTR: u32 = 0x8007_B7E0;

/// Every live record of a retail state's fog pool: 80 `0x18`-byte records
/// from pool `+0xA4`, alive byte `+0x05`
/// ([`legaia_engine_core::fog_particles`] has the layout).
pub fn retail_fog(ram: &[u8]) -> Vec<legaia_engine_core::fog_particles::FogParticle> {
    let pool = game_anchors::u32_at(ram, FOG_POOL_PTR);
    if (pool & 0xFFE0_0000) != 0x8000_0000 {
        return Vec::new();
    }
    (0..legaia_engine_core::fog_particles::FOG_POOL_SLOTS as u32)
        .map(|i| pool + 0xA4 + i * 0x18)
        .filter(|&r| game_anchors::u8_at(ram, r + 5) != 0)
        .map(|r| legaia_engine_core::fog_particles::FogParticle {
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

/// [`retail_fog`] as `LEGAIA_SEAT_FOG`: `slot,age,rate,vx,vz,x,z,y,grey` per
/// record, `;`-separated.
pub fn fog_env(fog: &[legaia_engine_core::fog_particles::FogParticle]) -> String {
    fog.iter()
        .map(|p| {
            format!(
                "{},{},{},{},{},{},{},{},{}",
                p.slot, p.age, p.rate, p.vx, p.vz, p.x, p.z, p.y, p.grey
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Inverse of [`fog_env`]; malformed entries are dropped.
pub fn fog_from_env(s: &str) -> Vec<legaia_engine_core::fog_particles::FogParticle> {
    s.split(';')
        .filter_map(|e| {
            let v: Vec<i64> = e.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            let [slot, age, rate, vx, vz, x, z, y, grey] = <[i64; 9]>::try_from(v).ok()?;
            Some(legaia_engine_core::fog_particles::FogParticle {
                age: age as u16,
                rate: rate as u16,
                slot: slot as u8,
                alive: true,
                vx: vx as i8,
                vz: vz as i8,
                x: x as i32,
                z: z as i32,
                y: y as i16,
                grey: grey as u8,
            })
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

/// [`retail_sprite_arms`] as `LEGAIA_SEAT_SPRITE_ARMS`:
/// `record,x,y,z,r24,r26,r28,scale,colour,level` per sheet, `;`-separated.
pub fn sprite_arms_env(s: &[legaia_engine_core::world::ambient::SpriteArmSeed]) -> String {
    s.iter()
        .map(|a| {
            let pose: String = a
                .pose
                .iter()
                .flat_map(|e| e.iter().map(|b| format!("{b:02x}")))
                .collect();
            format!(
                "{},{},{},{},{},{},{},{},{},{},{pose}",
                a.record_off,
                a.pos[0],
                a.pos[1],
                a.pos[2],
                a.rot[0],
                a.rot[1],
                a.rot[2],
                a.scale,
                a.colour,
                a.level
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Inverse of [`sprite_arms_env`]; malformed entries are dropped.
pub fn sprite_arms_from_env(s: &str) -> Vec<legaia_engine_core::world::ambient::SpriteArmSeed> {
    s.split(';')
        .filter_map(|e| {
            let mut fields: Vec<&str> = e.split(',').collect();
            let pose_hex = if fields.len() == 11 {
                fields.pop()?
            } else {
                ""
            };
            let v: Vec<i64> = fields
                .iter()
                .filter_map(|x| x.trim().parse().ok())
                .collect();
            let [rec, x, y, z, r0, r1, r2, scale, colour, level] = <[i64; 10]>::try_from(v).ok()?;
            let bytes: Vec<u8> = (0..pose_hex.len() / 2)
                .filter_map(|i| u8::from_str_radix(pose_hex.get(i * 2..i * 2 + 2)?, 16).ok())
                .collect();
            Some(legaia_engine_core::world::ambient::SpriteArmSeed {
                record_off: rec as u32,
                pos: [x as i16, y as i16, z as i16],
                rot: [r0 as i16, r1 as i16, r2 as i16],
                scale: scale as u16,
                colour: colour as u32,
                level: level as u16,
                pose: bytes.as_chunks::<8>().0.to_vec(),
            })
        })
        .collect()
}

/// Every live mode-3 CLUT-cell cycler on a retail state's actor lists, in
/// list order: a part ticked by `FUN_80021DF4` with render mode `+0x5A = 3`
/// past its first armed frame (`+0x9C > 1`), as the snapshot its next
/// `FUN_80019D50` write uses - rect `+0xA0..+0xA6`, adds `+0x90/92/94`, mode
/// `+0x9E`, white amount `+0x68` ([`legaia_engine_core::clut_cell_fx`]).
pub fn retail_cell_fx(ram: &[u8]) -> Vec<legaia_engine_core::clut_cell_fx::ClutCellFx> {
    crate::retail_compare_script::actor_nodes(ram)
        .into_iter()
        .filter(|&n| {
            game_anchors::u32_at(ram, n + 0x0C) == PART_TICK
                && game_anchors::i16_at(ram, n + 0x5A) == 3
                && game_anchors::i16_at(ram, n + 0x9C) > 1
        })
        .map(|n| legaia_engine_core::clut_cell_fx::ClutCellFx {
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

/// [`retail_cell_fx`] as `LEGAIA_SEAT_CLUT_FX`: `x,y,w,h,h,s,v,mode,white`
/// per part, `;`-separated.
pub fn cell_fx_env(fx: &[legaia_engine_core::clut_cell_fx::ClutCellFx]) -> String {
    fx.iter()
        .map(|f| {
            format!(
                "{},{},{},{},{},{},{},{},{}",
                f.rect.0, f.rect.1, f.rect.2, f.rect.3, f.h_add, f.s_add, f.v_add, f.mode, f.white
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Inverse of [`cell_fx_env`]; malformed entries are dropped.
pub fn cell_fx_from_env(s: &str) -> Vec<legaia_engine_core::clut_cell_fx::ClutCellFx> {
    s.split(';')
        .filter_map(|e| {
            let v: Vec<i32> = e.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            let [x, y, w, h, hh, ss, vv, mode, white] = <[i32; 9]>::try_from(v).ok()?;
            Some(legaia_engine_core::clut_cell_fx::ClutCellFx {
                rect: (x as u16, y as u16, w as u16, h as u16),
                h_add: hh as i16,
                s_add: ss as i16,
                v_add: vv as i16,
                mode: mode as i16,
                white: white as i16,
            })
        })
        .collect()
}

/// A menu-class capture the seed can reproduce: a pause-menu screen, named
/// by the root row whose confirm opens it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailMenu {
    /// `DAT_801E46A4`.
    pub subscreen: u8,
    /// The root row whose confirm reaches it.
    pub row: legaia_engine_core::field_menu::FieldMenuRow,
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
        use legaia_engine_core::field_menu::FieldMenuRow;
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
    block: &legaia_engine_core::camera_zone::CameraZoneConfig,
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

/// `LEGAIA_SEAT_CAMERA_BLOCK` for `play-window`: the block's 40 bytes from
/// [`CAMERA_BLOCK`], hex.
pub fn camera_block_env(block: &legaia_engine_core::camera_zone::CameraZoneConfig) -> String {
    block
        .to_retail_block()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Inverse of [`camera_block_env`].
pub fn camera_block_from_env(s: &str) -> Option<legaia_engine_core::camera_zone::CameraZoneConfig> {
    let s = s.trim();
    if s.len() != 0x50 {
        return None;
    }
    let bytes = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    legaia_engine_core::camera_zone::CameraZoneConfig::from_retail_block(&bytes)
}

pub(super) fn rd16(ram: &[u8], va: u32) -> i16 {
    game_anchors::i16_at(ram, va)
}

pub(super) fn rd32(ram: &[u8], va: u32) -> i32 {
    game_anchors::u32_at(ram, va) as i32
}
