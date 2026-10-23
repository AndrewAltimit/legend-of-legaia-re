//! The battle effect pool's children as a capture's **displayed** frame drew
//! them ([`retail_effect_children`]).
//!
//! The pool (`_DAT_8007BD30`, walked by `FUN_801E0080`) holds the children
//! of the pass the RAM holds; the frame on screen is one or two passes
//! older, and which one is the double buffer's phase at the instant the state
//! was saved ([`display_phase`]). The live children are taken back
//! that many passes' sweeps ([`rewind_effect_child`]); a child that retired
//! since is recovered from its dead slot when the older ordering table still
//! draws it ([`retired_children`]).

use super::*;
use legaia_engine_core::world::EffectChildSeed;
use legaia_ram_image::extract::PSX_RAM_KSEG0;
use legaia_ram_image::prim_pool::{self, Prim};

/// `_DAT_8007BD30` - the effect pool's base pointer (`FUN_801E0080`).
const EFFECT_POOL_PTR: u32 = 0x8007_BD30;
/// The pool's child slots: `0x80` of `0x20` bytes from `+0x10`.
const EFFECT_CHILD_SLOTS: u32 = 0x80;
/// Most pack0 batches the pointer table at head `+0x08` is searched over.
const PACK0_SEARCH: u32 = 0x100;

/// One `0x2E` effect quad as an ordering table holds it: `(clut, tpage,
/// colour, corner UVs)`.
pub(super) type EffectQuad = (u16, u16, u8, [(u8, u8); 4]);

/// Every live effect-pool child of the state, as the displayed frame held
/// it ([`rewind_effect_child`]), in slot order, then the retired children
/// that frame still drew ([`retired_children`]). A child's anim cursor
/// `+0x1C` points into its pack0 batch (`entry + 2 + 6 * frame_cursor`);
/// the batch index the engine keys on is the entry's place in the pointer
/// table at the pool head's `+0x08`. A child whose batch is not found is
/// left out.
///
/// The children are taken back the displayed passes' own steps
/// ([`pass_steps`]), one sweep a vsync - not [`display_lag_vsyncs`], whose
/// [`frame_step`] reads a clamped history entry (`0x2BC`, a frame of
/// `0x2D0` hsyncs or more) as step `3` where the frame driver stepped `4`.
/// `gimard_burning_attack`'s history is the clamp sixteen times over, and its
/// film shows the walker running four sweeps a frame.
///
/// `None` when the state holds no pool to read; an empty list is a read
/// pool with no live child, which the image child seats too: a replay that
/// reached the capture behind retail's clock still carries billboards
/// retail's walker had already retired (`rim_elm_gimard_seru_capture_before`:
/// the replayed Gimard is still on its approach clip, whose eight dust
/// puffs retail had drawn out a dozen frames before).
pub(super) fn retail_effect_children(ram: &[u8]) -> Option<Vec<EffectChildSeed>> {
    let pool = Pool::read(ram)?;
    let steps = pass_steps(ram).map(|s| u16::from(s.max(1)));
    let tables = effect_tables(ram);
    let live: Vec<(EffectChildSeed, u32)> = (0..EFFECT_CHILD_SLOTS)
        .filter_map(|slot| pool.child(slot))
        .filter(|(seed, _)| seed.frame_count != 0)
        .collect();
    let phase = display_phase(ram);
    let (back, newest) = (phase.back, phase.newest);
    let lag = phase.lag_vsyncs();
    let mut out: Vec<EffectChildSeed> = live
        .iter()
        .filter_map(|&(seed, entry)| pool.rewind(seed, entry, lag))
        .collect();
    if let Some(newest) = newest {
        let older: Vec<EffectQuad> = tables
            .iter()
            .filter(|(start, _)| *start != newest)
            .flat_map(|(_, q)| q.iter().copied())
            .collect();
        out.extend(retired_children(&pool, &live, &older, steps, back));
    }
    Some(out)
}

/// The newest ordering table by the effect pool, for [`display_phase`]: the
/// one that draws more of the live children's current quads than of the
/// quads they drew a pass before (a child that held its frame across the
/// pass tells the tables nothing). `None` without a unique winner.
pub(super) fn newest_table_by_children(ram: &[u8]) -> Option<u32> {
    let pool = Pool::read(ram)?;
    let step = u16::from(pass_steps(ram)[0].max(1));
    let tables = effect_tables(ram);
    let live: Vec<(EffectChildSeed, u32)> = (0..EFFECT_CHILD_SLOTS)
        .filter_map(|slot| pool.child(slot))
        .filter(|(seed, _)| seed.frame_count != 0)
        .collect();
    let prev: Vec<Option<EffectQuad>> = live
        .iter()
        .map(|&(s, e)| {
            let then = pool.rewind(s, e, step)?;
            Some(pool.quad(e, then.frame_count, then.frame_cursor, then.mirror))
        })
        .collect();
    let scores: Vec<(i32, u32)> = tables
        .iter()
        .map(|(start, quads)| {
            let n: i32 = live
                .iter()
                .zip(&prev)
                .map(|(&(s, e), p)| {
                    let now = pool.quad(e, s.frame_count, s.frame_cursor, s.mirror);
                    if Some(now) == *p {
                        0
                    } else {
                        i32::from(quads.contains(&now))
                            - i32::from(p.is_some_and(|p| quads.contains(&p)))
                    }
                })
                .sum();
            (n, *start)
        })
        .collect();
    unique_best(&scores)
}

/// A dead pool slot the older table still draws, as the displayed frame held
/// it.
///
/// Retirement zeroes only the slot's frame count: the mirror, the position
/// and the batch survive, the anim cursor run past the batch end by the
/// retirement overrun (`FUN_801E0088` pass 1: the frame loop keeps striding
/// while the delay byte it lands on is zero, a motion step each), so the
/// batch is still `+0x1C - 2 - 6 * cursor`. The slot does not say when it
/// retired; the older table does. It is the pass before the RAM's, and a
/// child alive on it drew a `0x2E` quad of its frame's atlas rect, CLUT, page
/// and brightness in the slot's mirror order. A dead slot whose frame `k`
/// quad that table holds beyond what the live children account for on that
/// pass (each taken back the newest pass's step) is taken as on frame `k`
/// there, at the longest wait that still retires it within the newest pass;
/// it is then taken back to the displayed pass like the live ones.
///
/// `battle_gimard_tail_fire_a`'s slot 6 is one: a flame puff at
/// `(133, -233, -811)` the older table draws at `x 148..238` with brightness
/// `18` (its last frame), gone from the newer one; the displayed frame, a
/// pass further back, shows it on frame 6, the faint haze left of the flame.
/// A child retired and its slot already reseeded is gone from the RAM, and
/// is not recovered (`gimard_burning_attack`'s slots 6 and 7).
fn retired_children(
    pool: &Pool<'_>,
    live: &[(EffectChildSeed, u32)],
    older: &[EffectQuad],
    steps: [u16; 2],
    back: u8,
) -> Vec<EffectChildSeed> {
    let mut accounted: Vec<EffectQuad> = live
        .iter()
        .filter_map(|&(seed, entry)| {
            let then = pool.rewind(seed, entry, steps[0])?;
            Some(pool.quad(entry, then.frame_count, then.frame_cursor, then.mirror))
        })
        .collect();
    let mut out = Vec::new();
    for slot in 0..EFFECT_CHILD_SLOTS {
        let Some((dead, entry)) = pool.retired(slot) else {
            continue;
        };
        let total = dead.frame_count;
        // The latest frame the older table draws it on.
        let Some(k) = (0..total).rev().find(|&k| {
            let q = pool.quad(entry, total, k, dead.mirror);
            let drawn = older.iter().filter(|&&o| o == q).count();
            drawn > accounted.iter().filter(|&&o| o == q).count()
        }) else {
            continue;
        };
        accounted.push(pool.quad(entry, total, k, dead.mirror));
        let held = pool.held(entry, k);
        let Some(wait) = (0..=held)
            .rev()
            .step_by(8)
            .find(|&w| pool.retires_within(entry, total, k, w, steps[0]))
        else {
            continue;
        };
        let then = EffectChildSeed {
            frame_cursor: k,
            wait,
            ..dead
        };
        let shown = if back == 1 {
            Some(then)
        } else {
            pool.rewind(then, entry, steps[1])
        };
        out.extend(shown);
    }
    out
}

/// The pool head and the tables it reads through.
struct Pool<'a> {
    ram: &'a [u8],
    base: u32,
    table: u32,
    atlas: u32,
    motion_scale: i32,
}

impl<'a> Pool<'a> {
    fn read(ram: &'a [u8]) -> Option<Self> {
        let base = game_anchors::u32_at(ram, EFFECT_POOL_PTR);
        if !in_ram(base) {
            return None;
        }
        let table = game_anchors::u32_at(ram, base + 8);
        if !in_ram(table) {
            return None;
        }
        Some(Self {
            ram,
            base,
            table,
            atlas: game_anchors::u32_at(ram, base + 4),
            motion_scale: i32::from(game_anchors::i16_at(ram, base)),
        })
    }

    fn slot(&self, slot: u32) -> u32 {
        self.base + 0x10 + slot * 0x20
    }

    /// Byte `b` of `entry`'s frame `k` (`+0` atlas index, `+1` hold delay,
    /// `+2` motion speed).
    fn frame(&self, entry: u32, k: u8, b: u32) -> u8 {
        game_anchors::u8_at(self.ram, entry + 2 + 6 * u32::from(k) + b)
    }

    /// A frame's hold, `delay << 3` truncated to the wait byte.
    fn held(&self, entry: u32, k: u8) -> u8 {
        (u32::from(self.frame(entry, k, 1)) << 3) as u8
    }

    /// Slot `slot` as the RAM holds it, with its batch entry, live or dead;
    /// `None` for a slot with no batch behind it.
    fn child(&self, slot: u32) -> Option<(EffectChildSeed, u32)> {
        let c = self.slot(slot);
        let cursor = game_anchors::u8_at(self.ram, c + 2);
        let entry =
            game_anchors::u32_at(self.ram, c + 0x1C).wrapping_sub(2 + 6 * u32::from(cursor));
        let anim_id = (0..PACK0_SEARCH)
            .find(|&i| game_anchors::u32_at(self.ram, self.table + 4 * i) == entry)?;
        Some((
            EffectChildSeed {
                anim_id: anim_id as u16,
                frame_count: game_anchors::u8_at(self.ram, c),
                frame_cursor: cursor,
                wait: game_anchors::u8_at(self.ram, c + 3),
                mirror: game_anchors::u8_at(self.ram, c + 1),
                pos: [0x0C, 0x10, 0x14].map(|o| game_anchors::u32_at(self.ram, c + o) as i32),
                velocity: [4, 6, 8].map(|o| game_anchors::i16_at(self.ram, c + o)),
            },
            entry,
        ))
    }

    /// A dead slot with its batch, its frame count restored from the batch
    /// and its position taken back over the retirement overrun's motion
    /// steps (frames `total..=cursor`).
    fn retired(&self, slot: u32) -> Option<(EffectChildSeed, u32)> {
        let (mut seed, entry) = self.child(slot)?;
        let total = u8::try_from(game_anchors::u16_at(self.ram, entry)).ok()?;
        if seed.frame_count != 0 || total == 0 || seed.frame_cursor < total {
            return None;
        }
        for k in total..=seed.frame_cursor {
            let speed = i32::from(self.frame(entry, k, 2));
            for (p, v) in seed.pos.iter_mut().zip(seed.velocity) {
                *p = p.wrapping_sub(self.motion_step(v, speed));
            }
        }
        seed.frame_count = total;
        Some((seed, entry))
    }

    fn motion_step(&self, v: i16, speed: i32) -> i32 {
        i32::from(v)
            .wrapping_mul(speed)
            .wrapping_mul(self.motion_scale)
            .wrapping_shl(3)
            >> 15
    }

    /// The quad a child on `entry`'s frame `k` draws - `Pool::child_billboards`
    /// and pass 2's corner order. The atlas entries are 8 bytes from the
    /// head's `+0x04`.
    fn quad(&self, entry: u32, count: u8, k: u8, mirror: u8) -> EffectQuad {
        let a = self.atlas + 8 * u32::from(self.frame(entry, k, 0));
        let b = |o: u32| game_anchors::u8_at(self.ram, a + o);
        let (mut u0, mut u1) = (b(0), b(0).wrapping_add(b(2).saturating_sub(1)));
        let (mut v0, mut v1) = (b(1), b(1).wrapping_add(b(3).saturating_sub(1)));
        if mirror & 1 == 0 {
            std::mem::swap(&mut u0, &mut u1);
        }
        if mirror & 2 == 0 {
            std::mem::swap(&mut v0, &mut v1);
        }
        (
            game_anchors::u16_at(self.ram, a + 4),
            game_anchors::u16_at(self.ram, a + 6),
            legaia_engine_vm::effect_vm::pass2_brightness(count, k),
            [(u0, v0), (u1, v0), (u0, v1), (u1, v1)],
        )
    }

    /// Whether a child on frame `k` at `wait` retires within `sweeps` walker
    /// sweeps (pass 1's child tick: a running wait drains `8`, a spent one
    /// advances frames while each new hold is zero).
    fn retires_within(&self, entry: u32, total: u8, k: u8, wait: u8, sweeps: u16) -> bool {
        let (mut k, mut wait) = (k, wait);
        for _ in 0..sweeps {
            if wait >= 8 {
                wait -= 8;
                continue;
            }
            loop {
                k += 1;
                if k >= total {
                    return true;
                }
                wait = self.held(entry, k);
                if wait != 0 {
                    break;
                }
            }
        }
        false
    }

    fn rewind(&self, seed: EffectChildSeed, entry: u32, sweeps: u16) -> Option<EffectChildSeed> {
        rewind_effect_child(seed, sweeps, self.motion_scale, |k, b| {
            self.frame(entry, k, b)
        })
    }
}

/// Each ordering table's `0x2E` effect quads, keyed by the table's bucket
/// array start (the word the double-buffer record holds at `+0x70`).
fn effect_tables(ram: &[u8]) -> Vec<(u32, Vec<EffectQuad>)> {
    prim_pool::find_ot_arrays(ram, PSX_RAM_KSEG0, 64)
        .iter()
        .filter_map(|ot| {
            let head = ot.head.checked_sub(PSX_RAM_KSEG0)? as usize;
            let quads = prim_pool::chain_walk(ram, PSX_RAM_KSEG0, head)
                .into_iter()
                .filter_map(|c| match c.prim {
                    Prim::PolyFt4 {
                        cmd: 0x2E,
                        color,
                        uvs,
                        clut,
                        tpage,
                        ..
                    } if color[0] == color[1] && color[1] == color[2] => {
                        Some((clut, tpage, color[0], uvs))
                    }
                    _ => None,
                })
                .collect();
            Some((ot.start, quads))
        })
        .collect()
}

/// Child `seed` taken back `sweeps` walker sweeps - the display lag, since
/// `FUN_801E0080` sweeps once a vsync. `frame(k, b)` reads byte `b` of the
/// batch's frame `k` (`+1` hold delay, `+2` motion speed). `None` for a
/// child the walker seeded inside the window, which the displayed frame
/// does not hold yet.
///
/// The inverse of one sweep (`0x801E0088` pass 1): a child whose wait still
/// holds its frame's whole delay entered that frame on the sweep, so it
/// stood on the previous frame at wait `0`; any other took `8` off its wait.
/// Either way it took one motion step, `vel * speed * scale * 8 >> 15`, on
/// the frame it ended the sweep on. A child on frame `0` within `8` of its
/// first delay was seeded on the sweep (the seed's batch is walked in the
/// same sweep, so it starts one decrement down).
pub(super) fn rewind_effect_child(
    mut seed: EffectChildSeed,
    sweeps: u16,
    motion_scale: i32,
    frame: impl Fn(u8, u32) -> u8,
) -> Option<EffectChildSeed> {
    for _ in 0..sweeps {
        let speed = i32::from(frame(seed.frame_cursor, 2));
        for (p, v) in seed.pos.iter_mut().zip(seed.velocity) {
            let step = i32::from(v)
                .wrapping_mul(speed)
                .wrapping_mul(motion_scale)
                .wrapping_shl(3)
                >> 15;
            *p = p.wrapping_sub(step);
        }
        let held = (u32::from(frame(seed.frame_cursor, 1)) << 3) as u8;
        if seed.frame_cursor == 0 {
            if seed.wait >= held.saturating_sub(8) {
                return None;
            }
            seed.wait += 8;
        } else if seed.wait == held {
            seed.frame_cursor -= 1;
            seed.wait = 0;
        } else {
            seed.wait = seed.wait.saturating_add(8);
        }
    }
    Some(seed)
}
