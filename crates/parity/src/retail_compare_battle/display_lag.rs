//! Which frame a capture's screen shows: the double buffer's phase at the
//! instant the state was saved ([`display_phase`]), and the vsyncs that
//! frame lags the RAM by ([`displayed_lag_vsyncs`]).
//!
//! It is a **draw-tick** phase. The render dispatcher runs the tick pass
//! (anim ticks, camera walker, module ticks, HUD glides) before the draw
//! tick, and an after-flip state is saved inside the next pass's draw: its
//! logic words already belong to that pass and stand two passes past the
//! displayed frame like a mid-pass capture's. Only state the draw tick
//! writes - the effect pool - is one pass short, so only the effect
//! children read this; every logic consumer keeps the two-pass law
//! (`display_lag_vsyncs`). Routing them all through it dropped
//! `gimard_burning_attack` from `.872` to `.712`.
//!
//! Retail keeps two records from `0x8007BF30` (`0x74` bytes each,
//! `FUN_8001DAF8` builds them): a display environment at `+0x00`, a draw
//! environment at `+0x14` and its ordering table's bucket array at `+0x70`.
//! Record 0 draws into the `y = 4` buffer and displays `y = 244`, record 1
//! the other way round. The frame-end flip (`0x800171E4..0x80017288` in
//! `FUN_80016B6C`) puts the current record (`gp+0x434`), flips the index
//! and hands the GPU the table just built, and the display scans out the
//! buffer the GPU drew the pass before.
//!
//! A state saved while the CPU still builds (or has built and waits on) its
//! pass has that pass's table under the current record: the screen shows
//! the pass two back - the summon flashes the double-buffer law was measured
//! on, and `battle_gimard_tail_fire_a`. A state saved after the flip has
//! the newest table under the other record, the GPU drawing it: the screen
//! shows the pass just before it, the older table itself. A PCSX-Redux film
//! of `gimard_burning_attack`'s cast (`autorun_effect_pool_film.lua`)
//! reproduces the capture's pool and displayed frame exactly, and the frame
//! on screen drew the previous pass.
//!
//! Which table is newest is read off two kinds of evidence, either one a
//! table that draws the RAM's state and not the pass before: the effect
//! pool's live children on their current frames
//! ([`newest_table_by_children`]), or the battle floor under the RAM's live
//! camera ([`newest_table_by_ground`]). A capture with neither (no live
//! effect, no floor, or a camera that held still across the pass) keeps the
//! two-pass law.

use super::*;

/// The double buffer's records, their stride, and the bucket-array word.
const DOUBLE_BUFFER: u32 = 0x8007_BF30;
const DOUBLE_BUFFER_STRIDE: u32 = 0x74;
const DOUBLE_BUFFER_OT: u32 = 0x70;
/// The current record's index (`gp+0x434`).
const DOUBLE_BUFFER_INDEX: u32 = 0x8007_B74C;

/// What told the phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseEvidence {
    /// The effect pool's live children.
    EffectChildren,
    /// The battle floor under the live camera.
    Floor,
    /// Nothing: the two-pass law.
    Default,
}

/// The displayed frame's place behind the RAM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayPhase {
    /// Passes the displayed frame is older than the RAM's: `1` or `2`.
    pub back: u8,
    /// The newest ordering table's bucket-array start, when known.
    pub newest: Option<u32>,
    /// The steps of the RAM's pass and the one before ([`pass_steps`]).
    pub steps: [u16; 2],
    /// What told the phase.
    pub evidence: PhaseEvidence,
}

impl DisplayPhase {
    /// Vsyncs the displayed frame lags the RAM by: the displayed passes' own
    /// steps.
    pub fn lag_vsyncs(&self) -> u16 {
        self.steps.iter().take(usize::from(self.back)).sum()
    }
}

/// The double buffer's phase at the capture instant ([`DisplayPhase`]).
pub fn display_phase(ram: &[u8]) -> DisplayPhase {
    let steps = pass_steps(ram).map(|s| u16::from(s.max(1)));
    let ctx = game_anchors::u32_at(ram, BATTLE_CTX);
    let (newest, evidence) = if let Some(t) = newest_table_by_children(ram) {
        (Some(t), PhaseEvidence::EffectChildren)
    } else if let Some(t) = in_ram(ctx)
        .then(|| newest_table_by_ground(ram, ctx))
        .flatten()
    {
        (Some(t), PhaseEvidence::Floor)
    } else {
        (None, PhaseEvidence::Default)
    };
    DisplayPhase {
        back: newest.map_or(2, |t| passes_back_for(ram, t)),
        newest,
        steps,
        evidence,
    }
}

/// `1` when the newest table `newest` sits under the record the frame-end
/// flip already moved off (the screen shows the pass just before it), `2`
/// when it sits under the current record.
pub fn passes_back_for(ram: &[u8], newest: u32) -> u8 {
    let cur = u32::from(game_anchors::u16_at(ram, DOUBLE_BUFFER_INDEX) & 1);
    let ot = |i: u32| {
        game_anchors::u32_at(
            ram,
            DOUBLE_BUFFER + i * DOUBLE_BUFFER_STRIDE + DOUBLE_BUFFER_OT,
        )
    };
    if newest == ot(cur ^ 1) { 1 } else { 2 }
}

/// The OT bucket a screen fade links at: `FUN_80024EE4` adds its quad at
/// `ot + id * 4`, and the summon fades carry id `1`.
const FADE_BUCKET: u32 = 1;
/// Buckets in a battle ordering table; the chain starts at the top one.
const OT_BUCKETS: u32 = 0x1000;

/// Whether record `i`'s ordering table links a flat semi-transparent quad
/// (`POLY_F4` `0x2A` / `0x2B`, the fade's packet) in [`FADE_BUCKET`]: the
/// bucket's chain, followed until it re-enters the bucket array.
fn record_links_fade(ram: &[u8], i: u32) -> bool {
    let ot = game_anchors::u32_at(
        ram,
        DOUBLE_BUFFER + i * DOUBLE_BUFFER_STRIDE + DOUBLE_BUFFER_OT,
    );
    let buckets = ot..ot.wrapping_add(OT_BUCKETS * 4);
    if !in_ram(ot) {
        return false;
    }
    let mut p = (game_anchors::u32_at(ram, ot + FADE_BUCKET * 4) & 0x00FF_FFFF) | 0x8000_0000;
    for _ in 0..64 {
        if buckets.contains(&p) || !in_ram(p) {
            return false;
        }
        let tag = game_anchors::u32_at(ram, p);
        if tag >> 24 != 0 && (game_anchors::u32_at(ram, p + 4) >> 24) & !1 == 0x2A {
            return true;
        }
        p = (tag & 0x00FF_FFFF) | 0x8000_0000;
    }
    false
}

/// Passes the displayed frame's screen fade is older than the RAM's fade
/// actor: the fade actor's tick links its quad into the table it steps
/// for, so the table holding it is the fade's own pass, whatever the logic
/// words did. Under the current record that is the two-pass law; under the
/// record the flip already moved off (the current one cleared and still
/// empty of it), the screen shows the pass just before. `None` when
/// neither table links a fade.
///
/// `palma_summon_mid_cast` and `horn_summon_mid_cast` are saved that way:
/// the current table is empty, the other one carries the flash-out at the
/// RAM actor's own `199` / `197`, and the screen is one 5-bit step brighter
/// than that table's raster (`scripts/mednafen/ot-raster.py`), one pass of
/// a fade that drains `2.125` a vsync. On the two-pass law the replay's
/// flash stood a pass too young and a step too bright. The rest of those
/// frames keeps the law: gated a pass later on the fade, `horn`'s sparks
/// and creature parted from retail's (`image` `.985` to `.979`), so the
/// image child keeps the gate and steps only the fades on
/// (`LEGAIA_SEAT_FADE_LEAD`). The slot-B cast parts' seeds follow the
/// fade's table: they tick with the fade actor, and `horn`'s newest table
/// draws the sparkles at the RAM's own levels (`cast_part_lag_vsyncs`).
pub fn fade_passes_back(ram: &[u8]) -> Option<u8> {
    let cur = u32::from(game_anchors::u16_at(ram, DOUBLE_BUFFER_INDEX) & 1);
    match (record_links_fade(ram, cur), record_links_fade(ram, cur ^ 1)) {
        (true, _) => Some(2),
        (false, true) => Some(1),
        (false, false) => None,
    }
}

/// Vsyncs the displayed frame's screen fades lag the RAM's fade actors by:
/// [`fade_passes_back`] passes of the frame step, the two-pass
/// [`display_lag_vsyncs`] when no table links a fade.
pub fn displayed_fade_lag_vsyncs(ram: &[u8]) -> u16 {
    match fade_passes_back(ram) {
        Some(back) => u16::from(back) * u16::from(frame_step(ram)),
        None => display_lag_vsyncs(ram),
    }
}

/// Passes the displayed frame is older than the RAM's ([`display_phase`]).
pub fn displayed_passes_back(ram: &[u8]) -> u8 {
    display_phase(ram).back
}

/// Vsyncs the displayed frame lags the RAM by ([`DisplayPhase::lag_vsyncs`]).
pub fn displayed_lag_vsyncs(ram: &[u8]) -> u16 {
    display_phase(ram).lag_vsyncs()
}

/// The unique highest positive score's key.
pub(super) fn unique_best(scores: &[(i32, u32)]) -> Option<u32> {
    let best = scores.iter().map(|&(n, _)| n).max()?;
    if best <= 0 || scores.iter().filter(|&&(n, _)| n == best).count() != 1 {
        return None;
    }
    scores.iter().find(|&&(n, _)| n == best).map(|&(_, s)| s)
}
