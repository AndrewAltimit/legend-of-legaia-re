//! The summon's screen fade as retail holds it in RAM: the fade actor found
//! in the actor pool and the template step it is on.

use super::*;

/// The SCUS fade actor's tick (`FUN_80025000`), stored at actor `+0x0C`.
pub(super) const FADE_ACTOR_TICK: u32 = 0x8002_5000;
/// The actor-list "done" bit (`actor[+0x10] |= 8`) a killed fade carries.
pub(super) const ACTOR_DONE: u32 = 0x8;
/// Where the effect-actor pool lives (`FUN_80020DE0`'s allocations).
pub(super) const ACTOR_POOL: std::ops::Range<u32> = 0x8007_0000..0x800A_0000;

/// The block's per-frame red delta for a template (`FUN_80020B00`).
pub(super) fn template_delta(
    t: &legaia_engine_battle_vm::battle_action::SummonFadeTemplate,
) -> i16 {
    (((i32::from(t.end_rgb[0]) - i32::from(t.start_rgb[0])) * 0x40) / i32::from(t.duration)) as i16
}

/// The band's sustain word `ctx[+0x6D8]`.
const BAND_WORD: u32 = 0x6D8;

/// The step of the pass a `0x35` capture's flash-out spawned on, off the
/// band's sustain word: the `0x34` pass arms the word to `0x78`
/// (`sh v0,0x2(s7)` at `0x801E4B20`) and spawns the flash in the same pass
/// (`jal 0x80024E80` at `0x801E4BAC`), the fade's tick drains the step on
/// that pass too ([`RetailFade::spawn_step`]), and each `0x35` pass after
/// drains the word by its own step (`0x801E4BCC..0x801E4BDC`). The age less
/// what the word drained is then the spawn pass's step. `None` where that
/// is no step (`1..=4`): a module that reseeds or drains the word itself,
/// or the drain clamp's pin at `1`, has left it something else.
fn flash_out_spawn_step(ram: &[u8], age: i32) -> Option<u8> {
    let ctx = game_anchors::u32_at(ram, BATTLE_CTX);
    if !in_ram(ctx) || game_anchors::u8_at(ram, ctx + 7) != 0x35 {
        return None;
    }
    let word = i32::from(game_anchors::u16_at(ram, ctx + BAND_WORD) as i16);
    let step = age - (0x78 - word);
    (1..=4).contains(&step).then_some(step as u8)
}

/// Find the live summon flash among the fade actors: tick word
/// `FUN_80025000`, not done, kind `1`, id `1`, and the delta of one of the
/// two summon templates (the creature's own fades share the actor and the
/// kind, never the delta and duration pair).
pub fn summon_fade(ram: &[u8]) -> Option<RetailFade> {
    summon_fade_on(
        ram,
        legaia_engine_session::native::battle_frame_cadence_on(),
    )
}

/// [`summon_fade`] for a replay on the battle-frame clock (`pass_clock`) or
/// the per-vsync one.
pub fn summon_fade_on(ram: &[u8], pass_clock: bool) -> Option<RetailFade> {
    use legaia_engine_battle_vm::battle_action::{
        SUMMON_FADE_ID, SUMMON_FLASH_IN, SUMMON_FLASH_OUT,
    };
    let s16 = |va: u32| game_anchors::u16_at(ram, va) as i16;
    let step = frame_step(ram).max(1);
    let mut a = ACTOR_POOL.start;
    while a + 0xA0 < ACTOR_POOL.end {
        let base = a;
        a += 4;
        if game_anchors::u32_at(ram, base + 0x0C) != FADE_ACTOR_TICK
            || game_anchors::u32_at(ram, base + 0x10) & ACTOR_DONE != 0
        {
            continue;
        }
        let b = base + 0x7C;
        if s16(b + 0x18) != 1 || s16(b + 0x22) != SUMMON_FADE_ID {
            continue;
        }
        let (delta, delay, duration) = (s16(b + 0x10), s16(b + 0x1C), s16(b + 0x20));
        for (t, to_white) in [(&SUMMON_FLASH_IN, true), (&SUMMON_FLASH_OUT, false)] {
            if delta != template_delta(t) {
                continue;
            }
            // `FUN_80020C14` counts the delay down first; the frame it lands
            // also steps the duration, so a landed block's age is one short
            // of the two countdowns' sum.
            //
            // Both countdowns drain the frame step a tick (`lbu v0,0x393(v0)`
            // at `0x80020C34`) and the delay keeps its overshoot once it
            // lands, so the exact age is `(delay0 - delay) + (duration0 -
            // duration) - landing_step`. The landing tick's step is not in
            // RAM; the overshoot only bounds it below. A replay on retail's
            // battle-frame cadence steps the flash with the pass, so it takes
            // the capture's own step (`nighto_summon_mid_cast`: delay `-1`,
            // duration `14`, ring step `3`, `24` vsyncs) and places the frame
            // on the pass that holds it. The per-vsync replay keeps the
            // smallest step the overshoot allows (`25` there), the reading
            // its gates are matched on.
            let age = if delay > 0 {
                i32::from(t.delay) - i32::from(delay)
            } else if t.delay > 0 && pass_clock {
                i32::from(t.delay) - i32::from(delay) + i32::from(t.duration)
                    - i32::from(duration)
                    - i32::from(step)
            } else if t.delay > 0 {
                i32::from(t.delay) + i32::from(t.duration) - i32::from(duration) - 1
            } else {
                i32::from(t.duration) - i32::from(duration)
            };
            return Some(RetailFade {
                to_white,
                age: age.clamp(0, i32::from(u16::MAX)) as u16,
                spawn_step: if to_white {
                    None
                } else {
                    flash_out_spawn_step(ram, age)
                },
            });
        }
    }
    None
}
